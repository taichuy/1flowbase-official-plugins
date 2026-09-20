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
        "deadline_exceeded" => "provider recovery deadline exceeded",
        "budget_exhausted" => "provider recovery attempt budget exhausted",
        "authorization_rejected" => "upstream authorization rejected the request",
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
    transport_error("websocket_close", close_category(frame.as_ref()), code)
}

pub(crate) fn close_category(frame: Option<&CloseFrame>) -> &'static str {
    let code = frame.map(|frame| u16::from(frame.code));
    let reason = frame
        .as_ref()
        .map(|frame| frame.reason.as_ref())
        .unwrap_or("")
        .to_ascii_lowercase();
    if code == Some(1008) && reason.contains("upstream continuation connection is unavailable") {
        "continuation_unavailable"
    } else if code == Some(1008) {
        "policy_rejected"
    } else if reason.contains("upstream websocket proxy failed") {
        "proxy_failed"
    } else {
        "transport_disconnected"
    }
}

pub(crate) fn failure(error: &anyhow::Error, socket: Option<u64>, owner: Option<u64>) -> Value {
    if let Some(physical) = error.downcast_ref::<super::websocket_io::Failure>() {
        let mut diagnostic = failure(
            &transport_error(
                if physical.close_code.is_some() {
                    "websocket_close"
                } else {
                    "websocket_error"
                },
                physical.category,
                physical.close_code,
            ),
            socket,
            owner,
        );
        diagnostic["websocket_error_kind"] = json!(physical.kind);
        diagnostic["failure_phase"] = json!(physical.phase);
        diagnostic["idle_duration_ms"] = json!(physical.idle_duration_ms);
        if let Some(kind) = physical.io_kind {
            diagnostic["io_error_kind"] = json!(kind);
        }
        return diagnostic;
    }
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
            } else if message.contains("401")
                || message.contains("403")
                || message.contains("unauthorized")
                || message.contains("forbidden")
                || message.contains("invalid_api_key")
            {
                "authorization_rejected"
            } else {
                "unclassified"
            };
            json!({"kind":if typed.is_some() {"provider_typed"} else {"provider_untyped"},
                "reason_category":category,"reason":safe_reason(category)})
        });
    if let Some(error) = typed {
        value["provider_error_kind"] =
            serde_json::to_value(&error.kind).expect("error kind serializes");
    }
    if let Some(error) = error.downcast_ref::<std::io::Error>() {
        value["io_error_kind"] = json!(io_kind(error.kind()));
    }
    value["socket_incarnation"] = json!(socket);
    value["owner_socket_incarnation"] = json!(owner);
    value
}

pub(crate) fn io_kind(kind: std::io::ErrorKind) -> &'static str {
    use std::io::ErrorKind;
    match kind {
        ErrorKind::ConnectionRefused => "connection_refused",
        ErrorKind::ConnectionReset => "connection_reset",
        ErrorKind::ConnectionAborted => "connection_aborted",
        ErrorKind::NotConnected => "not_connected",
        ErrorKind::BrokenPipe => "broken_pipe",
        ErrorKind::TimedOut => "timed_out",
        ErrorKind::UnexpectedEof => "unexpected_eof",
        ErrorKind::PermissionDenied => "permission_denied",
        ErrorKind::Interrupted => "interrupted",
        _ => "other",
    }
}

pub(crate) fn network_error(error: &std::io::Error) -> anyhow::Error {
    let mut typed = transport_error("network_error", "transport_disconnected", None)
        .downcast::<ProviderRuntimeError>()
        .expect("transport error is typed");
    typed.provider_details.as_mut().unwrap()[FAILURE_KEY]["io_error_kind"] =
        json!(io_kind(error.kind()));
    anyhow::Error::new(typed)
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

pub(crate) fn decision(transition: super::RecoveryTransition, committed: bool) -> &'static str {
    use super::recovery::{RecoveryDisposition as D, RecoveryReason as R};
    match transition.reason {
        R::DeadlineExceeded => return "deadline_exceeded",
        R::BudgetExhausted => return "budget_exhausted",
        _ => {}
    }
    match transition.disposition {
        D::SameEpochReconnect | D::OneFullContextRebuild => "retry_websocket",
        D::PreCommitHttpFallback => "retry_http",
        _ if committed => "committed",
        _ => "terminal",
    }
}

pub(crate) fn summary(attempts: &[Value]) -> Value {
    json!({"first_failure":attempts.first(), "last_failure":attempts.last(), "attempts":attempts,
        "association_valid": if attempts.iter().any(|value| value["reason_category"] == "continuation_unavailable") { Some(false) } else { None }})
}

#[cfg(test)]
#[path = "_tests/recovery_diagnostics.rs"]
mod tests;
