use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};

#[test]
fn wss_client_hello_reaches_loopback_with_both_rustls_backends_enabled() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("https://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "WSS client did not connect");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("loopback accept failed: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut hello = [0u8; 5];
        stream.read_exact(&mut hello).unwrap();
        assert_eq!(hello[0], 0x16, "first WSS bytes must be a TLS handshake");
        assert_eq!(hello[1], 0x03, "first WSS bytes must be a TLS record");
        // Closing after ClientHello produces a bounded TLS error from the worker.
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let request = json!({"method":"invoke","input":{
        "operation":"generate","contract_version":"1flowbase.provider/v2",
        "provider_instance_id":"tls-fixture","provider_code":"openai",
        "protocol":"openai_responses","model":"fixture-model",
        "provider_config":{"base_url":base_url,"api_key":"fixture","transport_mode":"responses_websocket"},
        "model_parameters":{"responses_transport_policy":"force_websocket"},
        "messages":[{"role":"user","content":"hello"}]
    }});
    let frame = json!({"protocol":"stdio_json_multiplex_v1","kind":"call",
        "call_id":"1","request":request});
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{frame}").unwrap();
    stdin.flush().unwrap();

    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let value: Value = serde_json::from_str(&line.unwrap()).unwrap();
            if value["kind"] == "response" {
                tx.send(value).unwrap();
                break;
            }
        }
    });
    let response = rx.recv_timeout(Duration::from_secs(8));
    drop(stdin);
    if response.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    reader.join().unwrap();
    server.join().unwrap();
    let response = response.expect("worker must complete the failed TLS call without panicking");
    assert_eq!(response["protocol"], "stdio_json_multiplex_v1");
    assert_eq!(response["call_id"], "1");
    assert_eq!(response["response"]["finish_reason"], "error");
}
