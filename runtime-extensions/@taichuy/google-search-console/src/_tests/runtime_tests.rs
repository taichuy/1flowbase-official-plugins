use crate::Runtime;
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::{
    serve_duplex_io, MultiplexEnvelope, MultiplexHostMessage, MultiplexHostService,
    MultiplexWorkerMessage, PluginDataFilter, PluginDataFilterOperator, PluginDataOperation,
    PluginDataOperationResult, PluginDataRequest, PluginDataRow, PluginDataTarget,
    PluginDataValue,
};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Default)]
struct HostSimulator {
    tables: BTreeMap<String, Vec<BTreeMap<String, PluginDataValue>>>,
    secrets: BTreeMap<String, Value>,
    ephemeral: BTreeMap<String, Value>,
    gate_acquires: usize,
    gate_releases: usize,
    observed_http: Vec<(String, String, String)>,
    next_gsc_status: VecDeque<u16>,
    revoke_refresh: bool,
    issued_tokens: usize,
    fail_sites_sync: bool,
}

impl HostSimulator {
    fn collection_name(target: &PluginDataTarget) -> &str {
        match target {
            PluginDataTarget::OwnedCollection { collection_code } => collection_code,
            PluginDataTarget::ExtensionProjection { target_table } => target_table,
        }
    }

    fn matches(row: &BTreeMap<String, PluginDataValue>, filters: &[PluginDataFilter]) -> bool {
        filters.iter().all(|f| {
            let cell = row.get(&f.field);
            match f.operator {
                PluginDataFilterOperator::Equal => cell == f.value.as_ref(),
                PluginDataFilterOperator::NotEqual => cell != f.value.as_ref(),
                PluginDataFilterOperator::IsNull => matches!(cell, None | Some(PluginDataValue::Null)),
                PluginDataFilterOperator::IsNotNull => !matches!(cell, None | Some(PluginDataValue::Null)),
                _ => false,
            }
        })
    }

    fn data(&mut self, request: Value) -> Value {
        let req: PluginDataRequest = serde_json::from_value(request).unwrap();
        req.validate().unwrap();
        let mut results = Vec::new();
        for op in req.operations {
            let res = match op {
                PluginDataOperation::Find { target, filters, page, .. } => {
                    let rows = self
                        .tables
                        .get(Self::collection_name(&target))
                        .into_iter()
                        .flatten()
                        .filter(|r| Self::matches(r, &filters))
                        .skip(page.offset as usize)
                        .take(page.limit as usize)
                        .cloned()
                        .map(|values| PluginDataRow { values })
                        .collect();
                    PluginDataOperationResult::Rows { rows }
                }
                PluginDataOperation::FindOne { target, filters, .. } => {
                    let row = self
                        .tables
                        .get(Self::collection_name(&target))
                        .into_iter()
                        .flatten()
                        .find(|r| Self::matches(r, &filters))
                        .cloned()
                        .map(|values| PluginDataRow { values });
                    PluginDataOperationResult::OptionalRow { row }
                }
                PluginDataOperation::Count { target, filters } => {
                    let count = self
                        .tables
                        .get(Self::collection_name(&target))
                        .into_iter()
                        .flatten()
                        .filter(|r| Self::matches(r, &filters))
                        .count() as u64;
                    PluginDataOperationResult::Count { count }
                }
                PluginDataOperation::Insert { target, mut values } => {
                    for reserved in ["id", "created_at", "updated_at"] {
                        assert!(!values.contains_key(reserved), "immutable field {reserved} written on Insert");
                    }
                    let ts = crate::model::now_rfc3339();
                    values.insert("created_at".to_owned(), PluginDataValue::Datetime(ts.clone()));
                    values.insert("updated_at".to_owned(), PluginDataValue::Datetime(ts));
                    self.tables
                        .entry(Self::collection_name(&target).to_owned())
                        .or_default()
                        .push(values);
                    PluginDataOperationResult::Mutation { affected: 1 }
                }
                PluginDataOperation::Update { target, filters, values } => {
                    for reserved in ["id", "created_at", "updated_at"] {
                        assert!(!values.contains_key(reserved), "immutable field {reserved} written on Update");
                    }
                    let ts = crate::model::now_rfc3339();
                    let mut affected = 0u64;
                    if let Some(rows) = self.tables.get_mut(Self::collection_name(&target)) {
                        for row in rows.iter_mut() {
                            if Self::matches(row, &filters) {
                                for (k, v) in &values {
                                    row.insert(k.clone(), v.clone());
                                }
                                row.insert("updated_at".to_owned(), PluginDataValue::Datetime(ts.clone()));
                                affected += 1;
                            }
                        }
                    }
                    PluginDataOperationResult::Mutation { affected }
                }
                PluginDataOperation::Upsert { target, identity, values } => {
                    for reserved in ["id", "created_at", "updated_at"] {
                        assert!(!values.contains_key(reserved), "immutable field {reserved} written on Upsert");
                    }
                    let ts = crate::model::now_rfc3339();
                    let filters: Vec<PluginDataFilter> = identity
                        .iter()
                        .map(|(field, value)| PluginDataFilter {
                            field: field.clone(),
                            operator: PluginDataFilterOperator::Equal,
                            value: Some(value.clone()),
                        })
                        .collect();
                    let rows = self
                        .tables
                        .entry(Self::collection_name(&target).to_owned())
                        .or_default();
                    if let Some(existing) = rows.iter_mut().find(|r| Self::matches(r, &filters)) {
                        for (k, v) in values {
                            existing.insert(k, v);
                        }
                        existing.insert("updated_at".to_owned(), PluginDataValue::Datetime(ts));
                    } else {
                        let mut merged = identity;
                        for (k, v) in values {
                            merged.insert(k, v);
                        }
                        merged.insert("created_at".to_owned(), PluginDataValue::Datetime(ts.clone()));
                        merged.insert("updated_at".to_owned(), PluginDataValue::Datetime(ts));
                        rows.push(merged);
                    }
                    PluginDataOperationResult::Mutation { affected: 1 }
                }
                PluginDataOperation::Delete { target, filters } => {
                    let rows = self
                        .tables
                        .entry(Self::collection_name(&target).to_owned())
                        .or_default();
                    let before = rows.len();
                    rows.retain(|r| !Self::matches(r, &filters));
                    PluginDataOperationResult::Mutation {
                        affected: (before - rows.len()) as u64,
                    }
                }
            };
            results.push(res);
        }
        json!({ "result": { "results": results, "replayed": false } })
    }

