use std::collections::BTreeMap;

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
            let output = invoke_chat_completion(input).await?;
            Ok(ProviderStdioResponse::ok(serde_json::to_value(output)?))
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
        url.query_pairs_mut()
            .append_pair("api-version", api_version);
    }
    Ok(url.to_string())
}

async fn request_json(
    config: &ProviderConfig,
    pathname: &str,
    method: Method,
    body: Option<Value>,
) -> Result<Value> {
    let response = send_provider_request(config, pathname, method, body).await?;
    let status = response.status();
    let payload = read_json_response(response).await?;
    ensure_success_status(status, &payload)?;

    Ok(payload)
}

async fn send_provider_request(
    config: &ProviderConfig,
    pathname: &str,
    method: Method,
    body: Option<Value>,
) -> Result<reqwest::Response> {
    let client = reqwest::Client::new();
    let mut request = client
        .request(method.clone(), build_url(config, pathname)?)
        .headers(build_headers(config, body.is_some())?);
    if let Some(body) = body {
        request = request.json(&body);
    }

    request.send().await.map_err(Into::into)
}

fn ensure_success_status(status: reqwest::StatusCode, payload: &Value) -> Result<()> {
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
            .unwrap_or_else(|| payload.to_string());
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
    if let Some(system) = input
        .system
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
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
    body.insert("stream".to_string(), Value::Bool(true));
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

    let response = send_provider_request(
        &config,
        "/chat/completions",
        Method::POST,
        Some(Value::Object(body)),
    )
    .await?;
    read_streaming_chat_completion(response, input.model).await
}

async fn read_streaming_chat_completion(
    response: reqwest::Response,
    request_model: String,
) -> Result<RuntimeInvocationEnvelope> {
    let status = response.status();
    if !status.is_success() {
        let payload = read_json_response(response).await?;
        ensure_success_status(status, &payload)?;
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
        }
    }

    if !buffer.trim().is_empty() {
        let line = std::mem::take(&mut buffer);
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
    }

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

    Ok(RuntimeInvocationEnvelope {
        events,
        result: ProviderInvocationResult {
            final_content: (!text.is_empty()).then_some(text),
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
        thread,
        time::Duration,
    };

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

    #[tokio::test]
    async fn invoke_chat_completion_forwards_extended_chat_completion_parameters() {
        let (base_url, capture_handle) = capture_single_json_request();

        let envelope = invoke_chat_completion(ProviderInvocationInput {
            model: "gpt-4o-mini".to_string(),
            provider_config: json!({
                "base_url": base_url,
                "api_key": "test-key"
            }),
            messages: vec![ProviderMessage {
                role: "user".to_string(),
                content: json!("hello"),
            }],
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
}
