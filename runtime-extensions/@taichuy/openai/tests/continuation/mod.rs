use super::*;
use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};

fn create(ws: &mut tokio_tungstenite::tungstenite::WebSocket<std::net::TcpStream>) -> Value {
    loop {
        let message = ws.read().unwrap();
        let value: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        if value["type"] == "response.create" {
            return value;
        }
    }
}

fn completed(
    ws: &mut tokio_tungstenite::tungstenite::WebSocket<std::net::TcpStream>,
    id: &str,
    output: Value,
) {
    ws.send(Message::Text(json!({"type":"response.completed","response":{"id":id,"status":"completed","output":output}}).to_string().into())).unwrap();
}

// One worker and one logical owner: real warmup, model tool call, committed result.
fn warmup_tool_result(close_reason: Option<&'static str>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut ws = tokio_tungstenite::tungstenite::accept(stream).unwrap();
        assert_eq!(create(&mut ws)["generate"], false);
        completed(&mut ws, "resp_warmup", json!([]));
        assert_eq!(create(&mut ws)["previous_response_id"], "resp_warmup");
        let item = json!({"type":"function_call","id":"item_tool","call_id":"call_once","name":"exec","arguments":"{}","status":"completed"});
        ws.send(Message::Text(
            json!({"type":"response.output_item.done","output_index":0,"item":item})
                .to_string()
                .into(),
        ))
        .unwrap();
        completed(&mut ws, "resp_tool", json!([item]));
        let result = create(&mut ws);
        assert_eq!(result["previous_response_id"], "resp_tool");
        assert_eq!(
            result["input"],
            json!([{"type":"function_call_output","call_id":"call_once","output":"committed-once"}])
        );
        if let Some(reason) = close_reason {
            ws.send(Message::Close(Some(CloseFrame {
                code: CloseCode::Policy,
                reason: reason.into(),
            })))
            .unwrap();
        } else {
            completed(&mut ws, "resp_done", json!([]));
        }
        listener.set_nonblocking(true).unwrap();
        listener
    });
    let (mut child, mut stdin, mut stdout) = spawn_native_worker();
    let warmup = next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(
            &base,
            json!({"generate":false,"input":[]}),
            native_opaque_directive(2),
        ),
    );
    assert!(!warmup.iter().any(|v| v["type"] == "error"), "{warmup:?}");
    let tool = next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(
            &base,
            json!({"previous_response_id":"resp_warmup","input":[]}),
            native_opaque_directive(2),
        ),
    );
    assert!(!tool.iter().any(|v| v["type"] == "error"), "{tool:?}");
    let body = json!({"previous_response_id":"resp_tool","input":[{"type":"function_call_output","call_id":"call_once","output":"committed-once"}]});
    let result = next_turn(
        &mut stdin,
        &mut stdout,
        native_managed_input(&base, body.clone(), native_opaque_directive(2)),
    );
    if let Some(reason) = close_reason {
        let error = result
            .iter()
            .find(|v| v["type"] == "error")
            .expect("1008 terminates");
        let diagnostic =
            &error["error"]["provider_details"]["1flowbase_provider_recovery_diagnostics"];
        let category = if reason.contains("continuation connection") {
            "continuation_unavailable"
        } else {
            "policy_rejected"
        };
        assert_eq!(diagnostic["last_failure"]["reason_category"], category);
        assert_eq!(diagnostic["last_failure"]["close_code"], 1008);
        assert_eq!(diagnostic["attempts"].as_array().unwrap().len(), 1);
        assert!(!diagnostic.to_string().contains("secret"));
        if category == "continuation_unavailable" {
            // A later invocation cannot resurrect the recorded owner after an explicit rejection.
            let rejected = next_turn(
                &mut stdin,
                &mut stdout,
                native_managed_input(&base, body, native_opaque_directive(2)),
            );
            assert!(rejected.iter().any(|v| v["type"] == "error"));
        }
    } else {
        assert!(!result.iter().any(|v| v["type"] == "error"), "{result:?}");
    }
    let listener = server.join().unwrap();
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "no replacement connection for a policy close"
    );
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn prewarm_first_tool_result_reuses_the_same_socket() {
    warmup_tool_result(None);
}

#[test]
fn prewarm_first_tool_result_continuation_rejection_invalidates_owner() {
    warmup_tool_result(Some(
        "upstream continuation connection is unavailable; token=secret",
    ));
}

#[test]
fn unknown_policy_close_is_terminal_and_redacted() {
    warmup_tool_result(Some("unknown policy https://private/?token=secret"));
}

mod budget;