    fn http(&mut self, request: Value) -> Value {
        let method = request["method"].as_str().unwrap().to_owned();
        let url = request["url"].as_str().unwrap().to_owned();
        let body_text = request["body_base64"]
            .as_str()
            .map(|b| String::from_utf8(STANDARD.decode(b).unwrap()).unwrap())
            .unwrap_or_default();
        self.observed_http.push((method.clone(), url.clone(), body_text.clone()));

        if url == "https://oauth2.googleapis.com/token" {
            if body_text.contains("grant_type=refresh_token") && self.revoke_refresh {
                return http_json(400, json!({ "error": "invalid_grant" }));
            }
            self.issued_tokens += 1;
            let token = format!("ya29.tok-{}", self.issued_tokens);
            let scope = if body_text.contains("code=code-full") {
                "openid email profile https://www.googleapis.com/auth/webmasters"
            } else {
                "openid email profile https://www.googleapis.com/auth/webmasters.readonly"
            };
            let mut resp = json!({
                "access_token": token,
                "expires_in": 3600,
                "token_type": "Bearer",
                "scope": scope,
            });
            if body_text.contains("grant_type=authorization_code") {
                resp["refresh_token"] = json!("1//refresh-secret-xyz");
            }
            return http_json(200, resp);
        }
        if url == "https://www.googleapis.com/oauth2/v2/userinfo" {
            return http_json(
                200,
                json!({
                    "id": "10823456789",
                    "email": "owner@taichuy.com",
                    "name": "Taichuy Owner",
                    "picture": "https://lh3.googleusercontent.com/avatar"
                }),
            );
        }
        if let Some(forced) = self.next_gsc_status.pop_front() {
            return http_json(forced, json!({ "error": { "code": forced } }));
        }
        if url == "https://www.googleapis.com/webmasters/v3/sites" {
            if self.fail_sites_sync {
                return http_json(503, json!({ "error": { "code": 503 } }));
            }
            return http_json(
                200,
                json!({
                    "siteEntry": [
                        { "siteUrl": "https://1flowbase.taichuy.com/", "permissionLevel": "siteOwner" },
                        { "siteUrl": "sc-domain:taichuy.com", "permissionLevel": "siteFullUser" }
                    ]
                }),
            );
        }
        if url == "https://www.googleapis.com/webmasters/v3/sites/https%3A%2F%2F1flowbase.taichuy.com%2F"
            && method == "GET"
        {
            return http_json(
                200,
                json!({ "siteUrl": "https://1flowbase.taichuy.com/", "permissionLevel": "siteOwner" }),
            );
        }
        if url == "https://www.googleapis.com/webmasters/v3/sites/https%3A%2F%2F1flowbase.taichuy.com%2F/sitemaps" {
            return http_json(
                200,
                json!({
                    "sitemap": [{
                        "path": "https://1flowbase.taichuy.com/sitemap-index.xml",
                        "lastSubmitted": "2026-11-20T00:00:00.000Z",
                        "isPending": false,
                        "isSitemapsIndex": true,
                        "warnings": "0",
                        "errors": "0"
                    }]
                }),
            );
        }
        if url == "https://www.googleapis.com/webmasters/v3/sites/https%3A%2F%2F1flowbase.taichuy.com%2F/sitemaps/https%3A%2F%2F1flowbase.taichuy.com%2Fsitemap-index.xml"
            && method == "PUT"
        {
            return http_text(204, "");
        }
        if url == "https://www.googleapis.com/webmasters/v3/sites/https%3A%2F%2F1flowbase.taichuy.com%2F/searchAnalytics/query" {
            return http_json(
                200,
                json!({
                    "rows": [{ "keys": ["1flowbase"], "clicks": 42.0, "impressions": 320.0, "ctr": 0.13125, "position": 3.4 }],
                    "responseAggregationType": "byPage"
                }),
            );
        }
        if url == "https://searchconsole.googleapis.com/v1/urlInspection/index:inspect" {
            return http_json(
                200,
                json!({
                    "inspectionResult": {
                        "indexStatusResult": { "verdict": "PASS", "coverageState": "Submitted and indexed" }
                    }
                }),
            );
        }
        if url == "https://1flowbase.taichuy.com/sitemap-index.xml" {
            return http_text(
                200,
                r#"<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                  <sitemap><loc>https://1flowbase.taichuy.com/sitemap-pages.xml</loc><lastmod>2026-11-21</lastmod></sitemap>
                </sitemapindex>"#,
            );
        }
        if url == "https://1flowbase.taichuy.com/sitemap-pages.xml" {
            return http_text(
                200,
                r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                  <url><loc>https://1flowbase.taichuy.com/</loc><lastmod>2026-11-20</lastmod></url>
                  <url><loc>https://1flowbase.taichuy.com/docs</loc></url>
                </urlset>"#,
            );
        }
        http_json(404, json!({ "error": { "code": 404 } }))
    }

