use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE},
    Method, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const PROVIDER_CODE: &str = "anthropic";
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_VALIDATE_MODEL: bool = true;
const DEFAULT_MAX_TOKENS: u64 = 4096;
const DEFAULT_THINKING_BUDGET_TOKENS: u64 = 1024;
const PASSTHROUGH_MESSAGES_PARAMETERS: &[&str] =
    &["temperature", "top_p", "top_k", "max_tokens", "tool_choice"];
const ANTHROPIC_CLIENT_PROTOCOL_HEADER_ALLOWLIST: &[&str] = &[
    "anthropic-version",
    "anthropic-beta",
    "x-claude-code-session-id",
    "anthropic-client-name",
    "anthropic-client-version",
    "x-client-name",
    "x-client-version",
    "user-agent",
];

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
    anthropic_version: String,
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

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderMessage {
    pub role: String,
    #[serde(default)]
    pub content: Value,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Value>,
    #[serde(default)]
    pub content_blocks: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProviderInvocationInput {
    #[serde(default)]
    pub provider_instance_id: String,
    #[serde(default)]
    pub provider_code: String,
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub previous_response_id: Option<String>,
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
    #[serde(default)]
    pub client_protocol_envelope: Option<ClientProtocolEnvelope>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClientProtocolEnvelope {
    #[serde(default)]
    pub source_protocol: String,
    #[serde(default)]
    pub policy: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
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
    ReasoningDelta { delta: String },
    ToolCallDelta { call_id: String, delta: Value },
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
        "validate" => validate_provider_config(&request.input).await,
        "list_models" => list_models(&request.input).await,
        "invoke" => {
            let input: ProviderInvocationInput = serde_json::from_value(request.input)?;
            let output = invoke_message(input).await?;
            Ok(ProviderStdioResponse::ok(serde_json::to_value(output)?))
        }
        other => Ok(ProviderStdioResponse::error(
            "provider_invalid_response",
            format!("unsupported method: {other}"),
        )),
    }
}

pub async fn handle_invoke_request_streaming<F>(
    input: Value,
    on_event: F,
) -> Result<ProviderInvocationResult>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let input: ProviderInvocationInput = serde_json::from_value(input)?;
    let output = invoke_message_with_event_sink(input, on_event).await?;
    Ok(output.result)
}

fn static_models() -> Vec<ProviderModelDescriptor> {
    vec![
        ProviderModelDescriptor {
            model_id: "claude-opus-4-1-20250805".to_string(),
            display_name: "Claude Opus 4.1".to_string(),
            source: "static".to_string(),
            supports_streaming: true,
            supports_tool_call: true,
            supports_multimodal: true,
            context_window: Some(200000),
            max_output_tokens: Some(32000),
            provider_metadata: json!({ "owned_by": "anthropic" }),
        },
        ProviderModelDescriptor {
            model_id: "claude-sonnet-4-20250514".to_string(),
            display_name: "Claude Sonnet 4".to_string(),
            source: "static".to_string(),
            supports_streaming: true,
            supports_tool_call: true,
            supports_multimodal: true,
            context_window: Some(200000),
            max_output_tokens: Some(64000),
            provider_metadata: json!({ "owned_by": "anthropic" }),
        },
        ProviderModelDescriptor {
            model_id: "claude-3-5-haiku-20241022".to_string(),
            display_name: "Claude Haiku 3.5".to_string(),
            source: "static".to_string(),
            supports_streaming: true,
            supports_tool_call: true,
            supports_multimodal: true,
            context_window: Some(200000),
            max_output_tokens: Some(8192),
            provider_metadata: json!({ "owned_by": "anthropic" }),
        },
    ]
}

async fn validate_provider_config(input: &Value) -> Result<ProviderStdioResponse> {
    let config = normalize_provider_config(input)?;
    let mut model_count = 0;

    if config.validate_model {
        let models = fetch_dynamic_models(&config).await?;
        model_count = models.len();

        if let Some(model_id) = configured_model_id(input) {
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
            "api_key": "***",
            "anthropic_version": config.anthropic_version,
            "validate_model": config.validate_model,
        },
        "model_count": model_count,
    })))
}

async fn list_models(input: &Value) -> Result<ProviderStdioResponse> {
    let config = normalize_provider_config(input)?;
    if !config.validate_model {
        return Ok(ProviderStdioResponse::ok(json!(static_models())));
    }

    let dynamic = fetch_dynamic_models(&config).await?;
    let models = if dynamic.is_empty() {
        static_models()
    } else {
        dynamic
    };
    Ok(ProviderStdioResponse::ok(json!(models)))
}

