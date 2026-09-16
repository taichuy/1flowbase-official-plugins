use super::*;
use std::{
    net::TcpListener,
    sync::{mpsc, Arc},
    thread,
};
use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};

#[derive(Debug)]
struct FixedClock(Instant);

impl WebsocketClock for FixedClock {
    fn now(&self) -> Instant {
        self.0
    }
}

fn websocket_input(base_url: &str) -> ProviderInvocationInput {
    ProviderInvocationInput {
        provider_instance_id: "fixture-provider".into(),
        provider_code: "openai".into(),
        protocol: "openai_responses".into(),
        model: "fixture-model".into(),
        provider_config: json!({
            "base_url": base_url,
            "api_key": "credential-canary",
            "transport_mode": "responses_websocket"
        }),
        messages: vec![ProviderMessage {
            role: ProviderMessageRole::User,
            content: "fixture request".into(),
            content_blocks: None,
            name: None,
            tool_call_id: None,
            is_error: None,
            tool_calls: None,
        }],
        model_parameters: BTreeMap::from([("store".into(), Value::Bool(false))]),
        run_context: BTreeMap::from([(
            TRANSPORT_SESSION_CONTEXT_KEY.into(),
            json!({
                "logical_session_id":"logical-fixture",
                "generation":9,
                "task_id":"task-fixture",
                "state":"active",
                "physical_deadline_unix_ms":4_102_444_800_000_i64
            }),
        )]),
        ..Default::default()
    }
}

fn start_closing_websocket(
    visible_output: bool,
    semantic_terminal: bool,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut websocket = tokio_tungstenite::tungstenite::accept(stream).unwrap();
        websocket.read().unwrap();
        if visible_output {
            websocket
                .send(Message::Text(
                    json!({"type":"response.output_text.delta","delta":"visible"})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        }
        if semantic_terminal {
            websocket
                .send(Message::Text(
                    json!({"type":"error","error":{"code":"invalid_request","message":"terminal"}})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        } else {
            websocket
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::Error,
                    reason: "fixture unavailable".into(),
                })))
                .unwrap();
        }
    });
    (base_url, handle)
}

#[test]
fn transport_unavailable_survives_stdio_serialization() {
    let response = ProviderStdioResponse::runtime_error(ProviderRuntimeError {
        kind: ProviderRuntimeErrorKind::ProviderTransportUnavailable,
        message: "pre-output websocket interruption".into(),
        provider_summary: None,
        provider_details: None,
    });

    assert_eq!(
        serde_json::to_value(response).unwrap()["error"]["kind"],
        "provider_transport_unavailable"
    );
}

#[test]
fn typed_session_context_rejects_unknown_fields_and_capacity_is_not_evicted() {
    let mut input = websocket_input("https://example.test/v1");
    input.run_context.insert(
        TRANSPORT_SESSION_CONTEXT_KEY.into(),
        json!({
            "logical_session_id":"logical-fixture",
            "generation":9,
            "task_id":"task-fixture",
            "state":"idle",
            "physical_deadline_unix_ms":4_102_444_800_000_i64,
            "session_key":"must-not-cross"
        }),
    );
    assert!(transport_session_directive(&input).is_err());

    input.run_context.insert(
        TRANSPORT_SESSION_CONTEXT_KEY.into(),
        json!({
            "logical_session_id":"logical-fixture",
            "task_id":"task-fixture",
            "state":"idle",
            "physical_deadline_unix_ms":4_102_444_800_000_i64
        }),
    );
    assert!(transport_session_directive(&input).is_err());

    let error = ensure_transport_session_capacity(64).unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<ProviderRuntimeError>()
            .map(|error| &error.kind),
        Some(&ProviderRuntimeErrorKind::ProviderTransportAdmissionFailed)
    );
}

