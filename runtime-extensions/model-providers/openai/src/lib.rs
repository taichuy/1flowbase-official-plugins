use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    Method, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Error as WebSocketError, Message},
    MaybeTlsStream, WebSocketStream,
};

const PROVIDER_CODE: &str = "openai";
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_VALIDATE_MODEL: bool = true;
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_millis(300_000);
const RESPONSES_WEBSOCKETS_BETA: &str = "responses_websockets=2026-02-06";
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
    validate_model: bool,
    transport_mode: OpenAiTransportMode,
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
    pub trace_context: BTreeMap<String, String>,
    #[serde(default)]
    pub run_context: BTreeMap<String, Value>,
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
    ProviderInvalidResponse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderRuntimeError {
    pub kind: ProviderRuntimeErrorKind,
    pub message: String,
    pub provider_summary: Option<String>,
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
        }
    }
}

#[derive(Debug, Default)]
pub struct OpenAiProviderRuntime {
    websocket_sessions: HashMap<String, ResponsesWebsocketSession>,
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
    })
}

fn normalize_transport_mode(value: Option<&Value>) -> Result<OpenAiTransportMode> {
    let Some(value) = value else {
        return Ok(OpenAiTransportMode::Auto);
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

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn build_json_headers(config: &ProviderConfig, include_json_body: bool) -> Result<HeaderMap> {
    build_headers(config, include_json_body, "application/json")
}

fn build_stream_headers(config: &ProviderConfig) -> Result<HeaderMap> {
    build_headers(config, true, "text/event-stream")
}

fn build_headers(
    config: &ProviderConfig,
    include_json_body: bool,
    accept: &'static str,
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
    Ok(headers)
}

fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .build()
        .context("building OpenAI HTTP client")
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
    let client = build_http_client()?;
    let mut request = client
        .request(method, build_url(config, pathname)?)
        .headers(build_json_headers(config, body.is_some())?);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await?;
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
            .unwrap_or_else(|| payload.as_str().unwrap_or("provider request failed"));
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
                invoke_response_http_sse(&config, body, input.model, &mut on_event).await
            }
            OpenAiTransportMode::ResponsesWebsocket => self
                .invoke_response_websocket(&config, &input, body, &mut on_event)
                .await
                .map_err(|error| error.source),
            OpenAiTransportMode::Auto => {
                let websocket_body = body.clone();
                match self
                    .invoke_response_websocket(&config, &input, websocket_body, &mut on_event)
                    .await
                {
                    Ok(output) => Ok(output),
                    Err(error) if !error.event_started && can_fallback_to_http(&error.source) => {
                        invoke_response_http_sse(&config, body, input.model, &mut on_event).await
                    }
                    Err(error) => Err(error.source),
                }
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
            let session = connect_responses_websocket(config)
                .await
                .map_err(WebsocketInvocationError::before_events)?;
            self.websocket_sessions.insert(session_key.clone(), session);
        }

        let session = self
            .websocket_sessions
            .get_mut(&session_key)
            .expect("websocket session should be initialized");
        let mut request_body = build_websocket_response_create_body(body);
        let result =
            read_websocket_response(session, &mut request_body, input.model.clone(), on_event)
                .await;

        match result {
            Ok(output) => Ok(output),
            Err(error) => {
                self.websocket_sessions.remove(&session_key);
                Err(error)
            }
        }
    }
}

async fn invoke_response_http_sse<F>(
    config: &ProviderConfig,
    body: Value,
    request_model: String,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let response = build_http_client()?
        .request(Method::POST, build_url(config, "/responses")?)
        .headers(build_stream_headers(config)?)
        .json(&body)
        .send()
        .await?;
    read_streaming_response(response, request_model, on_event).await
}

fn build_responses_body(input: &ProviderInvocationInput) -> Result<Value> {
    if input.model.trim().is_empty() {
        bail!("model is required");
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
    if let Some(system) = input
        .system
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        body.insert(
            "instructions".to_string(),
            Value::String(system.to_string()),
        );
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

fn build_responses_input(input: &ProviderInvocationInput) -> Vec<Value> {
    let mut items = Vec::new();
    for message in &input.messages {
        let role = message.role.trim().to_ascii_lowercase();
        if role == "tool" {
            if let Some(call_id) = message.tool_call_id.as_deref() {
                items.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": normalize_message_content(&message.content),
                }));
            }
            continue;
        }
        let content = normalize_message_content(&message.content);
        if !content.is_empty() {
            items.push(json!({
                "role": normalize_responses_role(&role),
                "content": content,
            }));
        }
        append_response_function_calls(&mut items, message.tool_calls.as_ref());
    }
    items
}

