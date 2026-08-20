use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use serde_json::{json, Value};

fn read_http_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("request read timeout should be configured");
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
                body_length = String::from_utf8_lossy(&buffer[..end])
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    });
            }
        }
        if let (Some(end), Some(length)) = (header_end, body_length) {
            if buffer.len() >= end + length {
                break;
            }
        }
        if header_end.is_some() && body_length.is_none() {
            break;
        }
    }
    String::from_utf8(buffer).expect("fixture HTTP request should be UTF-8")
}

fn write_json_response(stream: &mut TcpStream, status: &str, payload: Value) {
    let body = payload.to_string();
    write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len(),
    )
    .expect("fixture response should be writable");
}

fn account_operation_line(method: &str, base_url: &str, operation: Option<Value>) -> String {
    let config = json!({
        "base_url": base_url,
        "access_token": "test-access-token",
        "chatgpt_account_id": "account-fixture"
    });
    let input = match operation {
        Some(operation) => json!({ "provider_config": config, "operation": operation }),
        None => config,
    };
    serde_json::to_string(&json!({ "method": method, "input": input }))
        .expect("fixture request should serialize")
}

fn next_json_line(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .expect("provider stdout should be readable");
        assert!(read > 0, "provider worker exited before returning JSON");
        if !line.trim().is_empty() {
            return serde_json::from_str(line.trim()).expect("provider stdout should be JSON");
        }
    }
}

#[test]
fn account_operations_use_wham_endpoints_once_per_logical_attempt() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener should bind");
    let base_url = format!(
        "http://{}/backend-api/codex",
        listener.local_addr().unwrap()
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed_requests = Arc::clone(&requests);
    let server = thread::spawn(move || {
        for (method, path, response) in [
            (
                "GET",
                "/backend-api/wham/usage",
                json!({
                    "rate_limit": {
                        "primary_window": {
                            "used_percent": 42.0,
                            "limit_window_seconds": 18_000,
                            "reset_at": 1_770_000_000
                        },
                        "secondary_window": {
                            "used_percent": 61.0,
                            "limit_window_seconds": 604_800,
                            "reset_at": null
                        }
                    }
                }),
            ),
            (
                "GET",
                "/backend-api/wham/rate-limit-reset-credits",
                json!({ "available_count": 2, "credits": [{ "id": "not-exposed" }] }),
            ),
            (
                "POST",
                "/backend-api/wham/rate-limit-reset-credits/consume",
                json!({ "code": "reset", "credit": { "id": "not-exposed" } }),
            ),
        ] {
            let (mut stream, _) = listener
                .accept()
                .expect("fixture should receive one request");
            let request = read_http_request(&mut stream);
            assert!(
                request.starts_with(&format!("{method} {path} HTTP/1.1")),
                "unexpected request: {request}"
            );
            observed_requests.lock().unwrap().push(request);
            write_json_response(&mut stream, "200 OK", response);
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_chatgpt-codex-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("provider worker should spawn");
    let mut stdin = child.stdin.take().expect("provider stdin should be piped");
    let stdout = child
        .stdout
        .take()
        .expect("provider stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(
        stdin,
        "{}",
        account_operation_line("usage", &base_url, None)
    )
    .expect("usage request should write");
    stdin.flush().expect("usage request should flush");
    let usage = next_json_line(&mut stdout);
    assert_eq!(usage["ok"], true);
    assert_eq!(
        usage["result"]["windows"][0]["limit_window_seconds"],
        18_000
    );
    assert_eq!(usage["result"]["windows"][1]["used_percent"], 61.0);

    writeln!(
        stdin,
        "{}",
        account_operation_line("reset_credit", &base_url, Some(json!({ "type": "count" })))
    )
    .expect("count request should write");
    stdin.flush().expect("count request should flush");
    let count = next_json_line(&mut stdout);
    assert_eq!(count["ok"], true);
    assert_eq!(
        count["result"],
        json!({ "type": "count", "available_count": 2 })
    );

    writeln!(
        stdin,
        "{}",
        account_operation_line(
            "reset_credit",
            &base_url,
            Some(json!({ "type": "consume", "idempotency_key": "logical-attempt-1" }))
        )
    )
    .expect("consume request should write");
    stdin.flush().expect("consume request should flush");
    let consumed = next_json_line(&mut stdout);
    assert_eq!(consumed["ok"], true);
    assert_eq!(consumed["result"], json!({ "type": "consumed" }));

    drop(stdin);
    child
        .wait()
        .expect("provider worker should exit after stdin closes");
    server.join().expect("fixture server should finish");
    let requests = requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        3,
        "each logical action dispatches exactly once"
    );
    assert!(requests[2].contains("\"redeem_request_id\":\"logical-attempt-1\""));
    assert!(!requests[2].contains("not-exposed"));
}

#[test]
fn account_operation_status_failures_never_return_a_success_payload() {
    for status in ["401 Unauthorized", "403 Forbidden", "429 Too Many Requests"] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener should bind");
        let base_url = format!(
            "http://{}/backend-api/codex",
            listener.local_addr().unwrap()
        );
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("usage request should connect");
            let _ = read_http_request(&mut stream);
            write_json_response(&mut stream, status, json!({ "error": "fixture failure" }));
        });
        let mut child = Command::new(env!("CARGO_BIN_EXE_chatgpt-codex-provider"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("provider worker should spawn");
        let mut stdin = child.stdin.take().expect("provider stdin should be piped");
        let stdout = child
            .stdout
            .take()
            .expect("provider stdout should be piped");
        let mut stdout = BufReader::new(stdout);

        writeln!(
            stdin,
            "{}",
            account_operation_line("usage", &base_url, None)
        )
        .expect("usage request should write");
        stdin.flush().expect("usage request should flush");
        let response = next_json_line(&mut stdout);
        assert_eq!(response["ok"], false, "status={status}");
        assert_eq!(response["error"]["kind"], "provider_upstream_error");
        assert_eq!(
            response["error"]["provider_details"]["status"],
            status[..3].parse::<u16>().unwrap(),
            "status={status}"
        );

        drop(stdin);
        child
            .wait()
            .expect("provider worker should exit after stdin closes");
        server.join().expect("fixture server should finish");
    }
}
