use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    Method, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

mod stream;

use stream::{
    read_anthropic_streaming_response, read_chat_streaming_response,
    read_dashscope_streaming_response, read_responses_streaming_response,
};

const PROVIDER_CODE: &str = "aliyun_bailian";
const DEFAULT_BASE_URL: &str = "https://dashscope.aliyuncs.com";
const DEFAULT_VALIDATE_MODEL: bool = true;
const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_ANTHROPIC_MAX_TOKENS: u64 = 4096;
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
const PASSTHROUGH_CHAT_PARAMETERS: &[&str] = &[
    "temperature",
    "top_p",
    "max_tokens",
    "max_completion_tokens",
    "presence_penalty",
    "frequency_penalty",
    "stop",
    "tool_choice",
    "parallel_tool_calls",
    "enable_thinking",
    "enable_search",
    "search_options",
];
const PASSTHROUGH_RESPONSES_PARAMETERS: &[&str] = &[
    "temperature",
    "top_p",
    "max_output_tokens",
    "tool_choice",
    "parallel_tool_calls",
    "store",
    "include",
    "enable_thinking",
    "enable_search",
    "search_options",
];
const PASSTHROUGH_DASHSCOPE_PARAMETERS: &[&str] = &[
    "temperature",
    "top_p",
    "top_k",
    "max_tokens",
    "seed",
    "stop",
    "enable_search",
    "incremental_output",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BailianProtocol {
    OpenAiResponses,
    OpenAiChat,
    AnthropicMessages,
    DashScope,
}

impl BailianProtocol {
    fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "openai_responses",
            Self::OpenAiChat => "openai_chat",
            Self::AnthropicMessages => "anthropic_messages",
            Self::DashScope => "dashscope",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderConfig {
    base_url: String,
    api_key: String,
    api_protocol: BailianProtocol,
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
            let model_count = if config.validate_model {
                request_json(
                    &config,
                    BailianProtocol::OpenAiResponses,
                    "/models",
                    Method::GET,
                    None,
                    false,
                )
                .await
                .ok()
                .and_then(|payload| payload.get("data").and_then(Value::as_array).map(Vec::len))
                .unwrap_or(0)
            } else {
                0
            };
            Ok(ProviderStdioResponse::ok(json!({
                "ok": true,
                "provider_code": PROVIDER_CODE,
                "sanitized": {
                    "base_url": config.base_url,
                    "api_key": "***",
                    "api_protocol": config.api_protocol.as_str(),
                    "proxy_url": config.proxy_url.as_ref().map(|_| "***"),
                },
                "model_count": model_count,
            })))
        }
        "list_models" => {
            let config = normalize_provider_config(&request.input)?;
            let dynamic = if config.validate_model {
                request_json(
                    &config,
                    BailianProtocol::OpenAiResponses,
                    "/models",
                    Method::GET,
                    None,
                    false,
                )
                .await
                .ok()
                .and_then(|payload| {
                    normalize_model_entries(payload.get("data").unwrap_or(&Value::Null)).ok()
                })
                .unwrap_or_default()
            } else {
                Vec::new()
            };
            let models = if dynamic.is_empty() {
                static_models()
            } else {
                dynamic
            };
            Ok(ProviderStdioResponse::ok(json!(models)))
        }
        "invoke" => {
            let input: ProviderInvocationInput = serde_json::from_value(request.input)?;
            let output = invoke(input).await?;
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
    let output = invoke_with_event_sink(input, on_event).await?;
    Ok(output.result)
}

async fn invoke(input: ProviderInvocationInput) -> Result<RuntimeInvocationEnvelope> {
    invoke_with_event_sink(input, |_| Ok(())).await
}

async fn invoke_with_event_sink<F>(
    input: ProviderInvocationInput,
    mut on_event: F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
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
        bail!("Aliyun Bailian Generate does not support the requested semantic capabilities");
    }
    let config = normalize_provider_config(&input.provider_config)?;
    let protocol = invocation_protocol(&config, &input)?;
    match protocol {
        BailianProtocol::OpenAiChat => invoke_openai_chat(&config, &input, &mut on_event).await,
        BailianProtocol::OpenAiResponses => {
            invoke_openai_responses(&config, &input, &mut on_event).await
        }
        BailianProtocol::AnthropicMessages => {
            invoke_anthropic_messages(&config, &input, &mut on_event).await
        }
        BailianProtocol::DashScope => invoke_dashscope(&config, &input, &mut on_event).await,
    }
}

fn normalize_provider_config(input: &Value) -> Result<ProviderConfig> {
    let config = input
        .as_object()
        .ok_or_else(|| anyhow!("provider_config must be an object"))?;
    Ok(ProviderConfig {
        base_url: optional_text(config.get("base_url"))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        api_key: require_text(config.get("api_key"), "api_key")?,
        api_protocol: config
            .get("api_protocol")
            .and_then(|value| normalize_protocol(&value_to_string(value)).ok())
            .unwrap_or(BailianProtocol::OpenAiChat),
        validate_model: config
            .get("validate_model")
            .and_then(Value::as_bool)
            .unwrap_or(DEFAULT_VALIDATE_MODEL),
        proxy_url: normalize_proxy_url(config.get("proxy_url"))?,
    })
}

fn invocation_protocol(
    config: &ProviderConfig,
    input: &ProviderInvocationInput,
) -> Result<BailianProtocol> {
    parameter_value(input, "api_protocol")
        .map(|value| normalize_protocol(&value_to_string(&value)))
        .transpose()
        .map(|value| value.unwrap_or(config.api_protocol))
}

fn normalize_protocol(value: &str) -> Result<BailianProtocol> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => Ok(BailianProtocol::OpenAiChat),
        "openai_responses" | "responses" => Ok(BailianProtocol::OpenAiResponses),
        "openai_chat" | "chat" | "chat_completions" | "openai_compatible" => {
            Ok(BailianProtocol::OpenAiChat)
        }
        "anthropic_messages" | "anthropic" | "messages" => Ok(BailianProtocol::AnthropicMessages),
        "dashscope" | "dashscope_native" => Ok(BailianProtocol::DashScope),
        other => bail!("unsupported api_protocol: {other}"),
    }
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

