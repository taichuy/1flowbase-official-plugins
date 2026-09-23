use super::*;

fn bounded_continuation(budget: u16, retry_succeeds: bool, retry_handshake_fails: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let stream = accept_bounded(&listener);
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut ws = accept_hdr(stream, |_request: &tokio_tungstenite::tungstenite::handshake::server::Request, mut response: tokio_tungstenite::tungstenite::handshake::server::Response| {
            response.headers_mut().insert("x-codex-turn-state", "fixture-route".parse().unwrap());
            Ok(response)
        }).unwrap();
        let initial = create(&mut ws);
        completed(&mut ws, "resp_owner", rebuild_model_output());
        let expected = create(&mut ws);
        assert_eq!(initial["input"], rebuild_initial_input());
        assert_eq!(expected["previous_response_id"], "resp_owner");
        assert_eq!(expected["input"], rebuild_tool_result());
        ws.send(Message::Close(Some(CloseFrame {
            code: CloseCode::Error,
            reason: "upstream websocket proxy failed".into(),
        })))
        .unwrap();
        let mut sends = 1;
        if budget > 1 {
            let stream = accept_bounded(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            if retry_handshake_fails {
                drop(stream);
            } else {
                let mut retry = tokio_tungstenite::tungstenite::accept(stream).unwrap();
                let actual = create(&mut retry);
                assert_full_context_rebuild(&initial, &rebuild_model_output(), &expected, &actual);
                sends += 1;
                if retry_succeeds {
                    completed(&mut retry, "resp_success", json!([]));
                } else {
                    retry
                        .send(Message::Close(Some(CloseFrame {
                            code: CloseCode::Error,
                            reason: "upstream websocket proxy failed".into(),
                        })))
                        .unwrap();
                }
            }
        }
        listener.set_nonblocking(true).unwrap();
        (listener, sends)
    });
    let (mut child, mut stdin, mut stdout) = spawn_native_worker();
    next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(
            &base,
            json!({"input":rebuild_initial_input()}),
            native_opaque_directive(budget),
        ),
    );
    let result = next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(
            &base,
            json!({"previous_response_id":"resp_owner","input":rebuild_tool_result()}),
            native_opaque_directive(budget),
        ),
    );
    let success = budget > 1 && retry_succeeds;
    let metadata = if success {
        assert!(!result.iter().any(|value| value["type"] == "error"));
        &result.last().unwrap()["result"]["provider_metadata"]
    } else {
        &result
            .iter()
            .find(|value| value["type"] == "error")
            .unwrap()["error"]["provider_details"]
    };
    let receipt = &metadata["1flowbase_provider_recovery"];
    assert_eq!(receipt["attempt"], budget - 1);
    if !success {
        assert_eq!(receipt["reason"], "budget_exhausted");
        assert_eq!(receipt["disposition"], "logical_invocation_retry");
        assert_eq!(receipt["commit_level"], "lifecycle_only");
    } else {
        assert_eq!(receipt["disposition"], "one_full_context_rebuild");
    }
    let diagnostics = &metadata["1flowbase_provider_recovery_diagnostics"];
    assert_eq!(diagnostics["first_failure"]["attempt"], 0);
    assert_eq!(diagnostics["first_failure"]["close_code"], 1011);
    let (listener, sends) = server.join().unwrap();
    assert_eq!(
        sends,
        if retry_handshake_fails {
            budget - 1
        } else {
            budget
        }
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn budget_one_has_one_actual_send() {
    bounded_continuation(1, false, false);
}
#[test]
fn budget_two_exhaustion_has_two_actual_sends_and_valid_last_index() {
    bounded_continuation(2, false, false);
}
#[test]
fn successful_reconnect_receipt_includes_the_second_actual_attempt() {
    bounded_continuation(2, true, false);
}

#[test]
fn expired_deadline_has_zero_connections_and_no_fabricated_receipt() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut directive = native_opaque_directive(2);
    directive["policy"]["budget"]["absolute_deadline_unix_ms"] = json!(1);
    let (mut child, mut stdin, mut stdout) = spawn_native_worker();
    let result = next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(&base, json!({"input":[]}), directive),
    );
    let details = &result
        .iter()
        .find(|value| value["type"] == "error")
        .unwrap()["error"]["provider_details"];
    assert!(details.get("1flowbase_provider_recovery").is_none());
    assert_eq!(
        details["1flowbase_provider_recovery_diagnostics"]["last_failure"]["consumed_attempts"],
        0
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn failed_reconnect_handshake_consumes_the_second_attempt_without_an_extra_send() {
    bounded_continuation(2, false, true);
}

#[test]
fn proxy_failure_without_route_rebuilds_full_context_without_replaying_cursor() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let stream = accept_bounded(&listener);
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        // No provider routing token: recovery must rebuild saved history on a new socket.
        let mut ws = tokio_tungstenite::tungstenite::accept(stream).unwrap();
        let initial = create(&mut ws);
        completed(&mut ws, "resp_no_route", rebuild_model_output());
        let delta = create(&mut ws);
        assert_eq!(delta["previous_response_id"], "resp_no_route");
        ws.send(Message::Close(Some(CloseFrame {
            code: CloseCode::Error,
            reason: "upstream websocket proxy failed".into(),
        })))
        .unwrap();
        let stream = accept_bounded(&listener);
        let mut retry = accept_hdr(
            stream,
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
             response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                assert_eq!(request.headers()["x-codex-turn-state"], "client-turn");
                Ok(response)
            },
        )
        .unwrap();
        let actual = create(&mut retry);
        assert_full_context_rebuild(&initial, &rebuild_model_output(), &delta, &actual);
        completed(&mut retry, "resp_rebuilt", json!([]));
        listener
    });
    let (mut child, mut stdin, mut stdout) = spawn_native_worker();
    let mut first = native_managed_input(
        &base,
        json!({"input":rebuild_initial_input()}),
        native_opaque_directive(3),
    );
    first["input"]["client_protocol_envelope"]["headers"]
        .as_object_mut()
        .unwrap()
        .remove("x-codex-turn-state");
    next_turn(&mut stdin, &mut stdout, first);
    let result = next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(
            &base,
            json!({"previous_response_id":"resp_no_route","input":rebuild_tool_result()}),
            native_opaque_directive(3),
        ),
    );
    assert!(
        !result.iter().any(|value| value["type"] == "error"),
        "{result:?}"
    );
    assert_eq!(
        result.last().unwrap()["result"]["response_id"],
        "resp_rebuilt"
    );
    let details = &result.last().unwrap()["result"]["provider_metadata"];
    let receipt = &details["1flowbase_provider_recovery"];
    assert_eq!(receipt["attempt"], 1);
    assert_eq!(receipt["commit_level"], "lifecycle_only");
    assert_eq!(receipt["disposition"], "one_full_context_rebuild");
    let diagnostics = &details["1flowbase_provider_recovery_diagnostics"];
    assert_eq!(diagnostics["first_failure"]["close_code"], 1011);
    assert_eq!(diagnostics["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(diagnostics["last_failure"]["consumed_attempts"], 1);
    assert_eq!(diagnostics["last_failure"]["routing_token_present"], false);
    let listener = server.join().unwrap();
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
}
