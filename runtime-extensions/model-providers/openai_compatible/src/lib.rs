use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

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
const OPENAI_CHAT_PROTOCOL: &str = "openai_chat";
const PASSTHROUGH_CHAT_COMPLETION_PARAMETERS: &[&str] = &[
    "temperature",
    "top_p",
    "n",
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_details: Option<Value>,
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
                provider_details: None,
            }),
        }
    }

    pub fn runtime_error(error: ProviderRuntimeError) -> Self {
        Self {
            ok: false,
            result: Value::Null,
            error: Some(ProviderStdioError {
                kind: provider_runtime_error_kind(&error.kind).to_string(),
                message: error.message,
                provider_summary: error.provider_summary,
                provider_details: error.provider_details,
            }),
        }
    }
}

fn provider_runtime_error_kind(kind: &ProviderRuntimeErrorKind) -> &'static str {
    match kind {
        ProviderRuntimeErrorKind::AuthFailed => "auth_failed",
        ProviderRuntimeErrorKind::EndpointUnreachable => "endpoint_unreachable",
        ProviderRuntimeErrorKind::ModelNotFound => "model_not_found",
        ProviderRuntimeErrorKind::RateLimited => "rate_limited",
        ProviderRuntimeErrorKind::ProviderUpstreamError => "provider_upstream_error",
        ProviderRuntimeErrorKind::ProviderInvalidResponse => "provider_invalid_response",
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
    ProtocolContext,
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
    pub client_protocol_envelope: Option<ProtocolContextEnvelope>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProtocolContextEnvelope {
    pub source_protocol: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub query: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub body: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum NativeReasoningMode {
    Adaptive,
    #[default]
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NativeReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl NativeReasoningEffort {
    fn as_chat_value(self) -> Result<&'static str> {
        match self {
            Self::Minimal => Ok("minimal"),
            Self::Low => Ok("low"),
            Self::Medium => Ok("medium"),
            Self::High => Ok("high"),
            Self::Xhigh => Ok("xhigh"),
            Self::Max => bail!("reasoning.effort=max is not supported by the Chat-compatible wire"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeReasoningParameters {
    #[serde(default)]
    mode: NativeReasoningMode,
    #[serde(default)]
    effort: Option<NativeReasoningEffort>,
    #[serde(default)]
    budget_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct TypedModelParameters {
    max_output_tokens: Option<u64>,
    requested_context_window: Option<u64>,
    reasoning: Option<NativeReasoningParameters>,
}

impl TypedModelParameters {
    fn from_input(input: &ProviderInvocationInput) -> Result<Self> {
        let reasoning = input
            .model_parameters
            .get("reasoning")
            .map(|value| {
                serde_json::from_value::<NativeReasoningParameters>(value.clone())
                    .context("reasoning must contain only typed mode, effort, and budget_tokens")
            })
            .transpose()?;
        if reasoning
            .as_ref()
            .and_then(|reasoning| reasoning.budget_tokens)
            == Some(0)
        {
            bail!("reasoning.budget_tokens must be a positive integer");
        }
        Ok(Self {
            max_output_tokens: positive_model_parameter(input, "max_output_tokens")?,
            requested_context_window: positive_model_parameter(input, "requested_context_window")?,
            reasoning,
        })
    }
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
    Error,
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
    Error { error: ProviderRuntimeError },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuntimeInvocationEnvelope {
    pub events: Vec<ProviderStreamEvent>,
    pub result: ProviderInvocationResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRuntimeErrorKind {
    AuthFailed,
    EndpointUnreachable,
    ModelNotFound,
    RateLimited,
    ProviderUpstreamError,
    ProviderInvalidResponse,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProviderRuntimeError {
    pub kind: ProviderRuntimeErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_details: Option<Value>,
}

impl ProviderRuntimeError {
    pub fn normalize<M>(code: &str, message: M, provider_summary: Option<&str>) -> Self
    where
        M: Into<String>,
    {
        let message = message.into();
        let haystack = format!("{code} {message}").to_ascii_lowercase();
        let kind = if haystack.contains("auth")
            || haystack.contains("api_key")
            || haystack.contains("unauthorized")
            || haystack.contains("forbidden")
            || haystack.contains("401")
        {
            ProviderRuntimeErrorKind::AuthFailed
        } else if haystack.contains("rate")
            || haystack.contains("quota")
            || haystack.contains("too_many")
            || haystack.contains("429")
        {
            ProviderRuntimeErrorKind::RateLimited
        } else if (haystack.contains("model") && haystack.contains("not found"))
            || haystack.contains("unknown_model")
            || haystack.contains("model_not_found")
        {
            ProviderRuntimeErrorKind::ModelNotFound
        } else if haystack.contains("timeout")
            || haystack.contains("connect")
            || haystack.contains("unreachable")
            || haystack.contains("refused")
            || haystack.contains("dns")
            || haystack.contains("503")
        {
            ProviderRuntimeErrorKind::EndpointUnreachable
        } else {
            ProviderRuntimeErrorKind::ProviderInvalidResponse
        };

        Self {
            kind,
            message,
            provider_summary: provider_summary.map(ToOwned::to_owned),
            provider_details: None,
        }
    }
}

impl fmt::Display for ProviderRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.provider_summary {
            Some(summary) => write!(formatter, "{:?}: {} ({summary})", self.kind, self.message),
            None => write!(formatter, "{:?}: {}", self.kind, self.message),
        }
    }
}

impl std::error::Error for ProviderRuntimeError {}

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
    client_protocol_envelope: Option<&ProtocolContextEnvelope>,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    if include_json_body {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    }
    apply_client_protocol_headers(&mut headers, client_protocol_envelope)?;

    // Provider-instance configuration is authoritative and is injected only
    // after every client-protocol residual has been validated and restored.
    for (key, value) in &config.default_headers {
        let header_name = HeaderName::from_bytes(key.as_bytes())
            .with_context(|| format!("invalid default header name: {key}"))?;
        let header_value = HeaderValue::from_str(value)
            .with_context(|| format!("invalid default header value for {key}"))?;
        headers.insert(header_name, header_value);
    }
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
    let authorization_header = config
        .authorization_header
        .clone()
        .unwrap_or_else(|| format!("Bearer {}", config.api_key));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&authorization_header).context("invalid authorization header")?,
    );
    Ok(headers)
}

fn apply_client_protocol_headers(
    headers: &mut HeaderMap,
    envelope: Option<&ProtocolContextEnvelope>,
) -> Result<()> {
    let Some(envelope) = matching_protocol_context(envelope)? else {
        return Ok(());
    };
    for (raw_name, values) in &envelope.headers {
        let normalized_name = raw_name.trim().to_ascii_lowercase();
        if !protocol_context_header_is_safe(&normalized_name) {
            bail!("protocol context contains a reserved or typed header");
        }
        if values.is_empty() {
            bail!("protocol context header values must not be empty");
        }
        let name = HeaderName::from_bytes(normalized_name.as_bytes())
            .context("protocol context contains an invalid header name")?;
        for value in values {
            if value.trim().is_empty() {
                bail!("protocol context header values must not be empty");
            }
            headers.append(
                name.clone(),
                HeaderValue::from_str(value)
                    .context("protocol context contains an invalid header value")?,
            );
        }
    }
    Ok(())
}

fn matching_protocol_context(
    envelope: Option<&ProtocolContextEnvelope>,
) -> Result<Option<&ProtocolContextEnvelope>> {
    let Some(envelope) = envelope else {
        return Ok(None);
    };
    if envelope.source_protocol != OPENAI_CHAT_PROTOCOL {
        bail!("unconsumed foreign protocol context");
    }
    Ok(Some(envelope))
}

fn protocol_context_header_is_safe(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase().replace('_', "-");
    protocol_context_field_is_safe(&normalized)
        && !matches!(
            normalized.as_str(),
            "content-type" | "accept" | "accept-encoding" | "accept-language" | "origin"
        )
}

fn protocol_context_field_is_safe(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() || lower.starts_with("__") {
        return false;
    }
    let normalized = lower.replace('_', "-");
    !matches!(
        normalized.as_str(),
        "auth"
            | "authentication"
            | "authentication-info"
            | "authorization"
            | "x-authorization"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "proxy-authentication-info"
            | "www-authenticate"
            | "x-api-key"
            | "api-key"
            | "x-auth-token"
            | "auth-token"
            | "bearer-token"
            | "x-access-token"
            | "access-token"
            | "refresh-token"
            | "id-token"
            | "client-secret"
            | "api-secret"
            | "password"
            | "passwd"
            | "x-csrf-token"
            | "x-xsrf-token"
            | "csrf-token"
            | "cookie"
            | "set-cookie"
            | "host"
            | "connection"
            | "proxy-connection"
            | "keep-alive"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
            | "client-protocol-envelope"
            | "native-model-prompt-context"
            | "native-model-request-context"
            | "native-transport"
            | "provider-transport"
            | "request-context"
            | "run-context"
            | "trace-context"
            | "compatibility-mode"
            | "sys"
            | "env"
            | "trigger"
            | "forwarded"
            | "via"
            | "x-real-ip"
            | "true-client-ip"
            | "cf-connecting-ip"
            | "cf-ray"
            | "traceparent"
            | "tracestate"
            | "baggage"
            | "x-request-id"
            | "internal"
            | "x-internal"
            | "1flowbase"
            | "x-1flowbase"
    ) && !normalized.starts_with("x-1flowbase-")
        && !normalized.starts_with("x-internal-")
        && !normalized.starts_with("internal-")
        && !normalized.starts_with("x-forwarded-")
        && !normalized.starts_with("x-envoy-")
        && !normalized.starts_with("x-amzn-")
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

fn build_url_with_protocol_context(
    config: &ProviderConfig,
    pathname: &str,
    envelope: Option<&ProtocolContextEnvelope>,
) -> Result<String> {
    let mut url = Url::parse(&build_url(config, pathname)?)
        .context("building OpenAI-compatible protocol-context URL")?;
    let Some(envelope) = matching_protocol_context(envelope)? else {
        return Ok(url.to_string());
    };
    for (name, values) in &envelope.query {
        if !protocol_context_field_is_safe(name) {
            bail!("protocol context contains a reserved query field");
        }
        if config.api_version.is_some()
            && (name.trim().eq_ignore_ascii_case("api-version")
                || name.trim().eq_ignore_ascii_case("api_version"))
        {
            bail!("protocol context collides with the typed api-version query field");
        }
        if values.is_empty() {
            bail!("protocol context query values must not be empty");
        }
        for value in values {
            url.query_pairs_mut().append_pair(name, value);
        }
    }
    Ok(url.to_string())
}

fn restore_protocol_context_body(
    mut typed_body: Value,
    envelope: Option<&ProtocolContextEnvelope>,
) -> Result<Value> {
    let Some(envelope) = matching_protocol_context(envelope)? else {
        return Ok(typed_body);
    };
    let body = typed_body
        .as_object_mut()
        .context("typed OpenAI-compatible request body must be an object")?;
    for (name, value) in &envelope.body {
        if !protocol_context_field_is_safe(name) {
            bail!("protocol context contains a reserved body field");
        }
        if typed_chat_body_field(name) || body.contains_key(name) {
            bail!("protocol context collides with a typed Chat-compatible body field");
        }
        validate_protocol_context_value(value)?;
        body.insert(name.clone(), value.clone());
    }
    Ok(typed_body)
}

fn typed_chat_body_field(name: &str) -> bool {
    // This is the Host's typed OpenAI Chat root set. Provider-specific Chat
    // fields outside it remain residual unless the typed body actually owns them.
    matches!(
        name,
        "model"
            | "messages"
            | "stream"
            | "user"
            | "metadata"
            | "max_completion_tokens"
            | "max_tokens"
            | "audio"
            | "modalities"
            | "tools"
            | "tool_choice"
            | "function_call"
            | "parallel_tool_calls"
            | "response_format"
            | "reasoning_effort"
            | "temperature"
            | "top_p"
            | "presence_penalty"
            | "frequency_penalty"
            | "seed"
            | "stop"
            | "stream_options"
    )
}

fn validate_protocol_context_value(value: &Value) -> Result<()> {
    match value {
        Value::Array(values) => {
            for value in values {
                validate_protocol_context_value(value)?;
            }
        }
        Value::Object(object) => {
            for (name, value) in object {
                if !protocol_context_field_is_safe(name) {
                    bail!("protocol context contains a nested reserved body field");
                }
                validate_protocol_context_value(value)?;
            }
        }
        _ => {}
    }
    Ok(())
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
    if !status.is_success() {
        return Err(provider_upstream_error_from_response(response)
            .await?
            .into());
    }
    let payload = read_json_response(response).await?;

    Ok(payload)
}

async fn send_provider_request(
    config: &ProviderConfig,
    pathname: &str,
    method: Method,
    body: Option<Value>,
    client_protocol_envelope: Option<&ProtocolContextEnvelope>,
) -> Result<reqwest::Response> {
    let include_json_body = body.is_some();
    if !include_json_body
        && matching_protocol_context(client_protocol_envelope)?
            .is_some_and(|envelope| !envelope.body.is_empty())
    {
        bail!("unconsumed protocol context body");
    }
    let body = body
        .map(|typed_body| restore_protocol_context_body(typed_body, client_protocol_envelope))
        .transpose()?;
    let url = build_url_with_protocol_context(config, pathname, client_protocol_envelope)?;
    let headers = build_headers(config, include_json_body, client_protocol_envelope)?;
    let client = build_http_client(config)?;
    let mut request = client.request(method.clone(), url).headers(headers);
    if let Some(body) = body {
        request = request.json(&body);
    }

    request
        .send()
        .await
        .map_err(|error| sanitize_error(error, config))
}

async fn read_json_response(response: reqwest::Response) -> Result<Value> {
    let text = response.text().await?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).with_context(|| "provider returned invalid JSON")
}

async fn provider_upstream_error_from_response(
    response: reqwest::Response,
) -> Result<ProviderRuntimeError> {
    let status = response.status();
    let headers = response.headers().clone();
    let raw_body = response.text().await?;
    Ok(provider_upstream_error_from_parts(
        status, &headers, raw_body,
    ))
}

fn provider_upstream_error_from_parts(
    status: reqwest::StatusCode,
    headers: &HeaderMap,
    raw_body: String,
) -> ProviderRuntimeError {
    let message = if raw_body.is_empty() {
        format!("HTTP {status}")
    } else {
        raw_body
    };
    let mut provider_details = Map::new();
    provider_details.insert("status".to_string(), json!(status.as_u16()));
    if let Some(request_id) = response_request_id(headers) {
        provider_details.insert("request_id".to_string(), json!(request_id));
    }

    ProviderRuntimeError {
        kind: ProviderRuntimeErrorKind::ProviderUpstreamError,
        message: message.clone(),
        provider_summary: Some(message),
        provider_details: Some(Value::Object(provider_details)),
    }
}

fn response_request_id(headers: &HeaderMap) -> Option<String> {
    ["x-request-id", "request-id", "openai-request-id", "cf-ray"]
        .iter()
        .find_map(|name| {
            headers
                .get(*name)
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.chars().take(128).collect())
        })
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
            item.insert("tool_calls".to_string(), openai_chat_tool_calls(tool_calls));
        }
        messages.push(Value::Object(item));
    }
    messages
}

fn openai_chat_tool_calls(tool_calls: &Value) -> Value {
    let Some(tool_calls) = tool_calls.as_array() else {
        return tool_calls.clone();
    };
    Value::Array(
        tool_calls
            .iter()
            .enumerate()
            .map(|(index, call)| {
                if call.get("function").is_some() {
                    return call.clone();
                }
                let id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("tool_call_{}", index + 1));
                let name = call
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown_tool");
                let arguments = call
                    .get("arguments")
                    .map(response_tool_arguments)
                    .unwrap_or_else(|| "{}".to_string());
                json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    }
                })
            })
            .collect(),
    )
}

