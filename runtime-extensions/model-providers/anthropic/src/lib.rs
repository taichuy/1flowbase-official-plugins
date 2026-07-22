use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

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
const PASSTHROUGH_MESSAGES_PARAMETERS: &[&str] = &["temperature", "top_p", "top_k", "tool_choice"];
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
    anthropic_version: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
pub enum ProviderInvocationContractVersion {
    #[default]
    #[serde(rename = "1flowbase.provider/v2")]
    Current,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativePromptBlock {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<NativePromptCacheControl>,
    },
}

#[cfg(test)]
impl NativePromptBlock {
    fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            cache_control: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePromptCacheControl {
    #[serde(rename = "type")]
    cache_type: NativePromptCacheControlType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ttl: Option<NativePromptCacheTtl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePromptCacheControlType {
    Ephemeral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    CountTokens,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderWireOperation {
    Generate,
    CountTokens,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCountTokensInput {
    pub operation: ProviderWireOperation,
    pub contract_version: ProviderInvocationContractVersion,
    pub provider_instance_id: String,
    pub provider_code: String,
    pub protocol: String,
    pub model: String,
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
    pub client_protocol_envelope: Option<ClientProtocolEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCountTokensResult {
    pub operation: ProviderWireOperation,
    pub input_tokens: u64,
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
        "validate" => validate_provider_config(&request.input).await,
        "list_models" => list_models(&request.input).await,
        "invoke"
            if request.input.get("operation").and_then(Value::as_str) == Some("count_tokens") =>
        {
            let input: ProviderCountTokensInput = serde_json::from_value(request.input)?;
            let output = count_message_tokens(input).await?;
            Ok(ProviderStdioResponse::ok(serde_json::to_value(output)?))
        }
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
            "proxy_url": config.proxy_url.as_ref().map(|_| "***"),
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

fn build_http_client(config: &ProviderConfig) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy_url) = &config.proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy_url")?);
    }
    builder.build().context("building Anthropic HTTP client")
}

fn sanitize_reqwest_error(error: reqwest::Error, config: &ProviderConfig) -> anyhow::Error {
    let mut message = error.to_string().replace(&config.api_key, "***");
    if let Some(proxy_url) = &config.proxy_url {
        message = message.replace(proxy_url, "***");
    }
    anyhow!(message)
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
    let response = build_http_client(config)?
        .request(method, build_url_with_query(config, pathname, query)?)
        .headers(build_headers(config, None)?)
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, config))?;
    let status = response.status();
    if !status.is_success() {
        return Err(provider_upstream_error_from_response(response)
            .await?
            .into());
    }
    let payload = read_json_response(response).await?;
    Ok(payload)
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
    [
        "x-request-id",
        "request-id",
        "anthropic-request-id",
        "cf-ray",
    ]
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

async fn count_message_tokens(
    input: ProviderCountTokensInput,
) -> Result<ProviderCountTokensResult> {
    if input.operation != ProviderWireOperation::CountTokens {
        return Err(ProviderRuntimeError::normalize(
            "count_tokens",
            "CountTokens input must declare operation=count_tokens",
            None,
        )
        .into());
    }

    let config = normalize_provider_config(&input.provider_config)?;
    let body = build_count_tokens_body(&input)?;
    let response = build_http_client(&config)?
        .request(
            Method::POST,
            build_url(&config, "/v1/messages/count_tokens")?,
        )
        .headers(build_headers(
            &config,
            input.client_protocol_envelope.as_ref(),
        )?)
        .json(&body)
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, &config))?;
    if !response.status().is_success() {
        return Err(provider_upstream_error_from_response(response)
            .await?
            .into());
    }

