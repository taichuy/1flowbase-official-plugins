use super::*;

#[test]
fn http_upstream_recovery_requires_explicit_nonterminal_server_error() {
    let mut statuses = (400..=599).map(|status| json!(status)).collect::<Vec<_>>();
    statuses.extend([
        Value::Null,
        json!(399),
        json!(600),
        json!(-1),
        json!("502"),
        json!(502.5),
        json!(true),
    ]);
    for status in statuses {
        for marker in [
            Some(json!(false)),
            Some(json!(true)),
            None,
            Some(Value::Null),
            Some(json!("false")),
        ] {
            for committed in [false, true] {
                let mut details =
                    json!({"status_code": status, "raw_body": "original upstream diagnostic"});
                if let Some(marker) = marker.clone() {
                    details["semantic_terminal"] = marker;
                }
                let eligible = details["semantic_terminal"] == false
                    && status
                        .as_u64()
                        .is_some_and(|status| (500..600).contains(&status))
                    && !committed;
                let source = anyhow::Error::new(ProviderRuntimeError {
                    kind: ProviderRuntimeErrorKind::ProviderUpstreamError,
                    message: "original upstream message".into(),
                    provider_summary: None,
                    provider_details: Some(details.clone()),
                });
                let mut machine = RecoveryFsm::new(RecoveryConstraints {
                    policy: RecoveryPolicyKind::NativeOpaque,
                    max_inner_attempts: 2,
                    absolute_deadline_unix_ms: None,
                    initial_commit_level: recovery::CommitLevel::LifecycleOnly,
                });
                machine.begin_attempt().unwrap();
                let transition = http_failure_transition(&mut machine, &source, committed);
                assert_eq!(
                    transition.disposition == RecoveryDisposition::LogicalInvocationRetry,
                    eligible,
                    "status={status}, marker={marker:?}, committed={committed}"
                );
                assert_eq!(machine.consumed_attempts(), 1);
                let preserved = source.downcast_ref::<ProviderRuntimeError>().unwrap();
                assert_eq!(preserved.message, "original upstream message");
                assert_eq!(preserved.provider_details.as_ref(), Some(&details));
            }
        }
    }
    for kind in [
        ProviderRuntimeErrorKind::AuthFailed,
        ProviderRuntimeErrorKind::RateLimited,
        ProviderRuntimeErrorKind::ProviderInvalidResponse,
    ] {
        let source = anyhow::Error::new(ProviderRuntimeError {
            kind,
            message: "502 does not make this a transport failure".into(),
            provider_summary: None,
            provider_details: Some(json!({"status_code":502,"semantic_terminal":false})),
        });
        let mut machine = RecoveryFsm::new(RecoveryConstraints {
            policy: RecoveryPolicyKind::NativeOpaque,
            max_inner_attempts: 2,
            absolute_deadline_unix_ms: None,
            initial_commit_level: recovery::CommitLevel::LifecycleOnly,
        });
        machine.begin_attempt().unwrap();
        assert_eq!(
            http_failure_transition(&mut machine, &source, false).disposition,
            RecoveryDisposition::TerminalInterruption
        );
    }
}

#[tokio::test]
async fn http_502_preserves_error_and_delegates_without_provider_reconnect() {
    use std::io::{Read, Write};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let body = r#"{"error":{"message":"Upstream service temporarily unavailable","type":"upstream_error"}}"#;
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 8192];
        stream.read(&mut request).unwrap();
        write!(stream, "HTTP/1.1 502 Bad Gateway\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        listener.set_nonblocking(true).unwrap();
        listener
    });
    let mut input = visibility_native_input(&base);
    input.provider_config["transport_mode"] = json!("http_sse");
    let mut emitted = Vec::new();
    let error = OpenAiProviderRuntime::default()
        .invoke_response_with_event_sink(input, |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .await
        .unwrap_err();
    assert!(emitted.is_empty());
    let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    assert_eq!(typed.kind, ProviderRuntimeErrorKind::ProviderUpstreamError);
    assert_eq!(typed.message, "Upstream service temporarily unavailable");
    let details = typed.provider_details.as_ref().unwrap();
    assert_eq!(details["status_code"], 502);
    assert_eq!(details["raw_body"], body);
    assert_eq!(details["semantic_terminal"], false);
    let receipt = &details[recovery::RECOVERY_RECEIPT_METADATA_KEY];
    assert_eq!(receipt["transport"], "provider_http");
    assert_eq!(receipt["attempt"], 0);
    assert_eq!(receipt["commit_level"], "lifecycle_only");
    assert_eq!(receipt["disposition"], "logical_invocation_retry");
    assert_no_replacement_connection(server);
}

#[tokio::test]
async fn websocket_busy_then_http_502_preserves_two_charged_attempts() {
    use std::io::{Read, Write};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let body = r#"{"error":{"message":"Upstream service temporarily unavailable","type":"upstream_error"}}"#;
    let server = thread::spawn(move || {
        let accept = || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        return stream;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "expected recovery transport request"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("accept failed: {error}"),
                }
            }
        };
        let mut websocket =
            tokio_tungstenite::tungstenite::accept_with_config(accept(), Some(Default::default()))
                .unwrap();
        websocket.read().unwrap();
        websocket
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::from(1013),
                reason: "upstream websocket is busy, please retry later".into(),
            })))
            .unwrap();
        drop(websocket);
        let mut http = accept();
        let mut request = [0; 8192];
        let read = http.read(&mut request).unwrap();
        assert!(request[..read].starts_with(b"POST "));
        write!(http, "HTTP/1.1 502 Bad Gateway\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        listener
    });
    let mut emitted = Vec::new();
    let error = OpenAiProviderRuntime::default()
        .invoke_response_with_event_sink(managed_closing_input(&base, 2), |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .await
        .unwrap_err();
    assert!(emitted.is_empty());
    let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    assert_eq!(typed.kind, ProviderRuntimeErrorKind::ProviderUpstreamError);
    let details = typed.provider_details.as_ref().unwrap();
    assert_eq!(details["status_code"], 502);
    assert_eq!(details["raw_body"], body);
    let receipt = &details[recovery::RECOVERY_RECEIPT_METADATA_KEY];
    assert_eq!(receipt["transport"], "provider_http");
    assert_eq!(receipt["attempt"], 1);
    assert_eq!(receipt["commit_level"], "lifecycle_only");
    assert_eq!(receipt["disposition"], "logical_invocation_retry");
    let diagnostics = &details[recovery_diagnostics::KEY];
    assert_eq!(diagnostics["first_failure"]["close_code"], 1013);
    assert_eq!(diagnostics["last_failure"]["consumed_attempts"], 2);
    assert_no_replacement_connection(server);
}