    fn callback(&mut self, service: MultiplexHostService, request: Value) -> Value {
        match service {
            MultiplexHostService::PluginDataV1 => self.data(request),
            MultiplexHostService::PluginCredentialV1 | MultiplexHostService::PluginEphemeralV1 => {
                let is_ephemeral = service == MultiplexHostService::PluginEphemeralV1;
                let key = request[if is_ephemeral { "key" } else { "credential_id" }]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let map = if is_ephemeral { &mut self.ephemeral } else { &mut self.secrets };
                match request["operation"].as_str().unwrap() {
                    "get" => match map.get(&key) {
                        Some(v) => json!({ "result": { "value": v } }),
                        None => json!({ "result": {} }),
                    },
                    "put" => {
                        map.insert(key, request["value"].clone());
                        json!({ "result": {} })
                    }
                    "delete" => {
                        map.remove(&key);
                        json!({ "result": {} })
                    }
                    _ => panic!("unknown secret/ephemeral op"),
                }
            }
            MultiplexHostService::PluginExecutionGateV1 => {
                match request["operation"].as_str().unwrap() {
                    "acquire" => {
                        self.gate_acquires += 1;
                        json!({ "result": { "operation": "acquire", "token": "permit-1" } })
                    }
                    "release" => {
                        self.gate_releases += 1;
                        json!({ "result": { "operation": "release" } })
                    }
                    _ => panic!("unknown gate op"),
                }
            }
            MultiplexHostService::HttpEgressV1 => self.http(request),
            _ => panic!("unexpected host service"),
        }
    }
}

