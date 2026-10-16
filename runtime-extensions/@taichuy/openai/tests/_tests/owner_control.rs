use super::*;
use std::sync::mpsc::{self, Receiver};

struct Worker {
    child: std::process::Child,
    input: std::process::ChildStdin,
    frames: Receiver<Value>,
    sequence: u64,
}

impl Worker {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (send, frames) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                let Ok(frame) = serde_json::from_str(&line) else {
                    break;
                };
                if send.send(frame).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            frames,
            sequence: 0,
        }
    }

    fn call(&mut self, request: Value) -> Value {
        self.sequence += 1;
        let id = self.sequence.to_string();
        let frame = json!({"protocol":"stdio_json_multiplex_v1","kind":"call", "call_id":id,"request":request});
        writeln!(self.input, "{frame}").unwrap();
        self.input.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let frame = self
                .frames
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("worker must finish a bounded fixture call");
            assert_eq!(frame["call_id"], id);
            if frame["kind"] == "response" {
                return frame["response"].clone();
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn peer(listener: TcpListener, through_proxy: bool, connections: usize) -> thread::JoinHandle<()> {
    listener.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        for _ in 0..connections {
            let mut stream = accept_until(&listener, Instant::now() + Duration::from_secs(10))
                .expect("provider must open fixture connection");
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            if through_proxy {
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    header.push(byte[0]);
                }
                assert!(String::from_utf8(header).unwrap().starts_with("CONNECT "));
                stream
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .unwrap();
            }
            let mut socket = accept_with_config(stream, Some(Default::default())).unwrap();
            loop {
                match socket.read().unwrap() {
                    Message::Text(text) => {
                        let request: Value = serde_json::from_str(&text).unwrap();
                        assert_eq!(request["type"], "response.create");
                        let id = if through_proxy {
                            "resp-proxy"
                        } else {
                            "resp-direct"
                        };
                        socket
                            .send(Message::Text(
                                json!({"type":"response.completed",
                                    "response":{"id":id,"status":"completed","output":[]}
                                })
                                .to_string()
                                .into(),
                            ))
                            .unwrap();
                    }
                    Message::Close(_) => {
                        socket.flush().unwrap();
                        break;
                    }
                    Message::Ping(_) => socket.flush().unwrap(),
                    other => panic!("unexpected fixture frame: {other:?}"),
                }
            }
        }
    })
}

fn close(generation: u64, worker: Option<u64>) -> Value {
    json!({"method":"transport_session","input":{
        "logical_session_id":"semantic-native-fixture","generation":generation,
        "worker_incarnation":worker,"action":"close","deadline_unix_ms":4_102_444_800_000_i64
    }})
}

fn assert_closed(receipt: &Value, generation: u64) {
    assert_eq!(receipt["ok"], true, "{receipt}");
    let result = &receipt["result"];
    assert_eq!(result["physical_state"], "closed");
    assert_eq!(result["closure_evidence"]["local_released"], true);
    assert_eq!(
        result["closure_evidence"]["identity"],
        json!({
            "logical_session_id":"semantic-native-fixture", "generation":generation,"worker_incarnation":1
        })
    );
}

#[test]
fn proxy_removal_closes_original_physical_owners_without_cross_session_control() {
    let direct = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", direct.local_addr().unwrap());
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let direct_peer = peer(direct, false, 1);
    let proxy_peer = peer(proxy, true, 2);
    let mut worker = Worker::new();
    for generation in 1..=3 {
        let mut invocation = websocket_request(&base_url, None);
        invocation["input"]["run_context"]["physical_transport_session"]["generation"] =
            json!(generation);
        if generation < 3 {
            invocation["input"]["required_capabilities"] = json!(["network_egress_handoff/v1"]);
            invocation["input"]["run_context"]["network_egress"] = json!({
                "mode":"required_http_proxy", "http_proxy_url":proxy_url,
                "expires_at":"2100-01-01T00:00:00Z", "required":true
            });
        }
        let response_id = if generation < 3 {
            "resp-proxy"
        } else {
            "resp-direct"
        };
        assert_eq!(worker.call(invocation.clone())["response_id"], response_id);
        for command in [
            close(99, Some(1)),
            close(generation, Some(99)),
            close(generation, None),
        ] {
            let missing = worker.call(command);
            assert_eq!(missing["result"]["physical_state"], "faulted");
            assert!(missing["result"]["closure_evidence"].is_null());
        }
        if generation > 1 {
            assert_closed(&worker.call(close(generation - 1, Some(1))), generation - 1);
        }
        // Missing/stale controls must leave the currently owning socket usable.
        assert_eq!(worker.call(invocation)["response_id"], response_id);
        let receipt = worker.call(close(generation, Some(1)));
        assert_closed(&receipt, generation);
        assert_eq!(worker.call(close(generation, Some(1))), receipt);
    }
    drop(worker);
    proxy_peer.join().unwrap();
    direct_peer.join().unwrap();
}
