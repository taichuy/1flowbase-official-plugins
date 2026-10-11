use super::*;
use serde_json::json;
fn context() -> SessionContext {
    DeepSeekHarnessAdapter
        .create_context(&json!({"type":"collector_session","session_id":"s"}))
        .unwrap()
}
fn convert(ctx: &mut SessionContext, ty: &str, data: Value) -> Vec<AgentLogEvent> {
    let line = json!({"type":"dsh_event","session":{"id":"s","parentSession":"parent-session"},
        "event":{"type":ty,"seq":1,"time":1791676800123_i64,"data":data}});
    DeepSeekHarnessAdapter
        .convert(
            &Position {
                line,
                start: 0,
                end: 1024,
                bytes: vec![],
            },
            ctx,
        )
        .unwrap()
}
#[test]
fn fixture_preserves_message_blocks_usage_and_tools() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/native-events.json")).unwrap();
    let mut ctx = context();
    let mut facts = vec![];
    for line in fixture.as_array().unwrap() {
        facts.extend(
            DeepSeekHarnessAdapter
                .convert(
                    &Position {
                        line: line.clone(),
                        start: 0,
                        end: 1000,
                        bytes: vec![],
                    },
                    &mut ctx,
                )
                .unwrap(),
        );
    }
    assert_eq!(
        facts
            .iter()
            .filter(|f| f.kind == Kind::Assistant && f.phase.is_none())
            .count(),
        2
    );
    assert_eq!(
        facts
            .iter()
            .filter(|f| f.phase.as_deref() == Some("reasoning"))
            .count(),
        1
    );
    assert_eq!(facts.iter().filter(|f| f.kind == Kind::ToolCall).count(), 1);
    let u = facts.iter().find_map(|f| f.usage.as_ref()).unwrap();
    assert_eq!(u.input_tokens, Some(8));
    assert_eq!(u.input_cache_hit_tokens, Some(5));
    assert!(facts
        .iter()
        .all(|f| f.source_task_id == "7"
            && f.parent_source_task_id.as_deref() == Some("parent-turn")));
    assert!(ctx.turn.is_none());
}
#[test]
fn stream_attempt_aggregates_deltas_without_fabricating_terminal() {
    let mut ctx = context();
    convert(&mut ctx, "turn/start", json!({"turn":4}));
    let facts = convert(
        &mut ctx,
        "assistant/attempt",
        json!({"turn":4,"step":1,"stream":[
          {"type":"text-chunks","time0":1,"index":0,"dt":[0,1],"texts":["a","b"]},
          {"type":"chunk","time":3,"chunk":{"type":"text-delta","index":0,"text":"c"}},
          {"type":"reasoning-chunks","time0":1,"index":1,"dt":[0],"texts":["why"]}
        ]}),
    );
    assert_eq!(facts[0].content.as_deref(), Some("abc"));
    assert_eq!(facts[1].phase.as_deref(), Some("attempt_reasoning"));
    assert!(facts.iter().all(
        |f| f.phase.as_deref().unwrap_or("").starts_with("attempt") && f.kind != Kind::TaskEnd
    ));
    assert_eq!(ctx.turn.as_deref(), Some("4"));
}
#[test]
fn usage_absence_and_terminal_error_are_source_facts() {
    let mut ctx = context();
    let facts = convert(
        &mut ctx,
        "assistant/message",
        json!({"turn":2,"step":1,"interrupted":true,
        "message":{"content":[{"type":"text","text":"prefix"}],"source":{"provider":"p","model":"m"}},"stream":[]}),
    );
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].phase.as_deref(), Some("interrupted"));
    assert!(facts[0].usage.is_none());
    let facts = convert(
        &mut ctx,
        "turn/end",
        json!({"turn":2,"reason":{"kind":"error","error":{"code":"X","message":"failure"}}}),
    );
    assert_eq!(facts[0].phase.as_deref(), Some("error"));
    assert_eq!(
        facts[0].raw["event"]["data"]["reason"]["error"]["code"],
        "X"
    );
}
#[test]
fn identity_and_timestamp_mismatch_fail_closed() {
    let mut ctx = context();
    let p = Position {
        line: json!({"type":"dsh_event","session":{"id":"wrong"},"event":{"type":"turn/start","time":10,"data":{"turn":1}}}),
        start: 0,
        end: 1,
        bytes: vec![],
    };
    assert!(DeepSeekHarnessAdapter.convert(&p, &mut ctx).is_err());
}

