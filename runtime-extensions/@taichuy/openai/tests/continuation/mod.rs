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
        let mut ws =
            tokio_tungstenite::tungstenite::accept_with_config(stream, Some(Default::default()))
                .unwrap();
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
            if !reason.contains("continuation connection") {
                drop(ws);
                let stream = accept_bounded(&listener);
                let mut retry = tokio_tungstenite::tungstenite::accept_with_config(
                    stream,
                    Some(Default::default()),
                )
                .unwrap();
                let rebuilt = create(&mut retry);
                assert!(rebuilt.get("previous_response_id").is_none());
                assert_eq!(rebuilt["input"], json!([item, result["input"][0]]));
                completed(&mut retry, "resp_done", json!([]));
            }
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
        let continuation_rejected = reason.contains("continuation connection");
        let (diagnostic, category) = if continuation_rejected {
            let error = result
                .iter()
                .find(|v| v["type"] == "error")
                .expect("explicit continuation rejection terminates");
            assert_eq!(error["error"]["message"], reason);
            (
                &error["error"]["provider_details"]["1flowbase_provider_recovery_diagnostics"],
                "continuation_unavailable",
            )
        } else {
            assert!(!result.iter().any(|v| v["type"] == "error"), "{result:?}");
            let recovered = &result.last().unwrap()["result"];
            assert_eq!(recovered["response_id"], "resp_done");
            assert_eq!(
                recovered["provider_metadata"]["1flowbase_provider_recovery"]["attempt"],
                1
            );
            assert_eq!(
                recovered["provider_metadata"]["1flowbase_provider_recovery"]["disposition"],
                "one_full_context_rebuild"
            );
            (
                &recovered["provider_metadata"]["1flowbase_provider_recovery_diagnostics"],
                "transport_disconnected",
            )
        };
        assert_eq!(diagnostic["last_failure"]["reason_category"], category);
        assert_eq!(diagnostic["last_failure"]["close_code"], 1008);
        assert_eq!(diagnostic["attempts"].as_array().unwrap().len(), 1);
        assert_eq!(diagnostic["last_failure"]["close_reason"], reason);
        assert!(!diagnostic.to_string().contains("fixture-credential-secret"));
        if continuation_rejected {
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
        "no unexpected connection after the bounded continuation outcome"
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
fn bare_policy_close_rebuilds_complete_history_and_preserves_reason() {
    warmup_tool_result(Some("unknown policy https://private/?token=secret"));
}

mod budget;
