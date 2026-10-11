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
    assert_eq!(failed[0].kind, Kind::Context);
    assert_eq!(failed[0].phase.as_deref(), Some("error"));
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
    let installed = std::fs::read_dir(home.join("plugins")).unwrap().count();
    assert_eq!(installed, 2);
    hooks::register(&config, &home, false).unwrap();
    hooks::register(&config, &home, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(user_config).unwrap(),
        r#"// user-owned\n{"model":"fixture/model"}"#
    );
    assert!(other.is_file());
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
