use std::io::{self, Read};

use openai_compatible_provider::{handle_request, ProviderStdioRequest, ProviderStdioResponse};

#[tokio::main]
async fn main() {
    let mut stdin = String::new();
    io::stdin().read_to_string(&mut stdin).unwrap();

    let request: ProviderStdioRequest =
        serde_json::from_str(&stdin).unwrap_or(ProviderStdioRequest {
            method: "invalid".to_string(),
            input: serde_json::Value::Null,
        });
    let is_invoke = request.method == "invoke";
    let response = handle_request(request).await.unwrap_or_else(|error| {
        ProviderStdioResponse::error("provider_invalid_response", error.to_string())
    });

    if is_invoke && response.ok {
        print_runtime_lines(response.result);
        return;
    }

    print!("{}", serde_json::to_string(&response).unwrap());
}

fn print_runtime_lines(output: serde_json::Value) {
    let events = output
        .get("events")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for event in events {
        println!("{}", serde_json::to_string(&event).unwrap());
    }
    let result = output
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "type": "result",
            "result": result,
        }))
        .unwrap()
    );
}