fn http_json(status: u16, body: Value) -> Value {
    let encoded = STANDARD.encode(serde_json::to_vec(&body).unwrap());
    json!({ "result": { "status": status, "headers": {}, "body_base64": encoded } })
}

fn http_text(status: u16, body: &str) -> Value {
    let encoded = STANDARD.encode(body.as_bytes());
    json!({ "result": { "status": status, "headers": {}, "body_base64": encoded } })
}

async fn send_frame<W: AsyncWriteExt + Unpin>(write: &mut W, msg: MultiplexHostMessage) {
    let mut text = serde_json::to_string(&MultiplexEnvelope::new(msg)).unwrap();
    text.push('\n');
    write.write_all(text.as_bytes()).await.unwrap();
    write.flush().await.unwrap();
}

async fn call_actor(
    sim: Rc<RefCell<HostSimulator>>,
    actor_id: &str,
    handler: &str,
    path: Value,
    query: Value,
    body: Value,
) -> Value {
    let (host, worker) = tokio::io::duplex(128 * 1024);
    let (worker_read, worker_write) = tokio::io::split(worker);
    let runtime = Runtime::default();
    let runner = tokio::task::spawn_local(serve_duplex_io(
        worker_read,
        worker_write,
        move |req, emitter, _| {
            let rt = runtime.clone();
            async move { rt.dispatch(req, emitter).await }
        },
    ));

    let (read, mut write) = tokio::io::split(host);
    let mut lines = BufReader::new(read).lines();
    let deadline = crate::model::now_unix_ms() + 30_000;
    send_frame(
        &mut write,
        MultiplexHostMessage::Call {
            call_id: "1".into(),
            request: json!({
                "method": "execute",
                "input": {
                    "handler": handler,
                    "call_context": {
                        "actor_id": actor_id,
                        "workspace_id": "ws-1",
                        "deadline_unix_ms": deadline,
                    },
                    "input_payload": { "path": path, "query": query, "body": body }
                }
            }),
        },
    )
    .await;

    let final_response = loop {
        let line = lines.next_line().await.unwrap().unwrap();
        let env: MultiplexEnvelope<MultiplexWorkerMessage> = serde_json::from_str(&line).unwrap();
        match env.message {
            MultiplexWorkerMessage::Callback {
                call_id,
                callback_id,
                service,
                request,
            } => {
                let response = sim.borrow_mut().callback(service, request);
                send_frame(
                    &mut write,
                    MultiplexHostMessage::CallbackResult {
                        call_id,
                        callback_id,
                        response,
                    },
                )
                .await;
            }
            MultiplexWorkerMessage::Response { response, .. } => {
                break response;
            }
            _ => {}
        }
    };
    write.shutdown().await.unwrap();
    drop(lines);
    runner.await.unwrap().unwrap();
    final_response
}

async fn call_ok(
    sim: Rc<RefCell<HostSimulator>>,
    handler: &str,
    path: Value,
    query: Value,
    body: Value,
) -> Value {
    call_actor(sim, "actor-1", handler, path, query, body).await
}

