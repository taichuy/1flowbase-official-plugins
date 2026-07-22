use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    Method, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const PROVIDER_CODE: &str = "gemini";
const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";
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
const JSON_PARAMETERS: &[&str] = &[
    "tools",
    "tool_choice",
    "safety_settings",
    "response_schema",
    "thinking_config",
    "image_config",
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
enum AuthType {
    ApiKey,
    Bearer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderConfig {
    base_url: String,
    api_key: String,
    auth_type: AuthType,
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
    #[serde(default, skip_serializing_if = "is_empty_object")]
    pub provider_metadata: Value,
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
        "balance" => Ok(ProviderStdioResponse::ok(json!({
            "is_available": true,
            "balance_infos": []
        }))),
        "invoke" => {
            let input: ProviderInvocationInput = serde_json::from_value(request.input)?;
            let output = invoke_generate_content(input).await?;
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
    let output = invoke_generate_content_with_event_sink(input, on_event).await?;
    Ok(output.result)
}

async fn validate_provider_config(input: &Value) -> Result<ProviderStdioResponse> {
    let config = normalize_provider_config(input)?;
    let payload = request_json(&config, "/v1beta/models", Method::GET, None).await?;
    let models = normalize_model_entries(payload.get("models").unwrap_or(&Value::Null))?;

    if config.validate_model {
        if let Some(model_id) = configured_model_id(input) {
            let normalized_model = normalize_model_id(&model_id);
            let exists = models
                .iter()
                .any(|model| normalize_model_id(&model.model_id) == normalized_model);
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
            "auth_type": auth_type_name(&config.auth_type),
            "validate_model": config.validate_model,
            "proxy_url": config.proxy_url.as_ref().map(|_| "***")
        },
        "model_count": models.len(),
    })))
}

async fn list_models(input: &Value) -> Result<ProviderStdioResponse> {
    let config = normalize_provider_config(input)?;
    let payload = request_json(&config, "/v1beta/models", Method::GET, None).await?;
    Ok(ProviderStdioResponse::ok(json!(normalize_model_entries(
        payload.get("models").unwrap_or(&Value::Null)
    )?)))
}

fn normalize_provider_config(input: &Value) -> Result<ProviderConfig> {
    let config = provider_config_object(input)?;
    let base_url =
        optional_text(config.get("base_url")).unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let auth_type = normalize_auth_type(config.get("auth_type"))?;

    Ok(ProviderConfig {
        base_url,
        api_key: require_text(config.get("api_key"), "api_key")?,
        auth_type,
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

fn normalize_auth_type(value: Option<&Value>) -> Result<AuthType> {
    let raw = optional_text(value).unwrap_or_else(|| "api_key".to_string());
    match raw.trim().to_ascii_lowercase().as_str() {
        "api_key" | "x-goog-api-key" | "key" => Ok(AuthType::ApiKey),
        "bearer" | "oauth" | "authorization" => Ok(AuthType::Bearer),
        other => bail!("unsupported auth_type: {other}"),
    }
}

fn auth_type_name(auth_type: &AuthType) -> &'static str {
    match auth_type {
        AuthType::ApiKey => "api_key",
        AuthType::Bearer => "bearer",
    }
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

    let mut models = Vec::new();
    for entry in items {
        if model_supports_generation(entry) {
            models.push(normalize_model_entry(entry)?);
        }
    }
    Ok(models)
}

fn model_supports_generation(entry: &Value) -> bool {
    let Some(methods) = entry
        .get("supportedGenerationMethods")
        .or_else(|| entry.get("supported_generation_methods"))
        .and_then(Value::as_array)
    else {
        return true;
    };
    methods.iter().any(|method| {
        method.as_str() == Some("generateContent")
            || method.as_str() == Some("streamGenerateContent")
    })
}

fn normalize_model_entry(entry: &Value) -> Result<ProviderModelDescriptor> {
    let raw_model_id = entry
        .get("name")
        .or_else(|| entry.get("id"))
        .or_else(|| entry.get("model_id"))
        .map(value_to_string)
        .unwrap_or_default()
        .trim()
        .to_string();
    let model_id = normalize_model_id(&raw_model_id);
    if model_id.is_empty() {
        bail!("model_id is required");
    }

    let methods = entry
        .get("supportedGenerationMethods")
        .or_else(|| entry.get("supported_generation_methods"))
        .cloned()
        .unwrap_or(Value::Null);

    Ok(ProviderModelDescriptor {
        model_id: model_id.clone(),
        display_name: optional_text(entry.get("displayName"))
            .or_else(|| optional_text(entry.get("display_name")))
            .unwrap_or_else(|| model_id.clone()),
        source: "dynamic".to_string(),
        supports_streaming: methods
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .any(|item| item.as_str() == Some("streamGenerateContent"))
            })
            .unwrap_or(true),
        supports_tool_call: true,
        supports_multimodal: true,
        context_window: explicit_number_alias(entry, &["inputTokenLimit", "input_token_limit"]),
        max_output_tokens: explicit_number_alias(
            entry,
            &["outputTokenLimit", "output_token_limit"],
        ),
        provider_metadata: json!({
            "owned_by": "google",
            "name": raw_model_id,
            "description": entry.get("description").cloned().unwrap_or(Value::Null),
            "version": entry.get("version").cloned().unwrap_or(Value::Null),
            "base_model_id": entry.get("baseModelId").or_else(|| entry.get("base_model_id")).cloned().unwrap_or(Value::Null),
            "supported_generation_methods": methods,
            "pricing_source": "dynamic"
        }),
    })
}

fn normalize_model_id(model_id: &str) -> String {
    model_id
        .trim()
        .strip_prefix("models/")
        .unwrap_or_else(|| model_id.trim())
        .to_string()
}

fn explicit_number_alias(entry: &Value, aliases: &[&str]) -> Option<u64> {
    aliases
        .iter()
        .find_map(|alias| entry.get(alias).and_then(number_or_none_ref))
}

fn number_or_none_ref(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
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

    match config.auth_type {
        AuthType::ApiKey => {
            headers.insert(
                HeaderName::from_static("x-goog-api-key"),
                HeaderValue::from_str(&config.api_key)
                    .context("invalid api_key for x-goog-api-key header")?,
            );
        }
        AuthType::Bearer => {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", config.api_key))
                    .context("invalid api_key for authorization header")?,
            );
        }
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
    let url = Url::parse(&format!("{base_url}{pathname}"))
        .with_context(|| format!("invalid base_url: {}", config.base_url))?;
    Ok(url.to_string())
}