    let payload = read_json_response(response).await.map_err(|error| {
        ProviderRuntimeError::normalize(
            "count_tokens_response",
            format!("Anthropic CountTokens response is malformed: {error}"),
            None,
        )
    })?;
    let input_tokens = payload
        .get("input_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            ProviderRuntimeError::normalize(
                "count_tokens_response",
                "Anthropic CountTokens response must include input_tokens as an unsigned integer",
                None,
            )
        })?;

    Ok(ProviderCountTokensResult {
        operation: ProviderWireOperation::CountTokens,
        input_tokens,
    })
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
    let effective_max_output_tokens = body
        .get("max_tokens")
        .and_then(Value::as_u64)
        .context("Anthropic request max_tokens must be an unsigned integer")?;
    let response = build_http_client(&config)?
        .request(Method::POST, build_url(&config, "/v1/messages")?)
        .headers(build_headers(
            &config,
            input.client_protocol_envelope.as_ref(),
        )?)
        .json(&body)
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, &config))?;
    read_streaming_message(
        response,
        input.model,
        effective_max_output_tokens,
        &mut on_event,
    )
    .await
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
        Value::Array(build_anthropic_messages(&input.messages)),
    );
    body.insert("stream".to_string(), Value::Bool(true));
    let max_output_tokens = parameter_u64(input, "max_output_tokens").unwrap_or(DEFAULT_MAX_TOKENS);
    body.insert("max_tokens".to_string(), json!(max_output_tokens));
    if !input.system.is_empty() {
        body.insert(
            "system".to_string(),
            serde_json::to_value(&input.system).context("serializing Native system blocks")?,
        );
    }
    if let Some(end_user_reference) = input
        .request_context
        .end_user_reference
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.insert(
            "metadata".to_string(),
            json!({ "user_id": end_user_reference }),
        );
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
            if budget_tokens >= max_output_tokens {
                bail!("thinking_budget_tokens must be lower than max_output_tokens");
            }
            body.insert(
                "thinking".to_string(),
                json!({ "type": "enabled", "budget_tokens": budget_tokens }),
            );
        }
    }
    for key in PASSTHROUGH_MESSAGES_PARAMETERS {
        if let Some(value) = parameter_value(input, key) {
            body.insert(
                (*key).to_string(),
                normalize_anthropic_parameter(key, value),
            );
        }
    }
    Ok(Value::Object(body))
}

fn build_count_tokens_body(input: &ProviderCountTokensInput) -> Result<Value> {
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
        Value::Array(build_anthropic_messages(&input.messages)),
    );
    if !input.system.is_empty() {
        body.insert(
            "system".to_string(),
            serde_json::to_value(&input.system).context("serializing Native system blocks")?,
        );
    }
    if let Some(end_user_reference) = input
        .request_context
        .end_user_reference
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.insert(
            "metadata".to_string(),
            json!({ "user_id": end_user_reference }),
        );
    }
    Ok(Value::Object(body))
}

