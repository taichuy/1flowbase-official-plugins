use super::*;
use crate::stream::StreamState;

fn provider_message(role: &str, content: Value) -> ProviderMessage {
    ProviderMessage {
        role: role.to_string(),
        content,
        name: None,
        tool_call_id: None,
        tool_calls: None,
        content_blocks: None,
    }
}

#[test]
fn protocol_model_parameter_overrides_provider_config() {
    let config = ProviderConfig {
        base_url: DEFAULT_BASE_URL.to_string(),
        api_key: "test".to_string(),
        api_protocol: BailianProtocol::OpenAiResponses,
        validate_model: true,
    };
    let input = ProviderInvocationInput {
        model_parameters: BTreeMap::from([("api_protocol".to_string(), json!("openai_chat"))]),
        ..Default::default()
    };

    assert_eq!(
        invocation_protocol(&config, &input).unwrap(),
        BailianProtocol::OpenAiChat
    );
}

#[test]
fn provider_config_defaults_to_openai_chat_protocol() {
    let config = normalize_provider_config(&json!({
        "api_key": "test"
    }))
    .unwrap();

    assert_eq!(config.api_protocol, BailianProtocol::OpenAiChat);
}

#[test]
fn chat_messages_map_native_tool_calls_to_openai_function_shape() {
    let mut assistant = provider_message("assistant", Value::Null);
    assistant.tool_calls = Some(json!([{
        "id": "call_1",
        "name": "lookup",
        "arguments": { "query": "refund" }
    }]));
    let input = ProviderInvocationInput {
        messages: vec![assistant],
        ..Default::default()
    };

    let messages = build_chat_messages(&input);
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[0]["tool_calls"][0]["type"], "function");
    assert_eq!(messages[0]["tool_calls"][0]["function"]["name"], "lookup");
    assert_eq!(
        messages[0]["tool_calls"][0]["function"]["arguments"],
        "{\"query\":\"refund\"}"
    );
}

#[test]
fn responses_input_preserves_image_content_blocks() {
    let mut message = provider_message("user", json!("ignored"));
    message.content_blocks = Some(json!([
        {
            "type": "image_url",
            "image_url": { "url": "https://example.com/cat.png" }
        },
        { "type": "text", "text": "describe it" }
    ]));
    let input = ProviderInvocationInput {
        messages: vec![message],
        ..Default::default()
    };

    let items = build_responses_input(&input);
    assert_eq!(items[0]["content"][0]["type"], "input_image");
    assert_eq!(
        items[0]["content"][0]["image_url"],
        "https://example.com/cat.png"
    );
    assert_eq!(items[0]["content"][1]["type"], "input_text");
}

#[test]
fn anthropic_messages_map_tool_calls_and_tool_results() {
    let mut assistant = provider_message("assistant", Value::Null);
    assistant.tool_calls = Some(json!([{
        "id": "toolu_1",
        "name": "lookup",
        "arguments": { "query": "refund" }
    }]));
    let mut tool = provider_message("tool", json!("found"));
    tool.tool_call_id = Some("toolu_1".to_string());
    let input = ProviderInvocationInput {
        messages: vec![assistant, tool],
        ..Default::default()
    };

    let messages = build_anthropic_messages(&input);
    assert_eq!(messages[0]["content"][0]["type"], "tool_use");
    assert_eq!(messages[0]["content"][0]["input"]["query"], "refund");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"][0]["type"], "tool_result");
    assert_eq!(messages[1]["content"][0]["tool_use_id"], "toolu_1");
}

#[test]
fn dashscope_messages_map_openai_image_parts_to_native_image_parts() {
    let mut message = provider_message("user", Value::Null);
    message.content_blocks = Some(json!([
        {
            "type": "image_url",
            "image_url": { "url": "https://example.com/cat.png" }
        },
        { "type": "text", "text": "describe it" }
    ]));

    assert!(message_has_media(&message));
    assert_eq!(
        dashscope_content_value(&message),
        json!([
            { "image": "https://example.com/cat.png" },
            { "text": "describe it" }
        ])
    );
}

#[test]
fn dashscope_stream_extracts_multimodal_text_and_reasoning_deltas() {
    let mut state = StreamState::new(
        "qwen3.6-plus-2026-04-02".to_string(),
        BailianProtocol::DashScope,
    );

    let events = state.process_dashscope_payload(&json!({
        "request_id": "req_1",
        "output": {
            "choices": [{
                "message": {
                    "content": [
                        { "text": "o" },
                        { "type": "text", "text": "k" }
                    ],
                    "reasoning_content": "thinking"
                },
                "finish_reason": "null"
            }]
        },
        "usage": {
            "input_tokens": 14,
            "output_tokens": 3,
            "total_tokens": 17,
            "output_tokens_details": {
                "reasoning_tokens": 1
            }
        }
    }));

    assert_eq!(
        events,
        vec![
            ProviderStreamEvent::ReasoningDelta {
                delta: "thinking".to_string()
            },
            ProviderStreamEvent::TextDelta {
                delta: "ok".to_string()
            }
        ]
    );
    assert_eq!(state.text, "ok");
    assert_eq!(state.finish_reason, ProviderFinishReason::Unknown);
    assert_eq!(state.usage.reasoning_tokens, Some(1));
}
