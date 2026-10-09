use super::*;
use std::net::TcpListener;

fn native_fallback_http_body(stream: &mut std::net::TcpStream) -> Value {
    use std::io::Read;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream.read(&mut chunk).unwrap();
        assert!(read > 0, "HTTP request ended before its body");
        bytes.extend_from_slice(&chunk[..read]);
        assert!(bytes.len() < 65536, "fixture request is bounded");
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..end]).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .expect("fixture JSON has content-length");
            if bytes.len() >= end + 4 + length {
                return serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap();
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeFallbackCase {
    Complete,
    BudgetOne,
    BudgetTwo,
    Expired,
    ForceWebsocket,
    Semantic,
    Policy,
    Protocol,
    Authorization,
    MissingCompletedOutput,
    ForeignScope,
    ChangedConfig,
    ConnectionBound,
}

async fn native_full_context_fallback_fixture(case: NativeFallbackCase) {
    use std::io::Write;
    use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let reasoning = json!({"id":"rs_1","type":"reasoning","summary":[],"encrypted_content":"opaque+/=ciphertext","opaque_extension":{"keep":true}});
    let tool = json!({"id":"fc_1","type":"function_call","call_id":"call_once","name":"exec","arguments":"{\"path\":\"fixture\"}","status":"completed"});
    let output = json!([reasoning, tool]);
    let result_item =
        json!({"type":"function_call_output","call_id":"call_once","output":"committed-once"});
    let expected_input = json!([
        {"type":"message","role":"user","content":[{"type":"input_text","text":"call the tool"}]},
        reasoning, tool, result_item,
    ]);
    let (closed_tx, closed_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut ws = tokio_tungstenite::tungstenite::accept(stream).unwrap();
        let first: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(
            first["input"],
            json!([{"role":"user","content":"call the tool"}]),
            "existing WebSocket adaptation preserves the initial native string text"
        );
        ws.send(Message::Text(
            json!({"type":"response.output_item.done","output_index":1,"item":tool})
                .to_string()
                .into(),
        ))
        .unwrap();
        let mut completed = json!({"type":"response.completed","response":{"id":"resp_tool","status":"completed","output":output}});
        if case == NativeFallbackCase::MissingCompletedOutput {
            completed["response"]
                .as_object_mut()
                .unwrap()
                .remove("output");
        }
        ws.send(Message::Text(completed.to_string().into()))
            .unwrap();
        ws.send(Message::Close(None)).unwrap();
        drop(ws);
        closed_tx.send(()).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut rebuilds = Vec::new();
        let mut http_bodies = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut peek = [0_u8; 16];
                    let read = stream.peek(&mut peek).unwrap();
                    if peek[..read].starts_with(b"GET ") {
                        let mut ws = tokio_tungstenite::tungstenite::accept(stream).unwrap();
                        let rebuilt: Value =
                            serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
                        assert!(rebuilt.get("previous_response_id").is_none());
                        assert_eq!(rebuilt["input"], expected_input);
                        assert_eq!(rebuilt["store"], false);
                        assert_eq!(rebuilt["include"], json!(["reasoning.encrypted_content"]));
                        rebuilds.push(rebuilt);
                        if case == NativeFallbackCase::Semantic {
                            ws.send(Message::Text(json!({"type":"response.output_text.delta","delta":"semantic output","output_index":0,"content_index":0}).to_string().into())).unwrap();
                        } else if case == NativeFallbackCase::Protocol {
                            ws.send(Message::Text("invalid-json".into())).unwrap();
                        } else if case == NativeFallbackCase::Authorization {
                            ws.send(Message::Text(json!({"type":"error","error":{"code":"invalid_api_key","message":"upstream authorization rejected the request"}}).to_string().into())).unwrap();
                        }
                        let _ = ws.send(Message::Close(if case == NativeFallbackCase::Policy {
                            Some(CloseFrame {
                                code: CloseCode::Policy,
                                reason: "fixture policy refusal".into(),
                            })
                        } else {
                            None
                        }));
                    } else {
                        assert!(
                            peek[..read].starts_with(b"POST "),
                            "unexpected upstream request"
                        );
                        let body = native_fallback_http_body(&mut stream);
                        assert!(body.get("previous_response_id").is_none());
                        assert_eq!(body["input"], expected_input);
                        let mut expected = rebuilds
                            .last()
                            .expect("HTTP must follow a materialized WS rebuild")
                            .clone();
                        expected.as_object_mut().unwrap().remove("type");
                        assert_eq!(
                            body, expected,
                            "HTTP sends exactly the effective full-context request"
                        );
                        http_bodies.push(body);
                        let response = "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_done\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"finished\"}]}]}}\n\n";
                        write!(stream,"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",response.len(),response).unwrap();
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if done_rx.try_recv().is_ok() {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "bounded native fallback fixture timed out"
                    );
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("fixture accept failed: {error}"),
            }
        }
        (rebuilds, http_bodies)
    });
    let make_input = |body: Value| {
        ProviderInvocationInput {
        provider_instance_id: "fixture".into(), provider_code: "openai".into(),
        model: "fixture-model".into(), protocol: "openai_responses".into(),
        provider_config: json!({"base_url":base,"api_key":"fixture","transport_mode":"responses_websocket"}),
        model_parameters: [("responses_transport_policy".into(), json!(if case == NativeFallbackCase::ForceWebsocket {"force_websocket"} else {"inherit"}))].into(),
        required_capabilities: [ProviderInvocationCapability::ResponsesNativePassthrough, ProviderInvocationCapability::ResponsesNativeOutputV1].into(),
        run_context: [
            (TRANSPORT_SESSION_CONTEXT_KEY.into(), json!({"logical_session_id":"logical-fixture","generation":41,"task_id":"task-fixture","state":"active","physical_deadline_unix_ms":4102444800000_i64})),
            (recovery::RECOVERY_DIRECTIVE_CONTEXT_KEY.into(), json!({"policy":{"type":"native_opaque","budget":{"max_inner_attempts":3,"absolute_deadline_unix_ms":4102444800000_i64}},"transport_epoch":17,"initial_commit_level":"lifecycle_only"})),
        ].into(),
        native_transport: Some(ProviderNativeTransport {protocol:"openai_responses".into(),wire_body:body,digest:"fixture".into(),size_bytes:1}),
        ..Default::default()
    }
    };
    let mut runtime = OpenAiProviderRuntime::default();
    let mut events = Vec::new();
    runtime.invoke_response_with_event_sink(make_input(json!({"input":"call the tool","store":false,"include":["reasoning.encrypted_content"]})), |event| {events.push(event.clone());Ok(())}).await.unwrap();
    closed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut continuation = make_input(
        json!({"previous_response_id":"resp_tool","input":[result_item],"store":false,"include":["reasoning.encrypted_content"]}),
    );
    match case {
        NativeFallbackCase::BudgetOne | NativeFallbackCase::BudgetTwo => {
            continuation
                .run_context
                .get_mut(recovery::RECOVERY_DIRECTIVE_CONTEXT_KEY)
                .unwrap()["policy"]["budget"]["max_inner_attempts"] =
                json!(if case == NativeFallbackCase::BudgetOne {
                    1
                } else {
                    2
                });
        }
        NativeFallbackCase::Expired => {
            continuation
                .run_context
                .get_mut(recovery::RECOVERY_DIRECTIVE_CONTEXT_KEY)
                .unwrap()["policy"]["budget"]["absolute_deadline_unix_ms"] = json!(1)
        }
        NativeFallbackCase::ForeignScope => {
            continuation
                .run_context
                .get_mut(TRANSPORT_SESSION_CONTEXT_KEY)
                .unwrap()["logical_session_id"] = json!("foreign-session")
        }
        NativeFallbackCase::ChangedConfig => {
            continuation.provider_config["organization"] = json!("foreign-organization")
        }
        NativeFallbackCase::ConnectionBound => {
            continuation
                .run_context
                .get_mut(recovery::RECOVERY_DIRECTIVE_CONTEXT_KEY)
                .unwrap()["cursor_provenance"] = json!({"binding":{"type":"connection_bound","transport_epoch":17,"socket_incarnation":1}})
        }
        _ => {}
    }
    let resumed = runtime
        .invoke_response_with_event_sink(continuation, |event| {
            events.push(event.clone());
            Ok(())
        })
        .await;
    done_tx.send(()).unwrap();
    let (rebuilds, http_bodies) = server.join().unwrap();
    let expected_rebuilds = usize::from(matches!(
        case,
        NativeFallbackCase::Complete
            | NativeFallbackCase::BudgetTwo
            | NativeFallbackCase::ForceWebsocket
            | NativeFallbackCase::Semantic
            | NativeFallbackCase::Policy
            | NativeFallbackCase::Protocol
            | NativeFallbackCase::Authorization
    ));
    assert_eq!(rebuilds.len(), expected_rebuilds, "case={case:?}");
    assert_eq!(
        http_bodies.len(),
        usize::from(case == NativeFallbackCase::Complete),
        "case={case:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ProviderStreamEvent::ToolCallCommit { .. }))
            .count(),
        1,
        "tool is committed exactly once"
    );
    if case == NativeFallbackCase::Complete {
        let resumed = resumed.expect("scoped complete native context recovers through HTTP");
        assert_eq!(resumed.result.response_id.as_deref(), Some("resp_done"));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ProviderStreamEvent::Finish { .. }))
                .count(),
            2,
            "one terminal completion for the tool turn and one for the recovered continuation"
        );
        assert_eq!(
            resumed.result.provider_metadata[recovery::RECOVERY_RECEIPT_METADATA_KEY],
            json!({"attempt":2,"transport":"provider_http","transport_epoch":17,"commit_level":"lifecycle_only","disposition":"one_full_context_rebuild","reason":"transport_disconnected"})
        );
        let diagnostics = &resumed.result.provider_metadata[recovery_diagnostics::KEY];
        assert_eq!(diagnostics["first_failure"]["attempt"], 0);
        assert_eq!(diagnostics["last_failure"]["attempt"], 1);
        assert_eq!(diagnostics["last_failure"]["consumed_attempts"], 2);
        assert_eq!(
            diagnostics["first_failure"]["recovery_decision"],
            "retry_websocket"
        );
        assert_eq!(
            diagnostics["last_failure"]["recovery_decision"],
            "retry_http"
        );
        assert!(
            !resumed.result.provider_metadata[recovery::RECOVERY_ORIGINAL_ERROR_METADATA_KEY]
                .is_null()
        );
    } else {
        assert!(resumed.is_err(), "case={case:?} must refuse HTTP");
    }
}