fn response_tool_arguments(arguments: &Value) -> String {
    match arguments {
        Value::String(arguments) => arguments.clone(),
        arguments => serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string()),
    }
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

fn positive_model_parameter(input: &ProviderInvocationInput, key: &str) -> Result<Option<u64>> {
    let Some(value) = input.model_parameters.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_u64()
        .filter(|value| *value > 0)
        .with_context(|| format!("{key} must be a positive integer"))?;
    Ok(Some(value))
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
    let body = build_typed_chat_completion_body(&input)?;

    let response = send_provider_request(
        &config,
        "/chat/completions",
        Method::POST,
        Some(body),
        input.client_protocol_envelope.as_ref(),
    )
    .await?;
    read_streaming_chat_completion(response, input.model, &mut on_event).await
}

fn build_typed_chat_completion_body(input: &ProviderInvocationInput) -> Result<Value> {
    if input
        .required_capabilities
        .iter()
        .any(|capability| *capability != ProviderInvocationCapability::ProtocolContext)
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
    let typed_parameters = TypedModelParameters::from_input(input)?;
    if typed_parameters.requested_context_window.is_some() {
        bail!("requested_context_window is not supported by the Chat-compatible wire");
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
    if let Some(max_output_tokens) = typed_parameters.max_output_tokens {
        body.insert("max_tokens".to_string(), json!(max_output_tokens));
    }
    if let Some(reasoning) = typed_parameters.reasoning.as_ref() {
        if input.model_parameters.contains_key("reasoning_effort") {
            bail!("reasoning collides with legacy reasoning_effort");
        }
        body.insert(
            "reasoning_effort".to_string(),
            Value::String(chat_reasoning_effort(reasoning)?.to_string()),
        );
    }
    for key in PASSTHROUGH_CHAT_COMPLETION_PARAMETERS {
        if *key == "reasoning_effort" && typed_parameters.reasoning.is_some() {
            continue;
        }
        if let Some(value) = parameter_value(&input, key) {
            body.insert((*key).to_string(), value);
        }
    }

    Ok(Value::Object(body))
}

fn chat_reasoning_effort(reasoning: &NativeReasoningParameters) -> Result<&'static str> {
    if reasoning.budget_tokens.is_some() {
        bail!("reasoning.budget_tokens is not supported by the Chat-compatible wire");
    }
    match reasoning.mode {
        NativeReasoningMode::Adaptive => {
            bail!("reasoning.mode=adaptive is not supported by the Chat-compatible wire")
        }
        NativeReasoningMode::Disabled => {
            if reasoning.effort.is_some() {
                bail!("disabled reasoning must not declare reasoning.effort");
            }
            Ok("none")
        }
        NativeReasoningMode::Enabled => reasoning
            .effort
            .context("enabled reasoning requires effort on the Chat-compatible wire")?
            .as_chat_value(),
    }
}

