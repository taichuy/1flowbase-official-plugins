use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::{
    serve_duplex_io, MultiplexEnvelope, MultiplexHostMessage, MultiplexHostService,
    MultiplexWorkerMessage, PluginDataOperation, PluginDataOperationResult, PluginDataRequest,
    PluginDataRow, PluginDataValue,
};
use std::collections::{BTreeMap, VecDeque};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Default)]
struct HostSimulator {
    rows: BTreeMap<String, BTreeMap<String, PluginDataValue>>,
    secrets: BTreeMap<String, Value>,
    ephemeral: BTreeMap<String, Value>,
    login_count: usize,
    verify_count: usize,
    upstream_count: usize,
    statuses: VecDeque<u16>,
    verify_status: u16,
    two_factor: bool,
    fail_mutation: bool,
    network_failure: bool,
    text_response: bool,
    observed_urls: Vec<String>,
}
impl HostSimulator {
    fn data(&mut self, request: Value) -> Value {
        let request: PluginDataRequest = serde_json::from_value(request).unwrap();
        request.validate().unwrap();
        let mut results = vec![];
        for operation in request.operations {
            let result = match operation {
                PluginDataOperation::Find { page, .. } => PluginDataOperationResult::Rows {
                    rows: self
                        .rows
                        .values()
                        .skip(page.offset as usize)
                        .take(page.limit as usize)
                        .cloned()
                        .map(|values| PluginDataRow { values })
                        .collect(),
                },
                PluginDataOperation::FindOne { filters, .. } => {
                    let Some(PluginDataValue::String(id)) = filters[0].value.as_ref() else {
                        panic!("id filter")
                    };
                    PluginDataOperationResult::OptionalRow {
                        row: self
                            .rows
                            .get(id)
                            .cloned()
                            .map(|values| PluginDataRow { values }),
                    }
                }
                PluginDataOperation::Insert { values, .. } => {
                    if self.fail_mutation {
                        return json!({"error":{"code":"fixture_write_error"}});
                    }
                    let PluginDataValue::String(id) = &values["host_id"] else {
                        panic!("id")
                    };
                    self.rows.insert(id.clone(), values);
                    PluginDataOperationResult::Mutation { affected: 1 }
                }
                PluginDataOperation::Update {
                    values, filters, ..
                } => {
                    if self.fail_mutation {
                        return json!({"error":{"code":"fixture_write_error"}});
                    }
                    let Some(PluginDataValue::String(id)) = filters[0].value.as_ref() else {
                        panic!("id filter")
                    };
                    self.rows.insert(id.clone(), values);
                    PluginDataOperationResult::Mutation { affected: 1 }
                }
                PluginDataOperation::Delete { filters, .. } => {
                    let Some(PluginDataValue::String(id)) = filters[0].value.as_ref() else {
                        panic!("id filter")
                    };
                    let affected = u64::from(self.rows.remove(id).is_some());
                    PluginDataOperationResult::Mutation { affected }
                }
                _ => panic!("unexpected data operation"),
            };
            results.push(result);
        }
        json!({"result":{"results":results,"replayed":false}})
    }
    fn callback(&mut self, service: MultiplexHostService, request: Value) -> Value {
        match service {
            MultiplexHostService::PluginDataV1 => self.data(request),
            MultiplexHostService::PluginCredentialV1 | MultiplexHostService::PluginEphemeralV1 => {
                let ephemeral = service == MultiplexHostService::PluginEphemeralV1;
                let key = request[if ephemeral { "key" } else { "credential_id" }]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let values = if ephemeral {
                    &mut self.ephemeral
                } else {
                    &mut self.secrets
                };
                match request["operation"].as_str().unwrap() {
                    "get" => match values.get(&key) {
                        Some(value) => json!({"result":{"value":value}}),
                        None => json!({"result":{}}),
                    },
                    "put" => {
                        if ephemeral {
                            assert_eq!(request["ttl_ms"], 3_600_000);
                        }
                        values.insert(key, request["value"].clone());
                        json!({"result":{}})
                    }
                    "delete" => {
                        values.remove(&key);
                        json!({"result":{}})
                    }
                    _ => panic!("unexpected credential operation"),
                }
            }
            MultiplexHostService::HttpEgressV1 => {
                let url = request["url"].as_str().unwrap();
                let origin = url::Url::parse(url).unwrap().origin().ascii_serialization();
                assert_eq!(request["headers"]["user-agent"], "1flowbase-umami/0.1.0");
                assert_eq!(request["headers"]["origin"], origin);
                assert_eq!(request["headers"]["referer"], format!("{origin}/login"));
                if matches!(
                    request["method"].as_str().unwrap(),
                    "GET" | "HEAD" | "DELETE"
                ) {
                    assert!(request["body_base64"].is_null());
                    assert!(request["headers"].get("content-type").is_none());
                }
                self.observed_urls.push(url.to_owned());
                if self.network_failure {
                    self.upstream_count += 1;
                    return json!({"error":{"code":"fixture_network_error"}});
                }
                if self.text_response {
                    return json!({"result":{"status":200,"headers":{"content-type":"application/javascript"},"body_base64":STANDARD.encode("function umami() {}")}});
                }
                if url.ends_with("/api/auth/login") {
                    self.login_count += 1;
                    let bytes = STANDARD
                        .decode(request["body_base64"].as_str().unwrap())
                        .unwrap();
                    let body: Value = serde_json::from_slice(&bytes).unwrap();
                    if body["password"] != "secret-password" {
                        return http(401, json!({"error":{"code":"incorrect-username-password"}}));
                    }
                    if self.two_factor {
                        return http(
                            200,
                            json!({"requiresTwoFactor":true,"partialToken":"private-partial"}),
                        );
                    }
                    http(
                        200,
                        json!({"token":format!("private-token-{}",self.login_count),"user":{"id":"u1","username":"admin"}}),
                    )
                } else if url.ends_with("/api/auth/verify") {
                    self.verify_count += 1;
                    assert!(request["headers"]["authorization"]
                        .as_str()
                        .unwrap()
                        .starts_with("Bearer private-token-"));
                    http(
                        if self.verify_status == 0 {
                            401
                        } else {
                            self.verify_status
                        },
                        json!({"id":"u1"}),
                    )
                } else {
                    self.upstream_count += 1;
                    http(
                        self.statuses.pop_front().unwrap_or(200),
                        json!({"items":[{"properties":{"token":"event-property"}}],"count":7}),
                    )
                }
            }
            _ => panic!("unexpected host service"),
        }
    }
}
fn http(status: u16, body: Value) -> Value {
    json!({"result":{"status":status,"headers":{"content-type":"application/json"},"body_base64":STANDARD.encode(serde_json::to_vec(&body).unwrap())}})
}
fn body() -> Value {
    json!({"name":"Analytics","base_url":"https://analytics.example/base/","username":"admin","enabled":true,"password":"secret-password"})
}
fn invocation(handler: &str, path: Value, query: Value, body: Value) -> Value {
    json!({"method":"execute","input":{"handler":handler,"input_payload":{"path":path,"query":query,"body":body}}})
}
fn upstream_handler(method: &str, path: &str) -> String {
    let operations: Vec<Value> =
        serde_json::from_str(include_str!("../../_assets/api/operations.json")).unwrap();
    let op = operations
        .iter()
        .find(|op| op["method"] == method && op["path"] == path)
        .unwrap();
    format!("upstream.{}", op["operation_id"].as_str().unwrap())
}
async fn call(simulator: Rc<RefCell<HostSimulator>>, request: Value) -> Value {
    // Every call starts a fresh worker, proving token cache ownership is the Host.
    let (host, worker) = tokio::io::duplex(128 * 1024);
    let (worker_read, worker_write) = tokio::io::split(worker);
    let runtime = Runtime::default();
    let runner = tokio::task::spawn_local(serve_duplex_io(
        worker_read,
        worker_write,
        move |request, emitter, _| {
            let runtime = runtime.clone();
            async move { runtime.dispatch(request, emitter).await }
        },
    ));
    let (read, mut write) = tokio::io::split(host);
    let mut reader = BufReader::new(read);
    send(
        &mut write,
        MultiplexHostMessage::Call {
            call_id: "1".into(),
            request,
        },
    )
    .await;
    let result = loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).await.unwrap() > 0);
        let frame: MultiplexEnvelope<MultiplexWorkerMessage> = serde_json::from_str(&line).unwrap();
        match frame.message {
            MultiplexWorkerMessage::Callback {
                call_id,
                callback_id,
                service,
                request,
            } => {
                let response = simulator.borrow_mut().callback(service, request);
                send(
                    &mut write,
                    MultiplexHostMessage::CallbackResult {
                        call_id,
                        callback_id,
                        response,
                    },
                )
                .await;
            }
            MultiplexWorkerMessage::Response { response, .. } => break response,
            _ => panic!("unexpected worker frame"),
        }
    };
    write.shutdown().await.unwrap();
    runner.await.unwrap().unwrap();
    assert_eq!(result["ok"], true);
    let public = serde_json::to_string(&result).unwrap();
    assert!(!public.contains("secret-password"));
    assert!(!public.contains("private-token"));
    assert!(!public.contains("private-partial"));
    assert!(!public.contains("credential_id"));
    result["result"].clone()
}
async fn send<W: tokio::io::AsyncWrite + Unpin>(write: &mut W, message: MultiplexHostMessage) {
    let mut bytes = serde_json::to_vec(&MultiplexEnvelope::new(message)).unwrap();
    bytes.push(b'\n');
    write.write_all(&bytes).await.unwrap();
}
async fn create(sim: Rc<RefCell<HostSimulator>>) -> String {
    let result = call(
        sim,
        invocation("hosts.create", json!({}), json!({}), body()),
    )
    .await;
    assert_eq!(result["success"], true);
    result["host"]["host_id"].as_str().unwrap().into()
}
async fn get_websites(sim: Rc<RefCell<HostSimulator>>, id: &str) -> Value {
    call(
        sim,
        invocation(
            &upstream_handler("GET", "/api/websites"),
            json!({"host_id":id}),
            json!({}),
            json!({}),
        ),
    )
    .await
}

