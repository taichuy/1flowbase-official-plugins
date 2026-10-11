use crate::{build_chat_completion_body, ProviderInvocationInput};
use serde_json::{json, Value};

fn request_with_choice(choice: Value) -> anyhow::Result<Value> {
    let mut input = ProviderInvocationInput {
        model: "deepseek-chat".to_string(),
        tools: vec![json!({
            "type": "function",
            "function": {"name": "get_weather", "parameters": {"type": "object"}}
        })],
        ..ProviderInvocationInput::default()
    };
    input.model_parameters.insert("tool_choice".into(), choice);
    build_chat_completion_body(&input)
}

#[test]
fn request_builder_lowers_native_and_preserves_configured_chat_choices() {
    let named_wire = json!({"type": "function", "function": {"name": "get_weather"}});
    let cases = [
        (json!({"type": "auto"}), json!("auto")),
        (json!({"type": "none"}), json!("none")),
        (json!({"type": "required"}), json!("required")),
        (
            json!({"type": "tool", "name": "get_weather"}),
            named_wire.clone(),
        ),
        (json!("auto"), json!("auto")),
        (json!("none"), json!("none")),
        (json!("required"), json!("required")),
        (named_wire.clone(), named_wire.clone()),
        (json!(" auto "), json!("auto")),
        (json!("\"required\""), json!("required")),
        (json!(r#"{"type":"auto"}"#), json!("auto")),
        (
            json!(r#"{"type":"tool","name":"get_weather"}"#),
            named_wire.clone(),
        ),
        (json!(named_wire.to_string()), named_wire),
    ];
    for (choice, expected) in cases {
        let body = request_with_choice(choice.clone()).unwrap();
        assert_eq!(body["tool_choice"], expected, "choice: {choice}");
        assert_eq!(body["tools"][0]["function"]["name"], "get_weather");
    }
}

#[test]
fn request_builder_rejects_malformed_choices_instead_of_omitting_them() {
    let cases = [
        Value::Null,
        json!(true),
        json!(42),
        json!([]),
        json!(""),
        json!("   "),
        json!("any"),
        json!("{broken json"),
        json!({}),
        json!({"type": "any"}),
        json!({"type": "auto", "name": "get_weather"}),
        json!({"type": "auto", "disable_parallel_tool_use": true}),
        json!({"type": "tool"}),
        json!({"type": "tool", "name": ""}),
        json!({"type": "tool", "name": "  "}),
        json!({"type": "tool", "name": 42}),
        json!({"type": "tool", "name": "get_weather", "extra": true}),
        json!({"type": "function", "name": "get_weather"}),
        json!({"type": "function", "function": {}}),
        json!({"type": "function", "function": {"name": false}}),
        json!({"type": "function", "function": {"name": "  "}}),
        json!({"type": "function", "function": {"name": "get_weather", "extra": true}}),
        json!({"type": "function", "function": {"name": "get_weather"}, "name": "other"}),
        json!(r#"{"type":"tool","name":""}"#),
    ];
    for choice in cases {
        let error = request_with_choice(choice.clone()).unwrap_err();
        assert!(
            error.to_string().contains("tool_choice"),
            "choice: {choice}; error: {error}"
        );
    }
}

#[test]
fn request_builder_keeps_absent_choice_absent() {
    let input = ProviderInvocationInput {
        model: "deepseek-chat".to_string(),
        ..ProviderInvocationInput::default()
    };
    assert!(build_chat_completion_body(&input)
        .unwrap()
        .get("tool_choice")
        .is_none());
}
