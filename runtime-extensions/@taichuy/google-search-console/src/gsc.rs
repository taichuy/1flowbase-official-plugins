use crate::{
    model::{
        deterministic_site_record_id, now_rfc3339, property_type_for_site_url, utc_date_days_ago,
        Account, ScopeMode, Site,
    },
    oauth::{self, HostContext},
    required_string, store, Failure, Payload, Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::HttpEgressResponse;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

const WEBMASTERS_V3: &str = "https://www.googleapis.com/webmasters/v3";
const INSPECTION_V1: &str = "https://searchconsole.googleapis.com/v1/urlInspection/index:inspect";

pub(crate) fn encode_path_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    for &b in raw.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(hex_upper(b >> 4));
                out.push(hex_upper(b & 0x0f));
            }
        }
    }
    out
}

fn hex_upper(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'A' + (n - 10)) as char,
        _ => unreachable!(),
    }
}

fn require_full_scope(account: &Account) -> Result<()> {
    if account.scope_mode != ScopeMode::Full {
        return Err(Failure("scope_upgrade_required"));
    }
    Ok(())
}

fn parse_u64_param(obj: &Value, key: &str, default: u64) -> Result<u64> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::String(s)) => s.parse().map_err(|_| Failure("invalid_input")),
        Some(v) => v.as_u64().ok_or(Failure("invalid_input")),
    }
}

pub(crate) async fn gsc_request(
    ctx: &HostContext,
    account: &Account,
    method: &str,
    url: &str,
    body: Option<&Value>,
) -> Result<Value> {
    let token = oauth::access_token(ctx, account, None).await?;
    let resp = send_gsc(ctx, method, url, &token, body).await?;
    if resp.status == 401 {
        let fresh = oauth::access_token(ctx, account, Some(&token)).await?;
        let retry = send_gsc(ctx, method, url, &fresh, body).await?;
        return decode_gsc_response(ctx, Some(account), &retry).await;
    }
    decode_gsc_response(ctx, Some(account), &resp).await
}

pub(crate) async fn gsc_request_with_token(
    ctx: &HostContext,
    method: &str,
    url: &str,
    token: &str,
    body: Option<&Value>,
) -> Result<Value> {
    let resp = send_gsc(ctx, method, url, token, body).await?;
    decode_gsc_response(ctx, None, &resp).await
}

async fn send_gsc(
    ctx: &HostContext,
    method: &str,
    url: &str,
    token: &str,
    body: Option<&Value>,
) -> Result<HttpEgressResponse> {
    let mut headers = BTreeMap::new();
    headers.insert("authorization".to_owned(), format!("Bearer {token}"));
    headers.insert("accept".to_owned(), "application/json".to_owned());
    let body_bytes = match body {
        Some(v) => {
            headers.insert("content-type".to_owned(), "application/json".to_owned());
            Some(serde_json::to_vec(v).map_err(|_| Failure("invalid_input"))?)
        }
        None if method == "PUT" => Some(Vec::new()),
        None => None,
    };
    ctx.http_request(method, url.to_owned(), headers, body_bytes)
        .await
}

async fn decode_gsc_response(
    ctx: &HostContext,
    account: Option<&Account>,
    resp: &HttpEgressResponse,
) -> Result<Value> {
    if (200..300).contains(&resp.status) {
        if resp.status == 204 || resp.body_base64.is_empty() {
            return Ok(json!({}));
        }
        let bytes = STANDARD
            .decode(&resp.body_base64)
            .map_err(|_| Failure("google_upstream_error"))?;
        if bytes.is_empty() {
            return Ok(json!({}));
        }
        return serde_json::from_slice(&bytes).map_err(|_| Failure("google_upstream_error"));
    }
    let body: Value = STANDARD
        .decode(&resp.body_base64)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null);
    let reason = body
        .pointer("/error/errors/0/reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    if resp.status == 401 {
        if let Some(acct) = account {
            oauth::invalidate_account_cache(ctx, acct).await;
            let _ = store::update_account_status(
                &ctx.emitter,
                &acct.account_id,
                "reauth_required",
                Some("google_reauth_required"),
                &now_rfc3339(),
            )
            .await;
        }
        return Err(Failure("google_reauth_required"));
    }
    if resp.status == 429
        || matches!(
            reason,
            "rateLimitExceeded" | "userRateLimitExceeded" | "quotaExceeded" | "dailyLimitExceeded"
        )
    {
        return Err(Failure("google_quota_exceeded"));
    }
    if resp.status == 403 {
        return Err(Failure("google_permission_denied"));
    }
    if resp.status == 404 {
        return Err(Failure("google_not_found"));
    }
    if resp.status == 400 {
        return Err(Failure("google_bad_request"));
    }
    if (500..600).contains(&resp.status) {
        return Err(Failure("google_upstream_error"));
    }
    Err(Failure("google_api_error"))
}

