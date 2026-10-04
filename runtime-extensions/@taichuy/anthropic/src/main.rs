use anthropic_provider::{
    handle_invoke_request_streaming, handle_request, ProviderRuntimeError, ProviderStdioRequest,
    ProviderStdioResponse, ProviderStreamEvent,
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
                let _ = emitter.try_event(worker_stream_error(error));
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
    let response = handle_request(request)
        .await
        .unwrap_or_else(worker_unary_error);
    serde_json::to_value(response).unwrap_or(Value::Null)
}

fn unary_invoke(request: &ProviderStdioRequest) -> bool {
    request.method == "invoke"
        && request.input.get("operation").and_then(Value::as_str) == Some("count_tokens")
}

// Keep typed provider facts at the actual worker wire boundary. Display text is
// only a fallback for errors that carry no provider runtime contract.
fn worker_runtime_error(error: anyhow::Error) -> ProviderRuntimeError {
    error
        .downcast_ref::<ProviderRuntimeError>()
        .cloned()
        .unwrap_or_else(|| {
            ProviderRuntimeError::normalize("provider_invalid_response", error.to_string(), None)
        })
}

fn worker_stream_error(error: anyhow::Error) -> Value {
    serde_json::to_value(ProviderStreamEvent::Error {
        error: worker_runtime_error(error),
    })
    .expect("provider runtime error is JSON serializable")
}

fn worker_unary_error(error: anyhow::Error) -> ProviderStdioResponse {
    ProviderStdioResponse::runtime_error(worker_runtime_error(error))
}

#[cfg(test)]
#[path = "_tests/worker_errors.rs"]
mod worker_errors;
