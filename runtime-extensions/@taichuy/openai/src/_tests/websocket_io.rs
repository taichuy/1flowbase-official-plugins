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
    let server = tokio_tungstenite::accept_async(stream).await.unwrap();
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
    let mut peer = tokio_tungstenite::accept_async(stream).await.unwrap();
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

#[test]
fn byte_budget_covers_cumulative_payload_and_releases_on_consumption() {
    let bytes = Arc::new(Semaphore::new(BYTE_LIMIT));
    let buffered = buffer(Message::Binary(vec![0; BYTE_LIMIT].into()), &bytes).unwrap();
    assert_eq!(
        buffer(Message::Text("x".into()), &bytes).unwrap_err().kind,
        "queue_bytes_limit"
    );
    drop(buffered);
    assert!(buffer(Message::Text("x".into()), &bytes).is_ok());
    assert!(buffer(Message::Binary(vec![0; BYTE_LIMIT + 1].into()), &bytes).is_err());
}

#[tokio::test]
async fn idle_close_keeps_code_without_peer_reason_and_closes_transport() {
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
async fn command_count_overflow_invalidates_without_waiting_for_owner() {
    let (owner, _peer) = pair().await;
    // No await between filling the mailbox and admission: the owner cannot consume.
    let mut replies = Vec::new();
    for _ in 0..COMMAND_COUNT {
        let (ack, reply) = oneshot::channel();
        replies.push(reply);
        owner
            .commands
            .try_send(Command {
                buffered: buffer(Message::Text("x".into()), &owner.command_bytes).unwrap(),
                ack,
            })
            .unwrap();
    }
    let error = owner
        .send(Message::Text("overflow".into()))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("queue_count_limit"));
    assert_eq!(owner.failure().unwrap().kind, "queue_count_limit");
}

#[tokio::test]
async fn cumulative_event_bytes_apply_backpressure_and_resume() {
    let payload = vec![0; BYTE_LIMIT / 2 + 1];
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
async fn queued_completion_cannot_hide_single_oversized_frame() {
    let mut owner = SocketOwner::new(ScriptedSocket(std::collections::VecDeque::from([
        Ok(completed_frame()),
        Ok(Message::Binary(vec![0; BYTE_LIMIT + 1].into())),
    ])));
    (&mut owner.task).await.unwrap();
    assert_eq!(owner.failure().unwrap().kind, "queue_bytes_limit");
    assert!(owner
        .next()
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("queue_bytes_limit"));
}

#[tokio::test]
async fn close_follows_tool_frames_and_retains_safe_policy_details() {
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
    assert!(matches!(owner.next().await, Some(Ok(Message::Close(_)))));
    let failure = owner.failure().unwrap();
    assert_eq!(failure.close_code, Some(1008));
    assert_eq!(failure.category, "continuation_unavailable");
    assert!(!failure.to_string().contains("sentinel"));
}