fn normalize_provider_config(input: &Value) -> Result<ProviderConfig> {
    let config = provider_config_object(input)?;
    Ok(ProviderConfig {
        base_url: optional_text(config.get("base_url"))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        api_key: require_text(config.get("api_key"), "api_key")?,
        anthropic_version: optional_text(config.get("anthropic_version"))
            .unwrap_or_else(|| DEFAULT_ANTHROPIC_VERSION.to_string()),
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

fn build_headers(
    config: &ProviderConfig,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        HeaderName::from_static("x-api-key"),
        HeaderValue::from_str(&config.api_key).context("invalid api_key header")?,
    );
    headers.insert(
        HeaderName::from_static("anthropic-version"),
        HeaderValue::from_str(&config.anthropic_version)
            .context("invalid anthropic_version header")?,
    );
    apply_client_protocol_headers(&mut headers, client_protocol_envelope)?;
    Ok(headers)
}

fn apply_client_protocol_headers(
    headers: &mut HeaderMap,
    envelope: Option<&ClientProtocolEnvelope>,
) -> Result<()> {
    let Some(envelope) = envelope else {
        return Ok(());
    };
    if envelope.source_protocol != "anthropic_messages"
        || envelope.policy != "anthropic_messages_v1"
    {
        return Ok(());
    }
    for name in ANTHROPIC_CLIENT_PROTOCOL_HEADER_ALLOWLIST {
        let name = *name;
        let Some(value) = envelope.headers.get(name).map(String::as_str) else {
            continue;
        };
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_str(value)
                .with_context(|| format!("invalid client protocol header: {name}"))?,
        );
    }
    Ok(())
}

fn build_url(config: &ProviderConfig, pathname: &str) -> Result<String> {
    build_url_with_query(config, pathname, &[])
}

fn build_url_with_query(
    config: &ProviderConfig,
    pathname: &str,
    query: &[(&str, &str)],
) -> Result<String> {
    let base_url = config.base_url.trim_end_matches('/');
    let mut url = Url::parse(&format!("{base_url}{pathname}"))
        .with_context(|| format!("invalid base_url: {}", config.base_url))?;
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query.iter().copied());
    }
    Ok(url.to_string())
}

async fn fetch_dynamic_models(config: &ProviderConfig) -> Result<Vec<ProviderModelDescriptor>> {
    let mut models = Vec::new();
    let mut after_id: Option<String> = None;

    loop {
        let mut query = vec![("limit", "1000")];
        if let Some(after_id) = after_id.as_deref() {
            query.push(("after_id", after_id));
        }
        let payload = request_json_with_query(config, "/v1/models", Method::GET, &query).await?;
        models.extend(normalize_model_entries(
            payload.get("data").unwrap_or(&Value::Null),
        )?);

        if !payload
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            break;
        }

        after_id = Some(
            payload
                .get("last_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("model list response missing last_id for next page"))?
                .to_string(),
        );
    }

    Ok(models)
}

async fn request_json_with_query(
    config: &ProviderConfig,
    pathname: &str,
    method: Method,
    query: &[(&str, &str)],
) -> Result<Value> {
    let response = reqwest::Client::new()
        .request(method, build_url_with_query(config, pathname, query)?)
        .headers(build_headers(config, None)?)
        .send()
        .await?;
    let status = response.status();
    let payload = read_json_response(response).await?;
    ensure_success_status(status, &payload)?;
    Ok(payload)
}

fn ensure_success_status(status: reqwest::StatusCode, payload: &Value) -> Result<()> {
    if !status.is_success() {
        let message = payload
            .get("error")
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .or_else(|| payload.as_str())
            .unwrap_or("provider request failed");
        bail!("{} {}: {}", status.as_u16(), status, message);
    }
    Ok(())
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
        display_name: optional_text(entry.get("display_name"))
            .or_else(|| optional_text(entry.get("displayName")))
            .unwrap_or_else(|| model_id.clone()),
        source: "dynamic".to_string(),
        supports_streaming: true,
        supports_tool_call: true,
        supports_multimodal: true,
        context_window: None,
        max_output_tokens: None,
        provider_metadata: json!({
            "owned_by": "anthropic",
            "type": entry.get("type").cloned().unwrap_or(Value::Null),
            "created_at": entry.get("created_at").cloned().unwrap_or(Value::Null),
            "pricing_source": "dynamic",
        }),
    })
}

async fn invoke_message(input: ProviderInvocationInput) -> Result<RuntimeInvocationEnvelope> {
    invoke_message_with_event_sink(input, |_| Ok(())).await
}

async fn invoke_message_with_event_sink<F>(
    input: ProviderInvocationInput,
    mut on_event: F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let config = normalize_provider_config(&input.provider_config)?;
    let body = build_messages_body(&input)?;
    let response = reqwest::Client::new()
        .request(Method::POST, build_url(&config, "/v1/messages")?)
        .headers(build_headers(
            &config,
            input.client_protocol_envelope.as_ref(),
        )?)
        .json(&body)
        .send()
        .await?;
    read_streaming_message(response, input.model, &mut on_event).await
}

