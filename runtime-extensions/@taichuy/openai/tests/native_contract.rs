//! Root #2028 AC-007/010/012: real stdio worker and real WS upstream.
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use tokio_tungstenite::tungstenite::{accept_hdr, Message};

fn input(base: &str, body: Value, current: bool) -> Value {
    let mut capabilities = vec!["responses.native_passthrough"];
    if current {
        capabilities.push("responses.native_output.v1");
    }
    json!({"method":"invoke","input":{
        "contract_version":"1flowbase.provider/v2","provider_instance_id":"fixture",
        "provider_code":"openai","protocol":"openai_responses","model":"fixture",
        "provider_config":{"base_url":base,"api_key":"fixture","transport_mode":"responses_websocket"},
        "required_capabilities":capabilities,
        "run_context":{"physical_transport_session":{"logical_session_id":"logical-fixture","generation":41,"task_id":"task-fixture","state":"active","physical_deadline_unix_ms":4102444800000_i64}},
        "client_protocol_envelope":{"source_protocol":"openai_responses","headers":{"session-id":["fixture-session"],"thread-id":["fixture-thread"],"openai-beta":["responses_websockets=2026-02-06"],"x-codex-turn-state":["client-turn"]}},
        "native_transport":{"protocol":"openai_responses","wire_body":body,"digest":"fixture","size_bytes":1}
    }})
}

#[test]
fn old_host_is_rejected_by_real_worker_before_network() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        input("http://127.0.0.1:1", json!({"input":[]}), false)
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    let lines: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(lines[0]["type"], "error");
    assert!(lines[0].to_string().contains("responses.native_output.v1"));
    assert_eq!(lines.last().unwrap()["result"]["finish_reason"], "error");
}

