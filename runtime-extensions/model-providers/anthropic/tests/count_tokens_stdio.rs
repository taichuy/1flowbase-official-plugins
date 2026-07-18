use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use serde_json::{json, Value};

fn read_http_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("request read timeout should be configured");
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream
            .read(&mut buffer)
            .expect("request should be readable");
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.split_once(':').and_then(|(name, value)| {
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
            })
            .unwrap_or_default();
        if request.len() >= header_end + 4 + content_length {
            break;
        }
    }
    String::from_utf8(request).expect("request should be UTF-8")
}

fn start_count_tokens_server() -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!(
        "http://{}",
        listener.local_addr().expect("listener address")
    );
    let (request_tx, request_rx) = mpsc::channel();

    thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .expect("CountTokens request should connect");
        let request = read_http_request(&mut stream);
        request_tx
            .send(request)
            .expect("CountTokens request should be captured");
        let body = r#"{"input_tokens":37}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("CountTokens response should be writable");
    });

    (address, request_rx)
}

fn count_tokens_invoke_line(base_url: &str) -> String {
    json!({
        "method": "invoke",
        "input": {
            "operation": "count_tokens",
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-anthropic",
            "provider_code": "anthropic",
            "protocol": "anthropic_messages",
            "model": "claude-sonnet-4-20250514",
            "provider_config": {
                "base_url": base_url,
                "api_key": "stdio-secret",
                "anthropic_version": "2023-06-01"
            },
            "messages": [{ "role": "user", "content": "stdio prompt" }],
            "system": [{ "type": "text", "text": "stdio instructions" }],
            "request_context": { "end_user_reference": "stdio-user" },
            "required_capabilities": [
                "count_tokens",
                "system_prompt_blocks",
                "end_user_reference"
            ],
            "client_protocol_envelope": {
                "source_protocol": "anthropic_messages",
                "policy": "anthropic_messages_v1",
                "headers": { "anthropic-beta": "prompt-caching" }
            }
        }
    })
    .to_string()
}

#[test]
fn c2_count_tokens_uses_the_non_streaming_stdio_envelope() {
    let (base_url, request_rx) = start_count_tokens_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_anthropic-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Anthropic provider binary should spawn");
    let mut stdin = child.stdin.take().expect("provider stdin should be piped");

    writeln!(stdin, "{}", count_tokens_invoke_line(&base_url))
        .expect("CountTokens request should be written");
    stdin.flush().expect("CountTokens request should flush");
    drop(stdin);

    let output = child
        .wait_with_output()
        .expect("Anthropic provider process should finish");
    assert!(
        output.status.success(),
        "Anthropic provider failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout)
        .expect("CountTokens stdio response should be a JSON envelope");
    assert_eq!(response["ok"], json!(true));
    assert_eq!(
        response["result"],
        json!({ "operation": "count_tokens", "input_tokens": 37 })
    );
    assert_eq!(response["error"], Value::Null);

    let request = request_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("fake upstream should capture CountTokens request");
    let (headers, body) = request
        .split_once("\r\n\r\n")
        .expect("captured request should contain headers and body");
    let body: Value = serde_json::from_str(body).expect("captured CountTokens body is JSON");

    assert!(headers.starts_with("POST /v1/messages/count_tokens HTTP/1.1"));
    assert!(headers
        .to_ascii_lowercase()
        .contains("x-api-key: stdio-secret"));
    assert!(headers
        .to_ascii_lowercase()
        .contains("anthropic-beta: prompt-caching"));
    assert_eq!(
        body,
        json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "stdio prompt" }]
            }],
            "system": [{ "type": "text", "text": "stdio instructions" }],
            "metadata": { "user_id": "stdio-user" }
        })
    );
    assert!(body.get("stream").is_none());
    assert!(body.get("max_tokens").is_none());
}
