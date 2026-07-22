use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    Method, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const PROVIDER_CODE: &str = "deepseek";
const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_VALIDATE_MODEL: bool = true;
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
const PASSTHROUGH_CHAT_COMPLETION_PARAMETERS: &[&str] = &[
    "temperature",
    "top_p",
    "stop",
    "logprobs",
    "top_logprobs",
    "reasoning_effort",
    "tool_choice",
];
const JSON_CHAT_COMPLETION_PARAMETERS: &[&str] = &["stop", "tool_choice", "tools"];

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
    proxy_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderUsage {
    pub input_tokens: Option<u64>,
    pub input_cache_hit_tokens: Option<u64>,
    pub input_cache_miss_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

impl ProviderUsage {
    fn has_any_value(&self) -> bool {
        self.input_tokens.is_some()
            || self.input_cache_hit_tokens.is_some()
            || self.input_cache_miss_tokens.is_some()
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderMessage {
    pub role: ProviderMessageRole,
    pub content: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub is_error: Option<bool>,
    #[serde(default)]
    pub tool_calls: Option<Value>,
    #[serde(default)]
    pub content_blocks: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderMessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub enum ProviderInvocationContractVersion {
    #[default]
    #[serde(rename = "1flowbase.provider/v2")]
    Current,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativePromptBlock {
    Text {
        text: String,
        #[serde(default)]
        cache_control: Option<NativePromptCacheControl>,
    },
}

impl NativePromptBlock {
    fn text_content(&self) -> &str {
        match self {
            Self::Text { text, .. } => text,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePromptCacheControl {
    #[serde(rename = "type")]
    pub cache_type: NativePromptCacheControlType,
    #[serde(default)]
    pub ttl: Option<NativePromptCacheTtl>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePromptCacheControlType {
    Ephemeral,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum NativePromptCacheTtl {
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct NativeModelRequestContext {
    #[serde(default)]
    pub end_user_reference: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderInvocationCapability {
    SystemPromptBlocks,
    SystemPromptCacheControl,
    EndUserReference,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProviderInvocationInput {
    pub contract_version: ProviderInvocationContractVersion,
    pub provider_instance_id: String,
    pub provider_code: String,
    pub protocol: String,
    pub model: String,
    #[serde(default)]
    pub previous_response_id: Option<String>,
    #[serde(default)]
    pub provider_config: Value,
    #[serde(default)]
    pub messages: Vec<ProviderMessage>,
    #[serde(default)]
    pub system: Vec<NativePromptBlock>,
    #[serde(default)]
    pub request_context: NativeModelRequestContext,
    #[serde(default)]
    pub required_capabilities: BTreeSet<ProviderInvocationCapability>,
    #[serde(default)]
    pub tools: Vec<Value>,
    #[serde(default)]
    pub mcp_bindings: Vec<Value>,
    #[serde(default)]
    pub response_format: Option<Value>,
    #[serde(default)]
    pub model_parameters: BTreeMap<String, Value>,
    #[serde(default)]
    pub client_protocol_envelope: Option<ClientProtocolEnvelope>,
    #[serde(default)]
    pub trace_context: BTreeMap<String, String>,
    #[serde(default)]
    pub run_context: BTreeMap<String, Value>,
}

impl ProviderInvocationInput {
    fn system_text(&self) -> Option<String> {
        let text = self
            .system
            .iter()
            .map(NativePromptBlock::text_content)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        (!text.is_empty()).then_some(text)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClientProtocolEnvelope {
    pub source_protocol: String,
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
        "balance" => get_balance(&request.input).await,
        "invoke" => {
            let input: ProviderInvocationInput = serde_json::from_value(request.input)?;
            let output = invoke_chat_completion(input).await?;
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
    let output = invoke_chat_completion_with_event_sink(input, on_event).await?;
    Ok(output.result)
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
            "validate_model": config.validate_model,
            "proxy_url": config.proxy_url.as_ref().map(|_| "***")
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
        proxy_url: normalize_proxy_url(config.get("proxy_url"))?,
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

fn normalize_proxy_url(value: Option<&Value>) -> Result<Option<String>> {
    let Some(proxy_url) = optional_text(value) else {
        return Ok(None);
    };
    let parsed = Url::parse(&proxy_url).with_context(|| "invalid proxy_url")?;
    if parsed.scheme() != "http" {
        bail!("proxy_url must use http scheme");
    }
    Ok(Some(parsed.to_string()))
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

fn build_headers(
    config: &ProviderConfig,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", config.api_key))
            .context("invalid api_key for authorization header")?,
    );
    apply_default_client_protocol_policy(&mut headers, client_protocol_envelope)?;
    Ok(headers)
}

fn apply_default_client_protocol_policy(
    headers: &mut HeaderMap,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<()> {
    let Some(envelope) = client_protocol_envelope else {
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
    let base_url = config.base_url.trim_end_matches('/');
    let url = Url::parse(&format!("{base_url}{pathname}"))
        .with_context(|| format!("invalid base_url: {}", config.base_url))?;
    Ok(url.to_string())
}

fn build_http_client(config: &ProviderConfig) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy_url) = &config.proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy_url")?);
    }
    builder.build().context("building DeepSeek HTTP client")
}

async fn request_json(config: &ProviderConfig, pathname: &str, method: Method) -> Result<Value> {
    let client = build_http_client(config)?;
    let response = client
        .request(method, build_url(config, pathname)?)
        .headers(build_headers(config, None)?)
        .send()
        .await
        .map_err(|error| sanitize_error(error, config))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| sanitize_error(error, config))?;
    let payload = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&text).with_context(|| "provider returned invalid JSON")?
    };

    if !status.is_success() {
        let message = sanitize_text(provider_error_message(&payload), config);
        bail!("{} {}: {}", status.as_u16(), status, message);
    }

    Ok(payload)
}

async fn invoke_chat_completion(
    input: ProviderInvocationInput,
) -> Result<RuntimeInvocationEnvelope> {
    invoke_chat_completion_with_event_sink(input, |_| Ok(())).await
}

async fn invoke_chat_completion_with_event_sink<F>(
    input: ProviderInvocationInput,
    mut on_event: F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let config = normalize_provider_config(&input.provider_config)?;
    let body = build_chat_completion_body(&input)?;
    let client = build_http_client(&config)?;
    let response = client
        .request(
            Method::POST,
            build_url(&config, "/chat/completions").context("invalid chat completions endpoint")?,
        )
        .headers(build_headers(
            &config,
            input.client_protocol_envelope.as_ref(),
        )?)
        .json(&body)
        .send()
        .await
        .map_err(|error| sanitize_error(error, &config))?;

    read_streaming_chat_completion(response, input.model, &config.api_key, &mut on_event).await
}

fn build_chat_completion_body(input: &ProviderInvocationInput) -> Result<Value> {
    if !input.required_capabilities.is_empty()
        || input.system.iter().any(|block| {
            matches!(
                block,
                NativePromptBlock::Text {
                    cache_control: Some(_),
                    ..
                }
            )
        })
        || input.request_context.end_user_reference.is_some()
    {
        bail!("DeepSeek Generate does not support the requested semantic capabilities");
    }
    let model = input.model.trim();
    if model.is_empty() {
        bail!("model is required");
    }

    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model.to_string()));
    body.insert(
        "messages".to_string(),
        Value::Array(build_invocation_messages(input)),
    );
    body.insert("stream".to_string(), Value::Bool(true));
    body.insert(
        "stream_options".to_string(),
        json!({ "include_usage": true }),
    );

    if let Some(thinking_type) = parameter_value(input, "thinking_type") {
        body.insert("thinking".to_string(), json!({ "type": thinking_type }));
    }
    if let Some(response_format) = input
        .response_format
        .clone()
        .and_then(normalize_response_format_value)
        .or_else(|| parameter_value(input, "response_format"))
    {
        body.insert("response_format".to_string(), response_format);
    }
    if !input.tools.is_empty() {
        body.insert("tools".to_string(), Value::Array(input.tools.clone()));
    } else if let Some(tools) = parameter_value(input, "tools") {
        body.insert("tools".to_string(), tools);
    }
    if let Some(user_id) = parameter_value(input, "user_id") {
        body.insert("user_id".to_string(), user_id);
    }
    if let Some(max_output_tokens) = parameter_value(input, "max_output_tokens") {
        body.insert("max_tokens".to_string(), max_output_tokens);
    }
    for key in PASSTHROUGH_CHAT_COMPLETION_PARAMETERS {
        if let Some(value) = parameter_value(input, key) {
            body.insert((*key).to_string(), value);
        }
    }

    Ok(Value::Object(body))
}

fn build_invocation_messages(input: &ProviderInvocationInput) -> Vec<Value> {
    let mut messages = Vec::new();
    if let Some(system) = input.system_text() {
        messages.push(json!({
            "role": "system",
            "content": system,
        }));
    }
    for message in &input.messages {
        let mut item = Map::new();
        let role = match message.role {
            ProviderMessageRole::System => "system",
            ProviderMessageRole::User => "user",
            ProviderMessageRole::Assistant => "assistant",
            ProviderMessageRole::Tool => "tool",
        };
        item.insert("role".to_string(), Value::String(role.to_string()));
        item.insert(
            "content".to_string(),
            message
                .content_blocks
                .clone()
                .unwrap_or_else(|| Value::String(message.content.clone())),
        );
        if let Some(name) = message
            .name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            item.insert("name".to_string(), Value::String(name.to_string()));
        }
        if let Some(tool_call_id) = message
            .tool_call_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            item.insert(
                "tool_call_id".to_string(),
                Value::String(tool_call_id.to_string()),
            );
        }
        if let Some(tool_calls) = message.tool_calls.as_ref().filter(|value| !value.is_null()) {
            item.insert(
                "tool_calls".to_string(),
                build_chat_completion_tool_calls(tool_calls),
            );
        }
        messages.push(Value::Object(item));
    }
    messages
}

fn build_chat_completion_tool_calls(tool_calls: &Value) -> Value {
    let Some(calls) = tool_calls.as_array() else {
        return tool_calls.clone();
    };
    Value::Array(calls.iter().map(build_chat_completion_tool_call).collect())
}

fn build_chat_completion_tool_call(tool_call: &Value) -> Value {
    let Some(object) = tool_call.as_object() else {
        return tool_call.clone();
    };
    if object.contains_key("function") {
        let mut mapped = object.clone();
        mapped
            .entry("type".to_string())
            .or_insert_with(|| Value::String("function".to_string()));
        return Value::Object(mapped);
    }
    let Some(name) = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    else {
        return tool_call.clone();
    };
    let arguments = object
        .get("arguments")
        .map(chat_completion_tool_arguments)
        .unwrap_or_else(|| "{}".to_string());

    let mut function = Map::new();
    function.insert("name".to_string(), Value::String(name.to_string()));
    function.insert("arguments".to_string(), Value::String(arguments));

    let mut mapped = Map::new();
    if let Some(id) = object.get("id") {
        mapped.insert("id".to_string(), id.clone());
    }
    mapped.insert("type".to_string(), Value::String("function".to_string()));
    mapped.insert("function".to_string(), Value::Object(function));
    Value::Object(mapped)
}

fn chat_completion_tool_arguments(arguments: &Value) -> String {
    match arguments {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn parameter_value(input: &ProviderInvocationInput, key: &str) -> Option<Value> {
    input
        .model_parameters
        .get(key)
        .cloned()
        .and_then(|value| normalize_parameter_value(key, value))
}

fn normalize_parameter_value(key: &str, value: Value) -> Option<Value> {
    match key {
        "response_format" => normalize_response_format_value(value),
        _ if JSON_CHAT_COMPLETION_PARAMETERS.contains(&key) => normalize_json_parameter(value),
        _ => normalize_scalar_parameter(value),
    }
}

fn normalize_response_format_value(value: Value) -> Option<Value> {
    match normalize_scalar_parameter(value)? {
        Value::String(text) => serde_json::from_str::<Value>(&text)
            .ok()
            .map(response_format_type)
            .or_else(|| Some(response_format_type(Value::String(text)))),
        other => Some(response_format_type(other)),
    }
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

fn normalize_json_parameter(value: Value) -> Option<Value> {
    match normalize_scalar_parameter(value)? {
        Value::String(text) => serde_json::from_str::<Value>(&text)
            .ok()
            .or(Some(Value::String(text))),
        other => Some(other),
    }
}

fn response_format_type(value: Value) -> Value {
    match value {
        Value::Object(object) if object.contains_key("type") => Value::Object(object),
        other => json!({ "type": other }),
    }
}

async fn read_streaming_chat_completion<F>(
    response: reqwest::Response,
    request_model: String,
    api_key: &str,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let status = response.status();
    if !status.is_success() {
        let text = response
            .text()
            .await
            .with_context(|| "provider error response was not readable")?;
        let payload = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
        let message = provider_error_message(&payload).replace(api_key, "***");
        bail!("{} {}: {}", status.as_u16(), status, message);
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut events = Vec::new();
    let mut text = String::new();
    let mut tool_call_builders: Vec<ToolCallBuilder> = Vec::new();
    let mut usage = ProviderUsage::default();
    let mut finish_reason: Option<ProviderFinishReason> = None;
    let mut response_model = Value::Null;
    let mut response_id = Value::Null;
    let mut created = Value::Null;
    let mut system_fingerprint = Value::Null;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(line_end) = buffer.find('\n') {
            let mut line = buffer[..line_end].to_string();
            if line.ends_with('\r') {
                line.pop();
            }
            buffer.drain(..=line_end);
            let event_start = events.len();
            process_sse_line(
                &line,
                &mut events,
                &mut text,
                &mut tool_call_builders,
                &mut usage,
                &mut finish_reason,
                &mut response_model,
                &mut response_id,
                &mut created,
                &mut system_fingerprint,
            )?;
            emit_new_events(&events, event_start, on_event)?;
        }
    }

    if !buffer.trim().is_empty() {
        let line = std::mem::take(&mut buffer);
        let event_start = events.len();
        process_sse_line(
            &line,
            &mut events,
            &mut text,
            &mut tool_call_builders,
            &mut usage,
            &mut finish_reason,
            &mut response_model,
            &mut response_id,
            &mut created,
            &mut system_fingerprint,
        )?;
        emit_new_events(&events, event_start, on_event)?;
    }

    let final_event_start = events.len();
    let tool_calls = tool_call_builders
        .into_iter()
        .filter_map(ToolCallBuilder::into_tool_call)
        .collect::<Vec<_>>();
    for call in &tool_calls {
        events.push(ProviderStreamEvent::ToolCallCommit { call: call.clone() });
    }
    if usage.has_any_value() {
        events.push(ProviderStreamEvent::UsageSnapshot {
            usage: usage.clone(),
        });
    }
    let finish_reason = finish_reason.unwrap_or_else(|| normalize_finish_reason(None, &tool_calls));
    events.push(ProviderStreamEvent::Finish {
        reason: finish_reason.clone(),
    });
    emit_new_events(&events, final_event_start, on_event)?;
    Ok(RuntimeInvocationEnvelope {
        events,
        result: ProviderInvocationResult {
            final_content: (!text.is_empty()).then_some(text),
            response_id: None,
            tool_calls,
            mcp_calls: Vec::new(),
            usage,
            finish_reason: Some(finish_reason),
            provider_metadata: json!({
                "request_model": request_model,
                "response_model": response_model,
                "response_id": response_id,
                "created": created,
                "system_fingerprint": system_fingerprint,
            }),
        },
    })
}

fn emit_new_events<F>(events: &[ProviderStreamEvent], start: usize, on_event: &mut F) -> Result<()>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    for event in &events[start..] {
        on_event(event)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_sse_line(
    line: &str,
    events: &mut Vec<ProviderStreamEvent>,
    text: &mut String,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    usage: &mut ProviderUsage,
    finish_reason: &mut Option<ProviderFinishReason>,
    response_model: &mut Value,
    response_id: &mut Value,
    created: &mut Value,
    system_fingerprint: &mut Value,
) -> Result<()> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') || !line.starts_with("data:") {
        return Ok(());
    }
    let data = line.trim_start_matches("data:").trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let payload: Value =
        serde_json::from_str(data).with_context(|| "provider returned invalid SSE JSON")?;
    process_stream_payload(
        &payload,
        events,
        text,
        tool_call_builders,
        usage,
        finish_reason,
        response_model,
        response_id,
        created,
        system_fingerprint,
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_stream_payload(
    payload: &Value,
    events: &mut Vec<ProviderStreamEvent>,
    text: &mut String,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    usage: &mut ProviderUsage,
    finish_reason: &mut Option<ProviderFinishReason>,
    response_model: &mut Value,
    response_id: &mut Value,
    created: &mut Value,
    system_fingerprint: &mut Value,
) {
    if !payload.get("model").unwrap_or(&Value::Null).is_null() {
        *response_model = payload.get("model").cloned().unwrap_or(Value::Null);
    }
    if !payload.get("id").unwrap_or(&Value::Null).is_null() {
        *response_id = payload.get("id").cloned().unwrap_or(Value::Null);
    }
    if !payload.get("created").unwrap_or(&Value::Null).is_null() {
        *created = payload.get("created").cloned().unwrap_or(Value::Null);
    }
    if !payload
        .get("system_fingerprint")
        .unwrap_or(&Value::Null)
        .is_null()
    {
        *system_fingerprint = payload
            .get("system_fingerprint")
            .cloned()
            .unwrap_or(Value::Null);
    }
    if let Some(snapshot) = payload.get("usage").filter(|value| !value.is_null()) {
        *usage = normalize_usage(snapshot);
    }

    let Some(choice) = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    else {
        return;
    };
    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
        *finish_reason = Some(normalize_finish_reason(Some(reason), &[]));
    }
    let Some(delta) = choice.get("delta") else {
        return;
    };
    if let Some(reasoning) = extract_reasoning_delta(delta).filter(|value| !value.is_empty()) {
        events.push(ProviderStreamEvent::ReasoningDelta { delta: reasoning });
    }
    if let Some(content) = extract_content(delta.get("content")).filter(|value| !value.is_empty()) {
        text.push_str(&content);
        events.push(ProviderStreamEvent::TextDelta { delta: content });
    }
    merge_tool_call_deltas(delta.get("tool_calls"), tool_call_builders, events);
}

fn extract_reasoning_delta(delta: &Value) -> Option<String> {
    delta
        .get("reasoning_content")
        .or_else(|| delta.get("reasoning"))
        .or_else(|| delta.get("reasoning_delta"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
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

#[derive(Debug, Clone, Default)]
struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
    arguments_seen: bool,
}

impl ToolCallBuilder {
    fn into_tool_call(self) -> Option<ProviderToolCall> {
        let id = self.id?;
        let name = self.name?;
        if !self.arguments_seen {
            return None;
        }
        Some(ProviderToolCall {
            id,
            name,
            arguments: serde_json::from_str(&self.arguments)
                .unwrap_or_else(|_| json!({ "raw": self.arguments })),
        })
    }
}

fn merge_tool_call_deltas(
    tool_calls: Option<&Value>,
    builders: &mut Vec<ToolCallBuilder>,
    events: &mut Vec<ProviderStreamEvent>,
) {
    let Some(tool_calls) = tool_calls.and_then(Value::as_array) else {
        return;
    };
    for tool_call in tool_calls {
        let index = tool_call
            .get("index")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(builders.len());
        while builders.len() <= index {
            builders.push(ToolCallBuilder::default());
        }
        let builder = &mut builders[index];
        if let Some(id) = tool_call
            .get("id")
            .map(value_to_string)
            .filter(|value| !value.is_empty())
        {
            builder.id = Some(id);
        }
        if let Some(function) = tool_call.get("function") {
            if let Some(name) = function
                .get("name")
                .map(value_to_string)
                .filter(|value| !value.is_empty())
            {
                builder.name = Some(name);
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                builder.arguments_seen = true;
                builder.arguments.push_str(arguments);
            }
        }
        events.push(ProviderStreamEvent::ToolCallDelta {
            call_id: builder
                .id
                .clone()
                .unwrap_or_else(|| format!("tool_call_{}", index + 1)),
            delta: tool_call.clone(),
        });
    }
}

fn normalize_usage(usage: &Value) -> ProviderUsage {
    let input_cache_hit_tokens = number_or_none(usage.get("prompt_cache_hit_tokens"));
    ProviderUsage {
        input_tokens: number_or_none(usage.get("prompt_tokens")),
        input_cache_hit_tokens,
        input_cache_miss_tokens: number_or_none(usage.get("prompt_cache_miss_tokens")),
        output_tokens: number_or_none(usage.get("completion_tokens")),
        reasoning_tokens: usage
            .get("completion_tokens_details")
            .and_then(|value| value.get("reasoning_tokens"))
            .and_then(number_or_none_ref)
            .or_else(|| number_or_none(usage.get("reasoning_tokens"))),
        cache_read_tokens: input_cache_hit_tokens,
        cache_write_tokens: None,
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

fn provider_error_message(payload: &Value) -> String {
    payload
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "provider upstream request failed".to_string())
}

fn sanitize_error(error: reqwest::Error, config: &ProviderConfig) -> anyhow::Error {
    anyhow!(sanitize_text(error.to_string(), config))
}

fn sanitize_text(message: String, config: &ProviderConfig) -> String {
    let mut sanitized = message.replace(&config.api_key, "***");
    if let Some(proxy_url) = &config.proxy_url {
        sanitized = sanitized.replace(proxy_url, "***");
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    #[tokio::test]
    async fn ac_005_validate_redacts_configured_proxy_url() {
        let proxy_url = "http://proxy-user:proxy-pass@127.0.0.1:8080";
        let response = handle_request(ProviderStdioRequest {
            method: "validate".to_string(),
            input: json!({
                "api_key": "provider-secret",
                "validate_model": false,
                "proxy_url": proxy_url
            }),
        })
        .await
        .unwrap();

        assert!(response.ok);
        assert_eq!(response.result["sanitized"]["proxy_url"], "***");
        assert!(!response.result.to_string().contains(proxy_url));
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn capture_streaming_chat_request() -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = format!("http://{}", listener.local_addr().expect("listener addr"));

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should connect");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout");

            let mut buffer = Vec::new();
            let mut chunk = [0_u8; 4096];
            let mut header_end = None;
            let mut body_length = None;

            loop {
                let read = stream.read(&mut chunk).expect("request should be readable");
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);

                if header_end.is_none() {
                    header_end = find_bytes(&buffer, b"\r\n\r\n").map(|offset| offset + 4);
                    if let Some(end) = header_end {
                        let headers = String::from_utf8_lossy(&buffer[..end]);
                        body_length = headers.lines().find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            if name.eq_ignore_ascii_case("content-length") {
                                return value.trim().parse::<usize>().ok();
                            }
                            None
                        });
                    }
                }

                if let (Some(end), Some(length)) = (header_end, body_length) {
                    if buffer.len() >= end + length {
                        let response_body = concat!(
                            ": keepalive\n\n",
                            "\n",
                            "data: {\"id\":\"chatcmpl_test\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"reasoning_content\":\"think\"},\"finish_reason\":null}]}\n\n",
                            "data: {\"id\":\"chatcmpl_test\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"content\":\"answer\"},\"finish_reason\":null}]}\n\n",
                            "data: {\"id\":\"chatcmpl_test\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"query\\\":\\\"refund\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
                            "data: {\"id\":\"chatcmpl_test\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":100,\"prompt_cache_hit_tokens\":40,\"prompt_cache_miss_tokens\":60,\"completion_tokens\":12,\"total_tokens\":112,\"completion_tokens_details\":{\"reasoning_tokens\":5}}}\n\n",
                            "data: [DONE]\n\n"
                        );
                        let response = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                            response_body.len(),
                            response_body
                        );
                        stream
                            .write_all(response.as_bytes())
                            .expect("response should be writable");
                        return String::from_utf8(buffer[end..end + length].to_vec())
                            .expect("request body should be utf8");
                    }
                }
            }

            panic!("request body was not fully captured");
        });

        (address, handle)
    }

    fn start_error_chat_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = format!("http://{}", listener.local_addr().expect("listener addr"));

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should connect");
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer);
            let body = json!({
                "error": {
                    "message": "bad test-key"
                }
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("response should be writable");
        });

        address
    }

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

        let headers = build_headers(&config, None).unwrap();
        assert_eq!(headers.get(ACCEPT).unwrap(), "application/json");
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer secret");
    }

    #[test]
    fn client_protocol_envelope_uses_default_deny_policy_for_headers() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "deepseek",
            "protocol": "deepseek",
            "model": "deepseek-chat",
            "client_protocol_envelope": {
                "source_protocol": "openai_chat",
                "policy": "default_deny",
                "headers": {
                    "authorization": "Bearer client-secret",
                    "x-api-key": "client-api-key",
                    "x-client-name": "ClaudeCode",
                    "transfer-encoding": "chunked"
                }
            }
        }))
        .unwrap();

        assert!(input.client_protocol_envelope.is_some());

        let config = normalize_provider_config(&json!({
            "api_key": "provider-secret",
        }))
        .unwrap();
        let headers = build_headers(&config, input.client_protocol_envelope.as_ref()).unwrap();

        assert_eq!(
            headers.get(AUTHORIZATION).unwrap(),
            "Bearer provider-secret"
        );
        assert!(headers.get("x-api-key").is_none());
        assert!(headers.get("x-client-name").is_none());
        assert!(headers.get("transfer-encoding").is_none());
    }

    #[test]
    fn headers_restore_anthropic_client_protocol_envelope_and_keep_config_auth() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "deepseek",
            "protocol": "deepseek",
            "model": "deepseek-chat",
            "client_protocol_envelope": {
                "source_protocol": "anthropic_messages",
                "policy": "anthropic_messages_v1",
                "headers": {
                    "anthropic-version": "2023-06-01",
                    "anthropic-beta": "ccr-byoc-2025-07-29",
                    "x-claude-code-session-id": "session-123",
                    "x-client-name": "ClaudeCode",
                    "user-agent": "ClaudeCode/1.0",
                    "authorization": "Bearer client-secret",
                    "x-api-key": "client-auth-must-not-win"
                }
            }
        }))
        .unwrap();
        let config = normalize_provider_config(&json!({
            "api_key": "provider-secret",
        }))
        .unwrap();
        let headers = build_headers(&config, input.client_protocol_envelope.as_ref()).unwrap();

        assert_eq!(
            headers.get(AUTHORIZATION).unwrap(),
            "Bearer provider-secret"
        );
        assert!(headers.get("x-api-key").is_none());
        assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
        assert_eq!(
            headers.get("anthropic-beta").unwrap(),
            "ccr-byoc-2025-07-29"
        );
        assert_eq!(
            headers.get("x-claude-code-session-id").unwrap(),
            "session-123"
        );
        assert_eq!(headers.get("x-client-name").unwrap(), "ClaudeCode");
        assert_eq!(headers.get("user-agent").unwrap(), "ClaudeCode/1.0");
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

    #[test]
    fn ac_002_native_max_output_tokens_maps_to_deepseek_wire_field() {
        let input = ProviderInvocationInput {
            model: "deepseek-v4-pro".to_string(),
            provider_config: serde_json::json!({ "api_key": "secret" }),
            messages: vec![
                ProviderMessage {
                    role: ProviderMessageRole::Assistant,
                    content: String::new(),
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: Some(serde_json::json!([{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "lookup",
                            "arguments": "{\"query\":\"refund\"}"
                        }
                    }])),
                    content_blocks: None,
                },
                ProviderMessage {
                    role: ProviderMessageRole::User,
                    content: "Hi".to_string(),
                    name: Some("customer".to_string()),
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: None,
                    content_blocks: None,
                },
                ProviderMessage {
                    role: ProviderMessageRole::Tool,
                    content: "tool result".to_string(),
                    name: None,
                    tool_call_id: Some("call_1".to_string()),
                    is_error: None,
                    tool_calls: None,
                    content_blocks: None,
                },
            ],
            tools: vec![serde_json::json!({
                "type": "function",
                "function": { "name": "lookup", "parameters": { "type": "object" } }
            })],
            model_parameters: BTreeMap::from([
                ("thinking_type".to_string(), serde_json::json!("enabled")),
                ("reasoning_effort".to_string(), serde_json::json!("max")),
                (
                    "response_format".to_string(),
                    serde_json::json!("json_object"),
                ),
                ("tool_choice".to_string(), serde_json::json!("auto")),
                ("user_id".to_string(), serde_json::json!("user-1")),
                ("temperature".to_string(), serde_json::json!(0.7)),
                ("top_p".to_string(), serde_json::json!(0.9)),
                ("max_output_tokens".to_string(), serde_json::json!(512)),
                ("stop".to_string(), serde_json::json!(["END"])),
                ("logprobs".to_string(), serde_json::json!(true)),
                ("top_logprobs".to_string(), serde_json::json!(5)),
                ("frequency_penalty".to_string(), serde_json::json!(0.4)),
                ("presence_penalty".to_string(), serde_json::json!(0.2)),
                (
                    "tools".to_string(),
                    serde_json::json!([{
                        "type": "function",
                        "function": { "name": "raw_tool" }
                    }]),
                ),
            ]),
            ..ProviderInvocationInput::default()
        };

        let body = build_chat_completion_body(&input).unwrap();

        assert_eq!(body["model"], "deepseek-v4-pro");
        assert_eq!(body["messages"][0]["role"], "assistant");
        assert_eq!(body["messages"][0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            body["messages"][0]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "Hi");
        assert_eq!(body["messages"][1]["name"], "customer");
        assert_eq!(body["messages"][2]["role"], "tool");
        assert_eq!(body["messages"][2]["tool_call_id"], "call_1");
        assert_eq!(body["thinking"], serde_json::json!({ "type": "enabled" }));
        assert_eq!(body["reasoning_effort"], "max");
        assert_eq!(
            body["response_format"],
            serde_json::json!({ "type": "json_object" })
        );
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["user_id"], "user-1");
        assert_eq!(body["temperature"], serde_json::json!(0.7));
        assert_eq!(body["top_p"], serde_json::json!(0.9));
        assert_eq!(body["max_tokens"], serde_json::json!(512));
        assert_eq!(body["stop"], serde_json::json!(["END"]));
        assert_eq!(body["logprobs"], serde_json::json!(true));
        assert_eq!(body["top_logprobs"], serde_json::json!(5));
        assert_eq!(
            body["tools"][0]["function"]["name"],
            serde_json::json!("lookup")
        );
        assert_eq!(body.get("frequency_penalty"), None);
        assert_eq!(body.get("presence_penalty"), None);
        assert_eq!(body["stream"], true);
        assert_eq!(
            body["stream_options"],
            serde_json::json!({ "include_usage": true })
        );
    }

    #[test]
    fn build_chat_completion_body_maps_deepseek_json_mode_from_native_response_format() {
        let input = ProviderInvocationInput {
            model: "deepseek-v4-pro".to_string(),
            messages: vec![ProviderMessage {
                role: ProviderMessageRole::User,
                content: "Return JSON only".to_string(),
                name: None,
                tool_call_id: None,
                is_error: None,
                tool_calls: None,
                content_blocks: None,
            }],
            response_format: Some(serde_json::json!({ "type": "json_object" })),
            ..ProviderInvocationInput::default()
        };

        let body = build_chat_completion_body(&input).unwrap();

        assert_eq!(
            body["response_format"],
            serde_json::json!({ "type": "json_object" })
        );
    }

    #[test]
    fn build_chat_completion_body_maps_deepseek_json_mode_from_string_parameter() {
        let input = ProviderInvocationInput {
            model: "deepseek-v4-pro".to_string(),
            messages: vec![ProviderMessage {
                role: ProviderMessageRole::User,
                content: "Return JSON only".to_string(),
                name: None,
                tool_call_id: None,
                is_error: None,
                tool_calls: None,
                content_blocks: None,
            }],
            model_parameters: BTreeMap::from([(
                "response_format".to_string(),
                serde_json::json!(r#"{"type":"json_object"}"#),
            )]),
            ..ProviderInvocationInput::default()
        };

        let body = build_chat_completion_body(&input).unwrap();

        assert_eq!(
            body["response_format"],
            serde_json::json!({ "type": "json_object" })
        );
    }

    #[test]
    fn build_invocation_messages_maps_native_tool_calls_to_deepseek_wire_shape() {
        let input = ProviderInvocationInput {
            messages: vec![ProviderMessage {
                role: ProviderMessageRole::Assistant,
                content: String::new(),
                name: None,
                tool_call_id: None,
                is_error: None,
                tool_calls: Some(serde_json::json!([
                    {
                        "id": "call_00_XRooTtPLMotGXDkskaIA9845",
                        "name": "Read",
                        "arguments": {
                            "file_path": "E:\\code\\taichuCode\\1flowbase\\.memory\\user-memory.md"
                        }
                    }
                ])),
                content_blocks: None,
            }],
            ..ProviderInvocationInput::default()
        };

        let messages = build_invocation_messages(&input);
        let call = &messages[0]["tool_calls"][0];

        assert_eq!(call["id"], "call_00_XRooTtPLMotGXDkskaIA9845");
        assert_eq!(call["type"], "function");
        assert_eq!(call["function"]["name"], "Read");
        assert_eq!(
            call["function"]["arguments"],
            r#"{"file_path":"E:\\code\\taichuCode\\1flowbase\\.memory\\user-memory.md"}"#
        );
        assert!(call.get("name").is_none());
        assert!(call.get("arguments").is_none());
    }

    #[test]
    fn build_chat_completion_body_passes_deepseek_tool_calls_request_shape() {
        let input = ProviderInvocationInput {
            model: "deepseek-v4-pro".to_string(),
            messages: vec![
                ProviderMessage {
                    role: ProviderMessageRole::Assistant,
                    content: String::new(),
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: Some(serde_json::json!([
                        {
                            "id": "call_1",
                            "name": "get_weather",
                            "arguments": { "location": "Hangzhou" }
                        }
                    ])),
                    content_blocks: None,
                },
                ProviderMessage {
                    role: ProviderMessageRole::Tool,
                    content: "Sunny".to_string(),
                    name: None,
                    tool_call_id: Some("call_1".to_string()),
                    is_error: None,
                    tool_calls: None,
                    content_blocks: None,
                },
            ],
            tools: vec![serde_json::json!({
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "strict": true,
                    "description": "Get weather",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": { "type": "string" }
                        },
                        "required": ["location"],
                        "additionalProperties": false
                    }
                }
            })],
            model_parameters: BTreeMap::from([(
                "tool_choice".to_string(),
                serde_json::json!("required"),
            )]),
            ..ProviderInvocationInput::default()
        };

        let body = build_chat_completion_body(&input).unwrap();
        let tool_call = &body["messages"][0]["tool_calls"][0];

        assert_eq!(body["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(body["tools"][0]["function"]["strict"], true);
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["additionalProperties"],
            false
        );
        assert_eq!(body["tool_choice"], "required");
        assert_eq!(tool_call["type"], "function");
        assert_eq!(tool_call["function"]["name"], "get_weather");
        assert_eq!(
            tool_call["function"]["arguments"],
            serde_json::json!(r#"{"location":"Hangzhou"}"#)
        );
        assert_eq!(body["messages"][1]["role"], "tool");
        assert_eq!(body["messages"][1]["tool_call_id"], "call_1");
    }

    #[test]
    fn normalize_usage_maps_deepseek_cache_segments() {
        let usage = normalize_usage(&serde_json::json!({
            "prompt_tokens": 100,
            "prompt_cache_hit_tokens": 40,
            "prompt_cache_miss_tokens": 60,
            "completion_tokens": 12,
            "total_tokens": 112,
            "completion_tokens_details": { "reasoning_tokens": 5 }
        }));

        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.input_cache_hit_tokens, Some(40));
        assert_eq!(usage.input_cache_miss_tokens, Some(60));
        assert_eq!(usage.cache_read_tokens, Some(40));
        assert_eq!(usage.cache_write_tokens, None);
        assert_eq!(usage.output_tokens, Some(12));
        assert_eq!(usage.reasoning_tokens, Some(5));
        assert_eq!(usage.total_tokens, Some(112));
    }

    #[tokio::test]
    async fn ac_002_fake_upstream_receives_exact_generate_wire_and_maps_sse() {
        let (base_url, capture_handle) = capture_streaming_chat_request();
        let mut events = Vec::new();

        let result = handle_invoke_request_streaming(
            json!({
                "contract_version": "1flowbase.provider/v2",
                "provider_instance_id": "provider-test",
                "provider_code": "deepseek",
                "protocol": "deepseek",
                "model": "deepseek-v4-pro",
                "provider_config": {
                    "base_url": base_url,
                    "api_key": "test-key"
                },
                "system": [{ "type": "text", "text": "Be concise" }],
                "messages": [{
                    "role": "user",
                    "content": "hello"
                }]
            }),
            |event| {
                events.push(event.clone());
                Ok(())
            },
        )
        .await
        .expect("streaming invoke should succeed");

        let captured_body: Value =
            serde_json::from_str(&capture_handle.join().expect("capture thread should finish"))
                .expect("captured body should parse");

        assert_eq!(
            captured_body,
            json!({
                "model": "deepseek-v4-pro",
                "messages": [
                    { "role": "system", "content": "Be concise" },
                    { "role": "user", "content": "hello" }
                ],
                "stream": true,
                "stream_options": { "include_usage": true }
            })
        );
        assert!(events.contains(&ProviderStreamEvent::ReasoningDelta {
            delta: "think".to_string()
        }));
        assert!(events.contains(&ProviderStreamEvent::TextDelta {
            delta: "answer".to_string()
        }));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                ProviderStreamEvent::ToolCallDelta { call_id, .. } if call_id == "call_1"
            )
        }));
        let expected_tool_call = ProviderToolCall {
            id: "call_1".to_string(),
            name: "lookup".to_string(),
            arguments: json!({ "query": "refund" }),
        };
        assert!(events.contains(&ProviderStreamEvent::ToolCallCommit {
            call: expected_tool_call.clone()
        }));
        assert!(events.contains(&ProviderStreamEvent::UsageSnapshot {
            usage: ProviderUsage {
                input_tokens: Some(100),
                input_cache_hit_tokens: Some(40),
                input_cache_miss_tokens: Some(60),
                output_tokens: Some(12),
                reasoning_tokens: Some(5),
                cache_read_tokens: Some(40),
                total_tokens: Some(112),
                ..ProviderUsage::default()
            }
        }));
        assert!(events.contains(&ProviderStreamEvent::Finish {
            reason: ProviderFinishReason::Stop
        }));
        assert_eq!(result.final_content.as_deref(), Some("answer"));
        assert_eq!(result.tool_calls, vec![expected_tool_call]);
        assert_eq!(result.finish_reason, Some(ProviderFinishReason::Stop));
        assert_eq!(result.response_id, None);
        assert_eq!(
            result.provider_metadata["response_id"],
            json!("chatcmpl_test")
        );
        assert_eq!(result.usage.input_cache_hit_tokens, Some(40));
        assert_eq!(result.usage.input_cache_miss_tokens, Some(60));
        assert_eq!(result.usage.cache_write_tokens, None);
    }

    #[tokio::test]
    async fn chat_streaming_error_sanitizes_api_key() {
        let base_url = start_error_chat_server();

        let error = handle_invoke_request_streaming(
            json!({
                "contract_version": "1flowbase.provider/v2",
                "provider_instance_id": "provider-test",
                "provider_code": "deepseek",
                "protocol": "deepseek",
                "model": "deepseek-v4-pro",
                "provider_config": {
                    "base_url": base_url,
                    "api_key": "test-key"
                },
                "messages": [{
                    "role": "user",
                    "content": "hello"
                }]
            }),
            |_| Ok(()),
        )
        .await
        .expect_err("provider error should fail");
        let message = error.to_string();

        assert!(!message.contains("test-key"));
        assert!(message.contains("***"));
    }

    #[test]
    fn stream_payload_commits_complete_tool_calls() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut builders = Vec::new();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = None;
        let mut response_model = Value::Null;
        let mut response_id = Value::Null;
        let mut created = Value::Null;
        let mut system_fingerprint = Value::Null;

        process_stream_payload(
            &json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_1",
                            "function": {
                                "name": "lookup",
                                "arguments": "{\"query\":\"refund\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }),
            &mut events,
            &mut text,
            &mut builders,
            &mut usage,
            &mut finish_reason,
            &mut response_model,
            &mut response_id,
            &mut created,
            &mut system_fingerprint,
        );

        assert!(events.iter().any(|event| {
            matches!(
                event,
                ProviderStreamEvent::ToolCallDelta { call_id, .. } if call_id == "call_1"
            )
        }));
        let calls = builders
            .into_iter()
            .filter_map(ToolCallBuilder::into_tool_call)
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "lookup");
        assert_eq!(calls[0].arguments, json!({ "query": "refund" }));
        assert_eq!(finish_reason, Some(ProviderFinishReason::ToolCall));
    }

    #[test]
    fn insufficient_system_resource_finish_reason_is_unknown() {
        assert_eq!(
            normalize_finish_reason(Some("insufficient_system_resource"), &[]),
            ProviderFinishReason::Unknown
        );
    }

    #[test]
    fn ac_002_generate_contract_accepts_only_current_strict_input() {
        let missing = serde_json::from_value::<ProviderInvocationInput>(json!({
            "model": "deepseek-v4-pro"
        }))
        .expect_err("missing current contract must fail before provider invocation");
        assert!(missing.to_string().contains("contract_version"));

        let current = json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "deepseek",
            "protocol": "deepseek",
            "model": "deepseek-v4-pro"
        });
        serde_json::from_value::<ProviderInvocationInput>(current.clone())
            .expect("current Generate input should deserialize");

        let mut legacy = current.clone();
        legacy["contract_version"] = json!("1flowbase.provider/v1");
        assert!(serde_json::from_value::<ProviderInvocationInput>(legacy).is_err());

        let mut unknown = current;
        unknown["raw_body"] = json!("must-not-be-accepted");
        let error = serde_json::from_value::<ProviderInvocationInput>(unknown)
            .expect_err("unknown Generate fields must fail closed");
        assert!(error.to_string().contains("raw_body"));
    }

    #[test]
    fn ac_002_package_manifest_declares_only_current_generate_contract() {
        let manifest = include_str!("../manifest.yaml");

        assert!(manifest.contains("contract_version: 1flowbase.provider/v2"));
        assert!(!manifest.contains("1flowbase.provider/v1"));
        assert!(!manifest.contains("capabilities:"));
    }

    #[test]
    fn ac_002_rejects_undeclared_generate_capabilities_before_wire_rendering() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "deepseek",
            "protocol": "deepseek",
            "model": "deepseek-v4-pro",
            "required_capabilities": ["system_prompt_blocks"]
        }))
        .unwrap();

        let error = build_chat_completion_body(&input)
            .expect_err("undeclared semantic capabilities must not be projected away");
        assert!(error.to_string().contains("semantic capabilities"));
    }

    #[test]
    fn ac_005_raw_sensitive_upstream_body_is_not_retained() {
        let canary = "raw-prompt-canary provider-secret";
        let message = provider_error_message(&Value::String(canary.to_string()));

        assert_eq!(message, "provider upstream request failed");
        assert!(!message.contains(canary));
    }
}
