use super::*;

#[tokio::test]
async fn successful_handshake_with_error_event_is_not_silent_success() {
    // Actual HTTP/SSE reader boundary, including response request id and credential exclusion.
    let response = reqwest::get(tests::start_http_error_server(
        "HTTP/1.1 200 OK",
        "text/event-stream",
        "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"upstream overloaded\"}}\n\n",
    )).await.unwrap();
    let error = read_streaming_message(response, "model".to_string(), 4096, &mut |_| Ok(()))
        .await
        .expect_err("stream error must reject invocation");
    let runtime = error
        .downcast_ref::<ProviderRuntimeError>()
        .expect("must retain typed upstream error");
    assert_eq!(runtime.message, "upstream overloaded");
    let details = runtime.provider_details.as_ref().unwrap();
    assert_eq!(details["status_code"], 529);
    assert_eq!(details["semantic_terminal"], true);
    assert_eq!(details["upstream_error"]["type"], "overloaded_error");
    assert_eq!(details["upstream_error"]["request_id"], "req_stream");
    assert!(!serde_json::to_string(runtime)
        .unwrap()
        .contains("response-secret"));
}

#[tokio::test]
async fn obsolete_client_http_error_keeps_anthropic_shape_at_reader_boundary() {
    let raw = r#"{"type":"error","error":{"type":"invalid_request_error","message":"Your Claude Code version is too old. Please update."}}"#;
    let response = reqwest::get(tests::start_http_error_server(
        "HTTP/1.1 400 Bad Request",
        "application/json",
        raw,
    ))
    .await
    .unwrap();
    let error = read_streaming_message(response, "model".to_string(), 4096, &mut |_| Ok(()))
        .await
        .expect_err("obsolete client error must reject invocation");
    let runtime = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    assert_eq!(
        runtime.message,
        "Your Claude Code version is too old. Please update."
    );
    let details = runtime.provider_details.as_ref().unwrap();
    assert_eq!(details["status_code"], 400);
    assert_eq!(details["upstream_error"]["type"], "invalid_request_error");
    assert_eq!(details["upstream_error"]["request_id"], "req_stream");
    assert_eq!(details["raw_body"], raw);
}