#[tokio::test]
async fn native_string_idle_close_rebuild_then_http_sends_complete_scoped_body() {
    native_full_context_fallback_fixture(NativeFallbackCase::Complete).await;
}

#[tokio::test]
async fn native_full_context_http_refuses_incomplete_foreign_committed_or_fenced_replay() {
    for case in [
        NativeFallbackCase::BudgetOne,
        NativeFallbackCase::BudgetTwo,
        NativeFallbackCase::Expired,
        NativeFallbackCase::ForceWebsocket,
        NativeFallbackCase::Semantic,
        NativeFallbackCase::Policy,
        NativeFallbackCase::Protocol,
        NativeFallbackCase::Authorization,
        NativeFallbackCase::MissingCompletedOutput,
        NativeFallbackCase::ForeignScope,
        NativeFallbackCase::ChangedConfig,
        NativeFallbackCase::ConnectionBound,
    ] {
        native_full_context_fallback_fixture(case).await;
    }
}

#[test]
fn native_string_completed_history_rebuilds_exact_tool_continuation() {
    let initial = json!({
        "model": "fixture-model", "input": "  call the tool\n",
        "store": false, "include": ["reasoning.encrypted_content"],
    });
    let wire_before = initial.clone();
    let reasoning = json!({
        "id": "rs_1", "type": "reasoning", "summary": [],
        "encrypted_content": "opaque+/=ciphertext", "opaque_extension": {"keep": true},
    });
    let tool_call = json!({
        "id": "fc_1", "type": "function_call", "call_id": "call_1",
        "name": "exec", "arguments": "{\"path\":\"fixture\"}", "status": "completed",
    });
    let completed_output = vec![reasoning.clone(), tool_call.clone()];
    let tool_result =
        json!({"type":"function_call_output","call_id":"call_1","output":"fixture-result"});
    let continuation = json!({
        "model": "fixture-model", "previous_response_id": "resp_tool",
        "input": [tool_result], "store": false,
        "include": ["reasoning.encrypted_content"],
    });
    let input = ProviderInvocationInput {
        provider_instance_id: "fixture".into(),
        provider_code: "openai".into(),
        protocol: "openai_responses".into(),
        model: "fixture-model".into(),
        native_transport: Some(ProviderNativeTransport {
            protocol: "openai_responses".into(),
            wire_body: continuation.clone(),
            digest: "fixture".into(),
            size_bytes: 1,
        }),
        ..Default::default()
    };
    let config = normalize_provider_config(&json!({"api_key":"fixture-key"})).unwrap();
    let scope = websocket_history_scope(&config, &input);
    let mut runtime = OpenAiProviderRuntime::default();
    runtime.record_native_websocket_response_chain(
        "resp_tool",
        &initial,
        Some(&completed_output),
        &scope,
    );
    let rebuilt = runtime
        .websocket_scoped_retry_body(&config, &input, None, "resp_tool", &continuation)
        .expect("completed string input provides scoped full replay history");
    let mut expected = continuation.clone();
    expected
        .as_object_mut()
        .unwrap()
        .remove("previous_response_id");
    expected["input"] = json!([
        {"type":"message","role":"user","content":[{"type":"input_text","text":"  call the tool\n"}]},
        reasoning, tool_call, tool_result,
    ]);
    assert_eq!(
        rebuilt, expected,
        "replay preserves exact raw outputs and item order without synthesized items"
    );
    assert_eq!(initial, wire_before, "native wire body remains a string");
    assert_eq!(
        input.native_transport.as_ref().unwrap().wire_body,
        continuation
    );

    let mut foreign_input = input.clone();
    foreign_input.provider_instance_id = "foreign-provider".into();
    assert!(runtime
        .websocket_scoped_retry_body(&config, &foreign_input, None, "resp_tool", &continuation)
        .is_none());
    runtime.record_native_websocket_response_chain(
        "foreign_child",
        &continuation,
        Some(&[]),
        "foreign-scope",
    );
    assert!(!runtime
        .websocket_chain_inputs_by_response_id
        .contains_key("foreign_child"));
}

