use super::*;
use crate::stream::StreamState;

fn provider_message(role: ProviderMessageRole, content: impl Into<String>) -> ProviderMessage {
    ProviderMessage {
        role,
        content: content.into(),
        name: None,
        tool_call_id: None,
        is_error: None,
        tool_calls: None,
        content_blocks: None,
    }
}

#[tokio::test]
async fn ac_005_validate_redacts_configured_proxy_url() {
    let proxy_url = "http://proxy-user:proxy-pass@127.0.0.1:8080";
    let response = handle_request(ProviderStdioRequest {
        method: "validate".to_string(),
        input: json!({
            "api_key": "provider-secret",
            "validate_model": false,
            "proxy_url": proxy_url
        }),
    })
    .await
    .unwrap();

    assert!(response.ok);
    assert_eq!(response.result["sanitized"]["proxy_url"], "***");
    assert!(!response.result.to_string().contains(proxy_url));
}

#[test]
fn protocol_model_parameter_overrides_provider_config() {
    let config = ProviderConfig {
        base_url: DEFAULT_BASE_URL.to_string(),
        api_key: "test".to_string(),
        api_protocol: BailianProtocol::OpenAiResponses,
        validate_model: true,
        proxy_url: None,
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
fn ac_002_native_max_output_tokens_maps_to_each_bailian_wire_protocol() {
    let input = ProviderInvocationInput {
        model: "qwen-plus".to_string(),
        model_parameters: BTreeMap::from([("max_output_tokens".to_string(), json!(512))]),
        ..Default::default()
    };

    let chat = build_openai_chat_body(&input).unwrap();
    let responses = build_responses_body(&input).unwrap();
    let anthropic = build_anthropic_body(&input).unwrap();
    let dashscope = build_dashscope_body(&input).unwrap();

    assert_eq!(chat["max_tokens"], 512);
    assert_eq!(responses["max_output_tokens"], 512);
    assert_eq!(anthropic["max_tokens"], 512);
    assert_eq!(dashscope["parameters"]["max_tokens"], 512);
}

#[test]
fn client_protocol_envelope_uses_default_deny_policy_for_headers() {
    let input: ProviderInvocationInput = serde_json::from_value(json!({
        "contract_version": "1flowbase.provider/v2",
        "provider_instance_id": "provider-test",
        "provider_code": "aliyun_bailian",
        "protocol": "aliyun_bailian",
        "model": "qwen-plus",
        "client_protocol_envelope": {
            "source_protocol": "anthropic_messages",
            "policy": "default_deny",
            "headers": {
                "authorization": "Bearer client-secret",
                "x-api-key": "client-api-key",
                "anthropic-version": "client-version",
                "x-client-name": "ClaudeCode",
                "content-length": "123"
            }
        }
    }))
    .unwrap();

    assert!(input.client_protocol_envelope.is_some());

    let config = normalize_provider_config(&json!({
        "api_key": "provider-secret",
        "api_protocol": "anthropic_messages"
    }))
    .unwrap();
    let headers = build_headers(
        &config,
        BailianProtocol::AnthropicMessages,
        true,
        true,
        input.client_protocol_envelope.as_ref(),
    )
    .unwrap();

    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "Bearer provider-secret"
    );
    assert_eq!(headers.get("x-api-key").unwrap(), "provider-secret");
    assert_eq!(
        headers.get("anthropic-version").unwrap(),
        DEFAULT_ANTHROPIC_VERSION
    );
    assert!(headers.get("x-client-name").is_none());
    assert!(headers.get("content-length").is_none());
}

#[test]
fn headers_restore_anthropic_client_protocol_envelope_and_keep_config_auth() {
    let input: ProviderInvocationInput = serde_json::from_value(json!({
        "contract_version": "1flowbase.provider/v2",
        "provider_instance_id": "provider-test",
        "provider_code": "aliyun_bailian",
        "protocol": "aliyun_bailian",
        "model": "qwen-plus",
        "client_protocol_envelope": {
            "source_protocol": "anthropic_messages",
            "policy": "anthropic_messages_v1",
            "headers": {
                "anthropic-version": "2023-06-01",
                "anthropic-beta": "ccr-byoc-2025-07-29",
                "x-claude-code-session-id": "session-123",
                "x-client-name": "ClaudeCode",
                "user-agent": "ClaudeCode/1.0",
                "authorization": "Bearer client-secret",
                "x-api-key": "client-auth-must-not-win"
            }
        }
    }))
    .unwrap();
    let config = normalize_provider_config(&json!({
        "api_key": "provider-secret",
        "api_protocol": "anthropic_messages"
    }))
    .unwrap();
    let headers = build_headers(
        &config,
        BailianProtocol::AnthropicMessages,
        true,
        true,
        input.client_protocol_envelope.as_ref(),
    )
    .unwrap();

    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "Bearer provider-secret"
    );
    assert_eq!(headers.get("x-api-key").unwrap(), "provider-secret");
    assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
    assert_eq!(
        headers.get("anthropic-beta").unwrap(),
        "ccr-byoc-2025-07-29"
    );
    assert_eq!(
        headers.get("x-claude-code-session-id").unwrap(),
        "session-123"
    );
    assert_eq!(headers.get("x-client-name").unwrap(), "ClaudeCode");
    assert_eq!(headers.get("user-agent").unwrap(), "ClaudeCode/1.0");
}

