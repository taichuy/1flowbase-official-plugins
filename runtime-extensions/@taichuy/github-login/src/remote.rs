use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::{
    AuthenticationResponse, HttpEgressClient, HttpEgressRequest, MultiplexEmitter,
};
use serde_json::{json, Map, Value};

use crate::{config::Config, Failure, Result};

async fn send(
    client: &HttpEgressClient,
    method: &str,
    url: &str,
    mut headers: BTreeMap<String, String>,
    body: Option<String>,
) -> Result<Value> {
    headers.insert("accept".into(), "application/json".into());
    headers.insert("user-agent".into(), "1flowbase-github-login".into());
    let response = client
        .execute(HttpEgressRequest {
            method: method.into(),
            url: url.into(),
            headers,
            body_base64: body.map(|body| STANDARD.encode(body.as_bytes())),
        })
        .await
        .map_err(|_| Failure("github_http_unavailable"))?;
    if !(200..300).contains(&response.status) {
        return Err(Failure("github_http_rejected"));
    }
    let bytes = STANDARD
        .decode(response.body_base64)
        .map_err(|_| Failure("github_response_invalid"))?;
    serde_json::from_slice(&bytes).map_err(|_| Failure("github_response_invalid"))
}

pub(crate) async fn complete(
    config: &Config,
    code: &str,
    verifier: &str,
    emitter: MultiplexEmitter,
) -> Result<AuthenticationResponse> {
    let client = HttpEgressClient::new(emitter);
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &config.client_id)
        .append_pair("client_secret", &config.client_secret)
        .append_pair("redirect_uri", &config.callback_url)
        .append_pair("code", code)
        .append_pair("code_verifier", verifier)
        .finish();
    let token = send(
        &client,
        "POST",
        "https://github.com/login/oauth/access_token",
        BTreeMap::from([(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        )]),
        Some(body),
    )
    .await?;
    if token.get("error").is_some() {
        return Err(Failure("github_exchange_rejected"));
    }
    let token = token
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_graphic()))
        .ok_or(Failure("github_exchange_rejected"))?;
    let headers = BTreeMap::from([
        ("authorization".into(), format!("Bearer {token}")),
        ("x-github-api-version".into(), "2022-11-28".into()),
    ]);
    let user = send(
        &client,
        "GET",
        "https://api.github.com/user",
        headers.clone(),
        None,
    )
    .await?;
    let id = user
        .get("id")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
        .ok_or(Failure("github_identity_invalid"))?;
    let mut profile = Map::new();
    // User response may contain private fields and token-shaped values. Never clone it wholesale.
    for key in ["login", "name", "avatar_url", "created_at"] {
        if let Some(Value::String(value)) = user.get(key) {
            profile.insert(key.into(), Value::String(value.clone()));
        }
    }
    // Normalize provider-specific profile fields for Host-owned first-signup mapping.
    if let Some(login) = user.get("login").and_then(Value::as_str) {
        profile.insert("account".into(), json!(login));
        let nickname = user
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(login);
        profile.insert("nickname".into(), json!(nickname));
    }
    let emails = send(
        &client,
        "GET",
        "https://api.github.com/user/emails",
        headers,
        None,
    )
    .await?;
    let emails = emails
        .as_array()
        .ok_or(Failure("github_response_invalid"))?;
    let email = emails
        .iter()
        .find(|email| email["primary"] == true && email["verified"] == true)
        .and_then(|email| email.get("email"))
        .and_then(Value::as_str)
        .filter(|email| !email.trim().is_empty());
    if let Some(email) = email {
        profile.insert("email".into(), json!(email));
    }
    profile.insert("email_verified".into(), json!(email.is_some()));
    Ok(AuthenticationResponse::VerifiedIdentity {
        subject_type: "github_user_id".into(),
        subject_value: id.to_string(),
        issuer: "https://github.com".into(),
        realm: None,
        profile: Value::Object(profile),
    })
}