#[test]
fn paired_worker_preserves_items_and_accepts_both_tool_result_types() {
    for (kind, field, result_kind, raw) in [
        (
            "custom_tool_call",
            "input",
            "custom_tool_call_output",
            "text(await tools.exec_command({cmd:'cat fixture'}));",
        ),
        (
            "function_call",
            "arguments",
            "function_call_output",
            "{\"path\":\"fixture\"}",
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let items = vec![
            json!({"id":"rs_1","type":"reasoning","summary":[],"encrypted_content":"opaque-fixture"}),
            json!({"id":"msg_1","type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"reading"}]}),
            json!({"id":"tool_1","type":kind,"call_id":"call_1","name":"exec",field:raw,"status":"completed"}),
        ];
        let upstream_items = items.clone();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut ws=accept_hdr(stream, |request: &tokio_tungstenite::tungstenite::handshake::server::Request, mut response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                assert_eq!(request.headers()["openai-beta"], "responses_websockets=2026-02-06");
                assert_eq!(request.headers().get_all("openai-beta").iter().count(), 1);
                assert_eq!(request.headers()["x-codex-turn-state"], "client-turn");
                response.headers_mut().insert("x-codex-turn-state", "provider-turn".parse().unwrap());
                Ok(response)
            }).unwrap();
            let first: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(first["input"][0]["type"], "additional_tools");
            ws.send(Message::Text(
                json!({"type":"response.created","response":{"id":"resp_1"}})
                    .to_string()
                    .into(),
            ))
            .unwrap();
            for (index, item) in upstream_items.iter().enumerate() {
                for event in ["response.output_item.added", "response.output_item.done"] {
                    ws.send(Message::Text(
                        json!({"type":event,"output_index":index,"item":item})
                            .to_string()
                            .into(),
                    ))
                    .unwrap();
                }
            }
            ws.send(Message::Text(json!({"type":"response.completed","response":{"id":"resp_1","output":upstream_items}}).to_string().into())).unwrap();
            let next: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(
                next["type"], "response.create",
                "native transport must not synthesize response.processed"
            );
            assert_eq!(next["previous_response_id"], "resp_1");
            assert_eq!(
                next["input"],
                json!([{"type":result_kind,"call_id":"call_1","output":"nonce"}])
            );
            let final_item = json!({"id":"msg_final","type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"nonce"}]});
            for event in ["response.output_item.added", "response.output_item.done"] {
                ws.send(Message::Text(
                    json!({"type":event,"output_index":0,"item":final_item})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            }
            ws.send(Message::Text(json!({"type":"response.completed","response":{"id":"resp_2","output":[final_item]}}).to_string().into())).unwrap();
        });
        let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut turn = |body: Value| {
            writeln!(stdin, "{}", input(&base, body, true)).unwrap();
            stdin.flush().unwrap();
            let mut events = vec![];
            loop {
                let mut line = String::new();
                assert!(stdout.read_line(&mut line).unwrap() > 0);
                let value: Value = serde_json::from_str(&line).unwrap();
                let done = value["type"] == "result";
                events.push(value);
                if done {
                    return events;
                }
            }
        };
        let first = turn(json!({"input":[{"type":"additional_tools","tools":[]}]}));
        let actual: Vec<Value> = first
            .iter()
            .filter(|v| v["type"] == "output_item" && v["phase"] == "done")
            .map(|v| v["item"].clone())
            .collect();
        assert_eq!(actual, items);
        assert_eq!(
            first.last().unwrap()["result"]["provider_metadata"]["transport"],
            "responses_websocket"
        );
        let metadata = &first.last().unwrap()["result"]["provider_metadata"];
        assert_eq!(
            metadata["1flowbase_physical_transport_session"]["generation"],
            41
        );
        assert_eq!(
            metadata["1flowbase_physical_transport_session"]["physical_state"],
            "ready"
        );
        assert_eq!(
            metadata["1flowbase_provider_invocation_timing"]["termination_kind"],
            "completed"
        );
        assert!(metadata.get("1flowbase_provider_recovery").is_none());
        let second = turn(
            json!({"previous_response_id":"resp_1","input":[{"type":result_kind,"call_id":"call_1","output":"nonce"}]}),
        );
        assert_eq!(second.last().unwrap()["result"]["final_content"], "nonce");
        assert!(second.iter().any(|v| v["type"] == "output_item"
            && v["phase"] == "done"
            && v["item"]["phase"] == "final_answer"));
        drop(stdin);
        assert!(child.wait().unwrap().success());
        server.join().unwrap();
    }
}

#[test]
fn paired_worker_preserves_native_incomplete_terminal() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let item = json!({"id":"msg_partial","type":"message","role":"assistant","phase":"final_answer","status":"incomplete","content":[{"type":"output_text","text":"partial"}]});
    let expected = item.clone();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut ws = tokio_tungstenite::tungstenite::accept(stream).unwrap();
        ws.read().unwrap();
        for event in ["response.output_item.added", "response.output_item.done"] {
            ws.send(Message::Text(
                json!({"type":event,"output_index":0,"item":item})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        }
        ws.send(Message::Text(json!({"type":"response.incomplete","response":{"id":"resp_partial","status":"incomplete","output":[item],"incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":2,"output_tokens":3,"total_tokens":5}}}).to_string().into())).unwrap();
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        input(&base, json!({"input":[]}), true)
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let lines: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert!(!lines.iter().any(|v| v["type"] == "error"));
    assert_eq!(lines.last().unwrap()["result"]["finish_reason"], "length");
    assert_eq!(lines.last().unwrap()["result"]["final_content"], "partial");
    assert_eq!(lines.last().unwrap()["result"]["usage"]["total_tokens"], 5);
    assert!(lines
        .iter()
        .any(|v| v["type"] == "output_item" && v["phase"] == "done" && v["item"] == expected));
    server.join().unwrap();
}

// AC-008: an explicitly selected native WS transport must never create an HTTP cursor.
#[test]
fn explicit_native_websocket_handshake_failure_does_not_invoke_http() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let mut requests = vec![];
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut first = String::new();
                    reader.read_line(&mut first).unwrap();
                    requests.push(first);
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        if line == "\r\n" || line.is_empty() {
                            break;
                        }
                    }
                    stream.write_all(b"HTTP/1.1 426 Upgrade Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if stop_rx.recv_timeout(Duration::from_millis(10)).is_ok() {
                        break;
                    }
                }
                Err(e) => panic!("{e}"),
            }
        }
        requests
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        input(&base, json!({"input":[]}), true)
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    stop_tx.send(()).unwrap();
    let requests = server.join().unwrap();
    assert!(!requests.is_empty());
    assert!(
        requests.iter().all(|r| r.starts_with("GET ")),
        "HTTP model fallback observed: {requests:?}"
    );
    let lines: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert!(lines.iter().any(|v| v["type"] == "error"));
    assert_eq!(lines.last().unwrap()["result"]["finish_reason"], "error");
}