fn build_http_client(config: &ProviderConfig) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy_url) = &config.proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy_url")?);
    }
    builder.build().context("building Gemini HTTP client")
}

fn build_model_action_url(
    config: &ProviderConfig,
    model: &str,
    action: &str,
    stream: bool,
) -> Result<String> {
    let model_id = normalize_model_id(model);
    if model_id.is_empty() {
        bail!("model is required");
    }

    let base_url = config.base_url.trim_end_matches('/');
    let mut url = Url::parse(&format!("{base_url}/v1beta/models/{model_id}:{action}"))
        .with_context(|| format!("invalid base_url: {}", config.base_url))?;
    if stream {
        url.query_pairs_mut().append_pair("alt", "sse");
    }
    Ok(url.to_string())
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
        .headers(build_headers(config, body.is_some(), None)?);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .map_err(|error| sanitize_error(error, config))?;

    let status = response.status();
    let payload = read_json_response(response)
        .await
        .map_err(|error| sanitize_anyhow_error(error, config))?;
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
    if status.is_success() {
        return Ok(());
    }
    let message = sanitize_text(provider_error_message(payload), config);
    bail!("{} {}: {}", status.as_u16(), status, message);
}

fn provider_error_message(payload: &Value) -> String {
    payload
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .get("error")
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str)
        })
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "provider upstream request failed".to_string())
}

fn sanitize_error(error: reqwest::Error, config: &ProviderConfig) -> anyhow::Error {
    anyhow!(sanitize_text(error.to_string(), config))
}

fn sanitize_anyhow_error(error: anyhow::Error, config: &ProviderConfig) -> anyhow::Error {
    anyhow!(sanitize_text(error.to_string(), config))
}

fn sanitize_text(message: String, config: &ProviderConfig) -> String {
    let mut sanitized = message.replace(&config.api_key, "***");
    if let Some(proxy_url) = &config.proxy_url {
        sanitized = sanitized.replace(proxy_url, "***");
    }
    sanitized
}

async fn invoke_generate_content(
    input: ProviderInvocationInput,
) -> Result<RuntimeInvocationEnvelope> {
    invoke_generate_content_with_event_sink(input, |_| Ok(())).await
}

async fn invoke_generate_content_with_event_sink<F>(
    input: ProviderInvocationInput,
    mut on_event: F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let config = normalize_provider_config(&input.provider_config)?;
    let body = build_generate_content_body(&input)?;
    let client = build_http_client(&config)?;
    let response = client
        .request(
            Method::POST,
            build_model_action_url(&config, &input.model, "streamGenerateContent", true)
                .context("invalid streamGenerateContent endpoint")?,
        )
        .headers(build_headers(
            &config,
            true,
            input.client_protocol_envelope.as_ref(),
        )?)
        .json(&body)
        .send()
        .await
        .map_err(|error| sanitize_error(error, &config))?;

    read_streaming_generate_content(response, input.model, &config.api_key, &mut on_event).await
}

fn build_generate_content_body(input: &ProviderInvocationInput) -> Result<Value> {
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
        bail!("Gemini Generate does not support the requested semantic capabilities");
    }
    if input.model.trim().is_empty() {
        bail!("model is required");
    }

    let mut body = Map::new();
    let mut contents = Vec::new();
    let mut system_parts = Vec::new();
    let mut tool_call_names_by_id = BTreeMap::new();

    if let Some(system) = input.system_text() {
        system_parts.push(json!({ "text": system }));
    }

    for message in &input.messages {
        if message.role == ProviderMessageRole::System {
            if let Some(content_blocks) = &message.content_blocks {
                append_text_parts(&mut system_parts, content_blocks);
            } else if !message.content.trim().is_empty() {
                system_parts.push(json!({ "text": message.content }));
            }
            continue;
        }

        let gemini_role = normalize_gemini_role(message.role);
        let mut parts = if message.role == ProviderMessageRole::Tool {
            let content_block_parts = build_message_content_parts(message);
            if content_parts_contain_media(&content_block_parts) {
                content_block_parts
            } else {
                build_tool_response_parts(message, &tool_call_names_by_id)
            }
        } else {
            build_message_content_parts(message)
        };

        if gemini_role == "model" {
            append_tool_call_parts(
                &mut parts,
                message.tool_calls.as_ref(),
                &mut tool_call_names_by_id,
            );
        }

        if parts.is_empty() {
            continue;
        }

        contents.push(json!({
            "role": gemini_role,
            "parts": parts,
        }));
    }

    if contents.is_empty() {
        bail!("messages are required");
    }

    body.insert("contents".to_string(), Value::Array(contents));
    if !system_parts.is_empty() {
        body.insert(
            "systemInstruction".to_string(),
            json!({ "parts": system_parts }),
        );
    }

    let generation_config = build_generation_config(input)?;
    if !generation_config.is_empty() {
        body.insert(
            "generationConfig".to_string(),
            Value::Object(generation_config),
        );
    }

    let tools = build_tools(input)?;
    if !tools.is_empty() {
        body.insert("tools".to_string(), Value::Array(tools));
    }

    if let Some(tool_config) = build_tool_config(input)? {
        body.insert("toolConfig".to_string(), tool_config);
    }

    if let Some(safety_settings) = parameter_value(input, "safety_settings") {
        body.insert("safetySettings".to_string(), safety_settings);
    }

    Ok(Value::Object(body))
}

fn normalize_gemini_role(role: ProviderMessageRole) -> &'static str {
    match role {
        ProviderMessageRole::Assistant => "model",
        ProviderMessageRole::System | ProviderMessageRole::User | ProviderMessageRole::Tool => {
            "user"
        }
    }
}

fn append_text_parts(parts: &mut Vec<Value>, content: &Value) {
    let text = content_to_text(content);
    if !text.trim().is_empty() {
        parts.push(json!({ "text": text }));
    }
}

fn build_message_content_parts(message: &ProviderMessage) -> Vec<Value> {
    if let Some(content_blocks) = &message.content_blocks {
        let parts = build_content_parts(content_blocks);
        if !parts.is_empty() {
            return parts;
        }
    }
    build_content_parts(&Value::String(message.content.clone()))
}

fn content_parts_contain_media(parts: &[Value]) -> bool {
    parts
        .iter()
        .any(|part| part.get("inlineData").is_some() || part.get("fileData").is_some())
}