fn normalize_responses_role(role: &str) -> &str {
    match role {
        "system" => "developer",
        "developer" => "developer",
        "assistant" => "assistant",
        _ => "user",
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

fn parameter_value(input: &ProviderInvocationInput, key: &str) -> Option<Value> {
    input
        .model_parameters
        .get(key)
        .cloned()
        .or_else(|| input.extra.get(key).cloned())
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
}

#[derive(Debug)]
struct WebsocketInvocationError {
    source: anyhow::Error,
    event_started: bool,
}

impl WebsocketInvocationError {
    fn before_events(source: anyhow::Error) -> Self {
        Self {
            source,
            event_started: false,
        }
    }

    fn after_events(source: anyhow::Error) -> Self {
        Self {
            source,
            event_started: true,
        }
    }
}

async fn connect_responses_websocket(config: &ProviderConfig) -> Result<ResponsesWebsocketSession> {
    let url = build_websocket_url(config)?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|error| anyhow!("failed to build websocket request: {error}"))?;
    request
        .headers_mut()
        .extend(build_websocket_headers(config)?);
    let (stream, _response) = connect_async(request)
        .await
        .map_err(|error| anyhow!("failed to connect Responses websocket: {error}"))?;
    Ok(ResponsesWebsocketSession { stream })
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

fn build_websocket_headers(config: &ProviderConfig) -> Result<HeaderMap> {
    let mut headers = build_headers(config, false, "application/json")?;
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

fn build_response_processed_body(response_id: &str) -> Value {
    json!({
        "type": "response.processed",
        "response_id": response_id,
    })
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

async fn send_websocket_json(session: &mut ResponsesWebsocketSession, body: &Value) -> Result<()> {
    let payload = serde_json::to_string(body)?;
    session
        .stream
        .send(Message::Text(payload.into()))
        .await
        .map_err(map_websocket_error)
}

async fn read_websocket_response<F>(
    session: &mut ResponsesWebsocketSession,
    request_body: &mut Value,
    request_model: String,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope, WebsocketInvocationError>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    send_websocket_json(session, request_body)
        .await
        .map_err(WebsocketInvocationError::before_events)?;

    let mut events = Vec::new();
    let mut all_events = Vec::new();
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut usage = ProviderUsage::default();
    let mut finish_reason = ProviderFinishReason::Unknown;
    let mut response_id = Value::Null;
    let mut event_started = false;

    loop {
        let message = tokio::time::timeout(STREAM_IDLE_TIMEOUT, session.stream.next())
            .await
            .map_err(|_| {
                WebsocketInvocationError::after_events(anyhow!(
                    "idle timeout waiting for Responses websocket"
                ))
            })?
            .ok_or_else(|| {
                WebsocketInvocationError::after_events(anyhow!(
                    "websocket closed before response.completed"
                ))
            })?
            .map_err(|error| {
                if event_started {
                    WebsocketInvocationError::after_events(map_websocket_error(error))
                } else {
                    WebsocketInvocationError::before_events(map_websocket_error(error))
                }
            })?;

        match message {
            Message::Text(payload) => {
                let payload = payload.as_str();
                if let Some(message) = websocket_error_message(payload) {
                    let error = anyhow!(message);
                    return Err(if event_started {
                        WebsocketInvocationError::after_events(error)
                    } else {
                        WebsocketInvocationError::before_events(error)
                    });
                }
                if websocket_payload_started_response(payload) {
                    event_started = true;
                }
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
                    if event_started {
                        WebsocketInvocationError::after_events(error)
                    } else {
                        WebsocketInvocationError::before_events(error)
                    }
                })?;
                if !events.is_empty() {
                    event_started = true;
                    emit_new_events(&events, on_event)
                        .map_err(WebsocketInvocationError::after_events)?;
                    all_events.extend(events.drain(..));
                }
                if !response_id.is_null() {
                    break;
                }
            }
            Message::Ping(payload) => {
                session
                    .stream
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| {
                        WebsocketInvocationError::before_events(map_websocket_error(error))
                    })?;
            }
            Message::Pong(_) => {}
            Message::Close(_) => {
                return Err(WebsocketInvocationError::after_events(anyhow!(
                    "websocket closed by server before response.completed"
                )));
            }
            Message::Binary(_) | Message::Frame(_) => {}
        }
    }

    if let Some(response_id_text) = response_id.as_str() {
        let _ =
            send_websocket_json(session, &build_response_processed_body(response_id_text)).await;
    }

    finalize_response_stream(
        all_events,
        text,
        tool_calls,
        usage,
        finish_reason,
        response_id,
        json!({
            "request_model": request_model,
            "transport": "responses_websocket",
        }),
        on_event,
    )
    .map_err(WebsocketInvocationError::after_events)
}

fn map_websocket_error(error: WebSocketError) -> anyhow::Error {
    anyhow!("Responses websocket error: {error}")
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

fn websocket_payload_started_response(payload: &str) -> bool {
    serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .is_some_and(|event_type| event_type.starts_with("response."))
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
        let payload = read_json_response(response).await?;
        ensure_success_status(status, &payload)?;
        bail!("{} {}: provider request failed", status.as_u16(), status);
    }
    let headers = response.headers().clone();
    let upstream_request_id = header_text(&headers, "x-request-id");
    let upstream_model =
        header_text(&headers, "openai-model").or_else(|| header_text(&headers, "x-openai-model"));
    let models_etag = header_text(&headers, "x-models-etag");
    let mut events = Vec::new();
    let mut all_events = Vec::new();
    let mut text = String::new();
    let mut tool_calls = Vec::new();
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
            all_events.extend(events.drain(..));
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
        all_events.extend(events.drain(..));
    }
    if response_id.is_null() {
        bail!("stream closed before response.completed");
    }
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
    Ok(RuntimeInvocationEnvelope {
        events: all_events,
        result: ProviderInvocationResult {
            final_content: (!text.is_empty()).then_some(text),
            response_id: native_response_id,
            tool_calls,
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
    tool_calls: &mut Vec<ProviderToolCall>,
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
    tool_calls: &mut Vec<ProviderToolCall>,
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
    tool_calls: &mut Vec<ProviderToolCall>,
    usage: &mut ProviderUsage,
    finish_reason: &mut ProviderFinishReason,
    response_id: &mut Value,
) -> Result<()> {
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let payload: Value = serde_json::from_str(data)?;
    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match event_type {
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
        "response.output_item.done" => {
            if let Some(call) = provider_tool_call_from_response_item(payload.get("item")) {
                upsert_tool_call(tool_calls, call);
            }
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

fn process_terminal_response_event(
    payload: &Value,
    text: &mut String,
    tool_calls: &mut Vec<ProviderToolCall>,
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
    if let Some(calls) = response
        .get("output")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| provider_tool_call_from_response_item(Some(item)))
                .collect::<Vec<_>>()
        })
    {
        for call in calls {
            upsert_tool_call(tool_calls, call);
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

    #[test]
    fn responses_body_maps_native_tool_calls_and_tool_results() {
        let input = ProviderInvocationInput {
            model: "gpt-5.1".to_string(),
            previous_response_id: Some("resp_previous".to_string()),
            messages: vec![
                ProviderMessage {
                    role: "assistant".to_string(),
                    content: Value::Null,
                    name: None,
                    tool_call_id: None,
                    tool_calls: Some(
                        json!([{ "id": "call_1", "name": "lookup", "arguments": { "query": "refund" }}]),
                    ),
                },
                ProviderMessage {
                    role: "tool".to_string(),
                    content: json!("found"),
                    name: None,
                    tool_call_id: Some("call_1".to_string()),
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
                ("reasoning_effort".to_string(), json!("medium")),
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
        assert_eq!(body["reasoning"], json!({ "effort": "medium" }));
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
        let mut tool_calls = Vec::new();
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
    fn response_done_completes_websocket_stream() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = Vec::new();
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
        let mut tool_calls = Vec::new();
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
    fn response_failed_event_returns_error() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut tool_calls = Vec::new();
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
        let mut tool_calls = Vec::new();
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
        let mut tool_calls = Vec::new();
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
        let mut tool_calls = Vec::new();
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
        };

        let headers = build_stream_headers(&config).unwrap();

        assert_eq!(
            headers.get(ACCEPT).and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );
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
        };

        assert_eq!(
            build_websocket_url(&config).unwrap().as_str(),
            "wss://api.openai.com/v1/responses"
        );
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
        };

        let headers = build_websocket_headers(&config).unwrap();

        assert_eq!(
            headers
                .get(HeaderName::from_static("openai-beta"))
                .and_then(|value| value.to_str().ok()),
            Some(RESPONSES_WEBSOCKETS_BETA)
        );
        assert!(headers.get(CONTENT_TYPE).is_none());
    }

    #[test]
    fn websocket_response_created_marks_stream_started() {
        assert!(websocket_payload_started_response(
            r#"{"type":"response.created","response":{"id":"resp_1"}}"#
        ));
        assert!(!websocket_payload_started_response(
            r#"{"type":"error","status":426}"#
        ));
    }
}
