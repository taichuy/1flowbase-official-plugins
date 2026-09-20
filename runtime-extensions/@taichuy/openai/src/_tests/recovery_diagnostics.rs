use super::*;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

#[test]
fn close_classification_preserves_only_safe_fixed_reason() {
    for (reason, category) in [
        (
            "upstream continuation connection is unavailable; token=private",
            "continuation_unavailable",
        ),
        (
            "policy says https://private/?token=private",
            "policy_rejected",
        ),
    ] {
        let error = close_error(Some(CloseFrame {
            code: CloseCode::Policy,
            reason: reason.into(),
        }));
        let diagnostic = failure(&error, Some(7), Some(3));
        assert_eq!(diagnostic["close_code"], 1008);
        assert_eq!(diagnostic["reason_category"], category);
        assert_eq!(diagnostic["socket_incarnation"], 7);
        assert_eq!(diagnostic["owner_socket_incarnation"], 3);
        assert!(!diagnostic.to_string().contains("private"));
        assert!(!error.to_string().contains("private"));
    }
}

#[test]
fn first_untyped_failure_is_written_once_and_redacted() {
    let mut first = None;
    crate::remember_first_failure(
        &mut first,
        &anyhow::anyhow!("https://private/?token=secret"),
    );
    crate::remember_first_failure(
        &mut first,
        &transport_error("websocket_close", "proxy_failed", Some(1011)),
    );
    let first = first.unwrap();
    assert_eq!(
        first.provider_details.as_ref().unwrap()[FAILURE_KEY]["kind"],
        "provider_untyped"
    );
    assert!(!serde_json::to_string(&first).unwrap().contains("secret"));
}

#[test]
fn first_typed_failure_is_not_overwritten_by_last_close() {
    let mut first = None;
    crate::remember_first_failure(
        &mut first,
        &transport_error("websocket_close", "proxy_failed", Some(1011)),
    );
    crate::remember_first_failure(
        &mut first,
        &transport_error("websocket_close", "continuation_unavailable", Some(1008)),
    );
    assert!(first
        .unwrap()
        .message
        .contains("upstream websocket proxy failed"));
}