fn build_messages_body(input: &ProviderInvocationInput) -> Result<Value> {
    if input.model.trim().is_empty() {
        bail!("model is required");
    }
    let mut body = Map::new();
    body.insert(
        "model".to_string(),
        Value::String(input.model.trim().to_string()),
    );
    body.insert(
        "messages".to_string(),
        Value::Array(build_anthropic_messages(input)),
    );
    body.insert("stream".to_string(), Value::Bool(true));
    let max_tokens = parameter_u64(input, "max_tokens").unwrap_or(DEFAULT_MAX_TOKENS);
    body.insert("max_tokens".to_string(), json!(max_tokens));
    if let Some(system) = input
        .system
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        body.insert("system".to_string(), Value::String(system.to_string()));
    }
    if !input.tools.is_empty() {
        body.insert(
            "tools".to_string(),
            Value::Array(build_anthropic_tools(&input.tools)),
        );
    }
    if let Some(thinking_type) = parameter_value(input, "thinking_type") {
        if thinking_type.as_str() == Some("enabled") {
            let budget_tokens = parameter_u64(input, "thinking_budget_tokens")
                .unwrap_or(DEFAULT_THINKING_BUDGET_TOKENS);
            if budget_tokens >= max_tokens {
                bail!("thinking_budget_tokens must be lower than max_tokens");
            }
            body.insert(
                "thinking".to_string(),
                json!({ "type": "enabled", "budget_tokens": budget_tokens }),
            );
        }
    }
    for key in PASSTHROUGH_MESSAGES_PARAMETERS {
        if *key == "max_tokens" {
            continue;
        }
        if let Some(value) = parameter_value(input, key) {
            body.insert(
                (*key).to_string(),
                normalize_anthropic_parameter(key, value),
            );
        }
    }
    Ok(Value::Object(body))
}

fn build_anthropic_messages(input: &ProviderInvocationInput) -> Vec<Value> {
    let mut messages = Vec::new();
    for message in &input.messages {
        let role = message.role.trim().to_ascii_lowercase();
        if role == "system" || role == "developer" {
            continue;
        }
        if role == "tool" {
            if let Some(tool_use_id) = message.tool_call_id.as_deref() {
                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": tool_result_content(message),
                    }]
                }));
            }
            continue;
        }
        let mut content = message_content_blocks(message);
        append_tool_use_blocks(&mut content, message.tool_calls.as_ref());
        if !content.is_empty() {
            messages.push(json!({
                "role": if role == "assistant" { "assistant" } else { "user" },
                "content": content,
            }));
        }
    }
    messages
}

fn message_content_blocks(message: &ProviderMessage) -> Vec<Value> {
    if let Some(content_blocks) = message.content_blocks.as_ref() {
        let blocks = content_blocks_from_value(content_blocks);
        if !blocks.is_empty() {
            return blocks;
        }
    }
    text_content_block(&message.content).into_iter().collect()
}

fn content_blocks_from_value(content: &Value) -> Vec<Value> {
    match content {
        Value::Array(parts) => parts.iter().filter_map(content_block_from_part).collect(),
        Value::Object(object) => {
            if let Some(parts) = object.get("parts").and_then(Value::as_array) {
                return parts.iter().filter_map(content_block_from_part).collect();
            }
            content_block_from_part(content).into_iter().collect()
        }
        _ => text_content_block(content).into_iter().collect(),
    }
}

fn content_block_from_part(part: &Value) -> Option<Value> {
    match part {
        Value::String(text) => {
            (!text.trim().is_empty()).then(|| json!({ "type": "text", "text": text }))
        }
        Value::Object(object) => {
            let part_type = object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            match part_type.as_str() {
                "text" | "input_text" => text_block_from_object(object),
                "image" | "image_url" | "input_image" => image_block_from_object(object),
                "document" | "input_document" => document_block_from_object(object),
                "tool_use" | "tool_result" => Some(Value::Object(object.clone())),
                _ => object
                    .get("text")
                    .or_else(|| object.get("content"))
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .map(|text| json!({ "type": "text", "text": text })),
            }
        }
        Value::Null => None,
        other => text_content_block(other),
    }
}

fn text_block_from_object(object: &Map<String, Value>) -> Option<Value> {
    let text = object
        .get("text")
        .or_else(|| object.get("content"))
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())?;
    let mut block = Map::new();
    block.insert("type".to_string(), Value::String("text".to_string()));
    block.insert("text".to_string(), Value::String(text.to_string()));
    copy_optional_field(object, &mut block, "cache_control");
    Some(Value::Object(block))
}

fn image_block_from_object(object: &Map<String, Value>) -> Option<Value> {
    if object.get("source").is_some() {
        let mut block = object.clone();
        block.insert("type".to_string(), Value::String("image".to_string()));
        return Some(Value::Object(block));
    }
    let url = url_from_block_value(
        object
            .get("image_url")
            .or_else(|| object.get("image"))
            .or_else(|| object.get("url"))?,
    )?;
    let mut block = Map::new();
    block.insert("type".to_string(), Value::String("image".to_string()));
    block.insert(
        "source".to_string(),
        json!({
            "type": "url",
            "url": url,
        }),
    );
    copy_optional_field(object, &mut block, "cache_control");
    Some(Value::Object(block))
}

