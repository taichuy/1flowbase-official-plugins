#![cfg(target_os = "linux")]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{accept, Message};

const IO_TIMEOUT: Duration = Duration::from_secs(10);
// Tokio's default DNS blocking-thread idle timeout is 10 seconds.
const DNS_RETIREMENT_GAP: Duration = Duration::from_secs(12);

struct Worker {
    child: Child,
    input: Option<Sender<String>>,
    output: Receiver<Result<Value>>,
    readers: Vec<JoinHandle<()>>,
}

impl Worker {
    fn start() -> Result<Self> {
        let memory_bytes: u64 = include_str!("../manifest.yaml")
            .lines()
            .find_map(|line| line.trim().strip_prefix("memory_bytes:"))
            .context("manifest memory limit")?
            .trim()
            .parse()?;
        ensure!(
            memory_bytes == 268_435_456,
            "revisit fixture for changed budget"
        );
        let mut child = Command::new("sh")
            .args([
                "-c",
                "ulimit -c 0; ulimit -v \"$1\" || exit 125; exec \"$2\"",
                "sh",
            ])
            .arg((memory_bytes / 1024).to_string())
            .arg(env!("CARGO_BIN_EXE_openai-provider"))
            // Reproduce a multicore host; an explicit runtime worker budget overrides this.
            .env("TOKIO_WORKER_THREADS", "16")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().context("child stdin")?;
        let stdout = child.stdout.take().context("child stdout")?;
        let mut stderr = child.stderr.take().context("child stderr")?;
        let (input_tx, input_rx) = mpsc::channel::<String>();
        let (output_tx, output_rx) = mpsc::channel();
        let writer = thread::spawn(move || {
            for command in input_rx {
                if writeln!(stdin, "{command}")
                    .and_then(|_| stdin.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let frame = line
                    .map_err(anyhow::Error::from)
                    .and_then(|line| serde_json::from_str(&line).context("invalid worker NDJSON"));
                if output_tx.send(frame).is_err() {
                    break;
                }
            }
        });
        let diagnostics = thread::spawn(move || {
            // Drain all stderr but retain only a bounded synthetic-fixture diagnostic.
            let mut retained = Vec::new();
            let mut chunk = [0; 1024];
            while let Ok(count) = stderr.read(&mut chunk) {
                if count == 0 {
                    break;
                }
                let keep = count.min(4096_usize.saturating_sub(retained.len()));
                retained.extend_from_slice(&chunk[..keep]);
            }
            if !retained.is_empty() {
                eprintln!(
                    "worker stderr (first 4096 bytes): {}",
                    String::from_utf8_lossy(&retained)
                );
            }
        });
        Ok(Self {
            child,
            input: Some(input_tx),
            output: output_rx,
            readers: vec![writer, reader, diagnostics],
        })
    }

    fn invoke(&self, request: Value, expected_id: &str) -> Result<()> {
        self.input
            .as_ref()
            .context("stdin closed")?
            .send(request.to_string())?;
        let deadline = Instant::now() + IO_TIMEOUT;
        loop {
            let frame = self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .context("worker result deadline or stdout EOF")??;
            ensure!(frame["type"] != "error", "worker returned error: {frame}");
            if frame["type"] == "result" {
                ensure!(
                    frame["result"]["response_id"] == expected_id,
                    "unexpected result: {frame}"
                );
                return Ok(());
            }
        }
    }

    fn finish(&mut self) -> Result<()> {
        self.input.take();
        let deadline = Instant::now() + IO_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait()? {
                ensure!(status.success(), "worker exited abnormally: {status}");
                return Ok(());
            }
            ensure!(Instant::now() < deadline, "worker exit deadline");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

struct Server {
    address: String,
    ping: Sender<()>,
    pong: Receiver<()>,
    cancelled: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<()>>>,
}

impl Server {
    fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = format!("http://localhost:{}", listener.local_addr()?.port());
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let (ping, ping_rx) = mpsc::channel();
        let (pong_tx, pong) = mpsc::channel();
        let handle = thread::spawn(move || {
            for index in 0..3 {
                let deadline = Instant::now() + DNS_RETIREMENT_GAP + IO_TIMEOUT;
                let stream = loop {
                    ensure!(!stop.load(Ordering::Relaxed), "server cancelled");
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            ensure!(Instant::now() < deadline, "connection deadline");
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(error) => return Err(error.into()),
                    }
                };
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(IO_TIMEOUT))?;
                stream.set_write_timeout(Some(IO_TIMEOUT))?;
                let mut socket = accept(stream)?;
                let request = socket.read()?.into_text()?;
                ensure!(
                    request.len() >= 900_000,
                    "fixture must exercise large native history"
                );
                let request: Value = serde_json::from_str(&request)?;
                ensure!(
                    request["type"] == "response.create",
                    "expected response.create"
                );
                ensure!(
                    request["input"].as_array().map(Vec::len) == Some(208),
                    "history missing"
                );
                socket.send(Message::Text(
                    json!({
                        "type": "response.completed",
                        "response": {
                            "id": format!("resp_memory_{index}"), "status": "completed",
                            "output": [],
                            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
                        }
                    })
                    .to_string()
                    .into(),
                ))?;
                // Parent triggers this only after observing result, with stdin left open and idle.
                ping_rx.recv_timeout(IO_TIMEOUT)?;
                socket.send(Message::Ping(vec![index as u8].into()))?;
                match socket.read()? {
                    Message::Pong(payload) if payload[..] == [index as u8] => {}
                    message => bail!("expected idle Pong, got {message:?}"),
                }
                pong_tx.send(())?;
            }
            Ok(())
        });
        Ok(Self {
            address,
            ping,
            pong,
            cancelled,
            handle: Some(handle),
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn request(address: &str, index: usize) -> Value {
    let input: Vec<_> = (0..208).map(|item| json!({
        "type": "message", "role": "user",
        "content": [{"type": "input_text", "text": format!("synthetic-{item}:{}", "x".repeat(4608))}]
    })).collect();
    let body = json!({"model": "fixture-model", "input": input, "stream": true});
    let size_bytes = body.to_string().len();
    json!({
        "method": "invoke", "input": {
            "contract_version": "1flowbase.provider/v2", "operation": "generate",
            "provider_instance_id": "memory-fixture", "provider_code": "openai",
            "protocol": "openai_responses", "model": "fixture-model",
            "provider_config": {"base_url": address, "api_key": "synthetic", "transport_mode": "responses_websocket"},
            "required_capabilities": ["responses.native_passthrough", "responses.native_output.v1"],
            "native_transport": {"protocol": "openai_responses", "wire_body": body, "digest": "fixture", "size_bytes": size_bytes},
            "client_protocol_envelope": {"source_protocol": "openai_responses", "headers": {"session-id": [format!("synthetic-{index}")]}, "body": {}},
            "run_context": {"physical_transport_session": {
                "logical_session_id": format!("memory-session-{index}"), "generation": index + 1,
                "worker_incarnation": 1, "task_id": format!("task-{index}"), "state": "active",
                "physical_deadline_unix_ms": 4102444800000_u64
            }}
        }
    })
}

#[test]
fn native_history_survives_dns_retirement_within_manifest_memory_budget() -> Result<()> {
    let mut server = Server::start()?;
    let mut worker = Worker::start()?;
    for index in 0..3 {
        worker.invoke(
            request(&server.address, index),
            &format!("resp_memory_{index}"),
        )?;
        server.ping.send(())?;
        server
            .pong
            .recv_timeout(IO_TIMEOUT)
            .context("background I/O stalled while stdin was idle")?;
        if index < 2 {
            thread::sleep(DNS_RETIREMENT_GAP);
        }
    }
    worker.finish()?;
    server
        .handle
        .take()
        .context("server handle")?
        .join()
        .map_err(|_| anyhow::anyhow!("server panicked"))??;
    Ok(())
}
