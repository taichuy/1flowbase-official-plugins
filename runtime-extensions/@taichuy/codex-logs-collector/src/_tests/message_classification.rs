use super::*;
use serde_json::json;

fn events(payload: Value) -> Vec<AgentLogEvent> {
    let adapter = CodexAdapter {
        codex_home: PathBuf::from("/fixture"),
    };
    let mut context = adapter
        .create_context(&json!({"type":"session_meta","payload":{"id":"session"}}))
        .unwrap();
    context.turn = Some("native-turn".into());
    adapter.convert(&Position { line: json!({"type":"response_item","timestamp":"2026-10-12T00:00:00Z","payload":payload}), start:0,end:1,bytes:vec![] }, &mut context).unwrap()
}

#[test]
fn native_injected_user_role_is_context_not_human_input() {
    // Sanitized captured native shape: metadata is positional, not prompt wording.
    let p = json!({"type":"message","role":"user","content":[{"type":"input_text","text":"instructions"},{"type":"input_text","text":"environment"}],"internal_chat_message_metadata_passthrough":{"turn_id":"response-id","content_item_kinds":["agents_md.instructions","environments.environment_context"]}});
    let result = events(p.clone());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].kind, Kind::Context);
    assert_eq!(
        result[0].content.as_deref(),
        Some("instructions\nenvironment")
    );
    assert_eq!(result[0].source_task_id, "native-turn");
    assert_eq!(result[0].raw["payload"], p);
}

#[test]
fn typed_human_text_is_not_classified_by_its_prompt_wording() {
    let p = json!({"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions requested by the human"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":["user.text"]}});
    let result = events(p.clone());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].kind, Kind::User);
    assert_eq!(
        result[0].content.as_deref(),
        p["content"][0]["text"].as_str()
    );
}

#[test]
fn mixed_classified_content_keeps_all_human_fragments_in_one_input() {
    // Compatibility fixture from Codex's positional AnnotatedContent contract.
    let p = json!({"type":"message","role":"user","content":[{"text":"first"},{"text":"environment"},{"text":"second"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":["user.text","environments.environment_context","user.answered_question"]}});
    let result = events(p.clone());
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].kind, Kind::Context);
    assert_eq!(result[0].content.as_deref(), Some("environment"));
    assert_eq!(result[1].kind, Kind::User);
    assert_eq!(result[1].content.as_deref(), Some("first\nsecond"));
    for e in result {
        assert_eq!(e.raw["payload"], p);
        assert_eq!(e.source_task_id, "native-turn");
    }
}

#[test]
fn missing_and_future_positional_kinds_remain_unknown_context() {
    for kinds in [json!([]), json!(["future.kind"])] {
        let result = events(
            json!({"type":"message","role":"user","content":[{"text":"preserved"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":kinds}}),
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, Kind::Context);
        assert_eq!(result[0].content.as_deref(), Some("preserved"));
    }
    let result = events(
        json!({"type":"message","role":"user","content":[{"text":"human"},{"text":"unclassified"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":["user.text"]}}),
    );
    assert_eq!(result[0].content.as_deref(), Some("unclassified"));
    assert_eq!(result[0].kind, Kind::Context);
    assert_eq!(result[1].content.as_deref(), Some("human"));
    assert_eq!(result[1].kind, Kind::User);
}

#[test]
fn legacy_unclassified_human_messages_keep_their_role() {
    let result = events(
        json!({"type":"message","role":"user","content":[{"text":"legacy"}],"internal_chat_message_metadata_passthrough":{"turn_id":"response-turn"}}),
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].kind, Kind::User);
    assert_eq!(result[0].content.as_deref(), Some("legacy"));
    assert_eq!(result[0].source_task_id, "native-turn");
}

#[test]
fn native_user_event_and_assistant_mirror_are_distinct() {
    let adapter = CodexAdapter {
        codex_home: PathBuf::from("/fixture"),
    };
    let mut context = adapter
        .create_context(&json!({"type":"session_meta","payload":{"id":"session"}}))
        .unwrap();
    context.turn = Some("native-turn".into());
    for (kind, expected) in [
        ("user_message", Kind::User),
        ("agent_message", Kind::Context),
    ] {
        let raw = json!({"type":"event_msg","timestamp":"2026-10-12T00:00:00Z","payload":{"type":kind,"message":"message"}});
        let result = adapter
            .convert(
                &Position {
                    line: raw.clone(),
                    start: 0,
                    end: 1,
                    bytes: vec![],
                },
                &mut context,
            )
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, expected);
        assert_eq!(result[0].content.as_deref(), Some("message"));
        assert_eq!(result[0].raw, raw);
    }
}

#[test]
fn plaintext_agent_request_keeps_provenance_without_becoming_human() {
    let p = json!({"type":"agent_message","author":"/root","recipient":"/root/qa","content":[{"type":"input_text","text":"task"},{"type":"input_text","text":"detail"}],"internal_chat_message_metadata_passthrough":{"turn_id":"response-turn"}});
    let result = events(p.clone());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].kind, Kind::Context);
    assert_eq!(result[0].content.as_deref(), Some("task\ndetail"));
    assert_eq!(result[0].name.as_deref(), Some("/root"));
    assert_eq!(result[0].raw["payload"], p);
    assert_eq!(result[0].source_task_id, "native-turn");
}

#[test]
fn encrypted_agent_message_does_not_present_partial_text_as_complete() {
    let p = json!({"type":"agent_message","author":"/root","recipient":"/root/qa","content":[{"type":"input_text","text":"partial wrapper"},{"type":"encrypted_content","encrypted_content":"opaque"}]});
    let result = events(p.clone());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].kind, Kind::Context);
    assert_eq!(result[0].content, None);
    assert_eq!(result[0].name.as_deref(), Some("/root"));
    assert_eq!(result[0].raw["payload"], p);
}
