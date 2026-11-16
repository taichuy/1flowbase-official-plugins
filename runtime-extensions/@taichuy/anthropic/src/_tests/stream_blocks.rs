use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

fn wire(frames: &[Value]) -> String {
    frames
        .iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect()
}

#[tokio::test]
async fn anthropic_live_blocks_arrive_before_upstream_terminal_barrier() {
    let first = wire(&[
        json!({"type":"message_start","message":{"id":"msg_live","usage":{"input_tokens":2}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"检查🙂"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"opaque"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_live","name":"lookup","input":{}}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{ \"query\": \""}}),
    ]);
    let second = wire(&[
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"中文🙂\" }"}}),
        json!({"type":"content_block_stop","index":1}),
    ]);
    let terminal = wire(&[
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}),
        json!({"type":"message_stop"}),
    ]);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (release, barrier) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = socket.read(&mut chunk).unwrap();
            assert_ne!(count, 0);
            request.extend_from_slice(&chunk[..count]);
        }
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", first.len() + second.len() + terminal.len()).unwrap();
        socket.write_all(first.as_bytes()).unwrap();
        // Progress itself releases each barrier; no elapsed-time guess can complete a block.
        assert_eq!(
            barrier.recv_timeout(Duration::from_secs(5)).unwrap(),
            "first_arguments"
        );
        socket.write_all(second.as_bytes()).unwrap();
        assert_eq!(
            barrier.recv_timeout(Duration::from_secs(5)).unwrap(),
            "tool_done"
        );
        socket.write_all(terminal.as_bytes()).unwrap();
    });
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}"))
        .send()
        .await
        .unwrap();
    let mut observed = Vec::new();
    let mut fragments = Vec::new();
    let envelope = read_streaming_message(response, "fixture".into(), 1024, &mut |event| {
        match event {
            ProviderStreamEvent::ToolCallDelta { call_id, delta } => {
                assert_eq!(call_id, "toolu_live");
                if fragments.is_empty() {
                    let signature = observed.iter().filter_map(|event| match event {
                        ProviderStreamEvent::ReasoningSignatureDelta { signature } => Some(signature.as_str()),
                        _ => None,
                    }).collect::<String>();
                    assert_eq!(signature, "sig-opaque");
                    assert!(observed.iter().any(|event| matches!(event,
                        ProviderStreamEvent::OutputItem { phase: ProviderOutputItemPhase::Done, output_index: 0, item }
                        if item["id"] == "msg_live_block_0" && item["type"] == "reasoning"
                            && item["summary"][0]["text"] == "检查🙂")));
                    assert!(observed.iter().any(|event| matches!(event,
                        ProviderStreamEvent::OutputItem { phase: ProviderOutputItemPhase::Added, output_index: 1, item }
                        if item["id"] == "toolu_live" && item["call_id"] == "toolu_live"
                            && item["name"] == "lookup" && item["arguments"] == "")));
                    release.send("first_arguments").unwrap();
                }
                fragments.push(delta.as_str().unwrap().to_owned());
            }
            ProviderStreamEvent::OutputItem { phase: ProviderOutputItemPhase::Done, output_index: 1, item } => {
                assert_eq!(item["arguments"], "{ \"query\": \"中文🙂\" }");
                assert!(!observed.iter().any(|event| matches!(event,
                    ProviderStreamEvent::ToolCallCommit { .. } | ProviderStreamEvent::Finish { .. })));
                release.send("tool_done").unwrap();
            }
            _ => {}
        }
        observed.push(event.clone());
        Ok(())
    }).await.unwrap();
    server.join().unwrap();
    assert_eq!(fragments, ["{ \"query\": \"", "中文🙂\" }"]);
    assert_eq!(
        envelope.result.tool_calls,
        vec![ProviderToolCall {
            id: "toolu_live".into(),
            name: "lookup".into(),
            arguments: json!({"query":"中文🙂"}),
        }]
    );
    assert_eq!(envelope.result.usage.total_tokens, Some(5));
    let signature_index = observed
        .iter()
        .rposition(|event| matches!(event, ProviderStreamEvent::ReasoningSignatureDelta { .. }))
        .unwrap();
    let done_index = observed
        .iter()
        .position(|event| {
            matches!(
                event,
                ProviderStreamEvent::OutputItem {
                    phase: ProviderOutputItemPhase::Done,
                    output_index: 0,
                    ..
                }
            )
        })
        .unwrap();
    assert!(signature_index < done_index);
}

#[test]
fn anthropic_block_identity_and_stop_are_stable_and_terminal_is_absorbing() {
    let mut events = Vec::new();
    let mut text = String::new();
    let mut tool_builders = BTreeMap::new();
    let mut blocks = BTreeMap::new();
    let mut usage = ProviderUsage::default();
    let mut finish_reason = ProviderFinishReason::Unknown;
    let mut message_id = Value::Null;
    let mut saw_message_stop = false;
    for frame in [
        json!({"type":"message_start","message":{"id":"msg_text"}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"初"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"始"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"late"}}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
        json!({"type":"message_stop"}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":"late"}}),
        json!({"type":"message_start","message":{"id":"late"}}),
    ] {
        process_anthropic_sse_data(
            &frame.to_string(),
            &mut events,
            AnthropicStreamParseState {
                text: &mut text,
                tool_builders: &mut tool_builders,
                blocks: &mut blocks,
                usage: &mut usage,
                finish_reason: &mut finish_reason,
                message_id: &mut message_id,
                saw_message_stop: &mut saw_message_stop,
            },
        )
        .unwrap();
    }
    assert_eq!(text, "初始");
    assert_eq!(message_id, json!("msg_text"));
    let lifecycle = events
        .iter()
        .filter_map(|event| match event {
            ProviderStreamEvent::OutputItem {
                phase,
                output_index,
                item,
            } => Some((phase, output_index, item)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(lifecycle.len(), 2);
    assert_eq!(lifecycle[0].0, &ProviderOutputItemPhase::Added);
    assert_eq!(lifecycle[1].0, &ProviderOutputItemPhase::Done);
    assert_eq!(lifecycle[0].2["id"], lifecycle[1].2["id"]);
    assert_eq!(lifecycle[1].2["content"][0]["text"], "初始");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ProviderStreamEvent::TextDelta { .. }))
            .count(),
        2
    );
}
