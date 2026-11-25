use crate::{
    gsc,
    model::{
        deterministic_account_id, now_rfc3339, now_unix_ms, Account, CallContext, OAuthConfig,
        OAuthPendingState, ScopeMode, CONFIG_ID,
    },
    pkce::{code_challenge_s256, random_urlsafe_token, validate_redirect_uri},
    required_string, store, Failure, Payload, Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::{
    HttpEgressClient, HttpEgressRequest, HttpEgressResponse, MultiplexEmitter,
    PluginCredentialClient, PluginEphemeralClient, PluginExecutionGateClient,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use url::{form_urlencoded, Url};
use uuid::Uuid;

const OAUTH_AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const OAUTH_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const OAUTH_STATE_TTL_MS: u64 = 10 * 60 * 1000;
const USER_AGENT: &str = "1flowbase-google-search-console/0.1.0";

pub(crate) struct HostContext {
    pub emitter: MultiplexEmitter,
    pub actor: CallContext,
    pub credentials: PluginCredentialClient,
    pub ephemeral: PluginEphemeralClient,
    pub http: HttpEgressClient,
    pub gate: PluginExecutionGateClient,
}

impl HostContext {
    pub fn new(emitter: MultiplexEmitter, actor: CallContext) -> Self {
        Self {
            credentials: PluginCredentialClient::new(emitter.clone()),
            ephemeral: PluginEphemeralClient::new(emitter.clone()),
            http: HttpEgressClient::new(emitter.clone()),
            gate: PluginExecutionGateClient::new(emitter.clone()),
            emitter,
            actor,
        }
    }

    pub async fn http_request(
        &self,
        method: &str,
        url: String,
        mut headers: BTreeMap<String, String>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpEgressResponse> {
        headers
            .entry("user-agent".to_owned())
            .or_insert_with(|| USER_AGENT.to_owned());
        let body_base64 = body.map(|bytes| STANDARD.encode(bytes));
        self.http
            .execute(HttpEgressRequest {
                method: method.to_owned(),
                url,
                headers,
                body_base64,
            })
            .await
            .map_err(|_| Failure("google_network_error"))
    }
}

fn state_key(state: &str) -> String {
    format!("gsc-oauth-state:{state}")
}

fn validate_non_control(value: &str, code: &'static str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.chars().any(char::is_control)
        || trimmed.len() > 4096
    {
        return Err(Failure(code));
    }
    Ok(trimmed.to_owned())
}

pub(crate) async fn oauth_config_get(ctx: &HostContext) -> Result<Value> {
    match store::get_oauth_config(&ctx.emitter).await? {
        Some(cfg) => Ok(cfg.public()),
        None => Ok(json!({
            "configured": false,
            "auth_mode": "custom_oauth_client",
            "client_id": "",
            "has_client_secret": false,
            "default_scope_mode": "readonly",
            "updated_at": Value::Null,
        })),
    }
}

pub(crate) async fn oauth_config_save(ctx: &HostContext, input: Payload) -> Result<Value> {
    let client_id = validate_non_control(required_string(&input.body, "client_id")?, "invalid_input")?;
    if client_id.chars().any(char::is_whitespace) {
        return Err(Failure("invalid_input"));
    }
    let default_scope_mode = match input.body.get("default_scope_mode").and_then(Value::as_str) {
        Some(raw) => ScopeMode::parse(raw)?,
        None => ScopeMode::Readonly,
    };
    let existing = store::get_oauth_config(&ctx.emitter).await?;
    let provided_secret = match input.body.get("client_secret") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else if trimmed.chars().any(char::is_control) {
                return Err(Failure("invalid_input"));
            } else {
                Some(trimmed.to_owned())
            }
        }
        Some(_) => return Err(Failure("invalid_input")),
    };

    let (credential_id, old_credential_id) = match (provided_secret, existing.as_ref()) {
        (Some(secret), old) => {
            let id = format!("gsc-oauth-client-{}", Uuid::new_v4());
            ctx.credentials
                .put(id.clone(), json!({ "client_secret": secret }))
                .await
                .map_err(|_| Failure("gsc_storage_error"))?;
            (id, old.map(|cfg| cfg.credential_id.clone()))
        }
        (None, Some(old)) if !old.credential_id.is_empty() => (old.credential_id.clone(), None),
        (None, _) => return Err(Failure("client_secret_required")),
    };

    let cfg = OAuthConfig {
        config_id: CONFIG_ID.to_owned(),
        auth_mode: "custom_oauth_client".to_owned(),
        client_id,
        credential_id: credential_id.clone(),
        default_scope_mode,
        updated_at: now_rfc3339(),
    };

    if let Err(err) = store::upsert_oauth_config(&ctx.emitter, &cfg).await {
        if old_credential_id.is_some() || existing.is_none() {
            let _ = ctx.credentials.delete(credential_id).await;
        }
        return Err(err);
    }

    if let Some(old_id) = old_credential_id.filter(|id| id != &cfg.credential_id) {
        let _ = ctx.credentials.delete(old_id).await;
    }

    Ok(cfg.public())
}

async fn load_oauth_client(ctx: &HostContext) -> Result<(OAuthConfig, String)> {
    let cfg = store::get_oauth_config(&ctx.emitter)
        .await?
        .ok_or(Failure("oauth_not_configured"))?;
    let secret = ctx
        .credentials
        .get(cfg.credential_id.clone())
        .await
        .map_err(|_| Failure("gsc_storage_error"))?
        .and_then(|v| {
            v.get("client_secret")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|s| !s.is_empty())
        .ok_or(Failure("oauth_not_configured"))?;
    Ok((cfg, secret))
}

pub(crate) async fn oauth_start(ctx: &HostContext, input: Payload) -> Result<Value> {
    let redirect_uri = validate_redirect_uri(required_string(&input.body, "redirect_uri")?)?;
    let (cfg, _) = load_oauth_client(ctx).await?;
    let scope_mode = match input.body.get("scope_mode").and_then(Value::as_str) {
        Some(raw) => ScopeMode::parse(raw)?,
        None => cfg.default_scope_mode,
    };
    let reauth_account_id = match input.body.get("reauth_account_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) => {
            let id = validate_non_control(id, "invalid_input")?;
            let _ = store::get_account(&ctx.emitter, &id).await?;
            Some(id)
        }
        Some(_) => return Err(Failure("invalid_input")),
    };

    let state = random_urlsafe_token();
    let code_verifier = random_urlsafe_token();
    let challenge = code_challenge_s256(&code_verifier);
    let pending = OAuthPendingState {
        actor_id: ctx.actor.actor_id.clone(),
        workspace_id: ctx.actor.workspace_id.clone(),
        code_verifier,
        redirect_uri: redirect_uri.clone(),
        scope_mode,
        reauth_account_id,
        expires_at_unix_ms: now_unix_ms().saturating_add(OAUTH_STATE_TTL_MS as i64),
    };

    ctx.ephemeral
        .put(
            state_key(&state),
            serde_json::to_value(&pending).map_err(|_| Failure("gsc_storage_error"))?,
            OAUTH_STATE_TTL_MS,
        )
        .await
        .map_err(|_| Failure("gsc_storage_error"))?;

    let scope = scope_mode.oauth_scope_string();
    let auth_url = Url::parse_with_params(
        OAUTH_AUTHORIZE_URL,
        [
            ("client_id", cfg.client_id.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("response_type", "code"),
            ("scope", scope.as_str()),
            ("state", state.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("include_granted_scopes", "true"),
        ],
    )
    .map_err(|_| Failure("gsc_storage_error"))?;

    Ok(json!({
        "authorization_url": auth_url.as_str(),
        "expires_in_seconds": OAUTH_STATE_TTL_MS / 1000,
        "scope_mode": scope_mode.as_str(),
    }))
}

pub(crate) async fn oauth_complete(ctx: &HostContext, input: Payload) -> Result<Value> {
    let code = validate_non_control(required_string(&input.body, "code")?, "invalid_input")?;
    let state = validate_non_control(required_string(&input.body, "state")?, "oauth_state_invalid")?;
    let key = state_key(&state);
    let pending_value = ctx
        .ephemeral
        .get(key.clone())
        .await
        .map_err(|_| Failure("gsc_storage_error"))?
        .ok_or(Failure("oauth_state_expired"))?;
    let _ = ctx.ephemeral.delete(key).await;

    let pending: OAuthPendingState =
        serde_json::from_value(pending_value).map_err(|_| Failure("oauth_state_invalid"))?;
    if pending.actor_id != ctx.actor.actor_id || pending.workspace_id != ctx.actor.workspace_id {
        return Err(Failure("oauth_actor_mismatch"));
    }
    if now_unix_ms() > pending.expires_at_unix_ms {
        return Err(Failure("oauth_state_expired"));
    }

    let (cfg, client_secret) = load_oauth_client(ctx).await?;
    let form_body = form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", &code)
        .append_pair("client_id", &cfg.client_id)
        .append_pair("client_secret", &client_secret)
        .append_pair("redirect_uri", &pending.redirect_uri)
        .append_pair("code_verifier", &pending.code_verifier)
        .finish();
    let mut headers = BTreeMap::new();
    headers.insert(
        "content-type".to_owned(),
        "application/x-www-form-urlencoded".to_owned(),
    );
    headers.insert("accept".to_owned(), "application/json".to_owned());

    let token_resp = ctx
        .http_request(
            "POST",
            OAUTH_TOKEN_URL.to_owned(),
            headers,
            Some(form_body.into_bytes()),
        )
        .await?;
    let token_json = parse_token_response(&token_resp, false)?;
    let access_token = token_json
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(Failure("google_oauth_failed"))?
        .to_owned();
    let expires_in = token_json
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(3600);
    let new_refresh_token = token_json
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let granted_scopes: Vec<String> = token_json
        .get("scope")
        .and_then(Value::as_str)
        .map(|s| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
        .filter(|list| !list.is_empty())
        .unwrap_or_else(|| {
            pending
                .scope_mode
                .oauth_scope_string()
                .split_whitespace()
                .map(str::to_owned)
                .collect()
        });

    let profile = fetch_userinfo(ctx, &access_token).await?;
    let google_sub = profile
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(Failure("google_userinfo_failed"))?
        .to_owned();
    let email = profile
        .get("email")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(Failure("google_userinfo_failed"))?
        .to_owned();
    let display_name = profile
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&email)
        .to_owned();
    let avatar_url = profile
        .get("picture")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let account_id = deterministic_account_id(&google_sub);
    if let Some(expected) = pending.reauth_account_id.as_deref() {
        if expected != account_id {
            return Err(Failure("reauth_account_mismatch"));
        }
    }

    let existing = store::find_account(&ctx.emitter, &account_id).await?;
    let refresh_token = match (new_refresh_token, existing.as_ref()) {
        (Some(rt), _) => rt,
        (None, Some(old)) => read_refresh_token(ctx, old).await?,
        (None, None) => return Err(Failure("google_refresh_token_missing")),
    };

    let now = now_rfc3339();
    let new_cred_id = format!("gsc-acct-{}", Uuid::new_v4());
    ctx.credentials
        .put(new_cred_id.clone(), json!({ "refresh_token": refresh_token }))
        .await
        .map_err(|_| Failure("gsc_storage_error"))?;

    let scope_mode = ScopeMode::from_granted_scopes(&granted_scopes);
    let mut account = Account {
        account_id: account_id.clone(),
        google_sub,
        email,
        display_name,
        avatar_url,
        scope_mode,
        granted_scopes,
        enabled: existing.as_ref().map_or(true, |a| a.enabled),
        status: "active".to_owned(),
        last_error: None,
        credential_id: new_cred_id.clone(),
        site_count: existing.as_ref().map_or(0, |a| a.site_count),
        last_synced_at: existing.as_ref().and_then(|a| a.last_synced_at.clone()),
        created_at: existing
            .as_ref()
            .map_or_else(|| now.clone(), |a| a.created_at.clone()),
        updated_at: now,
    };

    if let Err(err) = store::upsert_account(&ctx.emitter, &account).await {
        let _ = ctx.credentials.delete(new_cred_id).await;
        return Err(err);
    }

    if let Some(old) = existing.as_ref() {
        let _ = ctx.ephemeral.delete(old.token_cache_key()).await;
        if old.credential_id != account.credential_id && !old.credential_id.is_empty() {
            let _ = ctx.credentials.delete(old.credential_id.clone()).await;
        }
    }

    cache_access_token(ctx, &account, &access_token, expires_in).await;

    match gsc::sync_sites_with_token(ctx, &mut account, &access_token).await {
        Ok(sites) => Ok(json!({
            "account": account.summary(),
            "sites": sites.iter().map(|s| s.summary()).collect::<Vec<_>>(),
            "site_count": account.site_count,
            "sync_error": Value::Null,
        })),
        Err(Failure(code)) => Ok(json!({
            "account": account.summary(),
            "sites": [],
            "site_count": account.site_count,
            "sync_error": code,
        })),
    }
}

async fn fetch_userinfo(ctx: &HostContext, access_token: &str) -> Result<Value> {
    let mut headers = BTreeMap::new();
    headers.insert("authorization".to_owned(), format!("Bearer {access_token}"));
    headers.insert("accept".to_owned(), "application/json".to_owned());
    let resp = ctx
        .http_request("GET", OAUTH_USERINFO_URL.to_owned(), headers, None)
        .await?;
    if !(200..300).contains(&resp.status) {
        return Err(Failure("google_userinfo_failed"));
    }
    let bytes = STANDARD
        .decode(&resp.body_base64)
        .map_err(|_| Failure("google_userinfo_failed"))?;
    serde_json::from_slice(&bytes).map_err(|_| Failure("google_userinfo_failed"))
}

pub(crate) async fn read_refresh_token(ctx: &HostContext, account: &Account) -> Result<String> {
    ctx.credentials
        .get(account.credential_id.clone())
        .await
        .map_err(|_| Failure("gsc_storage_error"))?
        .and_then(|v| {
            v.get("refresh_token")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|s| !s.is_empty())
        .ok_or(Failure("google_reauth_required"))
}

fn cache_ttl_ms(expires_in_secs: u64) -> u64 {
    let safe_secs = expires_in_secs.saturating_sub(120).clamp(60, 3300);
    safe_secs.saturating_mul(1000)
}

pub(crate) async fn cache_access_token(
    ctx: &HostContext,
    account: &Account,
    access_token: &str,
    expires_in_secs: u64,
) {
    let _ = ctx
        .ephemeral
        .put(
            account.token_cache_key(),
            json!({ "access_token": access_token }),
            cache_ttl_ms(expires_in_secs),
        )
        .await;
}

pub(crate) async fn invalidate_account_cache(ctx: &HostContext, account: &Account) {
    let _ = ctx.ephemeral.delete(account.token_cache_key()).await;
}

pub(crate) async fn access_token(
    ctx: &HostContext,
    account: &Account,
    failed_token: Option<&str>,
) -> Result<String> {
    if !account.enabled {
        return Err(Failure("account_disabled"));
    }
    if failed_token.is_none() {
        if let Some(cached) = read_cached_access_token(ctx, account).await {
            return Ok(cached);
        }
    }

    let permit = ctx
        .gate
        .acquire(format!("gsc-refresh:{}", account.account_id))
        .await
        .map_err(|_| Failure("gsc_storage_error"))?;

    let refresh_result = async {
        if let Some(cached) = read_cached_access_token(ctx, account).await {
            if failed_token != Some(cached.as_str()) {
                return Ok(cached);
            }
        }
        let (cfg, client_secret) = load_oauth_client(ctx).await?;
        let refresh_token = read_refresh_token(ctx, account).await?;
        let form_body = form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "refresh_token")
            .append_pair("client_id", &cfg.client_id)
            .append_pair("client_secret", &client_secret)
            .append_pair("refresh_token", &refresh_token)
            .finish();
        let mut headers = BTreeMap::new();
        headers.insert(
            "content-type".to_owned(),
            "application/x-www-form-urlencoded".to_owned(),
        );
        headers.insert("accept".to_owned(), "application/json".to_owned());

        let resp = ctx
            .http_request(
                "POST",
                OAUTH_TOKEN_URL.to_owned(),
                headers,
                Some(form_body.into_bytes()),
            )
            .await?;

        match parse_token_response(&resp, true) {
            Ok(token_json) => {
                let access_token = token_json
                    .get("access_token")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or(Failure("google_oauth_failed"))?
                    .to_owned();
                let expires_in = token_json
                    .get("expires_in")
                    .and_then(Value::as_u64)
                    .unwrap_or(3600);
                cache_access_token(ctx, account, &access_token, expires_in).await;
                if account.status != "active" || account.last_error.is_some() {
                    let _ = store::update_account_status(
                        &ctx.emitter,
                        &account.account_id,
                        "active",
                        None,
                        &now_rfc3339(),
                    )
                    .await;
                }
                Ok(access_token)
            }
            Err(Failure("google_reauth_required")) => {
                invalidate_account_cache(ctx, account).await;
                let _ = store::update_account_status(
                    &ctx.emitter,
                    &account.account_id,
                    "reauth_required",
                    Some("google_reauth_required"),
                    &now_rfc3339(),
                )
                .await;
                Err(Failure("google_reauth_required"))
            }
            Err(err) => Err(err),
        }
    }
    .await;

    let _ = permit.release().await;
    refresh_result
}

async fn read_cached_access_token(ctx: &HostContext, account: &Account) -> Option<String> {
    ctx.ephemeral
        .get(account.token_cache_key())
        .await
        .ok()
        .flatten()
        .and_then(|v| {
            v.get("access_token")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|s| !s.is_empty())
}

fn parse_token_response(resp: &HttpEgressResponse, is_refresh: bool) -> Result<Value> {
    let bytes = STANDARD
        .decode(&resp.body_base64)
        .map_err(|_| Failure("google_oauth_failed"))?;
    let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if (200..300).contains(&resp.status) {
        return Ok(parsed);
    }
    let error_code = parsed
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("");
    if error_code == "invalid_grant" || (is_refresh && error_code == "unauthorized_client") {
        return Err(Failure("google_reauth_required"));
    }
    if error_code == "invalid_client" {
        return Err(Failure("google_invalid_oauth_client"));
    }
    if error_code == "redirect_uri_mismatch" {
        return Err(Failure("google_redirect_uri_mismatch"));
    }
    if resp.status == 429 {
        return Err(Failure("google_quota_exceeded"));
    }
    if (500..600).contains(&resp.status) {
        return Err(Failure("google_upstream_error"));
    }
    Err(Failure("google_oauth_failed"))
}
