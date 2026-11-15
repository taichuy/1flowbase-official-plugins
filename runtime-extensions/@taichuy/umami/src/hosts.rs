use crate::{
    remote::{self, Context},
    required_string,
    store::{self, Host},
    Failure, Payload, Result,
};
use serde_json::{json, Value};
use uuid::Uuid;

pub(crate) async fn list(ctx: &Context, input: Payload) -> Result<Value> {
    fn number(query: &Value, key: &str, default: u64) -> Result<u64> {
        match query.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(Value::String(v)) => v.parse().map_err(|_| Failure("invalid_input")),
            Some(v) => v.as_u64().ok_or(Failure("invalid_input")),
        }
    }
    let page = number(&input.query, "page", 1)?;
    let page_size = number(&input.query, "page_size", 20)?;
    if page == 0 || !(1..=100).contains(&page_size) {
        return Err(Failure("invalid_input"));
    }
    let search = input
        .query
        .get("search")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let mut hosts = store::all(ctx).await?;
    hosts.retain(|host| {
        [&host.name, &host.base_url, &host.username]
            .iter()
            .any(|value| value.to_lowercase().contains(&search))
    });
    hosts.sort_by(|a, b| a.name.cmp(&b.name).then(a.host_id.cmp(&b.host_id)));
    let total = hosts.len();
    let offset = page
        .checked_sub(1)
        .and_then(|n| n.checked_mul(page_size))
        .ok_or(Failure("invalid_input"))?;
    let items: Vec<_> = hosts
        .into_iter()
        .skip(usize::try_from(offset).map_err(|_| Failure("invalid_input"))?)
        .take(page_size as usize)
        .map(|host| host.summary())
        .collect();
    Ok(json!({"success":true,"items":items,"total":total,"page":page,"page_size":page_size}))
}

fn draft(body: &Value, old: Option<&Host>) -> Result<Host> {
    Ok(Host {
        host_id: old
            .map(|h| h.host_id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        name: required_string(body, "name")?.trim().into(),
        base_url: remote::normalize_base(required_string(body, "base_url")?)?,
        username: required_string(body, "username")?.to_owned(),
        enabled: body
            .get("enabled")
            .and_then(Value::as_bool)
            .ok_or(Failure("invalid_input"))?,
        credential_id: old.map(|h| h.credential_id.clone()).unwrap_or_default(),
    })
}
async fn password(ctx: &Context, body: &Value, old: Option<&Host>) -> Result<String> {
    if let Some(value) = body.get("password") {
        let value = value.as_str().ok_or(Failure("invalid_input"))?;
        if !value.is_empty() {
            return Ok(value.to_owned());
        }
    }
    let host = old.ok_or(Failure("password_required"))?;
    ctx.password(host).await
}
pub(crate) async fn save(ctx: &Context, input: Payload, update: bool) -> Result<Value> {
    let old = if update {
        Some(store::get(ctx, required_string(&input.path, "host_id")?).await?)
    } else {
        None
    };
    let mut host = draft(&input.body, old.as_ref())?;
    let changed = old
        .as_ref()
        .is_none_or(|h| h.base_url != host.base_url || h.username != host.username)
        || input
            .body
            .get("password")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());
    if input
        .body
        .get("password")
        .is_some_and(|value| !value.is_string())
    {
        return Err(Failure("invalid_input"));
    }
    if changed {
        let password = password(ctx, &input.body, old.as_ref()).await?;
        let login = ctx.login(&host, &password).await?;
        host.credential_id = format!("umami-{}", Uuid::new_v4());
        ctx.credentials
            .put(host.credential_id.clone(), json!({"password":password}))
            .await
            .map_err(|_| Failure("umami_storage_error"))?;
        if let Err(error) = store::save(ctx, &host, update).await {
            // The old record/credential remains usable until the replacement record commits.
            let _ = ctx.credentials.delete(host.credential_id.clone()).await;
            return Err(error);
        }
        if let Some(old) = old {
            ctx.invalidate(&old).await?;
            ctx.credentials
                .delete(old.credential_id)
                .await
                .map_err(|_| Failure("umami_storage_error"))?;
        }
        if host.enabled {
            ctx.cache(&host, &login.token).await?;
        }
    } else {
        store::save(ctx, &host, update).await?;
        if !host.enabled {
            ctx.invalidate(&host).await?;
        }
    }
    Ok(json!({"success":true,"host":host.summary()}))
}
pub(crate) async fn delete(ctx: &Context, input: Payload) -> Result<Value> {
    let mut host = store::get(ctx, required_string(&input.path, "host_id")?).await?;
    // A partial cleanup retains a disabled record, so deletion can be retried safely.
    host.enabled = false;
    store::save(ctx, &host, true).await?;
    ctx.invalidate(&host).await?;
    ctx.credentials
        .delete(host.credential_id.clone())
        .await
        .map_err(|_| Failure("umami_storage_error"))?;
    store::delete(ctx, &host.host_id).await?;
    Ok(json!({"success":true}))
}
pub(crate) async fn test_draft(ctx: &Context, input: Payload) -> Result<Value> {
    let old = match input.body.get("host_id").and_then(Value::as_str) {
        Some(id) => Some(store::get(ctx, id).await?),
        None => None,
    };
    let host = draft(&input.body, old.as_ref())?;
    let password = password(ctx, &input.body, old.as_ref()).await?;
    let login = ctx.login(&host, &password).await?;
    Ok(json!({"success":true,"authenticated":true,"user":login.user}))
}
pub(crate) async fn test(ctx: &Context, input: Payload) -> Result<Value> {
    let host = store::get(ctx, required_string(&input.path, "host_id")?).await?;
    if !host.enabled {
        return Err(Failure("host_disabled"));
    }
    let login = ctx.login(&host, &ctx.password(&host).await?).await?;
    ctx.cache(&host, &login.token).await?;
    Ok(json!({"success":true,"authenticated":true,"user":login.user}))
}
