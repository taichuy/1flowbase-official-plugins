use super::*;
use serde_json::json;
const TS: &str = "2026-10-11T08:00:00Z";
fn adapter() -> ClaudeAdapter {
    ClaudeAdapter {
        client_home: PathBuf::from("/synthetic/.claude"),
    }
}
fn record(row: Value) -> Position {
    Position {
        line: row,
        start: 50,
        end: 10000,
        bytes: vec![],
    }
}
fn user(id: &str, content: Value) -> Value {
    json!({"type":"user","sessionId":"session","uuid":id,"timestamp":TS,"message":{"role":"user","content":content}})
}
fn convert(a: &ClaudeAdapter, c: &mut ClaudeContext, row: Value) -> Vec<AgentLogEvent> {
    a.convert(&record(row), c).unwrap()
}
#[test]
fn every_native_block_and_usage_share_the_real_prompt_turn() {
    let a = adapter();
    let prompt = user("prompt-uuid", json!("please read a file"));
    let mut c = a.create_context(&prompt).unwrap();
    convert(&a, &mut c, prompt);
    let row = json!({"type":"assistant","sessionId":"session","uuid":"assistant-a","parentUuid":"prompt-uuid","timestamp":TS,"message":{"id":"response","model":"source-model","role":"assistant","content":[{"type":"thinking","thinking":"reasoning","signature":"native-signature"},{"type":"text","text":"checking"},{"type":"tool_use","id":"call-a","name":"Read","input":{"file_path":"/fixture/a"}},{"type":"tool_use","id":"call-b","name":"Bash","input":{"command":"true"}},{"type":"redacted_thinking","data":"private-native-data"}],"usage":{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":30,"cache_creation_input_tokens":10}}});
    let facts = convert(&a, &mut c, row.clone());
    assert_eq!(facts.len(), 6);
    assert!(facts.iter().all(|e| e.source_task_id == "prompt-uuid"
        && e.source_session_id == "session"
        && e.raw == row));
    assert_eq!(facts[0].kind, Kind::Assistant);
    assert_eq!(facts[0].phase.as_deref(), Some("reasoning"));
    assert_eq!(facts[2].call_id.as_deref(), Some("call-a"));
    assert_eq!(facts[3].call_id.as_deref(), Some("call-b"));
    assert_eq!(facts[4].phase.as_deref(), Some("redacted_reasoning"));
    let usage = facts[5].usage.as_ref().unwrap();
    assert_eq!(usage.basis, AgentLogUsageBasis::Cumulative);
    assert_eq!(usage.response_id.as_deref(), Some("response"));
    assert_eq!(usage.input_cache_hit_tokens, Some(30));
    assert_eq!(usage.cache_write_tokens, Some(10));
    assert_eq!(usage.total_tokens, None);
    let result = user(
        "tool-result-uuid",
        json!([{"type":"tool_result","tool_use_id":"call-a","content":[{"type":"text","text":"file"}]},{"type":"tool_result","tool_use_id":"call-b","content":"done","is_error":true}]),
    );
    let facts = convert(&a, &mut c, result);
    assert_eq!(facts.len(), 2);
    assert!(facts
        .iter()
        .all(|e| e.source_task_id == "prompt-uuid" && e.kind == Kind::ToolResult));
    assert_eq!(facts[1].phase.as_deref(), Some("error"));
    let facts = convert(&a, &mut c, user("followup-uuid", json!("continue")));
    assert_eq!(facts[0].source_task_id, "followup-uuid");
    assert_eq!(facts[0].model_id, None);
}
#[test]
fn initial_snapshot_meta_and_compact_context_do_not_create_fake_turns() {
    let a = adapter();
    let mut c = a
        .create_context(&json!({"type":"file-history-snapshot","messageId":"snapshot"}))
        .unwrap();
    assert!(convert(
        &a,
        &mut c,
        json!({"type":"file-history-snapshot","messageId":"snapshot"})
    )
    .is_empty());
    let mut meta = user("meta", json!("injected"));
    meta["isMeta"] = json!(true);
    assert!(convert(&a, &mut c, meta)[0].source_task_id.is_empty());
    convert(&a, &mut c, user("real", json!("real prompt")));
    let mut compact = user("summary", json!("summary"));
    compact["isCompactSummary"] = json!(true);
    let fact = &convert(&a, &mut c, compact)[0];
    assert!(fact.inherited);
    assert_eq!(fact.source_task_id, "real");
    assert!(a.create_context(&json!({"type":"unsupported"})).is_err());
}
#[test]
fn terminal_requires_native_proof_and_transcript_timestamp() {
    let a = adapter();
    let prompt = user("turn", json!("prompt"));
    let mut c = a.create_context(&prompt).unwrap();
    convert(&a, &mut c, prompt);
    let mut row = json!({"type":"assistant","sessionId":"session","timestamp":TS,"message":{"content":[{"type":"text","text":"done"}],"stop_reason":null}});
    assert!(!convert(&a, &mut c, row.clone())
        .iter()
        .any(|e| e.kind == Kind::TaskEnd));
    row["message"]["stop_reason"] = json!("end_turn");
    assert!(convert(&a, &mut c, row.clone())
        .iter()
        .any(|e| e.kind == Kind::TaskEnd));
    row["timestamp"] = json!("bad");
    assert!(a.convert(&record(row), &mut c).is_err());
    let facts = convert(
        &a,
        &mut c,
        json!({"type":"system","subtype":"turn_duration","sessionId":"session","timestamp":TS,"durationMs":120}),
    );
    assert_eq!(facts[0].kind, Kind::TaskEnd);
    assert_eq!(facts[0].source_task_id, "turn");
}
#[test]
fn custom_file_and_native_home_roots_remain_targeted() {
    let a = adapter();
    assert_eq!(
        a.roots(&a.client_home),
        vec![a.client_home.join("projects")]
    );
    assert_eq!(
        a.roots(Path::new("/fixture/current.jsonl")),
        vec![PathBuf::from("/fixture/current.jsonl")]
    );
}
