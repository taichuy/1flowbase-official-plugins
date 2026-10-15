use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::{HttpEgressClientError, HttpEgressRequest};
use std::collections::{BTreeMap, BTreeSet};
use url::Url;

pub(crate) struct Remote {
    client: HttpEgressClient,
    connection: config::Connection,
}
impl Remote {
    pub(crate) async fn new(host: &Host) -> Result<Self> {
        Ok(Self {
            client: HttpEgressClient::new(host.emitter.clone()),
            connection: config::connection(host).await?,
        })
    }
    fn url(&self, path: &str) -> Result<Url> {
        self.connection
            .base
            .join(path)
            .map_err(|_| fail("langfuse_base_url"))
    }
    async fn send(
        &self,
        method: &str,
        url: Url,
        body: Option<Value>,
    ) -> Result<(u16, String, Vec<u8>)> {
        let headers = BTreeMap::from([
            (
                "authorization".into(),
                self.connection.authorization.clone(),
            ),
            ("content-type".into(), "application/json".into()),
            ("accept".into(), "application/json".into()),
        ]);
        let body_base64 = body
            .map(|v| serde_json::to_vec(&v).map(|b| STANDARD.encode(b)))
            .transpose()
            .map_err(|_| fail("langfuse_request"))?;
        let response = self
            .client
            .execute(HttpEgressRequest {
                method: method.into(),
                url: url.to_string(),
                headers,
                body_base64,
            })
            .await
            .map_err(|e| match e {
                HttpEgressClientError::Host(e) => Failure {
                    code: e.code,
                    status: e.status,
                    details: e.details,
                },
                HttpEgressClientError::Transport(
                    runtime_extension_sdk::MultiplexError::FrameTooLarge,
                ) => fail("host_http_frame_too_large"),
                _ => fail("host_http_transport"),
            })?;
        let mime = response
            .headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let bytes = STANDARD
            .decode(&response.body_base64)
            .map_err(|_| fail("host_http_response"))?;
        Ok((response.status, mime, bytes))
    }
    async fn get(&self, url: Url) -> Result<Value> {
        let (status, _, bytes) = self.send("GET", url, None).await?;
        check_status(status)?;
        serde_json::from_slice(&bytes).map_err(|_| fail("langfuse_response"))
    }
    pub(crate) async fn test(&self) -> Result<Value> {
        // Project-scoped credentials, without returning project records or keys.
        let value = self.get(self.url("api/public/projects")?).await?;
        if !value.get("data").is_some_and(Value::is_array) {
            return Err(fail("langfuse_projects_response"));
        }
        Ok(json!({"success":true,"connected":true}))
    }
    pub(crate) async fn export(&self, otlp: Value) -> Result<()> {
        let (status, mime, bytes) = self
            .send("POST", self.url("api/public/otel/v1/traces")?, Some(otlp))
            .await?;
        check_status(status)?;
        let ack: Value = match mime.split(';').next().unwrap_or("").trim() {
            "application/json" => {
                serde_json::from_slice(&bytes).map_err(|_| fail("langfuse_ack"))?
            }
            "application/x-protobuf" => {
                use prost::Message;
                let ack = OtlpAcknowledgement::decode(bytes.as_slice())
                    .map_err(|_| fail("langfuse_ack"))?;
                match ack.partial_success {
                    Some(partial) => {
                        json!({"partialSuccess":{"rejectedSpans":partial.rejected_spans,"errorMessage":partial.error_message}})
                    }
                    None => json!({}),
                }
            }
            _ => return Err(fail("langfuse_ack_format")),
        };
        validate_ack(&ack)
    }
    pub(crate) async fn traces(&self, from: &str, to: &str) -> Result<Vec<String>> {
        let (from_time, to_time) = super::operations::date_range(Some(from), Some(to))?;
        let mut page_number = 1u64;
        let mut result = BTreeSet::new();
        loop {
            let mut url = self.url("api/public/traces")?;
            url.query_pairs_mut()
                .append_pair("fromTimestamp", from)
                .append_pair("toTimestamp", to)
                .append_pair("page", &page_number.to_string());
            let (status, _, bytes) = self.send("GET", url, None).await?;
            // This capability response is distinct from a missing trace, generic
            // route 404, authentication failure or server failure.
            if status == 404 && page_number == 1 && events_only_404(&bytes) {
                return self.traces_from_observations(from, to).await;
            }
            check_status(status)?;
            let page: Value =
                serde_json::from_slice(&bytes).map_err(|_| fail("langfuse_response"))?;
            let items = items(&page)?;
            let more = numbered_more(&page, page_number, items.is_empty())?;
            for trace in items {
                let timestamp = trace
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .ok_or_else(|| fail("langfuse_trace_time"))?;
                let timestamp = time::OffsetDateTime::parse(
                    timestamp,
                    &time::format_description::well_known::Rfc3339,
                )
                .map_err(|_| fail("langfuse_trace_time"))?;
                if timestamp >= from_time && timestamp < to_time {
                    let id = trace
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| fail("langfuse_trace_id"))?;
                    result.insert(id.into());
                }
            }
            if !more {
                return Ok(result.into_iter().collect());
            }
            page_number = page_number
                .checked_add(1)
                .ok_or_else(|| fail("langfuse_page"))?;
        }
    }
    async fn traces_from_observations(&self, from: &str, to: &str) -> Result<Vec<String>> {
        let (from_time, to_time) = super::operations::date_range(Some(from), Some(to))?;
        let mut cursor: Option<String> = None;
        let mut seen = BTreeSet::new();
        let mut candidates = BTreeSet::new();
        loop {
            let mut url = self.url("api/public/v2/observations")?;
            url.query_pairs_mut()
                .append_pair("fromStartTime", from)
                .append_pair("toStartTime", to)
                .append_pair("fields", "basic,time");
            if let Some(cursor) = &cursor {
                url.query_pairs_mut().append_pair("cursor", cursor);
            }
            let page = self.get(url).await?;
            let items = items(&page)?;
            let next = next_cursor(&page)?;
            if items.is_empty() && next.is_some() {
                return Err(fail("langfuse_cursor"));
            }
            for observation in items {
                let start = observation_start(observation)?;
                if start >= from_time && start < to_time {
                    let trace = observation
                        .get("traceId")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| fail("langfuse_trace_id"))?;
                    candidates.insert(trace.to_owned());
                }
            }
            let Some(next) = next else { break };
            if !seen.insert(next.clone()) {
                return Err(fail("langfuse_cursor"));
            }
            cursor = Some(next);
        }
        let mut result = Vec::new();
        for trace in candidates {
            // An in-window child does not mean the trace started in-window.
            // Fetch without time filters, including every page and late child.
            let observations = self.observations(&trace).await?;
            let start = observations
                .iter()
                .map(observation_start)
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .min()
                .ok_or_else(|| fail("langfuse_trace_empty"))?;
            if start >= from_time && start < to_time {
                result.push(trace);
            }
        }
        Ok(result)
    }
    pub(crate) async fn observations(&self, trace: &str) -> Result<Vec<Value>> {
        let mut cursor: Option<String> = None;
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        loop {
            let mut url = self.url("api/public/v2/observations")?;
            url.query_pairs_mut()
                .append_pair("traceId", trace)
                .append_pair("fields", "basic,time,io,metadata,model,usage,trace_context")
                .append_pair("expandMetadata", "1flowbase_agent_log");
            if let Some(cursor) = &cursor {
                url.query_pairs_mut().append_pair("cursor", cursor);
            }
            let (status, mime, bytes) = self.send("GET", url, None).await?;
            if status == 404 && cursor.is_none() && capability_404(&mime, &bytes) {
                return self.legacy_observations(trace).await;
            }
            check_status(status)?;
            let page: Value =
                serde_json::from_slice(&bytes).map_err(|_| fail("langfuse_response"))?;
            let items = bound_items(&page, trace)?;
            let next = next_cursor(&page)?;
            if items.is_empty() && next.is_some() {
                return Err(fail("langfuse_cursor"));
            }
            result.extend(items.iter().cloned());
            let Some(next) = next else {
                return complete(result);
            };
            if !seen.insert(next.clone()) {
                return Err(fail("langfuse_cursor"));
            }
            cursor = Some(next);
        }
    }
    async fn legacy_observations(&self, trace: &str) -> Result<Vec<Value>> {
        let mut result = Vec::new();
        let mut page_number = 1u64;
        loop {
            let mut url = self.url("api/public/observations")?;
            url.query_pairs_mut()
                .append_pair("traceId", trace)
                .append_pair("page", &page_number.to_string());
            let page = self.get(url).await?;
            let items = bound_items(&page, trace)?;
            let more = numbered_more(&page, page_number, items.is_empty())?;
            result.extend(items.iter().cloned());
            if !more {
                return complete(result);
            }
            page_number = page_number
                .checked_add(1)
                .ok_or_else(|| fail("langfuse_page"))?;
        }
    }
}
pub(crate) fn validate_ack(ack: &Value) -> Result<()> {
    if ack.get("name").is_some() {
        return validate_queue_ack(ack);
    }
    if (ack.get("partialSuccess").is_some() && ack.get("partial_success").is_some())
        || !ack.is_object()
        || ack.as_object().is_some_and(|object| {
            object
                .keys()
                .any(|key| !matches!(key.as_str(), "partialSuccess" | "partial_success"))
        })
    {
        return Err(fail("langfuse_ack"));
    }
    if let Some(partial) = ack
        .get("partialSuccess")
        .or_else(|| ack.get("partial_success"))
    {
        if !partial.as_object().is_some_and(|object| {
            object.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "rejectedSpans" | "rejected_spans" | "errorMessage" | "error_message"
                )
            })
        }) || (partial.get("rejectedSpans").is_some() && partial.get("rejected_spans").is_some())
            || (partial.get("errorMessage").is_some() && partial.get("error_message").is_some())
        {
            return Err(fail("langfuse_ack"));
        }
        let rejected = partial
            .get("rejectedSpans")
            .or_else(|| partial.get("rejected_spans"));
        let rejected = match rejected {
            None => 0,
            Some(Value::String(s)) => s.parse::<u64>().map_err(|_| fail("langfuse_ack"))?,
            Some(v) => v.as_u64().ok_or_else(|| fail("langfuse_ack"))?,
        };
        if partial
            .get("errorMessage")
            .or_else(|| partial.get("error_message"))
            .is_some_and(|value| !value.is_string())
        {
            return Err(fail("langfuse_ack"));
        }
        let warning = partial
            .get("errorMessage")
            .or_else(|| partial.get("error_message"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !partial.is_object() || rejected != 0 || !warning.is_empty() {
            return Err(Failure {
                code: "langfuse_partial_export".into(),
                status: Some(502),
                details: Some(
                    json!({"rejected_spans":rejected,"acknowledgement_warning":!warning.is_empty()}),
                ),
            });
        }
    }
    Ok(())
}
fn validate_queue_ack(ack: &Value) -> Result<()> {
    // Langfuse awaits blob upload and queue.add before returning this Bull job.
    // It confirms asynchronous acceptance, not completed observation persistence.
    // Never include this object (which contains auth metadata) in a receipt/error.
    let nonempty = |value: &Value| value.as_str().is_some_and(|s| !s.is_empty());
    let queue = ack["queueQualifiedName"].as_str().unwrap_or("");
    let queue_ok = queue == "bull:otel-ingestion-queue"
        || queue
            .strip_prefix("bull:otel-ingestion-queue-")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit()));
    if !ack.as_object().is_some_and(|object| {
        object.keys().all(|key| {
            matches!(
                key.as_str(),
                "name"
                    | "data"
                    | "opts"
                    | "id"
                    | "progress"
                    | "returnvalue"
                    | "stacktrace"
                    | "priority"
                    | "attemptsStarted"
                    | "attemptsMade"
                    | "stalledCounter"
                    | "timestamp"
                    | "queueQualifiedName"
            )
        })
    }) || ack["name"] != "otel-ingestion-job"
        || ack["data"]["name"] != "otel-ingestion-job"
        || !nonempty(&ack["id"])
        || !nonempty(&ack["data"]["id"])
        || !ack["data"]["timestamp"]
            .as_str()
            .is_some_and(|s| parse_time(s).is_ok())
        || !nonempty(&ack["data"]["payload"]["data"]["fileKey"])
        || !nonempty(&ack["data"]["payload"]["data"]["publicKey"])
        || ack["data"]["payload"]["authCheck"]["validKey"] != true
        || ack["data"]["payload"]["authCheck"]["scope"]["accessLevel"] != "project"
        || !nonempty(&ack["data"]["payload"]["authCheck"]["scope"]["projectId"])
        || !queue_ok
        || !ack["opts"].is_object()
        || !ack["timestamp"].as_u64().is_some_and(|time| time > 0)
        || ack["progress"] != 0
        || ack.get("returnvalue") != Some(&Value::Null)
        || !ack["stacktrace"].as_array().is_some_and(Vec::is_empty)
        || ["attemptsMade", "attemptsStarted", "stalledCounter"]
            .iter()
            .any(|key| ack[*key] != 0)
        || ack["priority"].as_u64().is_none()
    {
        return Err(fail("langfuse_ack"));
    }
    Ok(())
}
fn check_status(status: u16) -> Result<()> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    Err(Failure {
        code: match status {
            401 | 403 => "langfuse_authentication",
            429 => "langfuse_rate_limited",
            404 => "langfuse_not_found",
            _ => "langfuse_http_error",
        }
        .into(),
        status: Some(status),
        details: None,
    })
}
fn items(page: &Value) -> Result<&Vec<Value>> {
    page.get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| fail("langfuse_response"))
}
pub(crate) fn bound_items<'a>(page: &'a Value, trace: &str) -> Result<&'a Vec<Value>> {
    let items = items(page)?;
    if !items
        .iter()
        .all(|v| v.get("traceId").and_then(Value::as_str) == Some(trace))
    {
        return Err(fail("langfuse_trace_mismatch"));
    }
    Ok(items)
}
pub(crate) fn next_cursor(page: &Value) -> Result<Option<String>> {
    let meta = page
        .get("meta")
        .and_then(Value::as_object)
        .ok_or_else(|| fail("langfuse_cursor"))?;
    let read = |key: &str| -> Result<Option<String>> {
        match meta.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if !s.is_empty() => Ok(Some(s.clone())),
            _ => Err(fail("langfuse_cursor")),
        }
    };
    let cursor = read("cursor")?;
    let next = read("nextCursor")?;
    if (cursor.is_some() && next.is_some() && cursor != next)
        || (!meta.is_empty() && !meta.contains_key("cursor") && !meta.contains_key("nextCursor"))
    {
        return Err(fail("langfuse_cursor"));
    }
    Ok(cursor.or(next))
}
pub(crate) fn numbered_more(page: &Value, number: u64, empty: bool) -> Result<bool> {
    let meta = &page["meta"];
    let total = meta
        .get("totalPages")
        .and_then(Value::as_u64)
        .ok_or_else(|| fail("langfuse_page"))?;
    if meta.get("page").and_then(Value::as_u64) != Some(number)
        || (total < number && !(number == 1 && total == 0 && empty))
        || (empty && number < total)
    {
        return Err(fail("langfuse_page"));
    }
    Ok(number < total)
}
pub(crate) fn complete(values: Vec<Value>) -> Result<Vec<Value>> {
    if values.is_empty() {
        return Err(fail("langfuse_trace_empty"));
    }
    let mut unique: BTreeMap<String, Value> = BTreeMap::new();
    for value in values {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| fail("langfuse_observation_id"))?
            .to_owned();
        // Only these top-level remote bookkeeping fields are nonsemantic.
        // Retain one entire original row, never merge or invent timestamps.
        let rank = observation_version(&value)?;
        if let Some(old) = unique.get(&id) {
            let facts = |row: &Value| {
                let mut row = row.clone();
                if let Some(object) = row.as_object_mut() {
                    object.remove("createdAt");
                    object.remove("updatedAt");
                }
                row
            };
            if facts(old) != facts(&value) {
                return Err(fail("langfuse_observation_conflict"));
            }
            if rank <= observation_version(old)? {
                continue;
            }
        }
        unique.insert(id, value);
    }
    Ok(unique.into_values().collect())
}
fn parse_time(value: &str) -> Result<time::OffsetDateTime> {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|_| fail("langfuse_observation_time"))
}
fn observation_start(value: &Value) -> Result<time::OffsetDateTime> {
    parse_time(
        value
            .get("startTime")
            .and_then(Value::as_str)
            .ok_or_else(|| fail("langfuse_observation_time"))?,
    )
}
fn observation_version(
    value: &Value,
) -> Result<(
    Option<time::OffsetDateTime>,
    Option<time::OffsetDateTime>,
    String,
)> {
    let timestamp = |key: &str| match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => parse_time(value).map(Some),
        _ => Err(fail("langfuse_observation_time")),
    };
    Ok((
        timestamp("updatedAt")?,
        timestamp("createdAt")?,
        value.to_string(),
    ))
}
fn events_only_404(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body).ok().is_some_and(|value| {
        value.get("message").and_then(Value::as_str).is_some_and(|message| {
            message.starts_with("This endpoint is not available on deployments running in Langfuse v4 events_only mode.")
        })
    })
}
fn capability_404(mime: &str, body: &[u8]) -> bool {
    let message = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
        .to_ascii_lowercase();
    message.contains("observations v2 api is only available")
        || ((message.contains("endpoint") || message.contains("route"))
            && (message.contains("not found")
                || message.contains("unsupported")
                || message.contains("unknown")))
        || (mime.starts_with("text/html")
            && String::from_utf8_lossy(body).contains("This page could not be found"))
}

#[derive(Clone, PartialEq, prost::Message)]
struct OtlpAcknowledgement {
    #[prost(message, optional, tag = "1")]
    partial_success: Option<OtlpPartialSuccess>,
}
#[derive(Clone, PartialEq, prost::Message)]
struct OtlpPartialSuccess {
    #[prost(int64, tag = "1")]
    rejected_spans: i64,
    #[prost(string, tag = "2")]
    error_message: String,
}
