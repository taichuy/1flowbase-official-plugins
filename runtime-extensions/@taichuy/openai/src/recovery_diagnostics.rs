//! Closed, bounded diagnostics: upstream text, URLs and routing tokens never cross this boundary.
use super::{ProviderRuntimeError, ProviderRuntimeErrorKind};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;

pub(crate) const KEY: &str = "1flowbase_provider_recovery_diagnostics";
const FAILURE_KEY: &str = "1flowbase_transport_failure";

fn safe_reason(category: &str) -> &'static str {
    match category {
        "continuation_unavailable" => "upstream continuation connection is unavailable",
        "previous_response_unavailable" => "previous_response_id is no longer available",
        "proxy_failed" => "upstream websocket proxy failed",
        "policy_rejected" => "upstream policy rejected the request; reason redacted",
        "transport_disconnected" => "websocket disconnected before response.completed",
        _ => "provider failure; unclassified details redacted",
    }
}

pub(crate) fn transport_error(kind: &str, category: &str, code: Option<u16>) -> anyhow::Error {
    anyhow::Error::new(ProviderRuntimeError {
        kind: ProviderRuntimeErrorKind::ProviderTransportUnavailable,
        message: safe_reason(category).into(),
        provider_summary: None,
        provider_details: Some(json!({ FAILURE_KEY: {
            "kind":kind, "reason_category":category, "reason":safe_reason(category), "close_code":code
        }})),
    })
}

pub(crate) fn close_error(frame: Option<CloseFrame>) -> anyhow::Error {
    let code = frame.as_ref().map(|frame| u16::from(frame.code));
    let reason = frame
        .as_ref()
        .map(|frame| frame.reason.as_ref())
        .unwrap_or("")
        .to_ascii_lowercase();
    let category = if code == Some(1008)
        && reason.contains("upstream continuation connection is unavailable")
    {
        "continuation_unavailable"
    } else if code == Some(1008) {
        "policy_rejected"
    } else if reason.contains("upstream websocket proxy failed") {
        "proxy_failed"
    } else {
        "transport_disconnected"
    };
    transport_error("websocket_close", category, code)
}

pub(crate) fn failure(error: &anyhow::Error, socket: Option<u64>, owner: Option<u64>) -> Value {
    let typed = error.downcast_ref::<ProviderRuntimeError>();
    let mut value = typed
        .and_then(|error| error.provider_details.as_ref())
        .and_then(|details| details.get(FAILURE_KEY))
        .cloned()
        .unwrap_or_else(|| {
            let message = error.to_string().to_ascii_lowercase();
            let category = if message.contains("previous_response_id")
                && (message.contains("not found") || message.contains("no longer available"))
            {
                "previous_response_unavailable"
            } else if message.contains("upstream websocket proxy failed") {
                "proxy_failed"
            } else {
                "unclassified"
            };
            json!({"kind":if typed.is_some() {"provider_typed"} else {"provider_untyped"},
                "reason_category":category,"reason":safe_reason(category)})
        });
    value["socket_incarnation"] = json!(socket);
    value["owner_socket_incarnation"] = json!(owner);
    value
}

pub(crate) fn safe_error(error: &anyhow::Error) -> ProviderRuntimeError {
    let diagnostic = failure(error, None, None);
    ProviderRuntimeError {
        kind: error
            .downcast_ref::<ProviderRuntimeError>()
            .map(|error| error.kind.clone())
            .unwrap_or(ProviderRuntimeErrorKind::ProviderTransportUnavailable),
        message: diagnostic["reason"]
            .as_str()
            .unwrap_or("provider failure")
            .into(),
        provider_summary: None,
        provider_details: Some(json!({FAILURE_KEY:diagnostic})),
    }
}

pub(crate) fn summary(attempts: &[Value]) -> Value {
    json!({"first_failure":attempts.first(), "last_failure":attempts.last(), "attempts":attempts,
        "association_valid": if attempts.iter().any(|value| value["reason_category"] == "continuation_unavailable") { Some(false) } else { None }})
}

#[cfg(test)]
#[path = "_tests/recovery_diagnostics.rs"]
mod tests;