#[tokio::test]
async fn model_crud_cache_restart_clear_and_secret_isolation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let sim = Rc::new(RefCell::new(HostSimulator::default()));
            let id = create(sim.clone()).await;
            assert_eq!(sim.borrow().login_count, 1);
            assert_eq!(sim.borrow().secrets.len(), 1);
            let result = get_websites(sim.clone(), &id).await;
            assert_eq!(result["data"]["count"], 7);
            assert_eq!(
                result["data"]["items"][0]["properties"]["token"],
                "event-property"
            );
            assert_eq!(sim.borrow().login_count, 1);
            sim.borrow_mut().ephemeral.clear();
            get_websites(sim.clone(), &id).await;
            assert_eq!(sim.borrow().login_count, 2);
            let result = call(
                sim.clone(),
                invocation(
                    "hosts.list",
                    json!({}),
                    json!({"page":"1","search":"Analytics"}),
                    Value::Null,
                ),
            )
            .await;
            assert_eq!(result["total"], 1);
            assert_eq!(result["items"][0]["has_password"], true);
            let mut update = body();
            update["name"] = json!("Renamed");
            update.as_object_mut().unwrap().remove("password");
            call(
                sim.clone(),
                invocation("hosts.update", json!({"host_id":id}), Value::Null, update),
            )
            .await;
            assert_eq!(sim.borrow().login_count, 2);
            let result = call(
                sim.clone(),
                invocation(
                    "hosts.delete",
                    json!({"host_id":id}),
                    Value::Null,
                    Value::Null,
                ),
            )
            .await;
            assert_eq!(result["success"], true);
            assert!(sim.borrow().secrets.is_empty());
            assert!(sim.borrow().ephemeral.is_empty());
            assert!(sim.borrow().rows.is_empty());
        })
        .await;
}
#[tokio::test]
async fn invalid_session_reauthenticates_once_and_valid_session_denies_without_replay() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let sim = Rc::new(RefCell::new(HostSimulator::default()));
            let id = create(sim.clone()).await;
            sim.borrow_mut().statuses = VecDeque::from([401, 200]);
            assert_eq!(get_websites(sim.clone(), &id).await["success"], true);
            assert_eq!(
                (
                    sim.borrow().login_count,
                    sim.borrow().verify_count,
                    sim.borrow().upstream_count
                ),
                (2, 1, 2)
            );
            sim.borrow_mut().statuses = VecDeque::from([401]);
            sim.borrow_mut().verify_status = 200;
            assert_eq!(
                get_websites(sim.clone(), &id).await["error"]["code"],
                "umami_permission_denied"
            );
            assert_eq!(
                (sim.borrow().login_count, sim.borrow().upstream_count),
                (2, 3)
            );
            sim.borrow_mut().statuses = VecDeque::from([403]);
            assert_eq!(get_websites(sim.clone(), &id).await["status"], 403);
            assert_eq!(sim.borrow().verify_count, 2);
            sim.borrow_mut().statuses = VecDeque::from([500]);
            get_websites(sim.clone(), &id).await;
            assert_eq!(sim.borrow().login_count, 2);
            sim.borrow_mut().statuses = VecDeque::from([401, 401]);
            sim.borrow_mut().verify_status = 401;
            assert_eq!(get_websites(sim.clone(), &id).await["status"], 401);
            assert_eq!(sim.borrow().login_count, 3);
        })
        .await;
}
#[tokio::test]
async fn wrong_password_two_factor_disabled_and_update_rollback() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let sim = Rc::new(RefCell::new(HostSimulator::default()));
            let mut wrong = body();
            wrong["password"] = json!("wrong");
            let result = call(
                sim.clone(),
                invocation("hosts.create", json!({}), Value::Null, wrong),
            )
            .await;
            assert_eq!(result["error"]["code"], "umami_authentication_failed");
            assert!(sim.borrow().rows.is_empty());
            assert!(sim.borrow().secrets.is_empty());
            sim.borrow_mut().two_factor = true;
            let result = call(
                sim.clone(),
                invocation("hosts.create", json!({}), Value::Null, body()),
            )
            .await;
            assert_eq!(result["error"]["code"], "umami_two_factor_unsupported");
            assert!(sim.borrow().ephemeral.is_empty());
            sim.borrow_mut().two_factor = false;
            let id = create(sim.clone()).await;
            let old_secrets = sim.borrow().secrets.clone();
            let old_rows = sim.borrow().rows.clone();
            sim.borrow_mut().fail_mutation = true;
            let mut update = body();
            update["username"] = json!("other");
            let result = call(
                sim.clone(),
                invocation("hosts.update", json!({"host_id":id}), Value::Null, update),
            )
            .await;
            assert_eq!(result["error"]["code"], "umami_storage_error");
            assert_eq!(sim.borrow().secrets, old_secrets);
            assert_eq!(sim.borrow().rows, old_rows);
            sim.borrow_mut().fail_mutation = false;
            let mut update = body();
            update["enabled"] = json!(false);
            update.as_object_mut().unwrap().remove("password");
            call(
                sim.clone(),
                invocation(
                    "hosts.update",
                    json!({"host_id":id}),
                    Value::Null,
                    update.clone(),
                ),
            )
            .await;
            let count = sim.borrow().login_count;
            assert_eq!(
                get_websites(sim.clone(), &id).await["error"]["code"],
                "host_disabled"
            );
            assert_eq!(
                call(
                    sim.clone(),
                    invocation(
                        "hosts.test",
                        json!({"host_id":id}),
                        Value::Null,
                        Value::Null
                    )
                )
                .await["error"]["code"],
                "host_disabled"
            );
            update["host_id"] = json!(id);
            assert_eq!(
                call(
                    sim.clone(),
                    invocation("hosts.test_draft", Value::Null, Value::Null, update)
                )
                .await["authenticated"],
                true
            );
            assert_eq!(sim.borrow().login_count, count + 1);
        })
        .await;
}
#[test]
fn path_encoding_query_structure_and_base_url_constraints() {
    use crate::remote::{build_url, normalize_base};
    let base = normalize_base("https://analytics.example/base/").unwrap();
    assert_eq!(base, "https://analytics.example/base");
    let url = build_url(
        &base,
        "/api/websites/{websiteId}",
        &json!({"websiteId":"a/b?x=1#fragment"}),
        &json!({"filters":[{"name":"path","value":"a&b"}],"startAt":1,"null":null}),
    )
    .unwrap();
    assert_eq!(url.path(), "/base/api/websites/a%2Fb%3Fx=1%23fragment");
    assert_eq!(
        url.query_pairs()
            .find(|(key, _)| key == "filters")
            .unwrap()
            .1,
        "[{\"name\":\"path\",\"value\":\"a&b\"}]"
    );
    assert!(build_url(
        &base,
        "/api/websites/{websiteId}",
        &json!({"websiteId":".."}),
        &Value::Null
    )
    .is_err());
    for invalid in [
        "file:///tmp/data",
        "https://user:password@example.com",
        "https://example.com?token=secret",
        "https://example.com#fragment",
    ] {
        assert!(normalize_base(invalid).is_err());
    }
}