pub(crate) async fn accounts_list(ctx: &HostContext, input: Payload) -> Result<Value> {
    let page = parse_u64_param(&input.query, "page", 1)?;
    let page_size = parse_u64_param(&input.query, "page_size", 20)?;
    if page == 0 || !(1..=100).contains(&page_size) {
        return Err(Failure("invalid_input"));
    }
    let search = input
        .query
        .get("search")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let mut accounts = store::list_all_accounts(&ctx.emitter).await?;
    if !search.is_empty() {
        accounts.retain(|a| {
            a.email.to_lowercase().contains(&search)
                || a.display_name.to_lowercase().contains(&search)
        });
    }
    accounts.sort_by(|a, b| a.email.cmp(&b.email).then(a.account_id.cmp(&b.account_id)));
    let total = accounts.len();
    let offset = usize::try_from((page - 1).saturating_mul(page_size))
        .map_err(|_| Failure("invalid_input"))?;
    let items: Vec<Value> = accounts
        .into_iter()
        .skip(offset)
        .take(page_size as usize)
        .map(|a| a.summary())
        .collect();
    Ok(json!({
        "items": items,
        "total": total,
        "page": page,
        "page_size": page_size,
    }))
}

pub(crate) async fn accounts_get(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let account = store::get_account(&ctx.emitter, account_id).await?;
    Ok(json!({ "account": account.summary() }))
}

pub(crate) async fn accounts_update(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let enabled = input
        .body
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or(Failure("invalid_input"))?;
    let mut account = store::get_account(&ctx.emitter, account_id).await?;
    account.enabled = enabled;
    account.updated_at = now_rfc3339();
    store::update_account(&ctx.emitter, &account).await?;
    if !account.enabled {
        oauth::invalidate_account_cache(ctx, &account).await;
    }
    Ok(json!({ "account": account.summary() }))
}

pub(crate) async fn accounts_test(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let mut account = store::get_account(&ctx.emitter, account_id).await?;
    if !account.enabled {
        return Err(Failure("account_disabled"));
    }
    let token = oauth::access_token(ctx, &account, Some("")).await?;
    let sites = sync_sites_with_token(ctx, &mut account, &token).await?;
    Ok(json!({
        "authenticated": true,
        "status": account.status,
        "site_count": sites.len(),
        "checked_at": account.last_synced_at,
    }))
}

pub(crate) async fn accounts_delete(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let mut account = store::get_account(&ctx.emitter, account_id).await?;
    account.enabled = false;
    account.updated_at = now_rfc3339();
    store::update_account(&ctx.emitter, &account).await?;
    oauth::invalidate_account_cache(ctx, &account).await;
    if !account.credential_id.is_empty() {
        ctx.credentials
            .delete(account.credential_id.clone())
            .await
            .map_err(|_| Failure("gsc_storage_error"))?;
    }
    store::delete_account_and_sites(&ctx.emitter, account_id).await?;
    Ok(json!({ "deleted": true }))
}

