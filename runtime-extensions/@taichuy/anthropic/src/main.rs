use std::io::{self, Read, Write};

use anthropic_provider::{
    handle_invoke_request_streaming, handle_request, ProviderFinishReason,
    ProviderInvocationResult, ProviderRuntimeError, ProviderStdioRequest, ProviderStdioResponse,
    ProviderUsage,
};

#[tokio::main]
async fn main() {
    let mut stdin = String::new();
    io::stdin().read_to_string(&mut stdin).unwrap();

    let request: ProviderStdioRequest =
        serde_json::from_str(&stdin).unwrap_or(ProviderStdioRequest {
            method: "invalid".to_string(),
            input: serde_json::Value::Null,
        });

    if request.method == "invoke"
        && request
            .input
            .get("operation")
            .and_then(|value| value.as_str())
            != Some("count_tokens")
    {
        run_streaming_invoke(request).await;
        return;
    }

    let response = handle_request(request).await.unwrap_or_else(|error| {
        error
            .downcast_ref::<ProviderRuntimeError>()
            .cloned()
            .map(ProviderStdioResponse::runtime_error)
            .unwrap_or_else(|| {
                ProviderStdioResponse::error("provider_invalid_response", error.to_string())
            })
    });
    print!("{}", serde_json::to_string(&response).unwrap());
}

async fn run_streaming_invoke(request: ProviderStdioRequest) {
    let mut stdout = io::stdout().lock();
    let result = handle_invoke_request_streaming(request.input, |event| {
        writeln!(stdout, "{}", serde_json::to_string(event)?)?;
        stdout.flush()?;
        Ok(())
    })
    .await;

    match result {
        Ok(result) => {
            writeln!(
                stdout,
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "type": "result",
                    "result": result,
                }))
                .unwrap()
            )
            .unwrap();
            stdout.flush().unwrap();
        }
        Err(error) => {
            let runtime_error = error
                .downcast_ref::<ProviderRuntimeError>()
                .cloned()
                .unwrap_or_else(|| {
                    ProviderRuntimeError::normalize("invoke", error.to_string(), None)
                });
            writeln!(
                stdout,
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "type": "error",
                    "error": runtime_error,
                }))
                .unwrap()
            )
            .unwrap();
            writeln!(
                stdout,
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "type": "result",
                    "result": ProviderInvocationResult {
                        final_content: None,
                        response_id: None,
                        tool_calls: Vec::new(),
                        mcp_calls: Vec::new(),
                        usage: ProviderUsage::default(),
                        finish_reason: Some(ProviderFinishReason::Error),
                        provider_metadata: serde_json::json!({}),
                    },
                }))
                .unwrap()
            )
            .unwrap();
            stdout.flush().unwrap();
        }
    }
}
