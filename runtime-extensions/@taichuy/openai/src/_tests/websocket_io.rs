use super::*;

async fn pair() -> (SocketOwner, WebSocketStream<TcpStream>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let client = tokio::spawn(async move {
        let (stream, _) =
            connect_async_with_config(format!("ws://{address}"), Some(config()), false)
                .await
                .unwrap();
        SocketOwner::new(stream)
    });
    let (stream, _) = listener.accept().await.unwrap();
    let server = tokio_tungstenite::accept_async_with_config(
        stream,
        Some(
            tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
                .max_message_size(None)
                .max_frame_size(None),
        ),
    )
    .await
    .unwrap();
    (client.await.unwrap(), server)
}

#[tokio::test]
async fn idle_ping_is_answered_without_invocation_and_other_socket_is_independent() {
    let (owner, mut peer) = pair().await;
    let (other, mut other_peer) = pair().await;
    peer.send(Message::Ping(vec![1, 2, 3].into()))
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), peer.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        Message::Pong(vec![1, 2, 3].into())
    );
    drop(owner);
    other_peer
        .send(Message::Ping(vec![4].into()))
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), other_peer.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        Message::Pong(vec![4].into())
    );
    drop(other);
}

#[tokio::test]
async fn active_socket_sends_ping_while_upstream_is_silent() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let client = tokio::spawn(async move {
        let (stream, _) =
            connect_async_with_config(format!("ws://{address}"), Some(config()), false)
                .await
                .unwrap();
        SocketOwner::new_with_keepalive(stream, Duration::from_millis(25))
    });
    let (stream, _) = listener.accept().await.unwrap();
    let mut peer = tokio_tungstenite::accept_async_with_config(
        stream,
        Some(
            tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
                .max_message_size(None)
                .max_frame_size(None),
        ),
    )
    .await
    .unwrap();
    let mut owner = client.await.unwrap();
    let activity = owner.activity();

    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(250), peer.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        Message::Ping(_)
    ));
    peer.send(Message::Text("upstream-output".into()))
        .await
        .unwrap();
    assert_eq!(
        owner.next().await.unwrap().unwrap(),
        Message::Text("upstream-output".into())
    );
    activity.complete();
}

