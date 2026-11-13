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

#[test]
fn claude_code_dynamic_tools_restore_inline_system_and_deferred_tools_exactly() {
    let source = json!({"model":"claude", "messages":[
        {"role":"user","content":"inspect project"},
        {"role":"system","content":[
            {"type":"tool_addition","tool":{"type":"tool_reference","name":"Glob"}},
            {"type":"tool_addition","tool":{"type":"tool_reference","name":"Grep"}}
        ]}
    ],"tools":[{"name":"Glob","description":"find files","input_schema":{"type":"object"},"defer_loading":true}]});
    let typed = json!({"model":"provider-model", "messages":[
        {"role":"user","content":[{"type":"text","text":"inspect project"}]}
    ],"tools":[{"name":"Glob","description":"find files","input_schema":{"type":"object"}}]});
    let restored = restore(source.clone(), typed.clone());
    assert_eq!(restored.body["messages"], source["messages"]);
    assert_eq!(restored.body["tools"], source["tools"]);
    assert_eq!(restored.body["model"], "provider-model");
    assert!(restored.body.get("system").is_none());
    let envelope: ProtocolContextEnvelope = serde_json::from_value(json!({
        "source_protocol":ANTHROPIC_MESSAGES_PROTOCOL,
        "source_request":{"body":source}
    }))
    .unwrap();
    for field in ["description", "input_schema"] {
        let mut changed = typed.clone();
        changed["tools"][0][field] = json!("Native edit");
        let error = restore_protocol_context_body_with_receipt(changed, Some(&envelope))
            .err()
            .unwrap();
        assert!(error
            .to_string()
            .contains("cannot preserve Anthropic defer_loading"));
    }
    let mut missing_tools = typed.clone();
    missing_tools.as_object_mut().unwrap().remove("tools");
    assert!(
        restore_protocol_context_body_with_receipt(missing_tools, Some(&envelope))
            .err()
            .unwrap()
            .to_string()
            .contains("cannot preserve Anthropic defer_loading")
    );
    let mut changed_query = typed.clone();
    changed_query["messages"][0]["content"][0]["text"] = json!("Native query edit");
    assert!(
        restore_protocol_context_body_with_receipt(changed_query, Some(&envelope))
            .err()
            .unwrap()
            .to_string()
            .contains("cannot preserve Anthropic tool_addition")
    );
}

#[test]
fn canonical_named_function_choice_produces_anthropic_tool_request_body() {
    let input = ProviderInvocationInput {
        model: "provider-model".into(),
        model_parameters: BTreeMap::from([(
            "tool_choice".into(),
            json!({"type":"function","name":"Read"}),
        )]),
        tools: vec![
            json!({"type":"function","function":{"name":"Read","parameters":{"type":"object"}}}),
        ],
        ..Default::default()
    };
    let typed = build_messages_body(&input).unwrap();
    assert_eq!(typed["tool_choice"], json!({"type":"tool","name":"Read"}));
    assert_eq!(typed["tools"][0]["name"], "Read");
    let count_input: ProviderCountTokensInput = serde_json::from_value(json!({
        "operation":"count_tokens", "contract_version":"1flowbase.provider/v2",
        "provider_instance_id":"fixture", "provider_code":"anthropic", "protocol":"anthropic_messages",
        "model":input.model, "tools":input.tools, "model_parameters":input.model_parameters
    })).unwrap();
    assert_eq!(
        build_count_tokens_body(&count_input).unwrap()["tool_choice"],
        typed["tool_choice"]
    );
    let source = json!({"model":"source-model","tool_choice":{"type":"tool","name":"Read"}});
    let restored = restore(source.clone(), typed.clone());
    assert_eq!(restored.body["tool_choice"], source["tool_choice"]);
    let mut changed = typed;
    changed["tool_choice"]["name"] = json!("Write");
    assert_eq!(
        restore(source, changed.clone()).body["tool_choice"],
        changed["tool_choice"]
    );
}

