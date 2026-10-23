use super::*;
use tokio::{sync::oneshot, task::JoinHandle};

const FIXTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The server cannot finish its chunked body until this test explicitly asks it
/// to. Successful reads must instead drop the response and close the peer socket.
struct HeldBody {
    response: reqwest::Response,
    release_eof: oneshot::Sender<()>,
    peer_closed: oneshot::Receiver<()>,
    server: JoinHandle<()>,
}

async fn write_events(socket: &mut TcpStream, events: &[Value]) {
    for event in events {
        let data = format!(
            "event: {}\ndata: {event}\n\n",
            event["type"].as_str().unwrap()
        );
        socket
            .write_all(format!("{:x}\r\n{data}\r\n", data.len()).as_bytes())
            .await
            .unwrap();
    }
    socket.flush().await.unwrap();
}

async fn held_body(
    prefix: Vec<Value>,
    continuation: Option<(oneshot::Receiver<()>, Vec<Value>)>,
) -> HeldBody {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (release_eof, eof) = oneshot::channel();
    let (closed, peer_closed) = oneshot::channel();
    let server = tokio::spawn(async move {
        // Cleanup has a longer deadline than the reader assertion, so a broken
        // reader cannot pass merely because the fixture timed out and sent EOF.
        tokio::time::timeout(FIXTURE_TIMEOUT * 4, async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                assert_eq!(socket.read(&mut byte).await.unwrap(), 1);
                request.push(byte[0]);
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\nx-request-id: fixture-request\r\n\r\n")
                .await
                .unwrap();
            write_events(&mut socket, &prefix).await;
            if let Some((release, events)) = continuation {
                // A separate barrier proves item.done is not the response terminal.
                if release.await.is_err() {
                    return;
                }
                write_events(&mut socket, &events).await;
            }
            tokio::select! {
                result = socket.read(&mut byte) => {
                    assert_eq!(result.unwrap(), 0, "client must close the unconsumed HTTP body");
                    let _ = closed.send(());
                }
                released = eof => {
                    // Sender cancellation cleans up a failed test without holding a task.
                    if released.is_ok() {
                        socket.write_all(b"0\r\n\r\n").await.unwrap();
                        socket.shutdown().await.unwrap();
                    }
                }
            }
        })
        .await
        .expect("loopback SSE fixture must not outlive its bounded test window");
    });
    let response = tokio::time::timeout(
        FIXTURE_TIMEOUT,
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/v1/responses"))
            .send(),
    )
    .await
    .unwrap()
    .unwrap();
    HeldBody {
        response,
        release_eof,
        peer_closed,
        server,
    }
}

async fn read_before_eof(
    payloads: Vec<Value>,
    native: bool,
) -> (Result<RuntimeInvocationEnvelope>, Vec<ProviderStreamEvent>) {
    let HeldBody {
        response,
        release_eof,
        peer_closed,
        server,
    } = held_body(payloads, None).await;
    let mut emitted = Vec::new();
    let result = tokio::time::timeout(
        FIXTURE_TIMEOUT,
        read_streaming_response(
            response,
            "fixture-model".into(),
            &mut |event: &ProviderStreamEvent| {
                emitted.push(event.clone());
                Ok(())
            },
            native,
        ),
    )
    .await
    .expect("semantic terminal must settle without releasing HTTP EOF");
    tokio::time::timeout(FIXTURE_TIMEOUT, peer_closed)
        .await
        .expect("terminal must release the upstream socket before HTTP EOF")
        .expect("server must observe client EOF, not its own release barrier");
    // Never send the EOF release on this path. The receiver only disappears
    // after the server has independently observed the client's socket close.
    assert!(release_eof.is_closed());
    server.await.unwrap();
    (result, emitted)
}

fn usage() -> Value {
    json!({
        "input_tokens":17, "output_tokens":8, "total_tokens":25,
        "input_tokens_details":{"cached_tokens":4,"cache_write_tokens":2,"future":[null,3]},
        "output_tokens_details":{"reasoning_tokens":3},
        "opaque_usage":{"kept":true}
    })
}

fn expected_usage() -> ProviderUsage {
    ProviderUsage {
        input_tokens: Some(17),
        output_tokens: Some(8),
        total_tokens: Some(25),
        reasoning_tokens: Some(3),
        cache_read_tokens: Some(4),
        cache_write_tokens: Some(2),
        ..Default::default()
    }
}

fn terminal(kind: &str, status: &str, output: Value) -> Value {
    json!({
        "type":kind,
        "sequence_number":7,
        "opaque_event":{"nested":[1,null,"中文🙂"]},
        "response":{
            "id":"resp_terminal", "status":status, "output":output,
            "usage":usage(), "opaque_response":{"ciphertext":"do-not-normalize"}
        }
    })
}

