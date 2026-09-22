use super::*;

fn semantic_probe_input(choice: Value) -> ProviderInvocationInput {
    ProviderInvocationInput {
        protocol: "openai_responses".into(),
        model: "gpt-5.6-luna".into(),
        messages: vec![ProviderMessage {
            role: ProviderMessageRole::User,
            content: "Call test_signal exactly once now.".into(),
            content_blocks: None,
            name: None,
            tool_call_id: None,
            is_error: None,
            tool_calls: None,
        }],
        tools: vec![json!({"type":"function","function":{
            "name":"test_signal","description":"Return the deterministic test marker.",
            "parameters":{"type":"object","properties":{},"additionalProperties":false,"required":[]}
        }})],
        model_parameters: BTreeMap::from([
            ("tool_choice".into(), choice),
            ("store".into(), json!(false)),
            ("reasoning_effort".into(), json!("max")),
        ]),
        ..Default::default()
    }
}

#[test]
fn semantic_probe_serializes_canonical_policy_choices_as_responses_scalars() {
    for policy in ["auto", "none", "required"] {
        let input = semantic_probe_input(json!({"type":policy}));
        let body = build_responses_request_body(&input).unwrap();
        let wire = build_websocket_response_create_body(body);
        let serialized: Value =
            serde_json::from_slice(&serde_json::to_vec(&wire).unwrap()).unwrap();
        assert_eq!(serialized["tool_choice"], json!(policy));
        assert_eq!(serialized["tools"][0]["type"], "function");
        assert_eq!(serialized["tools"][0]["name"], "test_signal");
        assert_eq!(serialized["reasoning"]["effort"], "max");
        assert_eq!(serialized["store"], false);
        assert_eq!(
            serialized["input"][0]["content"],
            "Call test_signal exactly once now."
        );
    }
}

#[test]
fn semantic_named_choice_maps_canonical_tool_and_preserves_wire_choices() {
    let canonical = semantic_probe_input(json!({"type":"tool","name":"test_signal"}));
    assert_eq!(
        build_responses_request_body(&canonical).unwrap()["tool_choice"],
        json!({"type":"function","name":"test_signal"})
    );
    for choice in [
        json!("auto"),
        json!("none"),
        json!("required"),
        json!({"type":"function","name":"test_signal"}),
        json!({"type":"allowed_tools","mode":"auto","tools":[{"type":"function","name":"test_signal"}]}),
        json!({"type":"mcp","server_label":"orders"}),
        json!({"type":"auto","extra":"must-not-be-dropped"}),
        json!({"type":"tool","name":"test_signal","extra":"must-not-be-dropped"}),
    ] {
        let input = semantic_probe_input(choice.clone());
        assert_eq!(
            build_responses_request_body(&input).unwrap()["tool_choice"],
            choice
        );
    }
}

#[test]
fn native_responses_choice_bypasses_semantic_conversion() {
    let mut input = semantic_probe_input(json!({"type":"none"}));
    input.required_capabilities.extend([
        ProviderInvocationCapability::ResponsesNativePassthrough,
        ProviderInvocationCapability::ResponsesNativeOutputV1,
    ]);
    for choice in [
        json!("auto"),
        json!({"type":"function","name":"test_signal"}),
        json!({"type":"allowed_tools","mode":"auto","tools":[{"type":"function","name":"test_signal"}]}),
    ] {
        let transport = ProviderNativeTransport {
            protocol: "openai_responses".into(),
            digest: "sha256:fixture".into(),
            size_bytes: 0,
            wire_body: json!({"model":"gpt-5.6-luna","input":[],"tool_choice":choice}),
        };
        let body = build_native_responses_request_body(&input, &transport).unwrap();
        assert_eq!(body["tool_choice"], transport.wire_body["tool_choice"]);
    }
}
