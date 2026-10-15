use crate::mapping::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

fn observation() -> Value {
    json!({"id":"external-generation","traceId":"external-trace","startTime":"2026-10-15T00:00:00Z","endTime":"2026-10-15T00:00:01Z","type":"GENERATION","model":"example","input":[{"role":"system","content":"rules"},{"role":"user","content":"question"}],"output":{"answer":{"text":"complete response"}},"usageDetails":{"input":12,"output":7},"metadata":{"unrelated":{"dotted.key":{"nested":true}}}})
}
#[test]
fn standard_external_output_is_an_assistant_message_and_keeps_complete_raw_structure() {
    let input = observation();
    let request = from_observations(&[input.clone()]).unwrap();
    let span = &request["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
    let attr = |key: &str| {
        span["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["key"] == key)
            .unwrap()["value"]["stringValue"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        serde_json::from_str::<Value>(&attr("gen_ai.output.messages")).unwrap(),
        json!([{"role":"assistant","content":{"answer":{"text":"complete response"}}}])
    );
    assert_eq!(
        serde_json::from_str::<Value>(&attr("1flowbase.external.raw")).unwrap(),
        input
    );
    assert_eq!(from_observations(&[observation()]).unwrap(), request);
    assert!(
        span.get("status").is_none(),
        "endTime is not evidence of successful TaskEnd"
    );
}
#[test]
fn lossless_capsule_roundtrip_preserves_large_raw_values_and_original_identity() {
    let fact = json!({"schema_version":"1flowbase.agent-logs/v1","source_id":"native","source_client":"agentflow","event":{"event_id":"stable","raw":{"a.b":{"c":"x".repeat(80_000)}},"content":"full body"}});
    let capsule_value = format!(
        "1flowbase.agent-logs.source-fact/v1;base64,{}",
        STANDARD.encode(serde_json::to_vec(&fact).unwrap())
    );
    let source = json!({"resourceSpans":[{"scopeSpans":[{"spans":[{"traceId":"0123456789abcdef0123456789abcdef","spanId":"0123456789abcdef","attributes":[attribute("1flowbase.agent_log.source_fact",json!(capsule_value))]}]}]}]});
    let exported = to_langfuse(source).unwrap();
    let attributes = exported["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["attributes"]
        .as_array()
        .unwrap();
    let metadata = attributes
        .iter()
        .find(|a| a["key"] == "langfuse.observation.metadata")
        .unwrap()["value"]["stringValue"]
        .as_str()
        .unwrap();
    assert_eq!(capsule(&json!(metadata)).unwrap(), Some(capsule_value));
}
#[test]
fn malformed_or_ambiguous_self_metadata_never_falls_back_to_generic_observation() {
    for metadata in [
        json!({"1flowbase_agent_log.event.raw":"ambiguous"}),
        json!({"1flowbase_agent_log":"bad encoding"}),
        json!({"1flowbase_agent_log":{"source_id":"incomplete"}}),
    ] {
        let mut value = observation();
        value["metadata"] = metadata;
        assert!(from_observations(&[value]).is_err());
    }
}
#[test]
fn tool_relationships_and_unfinished_observations_remain_truthful() {
    let mut value = observation();
    value["type"] = json!("TOOL");
    value["parentObservationId"] = json!("parent");
    value["endTime"] = Value::Null;
    let request = from_observations(&[value]).unwrap();
    let span = &request["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
    assert!(span.get("endTimeUnixNano").is_none());
    assert!(span["parentSpanId"].is_string());
    assert!(span["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["key"] == "gen_ai.tool.call.result"));
}