fn assert_native_events(emitted: &[ProviderStreamEvent], expected: &[Value]) {
    let native: Vec<_> = emitted
        .iter()
        .filter_map(|event| match event {
            ProviderStreamEvent::NativeEvent { protocol, event } => {
                assert_eq!(protocol, "openai_responses");
                Some(event.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        native, expected,
        "native JSON, including extensions, must remain exact"
    );
}

fn assert_finished_once(emitted: &[ProviderStreamEvent], reason: ProviderFinishReason) {
    let finishes: Vec<_> = emitted
        .iter()
        .filter_map(|event| match event {
            ProviderStreamEvent::Finish { reason } => Some(reason.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(finishes, vec![reason]);
    let snapshots: Vec<_> = emitted
        .iter()
        .filter_map(|event| match event {
            ProviderStreamEvent::UsageSnapshot { usage } => Some(usage.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(snapshots, vec![expected_usage()]);
}

#[tokio::test]
async fn completed_and_done_release_held_http_body_with_terminal_only_output() {
    for kind in ["response.completed", "response.done"] {
        for native in [false, true] {
            let output = json!([
                {"id":"reasoning_1","type":"reasoning","encrypted_content":"opaque+bytes=","summary":[]},
                {"id":"message_1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"完整🙂","annotations":[{"type":"future_annotation","extra":true}]}]},
                {"id":"function_1","type":"function_call","call_id":"call_function","name":"lookup","arguments":"{\"key\":\"value\"}","status":"completed","extra":{"kept":true}},
                {"id":"custom_1","type":"custom_tool_call","call_id":"call_custom","name":"exec","input":"printf '🙂'","status":"completed","extra":[1,null]}
            ]);
            let event = terminal(kind, "completed", output.clone());
            let (result, emitted) = read_before_eof(vec![event.clone()], native).await;
            let envelope = result.unwrap();
            assert_eq!(
                envelope.result.response_id.as_deref(),
                Some("resp_terminal")
            );
            assert_eq!(envelope.result.final_content.as_deref(), Some("完整🙂"));
            assert_eq!(envelope.result.usage, expected_usage());
            assert_eq!(
                envelope.result.finish_reason,
                Some(ProviderFinishReason::ToolCall)
            );
            assert_eq!(
                envelope.result.provider_metadata["upstream_request_id"],
                "fixture-request"
            );
            assert_eq!(
                envelope.result.tool_calls,
                vec![
                    ProviderToolCall {
                        id: "call_function".into(),
                        name: "lookup".into(),
                        arguments: json!({"key":"value"})
                    },
                    ProviderToolCall {
                        id: "call_custom".into(),
                        name: "exec".into(),
                        arguments: json!({"input":"printf '🙂'"})
                    },
                ]
            );
            let done: Vec<_> = emitted
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
            assert_eq!(done, vec![(2, output[2].clone()), (3, output[3].clone())]);
            let commits: Vec<_> = emitted
                .iter()
                .filter_map(|event| match event {
                    ProviderStreamEvent::ToolCallCommit { call } => Some(call.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(commits, envelope.result.tool_calls);
            assert_finished_once(&emitted, ProviderFinishReason::ToolCall);
            assert_native_events(
                &emitted,
                if native {
                    std::slice::from_ref(&event)
                } else {
                    &[]
                },
            );
        }
    }
}

#[tokio::test]
async fn supported_incomplete_releases_held_body_without_synthesizing_tool_item_done() {
    for kind in ["response.incomplete", "response.completed", "response.done"] {
        for (reason, finish) in [
            ("max_output_tokens", ProviderFinishReason::Length),
            ("content_filter", ProviderFinishReason::ContentFilter),
        ] {
            let mut event = terminal(
                kind,
                "incomplete",
                json!([
                    {"type":"message","content":[{"type":"output_text","text":"partial"}]},
                    {"id":"unfinished","type":"function_call","status":"in_progress","arguments":""}
                ]),
            );
            event["response"]["incomplete_details"] = json!({"reason":reason,"opaque":"preserved"});
            let (result, emitted) = read_before_eof(vec![event.clone()], true).await;
            let envelope = result.unwrap();
            assert_eq!(envelope.result.final_content.as_deref(), Some("partial"));
            assert_eq!(
                envelope.result.response_id.as_deref(),
                Some("resp_terminal")
            );
            assert_eq!(envelope.result.finish_reason, Some(finish.clone()));
            assert_eq!(envelope.result.usage, expected_usage());
            assert!(
                !emitted.iter().any(|event| matches!(
                    event,
                    ProviderStreamEvent::OutputItem {
                        phase: ProviderOutputItemPhase::Done,
                        ..
                    }
                )),
                "an incomplete terminal cannot synthesize tool item.done"
            );
            assert_native_events(&emitted, &[event]);
            assert_finished_once(&emitted, finish);
        }
    }
}

#[tokio::test]
async fn output_item_done_keeps_reading_until_the_real_response_terminal() {
    let item = json!({"id":"message_1","type":"message","content":[{"type":"output_text","text":"prefix"}]});
    let prefix = json!({"type":"response.output_item.done","response_id":"resp_terminal","output_index":0,"item":item});
    let completed = terminal("response.completed", "completed", json!([item]));
    let (release_terminal, terminal_ready) = oneshot::channel();
    let HeldBody {
        response,
        release_eof,
        peer_closed,
        server,
    } = held_body(
        vec![prefix.clone()],
        Some((terminal_ready, vec![completed.clone()])),
    )
    .await;
    let (seen, observed) = oneshot::channel();
    let mut seen = Some(seen);
    let mut emitted = Vec::new();
    let mut sink = |event: &ProviderStreamEvent| {
        if matches!(
            event,
            ProviderStreamEvent::OutputItem {
                phase: ProviderOutputItemPhase::Done,
                ..
            }
        ) {
            if let Some(seen) = seen.take() {
                let _ = seen.send(());
            }
        }
        emitted.push(event.clone());
        Ok(())
    };
    let mut reading = Box::pin(read_streaming_response(
        response,
        "fixture-model".into(),
        &mut sink,
        true,
    ));
    tokio::time::timeout(FIXTURE_TIMEOUT, async {
        tokio::select! {
            result = &mut reading => panic!("item.done cannot complete a Responses turn: {result:?}"),
            result = observed => result.unwrap(),
        }
    }).await.unwrap();
    std::future::poll_fn(|cx| {
        use std::future::Future;
        match reading.as_mut().poll(cx) {
            std::task::Poll::Pending => std::task::Poll::Ready(()),
            std::task::Poll::Ready(result) => panic!("reader stopped before terminal: {result:?}"),
        }
    })
    .await;
    release_terminal.send(()).unwrap();
    let envelope = tokio::time::timeout(FIXTURE_TIMEOUT, &mut reading)
        .await
        .expect("response terminal must not wait for EOF")
        .unwrap();
    drop(reading);
    assert_eq!(envelope.result.final_content.as_deref(), Some("prefix"));
    assert_native_events(&emitted, &[prefix, completed]);
    assert_finished_once(&emitted, ProviderFinishReason::Stop);
    tokio::time::timeout(FIXTURE_TIMEOUT, peer_closed)
        .await
        .unwrap()
        .unwrap();
    assert!(release_eof.is_closed());
    server.await.unwrap();
}

#[tokio::test]
async fn failed_and_error_keep_structured_supplier_facts_before_http_eof() {
    let original = json!({
        "code":"gateway_concurrency_limit", "type":"rate_limit_error", "param":null,
        "message":"first line\nsecond line", "limit":2, "future":{"nested":[1,null]}
    });
    for event in [
        json!({"type":"response.failed","response":{"id":"resp_failed","status":"failed","error":original}}),
        json!({"type":"error","status":429,"error":original}),
        json!({"type":"response.done","response":{"id":"resp_failed","status":"failed","error":original}}),
    ] {
        let (result, emitted) = read_before_eof(vec![event], true).await;
        let error = result.unwrap_err();
        let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
        assert_eq!(typed.kind, ProviderRuntimeErrorKind::ProviderUpstreamError);
        assert_eq!(typed.message, "first line\nsecond line");
        assert_eq!(
            typed.provider_summary.as_deref(),
            Some("first line\nsecond line")
        );
        let details = typed.provider_details.as_ref().unwrap();
        assert_eq!(details["upstream_error"], original);
        assert_eq!(details["semantic_terminal"], true);
        assert!(
            details.get("status_code").is_none(),
            "an SSE event is not an HTTP error response"
        );
        assert!(!emitted.iter().any(|event| matches!(
            event,
            ProviderStreamEvent::Finish { .. } | ProviderStreamEvent::ToolCallCommit { .. }
        )));
    }
}

#[tokio::test]
async fn eof_without_valid_response_terminal_remains_a_transport_failure() {
    for prefix in [
        vec![json!({"type":"response.created","response":{"id":"resp_missing"}})],
        vec![
            json!({"type":"response.output_item.done","response_id":"resp_missing","output_index":0,"item":{"type":"message","content":[{"type":"output_text","text":"partial"}]}}),
        ],
        vec![json!({"type":"response.completed"})],
        vec![json!({"type":"response.completed","response":{"status":"completed","output":[]}})],
    ] {
        let HeldBody {
            response,
            release_eof,
            peer_closed: _,
            server,
        } = held_body(prefix, None).await;
        release_eof.send(()).unwrap();
        let mut emitted = Vec::new();
        let error = tokio::time::timeout(
            FIXTURE_TIMEOUT,
            read_streaming_response(
                response,
                "fixture-model".into(),
                &mut |event: &ProviderStreamEvent| {
                    emitted.push(event.clone());
                    Ok(())
                },
                true,
            ),
        )
        .await
        .unwrap()
        .unwrap_err();
        let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
        assert_eq!(
            typed.kind,
            ProviderRuntimeErrorKind::ProviderTransportUnavailable
        );
        assert_eq!(
            typed.provider_details.as_ref().unwrap()["1flowbase_transport_failure"]["kind"],
            "http_eof"
        );
        assert_eq!(
            recovery_diagnostics::transport_category(&error),
            Some("transport_disconnected")
        );
        assert!(!emitted.iter().any(|event| matches!(
            event,
            ProviderStreamEvent::Finish { .. } | ProviderStreamEvent::ToolCallCommit { .. }
        )));
        server.await.unwrap();
    }
}
