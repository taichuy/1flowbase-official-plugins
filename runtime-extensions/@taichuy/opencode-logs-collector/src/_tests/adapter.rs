use super::*;
use serde_json::json;
fn context() -> OpenCodeContext {
    OpenCodeAdapter
        .create_context(&json!({"type":"collector_session","session_id":"ses_fixture"}))
        .unwrap()
}
fn record(kind: &str, properties: Value) -> Position {
    let line = json!({"id":format!("evt_{kind}"),"type":format!("session.next.{kind}"),"properties":properties});
    let bytes = serde_json::to_vec(&line).unwrap();
    Position {
        line,
        start: 0,
        end: bytes.len() as u64,
        bytes,
    }
}
fn props(extra: Value) -> Value {
    let mut base = json!({"sessionID":"ses_fixture","timestamp":1780000000000_i64});
    base.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    base
}
#[test]
fn canonical_boundaries_expand_usage_without_delta_text() {
    let mut context = context();
    let prompt = OpenCodeAdapter
        .convert(
            &record(
                "prompted",
                props(json!({"messageID":"msg_prompt","prompt":{"text":"Read fixture"}})),
            ),
            &mut context,
        )
        .unwrap();
    assert_eq!(prompt[0].source_task_id, "msg_prompt");
    assert_eq!(prompt[0].kind, Kind::User);
    assert!(OpenCodeAdapter
        .convert(
            &record("text.delta", props(json!({"delta":"partial"}))),
            &mut context
        )
        .unwrap()
        .is_empty());
    OpenCodeAdapter
        .convert(
            &record(
                "step.started",
                props(json!({"model":{"id":"gpt-6-luna","providerID":"fixture","variant":"max"}})),
            ),
            &mut context,
        )
        .unwrap();
    let reason = OpenCodeAdapter
        .convert(
            &record("reasoning.ended", props(json!({"text":"reason"}))),
            &mut context,
        )
        .unwrap();
    assert_eq!(reason[0].phase.as_deref(), Some("reasoning"));
    let result = OpenCodeAdapter.convert(&record("step.ended",props(json!({"assistantMessageID":"msg_reply","finish":"stop","tokens":{"input":7,"output":3,"cache":{"read":2,"write":1}}}))),&mut context).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].kind, Kind::Usage);
    assert_eq!(result[1].kind, Kind::TaskEnd);
    assert_eq!(result[0].source_task_id, result[1].source_task_id);
    assert_eq!(result[1].reasoning_effort.as_deref(), Some("max"));
    assert_eq!(result[0].usage.as_ref().unwrap().total_tokens, None);
}
#[test]
fn tool_and_failure_preserve_facts_without_fabricated_success() {
    let mut context = context();
    OpenCodeAdapter
        .convert(
            &record(
                "prompted",
                props(json!({"messageID":"msg_prompt","prompt":{"text":"task"}})),
            ),
            &mut context,
        )
        .unwrap();
    let call = OpenCodeAdapter
        .convert(
            &record(
                "tool.called",
                props(json!({"callID":"call_fixture","tool":"bash","input":{"command":"pwd"}})),
            ),
            &mut context,
        )
        .unwrap();
    assert_eq!(call[0].kind, Kind::ToolCall);
    assert_eq!(call[0].call_id.as_deref(), Some("call_fixture"));
    let output = OpenCodeAdapter
        .convert(
            &record(
                "tool.success",
                props(
                    json!({"callID":"call_fixture","content":[{"type":"text","text":"/fixture"}]}),
                ),
            ),
            &mut context,
        )
        .unwrap();
    assert_eq!(output[0].content.as_deref(), Some("/fixture"));
    let ended = OpenCodeAdapter
        .convert(
            &record(
                "step.ended",
                props(json!({"finish":"tool-calls","tokens":{}})),
            ),
            &mut context,
        )
        .unwrap();
    assert!(ended.iter().all(|fact| fact.kind != Kind::TaskEnd));
    let failed = OpenCodeAdapter
        .convert(
            &record(
                "step.failed",
                props(json!({"error":{"message":"provider failed"}})),
            ),
            &mut context,
        )
        .unwrap();
    assert_eq!(failed[0].kind, Kind::TaskEnd);
    assert_eq!(failed[0].phase.as_deref(), Some("error"));
    assert_eq!(failed[0].content.as_deref(), Some("provider failed"));
    assert_eq!(
        failed[0].raw["properties"]["error"]["message"],
        "provider failed"
    );
}
#[test]
fn rejects_foreign_session_missing_timestamp_and_header() {
    assert!(OpenCodeAdapter
        .create_context(&json!({"type":"session","id":"wrong"}))
        .is_err());
    let mut context = context();
    assert!(OpenCodeAdapter
        .convert(
            &record(
                "prompted",
                json!({"sessionID":"other","timestamp":1780000000000_i64})
            ),
            &mut context
        )
        .is_err());
    assert!(OpenCodeAdapter
        .convert(
            &record(
                "prompted",
                json!({"sessionID":"ses_fixture","messageID":"msg_fixture"})
            ),
            &mut context
        )
        .is_err());
}
#[test]
fn embedded_plugin_quotes_paths_as_json_literals() {
    let rendered = hooks::render(
        std::path::Path::new("/tmp/collector's binary"),
        std::path::Path::new("/tmp/config \"quoted\".json"),
    )
    .unwrap();
    assert!(rendered.contains("const binary = \"/tmp/collector's binary\""));
    assert!(rendered.contains("spawnSync"));
    assert!(!rendered.contains("__COLLECTOR_CONFIG__"));
    assert!(!rendered.contains("shell: true"));
}

