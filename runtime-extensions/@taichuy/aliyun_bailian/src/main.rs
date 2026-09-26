use aliyun_bailian_provider::{
    handle_invoke_request_streaming, handle_request, ProviderStdioRequest, ProviderStdioResponse,
};
use runtime_extension_sdk::{serve, MultiplexEmitter};
use serde_json::{json, Value};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = serve(|raw, emitter| async move { handle(raw, emitter).await }).await {
        eprintln!("provider worker transport failed: {error}");
    }
}

async fn handle(raw: Value, emitter: MultiplexEmitter) -> Value {
    let request =
        serde_json::from_value::<ProviderStdioRequest>(raw).unwrap_or(ProviderStdioRequest {
            method: "invalid".to_owned(),
            input: Value::Null,
        });
    if request.method == "invoke" && !unary_invoke(&request) {
        let result = handle_invoke_request_streaming(request.input, |event| {
            emitter.try_event(serde_json::to_value(event)?)?;
            Ok(())
        })
        .await;
        return match result {
            Ok(result) => serde_json::to_value(result).unwrap_or(Value::Null),
            Err(error) => {
                let _ = emitter.try_event(json!({
                    "type": "error",
                    "error": {
                        "kind": "provider_upstream_error",
                        "message": error.to_string(),
                        "provider_summary": null,
                        "provider_details": null
                    }
                }));
                json!({
                    "final_content": null,
                    "response_id": null,
                    "tool_calls": [],
                    "mcp_calls": [],
                    "usage": {},
                    "finish_reason": "error",
                    "provider_metadata": {}
                })
            }
        };
    }
    let response = handle_request(request).await.unwrap_or_else(|error| {
        ProviderStdioResponse::error("provider_invalid_response", error.to_string())
    });
    serde_json::to_value(response).unwrap_or(Value::Null)
}

fn unary_invoke(request: &ProviderStdioRequest) -> bool {
    request.method == "invoke"
        && request.input.get("operation").and_then(Value::as_str) == Some("count_tokens")
}
