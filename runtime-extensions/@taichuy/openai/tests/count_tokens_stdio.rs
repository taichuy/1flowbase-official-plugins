use openai_provider::{OpenAiProviderRuntime, ProviderStdioRequest};
use serde_json::json;

#[tokio::test]
async fn count_tokens_stdio_unknown_model_and_missing_media_asset_is_total() {
    let mut runtime = OpenAiProviderRuntime::default();
    let response = runtime
        .handle_request(ProviderStdioRequest {
            method: "invoke".to_string(),
            input: json!({
                "operation": "count_tokens",
                "contract_version": "1flowbase.provider/v2",
                "provider_instance_id": "fixture",
                "provider_code": "openai",
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
