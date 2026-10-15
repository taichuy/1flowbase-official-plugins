use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::PluginCredentialClient;
use url::Url;
mod store;

pub(crate) const DEFAULT: &str = "default";
#[derive(Clone)]
struct Instance {
    instance_id: String,
    name: String,
    base_url: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigInput {
    base_url: String,
    public_key: Option<String>,
    secret_key: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstanceInput {
    name: String,
    base_url: String,
    public_key: Option<String>,
    secret_key: Option<String>,
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
fn validate_id(id: &str) -> Result<()> {
    if id == DEFAULT || uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id) {
        Ok(())
    } else {
        Err(fail("langfuse_instance_id"))
    }
}
fn credential_id(id: &str) -> String {
    if id == DEFAULT {
        "langfuse_project".into()
    } else {
        format!("langfuse_instance:{id}")
    }
}
async fn credential(host: &Host, id: &str) -> Result<Option<Value>> {
    PluginCredentialClient::new(host.emitter.clone())
        .get(credential_id(id))
        .await
        .map_err(|_| fail("plugin_credential_read"))
}
fn keys(public: Option<String>, secret: Option<String>) -> Result<Option<Value>> {
    match (public, secret) {
        (None, None) => Ok(None),
        (Some(public), Some(secret))
            if !public.trim().is_empty() && !secret.trim().is_empty() && !public.contains(':') =>
        {
            Ok(Some(json!({"public_key":public,"secret_key":secret})))
        }
        _ => Err(fail("langfuse_credentials_pair")),
    }
}
fn instance(input: &InstanceInput, id: &str) -> Result<Instance> {
    if input.name.trim().is_empty() {
        return Err(fail("langfuse_instance_name"));
    }
    Ok(Instance {
        instance_id: id.into(),
        name: input.name.clone(),
        base_url: base(&input.base_url)?.to_string(),
    })
}
fn dto(instance: &Instance, configured: bool) -> Value {
    json!({"instance_id":instance.instance_id,"name":instance.name,"base_url":instance.base_url,"credential_configured":configured})
}
fn compensated(
    mut error: Failure,
    action: &str,
    data_restored: bool,
    credential_restored: bool,
) -> Failure {
    error.details = Some(
        json!({"action":action,"data_restored":data_restored,"credential_restored":credential_restored}),
    );
    error
}
async fn restore_credential(host: &Host, id: &str, previous: Option<Value>) -> bool {
    let client = PluginCredentialClient::new(host.emitter.clone());
    match previous {
        Some(value) => client.put(credential_id(id), value).await.is_ok(),
        None => client.delete(credential_id(id)).await.is_ok(),
    }
}
pub(crate) async fn list(host: &Host, query: Value) -> Result<Value> {
    let query = query.as_object().ok_or_else(|| fail("langfuse_page"))?;
    if query.keys().any(|key| key != "page" && key != "page_size") {
        return Err(fail("langfuse_page"));
    }
    let number = |key: &str, default: u64| -> Result<u64> {
        let value = match query.get(key) {
            None => default,
            Some(Value::String(value)) => value.parse().map_err(|_| fail("langfuse_page"))?,
            Some(value) => value.as_u64().ok_or_else(|| fail("langfuse_page"))?,
        };
        if value == 0 {
            Err(fail("langfuse_page"))
        } else {
            Ok(value)
        }
    };
    let page = number("page", 1)?;
    let page_size = number("page_size", 50)?;
    let (instances, total) = store::list(host, page, page_size).await?;
    let mut items = Vec::new();
    for instance in instances {
        items.push(dto(
            &instance,
            credential(host, &instance.instance_id).await?.is_some(),
        ));
    }
    Ok(json!({"success":true,"items":items,"page":page,"page_size":page_size,"total":total}))
}
pub(crate) async fn create(host: &Host, body: Value) -> Result<Value> {
    let input: InstanceInput = serde_json::from_value(body).map_err(|_| fail("langfuse_input"))?;
    let instance = instance(&input, &uuid::Uuid::new_v4().to_string())?;
    let keys = keys(input.public_key, input.secret_key)?
        .ok_or_else(|| fail("langfuse_credentials_required"))?;
    store::migrate(host).await?;
    let client = PluginCredentialClient::new(host.emitter.clone());
    if client
        .put(credential_id(&instance.instance_id), keys)
        .await
        .is_err()
    {
        let restored = restore_credential(host, &instance.instance_id, None).await;
        return Err(compensated(
            fail("plugin_credential_write"),
            "create",
            true,
            restored,
        ));
    }
    if let Err(error) = store::insert(host, &instance).await {
        // Both callbacks can fail after committing. Remove both owned artifacts and
        // report compensation failures instead of claiming an atomic transaction.
        let data_restored = store::delete(host, &instance.instance_id, None)
            .await
            .is_ok();
        let credential_restored = restore_credential(host, &instance.instance_id, None).await;
        return Err(compensated(
            error,
            "create",
            data_restored,
            credential_restored,
        ));
    }
    Ok(json!({"success":true,"instance":dto(&instance,true)}))
}
async fn replace(
    host: &Host,
    old: Option<Instance>,
    new: Instance,
    keys: Option<Value>,
) -> Result<Value> {
    let previous = credential(host, &new.instance_id).await?;
    if keys.is_none() && previous.is_none() {
        return Err(fail("langfuse_credentials_required"));
    }
    // Save the credential first, restoring the previous value if the data callback
    // fails. Credential rollback is also attempted after ambiguous callback errors.
    if let Some(keys) = keys {
        let client = PluginCredentialClient::new(host.emitter.clone());
        if client
            .put(credential_id(&new.instance_id), keys)
            .await
            .is_err()
        {
            let restored = restore_credential(host, &new.instance_id, previous).await;
            return Err(compensated(
                fail("plugin_credential_write"),
                "update",
                true,
                restored,
            ));
        }
    }
    let write = match &old {
        Some(_) => store::update(host, &new).await,
        None => store::insert(host, &new).await,
    };
    if let Err(error) = write {
        let data_restored = match old {
            Some(old) => store::update(host, &old).await.is_ok(),
            None => store::delete(host, &new.instance_id, None).await.is_ok(),
        };
        let credential_restored = restore_credential(host, &new.instance_id, previous).await;
        return Err(compensated(
            error,
            "update",
            data_restored,
            credential_restored,
        ));
    }
    Ok(json!({"success":true,"instance":dto(&new,true)}))
}
pub(crate) async fn update(host: &Host, id: &str, body: Value) -> Result<Value> {
    validate_id(id)?;
    let input: InstanceInput = serde_json::from_value(body).map_err(|_| fail("langfuse_input"))?;
    let new = instance(&input, id)?;
    let keys = keys(input.public_key, input.secret_key)?;
    store::migrate(host).await?;
    let old = store::get(host, id)
        .await?
        .ok_or_else(|| fail("langfuse_instance_not_found"))?;
    replace(host, Some(old), new, keys).await
}
pub(crate) async fn delete(host: &Host, id: &str) -> Result<Value> {
    validate_id(id)?;
    store::migrate(host).await?;
    let old = store::get(host, id)
        .await?
        .ok_or_else(|| fail("langfuse_instance_not_found"))?;
    let previous = credential(host, id).await?;
    if PluginCredentialClient::new(host.emitter.clone())
        .delete(credential_id(id))
        .await
        .is_err()
    {
        let restored = restore_credential(host, id, previous).await;
        return Err(compensated(
            fail("plugin_credential_delete"),
            "delete",
            true,
            restored,
        ));
    }
    if let Err(error) = store::delete(host, id, Some(1)).await {
        // Reinsert only after confirming the row disappeared, so an ambiguous
        // delete failure cannot create a duplicate stable identity.
        let data_restored = match store::get(host, id).await {
            Ok(Some(_)) => true,
            Ok(None) => store::insert(host, &old).await.is_ok(),
            Err(_) => false,
        };
        let credential_restored = restore_credential(host, id, previous).await;
        return Err(compensated(
            error,
            "delete",
            data_restored,
            credential_restored,
        ));
    }
    Ok(json!({"success":true}))
}
pub(crate) async fn read(host: &Host) -> Result<Value> {
    let stored = store::get(host, DEFAULT).await?;
    let base_url = stored.map(|instance| instance.base_url).unwrap_or_default();
    Ok(
        json!({"success":true,"base_url":base_url,"credential_configured":credential(host,DEFAULT).await?.is_some()}),
    )
}
pub(crate) async fn save(host: &Host, body: Value) -> Result<Value> {
    let input: ConfigInput =
        serde_json::from_value(body).map_err(|_| fail("langfuse_config_input"))?;
    let base_url = base(&input.base_url)?.to_string();
    let keys = keys(input.public_key, input.secret_key)?;
    store::migrate(host).await?;
    let old = store::get(host, DEFAULT).await?;
    let new = Instance {
        instance_id: DEFAULT.into(),
        name: old
            .as_ref()
            .map(|v| v.name.clone())
            .unwrap_or_else(|| DEFAULT.into()),
        base_url,
    };
    replace(host, old, new, keys).await?;
    read(host).await
}
pub(crate) async fn connection(host: &Host, id: &str) -> Result<Connection> {
    let instance = store::get(host, id)
        .await?
        .ok_or_else(|| fail("langfuse_config_required"))?;
    let credential = credential(host, id)
        .await?
        .ok_or_else(|| fail("langfuse_credentials_required"))?;
    let public = credential
        .get("public_key")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("langfuse_credentials_required"))?;
    let secret = credential
        .get("secret_key")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("langfuse_credentials_required"))?;
    keys(Some(public.into()), Some(secret.into()))?;
    Ok(Connection {
        base: base(&instance.base_url)?,
        authorization: format!("Basic {}", STANDARD.encode(format!("{public}:{secret}"))),
    })
}
