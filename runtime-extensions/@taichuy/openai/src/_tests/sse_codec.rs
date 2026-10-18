use super::*;
use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::{stream, StreamExt};

#[tokio::test]
async fn mature_codec_preserves_utf8_across_transport_partitions() {
    let wire = "event: response.output_text.delta\r\ndata: {\"delta\":\"中文🙂\"}\r\n\r\n";
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
    assert_eq!(event.event, "response.output_text.delta");
    assert_eq!(event.data, "{\"delta\":\"中文🙂\"}");
}

#[tokio::test]
async fn mature_codec_supports_comments_and_multiline_data() {
    let source = stream::iter(vec![Ok::<_, std::io::Error>(
        b":keepalive\ndata: {\"a\":\ndata: 1}\n\n".to_vec(),
    )]);
    let events = source.eventsource().collect::<Vec<_>>().await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].as_ref().unwrap().data, "{\"a\":\n1}");
}

#[tokio::test]
async fn mature_codec_rejects_invalid_utf8_instead_of_replacing_it() {
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
    let wire = format!(
        "data: {}\n\ndata: {}\n\n",
        json!({"type":"response.output_text.delta","delta":text}),
        json!({"type":"response.completed","response":{"id":"resp_large","status":"completed","output":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}})
    );
    let (response, server) = mock_sse(wire).await;
    let result = read_streaming_response(response, "fixture".into(), &mut |_| Ok(()), false)
        .await
        .unwrap();
    assert_eq!(result.result.final_content.as_deref(), Some(text.as_str()));
    server.await.unwrap();
}