fn document_block_from_object(object: &Map<String, Value>) -> Option<Value> {
    if object.get("source").is_some() {
        let mut block = object.clone();
        block.insert("type".to_string(), Value::String("document".to_string()));
        return Some(Value::Object(block));
    }
    let url = object
        .get("document")
        .or_else(|| object.get("document_url"))
        .or_else(|| object.get("url"))
        .and_then(url_from_block_value)?;
    let mut block = Map::new();
    block.insert("type".to_string(), Value::String("document".to_string()));
    block.insert(
        "source".to_string(),
        json!({
            "type": "url",
            "url": url,
        }),
    );
    copy_optional_field(object, &mut block, "title");
    copy_optional_field(object, &mut block, "cache_control");
    Some(Value::Object(block))
}

fn url_from_block_value(value: &Value) -> Option<String> {
    let url = match value {
        Value::String(text) => text.as_str(),
        Value::Object(object) => object.get("url").and_then(Value::as_str)?,
        _ => return None,
    }
    .trim();
    (!url.is_empty()).then(|| url.to_string())
}

fn copy_optional_field(from: &Map<String, Value>, to: &mut Map<String, Value>, key: &str) {
    if let Some(value) = from.get(key) {
        to.insert(key.to_string(), value.clone());
    }
}

fn text_content_block(content: &Value) -> Option<Value> {
    let text = normalize_message_content(content);
    (!text.trim().is_empty()).then(|| json!({ "type": "text", "text": text }))
}

fn tool_result_content(message: &ProviderMessage) -> Value {
    if let Some(content_blocks) = message.content_blocks.as_ref() {
        let blocks = content_blocks_from_value(content_blocks);
        if !blocks.is_empty() {
            return Value::Array(blocks);
        }
    }
    Value::String(normalize_message_content(&message.content))
}

fn append_tool_use_blocks(content: &mut Vec<Value>, tool_calls: Option<&Value>) {
    let Some(calls) = tool_calls.and_then(Value::as_array) else {
        return;
    };
    for (index, call) in calls.iter().enumerate() {
        if let Some(block) = tool_use_block_from_native(call, index) {
            content.push(block);
        }
    }
}

fn tool_use_block_from_native(tool_call: &Value, index: usize) -> Option<Value> {
    let object = tool_call.as_object()?;
    let function = object.get("function").and_then(Value::as_object);
    let name = function
        .and_then(|value| value.get("name"))
        .or_else(|| object.get("name"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())?;
    let input = function
        .and_then(|value| value.get("arguments"))
        .or_else(|| object.get("arguments"))
        .cloned()
        .map(normalize_tool_input)
        .unwrap_or_else(|| json!({}));
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("toolu_{index}"));
    Some(json!({
        "type": "tool_use",
        "id": id,
        "name": name,
        "input": input,
    }))
}

fn build_anthropic_tools(tools: &[Value]) -> Vec<Value> {
    tools.iter().map(build_anthropic_tool).collect()
}

fn build_anthropic_tool(tool: &Value) -> Value {
    if tool.get("type").and_then(Value::as_str) != Some("function") {
        return tool.clone();
    }
    let Some(function) = tool.get("function").and_then(Value::as_object) else {
        return tool.clone();
    };
    let mut mapped = Map::new();
    if let Some(name) = function.get("name") {
        mapped.insert("name".to_string(), name.clone());
    }
    if let Some(description) = function.get("description") {
        mapped.insert("description".to_string(), description.clone());
    }
    mapped.insert(
        "input_schema".to_string(),
        function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object" })),
    );
    Value::Object(mapped)
}

fn normalize_message_content(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .or_else(|| part.get("content"))
                    .and_then(Value::as_str)
            })
            .collect::<Vec<_>>()
            .join(""),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn normalize_tool_input(input: Value) -> Value {
    match input {
        Value::String(text) => serde_json::from_str::<Value>(&text)
            .ok()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({ "value": text })),
        Value::Null => json!({}),
        other => other,
    }
}

fn parameter_value(input: &ProviderInvocationInput, key: &str) -> Option<Value> {
    input
        .model_parameters
        .get(key)
        .cloned()
        .or_else(|| input.extra.get(key).cloned())
        .and_then(normalize_scalar_parameter)
}

fn parameter_u64(input: &ProviderInvocationInput, key: &str) -> Option<u64> {
    parameter_value(input, key).and_then(|value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse::<u64>().ok(),
        _ => None,
    })
}

fn normalize_scalar_parameter(value: Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then_some(Value::String(trimmed.to_string()))
        }
        other => Some(other),
    }
}

fn normalize_anthropic_parameter(key: &str, value: Value) -> Value {
    if key == "tool_choice" {
        match value.as_str() {
            Some("required") | Some("any") => json!({ "type": "any" }),
            Some("none") => json!({ "type": "none" }),
            Some("auto") => json!({ "type": "auto" }),
            _ => value,
        }
    } else {
        value
    }
}

