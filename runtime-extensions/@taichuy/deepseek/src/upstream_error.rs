use super::*;

pub(super) fn event(
    payload: &Value,
    raw: &str,
    api_key: &str,
    status: u16,
    semantic_terminal: bool,
) -> ProviderStreamEvent {
    let mut safe = payload.clone();
    redact(&mut safe, api_key);
    // Keep original bytes unless credential redaction actually changes them.
    let raw_body = if &safe != payload {
        safe.to_string()
    } else if api_key.is_empty() {
        raw.to_owned()
    } else {
        raw.replace(api_key, "***")
    };
    let mut details = json!({
        "status_code": status,
        "error_type": provider_error_type(&safe),
        "raw_body": raw_body,
    });
    if let Some(upstream) = safe.get("error") {
        details["upstream_error"] = upstream.clone();
    }
    if semantic_terminal {
        details["semantic_terminal"] = json!(true);
    }
    ProviderStreamEvent::Error {
        error: ProviderRuntimeError {
            kind: "provider_upstream_error".into(),
            message: provider_error_message(&safe),
            provider_summary: None,
            provider_details: Some(details),
        },
    }
}

/// An explicit upstream error is an absorbing terminal. Partial text remains in
/// the event history; unfinished tool fragments must never become executable.
pub(super) fn finish(
    events: Vec<ProviderStreamEvent>,
    usage: ProviderUsage,
    streaming: bool,
) -> RuntimeInvocationEnvelope {
    RuntimeInvocationEnvelope {
        events,
        result: ProviderInvocationResult {
            final_content: None,
            response_id: None,
            tool_calls: Vec::new(),
            mcp_calls: Vec::new(),
            usage,
            finish_reason: Some(ProviderFinishReason::Error),
            provider_metadata: if streaming {
                json!({"stream_termination":build_stream_termination_metadata(None, StreamTermination::Error)})
            } else {
                json!({})
            },
        },
    }
}

fn redact(value: &mut Value, secret: &str) {
    if secret.is_empty() {
        return;
    }
    match value {
        Value::String(text) => *text = text.replace(secret, "***"),
        Value::Array(items) => items.iter_mut().for_each(|item| redact(item, secret)),
        Value::Object(fields) => fields.values_mut().for_each(|item| redact(item, secret)),
        _ => {}
    }
}
