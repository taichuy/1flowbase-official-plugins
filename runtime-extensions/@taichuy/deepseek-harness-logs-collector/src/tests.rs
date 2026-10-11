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
    assert_eq!(u.input_tokens, Some(3));
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