#[test]
fn native_string_history_requires_raw_completed_output() {
    let mut runtime = OpenAiProviderRuntime::default();
    let initial = json!({"input":"call the tool"});
    runtime.record_native_websocket_response_chain("incomplete", &initial, None, "scope");
    assert!(!runtime
        .websocket_chain_inputs_by_response_id
        .contains_key("incomplete"));
    assert!(!runtime
        .websocket_chain_scopes_by_response_id
        .contains_key("incomplete"));
    assert_eq!(runtime.websocket_native_history_bytes, 0);
    assert!(runtime
        .websocket_full_context_retry_body(
            "incomplete",
            &json!({"previous_response_id":"incomplete","input":[{"type":"function_call_output","call_id":"call_1","output":"fixture-result"}]}),
        )
        .is_none());
    let partial = completed_native_output(&json!({
        "type":"response.incomplete",
        "response":{"id":"partial","status":"incomplete","output":[{"type":"function_call","call_id":"call_1","arguments":"partial"}]},
    }));
    assert!(partial.is_none());
    runtime.record_native_websocket_response_chain(
        "partial",
        &initial,
        partial.as_deref(),
        "scope",
    );
    assert!(!runtime
        .websocket_chain_inputs_by_response_id
        .contains_key("partial"));
}

// #2028 AC-007/008: exercise the real invocation entry and two turns on one socket.
#[tokio::test]
async fn issue_2028_native_tools_roundtrip_on_selected_websocket() {
    for (kind, field, result_kind, raw) in [
        (
            "custom_tool_call",
            "input",
            "custom_tool_call_output",
            "const x = await tools.exec_command({cmd: 'cat fixture'}); text(x);",
        ),
        (
            "function_call",
            "arguments",
            "function_call_output",
            "{\"path\":\"fixture\"}",
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let item = json!({"type": kind, "id": "item_1", "call_id": "call_1", "name": "exec", field: raw, "status": "completed"});
        let sent_item = item.clone();
        let upstream = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut ws = tokio_tungstenite::tungstenite::accept(stream)
                .expect("native must select WS, not HTTP");
            let first: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(first["type"], "response.create");
            assert_eq!(first["input"][0]["type"], "additional_tools");
            for event in [
                json!({"type":"response.created","response":{"id":"resp_1"}}),
                json!({"type":"response.output_item.added","output_index":0,"item":sent_item}),
                json!({"type":"response.output_item.done","output_index":0,"item":sent_item}),
                json!({"type":"response.completed","response":{"id":"resp_1","status":"completed","output":[sent_item]}}),
            ] {
                ws.send(Message::Text(event.to_string().into())).unwrap();
            }
            let next = loop {
                let value: Value =
                    serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
                if value["type"] == "response.create" {
                    break value;
                }
            };
            assert_eq!(next["previous_response_id"], "resp_1");
            assert_eq!(
                next["input"],
                json!([{"type":result_kind,"call_id":"call_1","output":"random-fixture-result"}])
            );
            ws.send(Message::Text(json!({"type":"response.completed","response":{"id":"resp_2","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"random-fixture-result"}]}]}}).to_string().into())).unwrap();
        });
        let make_input = |body| {
            ProviderInvocationInput {
            contract_version: ProviderInvocationContractVersion::Current,
            provider_instance_id: "fixture".into(),
            provider_code: "openai".into(),
            protocol: "openai_responses".into(),
            model: "fixture-model".into(),
            provider_config: json!({"base_url":base_url,"api_key":"fixture-key","transport_mode":"responses_websocket"}),
            required_capabilities: BTreeSet::from([
                ProviderInvocationCapability::ResponsesNativePassthrough,
                ProviderInvocationCapability::ResponsesNativeOutputV1,
            ]),
            client_protocol_envelope: Some(ProtocolContextEnvelope { source_protocol: "openai_responses".into(), headers: [("session-id".into(), vec!["fixture-session".into()])].into(), ..Default::default() }),
            run_context: [(TRANSPORT_SESSION_CONTEXT_KEY.into(), json!({"logical_session_id":"logical-fixture","generation":41,"task_id":"task-fixture","state":"active","physical_deadline_unix_ms":4102444800000_i64}))].into(),
        native_transport: Some(ProviderNativeTransport {
                protocol: "openai_responses".into(),
                wire_body: body,
                digest: "fixture".into(),
                size_bytes: 1,
            }),
            ..Default::default()
        }
        };
        let mut runtime = OpenAiProviderRuntime::default();
        let mut events = Vec::new();
        let first_input = make_input(json!({"input":[{"type":"additional_tools","tools":[]}]}));
        let first = protocol_observation::capture(
            "openai.responses".into(),
            |event| {
                events.push(event.clone());
                Ok(())
            },
            |sink| runtime.invoke_response_with_event_sink(first_input, sink),
        )
        .await
        .expect("native tool turn");
        assert!(events.iter().any(|event| matches!(event,
            ProviderStreamEvent::ProtocolObservation { transport, direction, kind, body, .. }
                if transport == "websocket" && direction == "prepared" && kind == "request_prepared"
                    && serde_json::from_str::<Value>(body).unwrap()["type"] == "response.create"
        )));
        assert!(events.iter().any(|event| matches!(event,
            ProviderStreamEvent::ProtocolObservation { transport, direction, kind, body, .. }
                if transport == "websocket" && direction == "received" && kind == "message"
                    && serde_json::from_str::<Value>(body).unwrap()["type"] == "response.completed"
        )));
        assert!(!serde_json::to_string(&events)
            .unwrap()
            .contains("fixture-key"));
        assert_eq!(first.result.response_id.as_deref(), Some("resp_1"));
        let items: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ProviderStreamEvent::OutputItem {
                    phase: ProviderOutputItemPhase::Done,
                    item,
                    ..
                } => Some(item),
                _ => None,
            })
            .collect();
        assert_eq!(
            items,
            vec![&item],
            "canonical output must preserve type, raw input, call id exactly once"
        );
        let continuation = make_input(
            json!({"previous_response_id":"resp_1","input":[{"type":result_kind,"call_id":"call_1","output":"random-fixture-result"}]}),
        );
        let mut changed_transport = continuation.clone();
        changed_transport.provider_config["transport_mode"] = json!("http_sse");
        assert!(runtime
            .invoke_response(changed_transport)
            .await
            .unwrap_err()
            .to_string()
            .contains("cannot switch to HTTP"));
        let second = runtime
            .invoke_response(continuation)
            .await
            .expect("native result continuation");
        assert_eq!(
            second.result.final_content.as_deref(),
            Some("random-fixture-result")
        );
        upstream.join().unwrap();
    }
}

// AC-008: absent or foreign cursor ownership fails before network I/O.
#[tokio::test]
async fn issue_2028_native_cursor_rejects_foreign_session_and_unknown_owner() {
    let mut runtime = OpenAiProviderRuntime::default();
    let input = ProviderInvocationInput {
        contract_version: ProviderInvocationContractVersion::Current,
        provider_instance_id: "other-provider".into(),
        provider_code: "openai".into(),
        protocol: "openai_responses".into(),
        model: "fixture-model".into(),
        provider_config: json!({"base_url":"http://127.0.0.1:1","api_key":"fixture","transport_mode":"auto"}),
        required_capabilities: BTreeSet::from([
            ProviderInvocationCapability::ResponsesNativePassthrough,
                ProviderInvocationCapability::ResponsesNativeOutputV1,
        ]),
        client_protocol_envelope: Some(ProtocolContextEnvelope { source_protocol: "openai_responses".into(), headers: [("session-id".into(), vec!["fixture-session".into()])].into(), ..Default::default() }),
        run_context: [(TRANSPORT_SESSION_CONTEXT_KEY.into(), json!({"logical_session_id":"logical-fixture","generation":41,"task_id":"task-fixture","state":"active","physical_deadline_unix_ms":4102444800000_i64}))].into(),
        native_transport: Some(ProviderNativeTransport {
            protocol: "openai_responses".into(),
            wire_body: json!({"previous_response_id":"resp_foreign","input":[]}),
            digest: "fixture".into(),
            size_bytes: 1,
        }),
        ..Default::default()
    };
    for owner in [None, Some("another-session")] {
        if let Some(session_key) = owner {
            runtime.websocket_response_owners.insert(
                "resp_foreign".into(),
                WebsocketResponseOwner {
                    session_key: session_key.into(),
                    generation: 41,
                },
            );
        }
        let error = runtime.invoke_response(input.clone()).await.unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<ProviderRuntimeError>()
                .map(|error| &error.kind),
            Some(&ProviderRuntimeErrorKind::ProviderTransportUnavailable)
        );
    }
}

// AC-010: an old host must fail before even trying the deliberately invalid URL.
#[tokio::test]
async fn native_output_contract_rejects_old_host_before_network() {
    let mut input = ProviderInvocationInput::default();
    input.model = "fixture".into();
    input.provider_config = json!({"base_url":"http://127.0.0.1:1","api_key":"fixture"});
    input
        .required_capabilities
        .insert(ProviderInvocationCapability::ResponsesNativePassthrough);
    input.native_transport = Some(ProviderNativeTransport {
        protocol: "openai_responses".into(),
        wire_body: json!({"input":[]}),
        digest: "fixture".into(),
        size_bytes: 1,
    });
    let error = OpenAiProviderRuntime::default()
        .invoke_response(input)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("responses.native_output.v1"),
        "{error}"
    );
}

