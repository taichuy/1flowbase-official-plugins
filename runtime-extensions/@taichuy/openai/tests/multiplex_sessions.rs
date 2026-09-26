use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};

fn request(base_url: &str, logical: &str) -> Value {
    json!({"method":"invoke","input":{
        "operation":"generate","contract_version":"1flowbase.provider/v2",
        "provider_instance_id":"fixture","provider_code":"openai",
        "protocol":"openai_responses","model":"fixture-model",
        "provider_config":{"base_url":base_url,"api_key":"fixture","transport_mode":"http_sse"},
        "messages":[{"role":"user","content":"hello"}],
        "run_context":{"physical_transport_session":{
            "logical_session_id":logical,"generation":1,"worker_incarnation":1,
            "task_id":"fixture-task","state":"active",
            "physical_deadline_unix_ms":4_102_444_800_000_i64
        }}
    }})
}

fn accept_until(listener: &TcpListener, deadline: Instant) -> Option<TcpStream> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}

fn respond(mut stream: TcpStream, id: &str) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut body_end = None;
    loop {
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(header_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let header_end = header_end + 4;
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            body_end = Some(header_end + length);
        }
        if body_end.is_some_and(|end| bytes.len() >= end) {
            break;
        }
    }
    let body = format!("data: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"{id}\",\"output\":[]}}}}\n\n");
    write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
}

#[test]
fn independent_logical_sessions_overlap_in_one_worker() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let first = accept_until(&listener, deadline).expect("first call must reach upstream");
        let second = accept_until(&listener, deadline);
        let overlapped = second.is_some();
        if let Some(second) = second {
            respond(second, "resp-second");
        }
        respond(first, "resp-first");
        overlapped
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    for (id, logical) in [(1, "logical-a"), (2, "logical-b")] {
        let frame = json!({"protocol":"stdio_json_multiplex_v1","kind":"call",
            "call_id":id.to_string(),"request":request(&base_url, logical)});
        writeln!(stdin, "{frame}").unwrap();
    }
    stdin.flush().unwrap();
    let mut terminal = Vec::new();
    while terminal.len() < 2 {
        let mut line = String::new();
        assert!(
            stdout.read_line(&mut line).unwrap() > 0,
            "worker exited early"
        );
        let frame: Value = serde_json::from_str(&line).unwrap();
        if frame["kind"] == "response" {
            terminal.push(frame);
        }
    }
    assert!(
        server.join().unwrap(),
        "second session was blocked behind first upstream call"
    );
    assert_ne!(terminal[0]["call_id"], terminal[1]["call_id"]);
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn same_logical_session_waits_for_previous_call() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let first = accept_until(&listener, Instant::now() + Duration::from_secs(5))
            .expect("first call must reach upstream");
        let overlapped = accept_until(&listener, Instant::now() + Duration::from_millis(250));
        let was_overlapped = overlapped.is_some();
        respond(first, "resp-first");
        let second = overlapped
            .or_else(|| accept_until(&listener, Instant::now() + Duration::from_secs(5)))
            .expect("second call must reach upstream");
        respond(second, "resp-second");
        was_overlapped
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    for id in 1..=2 {
        let frame = json!({"protocol":"stdio_json_multiplex_v1","kind":"call",
            "call_id":id.to_string(),"request":request(&base_url, "same-logical")});
        writeln!(stdin, "{frame}").unwrap();
    }
    stdin.flush().unwrap();
    let mut terminal = Vec::new();
    while terminal.len() < 2 {
        let mut line = String::new();
        assert!(
            stdout.read_line(&mut line).unwrap() > 0,
            "worker exited early"
        );
        let frame: Value = serde_json::from_str(&line).unwrap();
        if frame["kind"] == "response" {
            terminal.push(frame);
        }
    }
    assert!(
        !server.join().unwrap(),
        "same session issued two upstream requests concurrently"
    );
    assert_eq!(terminal[0]["call_id"], "1");
    assert_eq!(terminal[1]["call_id"], "2");
    drop(stdin);
    assert!(child.wait().unwrap().success());
}
