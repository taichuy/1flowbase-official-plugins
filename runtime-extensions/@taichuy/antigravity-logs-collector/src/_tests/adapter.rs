use super::*;
use agent_logs_collector::{collect, configure};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

const TS1: &str = "2026-11-27T11:01:27Z";
const TS2: &str = "2026-11-27T11:01:44Z";
const TS3: &str = "2026-11-27T11:01:52Z";

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agy-collector-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_conv(
    gemini_home: &Path,
    surface: &str,
    conv_id: &str,
    filename: &str,
    lines: &[Value],
) -> PathBuf {
    let logs = gemini_home
        .join(surface)
        .join("brain")
        .join(conv_id)
        .join(".system_generated")
        .join("logs");
    fs::create_dir_all(&logs).unwrap();
    let path = logs.join(filename);
    let body = lines
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&path, body).unwrap();
    path
}

fn start_mock_server() -> (
    String,
    Arc<Mutex<Vec<Value>>>,
    Arc<AtomicBool>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/api/logs/v1/events", listener.local_addr().unwrap());
    let received: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let r = received.clone();
    let s = stop.clone();
    let handle = thread::spawn(move || {
        while !s.load(Ordering::Relaxed) {
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_nonblocking(false).unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let mut content_len = None;
                let mut header_end = None;
                loop {
                    let n = stream.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if header_end.is_none() {
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            header_end = Some(pos + 4);
                            let headers = String::from_utf8_lossy(&buf[..pos]);
                            for line in headers.lines() {
                                if let Some(val) = line
                                    .to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                {
                                    content_len = val.trim().parse::<usize>().ok();
                                }
                            }
                        }
                    }
                    if let (Some(he), Some(cl)) = (header_end, content_len) {
                        if buf.len() >= he + cl {
                            break;
                        }
                    }
                }
                if let Some(he) = header_end {
                    let body: Value = serde_json::from_slice(&buf[he..]).unwrap();
                    let count = body["events"].as_array().map_or(0, Vec::len);
                    r.lock().unwrap().push(body);
                    let resp = json!({
                        "data": {
                            "accepted_events": count,
                            "duplicate_events": 0,
                            "record_ids": ["00000000-0000-0000-0000-000000000001"]
                        }
                    })
                    .to_string();
                    let wire = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        resp.len(),
                        resp
                    );
                    let _ = stream.write_all(wire.as_bytes());
                }
            } else {
                thread::sleep(Duration::from_millis(5));
            }
        }
    });
    (endpoint, received, stop, handle)
}