// AC-012: use the provider parser, including the same formal event serialization
// that the stdio boundary consumes; diagnostic events cannot satisfy this oracle.
#[test]
fn native_output_inventory_preserves_phase_opaque_and_delta_identity() {
    let items = [
        json!({"id":"rs_1","type":"reasoning","summary":[{"type":"summary_text","text":"thinking"}],"encrypted_content":"opaque-fixture"}),
        json!({"id":"msg_1","type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"reading"}]}),
        json!({"id":"msg_2","type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"done"}]}),
    ];
    let mut events = Vec::new();
    let mut text = String::new();
    let mut calls = ResponseToolCalls::default();
    let mut usage = ProviderUsage::default();
    let mut finish = ProviderFinishReason::Stop;
    let mut response = Value::Null;
    for (index, item) in items.iter().enumerate() {
        for kind in ["response.output_item.added", "response.output_item.done"] {
            process_response_sse_payload(
                &json!({"type":kind,"output_index":index,"item":item}).to_string(),
                &mut events,
                &mut text,
                &mut calls,
                &mut usage,
                &mut finish,
                &mut response,
            )
            .unwrap();
        }
    }
    let done: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ProviderStreamEvent::OutputItem {
                phase: ProviderOutputItemPhase::Done,
                item,
                ..
            } => Some(item.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(done, items);
    let delta = json!({"type":"response.custom_tool_call_input.delta","item_id":"tool_1","call_id":"call_1","output_index":3,"delta":"text(await tools.exec_command({cmd:'cat fixture'}));"});
    process_response_sse_payload(
        &delta.to_string(),
        &mut events,
        &mut text,
        &mut calls,
        &mut usage,
        &mut finish,
        &mut response,
    )
    .unwrap();
    assert!(events.iter().any(|e| serde_json::to_value(e).unwrap()
        == json!({"type":"responses_output_delta","event":delta})));
}

// AC-014/015: two upstream sockets remain alive at an explicit channel barrier;
// cursor ownership is generation-bound; a missing physical owner must fail before I/O.
#[tokio::test]
async fn native_sessions_are_isolated_and_owner_does_not_cross_generation() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let mut sockets = Vec::new();
        for nonce in ["a", "b"] {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut ws = tokio_tungstenite::tungstenite::accept(stream).unwrap();
            let frame: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(frame["input"], json!([{"role":"user","content":nonce}]));
            ws.send(Message::Text(json!({"type":"response.completed","response":{"id":format!("resp_{nonce}"),"output":[]}}).to_string().into())).unwrap();
            sockets.push(ws);
        }
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        listener.set_nonblocking(true).unwrap();
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "lost owner without routing evidence must not open a third connection"
        );
    });
    let make = |nonce: &str, body: Value| {
        ProviderInvocationInput {
        provider_instance_id:"fixture".into(),model:"fixture".into(),protocol:"openai_responses".into(),
        provider_config:json!({"base_url":base,"api_key":"fixture","transport_mode":"responses_websocket"}),
        required_capabilities:[ProviderInvocationCapability::ResponsesNativePassthrough,ProviderInvocationCapability::ResponsesNativeOutputV1].into(),
        client_protocol_envelope:Some(ProtocolContextEnvelope{source_protocol:"openai_responses".into(),headers:[("session-id".into(),vec![nonce.into()])].into(),..Default::default()}),
        run_context:[(TRANSPORT_SESSION_CONTEXT_KEY.into(),json!({"logical_session_id":format!("logical-{nonce}"),"generation":if nonce == "a" { 101 } else { 202 },"task_id":format!("task-{nonce}"),"state":"active","physical_deadline_unix_ms":4102444800000_i64}))].into(),
        native_transport:Some(ProviderNativeTransport{protocol:"openai_responses".into(),wire_body:body,digest:"fixture".into(),size_bytes:1}),..Default::default()
    }
    };
    let mut runtime = OpenAiProviderRuntime::default();
    for nonce in ["a", "b"] {
        let expected_generation = if nonce == "a" { 101 } else { 202 };
        let output = runtime
            .invoke_response(make(nonce, json!({"input":nonce})))
            .await
            .unwrap();
        assert_eq!(output.result.response_id, Some(format!("resp_{nonce}")));
        assert_eq!(
            output.result.provider_metadata[TRANSPORT_SESSION_RECEIPT_METADATA_KEY]["generation"],
            expected_generation
        );
    }
    assert_eq!(runtime.websocket_sessions.len(), 2);
    let continuation = json!({"previous_response_id":"resp_a","input":"a-result"});
    for error in [
        runtime
            .invoke_response(make("b", continuation.clone()))
            .await
            .unwrap_err(),
        OpenAiProviderRuntime::default()
            .invoke_response(make("a", continuation.clone()))
            .await
            .unwrap_err(),
    ] {
        assert_eq!(
            error
                .downcast_ref::<ProviderRuntimeError>()
                .map(|error| &error.kind),
            Some(&ProviderRuntimeErrorKind::ProviderTransportUnavailable)
        );
    }
    let disconnected = make("a", Value::Null);
    let config = normalize_provider_config(&disconnected.provider_config).unwrap();
    let directive = transport_session_directive(&disconnected).unwrap();
    runtime.websocket_sessions.remove(&websocket_session_key(
        &config,
        &disconnected,
        directive.as_ref(),
    ));
    let error = runtime
        .invoke_response(make("a", continuation))
        .await
        .unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<ProviderRuntimeError>()
            .map(|error| &error.kind),
        Some(&ProviderRuntimeErrorKind::ProviderTransportUnavailable)
    );
    let diagnostic = &error
        .downcast_ref::<ProviderRuntimeError>()
        .unwrap()
        .provider_details
        .as_ref()
        .unwrap()[recovery_diagnostics::KEY]["last_failure"];
    assert_eq!(diagnostic["kind"], "owner_rejected");
    assert_eq!(diagnostic["owner_socket_incarnation"], 1);
    assert!(diagnostic["socket_incarnation"].is_null());
    release_tx.send(()).unwrap();
    server.join().unwrap();
}

