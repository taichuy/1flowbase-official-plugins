use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt,
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    Method, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    client_async_tls_with_config, connect_async,
    tungstenite::{
        client::IntoClientRequest,
        handshake::client::{
            Request as ClientHandshakeRequest, Response as ClientHandshakeResponse,
        },
        Error as WebSocketError, Message,
    },
    MaybeTlsStream, WebSocketStream,
};

const PROVIDER_CODE: &str = "openai";
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_VALIDATE_MODEL: bool = true;
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_millis(300_000);
const WEBSOCKET_CURSOR_RECONNECT_ATTEMPTS: usize = 3;
const RESPONSES_WEBSOCKETS_BETA: &str = "responses_websockets=2026-02-06";
const X_CODEX_TURN_STATE_HEADER: &str = "x-codex-turn-state";
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
const PASSTHROUGH_RESPONSE_PARAMETERS: &[&str] = &[
    "temperature",
    "top_p",
    "max_output_tokens",
    "tool_choice",
    "store",
    "parallel_tool_calls",
    "include",
    "service_tier",
    "prompt_cache_key",
    "metadata",
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
        let kind = serde_json::to_value(&error.kind)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "provider_invalid_response".to_string());
        Self {
            ok: false,
            result: Value::Null,
            error: Some(ProviderStdioError {
                kind,
                message: error.message,
                provider_summary: error.provider_summary,
                provider_details: error.provider_details,
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
    validate_model: bool,
    transport_mode: OpenAiTransportMode,
    proxy_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAiTransportMode {
    Auto,
    HttpSse,
    ResponsesWebsocket,
}

impl OpenAiTransportMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::HttpSse => "http_sse",
            Self::ResponsesWebsocket => "responses_websocket",
        }
    }
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
    pub content_blocks: Option<Value>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub is_error: Option<bool>,
    #[serde(default)]
    pub tool_calls: Option<Value>,
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
    #[serde(rename = "1flowbase.provider/v2")]
    #[default]
    Current,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativePromptBlock {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<NativePromptCacheControl>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePromptCacheControl {
    #[serde(rename = "type")]
    pub cache_type: NativePromptCacheControlType,
    #[serde(default)]
    pub ttl: Option<NativePromptCacheTtl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePromptCacheControlType {
    Ephemeral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum NativePromptCacheTtl {
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

impl NativePromptBlock {
    fn text_content(&self) -> &str {
        match self {
            Self::Text { text, .. } => text,
        }
    }
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
    pub trace_context: BTreeMap<String, String>,
    #[serde(default)]
    pub run_context: BTreeMap<String, Value>,
    #[serde(default)]
    pub client_protocol_envelope: Option<ClientProtocolEnvelope>,
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

#[derive(Debug, Default)]
struct ResponseToolCalls {
    calls: Vec<ProviderToolCall>,
    item_id_to_call_id: HashMap<String, String>,
}

impl std::ops::Deref for ResponseToolCalls {
    type Target = [ProviderToolCall];

    fn deref(&self) -> &Self::Target {
        &self.calls
    }
}

impl ResponseToolCalls {
    fn into_vec(self) -> Vec<ProviderToolCall> {
        self.calls
    }

    fn upsert_from_added_item(&mut self, item: Option<&Value>) {
        let Some(item) = item else {
            return;
        };
        let has_stable_tool_identity = item
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        if has_stable_tool_identity {
            self.upsert_from_item(Some(item));
        }
    }

    fn upsert_from_item(&mut self, item: Option<&Value>) {
        let item_id = response_item_id(item);
        if let Some(call) = provider_tool_call_from_response_item(item) {
            self.upsert(call, item_id);
        }
    }

    fn upsert(&mut self, mut call: ProviderToolCall, item_id: Option<String>) {
        if let Some(item_id) = item_id.filter(|value| !value.is_empty()) {
            if item_id != call.id {
                if let Some(position) = self
                    .calls
                    .iter()
                    .position(|existing| existing.id == item_id)
                {
                    let previous = self.calls.remove(position);
                    merge_tool_call_fields(&mut call, previous);
                }
            }
            self.item_id_to_call_id.insert(item_id, call.id.clone());
        }
        upsert_tool_call(&mut self.calls, call);
    }

    fn find_by_id_or_item_id(&self, id: &str) -> Option<&ProviderToolCall> {
        let resolved_id = self
            .item_id_to_call_id
            .get(id)
            .map(String::as_str)
            .unwrap_or(id);
        self.calls.iter().find(|call| call.id == resolved_id)
    }

    fn call_id_for_item_id(&self, item_id: &str) -> Option<&str> {
        self.item_id_to_call_id.get(item_id).map(String::as_str)
    }
}

fn merge_tool_call_fields(call: &mut ProviderToolCall, previous: ProviderToolCall) {
    if call.name == "unknown_tool" && previous.name != "unknown_tool" {
        call.name = previous.name;
    }
    if tool_call_arguments_empty(&call.arguments) && !tool_call_arguments_empty(&previous.arguments)
    {
        call.arguments = previous.arguments;
    }
}

fn tool_call_arguments_empty(arguments: &Value) -> bool {
    match arguments {
        Value::Null => true,
        Value::Object(map) => map.is_empty(),
        Value::String(text) => text.is_empty(),
        _ => false,
    }
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

#[derive(Debug, Default)]
pub struct OpenAiProviderRuntime {
    websocket_sessions: HashMap<String, ResponsesWebsocketSession>,
    websocket_response_ids_seen: HashSet<String>,
    websocket_turn_states_by_response_id: HashMap<String, String>,
    websocket_chain_inputs_by_response_id: HashMap<String, Vec<Value>>,
}

impl OpenAiProviderRuntime {
    pub async fn handle_request(
        &mut self,
        request: ProviderStdioRequest,
    ) -> Result<ProviderStdioResponse> {
        match request.method.as_str() {
            "validate" => {
                let config = normalize_provider_config(&request.input)?;
                let model_count = if config.validate_model {
                    request_json(&config, "/models", Method::GET, None)
                        .await?
                        .get("data")
                        .and_then(Value::as_array)
                        .map(Vec::len)
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
                        "organization": config.organization,
                        "project": config.project,
                        "transport_mode": config.transport_mode.as_str(),
                        "proxy_url": config.proxy_url.as_ref().map(|_| "***"),
                    },
                    "model_count": model_count,
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
                let output = self.invoke_response(input).await?;
                Ok(ProviderStdioResponse::ok(serde_json::to_value(output)?))
            }
            other => Ok(ProviderStdioResponse::error(
                "provider_invalid_response",
                format!("unsupported method: {other}"),
            )),
        }
    }

    pub async fn handle_invoke_request_streaming<F>(
        &mut self,
        input: Value,
        on_event: F,
    ) -> Result<ProviderInvocationResult>
    where
        F: FnMut(&ProviderStreamEvent) -> Result<()>,
    {
        let input: ProviderInvocationInput = serde_json::from_value(input)?;
        let output = self
            .invoke_response_with_event_sink(input, on_event)
            .await?;
        Ok(output.result)
    }

    async fn invoke_response(
        &mut self,
        input: ProviderInvocationInput,
    ) -> Result<RuntimeInvocationEnvelope> {
        self.invoke_response_with_event_sink(input, |_| Ok(()))
            .await
    }
}

pub async fn handle_request(request: ProviderStdioRequest) -> Result<ProviderStdioResponse> {
    OpenAiProviderRuntime::default()
        .handle_request(request)
        .await
}

pub async fn handle_invoke_request_streaming<F>(
    input: Value,
    on_event: F,
) -> Result<ProviderInvocationResult>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    OpenAiProviderRuntime::default()
        .handle_invoke_request_streaming(input, on_event)
        .await
}

fn normalize_provider_config(input: &Value) -> Result<ProviderConfig> {
    let config = input
        .as_object()
        .ok_or_else(|| anyhow!("provider_config must be an object"))?;
    Ok(ProviderConfig {
        base_url: optional_text(config.get("base_url"))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        api_key: require_text(config.get("api_key"), "api_key")?,
        organization: optional_text(config.get("organization")),
        project: optional_text(config.get("project")),
        validate_model: config
            .get("validate_model")
            .and_then(Value::as_bool)
            .unwrap_or(DEFAULT_VALIDATE_MODEL),
        transport_mode: normalize_transport_mode(config.get("transport_mode"))?,
        proxy_url: normalize_proxy_url(config.get("proxy_url"))?,
    })
}

fn normalize_transport_mode(value: Option<&Value>) -> Result<OpenAiTransportMode> {
    let Some(value) = value else {
        return Ok(OpenAiTransportMode::HttpSse);
    };
    let text = value_to_string(value).trim().to_ascii_lowercase();
    match text.as_str() {
        "" | "auto" => Ok(OpenAiTransportMode::Auto),
        "http_sse" | "sse" | "http" => Ok(OpenAiTransportMode::HttpSse),
        "responses_websocket" | "websocket" | "ws" => Ok(OpenAiTransportMode::ResponsesWebsocket),
        other => bail!("unsupported transport_mode: {other}"),
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

fn build_json_headers(
    config: &ProviderConfig,
    include_json_body: bool,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<HeaderMap> {
    build_headers(
        config,
        include_json_body,
        "application/json",
        client_protocol_envelope,
    )
}

fn build_stream_headers(
    config: &ProviderConfig,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<HeaderMap> {
    build_headers(config, true, "text/event-stream", client_protocol_envelope)
}

fn build_headers(
    config: &ProviderConfig,
    include_json_body: bool,
    accept: &'static str,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static(accept));
    if include_json_body {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
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

fn build_http_client(config: &ProviderConfig) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy_url) = &config.proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy_url")?);
    }
    builder.build().context("building OpenAI HTTP client")
}

fn sanitize_reqwest_error(error: reqwest::Error, config: &ProviderConfig) -> anyhow::Error {
    anyhow!(redact_config_secrets(config, &error.to_string()))
}

fn redact_config_secrets(config: &ProviderConfig, message: &str) -> String {
    let mut sanitized = message.replace(&config.api_key, "***");
    if let Some(proxy_url) = &config.proxy_url {
        sanitized = sanitized.replace(proxy_url, "***");
    }
    sanitized
}

fn build_url(config: &ProviderConfig, pathname: &str) -> Result<String> {
    let base_url = config.base_url.trim_end_matches('/');
    Url::parse(&format!("{base_url}{pathname}"))
        .with_context(|| format!("invalid base_url: {}", config.base_url))
        .map(|value| value.to_string())
}

async fn request_json(
    config: &ProviderConfig,
    pathname: &str,
    method: Method,
    body: Option<Value>,
) -> Result<Value> {
    let client = build_http_client(config)?;
    let mut request = client
        .request(method, build_url(config, pathname)?)
        .headers(build_json_headers(config, body.is_some(), None)?);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, config))?;
    let status = response.status();
    if !status.is_success() {
        return Err(provider_upstream_error_from_response(response)
            .await?
            .into());
    }
    let text = response.text().await?;
    parse_json_response_text(&text)
}

fn parse_json_response_text(text: &str) -> Result<Value> {
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
    let upstream_message = upstream_error_message(&raw_body);
    let message = format!("{} {}: {}", status.as_u16(), status, upstream_message);
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

fn upstream_error_message(raw_body: &str) -> String {
    extract_upstream_error_message(raw_body)
        .unwrap_or_else(|| "provider upstream request failed".to_string())
}

fn extract_upstream_error_message(raw_body: &str) -> Option<String> {
    if let Ok(payload) = serde_json::from_str::<Value>(raw_body.trim()) {
        if let Some(message) = upstream_error_message_from_json(&payload) {
            return Some(message.to_string());
        }
    }

    raw_body.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            return None;
        }
        let payload = serde_json::from_str::<Value>(trimmed).ok()?;
        upstream_error_message_from_json(&payload).map(ToOwned::to_owned)
    })
}

fn upstream_error_message_from_json(payload: &Value) -> Option<&str> {
    payload
        .get("error")
        .and_then(|value| value.get("message").or(Some(value)))
        .and_then(Value::as_str)
        .or_else(|| payload.get("message").and_then(Value::as_str))
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

fn normalize_model_entries(raw: &Value) -> Result<Vec<ProviderModelDescriptor>> {
    let entries = raw
        .as_array()
        .ok_or_else(|| anyhow!("model list response must contain data array"))?;
    entries.iter().map(normalize_model_entry).collect()
}

fn normalize_model_entry(entry: &Value) -> Result<ProviderModelDescriptor> {
    let model_id = entry
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("model id is required"))?
        .to_string();
    Ok(ProviderModelDescriptor {
        display_name: model_id.clone(),
        model_id,
        source: "dynamic".to_string(),
        supports_streaming: true,
        supports_tool_call: true,
        supports_multimodal: true,
        context_window: None,
        max_output_tokens: None,
        provider_metadata: json!({ "owned_by": entry.get("owned_by").cloned().unwrap_or(Value::Null) }),
    })
}

impl OpenAiProviderRuntime {
    async fn invoke_response_with_event_sink<F>(
        &mut self,
        input: ProviderInvocationInput,
        mut on_event: F,
    ) -> Result<RuntimeInvocationEnvelope>
    where
        F: FnMut(&ProviderStreamEvent) -> Result<()>,
    {
        let config = normalize_provider_config(&input.provider_config)?;
        let body = build_responses_body(&input)?;
        match config.transport_mode {
            OpenAiTransportMode::HttpSse => {
                invoke_response_http_sse(
                    &config,
                    body,
                    input.model.clone(),
                    &mut on_event,
                    input.client_protocol_envelope.as_ref(),
                )
                .await
            }
            OpenAiTransportMode::ResponsesWebsocket => {
                let requires_websocket_cursor =
                    self.responses_body_uses_websocket_response_cursor(&body);
                match self
                    .invoke_response_websocket_with_cursor_retry(
                        &config,
                        &input,
                        body.clone(),
                        &mut on_event,
                    )
                    .await
                {
                    Ok(output) => Ok(output),
                    Err(error)
                        if error.fallback_allowed
                            && !requires_websocket_cursor
                            && can_fallback_to_http(&error.source) =>
                    {
                        invoke_response_http_sse(
                            &config,
                            body,
                            input.model.clone(),
                            &mut on_event,
                            input.client_protocol_envelope.as_ref(),
                        )
                        .await
                    }
                    Err(error) => Err(error.source),
                }
            }
            OpenAiTransportMode::Auto => {
                let requires_websocket_cursor =
                    self.responses_body_uses_websocket_response_cursor(&body);
                match self
                    .invoke_response_websocket_with_cursor_retry(
                        &config,
                        &input,
                        body.clone(),
                        &mut on_event,
                    )
                    .await
                {
                    Ok(output) => Ok(output),
                    Err(error)
                        if error.fallback_allowed
                            && !requires_websocket_cursor
                            && can_fallback_to_http(&error.source) =>
                    {
                        invoke_response_http_sse(
                            &config,
                            body,
                            input.model.clone(),
                            &mut on_event,
                            input.client_protocol_envelope.as_ref(),
                        )
                        .await
                    }
                    Err(error) => Err(error.source),
                }
            }
        }
    }

    async fn invoke_response_websocket_with_cursor_retry<F>(
        &mut self,
        config: &ProviderConfig,
        input: &ProviderInvocationInput,
        body: Value,
        on_event: &mut F,
    ) -> Result<RuntimeInvocationEnvelope, WebsocketInvocationError>
    where
        F: FnMut(&ProviderStreamEvent) -> Result<()>,
    {
        let mut retry_body = body;
        let mut reconnect_attempts = 0;
        let mut full_context_retry_used = false;
        loop {
            let retry_response_id =
                responses_body_previous_response_id(&retry_body).map(ToOwned::to_owned);
            match self
                .invoke_response_websocket(config, input, retry_body.clone(), on_event)
                .await
            {
                Ok(output) => return Ok(output),
                Err(error)
                    if retry_response_id.is_some()
                        && !full_context_retry_used
                        && (error.reconnect_allowed || error.fallback_allowed)
                        && websocket_previous_response_unavailable(&error.source) =>
                {
                    let Some(full_context_body) =
                        retry_response_id.as_deref().and_then(|response_id| {
                            self.websocket_full_context_retry_body(response_id, &retry_body)
                        })
                    else {
                        return Err(error);
                    };
                    retry_body = full_context_body;
                    reconnect_attempts = 0;
                    full_context_retry_used = true;
                    continue;
                }
                Err(error)
                    if retry_response_id.is_some()
                        && error.reconnect_allowed
                        && reconnect_attempts < WEBSOCKET_CURSOR_RECONNECT_ATTEMPTS =>
                {
                    reconnect_attempts += 1;
                    if websocket_proxy_failure_requires_fresh_turn_state(&error.source) {
                        if let Some(response_id) = retry_response_id.as_deref() {
                            self.websocket_turn_states_by_response_id
                                .remove(response_id);
                        }
                    }
                    continue;
                }
                Err(error)
                    if retry_response_id.is_some()
                        && error.reconnect_allowed
                        && !full_context_retry_used =>
                {
                    let Some(full_context_body) =
                        retry_response_id.as_deref().and_then(|response_id| {
                            self.websocket_full_context_retry_body(response_id, &retry_body)
                        })
                    else {
                        return Err(error);
                    };
                    retry_body = full_context_body;
                    reconnect_attempts = 0;
                    full_context_retry_used = true;
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn invoke_response_websocket<F>(
        &mut self,
        config: &ProviderConfig,
        input: &ProviderInvocationInput,
        body: Value,
        on_event: &mut F,
    ) -> Result<RuntimeInvocationEnvelope, WebsocketInvocationError>
    where
        F: FnMut(&ProviderStreamEvent) -> Result<()>,
    {
        let session_key = websocket_session_key(config, input);
        if !self.websocket_sessions.contains_key(&session_key) {
            let turn_state = responses_body_previous_response_id(&body)
                .and_then(|response_id| self.websocket_turn_states_by_response_id.get(response_id))
                .map(String::as_str);
            let session = connect_responses_websocket(
                config,
                turn_state,
                input.client_protocol_envelope.as_ref(),
            )
            .await
            .map_err(WebsocketInvocationError::fallback_allowed)?;
            self.websocket_sessions.insert(session_key.clone(), session);
        }

        let session = self
            .websocket_sessions
            .get_mut(&session_key)
            .expect("websocket session should be initialized");
        let mut request_body = build_websocket_response_create_body(body.clone());
        let result =
            read_websocket_response(session, &mut request_body, input.model.clone(), on_event)
                .await;

        match result {
            Ok(response) => {
                let output = response.envelope;
                let turn_state = self
                    .websocket_sessions
                    .get(&session_key)
                    .and_then(|session| session.turn_state.clone());
                if let Some(response_id) = output
                    .result
                    .response_id
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                {
                    self.websocket_response_ids_seen
                        .insert(response_id.to_string());
                    if let Some(turn_state) = turn_state.as_deref() {
                        self.websocket_turn_states_by_response_id
                            .insert(response_id.to_string(), turn_state.to_string());
                    }
                    self.record_websocket_response_chain(response_id, &body, &output.result);
                }
                if !response.session_reusable {
                    self.websocket_sessions.remove(&session_key);
                }
                Ok(output)
            }
            Err(error) => {
                self.websocket_sessions.remove(&session_key);
                Err(error)
            }
        }
    }

    fn responses_body_uses_websocket_response_cursor(&self, body: &Value) -> bool {
        responses_body_previous_response_id(body)
            .is_some_and(|response_id| self.websocket_response_ids_seen.contains(response_id))
    }

    fn websocket_full_context_retry_body(
        &self,
        previous_response_id: &str,
        body: &Value,
    ) -> Option<Value> {
        let previous_chain_inputs = self
            .websocket_chain_inputs_by_response_id
            .get(previous_response_id)?;
        let mut retry_body = body.clone();
        let object = retry_body.as_object_mut()?;
        object.remove("previous_response_id");
        let mut full_input = previous_chain_inputs.clone();
        full_input.extend(responses_body_input_items(body));
        object.insert("input".to_string(), Value::Array(full_input));
        Some(retry_body)
    }

    fn record_websocket_response_chain(
        &mut self,
        response_id: &str,
        request_body: &Value,
        result: &ProviderInvocationResult,
    ) {
        let previous_response_id = responses_body_previous_response_id(request_body);
        let mut chain_inputs = match previous_response_id {
            Some(previous_response_id) => {
                let Some(previous_inputs) = self
                    .websocket_chain_inputs_by_response_id
                    .get(previous_response_id)
                else {
                    return;
                };
                previous_inputs.clone()
            }
            None => Vec::new(),
        };
        chain_inputs.extend(responses_body_input_items(request_body));
        chain_inputs.extend(response_output_input_items(result));
        self.websocket_chain_inputs_by_response_id
            .insert(response_id.to_string(), chain_inputs);
    }
}

async fn invoke_response_http_sse<F>(
    config: &ProviderConfig,
    body: Value,
    request_model: String,
    on_event: &mut F,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let response = build_http_client(config)?
        .request(Method::POST, build_url(config, "/responses")?)
        .headers(build_stream_headers(config, client_protocol_envelope)?)
        .json(&body)
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, config))?;
    read_streaming_response(response, request_model, on_event).await
}

fn build_responses_body(input: &ProviderInvocationInput) -> Result<Value> {
    if input.model.trim().is_empty() {
        bail!("model is required");
    }
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
        bail!("OpenAI Generate does not support the requested semantic capabilities");
    }
    let mut body = Map::new();
    body.insert(
        "model".to_string(),
        Value::String(input.model.trim().to_string()),
    );
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
            Value::Array(build_response_tools(&input.tools)),
        );
    }
    if let Some(response_format) = input
        .response_format
        .clone()
        .and_then(normalize_response_text_format)
        .or_else(|| {
            parameter_value(input, "response_format").and_then(normalize_response_text_format)
        })
    {
        body.insert("text".to_string(), json!({ "format": response_format }));
    }
    if let Some(reasoning_effort) = parameter_value(input, "reasoning_effort") {
        body.insert(
            "reasoning".to_string(),
            json!({ "effort": reasoning_effort }),
        );
    }
    for key in PASSTHROUGH_RESPONSE_PARAMETERS {
        if let Some(value) = parameter_value(input, key) {
            body.insert((*key).to_string(), value);
        }
    }
    Ok(Value::Object(body))
}

fn responses_body_previous_response_id(body: &Value) -> Option<&str> {
    body.get("previous_response_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn responses_body_input_items(body: &Value) -> Vec<Value> {
    body.get("input")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn response_output_input_items(result: &ProviderInvocationResult) -> Vec<Value> {
    let mut items = Vec::new();
    if let Some(content) = result
        .final_content
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        items.push(json!({
            "role": "assistant",
            "content": content,
        }));
    }
    for call in &result.tool_calls {
        items.push(json!({
            "type": "function_call",
            "call_id": call.id,
            "name": call.name,
            "arguments": response_tool_arguments(&call.arguments),
        }));
    }
    items
}

fn build_responses_input(input: &ProviderInvocationInput) -> Vec<Value> {
    let mut items = Vec::new();
    for message in &input.messages {
        if message.role == ProviderMessageRole::Tool {
            if let Some(call_id) = message.tool_call_id.as_deref() {
                items.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": responses_tool_output(message),
                }));
            }
            continue;
        }
        if let Some(content) = responses_message_content(message) {
            items.push(json!({
                "role": responses_role(message.role),
                "content": content,
            }));
        }
        append_response_function_calls(&mut items, message.tool_calls.as_ref());
    }
    items
}

fn responses_message_content(message: &ProviderMessage) -> Option<Value> {
    let structured = message
        .content_blocks
        .as_ref()
        .and_then(responses_structured_content);
    if structured.is_some() {
        return structured;
    }
    (!message.content.is_empty()).then(|| Value::String(message.content.clone()))
}

fn responses_tool_output(message: &ProviderMessage) -> Value {
    message
        .content_blocks
        .as_ref()
        .and_then(responses_structured_content)
        .unwrap_or_else(|| Value::String(message.content.clone()))
}

fn responses_structured_content(content_blocks: &Value) -> Option<Value> {
    let items = responses_content_items_from_value(content_blocks);
    if !responses_content_items_contain_media(&items) {
        return None;
    }
    Some(Value::Array(items))
}

fn responses_content_items_from_value(content: &Value) -> Vec<Value> {
    match content {
        Value::Array(parts) => parts.iter().filter_map(responses_content_item).collect(),
        Value::Object(object) => {
            if let Some(parts) = object.get("parts").and_then(Value::as_array) {
                return parts.iter().filter_map(responses_content_item).collect();
            }
            responses_content_item(content).into_iter().collect()
        }
        Value::String(text) => responses_text_content_item(text).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn responses_content_item(part: &Value) -> Option<Value> {
    match part {
        Value::String(text) => responses_text_content_item(text),
        Value::Object(object) => {
            let part_type = object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            match part_type.as_str() {
                "text" | "input_text" => object
                    .get("text")
                    .or_else(|| object.get("content"))
                    .and_then(Value::as_str)
                    .and_then(responses_text_content_item),
                "image" | "image_url" | "input_image" => responses_image_content_item(object),
                _ => None,
            }
        }
        _ => None,
    }
}

fn responses_text_content_item(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| json!({ "type": "input_text", "text": text }))
}

fn responses_image_content_item(object: &Map<String, Value>) -> Option<Value> {
    let image_url = object
        .get("source")
        .and_then(responses_image_source_url)
        .or_else(|| object.get("image_url").and_then(responses_image_url))
        .or_else(|| object.get("image").and_then(responses_image_url))
        .or_else(|| object.get("url").and_then(responses_image_url))?;
    let mut item = Map::new();
    item.insert("type".to_string(), Value::String("input_image".to_string()));
    item.insert("image_url".to_string(), Value::String(image_url));
    if let Some(detail) = object
        .get("detail")
        .or_else(|| {
            object
                .get("image_url")
                .and_then(|value| value.get("detail"))
        })
        .cloned()
    {
        item.insert("detail".to_string(), detail);
    }
    Some(Value::Object(item))
}

fn responses_image_source_url(source: &Value) -> Option<String> {
    let object = source.as_object()?;
    match object.get("type").and_then(Value::as_str) {
        Some("base64") | None => responses_base64_image_source_url(object),
        Some("url") => object
            .get("url")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn responses_base64_image_source_url(object: &Map<String, Value>) -> Option<String> {
    let media_type = object
        .get("media_type")
        .and_then(Value::as_str)
        .unwrap_or("image/png");
    let data = object.get("data").and_then(Value::as_str)?;
    Some(format!("data:{media_type};base64,{data}"))
}

fn responses_image_url(value: &Value) -> Option<String> {
    value.as_str().map(ToOwned::to_owned).or_else(|| {
        value
            .get("url")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

fn responses_content_items_contain_media(items: &[Value]) -> bool {
    items.iter().any(|item| {
        item.get("type")
            .and_then(Value::as_str)
            .is_some_and(|item_type| item_type == "input_image" || item_type == "input_file")
    })
}

fn responses_role(role: ProviderMessageRole) -> &'static str {
    match role {
        ProviderMessageRole::System => "developer",
        ProviderMessageRole::Assistant => "assistant",
        ProviderMessageRole::User => "user",
        ProviderMessageRole::Tool => "tool",
    }
}

fn append_response_function_calls(items: &mut Vec<Value>, tool_calls: Option<&Value>) {
    let Some(calls) = tool_calls.and_then(Value::as_array) else {
        return;
    };
    for (index, call) in calls.iter().enumerate() {
        if let Some(function_call) = response_function_call_from_native(call, index) {
            items.push(function_call);
        }
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
        .map(response_tool_arguments)
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

fn build_response_tools(tools: &[Value]) -> Vec<Value> {
    tools.iter().map(build_response_tool).collect()
}

fn build_response_tool(tool: &Value) -> Value {
    if tool.get("type").and_then(Value::as_str) != Some("function") {
        return tool.clone();
    }
    let function = tool.get("function").and_then(Value::as_object);
    let Some(function) = function else {
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
    if let Some(strict) = function.get("strict") {
        mapped.insert("strict".to_string(), strict.clone());
    }
    Value::Object(mapped)
}

fn parameter_value(input: &ProviderInvocationInput, key: &str) -> Option<Value> {
    input
        .model_parameters
        .get(key)
        .cloned()
        .and_then(normalize_scalar_parameter)
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

fn normalize_response_text_format(value: Value) -> Option<Value> {
    match normalize_scalar_parameter(value)? {
        Value::Object(object) if object.contains_key("type") => Some(Value::Object(object)),
        Value::String(text) => serde_json::from_str::<Value>(&text)
            .ok()
            .filter(|value| value.get("type").is_some())
            .or_else(|| Some(json!({ "type": text }))),
        other => Some(json!({ "type": other })),
    }
}

fn response_tool_arguments(arguments: &Value) -> String {
    match arguments {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

#[derive(Debug)]
struct ResponsesWebsocketSession {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    turn_state: Option<String>,
}

#[derive(Debug)]
struct WebsocketResponseOutput {
    envelope: RuntimeInvocationEnvelope,
    session_reusable: bool,
}

#[derive(Debug)]
struct WebsocketInvocationError {
    source: anyhow::Error,
    fallback_allowed: bool,
    reconnect_allowed: bool,
}

impl WebsocketInvocationError {
    fn fallback_allowed(source: anyhow::Error) -> Self {
        Self {
            source,
            fallback_allowed: true,
            reconnect_allowed: false,
        }
    }

    fn fallback_blocked(source: anyhow::Error) -> Self {
        Self {
            source,
            fallback_allowed: false,
            reconnect_allowed: false,
        }
    }

    fn from_stream_state(source: anyhow::Error, fallback_blocked: bool) -> Self {
        if fallback_blocked {
            Self::fallback_blocked(source)
        } else {
            Self::fallback_allowed(source)
        }
    }

    fn reconnect_allowed(source: anyhow::Error) -> Self {
        Self {
            source,
            fallback_allowed: true,
            reconnect_allowed: true,
        }
    }

    fn from_reconnectable_stream_state(source: anyhow::Error, fallback_blocked: bool) -> Self {
        if fallback_blocked {
            Self::fallback_blocked(source)
        } else {
            Self::reconnect_allowed(source)
        }
    }
}

async fn connect_responses_websocket(
    config: &ProviderConfig,
    turn_state: Option<&str>,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<ResponsesWebsocketSession> {
    let url = build_websocket_url(config)?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|error| anyhow!("failed to build websocket request: {error}"))?;
    request.headers_mut().extend(build_websocket_headers(
        config,
        turn_state,
        client_protocol_envelope,
    )?);
    let (stream, response) = if config.proxy_url.is_some() {
        connect_responses_websocket_through_proxy(config, &url, request).await
    } else {
        connect_async(request)
            .await
            .map_err(|error| anyhow!("failed to connect Responses websocket: {error}"))
    }?;
    // Codex turn state is first-writer-wins within a turn; continuation handshakes
    // must not rotate the sticky routing token for later tool callbacks.
    let turn_state = turn_state.map(ToOwned::to_owned).or_else(|| {
        response
            .headers()
            .get(X_CODEX_TURN_STATE_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
    });
    Ok(ResponsesWebsocketSession { stream, turn_state })
}

async fn connect_responses_websocket_through_proxy(
    config: &ProviderConfig,
    url: &Url,
    request: ClientHandshakeRequest,
) -> Result<(
    WebSocketStream<MaybeTlsStream<TcpStream>>,
    ClientHandshakeResponse,
)> {
    let proxy_url = config
        .proxy_url
        .as_deref()
        .ok_or_else(|| anyhow!("proxy_url is required"))?;
    let target = proxy_connect_target(url)?;
    let stream = connect_http_proxy_tunnel(proxy_url, &target).await?;
    client_async_tls_with_config(request, stream, None, None)
        .await
        .map_err(|error| anyhow!("failed to connect Responses websocket through proxy: {error}"))
}

fn proxy_connect_target(url: &Url) -> Result<String> {
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("websocket url missing host"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("websocket url missing port"))?;
    Ok(format!("{}:{port}", format_authority_host(host)))
}

async fn connect_http_proxy_tunnel(proxy_url: &str, target: &str) -> Result<TcpStream> {
    let proxy = Url::parse(proxy_url).with_context(|| "invalid proxy_url")?;
    if proxy.scheme() != "http" {
        bail!("proxy_url must use http scheme");
    }
    let proxy_host = proxy
        .host_str()
        .ok_or_else(|| anyhow!("proxy_url missing host"))?;
    let proxy_port = proxy
        .port_or_known_default()
        .ok_or_else(|| anyhow!("proxy_url missing port"))?;
    let proxy_address = format!("{}:{proxy_port}", format_authority_host(proxy_host));
    let mut stream = TcpStream::connect(&proxy_address)
        .await
        .with_context(|| format!("connecting proxy {proxy_address}"))?;

    let mut request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
    if let Some(proxy_authorization) = proxy_authorization_header(&proxy) {
        request.push_str(&proxy_authorization);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .context("writing proxy CONNECT request")?;

    let mut response = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !response.windows(4).any(|window| window == b"\r\n\r\n") {
        if response.len() > 8192 {
            bail!("proxy CONNECT response headers exceeded 8192 bytes");
        }
        let read = stream
            .read(&mut chunk)
            .await
            .context("reading proxy CONNECT response")?;
        if read == 0 {
            bail!("proxy closed before CONNECT response");
        }
        response.extend_from_slice(&chunk[..read]);
    }

    let response_text = String::from_utf8_lossy(&response);
    let status_line = response_text.lines().next().unwrap_or_default();
    let status = status_line.split_whitespace().nth(1).unwrap_or_default();
    if status != "200" {
        bail!("proxy CONNECT failed: {status_line}");
    }
    Ok(stream)
}

fn proxy_authorization_header(proxy: &Url) -> Option<String> {
    let username = proxy.username();
    if username.is_empty() {
        return None;
    }
    let password = proxy.password().unwrap_or_default();
    let credentials = format!("{username}:{password}");
    let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
    Some(format!("Proxy-Authorization: Basic {encoded}"))
}

fn format_authority_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

fn build_websocket_url(config: &ProviderConfig) -> Result<Url> {
    let mut url = Url::parse(&build_url(config, "/responses")?)
        .with_context(|| format!("invalid base_url: {}", config.base_url))?;
    let websocket_scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        other => bail!("unsupported websocket base_url scheme: {other}"),
    };
    url.set_scheme(websocket_scheme)
        .map_err(|_| anyhow!("unsupported websocket base_url scheme"))?;
    Ok(url)
}

fn build_websocket_headers(
    config: &ProviderConfig,
    turn_state: Option<&str>,
    client_protocol_envelope: Option<&ClientProtocolEnvelope>,
) -> Result<HeaderMap> {
    let mut headers = build_headers(config, false, "application/json", client_protocol_envelope)?;
    if let Some(turn_state) = turn_state.filter(|value| !value.trim().is_empty()) {
        headers.insert(
            HeaderName::from_static(X_CODEX_TURN_STATE_HEADER),
            HeaderValue::from_str(turn_state).context("invalid x-codex-turn-state header")?,
        );
    }
    headers.insert(
        HeaderName::from_static("openai-beta"),
        HeaderValue::from_static(RESPONSES_WEBSOCKETS_BETA),
    );
    Ok(headers)
}

fn build_websocket_response_create_body(mut body: Value) -> Value {
    let mut object = Map::new();
    object.insert(
        "type".to_string(),
        Value::String("response.create".to_string()),
    );
    if let Value::Object(body_object) = &mut body {
        for (key, value) in std::mem::take(body_object) {
            object.insert(key, value);
        }
    }
    Value::Object(object)
}

fn websocket_session_key(config: &ProviderConfig, input: &ProviderInvocationInput) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        input.provider_instance_id,
        config.base_url,
        config.api_key,
        config.organization.as_deref().unwrap_or_default(),
        config.project.as_deref().unwrap_or_default(),
    )
}

fn can_fallback_to_http(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    !(message.contains("401")
        || message.contains("403")
        || message.contains("unauthorized")
        || message.contains("forbidden")
        || message.contains("invalid_api_key"))
}

fn websocket_proxy_failure_requires_fresh_turn_state(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("upstream websocket proxy failed")
}

fn websocket_previous_response_unavailable(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("previous_response_id")
        && (message.contains("no longer available")
            || message.contains("not available")
            || message.contains("unavailable"))
}

async fn send_websocket_json(session: &mut ResponsesWebsocketSession, body: &Value) -> Result<()> {
    let payload = serde_json::to_string(body)?;
    session
        .stream
        .send(Message::Text(payload.into()))
        .await
        .map_err(map_websocket_error)
}

async fn send_websocket_response_processed(
    session: &mut ResponsesWebsocketSession,
    response_id: &str,
) -> Result<()> {
    send_websocket_json(
        session,
        &json!({
            "type": "response.processed",
            "response_id": response_id,
        }),
    )
    .await
}

async fn read_websocket_response<F>(
    session: &mut ResponsesWebsocketSession,
    request_body: &mut Value,
    request_model: String,
    on_event: &mut F,
) -> Result<WebsocketResponseOutput, WebsocketInvocationError>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    send_websocket_json(session, request_body)
        .await
        .map_err(WebsocketInvocationError::reconnect_allowed)?;

    let mut events = Vec::new();
    let mut all_events = Vec::new();
    let mut text = String::new();
    let mut tool_calls = ResponseToolCalls::default();
    let mut usage = ProviderUsage::default();
    let mut finish_reason = ProviderFinishReason::Unknown;
    let mut response_id = Value::Null;
    let mut visible_output_started = false;
    let mut semantic_terminal_failure_seen = false;
    let mut session_reusable = true;

    loop {
        let next_message =
            match tokio::time::timeout(STREAM_IDLE_TIMEOUT, session.stream.next()).await {
                Ok(message) => message,
                Err(_) => {
                    let error = anyhow!("idle timeout waiting for Responses websocket");
                    return Err(WebsocketInvocationError::from_reconnectable_stream_state(
                        error,
                        visible_output_started || semantic_terminal_failure_seen,
                    ));
                }
            };
        let Some(message) = next_message else {
            if can_finalize_response_on_close(&response_id, &text, &tool_calls) {
                session_reusable = false;
                break;
            }
            let error = anyhow!("websocket closed before response.completed");
            return Err(WebsocketInvocationError::from_reconnectable_stream_state(
                error,
                visible_output_started || semantic_terminal_failure_seen,
            ));
        };
        let message = message.map_err(|error| {
            WebsocketInvocationError::from_reconnectable_stream_state(
                map_websocket_error(error),
                visible_output_started || semantic_terminal_failure_seen,
            )
        })?;

        match message {
            Message::Text(payload) => {
                let payload = payload.as_str();
                if let Some(message) = websocket_error_message(payload) {
                    let error = anyhow!(message);
                    return Err(WebsocketInvocationError::from_stream_state(
                        error,
                        visible_output_started || semantic_terminal_failure_seen,
                    ));
                }
                semantic_terminal_failure_seen |= websocket_payload_blocks_http_fallback(payload);
                process_response_sse_payload(
                    payload,
                    &mut events,
                    &mut text,
                    &mut tool_calls,
                    &mut usage,
                    &mut finish_reason,
                    &mut response_id,
                )
                .map_err(|error| {
                    WebsocketInvocationError::from_stream_state(
                        error,
                        visible_output_started || semantic_terminal_failure_seen,
                    )
                })?;
                if !events.is_empty() {
                    emit_new_events(&events, on_event)
                        .map_err(WebsocketInvocationError::fallback_blocked)?;
                    visible_output_started = true;
                    all_events.append(&mut events);
                }
                if response_stream_finished(&finish_reason) {
                    break;
                }
            }
            Message::Ping(payload) => {
                session
                    .stream
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| {
                        WebsocketInvocationError::from_reconnectable_stream_state(
                            map_websocket_error(error),
                            visible_output_started || semantic_terminal_failure_seen,
                        )
                    })?;
            }
            Message::Pong(_) => {}
            Message::Close(frame) => {
                if can_finalize_response_on_close(&response_id, &text, &tool_calls) {
                    session_reusable = false;
                    break;
                }
                let error = websocket_closed_before_completed_error(frame);
                return Err(WebsocketInvocationError::from_reconnectable_stream_state(
                    error,
                    visible_output_started || semantic_terminal_failure_seen,
                ));
            }
            Message::Binary(_) | Message::Frame(_) => {}
        }
    }

    if response_id.is_null() && !can_finalize_response_on_close(&response_id, &text, &tool_calls) {
        return Err(WebsocketInvocationError::from_stream_state(
            anyhow!("websocket closed before response.completed"),
            visible_output_started || semantic_terminal_failure_seen,
        ));
    }
    if response_id.is_null() && can_finalize_response_on_close(&response_id, &text, &tool_calls) {
        return Err(WebsocketInvocationError::fallback_blocked(anyhow!(
            "websocket closed before response.completed"
        )));
    }
    if matches!(finish_reason, ProviderFinishReason::Unknown)
        && can_finalize_response_on_close(&response_id, &text, &tool_calls)
    {
        finish_reason = close_finalized_finish_reason(&tool_calls);
    }

    let output = finalize_response_stream(
        all_events,
        text,
        tool_calls.into_vec(),
        usage,
        finish_reason,
        response_id,
        json!({
            "request_model": request_model,
            "transport": "responses_websocket",
        }),
        on_event,
    )
    .map_err(WebsocketInvocationError::fallback_blocked)?;
    if let Some(response_id) = output
        .result
        .response_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        if send_websocket_response_processed(session, response_id)
            .await
            .is_err()
        {
            session_reusable = false;
        }
    }
    Ok(WebsocketResponseOutput {
        envelope: output,
        session_reusable,
    })
}

fn map_websocket_error(error: WebSocketError) -> anyhow::Error {
    anyhow!("Responses websocket error: {error}")
}

fn websocket_closed_before_completed_error(
    frame: Option<tokio_tungstenite::tungstenite::protocol::CloseFrame>,
) -> anyhow::Error {
    let Some(frame) = frame else {
        return anyhow!("websocket closed by server before response.completed");
    };
    if frame.reason.is_empty() {
        anyhow!(
            "websocket closed by server before response.completed (code: {})",
            frame.code
        )
    } else {
        anyhow!(
            "websocket closed by server before response.completed (code: {}, reason: {})",
            frame.code,
            frame.reason
        )
    }
}

fn websocket_error_message(payload: &str) -> Option<String> {
    let value: Value = serde_json::from_str(payload).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("error") {
        return None;
    }
    let status = value
        .get("status")
        .or_else(|| value.get("status_code"))
        .map(value_to_string);
    let message = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Responses websocket error");
    let code = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str);
    Some(match (status, code) {
        (Some(status), Some(code)) => format!("{status} {code}: {message}"),
        (Some(status), None) => format!("{status}: {message}"),
        (None, Some(code)) => format!("{code}: {message}"),
        (None, None) => message.to_string(),
    })
}

fn websocket_payload_blocks_http_fallback(payload: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return false;
    };
    match value.get("type").and_then(Value::as_str) {
        Some("response.failed") | Some("response.incomplete") => true,
        Some("response.completed") | Some("response.done") => value
            .get("response")
            .is_some_and(response_status_blocks_http_fallback),
        _ => false,
    }
}

fn response_status_blocks_http_fallback(response: &Value) -> bool {
    response
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status, "failed" | "incomplete" | "cancelled"))
}

fn response_stream_finished(finish_reason: &ProviderFinishReason) -> bool {
    !matches!(finish_reason, ProviderFinishReason::Unknown)
}

fn can_finalize_response_on_close(
    response_id: &Value,
    text: &str,
    tool_calls: &[ProviderToolCall],
) -> bool {
    !response_id.is_null() && (!text.is_empty() || !tool_calls.is_empty())
}

fn close_finalized_finish_reason(tool_calls: &[ProviderToolCall]) -> ProviderFinishReason {
    if tool_calls.is_empty() {
        ProviderFinishReason::Stop
    } else {
        ProviderFinishReason::ToolCall
    }
}

async fn read_streaming_response<F>(
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
    let headers = response.headers().clone();
    let upstream_request_id = header_text(&headers, "x-request-id");
    let upstream_model =
        header_text(&headers, "openai-model").or_else(|| header_text(&headers, "x-openai-model"));
    let models_etag = header_text(&headers, "x-models-etag");
    let mut events = Vec::new();
    let mut all_events = Vec::new();
    let mut text = String::new();
    let mut tool_calls = ResponseToolCalls::default();
    let mut usage = ProviderUsage::default();
    let mut finish_reason = ProviderFinishReason::Unknown;
    let mut response_id = Value::Null;
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    while let Some(chunk) = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next())
        .await
        .context("idle timeout waiting for Responses SSE")?
    {
        buffer.push_str(&String::from_utf8_lossy(&chunk?));
        while let Some((index, delimiter_len)) = find_sse_event_boundary(&buffer) {
            let block = buffer[..index].to_string();
            buffer = buffer[index + delimiter_len..].to_string();
            process_response_sse_block(
                &block,
                &mut events,
                &mut text,
                &mut tool_calls,
                &mut usage,
                &mut finish_reason,
                &mut response_id,
            )?;
            emit_new_events(&events, on_event)?;
            all_events.append(&mut events);
        }
    }
    if !buffer.trim().is_empty() {
        process_response_sse_block(
            &buffer,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )?;
        emit_new_events(&events, on_event)?;
        all_events.append(&mut events);
    }
    if response_id.is_null() || !response_stream_finished(&finish_reason) {
        if can_finalize_response_on_close(&response_id, &text, &tool_calls) {
            finish_reason = close_finalized_finish_reason(&tool_calls);
        } else {
            bail!("stream closed before response.completed");
        }
    }
    if usage.has_any_value() {
        events.push(ProviderStreamEvent::UsageSnapshot {
            usage: usage.clone(),
        });
    }
    for call in tool_calls.iter() {
        events.push(ProviderStreamEvent::ToolCallCommit { call: call.clone() });
    }
    events.push(ProviderStreamEvent::Finish {
        reason: finish_reason.clone(),
    });
    emit_new_events(&events, on_event)?;
    all_events.extend(events);
    let native_response_id = response_id.as_str().map(ToOwned::to_owned);
    Ok(RuntimeInvocationEnvelope {
        events: all_events,
        result: ProviderInvocationResult {
            final_content: (!text.is_empty()).then_some(text),
            response_id: native_response_id,
            tool_calls: tool_calls.into_vec(),
            mcp_calls: Vec::new(),
            usage,
            finish_reason: Some(finish_reason),
            provider_metadata: json!({
                "request_model": request_model,
                "transport": "http_sse",
                "response_id": response_id,
                "upstream_request_id": upstream_request_id,
                "upstream_model": upstream_model,
                "models_etag": models_etag,
            }),
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn finalize_response_stream<F>(
    mut all_events: Vec<ProviderStreamEvent>,
    text: String,
    tool_calls: Vec<ProviderToolCall>,
    usage: ProviderUsage,
    finish_reason: ProviderFinishReason,
    response_id: Value,
    mut provider_metadata: Value,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    if response_id.is_null() {
        bail!("stream closed before response.completed");
    }
    let mut events = Vec::new();
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
    let native_response_id = response_id.as_str().map(ToOwned::to_owned);

    if let Some(metadata) = provider_metadata.as_object_mut() {
        metadata.insert("response_id".to_string(), response_id.clone());
    }

    Ok(RuntimeInvocationEnvelope {
        events: all_events,
        result: ProviderInvocationResult {
            final_content: (!text.is_empty()).then_some(text),
            response_id: native_response_id,
            tool_calls,
            mcp_calls: Vec::new(),
            usage,
            finish_reason: Some(finish_reason),
            provider_metadata,
        },
    })
}

fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn find_sse_event_boundary(buffer: &str) -> Option<(usize, usize)> {
    match (buffer.find("\n\n"), buffer.find("\r\n\r\n")) {
        (Some(left), Some(right)) if left < right => Some((left, 2)),
        (Some(_), Some(right)) => Some((right, 4)),
        (Some(left), None) => Some((left, 2)),
        (None, Some(right)) => Some((right, 4)),
        (None, None) => None,
    }
}

fn process_response_sse_block(
    block: &str,
    events: &mut Vec<ProviderStreamEvent>,
    text: &mut String,
    tool_calls: &mut ResponseToolCalls,
    usage: &mut ProviderUsage,
    finish_reason: &mut ProviderFinishReason,
    response_id: &mut Value,
) -> Result<()> {
    let mut data = Vec::new();
    for line in block.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("data:") {
            data.push(value.trim_start());
        }
    }
    if data.is_empty() {
        return Ok(());
    }
    process_response_sse_payload(
        data.join("\n").trim(),
        events,
        text,
        tool_calls,
        usage,
        finish_reason,
        response_id,
    )
}

#[cfg(test)]
fn process_response_sse_line(
    line: &str,
    events: &mut Vec<ProviderStreamEvent>,
    text: &mut String,
    tool_calls: &mut ResponseToolCalls,
    usage: &mut ProviderUsage,
    finish_reason: &mut ProviderFinishReason,
    response_id: &mut Value,
) -> Result<()> {
    let line = line.trim();
    if !line.starts_with("data:") {
        return Ok(());
    }
    let data = line.trim_start_matches("data:").trim();
    process_response_sse_payload(
        data,
        events,
        text,
        tool_calls,
        usage,
        finish_reason,
        response_id,
    )
}

fn process_response_sse_payload(
    data: &str,
    events: &mut Vec<ProviderStreamEvent>,
    text: &mut String,
    tool_calls: &mut ResponseToolCalls,
    usage: &mut ProviderUsage,
    finish_reason: &mut ProviderFinishReason,
    response_id: &mut Value,
) -> Result<()> {
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let payload: Value = serde_json::from_str(data)?;
    capture_response_id(&payload, response_id);
    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match event_type {
        "response.created" => {
            if let Some(id) = payload
                .get("response")
                .and_then(|response| response.get("id"))
            {
                *response_id = id.clone();
            }
        }
        "response.output_text.delta" => {
            if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                text.push_str(delta);
                events.push(ProviderStreamEvent::TextDelta {
                    delta: delta.to_string(),
                });
            }
        }
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                events.push(ProviderStreamEvent::ReasoningDelta {
                    delta: delta.to_string(),
                });
            }
        }
        "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
            let call_id = payload
                .get("item_id")
                .or_else(|| payload.get("call_id"))
                .map(value_to_string)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "tool_call_1".to_string());
            events.push(ProviderStreamEvent::ToolCallDelta {
                call_id,
                delta: payload.get("delta").cloned().unwrap_or(Value::Null),
            });
        }
        "response.output_item.added" => {
            tool_calls.upsert_from_added_item(payload.get("item"));
        }
        "response.function_call_arguments.done" => {
            if let Some(call) = provider_tool_call_from_function_call_arguments_done(
                &payload,
                tool_calls,
                ResponseToolCallKind::Function,
            ) {
                tool_calls.upsert(call, response_item_id_from_payload(&payload));
            }
        }
        "response.custom_tool_call_input.done" => {
            if let Some(call) = provider_tool_call_from_function_call_arguments_done(
                &payload,
                tool_calls,
                ResponseToolCallKind::Custom,
            ) {
                tool_calls.upsert(call, response_item_id_from_payload(&payload));
            }
        }
        "response.output_item.done" => {
            tool_calls.upsert_from_item(payload.get("item"));
            if text.is_empty() {
                if let Some(item_text) = response_item_text(payload.get("item")) {
                    text.push_str(&item_text);
                }
            }
        }
        "response.failed" => {
            bail!("{}", response_failed_message(payload.get("response")));
        }
        "response.incomplete" => {
            bail!("{}", response_incomplete_message(payload.get("response")));
        }
        "response.completed" | "response.done" => {
            process_terminal_response_event(
                &payload,
                text,
                tool_calls,
                usage,
                finish_reason,
                response_id,
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn capture_response_id(payload: &Value, response_id: &mut Value) {
    if !response_id.is_null() {
        return;
    }
    if let Some(id) = payload
        .get("response_id")
        .or_else(|| payload.get("item").and_then(|item| item.get("response_id")))
        .or_else(|| {
            payload
                .get("response")
                .and_then(|response| response.get("id"))
        })
    {
        *response_id = id.clone();
    }
}

fn process_terminal_response_event(
    payload: &Value,
    text: &mut String,
    tool_calls: &mut ResponseToolCalls,
    usage: &mut ProviderUsage,
    finish_reason: &mut ProviderFinishReason,
    response_id: &mut Value,
) -> Result<()> {
    let Some(response) = payload.get("response") else {
        return Ok(());
    };
    if let Some(status) = response.get("status").and_then(Value::as_str) {
        match status {
            "failed" => bail!("{}", response_failed_message(Some(response))),
            "incomplete" => bail!("{}", response_incomplete_message(Some(response))),
            "cancelled" => bail!("response.cancelled"),
            _ => {}
        }
    }
    if let Some(id) = response.get("id") {
        *response_id = id.clone();
    }
    *usage = normalize_usage(response.get("usage").unwrap_or(&Value::Null));
    if let Some(items) = response.get("output").and_then(Value::as_array) {
        for item in items {
            tool_calls.upsert_from_item(Some(item));
        }
    }
    if text.is_empty() {
        if let Some(output_text) = response
            .get("output")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| response_item_text(Some(item)))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .filter(|value| !value.is_empty())
        {
            text.push_str(&output_text);
        }
    }
    *finish_reason = if tool_calls.is_empty() {
        ProviderFinishReason::Stop
    } else {
        ProviderFinishReason::ToolCall
    };
    Ok(())
}

fn response_failed_message(response: Option<&Value>) -> String {
    let Some(error) = response.and_then(|value| value.get("error")) else {
        return "response.failed event received".to_string();
    };
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("response.failed event received");
    let code = error.get("code").and_then(Value::as_str);
    let error_type = error.get("type").and_then(Value::as_str);
    match (code, error_type) {
        (Some(code), Some(error_type)) => format!("{code} ({error_type}): {message}"),
        (Some(code), None) => format!("{code}: {message}"),
        (None, Some(error_type)) => format!("{error_type}: {message}"),
        (None, None) => message.to_string(),
    }
}

fn response_incomplete_message(response: Option<&Value>) -> String {
    let reason = response
        .and_then(|value| value.get("incomplete_details"))
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!("response.incomplete: {reason}")
}

fn upsert_tool_call(tool_calls: &mut Vec<ProviderToolCall>, call: ProviderToolCall) {
    if let Some(existing) = tool_calls
        .iter_mut()
        .find(|existing| existing.id == call.id)
    {
        *existing = call;
    } else {
        tool_calls.push(call);
    }
}

fn provider_tool_call_from_response_item(item: Option<&Value>) -> Option<ProviderToolCall> {
    let item = item?;
    let item_type = item.get("type").and_then(Value::as_str)?;
    let arguments = match item_type {
        "function_call" => item
            .get("arguments")
            .and_then(Value::as_str)
            .and_then(|value| serde_json::from_str::<Value>(value).ok())
            .unwrap_or_else(|| json!({})),
        "custom_tool_call" => item
            .get("input")
            .cloned()
            .map(normalize_custom_tool_input)
            .unwrap_or_else(|| json!({})),
        _ => return None,
    };
    Some(ProviderToolCall {
        id: item
            .get("call_id")
            .or_else(|| item.get("id"))
            .map(value_to_string)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "tool_call_1".to_string()),
        name: item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unknown_tool")
            .to_string(),
        arguments,
    })
}

fn response_item_id(item: Option<&Value>) -> Option<String> {
    item.and_then(|item| item.get("id"))
        .map(value_to_string)
        .filter(|value| !value.is_empty())
}

fn response_item_id_from_payload(payload: &Value) -> Option<String> {
    payload
        .get("item_id")
        .map(value_to_string)
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseToolCallKind {
    Function,
    Custom,
}

fn provider_tool_call_from_function_call_arguments_done(
    payload: &Value,
    tool_calls: &ResponseToolCalls,
    kind: ResponseToolCallKind,
) -> Option<ProviderToolCall> {
    let call_id = payload
        .get("call_id")
        .map(value_to_string)
        .filter(|value| !value.is_empty());
    let item_id = response_item_id_from_payload(payload);
    let id = call_id
        .clone()
        .or_else(|| {
            item_id
                .as_deref()
                .and_then(|id| tool_calls.call_id_for_item_id(id))
                .map(ToOwned::to_owned)
        })
        .or_else(|| item_id.clone())?;
    let existing = tool_calls.find_by_id_or_item_id(&id);
    let name = payload
        .get("name")
        .or_else(|| payload.get("item").and_then(|item| item.get("name")))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            existing
                .filter(|call| call.name != "unknown_tool")
                .map(|call| call.name.clone())
        });
    let name = name?;
    let arguments = match kind {
        ResponseToolCallKind::Function => payload
            .get("arguments")
            .map(function_call_arguments_value)
            .or_else(|| existing.map(|call| call.arguments.clone()))
            .unwrap_or_else(|| json!({})),
        ResponseToolCallKind::Custom => payload
            .get("input")
            .cloned()
            .map(normalize_custom_tool_input)
            .or_else(|| existing.map(|call| call.arguments.clone()))
            .unwrap_or_else(|| json!({})),
    };

    Some(ProviderToolCall {
        id,
        name,
        arguments,
    })
}

fn function_call_arguments_value(value: &Value) -> Value {
    match value {
        Value::String(text) => serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!({})),
        Value::Null => json!({}),
        other => other.clone(),
    }
}

fn normalize_custom_tool_input(input: Value) -> Value {
    match input {
        Value::String(text) => {
            serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "input": text }))
        }
        Value::Null => json!({}),
        other => other,
    }
}

