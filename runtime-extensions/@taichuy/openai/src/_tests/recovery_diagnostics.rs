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

#[test]
fn network_first_failure_retains_closed_io_kind_and_typed_provider_kind() {
    let source = network_error(&std::io::Error::new(
        std::io::ErrorKind::ConnectionReset,
        "https://private/?token=secret",
    ));
    let mut first = None;
    crate::remember_first_failure(&mut first, &source);
    crate::remember_first_failure(
        &mut first,
        &transport_error("websocket_close", "continuation_unavailable", Some(1008)),
    );
    let diagnostic = failure(&anyhow::Error::new(first.unwrap()), Some(1), Some(1));
    assert_eq!(diagnostic["kind"], "network_error");
    assert_eq!(diagnostic["io_error_kind"], "connection_reset");
    assert_eq!(
        diagnostic["provider_error_kind"],
        "provider_transport_unavailable"
    );
    assert!(!diagnostic.to_string().contains("secret"));
}

#[test]
fn historical_owner_without_live_socket_or_routing_evidence_is_not_available() {
    let mut runtime = crate::OpenAiProviderRuntime::default();
    runtime.websocket_response_owners.insert(
        "resp".into(),
        crate::WebsocketResponseOwner {
            session_key: "old".into(),
            generation: 3,
        },
    );
    let state = runtime.recovery_cursor_state(
        Some("resp"),
        None,
        crate::RecoverySignal::TransportDisconnected,
        Some(3),
    );
    assert!(matches!(
        state,
        crate::CursorState::ConnectionBound {
            owner_available: false,
            turn_state_available: false,
            ..
        }
    ));
    runtime
        .websocket_turn_states_by_response_id
        .insert("resp".into(), "fixture-route".into());
    let state = runtime.recovery_cursor_state(
        Some("resp"),
        None,
        crate::RecoverySignal::TransportDisconnected,
        Some(3),
    );
    assert!(matches!(
        state,
        crate::CursorState::ConnectionBound {
            owner_available: true,
            turn_state_available: true,
            ..
        }
    ));
    runtime.websocket_invalid_associations.insert("resp".into());
    let state = runtime.recovery_cursor_state(
        Some("resp"),
        None,
        crate::RecoverySignal::TransportDisconnected,
        Some(3),
    );
    assert!(matches!(
        state,
        crate::CursorState::ConnectionBound {
            owner_available: false,
            ..
        }
    ));
}
