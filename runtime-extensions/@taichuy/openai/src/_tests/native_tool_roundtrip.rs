use super::*;
use std::net::TcpListener;

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
        let make_input = |body| ProviderInvocationInput {
            contract_version: ProviderInvocationContractVersion::Current,
            provider_instance_id: "fixture".into(),
            provider_code: "openai".into(),
            protocol: "openai_responses".into(),
            model: "fixture-model".into(),
            provider_config: json!({"base_url":base_url,"api_key":"fixture-key","transport_mode":"responses_websocket"}),
            required_capabilities: BTreeSet::from([
                ProviderInvocationCapability::ResponsesNativePassthrough,
            ]),
            native_transport: Some(ProviderNativeTransport {
                protocol: "openai_responses".into(),
                wire_body: body,
                digest: "fixture".into(),
                size_bytes: 1,
            }),
            ..Default::default()
        };
        let mut runtime = OpenAiProviderRuntime::default();
        let mut events = Vec::new();
        let first = runtime
            .invoke_response_with_event_sink(
                make_input(json!({"input":[{"type":"additional_tools","tools":[]}]})),
                |event| {
                    events.push(event.clone());
                    Ok(())
                },
            )
            .await
            .expect("native tool turn");
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
        ]),
        native_transport: Some(ProviderNativeTransport {
            protocol: "openai_responses".into(),
            wire_body: json!({"previous_response_id":"resp_foreign","input":[]}),
            digest: "fixture".into(),
            size_bytes: 1,
        }),
        ..Default::default()
    };
    for owner in [None, Some("another-session")] {
        if let Some(owner) = owner {
            runtime
                .websocket_response_owners
                .insert("resp_foreign".into(), owner.into());
        }
        let error = runtime.invoke_response(input.clone()).await.unwrap_err();
        assert!(error
            .to_string()
            .contains("unavailable for this provider session"));
    }
}
