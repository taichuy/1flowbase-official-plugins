//! Tencent iLink notification supplier. Host owns all persistence and trusted call scope.
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::{
    execute_authorizable_notification, CapabilityStdioRequest, NotificationAuthorizationActionKind,
    NotificationAuthorizationOperation, NotificationAuthorizationStatus,
    NotificationAuthorizationUserAction, NotificationAuthorizationView, NotificationFailureCode,
    NotificationOperation, NotificationRuntimeInput, NotificationRuntimeResult,
    NotificationSendOutcome, NotificationTypeDescriptor,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, future::Future, pin::Pin, time::Duration};

const DEFAULT_BASE: &str = "https://ilinkai.weixin.qq.com";
const SECRET_KEYS: &[&str] = &[
    "qr_credential",
    "qr_code_url",
    "qr_base_url",
    "bot_token",
    "bot_id",
    "base_url",
    "bound_user_id",
    "cursor",
    "context_token",
    "authorization_invalid",
];
type Secrets = BTreeMap<String, Value>;
type ProviderFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, ProviderError>> + 'a>>;

// Deliberately carries neither credentials nor a raw provider error.
#[derive(Debug)]
pub enum ProviderError {
    Network,
    Timeout,
    Malformed,
    UnsafeUrl,
}
struct Request {
    base_url: String,
    endpoint: &'static str,
    query: Vec<(String, String)>,
    token: Option<String>,
    body: Option<Value>,
    timeout: Duration,
}
trait ILink {
    fn request(&self, request: Request) -> ProviderFuture<'_>;
}