fn content_to_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.clone()),
                Value::Object(object) => object
                    .get("text")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .or_else(|| {
                        object
                            .get("content")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    }),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| content.to_string()),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn build_content_parts(content: &Value) -> Vec<Value> {
    match content {
        Value::String(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "text": text })]
            }
        }
        Value::Array(items) => items.iter().filter_map(normalize_content_part).collect(),
        Value::Object(object) => {
            if let Some(parts) = object.get("parts").and_then(Value::as_array) {
                return parts.iter().filter_map(normalize_content_part).collect();
            }
            normalize_content_part(content).into_iter().collect()
        }
        Value::Null => Vec::new(),
        other => vec![json!({ "text": other.to_string() })],
    }
}

fn normalize_content_part(part: &Value) -> Option<Value> {
    match part {
        Value::String(text) => (!text.is_empty()).then_some(json!({ "text": text })),
        Value::Object(object) => {
            let part_type = object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();

            if part_type.is_empty() && is_native_gemini_part(object) {
                return Some(Value::Object(normalize_native_part_keys(object.clone())));
            }

            match part_type.as_str() {
                "text" | "input_text" => object
                    .get("text")
                    .or_else(|| object.get("content"))
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(|text| json!({ "text": text })),
                "image" | "image_url" | "input_image" => image_part_from_object(object),
                "function_response" | "tool_result" => function_response_part_from_object(object),
                _ => object
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(|text| json!({ "text": text }))
                    .or_else(|| Some(json!({ "text": Value::Object(object.clone()).to_string() }))),
            }
        }
        Value::Null => None,
        other => Some(json!({ "text": other.to_string() })),
    }
}

fn is_native_gemini_part(object: &Map<String, Value>) -> bool {
    object.contains_key("text")
        || object.contains_key("inlineData")
        || object.contains_key("inline_data")
        || object.contains_key("fileData")
        || object.contains_key("file_data")
        || object.contains_key("functionCall")
        || object.contains_key("function_call")
        || object.contains_key("functionResponse")
        || object.contains_key("function_response")
}

fn normalize_native_part_keys(mut object: Map<String, Value>) -> Map<String, Value> {
    rename_key(&mut object, "inline_data", "inlineData");
    rename_key(&mut object, "file_data", "fileData");
    rename_key(&mut object, "function_call", "functionCall");
    rename_key(&mut object, "function_response", "functionResponse");
    rename_key(&mut object, "thought_signature", "thoughtSignature");
    object
}

fn rename_key(object: &mut Map<String, Value>, from: &str, to: &str) {
    if object.contains_key(to) {
        return;
    }
    if let Some(value) = object.remove(from) {
        object.insert(to.to_string(), value);
    }
}

fn image_part_from_object(object: &Map<String, Value>) -> Option<Value> {
    if let Some(part) = object.get("source").and_then(image_part_from_source) {
        return Some(part);
    }

    let image_value = object
        .get("image_url")
        .or_else(|| object.get("imageUrl"))
        .or_else(|| object.get("url"))
        .or_else(|| object.get("data"));

    let url = match image_value {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Object(image)) => image
            .get("url")
            .or_else(|| image.get("data"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        _ => String::new(),
    };

    if url.is_empty() {
        return None;
    }

    image_part_from_url(&url, object)
}

fn image_part_from_source(source: &Value) -> Option<Value> {
    let object = source.as_object()?;
    if let Some(data) = object
        .get("data")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(inline_image_part(object, data));
    }
    let url = object
        .get("url")
        .or_else(|| object.get("image_url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    image_part_from_url(url, object)
}

fn image_part_from_url(url: &str, object: &Map<String, Value>) -> Option<Value> {
    if let Some((mime_type, data)) = parse_data_url(url) {
        return Some(json!({
            "inlineData": {
                "mimeType": mime_type,
                "data": data,
            }
        }));
    }

    let mime_type = object
        .get("mime_type")
        .or_else(|| object.get("mimeType"))
        .or_else(|| object.get("media_type"))
        .and_then(Value::as_str)
        .unwrap_or("image/*");
    Some(json!({
        "fileData": {
            "mimeType": mime_type,
            "fileUri": url,
        }
    }))
}

fn inline_image_part(object: &Map<String, Value>, data: &str) -> Value {
    let mime_type = object
        .get("media_type")
        .or_else(|| object.get("mime_type"))
        .or_else(|| object.get("mimeType"))
        .and_then(Value::as_str)
        .unwrap_or("image/png");
    json!({
        "inlineData": {
            "mimeType": mime_type,
            "data": data,
        }
    })
}

fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (mime_type, data) = rest.split_once(";base64,")?;
    Some((mime_type.to_string(), data.to_string()))
}

fn function_response_part_from_object(object: &Map<String, Value>) -> Option<Value> {
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .trim();
    if name.is_empty() {
        return None;
    }

    let response = object
        .get("response")
        .cloned()
        .or_else(|| object.get("content").cloned())
        .or_else(|| object.get("result").cloned())
        .unwrap_or(Value::Null);

    let mut function_response = Map::new();
    function_response.insert("name".to_string(), Value::String(name.to_string()));
    function_response.insert("response".to_string(), ensure_object_response(response));
    if let Some(id) = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    {
        function_response.insert("id".to_string(), Value::String(id.to_string()));
    }

    Some(json!({ "functionResponse": function_response }))
}

fn build_tool_response_parts(
    message: &ProviderMessage,
    tool_call_names_by_id: &BTreeMap<String, String>,
) -> Vec<Value> {
    let native_parts = message
        .content_blocks
        .as_ref()
        .map(build_content_parts)
        .unwrap_or_default()
        .into_iter()
        .filter(|part| part.get("functionResponse").is_some())
        .collect::<Vec<_>>();
    if !native_parts.is_empty() {
        return native_parts;
    }

    let name = message
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            message
                .tool_call_id
                .as_deref()
                .and_then(|id| tool_call_names_by_id.get(id))
                .map(String::as_str)
        })
        .or(message.tool_call_id.as_deref())
        .unwrap_or("tool");
    let mut response = Map::new();
    response.insert("name".to_string(), Value::String(name.to_string()));
    response.insert(
        "response".to_string(),
        ensure_object_response(Value::String(message.content.clone())),
    );
    if let Some(tool_call_id) = message
        .tool_call_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        response.insert("id".to_string(), Value::String(tool_call_id.to_string()));
    }

    vec![json!({ "functionResponse": response })]
}

fn ensure_object_response(value: Value) -> Value {
    match value {
        Value::Object(_) => value,
        Value::String(text) => serde_json::from_str::<Value>(&text)
            .ok()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({ "content": text })),
        Value::Null => json!({}),
        other => json!({ "content": other }),
    }
}

