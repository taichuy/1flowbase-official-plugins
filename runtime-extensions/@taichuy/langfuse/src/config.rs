use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::{MultiplexHostService, PluginCredentialClient};
use url::Url;

const CREDENTIAL: &str = "langfuse_project";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigInput {
    pub base_url: String,
    pub public_key: Option<String>,
    pub secret_key: Option<String>,
}
pub(crate) struct Connection {
    pub base: Url,
    pub authorization: String,
}
fn base(value: &str) -> Result<Url> {
    let mut url = Url::parse(value).map_err(|_| fail("langfuse_base_url"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(fail("langfuse_base_url"));
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}
async fn data(host: &Host, operation: Value) -> Result<Value> {
    let response = host
        .emitter
        .callback(
            MultiplexHostService::PluginDataV1,
            json!({"operations":[operation]}),
        )
        .await
        .map_err(|_| fail("plugin_data_transport"))?;
    if let Some(error) = response.get("error").filter(|v| !v.is_null()) {
        return Err(Failure {
            code: error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("plugin_data_error")
                .into(),
            status: None,
            details: None,
        });
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| fail("plugin_data_response"))
}
async fn stored_url(host: &Host) -> Result<Option<String>> {
    let value = data(host,json!({"operation":"find_one","target":{"kind":"owned_collection","collection_code":"connection"},"fields":["base_url"],"filters":[]})).await?;
    match value.pointer("/results/0/row/values/base_url/value") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(fail("plugin_data_response")),
    }
}
pub(crate) async fn read(host: &Host) -> Result<Value> {
    let base_url = stored_url(host).await?.unwrap_or_default();
    let credential = PluginCredentialClient::new(host.emitter.clone())
        .get(CREDENTIAL.into())
        .await
        .map_err(|_| fail("plugin_credential_read"))?;
    Ok(json!({"success":true,"base_url":base_url,"credential_configured":credential.is_some()}))
}
pub(crate) async fn save(host: &Host, value: Value) -> Result<Value> {
    let input: ConfigInput =
        serde_json::from_value(value).map_err(|_| fail("langfuse_config_input"))?;
    let url = base(&input.base_url)?;
    let client = PluginCredentialClient::new(host.emitter.clone());
    match (input.public_key, input.secret_key) {
        (Some(public), Some(secret))
            if !public.trim().is_empty() && !secret.trim().is_empty() && !public.contains(':') =>
        {
            client
                .put(
                    CREDENTIAL.into(),
                    json!({"public_key":public,"secret_key":secret}),
                )
                .await
                .map_err(|_| fail("plugin_credential_write"))?;
        }
        (None, None) => {
            if client
                .get(CREDENTIAL.into())
                .await
                .map_err(|_| fail("plugin_credential_read"))?
                .is_none()
            {
                return Err(fail("langfuse_credentials_required"));
            }
        }
        _ => return Err(fail("langfuse_credentials_pair")),
    }
    data(host,json!({"operation":"upsert","target":{"kind":"owned_collection","collection_code":"connection"},"identity":{"name":{"type":"string","value":"default"}},"values":{"base_url":{"type":"string","value":url.as_str()}}})).await?;
    read(host).await
}
pub(crate) async fn connection(host: &Host) -> Result<Connection> {
    let url = stored_url(host)
        .await?
        .ok_or_else(|| fail("langfuse_config_required"))?;
    let credential = PluginCredentialClient::new(host.emitter.clone())
        .get(CREDENTIAL.into())
        .await
        .map_err(|_| fail("plugin_credential_read"))?
        .ok_or_else(|| fail("langfuse_credentials_required"))?;
    let public = credential
        .get("public_key")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("langfuse_credentials_required"))?;
    let secret = credential
        .get("secret_key")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("langfuse_credentials_required"))?;
    Ok(Connection {
        base: base(&url)?,
        authorization: format!("Basic {}", STANDARD.encode(format!("{public}:{secret}"))),
    })
}
