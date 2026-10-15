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
            let page = self.get(url).await?;
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
fn complete(values: Vec<Value>) -> Result<Vec<Value>> {
    if values.is_empty() {
        return Err(fail("langfuse_trace_empty"));
    }
    let mut unique = BTreeMap::new();
    for value in values {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| fail("langfuse_observation_id"))?
            .to_owned();
        if let Some(old) = unique.insert(id, value.clone()) {
            if old != value {
                return Err(fail("langfuse_observation_conflict"));
            }
        }
    }
    Ok(unique.into_values().collect())
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
