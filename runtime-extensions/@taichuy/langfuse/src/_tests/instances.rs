use crate::workflow_tests::{invoke, invoke_payload};
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::MultiplexHostService;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const A: &str = "11111111-1111-4111-8111-111111111111";
const B: &str = "22222222-2222-4222-8222-222222222222";
fn row(id: Option<&str>, name: &str, url: &str) -> Value {
    json!({"values":{"instance_id":id.map(|id|json!({"type":"string","value":id})).unwrap_or(json!({"type":"null"})),
        "name":{"type":"string","value":name},"base_url":{"type":"string","value":url}}})
}
fn pair(name: &str) -> Value {
    json!({"public_key":format!("public-{name}"),"secret_key":format!("secret-{name}")})
}
fn key(id: &str) -> String {
    if id == "default" {
        "langfuse_project".into()
    } else {
        format!("langfuse_instance:{id}")
    }
}
#[derive(Default)]
struct Fixture {
    rows: Vec<Value>,
    credentials: BTreeMap<String, Value>,
    data_writes: usize,
    http: Vec<Value>,
    fail: Option<(String, bool)>,
    fail_cleanup: bool,
}
impl Fixture {
    fn seeded() -> Self {
        Self {
            rows: vec![
                row(Some(A), "same", "https://one.example/prefix/"),
                row(Some(B), "same", "https://two.example/"),
            ],
            credentials: BTreeMap::from([(key(A), pair("a")), (key(B), pair("b"))]),
            ..Self::default()
        }
    }
    fn callback(&mut self, service: MultiplexHostService, request: Value) -> Value {
        match service {
            MultiplexHostService::PluginDataV1 => {
                assert!(!request.to_string().contains("secret-"));
                let operation = &request["operations"][0];
                let op = operation["operation"].as_str().unwrap();
                let fail = self.fail.as_ref().is_some_and(|(name, _)| name == op);
                let after = fail && self.fail.as_ref().unwrap().1;
                if fail {
                    self.fail = None;
                }
                if fail && !after {
                    return json!({"error":{"code":"fixture_data_failure"}});
                }
                let matches = |row: &Value| {
                    operation["filters"].as_array().is_none_or(|filters| {
                        filters.iter().all(|filter| {
                            let cell = &row["values"][filter["field"].as_str().unwrap()];
                            match filter["operator"].as_str().unwrap() {
                                "equal" => cell == &filter["value"],
                                "is_null" => cell.is_null() || cell["type"] == "null",
                                other => panic!("unexpected filter {other}"),
                            }
                        })
                    })
                };
                let result = match op {
                    "find_one" => {
                        json!({"result":"optional_row","row":self.rows.iter().find(|row|matches(row))})
                    }
                    "count" => {
                        json!({"result":"count","count":self.rows.iter().filter(|row|matches(row)).count()})
                    }
                    "find" => {
                        let mut rows = self
                            .rows
                            .iter()
                            .filter(|row| matches(row))
                            .cloned()
                            .collect::<Vec<_>>();
                        rows.sort_by_key(|row| {
                            (
                                row["values"]["instance_id"]["value"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_owned(),
                                row["values"]["name"]["value"].as_str().unwrap().to_owned(),
                            )
                        });
                        let offset = operation["page"]["offset"].as_u64().unwrap() as usize;
                        let limit = operation["page"]["limit"].as_u64().unwrap() as usize;
                        assert!(limit <= 100);
                        json!({"result":"rows","rows":rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>()})
                    }
                    "insert" => {
                        self.data_writes += 1;
                        self.rows.push(json!({"values":operation["values"]}));
                        json!({"result":"mutation","affected":1})
                    }
                    "update" => {
                        self.data_writes += 1;
                        let mut affected = 0;
                        for row in &mut self.rows {
                            if matches(row) {
                                affected += 1;
                                for (key, value) in operation["values"].as_object().unwrap() {
                                    row["values"][key] = value.clone();
                                }
                            }
                        }
                        json!({"result":"mutation","affected":affected})
                    }
                    "delete" => {
                        self.data_writes += 1;
                        let before = self.rows.len();
                        self.rows.retain(|row| !matches(row));
                        json!({"result":"mutation","affected":before-self.rows.len()})
                    }
                    other => panic!("unexpected data operation {other}"),
                };
                if after {
                    json!({"error":{"code":"fixture_data_failure"}})
                } else {
                    json!({"result":{"results":[result],"replayed":false}})
                }
            }
            MultiplexHostService::PluginCredentialV1 => {
                let id = request["credential_id"].as_str().unwrap().to_owned();
                let op = request["operation"].as_str().unwrap();
                let fail = self
                    .fail
                    .as_ref()
                    .is_some_and(|(name, _)| name == &format!("credential_{op}"));
                let after = fail && self.fail.as_ref().unwrap().1;
                if fail {
                    self.fail = None;
                }
                if (fail && !after) || (self.fail_cleanup && op == "delete") {
                    return json!({"error":{"code":"fixture_credential_failure"}});
                }
                let value = match op {
                    "get" => self.credentials.get(&id).cloned(),
                    "put" => {
                        self.credentials.insert(id, request["value"].clone());
                        None
                    }
                    "delete" => {
                        self.credentials.remove(&id);
                        None
                    }
                    _ => panic!("unexpected credential operation"),
                };
                if after {
                    json!({"error":{"code":"fixture_credential_failure"}})
                } else {
                    json!({"result":{"value":value}})
                }
            }
            MultiplexHostService::HttpEgressV1 => {
                self.http.push(request);
                json!({"result":{"status":200,"headers":{"content-type":"application/json"},"body_base64":STANDARD.encode(br#"{"data":[]}"#)}})
            }
            _ => panic!("unexpected service {service:?}"),
        }
    }
    async fn run(&mut self, handler: &str, id: &str, body: Value) -> Value {
        invoke_payload(
            handler,
            json!({"path":{"instance_id":id},"query":{},"body":body}),
            |service, request| self.callback(service, request),
        )
        .await
    }
    async fn list(&mut self, query: Value) -> Value {
        invoke_payload(
            "instances_list",
            json!({"path":{},"query":query,"body":{}}),
            |service, request| self.callback(service, request),
        )
        .await
    }
}
fn input(name: &str) -> Value {
    json!({"name":name,"base_url":"https://shared.example/prefix","public_key":"new-public","secret_key":"new-secret"})
}

#[tokio::test]
async fn independent_create_rename_reload_test_and_delete_use_stable_credentials() {
    let mut fixture = Fixture::default();
    let first = fixture
        .run("instances_create", "", input("duplicate"))
        .await;
    let second = fixture
        .run("instances_create", "", input("duplicate"))
        .await;
    assert_eq!(first["success"], true);
    assert_eq!(second["success"], true);
    let a = first["instance"]["instance_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let b = second["instance"]["instance_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(a, b);
    assert_eq!(uuid::Uuid::parse_str(&a).unwrap().get_version_num(), 4);
    let edited = fixture
        .run(
            "instances_update",
            &a,
            json!({"name":"renamed","base_url":"https://changed.example/"}),
        )
        .await;
    assert_eq!(edited["instance"]["instance_id"], a);
    let listed = fixture.list(json!({})).await;
    assert_eq!(listed["total"], 2);
    assert_eq!(listed["items"].as_array().unwrap().len(), 2);
    for secret in ["new-public", "new-secret"] {
        assert!(!listed.to_string().contains(secret));
        assert!(!first.to_string().contains(secret));
    }
    assert_eq!(
        fixture.run("instances_test", &a, json!({})).await["connected"],
        true
    );
    assert!(fixture.http.last().unwrap()["url"]
        .as_str()
        .unwrap()
        .starts_with("https://changed.example/"));
    assert_eq!(
        fixture.run("instances_delete", &a, json!({})).await["success"],
        true
    );
    assert!(!fixture.credentials.contains_key(&key(&a)));
    assert!(fixture.credentials.contains_key(&key(&b)));
    assert_eq!(fixture.list(json!({})).await["total"], 1);
}
#[tokio::test]
async fn selected_test_and_export_use_only_selected_instance_pair_and_base() {
    let mut fixture = Fixture::seeded();
    for (id, base, name) in [
        (A, "https://one.example/prefix/", "a"),
        (B, "https://two.example/", "b"),
    ] {
        assert_eq!(
            fixture.run("instances_test", id, json!({})).await["connected"],
            true
        );
        let http = fixture.http.last().unwrap();
        assert_eq!(http["url"], format!("{base}api/public/projects"));
        assert_eq!(
            http["headers"]["authorization"],
            format!(
                "Basic {}",
                STANDARD.encode(format!("public-{name}:secret-{name}"))
            )
        );
        for handler in ["export", "import"] {
            let body = if handler == "export" {
                json!({"instance_id":id,"application_id":"source","record_ids":["record"]})
            } else {
                json!({"instance_id":id,"application_id":"target","trace_ids":["trace"]})
            };
            let result=invoke(handler,body,|service,request| {
                if service==MultiplexHostService::ApplicationLogsV1 {
                    return match request["operation"].as_str().unwrap() {
                        "list_applications"=>json!({"result":{"operation":"applications","items":[{"application_id":"target","name":"Target","application_type":"agent_logs"}]}}),
                        "export_otlp"=>json!({"result":{"operation":"exported","otlp":crate::workflow_tests::nonempty_otlp(),"record_ids":["record"],"trace_ids":["trace"],"exported_spans":3}}),
                        "import_otlp"=>json!({"result":{"operation":"imported","accepted_events":1,"duplicate_events":0,"record_ids":["record"]}}),
                        other=>panic!("unexpected logs operation {other}"),
                    };
                }
                if service==MultiplexHostService::HttpEgressV1 {
                    assert!(request["url"].as_str().unwrap().starts_with(base));
                    assert_eq!(request["headers"]["authorization"],format!("Basic {}",STANDARD.encode(format!("public-{name}:secret-{name}"))));
                    let body=if handler=="export" {crate::remote_tests::queue_ack()}
                        else {json!({"data":[{"id":"observation","traceId":"trace","startTime":"2026-10-15T00:00:00Z","output":"actual content"}],"meta":{"cursor":null}})};
                    return json!({"result":{"status":200,"headers":{"content-type":"application/json"},"body_base64":STANDARD.encode(serde_json::to_vec(&body).unwrap())}});
                }
                fixture.callback(service,request)
            }).await;
            assert_eq!(result["success"], true, "{handler}: {result}");
        }
    }
}
#[tokio::test]
async fn legacy_reads_project_without_writes_and_mutation_persists_default_once() {
    let mut fixture = Fixture {
        rows: vec![row(None, "default", "https://legacy.example/")],
        credentials: BTreeMap::from([("langfuse_project".into(), pair("legacy"))]),
        ..Fixture::default()
    };
    assert_eq!(
        fixture.list(json!({})).await["items"][0]["instance_id"],
        "default"
    );
    assert_eq!(fixture.data_writes, 0);
    assert_eq!(
        fixture
            .run(
                "instances_update",
                "default",
                json!({"name":"renamed","base_url":"https://legacy.example/"})
            )
            .await["success"],
        true
    );
    assert_eq!(fixture.rows[0]["values"]["instance_id"]["value"], "default");
    assert_eq!(fixture.data_writes, 2);
    assert_eq!(
        fixture
            .run(
                "instances_update",
                "default",
                json!({"name":"renamed again","base_url":"https://legacy.example/"})
            )
            .await["success"],
        true
    );
    assert_eq!(fixture.data_writes, 3);
    assert_eq!(fixture.credentials.len(), 1);
    let result = invoke("config_get", json!({}), |service, request| {
        fixture.callback(service, request)
    })
    .await;
    assert_eq!(result["base_url"], "https://legacy.example/");
    assert_eq!(result["credential_configured"], true);
}
#[tokio::test]
async fn default_never_selects_first_nondefault_and_invalid_pairs_do_not_mutate() {
    let mut fixture = Fixture::seeded();
    let result = invoke("config_get", json!({}), |service, request| {
        fixture.callback(service, request)
    })
    .await;
    assert_eq!(result["base_url"], "");
    assert_eq!(result["credential_configured"], false);
    let result = invoke("test", json!({}), |service, request| {
        fixture.callback(service, request)
    })
    .await;
    assert_eq!(result["success"], false);
    assert!(fixture.http.is_empty());
    let before = fixture.credentials.clone();
    for keys in [
        json!({"public_key":"partial"}),
        json!({"public_key":"","secret_key":"secret"}),
    ] {
        let mut body = json!({"name":"edited","base_url":"https://edited.example/"});
        body.as_object_mut()
            .unwrap()
            .extend(keys.as_object().unwrap().clone());
        assert_eq!(
            fixture.run("instances_update", A, body).await["error"]["code"],
            "langfuse_credentials_pair"
        );
    }
    assert_eq!(fixture.data_writes, 0);
    assert_eq!(fixture.credentials, before);
    assert_eq!(
        fixture
            .run(
                "instances_update",
                "33333333-3333-4333-8333-333333333333",
                json!({"name":"unknown","base_url":"https://edited.example/"})
            )
            .await["error"]["code"],
        "langfuse_instance_not_found"
    );
}
#[tokio::test]
async fn pagination_has_metadata_and_drains_transport_chunks_without_business_cap() {
    let mut fixture = Fixture::default();
    for n in 1..=125 {
        let id = uuid::Uuid::from_u128(n).to_string();
        fixture
            .rows
            .push(row(Some(&id), "same", "https://shared.example/"));
    }
    let all = fixture.list(json!({"page":"1","page_size":"125"})).await;
    assert_eq!(all["total"], 125);
    assert_eq!(all["page_size"], 125);
    assert_eq!(all["items"].as_array().unwrap().len(), 125);
    let page = fixture.list(json!({"page":3,"page_size":50})).await;
    assert_eq!(page["page"], 3);
    assert_eq!(page["items"].as_array().unwrap().len(), 25);
    assert!(fixture.list(json!({"page":9})).await["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(fixture.list(json!({"page":0})).await["success"], false);
    assert_eq!(fixture.data_writes, 0);
}
#[tokio::test]
async fn create_callback_failures_cleanup_credentials_and_rows_including_ambiguous_commits() {
    for operation in ["insert", "credential_put"] {
        for after in [false, true] {
            let mut fixture = Fixture {
                fail: Some((operation.into(), after)),
                ..Fixture::default()
            };
            let result = fixture.run("instances_create", "", input("test")).await;
            assert_eq!(result["success"], false);
            assert!(fixture.rows.is_empty());
            assert!(fixture.credentials.is_empty());
            assert_eq!(result["error"]["details"]["credential_restored"], true);
        }
    }
    let mut fixture = Fixture {
        fail: Some(("insert".into(), false)),
        fail_cleanup: true,
        ..Fixture::default()
    };
    let result = fixture.run("instances_create", "", input("test")).await;
    assert_eq!(result["success"], false);
    assert_eq!(result["error"]["details"]["credential_restored"], false);
    assert!(!result.to_string().contains("new-secret"));
}
#[tokio::test]
async fn update_and_delete_failure_restore_only_affected_instance_after_ambiguous_commit() {
    for operation in ["update", "delete", "credential_put", "credential_delete"] {
        let mut fixture = Fixture::seeded();
        let rows = fixture.rows.clone();
        let credentials = fixture.credentials.clone();
        fixture.fail = Some((operation.into(), true));
        let result = if operation.contains("delete") {
            fixture.run("instances_delete", A, json!({})).await
        } else {
            fixture.run("instances_update", A, input("new")).await
        };
        assert_eq!(result["success"], false);
        assert_eq!(fixture.credentials, credentials);
        let mut actual = fixture.rows.clone();
        let mut expected = rows;
        actual.sort_by_key(Value::to_string);
        expected.sort_by_key(Value::to_string);
        assert_eq!(actual, expected);
    }
}