async fn read_streaming_message<F>(
    response: reqwest::Response,
    request_model: String,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let status = response.status();
    if !status.is_success() {
        let payload = response.text().await.unwrap_or_default();
        bail!("{} {}: {}", status.as_u16(), status, payload);
    }
    let mut text = String::new();
    let mut events = Vec::new();
    let mut all_events = Vec::new();
    let mut tool_builders = BTreeMap::new();
    let mut usage = ProviderUsage::default();
    let mut finish_reason = ProviderFinishReason::Unknown;
    let mut message_id = Value::Null;
    let mut buffer = String::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        buffer.push_str(&String::from_utf8_lossy(&chunk?));
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].to_string();
            buffer = buffer[index + 1..].to_string();
            process_anthropic_sse_line(
                &line,
                &mut events,
                &mut text,
                &mut tool_builders,
                &mut usage,
                &mut finish_reason,
                &mut message_id,
            )?;
            emit_new_events(&events, on_event)?;
            all_events.append(&mut events);
        }
    }
    let tool_calls = tool_builders
        .into_values()
        .map(ToolUseBuilder::into_tool_call)
        .collect::<Vec<_>>();
    if usage.has_any_value() {
        events.push(ProviderStreamEvent::UsageSnapshot {
            usage: usage.clone(),
        });
    }
    for call in &tool_calls {
        events.push(ProviderStreamEvent::ToolCallCommit { call: call.clone() });
    }
    events.push(ProviderStreamEvent::Finish {
        reason: finish_reason.clone(),
    });
    emit_new_events(&events, on_event)?;
    all_events.extend(events);
    Ok(RuntimeInvocationEnvelope {
        events: all_events,
        result: ProviderInvocationResult {
            final_content: (!text.is_empty()).then_some(text),
            response_id: None,
            tool_calls,
            mcp_calls: Vec::new(),
            usage,
            finish_reason: Some(finish_reason),
            provider_metadata: json!({
                "request_model": request_model,
                "message_id": message_id,
            }),
        },
    })
}

#[derive(Default)]
struct ToolUseBuilder {
    id: String,
    name: String,
    input_json: String,
}

impl ToolUseBuilder {
    fn into_tool_call(self) -> ProviderToolCall {
        ProviderToolCall {
            id: self.id,
            name: self.name,
            arguments: serde_json::from_str(&self.input_json).unwrap_or_else(|_| json!({})),
        }
    }
}

