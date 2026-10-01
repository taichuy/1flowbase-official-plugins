//! Preserve supplier facts separately from local recovery diagnostics.
use super::{ProviderRuntimeError, ProviderRuntimeErrorKind};
use serde_json::{json, Map, Value};

pub(super) fn parse_http(raw: &str) -> Option<Value> {
    serde_json::from_str(raw).ok().or_else(|| {
        let (prefix, trailing) = raw.split_once('\n')?;
        trailing
            .trim_start()
            .starts_with("data:")
            .then(|| serde_json::from_str(prefix).ok())
            .flatten()
    })
}

pub(super) fn error(
    error: Option<&Value>,
    fallback: &str,
    status: Option<u16>,
    raw: Option<&str>,
) -> ProviderRuntimeError {
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_owned();
    let mut details = Map::new();
    if let Some(error) = error {
        details.insert("upstream_error".into(), error.clone());
    }
    if let Some(status) = status {
        details.insert("status_code".into(), json!(status));
    }
    if let Some(raw) = raw {
        details.insert("raw_body".into(), json!(raw));
    }
    // HTTP statuses are facts only when supplied by the HTTP transport. Event
    // terminals have no fabricated status and cannot become disconnect retries.
    details.insert(
        "semantic_terminal".into(),
        json!(status.is_none_or(|status| status < 500 && status != 429)),
    );
    ProviderRuntimeError {
        kind: ProviderRuntimeErrorKind::ProviderUpstreamError,
        message: message.clone(),
        provider_summary: Some(message),
        provider_details: Some(Value::Object(details)),
    }
}

pub(super) fn websocket(payload: &str) -> Option<ProviderRuntimeError> {
    let value: Value = serde_json::from_str(payload).ok()?;
    (value.get("type").and_then(Value::as_str) == Some("error")).then(|| {
        error(
            value.get("error"),
            "Responses websocket error",
            None,
            Some(payload),
        )
    })
}

pub(super) fn failed(response: Option<&Value>) -> ProviderRuntimeError {
    error(
        response.and_then(|response| response.get("error")),
        "response.failed event received",
        None,
        None,
    )
}

#[cfg(test)]
#[path = "_tests.rs"]
mod tests;
