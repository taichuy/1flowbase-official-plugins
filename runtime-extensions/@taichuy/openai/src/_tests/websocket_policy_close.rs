use super::*;

fn start_policy_close(
    events: Vec<Value>,
    reason: &'static str,
) -> (String, thread::JoinHandle<TcpListener>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut ws = tokio_tungstenite::tungstenite::accept(stream).unwrap();
        ws.read().unwrap();
        for event in events {
            ws.send(Message::Text(event.to_string().into())).unwrap();
        }
        // An upstream error may already have caused the client to release the socket.
        let _ = ws.send(Message::Close(Some(CloseFrame {
            code: CloseCode::Policy,
            reason: reason.into(),
        })));
        listener.set_nonblocking(true).unwrap();
        listener
    });
    (base, server)
}

#[tokio::test]
async fn bare_1008_precommit_grants_logical_retry_with_safe_physical_evidence() {
    for reason in [
        "",
        "policy private-canary https://private/?token=credential-canary",
    ] {
        let (base, server) = start_policy_close(vec![], reason);
        let mut emitted = Vec::new();
        let mut runtime = OpenAiProviderRuntime::default();
        let error = runtime
            .invoke_response_with_event_sink(visibility_native_input(&base), |event| {
                emitted.push(event.clone());
                Ok(())
            })
            .await
            .unwrap_err();
        assert!(emitted.is_empty());
        let diagnostic = assert_safe_terminal_with_disposition(
            &error,
            "transport_disconnected",
            "logical_invocation_retry",
            ProviderRuntimeErrorKind::ProviderTransportUnavailable,
        );
        let failure = &diagnostic["last_failure"];
        assert_eq!(failure["kind"], "websocket_close");
        assert_eq!(failure["close_code"], 1008);
        assert_eq!(failure["reason_category"], "transport_disconnected");
        assert_eq!(failure["buffered_scaffold_events"], 0);
        assert_eq!(failure["buffered_scaffold_bytes"], 0);
        assert!(failure["semantic_event_kind"].is_null());
        assert!(diagnostic["association_valid"].is_null());
        assert!(runtime.websocket_invalid_associations.is_empty());
        assert_no_replacement_connection(server);
    }
}

#[tokio::test]
async fn bare_1008_obeys_existing_attempt_budget() {
    let (base, server) = start_policy_close(vec![], "");
    let mut input = visibility_native_input(&base);
    input
        .run_context
        .get_mut(recovery::RECOVERY_DIRECTIVE_CONTEXT_KEY)
        .unwrap()["policy"]["budget"]["max_inner_attempts"] = json!(1);
    let error = OpenAiProviderRuntime::default()
        .invoke_response(input)
        .await
        .unwrap_err();
    let diagnostic = assert_safe_terminal(&error, "budget_exhausted");
    assert_eq!(
        diagnostic["last_failure"]["reason_category"],
        "transport_disconnected"
    );
    assert_eq!(diagnostic["last_failure"]["close_code"], 1008);
    assert_no_replacement_connection(server);
}

#[tokio::test]
async fn bare_1008_after_text_or_tool_output_cannot_replay() {
    for event in [
        json!({"type":"response.output_text.delta","delta":"visible"}),
        json!({"type":"response.function_call_arguments.delta","item_id":"call_once","delta":"{}"}),
    ] {
        let (base, server) = start_policy_close(vec![event], "private-canary");
        let mut emitted = Vec::new();
        let error = OpenAiProviderRuntime::default()
            .invoke_response_with_event_sink(visibility_native_input(&base), |event| {
                emitted.push(event.clone());
                Ok(())
            })
            .await
            .unwrap_err();
        assert!(emitted.iter().any(websocket_event_commits_output));
        let diagnostic = assert_safe_terminal_with_disposition(
            &error,
            "semantic_failed",
            "terminal_interruption",
            ProviderRuntimeErrorKind::ProviderTransportUnavailable,
        );
        assert_eq!(
            diagnostic["last_failure"]["reason_category"],
            "transport_disconnected"
        );
        assert_eq!(diagnostic["last_failure"]["close_code"], 1008);
        assert_no_replacement_connection(server);
    }
}

#[tokio::test]
async fn explicit_upstream_failure_before_1008_remains_terminal() {
    for event in [
        json!({"type":"response.failed","response":{"error":{"code":"invalid_request","message":"explicit refusal"}}}),
        json!({"type":"error","error":{"code":"invalid_api_key","message":"unauthorized"}}),
        json!({"type":"error","error":{"code":"invalid_request","message":"unsupported client event"}}),
    ] {
        let (base, server) = start_policy_close(vec![event.clone()], "private-canary");
        let error = OpenAiProviderRuntime::default()
            .invoke_response(visibility_native_input(&base))
            .await
            .unwrap_err();
        let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
        assert_eq!(typed.kind, ProviderRuntimeErrorKind::ProviderUpstreamError);
        let details = typed.provider_details.as_ref().unwrap();
        let receipt = &details[recovery::RECOVERY_RECEIPT_METADATA_KEY];
        assert_eq!(receipt["attempt"], 0);
        assert!(matches!(
            receipt["disposition"].as_str(),
            Some("semantic_terminal" | "terminal_interruption")
        ));
        assert_eq!(receipt["commit_level"], "terminal");
        let upstream_error = if event["type"] == "response.failed" {
            &event["response"]["error"]
        } else {
            &event["error"]
        };
        assert_eq!(details["upstream_error"], *upstream_error);
        assert_eq!(
            details[recovery_diagnostics::KEY]["last_failure"]["kind"],
            "provider_typed"
        );
        assert!(!serde_json::to_string(typed)
            .unwrap()
            .contains("private-canary"));
        assert_no_replacement_connection(server);
    }
}

#[tokio::test]
async fn fixed_continuation_unavailable_1008_remains_terminal() {
    let (base, server) = start_policy_close(
        vec![],
        "upstream continuation connection is unavailable; private-canary",
    );
    let error = OpenAiProviderRuntime::default()
        .invoke_response(visibility_native_input(&base))
        .await
        .unwrap_err();
    let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    assert_eq!(
        typed.kind,
        ProviderRuntimeErrorKind::ProviderTransportUnavailable
    );
    let details = typed.provider_details.as_ref().unwrap();
    let receipt = &details[recovery::RECOVERY_RECEIPT_METADATA_KEY];
    assert_eq!(receipt["disposition"], "terminal_interruption");
    assert_eq!(receipt["commit_level"], "terminal");
    assert_eq!(receipt["reason"], "transport_disconnected");
    assert_eq!(receipt["attempt"], 0);
    let diagnostics = &details[recovery_diagnostics::KEY];
    assert!(!serde_json::to_string(typed)
        .unwrap()
        .contains("private-canary"));
    assert_eq!(
        diagnostics["last_failure"]["reason_category"],
        "continuation_unavailable"
    );
    assert_eq!(diagnostics["association_valid"], false);
    assert_no_replacement_connection(server);
}