#[tokio::test]
async fn auth_none_text_payload_and_uncertain_network_do_not_replay() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let sim = Rc::new(RefCell::new(HostSimulator::default()));
            let id = create(sim.clone()).await;
            // A no-auth inventory route remains callable even with no saved password/token.
            sim.borrow_mut().ephemeral.clear();
            sim.borrow_mut().secrets.clear();
            let result = call(
                sim.clone(),
                invocation(
                    &upstream_handler("GET", "/api/heartbeat"),
                    json!({"host_id":id}),
                    Value::Null,
                    Value::Null,
                ),
            )
            .await;
            assert_eq!(result["success"], true);
            assert_eq!(sim.borrow().login_count, 1);
            sim.borrow_mut().text_response = true;
            let result = call(
                sim.clone(),
                invocation(
                    &upstream_handler("GET", "/api/heartbeat"),
                    json!({"host_id":id}),
                    Value::Null,
                    Value::Null,
                ),
            )
            .await;
            assert_eq!(result["data"], "function umami() {}");
            assert_eq!(result["content_type"], "application/javascript");
            sim.borrow_mut().text_response = false;
            sim.borrow_mut().network_failure = true;
            let before = sim.borrow().upstream_count;
            let result = call(
                sim.clone(),
                invocation(
                    &upstream_handler("POST", "/api/send"),
                    json!({"host_id":id}),
                    Value::Null,
                    json!({"type":"event","payload":{}}),
                ),
            )
            .await;
            assert_eq!(result["error"]["code"], "umami_network_error");
            assert_eq!(sim.borrow().upstream_count, before + 1);
            assert_eq!(sim.borrow().login_count, 1);
        })
        .await;
}

