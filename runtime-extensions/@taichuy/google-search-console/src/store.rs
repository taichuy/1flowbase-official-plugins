use crate::{
    model::{
        deterministic_site_record_id, property_type_for_site_url, Account, OAuthConfig, ScopeMode,
        Site, CONFIG_ID,
    },
    Failure, Result,
};
use runtime_extension_sdk::{
    MultiplexEmitter, MultiplexHostService, PluginDataFilter, PluginDataFilterOperator,
    PluginDataOperation, PluginDataOperationResult, PluginDataOrder, PluginDataOrderDirection,
    PluginDataPage, PluginDataRequest, PluginDataResponse, PluginDataRow, PluginDataTarget,
    PluginDataValue,
};
use serde_json::Value;
use std::collections::BTreeMap;

const CONFIG_COLLECTION: &str = "gsc_oauth_config";
const ACCOUNTS_COLLECTION: &str = "gsc_accounts";
const SITES_COLLECTION: &str = "gsc_sites";

const CONFIG_FIELDS: &[&str] = &[
    "config_id",
    "auth_mode",
    "client_id",
    "credential_id",
    "default_scope_mode",
    "updated_at",
];

const ACCOUNT_FIELDS: &[&str] = &[
    "account_id",
    "google_sub",
    "email",
    "display_name",
    "avatar_url",
    "scope_mode",
    "granted_scopes",
    "enabled",
    "status",
    "last_error",
    "credential_id",
    "site_count",
    "last_synced_at",
    "created_at",
    "updated_at",
];

const SITE_FIELDS: &[&str] = &[
    "site_record_id",
    "account_id",
    "site_url",
    "property_type",
    "permission_level",
    "sitemap_count",
    "last_synced_at",
];

fn target(collection: &str) -> PluginDataTarget {
    PluginDataTarget::OwnedCollection {
        collection_code: collection.to_owned(),
    }
}

fn fields_vec(fields: &[&str]) -> Vec<String> {
    fields.iter().map(|s| (*s).to_owned()).collect()
}

fn eq_filter(field: &str, value: &str) -> PluginDataFilter {
    PluginDataFilter {
        field: field.to_owned(),
        operator: PluginDataFilterOperator::Equal,
        value: Some(PluginDataValue::String(value.to_owned())),
    }
}

fn opt_string_value(value: Option<&str>) -> PluginDataValue {
    match value.map(str::trim).filter(|s| !s.is_empty()) {
        Some(v) => PluginDataValue::String(v.to_owned()),
        None => PluginDataValue::Null,
    }
}

fn req_str(row: &BTreeMap<String, PluginDataValue>, key: &str) -> Result<String> {
    match row.get(key) {
        Some(PluginDataValue::String(v))
        | Some(PluginDataValue::Uuid(v))
        | Some(PluginDataValue::Datetime(v)) => Ok(v.clone()),
        _ => Err(Failure("gsc_storage_error")),
    }
}

fn opt_str(row: &BTreeMap<String, PluginDataValue>, key: &str) -> Result<Option<String>> {
    match row.get(key) {
        None | Some(PluginDataValue::Null) => Ok(None),
        Some(PluginDataValue::String(v))
        | Some(PluginDataValue::Uuid(v))
        | Some(PluginDataValue::Datetime(v)) => {
            if v.is_empty() {
                Ok(None)
            } else {
                Ok(Some(v.clone()))
            }
        }
        _ => Err(Failure("gsc_storage_error")),
    }
}

fn req_bool(row: &BTreeMap<String, PluginDataValue>, key: &str) -> Result<bool> {
    match row.get(key) {
        Some(PluginDataValue::Boolean(v)) => Ok(*v),
        _ => Err(Failure("gsc_storage_error")),
    }
}

fn parse_u64(raw: &str) -> Result<u64> {
    if let Ok(v) = raw.parse::<u64>() {
        return Ok(v);
    }
    if let Ok(v) = raw.parse::<f64>() {
        if v.is_finite() && v >= 0.0 {
            return Ok(v.round() as u64);
        }
    }
    Err(Failure("gsc_storage_error"))
}

fn req_u64(row: &BTreeMap<String, PluginDataValue>, key: &str) -> Result<u64> {
    match row.get(key) {
        Some(PluginDataValue::Number(raw)) => parse_u64(raw),
        _ => Err(Failure("gsc_storage_error")),
    }
}

