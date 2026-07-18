use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    Method, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const PROVIDER_CODE: &str = "openai_compatible";
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
    "n",
    "max_tokens",
    "max_completion_tokens",
    "presence_penalty",
    "frequency_penalty",
    "stop",
    "logit_bias",
    "logprobs",
    "top_logprobs",
    "user",
    "seed",
    "tool_choice",
    "parallel_tool_calls",
    "store",
    "metadata",
    "audio",
    "modalities",
    "reasoning_effort",
];
const JSON_CHAT_COMPLETION_PARAMETERS: &[&str] = &[
    "audio",
    "logit_bias",
    "metadata",
    "modalities",
    "response_format",
    "tool_choice",
    "tools",
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
    authorization_header: Option<String>,
    organization: Option<String>,
    project: Option<String>,
    api_version: Option<String>,
    default_headers: BTreeMap<String, String>,
    #[allow(dead_code)]
    validate_model: bool,
    proxy_url: Option<String>,
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
        "validate" => {
            let config = normalize_provider_config(&request.input)?;
            let payload = request_json(&config, "/models", Method::GET, None).await?;
            Ok(ProviderStdioResponse::ok(json!({
                "ok": true,
                "provider_code": PROVIDER_CODE,
                "sanitized": {
                    "base_url": config.base_url,
                    "api_key": "***",
                    "authorization_header": config.authorization_header.as_ref().map(|_| "***"),
                    "organization": config.organization,
                    "project": config.project,
                    "api_version": config.api_version,
                    "default_headers": config.default_headers.keys().collect::<Vec<_>>(),
                    "proxy_url": config.proxy_url.as_ref().map(|_| "***"),
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

fn normalize_provider_config(input: &Value) -> Result<ProviderConfig> {
    let config = input
        .as_object()
        .ok_or_else(|| anyhow!("provider_config must be an object"))?;

    Ok(ProviderConfig {
        base_url: normalize_base_url(require_text(config.get("base_url"), "base_url")?),
        api_key: require_text(config.get("api_key"), "api_key")?,
        authorization_header: optional_text(config.get("authorization_header")),
        organization: optional_text(config.get("organization")),
        project: optional_text(config.get("project")),
        api_version: optional_text(config.get("api_version")),
        default_headers: parse_default_headers(config.get("default_headers"))?,
        validate_model: config
            .get("validate_model")
            .and_then(Value::as_bool)
            .unwrap_or(DEFAULT_VALIDATE_MODEL),
        proxy_url: normalize_proxy_url(config.get("proxy_url"))?,
    })
}

fn normalize_base_url(base_url: String) -> String {
    let trimmed = base_url.trim().trim_end_matches('/').to_string();
    let lower = trimmed.to_ascii_lowercase();
    let chat_completions_suffix = "/chat/completions";
    if lower.ends_with(chat_completions_suffix) {
        return trimmed[..trimmed.len() - chat_completions_suffix.len()]
            .trim_end_matches('/')
            .to_string();
    }
    trimmed
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
            let parsed: Value =
                serde_json::from_str(text).with_context(|| "default_headers must be valid JSON")?;
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

fn build_headers(
    config: &ProviderConfig,
    include_json_body: bool,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<HeaderMap> {
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
    let authorization_header = config
        .authorization_header
        .clone()
        .unwrap_or_else(|| format!("Bearer {}", config.api_key));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&authorization_header).context("invalid authorization header")?,
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
    let mut url = Url::parse(&format!("{base_url}{pathname}"))
        .with_context(|| format!("invalid base_url: {}", config.base_url))?;
    if let Some(api_version) = &config.api_version {
        url.query_pairs_mut()
            .append_pair("api-version", api_version);
    }
    Ok(url.to_string())
}

fn build_http_client(config: &ProviderConfig) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy_url) = &config.proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy_url")?);
    }
    builder
        .build()
        .context("building OpenAI-compatible HTTP client")
}

fn sanitize_error(error: reqwest::Error, config: &ProviderConfig) -> anyhow::Error {
    anyhow!(sanitize_text(error.to_string(), config))
}

fn sanitize_text(message: String, config: &ProviderConfig) -> String {
    let mut message = message.replace(&config.api_key, "***");
    if let Some(authorization_header) = &config.authorization_header {
        message = message.replace(authorization_header, "***");
    }
    if let Some(proxy_url) = &config.proxy_url {
        message = message.replace(proxy_url, "***");
    }
    message
}

async fn request_json(
    config: &ProviderConfig,
    pathname: &str,
    method: Method,
    body: Option<Value>,
) -> Result<Value> {
    let response = send_provider_request(config, pathname, method, body, None).await?;
    let status = response.status();
    let payload = read_json_response(response).await?;
    ensure_success_status(status, &payload, config)?;

    Ok(payload)
}

async fn send_provider_request(
    config: &ProviderConfig,
    pathname: &str,
    method: Method,
    body: Option<Value>,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<reqwest::Response> {
    let client = build_http_client(config)?;
    let mut request = client
        .request(method.clone(), build_url(config, pathname)?)
        .headers(build_headers(
            config,
            body.is_some(),
            client_protocol_envelope,
        )?);
    if let Some(body) = body {
        request = request.json(&body);
    }

    request
        .send()
        .await
        .map_err(|error| sanitize_error(error, config))
}

fn ensure_success_status(
    status: reqwest::StatusCode,
    payload: &Value,
    config: &ProviderConfig,
) -> Result<()> {
    if !status.is_success() {
        let message = payload
            .get("error")
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                payload
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "provider upstream request failed".to_string());
        bail!(
            "{} {}: {}",
            status.as_u16(),
            status,
            sanitize_text(message, config)
        );
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

fn explicit_number_alias(entry: &Value, aliases: &[&str]) -> Option<u64> {
    aliases
        .iter()
        .find_map(|alias| entry.get(alias).and_then(number_or_none_ref))
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
            Value::String(
                message
                    .content_blocks
                    .as_ref()
                    .map(normalize_message_content)
                    .unwrap_or_else(|| message.content.clone()),
            ),
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
            item.insert("tool_calls".to_string(), tool_calls.clone());
        }
        messages.push(Value::Object(item));
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
        .and_then(|value| normalize_parameter_value(key, value))
}

fn normalize_parameter_value(key: &str, value: Value) -> Option<Value> {
    match key {
        "stop" => normalize_stop_parameter(value),
        _ if JSON_CHAT_COMPLETION_PARAMETERS.contains(&key) => normalize_json_parameter(value),
        _ => normalize_scalar_parameter(value),
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

fn normalize_stop_parameter(value: Value) -> Option<Value> {
    match normalize_scalar_parameter(value)? {
        Value::String(text) => serde_json::from_str::<Value>(&text)
            .ok()
            .filter(Value::is_array)
            .or(Some(Value::String(text))),
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

    let response = send_provider_request(
        &config,
        "/chat/completions",
        Method::POST,
        Some(body),
        input.client_protocol_envelope.as_ref(),
    )
    .await?;
    read_streaming_chat_completion(response, input.model, &config, &mut on_event).await
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
        bail!("OpenAI Compatible Generate does not support the requested semantic capabilities");
    }
    let model = input.model.trim();
    if model.is_empty() {
        bail!("model is required");
    }
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model.to_string()));
    body.insert(
        "messages".to_string(),
        Value::Array(build_invocation_messages(&input)),
    );
    body.insert("stream".to_string(), Value::Bool(true));
    body.insert(
        "stream_options".to_string(),
        json!({ "include_usage": true }),
    );
    if let Some(response_format) = input
        .response_format
        .clone()
        .or_else(|| parameter_value(&input, "response_format"))
    {
        body.insert("response_format".to_string(), response_format);
    }
    if !input.tools.is_empty() {
        body.insert("tools".to_string(), Value::Array(input.tools.clone()));
    } else if let Some(tools) = parameter_value(&input, "tools") {
        body.insert("tools".to_string(), tools);
    }
    for key in PASSTHROUGH_CHAT_COMPLETION_PARAMETERS {
        if let Some(value) = parameter_value(&input, key) {
            body.insert((*key).to_string(), value);
        }
    }

    Ok(Value::Object(body))
}

async fn read_streaming_chat_completion<F>(
    response: reqwest::Response,
    request_model: String,
    config: &ProviderConfig,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let status = response.status();
    if !status.is_success() {
        let payload = read_json_response(response).await?;
        ensure_success_status(status, &payload, config)?;
        unreachable!("ensure_success_status returns error for non-success response");
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
        let chunk_text = String::from_utf8_lossy(&chunk);
        buffer.push_str(&chunk_text);
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
        .map(ToolCallBuilder::into_tool_call)
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
            response_id: response_id.as_str().map(ToOwned::to_owned),
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
    if let Some(content) = extract_content(delta.get("content")).filter(|value| !value.is_empty()) {
        text.push_str(&content);
        events.push(ProviderStreamEvent::TextDelta { delta: content });
    }
    if let Some(reasoning) = extract_reasoning_delta(delta).filter(|value| !value.is_empty()) {
        events.push(ProviderStreamEvent::ReasoningDelta { delta: reasoning });
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

#[derive(Debug, Clone, Default)]
struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl ToolCallBuilder {
    fn into_tool_call(self) -> ProviderToolCall {
        ProviderToolCall {
            id: self.id.unwrap_or_else(|| "tool_call_1".to_string()),
            name: self.name.unwrap_or_else(|| "unknown_tool".to_string()),
            arguments: serde_json::from_str(&self.arguments)
                .unwrap_or_else(|_| json!({ "raw": self.arguments })),
        }
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
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Duration,
    };

    #[tokio::test]
    async fn ac_005_validate_redacts_configured_proxy_url() {
        let (proxy_url, capture_handle) = capture_proxy_models_request();
        let response = handle_request(ProviderStdioRequest {
            method: "validate".to_string(),
            input: json!({
                "base_url": "http://api.example.test/v1",
                "api_key": "provider-secret",
                "validate_model": false,
                "proxy_url": proxy_url
            }),
        })
        .await
        .unwrap();

        assert!(response.ok);
        assert_eq!(response.result["sanitized"]["proxy_url"], "***");
        assert!(!response.result.to_string().contains(&proxy_url));
        assert!(!response.result.to_string().contains("proxy-pass"));

        let request = capture_handle
            .join()
            .expect("proxy capture should finish successfully");
        assert!(
            request.starts_with("GET http://api.example.test/v1/models "),
            "validate request should be sent through the configured proxy"
        );
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn capture_single_json_request() -> (String, thread::JoinHandle<String>) {
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
                            "data: {\"id\":\"chatcmpl_test\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\n",
                            "data: {\"id\":\"chatcmpl_test\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n\n",
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

    fn capture_blocked_streaming_request(
        release_tail: mpsc::Receiver<()>,
    ) -> (String, thread::JoinHandle<String>) {
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
                        stream
                            .write_all(
                                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
                            )
                            .expect("response headers should be writable");
                        write_chunk(
                            &mut stream,
                            "data: {\"id\":\"chatcmpl_test\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"hel\"},\"finish_reason\":null}]}\n\n",
                        );
                        stream.flush().expect("first chunk should flush");

                        release_tail
                            .recv_timeout(Duration::from_secs(5))
                            .expect("test should release response tail");
                        write_chunk(
                            &mut stream,
                            "data: {\"id\":\"chatcmpl_test\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":null}]}\n\n",
                        );
                        write_chunk(
                            &mut stream,
                            "data: {\"id\":\"chatcmpl_test\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                        );
                        write_chunk(&mut stream, "data: [DONE]\n\n");
                        stream
                            .write_all(b"0\r\n\r\n")
                            .expect("terminating chunk should be writable");
                        return String::from_utf8(buffer[end..end + length].to_vec())
                            .expect("request body should be utf8");
                    }
                }
            }

            panic!("request body was not fully captured");
        });

        (address, handle)
    }

    fn capture_proxy_chat_request() -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("proxy listener should bind");
        let proxy_url = format!(
            "http://{}",
            listener.local_addr().expect("proxy listener addr")
        );

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should connect to proxy");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("proxy read timeout");

            let mut buffer = Vec::new();
            let mut chunk = [0_u8; 4096];
            let mut header_end = None;
            let mut body_length = None;

            loop {
                let read = stream
                    .read(&mut chunk)
                    .expect("proxy request should be readable");
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
                            "data: {\"id\":\"chatcmpl_proxy\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"proxied\"},\"finish_reason\":null}]}\n\n",
                            "data: {\"id\":\"chatcmpl_proxy\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
                            "data: [DONE]\n\n"
                        );
                        let response = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                            response_body.len(),
                            response_body
                        );
                        stream
                            .write_all(response.as_bytes())
                            .expect("proxy response should be writable");
                        return String::from_utf8(buffer[..end + length].to_vec())
                            .expect("proxy request should be utf8");
                    }
                }
            }

            panic!("proxy request was not fully captured");
        });

        (proxy_url, handle)
    }

    fn capture_proxy_models_request() -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("proxy listener should bind");
        let proxy_url = format!(
            "http://proxy-user:proxy-pass@{}",
            listener.local_addr().expect("proxy listener addr")
        );

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should connect to proxy");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("proxy read timeout");

            let mut buffer = Vec::new();
            let mut chunk = [0_u8; 4096];

            loop {
                let read = stream
                    .read(&mut chunk)
                    .expect("proxy request should be readable");
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);

                if let Some(end) = find_bytes(&buffer, b"\r\n\r\n").map(|offset| offset + 4) {
                    let response_body = r#"{"data":[]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        response_body.len(),
                        response_body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("proxy response should be writable");
                    return String::from_utf8(buffer[..end].to_vec())
                        .expect("proxy request should be utf8");
                }
            }

            panic!("proxy request was not fully captured");
        });

        (proxy_url, handle)
    }

    fn write_chunk(stream: &mut std::net::TcpStream, payload: &str) {
        write!(stream, "{:x}\r\n", payload.len()).expect("chunk size should be writable");
        stream
            .write_all(payload.as_bytes())
            .expect("chunk payload should be writable");
        stream
            .write_all(b"\r\n")
            .expect("chunk trailer should be writable");
    }

    #[test]
    fn client_protocol_envelope_uses_default_deny_policy_for_headers() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "openai_compatible",
            "protocol": "openai_compatible",
            "model": "gpt-compatible",
            "client_protocol_envelope": {
                "source_protocol": "openai_chat",
                "policy": "default_deny",
                "headers": {
                    "authorization": "Bearer client-secret",
                    "x-api-key": "client-api-key",
                    "x-client-name": "ClaudeCode",
                    "connection": "keep-alive"
                }
            }
        }))
        .unwrap();

        assert!(input.client_protocol_envelope.is_some());

        let config = normalize_provider_config(&json!({
            "base_url": "https://compatible.example/v1",
            "api_key": "provider-secret",
            "default_headers": {
                "x-provider-default": "kept"
            }
        }))
        .unwrap();
        let headers =
            build_headers(&config, true, input.client_protocol_envelope.as_ref()).unwrap();

        assert_eq!(
            headers.get(AUTHORIZATION).unwrap(),
            "Bearer provider-secret"
        );
        assert_eq!(headers.get("x-provider-default").unwrap(), "kept");
        assert!(headers.get("x-api-key").is_none());
        assert!(headers.get("x-client-name").is_none());
        assert!(headers.get("connection").is_none());
    }

    #[test]
    fn headers_use_configured_authorization_header_without_bearer_prefix() {
        let config = normalize_provider_config(&json!({
            "base_url": "https://compatible.example/v1",
            "api_key": "provider-secret",
            "authorization_header": "123123123"
        }))
        .unwrap();

        let headers = build_headers(&config, true, None).unwrap();

        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "123123123");
    }

    #[test]
    fn headers_restore_anthropic_client_protocol_envelope_and_keep_config_auth() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "openai_compatible",
            "protocol": "openai_compatible",
            "model": "gpt-compatible",
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
            "base_url": "https://compatible.example/v1",
            "api_key": "provider-secret",
            "default_headers": {
                "x-provider-default": "kept"
            }
        }))
        .unwrap();
        let headers =
            build_headers(&config, true, input.client_protocol_envelope.as_ref()).unwrap();

        assert_eq!(
            headers.get(AUTHORIZATION).unwrap(),
            "Bearer provider-secret"
        );
        assert_eq!(headers.get("x-provider-default").unwrap(), "kept");
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

    #[test]
    fn normalize_provider_config_requires_base_url_and_api_key() {
        let error = normalize_provider_config(&json!({ "base_url": "", "api_key": "" }))
            .expect_err("missing credentials must fail");

        assert!(error.to_string().contains("base_url"));
    }

    #[test]
    fn normalize_provider_config_accepts_full_chat_completions_endpoint_as_base_url() {
        let config = normalize_provider_config(&json!({
            "base_url": "https://compatible.example/v1/chat/completions/",
            "api_key": "provider-secret"
        }))
        .unwrap();

        assert_eq!(config.base_url, "https://compatible.example/v1");
        assert_eq!(
            build_url(&config, "/chat/completions").unwrap(),
            "https://compatible.example/v1/chat/completions"
        );
    }

    #[tokio::test]
    async fn ac_003_http_invocation_uses_configured_proxy_url() {
        let (proxy_url, capture_handle) = capture_proxy_chat_request();

        let envelope = invoke_chat_completion(ProviderInvocationInput {
            model: "gpt-4o-mini".to_string(),
            provider_config: json!({
                "base_url": "http://127.0.0.1:9/v1",
                "api_key": "test-key",
                "proxy_url": proxy_url
            }),
            messages: vec![ProviderMessage {
                role: ProviderMessageRole::User,
                content: "hello".to_string(),
                name: None,
                tool_call_id: None,
                is_error: None,
                tool_calls: None,
                content_blocks: None,
            }],
            ..ProviderInvocationInput::default()
        })
        .await
        .expect("invocation should use proxy and succeed");

        assert_eq!(envelope.result.final_content.as_deref(), Some("proxied"));
        let captured = capture_handle
            .join()
            .expect("proxy capture thread should finish");
        assert!(
            captured.starts_with("POST http://127.0.0.1:9/v1/chat/completions HTTP/1.1"),
            "proxy should receive absolute-form upstream request, got: {captured}"
        );
        assert!(captured.contains("\"model\":\"gpt-4o-mini\""));
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

    #[tokio::test]
    async fn invoke_chat_completion_emits_text_delta_before_upstream_stream_finishes() {
        let (release_tail_tx, release_tail_rx) = mpsc::channel();
        let (base_url, capture_handle) = capture_blocked_streaming_request(release_tail_rx);
        let (event_tx, event_rx) = mpsc::channel();

        let invocation = tokio::spawn(async move {
            invoke_chat_completion_with_event_sink(
                ProviderInvocationInput {
                    model: "gpt-4o-mini".to_string(),
                    provider_config: json!({
                        "base_url": base_url,
                        "api_key": "test-key"
                    }),
                    messages: vec![ProviderMessage {
                        role: ProviderMessageRole::User,
                        content: "hello".to_string(),
                        name: None,
                        tool_call_id: None,
                        is_error: None,
                        tool_calls: None,
                        content_blocks: None,
                    }],
                    ..ProviderInvocationInput::default()
                },
                |event| {
                    let _ = event_tx.send(event.clone());
                    Ok(())
                },
            )
            .await
        });

        let first_event = match tokio::task::spawn_blocking(move || {
            event_rx.recv_timeout(Duration::from_secs(2))
        })
        .await
        .expect("event wait task should not panic")
        {
            Ok(event) => event,
            Err(error) => {
                let _ = release_tail_tx.send(());
                panic!("expected first delta before upstream stream finished: {error}");
            }
        };
        assert_eq!(
            first_event,
            ProviderStreamEvent::TextDelta {
                delta: "hel".to_string()
            }
        );

        release_tail_tx
            .send(())
            .expect("response tail should be released");
        let envelope = invocation
            .await
            .expect("invocation task should not panic")
            .expect("invocation should succeed");

        assert_eq!(envelope.result.final_content.as_deref(), Some("hello"));
        let captured_body: Value =
            serde_json::from_str(&capture_handle.join().expect("capture thread should finish"))
                .expect("captured body should parse");
        assert_eq!(captured_body["stream"], json!(true));
    }

    #[tokio::test]
    async fn invoke_chat_completion_forwards_extended_chat_completion_parameters() {
        let (base_url, capture_handle) = capture_single_json_request();

        let envelope = invoke_chat_completion(ProviderInvocationInput {
            model: "gpt-4o-mini".to_string(),
            provider_config: json!({
                "base_url": base_url,
                "api_key": "test-key"
            }),
            messages: vec![
                ProviderMessage {
                    role: ProviderMessageRole::Assistant,
                    content: String::new(),
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: Some(json!([{
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
                    role: ProviderMessageRole::Tool,
                    content: "tool result".to_string(),
                    name: None,
                    tool_call_id: Some("call_1".to_string()),
                    is_error: None,
                    tool_calls: None,
                    content_blocks: None,
                },
                ProviderMessage {
                    role: ProviderMessageRole::User,
                    content: "hello".to_string(),
                    name: Some("customer".to_string()),
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: None,
                    content_blocks: None,
                },
            ],
            model_parameters: BTreeMap::from([
                ("temperature".to_string(), json!(0.7)),
                ("top_p".to_string(), json!(0.9)),
                ("n".to_string(), json!(1)),
                ("max_tokens".to_string(), json!(512)),
                ("max_completion_tokens".to_string(), json!(1024)),
                ("presence_penalty".to_string(), json!(0.4)),
                ("frequency_penalty".to_string(), json!(-0.2)),
                ("stop".to_string(), json!(r#"["END","STOP"]"#)),
                ("logit_bias".to_string(), json!(r#"{"50256":-100}"#)),
                ("logprobs".to_string(), json!(true)),
                ("top_logprobs".to_string(), json!(5)),
                (
                    "response_format".to_string(),
                    json!(r#"{"type":"json_object"}"#),
                ),
                ("user".to_string(), json!("trace-user-1")),
                ("seed".to_string(), json!(42)),
                (
                    "tools".to_string(),
                    json!(r#"[{"type":"function","function":{"name":"lookup","parameters":{"type":"object","properties":{}}}}]"#),
                ),
                (
                    "tool_choice".to_string(),
                    json!(r#"{"type":"function","function":{"name":"lookup"}}"#),
                ),
                ("parallel_tool_calls".to_string(), json!(false)),
                ("store".to_string(), json!(true)),
                ("metadata".to_string(), json!(r#"{"trace_id":"trace-1"}"#)),
                (
                    "audio".to_string(),
                    json!(r#"{"voice":"alloy","format":"wav"}"#),
                ),
                ("modalities".to_string(), json!(r#"["text"]"#)),
                ("reasoning_effort".to_string(), json!("low")),
            ]),
            ..ProviderInvocationInput::default()
        })
        .await
        .expect("invocation should succeed");

        let captured_body: Value =
            serde_json::from_str(&capture_handle.join().expect("capture thread should finish"))
                .expect("captured body should parse");

        assert_eq!(captured_body["model"], "gpt-4o-mini");
        assert_eq!(captured_body["messages"][0]["role"], "assistant");
        assert_eq!(captured_body["messages"][0]["content"], "");
        assert_eq!(
            captured_body["messages"][0]["tool_calls"][0]["id"],
            "call_1"
        );
        assert_eq!(
            captured_body["messages"][0]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
        assert_eq!(captured_body["messages"][1]["role"], "tool");
        assert_eq!(captured_body["messages"][1]["tool_call_id"], "call_1");
        assert_eq!(captured_body["messages"][2]["name"], "customer");
        assert_eq!(captured_body["temperature"], json!(0.7));
        assert_eq!(captured_body["top_p"], json!(0.9));
        assert_eq!(captured_body["n"], json!(1));
        assert_eq!(captured_body["max_tokens"], json!(512));
        assert_eq!(captured_body["max_completion_tokens"], json!(1024));
        assert_eq!(captured_body["presence_penalty"], json!(0.4));
        assert_eq!(captured_body["frequency_penalty"], json!(-0.2));
        assert_eq!(captured_body["stop"], json!(["END", "STOP"]));
        assert_eq!(captured_body["logit_bias"], json!({ "50256": -100 }));
        assert_eq!(captured_body["logprobs"], json!(true));
        assert_eq!(captured_body["top_logprobs"], json!(5));
        assert_eq!(
            captured_body["response_format"],
            json!({ "type": "json_object" })
        );
        assert_eq!(captured_body["user"], json!("trace-user-1"));
        assert_eq!(captured_body["seed"], json!(42));
        assert_eq!(
            captured_body["tools"],
            json!([{
                "type": "function",
                "function": {
                    "name": "lookup",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                }
            }])
        );
        assert_eq!(
            captured_body["tool_choice"],
            json!({
                "type": "function",
                "function": {
                    "name": "lookup"
                }
            })
        );
        assert_eq!(captured_body["parallel_tool_calls"], json!(false));
        assert_eq!(captured_body["store"], json!(true));
        assert_eq!(captured_body["metadata"], json!({ "trace_id": "trace-1" }));
        assert_eq!(
            captured_body["audio"],
            json!({
                "voice": "alloy",
                "format": "wav"
            })
        );
        assert_eq!(captured_body["modalities"], json!(["text"]));
        assert_eq!(captured_body["reasoning_effort"], json!("low"));
        assert_eq!(captured_body["stream"], json!(true));
        assert_eq!(
            captured_body["stream_options"],
            json!({ "include_usage": true })
        );
        assert_eq!(envelope.result.final_content.as_deref(), Some("ok"));
        assert!(envelope.events.contains(&ProviderStreamEvent::TextDelta {
            delta: "ok".to_string()
        }));
        assert!(envelope
            .events
            .contains(&ProviderStreamEvent::UsageSnapshot {
                usage: ProviderUsage {
                    input_tokens: Some(3),
                    output_tokens: Some(2),
                    total_tokens: Some(5),
                    ..ProviderUsage::default()
                }
            }));
        assert!(envelope.events.contains(&ProviderStreamEvent::Finish {
            reason: ProviderFinishReason::Stop
        }));
    }

    #[tokio::test]
    async fn ac_002_fake_upstream_receives_exact_generate_wire() {
        let (base_url, capture_handle) = capture_single_json_request();
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "openai_compatible",
            "protocol": "openai_compatible",
            "model": "gpt-4o-mini",
            "provider_config": {
                "base_url": base_url,
                "api_key": "test-key"
            },
            "system": [{ "type": "text", "text": "Be concise" }],
            "messages": [{ "role": "user", "content": "hello" }]
        }))
        .unwrap();

        invoke_chat_completion(input)
            .await
            .expect("current Generate should complete against fake upstream");
        let captured_body: Value = serde_json::from_str(
            &capture_handle
                .join()
                .expect("fake upstream should capture request"),
        )
        .unwrap();

        assert_eq!(
            captured_body,
            json!({
                "model": "gpt-4o-mini",
                "messages": [
                    { "role": "system", "content": "Be concise" },
                    { "role": "user", "content": "hello" }
                ],
                "stream": true,
                "stream_options": { "include_usage": true }
            })
        );
    }

    #[test]
    fn ac_002_generate_contract_accepts_only_current_strict_input() {
        let missing = serde_json::from_value::<ProviderInvocationInput>(json!({
            "model": "gpt-compatible"
        }))
        .expect_err("missing current contract must fail before provider invocation");
        assert!(missing.to_string().contains("contract_version"));

        let current = json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "openai_compatible",
            "protocol": "openai_compatible",
            "model": "gpt-compatible"
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
            "provider_code": "openai_compatible",
            "protocol": "openai_compatible",
            "model": "gpt-compatible",
            "required_capabilities": ["end_user_reference"]
        }))
        .unwrap();

        let error = build_chat_completion_body(&input)
            .expect_err("undeclared semantic capabilities must not be projected away");
        assert!(error.to_string().contains("semantic capabilities"));
    }

    #[test]
    fn ac_005_raw_sensitive_upstream_body_is_not_retained() {
        let canary = "raw-prompt-canary provider-secret";
        let config = normalize_provider_config(&json!({
            "base_url": "https://compatible.example/v1",
            "api_key": "provider-secret"
        }))
        .unwrap();
        let error = ensure_success_status(
            reqwest::StatusCode::BAD_REQUEST,
            &Value::String(canary.to_string()),
            &config,
        )
        .expect_err("upstream failure should remain an error");
        let message = error.to_string();

        assert!(message.contains("provider upstream request failed"));
        assert!(!message.contains(canary));
    }
}
