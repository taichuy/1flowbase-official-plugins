use super::*;

fn restore(source: Value, typed: Value) -> RestoredProtocolContextBody {
    let envelope: ProtocolContextEnvelope = serde_json::from_value(json!({
        "source_protocol": ANTHROPIC_MESSAGES_PROTOCOL,
        "source_request": { "body": source }
    }))
    .unwrap();
    restore_protocol_context_body_with_receipt(typed, Some(&envelope)).unwrap()
}

#[test]
fn trailing_system_per_turn_configuration_restores_exact_source_order() {
    let source = json!({
        "model": "claude-sonnet-5-5",
        "system": [{"type":"text","text":"root"}],
        "messages": [
            {"role":"user","content":[{"type":"text","text":"hello"}]},
            {"role":"system","content":[{"type":"text","text":"environment","cache_control":{"type":"ephemeral"}}],
             "output_config":{"effort":"high"}}
        ],
        "thinking":{"type":"adaptive","display":"updates"}
    });
    let typed = json!({
        "model":"provider-model",
        "system":[{"type":"text","text":"root"},{"type":"text","text":"environment"}],
        "messages":[{"role":"user","content":[{"type":"text","text":"hello"}]}],
        "thinking":{"type":"adaptive"}
    });
    let result = restore(source.clone(), typed);
    assert_eq!(result.body["messages"], source["messages"]);
    assert_eq!(result.body["system"], source["system"]);
    assert_eq!(result.body["thinking"], source["thinking"]);
    assert_eq!(result.body["model"], "provider-model");
    assert!(result
        .receipt
        .reconstructed_source_fields
        .contains("messages"));
    assert!(!result.receipt.semantic_delta_fields.contains("system"));
}

