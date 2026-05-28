use std::io::{self, BufRead, Write};

use aliyun_bailian_provider::{handle_request, ProviderStdioRequest, ProviderStdioResponse};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<ProviderStdioRequest>(&line) {
            Ok(request) => match handle_request(request).await {
                Ok(response) => response,
                Err(error) => {
                    ProviderStdioResponse::error("provider_invalid_response", error.to_string())
                }
            },
            Err(error) => {
                ProviderStdioResponse::error("provider_invalid_response", error.to_string())
            }
        };

        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

    Ok(())
}