fn opt_u64(row: &BTreeMap<String, PluginDataValue>, key: &str) -> Result<Option<u64>> {
    match row.get(key) {
        None | Some(PluginDataValue::Null) => Ok(None),
        Some(PluginDataValue::Number(raw)) => parse_u64(raw).map(Some),
        _ => Err(Failure("gsc_storage_error")),
    }
}

fn req_string_vec(row: &BTreeMap<String, PluginDataValue>, key: &str) -> Result<Vec<String>> {
    match row.get(key) {
        Some(PluginDataValue::Json(Value::Array(items))) => Ok(items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()),
        _ => Err(Failure("gsc_storage_error")),
    }
}

pub(crate) async fn call(
    emitter: &MultiplexEmitter,
    operations: Vec<PluginDataOperation>,
) -> Result<Vec<PluginDataOperationResult>> {
    let request = PluginDataRequest {
        idempotency_key: None,
        operations,
    };
    request.validate().map_err(|_| Failure("gsc_storage_error"))?;
    let response = emitter
        .callback(
            MultiplexHostService::PluginDataV1,
            serde_json::to_value(request).map_err(|_| Failure("gsc_storage_error"))?,
        )
        .await
        .map_err(|_| Failure("gsc_storage_error"))?;
    if let Some(error) = response.get("error").filter(|e| !e.is_null()) {
        let code = error.get("code").and_then(Value::as_str).unwrap_or("");
        if code.contains("permission") {
            return Err(Failure("permission_denied"));
        }
        return Err(Failure("gsc_storage_error"));
    }
    let parsed: PluginDataResponse = serde_json::from_value(
        response
            .get("result")
            .cloned()
            .ok_or(Failure("gsc_storage_error"))?,
    )
    .map_err(|_| Failure("gsc_storage_error"))?;
    Ok(parsed.results)
}

fn decode_oauth_config(row: PluginDataRow) -> Result<OAuthConfig> {
    let v = &row.values;
    Ok(OAuthConfig {
        config_id: req_str(v, "config_id")?,
        auth_mode: req_str(v, "auth_mode")?,
        client_id: req_str(v, "client_id")?,
        credential_id: req_str(v, "credential_id")?,
        default_scope_mode: ScopeMode::parse(&req_str(v, "default_scope_mode")?)?,
        updated_at: req_str(v, "updated_at")?,
    })
}

fn decode_account(row: PluginDataRow) -> Result<Account> {
    let v = &row.values;
    Ok(Account {
        account_id: req_str(v, "account_id")?,
        google_sub: req_str(v, "google_sub")?,
        email: req_str(v, "email")?,
        display_name: req_str(v, "display_name")?,
        avatar_url: opt_str(v, "avatar_url")?,
        scope_mode: ScopeMode::parse(&req_str(v, "scope_mode")?)?,
        granted_scopes: req_string_vec(v, "granted_scopes")?,
        enabled: req_bool(v, "enabled")?,
        status: req_str(v, "status")?,
        last_error: opt_str(v, "last_error")?,
        credential_id: req_str(v, "credential_id")?,
        site_count: req_u64(v, "site_count")?,
        last_synced_at: opt_str(v, "last_synced_at")?,
        created_at: req_str(v, "created_at")?,
        updated_at: req_str(v, "updated_at")?,
    })
}

fn decode_site(row: PluginDataRow) -> Result<Site> {
    let v = &row.values;
    Ok(Site {
        site_record_id: req_str(v, "site_record_id")?,
        account_id: req_str(v, "account_id")?,
        site_url: req_str(v, "site_url")?,
        property_type: req_str(v, "property_type")?,
        permission_level: req_str(v, "permission_level")?,
        sitemap_count: opt_u64(v, "sitemap_count")?,
        last_synced_at: req_str(v, "last_synced_at")?,
    })
}