/// #2085 bounded recovery: the incident directive shape. The host emits a
/// native-opaque directive with no cursor provenance claim; the provider owns
/// the physical connection and cursor facts and must decide from its own
/// verified owner record instead of terminating on `None`.
fn native_managed_input(base: &str, body: Value, directive: Value) -> Value {
    let mut value = input(base, body, true);
    value["input"]["run_context"]["provider_recovery"] = directive;
    value
}

fn native_opaque_directive(max_inner_attempts: u16) -> Value {
    json!({
        "policy": {
            "type": "native_opaque",
            "budget": {
                "max_inner_attempts": max_inner_attempts,
                "absolute_deadline_unix_ms": 4_102_444_800_000_i64
            }
        },
        "transport_epoch": 149,
        "initial_commit_level": "lifecycle_only"
    })
}

fn spawn_native_worker() -> (
    std::process::Child,
    std::process::ChildStdin,
    BufReader<std::process::ChildStdout>,
) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    (child, stdin, stdout)
}

fn next_turn(
    stdin: &mut std::process::ChildStdin,
    stdout: &mut BufReader<std::process::ChildStdout>,
    line: Value,
) -> Vec<Value> {
    writeln!(stdin, "{line}").unwrap();
    stdin.flush().unwrap();
    let mut events = vec![];
    loop {
        let mut line = String::new();
        assert!(stdout.read_line(&mut line).unwrap() > 0);
        let value: Value = serde_json::from_str(&line).unwrap();
        let done = value["type"] == "result";
        events.push(value);
        if done {
            return events;
        }
    }
}

// A missing reconnect must fail the fixture instead of leaving accept blocked forever.
fn accept_bounded(listener: &TcpListener) -> std::net::TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for fixture connection"
                );
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("fixture accept failed: {error}"),
        }
    }
}

fn rebuild_initial_input() -> Value {
    json!([{"role":"user","content":[{"type":"input_text","text":"Run exec once and report the result."}]}])
}

fn rebuild_model_output() -> Value {
    json!([
        {"type":"reasoning","id":"reason_once","summary":[],"encrypted_content":"opaque-reasoning"},
        {"type":"function_call","id":"item_once","call_id":"call_once","name":"exec","arguments":"{}","status":"completed"}
    ])
}

fn rebuild_tool_result() -> Value {
    json!([{"type":"function_call_output","call_id":"call_once","output":"committed-once"}])
}

fn assert_full_context_rebuild(initial: &Value, output: &Value, delta: &Value, actual: &Value) {
    assert_eq!(initial["type"], "response.create");
    assert_eq!(initial["input"], rebuild_initial_input());
    assert_eq!(delta["type"], "response.create");
    assert_eq!(delta["input"], rebuild_tool_result());
    let mut expected = delta.clone();
    expected
        .as_object_mut()
        .unwrap()
        .remove("previous_response_id");
    let mut items = initial["input"].as_array().unwrap().clone();
    items.extend(output.as_array().unwrap().iter().cloned());
    items.extend(delta["input"].as_array().unwrap().iter().cloned());
    expected["input"] = json!(items);
    assert_eq!(actual, &expected, "physical reconnect must preserve ordered original input, opaque model output and exactly one committed tool result, without the old cursor");
}