// The regression tests exercise slow consumption rather than treating temporary
// mailbox saturation as a corrupt stream.
async fn wait_for_queued(owner: &SocketOwner, count: usize) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while owner.events.len() != count {
            assert!(owner.failure().is_none(), "{:?}", owner.failure());
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn event_count_backpressure_preserves_order_and_resumes() {
    let frames = (0..EVENT_COUNT * 3)
        .map(|i| Ok(Message::Text(i.to_string().into())))
        .collect();
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_reads = reads.clone();
    let stream = ScriptedSocket(frames).inspect(move |_| {
        observed_reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    let mut owner = SocketOwner::new(stream);
    wait_for_queued(&owner, EVENT_COUNT).await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while reads.load(std::sync::atomic::Ordering::SeqCst) < EVENT_COUNT + 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // A command must still progress while admission waits for queue capacity.
    tokio::time::timeout(
        Duration::from_secs(1),
        owner.send(Message::Text("command".into())),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(owner.failure().is_none());
    assert_eq!(
        reads.load(std::sync::atomic::Ordering::SeqCst),
        EVENT_COUNT + 1,
        "full queue allows only one pending frame, then stops polling the socket"
    );
    for i in 0..EVENT_COUNT * 3 {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), owner.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            Message::Text(i.to_string().into())
        );
    }
    assert!(owner.next().await.is_none());
}

#[tokio::test]
async fn close_and_cancel_remain_responsive_when_event_queue_is_full() {
    for cancel in [false, true] {
        let frames = (0..EVENT_COUNT * 3)
            .map(|_| Ok(Message::Text("x".into())))
            .collect();
        let owner = SocketOwner::new(ScriptedSocket(frames));
        let activity = owner.activity();
        wait_for_queued(&owner, EVENT_COUNT).await;
        if cancel {
            let terminal = owner.terminal.clone();
            let task = owner.task.abort_handle();
            drop(activity);
            tokio::time::timeout(Duration::from_secs(1), async {
                while !task.is_finished() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(
                terminal.lock().unwrap().failure.as_ref().unwrap().kind,
                "owner_cancelled"
            );
        } else {
            activity.complete();
            // Peer need not acknowledge; local close remains bounded while full.
            tokio::time::timeout(
                Duration::from_secs(1),
                owner.close(Duration::from_millis(20)),
            )
            .await
            .unwrap();
        }
    }
}

#[tokio::test]
async fn dropping_owner_while_backpressured_stops_pump() {
    let frames = (0..EVENT_COUNT * 3)
        .map(|_| Ok(Message::Text("x".into())))
        .collect();
    let owner = SocketOwner::new(ScriptedSocket(frames));
    wait_for_queued(&owner, EVENT_COUNT).await;
    let task = owner.task.abort_handle();
    drop(owner);
    tokio::time::timeout(Duration::from_secs(1), async {
        while !task.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn byte_window_waits_and_admits_larger_messages_after_consumption() {
    let bytes = Arc::new(Semaphore::new(BYTE_WINDOW_BYTES));
    let buffered = buffer(
        Message::Binary(vec![0; BYTE_WINDOW_BYTES + 1].into()),
        &bytes,
    )
    .await
    .unwrap();
    assert_eq!(bytes.available_permits(), 0);
    assert!(tokio::time::timeout(
        Duration::from_millis(10),
        buffer(Message::Text("x".into()), &bytes)
    )
    .await
    .is_err());
    drop(buffered);
    assert!(buffer(Message::Text("x".into()), &bytes).await.is_ok());
}

#[tokio::test]
async fn idle_close_preserves_code_and_peer_reason_and_closes_transport() {
    let (owner, mut peer) = pair().await;
    peer.send(Message::Close(Some(
        tokio_tungstenite::tungstenite::protocol::CloseFrame {
            code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Policy,
            reason: "secret-sentinel".into(),
        },
    )))
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while owner.failure().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let failure = owner.failure().unwrap();
    assert_eq!(failure.close_code, Some(1008));
    assert!(!failure.to_string().contains("secret-sentinel"));
}

#[tokio::test]
async fn drop_releases_idle_socket() {
    let (owner, mut peer) = pair().await;
    drop(owner);
    let message = tokio::time::timeout(Duration::from_secs(1), peer.next())
        .await
        .unwrap();
    assert!(!matches!(message, Some(Ok(Message::Text(_)))));
}

#[tokio::test]
async fn command_count_backpressure_preserves_all_writes() {
    let (owner, mut peer) = pair().await;
    // No yielding between filling the mailbox and admission: the owner cannot consume.
    let mut replies = Vec::new();
    for _ in 0..COMMAND_COUNT {
        let (ack, reply) = oneshot::channel();
        replies.push(reply);
        owner
            .commands
            .try_send(Command {
                buffered: buffer(Message::Text("x".into()), &owner.command_bytes)
                    .await
                    .unwrap(),
                ack,
            })
            .unwrap();
    }
    tokio::time::timeout(
        Duration::from_secs(2),
        owner.send(Message::Text("after-full".into())),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(owner.failure().is_none());
    for reply in replies {
        reply.await.unwrap().unwrap();
    }
    for _ in 0..COMMAND_COUNT {
        assert_eq!(
            peer.next()
                .await
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap()
                .as_str(),
            "x"
        );
    }
    assert_eq!(
        peer.next()
            .await
            .unwrap()
            .unwrap()
            .into_text()
            .unwrap()
            .as_str(),
        "after-full"
    );
}

#[tokio::test]
async fn cumulative_event_bytes_apply_backpressure_and_resume() {
    let payload = vec![0; BYTE_WINDOW_BYTES / 2 + 1];
    let mut owner = SocketOwner::new(ScriptedSocket(std::collections::VecDeque::from([
        Ok(Message::Binary(payload.clone().into())),
        Ok(Message::Binary(payload.clone().into())),
        Ok(completed_frame()),
    ])));
    wait_for_queued(&owner, 1).await;
    tokio::time::timeout(
        Duration::from_secs(1),
        owner.send(Message::Text("command".into())),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(owner.events.len(), 1);
    assert!(owner.failure().is_none());
    for _ in 0..2 {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), owner.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            Message::Binary(payload.clone().into())
        );
    }
    assert_eq!(owner.next().await.unwrap().unwrap(), completed_frame());
    assert!(owner.next().await.is_none());
}

#[tokio::test]
async fn dropped_active_consumer_invalidates_socket_and_keeps_first_failure() {
    let (owner, mut peer) = pair().await;
    let activity = owner.activity();
    drop(activity);
    assert_eq!(owner.failure().unwrap().kind, "owner_cancelled");
    assert_eq!(owner.failure().unwrap().phase, "active");
    assert!(owner
        .send(Message::Text("must-not-reuse".into()))
        .await
        .is_err());
    tokio::time::timeout(Duration::from_secs(1), peer.next())
        .await
        .unwrap();
}

#[tokio::test]
async fn completed_activity_marks_next_failure_idle_and_new_owner_isolated() {
    let (old, mut peer) = pair().await;
    old.activity().complete();
    peer.send(Message::Close(None)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while old.failure().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(old.failure().unwrap().phase, "idle");
    let (new, _) = pair().await;
    assert!(new.failure().is_none());
}

#[tokio::test]
async fn idle_session_ping_progresses_while_another_session_has_active_output() {
    let (idle, mut idle_peer) = pair().await;
    let (mut active, mut active_peer) = pair().await;
    let activity = active.activity();
    active_peer
        .send(Message::Text("active-output".into()))
        .await
        .unwrap();
    idle_peer.send(Message::Ping(vec![9].into())).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), idle_peer.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        Message::Pong(vec![9].into())
    );
    assert_eq!(
        active.next().await.unwrap().unwrap(),
        Message::Text("active-output".into())
    );
    activity.complete();
    assert!(idle.failure().is_none());
}

#[test]
fn owner_network_diagnostic_preserves_enum_without_raw_error_text() {
    let failure = Failure::websocket(WebSocketError::Io(std::io::Error::new(
        std::io::ErrorKind::ConnectionReset,
        "private-routing-sentinel",
    )));
    let diagnostic = recovery_diagnostics::failure(&failure.into(), Some(8), Some(7));
    assert_eq!(diagnostic["websocket_error_kind"], "io");
    assert_eq!(diagnostic["io_error_kind"], "connection_reset");
    assert!(!diagnostic.to_string().contains("sentinel"));
}

// Deterministic peer order: run the physical reader to its terminal state before
// allowing the application to consume any queued frames.
struct ScriptedSocket(std::collections::VecDeque<Result<Message, WebSocketError>>);
impl futures_util::Stream for ScriptedSocket {
    type Item = Result<Message, WebSocketError>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(self.0.pop_front())
    }
}
impl futures_util::Sink<Message> for ScriptedSocket {
    type Error = WebSocketError;
    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn start_send(self: std::pin::Pin<&mut Self>, _: Message) -> Result<(), Self::Error> {
        Ok(())
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}
fn completed_frame() -> Message {
    Message::Text(
        r#"{"type":"response.completed","response":{"id":"ordered-response","output":[]}}"#.into(),
    )
}

#[tokio::test]
async fn received_completion_precedes_later_io_protocol_close_and_eof() {
    let endings = [
        Some(Err(WebSocketError::Io(std::io::Error::from(
            std::io::ErrorKind::ConnectionReset,
        )))),
        Some(Err(WebSocketError::Protocol(
            tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
        ))),
        Some(Ok(Message::Close(None))),
        None,
    ];
    for ending in endings {
        let expects_close = matches!(&ending, Some(Ok(Message::Close(_))));
        let expects_eof = ending.is_none();
        let mut frames = std::collections::VecDeque::from([Ok(completed_frame())]);
        if let Some(ending) = ending {
            frames.push_back(ending);
        }
        let mut owner = SocketOwner::new(ScriptedSocket(frames));
        (&mut owner.task).await.unwrap();
        let first_kind = owner.failure().unwrap().kind;
        assert_eq!(owner.next().await.unwrap().unwrap(), completed_frame());
        let terminal = owner.next().await;
        if expects_close {
            assert!(matches!(terminal, Some(Ok(Message::Close(_)))));
        } else if expects_eof {
            assert!(terminal.is_none());
        } else {
            assert!(terminal.unwrap().is_err());
        }
        assert_eq!(
            owner.failure().unwrap().kind,
            first_kind,
            "delivery must retain the physical first failure"
        );
        assert!(owner
            .send(Message::Text("must-not-reuse".into()))
            .await
            .is_err());
    }
}

#[tokio::test]
async fn queued_completion_and_large_frame_are_delivered_before_eof() {
    let mut owner = SocketOwner::new(ScriptedSocket(std::collections::VecDeque::from([
        Ok(completed_frame()),
        Ok(Message::Binary(vec![0; BYTE_WINDOW_BYTES + 1].into())),
    ])));
    assert_eq!(owner.next().await.unwrap().unwrap(), completed_frame());
    let frame = owner.next().await.unwrap().unwrap();
    assert_eq!(frame.len(), BYTE_WINDOW_BYTES + 1);
    assert!(owner.next().await.is_none());
}

#[tokio::test]
async fn upstream_websocket_accepts_a_large_request_and_response_without_reconnect() {
    let (mut owner, mut peer) = pair().await;
    let payload = "x".repeat(16 * 1024 * 1024 + 1);
    let expected = payload.clone();
    let peer_task = tokio::spawn(async move {
        let received = peer.next().await.unwrap().unwrap();
        assert_eq!(received.into_text().unwrap().as_str(), expected.as_str());
        peer.send(Message::Text(expected.into())).await.unwrap();
    });
    owner
        .send(Message::Text(payload.clone().into()))
        .await
        .unwrap();
    assert_eq!(
        owner
            .next()
            .await
            .unwrap()
            .unwrap()
            .into_text()
            .unwrap()
            .as_str(),
        payload.as_str()
    );
    peer_task.await.unwrap();
}

#[tokio::test]
async fn close_follows_tool_frames_and_retains_original_policy_details() {
    let tool = Message::Text(
        r#"{"type":"response.function_call_arguments.done","call_id":"call_1","arguments":"{}"}"#
            .into(),
    );
    let mut owner = SocketOwner::new(ScriptedSocket(std::collections::VecDeque::from([
        Ok(tool.clone()),
        Ok(Message::Close(Some(
            tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Policy,
                reason: "upstream continuation connection is unavailable secret-sentinel".into(),
            },
        ))),
    ])));
    (&mut owner.task).await.unwrap();
    assert_eq!(owner.next().await.unwrap().unwrap(), tool);
    let Some(Ok(Message::Close(Some(frame)))) = owner.next().await else {
        panic!("peer close frame was lost");
    };
    assert_eq!(
        frame.reason.as_str(),
        "upstream continuation connection is unavailable secret-sentinel"
    );
    let failure = owner.failure().unwrap();
    assert_eq!(failure.close_code, Some(1008));
    assert_eq!(failure.category, "continuation_unavailable");
    assert!(!failure.to_string().contains("sentinel"));
    let diagnostic = recovery_diagnostics::failure(&failure.into(), None, None);
    assert_eq!(diagnostic["close_reason"], frame.reason.as_str());
}

// An open peer whose writes succeed independently of inbound progress. Unlike
// ScriptedSocket, an empty input remains open instead of manufacturing EOF.
struct LivenessSocket {
    incoming: mpsc::UnboundedReceiver<Result<Message, WebSocketError>>,
    sent: mpsc::UnboundedSender<Message>,
    ping_delay: Duration,
    flushing: Option<std::pin::Pin<Box<tokio::time::Sleep>>>,
}
impl futures_util::Stream for LivenessSocket {
    type Item = Result<Message, WebSocketError>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.incoming.poll_recv(cx)
    }
}
impl futures_util::Sink<Message> for LivenessSocket {
    type Error = WebSocketError;
    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn start_send(mut self: std::pin::Pin<&mut Self>, message: Message) -> Result<(), Self::Error> {
        if matches!(message, Message::Ping(_)) && !self.ping_delay.is_zero() {
            self.flushing = Some(Box::pin(tokio::time::sleep(self.ping_delay)));
        }
        let _ = self.sent.send(message);
        Ok(())
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        if let Some(flushing) = self.flushing.as_mut() {
            if std::future::Future::poll(flushing.as_mut(), cx).is_pending() {
                return std::task::Poll::Pending;
            }
        }
        self.flushing = None;
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}
fn liveness_owner(
    ping_delay: Duration,
) -> (
    SocketOwner,
    mpsc::UnboundedSender<Result<Message, WebSocketError>>,
    mpsc::UnboundedReceiver<Message>,
) {
    let (input, incoming) = mpsc::unbounded_channel();
    let (sent, output) = mpsc::unbounded_channel();
    let owner = SocketOwner::new_with_keepalive(
        LivenessSocket {
            incoming,
            sent,
            ping_delay,
            flushing: None,
        },
        KEEPALIVE_INTERVAL,
    );
    (owner, input, output)
}
async fn settle_owner() {
    // Let the bounded owner process its runnable work without auto-advancing
    // paused time by awaiting a timer or an as-yet absent peer message.
    for _ in 0..(EVENT_COUNT * 4) {
        tokio::task::yield_now().await;
    }
}
async fn elapse(duration: Duration) {
    tokio::time::advance(duration).await;
    settle_owner().await;
}
fn expect_ping(output: &mut mpsc::UnboundedReceiver<Message>) {
    assert!(matches!(output.try_recv().unwrap(), Message::Ping(_)));
}

#[tokio::test(start_paused = true)]
async fn successful_ping_writes_do_not_hide_receive_silence() {
    let (owner, _input, mut output) = liveness_owner(Duration::ZERO);
    settle_owner().await;
    elapse(KEEPALIVE_INTERVAL).await;
    expect_ping(&mut output);
    assert!(owner.failure().is_none());
    elapse(KEEPALIVE_INTERVAL - Duration::from_millis(1)).await;
    assert!(owner.failure().is_none());
    elapse(Duration::from_millis(1)).await;
    let failure = owner.failure().expect("silent peer must fail");
    assert_eq!(failure.kind, "read_liveness_timeout");
    assert_eq!(failure.category, "transport_disconnected");
    assert_eq!(failure.io_kind, Some("timed_out"));
}

#[tokio::test(start_paused = true)]
async fn timely_pongs_allow_active_inference_for_many_windows() {
    let (owner, input, mut output) = liveness_owner(Duration::ZERO);
    let activity = owner.activity();
    settle_owner().await;
    for _ in 0..12 {
        elapse(KEEPALIVE_INTERVAL).await;
        expect_ping(&mut output);
        input.send(Ok(Message::Pong(Vec::new().into()))).unwrap();
        settle_owner().await;
        assert!(owner.failure().is_none());
        assert!(owner.events.is_empty(), "Pongs are not application output");
    }
    activity.complete();
}

#[tokio::test(start_paused = true)]
async fn ready_pong_precedes_simultaneously_ready_probe_deadline() {
    let (owner, input, mut output) = liveness_owner(Duration::ZERO);
    settle_owner().await;
    elapse(KEEPALIVE_INTERVAL).await;
    expect_ping(&mut output);
    // No await between enqueue and advance: owner next observes both ready.
    input.send(Ok(Message::Pong(Vec::new().into()))).unwrap();
    elapse(KEEPALIVE_INTERVAL).await;
    assert!(owner.failure().is_none());
    elapse(KEEPALIVE_INTERVAL).await;
    assert!(
        owner.failure().is_none(),
        "tie Pong must clear the old probe"
    );
}

#[tokio::test(start_paused = true)]
async fn slow_successful_ping_gets_a_full_window_after_flush() {
    let (owner, _input, mut output) = liveness_owner(Duration::from_secs(9));
    settle_owner().await;
    elapse(KEEPALIVE_INTERVAL).await;
    expect_ping(&mut output);
    elapse(Duration::from_secs(9)).await; // Successful first flush at t=29.
    elapse(Duration::from_secs(11)).await; // Old interval phase t=40.
    assert!(owner.failure().is_none());
    elapse(Duration::from_secs(8)).await;
    assert!(
        owner.failure().is_none(),
        "only 19 seconds since successful Ping"
    );
    elapse(Duration::from_secs(1)).await;
    assert_eq!(owner.failure().unwrap().kind, "read_liveness_timeout");
}

#[tokio::test(start_paused = true)]
async fn backpressure_suspends_probe_and_resume_grants_a_fresh_window() {
    let (mut owner, input, mut output) = liveness_owner(Duration::ZERO);
    settle_owner().await;
    elapse(KEEPALIVE_INTERVAL).await;
    expect_ping(&mut output);
    for i in 0..=EVENT_COUNT {
        input.send(Ok(Message::Text(i.to_string().into()))).unwrap();
    }
    settle_owner().await;
    assert_eq!(owner.events.len(), EVENT_COUNT);
    for _ in 0..5 {
        elapse(KEEPALIVE_INTERVAL).await;
        expect_ping(&mut output);
        assert!(owner.failure().is_none(), "local admission disables reads");
    }
    for i in 0..=EVENT_COUNT {
        assert_eq!(
            owner.next().await.unwrap().unwrap(),
            Message::Text(i.to_string().into())
        );
    }
    settle_owner().await;
    assert!(owner.failure().is_none());
    elapse(KEEPALIVE_INTERVAL).await;
    expect_ping(&mut output);
    elapse(KEEPALIVE_INTERVAL - Duration::from_millis(1)).await;
    assert!(
        owner.failure().is_none(),
        "resumption needs a full fresh window"
    );
    elapse(Duration::from_millis(1)).await;
    assert_eq!(owner.failure().unwrap().kind, "read_liveness_timeout");
}

#[tokio::test(start_paused = true)]
async fn queued_completion_is_delivered_before_receive_timeout() {
    let (mut owner, input, _output) = liveness_owner(Duration::ZERO);
    input.send(Ok(completed_frame())).unwrap();
    settle_owner().await;
    assert_eq!(owner.events.len(), 1);
    elapse(KEEPALIVE_INTERVAL).await;
    elapse(KEEPALIVE_INTERVAL).await;
    assert_eq!(owner.failure().unwrap().kind, "read_liveness_timeout");
    assert_eq!(owner.next().await.unwrap().unwrap(), completed_frame());
    assert!(owner.next().await.unwrap().is_err());
}

#[tokio::test(start_paused = true)]
async fn stalled_ping_keeps_existing_write_timeout() {
    let (owner, _input, mut output) = liveness_owner(WRITE_TIMEOUT + Duration::from_secs(1));
    settle_owner().await;
    elapse(KEEPALIVE_INTERVAL).await;
    expect_ping(&mut output);
    elapse(WRITE_TIMEOUT - Duration::from_millis(1)).await;
    assert!(owner.failure().is_none());
    elapse(Duration::from_millis(1)).await;
    assert_eq!(owner.failure().unwrap().kind, "write_timeout");
}

#[tokio::test(start_paused = true)]
async fn application_frame_clears_probe_and_is_delivered_once() {
    let (mut owner, input, mut output) = liveness_owner(Duration::ZERO);
    settle_owner().await;
    elapse(KEEPALIVE_INTERVAL).await;
    expect_ping(&mut output);
    elapse(Duration::from_secs(19)).await;
    input.send(Ok(Message::Text("progress".into()))).unwrap();
    settle_owner().await;
    assert_eq!(
        owner.next().await.unwrap().unwrap(),
        Message::Text("progress".into())
    );
    elapse(Duration::from_secs(1)).await;
    assert!(
        owner.failure().is_none(),
        "inbound data cleared the expiring probe"
    );
    expect_ping(&mut output);
    assert!(owner.events.is_empty());
    elapse(KEEPALIVE_INTERVAL - Duration::from_millis(1)).await;
    assert!(owner.failure().is_none());
    elapse(Duration::from_millis(1)).await;
    assert_eq!(owner.failure().unwrap().kind, "read_liveness_timeout");
}

#[tokio::test(start_paused = true)]
async fn byte_backpressure_suspends_receive_timeout() {
    let (mut owner, input, mut output) = liveness_owner(Duration::ZERO);
    let payload = Message::Binary(vec![0; BYTE_WINDOW_BYTES / 2 + 1].into());
    settle_owner().await;
    elapse(KEEPALIVE_INTERVAL).await;
    expect_ping(&mut output);
    input.send(Ok(payload.clone())).unwrap();
    input.send(Ok(payload.clone())).unwrap();
    settle_owner().await;
    assert_eq!(owner.events.len(), 1);
    for _ in 0..3 {
        elapse(KEEPALIVE_INTERVAL).await;
        expect_ping(&mut output);
        assert!(owner.failure().is_none());
    }
    assert_eq!(owner.next().await.unwrap().unwrap(), payload);
    settle_owner().await;
    assert_eq!(owner.next().await.unwrap().unwrap(), payload);
    settle_owner().await;
    elapse(KEEPALIVE_INTERVAL).await;
    expect_ping(&mut output);
    elapse(KEEPALIVE_INTERVAL - Duration::from_millis(1)).await;
    assert!(owner.failure().is_none());
    elapse(Duration::from_millis(1)).await;
    assert_eq!(owner.failure().unwrap().kind, "read_liveness_timeout");
}
