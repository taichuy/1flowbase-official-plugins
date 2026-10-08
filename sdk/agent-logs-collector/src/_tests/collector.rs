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
            reasoning_effort: None,
            provider_code: None,
            usage: None,
            inherited: false,
            raw: record.line.clone(),
        }))
    }
}
struct Fixture {
    fail_request: Arc<Mutex<Option<usize>>>,
    wire_lengths: Arc<Mutex<Vec<usize>>>,
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
        let fail_request = Arc::new(Mutex::new(None));
        let wire_lengths = Arc::new(Mutex::new(Vec::new()));
        let failure = fail_request.clone();
        let lengths = wire_lengths.clone();
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
                lengths.lock().unwrap().push(content_len);
                let (mut code, override_receipt) = status.lock().unwrap().clone();
                if *failure.lock().unwrap() == Some(r.lock().unwrap().len()) {
                    code = 503;
                }
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
            fail_request,
            wire_lengths,
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
    assert!(f.state()["files"]
        .as_object()
        .unwrap()
        .values()
        .all(|cp| cp["offset"] == 0));
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
        assert!(f.state()["files"]
            .as_object()
            .unwrap()
            .values()
            .all(|cp| cp["offset"] == 0));
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
    assert!(f.state()["files"]
        .as_object()
        .unwrap()
        .values()
        .all(|cp| !cp["pending"].is_null()));
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
    assert!(f.state()["files"]
        .as_object()
        .unwrap()
        .values()
        .all(|cp| cp["offset"] == 0));
}