fn build_anthropic_messages(input_messages: &[ProviderMessage]) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut consecutive_tool_results = Vec::new();
    for message in input_messages {
        if message.role == ProviderMessageRole::Tool {
            if let Some(tool_use_id) = message.tool_call_id.as_deref() {
                let mut tool_result = json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": tool_result_content(message),
                });
                if let Some(is_error) = message.is_error {
                    tool_result["is_error"] = Value::Bool(is_error);
                }
                consecutive_tool_results.push(tool_result);
            }
            continue;
        }
        if !consecutive_tool_results.is_empty() {
            messages.push(json!({
                "role": "user",
                "content": std::mem::take(&mut consecutive_tool_results),
            }));
        }
        if message.role == ProviderMessageRole::System {
            continue;
        }
        let mut content = message_content_blocks(message);
        append_tool_use_blocks(&mut content, message.tool_calls.as_ref());
        if !content.is_empty() {
            messages.push(json!({
                "role": if message.role == ProviderMessageRole::Assistant { "assistant" } else { "user" },
                "content": content,
            }));
        }
    }
    if !consecutive_tool_results.is_empty() {
        messages.push(json!({
            "role": "user",
            "content": consecutive_tool_results,
        }));
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
    (!message.content.is_empty())
        .then(|| json!({ "type": "text", "text": message.content.clone() }))
        .into_iter()
        .collect()
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
    Value::String(message.content.clone())
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
    effective_max_output_tokens: u64,
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
                "effective_max_output_tokens": effective_max_output_tokens,
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
        net::{TcpListener, TcpStream},
        sync::mpsc,
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
                "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_generate\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
                "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
                "data: [DONE]\n\n"
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

    fn start_http_error_server(
        status_line: &'static str,
        content_type: &'static str,
        response_body: &'static str,
    ) -> String {
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
                "{status_line}\r\ncontent-type: {content_type}\r\nx-request-id: req_stream\r\nx-api-key: response-secret\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
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

    fn count_tokens_invoke_request(base_url: &str) -> ProviderStdioRequest {
        ProviderStdioRequest {
            method: "invoke".to_string(),
            input: json!({
                "operation": "count_tokens",
                "contract_version": "1flowbase.provider/v2",
                "provider_instance_id": "provider-anthropic",
                "provider_code": "anthropic",
                "protocol": "anthropic_messages",
                "model": "claude-sonnet-4-20250514",
                "provider_config": {
                    "base_url": base_url,
                    "api_key": "wire-secret",
                    "anthropic_version": "2023-06-01"
                },
                "messages": [{ "role": "user", "content": "wire prompt" }],
                "system": [{ "type": "text", "text": "wire instructions" }],
                "request_context": { "end_user_reference": "wire-user" },
                "required_capabilities": [
                    "count_tokens",
                    "system_prompt_blocks",
                    "end_user_reference"
                ],
                "client_protocol_envelope": {
                    "source_protocol": "anthropic_messages",
                    "policy": "anthropic_messages_v1",
                    "headers": { "anthropic-beta": "prompt-caching" }
                }
            }),
        }
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
    fn ac_002_current_generate_input_reaches_messages_renderer_without_projection() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-anthropic",
            "provider_code": "anthropic",
            "protocol": "anthropic_messages",
            "model": "claude-fable-5",
            "messages": [
                { "role": "user", "content": "hello" }
            ],
            "system": [
                {
                    "type": "text",
                    "text": "Use Claude Code project instructions.",
                    "cache_control": { "type": "ephemeral" }
                },
                {
                    "type": "text",
                    "text": "，语言偏好中文"
                }
            ],
            "request_context": {
                "end_user_reference": "claude-code-user-123"
            },
            "required_capabilities": [
                "system_prompt_blocks",
                "system_prompt_cache_control",
                "end_user_reference"
            ]
        }))
        .unwrap();

        let body = build_messages_body(&input).unwrap();

        assert_eq!(
            body["system"],
            json!([
                {
                    "type": "text",
                    "text": "Use Claude Code project instructions.",
                    "cache_control": { "type": "ephemeral" }
                },
                {
                    "type": "text",
                    "text": "，语言偏好中文"
                }
            ])
        );
        assert_eq!(
            body["metadata"],
            json!({ "user_id": "claude-code-user-123" })
        );
    }

    #[tokio::test]
    async fn ac_002_fake_upstream_receives_exact_anthropic_generate_wire() {
        let (base_url, request_rx) = start_generate_sse_server();
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-anthropic",
            "provider_code": "anthropic",
            "protocol": "anthropic_messages",
            "model": "claude-sonnet-4-20250514",
            "provider_config": {
                "base_url": base_url,
                "api_key": "wire-secret",
                "anthropic_version": "2023-06-01"
            },
            "messages": [{ "role": "user", "content": "wire prompt" }],
            "system": [{ "type": "text", "text": "wire instructions" }],
            "request_context": { "end_user_reference": "wire-user" },
            "required_capabilities": ["system_prompt_blocks", "end_user_reference"],
            "model_parameters": { "max_output_tokens": 128 }
        }))
        .unwrap();

        invoke_message(input)
            .await
            .expect("current Generate should complete against fake upstream");
        let request = request_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("fake upstream should capture Generate request");
        let (headers, body) = request
            .split_once("\r\n\r\n")
            .expect("captured request should contain headers and body");
        let body: Value = serde_json::from_str(body).unwrap();

        assert!(headers.starts_with("POST /v1/messages HTTP/1.1"));
        assert!(headers
            .to_ascii_lowercase()
            .contains("x-api-key: wire-secret"));
        assert_eq!(
            body,
            json!({
                "model": "claude-sonnet-4-20250514",
                "messages": [{
                    "role": "user",
                    "content": [{ "type": "text", "text": "wire prompt" }]
                }],
                "stream": true,
                "max_tokens": 128,
                "system": [{ "type": "text", "text": "wire instructions" }],
                "metadata": { "user_id": "wire-user" }
            })
        );
    }

    #[tokio::test]
    async fn c2_fake_upstream_receives_exact_anthropic_count_tokens_wire() {
        let (base_url, request_rx) = start_json_server(r#"{"input_tokens":37}"#);

        let response = handle_request(count_tokens_invoke_request(&base_url))
            .await
            .expect("CountTokens should complete against fake upstream");
        let request = request_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("fake upstream should capture CountTokens request");
        let (headers, body) = request
            .split_once("\r\n\r\n")
            .expect("captured request should contain headers and body");
        let body: Value = serde_json::from_str(body).expect("captured CountTokens body is JSON");

        assert!(response.ok);
        assert_eq!(
            response.result,
            json!({ "operation": "count_tokens", "input_tokens": 37 })
        );
        assert!(headers.starts_with("POST /v1/messages/count_tokens HTTP/1.1"));
        assert!(headers
            .to_ascii_lowercase()
            .contains("x-api-key: wire-secret"));
        assert!(headers
            .to_ascii_lowercase()
            .contains("anthropic-beta: prompt-caching"));
        assert_eq!(
            body,
            json!({
                "model": "claude-sonnet-4-20250514",
                "messages": [{
                    "role": "user",
                    "content": [{ "type": "text", "text": "wire prompt" }]
                }],
                "system": [{ "type": "text", "text": "wire instructions" }],
                "metadata": { "user_id": "wire-user" }
            })
        );
        assert!(body.get("stream").is_none());
        assert!(body.get("max_tokens").is_none());
    }

    #[tokio::test]
    async fn c2_count_tokens_rejects_malformed_or_missing_input_tokens() {
        for response_body in [r#"{"input_tokens":"37"}"#, r#"{}"#] {
            let (base_url, request_rx) = start_json_server(response_body);
            let error = handle_request(count_tokens_invoke_request(&base_url))
                .await
                .expect_err("CountTokens response must require an unsigned input_tokens value");
            let typed = error
                .downcast_ref::<ProviderRuntimeError>()
                .expect("malformed CountTokens response must preserve a typed runtime error");

            assert_eq!(
                typed.kind,
                ProviderRuntimeErrorKind::ProviderInvalidResponse
            );
            assert!(typed.message.contains("CountTokens response"));
            request_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("fake upstream should receive the malformed-result request");
        }
    }

    #[tokio::test]
    async fn c2_count_tokens_preserves_typed_upstream_errors() {
        let base_url = start_http_error_server(
            "HTTP/1.1 429 Too Many Requests",
            "application/json",
            r#"{"error":{"message":"CountTokens quota exceeded"}}"#,
        );

        let error = handle_request(count_tokens_invoke_request(&base_url))
            .await
            .expect_err("upstream CountTokens error should surface as a runtime error");
        let typed = error
            .downcast_ref::<ProviderRuntimeError>()
            .expect("upstream CountTokens error must preserve its typed runtime error");

        assert_eq!(typed.kind, ProviderRuntimeErrorKind::ProviderUpstreamError);
        assert!(typed.message.contains("CountTokens quota exceeded"));
    }

    #[test]
    fn ac_002_current_generate_input_rejects_missing_legacy_and_unknown_contract_shapes() {
        let error = serde_json::from_value::<ProviderInvocationInput>(json!({
            "model": "claude-fable-5",
            "messages": [{ "role": "user", "content": "hello" }],
            "system": []
        }))
        .unwrap_err();

        assert!(error.to_string().contains("contract_version"));

        let legacy = serde_json::from_value::<ProviderInvocationInput>(json!({
            "contract_version": "1flowbase.provider/v1",
            "provider_instance_id": "provider-anthropic",
            "provider_code": "anthropic",
            "protocol": "anthropic_messages",
            "model": "claude-fable-5"
        }))
        .expect_err("legacy provider contract must be rejected");
        assert!(legacy.to_string().contains("1flowbase.provider/v1"));

        let unknown = serde_json::from_value::<ProviderInvocationInput>(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-anthropic",
            "provider_code": "anthropic",
            "protocol": "anthropic_messages",
            "model": "claude-fable-5",
            "raw_body": "must-not-be-accepted"
        }))
        .expect_err("unknown current contract fields must be rejected");
        assert!(unknown.to_string().contains("raw_body"));
    }

    #[test]
    fn ac_002_package_manifest_declares_current_generate_capabilities() {
        let manifest = include_str!("../manifest.yaml");

        assert!(manifest.contains("contract_version: 1flowbase.provider/v2"));
        assert!(!manifest.contains("1flowbase.provider/v1"));
        for capability in [
            "system_prompt_blocks",
            "system_prompt_cache_control",
            "end_user_reference",
            "count_tokens",
        ] {
            assert_eq!(manifest.matches(capability).count(), 1);
        }
    }

    #[test]
    fn messages_body_maps_native_tool_calls_and_tool_results() {
        let input = ProviderInvocationInput {
            model: "claude-sonnet-4-20250514".to_string(),
            system: vec![NativePromptBlock::text("Be concise")],
            messages: vec![
                ProviderMessage {
                    role: ProviderMessageRole::Assistant,
                    content: String::new(),
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: Some(
                        json!([{ "id": "toolu_1", "name": "lookup", "arguments": { "query": "refund" }}]),
                    ),
                    content_blocks: None,
                },
                ProviderMessage {
                    role: ProviderMessageRole::Tool,
                    content: "found".to_string(),
                    name: None,
                    tool_call_id: Some("toolu_1".to_string()),
                    is_error: None,
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
                ("max_output_tokens".to_string(), json!(2048)),
                ("thinking_type".to_string(), json!("enabled")),
                ("thinking_budget_tokens".to_string(), json!(1024)),
            ]),
            ..Default::default()
        };

        let body = build_messages_body(&input).unwrap();
        assert_eq!(
            body["system"],
            json!([{ "type": "text", "text": "Be concise" }])
        );
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
    fn ac_001_messages_body_groups_multiple_tool_results_in_one_user_turn() {
        let input = ProviderInvocationInput {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![
                ProviderMessage {
                    role: ProviderMessageRole::Assistant,
                    content: "Checking both paths".to_string(),
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: Some(json!([
                        {
                            "id": "toolu_1",
                            "name": "lookup",
                            "arguments": { "query": "first" }
                        },
                        {
                            "id": "toolu_2",
                            "name": "lookup",
                            "arguments": { "query": "second" }
                        }
                    ])),
                    content_blocks: None,
                },
                ProviderMessage {
                    role: ProviderMessageRole::Tool,
                    content: "first result".to_string(),
                    name: Some("lookup".to_string()),
                    tool_call_id: Some("toolu_1".to_string()),
                    is_error: None,
                    tool_calls: None,
                    content_blocks: None,
                },
                ProviderMessage {
                    role: ProviderMessageRole::Tool,
                    content: "second result".to_string(),
                    name: Some("lookup".to_string()),
                    tool_call_id: Some("toolu_2".to_string()),
                    is_error: None,
                    tool_calls: None,
                    content_blocks: None,
                },
            ],
            ..Default::default()
        };

        let body = build_messages_body(&input).unwrap();

        assert_eq!(body["messages"].as_array().map(Vec::len), Some(2));
        assert_eq!(body["messages"][0]["content"][1]["id"], "toolu_1");
        assert_eq!(body["messages"][0]["content"][2]["id"], "toolu_2");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(
            body["messages"][1]["content"].as_array().map(Vec::len),
            Some(2)
        );
        assert_eq!(body["messages"][1]["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(body["messages"][1]["content"][1]["tool_use_id"], "toolu_2");
    }

    #[test]
    fn messages_body_preserves_media_tool_result_content_blocks() {
        let input = ProviderInvocationInput {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![ProviderMessage {
                role: ProviderMessageRole::Tool,
                content: String::new(),
                name: None,
                tool_call_id: Some("toolu_image".to_string()),
                is_error: None,
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
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-anthropic",
            "provider_code": "anthropic",
            "protocol": "anthropic_messages",
            "model": "claude-sonnet-4-20250514",
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
            proxy_url: None,
        };

        let headers = build_headers(&config, input.client_protocol_envelope.as_ref()).unwrap();

        assert_eq!(headers["x-api-key"], "provider-config-secret");
        assert_eq!(headers["anthropic-version"], "2023-06-01");
        assert_eq!(headers["anthropic-beta"], "prompt-caching");
        assert_eq!(headers["x-claude-code-session-id"], "session-123");
        assert_eq!(headers["user-agent"], "ClaudeCode/1.0");
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
        headers.insert(
            HeaderName::from_static("anthropic-request-id"),
            HeaderValue::from_static("anthropic_req_plain"),
        );
        headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer provider-secret"),
        );
        headers.insert(
            HeaderName::from_static("cookie"),
            HeaderValue::from_static("session=secret"),
        );
        headers.insert(
            HeaderName::from_static("set-cookie"),
            HeaderValue::from_static("session=secret"),
        );
        headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_static("response-secret"),
        );
        let raw_body = "plain upstream failure body with request payload".to_string();

        let error = provider_upstream_error_from_parts(
            reqwest::StatusCode::FORBIDDEN,
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
            &json!({ "status": 403, "request_id": "req_plain" })
        );
        let encoded = serde_json::to_string(&error).unwrap();
        assert!(!encoded.contains(&raw_body));
        assert!(!encoded.contains("provider-secret"));
        assert!(!encoded.contains("session=secret"));
        assert!(!encoded.contains("response-secret"));
    }

    #[test]
    fn upstream_json_body_uses_error_message_as_public_summary() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            HeaderName::from_static("request-id"),
            HeaderValue::from_static("req_json"),
        );
        let raw_body =
            r#"{"error":{"type":"permission_error","message":"Workspace access denied"}}"#
                .to_string();

        let error = provider_upstream_error_from_parts(
            reqwest::StatusCode::FORBIDDEN,
            &headers,
            raw_body.clone(),
        );

        assert_eq!(error.kind, ProviderRuntimeErrorKind::ProviderUpstreamError);
        assert!(error.message.contains("Workspace access denied"));
        assert_eq!(
            error.provider_summary.as_deref(),
            Some(error.message.as_str())
        );
        let details = error
            .provider_details
            .expect("upstream error should carry details");
        assert_eq!(details["status"], 403);
        assert_eq!(details["request_id"], "req_json");
        assert!(details.get("raw_body").is_none());
        assert!(details.get("headers").is_none());
    }

    #[test]
    fn messages_body_forwards_content_blocks_and_appends_native_tool_calls() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-anthropic",
            "provider_code": "anthropic",
            "protocol": "anthropic_messages",
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
    fn messages_body_without_content_blocks_uses_current_text_content() {
        let input = ProviderInvocationInput {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![ProviderMessage {
                role: ProviderMessageRole::User,
                content: "hello world".to_string(),
                name: None,
                tool_call_id: None,
                is_error: None,
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
    async fn ac_006_anthropic_streaming_result_records_effective_max_output_tokens() {
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
            DEFAULT_MAX_TOKENS,
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
        assert_eq!(
            envelope.result.provider_metadata["effective_max_output_tokens"],
            json!(4096)
        );
        assert!(events.contains(&ProviderStreamEvent::Finish {
            reason: ProviderFinishReason::Stop
        }));
    }

    #[tokio::test]
    async fn read_streaming_message_non_success_returns_provider_runtime_error() {
        let raw_body = "streaming upstream raw plaintext";
        let response = reqwest::get(start_http_error_server(
            "HTTP/1.1 403 Forbidden",
            "text/plain; charset=utf-8",
            raw_body,
        ))
        .await
        .unwrap();

        let error = read_streaming_message(
            response,
            "claude-sonnet-4-20250514".to_string(),
            DEFAULT_MAX_TOKENS,
            &mut |_| Ok(()),
        )
        .await
        .expect_err("non-success response should fail");
        let runtime_error = error
            .downcast_ref::<ProviderRuntimeError>()
            .expect("error should downcast to ProviderRuntimeError");
        let details = runtime_error
            .provider_details
            .as_ref()
            .expect("upstream error should carry details");

        assert_eq!(
            runtime_error.kind,
            ProviderRuntimeErrorKind::ProviderUpstreamError
        );
        assert!(!runtime_error.message.contains(raw_body));
        assert_eq!(
            details,
            &json!({ "status": 403, "request_id": "req_stream" })
        );
        let encoded = serde_json::to_string(runtime_error).unwrap();
        assert!(!encoded.contains(raw_body));
        assert!(!encoded.contains("response-secret"));
    }
}