#[test]
fn native_managed_proxy_failure_rebuilds_full_context_on_new_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let first_stream = accept_bounded(&listener);
        let mut first = accept_hdr(first_stream, |request: &tokio_tungstenite::tungstenite::handshake::server::Request, mut response: tokio_tungstenite::tungstenite::handshake::server::Response| {
            assert_eq!(request.headers()["x-codex-turn-state"], "client-turn");
            response
                .headers_mut()
                .insert("x-codex-turn-state", "provider-turn".parse().unwrap());
            Ok(response)
        })
        .unwrap();
        let initial: Value =
            serde_json::from_str(first.read().unwrap().to_text().unwrap()).unwrap();
        first
            .send(Message::Text(
                json!({"type":"response.completed","response":{"id":"resp_1","output":rebuild_model_output()}})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        let continuation = first.read().unwrap().into_text().unwrap();
        assert!(
            continuation.contains("\"previous_response_id\":\"resp_1\""),
            "continuation should carry the response cursor: {continuation}"
        );
        first
            .send(Message::Close(Some(
                tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Error,
                    reason: "upstream websocket proxy failed".into(),
                },
            )))
            .unwrap();

        let retry_stream = accept_bounded(&listener);
        let mut retry = accept_hdr(
            retry_stream,
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
             response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                assert_eq!(
                    request
                        .headers()
                        .get("x-codex-turn-state")
                        .and_then(|value| value.to_str().ok()),
                    Some("client-turn"),
                    "full-context rebuild uses caller routing metadata, not the previous socket route"
                );
                Ok(response)
            },
        )
        .unwrap();
        let retried = retry.read().unwrap().into_text().unwrap();
        let continuation: Value = serde_json::from_str(&continuation).unwrap();
        let retried: Value = serde_json::from_str(&retried).unwrap();
        assert_full_context_rebuild(&initial, &rebuild_model_output(), &continuation, &retried);
        retry
            .send(Message::Text(
                json!({"type":"response.completed","response":{"id":"resp_2","output":[{"id":"msg_2","type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"native recovered"}]}]}})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "successful rebuild must not create an extra socket"
        );
    });

    let (mut child, mut stdin, mut stdout) = spawn_native_worker();
    let first = next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(
            &base,
            json!({"input":rebuild_initial_input()}),
            native_opaque_directive(2),
        ),
    );
    assert_eq!(first.last().unwrap()["type"], "result");
    let second = next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(
            &base,
            json!({"previous_response_id":"resp_1","input":rebuild_tool_result()}),
            native_opaque_directive(2),
        ),
    );
    assert!(
        second.iter().all(|value| value["type"] != "error"),
        "a verified owner must reconnect instead of failing: {second:?}"
    );
    assert_eq!(
        second.last().unwrap()["result"]["response_id"],
        json!("resp_2")
    );
    let recovery =
        &second.last().unwrap()["result"]["provider_metadata"]["1flowbase_provider_recovery"];
    assert_eq!(recovery["disposition"], json!("one_full_context_rebuild"));
    assert_eq!(recovery["attempt"], 1);
    assert_eq!(recovery["commit_level"], json!("lifecycle_only"));
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    server.join().unwrap();
}

#[test]
fn native_managed_recovery_failure_preserves_the_original_transport_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (first_stream, _) = listener.accept().unwrap();
        let mut first = accept_hdr(first_stream, |_request: &tokio_tungstenite::tungstenite::handshake::server::Request, mut response: tokio_tungstenite::tungstenite::handshake::server::Response| {
            response
                .headers_mut()
                .insert("x-codex-turn-state", "provider-turn".parse().unwrap());
            Ok(response)
        })
        .unwrap();
        let _ = first.read().unwrap();
        first
            .send(Message::Text(
                json!({"type":"response.completed","response":{"id":"resp_1","output":[]}})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        let _ = first.read().unwrap();
        first
            .send(Message::Close(Some(
                tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Error,
                    reason: "upstream websocket proxy failed".into(),
                },
            )))
            .unwrap();

        let (retry_stream, _) = listener.accept().unwrap();
        let mut retry = tokio_tungstenite::tungstenite::accept(retry_stream).unwrap();
        let _ = retry.read().unwrap();
        retry
            .send(Message::Close(Some(
                tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code:
                        tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Policy,
                    reason: "upstream continuation connection is unavailable; private-token=secret"
                        .into(),
                },
            )))
            .unwrap();
    });

    let (mut child, mut stdin, mut stdout) = spawn_native_worker();
    let _ = next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(&base, json!({"input":[]}), native_opaque_directive(2)),
    );
    let second = next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(
            &base,
            json!({"previous_response_id":"resp_1","input":[{"type":"function_call_output","call_id":"call_1","output":"nonce"}]}),
            native_opaque_directive(2),
        ),
    );
    let error = second
        .iter()
        .find(|value| value["type"] == "error")
        .expect("the replacement association must fail closed");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("continuation connection is unavailable"),
        "the final failure stays primary: {error}"
    );
    let details = &error["error"]["provider_details"];
    let receipt = &details["1flowbase_provider_recovery"];
    assert_eq!(receipt["disposition"], json!("terminal_interruption"));
    assert_eq!(receipt["attempt"], 1);
    assert_eq!(receipt["reason"], "budget_exhausted");
    assert_eq!(receipt["commit_level"], json!("lifecycle_only"));
    assert!(
        receipt.get("socket_incarnation").is_some(),
        "a stream failure on a real socket reports its incarnation: {receipt}"
    );
    let diagnostics = &details["1flowbase_provider_recovery_diagnostics"];
    assert_eq!(diagnostics["first_failure"]["close_code"], 1011);
    assert_eq!(diagnostics["last_failure"]["close_code"], 1008);
    assert_eq!(diagnostics["first_failure"]["attempt"], 0);
    assert_eq!(diagnostics["last_failure"]["attempt"], 1);
    assert_eq!(diagnostics["last_failure"]["consumed_attempts"], 2);
    assert_eq!(diagnostics["attempts"].as_array().unwrap().len(), 2);
    assert!(!details.to_string().contains("private-token"));
    let original = &details["1flowbase_provider_recovery_original_error"];
    assert!(
        original["message"]
            .as_str()
            .unwrap()
            .contains("upstream websocket proxy failed"),
        "the original 1011 must survive a bounded recovery attempt: {details}"
    );
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    server.join().unwrap();
}