#[test]
fn change_driven_request_config_remains_active_across_turns() {
    let mut ctx = context();
    convert(&mut ctx, "turn/start", json!({"turn":1}));
    convert(
        &mut ctx,
        "request/header",
        json!({"header":{"config":{"provider":"p","model":"m","reasoningEffort":"max"}}}),
    );
    convert(
        &mut ctx,
        "turn/end",
        json!({"turn":1,"reason":{"kind":"completed"}}),
    );
    let facts = convert(&mut ctx, "turn/start", json!({"turn":2}));
    assert_eq!(facts[0].reasoning_effort.as_deref(), Some("max"));
    assert_eq!(facts[0].source_task_id, "2");
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
fn disjoint_usage_adds_cache_and_honors_native_omission_zero() {
    let present = usage(
        &serde_json::json!({"inputTokens":3,"outputTokens":2,"cacheReadTokens":5,"cacheWriteTokens":7}),
    );
    assert_eq!(present.input_tokens, Some(15));
    assert_eq!(present.input_cache_hit_tokens, Some(5));
    assert_eq!(present.cache_write_tokens, Some(7));
    let omitted = usage(&serde_json::json!({"inputTokens":3,"outputTokens":2}));
    assert_eq!(omitted.input_tokens, Some(3));
    assert_eq!(omitted.input_cache_hit_tokens, Some(0));
    assert_eq!(omitted.cache_write_tokens, Some(0));
    assert_eq!(usage(&serde_json::json!({})).input_tokens, None);
    assert_eq!(usage(&serde_json::json!({})).input_cache_hit_tokens, None);
    assert_eq!(usage(&serde_json::json!({})).cache_write_tokens, None);
    assert_eq!(
        usage(&serde_json::json!({"inputTokens":3,"cacheReadTokens":null})).input_tokens,
        None
    );
    assert_eq!(
        usage(&serde_json::json!({"inputTokens":3,"cacheWriteTokens":-1})).cache_write_tokens,
        None
    );
    assert_eq!(
        usage(&serde_json::json!({"inputTokens":9_007_199_254_740_991_i64,"cacheReadTokens":1}))
            .input_tokens,
        None
    );
}

#[test]
fn completed_terminal_confirms_only_last_complete_assistant_text() {
    let mut ctx = context();
    convert(&mut ctx, "turn/start", json!({"turn":1}));
    convert(
        &mut ctx,
        "assistant/message",
        json!({"turn":1,"step":1,"message":{"content":[
        {"type":"text","text":"intermediate"}, {"type":"tool-call","id":"call1","name":"read","arguments":"{}"}
    ]},"stream":[]}),
    );
    convert(
        &mut ctx,
        "tool/call",
        json!({"turn":1,"step":1,"callId":"call1","name":"read","arguments":"{}"}),
    );
    convert(
        &mut ctx,
        "tool/result",
        json!({"turn":1,"step":1,"message":{"toolCallId":"call1","content":[{"type":"text","text":"result"}]}}),
    );
    convert(
        &mut ctx,
        "assistant/message",
        json!({"turn":1,"step":2,"message":{"content":[
        {"type":"reasoning","text":"private"},{"type":"text","text":"FIRST"},{"type":"text","text":"_OK"}
    ]},"stream":[]}),
    );
    // Durable adapter snapshots retain the pending candidate until the terminal
    // record, even when a collect batch stops between these source records.
    let mut ctx: SessionContext =
        serde_json::from_value(serde_json::to_value(ctx).unwrap()).unwrap();
    let facts = convert(
        &mut ctx,
        "turn/end",
        json!({"turn":1,"reason":{"kind":"completed"}}),
    );
    assert_eq!(facts[0].kind, Kind::TaskEnd);
    assert_eq!(facts[0].phase.as_deref(), Some("final_answer"));
    assert_eq!(facts[0].content.as_deref(), Some("FIRST_OK"));
    assert_eq!(facts[0].raw["event"]["data"]["reason"]["kind"], "completed");
    assert!(ctx.final_text.is_none());
}

#[test]
fn non_completed_terminal_never_declares_final_answer() {
    for reason in [
        "error",
        "aborted",
        "max-tokens",
        "blocked",
        "interrupted",
        "forked",
    ] {
        let mut ctx = context();
        convert(&mut ctx, "turn/start", json!({"turn":1}));
        convert(
            &mut ctx,
            "assistant/message",
            json!({"turn":1,"step":1,"message":{"content":[{"type":"text","text":"visible prefix"}]},"stream":[]}),
        );
        let facts = convert(
            &mut ctx,
            "turn/end",
            json!({"turn":1,"reason":{"kind":reason}}),
        );
        assert!(facts
            .iter()
            .all(|f| f.phase.as_deref() != Some("final_answer") && f.content.is_none()));
        assert_eq!(
            facts[0].phase.as_deref(),
            Some(if reason == "aborted" {
                "cancelled"
            } else {
                reason
            })
        );
        assert_eq!(facts[0].raw["event"]["data"]["reason"]["kind"], reason);
        assert!(ctx.final_text.is_none());
    }
}

#[test]
fn incomplete_or_tool_output_and_previous_turn_never_supply_final() {
    for invalidator in [
        "tool/call",
        "assistant/attempt",
        "interrupted_message",
        "tool_message",
        "new_turn",
    ] {
        let mut ctx = context();
        convert(&mut ctx, "turn/start", json!({"turn":1}));
        convert(
            &mut ctx,
            "assistant/message",
            json!({"turn":1,"step":1,"message":{"content":[{"type":"text","text":"old candidate"}]},"stream":[]}),
        );
        match invalidator {
            "interrupted_message" => {
                convert(
                    &mut ctx,
                    "assistant/message",
                    json!({"turn":1,"step":2,"interrupted":true,"message":{"content":[{"type":"text","text":"prefix"}]},"stream":[]}),
                );
            }
            "tool_message" => {
                convert(
                    &mut ctx,
                    "assistant/message",
                    json!({"turn":1,"step":2,"message":{"content":[{"type":"text","text":"tool preface"},{"type":"tool-call","id":"c","name":"read","arguments":"{}"}]},"stream":[]}),
                );
            }
            "new_turn" => {
                convert(&mut ctx, "turn/start", json!({"turn":2}));
            }
            _ => {
                convert(
                    &mut ctx,
                    invalidator,
                    json!({"turn":1,"step":2,"stream":[]}),
                );
            }
        }
        let turn = if invalidator == "new_turn" { 2 } else { 1 };
        let facts = convert(
            &mut ctx,
            "turn/end",
            json!({"turn":turn,"reason":{"kind":"completed"}}),
        );
        assert_eq!(facts[0].phase.as_deref(), Some("completed"));
        assert!(facts[0].content.is_none());
    }
}

#[test]
fn native_source_distinguishes_real_user_from_every_context_producer() {
    let messages: Vec<Value> =
        serde_json::from_str(include_str!("../fixtures/source-classification.json")).unwrap();
    let mut ctx = context();
    convert(&mut ctx, "turn/start", json!({"turn":7}));
    let mut facts = Vec::new();
    for message in &messages {
        let converted = convert(&mut ctx, "user/message", message.clone());
        assert_eq!(converted[0].raw["event"]["data"], *message);
        assert_eq!(converted[0].source_task_id, "7");
        facts.extend(converted);
    }
    assert_eq!(
        facts.iter().filter(|fact| fact.kind == Kind::User).count(),
        1
    );
    assert_eq!(
        facts
            .iter()
            .filter(|fact| fact.kind == Kind::Context)
            .count(),
        6
    );
    assert_eq!(facts[1].name.as_deref(), Some("agent-instructions"));
    assert_eq!(facts[2].phase.as_deref(), Some("snapshot"));
}