pub(crate) async fn sync_sites_with_token(
    ctx: &HostContext,
    account: &mut Account,
    token: &str,
) -> Result<Vec<Site>> {
    let url = format!("{WEBMASTERS_V3}/sites");
    let data = gsc_request_with_token(ctx, "GET", &url, token, None).await?;
    let synced_at = now_rfc3339();
    let entries = data
        .get("siteEntry")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut sites = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(site_url) = entry.get("siteUrl").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if site_url.is_empty() {
            continue;
        }
        let permission_level = entry
            .get("permissionLevel")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("siteRestrictedUser")
            .to_owned();
        sites.push(Site {
            site_record_id: deterministic_site_record_id(&account.account_id, site_url),
            account_id: account.account_id.clone(),
            site_url: site_url.to_owned(),
            property_type: property_type_for_site_url(site_url),
            permission_level,
            sitemap_count: None,
            last_synced_at: synced_at.clone(),
        });
    }
    sites.sort_by(|a, b| a.site_url.cmp(&b.site_url));
    store::replace_sites_for_account(&ctx.emitter, &account.account_id, &sites).await?;
    account.site_count = sites.len() as u64;
    account.last_synced_at = Some(synced_at.clone());
    account.status = "active".to_owned();
    account.last_error = None;
    account.updated_at = synced_at;
    store::update_account(&ctx.emitter, account).await?;
    Ok(sites)
}

pub(crate) async fn sites_list(ctx: &HostContext, input: Payload) -> Result<Value> {
    let page = parse_u64_param(&input.query, "page", 1)?;
    let page_size = parse_u64_param(&input.query, "page_size", 50)?;
    if page == 0 || !(1..=100).contains(&page_size) {
        return Err(Failure("invalid_input"));
    }
    let account_filter = input
        .query
        .get("account_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let search = input
        .query
        .get("search")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();

    // Host filters gsc_accounts by caller's `view` record permissions automatically.
    let visible_accounts = store::list_all_accounts(&ctx.emitter).await?;
    let visible_ids: BTreeSet<String> = visible_accounts
        .into_iter()
        .map(|a| a.account_id)
        .collect();
    if let Some(id) = account_filter {
        if !visible_ids.contains(id) {
            return Err(Failure("account_not_found"));
        }
    }

    let mut sites = store::list_sites(&ctx.emitter, account_filter).await?;
    sites.retain(|site| visible_ids.contains(&site.account_id));
    if !search.is_empty() {
        sites.retain(|site| site.site_url.to_lowercase().contains(&search));
    }
    sites.sort_by(|a, b| {
        a.site_url
            .cmp(&b.site_url)
            .then(a.account_id.cmp(&b.account_id))
    });
    let total = sites.len();
    let offset = usize::try_from((page - 1).saturating_mul(page_size))
        .map_err(|_| Failure("invalid_input"))?;
    let items: Vec<Value> = sites
        .into_iter()
        .skip(offset)
        .take(page_size as usize)
        .map(|s| s.summary())
        .collect();
    Ok(json!({
        "items": items,
        "total": total,
        "page": page,
        "page_size": page_size,
    }))
}

pub(crate) async fn sites_sync(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let mut account = store::get_account(&ctx.emitter, account_id).await?;
    let token = oauth::access_token(ctx, &account, None).await?;
    let sites = match sync_sites_with_token(ctx, &mut account, &token).await {
        Ok(sites) => sites,
        Err(Failure("google_reauth_required")) => {
            let fresh = oauth::access_token(ctx, &account, Some(&token)).await?;
            sync_sites_with_token(ctx, &mut account, &fresh).await?
        }
        Err(err) => return Err(err),
    };
    Ok(json!({
        "account_id": account.account_id,
        "items": sites.iter().map(|s| s.summary()).collect::<Vec<_>>(),
        "total": sites.len(),
        "synced_at": account.last_synced_at,
    }))
}

pub(crate) async fn sites_get(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let site_url = required_string(&input.query, "site_url")?.trim();
    let account = store::get_account(&ctx.emitter, account_id).await?;
    let url = format!("{WEBMASTERS_V3}/sites/{}", encode_path_segment(site_url));
    let data = gsc_request(ctx, &account, "GET", &url, None).await?;
    Ok(json!({
        "site_url": data.get("siteUrl").and_then(Value::as_str).unwrap_or(site_url),
        "property_type": property_type_for_site_url(site_url),
        "permission_level": data.get("permissionLevel").and_then(Value::as_str).unwrap_or("siteRestrictedUser"),
    }))
}

pub(crate) async fn sites_add(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let site_url = required_string(&input.body, "site_url")?.trim();
    let account = store::get_account(&ctx.emitter, account_id).await?;
    require_full_scope(&account)?;
    let url = format!("{WEBMASTERS_V3}/sites/{}", encode_path_segment(site_url));
    let _ = gsc_request(ctx, &account, "PUT", &url, None).await?;
    let site = store::upsert_single_site(
        &ctx.emitter,
        &account.account_id,
        site_url,
        "siteOwner",
        &now_rfc3339(),
    )
    .await?;
    Ok(json!({ "added": true, "site": site.summary() }))
}

pub(crate) async fn sites_delete(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let site_url = required_string(&input.query, "site_url")?.trim();
    let account = store::get_account(&ctx.emitter, account_id).await?;
    require_full_scope(&account)?;
    let url = format!("{WEBMASTERS_V3}/sites/{}", encode_path_segment(site_url));
    let _ = gsc_request(ctx, &account, "DELETE", &url, None).await?;
    let _ = store::delete_single_site(&ctx.emitter, &account.account_id, site_url).await;
    Ok(json!({ "deleted": true, "site_url": site_url }))
}

pub(crate) async fn sitemaps_list(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let site_url = required_string(&input.query, "site_url")?.trim();
    let account = store::get_account(&ctx.emitter, account_id).await?;
    let mut url = format!(
        "{WEBMASTERS_V3}/sites/{}/sitemaps",
        encode_path_segment(site_url)
    );
    if let Some(index) = input
        .query
        .get("sitemap_index")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        url.push_str("?sitemapIndex=");
        url.push_str(&encode_path_segment(index));
    }
    let data = gsc_request(ctx, &account, "GET", &url, None).await?;
    let sitemaps = data
        .get("sitemap")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(json!({
        "site_url": site_url,
        "sitemaps": sitemaps,
    }))
}

