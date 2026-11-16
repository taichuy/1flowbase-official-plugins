use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

/// Keep the HTTP body open until the caller explicitly releases it. A protocol
/// terminal, rather than a transport EOF or a quiet-period guess, must complete
/// the invocation and allow the host to commit its callback.
async fn completed_with_open_http_body(
    blocks: &str,
    stop_reason: &str,
) -> (RuntimeInvocationEnvelope, Vec<ProviderStreamEvent>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (release, held) = mpsc::channel();
    let sse = format!(
        "data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_open\",\"usage\":{{\"input_tokens\":7}}}}}}\n\n\
         {blocks}\
         data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{stop_reason}\"}},\"usage\":{{\"output_tokens\":11}}}}\n\n\
         data: {{\"type\":\"message_stop\"}}\n\n"
    );
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = [0_u8; 4096];
        socket.read(&mut request).unwrap();
        write!(socket, "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n{}\r\n", sse.len(), sse).unwrap();
        socket.flush().unwrap();
        held.recv_timeout(Duration::from_secs(5)).unwrap();
        // A client that has consumed message_stop may already have closed this
        // connection; that is the expected successful transport cleanup.
        let _ = socket.write_all(b"0\r\n\r\n");
    });
    let response = reqwest::get(format!("http://{address}")).await.unwrap();
    let mut events = Vec::new();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        read_streaming_message(response, "fixture-model".into(), 4096, &mut |event| {
            events.push(event.clone());
            Ok(())
        }),
    )
    .await;
    release.send(()).unwrap();
    server.join().unwrap();
    let envelope = result
        .expect("message_stop must finish the invocation while the HTTP body remains open")
        .unwrap();
    assert_eq!(envelope.result.usage.input_tokens, Some(7));
    assert_eq!(envelope.result.usage.output_tokens, Some(11));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, ProviderStreamEvent::Finish { .. }))
            .count(),
        1
    );
    (envelope, events)
}

#[tokio::test]
async fn anthropic_message_stop_finishes_text_without_http_eof() {
    let (envelope, _) = completed_with_open_http_body(
        concat!(
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"done\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        ),
        "end_turn",
    ).await;
    assert_eq!(envelope.result.final_content.as_deref(), Some("done"));
}

#[tokio::test]
async fn anthropic_message_stop_finishes_tool_without_http_eof() {
    let (envelope, _) = completed_with_open_http_body(
        concat!(
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_open\",\"name\":\"Read\",\"input\":{}}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"file_path\\\":\\\"/tmp/probe\\\"}\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        ),
        "tool_use",
    ).await;
    assert_eq!(envelope.result.tool_calls.len(), 1);
    assert_eq!(
        envelope.result.tool_calls[0].arguments,
        json!({"file_path":"/tmp/probe"})
    );
    assert_eq!(
        envelope.result.finish_reason,
        Some(ProviderFinishReason::ToolCall)
    );
}
