use std::io::{self, Read, Write};

use deepseek_provider::{
    handle_invoke_request_streaming, handle_request, ProviderStdioRequest, ProviderStdioResponse,
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

    if request.method == "invoke" {
        run_streaming_invoke(request).await;
        return;
    }

    let response = handle_request(request).await.unwrap_or_else(|error| {
        ProviderStdioResponse::error("provider_invalid_response", error.to_string())
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
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