#[test]
fn roots_prefers_transcript_full_avoids_duplicates_and_ignores_telemetry() {
    let dir = temp_dir("roots");
    let app_conv = "11111111-1111-1111-1111-111111111111";
    let ide_conv = "22222222-2222-2222-2222-222222222222";
    let step = vec![json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":TS1,"content":"hi"})];

    // Both compact and full exist -> must select ONLY transcript_full.jsonl
    write_conv(&dir, "antigravity", app_conv, "transcript.jsonl", &step);
    let full = write_conv(&dir, "antigravity", app_conv, "transcript_full.jsonl", &step);
    // Unrelated telemetry and scratch JSONL must never be selected
    write_conv(&dir, "antigravity", app_conv, "powertools-telemetry.jsonl", &step);
    let scratch = dir.join("antigravity/brain").join(app_conv).join("scratch");
    fs::create_dir_all(&scratch).unwrap();
    fs::write(scratch.join("branch-turn-1.jsonl"), "{}\n").unwrap();

    // IDE conversation with only compact transcript.jsonl -> must fall back to transcript.jsonl
    let ide_compact = write_conv(&dir, "antigravity-ide", ide_conv, "transcript.jsonl", &step);

    let adapter = AntigravityAdapter::new(dir.clone(), None);
    let roots = adapter.roots(&dir);
    assert_eq!(roots, vec![full, ide_compact]);
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn converts_turns_inclusive_usage_tools_checkpoint_and_subagent_hierarchy() {
    let dir = temp_dir("convert");
    let gemini = dir.join("gemini");
    let parent_id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let child_id = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";

    write_conv(
        &gemini,
        "antigravity",
        parent_id,
        "transcript_full.jsonl",
        &[
            json!({
                "step_index": 0,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "status": "DONE",
                "created_at": TS1,
                "content": "<USER_REQUEST>\nInspect collector\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nnow\n</ADDITIONAL_METADATA>"
            }),
            json!({
                "step_index": 1,
                "source": "MODEL",
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "created_at": TS1,
                "model": "gemini-4-neon-xhigh",
                "input_tokens": 20459,
                "cache_read_tokens": 17238,
                "output_tokens": 1397,
                "thinking": "Checking repository files",
                "content": "Launching helper subagent",
                "tool_calls": [
                    { "name": "invoke_subagent", "args": { "TypeName": "research" } }
                ]
            }),
            json!({
                "step_index": 2,
                "source": "MODEL",
                "type": "GENERIC",
                "status": "DONE",
                "created_at": TS2,
                "content": format!("Created the following subagent:\n{{\n  \"conversationId\": \"{child_id}\"\n}}")
            }),
            json!({
                "step_index": 3,
                "source": "SYSTEM",
                "type": "CHECKPOINT",
                "status": "DONE",
                "created_at": TS2,
                "input_tokens": 1000,
                "cache_read_tokens": 5000,
                "output_tokens": 200,
                "content": "# Resuming from a compaction"
            }),
            json!({
                "step_index": 4,
                "source": "MODEL",
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "created_at": TS3,
                "model": "gemini-4-neon-xhigh",
                "input_tokens": 500,
                "cache_read_tokens": 1000,
                "output_tokens": 300,
                "content": "All checks passed."
            }),
        ],
    );

    // Subagent transcript starts with SYSTEM_MESSAGE (no USER_INPUT)
    write_conv(
        &gemini,
        "antigravity",
        child_id,
        "transcript_full.jsonl",
        &[
            json!({
                "step_index": 0,
                "source": "SYSTEM",
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "created_at": TS2,
                "content": "Subagent task prompt"
            }),
            json!({
                "step_index": 1,
                "source": "MODEL",
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "created_at": TS3,
                "model": "gemini-4-carbon-high",
                "input_tokens": 7347,
                "cache_read_tokens": 0,
                "output_tokens": 573,
                "content": "Subagent findings complete."
            }),
        ],
    );

    let (endpoint, received, stop, handle) = start_mock_server();
    let cfg_path = dir.join("private/config.json");
    let config = configure(&cfg_path, endpoint, &gemini, "test-key".into()).unwrap();
    let adapter = AntigravityAdapter::new(
        gemini.clone(),
        Some(dir.join("private/subagent-parents.json")),
    );

    let report = collect(&config, &adapter).await.unwrap();
    assert_eq!(report.unattributed_files, 0);
    assert!(report.uploaded >= 12);

    // Second run must be a no-op (checkpointed)
    let second = collect(&config, &adapter).await.unwrap();
    assert_eq!(second.uploaded, 0);

    stop.store(true, Ordering::Relaxed);
    let _ = handle.join();

    let events: Vec<Value> = received
        .lock()
        .unwrap()
        .iter()
        .flat_map(|batch| batch["events"].as_array().cloned().unwrap_or_default())
        .collect();

    // User prompt stripped cleanly
    let user = events
        .iter()
        .find(|e| e["source_session_id"] == parent_id && e["kind"] == "user")
        .expect("user event");
    assert_eq!(user["content"], "Inspect collector");
    assert_eq!(user["source_task_id"], format!("{parent_id}:step:0"));

    // Usage normalized inclusively: input = 20459 + 17238 = 37697, cache_write = 0
    let first_usage = events
        .iter()
        .find(|e| e["source_session_id"] == parent_id && e["kind"] == "usage")
        .expect("first usage");
    assert_eq!(first_usage["usage"]["basis"], "delta");
    assert_eq!(first_usage["usage"]["input_tokens"], 37697);
    assert_eq!(first_usage["usage"]["input_cache_hit_tokens"], 17238);
    assert_eq!(first_usage["usage"]["cache_write_tokens"], 0);
    assert_eq!(first_usage["usage"]["output_tokens"], 1397);
    assert_eq!(first_usage["usage"]["total_tokens"], 39094);
    assert_eq!(first_usage["model_id"], "gemini-4-neon-xhigh");

    // Tool call and tool result paired with identical call_id
    let tc = events
        .iter()
        .find(|e| e["kind"] == "tool_call")
        .expect("tool_call");
    let tr = events
        .iter()
        .find(|e| e["kind"] == "tool_result")
        .expect("tool_result");
    assert_eq!(tc["name"], "invoke_subagent");
    assert_eq!(tc["call_id"], tr["call_id"]);

    // Final answer + task_end emitted on DONE response without tool calls
    assert!(events.iter().any(|e| e["source_session_id"] == parent_id
        && e["kind"] == "task_end"
        && e["phase"] == "final_answer"
        && e["content"] == "All checks passed."));

    // Subagent events attributed to synthetic root task and linked to parent task
    let child_events: Vec<&Value> = events
        .iter()
        .filter(|e| e["source_session_id"] == child_id)
        .collect();
    assert!(!child_events.is_empty());
    assert!(child_events
        .iter()
        .all(|e| e["source_task_id"] == format!("{child_id}:root")));
    assert!(child_events
        .iter()
        .all(|e| e["parent_source_task_id"] == format!("{parent_id}:step:0")));

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn stream_interrupted_user_input_does_not_split_turn_and_error_message_ends_turn() {
    let dir = temp_dir("error-turn");
    let gemini = dir.join("gemini");
    let conv_id = "cccccccc-cccc-cccc-cccc-cccccccccccc";
    write_conv(
        &gemini,
        "antigravity",
        conv_id,
        "transcript.jsonl",
        &[
            json!({
                "step_index": 0,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "status": "DONE",
                "created_at": TS1,
                "content": "<USER_REQUEST>\nRun check\n</USER_REQUEST>"
            }),
            json!({
                "step_index": 1,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "status": "DONE",
                "created_at": TS2,
                "content": "Error: The stream was interrupted"
            }),
            json!({
                "step_index": 2,
                "source": "SYSTEM",
                "type": "ERROR_MESSAGE",
                "status": "DONE",
                "created_at": TS3,
                "error": "API error: EOF",
                "content": ""
            }),
        ],
    );

    let (endpoint, received, stop, handle) = start_mock_server();
    let cfg_path = dir.join("private/config.json");
    let config = configure(&cfg_path, endpoint, &gemini, "test-key".into()).unwrap();
    let adapter = AntigravityAdapter::new(gemini.clone(), None);
    let report = collect(&config, &adapter).await.unwrap();
    assert_eq!(report.uploaded, 3);
    stop.store(true, Ordering::Relaxed);
    let _ = handle.join();

    let events: Vec<Value> = received
        .lock()
        .unwrap()
        .iter()
        .flat_map(|batch| batch["events"].as_array().cloned().unwrap_or_default())
        .collect();
    assert!(events
        .iter()
        .all(|e| e["source_task_id"] == format!("{conv_id}:step:0")));
    assert_eq!(events[1]["kind"], "context");
    assert_eq!(events[2]["kind"], "task_end");
    assert_eq!(events[2]["phase"], "error");
    assert_eq!(events[2]["content"], "API error: EOF");
    let _ = fs::remove_dir_all(&dir);
}
