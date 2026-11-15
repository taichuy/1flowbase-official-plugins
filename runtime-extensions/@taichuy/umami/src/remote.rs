use crate::{
    required_string,
    store::{self, Host},
    Failure, Payload, Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::{
    HttpEgressClient, HttpEgressRequest, HttpEgressResponse, MultiplexEmitter,
    PluginCredentialClient, PluginEphemeralClient,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use url::Url;

const TOKEN_TTL_MS: u64 = 60 * 60 * 1000;
pub(crate) struct Context {
    pub emitter: MultiplexEmitter,
    pub credentials: PluginCredentialClient,
    ephemeral: PluginEphemeralClient,
    http: HttpEgressClient,
}
pub(crate) struct Login {
    pub token: String,
    pub user: Value,
}
impl Context {
    pub fn new(emitter: MultiplexEmitter) -> Self {
        Self {
            credentials: PluginCredentialClient::new(emitter.clone()),
            ephemeral: PluginEphemeralClient::new(emitter.clone()),
            http: HttpEgressClient::new(emitter.clone()),
            emitter,
        }
    }
    pub async fn password(&self, host: &Host) -> Result<String> {
        self.credentials
            .get(host.credential_id.clone())
            .await
            .map_err(|_| Failure("umami_storage_error"))?
            .and_then(|v| v.get("password").and_then(Value::as_str).map(str::to_owned))
            .filter(|s| !s.is_empty())
            .ok_or(Failure("password_required"))
    }
    pub async fn cache(&self, host: &Host, token: &str) -> Result<()> {
        self.ephemeral
            .put(host.token_key(), json!({"token":token}), TOKEN_TTL_MS)
            .await
            .map_err(|_| Failure("umami_storage_error"))
    }
    pub async fn invalidate(&self, host: &Host) -> Result<()> {
        self.ephemeral
            .delete(host.token_key())
            .await
            .map_err(|_| Failure("umami_storage_error"))?;
        self.ephemeral
            .delete(format!("{}:aux", host.token_key()))
            .await
            .map_err(|_| Failure("umami_storage_error"))
    }
    async fn token(&self, host: &Host) -> Result<String> {
        if let Some(token) = self
            .ephemeral
            .get(host.token_key())
            .await
            .map_err(|_| Failure("umami_storage_error"))?
            .and_then(|v| v.get("token").and_then(Value::as_str).map(str::to_owned))
            .filter(|s| !s.is_empty())
        {
            return Ok(token);
        }
        self.renew(host).await
    }
    async fn renew(&self, host: &Host) -> Result<String> {
        let login = self.login(host, &self.password(host).await?).await?;
        self.cache(host, &login.token).await?;
        Ok(login.token)
    }
    pub async fn login(&self, host: &Host, password: &str) -> Result<Login> {
        let url = build_url(
            &host.base_url,
            "/api/auth/login",
            &Value::Null,
            &Value::Null,
        )?;
        let response = self
            .request(
                "POST",
                url,
                None,
                Some(&json!({"username":host.username,"password":password})),
            )
            .await?;
        if response.status == 401 {
            return Err(Failure("umami_authentication_failed"));
        }
        if response.status == 403 {
            return Err(Failure("umami_permission_denied"));
        }
        if !(200..300).contains(&response.status) {
            return Err(Failure("umami_network_error"));
        }
        let data = decode_json(&response)?;
        if data.get("requiresTwoFactor").and_then(Value::as_bool) == Some(true) {
            return Err(Failure("umami_two_factor_unsupported"));
        }
        let token = data
            .get("token")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(Failure("umami_authentication_failed"))?
            .to_owned();
        let user = data
            .get("user")
            .filter(|v| v.is_object())
            .cloned()
            .ok_or(Failure("umami_authentication_failed"))?;
        Ok(Login { token, user })
    }
    async fn request(
        &self,
        method: &str,
        url: Url,
        token: Option<&str>,
        body: Option<&Value>,
    ) -> Result<HttpEgressResponse> {
        let origin = url.origin().ascii_serialization();
        let mut headers = BTreeMap::from([
            ("accept".into(), "application/json".into()),
            ("user-agent".into(), "1flowbase-umami/0.1.0".into()),
            ("origin".into(), origin.clone()),
            ("referer".into(), format!("{origin}/login")),
        ]);
        if let Some(token) = token {
            headers.insert("authorization".into(), format!("Bearer {token}"));
        }
        let body = if matches!(method, "POST" | "PUT" | "PATCH") {
            body
        } else {
            None
        };
        let body_base64 = body
            .map(|value| serde_json::to_vec(value).map(|bytes| STANDARD.encode(bytes)))
            .transpose()
            .map_err(|_| Failure("invalid_input"))?;
        if body_base64.is_some() {
            headers.insert("content-type".into(), "application/json".into());
        }
        self.http
            .execute(HttpEgressRequest {
                method: method.into(),
                url: url.into(),
                headers,
                body_base64,
            })
            .await
            .map_err(|_| Failure("umami_network_error"))
    }
}

pub(crate) fn normalize_base(value: &str) -> Result<String> {
    let mut url = Url::parse(value).map_err(|_| Failure("invalid_base_url"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(Failure("invalid_base_url"));
    }
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

/// Append to the configured base path; never resolve caller data as a URL or path.
pub(crate) fn build_url(base: &str, template: &str, path: &Value, query: &Value) -> Result<Url> {
    let mut url = Url::parse(base).map_err(|_| Failure("invalid_base_url"))?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| Failure("invalid_base_url"))?;
        segments.pop_if_empty();
        for segment in template.trim_start_matches('/').split('/') {
            let parameter = segment
                .strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .or_else(|| segment.strip_prefix('[').and_then(|s| s.strip_suffix(']')));
            let value = match parameter {
                Some(name) => required_string(path, name)?,
                None => segment,
            };
            // url::PathSegmentsMut deliberately elides dot segments, so reject these
            // rather than allowing a placeholder to silently change route ownership.
            if value == "." || value == ".." {
                return Err(Failure("invalid_input"));
            }
            segments.push(value);
        }
    }
    if !query.is_null() {
        let query = query.as_object().ok_or(Failure("invalid_input"))?;
        let mut pairs = url.query_pairs_mut();
        for (name, value) in query {
            if value.is_null() {
                continue;
            }
            let value = match value {
                Value::String(v) => v.clone(),
                other => serde_json::to_string(other).map_err(|_| Failure("invalid_input"))?,
            };
            pairs.append_pair(name, &value);
        }
    }
    Ok(url)
}
fn decode_json(response: &HttpEgressResponse) -> Result<Value> {
    let bytes = STANDARD
        .decode(&response.body_base64)
        .map_err(|_| Failure("umami_network_error"))?;
    serde_json::from_slice(&bytes).map_err(|_| Failure("umami_network_error"))
}
#[derive(Deserialize)]
struct Operation {
    operation_id: String,
    method: String,
    path: String,
    auth: String,
}
pub(crate) async fn upstream(ctx: &Context, id: &str, input: Payload) -> Result<Value> {
    let operations: Vec<Operation> =
        serde_json::from_str(include_str!("../_assets/api/operations.json"))
            .map_err(|_| Failure("invalid_input"))?;
    let operation = operations
        .into_iter()
        .find(|op| op.operation_id == id)
        .ok_or(Failure("invalid_input"))?;
    let host = store::get(ctx, required_string(&input.path, "host_id")?).await?;
    if !host.enabled {
        return Err(Failure("host_disabled"));
    }
    if operation.path == "/api/auth/login" {
        let login = ctx.login(&host, &ctx.password(&host).await?).await?;
        ctx.cache(&host, &login.token).await?;
        return Ok(
            json!({"success":true,"status":200,"data":{"authenticated":true,"user":login.user}}),
        );
    }
    let url = build_url(&host.base_url, &operation.path, &input.path, &input.query)?;
    let body = (!input.body.is_null()).then_some(&input.body);
    let token = if operation.auth == "none" {
        None
    } else {
        Some(ctx.token(&host).await?)
    };
    let mut response = ctx
        .request(&operation.method, url.clone(), token.as_deref(), body)
        .await?;
    if response.status == 401 {
        if let Some(token) = token {
            let verify_url = build_url(
                &host.base_url,
                "/api/auth/verify",
                &Value::Null,
                &Value::Null,
            )?;
            let verify = ctx.request("POST", verify_url, Some(&token), None).await?;
            if (200..300).contains(&verify.status) {
                return Err(Failure("umami_permission_denied"));
            }
            // Only an explicit authentication failure permits one login and replay.
            if verify.status == 401 {
                ctx.invalidate(&host).await?;
                let new_token = ctx.renew(&host).await?;
                response = ctx
                    .request(&operation.method, url, Some(&new_token), body)
                    .await?;
            }
        }
    }
    let bytes = STANDARD
        .decode(&response.body_base64)
        .map_err(|_| Failure("umami_network_error"))?;
    let (mut data, encoding) = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => (value, None),
        Err(_) => match String::from_utf8(bytes) {
            Ok(value) => (Value::String(value), None),
            Err(_) => (Value::String(response.body_base64), Some("base64")),
        },
    };
    // Only protocol auth responses contain session credentials. Analytics event
    // properties named `token` remain ordinary provider data and retain their shape.
    if matches!(operation.path.as_str(), "/api/auth/sso" | "/api/2fa/verify") {
        if let Some(object) = data.as_object_mut() {
            let token = object.remove("token");
            let partial = object.remove("partialToken");
            if token.is_some() || partial.is_some() {
                ctx.ephemeral
                    .put(
                        format!("{}:aux", host.token_key()),
                        json!({"token":token,"partialToken":partial}),
                        TOKEN_TTL_MS,
                    )
                    .await
                    .map_err(|_| Failure("umami_storage_error"))?;
                object.insert("authenticated".into(), json!(token.is_some()));
            }
        }
    }
    let success = (200..300).contains(&response.status);
    let mut result = json!({"success":success,"status":response.status,"data":data});
    if !success {
        result["error"] = json!({"code":"umami_http_error"});
    }
    if let Some(content_type) = response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v)
    {
        result["content_type"] = json!(content_type);
    }
    if let Some(encoding) = encoding {
        result["encoding"] = json!(encoding);
    }
    Ok(result)
}