async fn read_streaming_chat_completion<F>(
    response: reqwest::Response,
    request_model: String,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let status = response.status();
    if !status.is_success() {
        return Err(provider_upstream_error_from_response(response)
            .await?
            .into());
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
    fn wp_d2d_protocol_context_mirrors_the_frozen_host_abi() {
        let envelope: ProtocolContextEnvelope = serde_json::from_value(json!({
            "source_protocol": "openai_chat",
            "query": {"preview": ["one", "two"]},
            "headers": {
                "openai-organization": ["org-client"],
                "x-client-name": ["ChatClient", "ChatClient/2"]
            },
            "body": {"future_chat_option": {"shape": "opaque"}}
        }))
        .expect("the provider must deserialize the current Host envelope without projection");

        assert_eq!(
            envelope.query["preview"],
            vec!["one".to_string(), "two".to_string()]
        );
        assert_eq!(
            envelope.headers["x-client-name"],
            vec!["ChatClient".to_string(), "ChatClient/2".to_string()]
        );
        assert_eq!(
            serde_json::to_value(&envelope).unwrap(),
            json!({
                "source_protocol": "openai_chat",
                "query": {"preview": ["one", "two"]},
                "headers": {
                    "openai-organization": ["org-client"],
                    "x-client-name": ["ChatClient", "ChatClient/2"]
                },
                "body": {"future_chat_option": {"shape": "opaque"}}
            })
        );
        serde_json::from_value::<ProtocolContextEnvelope>(json!({
            "source_protocol": "openai_chat",
            "policy": "default_deny"
        }))
        .expect_err("legacy envelope policy must not create a second ABI shape");
    }

    #[test]
    fn wp_d2d_typed_chat_request_preserves_only_supported_native_intent() {
        let enabled: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "openai_compatible",
            "protocol": "openai_compatible",
            "model": "gpt-compatible",
            "required_capabilities": ["protocol_context"],
            "model_parameters": {
                "max_output_tokens": 512,
                "reasoning": {"mode": "enabled", "effort": "high"}
            }
        }))
        .unwrap();
        let enabled_body = build_typed_chat_completion_body(&enabled).unwrap();
        assert_eq!(enabled_body["max_tokens"], 512);
        assert_eq!(enabled_body["reasoning_effort"], "high");
        assert!(enabled_body.get("reasoning").is_none());

        let disabled: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "openai_compatible",
            "protocol": "openai_compatible",
            "model": "gpt-compatible",
            "model_parameters": {
                "reasoning": {"mode": "disabled"}
            }
        }))
        .unwrap();
        assert_eq!(
            build_typed_chat_completion_body(&disabled).unwrap()["reasoning_effort"],
            "none"
        );

        for (model_parameters, reason) in [
            (
                json!({"requested_context_window": 128000}),
                "a context-window request has no Chat-compatible wire field",
            ),
            (
                json!({"reasoning": {"mode": "adaptive", "effort": "high"}}),
                "adaptive reasoning has no Chat-compatible representation",
            ),
            (
                json!({"reasoning": {"mode": "enabled", "effort": "high", "budget_tokens": 1024}}),
                "Chat-compatible reasoning has no token-budget field",
            ),
            (
                json!({"reasoning": {"mode": "enabled", "effort": "max"}}),
                "Chat-compatible reasoning does not support max effort",
            ),
            (
                json!({"reasoning": {"mode": "enabled"}}),
                "enabled reasoning without effort cannot be represented exactly",
            ),
            (
                json!({
                    "reasoning": {"mode": "enabled", "effort": "high"},
                    "reasoning_effort": "low"
                }),
                "typed reasoning and legacy reasoning effort must not compete",
            ),
        ] {
            let input: ProviderInvocationInput = serde_json::from_value(json!({
                "contract_version": "1flowbase.provider/v2",
                "provider_instance_id": "provider-test",
                "provider_code": "openai_compatible",
                "protocol": "openai_compatible",
                "model": "gpt-compatible",
                "model_parameters": model_parameters
            }))
            .unwrap();
            build_typed_chat_completion_body(&input).expect_err(reason);
        }
    }

    #[test]
    fn wp_d2d_restores_safe_chat_residuals_before_configured_header_authority() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "openai_compatible",
            "protocol": "openai_compatible",
            "model": "gpt-compatible",
            "required_capabilities": ["protocol_context"],
            "model_parameters": {
                "max_output_tokens": 512,
                "reasoning": {"mode": "enabled", "effort": "high"}
            },
            "client_protocol_envelope": {
                "source_protocol": "openai_chat",
                "query": {"preview": ["one", "two"]},
                "headers": {
                    "x-client-name": ["ChatClient", "ChatClient/2"],
                    "x-shared": ["client-value"]
                },
                "body": {
                    "future_chat_option": {"shape": "opaque"},
                    "logit_bias": {"50256": -100},
                    "n": 2,
                    "service_tier": "priority"
                }
            }
        }))
        .unwrap();
        let config = normalize_provider_config(&json!({
            "base_url": "https://compatible.example/v1",
            "api_key": "provider-secret",
            "authorization_header": "Configured provider auth",
            "organization": "structured-org",
            "project": "structured-project",
            "api_version": "2026-07-28",
            "default_headers": {
                "authorization": "default auth must lose",
                "openai-organization": "default org must lose",
                "x-provider-default": "kept",
                "x-shared": "configured-value"
            }
        }))
        .unwrap();
        let typed_body = build_typed_chat_completion_body(&input).unwrap();
        assert!(typed_body.get("future_chat_option").is_none());
        let body =
            restore_protocol_context_body(typed_body, input.client_protocol_envelope.as_ref())
                .unwrap();
        let url = build_url_with_protocol_context(
            &config,
            "/chat/completions",
            input.client_protocol_envelope.as_ref(),
        )
        .unwrap();
        let headers =
            build_headers(&config, true, input.client_protocol_envelope.as_ref()).unwrap();

        assert_eq!(body["model"], "gpt-compatible");
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["reasoning_effort"], "high");
        assert_eq!(body["future_chat_option"]["shape"], "opaque");
        assert_eq!(body["logit_bias"], json!({"50256": -100}));
        assert_eq!(body["n"], 2);
        assert_eq!(body["service_tier"], "priority");
        assert_eq!(
            Url::parse(&url)
                .unwrap()
                .query_pairs()
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect::<Vec<_>>(),
            vec![
                ("api-version".to_string(), "2026-07-28".to_string()),
                ("preview".to_string(), "one".to_string()),
                ("preview".to_string(), "two".to_string())
            ]
        );
        let mut no_api_version_config = config.clone();
        no_api_version_config.api_version = None;
        let residual_api_version = ProtocolContextEnvelope {
            source_protocol: OPENAI_CHAT_PROTOCOL.to_string(),
            query: BTreeMap::from([(
                "api-version".to_string(),
                vec!["client-version".to_string()],
            )]),
            ..ProtocolContextEnvelope::default()
        };
        assert_eq!(
            Url::parse(
                &build_url_with_protocol_context(
                    &no_api_version_config,
                    "/chat/completions",
                    Some(&residual_api_version)
                )
                .unwrap()
            )
            .unwrap()
            .query_pairs()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect::<Vec<_>>(),
            vec![("api-version".to_string(), "client-version".to_string())]
        );
        assert_eq!(
            headers.get(AUTHORIZATION).unwrap(),
            "Configured provider auth"
        );
        assert_eq!(headers.get("x-provider-default").unwrap(), "kept");
        assert_eq!(headers.get("x-shared").unwrap(), "configured-value");
        assert_eq!(
            headers.get("openai-organization").unwrap(),
            "structured-org"
        );
        assert_eq!(headers.get("openai-project").unwrap(), "structured-project");
        assert_eq!(
            headers
                .get_all("x-client-name")
                .iter()
                .map(|value| value.to_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["ChatClient", "ChatClient/2"]
        );

        let mut bearer_config = config.clone();
        bearer_config.authorization_header = None;
        assert_eq!(
            build_headers(
                &bearer_config,
                true,
                input.client_protocol_envelope.as_ref()
            )
            .unwrap()
            .get(AUTHORIZATION)
            .unwrap(),
            "Bearer provider-secret"
        );
    }

    #[test]
    fn wp_d2d_rejects_foreign_reserved_colliding_or_unconsumed_context() {
        let config = normalize_provider_config(&json!({
            "base_url": "https://compatible.example/v1",
            "api_key": "provider-secret",
            "api_version": "2026-07-28"
        }))
        .unwrap();
        let typed_body = json!({
            "model": "gpt-compatible",
            "messages": [],
            "stream": true,
            "stream_options": {"include_usage": true}
        });

        let foreign: ProtocolContextEnvelope = serde_json::from_value(json!({
            "source_protocol": "openai_responses",
            "body": {"future_option": true}
        }))
        .unwrap();
        restore_protocol_context_body(typed_body.clone(), Some(&foreign))
            .expect_err("foreign context must never be silently discarded");

        let reserved_query: ProtocolContextEnvelope = serde_json::from_value(json!({
            "source_protocol": "openai_chat",
            "query": {"authorization": ["query-secret"]}
        }))
        .unwrap();
        build_url_with_protocol_context(&config, "/chat/completions", Some(&reserved_query))
            .expect_err("reserved query auth must be rejected");

        let typed_query: ProtocolContextEnvelope = serde_json::from_value(json!({
            "source_protocol": "openai_chat",
            "query": {"api-version": ["client-version"]}
        }))
        .unwrap();
        build_url_with_protocol_context(&config, "/chat/completions", Some(&typed_query))
            .expect_err("the residual must not collide with typed query configuration");

        for header in ["authorization", "connection", "content-type"] {
            let reserved_header = ProtocolContextEnvelope {
                source_protocol: OPENAI_CHAT_PROTOCOL.to_string(),
                headers: BTreeMap::from([(header.to_string(), vec!["must-not-cross".to_string()])]),
                ..ProtocolContextEnvelope::default()
            };
            build_headers(&config, true, Some(&reserved_header))
                .expect_err("reserved, hop-by-hop, or typed headers must be rejected");
        }

        for field in ["model", "reasoning_effort"] {
            let typed_collision = ProtocolContextEnvelope {
                source_protocol: OPENAI_CHAT_PROTOCOL.to_string(),
                body: BTreeMap::from([(field.to_string(), json!("context-must-not-win"))]),
                ..ProtocolContextEnvelope::default()
            };
            restore_protocol_context_body(typed_body.clone(), Some(&typed_collision))
                .expect_err("typed Chat body collisions must be rejected");
        }

        let nested_auth: ProtocolContextEnvelope = serde_json::from_value(json!({
            "source_protocol": "openai_chat",
            "body": {"future_option": {"authorization": "nested-secret"}}
        }))
        .unwrap();
        restore_protocol_context_body(typed_body, Some(&nested_auth))
            .expect_err("nested reserved auth must be rejected");
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
    async fn ac_002_native_max_output_tokens_maps_to_openai_compatible_wire_field() {
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
                        "name": "lookup",
                        "arguments": { "query": "refund" }
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
                ("max_output_tokens".to_string(), json!(512)),
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
        assert_eq!(
            captured_body["messages"][0]["tool_calls"][0]["function"]["arguments"],
            r#"{"query":"refund"}"#
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
    fn ac_002_package_manifest_declares_current_generate_contract_and_protocol_context() {
        let manifest = include_str!("../manifest.yaml");

        assert!(manifest.contains("contract_version: 1flowbase.provider/v2"));
        assert!(!manifest.contains("1flowbase.provider/v1"));
        assert!(manifest.contains("capabilities:\n    - protocol_context"));
        assert_eq!(manifest.matches("protocol_context").count(), 1);
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

        let error = build_typed_chat_completion_body(&input)
            .expect_err("undeclared semantic capabilities must not be projected away");
        assert!(error.to_string().contains("semantic capabilities"));
    }

    #[test]
    fn ac_008_upstream_error_body_is_the_typed_runtime_message_without_parsing() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("openai-request-id"),
            HeaderValue::from_static("req_chat_fidelity"),
        );
        for raw_body in [
            " \n{\"future_error\":{\"shape\":\"unknown\"},\"message\":\"keep complete body\"}\n ",
            "plain upstream text\nwith trailing newline \n",
            "<html><body>future provider failure</body></html>",
            " \r\n\t ",
        ] {
            let error = provider_upstream_error_from_parts(
                reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                &headers,
                raw_body.to_string(),
            );

            assert_eq!(error.kind, ProviderRuntimeErrorKind::ProviderUpstreamError);
            assert_eq!(error.message, raw_body);
            assert_eq!(error.provider_summary.as_deref(), Some(raw_body));
            assert_eq!(
                error.provider_details,
                Some(json!({ "status": 500, "request_id": "req_chat_fidelity" }))
            );
        }

        let empty = provider_upstream_error_from_parts(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            &headers,
            String::new(),
        );
        assert_eq!(empty.message, "HTTP 503 Service Unavailable");
    }
}
