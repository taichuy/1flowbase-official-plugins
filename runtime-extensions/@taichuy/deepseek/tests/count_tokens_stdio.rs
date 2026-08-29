use deepseek_provider::{handle_request, ProviderStdioRequest};
use serde_json::json;

// AC-001: CountTokens must use DeepSeek's pinned V4 tokenizer and chat template.
#[tokio::test]
async fn count_tokens_stdio_matches_deepseek_v4_text_fixture() {
    let response = handle_request(ProviderStdioRequest {
        method: "invoke".to_string(),
        input: json!({
            "operation": "count_tokens",
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "fixture",
            "provider_code": "deepseek",
            "protocol": "openai_chat",
            "model": "deepseek-v4-pro",
            "provider_config": {},
            "system": [{ "type": "text", "text": "Be concise" }],
            "messages": [{ "role": "user", "content": "Hello!" }],
            "required_capabilities": ["count_tokens"],
            "tools": [],
            "mcp_bindings": [],
            "response_format": null,
            "model_parameters": {},
            "trace_context": {},
            "run_context": {}
        }),
    })
    .await
    .expect("CountTokens must return one result");

    assert!(response.ok);
    assert_eq!(response.result["operation"], "count_tokens");
    assert_eq!(response.result["method"], "deepseek_v4_tokenizer");
    assert_eq!(response.result["coverage"], "complete");
    assert_eq!(response.result["input_tokens"], 7);
}

#[tokio::test]
async fn count_tokens_stdio_matches_deepseek_v4_tool_history_fixture() {
    let response = handle_request(ProviderStdioRequest {
        method: "invoke".to_string(),
        input: json!({
            "operation": "count_tokens",
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "fixture",
            "provider_code": "deepseek",
            "protocol": "openai_chat",
            "model": "deepseek-v4-pro",
            "provider_config": {},
            "messages": [
                { "role": "user", "content": "Weather?" },
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "type": "function",
                        "function": { "name": "weather", "arguments": "{\"city\":\"杭州\"}" }
                    }]
                },
                { "role": "tool", "content": "sunny", "tool_call_id": "call_1" }
            ],
            "required_capabilities": ["count_tokens"],
            "tools": [],
            "mcp_bindings": [],
            "response_format": null,
            "model_parameters": {},
            "trace_context": {},
            "run_context": {}
        }),
    })
    .await
    .expect("CountTokens must return one result");

    assert!(response.ok);
    assert_eq!(response.result["method"], "deepseek_v4_tokenizer");
    assert_eq!(response.result["coverage"], "complete");
    assert_eq!(response.result["input_tokens"], 27);
}

// AC-002: unsupported media must stay observable instead of claiming exact coverage.
#[tokio::test]
async fn count_tokens_stdio_unknown_model_and_missing_media_asset_is_total() {
    let response = handle_request(ProviderStdioRequest {
        method: "invoke".to_string(),
        input: json!({
            "operation": "count_tokens",
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "fixture",
            "provider_code": "deepseek",
            "protocol": "openai_chat",
            "model": "unknown-fixture-model",
            "provider_config": {},
            "messages": [{
                "role": "user",
                "content": "",
                "content_blocks": [{
                    "type": "image_url",
                    "image_url": {"url": "file:///missing-fixture.png"}
                }]
            }],
            "required_capabilities": ["count_tokens"],
            "tools": [],
            "mcp_bindings": [],
            "response_format": null,
            "model_parameters": {},
            "trace_context": {},
            "run_context": {}
        }),
    })
    .await
    .expect("CountTokens must return one total result");

    assert!(response.ok);
    assert_eq!(response.result["operation"], "count_tokens");
    assert_eq!(response.result["method"], "provider_estimate");
    assert_eq!(response.result["coverage"], "partial");
    assert_eq!(response.result["unknown_block_count"], 1);
    assert!(response.result["input_tokens"].as_u64().is_some());
}