fn build_url(config: &ProviderConfig, protocol: BailianProtocol, pathname: &str) -> Result<String> {
    let base = config.base_url.trim_end_matches('/');
    let base = match protocol {
        BailianProtocol::OpenAiChat | BailianProtocol::OpenAiResponses => {
            if base.ends_with("/compatible-mode/v1") {
                base.to_string()
            } else {
                format!("{base}/compatible-mode/v1")
            }
        }
        BailianProtocol::AnthropicMessages => {
            if base.ends_with("/apps/anthropic") {
                base.to_string()
            } else {
                format!("{base}/apps/anthropic")
            }
        }
        BailianProtocol::DashScope => {
            if base.ends_with("/api/v1") {
                base.to_string()
            } else {
                format!("{base}/api/v1")
            }
        }
    };
    Url::parse(&format!("{base}{pathname}"))
        .with_context(|| format!("invalid base_url: {}", config.base_url))
        .map(|value| value.to_string())
}

fn build_http_client(config: &ProviderConfig) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy_url) = &config.proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy_url")?);
    }
    builder.build().context("building Bailian HTTP client")
}

fn sanitize_reqwest_error(error: reqwest::Error, config: &ProviderConfig) -> anyhow::Error {
    anyhow!(sanitize_text(error.to_string(), config))
}

fn sanitize_text(message: String, config: &ProviderConfig) -> String {
    let mut message = message.replace(&config.api_key, "***");
    if let Some(proxy_url) = &config.proxy_url {
        message = message.replace(proxy_url, "***");
    }
    message
}