#[test]
fn multiple_mid_conversation_systems_tools_and_compaction_keep_wire_order() {
    let source = json!({
        "model":"claude",
        "messages":[
            {"role":"user","content":"first task"},
            {"role":"system","content":"first instruction","output_config":{"effort":"low"}},
            {"role":"assistant","content":[{"type":"text","text":"inspect"},
                {"type":"tool_use","id":"tool-1","name":"Read","input":{"path":"a.rs"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":"file contents"}]},
            {"role":"system","content":"updated instruction","output_config":{"effort":"high"}},
            {"role":"user","content":"Compacted context: preserve the earlier decision."},
            {"role":"system","content":"post-compaction environment"}
        ]
    });
    let typed = json!({
        "model":"claude",
        "system":[{"type":"text","text":"first instruction"},
                  {"type":"text","text":"updated instruction"},
                  {"type":"text","text":"post-compaction environment"}],
        "messages":[
            {"role":"user","content":[{"type":"text","text":"first task"}]},
            {"role":"assistant","content":[{"type":"text","text":"inspect"},
                {"type":"tool_use","id":"tool-1","name":"Read","input":{"path":"a.rs"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":"file contents"}]},
            {"role":"user","content":[{"type":"text","text":"Compacted context: preserve the earlier decision."}]}
        ]
    });
    let result = restore(source.clone(), typed);
    assert_eq!(result.body["messages"], source["messages"]);
    assert!(
        result.body.get("system").is_none(),
        "inline systems must not also appear before the conversation"
    );
}

#[test]
fn native_system_or_message_edits_block_joint_source_restoration() {
    let source = json!({"model":"claude","system":"root","messages":[
        {"role":"user","content":"question"},
        {"role":"system","content":"original instruction","output_config":{"effort":"high"}}
    ]});
    let typed = json!({"model":"claude","system":[
        {"type":"text","text":"root"},{"type":"text","text":"original instruction"}
    ],"messages":[{"role":"user","content":[{"type":"text","text":"question"}]}]});
    for field in ["system", "messages"] {
        let mut changed = typed.clone();
        if field == "system" {
            changed["system"][1]["text"] = json!("Native instruction rewrite");
        } else {
            changed["messages"][0]["content"][0]["text"] = json!("Native content rewrite");
        }
        let result = restore(source.clone(), changed.clone());
        assert_eq!(result.body[field], changed[field]);
        assert_eq!(result.body["messages"], changed["messages"]);
        assert!(result.receipt.semantic_delta_fields.contains("messages"));
    }
}

#[test]
fn changed_native_reasoning_is_not_overwritten_by_source_display() {
    let source = json!({"model":"claude","thinking":{"type":"enabled","budget_tokens":2048,"display":"updates"}});
    let typed = json!({"model":"claude","thinking":{"type":"enabled","budget_tokens":4096}});
    let result = restore(source, typed.clone());
    assert_eq!(result.body["thinking"], typed["thinking"]);
    assert!(result.receipt.semantic_delta_fields.contains("thinking"));
}

#[test]
fn moving_the_last_user_query_is_reversible_only_when_content_is_unchanged() {
    let source = json!({"model":"claude","messages":[
        {"role":"user","content":"question"},
        {"role":"assistant","content":"prefill"}
    ]});
    let typed = json!({"model":"claude","messages":[
        {"role":"assistant","content":[{"type":"text","text":"prefill"}]},
        {"role":"user","content":[{"type":"text","text":"question"}]}
    ]});
    assert_eq!(
        restore(source.clone(), typed.clone()).body["messages"],
        source["messages"]
    );
    let mut changed = typed;
    changed["messages"][1]["content"][0]["text"] = json!("rewritten question");
    assert_eq!(
        restore(source, changed.clone()).body["messages"],
        changed["messages"]
    );
}

#[test]
fn ingress_newline_joined_text_blocks_restore_without_erasing_native_edits() {
    // Golden projection from anthropic_current_user_text_content/history_text_content
    // -> native::input_mapping -> llm_context -> build_typed_messages_body.
    // The captured Claude Code 2.1.289 initial request has this same 3-text-block
    // user + trailing system/output_config shape.
    let source = json!({"model":"claude","messages":[
        {"role":"user","content":[{"type":"text","text":"history a"},{"type":"text","text":"history b"}]},
        {"role":"assistant","content":[{"type":"text","text":"answer a"},{"type":"text","text":"answer b"}]},
        {"role":"user","content":[{"type":"text","text":"a"},{"type":"text","text":"b"},{"type":"text","text":"c"}]},
        {"role":"system","content":[{"type":"text","text":"instruction a"},{"type":"text","text":"instruction b"}],"output_config":{"effort":"high"}}
    ]});
    let typed = json!({"model":"claude","system":[{"type":"text","text":"instruction a\ninstruction b"}],"messages":[
        {"role":"user","content":[{"type":"text","text":"history a\nhistory b"}]},
        {"role":"assistant","content":[{"type":"text","text":"answer a\nanswer b"}]},
        {"role":"user","content":[{"type":"text","text":"a\nb\nc"}]}
    ]});
    assert_eq!(
        restore(source.clone(), typed.clone()).body["messages"],
        source["messages"]
    );
    for index in 0..3 {
        let mut changed = typed.clone();
        changed["messages"][index]["content"][0]["text"] = json!("Native replacement");
        assert_eq!(
            restore(source.clone(), changed.clone()).body["messages"],
            changed["messages"]
        );
    }
}

#[test]
fn latest_tool_result_query_duplicate_is_verified_before_restoring_wire() {
    let source = json!({"model":"claude","messages":[
        {"role":"assistant","content":[{"type":"tool_use","id":"tool-1","name":"Read","input":{}}]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":[{"type":"text","text":"line a"},{"type":"text","text":"line b"}]}]},
        {"role":"system","content":"environment","output_config":{"effort":"high"}}
    ]});
    // translate_messages_request keeps the tool result in history and also
    // sets query to its text when there is no visible user text.
    let typed = json!({"model":"claude","system":[{"type":"text","text":"environment"}],"messages":[
        {"role":"assistant","content":[{"type":"tool_use","id":"tool-1","name":"Read","input":{}}]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":[{"type":"text","text":"line a"},{"type":"text","text":"line b"}]}]},
        {"role":"user","content":[{"type":"text","text":"line a\nline b"}]}
    ]});
    assert_eq!(
        restore(source.clone(), typed.clone()).body["messages"],
        source["messages"]
    );
    let mut changed_query = typed.clone();
    changed_query["messages"][2]["content"][0]["text"] = json!("Native follow-up");
    assert_eq!(
        restore(source.clone(), changed_query.clone()).body["messages"],
        changed_query["messages"]
    );
    let mut changed_result = typed;
    changed_result["messages"][1]["content"][0]["content"][0]["text"] =
        json!("different tool output");
    assert_eq!(
        restore(source, changed_result.clone()).body["messages"],
        changed_result["messages"]
    );
}

#[test]
fn native_root_effort_change_cannot_be_shadowed_by_source_per_turn_effort() {
    let source = json!({"model":"claude","output_config":{"effort":"high"},"messages":[
        {"role":"user","content":"question"},
        {"role":"system","content":"environment","output_config":{"effort":"high"}}
    ]});
    let typed = json!({"model":"claude","output_config":{"effort":"low"},
        "system":[{"type":"text","text":"environment"}],
        "messages":[{"role":"user","content":[{"type":"text","text":"question"}]}]});
    let result = restore(source, typed.clone());
    assert_eq!(result.body["output_config"], typed["output_config"]);
    assert_eq!(result.body["messages"], typed["messages"]);
    assert!(result
        .receipt
        .semantic_delta_fields
        .contains("output_config"));
}

#[test]
fn mixed_latest_tool_result_and_visible_text_do_not_duplicate_tool_text_in_query() {
    let source = json!({"model":"claude","messages":[
        {"role":"user","content":[
            {"type":"tool_result","tool_use_id":"tool-1","content":"tool text"},
            {"type":"text","text":"user a"},{"type":"text","text":"user b"}
        ]},
        {"role":"system","content":"environment","output_config":{"effort":"high"}}
    ]});
    let typed = json!({"model":"claude","system":[{"type":"text","text":"environment"}],"messages":[
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":"tool text"}]},
        {"role":"user","content":[{"type":"text","text":"user a\nuser b"}]}
    ]});
    assert_eq!(
        restore(source.clone(), typed).body["messages"],
        source["messages"]
    );
}

#[test]
fn removing_native_inserted_newline_never_falls_back_to_source_block_concatenation() {
    for per_turn in [false, true] {
        let mut source = json!({"model":"claude","messages":[
            {"role":"user","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}
        ]});
        let mut typed = json!({"model":"claude","messages":[
            {"role":"user","content":[{"type":"text","text":"ab"}]}
        ]});
        if per_turn {
            source["output_config"] = json!({"effort":"high"});
            source["messages"][0]["output_config"] = json!({"effort":"high"});
            typed["output_config"] = json!({"effort":"high"});
        }
        // The unchanged ingress projection is a\nb. Native's deliberate ab
        // rewrite used to match the generic concatenation fallback after the
        // exact projection rejected it.
        let result = restore(source, typed.clone());
        assert_eq!(result.body["messages"], typed["messages"]);
        assert!(result.receipt.semantic_delta_fields.contains("messages"));
        assert!(!result
            .receipt
            .reconstructed_source_fields
            .contains("messages"));
    }
}

#[test]
fn root_effort_guard_has_no_generic_message_fallback_without_inline_system() {
    let source = json!({"model":"claude","output_config":{"effort":"high"},"messages":[
        {"role":"user","content":"question","output_config":{"effort":"high"}}
    ]});
    let typed = json!({"model":"claude","output_config":{"effort":"low"},"messages":[
        {"role":"user","content":[{"type":"text","text":"question"}]}
    ]});
    let result = restore(source, typed.clone());
    assert_eq!(result.body["messages"], typed["messages"]);
    assert_eq!(result.body["output_config"], typed["output_config"]);
    assert!(result.receipt.semantic_delta_fields.contains("messages"));
}

#[test]
fn exact_message_projection_can_restore_without_overwriting_independent_root_system_delta() {
    let source = json!({"model":"claude","system":"source root","messages":[
        {"role":"user","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}
    ]});
    let typed = json!({"model":"claude","system":[{"type":"text","text":"Native root replacement"}],"messages":[
        {"role":"user","content":[{"type":"text","text":"a\nb"}]}
    ]});
    let result = restore(source.clone(), typed.clone());
    assert_eq!(result.body["messages"], source["messages"]);
    assert_eq!(result.body["system"], typed["system"]);
    assert!(result
        .receipt
        .reconstructed_source_fields
        .contains("messages"));
    assert!(result.receipt.semantic_delta_fields.contains("system"));
}

#[test]
fn native_system_block_merge_is_a_delta_and_has_no_generic_fallback() {
    for inline_system in [false, true] {
        let mut source = json!({"model":"claude","system":[
            {"type":"text","text":"a","cache_control":{"type":"ephemeral"}},
            {"type":"text","text":"b"}
        ],"messages":[{"role":"user","content":"question"}]});
        let mut typed = json!({"model":"claude","system":[{"type":"text","text":"ab"}],"messages":[
            {"role":"user","content":[{"type":"text","text":"question"}]}
        ]});
        if inline_system {
            source["messages"].as_array_mut().unwrap().push(
                json!({"role":"system","content":"environment","output_config":{"effort":"high"}}),
            );
            typed["system"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"text","text":"environment"}));
        }
        let result = restore(source, typed.clone());
        assert_eq!(result.body["system"], typed["system"]);
        assert!(result.receipt.semantic_delta_fields.contains("system"));
        assert!(!result
            .receipt
            .reconstructed_source_fields
            .contains("system"));
        if inline_system {
            assert_eq!(result.body["messages"], typed["messages"]);
        }
    }
}

#[test]
fn exact_system_block_boundaries_restore_cache_hints_and_inline_order() {
    let source = json!({"model":"claude","system":[
        {"type":"text","text":"a","cache_control":{"type":"ephemeral","ttl":"1h"}},
        {"type":"text","text":" b "}
    ],"messages":[
        {"role":"user","content":"question"},
        {"role":"system","content":[{"type":"text","text":" environment "}],"output_config":{"effort":"high"}}
    ]});
    let typed = json!({"model":"claude","system":[
        {"type":"text","text":"a"},{"type":"text","text":" b "},{"type":"text","text":" environment "}
    ],"messages":[{"role":"user","content":[{"type":"text","text":"question"}]}]});
    let result = restore(source.clone(), typed);
    assert_eq!(result.body["system"], source["system"]);
    assert_eq!(result.body["messages"], source["messages"]);
    assert!(result
        .receipt
        .reconstructed_source_fields
        .contains("system"));
}

#[test]
fn reasoning_assistant_text_block_merge_is_not_source_equivalence() {
    let source = json!({"model":"claude","messages":[
        {"role":"assistant","content":[
            {"type":"thinking","thinking":"T","signature":"signed"},
            {"type":"text","text":"a"},
            {"type":"text","text":"b","cache_control":{"type":"ephemeral"}}
        ]},
        {"role":"user","content":"q"}
    ]});
    // Native reasoning content_blocks preserve these two text block boundaries;
    // the provider codec emits them individually rather than using history text.
    let typed = json!({"model":"claude","messages":[
        {"role":"assistant","content":[
            {"type":"thinking","thinking":"T","signature":"signed"},
            {"type":"text","text":"a"},{"type":"text","text":"b"}
        ]},
        {"role":"user","content":[{"type":"text","text":"q"}]}
    ]});
    assert_eq!(
        restore(source.clone(), typed.clone()).body["messages"],
        source["messages"]
    );
    let mut changed = typed;
    changed["messages"][0]["content"] = json!([
        {"type":"thinking","thinking":"T","signature":"signed"},
        {"type":"text","text":"ab"}
    ]);
    let result = restore(source, changed.clone());
    assert_eq!(result.body["messages"], changed["messages"]);
    assert!(result.receipt.semantic_delta_fields.contains("messages"));
}

#[test]
fn nested_tool_result_text_block_merge_is_not_source_equivalence() {
    let source = json!({"model":"claude","messages":[
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":[
            {"type":"text","text":"a"},
            {"type":"text","text":"b","cache_control":{"type":"ephemeral"}}
        ]}]}
    ]});
    // Native tool content_blocks and provider tool_result_content preserve each
    // nested block, while the separately projected query uses newline joining.
    let typed = json!({"model":"claude","messages":[
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":[
            {"type":"text","text":"a"},{"type":"text","text":"b"}
        ]}]},
        {"role":"user","content":[{"type":"text","text":"a\nb"}]}
    ]});
    assert_eq!(
        restore(source.clone(), typed.clone()).body["messages"],
        source["messages"]
    );
    let mut changed = typed;
    changed["messages"][0]["content"][0]["content"] = json!([{ "type":"text", "text":"ab" }]);
    let result = restore(source, changed.clone());
    assert_eq!(result.body["messages"], changed["messages"]);
    assert!(result.receipt.semantic_delta_fields.contains("messages"));
}