#[test]
fn native_managed_preconnect_failure_reports_a_socketless_precommit_retry_receipt() {
    let unused = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", unused.local_addr().unwrap());
    drop(unused);

    let (mut child, mut stdin, mut stdout) = spawn_native_worker();
    let lines = next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(&base, json!({"input":[]}), native_opaque_directive(2)),
    );
    let error = lines
        .iter()
        .find(|value| value["type"] == "error")
        .expect("an unreachable endpoint is a real failure");
    let receipt = &error["error"]["provider_details"]["1flowbase_provider_recovery"];
    assert_eq!(receipt["disposition"], json!("logical_invocation_retry"));
    assert_eq!(receipt["commit_level"], json!("lifecycle_only"));
    assert_eq!(receipt["attempt"], 0);
    assert_eq!(receipt["reason"], "transport_disconnected");
    let diagnostics =
        &error["error"]["provider_details"]["1flowbase_provider_recovery_diagnostics"];
    assert_eq!(diagnostics["first_failure"], diagnostics["last_failure"]);
    assert_eq!(diagnostics["last_failure"]["consumed_attempts"], 1);
    assert_eq!(diagnostics["attempts"].as_array().unwrap().len(), 1);
    assert!(
        receipt.get("socket_incarnation").is_none(),
        "a pre-connect failure must not fabricate a socket incarnation: {receipt}"
    );
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
}

#[path = "continuation/mod.rs"]
mod continuation;