fn build_headers(
    config: &ProviderConfig,
    protocol: BailianProtocol,
    include_json_body: bool,
    stream: bool,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(if stream {
            "text/event-stream"
        } else {
            "application/json"
        }),
    );
    if include_json_body {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    }
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", config.api_key))
            .context("invalid api_key for authorization header")?,
    );
    if protocol == BailianProtocol::AnthropicMessages {
        headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(&config.api_key).context("invalid api_key header")?,
        );
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static(DEFAULT_ANTHROPIC_VERSION),
        );
    }
    if protocol == BailianProtocol::DashScope && stream {
        headers.insert(
            HeaderName::from_static("x-dashscope-sse"),
            HeaderValue::from_static("enable"),
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

async fn request_json(
    config: &ProviderConfig,
    protocol: BailianProtocol,
    pathname: &str,
    method: Method,
    body: Option<Value>,
    stream: bool,
) -> Result<Value> {
    let client = build_http_client(config)?;
    let mut request = client
        .request(method, build_url(config, protocol, pathname)?)
        .headers(build_headers(
            config,
            protocol,
            body.is_some(),
            stream,
            None,
        )?);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, config))?;
    let status = response.status();
    let payload = read_json_response(response).await?;
    ensure_success_status(status, &payload, config)?;
    Ok(payload)
}

async fn read_json_response(response: reqwest::Response) -> Result<Value> {
    let text = response.text().await?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).with_context(|| "provider returned invalid JSON")
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
            .or_else(|| payload.get("message").and_then(Value::as_str))
            .unwrap_or_else(|| payload.as_str().unwrap_or("provider request failed"));
        bail!(
            "{} {}: {}",
            status.as_u16(),
            status,
            sanitize_text(message.to_string(), config)
        );
    }
    Ok(())
}

async fn invoke_openai_chat<F>(
    config: &ProviderConfig,
    input: &ProviderInvocationInput,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(required_model(input)?));
    body.insert(
        "messages".to_string(),
        Value::Array(build_chat_messages(input)),
    );
    body.insert("stream".to_string(), Value::Bool(true));
    body.insert(
        "stream_options".to_string(),
        json!({ "include_usage": true }),
    );
    if !input.tools.is_empty() {
        body.insert("tools".to_string(), Value::Array(input.tools.clone()));
    }
    if let Some(response_format) = input
        .response_format
        .clone()
        .or_else(|| parameter_value(input, "response_format"))
        .map(normalize_response_format)
    {
        body.insert("response_format".to_string(), response_format);
    }
    for key in PASSTHROUGH_CHAT_PARAMETERS {
        if let Some(value) = parameter_value(input, key) {
            body.insert((*key).to_string(), normalize_jsonish_parameter(value));
        }
    }
    let response = build_http_client(config)?
        .post(build_url(
            config,
            BailianProtocol::OpenAiChat,
            "/chat/completions",
        )?)
        .headers(build_headers(
            config,
            BailianProtocol::OpenAiChat,
            true,
            true,
            input.client_protocol_envelope.as_ref(),
        )?)
        .json(&Value::Object(body))
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, config))?;
    read_chat_streaming_response(
        response,
        input.model.clone(),
        BailianProtocol::OpenAiChat,
        config,
        on_event,
    )
    .await
}