#[test]
fn recoverable_transport_classification_covers_close_reset_eof_and_idle() {
    for code in [1011, 1012, 1013] {
        assert!(websocket_close_code_is_recoverable(code));
    }
    for code in [1000, 1008] {
        assert!(!websocket_close_code_is_recoverable(code));
    }
    for message in [
        "connection reset by peer",
        "websocket closed before response.completed",
        "idle timeout waiting for Responses websocket",
    ] {
        let error =
            WebsocketInvocationError::from_reconnectable_stream_state(anyhow!(message), false);
        assert_eq!(
            error
                .source
                .downcast_ref::<ProviderRuntimeError>()
                .map(|error| &error.kind),
            Some(&ProviderRuntimeErrorKind::ProviderTransportUnavailable)
        );
        assert!(error.reconnect_allowed);
    }
}

#[tokio::test]
async fn recoverable_close_is_typed_only_before_visible_output() {
    let (base_url, upstream) = start_closing_websocket(false, false);
    let error = OpenAiProviderRuntime::default()
        .invoke_response(websocket_input(&base_url))
        .await
        .unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<ProviderRuntimeError>()
            .map(|error| &error.kind),
        Some(&ProviderRuntimeErrorKind::ProviderTransportUnavailable),
        "unexpected error: {error:?}"
    );
    upstream.join().unwrap();

    let (base_url, upstream) = start_closing_websocket(true, false);
    let mut emitted = Vec::new();
    let error = OpenAiProviderRuntime::default()
        .invoke_response_with_event_sink(websocket_input(&base_url), |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .await
        .unwrap_err();
    assert!(emitted.iter().any(
        |event| matches!(event, ProviderStreamEvent::TextDelta { delta } if delta == "visible")
    ));
    assert_ne!(
        error
            .downcast_ref::<ProviderRuntimeError>()
            .map(|error| &error.kind),
        Some(&ProviderRuntimeErrorKind::ProviderTransportUnavailable)
    );
    upstream.join().unwrap();
}

#[tokio::test]
async fn semantic_terminal_is_never_marked_replayable() {
    let (base_url, upstream) = start_closing_websocket(false, true);
    let error = OpenAiProviderRuntime::default()
        .invoke_response(websocket_input(&base_url))
        .await
        .unwrap_err();

    assert_ne!(
        error
            .downcast_ref::<ProviderRuntimeError>()
            .map(|error| &error.kind),
        Some(&ProviderRuntimeErrorKind::ProviderTransportUnavailable)
    );
    upstream.join().unwrap();
}

#[test]
fn lifecycle_policy_soft_drains_between_50_and_55_minutes_and_hard_closes_at_58() {
    let created_at = Instant::now();
    let policy = WebsocketLifecyclePolicy::default();
    let soft_age = policy.soft_drain_age(17);
    assert!(soft_age >= Duration::from_secs(50 * 60));
    assert!(soft_age <= Duration::from_secs(55 * 60));
    assert_eq!(
        policy.state_at(
            17,
            created_at,
            created_at + soft_age - Duration::from_millis(1)
        ),
        WebsocketConnectionState::Ready
    );
    assert_eq!(
        policy.state_at(17, created_at, created_at + soft_age),
        WebsocketConnectionState::Draining
    );
    assert_eq!(
        policy.state_at(17, created_at, created_at + Duration::from_secs(58 * 60)),
        WebsocketConnectionState::Closing
    );

    let mut runtime = OpenAiProviderRuntime::default();
    runtime.websocket_clock = Arc::new(FixedClock(created_at + soft_age));
    assert_eq!(runtime.websocket_clock.now(), created_at + soft_age);
}