#[tokio::test]
async fn no_total_capacity_limit_and_ack_each_batch_before_advancing() {
    let f = Fixture::new();
    let mut rows = vec![json!({"header":"fixed"}), json!({"turn":"a"})];
    for n in 0..213 {
        rows.push(json!({"content":format!("entry-{n}")}));
    }
    f.write(&rows);
    let options = CollectOptions {
        batch_target_bytes: 1600,
    };
    assert_eq!(
        collect_with_options(&f.config, &Generic, options)
            .await
            .unwrap()
            .uploaded,
        215
    );
    let received = f.received.lock().unwrap();
    assert!(received.len() > 3);
    assert!(f
        .wire_lengths
        .lock()
        .unwrap()
        .iter()
        .all(|len| *len <= options.batch_target_bytes));
    assert_eq!(
        received
            .iter()
            .map(|batch| batch["events"].as_array().unwrap().len())
            .sum::<usize>(),
        215
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

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct StatefulContext {
    task: Option<String>,
    model: Option<String>,
    provider: Option<String>,
}
#[derive(Default)]
struct Stateful {
    converted: Mutex<Vec<u64>>,
}
impl SourceAdapter for Stateful {
    type Context = StatefulContext;
    fn source_client(&self) -> &'static str {
        "stateful-fixture"
    }
    fn create_context(&self, _: &Value) -> anyhow::Result<Self::Context> {
        Ok(StatefulContext::default())
    }
    fn convert(
        &self,
        record: &Position,
        context: &mut Self::Context,
    ) -> anyhow::Result<Option<AgentLogEvent>> {
        self.converted.lock().unwrap().push(record.start);
        if let Some(value) = record.line["model"].as_str() {
            context.model = Some(value.into());
        }
        if let Some(value) = record.line["provider"].as_str() {
            context.provider = Some(value.into());
        }
        if record.line["skip"] == true {
            return Ok(None);
        }
        let mut event = Generic.convert(record, &mut context.task)?.unwrap();
        event.model_id = context.model.clone();
        event.provider_code = context.provider.clone();
        event.inherited = record.line["inherited"] == true;
        if record.line["end"] == true {
            event.kind = AgentLogEventKind::TaskEnd;
            context.task = None;
        }
        Ok(Some(event))
    }
}
#[tokio::test]
async fn current_snapshot_skips_all_historical_convert_and_preserves_none_updates() {
    let f = Fixture::new();
    let adapter = Stateful::default();
    let rows = [
        json!({"turn":"a"}),
        json!({"skip":true,"model":"quiet-model","provider":"quiet-provider"}),
    ];
    f.write(&rows);
    assert_eq!(collect(&f.config, &adapter).await.unwrap().uploaded, 1);
    adapter.converted.lock().unwrap().clear();
    assert_eq!(collect(&f.config, &adapter).await.unwrap().uploaded, 0);
    assert!(adapter.converted.lock().unwrap().is_empty());
    f.write(&[
        rows[0].clone(),
        rows[1].clone(),
        json!({"content":"appended"}),
    ]);
    assert_eq!(collect(&f.config, &adapter).await.unwrap().uploaded, 1);
    assert_eq!(
        *adapter.converted.lock().unwrap(),
        vec![encode(&rows).len() as u64]
    );
    let last = f.events().pop().unwrap();
    assert_eq!(last["model_id"], "quiet-model");
    assert_eq!(last["provider_code"], "quiet-provider");
}
#[tokio::test]
async fn pending_claim_intermediate_failure_replays_original_context_exactly() {
    let f = Fixture::new();
    let adapter = Stateful::default();
    let rows = [
        json!({"header":"fixed","model":"original","provider":"first"}),
        json!({"content":"unowned","inherited":true}),
        json!({"skip":true,"model":"middle"}),
        json!({"content":"second"}),
        json!({"turn":"future","model":"future-model","provider":"future-provider"}),
        json!({"content":"tail"}),
    ];
    f.write(&rows[..4]);
    let options = CollectOptions {
        batch_target_bytes: 1,
    };
    assert_eq!(
        collect_with_options(&f.config, &adapter, options)
            .await
            .unwrap()
            .uploaded,
        0
    );
    f.write(&rows);
    *f.fail_request.lock().unwrap() = Some(2);
    assert!(collect_with_options(&f.config, &adapter, options)
        .await
        .is_err());
    let cp = f.state()["files"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    assert_eq!(cp["context"]["value"]["model"], "original");
    assert_eq!(cp["context"]["value"]["task"], Value::Null);
    assert_eq!(cp["claim"]["task"], "future");
    assert_eq!(cp["offset"], encode(&rows[..1]).len() as u64);
    let failed = f.received.lock().unwrap()[1]["events"].clone();
    *f.fail_request.lock().unwrap() = None;
    assert_eq!(
        collect_with_options(&f.config, &adapter, options)
            .await
            .unwrap()
            .uploaded,
        4
    );
    assert_eq!(f.received.lock().unwrap()[2]["events"], failed);
    let events = f.events();
    assert!(events.iter().all(|e| e["source_task_id"] == "future"));
    assert_eq!(events[0]["model_id"], "original");
    assert_eq!(events[1]["model_id"], "original");
    assert_eq!(events[1]["provider_code"], "first");
    assert_eq!(events[1]["inherited"], true);
    assert_eq!(events[3]["model_id"], "middle");
    assert_eq!(events[4]["model_id"], "future-model");
    assert_eq!(events[1]["raw"], rows[1]);
    let identity = hash(format!("stateful-fixture:{}", rows[0]));
    assert_eq!(
        events[1]["event_id"],
        hash(format!("{identity}:{}", encode(&rows[..1]).len()))
    );
    let cp = f.state()["files"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    assert!(cp["claim"].is_null());
    assert!(cp["pending"].is_null());
}
#[tokio::test]
async fn task_end_snapshot_clears_turn_and_next_claim_only_changes_ownership() {
    let f = Fixture::new();
    let adapter = Stateful::default();
    let first = [
        json!({"turn":"old","model":"old-model"}),
        json!({"end":true}),
    ];
    f.write(&first);
    collect(&f.config, &adapter).await.unwrap();
    let cp = f.state()["files"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    assert!(cp["context"]["value"]["task"].is_null());
    f.write(&[
        first[0].clone(),
        first[1].clone(),
        json!({"content":"between"}),
        json!({"turn":"new","model":"new-model"}),
    ]);
    collect(&f.config, &adapter).await.unwrap();
    let events = f.events();
    assert_eq!(events[2]["source_task_id"], "new");
    assert_eq!(events[2]["model_id"], "old-model");
    assert_eq!(events[3]["model_id"], "new-model");
}
fn legacy_state(f: &Fixture, rows: &[Value], digest: String) -> Value {
    let identity = hash(format!("stateful-fixture:{}", rows[0]));
    json!({"version":1,"source_id":"legacy-installation","source_client":"stateful-fixture","endpoint":f.config.endpoint,"source_path":f.config.source_path,
        "files":{(identity):{"offset":encode(rows).len(),"prefix_hash":digest,"paths":[f.config.source_path.join("a.jsonl")]}}})
}
#[tokio::test]
async fn v1_migration_verifies_old_nonblank_end_digest_then_replays_once() {
    use sha2::{Digest, Sha256};
    let f = Fixture::new();
    let adapter = Stateful::default();
    let rows = [
        json!({"turn":"a","model":"old-model"}),
        json!({"skip":true,"model":"updated"}),
    ];
    let source = format!("{}\n \n{}\n", rows[0], rows[1]);
    fs::write(f.config.source_path.join("a.jsonl"), &source).unwrap();
    let mut digest = Sha256::new();
    digest.update(rows[0].to_string().as_bytes());
    digest.update((rows[0].to_string().len() + 1).to_string().as_bytes());
    digest.update(rows[1].to_string().as_bytes());
    digest.update(source.len().to_string().as_bytes());
    let mut legacy = legacy_state(&f, &rows, format!("{:x}", digest.finalize()));
    let id = legacy["files"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    legacy["files"][&id]["offset"] = json!(source.len());
    fs::write(&f.config.state_path, legacy.to_string()).unwrap();
    let report = collect(&f.config, &adapter).await.unwrap();
    assert_eq!(report.source_id, "legacy-installation");
    assert_eq!(report.uploaded, 0);
    assert_eq!(adapter.converted.lock().unwrap().len(), 2);
    assert_eq!(f.state()["version"], 2);
    assert_eq!(
        f.state()["files"][&id]["context"]["value"]["model"],
        "updated"
    );
    adapter.converted.lock().unwrap().clear();
    collect(&f.config, &adapter).await.unwrap();
    assert!(adapter.converted.lock().unwrap().is_empty());
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(f.config.source_path.join("a.jsonl"))
        .unwrap()
        .write_all(b"{\"content\":\"new\"}\n")
        .unwrap();
    collect(&f.config, &adapter).await.unwrap();
    let event = f.events().pop().unwrap();
    assert_eq!(event["event_id"], hash(format!("{id}:{}", source.len())));
    assert_eq!(event["model_id"], "updated");
}
#[tokio::test]
async fn incorrect_legacy_digest_never_replays_or_migrates() {
    let f = Fixture::new();
    let adapter = Stateful::default();
    let rows = [json!({"turn":"a"}), json!({"content":"old"})];
    f.write(&rows);
    // Controlled negative: raw SHA must NOT be accepted as the original v1 digest.
    let state = legacy_state(&f, &rows, hash(encode(&rows)));
    fs::write(&f.config.state_path, state.to_string()).unwrap();
    assert!(collect(&f.config, &adapter).await.is_err());
    assert!(adapter.converted.lock().unwrap().is_empty());
    assert_eq!(f.state(), state);
}
#[tokio::test]
async fn exact_wire_bytes_escaping_metadata_oversized_singleton_and_round_robin() {
    let f = Fixture::new();
    let options = CollectOptions {
        batch_target_bytes: 1800,
    };
    let mut rows_a = vec![json!({"turn":"a","header":"a"})];
    let mut rows_b = vec![json!({"turn":"b","header":"b"})];
    for n in 0..6 {
        rows_a.push(json!({"content":format!("a-{n}\n\\\"{}","λ".repeat(70))}));
        rows_b.push(json!({"content":format!("b-{n}\n\\\"{}","λ".repeat(70))}));
    }
    rows_a.push(json!({"content":"oversized".repeat(1500)}));
    f.write(&rows_a);
    fs::write(f.config.source_path.join("b.jsonl"), encode(&rows_b)).unwrap();
    assert_eq!(
        collect_with_options(&f.config, &Generic, options)
            .await
            .unwrap()
            .uploaded,
        15
    );
    let received = f.received.lock().unwrap();
    let lengths = f.wire_lengths.lock().unwrap();
    for (batch, len) in received.iter().zip(lengths.iter()) {
        assert_eq!(*len, serde_json::to_vec(batch).unwrap().len());
        assert!(
            *len <= options.batch_target_bytes || batch["events"].as_array().unwrap().len() == 1
        );
    }
    let turns: Vec<_> = received
        .iter()
        .map(|b| b["events"][0]["source_task_id"].as_str().unwrap())
        .collect();
    assert_eq!(&turns[..4], &["a", "b", "a", "b"]);
    let events = received
        .iter()
        .flat_map(|b| b["events"].as_array().unwrap())
        .collect::<Vec<_>>();
    for turn in ["a", "b"] {
        let sequence = events
            .iter()
            .filter(|e| e["source_task_id"] == turn)
            .map(|e| e["sequence"].as_i64().unwrap())
            .collect::<Vec<_>>();
        assert!(sequence.windows(2).all(|w| w[0] < w[1]));
    }
    let largest = received
        .iter()
        .max_by_key(|b| serde_json::to_vec(b).unwrap().len())
        .unwrap();
    assert_eq!(largest["events"].as_array().unwrap().len(), 1);
    assert_eq!(largest["events"][0]["raw"], rows_a.last().unwrap().clone());
}

#[tokio::test]
async fn persisted_claim_also_guards_scanned_future_prefix_against_mutation() {
    let f = Fixture::new();
    let adapter = Stateful::default();
    let rows = [
        json!({"header":"fixed"}),
        json!({"content":"pending"}),
        json!({"turn":"future"}),
    ];
    f.write(&rows);
    *f.fail_request.lock().unwrap() = Some(2);
    assert!(collect_with_options(
        &f.config,
        &adapter,
        CollectOptions {
            batch_target_bytes: 1
        }
    )
    .await
    .is_err());
    let state = f.state();
    f.write(&[rows[0].clone(), rows[1].clone(), json!({"turn":"altered"})]);
    adapter.converted.lock().unwrap().clear();
    assert!(collect(&f.config, &adapter).await.is_err());
    assert!(adapter.converted.lock().unwrap().is_empty());
    assert_eq!(f.state(), state);
}

#[tokio::test]
async fn byte_boundary_failure_snapshot_matches_original_record_before_none_update() {
    let f = Fixture::new();
    let adapter = Stateful::default();
    f.write(&[
        json!({"turn":"a","model":"original"}),
        json!({"skip":true,"model":"none-updated"}),
        json!({"content":"second"}),
    ]);
    *f.fail_request.lock().unwrap() = Some(2);
    let options = CollectOptions {
        batch_target_bytes: 1,
    };
    assert!(collect_with_options(&f.config, &adapter, options)
        .await
        .is_err());
    let cp = f.state()["files"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    assert_eq!(cp["context"]["value"]["model"], "original");
    let failed = f.received.lock().unwrap()[1]["events"].clone();
    *f.fail_request.lock().unwrap() = None;
    assert_eq!(
        collect_with_options(&f.config, &adapter, options)
            .await
            .unwrap()
            .uploaded,
        1
    );
    assert_eq!(f.received.lock().unwrap()[2]["events"], failed);
    assert_eq!(failed[0]["model_id"], "none-updated");
}

#[tokio::test]
async fn simultaneous_archive_snapshot_deduplicates_and_keeps_longest_suffix() {
    let f = Fixture::new();
    let rows = [
        json!({"turn":"a"}),
        json!({"content":"old"}),
        json!({"content":"new"}),
    ];
    fs::create_dir_all(f.config.source_path.join("archive")).unwrap();
    fs::write(
        f.config.source_path.join("archive/old.jsonl"),
        encode(&rows[..2]),
    )
    .unwrap();
    f.write(&rows);
    assert_eq!(collect(&f.config, &Generic).await.unwrap().uploaded, 3);
    let cp = f.state()["files"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    assert_eq!(cp["paths"].as_array().unwrap().len(), 2);
    assert_eq!(f.events().last().unwrap()["raw"], rows[2]);
    assert_eq!(collect(&f.config, &Generic).await.unwrap().uploaded, 0);
}

#[tokio::test]
async fn pending_scan_is_durable_without_advancing_acknowledged_cursor() {
    let f = Fixture::new();
    let adapter = Stateful::default();
    let rows = [
        json!({"header":"fixed","model":"old"}),
        json!({"content":"pending"}),
        json!({"skip":true,"model":"scanned"}),
    ];
    f.write(&rows);
    assert_eq!(collect(&f.config, &adapter).await.unwrap().uploaded, 0);
    let cp = f.state()["files"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    assert_eq!(cp["offset"], 0);
    assert!(cp["context"]["value"]["model"].is_null());
    assert_eq!(cp["scan"]["offset"], encode(&rows).len() as u64);
    assert_eq!(cp["scan"]["context"]["value"]["model"], "scanned");
    assert!(cp["pending"]["context"]["value"]["model"].is_null());
    adapter.converted.lock().unwrap().clear();
    collect(&f.config, &adapter).await.unwrap();
    assert!(adapter.converted.lock().unwrap().is_empty());
    assert_eq!(
        f.state()["files"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()["offset"],
        0
    );
}
#[tokio::test]
async fn missing_legacy_source_does_not_block_new_files_and_migrates_when_returned() {
    use sha2::{Digest, Sha256};
    let f = Fixture::new();
    let adapter = Stateful::default();
    let old = [json!({"turn":"old"}), json!({"content":"acknowledged"})];
    let mut digest = Sha256::new();
    let mut end = 0;
    for row in &old {
        end += row.to_string().len() + 1;
        digest.update(row.to_string().as_bytes());
        digest.update(end.to_string().as_bytes());
    }
    let state = legacy_state(&f, &old, format!("{:x}", digest.finalize()));
    let old_id = state["files"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    fs::write(&f.config.state_path, state.to_string()).unwrap();
    // Original old path is absent; a distinct new source must still upload normally.
    fs::write(
        f.config.source_path.join("new.jsonl"),
        encode(&[json!({"turn":"new"})]),
    )
    .unwrap();
    assert_eq!(collect(&f.config, &adapter).await.unwrap().uploaded, 1);
    assert_eq!(f.state()["files"][&old_id]["offset"], end);
    assert_eq!(f.state()["files"][&old_id]["legacy"], true);
    f.write(&old);
    adapter.converted.lock().unwrap().clear();
    assert_eq!(collect(&f.config, &adapter).await.unwrap().uploaded, 0);
    assert_eq!(adapter.converted.lock().unwrap().len(), 2);
    assert_eq!(f.state()["files"][&old_id]["legacy"], false);
    adapter.converted.lock().unwrap().clear();
    collect(&f.config, &adapter).await.unwrap();
    assert!(adapter.converted.lock().unwrap().is_empty());
}
#[tokio::test]
async fn divergent_same_header_aliases_fail_before_any_upload() {
    let f = Fixture::new();
    f.write(&[json!({"turn":"a"}), json!({"content":"first"})]);
    fs::write(
        f.config.source_path.join("b.jsonl"),
        encode(&[json!({"turn":"a"}), json!({"content":"other"})]),
    )
    .unwrap();
    assert!(collect(&f.config, &Generic)
        .await
        .unwrap_err()
        .to_string()
        .contains("aliases diverged"));
    assert!(f.events().is_empty());
    assert!(f.state()["files"].as_object().unwrap().is_empty());
}

#[tokio::test]
async fn idle_collect_does_not_rewrite_checkpoint_file() {
    let f = Fixture::new();
    f.write(&[json!({"turn":"a"}), json!({"content":"done"})]);
    collect(&f.config, &Generic).await.unwrap();
    let time = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(18_000);
    fs::File::options()
        .write(true)
        .open(&f.config.state_path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(time))
        .unwrap();
    let before = fs::metadata(&f.config.state_path)
        .unwrap()
        .modified()
        .unwrap();
    let bytes = fs::read(&f.config.state_path).unwrap();
    assert_eq!(collect(&f.config, &Generic).await.unwrap().uploaded, 0);
    assert_eq!(fs::read(&f.config.state_path).unwrap(), bytes);
    assert_eq!(
        fs::metadata(&f.config.state_path)
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
}
#[tokio::test]
async fn unsupported_snapshot_version_fails_without_converting_history() {
    let f = Fixture::new();
    let adapter = Stateful::default();
    f.write(&[json!({"turn":"a"})]);
    collect(&f.config, &adapter).await.unwrap();
    let mut state = f.state();
    let cp = state["files"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    cp["context"]["version"] = json!(99);
    fs::write(&f.config.state_path, state.to_string()).unwrap();
    adapter.converted.lock().unwrap().clear();
    assert!(collect(&f.config, &adapter)
        .await
        .unwrap_err()
        .to_string()
        .contains("context version"));
    assert!(adapter.converted.lock().unwrap().is_empty());
    assert_eq!(f.state(), state);
}