fn build_chat_messages(input: &ProviderInvocationInput) -> Vec<Value> {
    let mut messages = Vec::new();
    if let Some(system) = input.system_text() {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in &input.messages {
        let mut item = Map::new();
        item.insert(
            "role".to_string(),
            Value::String(message_role_name(message.role).to_string()),
        );
        item.insert("content".to_string(), chat_content_value(message));
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
        if let Some(tool_calls) = normalized_chat_tool_calls(message.tool_calls.as_ref()) {
            item.insert("tool_calls".to_string(), Value::Array(tool_calls));
        }
        messages.push(Value::Object(item));
    }
    messages
}

fn chat_content_value(message: &ProviderMessage) -> Value {
    let Some(content) = message.content_blocks.as_ref() else {
        return Value::String(message.content.clone());
    };
    match content {
        Value::Array(parts) => Value::Array(parts.iter().filter_map(chat_content_part).collect()),
        Value::Null => Value::String(message.content.clone()),
        Value::String(_) => content.clone(),
        other => Value::String(other.to_string()),
    }
}

fn chat_content_part(part: &Value) -> Option<Value> {
    let object = part.as_object()?;
    let part_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match part_type {
        "text" | "input_text" => object
            .get("text")
            .or_else(|| object.get("content"))
            .and_then(Value::as_str)
            .map(|text| json!({ "type": "text", "text": text })),
        "image_url" => object.get("image_url").map(|image_url| {
            json!({
                "type": "image_url",
                "image_url": image_url,
            })
        }),
        "input_image" => object.get("image_url").or_else(|| object.get("url")).map(
            |image_url| json!({ "type": "image_url", "image_url": image_url_value(image_url) }),
        ),
        "image" => object
            .get("image")
            .map(|image| json!({ "type": "image_url", "image_url": image_url_value(image) })),
        _ => Some(part.clone()),
    }
}

fn image_url_value(value: &Value) -> Value {
    if value.is_object() {
        value.clone()
    } else {
        json!({ "url": value_to_string(value) })
    }
}

fn normalized_chat_tool_calls(tool_calls: Option<&Value>) -> Option<Vec<Value>> {
    let calls = tool_calls.and_then(Value::as_array)?;
    Some(
        calls
            .iter()
            .enumerate()
            .filter_map(chat_tool_call_from_native)
            .collect(),
    )
}

fn chat_tool_call_from_native((index, tool_call): (usize, &Value)) -> Option<Value> {
    let object = tool_call.as_object()?;
    let function = object.get("function").and_then(Value::as_object);
    let name = function
        .and_then(|value| value.get("name"))
        .or_else(|| object.get("name"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())?;
    let arguments = function
        .and_then(|value| value.get("arguments"))
        .or_else(|| object.get("arguments"))
        .map(tool_arguments_string)
        .unwrap_or_else(|| "{}".to_string());
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("call_{index}"));
    Some(json!({
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments,
        }
    }))
}

fn tool_arguments_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "{}".to_string(),
        other => other.to_string(),
    }
}

async fn invoke_openai_responses<F>(
    config: &ProviderConfig,
    input: &ProviderInvocationInput,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let body = build_responses_body(input)?;
    let response = build_http_client(config)?
        .post(build_url(
            config,
            BailianProtocol::OpenAiResponses,
            "/responses",
        )?)
        .headers(build_headers(
            config,
            BailianProtocol::OpenAiResponses,
            true,
            true,
            input.client_protocol_envelope.as_ref(),
        )?)
        .json(&body)
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, config))?;
    read_responses_streaming_response(response, input.model.clone(), config, on_event).await
}

fn build_responses_body(input: &ProviderInvocationInput) -> Result<Value> {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(required_model(input)?));
    body.insert(
        "input".to_string(),
        Value::Array(build_responses_input(input)),
    );
    if let Some(previous_response_id) = input
        .previous_response_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        body.insert(
            "previous_response_id".to_string(),
            Value::String(previous_response_id.to_string()),
        );
    }
    body.insert("stream".to_string(), Value::Bool(true));
    if let Some(system) = input.system_text() {
        body.insert("instructions".to_string(), Value::String(system));
    }
    if !input.tools.is_empty() {
        body.insert(
            "tools".to_string(),
            Value::Array(input.tools.iter().map(response_tool).collect()),
        );
    }
    if let Some(response_format) = input
        .response_format
        .clone()
        .or_else(|| parameter_value(input, "response_format"))
        .map(normalize_response_format)
    {
        body.insert("text".to_string(), json!({ "format": response_format }));
    }
    if let Some(reasoning_effort) = parameter_value(input, "reasoning_effort") {
        body.insert(
            "reasoning".to_string(),
            json!({ "effort": reasoning_effort }),
        );
    }
    for key in PASSTHROUGH_RESPONSES_PARAMETERS {
        if let Some(value) = parameter_value(input, key) {
            body.insert((*key).to_string(), normalize_jsonish_parameter(value));
        }
    }
    Ok(Value::Object(body))
}

