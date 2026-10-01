use super::*;
use openai_compatible_provider::ProviderRuntimeErrorKind;

#[test]
fn streaming_error_event_keeps_typed_http_body_and_status() {
    let body = " \n{\"message\":\"keep complete body\"}\n ";
    let error = ProviderRuntimeError {
        kind: ProviderRuntimeErrorKind::ProviderUpstreamError,
        message: body.into(),
        provider_summary: Some(body.into()),
        provider_details: Some(json!({ "status": 500 })),
    };
    let wire = stream_error_event(&anyhow::Error::new(error));
    assert_eq!(wire["error"]["kind"], "provider_upstream_error");
    assert_eq!(wire["error"]["message"], body);
    assert_eq!(wire["error"]["provider_details"]["status"], 500);
}
