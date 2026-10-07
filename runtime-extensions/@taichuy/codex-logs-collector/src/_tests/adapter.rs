use super::*;
use serde_json::json;
const TS: &str = "2026-10-07T08:00:00.123Z";
fn row(kind: &str, payload: Value) -> Value {
    json!({"timestamp":TS,"type":kind,"payload":payload})
}
fn meta() -> Value {
    row(
        "session_meta",
        json!({"id":"child-session","model_provider":"source-provider","subagent_history_start_ordinal":4}),
    )
}
fn adapter() -> CodexAdapter {
    CodexAdapter {
        codex_home: PathBuf::from("/synthetic/codex"),
    }
}
fn convert(adapter: &CodexAdapter, context: &mut CodexContext, value: Value) -> AgentLogEvent {
    adapter
        .convert(
            &Position {
                line: value,
                start: 0,
                end: 1,
                bytes: Vec::new(),
            },
            context,
        )
        .unwrap()
        .unwrap()
}
#[test]
fn inherited_history_waits_for_child_turn_and_preserves_raw() {
    let a = adapter();
    let mut context = a.create_context(&meta()).unwrap();
    let inherited = json!({"timestamp":TS,"type":"turn_context","payload":{"turn_id":"copied-parent-turn","model":"parent-model"},"ordinal":2});
    let event = convert(&a, &mut context, inherited.clone());
    assert!(event.inherited);
    assert!(event.source_task_id.is_empty());
    assert_eq!(event.raw, inherited);
    assert_eq!(event.model_id, None);
    let child = convert(
        &a,
        &mut context,
        row(
            "turn_context",
            json!({"turn_id":"child-turn","root_turn_id":"root-turn","model":"source-model"}),
        ),
    );
    assert_eq!(child.source_task_id, "child-turn");
    assert_eq!(child.parent_source_task_id.as_deref(), Some("root-turn"));
    assert_eq!(child.model_id.as_deref(), Some("source-model"));
    assert_eq!(child.provider_code.as_deref(), Some("source-provider"));
}
#[test]
fn typed_response_delta_never_infers_increment_from_cumulative() {
    let a = adapter();
    let mut context = a.create_context(&meta()).unwrap();
    let source = row(
        "token_usage_record",
        json!({"turn_id":"child-turn","root_turn_id":"root-turn","response_id":"response-a","usage":{"input_tokens":12,"cached_input_tokens":2,"cache_write_input_tokens":1,"output_tokens":4,"total_tokens":16},"turn_token_usage":{"total_tokens":80},"thread_token_usage":{"total_tokens":500}}),
    );
    let event = convert(&a, &mut context, source.clone());
    assert_eq!(event.raw, source);
    assert_eq!(event.source_task_id, "child-turn");
    let usage = event.usage.unwrap();
    assert!(matches!(usage.basis, AgentLogUsageBasis::Delta));
    assert_eq!(usage.total_tokens, Some(16));
    assert_eq!(usage.response_id.as_deref(), Some("response-a"));
    assert_eq!(usage.input_cache_hit_tokens, Some(2));
    assert_eq!(usage.cache_write_tokens, Some(1));
    let historical = convert(
        &a,
        &mut context,
        row(
            "event_msg",
            json!({"type":"token_count","info":{"total_token_usage":{"total_tokens":700},"last_token_usage":{"total_tokens":200}}}),
        ),
    );
    let usage = historical.usage.unwrap();
    assert!(matches!(usage.basis, AgentLogUsageBasis::Cumulative));
    assert_eq!(usage.total_tokens, Some(700));
    assert_eq!(usage.response_id, None);
}
#[test]
fn explicit_completion_aliases_and_cancelled_are_never_guessed() {
    let a = adapter();
    for alias in ["task_complete", "turn_complete"] {
        let mut context = a.create_context(&meta()).unwrap();
        let ordinary = convert(
            &a,
            &mut context,
            row(
                "response_item",
                json!({"type":"message","role":"assistant","content":[{"text":"ordinary answer"}]}),
            ),
        );
        assert_eq!(ordinary.phase, None);
        assert!(ordinary.source_task_id.is_empty());
        convert(
            &a,
            &mut context,
            row("turn_context", json!({"turn_id":"real-turn"})),
        );
        let source = row(
            "event_msg",
            json!({"type":alias,"turn_id":"real-turn","last_agent_message":"  declared final\ntext  "}),
        );
        let event = convert(&a, &mut context, source.clone());
        assert_eq!(event.kind, Kind::TaskEnd);
        assert_eq!(event.phase.as_deref(), Some("final_answer"));
        assert_eq!(event.content.as_deref(), Some("  declared final\ntext  "));
        assert_eq!(event.raw, source);
        assert_eq!(context.turn, None);
        for value in [Value::Null, json!(""), json!("   "), json!(17)] {
            let event = convert(
                &a,
                &mut context,
                row(
                    "event_msg",
                    json!({"type":alias,"turn_id":"real-turn","last_agent_message":value}),
                ),
            );
            assert_eq!(event.phase, None);
            assert_eq!(event.content, None);
        }
    }
    let mut context = a.create_context(&meta()).unwrap();
    let cancelled = convert(
        &a,
        &mut context,
        row(
            "event_msg",
            json!({"type":"turn_aborted","turn_id":"cancelled-turn","last_agent_message":"not final","reason":"interrupted"}),
        ),
    );
    assert_eq!(cancelled.phase.as_deref(), Some("cancelled"));
    assert_eq!(cancelled.content, None);
    assert_eq!(cancelled.source_task_id, "cancelled-turn");
    assert_eq!(context.turn, None);
}
#[test]
fn final_phase_tool_arguments_and_mirrors_remain_distinct() {
    let a = adapter();
    let mut context = a.create_context(&meta()).unwrap();
    let final_message = convert(
        &a,
        &mut context,
        row(
            "response_item",
            json!({"type":"message","role":"assistant","phase":"final_answer","content":[{"text":"final"}],"internal_chat_message_metadata_passthrough":{"turn_id":"source-turn"}}),
        ),
    );
    assert_eq!(final_message.kind, Kind::Assistant);
    assert_eq!(final_message.source_task_id, "source-turn");
    assert_eq!(final_message.phase.as_deref(), Some("final_answer"));
    let mirror = convert(
        &a,
        &mut context,
        row(
            "event_msg",
            json!({"type":"agent_message","message":"final","phase":"final_answer"}),
        ),
    );
    assert_eq!(mirror.kind, Kind::Context);
    let source = row(
        "response_item",
        json!({"type":"function_call","name":"example","call_id":"call-a","arguments":"{\"exact\":true}"}),
    );
    let call = convert(&a, &mut context, source.clone());
    assert_eq!(call.kind, Kind::ToolCall);
    assert_eq!(call.content.as_deref(), Some("{\"exact\":true}"));
    assert_eq!(call.raw, source);
    let result = convert(
        &a,
        &mut context,
        row(
            "response_item",
            json!({"type":"function_call_output","call_id":"call-a","output":{"structured":1}}),
        ),
    );
    assert_eq!(result.kind, Kind::ToolResult);
    assert_eq!(result.call_id.as_deref(), Some("call-a"));
    assert_eq!(result.content.as_deref(), Some("{\"structured\":1}"));
}
#[test]
fn inherited_declared_final_remains_inherited_and_unowned() {
    let a = adapter();
    let mut context = a.create_context(&meta()).unwrap();
    let source = json!({"timestamp":TS,"type":"event_msg","payload":{"type":"task_complete","turn_id":"parent-turn","last_agent_message":"parent final"},"ordinal":3});
    let event = convert(&a, &mut context, source);
    assert!(event.inherited);
    assert!(event.source_task_id.is_empty());
    assert_eq!(event.phase.as_deref(), Some("final_answer"));
    assert_eq!(context.turn, None);
}
#[test]
fn default_codex_root_excludes_history_but_explicit_directory_is_supported() {
    let a = adapter();
    assert_eq!(
        a.roots(&a.codex_home),
        vec![
            a.codex_home.join("sessions"),
            a.codex_home.join("archived_sessions")
        ]
    );
    assert_eq!(
        a.roots(Path::new("/selected")),
        vec![PathBuf::from("/selected")]
    );
    assert_eq!(
        a.roots(Path::new("/selected.jsonl")),
        vec![PathBuf::from("/selected.jsonl")]
    );
}
#[test]
fn invalid_header_timestamp_and_negative_usage_do_not_leak_source_content() {
    let a = adapter();
    assert!(a
        .create_context(&json!({"type":"response_item","payload":{"secret":"never-print"}}))
        .is_err());
    let mut context = a.create_context(&meta()).unwrap();
    let line = json!({"type":"response_item","timestamp":"source-secret","payload":{"content":"sensitive"}});
    let error = a
        .convert(
            &Position {
                line,
                start: 0,
                end: 1,
                bytes: Vec::new(),
            },
            &mut context,
        )
        .err()
        .unwrap()
        .to_string();
    assert!(!error.contains("source-secret"));
    assert!(!error.contains("sensitive"));
    let event = convert(
        &a,
        &mut context,
        row(
            "token_usage_record",
            json!({"usage":{"input_tokens":-1,"output_tokens":4,"total_tokens":9007199254740992u64}}),
        ),
    );
    let usage = event.usage.unwrap();
    assert_eq!(usage.input_tokens, None);
    assert_eq!(usage.output_tokens, Some(4));
    assert_eq!(usage.total_tokens, None);
}