fn response_item_text(item: Option<&Value>) -> Option<String> {
    let item = item?;
    if item.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    let content = item.get("content")?.as_array()?;
    let text = content
        .iter()
        .filter_map(|part| {
            if part.get("type").and_then(Value::as_str) != Some("output_text") {
                return None;
            }
            part.get("text").and_then(Value::as_str)
        })
        .collect::<Vec<_>>()
        .join("");
    (!text.is_empty()).then_some(text)
}

fn normalize_usage(raw: &Value) -> ProviderUsage {
    ProviderUsage {
        input_tokens: raw.get("input_tokens").and_then(Value::as_u64),
        output_tokens: raw.get("output_tokens").and_then(Value::as_u64),
        total_tokens: raw.get("total_tokens").and_then(Value::as_u64),
        reasoning_tokens: raw
            .get("output_tokens_details")
            .and_then(|value| value.get("reasoning_tokens"))
            .and_then(Value::as_u64),
        cache_read_tokens: raw
            .get("input_tokens_details")
            .and_then(|value| value.get("cached_tokens"))
            .and_then(Value::as_u64),
        cache_write_tokens: None,
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
        net::{TcpListener, TcpStream},
        sync::mpsc,
        thread,
        time::Duration,
    };
    use tokio_tungstenite::tungstenite::{
        accept_hdr,
        handshake::server::{
            Request as ServerHandshakeRequest, Response as ServerHandshakeResponse,
        },
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

    fn read_http_request_with_body(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("request read timeout");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream
                .read(&mut buffer)
                .expect("request should be readable");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                })
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        String::from_utf8(request).expect("request should be utf8")
    }

    fn start_generate_sse_server() -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("generate listener should bind");
        let base_url = format!("http://{}", listener.local_addr().expect("listener addr"));
        let (request_tx, request_rx) = mpsc::channel();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("generate request should connect");
            let request = read_http_request_with_body(&mut stream);
            request_tx
                .send(request)
                .expect("generate request should be captured");
            let body = concat!(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_generate\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2},\"output\":[]}}\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("generate response should be writable");
        });

        (base_url, request_rx)
    }

    fn start_websocket_connect_proxy() -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("proxy listener should bind");
        let proxy_url = format!(
            "http://{}",
            listener.local_addr().expect("proxy listener addr")
        );
        let (request_tx, request_rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener
                .accept()
                .expect("websocket should connect to proxy");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("proxy read timeout");

            let mut buffer = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                let read = stream
                    .read(&mut chunk)
                    .expect("CONNECT request should be readable");
                if read == 0 {
                    panic!("proxy connection closed before CONNECT request");
                }
                buffer.extend_from_slice(&chunk[..read]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let connect_request =
                String::from_utf8(buffer).expect("CONNECT request should be utf8");
            request_tx
                .send(connect_request)
                .expect("CONNECT request should be observed");
            stream
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .expect("CONNECT response should be writable");

            let _websocket = accept_hdr(
                stream,
                |_request: &ServerHandshakeRequest, mut response: ServerHandshakeResponse| {
                    response
                        .headers_mut()
                        .insert("x-codex-turn-state", "proxy-turn".parse().unwrap());
                    Ok(response)
                },
            )
            .expect("websocket handshake should succeed through proxy");
        });

        (proxy_url, request_rx, handle)
    }

    #[test]
    fn ac_005_upstream_raw_body_and_secrets_stay_out_of_error_contract() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        headers.insert(
            HeaderName::from_static("x-request-id"),
            HeaderValue::from_static("req_plain"),
        );
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer sk-secret"));
        headers.insert(
            HeaderName::from_static("set-cookie"),
            HeaderValue::from_static("session=secret"),
        );
        let raw_body = "plain upstream failure body with request payload".to_string();

        let error = provider_upstream_error_from_parts(
            reqwest::StatusCode::BAD_REQUEST,
            &headers,
            raw_body.clone(),
        );

        assert_eq!(error.kind, ProviderRuntimeErrorKind::ProviderUpstreamError);
        assert!(error.message.contains("provider upstream request failed"));
        assert!(!error.message.contains(&raw_body));
        assert!(!error
            .provider_summary
            .as_deref()
            .expect("provider_summary should exist")
            .contains(&raw_body));
        let details = error
            .provider_details
            .as_ref()
            .expect("upstream error should carry details");
        assert_eq!(
            details,
            &json!({ "status": 400, "request_id": "req_plain" })
        );
        let encoded = serde_json::to_string(&error).unwrap();
        assert!(!encoded.contains(&raw_body));
        assert!(!encoded.contains("sk-secret"));
        assert!(!encoded.contains("session=secret"));
    }

    #[test]
    fn responses_body_maps_native_tool_calls_and_tool_results() {
        let input = ProviderInvocationInput {
            contract_version: ProviderInvocationContractVersion::Current,
            model: "gpt-5.1".to_string(),
            previous_response_id: Some("resp_previous".to_string()),
            messages: vec![
                ProviderMessage {
                    role: ProviderMessageRole::Assistant,
                    content: String::new(),
                    content_blocks: None,
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: Some(
                        json!([{ "id": "call_1", "name": "lookup", "arguments": { "query": "refund" }}]),
                    ),
                },
                ProviderMessage {
                    role: ProviderMessageRole::Tool,
                    content: "found".to_string(),
                    content_blocks: None,
                    name: None,
                    tool_call_id: Some("call_1".to_string()),
                    is_error: None,
                    tool_calls: None,
                },
            ],
            tools: vec![json!({
                "type": "function",
                "function": {
                    "name": "lookup",
                    "description": "Lookup docs",
                    "parameters": { "type": "object" },
                    "strict": true
                }
            })],
            model_parameters: BTreeMap::from([
                ("response_format".to_string(), json!("json_object")),
                ("reasoning_effort".to_string(), json!("xhigh")),
                ("tool_choice".to_string(), json!("required")),
                ("parallel_tool_calls".to_string(), json!(true)),
                (
                    "include".to_string(),
                    json!(["reasoning.encrypted_content"]),
                ),
                ("prompt_cache_key".to_string(), json!("thread_1")),
            ]),
            ..Default::default()
        };

        let body = build_responses_body(&input).unwrap();
        assert_eq!(body["model"], "gpt-5.1");
        assert_eq!(body["previous_response_id"], "resp_previous");
        assert_eq!(body["tools"][0]["name"], "lookup");
        assert_eq!(body["tools"][0]["strict"], true);
        assert_eq!(body["tool_choice"], "required");
        assert_eq!(body["reasoning"], json!({ "effort": "xhigh" }));
        assert_eq!(body["text"]["format"], json!({ "type": "json_object" }));
        assert_eq!(body["parallel_tool_calls"], true);
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["prompt_cache_key"], "thread_1");
        assert_eq!(body["input"][0]["type"], "function_call");
        assert_eq!(body["input"][0]["call_id"], "call_1");
        assert_eq!(body["input"][0]["arguments"], r#"{"query":"refund"}"#);
        assert_eq!(body["input"][1]["type"], "function_call_output");
        assert_eq!(body["input"][1]["call_id"], "call_1");
    }

    #[test]
    fn ac_002_current_generate_input_reaches_responses_renderer_without_projection() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-openai",
            "provider_code": "openai",
            "protocol": "openai_responses",
            "model": "gpt-5.4-mini",
            "previous_response_id": null,
            "provider_config": {},
            "messages": [{
                "role": "user",
                "content": "hello",
                "name": null,
                "tool_call_id": null,
                "is_error": null,
                "tool_calls": null,
                "content_blocks": null
            }],
            "system": [{
                "type": "text",
                "text": "D1 seed instructions"
            }],
            "tools": [],
            "mcp_bindings": [],
            "response_format": null,
            "model_parameters": { "max_output_tokens": 512 },
            "client_protocol_envelope": null,
            "trace_context": {},
            "run_context": {}
        }))
        .expect("D1 current ProviderInvocationInput must deserialize directly");

        let body = build_responses_body(&input).expect("Responses body should render");

        assert_eq!(body["instructions"], json!("D1 seed instructions"));
        assert_eq!(body["max_output_tokens"], json!(512));
        assert!(body.get("max_tokens").is_none());
    }

    #[tokio::test]
    async fn ac_002_fake_upstream_receives_exact_openai_generate_wire() {
        let (base_url, request_rx) = start_generate_sse_server();
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-openai",
            "provider_code": "openai",
            "protocol": "openai_responses",
            "model": "gpt-5.4-mini",
            "provider_config": {
                "base_url": base_url,
                "api_key": "wire-secret",
                "transport_mode": "http_sse"
            },
            "messages": [{ "role": "user", "content": "wire prompt" }],
            "system": [{ "type": "text", "text": "wire instructions" }],
            "model_parameters": { "max_output_tokens": 128 }
        }))
        .unwrap();

        OpenAiProviderRuntime::default()
            .invoke_response(input)
            .await
            .expect("current Generate should complete against fake upstream");
        let request = request_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("fake upstream should capture Generate request");
        let (headers, body) = request
            .split_once("\r\n\r\n")
            .expect("captured request should contain headers and body");
        let body: Value = serde_json::from_str(body).unwrap();

        assert!(headers.starts_with("POST /responses HTTP/1.1"));
        assert!(headers
            .to_ascii_lowercase()
            .contains("authorization: bearer wire-secret"));
        assert_eq!(
            body,
            json!({
                "model": "gpt-5.4-mini",
                "input": [{ "role": "user", "content": "wire prompt" }],
                "stream": true,
                "instructions": "wire instructions",
                "max_output_tokens": 128
            })
        );
    }

    #[test]
    fn ac_002_current_generate_input_rejects_missing_legacy_and_unknown_contract_shapes() {
        let error = serde_json::from_value::<ProviderInvocationInput>(json!({
            "model": "gpt-5.4-mini",
            "provider_config": {
                "base_url": "http://127.0.0.1:9",
                "api_key": "test-key"
            },
            "messages": [{ "role": "user", "content": "hello" }]
        }))
        .expect_err("missing current contract must fail before provider invocation");

        assert!(error.to_string().contains("contract_version"));

        let legacy = serde_json::from_value::<ProviderInvocationInput>(json!({
            "contract_version": "1flowbase.provider/v1",
            "provider_instance_id": "provider-openai",
            "provider_code": "openai",
            "protocol": "openai_responses",
            "model": "gpt-5.4-mini"
        }))
        .expect_err("legacy provider contract must be rejected");
        assert!(legacy.to_string().contains("1flowbase.provider/v1"));

        let unknown = serde_json::from_value::<ProviderInvocationInput>(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-openai",
            "provider_code": "openai",
            "protocol": "openai_responses",
            "model": "gpt-5.4-mini",
            "raw_body": "must-not-be-accepted"
        }))
        .expect_err("unknown current contract fields must be rejected");
        assert!(unknown.to_string().contains("raw_body"));
    }

    #[test]
    fn ac_002_package_manifest_declares_only_current_generate_contract() {
        let manifest = include_str!("../manifest.yaml");

        assert!(manifest.contains("contract_version: 1flowbase.provider/v2"));
        assert!(!manifest.contains("1flowbase.provider/v1"));
        assert!(!manifest.contains("capabilities:"));
    }

    #[test]
    fn ac_002_openai_rejects_undeclared_generate_capabilities_without_projection() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-openai",
            "provider_code": "openai",
            "protocol": "openai_responses",
            "model": "gpt-5.4-mini",
            "system": [{
                "type": "text",
                "text": "must preserve cache policy",
                "cache_control": { "type": "ephemeral" }
            }],
            "required_capabilities": [
                "system_prompt_blocks",
                "system_prompt_cache_control"
            ]
        }))
        .unwrap();

        let error = build_responses_body(&input)
            .expect_err("undeclared Generate capabilities must not be projected away");

        assert!(error.to_string().contains("semantic capabilities"));
    }

    #[test]
    fn responses_body_preserves_media_content_blocks() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-openai",
            "provider_code": "openai",
            "protocol": "openai_responses",
            "model": "gpt-5.1",
            "messages": [
                {
                    "role": "user",
                    "content": "Describe image",
                    "content_blocks": [
                        {"type": "text", "text": "Describe image"},
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/png",
                                "data": "aW1hZ2U="
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_read",
                    "content": "",
                    "content_blocks": [
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/png",
                                "data": "cmVzdWx0"
                            }
                        }
                    ]
                }
            ]
        }))
        .unwrap();

        let body = build_responses_body(&input).unwrap();

        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["input"][0]["content"][1]["type"], "input_image");
        assert_eq!(
            body["input"][0]["content"][1]["image_url"],
            "data:image/png;base64,aW1hZ2U="
        );
        assert_eq!(body["input"][1]["type"], "function_call_output");
        assert_eq!(body["input"][1]["output"][0]["type"], "input_image");
        assert_eq!(
            body["input"][1]["output"][0]["image_url"],
            "data:image/png;base64,cmVzdWx0"
        );
    }

    #[test]
    fn responses_body_accepts_native_image_source_data_without_type() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-openai",
            "provider_code": "openai",
            "protocol": "openai_responses",
            "model": "gpt-5.1",
            "messages": [
                {
                    "role": "tool",
                    "tool_call_id": "call_read",
                    "content": "",
                    "content_blocks": [
                        {
                            "type": "image",
                            "source": {
                                "data": "cmVzdWx0"
                            }
                        }
                    ]
                }
            ]
        }))
        .unwrap();

        let body = build_responses_body(&input).unwrap();

        assert_eq!(body["input"][0]["type"], "function_call_output");
        assert_eq!(body["input"][0]["output"][0]["type"], "input_image");
        assert_eq!(
            body["input"][0]["output"][0]["image_url"],
            "data:image/png;base64,cmVzdWx0"
        );
    }

    #[test]
    fn finalized_response_exposes_native_response_id() {
        let mut emitted = Vec::new();
        let envelope = finalize_response_stream(
            Vec::new(),
            "hello".to_string(),
            Vec::new(),
            ProviderUsage::default(),
            ProviderFinishReason::Stop,
            json!("resp_current"),
            json!({ "transport": "http_sse" }),
            &mut |event| {
                emitted.push(event.clone());
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(envelope.result.response_id.as_deref(), Some("resp_current"));
        assert_eq!(
            envelope.result.provider_metadata["response_id"],
            json!("resp_current")
        );
        assert!(emitted.iter().any(|event| {
            matches!(
                event,
                ProviderStreamEvent::Finish {
                    reason: ProviderFinishReason::Stop
                }
            )
        }));
    }

    #[test]
    fn response_completed_commits_function_calls() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        process_response_sse_line(
            r#"data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"query\":\"refund\"}"}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();
        process_response_sse_line(
            r#"data: {"type":"response.completed","response":{"id":"resp_1","output":[{"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"query\":\"refund\"}"}],"usage":{"input_tokens":2,"output_tokens":3,"total_tokens":5}}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();

        assert_eq!(response_id, json!("resp_1"));
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "lookup");
        assert_eq!(tool_calls[0].arguments["query"], "refund");
        assert_eq!(usage.total_tokens, Some(5));
        assert_eq!(finish_reason, ProviderFinishReason::ToolCall);
    }

    #[test]
    fn response_function_arguments_done_merges_output_item_alias() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        process_response_sse_line(
            r#"data: {"type":"response.output_item.added","item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"Bash","arguments":""}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();
        process_response_sse_line(
            r#"data: {"type":"response.function_call_arguments.done","item_id":"fc_1","arguments":"{\"command\":\"pwd\"}" }"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();

        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "Bash");
        assert_eq!(tool_calls[0].arguments["command"], "pwd");
    }

    #[test]
    fn response_function_arguments_done_without_alias_waits_for_complete_item() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        process_response_sse_line(
            r#"data: {"type":"response.function_call_arguments.done","item_id":"fc_1","arguments":"{\"command\":\"pwd\"}" }"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();
        assert!(tool_calls.is_empty());

        process_response_sse_line(
            r#"data: {"type":"response.output_item.done","item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"Bash","arguments":"{\"command\":\"pwd\"}"}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();

        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "Bash");
        assert_eq!(tool_calls[0].arguments["command"], "pwd");
    }

    #[test]
    fn response_output_item_added_without_name_waits_for_stable_identity() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        process_response_sse_line(
            r#"data: {"type":"response.output_item.added","item":{"type":"function_call","id":"fc_1","call_id":"call_1","arguments":""}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();
        assert!(tool_calls.is_empty());

        process_response_sse_line(
            r#"data: {"type":"response.function_call_arguments.done","item_id":"fc_1","call_id":"call_1","arguments":"{\"command\":\"pwd\"}" }"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();
        assert!(tool_calls.is_empty());

        process_response_sse_line(
            r#"data: {"type":"response.output_item.done","item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"Bash","arguments":"{\"command\":\"pwd\"}"}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "Bash");
    }

    #[test]
    fn response_done_completes_websocket_stream() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        process_response_sse_payload(
            r#"{"type":"response.done","response":{"id":"resp_ws","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"OK"}]}],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();

        assert_eq!(response_id, json!("resp_ws"));
        assert_eq!(text, "OK");
        assert_eq!(usage.total_tokens, Some(2));
        assert_eq!(finish_reason, ProviderFinishReason::Stop);
    }

    #[test]
    fn response_done_failed_status_returns_provider_error() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        let error = process_response_sse_payload(
            r#"{"type":"response.done","response":{"id":"resp_ws","status":"failed","error":{"code":"server_error","message":"upstream closed"}}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "server_error: upstream closed");
    }

    #[test]
    fn response_created_and_text_delta_can_finalize_on_transport_close_without_terminal_event() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        process_response_sse_payload(
            r#"{"type":"response.created","response":{"id":"resp_ws"}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();
        process_response_sse_payload(
            r#"{"type":"response.output_text.delta","delta":"OK"}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();

        assert_eq!(response_id, json!("resp_ws"));
        assert!(!response_stream_finished(&finish_reason));
        assert!(can_finalize_response_on_close(
            &response_id,
            &text,
            &tool_calls
        ));
        finish_reason = close_finalized_finish_reason(&tool_calls);
        assert_eq!(finish_reason, ProviderFinishReason::Stop);
    }

    #[test]
    fn response_text_delta_captures_top_level_response_id() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        process_response_sse_payload(
            r#"{"type":"response.output_text.delta","response_id":"resp_delta","delta":"OK"}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();

        assert_eq!(response_id, json!("resp_delta"));
        assert_eq!(text, "OK");
        assert!(can_finalize_response_on_close(
            &response_id,
            &text,
            &tool_calls
        ));
    }

    #[test]
    fn response_created_without_content_does_not_finalize_on_transport_close() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        process_response_sse_payload(
            r#"{"type":"response.created","response":{"id":"resp_ws"}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();

        assert_eq!(response_id, json!("resp_ws"));
        assert!(!response_stream_finished(&finish_reason));
        assert!(!can_finalize_response_on_close(
            &response_id,
            &text,
            &tool_calls
        ));
    }

    #[test]
    fn response_failed_event_returns_error() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        let error = process_response_sse_line(
            r#"data: {"type":"response.failed","response":{"error":{"code":"insufficient_quota","message":"quota exceeded"}}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap_err();

        assert!(error.to_string().contains("quota exceeded"));
    }

    #[test]
    fn response_stream_maps_reasoning_and_custom_tool_delta() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        process_response_sse_line(
            r#"data: {"type":"response.reasoning_text.delta","content_index":0,"delta":"thinking"}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();
        process_response_sse_line(
            r#"data: {"type":"response.custom_tool_call_input.delta","item_id":"call_custom","call_id":"call_custom","delta":"{\"cmd\":\"pwd\"}"}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::ReasoningDelta {
                    delta: "thinking".to_string()
                },
                ProviderStreamEvent::ToolCallDelta {
                    call_id: "call_custom".to_string(),
                    delta: json!("{\"cmd\":\"pwd\"}")
                }
            ]
        );
    }

    #[test]
    fn response_incomplete_event_returns_error() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        let error = process_response_sse_line(
            r#"data: {"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "response.incomplete: max_output_tokens");
    }

    #[test]
    fn response_output_item_done_supplies_text_fallback() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = ResponseToolCalls::default();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = ProviderFinishReason::Unknown;
        let mut response_id = Value::Null;

        process_response_sse_line(
            r#"data: {"type":"response.output_item.done","item":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hello"},{"type":"output_text","text":" world"}]}}"#,
            &mut events,
            &mut text,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
        )
        .unwrap();

        assert_eq!(text, "Hello world");
    }

    #[test]
    fn custom_tool_call_done_maps_to_provider_tool_call() {
        let call = provider_tool_call_from_response_item(Some(&json!({
            "type": "custom_tool_call",
            "call_id": "call_custom",
            "name": "shell",
            "input": "{\"cmd\":\"pwd\"}"
        })))
        .unwrap();

        assert_eq!(
            call,
            ProviderToolCall {
                id: "call_custom".to_string(),
                name: "shell".to_string(),
                arguments: json!({ "cmd": "pwd" })
            }
        );
    }

    #[test]
    fn stream_headers_request_event_stream() {
        let config = ProviderConfig {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "sk-test".to_string(),
            organization: None,
            project: None,
            validate_model: false,
            transport_mode: OpenAiTransportMode::Auto,
            proxy_url: None,
        };

        let headers = build_stream_headers(&config, None).unwrap();

        assert_eq!(
            headers.get(ACCEPT).and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );
    }

    #[test]
    fn client_protocol_envelope_uses_default_deny_policy_for_http_headers() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-openai",
            "provider_code": "openai",
            "protocol": "openai_responses",
            "model": "gpt-5.1",
            "client_protocol_envelope": {
                "source_protocol": "openai_responses",
                "policy": "default_deny",
                "headers": {
                    "authorization": "Bearer client-secret",
                    "x-api-key": "client-api-key",
                    "openai-beta": "client-beta",
                    "x-client-name": "ClaudeCode",
                    "host": "evil.example"
                }
            }
        }))
        .unwrap();
        let config = ProviderConfig {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "sk-provider".to_string(),
            organization: None,
            project: None,
            validate_model: false,
            transport_mode: OpenAiTransportMode::HttpSse,
            proxy_url: None,
        };

        assert!(input.client_protocol_envelope.is_some());

        let headers =
            build_json_headers(&config, true, input.client_protocol_envelope.as_ref()).unwrap();

        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer sk-provider");
        assert!(headers.get("x-api-key").is_none());
        assert!(headers.get("openai-beta").is_none());
        assert!(headers.get("x-client-name").is_none());
        assert!(headers.get("host").is_none());
    }

    #[test]
    fn headers_restore_anthropic_client_protocol_envelope_and_keep_config_auth() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-openai",
            "provider_code": "openai",
            "protocol": "openai_responses",
            "model": "gpt-5.1",
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
        let config = ProviderConfig {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "sk-provider".to_string(),
            organization: None,
            project: None,
            validate_model: false,
            transport_mode: OpenAiTransportMode::HttpSse,
            proxy_url: None,
        };

        let headers =
            build_json_headers(&config, true, input.client_protocol_envelope.as_ref()).unwrap();

        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer sk-provider");
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
    fn default_transport_mode_is_http_sse() {
        let config = normalize_provider_config(&json!({
            "api_key": "sk-test"
        }))
        .unwrap();

        assert_eq!(config.transport_mode, OpenAiTransportMode::HttpSse);
    }

    #[test]
    fn explicit_auto_transport_mode_stays_available() {
        let config = normalize_provider_config(&json!({
            "api_key": "sk-test",
            "transport_mode": "auto"
        }))
        .unwrap();

        assert_eq!(config.transport_mode, OpenAiTransportMode::Auto);
    }

    #[test]
    fn websocket_url_maps_responses_https_to_wss() {
        let config = ProviderConfig {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "sk-test".to_string(),
            organization: None,
            project: None,
            validate_model: false,
            transport_mode: OpenAiTransportMode::ResponsesWebsocket,
            proxy_url: None,
        };

        assert_eq!(
            build_websocket_url(&config).unwrap().as_str(),
            "wss://api.openai.com/v1/responses"
        );
    }

    #[tokio::test]
    async fn ac_004_websocket_connects_through_configured_proxy_url() {
        let (proxy_url, request_rx, handle) = start_websocket_connect_proxy();
        let config = ProviderConfig {
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key: "sk-test".to_string(),
            organization: None,
            project: None,
            validate_model: false,
            transport_mode: OpenAiTransportMode::ResponsesWebsocket,
            proxy_url: Some(proxy_url),
        };

        let session = connect_responses_websocket(&config, None, None)
            .await
            .expect("websocket should connect through configured proxy");

        assert_eq!(session.turn_state.as_deref(), Some("proxy-turn"));
        drop(session);
        let connect_request = request_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("proxy should observe CONNECT request");
        assert!(
            connect_request.starts_with("CONNECT 127.0.0.1:9 HTTP/1.1"),
            "proxy should receive CONNECT target, got: {connect_request}"
        );
        handle.join().expect("proxy thread should finish");
    }

    #[test]
    fn websocket_body_wraps_response_create_and_optional_previous_response() {
        let body = json!({
            "model": "gpt-5.1",
            "stream": true,
            "input": [],
            "previous_response_id": "resp_previous"
        });

        let websocket_body = build_websocket_response_create_body(body);

        assert_eq!(websocket_body["type"], "response.create");
        assert_eq!(websocket_body["model"], "gpt-5.1");
        assert_eq!(websocket_body["stream"], true);
        assert_eq!(websocket_body["previous_response_id"], "resp_previous");
    }

    #[test]
    fn websocket_headers_request_responses_beta_without_content_type() {
        let config = ProviderConfig {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "sk-test".to_string(),
            organization: None,
            project: None,
            validate_model: false,
            transport_mode: OpenAiTransportMode::ResponsesWebsocket,
            proxy_url: None,
        };

        let headers = build_websocket_headers(&config, None, None).unwrap();

        assert_eq!(
            headers
                .get(HeaderName::from_static("openai-beta"))
                .and_then(|value| value.to_str().ok()),
            Some(RESPONSES_WEBSOCKETS_BETA)
        );
        assert!(headers.get(CONTENT_TYPE).is_none());
    }

    #[test]
    fn websocket_headers_replay_turn_state_when_available() {
        let config = ProviderConfig {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "sk-test".to_string(),
            organization: None,
            project: None,
            validate_model: false,
            transport_mode: OpenAiTransportMode::ResponsesWebsocket,
            proxy_url: None,
        };

        let headers = build_websocket_headers(&config, Some("sticky-turn-1"), None).unwrap();

        assert_eq!(
            headers
                .get(HeaderName::from_static(X_CODEX_TURN_STATE_HEADER))
                .and_then(|value| value.to_str().ok()),
            Some("sticky-turn-1")
        );
    }

    #[test]
    fn websocket_headers_keep_internal_beta_and_turn_state_over_client_envelope() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-openai",
            "provider_code": "openai",
            "protocol": "openai_responses",
            "model": "gpt-5.1",
            "client_protocol_envelope": {
                "source_protocol": "openai_responses",
                "policy": "default_deny",
                "headers": {
                    "authorization": "Bearer client-secret",
                    "openai-beta": "client-beta",
                    "x-codex-turn-state": "client-turn-state"
                }
            }
        }))
        .unwrap();
        let config = ProviderConfig {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: "sk-provider".to_string(),
            organization: None,
            project: None,
            validate_model: false,
            transport_mode: OpenAiTransportMode::ResponsesWebsocket,
            proxy_url: None,
        };

        let headers = build_websocket_headers(
            &config,
            Some("sticky-turn-1"),
            input.client_protocol_envelope.as_ref(),
        )
        .unwrap();

        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer sk-provider");
        assert_eq!(
            headers
                .get(HeaderName::from_static("openai-beta"))
                .and_then(|value| value.to_str().ok()),
            Some(RESPONSES_WEBSOCKETS_BETA)
        );
        assert_eq!(
            headers
                .get(HeaderName::from_static(X_CODEX_TURN_STATE_HEADER))
                .and_then(|value| value.to_str().ok()),
            Some("sticky-turn-1")
        );
    }

    #[test]
    fn websocket_lifecycle_frame_does_not_block_http_fallback() {
        assert!(!websocket_payload_blocks_http_fallback(
            r#"{"type":"response.created","response":{"id":"resp_1"}}"#
        ));
        assert!(!websocket_payload_blocks_http_fallback(
            r#"{"type":"error","status":426}"#
        ));
        assert!(websocket_payload_blocks_http_fallback(
            r#"{"type":"response.failed","response":{"error":{"message":"failed"}}}"#
        ));
        assert!(websocket_payload_blocks_http_fallback(
            r#"{"type":"response.done","response":{"status":"failed","error":{"message":"failed"}}}"#
        ));
    }
}