fn account_values(account: &Account, include_identity: bool) -> BTreeMap<String, PluginDataValue> {
    let mut values = BTreeMap::new();
    if include_identity {
        values.insert(
            "account_id".to_owned(),
            PluginDataValue::String(account.account_id.clone()),
        );
    }
    values.insert(
        "google_sub".to_owned(),
        PluginDataValue::String(account.google_sub.clone()),
    );
    values.insert(
        "email".to_owned(),
        PluginDataValue::String(account.email.clone()),
    );
    let display_name = if account.display_name.trim().is_empty() {
        account.email.as_str()
    } else {
        account.display_name.as_str()
    };
    values.insert(
        "display_name".to_owned(),
        PluginDataValue::String(display_name.to_owned()),
    );
    values.insert(
        "avatar_url".to_owned(),
        opt_string_value(account.avatar_url.as_deref()),
    );
    values.insert(
        "scope_mode".to_owned(),
        PluginDataValue::String(account.scope_mode.as_str().to_owned()),
    );
    values.insert(
        "granted_scopes".to_owned(),
        PluginDataValue::Json(Value::Array(
            account
                .granted_scopes
                .iter()
                .map(|s| Value::String(s.clone()))
                .collect(),
        )),
    );
    values.insert("enabled".to_owned(), PluginDataValue::Boolean(account.enabled));
    values.insert(
        "status".to_owned(),
        PluginDataValue::String(account.status.clone()),
    );
    values.insert(
        "last_error".to_owned(),
        opt_string_value(account.last_error.as_deref()),
    );
    values.insert(
        "credential_id".to_owned(),
        PluginDataValue::String(account.credential_id.clone()),
    );
    values.insert(
        "site_count".to_owned(),
        PluginDataValue::Number(account.site_count.to_string()),
    );
    values.insert(
        "last_synced_at".to_owned(),
        opt_string_value(account.last_synced_at.as_deref()),
    );
    values
}

fn site_values(site: &Site) -> BTreeMap<String, PluginDataValue> {
    let mut values = BTreeMap::new();
    values.insert(
        "site_record_id".to_owned(),
        PluginDataValue::String(site.site_record_id.clone()),
    );
    values.insert(
        "account_id".to_owned(),
        PluginDataValue::String(site.account_id.clone()),
    );
    values.insert(
        "site_url".to_owned(),
        PluginDataValue::String(site.site_url.clone()),
    );
    values.insert(
        "property_type".to_owned(),
        PluginDataValue::String(site.property_type.clone()),
    );
    values.insert(
        "permission_level".to_owned(),
        PluginDataValue::String(site.permission_level.clone()),
    );
    values.insert(
        "sitemap_count".to_owned(),
        match site.sitemap_count {
            Some(count) => PluginDataValue::Number(count.to_string()),
            None => PluginDataValue::Null,
        },
    );
    values.insert(
        "last_synced_at".to_owned(),
        PluginDataValue::String(site.last_synced_at.clone()),
    );
    values
}

pub(crate) async fn get_oauth_config(emitter: &MultiplexEmitter) -> Result<Option<OAuthConfig>> {
    match call(
        emitter,
        vec![PluginDataOperation::FindOne {
            target: target(CONFIG_COLLECTION),
            fields: fields_vec(CONFIG_FIELDS),
            filters: vec![eq_filter("config_id", CONFIG_ID)],
        }],
    )
    .await?
    .pop()
    {
        Some(PluginDataOperationResult::OptionalRow { row: Some(row) }) => {
            decode_oauth_config(row).map(Some)
        }
        Some(PluginDataOperationResult::OptionalRow { row: None }) => Ok(None),
        _ => Err(Failure("gsc_storage_error")),
    }
}

pub(crate) async fn upsert_oauth_config(
    emitter: &MultiplexEmitter,
    config: &OAuthConfig,
) -> Result<()> {
    let mut identity = BTreeMap::new();
    identity.insert(
        "config_id".to_owned(),
        PluginDataValue::String(config.config_id.clone()),
    );
    let mut values = BTreeMap::new();
    values.insert(
        "config_id".to_owned(),
        PluginDataValue::String(config.config_id.clone()),
    );
    values.insert(
        "auth_mode".to_owned(),
        PluginDataValue::String(config.auth_mode.clone()),
    );
    values.insert(
        "client_id".to_owned(),
        PluginDataValue::String(config.client_id.clone()),
    );
    values.insert(
        "credential_id".to_owned(),
        PluginDataValue::String(config.credential_id.clone()),
    );
    values.insert(
        "default_scope_mode".to_owned(),
        PluginDataValue::String(config.default_scope_mode.as_str().to_owned()),
    );
    call(
        emitter,
        vec![PluginDataOperation::Upsert {
            target: target(CONFIG_COLLECTION),
            identity,
            values,
        }],
    )
    .await?;
    Ok(())
}

