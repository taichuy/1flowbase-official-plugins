use super::*;
use crate::{ProviderOutputItemPhase, ProviderRuntimeError, ProviderRuntimeErrorKind};

fn scaffold() -> ProviderStreamEvent {
    ProviderStreamEvent::OutputItem {
        phase: ProviderOutputItemPhase::Added,
        output_index: 0,
        item: json!({"type":"reasoning","id":"empty","summary":[]}),
    }
}

#[test]
fn only_known_empty_scaffolding_can_wait() {
    for kind in ["response.created", "response.in_progress"] {
        for output in [
            json!([{"type":"message","content":[{"type":"output_text","text":"private-canary"}]}]),
            json!({"unknown":true}),
            Value::Null,
        ] {
            assert_eq!(
                semantic_kind(&json!({"type":kind,"response":{"output":output}})),
                Some("other")
            );
        }
        assert_eq!(
            semantic_kind(&json!({"type":kind,"response":{"output":[]}})),
            None
        );
        assert_eq!(
            semantic_kind(&json!({"type":kind,"response":{"id":"fixture"}})),
            None
        );
        assert_eq!(
            semantic_kind(&json!({"type":kind,"response":[]})),
            Some("other")
        );
    }
    for kind in ["response.completed", "response.done"] {
        assert_eq!(
            semantic_kind(&json!({"type":kind,"response":{"status":"completed","output":[]}})),
            Some("other")
        );
    }
    for item in [
        json!({"type":"reasoning","id":"r","summary":[]}),
        json!({"type":"message","id":"m","role":"assistant","content":[],"status":"in_progress"}),
    ] {
        assert_eq!(
            semantic_kind(
                &json!({"type":"response.output_item.added","output_index":0,"item":item})
            ),
            None
        );
    }
    for item in [
        json!({"type":"reasoning","summary":[{"type":"summary_text","text":"private-canary"}]}),
        json!({"type":"reasoning","summary":[],"encrypted_content":"private-canary"}),
        json!({"type":"message","content":[],"future":"private-canary"}),
        json!({"type":"function_call","arguments":"","name":"write"}),
        json!({"type":"future","content":[]}),
    ] {
        assert!(semantic_kind(
            &json!({"type":"response.output_item.added","output_index":0,"item":item})
        )
        .is_some());
    }
    for kind in [
        "response.content_part.added",
        "response.reasoning_summary_part.added",
    ] {
        let mut event =
            json!({"type":kind,"output_index":0,"part":{"type":"summary_text","text":""}});
        assert_eq!(semantic_kind(&event), None);
        event["future"] = json!(true);
        assert!(semantic_kind(&event).is_some());
    }
    assert_eq!(
        semantic_kind(&json!({"type":"response.failed"})),
        Some("other")
    );
    assert_eq!(
        semantic_kind(&json!({"type":"response.future"})),
        Some("other")
    );
}

#[test]
fn publication_preserves_order_and_first_semantic_category_without_content() {
    let mut visibility = Visibility::default();
    let mut all = Vec::new();
    let mut emitted = Vec::new();
    visibility
        .publish(&mut vec![scaffold()], &mut all, &mut |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .unwrap();
    assert!(emitted.is_empty());
    visibility.observe(&json!({"type":"response.output_text.delta","delta":"private-canary"}));
    visibility
        .publish(
            &mut vec![ProviderStreamEvent::TextDelta {
                delta: "visible".into(),
            }],
            &mut all,
            &mut |event| {
                emitted.push(event.clone());
                Ok(())
            },
        )
        .unwrap();
    visibility.observe(&json!({"type":"response.future"}));
    assert_eq!(emitted, all);
    assert_eq!(emitted[0], scaffold());
    let mut diagnostic = json!({});
    visibility.snapshot().annotate(&mut diagnostic);
    assert_eq!(diagnostic["semantic_event_kind"], "text_delta");
    assert!(!diagnostic.to_string().contains("private-canary"));
}

#[test]
fn successful_empty_response_flushes_once_and_pending_limits_refuse_without_exposure() {
    let mut visibility = Visibility::default();
    let mut all = Vec::new();
    let mut emitted = Vec::new();
    visibility
        .publish(&mut vec![scaffold()], &mut all, &mut |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .unwrap();
    visibility
        .flush(&mut all, &mut |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .unwrap();
    visibility
        .flush(&mut all, &mut |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .unwrap();
    assert_eq!(emitted, vec![scaffold()]);

    let mut visibility = Visibility::default();
    visibility
        .publish(
            &mut vec![scaffold(); MAX_EVENTS],
            &mut Vec::new(),
            &mut |_| panic!("uncommitted events must not escape"),
        )
        .unwrap();
    let error = visibility
        .publish(&mut vec![scaffold()], &mut Vec::new(), &mut |_| {
            panic!("overflow must not flush")
        })
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<ProviderRuntimeError>().unwrap().kind,
        ProviderRuntimeErrorKind::ProviderInvalidResponse
    );
    assert_eq!(visibility.snapshot().count, MAX_EVENTS);

    let mut visibility = Visibility::default();
    let oversized = ProviderStreamEvent::NativeEvent {
        protocol: "openai_responses".into(),
        event: json!({"type":"response.created","padding":"x".repeat(MAX_BYTES)}),
    };
    assert!(visibility
        .publish(&mut vec![oversized], &mut Vec::new(), &mut |_| panic!(
            "oversized pending event must not escape"
        ))
        .is_err());
    assert_eq!(visibility.snapshot().bytes, 0);
}

#[test]
fn control_metadata_is_visible_without_committing_or_flushing_content_slots() {
    for kind in ["codex.rate_limits", "codex.response.metadata"] {
        let mut visibility = Visibility::default();
        let mut all = Vec::new();
        let mut emitted = Vec::new();
        visibility
            .publish(&mut vec![scaffold()], &mut all, &mut |event| {
                emitted.push(event.clone());
                Ok(())
            })
            .unwrap();
        let raw = json!({"type":kind,"headers":{"x-models-etag":"fixture"}});
        let metadata = ProviderStreamEvent::NativeEvent {
            protocol: "openai_responses".into(),
            event: raw.clone(),
        };
        visibility.observe(&raw);
        visibility
            .publish(&mut vec![metadata.clone()], &mut all, &mut |event| {
                emitted.push(event.clone());
                Ok(())
            })
            .unwrap();
        assert_eq!(emitted, vec![metadata]);
        assert!(!visibility.committed());
        assert_eq!(visibility.snapshot().count, 1);
        assert!(all.is_empty());
        let mut diagnostic = json!({});
        visibility.snapshot().annotate(&mut diagnostic);
        assert!(diagnostic["semantic_event_type_digest"].is_null());
    }
}

#[test]
fn unknown_event_remains_protected_and_only_its_type_digest_is_recorded() {
    let mut visibility = Visibility::default();
    visibility.observe(&json!({"type":"PRIVATE_CANARY","content":"PRIVATE_CANARY"}));
    assert!(visibility.committed());
    let mut diagnostic = json!({});
    visibility.snapshot().annotate(&mut diagnostic);
    assert_eq!(diagnostic["semantic_event_kind"], "other");
    let digest = diagnostic["semantic_event_type_digest"].as_str().unwrap();
    assert!(digest.starts_with("sha256:"));
    assert_eq!(digest.len(), 71);
    assert!(!diagnostic.to_string().contains("PRIVATE_CANARY"));
    visibility.observe(&json!({"type":"codex.rate_limits"}));
    assert!(
        visibility.committed(),
        "metadata cannot roll back an earlier commit"
    );
}
