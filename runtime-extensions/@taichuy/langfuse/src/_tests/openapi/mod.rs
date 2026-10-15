use crate::workflow_tests::invoke;
use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::MultiplexHostService;
use serde_json::{json, Value};
mod failures;
mod operations;

const INSTANCE: &str = "11111111-1111-4111-8111-111111111111";
const OTHER: &str = "22222222-2222-4222-8222-222222222222";
const SPEC: &str = include_str!("fixture.yml");
fn response(status: u16, bytes: &[u8]) -> Value {
    json!({"result":{"status":status,"headers":{"content-type":"application/octet-stream","x-upstream":"retained"},"body_base64":STANDARD.encode(bytes)}})
}
fn json_response(status: u16, body: Value) -> Value {
    response(status, &serde_json::to_vec(&body).unwrap())
}
struct Fixture {
    spec: Vec<u8>,
    requests: Vec<Value>,
    spec_status: u16,
    upstream: Value,
}
impl Default for Fixture {
    fn default() -> Self {
        Self {
            spec: SPEC.as_bytes().into(),
            requests: vec![],
            spec_status: 200,
            upstream: json_response(200, json!({"data":[]})),
        }
    }
}
impl Fixture {
    async fn run(&mut self, handler: &str, mut body: Value) -> Value {
        if body.get("instance_id").is_none() {
            body["instance_id"] = json!(INSTANCE);
        }
        let id = body["instance_id"].as_str().unwrap().to_owned();
        invoke(handler,body,|service,request|{
            match service {
                MultiplexHostService::PluginDataV1=>{
                    assert_eq!(request["operations"][0]["operation"],"find_one");
                    assert_eq!(request["operations"][0]["filters"],json!([{"field":"instance_id","operator":"equal","value":{"type":"string","value":id}}]));
                    json!({"result":{"results":[{"result":"optional_row","row":{"values":{"instance_id":{"type":"string","value":id},"name":{"type":"string","value":"name"},"base_url":{"type":"string","value":if id==INSTANCE {"https://selected.example/prefix/"}else{"https://other.example/root/"}}}}}],"replayed":false}})
                },
                MultiplexHostService::PluginCredentialV1=>{
                    assert_eq!(request["operation"],"get");
                    assert_eq!(request["credential_id"],format!("langfuse_instance:{id}"));
                    json!({"result":{"value":{"public_key":format!("public-{id}"),"secret_key":format!("secret-{id}")}}})
                },
                MultiplexHostService::HttpEgressV1=>{
                    assert_eq!(request["headers"]["authorization"],format!("Basic {}",STANDARD.encode(format!("public-{id}:secret-{id}"))));
                    assert!(!request.to_string().contains("actor_id"));
                    self.requests.push(request.clone());
                    if self.requests.len()==1 {
                        assert_eq!(request["method"],"GET");
                        assert_eq!(request["url"],if id==INSTANCE {"https://selected.example/prefix/generated/api/openapi.yml"}else{"https://other.example/root/generated/api/openapi.yml"});
                        response(self.spec_status,&self.spec)
                    }else {assert_eq!(self.requests.len(),2,"execute must send exactly once after the spec fetch");self.upstream.clone()}
                },
                _=>panic!("unexpected service {service:?}"),
            }
        }).await
    }
}
fn document() -> Value {
    serde_yaml::from_str(SPEC).unwrap()
}
fn with_document(document: Value) -> Fixture {
    Fixture {
        spec: serde_json::to_vec(&document).unwrap(),
        ..Fixture::default()
    }
}
fn assert_safe(value: &Value) {
    assert!(!value.to_string().contains("secret-"));
    assert!(!value.to_string().contains("public-"));
    assert!(!value.to_string().contains("authorization"));
}
