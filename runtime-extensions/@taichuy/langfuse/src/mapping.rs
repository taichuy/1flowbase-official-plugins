//! Lossless self-capsules and standard GenAI normalization; no terminal fact is inferred.
use super::{fail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
const SOURCE: &str = "1flowbase.agent_log.source_fact";
const PREFIX: &str = "1flowbase.agent-logs.source-fact/v1;base64,";

pub fn attribute(key: &str, value: Value) -> Value {
    json!({"key":key,"value":{"stringValue": match value { Value::String(s) => s, other => other.to_string() }}})
}
fn attrs(span: &Value) -> Vec<Value> {
    span.get("attributes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}
fn text(attrs: &[Value], key: &str) -> Option<String> {
    attrs
        .iter()
        .find(|v| v["key"] == key)
        .and_then(|v| v.pointer("/value/stringValue"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}
fn structured(value: &Value) -> Value {
    if let Some(s) = value.as_str() {
        serde_json::from_str(s).unwrap_or_else(|_| value.clone())
    } else {
        value.clone()
    }
}
pub fn capsule(metadata: &Value) -> Result<Option<String>> {
    if let Some(text) = metadata.as_str() {
        if text.contains("1flowbase_agent_log") && serde_json::from_str::<Value>(text).is_err() {
            return Err(fail("langfuse_source_fact_encoding"));
        }
    }
    let metadata = structured(metadata);
    if metadata
        .as_object()
        .is_some_and(|o| o.keys().any(|k| k.starts_with("1flowbase_agent_log.")))
    {
        return Err(fail("langfuse_source_fact_flattened"));
    }
    let Some(value) = metadata.get("1flowbase_agent_log") else {
        return Ok(None);
    };
    let fact = if let Some(encoded) = value.as_str() {
        let payload = encoded
            .strip_prefix(PREFIX)
            .ok_or_else(|| fail("langfuse_source_fact_encoding"))?;
        let bytes = STANDARD
            .decode(payload)
            .map_err(|_| fail("langfuse_source_fact_encoding"))?;
        if STANDARD.encode(&bytes) != payload {
            return Err(fail("langfuse_source_fact_encoding"));
        }
        serde_json::from_slice::<Value>(&bytes)
            .map_err(|_| fail("langfuse_source_fact_encoding"))?
    } else {
        value.clone()
    };
    let object = fact
        .as_object()
        .ok_or_else(|| fail("langfuse_source_fact_invalid"))?;
    if object.len() != 4
        || !["schema_version", "source_id", "source_client"]
            .iter()
            .all(|key| fact[*key].as_str().is_some_and(|s| !s.is_empty()))
        || !fact["event"].is_object()
    {
        return Err(fail("langfuse_source_fact_invalid"));
    }
    Ok(Some(format!(
        "{PREFIX}{}",
        STANDARD
            .encode(serde_json::to_vec(&fact).map_err(|_| fail("langfuse_source_fact_invalid"))?)
    )))
}
/// The source capsule is the authority. Other attributes are display projections only.
pub fn to_langfuse(mut otlp: Value) -> Result<Value> {
    for resource in otlp
        .get_mut("resourceSpans")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| fail("otlp_resource_spans"))?
    {
        for scope in resource
            .get_mut("scopeSpans")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| fail("otlp_scope_spans"))?
        {
            for span in scope
                .get_mut("spans")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| fail("otlp_spans"))?
            {
                let mut attributes = attrs(span);
                if let Some(encoded) = text(&attributes, SOURCE) {
                    let metadata = json!({"1flowbase_agent_log":encoded});
                    capsule(&metadata)?;
                    attributes.push(attribute("langfuse.observation.metadata", metadata));
                }
                for (from, to) in [
                    ("gen_ai.input.messages", "langfuse.observation.input"),
                    ("gen_ai.output.messages", "langfuse.observation.output"),
                    ("gen_ai.response.model", "langfuse.observation.model.name"),
                ] {
                    if let Some(value) = text(&attributes, from) {
                        attributes.push(attribute(to, Value::String(value)));
                    }
                }
                if text(&attributes, "langfuse.observation.type").is_none() {
                    let kind = if text(&attributes, "gen_ai.tool.name").is_some() {
                        "tool"
                    } else if text(&attributes, "gen_ai.response.model").is_some() {
                        "generation"
                    } else {
                        "span"
                    };
                    attributes.push(attribute("langfuse.observation.type", json!(kind)));
                }
                span["attributes"] = json!(attributes);
            }
        }
    }
    Ok(otlp)
}
fn required<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v.get(k)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| fail("langfuse_observation_fields"))
}
fn id(value: &str, length: usize) -> String {
    if value.len() == length
        && value.bytes().all(|c| c.is_ascii_hexdigit())
        && value.bytes().any(|c| c != b'0')
    {
        value.to_ascii_lowercase()
    } else {
        format!("{:x}", Sha256::digest(value.as_bytes()))[..length].into()
    }
}
fn nanos(value: &str) -> Result<String> {
    let n = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| fail("langfuse_observation_time"))?
        .unix_timestamp_nanos();
    if n <= 0 || n > u64::MAX as i128 {
        return Err(fail("langfuse_observation_time"));
    }
    Ok(n.to_string())
}
fn messages(value: &Value, role: &str) -> Value {
    let value = structured(value);
    if let Some(items) = value
        .get("messages")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
    {
        // A plain content-parts array is one message, not many invented turns.
        if items
            .iter()
            .all(|v| v.get("role").and_then(Value::as_str).is_some())
        {
            return json!(items);
        }
    }
    if value.get("role").and_then(Value::as_str).is_some() {
        return json!([value]);
    }
    json!([{"role":role,"content":value}])
}
pub fn from_observations(observations: &[Value]) -> Result<Value> {
    let mut spans = Vec::new();
    for observation in observations {
        let trace = required(observation, "traceId")?;
        let observation_id = required(observation, "id")?;
        let start = nanos(required(observation, "startTime")?)?;
        let mut attributes = vec![
            attribute(
                "gen_ai.conversation.id",
                json!(observation
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .or_else(|| observation
                        .pointer("/trace/sessionId")
                        .and_then(Value::as_str))
                    .unwrap_or(trace)),
            ),
            attribute("coding_agent.client", json!("langfuse")),
            attribute("1flowbase.external.raw", observation.clone()),
        ];
        if let Some(metadata) = observation.get("metadata") {
            if let Some(encoded) = capsule(metadata)? {
                attributes.push(attribute(SOURCE, json!(encoded)));
            }
        }
        let tool = observation
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case("tool"));
        for (field, key, role) in [
            (
                "input",
                if tool {
                    "gen_ai.tool.call.arguments"
                } else {
                    "gen_ai.input.messages"
                },
                "user",
            ),
            (
                "output",
                if tool {
                    "gen_ai.tool.call.result"
                } else {
                    "gen_ai.output.messages"
                },
                "assistant",
            ),
        ] {
            if let Some(value) = observation.get(field).filter(|v| !v.is_null()) {
                attributes.push(attribute(
                    key,
                    if tool {
                        structured(value)
                    } else {
                        messages(value, role)
                    },
                ));
            }
        }
        if tool {
            attributes.push(attribute(
                "gen_ai.tool.name",
                observation.get("name").cloned().unwrap_or(json!("tool")),
            ));
            attributes.push(attribute("gen_ai.tool.call.id", json!(observation_id)));
        }
        if let Some(model) = observation.get("model").filter(|v| !v.is_null()) {
            attributes.push(attribute("gen_ai.response.model", model.clone()));
        }
        if let Some(provider) = observation
            .get("provider")
            .or_else(|| observation.pointer("/metadata/gen_ai.provider.name"))
        {
            attributes.push(attribute("gen_ai.provider.name", provider.clone()));
        }
        if let Some(usage) = observation
            .get("usageDetails")
            .or_else(|| observation.get("usage"))
        {
            let usage = structured(usage);
            for (keys, key) in [
                (
                    &["input", "input_tokens", "promptTokens", "prompt_tokens"][..],
                    "gen_ai.usage.input_tokens",
                ),
                (
                    &[
                        "output",
                        "output_tokens",
                        "completionTokens",
                        "completion_tokens",
                    ][..],
                    "gen_ai.usage.output_tokens",
                ),
                (
                    &["total", "total_tokens", "totalTokens"][..],
                    "gen_ai.usage.total_tokens",
                ),
                (
                    &["cache_read_input_tokens", "input_cached_tokens"][..],
                    "gen_ai.usage.cache.read_input_tokens",
                ),
                (
                    &["cache_creation_input_tokens", "cache_write_tokens"][..],
                    "gen_ai.usage.cache.creation_input_tokens",
                ),
            ] {
                if let Some(value) = keys
                    .iter()
                    .find_map(|k| usage.get(*k))
                    .filter(|v| !v.is_null())
                {
                    attributes.push(attribute(key, value.clone()));
                }
            }
        }
        let mut span = json!({"traceId":id(trace,32),"spanId":id(&format!("{trace}/{observation_id}"),16),"name":observation.get("name").and_then(Value::as_str).unwrap_or("langfuse.observation"),"startTimeUnixNano":start,"attributes":attributes});
        if let Some(parent) = observation
            .get("parentObservationId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            span["parentSpanId"] = json!(id(&format!("{trace}/{parent}"), 16));
        }
        if let Some(end) = observation.get("endTime").filter(|v| !v.is_null()) {
            let end = nanos(
                end.as_str()
                    .ok_or_else(|| fail("langfuse_observation_time"))?,
            )?;
            if end.parse::<u64>().ok()
                < span["startTimeUnixNano"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
            {
                return Err(fail("langfuse_observation_time_order"));
            }
            span["endTimeUnixNano"] = json!(end);
        }
        spans.push(span);
    }
    Ok(
        json!({"resourceSpans":[{"resource":{"attributes":[attribute("service.name",json!("langfuse"))]},"scopeSpans":[{"scope":{"name":"1flowbase.langfuse"},"spans":spans}]}]}),
    )
}