fn append_tool_call_parts(
    parts: &mut Vec<Value>,
    tool_calls: Option<&Value>,
    tool_call_names_by_id: &mut BTreeMap<String, String>,
) {
    let Some(tool_calls) = tool_calls else {
        return;
    };
    let Some(items) = tool_calls.as_array() else {
        return;
    };

    for (index, item) in items.iter().enumerate() {
        if let Some((part, id, name)) = function_call_part_from_tool_call(item, index) {
            tool_call_names_by_id.insert(id, name);
            parts.push(part);
        }
    }
}

fn function_call_part_from_tool_call(
    tool_call: &Value,
    index: usize,
) -> Option<(Value, String, String)> {
    let object = tool_call.as_object()?;
    let function = object.get("function").and_then(Value::as_object);

    let name = function
        .and_then(|value| value.get("name"))
        .or_else(|| object.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }

    let args = function
        .and_then(|value| value.get("arguments"))
        .or_else(|| object.get("arguments"))
        .cloned()
        .map(normalize_arguments)
        .unwrap_or_else(|| json!({}));
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{name}-{index}"));
    let mut part = json!({
        "functionCall": {
            "name": name.clone(),
            "args": args,
            "id": id.clone()
        }
    });
    if let Some(thought_signature) = tool_call_thought_signature(object) {
        part["thoughtSignature"] = Value::String(thought_signature.to_string());
    }

    Some((part, id, name))
}

fn tool_call_thought_signature(object: &Map<String, Value>) -> Option<&str> {
    object
        .get("provider_metadata")
        .and_then(Value::as_object)
        .and_then(|metadata| {
            metadata
                .get("gemini")
                .and_then(Value::as_object)
                .and_then(|gemini| {
                    gemini
                        .get("thought_signature")
                        .or_else(|| gemini.get("thoughtSignature"))
                        .and_then(Value::as_str)
                })
                .or_else(|| {
                    metadata
                        .get("thought_signature")
                        .or_else(|| metadata.get("thoughtSignature"))
                        .and_then(Value::as_str)
                })
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn is_empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(|object| object.is_empty())
}

fn normalize_arguments(value: Value) -> Value {
    match value {
        Value::String(text) => serde_json::from_str::<Value>(&text)
            .ok()
            .unwrap_or_else(|| json!({ "value": text })),
        Value::Null => json!({}),
        other => other,
    }
}

fn build_generation_config(input: &ProviderInvocationInput) -> Result<Map<String, Value>> {
    let mut config = Map::new();

    insert_parameter(
        &mut config,
        "temperature",
        parameter_value(input, "temperature"),
    );
    insert_parameter(&mut config, "topP", parameter_value(input, "top_p"));
    insert_parameter(&mut config, "topK", parameter_value(input, "top_k"));
    insert_parameter(
        &mut config,
        "candidateCount",
        parameter_value(input, "candidate_count"),
    );

    if let Some(max_output_tokens) = parameter_value(input, "max_output_tokens") {
        config.insert("maxOutputTokens".to_string(), max_output_tokens);
    }

    if let Some(stop) = parameter_value(input, "stop").and_then(stop_sequences_value) {
        config.insert("stopSequences".to_string(), stop);
    }

    if let Some(response_format) = input
        .response_format
        .clone()
        .or_else(|| parameter_value(input, "response_format"))
    {
        apply_response_format(&mut config, response_format)?;
    }

    if let Some(response_mime_type) = parameter_value(input, "response_mime_type") {
        config.insert("responseMimeType".to_string(), response_mime_type);
    }
    if let Some(response_schema) = parameter_value(input, "response_schema") {
        config.insert("responseSchema".to_string(), response_schema);
    }

    if let Some(thinking_config) = build_thinking_config(input) {
        config.insert("thinkingConfig".to_string(), thinking_config);
    }

    if let Some(image_config) = build_image_config(input) {
        config.insert("imageConfig".to_string(), image_config);
    }

    Ok(config)
}

fn insert_parameter(config: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        config.insert(key.to_string(), value);
    }
}

fn stop_sequences_value(value: Value) -> Option<Value> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(Value::Array(vec![Value::String(trimmed.to_string())]))
            }
        }
        Value::Array(items) => {
            let values = items
                .into_iter()
                .filter_map(|item| optional_text(Some(&item)).map(Value::String))
                .collect::<Vec<_>>();
            (!values.is_empty()).then_some(Value::Array(values))
        }
        _ => None,
    }
}

fn apply_response_format(config: &mut Map<String, Value>, value: Value) -> Result<()> {
    match value {
        Value::String(text) => match text.trim() {
            "json_object" | "json_schema" | "json" | "application/json" => {
                config.insert(
                    "responseMimeType".to_string(),
                    Value::String("application/json".to_string()),
                );
            }
            "text" | "text/plain" => {
                config.insert(
                    "responseMimeType".to_string(),
                    Value::String("text/plain".to_string()),
                );
            }
            other if !other.is_empty() => {
                config.insert(
                    "responseMimeType".to_string(),
                    Value::String(other.to_string()),
                );
            }
            _ => {}
        },
        Value::Object(object) => {
            if let Some(format_type) = object.get("type").and_then(Value::as_str) {
                apply_response_format(config, Value::String(format_type.to_string()))?;
            }
            if let Some(schema) = object
                .get("json_schema")
                .and_then(|value| value.get("schema"))
                .or_else(|| object.get("schema"))
            {
                config.insert("responseSchema".to_string(), schema.clone());
            }
        }
        Value::Null => {}
        other => {
            config.insert("responseMimeType".to_string(), other);
        }
    }

    Ok(())
}

fn build_thinking_config(input: &ProviderInvocationInput) -> Option<Value> {
    if let Some(config) = parameter_value(input, "thinking_config") {
        return Some(config);
    }

    let include_thoughts = parameter_value(input, "include_thoughts").and_then(bool_value);
    let thinking_budget = parameter_value(input, "thinking_budget").and_then(integer_value);
    if include_thoughts.is_none() && thinking_budget.is_none() {
        return None;
    }

    let mut config = Map::new();
    if let Some(include_thoughts) = include_thoughts {
        config.insert("includeThoughts".to_string(), Value::Bool(include_thoughts));
    }
    if let Some(thinking_budget) = thinking_budget {
        config.insert("thinkingBudget".to_string(), json!(thinking_budget));
    }
    Some(Value::Object(config))
}