pub(crate) async fn sitemaps_get(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let site_url = required_string(&input.query, "site_url")?.trim();
    let feedpath = required_string(&input.query, "feedpath")?.trim();
    let account = store::get_account(&ctx.emitter, account_id).await?;
    let url = format!(
        "{WEBMASTERS_V3}/sites/{}/sitemaps/{}",
        encode_path_segment(site_url),
        encode_path_segment(feedpath)
    );
    let data = gsc_request(ctx, &account, "GET", &url, None).await?;
    Ok(json!({
        "site_url": site_url,
        "sitemap": data,
    }))
}

pub(crate) async fn sitemaps_submit(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let site_url = required_string(&input.body, "site_url")?.trim();
    let feedpath = required_string(&input.body, "feedpath")?.trim();
    let account = store::get_account(&ctx.emitter, account_id).await?;
    require_full_scope(&account)?;
    let url = format!(
        "{WEBMASTERS_V3}/sites/{}/sitemaps/{}",
        encode_path_segment(site_url),
        encode_path_segment(feedpath)
    );
    let _ = gsc_request(ctx, &account, "PUT", &url, None).await?;
    Ok(json!({
        "submitted": true,
        "site_url": site_url,
        "feedpath": feedpath,
    }))
}

pub(crate) async fn sitemaps_delete(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let site_url = required_string(&input.query, "site_url")?.trim();
    let feedpath = required_string(&input.query, "feedpath")?.trim();
    let account = store::get_account(&ctx.emitter, account_id).await?;
    require_full_scope(&account)?;
    let url = format!(
        "{WEBMASTERS_V3}/sites/{}/sitemaps/{}",
        encode_path_segment(site_url),
        encode_path_segment(feedpath)
    );
    let _ = gsc_request(ctx, &account, "DELETE", &url, None).await?;
    Ok(json!({
        "deleted": true,
        "site_url": site_url,
        "feedpath": feedpath,
    }))
}