#[test]
fn chat_messages_map_native_tool_calls_to_openai_function_shape() {
    let mut assistant = provider_message(ProviderMessageRole::Assistant, "");
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
    let mut message = provider_message(ProviderMessageRole::User, "ignored");
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
    let mut assistant = provider_message(ProviderMessageRole::Assistant, "");
    assistant.tool_calls = Some(json!([{
        "id": "toolu_1",
        "name": "lookup",
        "arguments": { "query": "refund" }
    }]));
    let mut tool = provider_message(ProviderMessageRole::Tool, "found");
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
    let mut message = provider_message(ProviderMessageRole::User, "");
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

#[test]
fn ac_002_generate_contract_accepts_only_current_strict_input() {
    let missing = serde_json::from_value::<ProviderInvocationInput>(json!({
        "model": "qwen-plus"
    }))
    .expect_err("missing current contract must fail before provider invocation");
    assert!(missing.to_string().contains("contract_version"));

    let current = json!({
        "contract_version": "1flowbase.provider/v2",
        "provider_instance_id": "provider-test",
        "provider_code": "aliyun_bailian",
        "protocol": "aliyun_bailian",
        "model": "qwen-plus"
    });
    serde_json::from_value::<ProviderInvocationInput>(current.clone())
        .expect("current Generate input should deserialize");

    let mut legacy = current.clone();
    legacy["contract_version"] = json!("1flowbase.provider/v1");
    assert!(serde_json::from_value::<ProviderInvocationInput>(legacy).is_err());

    let mut unknown = current;
    unknown["raw_body"] = json!("must-not-be-accepted");
    let error = serde_json::from_value::<ProviderInvocationInput>(unknown)
        .expect_err("unknown Generate fields must fail closed");
    assert!(error.to_string().contains("raw_body"));
}

#[test]
fn ac_002_package_manifest_declares_only_current_generate_contract() {
    let manifest = include_str!("../manifest.yaml");

    assert!(manifest.contains("contract_version: 1flowbase.provider/v2"));
    assert!(!manifest.contains("1flowbase.provider/v1"));
    assert!(!manifest.contains("capabilities:"));
}

#[tokio::test]
async fn ac_002_rejects_undeclared_generate_capabilities_before_wire_rendering() {
    let input: ProviderInvocationInput = serde_json::from_value(json!({
        "contract_version": "1flowbase.provider/v2",
        "provider_instance_id": "provider-test",
        "provider_code": "aliyun_bailian",
        "protocol": "aliyun_bailian",
        "model": "qwen-plus",
        "required_capabilities": ["end_user_reference"]
    }))
    .unwrap();

    let error = invoke(input)
        .await
        .expect_err("undeclared semantic capabilities must fail before provider configuration");
    assert!(error.to_string().contains("semantic capabilities"));
}

#[test]
fn ac_005_raw_sensitive_upstream_body_is_not_retained() {
    let canary = "raw-prompt-canary provider-secret";
    let config = normalize_provider_config(&json!({ "api_key": "provider-secret" })).unwrap();
    let error = ensure_success_status(
        reqwest::StatusCode::BAD_REQUEST,
        &Value::String(canary.to_string()),
        &config,
    )
    .expect_err("upstream failure should remain an error");
    let message = error.to_string();

    assert!(message.contains("provider request failed"));
    assert!(!message.contains(canary));
}
