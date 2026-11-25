mod gsc;
mod model;
mod oauth;
mod pkce;
mod sitemap_xml;
mod store;

use model::CallContext;
use oauth::HostContext;
use runtime_extension_sdk::MultiplexEmitter;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug)]
pub(crate) struct Failure(pub &'static str);
pub(crate) type Result<T> = std::result::Result<T, Failure>;

#[derive(Default, Deserialize)]
pub(crate) struct Payload {
    #[serde(default)]
    pub path: Value,
    #[serde(default)]
    pub query: Value,
    #[serde(default)]
    pub body: Value,
}

#[derive(Deserialize)]
struct Invocation {
    method: String,
    input: InvocationInput,
}

#[derive(Deserialize)]
struct InvocationInput {
    handler: String,
    #[serde(default)]
    input_payload: Payload,
}

#[derive(Clone, Default)]
pub struct Runtime;

impl Runtime {
    pub async fn dispatch(&self, request: Value, emitter: MultiplexEmitter) -> Value {
        let result = dispatch_inner(request, emitter).await;
        match result {
            Ok(value) => json!({ "ok": true, "result": value }),
            Err(Failure(code)) => json!({ "ok": false, "error": { "message": code } }),
        }
    }
}

async fn dispatch_inner(request: Value, emitter: MultiplexEmitter) -> Result<Value> {
    let actor = CallContext::from_request(&request)?;
    let invocation: Invocation =
        serde_json::from_value(request).map_err(|_| Failure("invalid_input"))?;
    if invocation.method != "execute" {
        return Err(Failure("invalid_input"));
    }
    let ctx = HostContext::new(emitter, actor);
    let input = invocation.input.input_payload;
    match invocation.input.handler.as_str() {
        "oauth_config.get" => oauth::oauth_config_get(&ctx).await,
        "oauth_config.save" => oauth::oauth_config_save(&ctx, input).await,
        "oauth.start" => oauth::oauth_start(&ctx, input).await,
        "oauth.complete" => oauth::oauth_complete(&ctx, input).await,
        "accounts.list" => gsc::accounts_list(&ctx, input).await,
        "accounts.get" => gsc::accounts_get(&ctx, input).await,
        "accounts.update" => gsc::accounts_update(&ctx, input).await,
        "accounts.test" => gsc::accounts_test(&ctx, input).await,
        "accounts.delete" => gsc::accounts_delete(&ctx, input).await,
        "sites.list" => gsc::sites_list(&ctx, input).await,
        "sites.sync" => gsc::sites_sync(&ctx, input).await,
        "sites.get" => gsc::sites_get(&ctx, input).await,
        "sites.add" => gsc::sites_add(&ctx, input).await,
        "sites.delete" => gsc::sites_delete(&ctx, input).await,
        "sites.overview" => gsc::sites_overview(&ctx, input).await,
        "sitemaps.list" => gsc::sitemaps_list(&ctx, input).await,
        "sitemaps.get" => gsc::sitemaps_get(&ctx, input).await,
        "sitemaps.submit" => gsc::sitemaps_submit(&ctx, input).await,
        "sitemaps.delete" => gsc::sitemaps_delete(&ctx, input).await,
        "sitemaps.fetch_entries" => sitemap_xml::fetch_entries(&ctx, input).await,
        "analytics.query" => gsc::analytics_query(&ctx, input).await,
        "inspection.inspect" => gsc::inspection_inspect(&ctx, input).await,
        _ => Err(Failure("unknown_handler")),
    }
}

pub(crate) fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or(Failure("invalid_input"))
}

#[cfg(test)]
#[path = "_tests/mod.rs"]
mod tests;