fn build_responses_input(input: &ProviderInvocationInput) -> Vec<Value> {
    let mut items = Vec::new();
    for message in &input.messages {
        if message.role == ProviderMessageRole::Tool {
            if let Some(call_id) = message.tool_call_id.as_deref() {
                items.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": message.content_blocks.as_ref().map(normalize_message_text).unwrap_or_else(|| message.content.clone()),
                }));
            }
            continue;
        }
        let content = responses_content_value(message);
        if !is_empty_content(&content) {
            items.push(json!({
                "role": responses_role(message.role),
                "content": content,
            }));
        }
        if let Some(calls) = message.tool_calls.as_ref().and_then(Value::as_array) {
            for (index, call) in calls.iter().enumerate() {
                if let Some(function_call) = response_function_call_from_native(call, index) {
                    items.push(function_call);
                }
            }
        }
    }
    items
}

fn responses_content_value(message: &ProviderMessage) -> Value {
    let Some(content) = message.content_blocks.as_ref() else {
        return Value::String(message.content.clone());
    };
    match content {
        Value::Array(parts) => Value::Array(
            parts
                .iter()
                .filter_map(|part| {
                    let object = part.as_object()?;
                    let part_type = object.get("type").and_then(Value::as_str).unwrap_or_default();
                    match part_type {
                        "text" | "input_text" => object
                            .get("text")
                            .or_else(|| object.get("content"))
                            .and_then(Value::as_str)
                            .map(|text| json!({ "type": "input_text", "text": text })),
                        "image_url" | "input_image" | "image" => object
                            .get("image_url")
                            .or_else(|| object.get("image"))
                            .or_else(|| object.get("url"))
                            .map(|image_url| {
                                json!({
                                    "type": "input_image",
                                    "image_url": image_url_value(image_url).get("url").cloned().unwrap_or_else(|| image_url.clone()),
                                })
                            }),
                        _ => Some(part.clone()),
                    }
                })
                .collect(),
        ),
        Value::String(text) => Value::String(text.clone()),
        Value::Null => Value::String(String::new()),
        other => Value::String(other.to_string()),
    }
}

fn response_function_call_from_native(tool_call: &Value, index: usize) -> Option<Value> {
    let object = tool_call.as_object()?;
    let function = object.get("function").and_then(Value::as_object);
    let name = function
        .and_then(|value| value.get("name"))
        .or_else(|| object.get("name"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())?;
    let arguments = function
        .and_then(|value| value.get("arguments"))
        .or_else(|| object.get("arguments"))
        .map(tool_arguments_string)
        .unwrap_or_else(|| "{}".to_string());
    let call_id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("call_{index}"));
    Some(json!({
        "type": "function_call",
        "call_id": call_id,
        "name": name,
        "arguments": arguments,
    }))
}

fn response_tool(tool: &Value) -> Value {
    if tool.get("type").and_then(Value::as_str) != Some("function") {
        return tool.clone();
    }
    let Some(function) = tool.get("function").and_then(Value::as_object) else {
        return tool.clone();
    };
    let mut mapped = Map::new();
    mapped.insert("type".to_string(), Value::String("function".to_string()));
    if let Some(name) = function.get("name") {
        mapped.insert("name".to_string(), name.clone());
    }
    if let Some(description) = function.get("description") {
        mapped.insert("description".to_string(), description.clone());
    }
    if let Some(parameters) = function.get("parameters") {
        mapped.insert("parameters".to_string(), parameters.clone());
    }
    Value::Object(mapped)
}

fn responses_role(role: ProviderMessageRole) -> &'static str {
    match role {
        ProviderMessageRole::System => "developer",
        ProviderMessageRole::Assistant => "assistant",
        ProviderMessageRole::User | ProviderMessageRole::Tool => "user",
    }
}

