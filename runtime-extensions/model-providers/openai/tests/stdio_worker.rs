use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use serde_json::{json, Value};

fn read_http_request(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut header_end = None;
    let mut body_length = None;

    loop {
        let read = stream.read(&mut chunk).expect("request should be readable");
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);

        if header_end.is_none() {
            header_end = buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|offset| offset + 4);
            if let Some(end) = header_end {
                let headers = String::from_utf8_lossy(&buffer[..end]);
                body_length = headers.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                });
            }
        }

        if let (Some(end), Some(length)) = (header_end, body_length) {
            if buffer.len() >= end + length {
                return;
            }
        }
        if header_end.is_some() && body_length.is_none() {
            return;
        }
    }
}

fn start_two_response_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (mut first_stream, _) = listener.accept().expect("first request should connect");
        read_http_request(&mut first_stream);
        let error_body = r#"{"error":{"message":"upstream exploded"}}"#;
        write!(
            first_stream,
            "HTTP/1.1 500 Internal Server Error\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            error_body.len(),
            error_body
        )
        .expect("error response should be writable");

        let (mut second_stream, _) = listener.accept().expect("second request should connect");
        read_http_request(&mut second_stream);
        let response_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_ok\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2},\"output\":[]}}\n\n"
        );
        write!(
            second_stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        )
        .expect("success response should be writable");
    });

    (address, handle)
}

fn start_websocket_reject_then_sse_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (mut first_stream, _) = listener.accept().expect("websocket request should connect");
        read_http_request(&mut first_stream);
        let error_body = r#"{"error":{"message":"websocket unavailable"}}"#;
        write!(
            first_stream,
            "HTTP/1.1 405 Method Not Allowed\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            error_body.len(),
            error_body
        )
        .expect("websocket rejection should be writable");

        let (mut second_stream, _) = listener.accept().expect("fallback request should connect");
        read_http_request(&mut second_stream);
        let response_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"response_id\":\"resp_fallback\",\"delta\":\"fallback ok\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_fallback\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2},\"output\":[]}}\n\n"
        );
        write!(
            second_stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        )
        .expect("fallback response should be writable");
    });

    (address, handle)
}

fn invoke_line(base_url: &str, transport_mode: &str) -> String {
    serde_json::to_string(&json!({
        "method": "invoke",
        "input": {
            "model": "gpt-5.3-codex-spark",
            "provider_config": {
                "base_url": base_url,
                "api_key": "test-key",
                "transport_mode": transport_mode
            },
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        }
    }))
    .expect("invoke request should serialize")
}

fn next_json_line(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .expect("stdout should be readable");
        assert!(read > 0, "provider worker exited before the next JSON line");
        if !line.trim().is_empty() {
            return serde_json::from_str(line.trim()).expect("stdout line should be JSON");
        }
    }
}

#[test]
fn invoke_error_emits_result_line_and_keeps_worker_reusable() {
    let (base_url, server) = start_two_response_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "http_sse")).expect("first request should write");
    stdin.flush().expect("first request should flush");

    let error_line = next_json_line(&mut stdout);
    assert_eq!(error_line["type"], "error");
    assert_eq!(error_line["error"]["kind"], "provider_invalid_response");
    assert!(error_line["error"]["message"]
        .as_str()
        .expect("error message should be a string")
        .contains("upstream exploded"));

    let result_line = next_json_line(&mut stdout);
    assert_eq!(result_line["type"], "result");
    assert_eq!(result_line["result"]["finish_reason"], "error");

    writeln!(stdin, "{}", invoke_line(&base_url, "http_sse")).expect("second request should write");
    stdin.flush().expect("second request should flush");

    let mut saw_text_delta = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("text_delta") => {
                saw_text_delta = true;
                assert_eq!(line["delta"], "ok");
            }
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "ok");
                assert_eq!(line["result"]["response_id"], "resp_ok");
                break;
            }
            _ => {}
        }
    }
    assert!(saw_text_delta);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}

#[test]
fn websocket_transport_falls_back_to_sse_before_response_events() {
    let (base_url, server) = start_websocket_reject_then_sse_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "responses_websocket"))
        .expect("request should write");
    stdin.flush().expect("request should flush");

    let mut saw_text_delta = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("text_delta") => {
                saw_text_delta = true;
                assert_eq!(line["delta"], "fallback ok");
            }
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "fallback ok");
                assert_eq!(line["result"]["response_id"], "resp_fallback");
                assert_eq!(line["result"]["provider_metadata"]["transport"], "http_sse");
                break;
            }
            Some("error") => panic!("fallback should not emit an error line: {line}"),
            _ => {}
        }
    }
    assert!(saw_text_delta);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}
