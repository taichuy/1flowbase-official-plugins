use super::*;
use anthropic_provider::ProviderRuntimeErrorKind;

fn upstream_failure() -> anyhow::Error {
    let error = ProviderRuntimeError {
        kind: ProviderRuntimeErrorKind::ProviderUpstreamError,
        message: "error code: 522\n".to_string(),
        provider_summary: Some("error code: 522\n".to_string()),
        provider_details: Some(json!({
            "status": 522,
            "status_code": 522,
            "raw_body": "error code: 522\n",
            "request_id": "req_522",
            "upstream_error": {
                "type": "api_error",
                "message": "error code: 522\n",
                "request_id": "req_522"
            }
        })),
    };
    // Context must not hide the typed cause or get added to the client message.
    anyhow::Error::new(error).context("worker invocation context")
}

fn assert_upstream_wire_error(error: &Value) {
    assert_eq!(error["kind"], "provider_upstream_error");
    assert_eq!(error["message"], "error code: 522\n");
    assert_eq!(error["provider_summary"], "error code: 522\n");
    assert_eq!(error["provider_details"]["status"], 522);
    assert_eq!(error["provider_details"]["status_code"], 522);
    assert_eq!(error["provider_details"]["raw_body"], "error code: 522\n");
    assert_eq!(error["provider_details"]["request_id"], "req_522");
    assert_eq!(
        error["provider_details"]["upstream_error"],
        json!({
            "type":"api_error", "message":"error code: 522\n", "request_id":"req_522"
        })
    );
    assert!(!error["message"]
        .as_str()
        .unwrap()
        .contains("ProviderUpstreamError"));
    assert!(!error["message"]
        .as_str()
        .unwrap()
        .contains("worker invocation context"));
}

#[test]
fn streaming_worker_serialization_preserves_typed_522_error() {
    let wire = worker_stream_error(upstream_failure());
    let encoded = serde_json::to_vec(&wire).unwrap();
    let decoded: Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded["type"], "error");
    assert_upstream_wire_error(&decoded["error"]);
}

#[test]
fn unary_worker_serialization_preserves_typed_522_error() {
    let wire = serde_json::to_value(worker_unary_error(upstream_failure())).unwrap();
    let encoded = serde_json::to_vec(&wire).unwrap();
    let decoded: Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded["ok"], false);
    assert!(decoded["result"].is_null());
    assert_upstream_wire_error(&decoded["error"]);
}

#[test]
fn untyped_worker_error_remains_an_explicit_failure() {
    let stream = worker_stream_error(anyhow::anyhow!("unexpected worker failure"));
    assert_eq!(stream["type"], "error");
    assert_eq!(stream["error"]["message"], "unexpected worker failure");
    let unary = serde_json::to_value(worker_unary_error(anyhow::anyhow!(
        "unexpected worker failure"
    )))
    .unwrap();
    assert_eq!(unary["ok"], false);
    assert_eq!(unary["error"]["message"], "unexpected worker failure");
}
