use std::{
    io::{BufRead, BufReader, Read, Write},
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
            "model": "unknown-fixture-model",
            "provider_config": {
                "base_url": base_url,
                "api_key": "stdio-secret",
                "anthropic_version": "2023-06-01"
            },
            "messages": [{ "role": "user", "content": "stdio prompt" }],
            "system": [{ "type": "text", "text": "stdio instructions" }],
            "tools": [{
                "name": "weather",
                "description": "Get weather",
                "input_schema": {"type": "object"}
            }],
            "request_context": { "end_user_reference": "stdio-user" },
            "required_capabilities": [
                "count_tokens",
                "system_prompt_blocks",
                "end_user_reference",
                "protocol_context"
            ],
            "client_protocol_envelope": {
                "source_protocol": "anthropic_messages",
                "headers": { "anthropic-beta": ["prompt-caching"] }
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

    let request: Value = serde_json::from_str(&count_tokens_invoke_line(&base_url)).unwrap();
    writeln!(stdin, "{}", json!({
        "protocol": "stdio_json_multiplex_v1", "kind": "call", "call_id": "1", "request": request
    })).expect("CountTokens call should be written");
    stdin.flush().expect("CountTokens call should flush");
    let stdout = child
        .stdout
        .take()
        .expect("provider stdout should be piped");
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("response should be readable");
    let frame: Value = serde_json::from_str(&line).expect("multiplex response should be JSON");
    assert_eq!(frame["protocol"], "stdio_json_multiplex_v1");
    assert_eq!(frame["kind"], "response");
    assert_eq!(frame["call_id"], "1");
    let response = &frame["response"];
    drop(stdin);
    let output = child
        .wait_with_output()
        .expect("provider should exit on EOF");
    assert!(
        output.status.success(),
        "provider failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(response["ok"], json!(true));
    assert_eq!(
        response["result"],
        json!({
            "operation": "count_tokens",
            "input_tokens": 37,
            "method": "upstream_api",
            "coverage": "complete",
            "unknown_block_count": 0
        })
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
            "model": "unknown-fixture-model",
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "stdio prompt" }]
            }],
            "system": [{ "type": "text", "text": "stdio instructions" }],
            "tools": [{
                "name": "weather",
                "description": "Get weather",
                "input_schema": {"type": "object"}
            }],
            "metadata": { "user_id": "stdio-user" }
        })
    );
    assert!(body.get("stream").is_none());
    assert!(body.get("max_tokens").is_none());
}

#[test]
fn multiplex_calls_overlap_and_one_error_does_not_stop_siblings() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fixture should bind");
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let upstream = thread::spawn(move || {
        let mut replies = Vec::new();
        for index in 0..2 {
            let (mut stream, _) = listener.accept().expect("upstream call should connect");
            if index == 0 {
                accepted_tx
                    .send(())
                    .expect("first call should signal acceptance");
            }
            replies.push(thread::spawn(move || {
                let _ = read_http_request(&mut stream);
                if index == 0 {
                    thread::sleep(Duration::from_millis(250));
                }
                let body = format!("{{\"input_tokens\":{}}}", if index == 0 { 11 } else { 22 });
                write!(stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(), body).expect("fixture response should write");
            }));
        }
        for reply in replies {
            reply.join().expect("upstream reply should finish");
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_anthropic-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("worker should start");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stdout = BufReader::new(stdout);
    let request: Value = serde_json::from_str(&count_tokens_invoke_line(&base_url)).unwrap();
    writeln!(
        stdin,
        "{}",
        json!({
            "protocol": "stdio_json_multiplex_v1", "kind": "call", "call_id": "1",
            "request": request
        })
    )
    .unwrap();
    stdin.flush().unwrap();
    accepted_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("first call should reach upstream");
    writeln!(
        stdin,
        "{}",
        json!({
            "protocol": "stdio_json_multiplex_v1", "kind": "call", "call_id": "2",
            "request": request
        })
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        json!({
            "protocol": "stdio_json_multiplex_v1", "kind": "call", "call_id": "3",
            "request": {"method": "invalid", "input": null}
        })
    )
    .unwrap();
    stdin.flush().unwrap();

    let mut frames = Vec::new();
    for _ in 0..3 {
        let mut line = String::new();
        stdout
            .read_line(&mut line)
            .expect("worker response should read");
        frames.push(serde_json::from_str::<Value>(&line).expect("worker response should be JSON"));
    }
    let by_id = |id: &str| {
        frames
            .iter()
            .find(|frame| frame["call_id"] == id)
            .expect("call should finish")
    };
    assert_eq!(by_id("3")["response"]["ok"], false);
    assert_eq!(by_id("2")["response"]["result"]["input_tokens"], 22);
    assert_eq!(by_id("1")["response"]["result"]["input_tokens"], 11);
    let completed = |id: &str| {
        frames
            .iter()
            .position(|frame| frame["call_id"] == id)
            .unwrap()
    };
    assert!(
        completed("2") < completed("1"),
        "fast upstream should pass the slow sibling"
    );
    drop(stdin);
    assert!(child
        .wait()
        .expect("worker should exit after EOF")
        .success());
    upstream.join().expect("upstream should finish");
}