#[test]
fn credential_and_generation_ownership_are_explicit() {
    let input = websocket_input("https://example.test/v1");
    let config = normalize_provider_config(&input.provider_config).unwrap();
    let directive = transport_session_directive(&input).unwrap();
    let key = websocket_session_key(&config, &input, directive.as_ref());
    assert!(!key.contains("credential-canary"));
    assert!(key.contains("sha256:"));

    let mut runtime = OpenAiProviderRuntime::default();
    runtime.websocket_response_owners.insert(
        "resp_old".into(),
        WebsocketResponseOwner {
            session_key: key,
            generation: 7,
        },
    );
    runtime.websocket_chain_inputs_by_response_id.insert(
        "resp_old".into(),
        vec![json!({"role":"user","content":"old context"})],
    );
    let replay = runtime
        .websocket_full_context_retry_body(
            "resp_old",
            &json!({
                "previous_response_id":"resp_old",
                "input":[{"role":"user","content":"new context"}],
                "store":false
            }),
        )
        .unwrap();
    assert!(replay.get("previous_response_id").is_none());
    assert_eq!(replay["input"].as_array().unwrap().len(), 2);
}

#[test]
fn invocation_timing_receipt_is_bounded_and_payload_free() {
    let mut metadata = json!({});
    attach_invocation_timing_receipt(
        &mut metadata,
        Some(Duration::from_millis(17)),
        Duration::from_millis(241),
    )
    .unwrap();

    let receipt = &metadata[INVOCATION_TIMING_RECEIPT_METADATA_KEY];
    assert_eq!(receipt["schema_version"], 1);
    assert_eq!(receipt["connect_ms"], 17);
    assert_eq!(receipt["upstream_ms"], 241);
    assert_eq!(receipt["termination_kind"], "completed");
    let serialized = serde_json::to_string(&metadata).unwrap();
    for forbidden in [
        "api_key",
        "prompt",
        "tool_output",
        "encrypted_content",
        "previous_response_id",
        "response_id",
    ] {
        assert!(!serialized.contains(forbidden));
    }
}

#[tokio::test]
async fn proactive_close_flushes_frame_and_waits_for_peer_ack() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let (close_tx, close_rx) = mpsc::channel();
    let upstream = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut websocket = tokio_tungstenite::tungstenite::accept(stream).unwrap();
        let close = websocket.read().unwrap();
        close_tx.send(matches!(close, Message::Close(_))).unwrap();
        let _ = websocket.flush();
    });
    let input = websocket_input(&base_url);
    let config = normalize_provider_config(&input.provider_config).unwrap();
    let session = connect_responses_websocket(
        &config,
        None,
        &RestoredProtocolContext::default(),
        9,
        Some(9),
        Instant::now(),
    )
    .await
    .unwrap();
    let mut runtime = OpenAiProviderRuntime::default();
    runtime
        .websocket_sessions
        .insert("physical-fixture".into(), session);
    runtime
        .websocket_logical_sessions
        .insert("logical-fixture".into(), "physical-fixture".into());
    let stale = runtime
        .control_transport_session(TransportSessionCommand {
            logical_session_id: "logical-fixture".into(),
            generation: 8,
            action: TransportSessionAction::Drain,
            deadline_unix_ms: 4_102_444_800_000,
        })
        .await
        .unwrap_err();
    assert_eq!(
        stale
            .downcast_ref::<ProviderRuntimeError>()
            .map(|error| &error.kind),
        Some(&ProviderRuntimeErrorKind::ProviderTransportUnavailable)
    );
    assert!(runtime.websocket_sessions.contains_key("physical-fixture"));
    let receipt = runtime
        .control_transport_session(TransportSessionCommand {
            logical_session_id: "logical-fixture".into(),
            generation: 9,
            action: TransportSessionAction::Drain,
            deadline_unix_ms: 4_102_444_800_000,
        })
        .await
        .unwrap();

    assert_eq!(receipt.generation, 9);
    assert_eq!(receipt.physical_state, PhysicalTransportState::Closed);
    assert_eq!(
        receipt.close_reason,
        Some(TransportSessionCloseReason::RequestedDrain)
    );
    assert_eq!(receipt.close_acknowledged, Some(true));
    assert!(runtime.websocket_sessions.is_empty());
    assert!(close_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    upstream.join().unwrap();
}