// #2204: exercise the real WS invocation and recovery receipt, not a copied parser.
#[tokio::test]
async fn native_tool_interruption_never_commits_partial_calls() {
    for (kind, field, delta_type) in [
        (
            "custom_tool_call",
            "input",
            "response.custom_tool_call_input.delta",
        ),
        (
            "function_call",
            "arguments",
            "response.function_call_arguments.delta",
        ),
    ] {
        for clean_close in [true, false] {
            for item_done in [false, true] {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let base_url = format!("http://{}", listener.local_addr().unwrap());
                listener.set_nonblocking(true).unwrap();
                let upstream = std::thread::spawn(move || {
                    let started = std::time::Instant::now();
                    let stream = loop {
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(error)
                                if error.kind() == std::io::ErrorKind::WouldBlock
                                    && started.elapsed() < Duration::from_secs(5) =>
                            {
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            Err(error) => panic!("fixture connection not established: {error}"),
                        }
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut ws = tokio_tungstenite::tungstenite::accept(stream).unwrap();
                    let request: Value =
                        serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
                    assert_eq!(request["type"], "response.create");
                    for event in [
                        json!({"type":"response.created","response":{"id":"resp_interrupted"}}),
                        json!({"type":"response.output_item.added","output_index":0,"item":{"id":"item_1","type":kind,"call_id":"call_1","name":"exec",field:"","status":"in_progress"}}),
                        json!({"type":delta_type,"output_index":0,"item_id":"item_1","call_id":"call_1","delta":"partial-input"}),
                    ] {
                        ws.send(Message::Text(event.to_string().into())).unwrap();
                    }
                    if item_done {
                        ws.send(Message::Text(json!({"type":"response.output_item.done","output_index":0,"item":{"id":"item_1","type":kind,"call_id":"call_1","name":"exec",field:"partial-input","status":"completed"}}).to_string().into())).unwrap();
                    }
                    if clean_close {
                        ws.close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                            code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Error,
                            reason: "upstream websocket proxy failed".into(),
                        })).unwrap();
                    }
                    // Otherwise the peer disappears without a WS closing handshake.
                });
                let input = ProviderInvocationInput {
                    contract_version: ProviderInvocationContractVersion::Current,
                    provider_instance_id: "fixture".into(),
                    provider_code: "openai".into(),
                    protocol: "openai_responses".into(),
                    model: "fixture".into(),
                    provider_config: json!({"base_url":base_url,"api_key":"fixture","transport_mode":"responses_websocket"}),
                    required_capabilities: BTreeSet::from([
                        ProviderInvocationCapability::ResponsesNativePassthrough,
                        ProviderInvocationCapability::ResponsesNativeOutputV1,
                    ]),
                    client_protocol_envelope: Some(ProtocolContextEnvelope {
                        source_protocol: "openai_responses".into(),
                        headers: [("session-id".into(), vec!["interruption-fixture".into()])].into(),
                        ..Default::default()
                    }),
                    run_context: [
                        (TRANSPORT_SESSION_CONTEXT_KEY.into(), json!({"logical_session_id":"interruption-fixture","generation":9,"task_id":"interruption-fixture","state":"active","physical_deadline_unix_ms":4102444800000_i64})),
                        ("provider_recovery".into(), json!({"policy":{"type":"native_opaque","budget":{"max_inner_attempts":1,"absolute_deadline_unix_ms":4102444800000_i64}},"transport_epoch":9,"initial_commit_level":"lifecycle_only"}))].into(),
                    native_transport: Some(ProviderNativeTransport {
                        protocol: "openai_responses".into(),
                        wire_body: json!({"input":[{"role":"user","content":"fixture"}]}),
                        digest: "fixture".into(),
                        size_bytes: 1,
                    }),
                    ..Default::default()
                };
                let mut events = Vec::new();
                let error = OpenAiProviderRuntime::default()
                    .invoke_response_with_event_sink(input, &mut |event: &ProviderStreamEvent| {
                        events.push(event.clone());
                        Ok(())
                    })
                    .await
                    .expect_err("tool scaffolding or item done cannot replace response.completed");
                upstream.join().unwrap();
                let error = error.downcast_ref::<ProviderRuntimeError>().unwrap();
                assert_eq!(
                    error.kind,
                    ProviderRuntimeErrorKind::ProviderTransportUnavailable
                );
                let receipt = &error.provider_details.as_ref().unwrap()
                    [recovery::RECOVERY_RECEIPT_METADATA_KEY];
                assert_eq!(receipt["disposition"], "terminal_interruption");
                assert_eq!(receipt["commit_level"], "terminal");
                assert_eq!(receipt["attempt"], 0);
                assert!(!events.iter().any(|event| matches!(
                    event,
                    ProviderStreamEvent::ToolCallCommit { .. } | ProviderStreamEvent::Finish { .. }
                )));
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            event,
                            ProviderStreamEvent::OutputItem {
                                phase: ProviderOutputItemPhase::Done,
                                ..
                            }
                        ))
                        .count(),
                    usize::from(item_done),
                    "only upstream item done may be emitted before failure"
                );
            }
        }
    }
}

