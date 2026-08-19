use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

use crate::{
    ProviderAuthOperation, ProviderAuthResult, ProviderAuthRuntimeInput, ProviderAuthStatus,
    ProviderAuthUserAction, ProviderAuthUserActionKind,
};

const CHATGPT_OAUTH_AUTHORITY: &str = "https://auth.openai.com";
const CODEX_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const DEFAULT_PKCE_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const DEVICE_CODE_LIFETIME: Duration = Duration::minutes(15);
const REFRESH_EARLY_WINDOW: Duration = Duration::seconds(60);

#[derive(Debug, Clone)]
struct AuthConfig {
    authority: String,
    redirect_uri: String,
    proxy_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_auth_id: String,
    #[serde(alias = "user_code", alias = "usercode")]
    user_code: String,
    #[serde(default, deserialize_with = "deserialize_poll_interval")]
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct DevicePollResponse {
    authorization_code: String,
    code_verifier: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

pub(crate) async fn authenticate(input: ProviderAuthRuntimeInput) -> Result<ProviderAuthResult> {
    let auth = normalize_auth_config(&input.provider_config)?;
    match input.operation {
        ProviderAuthOperation::Begin { action } => match action.as_str() {
            "device_code" => begin_device_code(&auth, &input.provider_config).await,
            "pkce_callback" => begin_pkce_callback(&auth, &input.provider_config),
            _ => bail!("unsupported ChatGPT authentication action"),
        },
        ProviderAuthOperation::Poll => poll_device_code(&auth, &input.provider_config).await,
        ProviderAuthOperation::Submit { value } => {
            submit_pkce_callback(&auth, &input.provider_config, &value).await
        }
        ProviderAuthOperation::Cancel => Ok(cancel_pending_auth()),
        ProviderAuthOperation::Maintain => {
            maintain_access_token(&auth, &input.provider_config).await
        }
    }
}

async fn begin_device_code(
    auth: &AuthConfig,
    provider_config: &Value,
) -> Result<ProviderAuthResult> {
    let client = auth_client(auth)?;
    let endpoint = authority_path(&auth.authority, "/api/accounts/deviceauth/usercode")?;
    let response = client
        .post(endpoint)
        .json(&json!({ "client_id": CODEX_OAUTH_CLIENT_ID }))
        .send()
        .await
        .context("ChatGPT device-code request failed")?;
    if !response.status().is_success() {
        return Ok(failed_auth("ChatGPT device-code login is unavailable"));
    }
    let device: DeviceCodeResponse = response
        .json()
        .await
        .context("ChatGPT device-code response is invalid")?;
    if device.device_auth_id.trim().is_empty() || device.user_code.trim().is_empty() {
        bail!("ChatGPT device-code response is incomplete");
    }
    let expires_at = expires_at_from_now(DEVICE_CODE_LIFETIME)?;
    let poll_interval_seconds = device.interval.clamp(1, 60);
    let mut patch = clear_pending_patch();
    patch.insert("device_auth_id".to_string(), json!(device.device_auth_id));
    patch.insert("device_user_code".to_string(), json!(device.user_code));
    patch.insert("device_expires_at".to_string(), json!(expires_at));
    patch.insert(
        "device_poll_interval".to_string(),
        json!(poll_interval_seconds),
    );
    patch.insert(
        "device_verification_url".to_string(),
        json!(device_verification_url(auth)?),
    );
    patch.insert(
        "instance_cookie_key".to_string(),
        json!(instance_cookie_key(provider_config)),
    );
    Ok(ProviderAuthResult {
        status: ProviderAuthStatus::Pending,
        message: None,
        user_action: Some(ProviderAuthUserAction {
            kind: ProviderAuthUserActionKind::DeviceCode,
            open_url: patch
                .get("device_verification_url")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            user_code: patch
                .get("device_user_code")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            expires_at: patch
                .get("device_expires_at")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            poll_interval_seconds: Some(poll_interval_seconds),
            prompt: Some(
                "Open the verification URL, sign in to ChatGPT, and enter the device code."
                    .to_string(),
            ),
        }),
        managed_secret_patch: patch,
    })
}

async fn poll_device_code(
    auth: &AuthConfig,
    provider_config: &Value,
) -> Result<ProviderAuthResult> {
    if pending_device_code_expired(provider_config) {
        return Ok(ProviderAuthResult {
            status: ProviderAuthStatus::Failed,
            message: Some(
                "The ChatGPT device code expired. Start a new sign-in attempt.".to_string(),
            ),
            user_action: None,
            managed_secret_patch: clear_pending_patch(),
        });
    }
    let device_auth_id = required_secret(provider_config, "device_auth_id")?;
    let user_code = required_secret(provider_config, "device_user_code")?;
    let client = auth_client(auth)?;
    let endpoint = authority_path(&auth.authority, "/api/accounts/deviceauth/token")?;
    let response = client
        .post(endpoint)
        .json(&json!({
            "device_auth_id": device_auth_id,
            "user_code": user_code,
        }))
        .send()
        .await
        .context("ChatGPT device-code poll failed")?;
    if matches!(response.status().as_u16(), 403 | 404) {
        return Ok(pending_device_result(provider_config));
    }
    if !response.status().is_success() {
        return Ok(failed_auth("ChatGPT device-code authorization failed"));
    }
    let grant: DevicePollResponse = response
        .json()
        .await
        .context("ChatGPT device-code grant is invalid")?;
    let token = exchange_authorization_code(
        auth,
        &grant.authorization_code,
        &grant.code_verifier,
        &authority_path(&auth.authority, "/deviceauth/callback")?,
    )
    .await?;
    Ok(authorize_with_tokens(token, provider_config))
}

fn begin_pkce_callback(auth: &AuthConfig, provider_config: &Value) -> Result<ProviderAuthResult> {
    let verifier = random_secret(48)?;
    let state = random_secret(24)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = Url::parse(&authority_path(&auth.authority, "/oauth/authorize")?)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CODEX_OAUTH_CLIENT_ID)
        .append_pair("redirect_uri", &auth.redirect_uri)
        .append_pair("scope", "openid profile email offline_access")
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true");
    let mut patch = clear_pending_patch();
    patch.insert("pkce_state".to_string(), json!(state));
    patch.insert("pkce_code_verifier".to_string(), json!(verifier));
    patch.insert("pkce_redirect_uri".to_string(), json!(&auth.redirect_uri));
    patch.insert(
        "instance_cookie_key".to_string(),
        json!(instance_cookie_key(provider_config)),
    );
    Ok(ProviderAuthResult {
        status: ProviderAuthStatus::Pending,
        message: None,
        user_action: Some(ProviderAuthUserAction {
            kind: ProviderAuthUserActionKind::PasteCallbackUrl,
            open_url: Some(url.to_string()),
            user_code: None,
            expires_at: None,
            poll_interval_seconds: None,
            prompt: Some("Finish the browser sign-in, then paste the complete callback URL or authorization code."
                .to_string()),
        }),
        managed_secret_patch: patch,
    })
}

async fn submit_pkce_callback(
    auth: &AuthConfig,
    provider_config: &Value,
    value: &str,
) -> Result<ProviderAuthResult> {
    let expected_state = required_secret(provider_config, "pkce_state")?;
    let verifier = required_secret(provider_config, "pkce_code_verifier")?;
    let redirect_uri = required_secret(provider_config, "pkce_redirect_uri")?;
    let (code, callback_state) = parse_callback_submission(value)?;
    if callback_state.as_deref() != Some(expected_state.as_str()) {
        return Ok(failed_auth(
            "The OAuth callback state does not match this sign-in attempt",
        ));
    }
    let token = exchange_authorization_code(auth, &code, &verifier, &redirect_uri).await?;
    Ok(authorize_with_tokens(token, provider_config))
}

async fn maintain_access_token(
    auth: &AuthConfig,
    provider_config: &Value,
) -> Result<ProviderAuthResult> {
    let Some(access_token) = optional_secret(provider_config, "access_token") else {
        return Ok(failed_auth("ChatGPT sign-in is required"));
    };
    if !token_refresh_is_required(provider_config, &access_token) {
        return Ok(authorized_without_patch());
    }
    let Some(refresh_token) = optional_secret(provider_config, "refresh_token") else {
        return Ok(failed_auth(
            "ChatGPT sign-in expired and requires reauthentication",
        ));
    };
    let client = auth_client(auth)?;
    let endpoint = authority_path(&auth.authority, "/oauth/token")?;
    let response = client
        .post(endpoint)
        .json(&json!({
            "client_id": CODEX_OAUTH_CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .context("ChatGPT token refresh failed")?;
    if !response.status().is_success() {
        return Ok(failed_auth("ChatGPT token refresh failed; sign in again"));
    }
    let token: TokenResponse = response
        .json()
        .await
        .context("ChatGPT refresh response is invalid")?;
    Ok(authorize_with_tokens(token, provider_config))
}

async fn exchange_authorization_code(
    auth: &AuthConfig,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    let client = auth_client(auth)?;
    let endpoint = authority_path(&auth.authority, "/oauth/token")?;
    let response = client
        .post(endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CODEX_OAUTH_CLIENT_ID),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .context("ChatGPT authorization-code exchange failed")?;
    if !response.status().is_success() {
        bail!("ChatGPT authorization-code exchange was rejected");
    }
    response
        .json()
        .await
        .context("ChatGPT token exchange response is invalid")
}

fn authorize_with_tokens(token: TokenResponse, provider_config: &Value) -> ProviderAuthResult {
    let mut patch = clear_pending_patch();
    patch.insert("access_token".to_string(), json!(token.access_token));
    if let Some(refresh_token) = token.refresh_token.filter(|value| !value.trim().is_empty()) {
        patch.insert("refresh_token".to_string(), json!(refresh_token));
    }
    if let Some(id_token) = token.id_token.filter(|value| !value.trim().is_empty()) {
        if let Some(account_id) = chatgpt_account_id(&id_token) {
            patch.insert("chatgpt_account_id".to_string(), json!(account_id));
        }
        patch.insert("id_token".to_string(), json!(id_token));
    }
    let access_token = patch
        .get("access_token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if patch.get("chatgpt_account_id").is_none() {
        if let Some(account_id) = chatgpt_account_id(&access_token) {
            patch.insert("chatgpt_account_id".to_string(), json!(account_id));
        }
    }
    let expires_at = token
        .expires_in
        .filter(|value| *value > 0)
        .and_then(|seconds| expires_at_from_now(Duration::seconds(seconds)).ok())
        .or_else(|| jwt_expiry(&access_token).and_then(|value| value.format(&Rfc3339).ok()));
    if let Some(expires_at) = expires_at {
        patch.insert("expires_at".to_string(), json!(expires_at));
    }
    patch
        .entry("instance_cookie_key".to_string())
        .or_insert_with(|| json!(instance_cookie_key(provider_config)));
    ProviderAuthResult {
        status: ProviderAuthStatus::Authorized,
        message: None,
        user_action: None,
        managed_secret_patch: patch,
    }
}

fn pending_device_result(provider_config: &Value) -> ProviderAuthResult {
    ProviderAuthResult {
        status: ProviderAuthStatus::Pending,
        message: None,
        user_action: Some(ProviderAuthUserAction {
            kind: ProviderAuthUserActionKind::DeviceCode,
            open_url: optional_secret(provider_config, "device_verification_url"),
            user_code: optional_secret(provider_config, "device_user_code"),
            expires_at: optional_secret(provider_config, "device_expires_at"),
            poll_interval_seconds: provider_config
                .get("device_poll_interval")
                .and_then(Value::as_u64),
            prompt: Some(
                "Complete the device-code sign-in in your browser, then keep polling.".to_string(),
            ),
        }),
        managed_secret_patch: BTreeMap::new(),
    }
}

fn cancel_pending_auth() -> ProviderAuthResult {
    ProviderAuthResult {
        status: ProviderAuthStatus::Cancelled,
        message: None,
        user_action: None,
        managed_secret_patch: clear_pending_patch(),
    }
}

fn authorized_without_patch() -> ProviderAuthResult {
    ProviderAuthResult {
        status: ProviderAuthStatus::Authorized,
        message: None,
        user_action: None,
        managed_secret_patch: BTreeMap::new(),
    }
}

fn failed_auth(message: &str) -> ProviderAuthResult {
    ProviderAuthResult {
        status: ProviderAuthStatus::Failed,
        message: Some(message.to_string()),
        user_action: None,
        managed_secret_patch: BTreeMap::new(),
    }
}

fn clear_pending_patch() -> BTreeMap<String, Value> {
    [
        "device_auth_id",
        "device_user_code",
        "device_expires_at",
        "device_poll_interval",
        "device_verification_url",
        "device_code_verifier",
        "pkce_state",
        "pkce_code_verifier",
        "pkce_redirect_uri",
    ]
    .into_iter()
    .map(|key| (key.to_string(), Value::Null))
    .collect()
}

fn normalize_auth_config(provider_config: &Value) -> Result<AuthConfig> {
    let config = provider_config
        .as_object()
        .ok_or_else(|| anyhow!("provider_config must be an object"))?;
    let authority = optional_config_text(config.get("auth_base_url"))
        .unwrap_or_else(|| CHATGPT_OAUTH_AUTHORITY.to_string());
    let parsed = Url::parse(&authority).context("invalid auth_base_url")?;
    if parsed.scheme() != "https"
        && !matches!(parsed.host_str(), Some("127.0.0.1") | Some("localhost"))
    {
        bail!("auth_base_url must use HTTPS outside controlled loopback fixtures");
    }
    Ok(AuthConfig {
        authority: authority.trim_end_matches('/').to_string(),
        redirect_uri: optional_config_text(config.get("pkce_redirect_uri"))
            .unwrap_or_else(|| DEFAULT_PKCE_REDIRECT_URI.to_string()),
        proxy_url: optional_config_text(config.get("proxy_url")),
    })
}

fn auth_client(auth: &AuthConfig) -> Result<Client> {
    let mut builder = Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30));
    if let Some(proxy_url) = &auth.proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy_url")?);
    }
    builder.build().context("building ChatGPT auth client")
}

fn authority_path(authority: &str, path: &str) -> Result<String> {
    let base = Url::parse(authority).context("invalid ChatGPT auth authority")?;
    Ok(base.join(path)?.to_string())
}

fn device_verification_url(auth: &AuthConfig) -> Result<String> {
    authority_path(&auth.authority, "/codex/device")
}

fn required_secret(provider_config: &Value, key: &str) -> Result<String> {
    optional_secret(provider_config, key).ok_or_else(|| anyhow!("{key} is required"))
}

fn optional_secret(provider_config: &Value, key: &str) -> Option<String> {
    provider_config
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn optional_config_text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_callback_submission(value: &str) -> Result<(String, Option<String>)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("OAuth callback value is required");
    }
    let Ok(url) = Url::parse(trimmed) else {
        return Ok((trimmed.to_string(), None));
    };
    let mut code = None;
    let mut state = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            _ => {}
        }
    }
    Ok((
        code.filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("OAuth callback is missing code"))?,
        state,
    ))
}

