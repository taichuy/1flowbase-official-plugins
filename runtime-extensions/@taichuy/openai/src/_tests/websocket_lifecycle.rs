use super::*;
use crate::close::CloseIdentity;
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
                "worker_incarnation":1,
                "task_id":"task-fixture",
                "state":"active",
                "physical_deadline_unix_ms":4_102_444_800_000_i64
            }),
        )]),
        ..Default::default()
    }
}

#[tokio::test]
async fn failed_http_turn_releases_bound_transport_generation_without_masking_upstream_error() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let upstream = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 8192];
        let _ = stream.read(&mut request).await.unwrap();
        let body = br#"{"error":{"message":"prewarm rejected"}}"#;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(body).await.unwrap();
    });
    let mut input = websocket_input(&base_url);
    input.provider_config["transport_mode"] = json!("http_sse");
    let directive = transport_session_directive(&input).unwrap().unwrap();
    let id = CloseIdentity::directive(&directive).unwrap();
    let mut runtime = OpenAiProviderRuntime::default();
    let error = runtime.invoke_response(input).await.unwrap_err();
    upstream.await.unwrap();
    let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    assert_eq!(typed.kind, ProviderRuntimeErrorKind::ProviderUpstreamError);
    assert_eq!(typed.provider_details.as_ref().unwrap()["status"], 400);
    let receipt = runtime
        .control_transport_session(TransportSessionCommand {
            logical_session_id: id.logical_session_id,
            generation: id.generation,
            worker_incarnation: Some(id.worker_incarnation),
            action: TransportSessionAction::Close,
            deadline_unix_ms: close::unix_time_ms() + 5_000,
        })
        .await
        .unwrap();
    assert_eq!(receipt.physical_state, PhysicalTransportState::Closed);
    assert_eq!(
        serde_json::to_value(receipt.closure_evidence).unwrap()["local_released"],
        true
    );
}

fn start_closing_websocket(
    visible_output: bool,
    semantic_terminal: bool,
) -> (String, thread::JoinHandle<TcpListener>) {
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
                    json!({"type":"response.failed","response":{"error":{"code":"invalid_request","message":"terminal private-canary"}}})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        } else {
            websocket
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::Error,
                    reason: "upstream websocket proxy failed".into(),
                })))
                .unwrap();
        }
        listener.set_nonblocking(true).unwrap();
        listener
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
}

fn managed_closing_input(base_url: &str, budget: u16) -> ProviderInvocationInput {
    let mut input = websocket_input(base_url);
    input.run_context.insert(recovery::RECOVERY_DIRECTIVE_CONTEXT_KEY.into(), json!({
        "policy":{"type":"semantic_mapped","budget":{"max_inner_attempts":budget,"absolute_deadline_unix_ms":4102444800000_i64}},
        "transport_epoch":9,"initial_commit_level":"lifecycle_only"
    }));
    input
}

fn assert_no_replacement_connection(upstream: thread::JoinHandle<TcpListener>) {
    let listener = upstream.join().unwrap();
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "terminal failure must not perform additional network I/O"
    );
}

