use anyhow::{anyhow, bail, Context, Result};
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION},
    Method, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const PROVIDER_CODE: &str = "deepseek";
const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_VALIDATE_MODEL: bool = true;

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderConfig {
    base_url: String,
    api_key: String,
    validate_model: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ProviderModelDescriptor {
    model_id: String,
    display_name: String,
    source: String,
    supports_streaming: bool,
    supports_tool_call: bool,
    supports_multimodal: bool,
    context_window: Option<u64>,
    max_output_tokens: Option<u64>,
    provider_metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProviderBalanceResult {
    is_available: bool,
    #[serde(default)]
    balance_infos: Vec<ProviderBalanceInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProviderBalanceInfo {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

pub async fn handle_request(request: ProviderStdioRequest) -> Result<ProviderStdioResponse> {
    match request.method.as_str() {
        "validate" => validate_provider_config(&request.input).await,
        "list_models" => list_models(&request.input).await,
        "balance" => get_balance(&request.input).await,
        "invoke" => bail!("invoke is not implemented in this scaffold"),
        other => Ok(ProviderStdioResponse::error(
            "provider_invalid_response",
            format!("unsupported method: {other}"),
        )),
    }
}

async fn validate_provider_config(input: &Value) -> Result<ProviderStdioResponse> {
    let config = normalize_provider_config(input)?;

    if config.validate_model {
        if let Some(model_id) = configured_model_id(input) {
            let payload = request_json(&config, "/models", Method::GET).await?;
            let models = normalize_model_entries(payload.get("data").unwrap_or(&Value::Null))?;
            let exists = models.iter().any(|model| model.model_id == model_id);
            if !exists {
                bail!("configured model was not found");
            }
        }
    }

    Ok(ProviderStdioResponse::ok(json!({
        "ok": true,
        "provider_code": PROVIDER_CODE,
        "sanitized": {
            "base_url": config.base_url,
            "validate_model": config.validate_model
        }
    })))
}

async fn list_models(input: &Value) -> Result<ProviderStdioResponse> {
    let config = normalize_provider_config(input)?;
    let payload = request_json(&config, "/models", Method::GET).await?;
    Ok(ProviderStdioResponse::ok(json!(normalize_model_entries(
        payload.get("data").unwrap_or(&Value::Null)
    )?)))
}

async fn get_balance(input: &Value) -> Result<ProviderStdioResponse> {
    let config = normalize_provider_config(input)?;
    let payload = request_json(&config, "/user/balance", Method::GET).await?;
    Ok(ProviderStdioResponse::ok(json!(normalize_balance_payload(
        &payload
    )?)))
}

fn normalize_provider_config(input: &Value) -> Result<ProviderConfig> {
    let config = provider_config_object(input)?;
    let base_url =
        optional_text(config.get("base_url")).unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

    Ok(ProviderConfig {
        base_url,
        api_key: require_text(config.get("api_key"), "api_key")?,
        validate_model: config
            .get("validate_model")
            .and_then(Value::as_bool)
            .unwrap_or(DEFAULT_VALIDATE_MODEL),
    })
}

fn provider_config_object(input: &Value) -> Result<&Map<String, Value>> {
    let object = input
        .as_object()
        .ok_or_else(|| anyhow!("provider_config must be an object"))?;
    match object.get("provider_config") {
        Some(Value::Object(provider_config)) => Ok(provider_config),
        Some(_) => bail!("provider_config must be an object"),
        None => Ok(object),
    }
}

fn configured_model_id(input: &Value) -> Option<String> {
    let object = input.as_object()?;
    ["model", "model_id"]
        .iter()
        .find_map(|key| optional_text(object.get(*key)))
}

fn require_text(value: Option<&Value>, field: &str) -> Result<String> {
    let text = optional_text(value).unwrap_or_default();
    if text.is_empty() {
        bail!("{field} is required");
    }
    Ok(text)
}

fn optional_text(value: Option<&Value>) -> Option<String> {
    let text = value
        .map(value_to_string)
        .unwrap_or_default()
        .trim()
        .to_string();
    (!text.is_empty()).then_some(text)
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn normalize_model_entries(data: &Value) -> Result<Vec<ProviderModelDescriptor>> {
    let Some(items) = data.as_array() else {
        return Ok(Vec::new());
    };

    items.iter().map(normalize_model_entry).collect()
}

fn normalize_model_entry(entry: &Value) -> Result<ProviderModelDescriptor> {
    let model_id = entry
        .get("id")
        .or_else(|| entry.get("model_id"))
        .map(value_to_string)
        .unwrap_or_default()
        .trim()
        .to_string();
    if model_id.is_empty() {
        bail!("model_id is required");
    }

    Ok(ProviderModelDescriptor {
        model_id: model_id.clone(),
        display_name: model_id,
        source: "dynamic".to_string(),
        supports_streaming: true,
        supports_tool_call: true,
        supports_multimodal: false,
        context_window: Some(1_000_000),
        max_output_tokens: Some(384_000),
        provider_metadata: json!({
            "owned_by": PROVIDER_CODE,
            "reasoning": true,
            "pricing_source": "dynamic"
        }),
    })
}

fn normalize_balance_payload(payload: &Value) -> Result<ProviderBalanceResult> {
    serde_json::from_value(payload.clone()).with_context(|| "provider returned invalid balance")
}

fn build_headers(config: &ProviderConfig) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", config.api_key))
            .context("invalid api_key for authorization header")?,
    );
    Ok(headers)
}

fn build_url(config: &ProviderConfig, pathname: &str) -> Result<String> {
    let base_url = config.base_url.trim_end_matches('/');
    let url = Url::parse(&format!("{base_url}{pathname}"))
        .with_context(|| format!("invalid base_url: {}", config.base_url))?;
    Ok(url.to_string())
}

async fn request_json(config: &ProviderConfig, pathname: &str, method: Method) -> Result<Value> {
    let client = reqwest::Client::new();
    let response = client
        .request(method, build_url(config, pathname)?)
        .headers(build_headers(config)?)
        .send()
        .await
        .map_err(|error| sanitize_error(error, &config.api_key))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| sanitize_error(error, &config.api_key))?;
    let payload = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&text).with_context(|| "provider returned invalid JSON")?
    };

    if !status.is_success() {
        let message = provider_error_message(&payload).replace(&config.api_key, "***");
        bail!("{} {}: {}", status.as_u16(), status, message);
    }

    Ok(payload)
}