async fn invoke_anthropic_messages<F>(
    config: &ProviderConfig,
    input: &ProviderInvocationInput,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let body = build_anthropic_body(input)?;
    let response = build_http_client(config)?
        .post(build_url(
            config,
            BailianProtocol::AnthropicMessages,
            "/v1/messages",
        )?)
        .headers(build_headers(
            config,
            BailianProtocol::AnthropicMessages,
            true,
            true,
            input.client_protocol_envelope.as_ref(),
        )?)
        .json(&body)
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, config))?;
    read_anthropic_streaming_response(response, input.model.clone(), config, on_event).await
}

fn build_anthropic_body(input: &ProviderInvocationInput) -> Result<Value> {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(required_model(input)?));
    body.insert(
        "messages".to_string(),
        Value::Array(build_anthropic_messages(input)),
    );
    body.insert("stream".to_string(), Value::Bool(true));
    body.insert(
        "max_tokens".to_string(),
        json!(parameter_u64(input, "max_tokens").unwrap_or(DEFAULT_ANTHROPIC_MAX_TOKENS)),
    );
    if let Some(system) = input.system_text() {
        body.insert("system".to_string(), Value::String(system));
    }
    if !input.tools.is_empty() {
        body.insert(
            "tools".to_string(),
            Value::Array(input.tools.iter().map(anthropic_tool).collect()),
        );
    }
    for key in ["temperature", "top_p", "top_k"] {
        if let Some(value) = parameter_value(input, key) {
            body.insert(key.to_string(), value);
        }
    }
    if let Some(tool_choice) = parameter_value(input, "tool_choice") {
        body.insert(
            "tool_choice".to_string(),
            anthropic_tool_choice(tool_choice),
        );
    }
    Ok(Value::Object(body))
}

fn build_anthropic_messages(input: &ProviderInvocationInput) -> Vec<Value> {
    let mut messages = Vec::new();
    for message in &input.messages {
        if message.role == ProviderMessageRole::System {
            continue;
        }
        if message.role == ProviderMessageRole::Tool {
            if let Some(tool_use_id) = message.tool_call_id.as_deref() {
                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": message.content_blocks.as_ref().map(normalize_message_text).unwrap_or_else(|| message.content.clone()),
                    }]
                }));
            }
            continue;
        }
        let mut content = anthropic_content_blocks(message);
        append_anthropic_tool_use_blocks(&mut content, message.tool_calls.as_ref());
        if !content.is_empty() {
            messages.push(json!({
                "role": if message.role == ProviderMessageRole::Assistant { "assistant" } else { "user" },
                "content": content,
            }));
        }
    }
    messages
}

fn anthropic_content_blocks(message: &ProviderMessage) -> Vec<Value> {
    let Some(content) = message.content_blocks.as_ref() else {
        return if message.content.trim().is_empty() {
            Vec::new()
        } else {
            vec![json!({ "type": "text", "text": message.content })]
        };
    };
    match content {
        Value::Array(parts) => parts.iter().filter_map(anthropic_content_part).collect(),
        _ => {
            let text = normalize_message_text(content);
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![json!({ "type": "text", "text": text })]
            }
        }
    }
}

fn anthropic_content_part(part: &Value) -> Option<Value> {
    let object = part.as_object()?;
    let part_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match part_type {
        "text" | "input_text" => object
            .get("text")
            .or_else(|| object.get("content"))
            .and_then(Value::as_str)
            .map(|text| json!({ "type": "text", "text": text })),
        "image" | "image_url" | "input_image" => object
            .get("image_url")
            .or_else(|| object.get("image"))
            .or_else(|| object.get("url"))
            .map(|image| {
                json!({
                    "type": "image",
                    "source": {
                        "type": "url",
                        "url": image_url_value(image).get("url").cloned().unwrap_or_else(|| image.clone()),
                    }
                })
            }),
        _ => Some(part.clone()),
    }
}