fn pending_device_code_expired(provider_config: &Value) -> bool {
    optional_secret(provider_config, "device_expires_at")
        .and_then(|value| OffsetDateTime::parse(&value, &Rfc3339).ok())
        .is_some_and(|expires_at| expires_at <= OffsetDateTime::now_utc())
}

fn token_refresh_is_required(provider_config: &Value, access_token: &str) -> bool {
    optional_secret(provider_config, "expires_at")
        .and_then(|value| OffsetDateTime::parse(&value, &Rfc3339).ok())
        .or_else(|| jwt_expiry(access_token))
        .is_some_and(|expires_at| expires_at <= OffsetDateTime::now_utc() + REFRESH_EARLY_WINDOW)
}

fn expires_at_from_now(duration: Duration) -> Result<String> {
    (OffsetDateTime::now_utc() + duration)
        .format(&Rfc3339)
        .context("formatting ChatGPT token expiry")
}

fn jwt_payload(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn jwt_expiry(token: &str) -> Option<OffsetDateTime> {
    let seconds = jwt_payload(token)?.get("exp")?.as_i64()?;
    OffsetDateTime::from_unix_timestamp(seconds).ok()
}

fn chatgpt_account_id(token: &str) -> Option<String> {
    jwt_payload(token)?
        .get("https://api.openai.com/auth")?
        .get("chatgpt_account_id")?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn instance_cookie_key(provider_config: &Value) -> String {
    optional_secret(provider_config, "instance_cookie_key").unwrap_or_else(|| {
        let mut key = [0_u8; 24];
        rand::thread_rng().fill_bytes(&mut key);
        URL_SAFE_NO_PAD.encode(key)
    })
}

fn random_secret(bytes: usize) -> Result<String> {
    let mut value = vec![0_u8; bytes];
    rand::thread_rng().try_fill_bytes(&mut value)?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}

fn deserialize_poll_interval<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(value) => value.trim().parse().map_err(serde::de::Error::custom),
        Value::Number(value) => value
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("invalid interval")),
        Value::Null => Ok(5),
        _ => Err(serde::de::Error::custom("invalid interval")),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use serde_json::{json, Map, Value};

    use super::*;

    fn start_auth_server(responses: Vec<&'static str>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("auth fixture should bind");
        let base_url = format!("http://{}", listener.local_addr().expect("fixture address"));
        let handle = thread::spawn(move || {
            for body in responses {
                let (mut stream, _) = listener.accept().expect("auth request should connect");
                let mut buffer = [0_u8; 4096];
                let _ = stream
                    .read(&mut buffer)
                    .expect("auth request should be readable");
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .expect("auth response should be writable");
            }
        });
        (base_url, handle)
    }

    fn with_patch(mut config: Value, patch: &BTreeMap<String, Value>) -> Value {
        let object = config
            .as_object_mut()
            .expect("auth fixture config must be an object");
        for (key, value) in patch {
            if !value.is_null() {
                object.insert(key.clone(), value.clone());
            }
        }
        config
    }

    #[tokio::test]
    async fn device_code_poll_authorizes_and_clears_transient_grant_material() {
        let (base_url, server) = start_auth_server(vec![
            r#"{"device_auth_id":"device_fixture","user_code":"ABCD-EFGH","interval":3}"#,
            r#"{"authorization_code":"authorize_fixture","code_verifier":"verifier_fixture"}"#,
            r#"{"access_token":"access_fixture","refresh_token":"refresh_fixture","expires_in":3600}"#,
        ]);
        let config = json!({ "auth_base_url": base_url });
        let begin = authenticate(ProviderAuthRuntimeInput {
            provider_config: config.clone(),
            operation: ProviderAuthOperation::Begin {
                action: "device_code".to_string(),
            },
        })
        .await
        .expect("device begin should succeed");

        assert_eq!(begin.status, ProviderAuthStatus::Pending);
        let verification_url = format!("{base_url}/codex/device");
        assert_eq!(
            begin
                .user_action
                .as_ref()
                .and_then(|action| action.open_url.as_deref()),
            Some(verification_url.as_str())
        );
        assert_eq!(
            begin.managed_secret_patch.get("device_user_code"),
            Some(&json!("ABCD-EFGH"))
        );

        let authorized = authenticate(ProviderAuthRuntimeInput {
            provider_config: with_patch(config, &begin.managed_secret_patch),
            operation: ProviderAuthOperation::Poll,
        })
        .await
        .expect("device poll should exchange its authorization code");

        assert_eq!(authorized.status, ProviderAuthStatus::Authorized);
        assert_eq!(
            authorized.managed_secret_patch.get("access_token"),
            Some(&json!("access_fixture"))
        );
        for key in [
            "device_auth_id",
            "device_user_code",
            "pkce_state",
            "pkce_code_verifier",
        ] {
            assert_eq!(authorized.managed_secret_patch.get(key), Some(&Value::Null));
        }
        assert!(!authorized.managed_secret_patch.values().any(|value| {
            matches!(
                value.as_str(),
                Some("authorize_fixture" | "verifier_fixture")
            )
        }));
        server.join().expect("auth fixture should finish");
    }

    #[tokio::test]
    async fn refresh_rotation_returns_only_the_replacement_secret_patch() {
        let (base_url, server) = start_auth_server(vec![
            r#"{"access_token":"access_rotated","refresh_token":"refresh_rotated","expires_in":3600}"#,
        ]);
        let result = authenticate(ProviderAuthRuntimeInput {
            provider_config: json!({
                "auth_base_url": base_url,
                "access_token": "header.eyJleHAiOjB9.signature",
                "refresh_token": "refresh_original"
            }),
            operation: ProviderAuthOperation::Maintain,
        })
        .await
        .expect("refresh should succeed");

        assert_eq!(result.status, ProviderAuthStatus::Authorized);
        assert_eq!(
            result.managed_secret_patch.get("access_token"),
            Some(&json!("access_rotated"))
        );
        assert_eq!(
            result.managed_secret_patch.get("refresh_token"),
            Some(&json!("refresh_rotated"))
        );
        server.join().expect("refresh fixture should finish");
    }

    #[test]
    fn callback_submission_requires_the_original_pkce_state() {
        let (code, state) = parse_callback_submission(
            "http://localhost:1455/auth/callback?code=code_fixture&state=state_fixture",
        )
        .expect("callback URL should parse");
        assert_eq!(code, "code_fixture");
        assert_eq!(state.as_deref(), Some("state_fixture"));

        let config = Map::from_iter([("pkce_state".to_string(), json!("state_fixture"))]);
        assert_eq!(
            optional_config_text(config.get("pkce_state")),
            Some("state_fixture".to_string())
        );
    }
}