#[tokio::test(flavor = "current_thread")]
async fn end_to_end_oauth_sites_analytics_sitemap_and_refresh_flow() {
    let local = tokio::task::LocalSet::new();
    local.run_until(async move {
    let sim = Rc::new(RefCell::new(HostSimulator::default()));

    // 1. Unconfigured OAuth state
    let cfg = call_ok(sim.clone(), "oauth_config.get", json!({}), json!({}), json!({})).await;
    assert_eq!(cfg["result"]["configured"], false);
    let err = call_ok(
        sim.clone(),
        "oauth.start",
        json!({}),
        json!({}),
        json!({ "redirect_uri": "https://1flowbase.taichuy.cn/settings/google-search-console" }),
    )
    .await;
    assert_eq!(err["error"]["message"], "oauth_not_configured");

    // 2. Save OAuth config (requires secret initially, preserves secret on subsequent edit)
    let missing_sec = call_ok(
        sim.clone(),
        "oauth_config.save",
        json!({}),
        json!({}),
        json!({ "client_id": "cid-1.apps.googleusercontent.com", "default_scope_mode": "readonly" }),
    )
    .await;
    assert_eq!(missing_sec["error"]["message"], "client_secret_required");

    let saved = call_ok(
        sim.clone(),
        "oauth_config.save",
        json!({}),
        json!({}),
        json!({
            "client_id": "cid-1.apps.googleusercontent.com",
            "client_secret": "gsec-top-secret",
            "default_scope_mode": "readonly"
        }),
    )
    .await;
    assert_eq!(saved["ok"], true);
    assert_eq!(saved["result"]["configured"], true);
    assert_eq!(saved["result"]["has_client_secret"], true);
    assert!(!saved.to_string().contains("gsec-top-secret"));

    // 3. Start OAuth PKCE flow
    let started = call_ok(
        sim.clone(),
        "oauth.start",
        json!({}),
        json!({}),
        json!({ "redirect_uri": "https://1flowbase.taichuy.cn/settings/google-search-console" }),
    )
    .await;
    assert_eq!(started["ok"], true);
    let auth_url = url::Url::parse(started["result"]["authorization_url"].as_str().unwrap()).unwrap();
    let state = auth_url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    assert!(auth_url.query_pairs().any(|(k, v)| k == "code_challenge_method" && v == "S256"));

    // Actor mismatch cannot consume another user's state (note: consumes state on check or rejects; wait - check order!)
    let wrong_actor = call_actor(
        sim.clone(),
        "actor-other",
        "oauth.complete",
        json!({}),
        json!({}),
        json!({ "code": "code-ro", "state": state }),
    )
    .await;
    assert_eq!(wrong_actor["error"]["message"], "oauth_actor_mismatch");

    // Start fresh OAuth flow and complete as actor-1
    let started2 = call_ok(
        sim.clone(),
        "oauth.start",
        json!({}),
        json!({}),
        json!({ "redirect_uri": "https://1flowbase.taichuy.cn/settings/google-search-console" }),
    )
    .await;
    let state2 = url::Url::parse(started2["result"]["authorization_url"].as_str().unwrap())
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let completed = call_ok(
        sim.clone(),
        "oauth.complete",
        json!({}),
        json!({}),
        json!({ "code": "code-ro", "state": state2 }),
    )
    .await;
    assert_eq!(completed["ok"], true);
    assert_eq!(completed["result"]["site_count"], 2);
    assert!(!completed.to_string().contains("1//refresh-secret-xyz"));
    assert!(!completed.to_string().contains("ya29.tok-"));
    let account_id = completed["result"]["account"]["account_id"].as_str().unwrap().to_owned();

    // Replay same state must fail
    let replay = call_ok(
        sim.clone(),
        "oauth.complete",
        json!({}),
        json!({}),
        json!({ "code": "code-ro", "state": state2 }),
    )
    .await;
    assert_eq!(replay["error"]["message"], "oauth_state_expired");

    // 4. Readonly scope blocks sitemap submit
    let denied_write = call_ok(
        sim.clone(),
        "sitemaps.submit",
        json!({ "account_id": account_id }),
        json!({}),
        json!({ "site_url": "https://1flowbase.taichuy.com/", "feedpath": "https://1flowbase.taichuy.com/sitemap-index.xml" }),
    )
    .await;
    assert_eq!(denied_write["error"]["message"], "scope_upgrade_required");

    // 5. Sites list, overview, analytics query, inspection, and sitemap XML entries
    let sites = call_ok(sim.clone(), "sites.list", json!({}), json!({}), json!({})).await;
    assert_eq!(sites["result"]["total"], 2);

    let sa = call_ok(
        sim.clone(),
        "analytics.query",
        json!({ "account_id": account_id }),
        json!({}),
        json!({
            "site_url": "https://1flowbase.taichuy.com/",
            "start_date": "2026-11-01",
            "end_date": "2026-11-20",
            "dimensions": ["query"],
            "row_limit": 10
        }),
    )
    .await;
    assert_eq!(sa["ok"], true);
    assert_eq!(sa["result"]["rows"][0]["clicks"], 42.0);

    let overview = call_ok(
        sim.clone(),
        "sites.overview",
        json!({ "account_id": account_id }),
        json!({}),
        json!({ "site_url": "https://1flowbase.taichuy.com/", "days": 28 }),
    )
    .await;
    assert_eq!(overview["ok"], true);
    assert_eq!(overview["result"]["totals"]["clicks"], 42.0);
    assert_eq!(overview["result"]["sitemaps"].as_array().unwrap().len(), 1);

    let inspect = call_ok(
        sim.clone(),
        "inspection.inspect",
        json!({ "account_id": account_id }),
        json!({}),
        json!({
            "site_url": "https://1flowbase.taichuy.com/",
            "inspection_url": "https://1flowbase.taichuy.com/docs"
        }),
    )
    .await;
    assert_eq!(inspect["ok"], true);
    assert_eq!(inspect["result"]["inspection_result"]["indexStatusResult"]["verdict"], "PASS");

    let entries = call_ok(
        sim.clone(),
        "sitemaps.fetch_entries",
        json!({ "account_id": account_id }),
        json!({}),
        json!({
            "site_url": "https://1flowbase.taichuy.com/",
            "feedpath": "https://1flowbase.taichuy.com/sitemap-index.xml",
            "follow_index": true
        }),
    )
    .await;
    assert_eq!(entries["ok"], true);
    assert_eq!(entries["result"]["kind"], "sitemapindex");
    assert_eq!(entries["result"]["returned_count"], 2);

    // 6. Simulate expired access token (401 -> single-flight ExecutionGate refresh & retry)
    sim.borrow_mut().next_gsc_status.push_back(401);
    let retried = call_ok(
        sim.clone(),
        "sitemaps.list",
        json!({ "account_id": account_id }),
        json!({ "site_url": "https://1flowbase.taichuy.com/" }),
        json!({}),
    )
    .await;
    assert_eq!(retried["ok"], true);
    assert_eq!(sim.borrow().gate_acquires, 1);
    assert_eq!(sim.borrow().gate_releases, 1);

    // 7. Upgrade scope via reauth and test full write permission
    let reauth = call_ok(
        sim.clone(),
        "oauth.start",
        json!({}),
        json!({}),
        json!({
            "redirect_uri": "https://1flowbase.taichuy.cn/settings/google-search-console",
            "scope_mode": "full",
            "reauth_account_id": account_id
        }),
    )
    .await;
    let reauth_state = url::Url::parse(reauth["result"]["authorization_url"].as_str().unwrap())
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let upgraded = call_ok(
        sim.clone(),
        "oauth.complete",
        json!({}),
        json!({}),
        json!({ "code": "code-full", "state": reauth_state }),
    )
    .await;
    assert_eq!(upgraded["result"]["account"]["scope_mode"], "full");
    let submit = call_ok(
        sim.clone(),
        "sitemaps.submit",
        json!({ "account_id": account_id }),
        json!({}),
        json!({ "site_url": "https://1flowbase.taichuy.com/", "feedpath": "https://1flowbase.taichuy.com/sitemap-index.xml" }),
    )
    .await;
    assert_eq!(submit["ok"], true);
    assert_eq!(submit["result"]["submitted"], true);

    // 8. Revoked refresh token marks account reauth_required
    sim.borrow_mut().next_gsc_status.push_back(401);
    sim.borrow_mut().revoke_refresh = true;
    let revoked = call_ok(
        sim.clone(),
        "sitemaps.list",
        json!({ "account_id": account_id }),
        json!({ "site_url": "https://1flowbase.taichuy.com/" }),
        json!({}),
    )
    .await;
    assert_eq!(revoked["error"]["message"], "google_reauth_required");
    let acct = call_ok(sim.clone(), "accounts.get", json!({ "account_id": account_id }), json!({}), json!({})).await;
    assert_eq!(acct["result"]["account"]["status"], "reauth_required");

    // 9. Delete cleans up account, sites, credential, and cache
    let del = call_ok(sim.clone(), "accounts.delete", json!({ "account_id": account_id }), json!({}), json!({})).await;
    assert_eq!(del["ok"], true);
    let list_after = call_ok(sim.clone(), "accounts.list", json!({}), json!({}), json!({})).await;
    assert_eq!(list_after["result"]["total"], 0);
    }).await;
}
