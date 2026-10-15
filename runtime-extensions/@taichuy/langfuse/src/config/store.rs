use super::*;
use runtime_extension_sdk::MultiplexHostService;

fn target() -> Value {
    json!({"kind":"owned_collection","collection_code":"connection"})
}
pub(super) fn string(value: &str) -> Value {
    json!({"type":"string","value":value})
}
pub(super) fn identity(id: &str) -> Value {
    json!([{"field":"instance_id","operator":"equal","value":string(id)}])
}
fn legacy_filter() -> Value {
    json!([{"field":"name","operator":"equal","value":string(DEFAULT)},
        {"field":"instance_id","operator":"is_null"}])
}
async fn data(host: &Host, mut operation: Value) -> Result<Value> {
    operation["target"] = target();
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
        .pointer("/result/results/0")
        .cloned()
        .ok_or_else(|| fail("plugin_data_response"))
}
fn parse(row: &Value) -> Result<Instance> {
    let value = |key: &str| row["values"][key]["value"].as_str().map(str::to_owned);
    let name = value("name").ok_or_else(|| fail("plugin_data_response"))?;
    let instance_id = match value("instance_id") {
        Some(id) => {
            validate_id(&id)?;
            id
        }
        None if name == DEFAULT => DEFAULT.into(),
        None => return Err(fail("plugin_data_response")),
    };
    Ok(Instance {
        instance_id,
        name,
        base_url: value("base_url").ok_or_else(|| fail("plugin_data_response"))?,
    })
}
async fn find(host: &Host, filters: Value) -> Result<Option<Instance>> {
    let result = data(host, json!({"operation":"find_one","fields":["instance_id","name","base_url"],"filters":filters})).await?;
    match result.get("row") {
        Some(Value::Null) => Ok(None),
        Some(row) => parse(row).map(Some),
        None => Err(fail("plugin_data_response")),
    }
}
pub(super) async fn get(host: &Host, id: &str) -> Result<Option<Instance>> {
    validate_id(id)?;
    let found = find(host, identity(id)).await?;
    if found.is_some() || id != DEFAULT {
        return Ok(found);
    }
    find(host, legacy_filter()).await
}
/// Reads project the legacy identity. Only mutation paths persist it, idempotently.
pub(super) async fn migrate(host: &Host) -> Result<()> {
    if find(host, legacy_filter()).await?.is_some() {
        mutate(
            host,
            json!({"operation":"update","filters":legacy_filter(),
            "values":{"instance_id":string(DEFAULT)}}),
            None,
        )
        .await?;
    }
    Ok(())
}
async fn mutate(host: &Host, operation: Value, expected: Option<u64>) -> Result<()> {
    let result = data(host, operation).await?;
    let affected = result
        .get("affected")
        .and_then(Value::as_u64)
        .ok_or_else(|| fail("plugin_data_response"))?;
    if expected.is_some_and(|count| count != affected) {
        return Err(fail("langfuse_instance_write_conflict"));
    }
    Ok(())
}
fn values(instance: &Instance) -> Value {
    json!({"instance_id":string(&instance.instance_id),"name":string(&instance.name),"base_url":string(&instance.base_url)})
}
pub(super) async fn insert(host: &Host, instance: &Instance) -> Result<()> {
    mutate(
        host,
        json!({"operation":"insert","values":values(instance)}),
        Some(1),
    )
    .await
}
pub(super) async fn update(host: &Host, instance: &Instance) -> Result<()> {
    mutate(host, json!({"operation":"update","filters":identity(&instance.instance_id),"values":values(instance)}), Some(1)).await
}
pub(super) async fn delete(host: &Host, id: &str, expected: Option<u64>) -> Result<()> {
    mutate(
        host,
        json!({"operation":"delete","filters":identity(id)}),
        expected,
    )
    .await
}
pub(super) async fn list(host: &Host, page: u64, page_size: u64) -> Result<(Vec<Instance>, u64)> {
    let mut offset = (page - 1)
        .checked_mul(page_size)
        .ok_or_else(|| fail("langfuse_page"))?;
    let total = data(host, json!({"operation":"count","filters":[]}))
        .await?
        .get("count")
        .and_then(Value::as_u64)
        .ok_or_else(|| fail("plugin_data_response"))?;
    let mut remaining = page_size.min(total.saturating_sub(offset));
    let mut instances = Vec::new();
    while remaining > 0 {
        // Chunk at the Host plugin_data transport limit; never cap the requested page.
        let limit = remaining.min(100);
        let result = data(host, json!({"operation":"find","fields":["instance_id","name","base_url"],
            "filters":[],"order":[{"field":"instance_id","direction":"ascending"},{"field":"name","direction":"ascending"}],
            "page":{"limit":limit,"offset":offset}})).await?;
        let rows = result
            .get("rows")
            .and_then(Value::as_array)
            .ok_or_else(|| fail("plugin_data_response"))?;
        if rows.len() as u64 > limit {
            return Err(fail("plugin_data_response"));
        }
        for row in rows {
            instances.push(parse(row)?);
        }
        if (rows.len() as u64) < limit {
            break;
        }
        remaining -= limit;
        offset = offset
            .checked_add(limit)
            .ok_or_else(|| fail("langfuse_page"))?;
    }
    Ok((instances, total))
}
