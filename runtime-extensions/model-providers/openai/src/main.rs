use std::io::{self, BufRead, Write};

use openai_provider::{OpenAiProviderRuntime, ProviderStdioRequest, ProviderStdioResponse};

#[tokio::main]
async fn main() {
    let stdin = io::stdin();
    let mut runtime = OpenAiProviderRuntime::default();
    for line in stdin.lock().lines() {
        let line = line.unwrap_or_default();
        if line.trim().is_empty() {
            continue;
        }
        let request: ProviderStdioRequest =
            serde_json::from_str(&line).unwrap_or(ProviderStdioRequest {
                method: "invalid".to_string(),
                input: serde_json::Value::Null,
            });
        if request.method == "invoke" {
            run_streaming_invoke(&mut runtime, request).await;
            continue;
        }

        let response = runtime
            .handle_request(request)
            .await
            .unwrap_or_else(|error| {
                ProviderStdioResponse::error("provider_invalid_response", error.to_string())
            });
        println!("{}", serde_json::to_string(&response).unwrap());
        io::stdout().flush().unwrap();
    }
}

async fn run_streaming_invoke(runtime: &mut OpenAiProviderRuntime, request: ProviderStdioRequest) {
    let mut stdout = io::stdout().lock();
    let result = runtime
        .handle_invoke_request_streaming(request.input, |event| {
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
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
