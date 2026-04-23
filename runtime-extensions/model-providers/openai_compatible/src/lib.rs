use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    Method, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const PROVIDER_CODE: &str = "openai_compatible";
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
    organization: Option<String>,
    project: Option<String>,
    api_version: Option<String>,
    default_headers: BTreeMap<String, String>,
    #[allow(dead_code)]
    validate_model: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

impl ProviderUsage {
    fn has_any_value(&self) -> bool {
        self.input_tokens.is_some()
            || self.output_tokens.is_some()
            || self.reasoning_tokens.is_some()
            || self.cache_read_tokens.is_some()
            || self.cache_write_tokens.is_some()
            || self.total_tokens.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderModelDescriptor {
    pub model_id: String,
    pub display_name: String,
    pub source: String,
    pub supports_streaming: bool,
    pub supports_tool_call: bool,
    pub supports_multimodal: bool,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub provider_metadata: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderMessage {
    pub role: String,
    #[serde(default)]
    pub content: Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProviderInvocationInput {
    #[serde(default)]
    pub provider_instance_id: String,
    #[serde(default)]
    pub provider_code: String,
    #[serde(default)]
    pub protocol: String,
    pub model: String,
    #[serde(default)]
    pub provider_config: Value,
    #[serde(default)]
    pub messages: Vec<ProviderMessage>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub tools: Vec<Value>,
    #[serde(default)]
    pub response_format: Option<Value>,
    #[serde(default)]
    pub model_parameters: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderToolCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderMcpCall {
    pub id: String,
    pub server: String,
    pub method: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFinishReason {
    Stop,
    Length,
    ToolCall,
    ContentFilter,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProviderInvocationResult {
    pub final_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ProviderToolCall>,
    #[serde(default)]
    pub mcp_calls: Vec<ProviderMcpCall>,
    #[serde(default)]
    pub usage: ProviderUsage,
    pub finish_reason: Option<ProviderFinishReason>,
    #[serde(default)]
    pub provider_metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderStreamEvent {
    TextDelta { delta: String },
    ToolCallCommit { call: ProviderToolCall },
    UsageSnapshot { usage: ProviderUsage },
    Finish { reason: ProviderFinishReason },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuntimeInvocationEnvelope {
    pub events: Vec<ProviderStreamEvent>,
    pub result: ProviderInvocationResult,
}

pub async fn handle_request(request: ProviderStdioRequest) -> Result<ProviderStdioResponse> {
    match request.method.as_str() {
        "validate" => {
            let config = normalize_provider_config(&request.input)?;
            let payload = request_json(&config, "/models", Method::GET, None).await?;
            Ok(ProviderStdioResponse::ok(json!({
                "ok": true,
                "provider_code": PROVIDER_CODE,
                "sanitized": {
                    "base_url": config.base_url,
                    "api_key": "***",
                    "organization": config.organization,
                    "project": config.project,
                    "api_version": config.api_version,
                    "default_headers": config.default_headers.keys().collect::<Vec<_>>(),
                },
                "model_count": payload["data"].as_array().map(|items| items.len()).unwrap_or(0),
            })))
        }
        "list_models" => {
            let config = normalize_provider_config(&request.input)?;
            let payload = request_json(&config, "/models", Method::GET, None).await?;
            Ok(ProviderStdioResponse::ok(json!(normalize_model_entries(
                payload.get("data").unwrap_or(&Value::Null)
            )?)))
        }
        "invoke" => {
            let input: ProviderInvocationInput = serde_json::from_value(request.input)?;
            Ok(ProviderStdioResponse::ok(serde_json::to_value(
                invoke_chat_completion(input).await?,
            )?))
        }
        other => Ok(ProviderStdioResponse::error(
            "provider_invalid_response",
            format!("unsupported method: {other}"),
        )),
    }
}

fn normalize_provider_config(input: &Value) -> Result<ProviderConfig> {
    let config = input
        .as_object()
        .ok_or_else(|| anyhow!("provider_config must be an object"))?;

    Ok(ProviderConfig {
        base_url: require_text(config.get("base_url"), "base_url")?,
        api_key: require_text(config.get("api_key"), "api_key")?,
        organization: optional_text(config.get("organization")),
        project: optional_text(config.get("project")),
        api_version: optional_text(config.get("api_version")),
        default_headers: parse_default_headers(config.get("default_headers"))?,
        validate_model: config
            .get("validate_model")
            .and_then(Value::as_bool)
            .unwrap_or(DEFAULT_VALIDATE_MODEL),
    })
}

fn require_text(value: Option<&Value>, field: &str) -> Result<String> {
    let text = value
        .map(value_to_string)
        .unwrap_or_default()
        .trim()
        .to_string();
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

fn parse_default_headers(value: Option<&Value>) -> Result<BTreeMap<String, String>> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };

    match value {
        Value::Null => Ok(BTreeMap::new()),
        Value::String(text) if text.trim().is_empty() => Ok(BTreeMap::new()),
        Value::Object(object) => Ok(object
            .iter()
            .map(|(key, entry)| (key.clone(), value_to_string(entry)))
            .collect()),
        Value::String(text) => {
            let parsed: Value = serde_json::from_str(text)
                .with_context(|| "default_headers must be valid JSON")?;
            let object = parsed
                .as_object()
                .ok_or_else(|| anyhow!("default_headers must decode to a JSON object"))?;
            Ok(object
                .iter()
                .map(|(key, entry)| (key.clone(), value_to_string(entry)))
                .collect())
        }
        _ => bail!("default_headers must be a JSON object string"),
    }
}

fn build_headers(config: &ProviderConfig, include_json_body: bool) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    if include_json_body {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    }
    for (key, value) in &config.default_headers {
        let header_name = HeaderName::from_bytes(key.as_bytes())
            .with_context(|| format!("invalid default header name: {key}"))?;
        let header_value = HeaderValue::from_str(value)
            .with_context(|| format!("invalid default header value for {key}"))?;
        headers.insert(header_name, header_value);
    }
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", config.api_key))
            .context("invalid api_key for authorization header")?,
    );
    if let Some(organization) = &config.organization {
        headers.insert(
            HeaderName::from_static("openai-organization"),
            HeaderValue::from_str(organization).context("invalid organization header")?,
        );
    }
    if let Some(project) = &config.project {
        headers.insert(
            HeaderName::from_static("openai-project"),
            HeaderValue::from_str(project).context("invalid project header")?,
        );
    }
    Ok(headers)
}

fn build_url(config: &ProviderConfig, pathname: &str) -> Result<String> {
    let base_url = config.base_url.trim_end_matches('/');
    let mut url = Url::parse(&format!("{base_url}{pathname}"))
        .with_context(|| format!("invalid base_url: {}", config.base_url))?;
    if let Some(api_version) = &config.api_version {
        url.query_pairs_mut().append_pair("api-version", api_version);
    }
    Ok(url.to_string())
}

async fn request_json(
    config: &ProviderConfig,
    pathname: &str,
    method: Method,
    body: Option<Value>,
) -> Result<Value> {
    let client = reqwest::Client::new();
    let mut request = client
        .request(method.clone(), build_url(config, pathname)?)
        .headers(build_headers(config, body.is_some())?);
    if let Some(body) = body {
        request = request.json(&body);
    }

    let response = request.send().await?;
    let status = response.status();
    let payload = read_json_response(response).await?;
    if !status.is_success() {
        let message = payload
            .get("error")
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| payload.get("message").and_then(Value::as_str).map(ToOwned::to_owned))
            .unwrap_or_else(|| payload.to_string());
        bail!("{} {}: {}", status.as_u16(), status, message);
    }

    Ok(payload)
}

async fn read_json_response(response: reqwest::Response) -> Result<Value> {
    let text = response.text().await?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).with_context(|| "provider returned invalid JSON")
}

fn normalize_model_entries(data: &Value) -> Result<Vec<ProviderModelDescriptor>> {
    let Some(items) = data.as_array() else {
        return Ok(Vec::new());
    };

    items.iter().map(normalize_model_entry).collect()
}

fn explicit_number_alias(entry: &Value, aliases: &[&str]) -> Option<u64> {
    aliases.iter().find_map(|alias| entry.get(alias).and_then(number_or_none_ref))
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
        context_window: explicit_number_alias(
            entry,
            &["context_window", "context_length", "input_token_limit"],
        ),
        max_output_tokens: explicit_number_alias(
            entry,
            &["max_output_tokens", "output_token_limit", "max_tokens"],
        ),
        provider_metadata: json!({
            "owned_by": entry.get("owned_by").cloned().unwrap_or(Value::Null),
            "created": entry.get("created").cloned().unwrap_or(Value::Null),
        }),
    })
}

fn build_invocation_messages(input: &ProviderInvocationInput) -> Vec<Value> {
    let mut messages = Vec::new();
    if let Some(system) = input.system.as_deref().filter(|value| !value.trim().is_empty()) {
        messages.push(json!({
            "role": "system",
            "content": system,
        }));
    }
    for message in &input.messages {
        messages.push(json!({
            "role": message.role,
            "content": normalize_message_content(&message.content),
        }));
    }
    messages
}

fn normalize_message_content(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn parameter_value(input: &ProviderInvocationInput, key: &str) -> Option<Value> {
    input
        .model_parameters
        .get(key)
        .cloned()
        .or_else(|| input.extra.get(key).cloned())
        .filter(|value| !value.is_null())
}

async fn invoke_chat_completion(input: ProviderInvocationInput) -> Result<RuntimeInvocationEnvelope> {
    let config = normalize_provider_config(&input.provider_config)?;
    let mut body = Map::new();
    body.insert(
        "model".to_string(),
        Value::String(input.model.trim().to_string()),
    );
    body.insert(
        "messages".to_string(),
        Value::Array(build_invocation_messages(&input)),
    );
    body.insert("stream".to_string(), Value::Bool(false));
    if let Some(response_format) = input.response_format.clone() {
        body.insert("response_format".to_string(), response_format);
    }
    if !input.tools.is_empty() {
        body.insert("tools".to_string(), Value::Array(input.tools.clone()));
    }
    for key in ["temperature", "top_p", "max_tokens", "seed"] {
        if let Some(value) = parameter_value(&input, key) {
            body.insert(key.to_string(), value);
        }
    }

    let payload = request_json(
        &config,
        "/chat/completions",
        Method::POST,
        Some(Value::Object(body)),
    )
    .await?;
    let choice = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let message = choice.get("message").cloned().unwrap_or_else(|| json!({}));
    let text = extract_content(message.get("content"));
    let tool_calls = normalize_tool_calls(message.get("tool_calls"));
    let usage = normalize_usage(payload.get("usage").unwrap_or(&Value::Null));
    let finish_reason =
        normalize_finish_reason(choice.get("finish_reason").and_then(Value::as_str), &tool_calls);

    let mut events = Vec::new();
    if let Some(text) = text.clone().filter(|value| !value.is_empty()) {
        events.push(ProviderStreamEvent::TextDelta { delta: text });
    }
    for call in &tool_calls {
        events.push(ProviderStreamEvent::ToolCallCommit { call: call.clone() });
    }
    if usage.has_any_value() {
        events.push(ProviderStreamEvent::UsageSnapshot {
            usage: usage.clone(),
        });
    }
    events.push(ProviderStreamEvent::Finish {
        reason: finish_reason.clone(),
    });

    Ok(RuntimeInvocationEnvelope {
        events,
        result: ProviderInvocationResult {
            final_content: text,
            tool_calls,
            mcp_calls: Vec::new(),
            usage,
            finish_reason: Some(finish_reason),
            provider_metadata: json!({
                "request_model": input.model,
                "response_model": payload.get("model").cloned().unwrap_or(Value::Null),
                "response_id": payload.get("id").cloned().unwrap_or(Value::Null),
                "created": payload.get("created").cloned().unwrap_or(Value::Null),
                "system_fingerprint": payload
                    .get("system_fingerprint")
                    .cloned()
                    .unwrap_or(Value::Null),
            }),
        },
    })
}

fn extract_content(content: Option<&Value>) -> Option<String> {
    let content = content?;
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let joined = parts
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("");
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

fn normalize_tool_calls(tool_calls: Option<&Value>) -> Vec<ProviderToolCall> {
    let Some(tool_calls) = tool_calls.and_then(Value::as_array) else {
        return Vec::new();
    };

    tool_calls
        .iter()
        .enumerate()
        .map(|(index, tool_call)| ProviderToolCall {
            id: tool_call
                .get("id")
                .map(value_to_string)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| format!("tool_call_{}", index + 1)),
            name: tool_call
                .get("function")
                .and_then(|value| value.get("name"))
                .map(value_to_string)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "unknown_tool".to_string()),
            arguments: parse_tool_arguments(
                tool_call
                    .get("function")
                    .and_then(|value| value.get("arguments")),
            ),
        })
        .collect()
}

fn parse_tool_arguments(raw_arguments: Option<&Value>) -> Value {
    match raw_arguments {
        None | Some(Value::Null) => json!({}),
        Some(Value::String(text)) => serde_json::from_str(text)
            .unwrap_or_else(|_| json!({ "raw": text })),
        Some(other) => other.clone(),
    }
}

fn normalize_usage(usage: &Value) -> ProviderUsage {
    ProviderUsage {
        input_tokens: number_or_none(usage.get("prompt_tokens")),
        output_tokens: number_or_none(usage.get("completion_tokens")),
        reasoning_tokens: number_or_none(usage.get("reasoning_tokens")),
        cache_read_tokens: usage
            .get("prompt_tokens_details")
            .and_then(|value| value.get("cached_tokens"))
            .and_then(number_or_none_ref),
        cache_write_tokens: usage
            .get("completion_tokens_details")
            .and_then(|value| value.get("cached_tokens"))
            .and_then(number_or_none_ref),
        total_tokens: number_or_none(usage.get("total_tokens")),
    }
}

fn number_or_none(value: Option<&Value>) -> Option<u64> {
    value.and_then(number_or_none_ref)
}

fn number_or_none_ref(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_i64()
            .and_then(|raw| (raw >= 0).then_some(raw as u64))
    })
}