#[tokio::test]
async fn concurrent_same_host_calls_singleflight_login_and_explicit_login_hides_token() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let sim = Rc::new(RefCell::new(HostSimulator::default()));
            let id = create(sim.clone()).await;
            sim.borrow_mut().ephemeral.clear();
            let (host, worker) = tokio::io::duplex(128 * 1024);
            let (worker_read, worker_write) = tokio::io::split(worker);
            let runtime = Runtime::default();
            let runner = tokio::task::spawn_local(serve_duplex_io(
                worker_read,
                worker_write,
                move |request, emitter, _| {
                    let runtime = runtime.clone();
                    async move { runtime.dispatch(request, emitter).await }
                },
            ));
            let (read, mut write) = tokio::io::split(host);
            let mut reader = BufReader::new(read);
            for call_id in ["1", "2"] {
                send(
                    &mut write,
                    MultiplexHostMessage::Call {
                        call_id: call_id.into(),
                        request: invocation(
                            &upstream_handler("GET", "/api/websites"),
                            json!({"host_id":id}),
                            Value::Null,
                            Value::Null,
                        ),
                    },
                )
                .await;
            }
            let mut complete = 0;
            while complete < 2 {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).await.unwrap() > 0);
                let frame: MultiplexEnvelope<MultiplexWorkerMessage> =
                    serde_json::from_str(&line).unwrap();
                match frame.message {
                    MultiplexWorkerMessage::Callback {
                        call_id,
                        callback_id,
                        service,
                        request,
                    } => {
                        let response = sim.borrow_mut().callback(service, request);
                        send(
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
                        assert_eq!(response["result"]["success"], true);
                        complete += 1;
                    }
                    _ => panic!("unexpected worker frame"),
                }
            }
            write.shutdown().await.unwrap();
            runner.await.unwrap().unwrap();
            assert_eq!(sim.borrow().login_count, 2);
            assert_eq!(sim.borrow().upstream_count, 2);
            let result = call(
                sim.clone(),
                invocation(
                    &upstream_handler("POST", "/api/auth/login"),
                    json!({"host_id":id}),
                    Value::Null,
                    json!({"password":"attacker-input"}),
                ),
            )
            .await;
            assert_eq!(result["data"]["authenticated"], true);
            assert!(result["data"].get("token").is_none());
        })
        .await;
}

#[tokio::test]
async fn egress_headers_follow_saved_origin_with_scheme_port_and_base_path() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for base_url in [
                "http://analytics.example:8081/nested/base/",
                "https://analytics.example:8443/nested/",
            ] {
                let sim = Rc::new(RefCell::new(HostSimulator::default()));
                let mut draft = body();
                draft["base_url"] = json!(base_url);
                let created = call(
                    sim.clone(),
                    invocation("hosts.create", json!({}), json!({}), draft),
                )
                .await;
                assert_eq!(created["success"], true);
                let id = created["host"]["host_id"].as_str().unwrap();
                let result = call(sim.clone(), invocation(
                &upstream_handler("GET", "/api/websites"), json!({"host_id":id}), json!({}),
                json!({"headers":{"origin":"https://caller.invalid","user-agent":"caller-agent"}}),
            )).await;
                assert_eq!(result["success"], true);
                assert_eq!(
                    sim.borrow().observed_urls.last().unwrap(),
                    &format!("{}api/websites", base_url)
                );
            }
        })
        .await;
}