fn build_image_config(input: &ProviderInvocationInput) -> Option<Value> {
    if let Some(config) = parameter_value(input, "image_config") {
        return Some(config);
    }

    let aspect_ratio = parameter_value(input, "aspect_ratio");
    let image_size = parameter_value(input, "image_size");
    if aspect_ratio.is_none() && image_size.is_none() {
        return None;
    }

    let mut config = Map::new();
    if let Some(value) = aspect_ratio {
        config.insert("aspectRatio".to_string(), value);
    }
    if let Some(value) = image_size {
        config.insert("imageSize".to_string(), value);
    }
    Some(Value::Object(config))
}

fn bool_value(value: Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(value),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn integer_value(value: Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().map(|value| value as i64)),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn build_tools(input: &ProviderInvocationInput) -> Result<Vec<Value>> {
    let raw_tools = if !input.tools.is_empty() {
        Some(Value::Array(input.tools.clone()))
    } else {
        parameter_value(input, "tools")
    };

    let Some(raw_tools) = raw_tools else {
        return Ok(Vec::new());
    };
    let Some(items) = raw_tools.as_array() else {
        return Ok(Vec::new());
    };

    let mut tools = Vec::new();
    let mut function_declarations = Vec::new();

    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };

        if is_native_gemini_tool(object) {
            tools.push(Value::Object(normalize_native_tool_keys(object.clone())));
            continue;
        }

        if is_web_search_tool(object) {
            tools.push(json!({ "google_search": {} }));
            continue;
        }

        if let Some(declaration) = function_declaration_from_tool(object) {
            function_declarations.push(declaration);
        }
    }

    if !function_declarations.is_empty() {
        tools.insert(
            0,
            json!({
                "functionDeclarations": function_declarations,
            }),
        );
    }

    Ok(tools)
}

fn is_native_gemini_tool(object: &Map<String, Value>) -> bool {
    object.contains_key("functionDeclarations")
        || object.contains_key("function_declarations")
        || object.contains_key("googleSearch")
        || object.contains_key("google_search")
        || object.contains_key("codeExecution")
        || object.contains_key("code_execution")
}

fn normalize_native_tool_keys(mut object: Map<String, Value>) -> Map<String, Value> {
    rename_key(&mut object, "function_declarations", "functionDeclarations");
    rename_key(&mut object, "googleSearch", "google_search");
    rename_key(&mut object, "code_execution", "codeExecution");
    object
}

fn is_web_search_tool(object: &Map<String, Value>) -> bool {
    object
        .get("type")
        .and_then(Value::as_str)
        .map(|value| value.starts_with("web_search") || value == "google_search")
        .unwrap_or(false)
        || object
            .get("name")
            .and_then(Value::as_str)
            .map(|value| {
                matches!(
                    value.trim(),
                    "web_search" | "google_search" | "web_search_20250305"
                )
            })
            .unwrap_or(false)
}

fn function_declaration_from_tool(object: &Map<String, Value>) -> Option<Value> {
    let function = object.get("function").and_then(Value::as_object);
    let name = function
        .and_then(|value| value.get("name"))
        .or_else(|| object.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if name.is_empty() {
        return None;
    }

    let description = function
        .and_then(|value| value.get("description"))
        .or_else(|| object.get("description"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let parameters = function
        .and_then(|value| value.get("parameters"))
        .or_else(|| object.get("parameters"))
        .or_else(|| object.get("input_schema"))
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));

    let mut declaration = Map::new();
    declaration.insert("name".to_string(), Value::String(name.to_string()));
    if !description.is_empty() {
        declaration.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    declaration.insert("parameters".to_string(), clean_schema(parameters));
    Some(Value::Object(declaration))
}

fn clean_schema(value: Value) -> Value {
    match value {
        Value::Object(mut object) => {
            object.remove("$schema");
            object.remove("additionalProperties");
            let keys = object.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                if let Some(value) = object.remove(&key) {
                    object.insert(key, clean_schema(value));
                }
            }
            Value::Object(object)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(clean_schema).collect()),
        other => other,
    }
}

fn build_tool_config(input: &ProviderInvocationInput) -> Result<Option<Value>> {
    let Some(tool_choice) = parameter_value(input, "tool_choice") else {
        return Ok(None);
    };

    match tool_choice {
        Value::String(text) => {
            let mode = match text.trim().to_ascii_lowercase().as_str() {
                "none" => "NONE".to_string(),
                "auto" => "AUTO".to_string(),
                "required" | "any" => "ANY".to_string(),
                "validated" => "VALIDATED".to_string(),
                "" => return Ok(None),
                other => other.to_string(),
            };
            Ok(Some(json!({
                "functionCallingConfig": {
                    "mode": mode,
                }
            })))
        }
        Value::Object(object) => {
            let function_name = object
                .get("function")
                .and_then(|value| value.get("name"))
                .or_else(|| object.get("name"))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty());
            if let Some(function_name) = function_name {
                return Ok(Some(json!({
                    "functionCallingConfig": {
                        "mode": "ANY",
                        "allowedFunctionNames": [function_name],
                    }
                })));
            }
            Ok(Some(Value::Object(object)))
        }
        Value::Null => Ok(None),
        other => Ok(Some(other)),
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
        _ if JSON_PARAMETERS.contains(&key) => normalize_json_parameter(value),
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

async fn read_streaming_generate_content<F>(
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
        let payload =
            serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "message": text }));
        let message = provider_error_message(&payload).replace(api_key, "***");
        bail!("{} {}: {}", status.as_u16(), status, message);
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut events = Vec::new();
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    let mut usage = ProviderUsage::default();
    let mut finish_reason: Option<ProviderFinishReason> = None;
    let mut response_id = Value::Null;
    let mut model_version = Value::Null;

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
                &mut reasoning,
                &mut tool_calls,
                &mut usage,
                &mut finish_reason,
                &mut response_id,
                &mut model_version,
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
            &mut reasoning,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
            &mut model_version,
        )?;
        emit_new_events(&events, event_start, on_event)?;
    }

    let final_event_start = events.len();
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
                "response_id": response_id,
                "model_version": model_version,
                "reasoning": if reasoning.is_empty() { Value::Null } else { Value::String(reasoning) },
            }),
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn process_sse_line(
    line: &str,
    events: &mut Vec<ProviderStreamEvent>,
    text: &mut String,
    reasoning: &mut String,
    tool_calls: &mut Vec<ProviderToolCall>,
    usage: &mut ProviderUsage,
    finish_reason: &mut Option<ProviderFinishReason>,
    response_id: &mut Value,
    model_version: &mut Value,
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
        reasoning,
        tool_calls,
        usage,
        finish_reason,
        response_id,
        model_version,
    )
}

