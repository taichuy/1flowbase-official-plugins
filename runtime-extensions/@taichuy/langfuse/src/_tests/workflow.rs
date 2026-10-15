use crate::dispatch;
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::{
    serve_io, MultiplexEnvelope, MultiplexHostMessage, MultiplexHostService, MultiplexWorkerMessage,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub(crate) async fn invoke<F>(handler: &str, body: Value, callback: F) -> Value
where
    F: FnMut(MultiplexHostService, Value) -> Value,
{
    invoke_payload(handler, json!({"path":{},"query":{},"body":body}), callback).await
}
pub(crate) async fn invoke_payload<F>(handler: &str, payload: Value, mut callback: F) -> Value
where
    F: FnMut(MultiplexHostService, Value) -> Value,
{
    tokio::task::LocalSet::new().run_until(async {
        let (host,worker)=tokio::io::duplex(8192);
        let (worker_read,worker_write)=tokio::io::split(worker);
        let runner=tokio::task::spawn_local(serve_io(worker_read,worker_write,dispatch));
        let (host_read,mut host_write)=tokio::io::split(host);
        let mut reader=BufReader::new(host_read);
        let message=MultiplexEnvelope::new(MultiplexHostMessage::Call {call_id:"1".into(),request:json!({"method":"execute","input":{"handler":handler,"input_payload":payload}})});
        host_write.write_all(format!("{}\n",serde_json::to_string(&message).unwrap()).as_bytes()).await.unwrap();
        let result=loop {
            let mut line=String::new();
            assert!(reader.read_line(&mut line).await.unwrap()>0);
            let frame: MultiplexEnvelope<MultiplexWorkerMessage>=serde_json::from_str(&line).unwrap();
            match frame.message {
                MultiplexWorkerMessage::Callback {call_id,callback_id,service,request} => {
                    assert!(request.get("actor_id").is_none());assert!(request.get("workspace_id").is_none());
                    let response=callback(service,request);
                    let frame=MultiplexEnvelope::new(MultiplexHostMessage::CallbackResult {call_id,callback_id,response});
                    host_write.write_all(format!("{}\n",serde_json::to_string(&frame).unwrap()).as_bytes()).await.unwrap();
                },
                MultiplexWorkerMessage::Response {response,..} => break response,
                other=>panic!("unexpected worker message: {other:?}"),
            }
        };
        host_write.shutdown().await.unwrap();runner.await.unwrap().unwrap();
        assert_eq!(result["ok"],true);
        result["result"].clone()
    }).await
}
fn configuration(service: &MultiplexHostService, request: &Value) -> Option<Value> {
    match service {
        MultiplexHostService::PluginDataV1
            if request["operations"][0]["filters"]
                .as_array()
                .is_some_and(|filters| {
                    filters.iter().any(|filter| filter["operator"] == "is_null")
                }) =>
        {
            Some(
                json!({"result":{"results":[{"result":"optional_row","row":null}],"replayed":false}}),
            )
        }
        MultiplexHostService::PluginDataV1 => Some(
            json!({"result":{"results":[{"result":"optional_row","row":{"values":{"instance_id":{"type":"string","value":"default"},"name":{"type":"string","value":"default"},"base_url":{"type":"string","value":"https://langfuse.example/prefix/"}}}}],"replayed":false}}),
        ),
        MultiplexHostService::PluginCredentialV1 => Some(
            json!({"result":{"value":{"public_key":"fixture-public","secret_key":"fixture-secret"}}}),
        ),
        _ => None,
    }
}
fn http(value: Value) -> Value {
    http_status(200, value)
}
fn http_status(status: u16, value: Value) -> Value {
    json!({"result":{"status":status,"headers":{"content-type":"application/json"},"body_base64":STANDARD.encode(serde_json::to_vec(&value).unwrap())}})
}
pub(crate) fn nonempty_otlp() -> Value {
    json!({"resourceSpans":[{"scopeSpans":[{"spans":[
        {"traceId":"0123456789abcdef0123456789abcdef","spanId":"0000000000000001","name":"fixture","startTimeUnixNano":"1792022400000000000","attributes":[]},
        {"traceId":"0123456789abcdef0123456789abcdef","spanId":"0000000000000002","name":"fixture child","startTimeUnixNano":"1792022400000000001","attributes":[]},
        {"traceId":"0123456789abcdef0123456789abcdef","spanId":"0000000000000003","name":"fixture final","startTimeUnixNano":"1792022400000000002","attributes":[]}
    ]}]}]})
}
#[tokio::test]
async fn config_never_echoes_either_project_key() {
    let result = invoke("config_get", json!({}), |service, request| {
        configuration(&service, &request).unwrap()
    })
    .await;
    assert_eq!(result["credential_configured"], true);
    assert_eq!(result["base_url"], "https://langfuse.example/prefix/");
    assert!(!result.to_string().contains("fixture-public"));
    assert!(!result.to_string().contains("fixture-secret"));
}
#[tokio::test]
async fn date_export_drains_all_host_pages_and_reports_only_acknowledged_progress() {
    let mut page = 0;
    let mut sends = 0;
    let result=invoke("export",json!({"application_id":"source","started_at_from":"2026-10-01T00:00:00Z","started_at_to":"2026-11-01T00:00:00Z"}),|service,request| {
        if let Some(config)=configuration(&service, &request) {return config;}
        match service {
            MultiplexHostService::ApplicationLogsV1 => match request["operation"].as_str().unwrap() {
                "list_records" => {page+=1;assert_eq!(request["cursor"],if page==1 {Value::Null} else {json!("page-2")});json!({"result":{"operation":"records","items":[{"record_id":format!("record-{page}"),"log_conversation_id":"c","title":"full source","started_at":"2026-10-15T00:00:00Z","source_kind":"native"}],"next_cursor":if page==1 {json!("page-2")} else {Value::Null}}})},
                "export_otlp" => json!({"result":{"operation":"exported","otlp":nonempty_otlp(),"record_ids":request["record_ids"],"trace_ids":[format!("trace-{page}")],"exported_spans":3}}),
                _=>panic!("unexpected logs operation"),
            },
            MultiplexHostService::HttpEgressV1 => {sends+=1;assert!(request["url"].as_str().unwrap().ends_with("/prefix/api/public/otel/v1/traces"));
                let body:Value=serde_json::from_slice(&STANDARD.decode(request["body_base64"].as_str().unwrap()).unwrap()).unwrap();
                assert_eq!(body["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array().unwrap().len(),3);
                if sends==1 {http(crate::remote_tests::queue_ack())} else {http(json!({"partialSuccess":{"rejectedSpans":"1"}}))}},
            _=>panic!("unexpected callback"),
        }
    }).await;
    assert_eq!(page, 2);
    assert_eq!(sends, 2);
    assert_eq!(result["success"], false);
    assert_eq!(
        result.pointer("/error/details/progress/exported_records"),
        Some(&json!(1))
    );
    assert_eq!(
        result.pointer("/error/details/progress/exported_spans"),
        Some(&json!(3))
    );
    assert_eq!(
        result.pointer("/error/details/cause_details/rejected_spans"),
        Some(&json!(1))
    );
    assert!(!result.to_string().contains("fixture-public"));
    assert!(!result.to_string().contains("fixture-project"));
}
#[tokio::test]
async fn complete_import_drains_remote_cursor_and_preserves_host_dedup_and_partial_receipt() {
    let mut pages = 0;
    let mut imports = 0;
    let result=invoke("import",json!({"application_id":"target","trace_ids":["trace-a","trace-b"]}),|service,request| {
        if let Some(config)=configuration(&service, &request) {return config;}
        match service {
            MultiplexHostService::ApplicationLogsV1 => match request["operation"].as_str().unwrap() {
                "list_applications"=>json!({"result":{"operation":"applications","items":[{"application_id":"target","name":"Target","application_type":"agent_logs"}]}}),
                "import_otlp"=>{imports+=1;if imports==1 {assert_eq!(request["otlp"]["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array().unwrap().len(),2);json!({"result":{"operation":"imported","accepted_events":2,"duplicate_events":4,"record_ids":["record-a"]}})} else {json!({"error":{"code":"agent_logs_partial_import","status":409,"details":{"accepted_events":1,"duplicate_events":0,"committed_records":1}}})}},
                _=>panic!("unexpected logs operation"),
            },
            MultiplexHostService::HttpEgressV1=>{pages+=1;let url=url::Url::parse(request["url"].as_str().unwrap()).unwrap();let trace=url.query_pairs().find(|(k,_)|k=="traceId").unwrap().1.into_owned();let cursor=url.query_pairs().any(|(k,_)|k=="cursor");http(json!({"data":[{"id":format!("observation-{pages}"),"traceId":trace,"startTime":"2026-10-15T00:00:00Z","output":"actual content"}],"meta":{"cursor":if trace=="trace-a" && !cursor {json!("next")} else {Value::Null}}}))},
            _=>panic!("unexpected callback"),
        }
    }).await;
    assert_eq!(pages, 3);
    assert_eq!(imports, 2);
    assert_eq!(result["error"]["status"], 409);
    assert_eq!(
        result.pointer("/error/details/progress/accepted_events"),
        Some(&json!(3))
    );
    assert_eq!(
        result.pointer("/error/details/progress/duplicate_events"),
        Some(&json!(4))
    );
    assert_eq!(
        result.pointer("/error/details/progress/partial_committed_records"),
        Some(&json!(1))
    );
    assert_eq!(
        result.pointer("/error/details/cause_details/accepted_events"),
        Some(&json!(1))
    );
}
#[tokio::test]
async fn foreign_or_native_import_target_is_rejected_before_network_or_mutation() {
    let result = invoke(
        "import",
        json!({"application_id":"foreign","trace_ids":["trace"]}),
        |service, request| {
            assert_eq!(service, MultiplexHostService::ApplicationLogsV1);
            assert_eq!(request["operation"], "list_applications");
            json!({"result":{"operation":"applications","items":[]}})
        },
    )
    .await;
    assert_eq!(
        result["error"]["code"],
        "langfuse_target_agent_logs_required"
    );
}

#[tokio::test]
async fn save_writes_only_url_to_owned_data_and_keys_to_credential_service() {
    let mut credential_puts = 0;
    let mut data_writes = 0;
    let result = invoke("config_put",json!({"base_url":"https://langfuse.example/prefix","public_key":"fixture-public","secret_key":"fixture-secret"}),|service,request| {
        match service {
            MultiplexHostService::PluginCredentialV1 if request["operation"] == "put" => {
                credential_puts += 1;
                assert_eq!(request["value"],json!({"public_key":"fixture-public","secret_key":"fixture-secret"}));
                json!({"result":{"value":null}})
            }
            MultiplexHostService::PluginDataV1 if request["operations"][0]["operation"] == "update" => {
                data_writes += 1;
                assert!(!request.to_string().contains("fixture-secret"));
                assert!(!request.to_string().contains("fixture-public"));
                assert_eq!(request["operations"][0]["values"]["base_url"]["value"],"https://langfuse.example/prefix/");
                json!({"result":{"results":[{"result":"mutation","affected":1}],"replayed":false}})
            }
            _ => configuration(&service, &request).unwrap(),
        }
    }).await;
    assert_eq!(credential_puts, 1);
    assert_eq!(data_writes, 1);
    assert_eq!(result["success"], true);
    assert!(!result.to_string().contains("fixture-secret"));
}

#[tokio::test]
async fn date_import_drains_trace_pages_then_fetches_whole_selected_traces() {
    let mut trace_pages = 0;
    let mut observations = 0;
    let mut imports = 0;
    let result = invoke("import",json!({"application_id":"target","started_at_from":"2026-10-01T00:00:00Z","started_at_to":"2026-11-01T00:00:00Z"}),|service,request| {
        if let Some(config) = configuration(&service, &request) { return config; }
        match service {
            MultiplexHostService::ApplicationLogsV1 if request["operation"] == "list_applications" => json!({"result":{"operation":"applications","items":[{"application_id":"target","name":"Target","application_type":"agent_logs"}]}}),
            MultiplexHostService::ApplicationLogsV1 => {
                imports += 1;
                assert_eq!(request["operation"],"import_otlp");
                json!({"result":{"operation":"imported","accepted_events":1,"duplicate_events":0,"record_ids":[format!("record-{imports}")]}})
            }
            MultiplexHostService::HttpEgressV1 => {
                let url = url::Url::parse(request["url"].as_str().unwrap()).unwrap();
                if url.path().ends_with("/traces") {
                    trace_pages += 1;
                    assert_eq!(url.query_pairs().find(|(k,_)|k=="page").unwrap().1,trace_pages.to_string());
                    http(json!({"data":[{"id":format!("trace-{trace_pages}"),"timestamp":"2026-10-15T00:00:00Z"}],"meta":{"page":trace_pages,"totalPages":2}}))
                } else {
                    observations += 1;
                    let trace = url.query_pairs().find(|(k,_)|k=="traceId").unwrap().1.into_owned();
                    http(json!({"data":[{"id":"observation","traceId":trace,"startTime":"2026-10-15T00:00:00Z","output":"complete"}],"meta":{"cursor":null}}))
                }
            }
            _ => panic!("unexpected callback"),
        }
    }).await;
    assert_eq!(trace_pages, 2);
    assert_eq!(observations, 2);
    assert_eq!(imports, 2);
    assert_eq!(result["receipt"]["accepted_events"], 2);
    assert_eq!(result["receipt"]["imported_traces"], 2);
}

fn events_only() -> Value {
    http_status(
        404,
        json!({"message":"This endpoint is not available on deployments running in Langfuse v4 events_only mode. Learn more about Langfuse v4 at: https://langfuse.com/docs/v4"}),
    )
}
fn date_import() -> Value {
    json!({"application_id":"target","started_at_from":"2026-10-01T00:00:00Z","started_at_to":"2026-11-01T00:00:00Z"})
}
fn target_application() -> Value {
    json!({"result":{"operation":"applications","items":[{"application_id":"target","name":"Target","application_type":"agent_logs"}]}})
}
fn observation(id: &str, trace: &str, start: &str) -> Value {
    json!({"id":id,"traceId":trace,"startTime":start,"output":"full fixture content"})
}

#[tokio::test]
async fn events_only_date_import_preserves_trace_start_boundaries_and_complete_traces() {
    let mut discovery_pages = 0;
    let mut complete_pages = std::collections::BTreeMap::<String, usize>::new();
    let mut imported = Vec::new();
    let result = invoke("import",date_import(),|service,request| {
        if let Some(config)=configuration(&service, &request) {return config;}
        match service {
            MultiplexHostService::ApplicationLogsV1 if request["operation"]=="list_applications" => target_application(),
            MultiplexHostService::ApplicationLogsV1 => {
                assert_eq!(request["operation"],"import_otlp");
                let spans=request["otlp"]["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array().unwrap();
                assert_eq!(spans.len(),2,"the whole trace includes a child beyond the date interval; duplicate versions collapse");
                let raw:Vec<Value>=spans.iter().map(|span| {
                    let raw=span["attributes"].as_array().unwrap().iter().find(|attr|attr["key"]=="1flowbase.external.raw").unwrap();
                    serde_json::from_str(raw["value"]["stringValue"].as_str().unwrap()).unwrap()
                }).collect();
                let trace=raw[0]["traceId"].as_str().unwrap().to_owned();
                assert!(trace=="at-from" || trace=="inside");
                assert!(raw.iter().any(|row|row["startTime"]=="2026-11-02T00:00:00Z"));
                assert!(raw.iter().any(|row|row["updatedAt"]=="2026-10-15T12:00:00Z"));
                assert!(!raw.iter().any(|row|row["updatedAt"]=="2026-10-15T10:00:00Z"));
                imported.push(trace);
                json!({"result":{"operation":"imported","accepted_events":2,"duplicate_events":0,"record_ids":[format!("record-{}",imported.len())]}})
            }
            MultiplexHostService::HttpEgressV1 => {
                let url=url::Url::parse(request["url"].as_str().unwrap()).unwrap();
                if url.path().ends_with("/api/public/traces") {return events_only();}
                assert!(url.path().ends_with("/api/public/v2/observations"));
                let query:std::collections::BTreeMap<_,_>=url.query_pairs().into_owned().collect();
                if !query.contains_key("traceId") {
                    discovery_pages+=1;
                    assert_eq!(query["fromStartTime"],"2026-10-01T00:00:00Z");
                    assert_eq!(query["toStartTime"],"2026-11-01T00:00:00Z");
                    if discovery_pages==1 {
                        assert!(!query.contains_key("cursor"));
                        http(json!({"data":[
                            observation("child","before","2026-10-15T00:00:00Z"),
                            observation("root","at-from","2026-10-01T00:00:00Z"),
                            observation("root","at-to","2026-11-01T00:00:00Z")
                        ],"meta":{"cursor":"candidates-2"}}))
                    } else {
                        assert_eq!(discovery_pages,2);
                        assert_eq!(query["cursor"],"candidates-2");
                        http(json!({"data":[
                            observation("root","inside","2026-10-15T00:00:00Z"),
                            observation("root","at-from","2026-10-01T00:00:00Z")
                        ],"meta":{}}))
                    }
                } else {
                    assert!(!query.contains_key("fromStartTime") && !query.contains_key("toStartTime"));
                    let trace=&query["traceId"];
                    assert_ne!(trace,"at-to");
                    *complete_pages.entry(trace.clone()).or_default()+=1;
                    if trace=="before" {
                        // Earliest observation is on the second full-trace page.
                        if query.contains_key("cursor") {
                            http(json!({"data":[observation("root",trace,"2026-09-30T23:59:59.999Z")],"meta":{}}))
                        } else {
                            http(json!({"data":[observation("child",trace,"2026-10-15T00:00:00Z")],"meta":{"cursor":"whole-2"}}))
                        }
                    } else {
                        let start=if trace=="at-from" {"2026-10-01T00:00:00Z"} else {"2026-10-15T00:00:00Z"};
                        let mut root=observation("root",trace,start);
                        root["createdAt"]=json!("2026-10-15T10:00:00Z");
                        root["updatedAt"]=json!("2026-10-15T10:00:00Z");
                        if query.contains_key("cursor") {
                            root["createdAt"]=json!("2026-10-15T12:00:00Z");
                            root["updatedAt"]=json!("2026-10-15T12:00:00Z");
                            http(json!({"data":[root,observation("late-child",trace,"2026-11-02T00:00:00Z")],"meta":{}}))
                        } else {
                            http(json!({"data":[root],"meta":{"cursor":"whole-2"}}))
                        }
                    }
                }
            }
            _=>panic!("unexpected callback"),
        }
    }).await;
    assert_eq!(result["success"], true);
    assert_eq!(discovery_pages, 2);
    assert_eq!(imported, vec!["at-from", "inside"]);
    assert_eq!(complete_pages["before"], 2);
    assert_eq!(complete_pages["at-from"], 4);
    assert_eq!(complete_pages["inside"], 4);
    assert_eq!(result["receipt"]["imported_traces"], 2);
    assert_eq!(result["receipt"]["accepted_events"], 4);
}

#[tokio::test]
async fn events_only_cursor_loops_fail_before_any_host_import() {
    for loop_in_whole_trace in [false, true] {
        let mut pages = 0;
        let result=invoke("import",date_import(),|service,request| {
            if let Some(config)=configuration(&service, &request) {return config;}
            match service {
                MultiplexHostService::ApplicationLogsV1 => {
                    assert_eq!(request["operation"],"list_applications");
                    target_application()
                }
                MultiplexHostService::HttpEgressV1 => {
                    let url=url::Url::parse(request["url"].as_str().unwrap()).unwrap();
                    if url.path().ends_with("/api/public/traces") {return events_only();}
                    pages+=1;
                    let whole=url.query_pairs().any(|(key,_)|key=="traceId");
                    http(json!({"data":[observation("root","trace","2026-10-15T00:00:00Z")],
                        "meta":{"cursor":if loop_in_whole_trace && !whole {Value::Null} else {json!("repeated")}}}))
                }
                _=>panic!("unexpected callback"),
            }
        }).await;
        assert_eq!(result["error"]["code"], "langfuse_cursor");
        assert_eq!(pages, if loop_in_whole_trace { 3 } else { 2 });
    }
}

#[tokio::test]
async fn date_import_never_falls_back_on_generic_404_auth_or_server_errors() {
    for (status, body) in [
        (404, json!({"message":"Endpoint not found"})),
        (404, json!({"message":"Trace not found"})),
        (
            401,
            json!({"message":"This endpoint is not available on deployments running in Langfuse v4 events_only mode."}),
        ),
        (
            500,
            json!({"message":"This endpoint is not available on deployments running in Langfuse v4 events_only mode."}),
        ),
    ] {
        let mut network = 0;
        let result = invoke("import", date_import(), |service, request| {
            if let Some(config) = configuration(&service, &request) {
                return config;
            }
            match service {
                MultiplexHostService::ApplicationLogsV1 => {
                    assert_eq!(request["operation"], "list_applications");
                    target_application()
                }
                MultiplexHostService::HttpEgressV1 => {
                    network += 1;
                    assert!(request["url"]
                        .as_str()
                        .unwrap()
                        .contains("/api/public/traces?"));
                    http_status(status, body.clone())
                }
                _ => panic!("unexpected callback"),
            }
        })
        .await;
        assert_eq!(network, 1);
        assert_eq!(result["success"], false);
        assert_eq!(result["error"]["status"], status);
    }
}

#[tokio::test]
async fn observation_content_conflict_stops_import_after_complete_pagination() {
    let mut pages = 0;
    let result=invoke("import",json!({"application_id":"target","trace_ids":["trace"]}),|service,request| {
        if let Some(config)=configuration(&service, &request) {return config;}
        match service {
            MultiplexHostService::ApplicationLogsV1=>{
                assert_eq!(request["operation"],"list_applications");target_application()
            }
            MultiplexHostService::HttpEgressV1=>{
                pages+=1;
                let mut row=observation("root","trace","2026-10-15T00:00:00Z");
                if pages==2 {row["output"]=json!("genuine different content");}
                http(json!({"data":[row],"meta":{"cursor":if pages==1 {json!("page-2")} else {Value::Null}}}))
            }
            _=>panic!("unexpected callback"),
        }
    }).await;
    assert_eq!(pages, 2);
    assert_eq!(result["error"]["code"], "langfuse_observation_conflict");
}

#[tokio::test]
async fn export_accepts_standard_and_queue_acknowledgements_without_exposing_remote_data() {
    for response in [
        http(json!({})),
        http(json!({"partialSuccess":{"rejectedSpans":"0","errorMessage":""}})),
        http(crate::remote_tests::queue_ack()),
        json!({"result":{"status":200,"headers":{"content-type":"application/x-protobuf"},"body_base64":""}}),
    ] {
        let result = export_with_ack(response).await;
        assert_eq!(result["success"], true);
        assert_eq!(result["receipt"]["exported_records"], 1);
        assert_eq!(result["receipt"]["exported_spans"], 3);
        assert!(!result.to_string().contains("fixture-public"));
        assert!(!result.to_string().contains("fixture-project"));
        assert!(!result.to_string().contains("otel/"));
    }
}

async fn export_with_ack(response: Value) -> Value {
    invoke("export",json!({"application_id":"source","record_ids":["record"]}),|service,request| {
        if let Some(config)=configuration(&service, &request) {return config;}
        match service {
            MultiplexHostService::ApplicationLogsV1=>{
                assert_eq!(request["operation"],"export_otlp");
                json!({"result":{"operation":"exported","otlp":nonempty_otlp(),"record_ids":["record"],"trace_ids":["trace"],"exported_spans":3}})
            }
            MultiplexHostService::HttpEgressV1=>{
                let body:Value=serde_json::from_slice(&STANDARD.decode(request["body_base64"].as_str().unwrap()).unwrap()).unwrap();
                assert_eq!(body["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array().unwrap().len(),3);
                response.clone()
            }
            _=>panic!("unexpected callback"),
        }
    }).await
}

#[tokio::test]
async fn export_rejections_never_count_unacknowledged_nonempty_spans_or_leak_private_ack() {
    let mut warning = crate::remote_tests::queue_ack();
    warning["warnings"] = json!(["private fixture warning"]);
    for response in [
        http(warning),
        http(json!({"name":"otel-ingestion-job","data":{"private":"private fixture warning"}})),
        http(json!({"partialSuccess":{"errorMessage":"private fixture warning"}})),
        json!({"result":{"status":200,"headers":{"content-type":"application/x-protobuf"},"body_base64":STANDARD.encode([0x0a,0x02,0x08,0x01])}}),
    ] {
        let result = export_with_ack(response).await;
        assert_eq!(result["success"], false);
        assert_eq!(
            result["error"]["details"]["progress"]["exported_records"],
            0
        );
        assert_eq!(result["error"]["details"]["progress"]["exported_spans"], 0);
        assert!(!result.to_string().contains("private fixture warning"));
        assert!(!result.to_string().contains("fixture-public"));
    }
}