#[test]
fn idle_worker_maintains_ping_and_routes_first_close_through_existing_recovery() {
    for (token, code, reason, succeeds, category) in [
        (
            true,
            1011u16,
            "transport lost secret-sentinel",
            true,
            "transport_disconnected",
        ),
        (
            false,
            1011,
            "transport lost secret-sentinel",
            true,
            "transport_disconnected",
        ),
        (
            true,
            1008,
            "policy secret-sentinel",
            false,
            "policy_rejected",
        ),
        (
            true,
            1008,
            "upstream continuation connection is unavailable secret-sentinel",
            false,
            "continuation_unavailable",
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (idle_tx, idle_rx) = std::sync::mpsc::channel();
        let (closed_tx, closed_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let stream = accept_bounded(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut ws = accept_hdr(stream, move |_: &tokio_tungstenite::tungstenite::handshake::server::Request, mut response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                if token { response.headers_mut().insert("x-codex-turn-state", "private-routing-sentinel".parse().unwrap()); }
                Ok(response)
            }).unwrap();
            let initial: Value =
                serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            ws.send(Message::Text(
                json!({"type":"response.completed","response":{"id":"idle_resp_1","output":rebuild_model_output()}})
                    .to_string()
                    .into(),
            ))
            .unwrap();
            idle_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            ws.send(Message::Ping(vec![1, 2, 3].into())).unwrap();
            assert_eq!(ws.read().unwrap(), Message::Pong(vec![1, 2, 3].into()));
            ws.send(Message::Close(Some(
                tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code: code.into(),
                    reason: reason.into(),
                },
            )))
            .unwrap();
            assert!(matches!(ws.read(), Ok(Message::Close(_))));
            closed_tx.send(()).unwrap();
            if succeeds {
                let stream = accept_bounded(&listener);
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut retry = accept_hdr(stream, |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                    assert_eq!(request.headers()["x-codex-turn-state"], "client-turn");
                    Ok(response)
                }).unwrap();
                let body: Value =
                    serde_json::from_str(retry.read().unwrap().to_text().unwrap()).unwrap();
                let mut delta = initial.clone();
                delta["previous_response_id"] = json!("idle_resp_1");
                delta["input"] = rebuild_tool_result();
                assert_full_context_rebuild(&initial, &rebuild_model_output(), &delta, &body);
                retry.send(Message::Text(json!({"type":"response.completed","response":{"id":"idle_resp_2","output":[]}}).to_string().into())).unwrap();
                stop_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                assert!(
                    matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
                    "successful rebuild must not create an extra socket"
                );
            } else {
                stop_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                listener.set_nonblocking(true).unwrap();
                assert!(
                    matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
                    "idle close without safe continuation must not create another socket"
                );
            }
        });
        let (mut child, mut stdin, mut stdout) = spawn_native_worker();
        let mut first = native_managed_input(
            &base,
            json!({"input":rebuild_initial_input()}),
            native_opaque_directive(2),
        );
        // Absence of an upstream routing token is intentional in the no-token case.
        first["input"]["client_protocol_envelope"]["headers"]
            .as_object_mut()
            .unwrap()
            .remove("x-codex-turn-state");
        let first = next_turn(&mut stdin, &mut stdout, first);
        assert_eq!(
            first.last().unwrap()["result"]["response_id"],
            "idle_resp_1"
        );
        idle_tx.send(()).unwrap();
        closed_rx.recv_timeout(Duration::from_secs(4)).unwrap();
        let second = next_turn(
            &mut stdin,
            &mut stdout,
            native_managed_input(
                &base,
                json!({"previous_response_id":"idle_resp_1","input":rebuild_tool_result()}),
                native_opaque_directive(2),
            ),
        );
        let diagnostic = if succeeds {
            assert_eq!(
                second.last().unwrap()["result"]["response_id"],
                "idle_resp_2"
            );
            &second.last().unwrap()["result"]["provider_metadata"]
                ["1flowbase_provider_recovery_diagnostics"]["first_failure"]
        } else {
            &second
                .iter()
                .find(|v| v["type"] == "error")
                .expect("must terminate")["error"]["provider_details"]
                ["1flowbase_provider_recovery_diagnostics"]["first_failure"]
        };
        assert_eq!(diagnostic["failure_phase"], "idle");
        assert_eq!(diagnostic["reason_category"], category);
        assert_eq!(diagnostic["routing_token_present"], token);
        assert_eq!(diagnostic["association_present"], true);
        assert_eq!(
            diagnostic["recovery_decision"],
            if succeeds {
                "retry_websocket"
            } else {
                "terminal"
            }
        );
        assert!(!diagnostic.to_string().contains("sentinel"));
        stop_tx.send(()).unwrap();
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
        server.join().unwrap();
    }
}
