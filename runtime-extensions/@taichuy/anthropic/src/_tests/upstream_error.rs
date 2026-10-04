use super::*;

#[test]
fn obsolete_client_error_preserves_wire_facts_and_excludes_credentials() {
    let mut headers = HeaderMap::new();
    headers.insert("request-id", HeaderValue::from_static("req_obsolete"));
    headers.insert("authorization", HeaderValue::from_static("Bearer SECRET"));
    let raw = r#"{"type":"error","error":{"type":"invalid_request_error","message":"Your Claude Code version is too old. Please update."}}"#;
    let error = from_parts(
        Some(reqwest::StatusCode::BAD_REQUEST),
        &headers,
        raw.to_string(),
    );
    assert_eq!(
        error.message,
        "Your Claude Code version is too old. Please update."
    );
    let details = error.provider_details.as_ref().unwrap();
    assert_eq!(details["status_code"], 400);
    assert_eq!(details["raw_body"], raw);
    assert_eq!(details["upstream_error"]["type"], "invalid_request_error");
    assert_eq!(details["upstream_error"]["request_id"], "req_obsolete");
    assert!(!serde_json::to_string(&error).unwrap().contains("SECRET"));
}

#[test]
fn stream_error_is_typed_terminal_failure_with_truthful_message() {
    for (kind, status) in [
        ("rate_limit_error", 429),
        ("overloaded_error", 529),
        ("api_error", 500),
    ] {
        let raw = json!({"type":"error","request_id":"req_stream","error":{"type":kind,"message":"upstream refused"}}).to_string();
        let error = from_parts(None, &HeaderMap::new(), raw.clone());
        assert_eq!(error.kind, ProviderRuntimeErrorKind::ProviderUpstreamError);
        assert_eq!(error.message, "upstream refused");
        let details = error.provider_details.unwrap();
        assert_eq!(details["semantic_terminal"], true);
        assert_eq!(details["status_code"], status);
        assert_eq!(details["upstream_error"]["type"], kind);
        assert_eq!(details["upstream_error"]["request_id"], "req_stream");
        assert_eq!(details["raw_body"], raw);
    }
}