#[test]
fn native_iso_timestamp_matches_effect_datetime_json() {
    let mut context = context();
    let facts = OpenCodeAdapter
        .convert(
            &record(
                "prompted",
                serde_json::json!({
                    "sessionID":"ses_fixture", "timestamp":"2026-10-11T01:02:03.123Z",
                    "messageID":"msg_fixture","prompt":{"text":"ISO source time"}
                }),
            ),
            &mut context,
        )
        .unwrap();
    assert_eq!(facts[0].occurred_at, "2026-10-11T01:02:03.123Z");
}
#[test]
fn fixture_replays_complete_native_sequence() {
    let mut context = context();
    let mut kinds = Vec::new();
    for (index, line) in include_str!("fixtures/events.jsonl").lines().enumerate() {
        let bytes = line.as_bytes().to_vec();
        let position = Position {
            line: serde_json::from_str(line).unwrap(),
            start: index as u64,
            end: index as u64 + 1,
            bytes,
        };
        kinds.extend(
            OpenCodeAdapter
                .convert(&position, &mut context)
                .unwrap()
                .into_iter()
                .map(|event| event.kind),
        );
    }
    assert_eq!(
        kinds,
        vec![
            Kind::User,
            Kind::Context,
            Kind::Assistant,
            Kind::ToolCall,
            Kind::ToolResult,
            Kind::Usage,
            Kind::Context,
            Kind::Assistant,
            Kind::Usage,
            Kind::TaskEnd
        ]
    );
}

