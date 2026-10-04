use super::*;

/// Only upstream response facts are projected; response credentials are never copied.
pub(super) fn from_parts(
    status: Option<reqwest::StatusCode>,
    headers: &HeaderMap,
    raw_body: String,
) -> ProviderRuntimeError {
    let payload = serde_json::from_str::<Value>(&raw_body).ok();
    let mut upstream = payload
        .as_ref()
        .and_then(|body| body.get("error"))
        .filter(|error| error.is_object())
        .cloned();
    let message = upstream
        .as_ref()
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            if raw_body.is_empty() {
                status
                    .map(|status| format!("HTTP {status}"))
                    .unwrap_or_else(|| "Anthropic stream error event received".to_string())
            } else {
                raw_body.clone()
            }
        });
    if upstream.is_none() {
        let error_type = match status.map(|status| status.as_u16()) {
            Some(400 | 422) => "invalid_request_error",
            Some(401) => "authentication_error",
            Some(403) => "permission_error",
            Some(404) => "not_found_error",
            Some(413) => "request_too_large",
            Some(429) => "rate_limit_error",
            Some(529) => "overloaded_error",
            _ => "api_error",
        };
        // A normalized protocol error carries plain-body diagnostics and request id
        // through Native's existing upstream facts boundary.
        upstream = Some(json!({"type":error_type,"message":message}));
    }
    let request_id = response_request_id(headers).or_else(|| {
        payload
            .as_ref()
            .and_then(|body| body.get("request_id"))
            .and_then(Value::as_str)
            .map(|id| id.chars().take(128).collect::<String>())
    });
    let mut details = Map::new();
    if let Some(status) = status {
        details.insert("status".to_string(), json!(status.as_u16()));
        details.insert("status_code".to_string(), json!(status.as_u16()));
    } else {
        // A successful HTTP handshake can still carry a semantic stream failure.
        details.insert("semantic_terminal".to_string(), json!(true));
        let inferred = match upstream
            .as_ref()
            .and_then(|error| error.get("type"))
            .and_then(Value::as_str)
        {
            Some("invalid_request_error") => 400,
            Some("authentication_error") => 401,
            Some("permission_error") => 403,
            Some("not_found_error") => 404,
            Some("request_too_large") => 413,
            Some("rate_limit_error") => 429,
            Some("overloaded_error") => 529,
            _ => 500,
        };
        details.insert("status_code".to_string(), json!(inferred));
    }
    if let Some(request_id) = request_id {
        details.insert("request_id".to_string(), json!(request_id));
        if let Some(error) = upstream.as_mut().and_then(Value::as_object_mut) {
            error.entry("request_id").or_insert(json!(request_id));
        }
    }
    if let Some(upstream) = upstream {
        details.insert("upstream_error".to_string(), upstream);
    }
    details.insert("raw_body".to_string(), json!(raw_body));
    ProviderRuntimeError {
        kind: ProviderRuntimeErrorKind::ProviderUpstreamError,
        message: message.clone(),
        provider_summary: Some(message),
        provider_details: Some(Value::Object(details)),
    }
}

#[cfg(test)]
#[path = "_tests/upstream_error.rs"]
mod tests;
