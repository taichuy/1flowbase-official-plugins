use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use serde_json::{json, Value};

fn read_http_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout should be set");
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
                return String::from_utf8(buffer[end..end + length].to_vec())
                    .expect("request body should be utf8");
            }
        }
        if header_end.is_some() && body_length.is_none() {
            return String::new();
        }
    }

    panic!("request body was not fully captured");
}

fn start_chat_sse_server() -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("request should connect");
        let request_body = read_http_request(&mut stream);
        let response_body = concat!(
            "data: {\"id\":\"chatcmpl_test\",\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_test\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n",
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        )
        .expect("response should be writable");
        request_body
    });

    (address, handle)
}

fn invoke_line(base_url: &str) -> String {
    json!({
        "method": "invoke",
        "input": {
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "aliyun_bailian",
            "protocol": "aliyun_bailian",
            "model": "qwen-plus",
            "provider_config": {
                "base_url": base_url,
                "api_key": "test-key",
                "api_protocol": "openai_chat",
                "validate_model": false
            },
            "system": [{ "type": "text", "text": "Be concise" }],
            "messages": [
                {
                    "role": "user",
                    "content": "hello"
                }
            ],
            "model_parameters": {}
        }
    })
    .to_string()
}

fn next_json_line(stdout: &mut BufReader<impl Read>) -> Value {
    let mut line = String::new();
    stdout
        .read_line(&mut line)
        .expect("stdout line should be readable");
    assert!(!line.trim().is_empty(), "stdout should include a JSON line");
    serde_json::from_str(line.trim()).expect("stdout line should be JSON")
}

#[test]
fn ac_002_fake_upstream_receives_exact_generate_wire_through_stdio() {
    let (base_url, handle) = start_chat_sse_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_aliyun_bailian-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("aliyun bailian provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url)).expect("request should write");
    stdin.flush().expect("request should flush");
    drop(stdin);

    let first_line = next_json_line(&mut stdout);
    assert_eq!(first_line["type"], "text_delta");
    assert_eq!(first_line["delta"], "hello");

    let mut saw_result = false;
    for _ in 0..4 {
        let line = next_json_line(&mut stdout);
        if line["type"] == "result" {
            saw_result = true;
            assert_eq!(line["result"]["final_content"], "hello");
            break;
        }
    }
    assert!(saw_result, "invoke should end with a result line");

    child.kill().expect("provider process should stop");
    let _ = child.wait();
    let body: Value = serde_json::from_str(
        &handle
            .join()
            .expect("fake upstream should capture Generate request"),
    )
    .expect("captured Generate body should be JSON");
    assert_eq!(
        body,
        json!({
            "model": "qwen-plus",
            "messages": [
                { "role": "system", "content": "Be concise" },
                { "role": "user", "content": "hello" }
            ],
            "stream": true,
            "stream_options": { "include_usage": true }
        })
    );
}