#[test]
fn false_defer_loading_retains_harmless_native_tool_edit_fallback() {
    let source = json!({"model":"source-model","tools":[{"name":"Read",
        "input_schema":{"type":"object"},"defer_loading":false}]});
    let typed = json!({"model":"provider-model","tools":[{"name":"Read",
        "input_schema":{"type":"object","properties":{"path":{"type":"string"}}}}]});
    let result = restore(source, typed.clone());
    assert_eq!(result.body["tools"], typed["tools"]);
    assert!(result.receipt.semantic_delta_fields.contains("tools"));
}

#[test]
fn source_tool_description_whitespace_restores_exact_original_after_native_projection() {
    let source = json!({"model":"source-model","tools":[
        {"name":"Read","description":"  Read a file.\n","input_schema":{"type":"object"},"defer_loading":true},
        {"name":"Glob","description":" \n\t","input_schema":{"type":"object"}}
    ]});
    let typed = json!({"model":"provider-model","tools":[
        {"name":"Read","description":"Read a file.","input_schema":{"type":"object"}},
        {"name":"Glob","input_schema":{"type":"object"}}
    ]});
    let restored = restore(source.clone(), typed.clone());
    assert_eq!(restored.body["tools"], source["tools"]);
    assert!(restored
        .receipt
        .reconstructed_source_fields
        .contains("tools"));
    assert!(!restored.receipt.semantic_delta_fields.contains("tools"));
    assert_eq!(restored.body["model"], "provider-model");

    let envelope: ProtocolContextEnvelope = serde_json::from_value(json!({
        "source_protocol":ANTHROPIC_MESSAGES_PROTOCOL,"source_request":{"body":source}
    }))
    .unwrap();
    let mut whitespace_edit = typed.clone();
    whitespace_edit["tools"][0]["description"] = json!("Read a file. ");
    let mut empty_edit = typed.clone();
    empty_edit["tools"][1]["description"] = json!(" ");
    let mut description_edit = typed.clone();
    description_edit["tools"][0]["description"] = json!("Native replacement");
    let mut name_edit = typed.clone();
    name_edit["tools"][0]["name"] = json!("Write");
    let mut schema_edit = typed.clone();
    schema_edit["tools"][0]["input_schema"] = json!({"type":"object","required":["path"]});
    let mut order_edit = typed;
    order_edit["tools"].as_array_mut().unwrap().reverse();
    for changed in [
        whitespace_edit,
        empty_edit,
        description_edit,
        name_edit,
        schema_edit,
        order_edit,
    ] {
        let error = restore_protocol_context_body_with_receipt(changed, Some(&envelope))
            .err()
            .unwrap();
        assert!(error
            .to_string()
            .contains("cannot preserve Anthropic defer_loading"));
    }
}

fn projected_source_system(body: &Value) -> Vec<Value> {
    let mut system = body
        .get("system")
        .map(|value| anthropic_system_blocks(value, true).unwrap())
        .unwrap_or_default();
    let messages = body["messages"].as_array().unwrap();
    let last_user = messages
        .iter()
        .rposition(|message| message["role"] == "user")
        .unwrap();
    native_source_messages(messages, last_user, &mut system).unwrap();
    system
}

