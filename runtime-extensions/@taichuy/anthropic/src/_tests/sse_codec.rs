use super::*;
use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::{stream, StreamExt};

#[tokio::test]
async fn codec_preserves_chinese_and_emoji_split_across_tcp_chunks() {
    let wire = "event: content_block_delta\r\ndata: {\"type\":\"content_block_delta\",\r\ndata: \"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"中文🙂\"}}\r\n\r\n";
    let bytes = wire.as_bytes();
    let emoji = wire.find('🙂').unwrap();
    let source = stream::iter(vec![
        Ok::<_, std::io::Error>(bytes[..emoji + 1].to_vec()),
        Ok(bytes[emoji + 1..emoji + 3].to_vec()),
        Ok(bytes[emoji + 3..].to_vec()),
    ]);
    let events = source.eventsource().collect::<Vec<_>>().await;
    assert_eq!(events.len(), 1);
    let event = events[0].as_ref().unwrap();
    assert_eq!(event.event, "content_block_delta");
    assert_eq!(
            event.data,
            "{\"type\":\"content_block_delta\",\n\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"中文🙂\"}}"
        );
}

#[tokio::test]
async fn codec_supports_comments_and_multiline_data() {
    let source = stream::iter(vec![Ok::<_, std::io::Error>(
        b":keepalive\ndata: {\"type\":\"ping\",\ndata: \"extra\":true}\n\n".to_vec(),
    )]);
    let events = source.eventsource().collect::<Vec<_>>().await;
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].as_ref().unwrap().data,
        "{\"type\":\"ping\",\n\"extra\":true}"
    );
}

#[tokio::test]
async fn codec_rejects_invalid_utf8_instead_of_replacing_it() {
    let source = stream::iter(vec![Ok::<_, std::io::Error>(vec![
        b'd', b'a', b't', b'a', b':', b' ', 0xf0, 0x9f,
    ])]);
    let events = source.eventsource().collect::<Vec<_>>().await;
    assert!(events
        .iter()
        .any(|event| matches!(event, Err(EventStreamError::Utf8(_)))));
}

async fn mock_sse(body: String) -> (reqwest::Response, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut headers = [0_u8; 2048];
        socket.read(&mut headers).await.unwrap();
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
        socket.write_all(body.as_bytes()).await.unwrap();
    });
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}"))
        .send()
        .await
        .unwrap();
    (response, server)
}

#[tokio::test]
async fn provider_preserves_sse_event_above_old_sixteen_mebibyte_limit() {
    let text = format!("{}完整🙂", "x".repeat(16 * 1024 * 1024 + 1));
    let wire = format!("event: message_start\ndata: {}\n\nevent: content_block_delta\ndata: {}\n\nevent: message_delta\ndata: {}\n\nevent: message_stop\ndata: {}\n\n", json!({"type":"message_start","message":{"id":"msg_large","usage":{"input_tokens":1}}}), json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}), json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}), json!({"type":"message_stop"}));
    let (response, server) = mock_sse(wire).await;
    let result = read_streaming_message(response, "fixture".into(), 1024, &mut |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(result.result.final_content.as_deref(), Some(text.as_str()));
    server.await.unwrap();
}