async fn find_all(
    emitter: &MultiplexEmitter,
    collection: &str,
    fields: &[&str],
    filters: Vec<PluginDataFilter>,
    order_field: &str,
) -> Result<Vec<PluginDataRow>> {
    let mut out = Vec::new();
    let mut offset = 0u64;
    let field_names = fields_vec(fields);
    loop {
        let mut results = call(
            emitter,
            vec![PluginDataOperation::Find {
                target: target(collection),
                fields: field_names.clone(),
                filters: filters.clone(),
                order: vec![PluginDataOrder {
                    field: order_field.to_owned(),
                    direction: PluginDataOrderDirection::Ascending,
                }],
                page: PluginDataPage { limit: 100, offset },
            }],
        )
        .await?;
        match results.pop() {
            Some(PluginDataOperationResult::Rows { rows }) => {
                let len = rows.len();
                out.extend(rows);
                if len < 100 {
                    break;
                }
                offset = offset.saturating_add(100);
            }
            _ => return Err(Failure("gsc_storage_error")),
        }
    }
    Ok(out)
}

pub(crate) async fn find_account(
    emitter: &MultiplexEmitter,
    account_id: &str,
) -> Result<Option<Account>> {
    match call(
        emitter,
        vec![PluginDataOperation::FindOne {
            target: target(ACCOUNTS_COLLECTION),
            fields: fields_vec(ACCOUNT_FIELDS),
            filters: vec![eq_filter("account_id", account_id)],
        }],
    )
    .await?
    .pop()
    {
        Some(PluginDataOperationResult::OptionalRow { row: Some(row) }) => {
            decode_account(row).map(Some)
        }
        Some(PluginDataOperationResult::OptionalRow { row: None }) => Ok(None),
        _ => Err(Failure("gsc_storage_error")),
    }
}

pub(crate) async fn get_account(emitter: &MultiplexEmitter, account_id: &str) -> Result<Account> {
    find_account(emitter, account_id)
        .await?
        .ok_or(Failure("account_not_found"))
}

pub(crate) async fn list_all_accounts(emitter: &MultiplexEmitter) -> Result<Vec<Account>> {
    find_all(
        emitter,
        ACCOUNTS_COLLECTION,
        ACCOUNT_FIELDS,
        vec![],
        "email",
    )
    .await?
    .into_iter()
    .map(decode_account)
    .collect()
}

pub(crate) async fn upsert_account(emitter: &MultiplexEmitter, account: &Account) -> Result<()> {
    let mut identity = BTreeMap::new();
    identity.insert(
        "account_id".to_owned(),
        PluginDataValue::String(account.account_id.clone()),
    );
    let values = account_values(account, true);
    call(
        emitter,
        vec![PluginDataOperation::Upsert {
            target: target(ACCOUNTS_COLLECTION),
            identity,
            values,
        }],
    )
    .await?;
    Ok(())
}

pub(crate) async fn update_account(emitter: &MultiplexEmitter, account: &Account) -> Result<()> {
    let results = call(
        emitter,
        vec![PluginDataOperation::Update {
            target: target(ACCOUNTS_COLLECTION),
            filters: vec![eq_filter("account_id", &account.account_id)],
            values: account_values(account, false),
        }],
    )
    .await?;
    match results.as_slice() {
        [PluginDataOperationResult::Mutation { affected }] if *affected > 0 => Ok(()),
        [PluginDataOperationResult::Mutation { .. }] => Err(Failure("account_not_found")),
        _ => Err(Failure("gsc_storage_error")),
    }
}

pub(crate) async fn update_account_status(
    emitter: &MultiplexEmitter,
    account_id: &str,
    status: &str,
    last_error: Option<&str>,
    _updated_at: &str,
) -> Result<()> {
    let mut values = BTreeMap::new();
    values.insert(
        "status".to_owned(),
        PluginDataValue::String(status.to_owned()),
    );
    values.insert("last_error".to_owned(), opt_string_value(last_error));
    let _ = call(
        emitter,
        vec![PluginDataOperation::Update {
            target: target(ACCOUNTS_COLLECTION),
            filters: vec![eq_filter("account_id", account_id)],
            values,
        }],
    )
    .await?;
    Ok(())
}