fn normalize_finish_reason(
    finish_reason: Option<&str>,
    tool_calls: &[ProviderToolCall],
) -> ProviderFinishReason {
    if !tool_calls.is_empty() || finish_reason == Some("tool_calls") {
        return ProviderFinishReason::ToolCall;
    }

    match finish_reason {
        Some("stop") => ProviderFinishReason::Stop,
        Some("length") => ProviderFinishReason::Length,
        Some("content_filter") => ProviderFinishReason::ContentFilter,
        _ => ProviderFinishReason::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_provider_config_requires_base_url_and_api_key() {
        let error = normalize_provider_config(&json!({ "base_url": "", "api_key": "" }))
            .expect_err("missing credentials must fail");

        assert!(error.to_string().contains("base_url"));
    }

    #[test]
    fn normalize_usage_maps_openai_usage_fields() {
        let usage = normalize_usage(&json!({
            "prompt_tokens": 5,
            "completion_tokens": 7,
            "total_tokens": 12
        }));

        assert_eq!(usage.input_tokens, Some(5));
        assert_eq!(usage.output_tokens, Some(7));
        assert_eq!(usage.total_tokens, Some(12));
    }

    #[test]
    fn normalize_model_entry_extracts_explicit_context_aliases() {
        let aliases = [
            json!({ "id": "gpt-4o-mini", "context_window": 128000 }),
            json!({ "id": "gpt-4o-mini", "context_length": 256000 }),
            json!({ "id": "gpt-4o-mini", "input_token_limit": 64000 }),
        ];

        let normalized = aliases
            .iter()
            .map(normalize_model_entry)
            .collect::<Result<Vec<_>>>()
            .expect("context aliases should normalize");

        assert_eq!(normalized[0].context_window, Some(128000));
        assert_eq!(normalized[1].context_window, Some(256000));
        assert_eq!(normalized[2].context_window, Some(64000));
    }

    #[test]
    fn normalize_model_entry_extracts_explicit_output_aliases() {
        let aliases = [
            json!({ "id": "gpt-4o-mini", "max_output_tokens": 8192 }),
            json!({ "id": "gpt-4o-mini", "output_token_limit": 4096 }),
            json!({ "id": "gpt-4o-mini", "max_tokens": 2048 }),
        ];

        let normalized = aliases
            .iter()
            .map(normalize_model_entry)
            .collect::<Result<Vec<_>>>()
            .expect("output aliases should normalize");

        assert_eq!(normalized[0].max_output_tokens, Some(8192));
        assert_eq!(normalized[1].max_output_tokens, Some(4096));
        assert_eq!(normalized[2].max_output_tokens, Some(2048));
    }

    #[test]
    fn normalize_model_entry_keeps_unknown_or_malformed_limits_as_none() {
        let descriptor = normalize_model_entry(&json!({
            "id": "gpt-4o-mini",
            "context_window": "128000",
            "max_output_tokens": "8192"
        }))
        .expect("model should still normalize");

        assert_eq!(descriptor.context_window, None);
        assert_eq!(descriptor.max_output_tokens, None);
    }
}