fn provider_error_message(payload: &Value) -> String {
    payload
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| payload.to_string())
}

fn sanitize_error(error: reqwest::Error, api_key: &str) -> anyhow::Error {
    let message = error.to_string().replace(api_key, "***");
    anyhow!(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_provider_config_defaults_base_url() {
        let config =
            normalize_provider_config(&serde_json::json!({ "api_key": "secret" })).unwrap();
        assert_eq!(config.base_url, "https://api.deepseek.com");
        assert_eq!(config.api_key, "secret");
        assert!(config.validate_model);
    }

    #[test]
    fn normalize_model_entry_merges_deepseek_static_metadata() {
        let model = normalize_model_entry(&serde_json::json!({
            "id": "deepseek-v4-pro",
            "object": "model",
            "owned_by": "unexpected-upstream-owner"
        }))
        .unwrap();

        assert_eq!(model.model_id, "deepseek-v4-pro");
        assert_eq!(model.context_window, Some(1_000_000));
        assert_eq!(model.max_output_tokens, Some(384_000));
        assert_eq!(model.provider_metadata["owned_by"], "deepseek");
        assert_eq!(model.provider_metadata["pricing_source"], "dynamic");
    }

    #[test]
    fn normalize_balance_payload_preserves_deepseek_balances() {
        let result = normalize_balance_payload(&serde_json::json!({
            "is_available": true,
            "balance_infos": [{
                "currency": "CNY",
                "total_balance": "110.00",
                "granted_balance": "10.00",
                "topped_up_balance": "100.00"
            }]
        }))
        .unwrap();

        assert!(result.is_available);
        assert_eq!(result.balance_infos[0].currency, "CNY");
        assert_eq!(result.balance_infos[0].total_balance, "110.00");
    }

    #[test]
    fn build_http_contract_uses_deepseek_paths_and_bearer_auth() {
        let config = normalize_provider_config(&json!({
            "base_url": "https://api.deepseek.com/",
            "api_key": "secret",
        }))
        .unwrap();

        assert_eq!(
            build_url(&config, "/models").unwrap(),
            "https://api.deepseek.com/models"
        );
        assert_eq!(
            build_url(&config, "/user/balance").unwrap(),
            "https://api.deepseek.com/user/balance"
        );

        let headers = build_headers(&config).unwrap();
        assert_eq!(headers.get(ACCEPT).unwrap(), "application/json");
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer secret");
    }

    #[tokio::test]
    async fn validate_scaffold_without_model_does_not_call_network() {
        let validate = handle_request(ProviderStdioRequest {
            method: "validate".to_string(),
            input: json!({ "api_key": "secret" }),
        })
        .await
        .unwrap();
        assert!(validate.ok);
        assert_eq!(validate.result["sanitized"]["api_key"], Value::Null);
        assert!(!validate.result.to_string().contains("secret"));
    }
}