#[test]
fn completed_payload_supplies_missing_tool_done_once_without_promoting_incomplete() {
    for (kind, field) in [
        ("custom_tool_call", "input"),
        ("function_call", "arguments"),
    ] {
        for prior_done in [false, true] {
            for (terminal_type, status) in [
                ("response.completed", "completed"),
                ("response.done", "completed"),
                ("response.incomplete", "incomplete"),
                ("response.completed", "incomplete"),
                ("response.failed", "failed"),
            ] {
                let item = json!({"id":"item_1","type":kind,"call_id":"call_1","name":"exec",field:"complete-input","status":"completed"});
                let mut events = Vec::new();
                let mut calls = ResponseToolCalls::default();
                let mut text = String::new();
                let mut usage = ProviderUsage::default();
                let mut finish = ProviderFinishReason::Unknown;
                let mut response_id = Value::Null;
                if prior_done {
                    process_response_sse_payload(
                        &json!({"type":"response.output_item.done","output_index":0,"item":item})
                            .to_string(),
                        &mut events,
                        &mut text,
                        &mut calls,
                        &mut usage,
                        &mut finish,
                        &mut response_id,
                    )
                    .unwrap();
                }
                let terminal = json!({"type":terminal_type,"response":{"id":"resp_complete","status":status,"output":[item],"incomplete_details":{"reason":"max_output_tokens"},"error":{"code":"server_error","message":"fixture"}}});
                let result = process_response_sse_payload(
                    &terminal.to_string(),
                    &mut events,
                    &mut text,
                    &mut calls,
                    &mut usage,
                    &mut finish,
                    &mut response_id,
                );
                assert_eq!(result.is_err(), status == "failed");
                let done: Vec<_> = events
                    .iter()
                    .filter_map(|event| match event {
                        ProviderStreamEvent::OutputItem {
                            phase: ProviderOutputItemPhase::Done,
                            output_index,
                            item,
                        } => Some((*output_index, item.clone())),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    done,
                    if prior_done || status == "completed" {
                        vec![(0, item.clone())]
                    } else {
                        vec![]
                    }
                );
                if status == "completed" {
                    assert_eq!(
                        calls[0].arguments,
                        if kind == "custom_tool_call" {
                            json!({"input":"complete-input"})
                        } else {
                            json!({})
                        }
                    );
                    assert_eq!(finish, ProviderFinishReason::ToolCall);
                }
            }
        }
    }
}

// Gate scripted reads on the real response.create send, so EOF is deterministic
// rather than depending on tungstenite's TCP reset/closing-handshake mapping.
struct RequestGatedFrames {
    frames: std::collections::VecDeque<Result<Message, tokio_tungstenite::tungstenite::Error>>,
    sent: bool,
    reader: Option<std::task::Waker>,
}
impl futures_util::Stream for RequestGatedFrames {
    type Item = Result<Message, tokio_tungstenite::tungstenite::Error>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.sent {
            std::task::Poll::Ready(self.frames.pop_front())
        } else {
            self.reader = Some(cx.waker().clone());
            std::task::Poll::Pending
        }
    }
}
impl futures_util::Sink<Message> for RequestGatedFrames {
    type Error = tokio_tungstenite::tungstenite::Error;
    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn start_send(mut self: std::pin::Pin<&mut Self>, message: Message) -> Result<(), Self::Error> {
        assert_eq!(
            serde_json::from_str::<Value>(message.to_text().unwrap()).unwrap()["type"],
            "response.create"
        );
        self.sent = true;
        if let Some(reader) = self.reader.take() {
            reader.wake();
        }
        Ok(())
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn native_tool_eof_requires_completion_and_completion_precedes_late_close() {
    for (kind, field) in [
        ("custom_tool_call", "input"),
        ("function_call", "arguments"),
    ] {
        for completed in [false, true] {
            for prior_done in [false, true] {
                for late_close in [false, true] {
                    let item = json!({"id":"item_1","type":kind,"call_id":"call_1","name":"exec",field:"{\"path\":\"fixture\"}","status":"completed"});
                    let mut added = item.clone();
                    added[field] = json!("");
                    added["status"] = json!("in_progress");
                    let mut frames = std::collections::VecDeque::from([
                        Ok(Message::Text(json!({"type":"response.created","response":{"id":"resp_ordered"}}).to_string().into())),
                        Ok(Message::Text(json!({"type":"response.output_item.added","output_index":0,"item":added}).to_string().into())),
                    ]);
                    if prior_done {
                        frames.push_back(Ok(Message::Text(json!({"type":"response.output_item.done","output_index":0,"item":item}).to_string().into())));
                    }
                    if completed {
                        frames.push_back(Ok(Message::Text(json!({"type":"response.completed","response":{"id":"resp_ordered","status":"completed","output":[item]}}).to_string().into())));
                    }
                    if late_close {
                        frames.push_back(Ok(Message::Close(None)));
                    }
                    let now = Instant::now();
                    let mut session = ResponsesWebsocketSession {
                        stream: websocket_io::SocketOwner::new(RequestGatedFrames {
                            frames,
                            sent: false,
                            reader: None,
                        }),
                        turn_state: None,
                        socket_generation: 1,
                        contract_generation: None,
                        close_identity: None,
                        created_at: now,
                        last_activity: now,
                        state: WebsocketConnectionState::Ready,
                    };
                    let mut events = Vec::new();
                    let result = read_websocket_response(
                        &mut session,
                        &mut json!({"type":"response.create","input":[]}),
                        &ProviderInvocationInput::default(),
                        &mut |event| {
                            events.push(event.clone());
                            Ok(())
                        },
                    )
                    .await;
                    if completed {
                        let output = result
                            .expect("completed tool response survives late transport close/EOF");
                        assert_eq!(
                            output.envelope.result.finish_reason,
                            Some(ProviderFinishReason::ToolCall)
                        );
                        assert_eq!(
                            output.envelope.result.tool_calls[0].arguments,
                            json!({"path":"fixture"})
                        );
                        assert_eq!(
                            events
                                .iter()
                                .filter_map(|event| match event {
                                    ProviderStreamEvent::OutputItem {
                                        phase: ProviderOutputItemPhase::Done,
                                        item,
                                        ..
                                    } => Some(item.clone()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>(),
                            vec![item]
                        );
                        assert_eq!(
                            events
                                .iter()
                                .filter(|event| matches!(
                                    event,
                                    ProviderStreamEvent::ToolCallCommit { .. }
                                ))
                                .count(),
                            1
                        );
                    } else {
                        let error =
                            result.expect_err("tool item cannot complete a response at EOF/Close");
                        assert!(error.semantic_committed);
                        assert!(!error.fallback_allowed);
                        assert!(!events.iter().any(|event| matches!(
                            event,
                            ProviderStreamEvent::ToolCallCommit { .. }
                                | ProviderStreamEvent::Finish { .. }
                        )));
                    }
                }
            }
        }
    }
}

#[test]
fn completed_response_does_not_fabricate_done_for_unfinished_or_missing_tool_fields() {
    for item in [
        json!({"type":"custom_tool_call","call_id":"call_1","name":"exec","input":"partial","status":"in_progress"}),
        json!({"type":"custom_tool_call","call_id":"call_1","name":"exec","status":"completed"}),
        json!({"type":"function_call","call_id":"call_1","arguments":"{}","status":"completed"}),
    ] {
        let mut events = Vec::new();
        process_response_sse_payload(
            &json!({"type":"response.completed","response":{"id":"resp_1","status":"completed","output":[item]}}).to_string(),
            &mut events,
            &mut String::new(),
            &mut ResponseToolCalls::default(),
            &mut ProviderUsage::default(),
            &mut ProviderFinishReason::Unknown,
            &mut Value::Null,
        ).unwrap();
        assert!(!events.iter().any(|event| matches!(
            event,
            ProviderStreamEvent::OutputItem {
                phase: ProviderOutputItemPhase::Done,
                ..
            }
        )));
    }
}
