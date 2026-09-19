use super::*;

fn identity(session: &str, generation: u64) -> CloseIdentity {
    CloseIdentity {
        logical_session_id: session.into(),
        generation,
        worker_incarnation: 7,
    }
}
fn command(identity: &CloseIdentity) -> TransportSessionCommand {
    TransportSessionCommand {
        logical_session_id: identity.logical_session_id.clone(),
        generation: identity.generation,
        worker_incarnation: Some(identity.worker_incarnation),
        action: TransportSessionAction::Close,
        deadline_unix_ms: unix_time_ms() + 5_000,
    }
}

#[test]
fn closure_retention_pins_active_records_and_refuses_capacity_without_eviction() {
    let now = Instant::now();
    let mut ledger = CloseLedger::default();
    for index in 0..CAPACITY {
        let id = identity(&format!("session-{index}"), 1);
        ledger.reserve(id.clone(), now).unwrap();
        ledger.activated(&id);
    }
    ledger.prune(now + RETENTION * 2);
    assert_eq!(ledger.entries.len(), CAPACITY);
    assert!(ledger
        .reserve(identity("overflow", 1), now + RETENTION * 2)
        .is_err());
    let id = identity("session-0", 1);
    ledger.released(&id, Some(NoAckReason::Timeout), Duration::from_secs(1), now);
    let receipt = ledger.complete(&id, &command(&id), now).unwrap();
    assert_eq!(receipt.close_acknowledged, Some(false));
    ledger.prune(now + RETENTION - Duration::from_millis(1));
    assert_eq!(ledger.completed(&id), Some(receipt));
    ledger.prune(now + RETENTION);
    assert!(ledger.completed(&id).is_none());
    assert!(ledger
        .complete(&id, &command(&id), now + RETENTION)
        .is_none());
    ledger.reserve(identity("new", 1), now + RETENTION).unwrap();
}

#[test]
fn replacement_generation_preserves_every_socket_release_and_prior_missing_ack() {
    let now = Instant::now();
    let id = identity("replace", 1);
    let mut ledger = CloseLedger::default();
    ledger.reserve(id.clone(), now).unwrap();
    ledger.activated(&id);
    ledger.released(&id, Some(NoAckReason::Timeout), Duration::from_secs(2), now);
    ledger.reserve(id.clone(), now).unwrap();
    ledger.activated(&id);
    assert!(ledger.complete(&id, &command(&id), now).is_none());
    ledger.released(&id, None, Duration::from_secs(1), now);
    let first = ledger.complete(&id, &command(&id), now).unwrap();
    assert_eq!(first.close_acknowledged, Some(false));
    assert_eq!(
        first.closure_evidence.as_ref().unwrap().no_ack_reason,
        Some(NoAckReason::Timeout)
    );
    assert_eq!(ledger.complete(&id, &command(&id), now), Some(first));
    assert!(ledger.reserve(id, now).is_err());
}

async fn connected_session(
    no_ack: bool,
) -> (ResponsesWebsocketSession, tokio::task::JoinHandle<usize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = normalize_provider_config(&json!({"api_key":"fixture", "base_url":format!("http://{}",listener.local_addr().unwrap())})).unwrap();
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let frame = socket.next().await.unwrap().unwrap();
        assert!(matches!(frame, Message::Close(_)));
        if no_ack {
            tokio::time::sleep(Duration::from_millis(150)).await;
        } else {
            let _ = socket.flush().await;
        }
        1
    });
    let session = connect_responses_websocket(
        &config,
        None,
        &RestoredProtocolContext::default(),
        1,
        Some(1),
        Instant::now(),
    )
    .await
    .unwrap();
    (session, peer)
}

#[tokio::test]
async fn close_without_ack_is_local_release_and_concurrent_duplicates_share_one_receipt() {
    let (mut session, peer) = connected_session(true).await;
    let id = identity("no-ack", 1);
    session.close_identity = Some(id.clone());
    let mut runtime = OpenAiProviderRuntime::default();
    runtime.close_worker_incarnation = Some(7);
    runtime.websocket_lifecycle_policy.close_ack_timeout = Duration::from_millis(20);
    runtime
        .close_ledger
        .reserve(id.clone(), Instant::now())
        .unwrap();
    runtime.close_ledger.activated(&id);
    runtime
        .websocket_sessions
        .insert("session-a".into(), session);
    runtime
        .websocket_logical_sessions
        .insert(id.logical_session_id.clone(), "session-a".into());
    let runtime = Arc::new(tokio::sync::Mutex::new(runtime));
    let (left, right) = tokio::join!(
        async {
            runtime
                .lock()
                .await
                .control_transport_session(command(&id))
                .await
                .unwrap()
        },
        async {
            runtime
                .lock()
                .await
                .control_transport_session(command(&id))
                .await
                .unwrap()
        },
    );
    assert_eq!(left, right);
    assert_eq!(left.close_acknowledged, Some(false));
    let evidence = left.closure_evidence.unwrap();
    assert!(evidence.local_released);
    assert_eq!(evidence.no_ack_reason, Some(NoAckReason::Timeout));
    assert_eq!(evidence.source, "provider_local_release");
    assert_eq!(peer.await.unwrap(), 1);
    let mut expired = command(&id);
    expired.deadline_unix_ms = 1;
    assert_eq!(
        runtime
            .lock()
            .await
            .control_transport_session(expired)
            .await
            .unwrap(),
        right
    );
    let mut wrong = command(&id);
    wrong.worker_incarnation = Some(8);
    assert!(runtime
        .lock()
        .await
        .control_transport_session(wrong)
        .await
        .unwrap()
        .closure_evidence
        .is_none());
}

