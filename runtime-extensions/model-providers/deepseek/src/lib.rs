use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderStdioRequest {
    pub method: String,
    #[serde(default)]
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStdioError {
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub provider_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStdioResponse {
    pub ok: bool,
    #[serde(default)]
    pub result: Value,
    #[serde(default)]
    pub error: Option<ProviderStdioError>,
}

impl ProviderStdioResponse {
    pub fn ok(result: Value) -> Self {
        Self {
            ok: true,
            result,
            error: None,
        }
    }

    pub fn error(kind: &str, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            result: Value::Null,
            error: Some(ProviderStdioError {
                kind: kind.to_string(),
                message: message.into(),
                provider_summary: None,
            }),
        }
    }
}

pub async fn handle_request(request: ProviderStdioRequest) -> Result<ProviderStdioResponse> {
    match request.method.as_str() {
        "validate" => Ok(ProviderStdioResponse::ok(serde_json::json!({ "ok": true }))),
        "list_models" => Ok(ProviderStdioResponse::ok(serde_json::json!([]))),
        "balance" => Ok(ProviderStdioResponse::ok(serde_json::json!({
            "is_available": false,
            "balance_infos": []
        }))),
        "invoke" => bail!("invoke is not implemented in this scaffold"),
        other => Ok(ProviderStdioResponse::error(
            "provider_invalid_response",
            format!("unsupported method: {other}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handle_scaffold_methods() {
        let validate = handle_request(ProviderStdioRequest {
            method: "validate".to_string(),
            input: Value::Null,
        })
        .await
        .unwrap();
        assert!(validate.ok);

        let list_models = handle_request(ProviderStdioRequest {
            method: "list_models".to_string(),
            input: Value::Null,
        })
        .await
        .unwrap();
        assert_eq!(list_models.result, serde_json::json!([]));

        let balance = handle_request(ProviderStdioRequest {
            method: "balance".to_string(),
            input: Value::Null,
        })
        .await
        .unwrap();
        assert_eq!(
            balance.result,
            serde_json::json!({
                "is_available": false,
                "balance_infos": []
            })
        );
    }
}