#[test]
fn appended_native_system_preserves_source_messages_effort_cache_and_projection() {
    for has_root in [false, true] {
        for tail_count in [1, 2] {
            let mut source = json!({"model":"source-model", "output_config":{"effort":"max"}, "messages":[
                {"role":"user","content":"question"},
                {"role":"system","content":"historical instruction","output_config":{"effort":"medium"}},
                {"role":"system","content":[
                    {"type":"text","text":"current instruction"},
                    {"type":"tool_addition","tool":{"type":"tool_reference","name":"Glob"},"cache_control":{"type":"ephemeral"}}
                ],"output_config":{"effort":"max"}}
            ]});
            if has_root {
                source["system"] = json!([{"type":"text","text":"client root","cache_control":{"type":"ephemeral"}}]);
            }
            let mut typed_system = projected_source_system(&source);
            // The second appended block intentionally repeats a client text.
            // Ownership is positional; equal text must never trigger removal.
            let tail = vec![
                json!({"type":"text","text":"application instruction","cache_control":{"type":"ephemeral"}}),
                json!({"type":"text","text":"current instruction"}),
            ];
            typed_system.extend(tail[..tail_count].iter().cloned());
            let typed = json!({"model":"provider-model", "output_config":{"effort":"max"},
                "system":typed_system,"messages":[{"role":"user","content":[{"type":"text","text":"question"}]}]});
            let result = restore(source.clone(), typed.clone());
            let original_messages = source["messages"].as_array().unwrap();
            let restored_messages = result.body["messages"].as_array().unwrap();
            assert_eq!(
                &restored_messages[..original_messages.len()],
                original_messages
            );
            assert_eq!(
                restored_messages.len(),
                original_messages.len() + tail_count
            );
            for (message, block) in restored_messages[original_messages.len()..]
                .iter()
                .zip(&tail)
            {
                assert_eq!(message, &json!({"role":"system","content":[block]}));
                assert!(message.get("output_config").is_none());
            }
            assert_eq!(result.body.get("system"), source.get("system"));
            assert_eq!(result.body["output_config"], typed["output_config"]);
            assert_eq!(
                projected_source_system(&result.body),
                anthropic_system_blocks(&typed["system"], false).unwrap()
            );
            let last_user = restored_messages
                .iter()
                .rposition(|message| message["role"] == "user")
                .unwrap();
            let projected =
                native_source_messages(restored_messages, last_user, &mut Vec::new()).unwrap();
            assert_eq!(
                Value::Array(build_anthropic_messages(&projected)),
                typed["messages"]
            );
            assert!(result.receipt.semantic_delta_fields.contains("messages"));
            assert!(result.receipt.semantic_delta_fields.contains("system"));
            assert!(!result
                .receipt
                .reconstructed_source_fields
                .contains("messages"));
            assert!(!result
                .receipt
                .reconstructed_source_fields
                .contains("system"));
        }
    }
}

#[test]
fn appended_native_system_requires_exact_source_prefix_and_messages() {
    let source = json!({"model":"source-model", "system":[{"type":"text","text":"client root"}],
        "messages":[{"role":"user","content":"question"},
            {"role":"system","content":[{"type":"text","text":"client inline"},
                {"type":"tool_addition","tool":{"type":"tool_reference","name":"Glob"}}]}]});
    let typed = json!({"model":"provider-model","system":[
        {"type":"text","text":"client root"},{"type":"text","text":"client inline"},
        {"type":"text","text":"application addition"}],
        "messages":[{"role":"user","content":[{"type":"text","text":"question"}]}]});
    let envelope: ProtocolContextEnvelope = serde_json::from_value(json!({
        "source_protocol":ANTHROPIC_MESSAGES_PROTOCOL,"source_request":{"body":source}
    }))
    .unwrap();
    let mut rewrite = typed.clone();
    rewrite["system"][0]["text"] = json!("Native rewrite");
    let mut remove = typed.clone();
    remove["system"].as_array_mut().unwrap().remove(0);
    let mut reorder = typed.clone();
    reorder["system"].as_array_mut().unwrap().swap(0, 1);
    let mut message_edit = typed;
    message_edit["messages"][0]["content"][0]["text"] = json!("Native question edit");
    for changed in [rewrite, remove, reorder, message_edit] {
        let error = restore_protocol_context_body_with_receipt(changed, Some(&envelope))
            .err()
            .unwrap();
        assert!(error
            .to_string()
            .contains("cannot preserve Anthropic tool_addition"));
    }
}

#[test]
fn no_inline_system_keeps_native_root_additions_in_root() {
    let source = json!({"model":"source-model","system":[{"type":"text","text":"client root"}],
        "messages":[{"role":"user","content":"question"}]});
    let typed = json!({"model":"provider-model","system":[{"type":"text","text":"client root"},
        {"type":"text","text":"Native addition"}],
        "messages":[{"role":"user","content":[{"type":"text","text":"question"}]}]});
    let result = restore(source.clone(), typed.clone());
    assert_eq!(result.body["system"], typed["system"]);
    assert_eq!(result.body["messages"], source["messages"]);
    assert!(result.receipt.semantic_delta_fields.contains("system"));
    assert!(!result.receipt.semantic_delta_fields.contains("messages"));
}