pub(crate) async fn analytics_query(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let site_url = required_string(&input.body, "site_url")?.trim();
    let start_date = required_string(&input.body, "start_date")?.trim();
    let end_date = required_string(&input.body, "end_date")?.trim();
    let account = store::get_account(&ctx.emitter, account_id).await?;

    let mut payload = Map::new();
    payload.insert("startDate".to_owned(), Value::String(start_date.to_owned()));
    payload.insert("endDate".to_owned(), Value::String(end_date.to_owned()));
    if let Some(dimensions) = input.body.get("dimensions").filter(|v| v.is_array()) {
        payload.insert("dimensions".to_owned(), dimensions.clone());
    }
    if let Some(search_type) = input.body.get("type").and_then(Value::as_str) {
        payload.insert("type".to_owned(), Value::String(search_type.to_owned()));
    }
    if let Some(agg) = input.body.get("aggregation_type").and_then(Value::as_str) {
        payload.insert("aggregationType".to_owned(), Value::String(agg.to_owned()));
    }
    if let Some(filters) = input
        .body
        .get("dimension_filter_groups")
        .filter(|v| v.is_array())
    {
        payload.insert("dimensionFilterGroups".to_owned(), filters.clone());
    }
    if let Some(row_limit) = input.body.get("row_limit").and_then(Value::as_u64) {
        if !(1..=25_000).contains(&row_limit) {
            return Err(Failure("invalid_input"));
        }
        payload.insert("rowLimit".to_owned(), Value::Number(row_limit.into()));
    }
    if let Some(start_row) = input.body.get("start_row").and_then(Value::as_u64) {
        payload.insert("startRow".to_owned(), Value::Number(start_row.into()));
    }
    if let Some(data_state) = input.body.get("data_state").and_then(Value::as_str) {
        payload.insert("dataState".to_owned(), Value::String(data_state.to_owned()));
    }

    let url = format!(
        "{WEBMASTERS_V3}/sites/{}/searchAnalytics/query",
        encode_path_segment(site_url)
    );
    let data = gsc_request(ctx, &account, "POST", &url, Some(&Value::Object(payload))).await?;
    Ok(json!({
        "site_url": site_url,
        "rows": data.get("rows").and_then(Value::as_array).cloned().unwrap_or_default(),
        "response_aggregation_type": data.get("responseAggregationType").cloned().unwrap_or(Value::Null),
    }))
}