struct InstallationFixture(std::path::PathBuf);
impl InstallationFixture {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "opencode-collector-test-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for InstallationFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn registration_preserves_user_config_and_other_plugins() {
    let fixture = InstallationFixture::new();
    let home = fixture.0.join("client");
    std::fs::create_dir_all(home.join("plugins")).unwrap();
    let user_config = home.join("opencode.jsonc");
    std::fs::write(&user_config, r#"// user-owned\n{"model":"fixture/model"}"#).unwrap();
    let other = home.join("plugins/other.js");
    std::fs::write(&other, "export default async () => ({})").unwrap();
    let config = fixture.0.join("collector/config.json");
    agent_logs_collector::configure(
        &config,
        "http://127.0.0.1:7800/agent-logs".into(),
        &fixture.0.join("sessions"),
        "synthetic-test-key".into(),
    )
    .unwrap();
    hooks::register(&config, &home, true).unwrap();
    hooks::register(&config, &home, true).unwrap();
    let loadable = || {
        std::fs::read_dir(home.join("plugins"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "js"))
            .collect::<Vec<_>>()
    };
    let installed = loadable();
    assert_eq!(installed.len(), 2);
    let owned = installed
        .iter()
        .find(|path| **path != other)
        .unwrap()
        .clone();
    hooks::register(&config, &home, false).unwrap();
    hooks::register(&config, &home, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(user_config).unwrap(),
        r#"// user-owned\n{"model":"fixture/model"}"#
    );
    assert!(other.is_file());
    assert!(
        !owned.exists(),
        "uninstall removes the owned loadable plugin"
    );
    assert_eq!(loadable(), vec![other]);
}
#[cfg(unix)]
#[test]
fn registration_rejects_symlink_client_root() {
    let fixture = InstallationFixture::new();
    let target = fixture.0.join("target");
    std::fs::create_dir(&target).unwrap();
    let linked = fixture.0.join("linked");
    std::os::unix::fs::symlink(&target, &linked).unwrap();
    assert!(hooks::register(&fixture.0.join("config.json"), &linked, false).is_err());
    assert!(std::fs::read_dir(target).unwrap().next().is_none());
}

fn v1(kind: &str, properties: Value) -> Position {
    let line = json!({"id":"evt_v1","type":kind,"properties":properties});
    let bytes = serde_json::to_vec(&line).unwrap();
    Position {
        line,
        start: 0,
        end: bytes.len() as u64,
        bytes,
    }
}
fn v1_fixture() -> Vec<Position> {
    include_str!("fixtures/v1-events.jsonl")
        .lines()
        .enumerate()
        .map(|(index, line)| Position {
            line: serde_json::from_str(line).unwrap(),
            start: index as u64,
            end: index as u64 + 1,
            bytes: line.as_bytes().to_vec(),
        })
        .collect()
}
#[test]
fn default_v1_preserves_final_text_and_tool_continuation_after_checkpoint_restore() {
    let rows = v1_fixture();
    let mut context = context();
    let mut all = Vec::new();
    for record in &rows[..5] {
        all.extend(OpenCodeAdapter.convert(record, &mut context).unwrap());
    }
    let checkpoint = serde_json::to_vec(&context).unwrap();
    let mut restored: OpenCodeContext = serde_json::from_slice(&checkpoint).unwrap();
    assert!(
        OpenCodeAdapter
            .convert(&rows[4], &mut restored)
            .unwrap()
            .is_empty(),
        "running tool retry is deduplicated"
    );
    for record in &rows[5..] {
        all.extend(OpenCodeAdapter.convert(record, &mut restored).unwrap());
    }
    assert_eq!(
        all.iter()
            .filter(|fact| fact.kind == Kind::ToolCall)
            .count(),
        1
    );
    assert_eq!(
        all.iter()
            .filter(|fact| fact.kind == Kind::ToolResult)
            .count(),
        1
    );
    assert_eq!(
        all.iter().filter(|fact| fact.kind == Kind::TaskEnd).count(),
        1
    );
    assert_eq!(
        all.iter().filter(|fact| fact.kind == Kind::Usage).count(),
        2
    );
    let final_fact = all.last().unwrap();
    assert_eq!(final_fact.kind, Kind::TaskEnd);
    assert_eq!(final_fact.phase.as_deref(), Some("final_answer"));
    assert_eq!(final_fact.content.as_deref(), Some("fixture contents"));
    assert!(all.iter().all(|fact| fact.source_task_id == "msg_user"
        || (fact.kind == Kind::Context && fact.source_task_id.is_empty())));
    assert!(!all
        .iter()
        .any(|fact| fact.content.as_deref() == Some("partial")));
    assert!(OpenCodeAdapter
        .convert(rows.last().unwrap(), &mut restored)
        .unwrap()
        .is_empty());
}
#[test]
fn default_v1_completed_tool_contains_call_even_without_running_notification() {
    let rows = v1_fixture();
    let mut context = context();
    OpenCodeAdapter.convert(&rows[0], &mut context).unwrap();
    OpenCodeAdapter.convert(&rows[2], &mut context).unwrap();
    let facts = OpenCodeAdapter.convert(&rows[5], &mut context).unwrap();
    assert_eq!(
        facts
            .iter()
            .map(|fact| fact.kind.clone())
            .collect::<Vec<_>>(),
        vec![Kind::ToolCall, Kind::ToolResult]
    );
    assert_eq!(facts[0].content.as_deref(), Some("{\"path\":\"/fixture\"}"));
}
#[test]
fn default_v1_rejects_foreign_parts_and_conflicting_completed_text() {
    let rows = v1_fixture();
    let mut context = context();
    OpenCodeAdapter.convert(&rows[0], &mut context).unwrap();
    OpenCodeAdapter.convert(&rows[7], &mut context).unwrap();
    OpenCodeAdapter.convert(&rows[9], &mut context).unwrap();
    let mut changed = rows[9].line["properties"].clone();
    changed["part"]["text"] = json!("conflict");
    assert!(OpenCodeAdapter
        .convert(&v1("message.part.updated", changed), &mut context)
        .is_err());
    let mut foreign = rows[9].line["properties"].clone();
    foreign["part"]["sessionID"] = json!("foreign");
    assert!(OpenCodeAdapter
        .convert(&v1("message.part.updated", foreign), &mut context)
        .is_err());
}
#[test]
fn next_real_user_retains_user_aliases_and_prunes_settled_assistants() {
    let rows = v1_fixture();
    let mut context = context();
    for record in rows {
        OpenCodeAdapter.convert(&record, &mut context).unwrap();
    }
    let fresh = v1(
        "message.updated",
        json!({"sessionID":"ses_fixture","info":{
            "sessionID":"ses_fixture","id":"msg_next","role":"user","time":{"created":1780000000010_i64}
        }}),
    );
    OpenCodeAdapter.convert(&fresh, &mut context).unwrap();
    assert_eq!(
        context.turn.as_deref(),
        Some("msg_user"),
        "metadata alone is not a human prompt"
    );
    let part = v1(
        "message.part.updated",
        json!({"part":{
            "sessionID":"ses_fixture","id":"prt_next","messageID":"msg_next","type":"text","text":"next"
        }}),
    );
    OpenCodeAdapter.convert(&part, &mut context).unwrap();
    assert_eq!(context.turn.as_deref(), Some("msg_next"));
    assert!(context.messages.contains_key("msg_user"));
    assert!(context
        .messages
        .values()
        .all(|message| message.role == "user" || !message.completed));
}

#[test]
fn user_part_without_end_or_notification_time_uses_message_creation() {
    let rows = v1_fixture();
    let mut context = context();
    OpenCodeAdapter.convert(&rows[0], &mut context).unwrap();
    let mut properties = rows[1].line["properties"].clone();
    properties.as_object_mut().unwrap().remove("time");
    let facts = OpenCodeAdapter
        .convert(&v1("message.part.updated", properties), &mut context)
        .unwrap();
    assert_eq!(facts[0].kind, Kind::User);
    assert_eq!(
        facts[0].occurred_at,
        timestamp(&json!(1780000000000_i64)).unwrap()
    );
}
#[test]
fn v2_final_projection_uses_full_value_text_without_tool_step_completion() {
    let mut context = context();
    OpenCodeAdapter
        .convert(
            &record(
                "prompted",
                props(json!({"messageID":"msg_prompt","prompt":{"text":"task"}})),
            ),
            &mut context,
        )
        .unwrap();
    OpenCodeAdapter
        .convert(
            &record(
                "step.started",
                props(json!({"model":{"id":"fixture","providerID":"gateway"}})),
            ),
            &mut context,
        )
        .unwrap();
    OpenCodeAdapter
        .convert(
            &record("text.ended", props(json!({"text":"actual answer"}))),
            &mut context,
        )
        .unwrap();
    let facts = OpenCodeAdapter
        .convert(
            &record("step.ended", props(json!({"finish":"stop","tokens":{}}))),
            &mut context,
        )
        .unwrap();
    assert_eq!(facts.last().unwrap().phase.as_deref(), Some("final_answer"));
    assert_eq!(
        facts.last().unwrap().content.as_deref(),
        Some("actual answer")
    );
}

#[test]
fn inclusive_input_preserves_unknown_and_rejects_overflow() {
    assert_eq!(inclusive_input(Some(3), Some(5), Some(7)), Some(15));
    assert_eq!(inclusive_input(None, Some(5), Some(7)), None);
    assert_eq!(inclusive_input(Some(3), None, Some(7)), None);
    assert_eq!(inclusive_input(Some(3), Some(5), None), None);
    assert_eq!(inclusive_input(Some(i64::MAX), Some(1), Some(0)), None);
    assert_eq!(
        inclusive_input(Some(9_007_199_254_740_991), Some(1), Some(0)),
        None
    );
}

#[test]
fn opencode_v1_and_v2_usage_includes_disjoint_cache_tokens() {
    let tokens = json!({"input":3,"output":2,"cache":{"read":5,"write":7}});
    let usage = native_usage(&tokens, Some("msg_usage".into())).unwrap();
    assert_eq!(usage.input_tokens, Some(15));
    assert_eq!(usage.input_cache_hit_tokens, Some(5));
    assert_eq!(usage.cache_write_tokens, Some(7));
    assert_eq!(
        native_usage(&json!({"input":3,"cache":{"read":5}}), None)
            .unwrap()
            .input_tokens,
        None
    );
    let mut context = context();
    OpenCodeAdapter
        .convert(
            &record(
                "prompted",
                props(json!({"messageID":"msg_prompt","prompt":{"text":"usage"}})),
            ),
            &mut context,
        )
        .unwrap();
    let facts = OpenCodeAdapter
        .convert(
            &record(
                "step.ended",
                props(json!({"finish":"tool-calls","tokens":tokens})),
            ),
            &mut context,
        )
        .unwrap();
    assert_eq!(facts[0].usage.as_ref().unwrap().input_tokens, Some(15));
}
#[test]
fn default_v1_accepts_native_nested_session_identity() {
    let rows = v1_fixture();
    let mut context = context();
    let mut properties = rows[0].line["properties"].clone();
    properties.as_object_mut().unwrap().remove("sessionID");
    OpenCodeAdapter
        .convert(&v1("message.updated", properties), &mut context)
        .unwrap();
    let mut properties = rows[1].line["properties"].clone();
    properties.as_object_mut().unwrap().remove("sessionID");
    let facts = OpenCodeAdapter
        .convert(&v1("message.part.updated", properties), &mut context)
        .unwrap();
    assert_eq!(facts[0].kind, Kind::User);
}

#[test]
fn default_v1_terminal_error_requires_completed_assistant_and_preserves_error_envelope() {
    let rows = v1_fixture();
    for (name, phase) in [("APIError", "error"), ("MessageAbortedError", "cancelled")] {
        let mut context = context();
        OpenCodeAdapter.convert(&rows[0], &mut context).unwrap();
        OpenCodeAdapter.convert(&rows[2], &mut context).unwrap();
        // Already streamed text must not turn a failed task into a final answer.
        let part = v1(
            "message.part.updated",
            json!({"part":{
                "id":"part_partial","sessionID":"ses_fixture","messageID":"msg_assistant",
                "type":"text","text":"unfinished answer","time":{"start":1780000000001_i64,"end":1780000000002_i64}
            }}),
        );
        let assistant_id = rows[2].line["properties"]["info"]["id"].as_str().unwrap();
        let mut part = part;
        part.line["properties"]["part"]["messageID"] = json!(assistant_id);
        OpenCodeAdapter.convert(&part, &mut context).unwrap();
        let mut native = rows[2].line["properties"]["info"].clone();
        native["error"] = json!({"name":name,"data":{"message":"terminal failure","statusCode":400,"isRetryable":false}});
        let pending = v1("message.updated", json!({"info":native.clone()}));
        let pending_facts = OpenCodeAdapter.convert(&pending, &mut context).unwrap();
        assert!(pending_facts.iter().all(|fact| fact.kind != Kind::TaskEnd));
        native["time"]["completed"] = json!(1780000000003_i64);
        let completed = v1("message.updated", json!({"info":native}));
        let facts = OpenCodeAdapter.convert(&completed, &mut context).unwrap();
        let terminal = facts.last().unwrap();
        assert_eq!(terminal.kind, Kind::TaskEnd);
        assert_eq!(terminal.phase.as_deref(), Some(phase));
        assert_eq!(terminal.content.as_deref(), Some("terminal failure"));
        assert_eq!(terminal.raw, completed.line);
        assert_eq!(terminal.source_task_id, "msg_user");
        assert!(OpenCodeAdapter
            .convert(&completed, &mut context)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn default_v1_recoverable_completion_and_idle_do_not_end_task() {
    let rows = v1_fixture();
    let mut context = context();
    OpenCodeAdapter.convert(&rows[0], &mut context).unwrap();
    let mut native = rows[2].line["properties"]["info"].clone();
    native["time"]["completed"] = json!(1780000000003_i64);
    // Recoverable overflow causes cleanup without an assistant error or stop finish.
    native.as_object_mut().unwrap().remove("error");
    native.as_object_mut().unwrap().remove("finish");
    let facts = OpenCodeAdapter
        .convert(&v1("message.updated", json!({"info":native})), &mut context)
        .unwrap();
    assert!(facts.iter().all(|fact| fact.kind != Kind::TaskEnd));
    assert!(OpenCodeAdapter
        .convert(
            &v1("session.idle", json!({"sessionID":"ses_fixture"})),
            &mut context
        )
        .unwrap()
        .is_empty());
}

#[test]
fn synthetic_and_mixed_native_parts_preserve_real_turn_parent_and_checkpoint() {
    let mut ctx = context();
    let mut facts = Vec::new();
    for (index, line) in include_str!("fixtures/source-classification.jsonl")
        .lines()
        .enumerate()
    {
        let raw: Value = serde_json::from_str(line).unwrap();
        let record = Position {
            line: raw.clone(),
            start: 0,
            end: line.len() as u64,
            bytes: line.as_bytes().to_vec(),
        };
        let converted = OpenCodeAdapter.convert(&record, &mut ctx).unwrap();
        assert!(converted.iter().all(|fact| fact.raw == raw));
        if let Some(first) = converted.first() {
            assert!(
                converted
                    .iter()
                    .all(|fact| fact.source_task_id == first.source_task_id),
                "SDK requires one source record to have one attribution"
            );
        }
        facts.extend(converted);
        if index == 9 {
            assert_eq!(
                ctx.turn.as_deref(),
                Some("real"),
                "synthetic alone cannot start a real turn"
            );
            ctx = serde_json::from_slice(&serde_json::to_vec(&ctx).unwrap()).unwrap();
        }
    }
    let users: Vec<_> = facts
        .iter()
        .filter(|fact| fact.kind == Kind::User)
        .collect();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].source_task_id, "real");
    assert_eq!(users[1].source_task_id, "mixed");
    assert_eq!(
        facts
            .iter()
            .filter(|fact| fact.phase.as_deref() == Some("synthetic") && fact.kind == Kind::Context)
            .count(),
        2
    );
    assert!(!facts.iter().any(|fact| fact.source_task_id == "synthetic"));
    assert!(!facts
        .iter()
        .any(|fact| fact.content.as_deref() == Some("unfinished prefix")));
    let finals: Vec<_> = facts
        .iter()
        .filter(|fact| fact.kind == Kind::TaskEnd)
        .collect();
    assert_eq!(finals.len(), 2);
    assert_eq!(finals[0].source_task_id, "mixed");
    assert_eq!(finals[0].content.as_deref(), Some("second answer"));
    assert_eq!(finals[1].source_task_id, "real");
    assert_eq!(finals[1].content.as_deref(), Some("original answer"));
    assert_eq!(ctx.turn.as_deref(), Some("mixed"));
}

#[test]
fn unseen_native_parent_keeps_supported_assistant_facts_without_inventing_user() {
    let mut ctx = context();
    let info = v1(
        "message.updated",
        json!({"info":{
            "sessionID":"ses_fixture","id":"assistant-mid-session","role":"assistant",
            "parentID":"native-unseen-parent","time":{"created":1780000000000_i64}
        }}),
    );
    let meta = OpenCodeAdapter.convert(&info, &mut ctx).unwrap();
    assert_eq!(meta[0].source_task_id, "native-unseen-parent");
    assert!(ctx.turn.is_none());
    let text = v1(
        "message.part.updated",
        json!({"part":{
            "sessionID":"ses_fixture","id":"part-mid-session","messageID":"assistant-mid-session",
            "type":"text","text":"observed answer","time":{"end":1780000000001_i64}
        }}),
    );
    let facts = OpenCodeAdapter.convert(&text, &mut ctx).unwrap();
    assert_eq!(facts[0].kind, Kind::Assistant);
    assert_eq!(facts[0].source_task_id, "native-unseen-parent");
    assert!(!facts.iter().any(|fact| fact.kind == Kind::User));
}

#[test]
fn official_system_hook_snapshot_is_system_and_preserves_native_array() {
    let mut ctx = context();
    OpenCodeAdapter
        .convert(
            &record(
                "prompted",
                props(json!({
        "messageID":"real","prompt":{"text":"input"}})),
            ),
            &mut ctx,
        )
        .unwrap();
    let raw = json!({"id":"native-hook","type":"collector.system.transform","properties":{
        "sessionID":"ses_fixture","timestamp":1780000000000_i64,
        "system":["native provider instructions","native project instructions"],
        "model":{"id":"native-model","providerID":"gateway"}
    }});
    let position = Position {
        line: raw.clone(),
        start: 0,
        end: 1000,
        bytes: vec![],
    };
    let facts = OpenCodeAdapter.convert(&position, &mut ctx).unwrap();
    assert_eq!(facts[0].kind, Kind::System);
    assert_eq!(facts[0].source_task_id, "real");
    assert_eq!(
        facts[0].content.as_deref(),
        Some("native provider instructions\nnative project instructions")
    );
    assert_eq!(facts[0].phase.as_deref(), Some("request_system_snapshot"));
    assert_eq!(facts[0].raw, raw);
}

#[test]
fn entirely_synthetic_initial_message_cannot_establish_human_turn() {
    let mut ctx = context();
    let info = v1(
        "message.updated",
        json!({"info":{
            "sessionID":"ses_fixture","id":"synthetic-only","role":"user",
            "time":{"created":1780000000000_i64}
        }}),
    );
    let meta = OpenCodeAdapter.convert(&info, &mut ctx).unwrap();
    assert!(meta[0].source_task_id.is_empty());
    let part = v1(
        "message.part.updated",
        json!({"part":{
            "sessionID":"ses_fixture","id":"synthetic-only-part","messageID":"synthetic-only",
            "type":"text","text":"runtime prompt","synthetic":true
        }}),
    );
    let facts = OpenCodeAdapter.convert(&part, &mut ctx).unwrap();
    assert_eq!(facts[0].kind, Kind::Context);
    assert!(facts[0].source_task_id.is_empty());
    assert!(ctx.turn.is_none());
}

#[test]
fn late_old_user_part_does_not_steal_active_steering_turn() {
    let mut ctx = context();
    for (id, at) in [("old", 1780000000000_i64), ("new", 1780000000100_i64)] {
        let info = v1(
            "message.updated",
            json!({"info":{
                "sessionID":"ses_fixture","id":id,"role":"user","time":{"created":at}
            }}),
        );
        OpenCodeAdapter.convert(&info, &mut ctx).unwrap();
    }
    for id in ["new", "old"] {
        let part = v1(
            "message.part.updated",
            json!({"part":{
                "sessionID":"ses_fixture","id":format!("part-{id}"),"messageID":id,
                "type":"text","text":id
            }}),
        );
        let facts = OpenCodeAdapter.convert(&part, &mut ctx).unwrap();
        assert_eq!(facts[0].source_task_id, id);
        assert_eq!(ctx.turn.as_deref(), Some("new"));
    }
}