#[allow(clippy::too_many_arguments)]
fn process_stream_payload(
    payload: &Value,
    events: &mut Vec<ProviderStreamEvent>,
    text: &mut String,
    reasoning: &mut String,
    tool_calls: &mut Vec<ProviderToolCall>,
    usage: &mut ProviderUsage,
    finish_reason: &mut Option<ProviderFinishReason>,
    response_id: &mut Value,
    model_version: &mut Value,
) -> Result<()> {
    let payload = payload.get("response").unwrap_or(payload);
    if payload.get("error").is_some() {
        bail!("{}", provider_error_message(payload));
    }

    if let Some(value) = payload
        .get("responseId")
        .or_else(|| payload.get("response_id"))
        .filter(|value| !value.is_null())
    {
        *response_id = value.clone();
    }
    if let Some(value) = payload
        .get("modelVersion")
        .or_else(|| payload.get("model_version"))
        .filter(|value| !value.is_null())
    {
        *model_version = value.clone();
    }

    if let Some(usage_metadata) = payload
        .get("usageMetadata")
        .or_else(|| payload.get("usage_metadata"))
    {
        *usage = normalize_usage(usage_metadata);
    }

    let Some(candidates) = payload.get("candidates").and_then(Value::as_array) else {
        return Ok(());
    };

    for candidate in candidates {
        if let Some(reason) = candidate
            .get("finishReason")
            .or_else(|| candidate.get("finish_reason"))
            .and_then(Value::as_str)
        {
            *finish_reason = Some(normalize_finish_reason(Some(reason), tool_calls));
        }

        let Some(parts) = candidate
            .get("content")
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
        else {
            continue;
        };

        for part in parts {
            process_response_part(part, events, text, reasoning, tool_calls)?;
        }
    }

    Ok(())
}

fn process_response_part(
    part: &Value,
    events: &mut Vec<ProviderStreamEvent>,
    text: &mut String,
    reasoning: &mut String,
    tool_calls: &mut Vec<ProviderToolCall>,
) -> Result<()> {
    if let Some(function_call) = part
        .get("functionCall")
        .or_else(|| part.get("function_call"))
    {
        if let Some(call) = provider_tool_call_from_gemini(part, function_call, tool_calls.len()) {
            tool_calls.push(call);
        }
    }

    if let Some(delta) = part.get("text").and_then(Value::as_str) {
        if delta.is_empty() {
            return Ok(());
        }
        if part
            .get("thought")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            reasoning.push_str(delta);
            events.push(ProviderStreamEvent::ReasoningDelta {
                delta: delta.to_string(),
            });
        } else {
            text.push_str(delta);
            events.push(ProviderStreamEvent::TextDelta {
                delta: delta.to_string(),
            });
        }
    }

    if let Some(inline_data) = part
        .get("inlineData")
        .or_else(|| part.get("inline_data"))
        .and_then(Value::as_object)
    {
        if let Some(markdown_image) = markdown_image_from_inline_data(inline_data) {
            text.push_str(&markdown_image);
            events.push(ProviderStreamEvent::TextDelta {
                delta: markdown_image,
            });
        }
    }

    Ok(())
}

