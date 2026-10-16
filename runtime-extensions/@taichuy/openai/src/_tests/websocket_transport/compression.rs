use super::*;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tungstenite::tungstenite::{
    handshake::server::{Request, Response},
    protocol::{frame::coding::CloseCode, CloseFrame, WebSocketConfig},
};

#[derive(Clone, Default)]
struct WireTrace {
    read: Arc<Mutex<Vec<u8>>>,
    written: Arc<Mutex<Vec<u8>>>,
}

struct TracedStream {
    inner: TcpStream,
    trace: WireTrace,
}

impl AsyncRead for TracedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buffer.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buffer);
        if matches!(result, Poll::Ready(Ok(()))) {
            self.trace
                .read
                .lock()
                .unwrap()
                .extend_from_slice(&buffer.filled()[before..]);
        }
        result
    }
}

impl AsyncWrite for TracedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, bytes);
        if let Poll::Ready(Ok(size)) = result {
            self.trace
                .written
                .lock()
                .unwrap()
                .extend_from_slice(&bytes[..size]);
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

fn image_request() -> String {
    // Deterministic, nontrivial base64 content, larger than the transport write buffer.
    let mut state = 1_u32;
    let pixels = (0..2 * 1024 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect::<Vec<_>>();
    json!({"type":"response.create","model":"fixture","input":[{
        "role":"user","content":[
            {"type":"input_text","text":"原样保留历史\n\u{0} 🦀"},
            {"type":"input_image","image_url":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(pixels))}
        ]
    }]}).to_string()
}

async fn compression_roundtrip(through_proxy: bool, compressed: bool) {
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = upstream.local_addr().unwrap();
    let request = image_request();
    let response =
        json!({"type":"response.output_text.delta","delta":"中文 🦀\n".repeat(32_768)}).to_string();
    let expected_request = request.clone();
    let expected_response = response.clone();
    let trace = WireTrace::default();
    let server_trace = trace.clone();
    let (ready_tx, ready_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = upstream.accept().await.unwrap();
        let mut server_config = WebSocketConfig::default();
        if compressed {
            server_config.extensions.permessage_deflate = config().extensions.permessage_deflate;
        }
        let mut peer = tokio_tungstenite::accept_hdr_async_with_config(
            TracedStream {
                inner: stream,
                trace: server_trace.clone(),
            },
            |request: &Request, response: Response| {
                assert!(request.headers()["sec-websocket-extensions"]
                    .to_str()
                    .unwrap()
                    .contains("permessage-deflate"));
                Ok(response)
            },
            Some(server_config),
        )
        .await
        .unwrap();
        let headers = String::from_utf8(server_trace.written.lock().unwrap().clone()).unwrap();
        assert_eq!(
            headers
                .to_ascii_lowercase()
                .contains("sec-websocket-extensions:"),
            compressed
        );
        server_trace.read.lock().unwrap().clear();
        server_trace.written.lock().unwrap().clear();
        ready_tx.send(()).unwrap();
        assert_eq!(
            peer.next()
                .await
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap()
                .as_str(),
            expected_request
        );
        peer.send(Message::Text(expected_response.into()))
            .await
            .unwrap();
        peer.close(Some(CloseFrame {
            code: CloseCode::Policy,
            reason: "missing first response.create message".into(),
        }))
        .await
        .unwrap();
        assert!(matches!(peer.next().await, Some(Ok(Message::Close(_)))));
    });

    let mut proxy_task = None;
    let proxy_url = if through_proxy {
        let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", proxy.local_addr().unwrap());
        proxy_task = Some(tokio::spawn(async move {
            let (mut client, _) = proxy.accept().await.unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                headers.push(client.read_u8().await.unwrap());
            }
            assert!(String::from_utf8(headers)
                .unwrap()
                .starts_with(&format!("CONNECT {address} HTTP/1.1")));
            let mut target = TcpStream::connect(address).await.unwrap();
            client
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .unwrap();
            let _ = tokio::io::copy_bidirectional(&mut client, &mut target).await;
        }));
        Some(url)
    } else {
        None
    };
    let provider = ProviderConfig {
        base_url: format!("http://{address}"),
        api_key: "fixture-credential".into(),
        organization: None,
        project: None,
        validate_model: false,
        transport_mode: OpenAiTransportMode::ResponsesWebsocket,
        proxy_url,
    };
    let mut session = connect_responses_websocket(
        &provider,
        None,
        &RestoredProtocolContext::default(),
        1,
        None,
        Instant::now(),
    )
    .await
    .unwrap();
    ready_rx.await.unwrap();
    session
        .stream
        .send(Message::Text(request.clone().into()))
        .await
        .unwrap();
    assert_eq!(
        session
            .stream
            .next()
            .await
            .unwrap()
            .unwrap()
            .into_text()
            .unwrap()
            .as_str(),
        response
    );
    let Message::Close(Some(frame)) = session.stream.next().await.unwrap().unwrap() else {
        panic!("original peer close frame must survive the socket owner");
    };
    assert_eq!(frame.code, CloseCode::Policy);
    assert_eq!(
        frame.reason.as_str(),
        "missing first response.create message"
    );
    let physical = session.stream.failure().unwrap();
    let diagnostic = recovery_diagnostics::failure(&physical.into(), Some(1), None);
    assert_eq!(
        diagnostic["close_reason"],
        "missing first response.create message"
    );
    server.await.unwrap();
    drop(session);
    if let Some(proxy) = proxy_task {
        proxy.await.unwrap();
    }

    // Inspect actual WebSocket frame bytes, not only two peers' decoded values.
    for (wire, plain_len) in [
        (&trace.read, request.len()),
        (&trace.written, response.len()),
    ] {
        let bytes = wire.lock().unwrap();
        assert_eq!(
            bytes[0] & 0x40 != 0,
            compressed,
            "RSV1 marks negotiated compressed data"
        );
        if compressed {
            assert!(
                bytes.len() < plain_len,
                "compression must reduce transmitted bytes"
            );
        } else {
            assert!(
                bytes.len() >= plain_len,
                "unnegotiated peer receives original bytes"
            );
        }
    }
}

#[tokio::test]
async fn direct_egress_negotiates_compression_and_preserves_large_messages() {
    tokio::time::timeout(Duration::from_secs(15), compression_roundtrip(false, true))
        .await
        .unwrap();
}

#[tokio::test]
async fn proxy_egress_negotiates_compression_and_preserves_large_messages() {
    tokio::time::timeout(Duration::from_secs(15), compression_roundtrip(true, true))
        .await
        .unwrap();
}

#[tokio::test]
async fn upstream_without_compression_remains_interoperable() {
    tokio::time::timeout(Duration::from_secs(15), compression_roundtrip(false, false))
        .await
        .unwrap();
}