#[tokio::test]
async fn close_unknown_unbound_and_failed_handshake_never_fabricate_peer_ack() {
    let mut runtime = OpenAiProviderRuntime::default();
    let id = identity("failed-handshake", 1);
    assert!(runtime
        .control_transport_session(command(&id))
        .await
        .unwrap()
        .closure_evidence
        .is_none());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let config =
        normalize_provider_config(&json!({"api_key":"fixture", "base_url":base_url})).unwrap();
    let input = ProviderInvocationInput {
        model: "fixture".into(),
        protocol: "openai_responses".into(),
        run_context: BTreeMap::from([(
            TRANSPORT_SESSION_CONTEXT_KEY.into(),
            json!({
                "logical_session_id":id.logical_session_id, "generation":1, "worker_incarnation":7,
                "task_id":"failed-handshake", "state":"active", "physical_deadline_unix_ms":4_102_444_800_000_i64
            }),
        )]),
        ..Default::default()
    };
    let failed = runtime
        .invoke_response_websocket(
            &config,
            &input,
            json!({"model":"fixture", "input":[]}),
            &RestoredProtocolContext::default(),
            None,
            &mut |_| Ok(()),
        )
        .await;
    assert!(failed.is_err());
    assert!(runtime.websocket_sessions.is_empty());
    let mut unbound = command(&id);
    unbound.worker_incarnation = None;
    assert!(runtime
        .control_transport_session(unbound)
        .await
        .unwrap()
        .closure_evidence
        .is_none());
    let receipt = runtime
        .control_transport_session(command(&id))
        .await
        .unwrap();
    assert_eq!(receipt.close_acknowledged, None);
    assert!(receipt.closure_evidence.unwrap().local_released);
}

struct StalledClose {
    stall_send: bool,
    dropped: Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for StalledClose {
    fn drop(&mut self) {
        self.dropped
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
impl futures_util::Stream for StalledClose {
    type Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>;
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Pending
    }
}
impl futures_util::Sink<Message> for StalledClose {
    type Error = tokio_tungstenite::tungstenite::Error;
    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), Self::Error>> {
        if self.stall_send {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(Ok(()))
        }
    }
    fn start_send(
        self: std::pin::Pin<&mut Self>,
        _: Message,
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), Self::Error>> {
        std::task::Poll::Pending
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), Self::Error>> {
        std::task::Poll::Pending
    }
}
#[tokio::test]
async fn overall_close_deadline_bounds_stalled_send_and_flush_and_drops_local_resource() {
    for stall_send in [true, false] {
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let outcome = tokio::time::timeout(
            Duration::from_secs(1),
            release_stream(
                StalledClose {
                    stall_send,
                    dropped: dropped.clone(),
                },
                Duration::from_millis(10),
            ),
        )
        .await
        .expect("whole close must finish even before ACK reading begins");
        assert_eq!(outcome, Some(NoAckReason::Timeout));
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }
}

#[tokio::test]
async fn autonomous_cleanup_keeps_release_evidence_for_later_control_without_second_close() {
    let (mut session, peer) = connected_session(false).await;
    let id = identity("autonomous", 1);
    session.close_identity = Some(id.clone());
    let mut runtime = OpenAiProviderRuntime::default();
    runtime.close_worker_incarnation = Some(7);
    runtime
        .close_ledger
        .reserve(id.clone(), Instant::now())
        .unwrap();
    runtime.close_ledger.activated(&id);
    // TTL, replacement, non-reusable completion, and fault branches use this same owner.
    runtime
        .release_session(session, Duration::from_secs(1))
        .await;
    let first = runtime
        .control_transport_session(command(&id))
        .await
        .unwrap();
    assert_eq!(first.close_acknowledged, Some(true));
    assert!(first.closure_evidence.as_ref().unwrap().local_released);
    assert_eq!(
        runtime
            .control_transport_session(command(&id))
            .await
            .unwrap(),
        first
    );
    assert_eq!(peer.await.unwrap(), 1);
}