fn provider_tool_call_from_gemini(
    part: &Value,
    function_call: &Value,
    index: usize,
) -> Option<ProviderToolCall> {
    let object = function_call.as_object()?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if name.is_empty() {
        return None;
    }
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{name}-{index}"));
    let arguments = object
        .get("args")
        .or_else(|| object.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let provider_metadata = gemini_tool_call_provider_metadata(part, function_call);

    Some(ProviderToolCall {
        id,
        name: name.to_string(),
        arguments,
        provider_metadata,
    })
}

fn gemini_tool_call_provider_metadata(part: &Value, function_call: &Value) -> Value {
    let thought_signature = part
        .get("thoughtSignature")
        .or_else(|| part.get("thought_signature"))
        .or_else(|| function_call.get("thoughtSignature"))
        .or_else(|| function_call.get("thought_signature"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match thought_signature {
        Some(thought_signature) => json!({
            "gemini": {
                "thought_signature": thought_signature,
            }
        }),
        None => json!({}),
    }
}

fn markdown_image_from_inline_data(object: &Map<String, Value>) -> Option<String> {
    let mime_type = object
        .get("mimeType")
        .or_else(|| object.get("mime_type"))
        .and_then(Value::as_str)
        .unwrap_or("image/png");
    let data = object.get("data").and_then(Value::as_str)?;
    Some(format!("![image](data:{mime_type};base64,{data})"))
}

fn normalize_usage(metadata: &Value) -> ProviderUsage {
    let input_tokens = metadata
        .get("promptTokenCount")
        .or_else(|| metadata.get("prompt_token_count"))
        .and_then(number_or_none_ref);
    let cache_read_tokens = metadata
        .get("cachedContentTokenCount")
        .or_else(|| metadata.get("cached_content_token_count"))
        .and_then(number_or_none_ref);

    ProviderUsage {
        input_tokens,
        input_cache_hit_tokens: cache_read_tokens,
        input_cache_miss_tokens: input_tokens
            .zip(cache_read_tokens)
            .and_then(|(input, cached)| (input >= cached).then_some(input - cached)),
        output_tokens: metadata
            .get("candidatesTokenCount")
            .or_else(|| metadata.get("candidates_token_count"))
            .and_then(number_or_none_ref),
        reasoning_tokens: metadata
            .get("thoughtsTokenCount")
            .or_else(|| metadata.get("thoughts_token_count"))
            .and_then(number_or_none_ref),
        cache_read_tokens,
        cache_write_tokens: None,
        total_tokens: metadata
            .get("totalTokenCount")
            .or_else(|| metadata.get("total_token_count"))
            .and_then(number_or_none_ref),
    }
}

fn normalize_finish_reason(
    reason: Option<&str>,
    tool_calls: &[ProviderToolCall],
) -> ProviderFinishReason {
    let Some(reason) = reason else {
        return if tool_calls.is_empty() {
            ProviderFinishReason::Unknown
        } else {
            ProviderFinishReason::ToolCall
        };
    };

    match reason.trim().to_ascii_uppercase().as_str() {
        "STOP" => {
            if tool_calls.is_empty() {
                ProviderFinishReason::Stop
            } else {
                ProviderFinishReason::ToolCall
            }
        }
        "MAX_TOKENS" => ProviderFinishReason::Length,
        "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" => {
            ProviderFinishReason::ContentFilter
        }
        "MALFORMED_FUNCTION_CALL" => ProviderFinishReason::ToolCall,
        _ => ProviderFinishReason::Unknown,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    #[test]
    fn ac_002_native_max_output_tokens_maps_to_gemini_wire_field() {
        let input = ProviderInvocationInput {
            model: "gemini-2.5-flash".to_string(),
            model_parameters: BTreeMap::from([("max_output_tokens".to_string(), json!(512))]),
            ..Default::default()
        };

        let body = build_generate_content_body(&input).unwrap();
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 512);
    }

    #[tokio::test]
    async fn ac_005_validate_redacts_configured_proxy_url() {
        let (proxy_url, capture_handle) = capture_proxy_models_request();
        let response = handle_request(ProviderStdioRequest {
            method: "validate".to_string(),
            input: json!({
                "base_url": "http://gemini.example.test",
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
            request.starts_with("GET http://gemini.example.test/v1beta/models "),
            "validate request should be sent through the configured proxy"
        );
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn capture_generate_content_request() -> (String, thread::JoinHandle<String>) {
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
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        });
                    }
                }
                if let (Some(end), Some(length)) = (header_end, body_length) {
                    if buffer.len() >= end + length {
                        let response_body = "data: {\"responseId\":\"resp_gemini\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]},\"finishReason\":\"STOP\"}]}\n\n";
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                            response_body.len(),
                            response_body
                        )
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
                    let response_body = r#"{"models":[]}"#;
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

    #[test]
    fn client_protocol_envelope_uses_default_deny_policy_for_headers() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "gemini",
            "protocol": "gemini",
            "model": "gemini-2.5-flash",
            "client_protocol_envelope": {
                "source_protocol": "openai_chat",
                "policy": "default_deny",
                "headers": {
                    "authorization": "Bearer client-secret",
                    "x-goog-api-key": "client-api-key",
                    "x-client-name": "ClaudeCode",
                    "accept-encoding": "gzip"
                }
            }
        }))
        .unwrap();

        assert!(input.client_protocol_envelope.is_some());

        let config = normalize_provider_config(&json!({
            "api_key": "provider-secret",
            "auth_type": "api_key"
        }))
        .unwrap();
        let headers =
            build_headers(&config, true, input.client_protocol_envelope.as_ref()).unwrap();

        assert_eq!(headers.get("x-goog-api-key").unwrap(), "provider-secret");
        assert!(headers.get(AUTHORIZATION).is_none());
        assert!(headers.get("x-client-name").is_none());
        assert!(headers.get("accept-encoding").is_none());
    }

    #[test]
    fn headers_restore_anthropic_client_protocol_envelope_and_keep_config_auth() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "gemini",
            "protocol": "gemini",
            "model": "gemini-2.5-flash",
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
                    "x-goog-api-key": "client-auth-must-not-win"
                }
            }
        }))
        .unwrap();
        let config = normalize_provider_config(&json!({
            "api_key": "provider-secret",
            "auth_type": "api_key"
        }))
        .unwrap();
        let headers =
            build_headers(&config, true, input.client_protocol_envelope.as_ref()).unwrap();

        assert_eq!(headers.get("x-goog-api-key").unwrap(), "provider-secret");
        assert!(headers.get(AUTHORIZATION).is_none());
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
    fn normalizes_openai_tool_to_gemini_function_declaration() {
        let input = ProviderInvocationInput {
            model: "gemini-2.5-flash".to_string(),
            messages: vec![ProviderMessage {
                role: ProviderMessageRole::User,
                content: "hi".to_string(),
                content_blocks: None,
                name: None,
                tool_call_id: None,
                is_error: None,
                tool_calls: None,
            }],
            tools: vec![json!({
                "type": "function",
                "function": {
                    "name": "lookup",
                    "description": "Lookup a value",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" }
                        },
                        "additionalProperties": false
                    }
                }
            })],
            ..Default::default()
        };

        let body = build_generate_content_body(&input).unwrap();
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["name"],
            Value::String("lookup".to_string())
        );
        assert!(body["tools"][0]["functionDeclarations"][0]["parameters"]
            .get("additionalProperties")
            .is_none());
    }

    #[test]
    fn generate_content_body_replays_gemini_tool_call_thought_signature() {
        let input = ProviderInvocationInput {
            model: "gemini-3-flash-preview".to_string(),
            messages: vec![
                ProviderMessage {
                    role: ProviderMessageRole::User,
                    content: "Search files".to_string(),
                    content_blocks: None,
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: None,
                },
                ProviderMessage {
                    role: ProviderMessageRole::Assistant,
                    content: String::new(),
                    content_blocks: None,
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: Some(json!([{
                        "id": "call_glob",
                        "name": "Glob",
                        "arguments": { "pattern": ".memory/**/*.md" },
                        "provider_metadata": {
                            "gemini": {
                                "thought_signature": "real-gemini-signature"
                            }
                        }
                    }])),
                },
                ProviderMessage {
                    role: ProviderMessageRole::Tool,
                    content: "No files matched".to_string(),
                    content_blocks: None,
                    name: Some("Glob".to_string()),
                    tool_call_id: Some("call_glob".to_string()),
                    is_error: None,
                    tool_calls: None,
                },
            ],
            ..Default::default()
        };

        let body = build_generate_content_body(&input).unwrap();
        assert_eq!(
            body["contents"][1]["parts"][0]["thoughtSignature"],
            json!("real-gemini-signature")
        );
    }

    #[test]
    fn generate_content_body_omits_absent_gemini_thought_signature() {
        let input = ProviderInvocationInput {
            model: "gemini-3-flash-preview".to_string(),
            messages: vec![
                ProviderMessage {
                    role: ProviderMessageRole::User,
                    content: "Search files".to_string(),
                    content_blocks: None,
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: None,
                },
                ProviderMessage {
                    role: ProviderMessageRole::Assistant,
                    content: String::new(),
                    content_blocks: None,
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: Some(json!([{
                        "id": "call_glob",
                        "name": "Glob",
                        "arguments": { "pattern": ".memory/**/*.md" }
                    }])),
                },
                ProviderMessage {
                    role: ProviderMessageRole::Tool,
                    content: "No files matched".to_string(),
                    content_blocks: None,
                    name: Some("Glob".to_string()),
                    tool_call_id: Some("call_glob".to_string()),
                    is_error: None,
                    tool_calls: None,
                },
            ],
            ..Default::default()
        };

        let body = build_generate_content_body(&input).unwrap();
        assert!(body["contents"][1]["parts"][0]
            .get("thoughtSignature")
            .is_none());
    }

    #[test]
    fn generate_content_body_uses_prior_tool_call_name_for_tool_result() {
        let input = ProviderInvocationInput {
            model: "gemini-3-flash-preview".to_string(),
            messages: vec![
                ProviderMessage {
                    role: ProviderMessageRole::User,
                    content: "Search files".to_string(),
                    content_blocks: None,
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: None,
                },
                ProviderMessage {
                    role: ProviderMessageRole::Assistant,
                    content: String::new(),
                    content_blocks: None,
                    name: None,
                    tool_call_id: None,
                    is_error: None,
                    tool_calls: Some(json!([{
                        "id": "call_glob",
                        "name": "Glob",
                        "arguments": { "pattern": ".memory/**/*.md" },
                        "provider_metadata": {
                            "gemini": {
                                "thought_signature": "real-gemini-signature"
                            }
                        }
                    }])),
                },
                ProviderMessage {
                    role: ProviderMessageRole::Tool,
                    content: "No files matched".to_string(),
                    content_blocks: None,
                    name: None,
                    tool_call_id: Some("call_glob".to_string()),
                    is_error: None,
                    tool_calls: None,
                },
            ],
            ..Default::default()
        };

        let body = build_generate_content_body(&input).unwrap();
        assert_eq!(
            body["contents"][2]["parts"][0]["functionResponse"]["name"],
            json!("Glob")
        );
        assert_eq!(
            body["contents"][2]["parts"][0]["functionResponse"]["id"],
            json!("call_glob")
        );
    }

    #[test]
    fn generate_content_body_maps_content_blocks_image_to_inline_data() {
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "gemini",
            "protocol": "gemini",
            "model": "gemini-3-flash-preview",
            "messages": [
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {
                            "id": "call_read",
                            "name": "Read",
                            "arguments": { "file_path": "image.png" }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_read",
                    "name": "Read",
                    "content": "",
                    "content_blocks": [
                        {
                            "type": "image",
                            "source": {
                                "data": "aW1hZ2U="
                            }
                        }
                    ]
                }
            ]
        }))
        .unwrap();

        let body = build_generate_content_body(&input).unwrap();
        let parts = body["contents"][1]["parts"]
            .as_array()
            .expect("tool result should have parts");
        let image_part = parts
            .iter()
            .find(|part| part.get("inlineData").is_some())
            .expect("tool result should include inline image data");

        assert_eq!(image_part["inlineData"]["mimeType"], json!("image/png"));
        assert_eq!(image_part["inlineData"]["data"], json!("aW1hZ2U="));
    }

    #[test]
    fn parses_gemini_tool_call_thought_signature_into_provider_metadata() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut tool_calls = Vec::new();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = None;
        let mut response_id = Value::Null;
        let mut model_version = Value::Null;

        process_sse_line(
            r#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"Glob","args":{"pattern":".memory/**/*.md"},"id":"call_glob"},"thoughtSignature":"real-gemini-signature"}]},"finishReason":"MALFORMED_FUNCTION_CALL"}]}"#,
            &mut events,
            &mut text,
            &mut reasoning,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
            &mut model_version,
        )
        .unwrap();

        assert_eq!(tool_calls.len(), 1);
        assert_eq!(
            tool_calls[0].provider_metadata["gemini"]["thought_signature"],
            json!("real-gemini-signature")
        );
    }

    #[test]
    fn parses_gemini_sse_text_usage_and_finish() {
        let mut events = Vec::new();
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut tool_calls = Vec::new();
        let mut usage = ProviderUsage::default();
        let mut finish_reason = None;
        let mut response_id = Value::Null;
        let mut model_version = Value::Null;

        process_sse_line(
            r#"data: {"responseId":"resp_gemini","candidates":[{"content":{"parts":[{"text":"hello"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":2,"candidatesTokenCount":3,"totalTokenCount":5}}"#,
            &mut events,
            &mut text,
            &mut reasoning,
            &mut tool_calls,
            &mut usage,
            &mut finish_reason,
            &mut response_id,
            &mut model_version,
        )
        .unwrap();

        assert_eq!(text, "hello");
        assert_eq!(usage.input_tokens, Some(2));
        assert_eq!(usage.output_tokens, Some(3));
        assert_eq!(finish_reason, Some(ProviderFinishReason::Stop));
        assert_eq!(response_id, json!("resp_gemini"));
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn gemini_streaming_result_keeps_response_id_metadata_only() {
        let response = reqwest::get(start_sse_server(
            "data: {\"responseId\":\"resp_gemini\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hello\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":2,\"candidatesTokenCount\":3,\"totalTokenCount\":5}}\n\n",
        ))
        .await
        .unwrap();
        let mut events = Vec::new();

        let envelope = read_streaming_generate_content(
            response,
            "gemini-2.5-flash".to_string(),
            "test-key",
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
            envelope.result.provider_metadata["response_id"],
            json!("resp_gemini")
        );
        assert!(events.contains(&ProviderStreamEvent::Finish {
            reason: ProviderFinishReason::Stop
        }));
    }

    #[tokio::test]
    async fn ac_002_fake_upstream_receives_exact_generate_wire() {
        let (base_url, capture_handle) = capture_generate_content_request();
        let input: ProviderInvocationInput = serde_json::from_value(json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "gemini",
            "protocol": "gemini",
            "model": "gemini-2.5-flash",
            "provider_config": {
                "base_url": base_url,
                "api_key": "test-key",
                "auth_type": "api_key"
            },
            "system": [{ "type": "text", "text": "Be concise" }],
            "messages": [{ "role": "user", "content": "hello" }]
        }))
        .unwrap();

        invoke_generate_content(input)
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
                "contents": [{
                    "role": "user",
                    "parts": [{ "text": "hello" }]
                }],
                "systemInstruction": {
                    "parts": [{ "text": "Be concise" }]
                }
            })
        );
    }

    #[test]
    fn ac_002_generate_contract_accepts_only_current_strict_input() {
        let missing = serde_json::from_value::<ProviderInvocationInput>(json!({
            "model": "gemini-2.5-flash"
        }))
        .expect_err("missing current contract must fail before provider invocation");
        assert!(missing.to_string().contains("contract_version"));

        let current = json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test",
            "provider_code": "gemini",
            "protocol": "gemini",
            "model": "gemini-2.5-flash"
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
            "provider_code": "gemini",
            "protocol": "gemini",
            "model": "gemini-2.5-flash",
            "messages": [{ "role": "user", "content": "hello" }],
            "required_capabilities": ["system_prompt_cache_control"]
        }))
        .unwrap();

        let error = build_generate_content_body(&input)
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
