use super::*;
use futures_util::StreamExt;
use std::io::{Read, Write};

#[tokio::test]
async fn records_actual_serialized_http_and_sse_without_credentials() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let body = "{\"model\":\"fixture\",\"stream\":true}";
    let sse = "data: {\"text\":\"真实响应\"}\n\ndata: [DONE]\n\n";
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
            if request.ends_with(body.as_bytes()) {
                break;
            }
        }
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", sse.len(), sse).unwrap();
        request
    });
    let mut events = Vec::new();
    capture(
        "fixture".into(),
        |event| {
            events.push(event.clone());
            Ok(())
        },
        |_sink| async move {
            let response = reqwest::Client::new()
                .post(format!("http://{address}/responses"))
                .bearer_auth("must-not-be-logged")
                .body(body)
                .send_observed()
                .await?;
            let mut stream = response.bytes_stream().inspect(observe_chunk);
            while let Some(chunk) = stream.next().await {
                chunk?;
            }
            Ok(())
        },
    )
    .await
    .unwrap();
    let wire = String::from_utf8(server.join().unwrap()).unwrap();
    assert!(wire.ends_with(body));
    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains("must-not-be-logged"));
    let mut received = String::new();
    let mut saw_request = false;
    let mut saw_end = false;
    for event in events {
        if let ProviderStreamEvent::ProtocolObservation {
            protocol,
            direction,
            kind,
            body: observed,
            encoding,
            ..
        } = event
        {
            assert_eq!(protocol, "openai.responses");
            assert_eq!(encoding, "utf8");
            if kind == "request" {
                assert_eq!(direction, "sent");
                assert_eq!(observed, body);
                saw_request = true;
            }
            if kind == "response_body" {
                received.push_str(&observed);
            }
            if kind == "stream_end" {
                saw_end = true;
            }
        }
    }
    assert!(saw_request && saw_end);
    assert_eq!(received, sse);
}

#[tokio::test]
async fn failed_transport_never_claims_complete_and_split_utf8_is_lossless() {
    let mut events = Vec::new();
    let result: Result<()> = capture(
        "fixture".into(),
        |event| {
            events.push(event.clone());
            Ok(())
        },
        |_sink| async {
            record("sse", "received", "response_body", &[0xe4, 0xb8], None);
            anyhow::bail!("connection reset")
        },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(events.len(), 1);
    let ProviderStreamEvent::ProtocolObservation {
        encoding,
        body,
        kind,
        ..
    } = &events[0]
    else {
        panic!("missing observation")
    };
    assert_eq!(encoding, "base64");
    assert_eq!(body, "5Lg=");
    assert_eq!(kind, "response_body");
}