pub(crate) async fn sites_overview(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let site_url = required_string(&input.body, "site_url")?.trim();
    let days = parse_u64_param(&input.body, "days", 28)?;
    let top_limit = parse_u64_param(&input.body, "top_limit", 20)?;
    if !(1..=480).contains(&days) || !(1..=100).contains(&top_limit) {
        return Err(Failure("invalid_input"));
    }
    let search_type = input
        .body
        .get("search_type")
        .and_then(Value::as_str)
        .unwrap_or("web");
    let account = store::get_account(&ctx.emitter, account_id).await?;
    let _ = oauth::access_token(ctx, &account, None).await?;

    let end_offset = 2i64;
    let start_offset = end_offset + (days as i64) - 1;
    let prev_end_offset = start_offset + 1;
    let prev_start_offset = prev_end_offset + (days as i64) - 1;

    let start_date = utc_date_days_ago(start_offset);
    let end_date = utc_date_days_ago(end_offset);
    let prev_start_date = utc_date_days_ago(prev_start_offset);
    let prev_end_date = utc_date_days_ago(prev_end_offset);

    let encoded_site = encode_path_segment(site_url);
    let site_detail_url = format!("{WEBMASTERS_V3}/sites/{encoded_site}");
    let sitemaps_url = format!("{WEBMASTERS_V3}/sites/{encoded_site}/sitemaps");
    let sa_url = format!("{WEBMASTERS_V3}/sites/{encoded_site}/searchAnalytics/query");

    let cur_total_body = json!({ "startDate": start_date, "endDate": end_date, "type": search_type });
    let prev_total_body =
        json!({ "startDate": prev_start_date, "endDate": prev_end_date, "type": search_type });
    let daily_body = json!({
        "startDate": start_date, "endDate": end_date, "type": search_type,
        "dimensions": ["date"], "rowLimit": days.min(1000)
    });
    let top_queries_body = json!({
        "startDate": start_date, "endDate": end_date, "type": search_type,
        "dimensions": ["query"], "rowLimit": top_limit
    });
    let top_pages_body = json!({
        "startDate": start_date, "endDate": end_date, "type": search_type,
        "dimensions": ["page"], "rowLimit": top_limit
    });
    let devices_body = json!({
        "startDate": start_date, "endDate": end_date, "type": search_type,
        "dimensions": ["device"], "rowLimit": 10
    });

    let (site_res, sitemaps_res, cur_res, prev_res, daily_res, queries_res, pages_res, devices_res) = tokio::join!(
        gsc_request(ctx, &account, "GET", &site_detail_url, None),
        gsc_request(ctx, &account, "GET", &sitemaps_url, None),
        gsc_request(ctx, &account, "POST", &sa_url, Some(&cur_total_body)),
        gsc_request(ctx, &account, "POST", &sa_url, Some(&prev_total_body)),
        gsc_request(ctx, &account, "POST", &sa_url, Some(&daily_body)),
        gsc_request(ctx, &account, "POST", &sa_url, Some(&top_queries_body)),
        gsc_request(ctx, &account, "POST", &sa_url, Some(&top_pages_body)),
        gsc_request(ctx, &account, "POST", &sa_url, Some(&devices_body)),
    );

    let site_info = site_res?;
    let sitemaps = sitemaps_res?;
    let cur_totals = extract_totals(&cur_res?);
    let prev_totals = extract_totals(&prev_res?);

    Ok(json!({
        "site": {
            "site_url": site_info.get("siteUrl").and_then(Value::as_str).unwrap_or(site_url),
            "property_type": property_type_for_site_url(site_url),
            "permission_level": site_info.get("permissionLevel").and_then(Value::as_str).unwrap_or("siteRestrictedUser"),
        },
        "window": {
            "days": days,
            "search_type": search_type,
            "start_date": start_date,
            "end_date": end_date,
            "previous_start_date": prev_start_date,
            "previous_end_date": prev_end_date,
        },
        "totals": cur_totals,
        "previous_totals": prev_totals,
        "sitemaps": sitemaps.get("sitemap").and_then(Value::as_array).cloned().unwrap_or_default(),
        "daily_trend": daily_res?.get("rows").and_then(Value::as_array).cloned().unwrap_or_default(),
        "top_queries": queries_res?.get("rows").and_then(Value::as_array).cloned().unwrap_or_default(),
        "top_pages": pages_res?.get("rows").and_then(Value::as_array).cloned().unwrap_or_default(),
        "devices": devices_res?.get("rows").and_then(Value::as_array).cloned().unwrap_or_default(),
    }))
}

fn extract_totals(data: &Value) -> Value {
    let row = data
        .get("rows")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first());
    match row {
        Some(r) => json!({
            "clicks": r.get("clicks").and_then(Value::as_f64).unwrap_or(0.0),
            "impressions": r.get("impressions").and_then(Value::as_f64).unwrap_or(0.0),
            "ctr": r.get("ctr").and_then(Value::as_f64).unwrap_or(0.0),
            "position": r.get("position").and_then(Value::as_f64).unwrap_or(0.0),
        }),
        None => json!({ "clicks": 0.0, "impressions": 0.0, "ctr": 0.0, "position": 0.0 }),
    }
}

pub(crate) async fn inspection_inspect(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let site_url = required_string(&input.body, "site_url")?.trim();
    let inspection_url = required_string(&input.body, "inspection_url")?.trim();
    let account = store::get_account(&ctx.emitter, account_id).await?;

    let mut payload = Map::new();
    payload.insert("siteUrl".to_owned(), Value::String(site_url.to_owned()));
    payload.insert(
        "inspectionUrl".to_owned(),
        Value::String(inspection_url.to_owned()),
    );
    if let Some(lang) = input
        .body
        .get("language_code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        payload.insert("languageCode".to_owned(), Value::String(lang.to_owned()));
    }

    let data = gsc_request(
        ctx,
        &account,
        "POST",
        INSPECTION_V1,
        Some(&Value::Object(payload)),
    )
    .await?;
    Ok(json!({
        "site_url": site_url,
        "inspection_url": inspection_url,
        "inspection_result": data.get("inspectionResult").cloned().unwrap_or(data),
    }))
}
