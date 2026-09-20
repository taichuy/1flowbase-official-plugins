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
async fn event_count_overflow_retains_terminal_outside_full_queue() {
    let (mut owner, mut peer) = pair().await;
    for _ in 0..=EVENT_COUNT {
        peer.send(Message::Text("x".into())).await.unwrap();
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        while owner.failure().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(owner.failure().unwrap().kind, "queue_count_limit");
    assert!(owner
        .next()
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("queue_count_limit"));
    assert!(owner.send(Message::Text("later".into())).await.is_err());
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
async fn event_bytes_overflow_fails_even_below_count_limit() {
    let (owner, mut peer) = pair().await;
    let payload = vec![0; BYTE_LIMIT / 2 + 1];
    peer.send(Message::Binary(payload.clone().into()))
        .await
        .unwrap();
    peer.send(Message::Binary(payload.into())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while owner.failure().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(owner.failure().unwrap().kind, "queue_bytes_limit");
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
