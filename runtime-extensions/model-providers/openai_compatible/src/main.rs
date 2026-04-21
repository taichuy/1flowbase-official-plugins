use std::io::{self, Read};

use openai_compatible_provider::{handle_request, ProviderStdioRequest, ProviderStdioResponse};

#[tokio::main]
async fn main() {
    let mut stdin = String::new();
    io::stdin().read_to_string(&mut stdin).unwrap();

    let request: ProviderStdioRequest = serde_json::from_str(&stdin).unwrap_or(ProviderStdioRequest {
        method: "invalid".to_string(),
        input: serde_json::Value::Null,
    });
    let response = handle_request(request).await.unwrap_or_else(|error| {
        ProviderStdioResponse::error("provider_invalid_response", error.to_string())
    });

    print!("{}", serde_json::to_string(&response).unwrap());
}
