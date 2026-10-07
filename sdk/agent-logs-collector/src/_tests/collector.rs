use super::*;
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

const TS: &str = "2026-10-07T08:00:00.123Z";
struct Generic;
impl SourceAdapter for Generic {
    type Context = Option<String>;
    fn source_client(&self) -> &'static str {
        "generic-fixture"
    }
    fn create_context(&self, _: &Value) -> anyhow::Result<Self::Context> {
        Ok(None)
    }
    fn convert(
        &self,
        record: &Position,
        turn: &mut Self::Context,
    ) -> anyhow::Result<Option<AgentLogEvent>> {
        if let Some(value) = record.line["turn"].as_str() {
            *turn = Some(value.to_owned());
        }
        Ok(Some(AgentLogEvent {
            event_id: String::new(),
            source_session_id: "fixture-session".into(),
            source_task_id: turn.clone().unwrap_or_default(),
            parent_source_task_id: None,
            sequence: 0,
            occurred_at: TS.into(),
            kind: AgentLogEventKind::Context,
            content: record.line["content"].as_str().map(str::to_owned),
            phase: None,
            name: None,
            call_id: None,
            model_id: None,
            provider_code: None,
            usage: None,
            inherited: false,
            raw: record.line.clone(),
        }))
    }
}
struct Fixture {
    directory: PathBuf,
    config: Config,
    received: Arc<Mutex<Vec<Value>>>,
    response: Arc<Mutex<(u16, Option<Value>)>>,
    stop: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("collector-fixture-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(directory.join("source")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/events", listener.local_addr().unwrap());
        let received = Arc::new(Mutex::new(Vec::new()));
        let response = Arc::new(Mutex::new((200, None::<Value>)));
        let stop = Arc::new(AtomicBool::new(false));
        let (r, status, done) = (received.clone(), response.clone(), stop.clone());
        let server = thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let (mut socket, _) = match listener.accept() {
                    Ok(socket) => socket,
                    Err(_) => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut data = Vec::new();
                let mut buffer = [0; 4096];
                let (header_end, content_len) = loop {
                    let size = socket.read(&mut buffer).unwrap();
                    if size == 0 {
                        return;
                    }
                    data.extend_from_slice(&buffer[..size]);
                    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&data[..pos]).to_lowercase();
                        assert!(header.contains("authorization: bearer fixture-secret"));
                        let size: usize = header
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        break (pos + 4, size);
                    }
                };
                while data.len() < header_end + content_len {
                    let size = socket.read(&mut buffer).unwrap();
                    if size == 0 {
                        return;
                    }
                    data.extend_from_slice(&buffer[..size]);
                }
                let body: Value =
                    serde_json::from_slice(&data[header_end..header_end + content_len]).unwrap();
                let len = body["events"].as_array().unwrap().len();
                r.lock().unwrap().push(body);
                let (code, override_receipt) = status.lock().unwrap().clone();
                let body = override_receipt.unwrap_or_else(|| json!({"data":{"accepted_events":len,"duplicate_events":0,"record_ids":["00000000-0000-4000-8000-000000000001"]},"meta":null})).to_string();
                write!(socket, "HTTP/1.1 {code} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let config = Config {
            version: 1,
            endpoint,
            source_path: directory.join("source"),
            state_path: directory.join("state.json"),
            api_key: "fixture-secret".into(),
        };
        Self {
            directory,
            config,
            received,
            response,
            stop,
            server: Some(server),
        }
    }
    fn write(&self, rows: &[Value]) {
        fs::write(self.config.source_path.join("a.jsonl"), encode(rows)).unwrap();
    }
    fn state(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.config.state_path).unwrap()).unwrap()
    }
    fn events(&self) -> Vec<Value> {
        self.received
            .lock()
            .unwrap()
            .iter()
            .flat_map(|v| v["events"].as_array().unwrap().clone())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.take() {
            server.join().unwrap();
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn encode(rows: &[Value]) -> String {
    rows.iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[tokio::test]
async fn committed_records_resume_archive_and_stable_byte_identity() {
    let f = Fixture::new();
    let rows = [
        json!({"header":"immutable"}),
        json!({"turn":"turn-a"}),
        json!({"content":"same"}),
        json!({"content":"same"}),
    ];
    let partial = json!({"content":"later"}).to_string();
    fs::write(
        f.config.source_path.join("a.jsonl"),
        encode(&rows) + &partial,
    )
    .unwrap();
    assert_eq!(collect(&f.config, &Generic).await.unwrap().uploaded, 4);
    assert_eq!(collect(&f.config, &Generic).await.unwrap().uploaded, 0);
    let identity = hash(format!("generic-fixture:{}", rows[0]));
    let events = f.events();
    assert_eq!(events[0]["event_id"], hash(format!("{identity}:0")));
    assert_ne!(events[2]["event_id"], events[3]["event_id"]);
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(f.config.source_path.join("a.jsonl"))
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    assert_eq!(collect(&f.config, &Generic).await.unwrap().uploaded, 1);
    fs::create_dir_all(f.config.source_path.join("archive")).unwrap();
    fs::rename(
        f.config.source_path.join("a.jsonl"),
        f.config.source_path.join("archive/a.jsonl"),
    )
    .unwrap();
    assert_eq!(collect(&f.config, &Generic).await.unwrap().uploaded, 0);
    assert!(f
        .events()
        .iter()
        .all(|event| event["source_task_id"] == "turn-a"));
}
#[tokio::test]
async fn identity_saved_before_failed_http_and_full_ack_required() {
    let f = Fixture::new();
    f.write(&[json!({"header":"fixed"}), json!({"turn":"a"})]);
    *f.response.lock().unwrap() = (503, None);
    assert!(collect(&f.config, &Generic)
        .await
        .unwrap_err()
        .to_string()
        .contains("503"));
    let identity = f.state()["source_id"].clone();
    assert_eq!(f.state()["files"], json!({}));
    let invalid = [
        json!({"accepted_events":2,"duplicate_events":0,"record_ids":[]}),
        json!({"data":null}),
        json!({"data":{"accepted_events":0,"duplicate_events":0,"record_ids":[]}}),
        json!({"data":{"accepted_events":2,"duplicate_events":0,"record_ids":["not-a-uuid"]}}),
        json!({"data":{"accepted_events":-1,"duplicate_events":3,"record_ids":[]}}),
        json!({"data":{"accepted_events":1.5,"duplicate_events":0.5,"record_ids":[]}}),
    ];
    for receipt in invalid {
        *f.response.lock().unwrap() = (200, Some(receipt));
        assert!(collect(&f.config, &Generic).await.is_err());
        assert_eq!(f.state()["files"], json!({}));
        assert_eq!(f.state()["source_id"], identity);
    }
    *f.response.lock().unwrap() = (
        200,
        Some(
            json!({"data":{"accepted_events":0,"duplicate_events":2,"record_ids":["00000000-0000-4000-8000-000000000001"]}}),
        ),
    );
    assert_eq!(collect(&f.config, &Generic).await.unwrap().uploaded, 2);
    let received = f.received.lock().unwrap();
    assert!(received.iter().all(|batch| batch["source_id"] == identity));
    assert!(received
        .windows(2)
        .all(|pair| pair[0]["events"] == pair[1]["events"]));
}
#[tokio::test]
async fn source_prefix_mutation_truncation_and_binding_fail_closed() {
    let f = Fixture::new();
    let header = json!({"header":"fixed"});
    f.write(&[
        header.clone(),
        json!({"turn":"a"}),
        json!({"content":"original"}),
    ]);
    collect(&f.config, &Generic).await.unwrap();
    let state = f.state();
    f.write(&[
        header.clone(),
        json!({"turn":"a"}),
        json!({"content":"mutated!"}),
    ]);
    assert!(collect(&f.config, &Generic).await.is_err());
    assert_eq!(f.state(), state);
    f.write(&[header]);
    assert!(collect(&f.config, &Generic)
        .await
        .unwrap_err()
        .to_string()
        .contains("truncated"));
    let mut other = f.config.clone();
    other.endpoint.push_str("/other");
    assert!(collect(&other, &Generic).await.is_err());
    other = f.config.clone();
    other.source_path = f.directory.join("other-source");
    assert!(collect(&other, &Generic).await.is_err());
}
#[tokio::test]
async fn unowned_facts_wait_then_preserve_identity_and_raw_when_claimed() {
    let f = Fixture::new();
    let rows = [json!({"header":"fixed"}), json!({"content":"pre-turn"})];
    f.write(&rows);
    let report = collect(&f.config, &Generic).await.unwrap();
    assert_eq!(report.uploaded, 0);
    assert_eq!(report.unattributed_files, 1);
    assert_eq!(f.state()["files"], json!({}));
    f.write(&[rows[0].clone(), rows[1].clone(), json!({"turn":"explicit"})]);
    assert_eq!(collect(&f.config, &Generic).await.unwrap().uploaded, 3);
    assert_eq!(f.events()[1]["raw"], rows[1]);
    assert_eq!(f.events()[1]["source_task_id"], "explicit");
}
#[test]
fn lock_exclusion_release_and_crash_recovery() {
    let path = std::env::temp_dir().join(format!("collector-lock-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    let state = path.join("state.json");
    let guard = CheckpointLock::acquire(&state).unwrap();
    assert!(CheckpointLock::acquire(&state).is_err());
    drop(guard);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::child_crash_lock", "--nocapture"])
        .env("COLLECTOR_FIXTURE_LOCK", &state)
        .status()
        .unwrap();
    assert_eq!(child.code(), Some(7));
    let recovered = CheckpointLock::acquire(&state).unwrap();
    drop(recovered);
    fs::remove_dir_all(path).unwrap();
}
#[test]
fn child_crash_lock() {
    if let Some(path) = std::env::var_os("COLLECTOR_FIXTURE_LOCK") {
        let _guard = CheckpointLock::acquire(Path::new(&path)).unwrap();
        std::process::exit(7);
    }
}
#[test]
fn reconfigure_preserves_state_and_key_is_private_on_unix() {
    let f = Fixture::new();
    let path = f.directory.join("config.json");
    configure(
        &path,
        f.config.endpoint.clone(),
        &f.config.source_path,
        "first-key".into(),
    )
    .unwrap();
    fs::write(&f.config.state_path, b"existing checkpoint bytes").unwrap();
    configure(
        &path,
        f.config.endpoint.clone(),
        &f.config.source_path,
        "second-key".into(),
    )
    .unwrap();
    assert_eq!(
        fs::read(&f.config.state_path).unwrap(),
        b"existing checkpoint bytes"
    );
    assert_eq!(Config::load(&path).unwrap().api_key, "second-key");
    assert!(configure(
        &path,
        "https://example.test/other".into(),
        &f.config.source_path,
        "key".into()
    )
    .is_err());
    assert!(configure(
        &path,
        "https://key@example.test/".into(),
        &f.config.source_path,
        "key".into()
    )
    .is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
#[cfg(unix)]
#[tokio::test]
async fn source_symlink_is_not_followed() {
    let f = Fixture::new();
    let outside = f.directory.join("outside.jsonl");
    fs::write(&outside, encode(&[json!({"turn":"a"})])).unwrap();
    std::os::unix::fs::symlink(outside, f.config.source_path.join("linked.jsonl")).unwrap();
    assert_eq!(collect(&f.config, &Generic).await.unwrap().uploaded, 0);
}
#[tokio::test]
async fn malformed_committed_record_never_uploads_pending_data() {
    let f = Fixture::new();
    fs::write(
        f.config.source_path.join("a.jsonl"),
        "{\"header\":\"fixed\"}\n{invalid}\n",
    )
    .unwrap();
    assert!(collect(&f.config, &Generic).await.is_err());
    assert!(f.events().is_empty());
    assert_eq!(f.state()["files"], json!({}));
}

#[tokio::test]
async fn no_total_capacity_limit_and_ack_each_batch_before_advancing() {
    let f = Fixture::new();
    let mut rows = vec![json!({"header":"fixed"}), json!({"turn":"a"})];
    for n in 0..213 {
        rows.push(json!({"content":format!("entry-{n}")}));
    }
    f.write(&rows);
    assert_eq!(collect(&f.config, &Generic).await.unwrap().uploaded, 215);
    let received = f.received.lock().unwrap();
    assert_eq!(
        received
            .iter()
            .map(|batch| batch["events"].as_array().unwrap().len())
            .collect::<Vec<_>>(),
        vec![100, 100, 15]
    );
}

#[tokio::test]
async fn empty_truncation_and_header_replacement_fail_after_ack() {
    let f = Fixture::new();
    f.write(&[json!({"header":"fixed"}), json!({"turn":"a"})]);
    collect(&f.config, &Generic).await.unwrap();
    let state = f.state();
    fs::write(f.config.source_path.join("a.jsonl"), b"").unwrap();
    assert!(collect(&f.config, &Generic)
        .await
        .unwrap_err()
        .to_string()
        .contains("truncated"));
    assert_eq!(f.state(), state);
    f.write(&[json!({"header":"replaced"}), json!({"turn":"a"})]);
    assert!(collect(&f.config, &Generic)
        .await
        .unwrap_err()
        .to_string()
        .contains("header changed"));
    assert_eq!(f.state(), state);
}