fn append_anthropic_tool_use_blocks(content: &mut Vec<Value>, tool_calls: Option<&Value>) {
    let Some(calls) = tool_calls.and_then(Value::as_array) else {
        return;
    };
    for (index, call) in calls.iter().enumerate() {
        if let Some(block) = anthropic_tool_use_block(call, index) {
            content.push(block);
        }
    }
}

fn anthropic_tool_use_block(tool_call: &Value, index: usize) -> Option<Value> {
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
    Some(json!({ "type": "tool_use", "id": id, "name": name, "input": input }))
}

fn anthropic_tool(tool: &Value) -> Value {
    if tool.get("type").and_then(Value::as_str) != Some("function") {
        return tool.clone();
    }
    let Some(function) = tool.get("function").and_then(Value::as_object) else {
        return tool.clone();
    };
    json!({
        "name": function.get("name").cloned().unwrap_or(Value::Null),
        "description": function.get("description").cloned().unwrap_or(Value::Null),
        "input_schema": function.get("parameters").cloned().unwrap_or_else(|| json!({ "type": "object" })),
    })
}

fn anthropic_tool_choice(value: Value) -> Value {
    match value.as_str() {
        Some("required") | Some("any") => json!({ "type": "any" }),
        Some("none") => json!({ "type": "none" }),
        Some("auto") => json!({ "type": "auto" }),
        _ => value,
    }
}

async fn invoke_dashscope<F>(
    config: &ProviderConfig,
    input: &ProviderInvocationInput,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let multimodal = dashscope_model_uses_multimodal_endpoint(&input.model)
        || input.messages.iter().any(message_has_media);
    let pathname = if multimodal {
        "/services/aigc/multimodal-generation/generation"
    } else {
        "/services/aigc/text-generation/generation"
    };
    let body = build_dashscope_body(input)?;
    let response = build_http_client(config)?
        .post(build_url(config, BailianProtocol::DashScope, pathname)?)
        .headers(build_headers(
            config,
            BailianProtocol::DashScope,
            true,
            true,
            input.client_protocol_envelope.as_ref(),
        )?)
        .json(&body)
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, config))?;
    read_dashscope_streaming_response(response, input.model.clone(), config, on_event).await
}

fn build_dashscope_body(input: &ProviderInvocationInput) -> Result<Value> {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(required_model(input)?));
    body.insert(
        "input".to_string(),
        json!({ "messages": build_dashscope_messages(input) }),
    );
    let mut parameters = Map::new();
    parameters.insert("incremental_output".to_string(), Value::Bool(true));
    if let Some(response_format) = input
        .response_format
        .clone()
        .or_else(|| parameter_value(input, "response_format"))
        .map(normalize_response_format)
    {
        parameters.insert("response_format".to_string(), response_format);
    }
    for key in PASSTHROUGH_DASHSCOPE_PARAMETERS {
        if let Some(value) = parameter_value(input, key) {
            parameters.insert((*key).to_string(), normalize_jsonish_parameter(value));
        }
    }
    body.insert("parameters".to_string(), Value::Object(parameters));
    Ok(Value::Object(body))
}