fn assert_safe_terminal<'a>(error: &'a anyhow::Error, expected_reason: &str) -> &'a Value {
    let typed = error
        .downcast_ref::<ProviderRuntimeError>()
        .expect("recovery boundary returns a safe canonical error");
    assert_eq!(
        typed.kind,
        ProviderRuntimeErrorKind::ProviderTransportUnavailable
    );
    let details = typed.provider_details.as_ref().unwrap();
    let receipt = &details[recovery::RECOVERY_RECEIPT_METADATA_KEY];
    assert_eq!(
        receipt["disposition"],
        if expected_reason == "budget_exhausted" {
            "logical_invocation_retry"
        } else {
            "terminal_interruption"
        }
    );
    assert_eq!(
        receipt["commit_level"],
        if expected_reason == "semantic_failed" {
            "terminal"
        } else {
            "lifecycle_only"
        }
    );
    assert_eq!(receipt["reason"], expected_reason);
    assert_eq!(receipt["attempt"], 0);
    let diagnostics = &details[recovery_diagnostics::KEY];
    assert_eq!(diagnostics["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(diagnostics["first_failure"], diagnostics["last_failure"]);
    assert_eq!(diagnostics["last_failure"]["consumed_attempts"], 1);
    let serialized = serde_json::to_string(typed).unwrap();
    assert!(!serialized.contains("credential-canary"));
    assert!(!serialized.contains("private-canary"));
    diagnostics
}

#[test]
fn recoverable_transport_classification_covers_close_reset_eof_and_idle() {
    for code in [1011, 1012, 1013] {
        let source = websocket_closed_before_completed_error(Some(CloseFrame {
            code: CloseCode::from(code),
            reason: "private-canary".into(),
        }));
        let diagnostic = recovery_diagnostics::failure(&source, None, None);
        assert_eq!(diagnostic["kind"], "websocket_close");
        assert_eq!(diagnostic["close_code"], code);
        assert_eq!(diagnostic["reason_category"], "transport_disconnected");
    }
    for message in [
        "connection reset by peer",
        "websocket closed before response.completed",
        "idle timeout waiting for Responses websocket",
    ] {
        let mut error =
            WebsocketInvocationError::from_reconnectable_stream_state(anyhow!(message), false);
        assert!(error.reconnect_allowed);
        assert!(!error.semantic_committed);
        error
            .failure_diagnostics
            .push(recovery_diagnostics::failure(&error.source, None, None));
        let normalized = recovery_error_source(error, None);
        let typed = normalized.downcast_ref::<ProviderRuntimeError>().unwrap();
        assert_eq!(
            typed.kind,
            ProviderRuntimeErrorKind::ProviderTransportUnavailable
        );
        assert_eq!(
            typed.provider_details.as_ref().unwrap()[recovery_diagnostics::KEY]["first_failure"]
                ["kind"],
            "provider_untyped"
        );
    }
}

#[tokio::test]
async fn recoverable_close_type_is_independent_of_semantic_replay_permission() {
    let (base_url, upstream) = start_closing_websocket(false, false);
    let error = OpenAiProviderRuntime::default()
        .invoke_response(managed_closing_input(&base_url, 1))
        .await
        .unwrap_err();
    let diagnostics = assert_safe_terminal(&error, "budget_exhausted");
    assert_eq!(diagnostics["first_failure"]["kind"], "websocket_close");
    assert_eq!(diagnostics["first_failure"]["close_code"], 1011);
    assert_no_replacement_connection(upstream);

    let (base_url, upstream) = start_closing_websocket(true, false);
    let mut emitted = Vec::new();
    let error = OpenAiProviderRuntime::default()
        .invoke_response_with_event_sink(managed_closing_input(&base_url, 3), |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .await
        .unwrap_err();
    assert_eq!(emitted.iter().filter(|event| matches!(event, ProviderStreamEvent::TextDelta { delta } if delta == "visible")).count(), 1);
    let diagnostics = assert_safe_terminal(&error, "semantic_failed");
    assert_eq!(diagnostics["last_failure"]["close_code"], 1011);
    assert_no_replacement_connection(upstream);
}

#[tokio::test]
async fn semantic_terminal_is_never_marked_replayable() {
    let (base_url, upstream) = start_closing_websocket(false, true);
    let error = OpenAiProviderRuntime::default()
        .invoke_response(managed_closing_input(&base_url, 3))
        .await
        .unwrap_err();
    let diagnostics = assert_safe_terminal(&error, "semantic_failed");
    assert_eq!(diagnostics["first_failure"]["kind"], "provider_untyped");
    assert_no_replacement_connection(upstream);
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
fn sealed_session_key_survives_semantic_to_native_turn_without_merging_routing_context() {
    let mut semantic = websocket_input("https://example.test/v1");
    semantic.client_protocol_envelope = Some(ProtocolContextEnvelope {
        source_protocol: "openai_responses".into(),
        headers: BTreeMap::from([
            ("session-id".into(), vec!["stable-session".into()]),
            ("thread-id".into(), vec!["stable-thread".into()]),
            ("openai-organization".into(), vec!["org-a".into()]),
            ("x-codex-turn-metadata".into(), vec!["turn-one".into()]),
        ]),
        ..Default::default()
    });
    let config = normalize_provider_config(&semantic.provider_config).unwrap();
    let directive = transport_session_directive(&semantic).unwrap();
    let first_key = websocket_session_key(&config, &semantic, directive.as_ref());

    let mut continuation = semantic.clone();
    continuation.native_transport = Some(ProviderNativeTransport {
        protocol: "openai_responses".into(),
        wire_body: json!({"previous_response_id":"resp_previous","input":"next"}),
        digest: "fixture".into(),
        size_bytes: 0,
    });
    continuation
        .client_protocol_envelope
        .as_mut()
        .unwrap()
        .headers
        .insert("x-codex-turn-metadata".into(), vec!["turn-two".into()]);
    assert_eq!(
        first_key,
        websocket_session_key(&config, &continuation, directive.as_ref())
    );

    continuation
        .client_protocol_envelope
        .as_mut()
        .unwrap()
        .headers
        .insert("openai-organization".into(), vec!["org-b".into()]);
    assert_ne!(
        first_key,
        websocket_session_key(&config, &continuation, directive.as_ref())
    );
}

#[test]
fn websocket_response_create_accepts_http_responses_string_input_as_one_user_item() {
    let body = json!({
        "model":"gpt-6-luna",
        "previous_response_id":"resp_provider_owned",
        "input":"next turn",
        "reasoning":{"effort":"max"}
    });
    let wire = build_websocket_response_create_body(body);
    assert_eq!(wire["type"], "response.create");
    assert_eq!(wire["previous_response_id"], "resp_provider_owned");
    assert_eq!(
        wire["input"],
        json!([{"role":"user","content":"next turn"}])
    );
    assert_eq!(wire["reasoning"]["effort"], "max");

    let native_items =
        json!({"input":[{"type":"function_call_output","call_id":"call_1","output":"done"}]});
    assert_eq!(
        build_websocket_response_create_body(native_items)["input"],
        json!([{"type":"function_call_output","call_id":"call_1","output":"done"}])
    );
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
    let identity = close::CloseIdentity {
        logical_session_id: "logical-fixture".into(),
        generation: 9,
        worker_incarnation: 1,
    };
    let mut session = session;
    session.close_identity = Some(identity.clone());
    runtime.close_worker_incarnation = Some(1);
    runtime
        .close_ledger
        .reserve(identity.clone(), Instant::now())
        .unwrap();
    runtime.close_ledger.activated(&identity);
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
            worker_incarnation: Some(1),
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
            worker_incarnation: Some(1),
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

#[test]
fn failed_fallback_preserves_first_typed_diagnostic_and_terminal_http_evidence() {
    let directive: ProviderRecoveryDirective = serde_json::from_value(json!({
        "policy":{"type":"semantic_mapped","budget":{"max_inner_attempts":3,"absolute_deadline_unix_ms":4102444800000_i64}},
        "transport_epoch":19,"initial_commit_level":"lifecycle_only"
    })).unwrap();
    let mut original = WebsocketInvocationError::transport_unavailable("first websocket failure");
    original.transition = Some(RecoveryTransition {
        attempt: 1,
        commit_level: recovery::CommitLevel::LifecycleOnly,
        disposition: RecoveryDisposition::PreCommitHttpFallback,
        reason: recovery::RecoveryReason::TransportDisconnected,
    });
    original.socket_incarnation = Some(4);
    let secondary = ProviderRuntimeError::normalize("auth", "HTTP denied", None);
    let error = recovery_fallback_error_source(
        original,
        Some(&directive),
        anyhow::Error::new(secondary),
        false,
    );
    let primary = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    assert_eq!(primary.kind, ProviderRuntimeErrorKind::AuthFailed);
    assert_eq!(primary.message, "HTTP denied");
    let original =
        &primary.provider_details.as_ref().unwrap()[recovery::RECOVERY_ORIGINAL_ERROR_METADATA_KEY];
    assert_eq!(original["kind"], "provider_transport_unavailable");
    let diagnostics = &primary.provider_details.as_ref().unwrap()[recovery_diagnostics::KEY];
    assert_eq!(diagnostics["first_failure"]["kind"], "provider_typed");
    assert_eq!(diagnostics["last_failure"]["kind"], "provider_typed");
    let metadata = primary.failure_metadata();
    let receipt = &metadata[recovery::RECOVERY_RECEIPT_METADATA_KEY];
    assert_eq!(receipt["transport"], "provider_http");
    assert_eq!(receipt["transport_epoch"], 19);
    assert_eq!(receipt["attempt"], 1);
    assert_eq!(receipt["disposition"], "terminal_interruption");
    assert_eq!(receipt["commit_level"], "terminal");
    assert_eq!(receipt["reason"], "protocol_error");
    assert!(receipt.get("socket_incarnation").is_none());
    assert!(metadata
        .get(TRANSPORT_SESSION_RECEIPT_METADATA_KEY)
        .is_none());
    assert!(metadata.get("fallback_error").is_none());
}

#[test]
fn failure_timing_never_claims_completed_or_ready() {
    let original = ProviderRuntimeError::normalize("auth", "denied", None);
    let error = attach_failed_timing(
        anyhow::Error::new(original),
        Some(Duration::from_millis(7)),
        Duration::from_millis(23),
    );
    let metadata = error
        .downcast_ref::<ProviderRuntimeError>()
        .unwrap()
        .failure_metadata();
    let timing = &metadata[INVOCATION_TIMING_RECEIPT_METADATA_KEY];
    assert_eq!(timing["termination_kind"], "upstream_error");
    assert_eq!(timing["connect_ms"], 7);
    assert_eq!(timing["upstream_ms"], 23);
    assert!(metadata
        .get(TRANSPORT_SESSION_RECEIPT_METADATA_KEY)
        .is_none());
    let transport = WebsocketInvocationError::transport_unavailable("socket closed").source;
    let error = attach_failed_timing(transport, None, Duration::from_millis(11));
    assert_eq!(
        error
            .downcast_ref::<ProviderRuntimeError>()
            .unwrap()
            .failure_metadata()[INVOCATION_TIMING_RECEIPT_METADATA_KEY]["termination_kind"],
        "transport_error"
    );
    // Untyped errors retain their original downcast identity and gain no made-up receipt.
    let original = anyhow::Error::new(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "EOF",
    ));
    assert!(attach_failed_timing(original, None, Duration::ZERO)
        .downcast_ref::<std::io::Error>()
        .is_some());
}

#[test]
fn failed_fallback_keeps_safe_untyped_first_and_independent_last_failure() {
    for secondary in [
        anyhow::Error::new(ProviderRuntimeError::normalize(
            "auth",
            "secondary denied",
            None,
        )),
        anyhow::anyhow!("secondary https://private/?token=fixture-secret"),
    ] {
        let error = recovery_fallback_error_source(
            WebsocketInvocationError::fallback_allowed(anyhow::anyhow!(
                "first https://private/?token=fixture-secret"
            )),
            None,
            secondary,
            false,
        );
        let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
        let details = typed.provider_details.as_ref().unwrap();
        assert_eq!(
            details[recovery_diagnostics::KEY]["first_failure"]["kind"],
            "provider_untyped"
        );
        assert_eq!(
            details[recovery_diagnostics::KEY]["attempts"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(details
            .get(recovery::RECOVERY_ORIGINAL_ERROR_METADATA_KEY)
            .is_some());
        assert!(!serde_json::to_string(typed)
            .unwrap()
            .contains("fixture-secret"));
    }
}

#[test]
fn failed_fallback_keeps_typed_first_with_redacted_untyped_last() {
    let error = recovery_fallback_error_source(
        WebsocketInvocationError::transport_unavailable("first websocket failure"),
        None,
        anyhow::anyhow!("secondary contains fixture-secret"),
        false,
    );
    let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    let details = typed.provider_details.as_ref().unwrap();
    assert_eq!(
        details[recovery_diagnostics::KEY]["first_failure"]["kind"],
        "provider_typed"
    );
    assert_eq!(
        details[recovery_diagnostics::KEY]["last_failure"]["kind"],
        "provider_untyped"
    );
    assert!(!serde_json::to_string(typed)
        .unwrap()
        .contains("fixture-secret"));
}

#[test]
fn precommit_transport_receipt_survives_without_socket_incarnation() {
    let directive: ProviderRecoveryDirective = serde_json::from_value(json!({
        "policy":{"type":"native_opaque","budget":{"max_inner_attempts":3,"absolute_deadline_unix_ms":4102444800000_i64}},
        "transport_epoch":19,"initial_commit_level":"lifecycle_only"
    })).unwrap();
    let mut machine = RecoveryFsm::new(directive.constraints());
    machine.begin_attempt().unwrap();
    let mut error = WebsocketInvocationError::transport_unavailable("connection unavailable");
    error.transition = Some(machine.decide_transition(RecoveryFacts {
        signal: RecoverySignal::TransportDisconnected,
        cursor: CursorState::None,
        full_context_available: false,
    }));
    let error = recovery_error_source(error, Some(&directive));
    let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    let receipt =
        &typed.provider_details.as_ref().unwrap()[recovery::RECOVERY_RECEIPT_METADATA_KEY];
    assert_eq!(receipt["disposition"], "logical_invocation_retry");
    assert_eq!(receipt["commit_level"], "lifecycle_only");
    assert!(receipt.get("socket_incarnation").is_none());
}

#[tokio::test]
async fn http_eof_reports_precommit_but_output_and_tool_commit_refuse_retry() {
    use std::io::{Read, Write};
    for (event, committed) in [
        (
            json!({"type":"response.created","response":{"id":"http_lifecycle"}}),
            false,
        ),
        (
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","summary":[]}}),
            false,
        ),
        (
            json!({"type":"response.output_text.delta","delta":"visible"}),
            true,
        ),
        (
            json!({"type":"response.function_call_arguments.delta","item_id":"call_once","delta":"{}"}),
            true,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 8192];
            stream.read(&mut request).unwrap();
            let body = format!("data: {event}\n\n");
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            listener.set_nonblocking(true).unwrap();
            listener
        });
        let mut input = visibility_native_input(&base);
        input.provider_config["transport_mode"] = json!("http_sse");
        let mut emitted = Vec::new();
        let error = OpenAiProviderRuntime::default()
            .invoke_response_with_event_sink(input, |event| {
                emitted.push(event.clone());
                Ok(())
            })
            .await
            .unwrap_err();
        if !committed {
            assert!(emitted.is_empty());
        }
        let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
        let receipt =
            &typed.provider_details.as_ref().unwrap()[recovery::RECOVERY_RECEIPT_METADATA_KEY];
        assert_eq!(receipt["transport"], "provider_http");
        assert_eq!(receipt["attempt"], 0);
        assert_eq!(
            receipt["commit_level"],
            if committed {
                "terminal"
            } else {
                "lifecycle_only"
            }
        );
        assert_eq!(
            receipt["disposition"],
            if committed {
                "terminal_interruption"
            } else {
                "logical_invocation_retry"
            }
        );
        assert_no_replacement_connection(server);
    }
}

#[test]
fn http_failure_preserves_shared_budget_and_initial_commit_barrier() {
    for (budget, initial, disposition, reason) in [
        (
            1,
            recovery::CommitLevel::LifecycleOnly,
            RecoveryDisposition::LogicalInvocationRetry,
            recovery::RecoveryReason::BudgetExhausted,
        ),
        (
            3,
            recovery::CommitLevel::LifecycleOnly,
            RecoveryDisposition::LogicalInvocationRetry,
            recovery::RecoveryReason::TransportDisconnected,
        ),
        (
            3,
            recovery::CommitLevel::SemanticCommitted,
            RecoveryDisposition::TerminalInterruption,
            recovery::RecoveryReason::SemanticFailed,
        ),
    ] {
        let mut machine = RecoveryFsm::new(RecoveryConstraints {
            policy: RecoveryPolicyKind::NativeOpaque,
            max_inner_attempts: budget,
            absolute_deadline_unix_ms: None,
            initial_commit_level: initial,
        });
        machine.begin_attempt().unwrap();
        let source =
            recovery_diagnostics::transport_error("http_send", "transport_disconnected", None);
        let transition = http_failure_transition(&mut machine, &source, false);
        assert_eq!(transition.disposition, disposition);
        assert_eq!(transition.reason, reason);
        assert_eq!(machine.consumed_attempts(), 1);
    }
}

#[tokio::test]
async fn fresh_proxy_1011_without_output_or_route_delegates_without_reconnect() {
    let (base, server) = start_closing_websocket(false, false);
    let mut emitted = Vec::new();
    let error = OpenAiProviderRuntime::default()
        .invoke_response_with_event_sink(managed_closing_input(&base, 3), |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .await
        .unwrap_err();
    assert!(!emitted.iter().any(websocket_event_commits_output));
    let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    let details = typed.provider_details.as_ref().unwrap();
    let receipt = &details[recovery::RECOVERY_RECEIPT_METADATA_KEY];
    assert_eq!(receipt["disposition"], "logical_invocation_retry");
    assert_eq!(receipt["commit_level"], "lifecycle_only");
    assert_eq!(receipt["reason"], "transport_disconnected");
    assert_eq!(receipt["attempt"], 0);
    let diagnostics = &details[recovery_diagnostics::KEY];
    assert_eq!(diagnostics["first_failure"]["close_code"], 1011);
    assert_eq!(diagnostics["first_failure"], diagnostics["last_failure"]);
    assert_eq!(diagnostics["last_failure"]["consumed_attempts"], 1);
    assert_no_replacement_connection(server);
}

fn visibility_native_input(base: &str) -> ProviderInvocationInput {
    let mut input = managed_closing_input(base, 3);
    input.required_capabilities.extend([
        ProviderInvocationCapability::ResponsesNativePassthrough,
        ProviderInvocationCapability::ResponsesNativeOutputV1,
    ]);
    let body = json!({"model":"fixture-model","input":[{"role":"user","content":"fixture"}],"generate":false});
    input.native_transport = Some(ProviderNativeTransport {
        protocol: "openai_responses".into(),
        digest: "sha256:synthetic".into(),
        size_bytes: body.to_string().len() as u64,
        wire_body: body,
    });
    input
        .run_context
        .get_mut(recovery::RECOVERY_DIRECTIVE_CONTEXT_KEY)
        .unwrap()["policy"]["type"] = json!("native_opaque");
    input
}

fn start_visibility_websocket(
    rounds: Vec<(Vec<Value>, bool)>,
) -> (String, thread::JoinHandle<TcpListener>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for (events, success) in rounds {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut ws = tokio_tungstenite::tungstenite::accept(stream).unwrap();
            ws.read().unwrap();
            for event in events {
                ws.send(Message::Text(event.to_string().into())).unwrap();
            }
            if success {
                ws.send(Message::Text(json!({"type":"response.completed","response":{"id":"success","status":"completed","output":[]}}).to_string().into())).unwrap();
            } else {
                let _ = ws.send(Message::Close(Some(CloseFrame {
                    code: CloseCode::Error,
                    reason: "upstream websocket proxy failed".into(),
                })));
            }
        }
        listener.set_nonblocking(true).unwrap();
        listener
    });
    (base, server)
}

#[tokio::test]
async fn empty_added_proxy_failure_delegates_and_logical_retry_has_no_abandoned_slots() {
    let abandoned = json!({"type":"response.output_item.added","output_index":0,"item":{"id":"abandoned","type":"reasoning","summary":[]}});
    let fresh = json!({"type":"response.output_item.added","output_index":0,"item":{"id":"fresh","type":"reasoning","summary":[]}});
    let done = json!({"type":"response.output_item.done","output_index":0,"item":{"id":"fresh","type":"reasoning","summary":[]}});
    let (base, server) =
        start_visibility_websocket(vec![(vec![abandoned], false), (vec![fresh, done], true)]);
    let mut runtime = OpenAiProviderRuntime::default();
    let mut emitted = Vec::new();
    let error = runtime
        .invoke_response_with_event_sink(visibility_native_input(&base), |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .await
        .unwrap_err();
    assert!(
        emitted.is_empty(),
        "abandoned Added must not escape to host slots"
    );
    let details = error
        .downcast_ref::<ProviderRuntimeError>()
        .unwrap()
        .provider_details
        .as_ref()
        .unwrap();
    assert_eq!(
        details[recovery::RECOVERY_RECEIPT_METADATA_KEY]["disposition"],
        "logical_invocation_retry"
    );
    assert_eq!(
        details[recovery::RECOVERY_RECEIPT_METADATA_KEY]["commit_level"],
        "lifecycle_only"
    );
    let diagnostic = &details[recovery_diagnostics::KEY]["last_failure"];
    assert_eq!(diagnostic["close_code"], 1011);
    assert!(diagnostic["semantic_event_kind"].is_null());
    assert_eq!(diagnostic["buffered_scaffold_events"], 1);
    assert!(diagnostic["buffered_scaffold_bytes"].as_u64().unwrap() > 0);
    // The caller accepts the logical retry grant and advances the closed physical
    // generation; this is a new invocation, not a provider same-epoch reconnect.
    let mut retry_input = visibility_native_input(&base);
    retry_input
        .run_context
        .get_mut(TRANSPORT_SESSION_CONTEXT_KEY)
        .unwrap()["generation"] = json!(10);
    let output = runtime
        .invoke_response_with_event_sink(retry_input, |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .await
        .unwrap();
    assert!(!serde_json::to_string(&emitted)
        .unwrap()
        .contains("abandoned"));
    let phases: Vec<_> = emitted
        .iter()
        .filter_map(|event| match event {
            ProviderStreamEvent::OutputItem { phase, item, .. } => {
                assert_eq!(item["id"], "fresh");
                Some(*phase)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        phases,
        vec![
            ProviderOutputItemPhase::Added,
            ProviderOutputItemPhase::Done
        ]
    );
    assert_eq!(output.result.response_id.as_deref(), Some("success"));
    assert_no_replacement_connection(server);
}

#[tokio::test]
async fn empty_scaffolding_success_flushes_before_finish_even_for_generate_false() {
    let item = json!({"type":"response.output_item.added","output_index":0,"item":{"id":"empty","type":"message","role":"assistant","content":[]}});
    let part = json!({"type":"response.content_part.added","output_index":0,"item_id":"empty","content_index":0,"part":{"type":"output_text","text":"","annotations":[]}});
    let (base, server) = start_visibility_websocket(vec![(vec![item, part], true)]);
    let mut emitted = Vec::new();
    let output = OpenAiProviderRuntime::default()
        .invoke_response_with_event_sink(visibility_native_input(&base), |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        &emitted[0],
        ProviderStreamEvent::OutputItem {
            phase: ProviderOutputItemPhase::Added,
            ..
        }
    ));
    assert!(
        matches!(&emitted[1], ProviderStreamEvent::ResponsesOutputDelta { event } if event["type"] == "response.content_part.added")
    );
    assert!(matches!(
        emitted.last(),
        Some(ProviderStreamEvent::Finish { .. })
    ));
    assert_eq!(emitted, output.events);
    assert_no_replacement_connection(server);
}

#[tokio::test]
async fn meaningful_and_unknown_added_items_still_block_replay() {
    for (item, category) in [
        (
            json!({"type":"message","content":[{"type":"output_text","text":"private-canary"}]}),
            "message_added",
        ),
        (
            json!({"type":"reasoning","summary":[],"encrypted_content":"private-canary"}),
            "reasoning_added",
        ),
        (
            json!({"type":"reasoning","summary":[],"future":true}),
            "reasoning_added",
        ),
        (
            json!({"type":"function_call","call_id":"tool","name":"write","arguments":""}),
            "tool_item_added",
        ),
        (json!({"type":"future_item","content":[]}), "unknown_item"),
    ] {
        let (base, server) = start_visibility_websocket(vec![(
            vec![json!({"type":"response.output_item.added","output_index":0,"item":item})],
            false,
        )]);
        let error = OpenAiProviderRuntime::default()
            .invoke_response(visibility_native_input(&base))
            .await
            .unwrap_err();
        let diagnostic = assert_safe_terminal(&error, "semantic_failed");
        assert_eq!(diagnostic["last_failure"]["semantic_event_kind"], category);
        assert_no_replacement_connection(server);
    }
}

#[tokio::test]
async fn empty_scaffolding_does_not_make_response_failed_replayable() {
    let (base, server) = start_visibility_websocket(vec![(
        vec![
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","summary":[]}}),
            json!({"type":"response.failed","response":{"error":{"code":"invalid_request","message":"private-canary"}}}),
        ],
        false,
    )]);
    let mut emitted = Vec::new();
    let error = OpenAiProviderRuntime::default()
        .invoke_response_with_event_sink(visibility_native_input(&base), |event| {
            emitted.push(event.clone());
            Ok(())
        })
        .await
        .unwrap_err();
    assert!(emitted.is_empty());
    let diagnostics = assert_safe_terminal(&error, "semantic_failed");
    assert_eq!(diagnostics["last_failure"]["semantic_event_kind"], "other");
    assert_no_replacement_connection(server);
}

#[tokio::test]
async fn codex_control_metadata_before_proxy_close_does_not_commit_inference() {
    for kind in ["codex.rate_limits", "codex.response.metadata"] {
        let (base, server) = start_visibility_websocket(vec![(
            vec![json!({"type":kind,"headers":{"x-models-etag":"fixture"}})],
            false,
        )]);
        let error = OpenAiProviderRuntime::default()
            .invoke_response(visibility_native_input(&base))
            .await
            .unwrap_err();
        let details = error
            .downcast_ref::<ProviderRuntimeError>()
            .unwrap()
            .provider_details
            .as_ref()
            .unwrap();
        assert_eq!(
            details[recovery::RECOVERY_RECEIPT_METADATA_KEY]["disposition"],
            "logical_invocation_retry"
        );
        assert_eq!(
            details[recovery::RECOVERY_RECEIPT_METADATA_KEY]["commit_level"],
            "lifecycle_only"
        );
        assert!(
            details[recovery_diagnostics::KEY]["last_failure"]["semantic_event_kind"].is_null()
        );
        assert_no_replacement_connection(server);
    }
}

#[tokio::test]
async fn final_websocket_failure_piggybacks_real_local_release_and_rejects_same_generation() {
    let (base_url, upstream) = start_closing_websocket(false, false);
    let input = managed_closing_input(&base_url, 1);
    let mut runtime = OpenAiProviderRuntime::default();
    let error = runtime.invoke_response(input.clone()).await.unwrap_err();
    assert_safe_terminal(&error, "budget_exhausted");
    let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    let proof = &typed.provider_details.as_ref().unwrap()[TRANSPORT_SESSION_RECEIPT_METADATA_KEY];
    assert_eq!(proof["physical_state"], "closed");
    assert_eq!(proof["ttl_remaining_ms"], 0);
    assert_eq!(
        proof["closure_evidence"]["source"],
        "provider_local_release"
    );
    assert_eq!(proof["closure_evidence"]["local_released"], true);
    assert_eq!(
        proof["closure_evidence"]["identity"],
        json!({
            "logical_session_id":"logical-fixture", "generation":9, "worker_incarnation":1,
        })
    );
    assert!(runtime.websocket_sessions.is_empty());
    assert_eq!(
        typed.failure_metadata()[TRANSPORT_SESSION_RECEIPT_METADATA_KEY],
        *proof
    );
    let rejected = runtime.invoke_response(input).await.unwrap_err();
    assert!(
        rejected.to_string().contains("closed")
            || rejected.downcast_ref::<ProviderRuntimeError>().is_some()
    );
    assert_no_replacement_connection(upstream);
}

#[test]
fn native_complete_history_preserves_empty_prewarm_and_opaque_multi_turn_items() {
    let mut runtime = OpenAiProviderRuntime::default();
    let input = visibility_native_input("https://example.test/v1");
    let config = normalize_provider_config(&input.provider_config).unwrap();
    let scope = websocket_history_scope(&config, &input);
    let empty = completed_native_output(
        &json!({"type":"response.completed","response":{"status":"completed","output":[]}}),
    )
    .unwrap();
    runtime.record_native_websocket_response_chain(
        "warm",
        &json!({"input":[]}),
        Some(&empty),
        &scope,
    );
    let opaque =
        json!({"type":"reasoning","id":"rs_1","encrypted_content":"opaque-exact","summary":[]});
    let turn = json!({"previous_response_id":"warm","input":[{"role":"user","content":"one"}]});
    runtime.record_native_websocket_response_chain("turn", &turn, Some(&[opaque.clone()]), &scope);
    let next = json!({"previous_response_id":"turn","input":[{"type":"function_call_output","call_id":"c1","output":"two"}],"store":false});
    let replay = runtime
        .websocket_scoped_retry_body(&config, &input, None, "turn", &next)
        .unwrap();
    assert!(replay.get("previous_response_id").is_none());
    assert_eq!(
        replay["input"],
        json!([{"role":"user","content":"one"},opaque,{"type":"function_call_output","call_id":"c1","output":"two"}])
    );
    assert_eq!(replay["store"], false);
    let bound: ProviderRecoveryDirective = serde_json::from_value(json!({
        "policy":{"type":"native_opaque","budget":{"max_inner_attempts":3,"absolute_deadline_unix_ms":4102444800000_i64}},
        "transport_epoch":19,"initial_commit_level":"lifecycle_only",
        "cursor_provenance":{"binding":{"type":"connection_bound","transport_epoch":19,"socket_incarnation":1}}
    })).unwrap();
    assert!(runtime
        .websocket_scoped_retry_body(&config, &input, Some(&bound), "turn", &next)
        .is_none());
    let mut changed = input.clone();
    changed.model = "other-model".into();
    assert!(runtime
        .websocket_scoped_retry_body(&config, &changed, None, "turn", &next)
        .is_none());
    changed = input.clone();
    changed.provider_instance_id = "other-tenant-provider".into();
    assert!(runtime
        .websocket_scoped_retry_body(&config, &changed, None, "turn", &next)
        .is_none());
    changed = input.clone();
    changed
        .run_context
        .get_mut(TRANSPORT_SESSION_CONTEXT_KEY)
        .unwrap()["generation"] = json!(10);
    assert!(runtime
        .websocket_scoped_retry_body(&config, &changed, None, "turn", &next)
        .is_some());
    changed
        .run_context
        .get_mut(TRANSPORT_SESSION_CONTEXT_KEY)
        .unwrap()["logical_session_id"] = json!("different-host-sealed-logical-session");
    assert!(runtime
        .websocket_scoped_retry_body(&config, &changed, None, "turn", &next)
        .is_none());
}

#[test]
fn native_history_refuses_unknown_incomplete_evicted_and_oversized_chains() {
    let mut runtime = OpenAiProviderRuntime::default();
    for payload in [
        json!({"type":"response.incomplete","response":{"output":[]}}),
        json!({"type":"response.completed","response":{"status":"incomplete","output":[]}}),
        json!({"type":"response.completed","response":{"status":"completed"}}),
    ] {
        assert!(completed_native_output(&payload).is_none());
    }
    runtime.record_native_websocket_response_chain("partial", &json!({"input":[]}), None, "scope");
    runtime.record_native_websocket_response_chain(
        "orphan",
        &json!({"previous_response_id":"unknown","input":[]}),
        Some(&[]),
        "scope",
    );
    runtime.record_native_websocket_response_chain(
        "large",
        &json!({"input":[{"content":"x".repeat(NATIVE_HISTORY_MAX_ENTRY_BYTES)}]}),
        Some(&[]),
        "scope",
    );
    for id in ["unknown", "partial", "orphan", "large"] {
        assert!(runtime
            .websocket_full_context_retry_body(id, &json!({"input":[]}))
            .is_none());
    }
    runtime.record_native_websocket_response_chain(
        "evicted",
        &json!({"input":[]}),
        Some(&[]),
        "scope",
    );
    runtime.evict_websocket_response_history("evicted");
    assert!(runtime
        .websocket_full_context_retry_body("evicted", &json!({"input":[]}))
        .is_none());
}

#[tokio::test]
async fn native_completed_prewarm_rebuilds_after_host_generation_rotation() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (sent, received) = mpsc::channel();
    let server = thread::spawn(move || {
        for id in ["warm", "fresh"] {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut ws = tokio_tungstenite::tungstenite::accept(stream).unwrap();
            let body: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            sent.send(body).unwrap();
            ws.send(Message::Text(json!({"type":"response.completed","response":{"id":id,"status":"completed","output":[]}}).to_string().into())).unwrap();
        }
    });
    let mut runtime = OpenAiProviderRuntime::default();
    let mut warm = visibility_native_input(&base);
    warm.native_transport.as_mut().unwrap().wire_body =
        json!({"model":"fixture-model","input":[],"generate":false});
    let result = runtime.invoke_response(warm).await.unwrap();
    assert_eq!(result.result.response_id.as_deref(), Some("warm"));
    let mut next = visibility_native_input(&base);
    next.run_context
        .get_mut(TRANSPORT_SESSION_CONTEXT_KEY)
        .unwrap()["generation"] = json!(10);
    next.native_transport.as_mut().unwrap().wire_body = json!({"model":"fixture-model","previous_response_id":"warm","input":[{"role":"user","content":"after idle rotation"}]});
    let result = runtime.invoke_response(next).await.unwrap();
    assert_eq!(result.result.response_id.as_deref(), Some("fresh"));
    assert_eq!(
        received.recv_timeout(Duration::from_secs(5)).unwrap()["generate"],
        false
    );
    let rebuilt = received.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(rebuilt.get("previous_response_id").is_none());
    assert_eq!(
        rebuilt["input"],
        json!([{"role":"user","content":"after idle rotation"}])
    );
    server.join().unwrap();
}

#[test]
fn native_history_total_budget_evicts_oldest_and_preserves_latest_complete_chain() {
    let mut runtime = OpenAiProviderRuntime::default();
    let large_input = json!({"input":[{"role":"user","content":"x".repeat(7 * 1024 * 1024)}]});
    for index in 0..5 {
        runtime.record_native_websocket_response_chain(
            &format!("history_{index}"),
            &large_input,
            Some(&[]),
            "scope",
        );
        assert!(runtime.websocket_native_history_bytes <= NATIVE_HISTORY_MAX_TOTAL_BYTES);
        assert_eq!(
            runtime.websocket_native_history_bytes,
            runtime
                .websocket_native_history_sizes
                .values()
                .sum::<usize>()
        );
    }
    assert!(!runtime
        .websocket_chain_inputs_by_response_id
        .contains_key("history_0"));
    assert!(!runtime
        .websocket_chain_scopes_by_response_id
        .contains_key("history_0"));
    let opaque = json!({"type":"reasoning","encrypted_content":"latest-exact","summary":[]});
    runtime.record_native_websocket_response_chain(
        "latest",
        &json!({"previous_response_id":"history_4","input":[{"role":"user","content":"next"}]}),
        Some(&[opaque.clone()]),
        "scope",
    );
    let replay = runtime
        .websocket_full_context_retry_body("latest", &json!({"input":[]}))
        .unwrap();
    assert_eq!(replay["input"][0], large_input["input"][0]);
    assert_eq!(replay["input"][1], json!({"role":"user","content":"next"}));
    assert_eq!(replay["input"][2], opaque);
    assert!(runtime.websocket_native_history_bytes <= NATIVE_HISTORY_MAX_TOTAL_BYTES);
    assert!(!runtime
        .websocket_chain_inputs_by_response_id
        .contains_key("history_1"));
    assert!(!runtime
        .websocket_chain_scopes_by_response_id
        .contains_key("history_1"));
    runtime.evict_websocket_response_history("latest");
    assert_eq!(
        runtime.websocket_native_history_bytes,
        runtime
            .websocket_native_history_sizes
            .values()
            .sum::<usize>()
    );
    assert!(!runtime
        .websocket_chain_scopes_by_response_id
        .contains_key("latest"));
}