pub struct HttpClient {
    client: reqwest::Client,
}
impl HttpClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder()
                // HTTP redirects must not forward credentials or escape the allowlist.
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .build()?,
        })
    }
}
impl ILink for HttpClient {
    fn request(&self, request: Request) -> ProviderFuture<'_> {
        Box::pin(async move {
            let base = trusted_base(&request.base_url)?;
            let url = base
                .join(request.endpoint)
                .map_err(|_| ProviderError::UnsafeUrl)?;
            let mut builder = if let Some(mut body) = request.body {
                body["base_info"] =
                    json!({"channel_version":"1.0.0", "bot_agent":"1flowbase/1.0.0"});
                let id = uuid::Uuid::new_v4();
                let uin =
                    u32::from_be_bytes(id.as_bytes()[..4].try_into().expect("four UUID bytes"));
                self.client
                    .post(url)
                    .json(&body)
                    .header("AuthorizationType", "ilink_bot_token")
                    .header("X-WECHAT-UIN", STANDARD.encode(uin.to_string()))
            } else {
                self.client.get(url)
            };
            builder = builder
                .query(&request.query)
                .header("iLink-App-Id", "bot")
                .header("iLink-App-ClientVersion", "65536")
                .timeout(request.timeout);
            if let Some(token) = request.token {
                builder = builder.bearer_auth(token);
            }
            let response = builder.send().await.map_err(http_error)?;
            let successful_http = response.status().is_success();
            let value: Value = response.json().await.map_err(|error| {
                if error.is_timeout() {
                    ProviderError::Timeout
                } else {
                    ProviderError::Malformed
                }
            })?;
            // A non-2xx response with no definite protocol error cannot prove delivery.
            if !successful_http && !matches!(provider_code(&value), ProviderCode::Failure(_)) {
                return Err(ProviderError::Network);
            }
            Ok(value)
        })
    }
}
fn http_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else {
        ProviderError::Network
    }
}
fn trusted_base(raw: &str) -> Result<reqwest::Url, ProviderError> {
    let url = reqwest::Url::parse(raw).map_err(|_| ProviderError::UnsafeUrl)?;
    let host = url.host_str().unwrap_or("");
    if url.scheme() != "https"
        || !host.ends_with(".weixin.qq.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ProviderError::UnsafeUrl);
    }
    Ok(url)
}
fn secret<'a>(secrets: &'a Secrets, key: &str) -> Option<&'a str> {
    secrets
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}
fn clear(keys: &[&str]) -> Secrets {
    keys.iter()
        .map(|key| (key.to_string(), Value::Null))
        .collect()
}
const QR_KEYS: &[&str] = &["qr_credential", "qr_code_url", "qr_base_url"];
const ACCOUNT_KEYS: &[&str] = &[
    "bot_token",
    "bot_id",
    "base_url",
    "bound_user_id",
    "cursor",
    "context_token",
];
fn invalid_patch() -> Secrets {
    let mut patch = clear(ACCOUNT_KEYS);
    patch.insert("authorization_invalid".into(), json!(true));
    patch
}
fn has_account(secrets: &Secrets) -> bool {
    ["bot_token", "bot_id", "base_url", "bound_user_id"]
        .iter()
        .all(|key| secret(secrets, key).is_some())
}
fn status(secrets: &Secrets) -> NotificationAuthorizationStatus {
    if has_account(secrets) {
        NotificationAuthorizationStatus::Authorized
    } else if secrets.get("authorization_invalid") == Some(&json!(true)) {
        NotificationAuthorizationStatus::Invalid
    } else {
        NotificationAuthorizationStatus::Unauthorized
    }
}
fn plain_view(status: NotificationAuthorizationStatus) -> NotificationAuthorizationView {
    NotificationAuthorizationView {
        status,
        user_action: None,
    }
}
fn pending(secrets: &Secrets) -> Result<NotificationAuthorizationView, ProviderError> {
    let raw = secret(secrets, "qr_code_url").ok_or(ProviderError::Malformed)?;
    let url = reqwest::Url::parse(raw).map_err(|_| ProviderError::Malformed)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ProviderError::Malformed);
    }
    Ok(NotificationAuthorizationView {
        status: NotificationAuthorizationStatus::Pending,
        user_action: Some(NotificationAuthorizationUserAction {
            kind: NotificationAuthorizationActionKind::QrCode,
            qr_code_url: raw.to_string(),
            poll_interval_seconds: 2,
            expires_at: None,
        }),
    })
}
fn authorization(
    authorization: NotificationAuthorizationView,
    managed_secret_patch: Secrets,
) -> NotificationRuntimeResult {
    NotificationRuntimeResult::Authorization {
        authorization,
        managed_secret_patch,
    }
}
fn send_result(
    outcome: NotificationSendOutcome,
    state: NotificationAuthorizationStatus,
    patch: Secrets,
) -> NotificationRuntimeResult {
    NotificationRuntimeResult::Send {
        outcome,
        authorization: Some(plain_view(state)),
        managed_secret_patch: patch,
    }
}
fn failed(
    code: NotificationFailureCode,
    state: NotificationAuthorizationStatus,
    patch: Secrets,
) -> NotificationRuntimeResult {
    send_result(
        NotificationSendOutcome::Failed { failure_code: code },
        state,
        patch,
    )
}
#[derive(PartialEq, Eq)]
enum ProviderCode {
    Success,
    Failure(NotificationFailureCode),
    Unknown,
}
fn provider_code(value: &Value) -> ProviderCode {
    // A missing or malformed status cannot prove that the provider accepted a send.
    let ret = value.get("ret").and_then(Value::as_i64);
    let error = value.get("errcode").and_then(Value::as_i64);
    if ret == Some(-14) || error == Some(-14) {
        return ProviderCode::Failure(NotificationFailureCode::AuthorizationInvalid);
    }
    if ret.is_some_and(|v| v != 0) || error.is_some_and(|v| v != 0) {
        let message = value["errmsg"].as_str().unwrap_or("").to_ascii_lowercase();
        let code = if ret == Some(-2) || error == Some(-2) || message.contains("prepare failed") {
            NotificationFailureCode::RecipientNotReady
        } else {
            NotificationFailureCode::ProviderRejected
        };
        return ProviderCode::Failure(code);
    }
    if ret == Some(0) && (value.get("errcode").is_none() || error == Some(0)) {
        ProviderCode::Success
    } else {
        ProviderCode::Unknown
    }
}
fn request(
    secrets: &Secrets,
    endpoint: &'static str,
    body: Value,
    timeout: Duration,
) -> Result<Request, ProviderError> {
    let base_url = secret(secrets, "base_url")
        .ok_or(ProviderError::Malformed)?
        .to_string();
    trusted_base(&base_url)?;
    Ok(Request {
        base_url,
        endpoint,
        query: vec![],
        token: secret(secrets, "bot_token").map(str::to_string),
        body: Some(body),
        timeout,
    })
}
// One bounded preparation poll; other peers cannot rebind or replace the scanner's context.
async fn refresh(client: &impl ILink, secrets: &Secrets) -> Result<Secrets, ProviderError> {
    let req = request(
        secrets,
        "ilink/bot/getupdates",
        json!({"get_updates_buf":secret(secrets,"cursor").unwrap_or("")}),
        Duration::from_secs(1),
    )?;
    let response = match client.request(req).await {
        Err(ProviderError::Timeout) => return Ok(Secrets::new()),
        result => result?,
    };
    match provider_code(&response) {
        ProviderCode::Failure(NotificationFailureCode::AuthorizationInvalid) => {
            return Ok(invalid_patch())
        }
        ProviderCode::Success => {}
        _ => return Err(ProviderError::Malformed),
    }
    let mut patch = Secrets::new();
    if let Some(cursor) = response["get_updates_buf"].as_str() {
        patch.insert("cursor".into(), json!(cursor));
    }
    if let Some(messages) = response["msgs"].as_array() {
        for message in messages {
            if message["from_user_id"].as_str() == secret(secrets, "bound_user_id") {
                if let Some(context) = message["context_token"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                {
                    patch.insert("context_token".into(), json!(context));
                }
            }
        }
    }
    Ok(patch)
}

async fn authorize(
    client: &impl ILink,
    operation: NotificationAuthorizationOperation,
    secrets: Secrets,
) -> Result<NotificationRuntimeResult, ProviderError> {
    match operation {
        NotificationAuthorizationOperation::Begin { .. } => {
            let response = client
                .request(Request {
                    base_url: DEFAULT_BASE.into(),
                    endpoint: "ilink/bot/get_bot_qrcode",
                    query: vec![("bot_type".into(), "3".into())],
                    token: None,
                    body: None,
                    timeout: Duration::from_secs(15),
                })
                .await?;
            let qr = response["qrcode"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or(ProviderError::Malformed)?;
            let url = response["qrcode_img_content"]
                .as_str()
                .ok_or(ProviderError::Malformed)?;
            let mut patch = clear(SECRET_KEYS);
            patch.insert("qr_credential".into(), json!(qr));
            patch.insert("qr_code_url".into(), json!(url));
            patch.insert("qr_base_url".into(), json!(DEFAULT_BASE));
            Ok(authorization(pending(&patch)?, patch))
        }
        NotificationAuthorizationOperation::Poll => {
            let Some(qr) = secret(&secrets, "qr_credential") else {
                return Ok(authorization(plain_view(status(&secrets)), Secrets::new()));
            };
            let base = secret(&secrets, "qr_base_url").unwrap_or(DEFAULT_BASE);
            trusted_base(base)?;
            let response = client
                .request(Request {
                    base_url: base.into(),
                    endpoint: "ilink/bot/get_qrcode_status",
                    query: vec![("qrcode".into(), qr.into())],
                    token: None,
                    body: None,
                    timeout: Duration::from_secs(15),
                })
                .await?;
            match response["status"].as_str() {
                Some("wait" | "scaned") => Ok(authorization(pending(&secrets)?, Secrets::new())),
                Some("scaned_but_redirect" | "redirect") => {
                    let host = response["redirect_host"]
                        .as_str()
                        .ok_or(ProviderError::Malformed)?;
                    let base = format!("https://{host}");
                    trusted_base(&base)?;
                    Ok(authorization(
                        pending(&secrets)?,
                        BTreeMap::from([("qr_base_url".into(), json!(base))]),
                    ))
                }
                Some("expired") => Ok(authorization(plain_view(status(&secrets)), clear(QR_KEYS))),
                Some("confirmed") => {
                    let mut patch = clear(SECRET_KEYS);
                    for (key, field) in [
                        ("bot_token", "bot_token"),
                        ("bot_id", "ilink_bot_id"),
                        ("bound_user_id", "ilink_user_id"),
                    ] {
                        let value = response[field]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .ok_or(ProviderError::Malformed)?;
                        patch.insert(key.into(), json!(value));
                    }
                    let base = response["baseurl"].as_str().unwrap_or(DEFAULT_BASE);
                    trusted_base(base)?;
                    patch.insert("base_url".into(), json!(base));
                    Ok(authorization(
                        plain_view(NotificationAuthorizationStatus::Authorized),
                        patch,
                    ))
                }
                _ => Err(ProviderError::Malformed),
            }
        }
        NotificationAuthorizationOperation::Cancel => {
            Ok(authorization(plain_view(status(&secrets)), clear(QR_KEYS)))
        }
        NotificationAuthorizationOperation::Maintain => {
            if secret(&secrets, "qr_credential").is_some() {
                return Ok(authorization(pending(&secrets)?, Secrets::new()));
            }
            if !has_account(&secrets) {
                return Ok(authorization(plain_view(status(&secrets)), Secrets::new()));
            }
            let patch = refresh(client, &secrets).await?;
            let state = if patch.contains_key("authorization_invalid") {
                NotificationAuthorizationStatus::Invalid
            } else {
                NotificationAuthorizationStatus::Authorized
            };
            Ok(authorization(plain_view(state), patch))
        }
    }
}
async fn send_notification(
    client: &impl ILink,
    secrets: Secrets,
    input: Value,
) -> Result<NotificationRuntimeResult, ProviderError> {
    if !has_account(&secrets) {
        let state = status(&secrets);
        let code = if state == NotificationAuthorizationStatus::Invalid {
            NotificationFailureCode::AuthorizationInvalid
        } else {
            NotificationFailureCode::AuthorizationRequired
        };
        return Ok(failed(code, state, Secrets::new()));
    }
    // This is preparation only: loss here does not imply an ambiguous external send.
    // Keep saved context and still attempt sending after any unavailable preparation response.
    let mut patch = refresh(client, &secrets).await.unwrap_or_default();
    if patch.contains_key("authorization_invalid") {
        return Ok(failed(
            NotificationFailureCode::AuthorizationInvalid,
            NotificationAuthorizationStatus::Invalid,
            patch,
        ));
    }
    let client_id = format!("1flowbase-{}", uuid::Uuid::new_v4());
    let mut message = json!({"from_user_id":"", "to_user_id":secret(&secrets,"bound_user_id"),
        "client_id":client_id, "message_type":2, "message_state":2,
        "item_list":[{"type":1,"text_item":{"text":input["text"]}}]});
    if let Some(context) =
        secret(&patch, "context_token").or_else(|| secret(&secrets, "context_token"))
    {
        message["context_token"] = json!(context);
    }
    let req = request(
        &secrets,
        "ilink/bot/sendmessage",
        json!({"msg":message}),
        Duration::from_secs(15),
    )?;
    // Never retry an ambiguous send. A supplier-confirmed rejection is not an acceptance.
    let code = match client.request(req).await {
        Ok(response) => provider_code(&response),
        Err(_) => ProviderCode::Unknown,
    };
    let result = match code {
        ProviderCode::Success => send_result(
            NotificationSendOutcome::Succeeded {
                result: json!({"accepted":true,"delivery_id":client_id}),
            },
            NotificationAuthorizationStatus::Authorized,
            patch,
        ),
        ProviderCode::Failure(NotificationFailureCode::AuthorizationInvalid) => {
            patch.extend(invalid_patch());
            failed(
                NotificationFailureCode::AuthorizationInvalid,
                NotificationAuthorizationStatus::Invalid,
                patch,
            )
        }
        ProviderCode::Failure(code) => {
            failed(code, NotificationAuthorizationStatus::Authorized, patch)
        }
        ProviderCode::Unknown => send_result(
            NotificationSendOutcome::unknown(),
            NotificationAuthorizationStatus::Authorized,
            patch,
        ),
    };
    Ok(result)
}
async fn run(
    client: &impl ILink,
    input: NotificationRuntimeInput,
) -> Result<NotificationRuntimeResult, ProviderError> {
    match input.operation {
        NotificationOperation::Authorize { operation } => {
            authorize(client, operation, input.managed_secrets).await
        }
        NotificationOperation::Send { input_payload } => {
            send_notification(client, input.managed_secrets, input_payload).await
        }
    }
}
pub fn descriptor() -> NotificationTypeDescriptor {
    let manifest: Value =
        serde_yaml::from_str(include_str!("../manifest.yaml")).expect("compiled manifest");
    serde_json::from_value(manifest["notification_types"][0].clone()).expect("compiled descriptor")
}
pub async fn dispatch(client: &HttpClient, request: Value) -> Value {
    let request: CapabilityStdioRequest = match serde_json::from_value(request) {
        Ok(request) => request,
        Err(_) => {
            return json!({"ok":false,"result":null,"error":{"message":"invalid notification request"}})
        }
    };
    let response =
        execute_authorizable_notification(request, &descriptor(), |_, input| run(client, input))
            .await;
    serde_json::to_value(response).expect("typed SDK response")
}
#[cfg(test)]
#[path = "_tests/behavior.rs"]
mod tests;