fn build_dashscope_messages(input: &ProviderInvocationInput) -> Vec<Value> {
    let mut messages = Vec::new();
    if let Some(system) = input.system_text() {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in &input.messages {
        messages.push(json!({
            "role": message_role_name(message.role),
            "content": dashscope_content_value(message),
        }));
    }
    messages
}

fn dashscope_content_value(message: &ProviderMessage) -> Value {
    let Some(content) = message.content_blocks.as_ref() else {
        return Value::String(message.content.clone());
    };
    match content {
        Value::Array(parts) => {
            Value::Array(parts.iter().filter_map(dashscope_content_part).collect())
        }
        Value::String(_) => content.clone(),
        Value::Null => Value::String(String::new()),
        other => Value::String(other.to_string()),
    }
}

fn dashscope_content_part(part: &Value) -> Option<Value> {
    let object = part.as_object()?;
    let part_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match part_type {
        "text" | "input_text" => object
            .get("text")
            .or_else(|| object.get("content"))
            .and_then(Value::as_str)
            .map(|text| json!({ "text": text })),
        "image" | "image_url" | "input_image" => object
            .get("image_url")
            .or_else(|| object.get("image"))
            .or_else(|| object.get("url"))
            .map(|image| json!({ "image": image_url_value(image).get("url").cloned().unwrap_or_else(|| image.clone()) })),
        "video" | "video_url" | "input_video" => object
            .get("video")
            .or_else(|| object.get("video_url"))
            .or_else(|| object.get("url"))
            .map(|video| json!({ "video": video.clone() })),
        _ => Some(part.clone()),
    }
}

fn message_has_media(message: &ProviderMessage) -> bool {
    message.content_blocks.as_ref().is_some_and(|content| {
        content.as_array().is_some_and(|parts| {
            parts.iter().any(|part| {
                let part_type = part.get("type").and_then(Value::as_str).unwrap_or_default();
                matches!(
                    part_type,
                    "image" | "image_url" | "input_image" | "video" | "video_url" | "input_video"
                ) || part.get("image").is_some()
                    || part.get("image_url").is_some()
                    || part.get("video").is_some()
                    || part.get("video_url").is_some()
            })
        })
    })
}

fn dashscope_model_uses_multimodal_endpoint(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.contains("qwen3.6")
        || model.contains("-vl")
        || model.contains("qwen-vl")
        || model.contains("omni")
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

fn required_model(input: &ProviderInvocationInput) -> Result<String> {
    let model = input.model.trim().to_string();
    if model.is_empty() {
        bail!("model is required");
    }
    Ok(model)
}

fn normalize_message_text(content: &Value) -> String {
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

fn is_empty_content(content: &Value) -> bool {
    match content {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

fn parameter_value(input: &ProviderInvocationInput, key: &str) -> Option<Value> {
    input
        .model_parameters
        .get(key)
        .cloned()
        .and_then(normalize_scalar_parameter)
}

fn message_role_name(role: ProviderMessageRole) -> &'static str {
    match role {
        ProviderMessageRole::System => "system",
        ProviderMessageRole::User => "user",
        ProviderMessageRole::Assistant => "assistant",
        ProviderMessageRole::Tool => "tool",
    }
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

fn normalize_jsonish_parameter(value: Value) -> Value {
    match value {
        Value::String(text) => serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text)),
        other => other,
    }
}

fn normalize_response_format(value: Value) -> Value {
    match normalize_jsonish_parameter(value) {
        Value::Object(object) if object.contains_key("type") => Value::Object(object),
        Value::String(text) => json!({ "type": text }),
        other => other,
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
        supports_multimodal: true,
        context_window: None,
        max_output_tokens: None,
        provider_metadata: json!({
            "owned_by": entry.get("owned_by").cloned().unwrap_or(Value::Null),
            "created": entry.get("created").cloned().unwrap_or(Value::Null),
        }),
    })
}

fn static_models() -> Vec<ProviderModelDescriptor> {
    [
        ("qwen3.6-plus-2026-04-02", "Qwen 3.6 Plus 2026-04-02", true),
        ("qwen3.6-plus", "Qwen 3.6 Plus", true),
        ("qwen-plus", "Qwen Plus", false),
        ("qwen3-vl-plus", "Qwen3 VL Plus", true),
    ]
    .into_iter()
    .map(
        |(model_id, display_name, multimodal)| ProviderModelDescriptor {
            model_id: model_id.to_string(),
            display_name: display_name.to_string(),
            source: "static".to_string(),
            supports_streaming: true,
            supports_tool_call: true,
            supports_multimodal: multimodal,
            context_window: None,
            max_output_tokens: None,
            provider_metadata: json!({ "owned_by": "aliyun_bailian" }),
        },
    )
    .collect()
}

#[cfg(test)]
mod tests;