fn process_anthropic_sse_line(
    line: &str,
    events: &mut Vec<ProviderStreamEvent>,
    text: &mut String,
    tool_builders: &mut BTreeMap<usize, ToolUseBuilder>,
    usage: &mut ProviderUsage,
    finish_reason: &mut ProviderFinishReason,
    message_id: &mut Value,
) -> Result<()> {
    let line = line.trim();
    if !line.starts_with("data:") {
        return Ok(());
    }
    let data = line.trim_start_matches("data:").trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let payload: Value = serde_json::from_str(data)?;
    match payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "message_start" => {
            if let Some(message) = payload.get("message") {
                if let Some(id) = message.get("id") {
                    *message_id = id.clone();
                }
                *usage = normalize_usage(message.get("usage").unwrap_or(&Value::Null));
            }
        }
        "content_block_start" => {
            let index = payload.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            if let Some(block) = payload.get("content_block") {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    tool_builders.insert(
                        index,
                        ToolUseBuilder {
                            id: block.get("id").map(value_to_string).unwrap_or_default(),
                            name: block.get("name").map(value_to_string).unwrap_or_default(),
                            input_json: block
                                .get("input")
                                .filter(|value| !is_empty_tool_input(value))
                                .map(Value::to_string)
                                .unwrap_or_default(),
                        },
                    );
                }
            }
        }
        "content_block_delta" => {
            let index = payload.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let delta = payload.get("delta").unwrap_or(&Value::Null);
            match delta
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "text_delta" => {
                    if let Some(value) = delta.get("text").and_then(Value::as_str) {
                        text.push_str(value);
                        events.push(ProviderStreamEvent::TextDelta {
                            delta: value.to_string(),
                        });
                    }
                }
                "input_json_delta" => {
                    if let Some(value) = delta.get("partial_json").and_then(Value::as_str) {
                        if let Some(builder) = tool_builders.get_mut(&index) {
                            builder.input_json.push_str(value);
                            events.push(ProviderStreamEvent::ToolCallDelta {
                                call_id: builder.id.clone(),
                                delta: Value::String(value.to_string()),
                            });
                        }
                    }
                }
                "thinking_delta" => {
                    if let Some(value) = delta.get("thinking").and_then(Value::as_str) {
                        events.push(ProviderStreamEvent::ReasoningDelta {
                            delta: value.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        "message_delta" => {
            if let Some(delta) = payload.get("delta") {
                if let Some(reason) = delta.get("stop_reason").and_then(Value::as_str) {
                    *finish_reason = normalize_stop_reason(reason);
                }
            }
            if let Some(snapshot) = payload.get("usage") {
                merge_usage(usage, normalize_usage(snapshot));
            }
        }
        _ => {}
    }
    Ok(())
}

fn normalize_usage(raw: &Value) -> ProviderUsage {
    let input = raw.get("input_tokens").and_then(Value::as_u64);
    let output = raw.get("output_tokens").and_then(Value::as_u64);
    ProviderUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: input.zip(output).map(|(left, right)| left + right),
        reasoning_tokens: None,
        cache_read_tokens: raw.get("cache_read_input_tokens").and_then(Value::as_u64),
        cache_write_tokens: raw
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64),
    }
}

fn merge_usage(current: &mut ProviderUsage, snapshot: ProviderUsage) {
    current.input_tokens = snapshot.input_tokens.or(current.input_tokens);
    current.output_tokens = snapshot.output_tokens.or(current.output_tokens);
    current.reasoning_tokens = snapshot.reasoning_tokens.or(current.reasoning_tokens);
    current.cache_read_tokens = snapshot.cache_read_tokens.or(current.cache_read_tokens);
    current.cache_write_tokens = snapshot.cache_write_tokens.or(current.cache_write_tokens);
    current.total_tokens = current
        .input_tokens
        .zip(current.output_tokens)
        .map(|(left, right)| left + right)
        .or(snapshot.total_tokens)
        .or(current.total_tokens);
}

fn is_empty_tool_input(value: &Value) -> bool {
    value.is_null()
        || value
            .as_object()
            .map(|object| object.is_empty())
            .unwrap_or(false)
}

fn normalize_stop_reason(reason: &str) -> ProviderFinishReason {
    match reason {
        "end_turn" | "stop_sequence" => ProviderFinishReason::Stop,
        "max_tokens" => ProviderFinishReason::Length,
        "tool_use" => ProviderFinishReason::ToolCall,
        _ => ProviderFinishReason::Unknown,
    }
}

fn emit_new_events<F>(events: &[ProviderStreamEvent], on_event: &mut F) -> Result<()>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    for event in events {
        on_event(event)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Duration,
    };

    fn start_sse_server(response_body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = format!("http://{}", listener.local_addr().expect("listener addr"));

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should connect");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout");
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .expect("response should be writable");
        });

        address
    }

    fn start_json_server(response_body: &'static str) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = format!("http://{}", listener.local_addr().expect("listener addr"));
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should connect");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout");
            let mut buffer = [0_u8; 4096];
            let byte_count = stream
                .read(&mut buffer)
                .expect("request should be readable");
            sender
                .send(String::from_utf8_lossy(&buffer[..byte_count]).to_string())
                .expect("request capture should send");
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .expect("response should be writable");
        });

        (address, receiver)
    }

    fn start_json_sequence_server(
        response_bodies: Vec<&'static str>,
    ) -> (String, mpsc::Receiver<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = format!("http://{}", listener.local_addr().expect("listener addr"));
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let mut requests = Vec::new();
            for response_body in response_bodies {
                let (mut stream, _) = listener.accept().expect("request should connect");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("read timeout");
                let mut buffer = [0_u8; 4096];
                let byte_count = stream
                    .read(&mut buffer)
                    .expect("request should be readable");
                requests.push(String::from_utf8_lossy(&buffer[..byte_count]).to_string());
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("response should be writable");
            }
            sender.send(requests).expect("request captures should send");
        });

        (address, receiver)
    }

    #[test]
    fn normalizes_anthropic_models_payload() {
        let models = normalize_model_entries(&json!([
            {
                "type": "model",
                "id": "claude-live-20260601",
                "display_name": "Claude Live",
                "created_at": "2026-06-01T00:00:00Z"
            }
        ]))
        .unwrap();

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_id, "claude-live-20260601");
        assert_eq!(models[0].display_name, "Claude Live");
        assert_eq!(models[0].source, "dynamic");
        assert_eq!(models[0].provider_metadata["owned_by"], "anthropic");
        assert_eq!(
            models[0].provider_metadata["created_at"],
            "2026-06-01T00:00:00Z"
        );
    }

    #[tokio::test]
    async fn list_models_fetches_anthropic_v1_models() {
        let (base_url, request_receiver) = start_json_server(
            r#"{"data":[{"type":"model","id":"claude-live-20260601","display_name":"Claude Live","created_at":"2026-06-01T00:00:00Z"}]}"#,
        );

        let response = list_models(&json!({
            "base_url": base_url,
            "api_key": "test-key",
            "anthropic_version": "2023-06-01"
        }))
        .await
        .unwrap();
        let captured_request = request_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("request should be captured");

        assert!(response.ok);
        assert_eq!(response.result[0]["model_id"], "claude-live-20260601");
        assert!(captured_request.starts_with("GET /v1/models?limit=1000 HTTP/1.1"));
        assert!(captured_request.contains("x-api-key: test-key"));
        assert!(captured_request.contains("anthropic-version: 2023-06-01"));
    }

    #[tokio::test]
    async fn list_models_follows_anthropic_pagination() {
        let (base_url, request_receiver) = start_json_sequence_server(vec![
            r#"{"data":[{"type":"model","id":"claude-page-1","display_name":"Claude Page 1"}],"has_more":true,"last_id":"claude-page-1"}"#,
            r#"{"data":[{"type":"model","id":"claude-page-2","display_name":"Claude Page 2"}],"has_more":false}"#,
        ]);

        let response = list_models(&json!({
            "base_url": base_url,
            "api_key": "test-key",
            "anthropic_version": "2023-06-01"
        }))
        .await
        .unwrap();
        let captured_requests = request_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("requests should be captured");

        assert_eq!(response.result.as_array().unwrap().len(), 2);
        assert_eq!(response.result[0]["model_id"], "claude-page-1");
        assert_eq!(response.result[1]["model_id"], "claude-page-2");
        assert!(captured_requests[0].starts_with("GET /v1/models?limit=1000 HTTP/1.1"));
        assert!(captured_requests[1]
            .starts_with("GET /v1/models?limit=1000&after_id=claude-page-1 HTTP/1.1"));
    }

    #[test]
    fn messages_body_maps_native_tool_calls_and_tool_results() {
        let input = ProviderInvocationInput {
            model: "claude-sonnet-4-20250514".to_string(),
            system: Some("Be concise".to_string()),
            messages: vec![
                ProviderMessage {
                    role: "assistant".to_string(),
                    content: Value::Null,
                    name: None,
                    tool_call_id: None,
                    tool_calls: Some(
                        json!([{ "id": "toolu_1", "name": "lookup", "arguments": { "query": "refund" }}]),
                    ),
                    content_blocks: None,
                },
                ProviderMessage {
                    role: "tool".to_string(),
                    content: json!("found"),
                    name: None,
                    tool_call_id: Some("toolu_1".to_string()),
                    tool_calls: None,
                    content_blocks: None,
                },
            ],
            tools: vec![json!({
                "type": "function",
                "function": {
                    "name": "lookup",
                    "description": "Lookup docs",
                    "parameters": { "type": "object" }
                }
            })],
            model_parameters: BTreeMap::from([
                ("tool_choice".to_string(), json!("required")),
                ("max_tokens".to_string(), json!(2048)),
                ("thinking_type".to_string(), json!("enabled")),
                ("thinking_budget_tokens".to_string(), json!(1024)),
            ]),
            ..Default::default()
        };

        let body = build_messages_body(&input).unwrap();
        assert_eq!(body["system"], "Be concise");
        assert_eq!(body["max_tokens"], 2048);
        assert_eq!(
            body["thinking"],
            json!({ "type": "enabled", "budget_tokens": 1024 })
        );
        assert_eq!(body["tool_choice"], json!({ "type": "any" }));
        assert_eq!(body["tools"][0]["name"], "lookup");
        assert_eq!(
            body["tools"][0]["input_schema"],
            json!({ "type": "object" })
        );
        assert_eq!(body["messages"][0]["role"], "assistant");
        assert_eq!(body["messages"][0]["content"][0]["type"], "tool_use");
        assert_eq!(body["messages"][0]["content"][0]["id"], "toolu_1");
        assert_eq!(
            body["messages"][0]["content"][0]["input"]["query"],
            "refund"
        );
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][1]["content"][0]["tool_use_id"], "toolu_1");
    }

    #[test]
    fn messages_body_preserves_media_tool_result_content_blocks() {
        let input = ProviderInvocationInput {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![ProviderMessage {
                role: "tool".to_string(),
                content: Value::Null,
                name: None,
                tool_call_id: Some("toolu_image".to_string()),
                tool_calls: None,
                content_blocks: Some(json!([
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": "aW1hZ2U="
                        }
                    }
                ])),
            }],
            ..Default::default()
        };

        let body = build_messages_body(&input).unwrap();

        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["type"], "tool_result");
        assert_eq!(
            body["messages"][0]["content"][0]["tool_use_id"],
            "toolu_image"
        );
        assert_eq!(
            body["messages"][0]["content"][0]["content"][0]["type"],
            "image"
        );
        assert_eq!(
            body["messages"][0]["content"][0]["content"][0]["source"]["media_type"],
            "image/png"
        );
    }

    #[test]
    fn headers_restore_anthropic_client_protocol_envelope_and_keep_config_auth() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "client_protocol_envelope": {
                "source_protocol": "anthropic_messages",
                "policy": "anthropic_messages_v1",
                "headers": {
                    "anthropic-version": "2023-06-01",
                    "anthropic-beta": "prompt-caching",
                    "x-claude-code-session-id": "session-123",
                    "user-agent": "ClaudeCode/1.0",
                    "x-api-key": "client-auth-must-not-win"
                }
            }
        }))
        .unwrap();
        let config = ProviderConfig {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "provider-config-secret".to_string(),
            anthropic_version: "config-version".to_string(),
            validate_model: true,
        };

        let headers = build_headers(&config, input.client_protocol_envelope.as_ref()).unwrap();

        assert_eq!(headers["x-api-key"], "provider-config-secret");
        assert_eq!(headers["anthropic-version"], "2023-06-01");
        assert_eq!(headers["anthropic-beta"], "prompt-caching");
        assert_eq!(headers["x-claude-code-session-id"], "session-123");
        assert_eq!(headers["user-agent"], "ClaudeCode/1.0");
    }

    #[test]
    fn messages_body_forwards_content_blocks_and_appends_native_tool_calls() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {
                    "role": "user",
                    "content": "fallback text",
                    "content_blocks": [
                        { "type": "text", "text": "Describe these" },
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/png",
                                "data": "aW1hZ2U="
                            }
                        },
                        {
                            "type": "document",
                            "source": {
                                "type": "url",
                                "url": "https://example.com/file.pdf"
                            }
                        },
                        {
                            "type": "tool_result",
                            "tool_use_id": "toolu_previous",
                            "content": "previous result"
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": "fallback assistant",
                    "content_blocks": [
                        { "type": "text", "text": "I will use a tool" },
                        {
                            "type": "tool_use",
                            "id": "toolu_prior",
                            "name": "lookup",
                            "input": { "query": "prior" }
                        }
                    ],
                    "tool_calls": [
                        {
                            "id": "toolu_native",
                            "name": "lookup",
                            "arguments": "{\"query\":\"native\"}"
                        }
                    ]
                }
            ]
        }))
        .unwrap();

        let body = build_messages_body(&input).unwrap();

        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["text"], "Describe these");
        assert_eq!(body["messages"][0]["content"][1]["type"], "image");
        assert_eq!(
            body["messages"][0]["content"][1]["source"]["media_type"],
            "image/png"
        );
        assert_eq!(body["messages"][0]["content"][2]["type"], "document");
        assert_eq!(
            body["messages"][0]["content"][2]["source"]["url"],
            "https://example.com/file.pdf"
        );
        assert_eq!(body["messages"][0]["content"][3]["type"], "tool_result");
        assert_eq!(
            body["messages"][0]["content"][3]["tool_use_id"],
            "toolu_previous"
        );
        assert_eq!(body["messages"][1]["role"], "assistant");
        assert_eq!(
            body["messages"][1]["content"][0]["text"],
            "I will use a tool"
        );
        assert_eq!(body["messages"][1]["content"][1]["type"], "tool_use");
        assert_eq!(body["messages"][1]["content"][1]["id"], "toolu_prior");
        assert_eq!(body["messages"][1]["content"][2]["type"], "tool_use");
        assert_eq!(body["messages"][1]["content"][2]["id"], "toolu_native");
        assert_eq!(
            body["messages"][1]["content"][2]["input"]["query"],
            "native"
        );
    }

    #[test]
    fn messages_body_without_content_blocks_keeps_legacy_text_normalization() {
        let input = ProviderInvocationInput {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![ProviderMessage {
                role: "user".to_string(),
                content: json!([
                    { "type": "text", "text": "hello " },
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": "aW1hZ2U="
                        }
                    },
                    { "content": "world" }
                ]),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                content_blocks: None,
            }],
            ..Default::default()
        };

        let body = build_messages_body(&input).unwrap();

        assert_eq!(
            body["messages"][0]["content"],
            json!([{ "type": "text", "text": "hello world" }])
        );
    }

    #[test]
    fn anthropic_stream_commits_tool_use() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut builders = BTreeMap::new();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut message_id = Value::Null;

        for line in [
            r#"data: {"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":2,"output_tokens":1}}}"#,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"refund\"}"}}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}"#,
        ] {
            process_anthropic_sse_line(
                line,
                &mut events,
                &mut text,
                &mut builders,
                &mut usage,
                &mut finish_reason,
                &mut message_id,
            )
            .unwrap();
        }

        let call = builders.remove(&0).unwrap().into_tool_call();
        assert_eq!(message_id, json!("msg_1"));
        assert_eq!(call.id, "toolu_1");
        assert_eq!(call.name, "lookup");
        assert_eq!(call.arguments["query"], "refund");
        assert_eq!(usage.input_tokens, Some(2));
        assert_eq!(usage.output_tokens, Some(3));
        assert_eq!(usage.total_tokens, Some(5));
        assert_eq!(finish_reason, ProviderFinishReason::ToolCall);
    }

    #[tokio::test]
    async fn anthropic_streaming_result_keeps_message_id_metadata_only() {
        let response = reqwest::get(start_sse_server(concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1}}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        )))
        .await
        .unwrap();
        let mut events = Vec::new();

        let envelope = read_streaming_message(
            response,
            "claude-sonnet-4-20250514".to_string(),
            &mut |event| {
                events.push(event.clone());
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(envelope.result.final_content.as_deref(), Some("hello"));
        assert_eq!(envelope.result.response_id, None);
        assert_eq!(
            envelope.result.provider_metadata["message_id"],
            json!("msg_1")
        );
        assert!(events.contains(&ProviderStreamEvent::Finish {
            reason: ProviderFinishReason::Stop
        }));
    }
}