pub(crate) async fn delete_account_and_sites(
    emitter: &MultiplexEmitter,
    account_id: &str,
) -> Result<()> {
    call(
        emitter,
        vec![
            PluginDataOperation::Delete {
                target: target(SITES_COLLECTION),
                filters: vec![eq_filter("account_id", account_id)],
            },
            PluginDataOperation::Delete {
                target: target(ACCOUNTS_COLLECTION),
                filters: vec![eq_filter("account_id", account_id)],
            },
        ],
    )
    .await?;
    Ok(())
}

pub(crate) async fn list_sites(
    emitter: &MultiplexEmitter,
    account_id: Option<&str>,
) -> Result<Vec<Site>> {
    let filters = account_id
        .map(|id| vec![eq_filter("account_id", id)])
        .unwrap_or_default();
    find_all(emitter, SITES_COLLECTION, SITE_FIELDS, filters, "site_url")
        .await?
        .into_iter()
        .map(decode_site)
        .collect()
}

pub(crate) async fn find_site_for_account(
    emitter: &MultiplexEmitter,
    account_id: &str,
    site_url: &str,
) -> Result<Option<Site>> {
    match call(
        emitter,
        vec![PluginDataOperation::FindOne {
            target: target(SITES_COLLECTION),
            fields: fields_vec(SITE_FIELDS),
            filters: vec![
                eq_filter("account_id", account_id),
                eq_filter("site_url", site_url),
            ],
        }],
    )
    .await?
    .pop()
    {
        Some(PluginDataOperationResult::OptionalRow { row: Some(row) }) => {
            decode_site(row).map(Some)
        }
        Some(PluginDataOperationResult::OptionalRow { row: None }) => Ok(None),
        _ => Err(Failure("gsc_storage_error")),
    }
}

pub(crate) async fn replace_sites_for_account(
    emitter: &MultiplexEmitter,
    account_id: &str,
    sites: &[Site],
) -> Result<()> {
    let delete_op = PluginDataOperation::Delete {
        target: target(SITES_COLLECTION),
        filters: vec![eq_filter("account_id", account_id)],
    };
    if sites.len() < 64 {
        let mut ops = Vec::with_capacity(1 + sites.len());
        ops.push(delete_op);
        for site in sites {
            ops.push(PluginDataOperation::Insert {
                target: target(SITES_COLLECTION),
                values: site_values(site),
            });
        }
        call(emitter, ops).await?;
        return Ok(());
    }
    call(emitter, vec![delete_op]).await?;
    for chunk in sites.chunks(64) {
        let ops = chunk
            .iter()
            .map(|site| PluginDataOperation::Insert {
                target: target(SITES_COLLECTION),
                values: site_values(site),
            })
            .collect();
        call(emitter, ops).await?;
    }
    Ok(())
}

pub(crate) async fn upsert_single_site(
    emitter: &MultiplexEmitter,
    account_id: &str,
    site_url: &str,
    permission_level: &str,
    synced_at: &str,
) -> Result<Site> {
    let site = Site {
        site_record_id: deterministic_site_record_id(account_id, site_url),
        account_id: account_id.to_owned(),
        site_url: site_url.to_owned(),
        property_type: property_type_for_site_url(site_url),
        permission_level: permission_level.to_owned(),
        sitemap_count: None,
        last_synced_at: synced_at.to_owned(),
    };
    let mut identity = BTreeMap::new();
    identity.insert(
        "site_record_id".to_owned(),
        PluginDataValue::String(site.site_record_id.clone()),
    );
    call(
        emitter,
        vec![PluginDataOperation::Upsert {
            target: target(SITES_COLLECTION),
            identity,
            values: site_values(&site),
        }],
    )
    .await?;
    Ok(site)
}

pub(crate) async fn delete_single_site(
    emitter: &MultiplexEmitter,
    account_id: &str,
    site_url: &str,
) -> Result<()> {
    call(
        emitter,
        vec![PluginDataOperation::Delete {
            target: target(SITES_COLLECTION),
            filters: vec![
                eq_filter("account_id", account_id),
                eq_filter("site_url", site_url),
            ],
        }],
    )
    .await?;
    Ok(())
}
