use super::*;
use openai_compatible_provider::ProviderRuntimeErrorKind;

fn upstream_failure(body: &str) -> anyhow::Error {
    anyhow::Error::new(ProviderRuntimeError {
        kind: ProviderRuntimeErrorKind::ProviderUpstreamError,
        message: body.to_owned(),
        provider_summary: Some(body.to_owned()),
        provider_details: Some(json!({"status": 500, "request_id": "req_failure"})),
    })
    .context("worker invocation context")
}

fn assert_upstream_wire_error(error: &Value, body: &str) {
    assert_eq!(error["kind"], "provider_upstream_error");
    assert_eq!(error["message"], body);
    assert_eq!(error["provider_summary"], body);
    assert_eq!(
        error["provider_details"],
        json!({"status": 500, "request_id": "req_failure"})
    );
}

#[test]
fn worker_serialization_preserves_typed_upstream_error_without_display_wrapping() {
    for body in [
        r#"{"error":{"message":"upstream unavailable"}}"#,
        "error code: 522\n",
        "<html>unavailable</html>",
        "retry later",
    ] {
        let stream = worker_stream_error(upstream_failure(body));
        let decoded: Value = serde_json::from_slice(&serde_json::to_vec(&stream).unwrap()).unwrap();
        assert_eq!(decoded["type"], "error");
        assert_upstream_wire_error(&decoded["error"], body);
        let unary = serde_json::to_value(worker_unary_error(upstream_failure(body))).unwrap();
        let decoded: Value = serde_json::from_slice(&serde_json::to_vec(&unary).unwrap()).unwrap();
        assert_eq!(decoded["ok"], false);
        assert!(decoded["result"].is_null());
        assert_upstream_wire_error(&decoded["error"], body);
    }
}

#[test]
fn untyped_worker_error_remains_an_explicit_failure() {
    let stream = worker_stream_error(anyhow::anyhow!("unexpected worker failure"));
    assert_eq!(stream["type"], "error");
    assert_eq!(stream["error"]["kind"], "provider_invalid_response");
    assert_eq!(stream["error"]["message"], "unexpected worker failure");
    let unary = serde_json::to_value(worker_unary_error(anyhow::anyhow!(
        "unexpected worker failure"
    )))
    .unwrap();
    assert_eq!(unary["ok"], false);
    assert_eq!(unary["error"]["kind"], "provider_invalid_response");
    assert_eq!(unary["error"]["message"], "unexpected worker failure");
}
