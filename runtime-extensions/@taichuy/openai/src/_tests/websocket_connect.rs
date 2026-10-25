//! Establishment owns one transport budget; established inference has no such timer.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn assert_setup_timeout(error: &anyhow::Error) {
    let diagnostic = recovery_diagnostics::failure(error, None, None);
    assert_eq!(diagnostic["kind"], "websocket_connect_timeout");
    assert_eq!(diagnostic["reason_category"], "transport_disconnected");
    let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    let serialized = serde_json::to_value(typed).unwrap();
    assert_eq!(
        serialized["provider_details"]["1flowbase_transport_failure"]["kind"],
        "websocket_connect_timeout"
    );
    assert_eq!(
        serialized["provider_details"]["1flowbase_transport_failure"]["reason_category"],
        "transport_disconnected"
    );
    let routed =
        WebsocketInvocationError::connect_unavailable(recovery_diagnostics::transport_error(
            "websocket_connect_timeout",
            "transport_disconnected",
            None,
        ));
    assert_eq!(
        recovery_diagnostics::failure(&routed.source, None, None)["kind"],
        "websocket_connect_timeout"
    );
}

#[tokio::test(start_paused = true)]
async fn establishment_stages_share_one_transport_budget() {
    let started = tokio::time::Instant::now();
    let stages = Arc::new(AtomicUsize::new(0));
    let observed = stages.clone();
    let error = bounded_websocket_establishment(None, async move {
        for _ in 0..3 {
            tokio::time::sleep(Duration::from_secs(6)).await;
            observed.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    })
    .await
    .unwrap_err();
    assert_setup_timeout(&error);
    assert_eq!(started.elapsed(), websocket_io::WRITE_TIMEOUT);
    assert_eq!(stages.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn nearer_request_deadline_bounds_establishment() {
    let started = tokio::time::Instant::now();
    let error = bounded_websocket_establishment(
        Some(close::unix_time_ms() + 2_000),
        std::future::pending::<Result<()>>(),
    )
    .await
    .unwrap_err();
    assert_setup_timeout(&error);
    assert!(started.elapsed() <= Duration::from_secs(2));
    assert!(started.elapsed() > Duration::from_secs(1));
}

#[tokio::test(start_paused = true)]
async fn expired_request_never_polls_establishment() {
    let polls = AtomicUsize::new(0);
    let establish = std::future::poll_fn(|_| {
        polls.fetch_add(1, Ordering::SeqCst);
        std::task::Poll::Ready(Ok(()))
    });
    let error = bounded_websocket_establishment(Some(0), establish)
        .await
        .unwrap_err();
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    let diagnostic = recovery_diagnostics::failure(&error, None, None);
    assert_eq!(diagnostic["kind"], "attempt_admission");
    assert_eq!(diagnostic["reason_category"], "deadline_exceeded");
}

#[derive(Debug)]
struct SetupResource(Arc<AtomicBool>);
impl Drop for SetupResource {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn success_at_establishment_deadline_is_rejected_and_dropped() {
    let dropped = Arc::new(AtomicBool::new(false));
    let resource = SetupResource(dropped.clone());
    let result = bounded_websocket_establishment(None, async {
        // The inner future is ready at exactly D. This tests actual monotonic
        // boundary readiness rather than enqueueing a message before advancing.
        tokio::time::sleep(websocket_io::WRITE_TIMEOUT).await;
        Ok(resource)
    })
    .await;
    assert_setup_timeout(&result.unwrap_err());
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test(start_paused = true)]
async fn genuine_error_ready_at_deadline_retains_its_classification() {
    let original = recovery_diagnostics::transport_error(
        "websocket_handshake",
        "authorization_rejected",
        Some(403),
    );
    let expected = recovery_diagnostics::failure(&original, None, None);
    let error = bounded_websocket_establishment(None, async {
        tokio::time::sleep(websocket_io::WRITE_TIMEOUT).await;
        Err::<(), _>(original)
    })
    .await
    .unwrap_err();
    assert_eq!(recovery_diagnostics::failure(&error, None, None), expected);
}

#[tokio::test(start_paused = true)]
async fn earlier_success_and_genuine_error_keep_original_results() {
    let dropped = Arc::new(AtomicBool::new(false));
    let resource = SetupResource(dropped.clone());
    let accepted = bounded_websocket_establishment(None, async {
        tokio::time::sleep(websocket_io::WRITE_TIMEOUT - Duration::from_secs(1)).await;
        Ok(resource)
    })
    .await
    .unwrap();
    tokio::time::sleep(websocket_io::WRITE_TIMEOUT * 4).await;
    assert!(
        !dropped.load(Ordering::SeqCst),
        "setup timer must not survive acceptance"
    );
    drop(accepted);
    assert!(dropped.load(Ordering::SeqCst));

    let originals = [
        recovery_diagnostics::transport_error(
            "websocket_handshake",
            "authorization_rejected",
            Some(403),
        ),
        recovery_diagnostics::network_error(&std::io::Error::from(
            std::io::ErrorKind::ConnectionRefused,
        )),
    ];
    for original in originals {
        let expected = recovery_diagnostics::failure(&original, None, None);
        let error = bounded_websocket_establishment(None, async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Err::<(), _>(original)
        })
        .await
        .unwrap_err();
        assert_eq!(recovery_diagnostics::failure(&error, None, None), expected);
    }
}

fn setup_config(base_url: String, proxy_url: Option<String>) -> ProviderConfig {
    ProviderConfig {
        base_url,
        api_key: "sk-connect-test".into(),
        organization: None,
        project: None,
        validate_model: false,
        transport_mode: OpenAiTransportMode::ResponsesWebsocket,
        proxy_url,
    }
}

async fn read_headers(stream: &mut TcpStream) -> String {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            let byte = stream.read_u8().await.unwrap();
            bytes.push(byte);
            assert!(bytes.len() < 16_384);
        }
        String::from_utf8(bytes).unwrap()
    })
    .await
    .expect("peer should observe the requested handshake phase")
}

async fn assert_peer_eof(mut peer: TcpStream) {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut bytes = [0; 4096];
        loop {
            match peer.read(&mut bytes).await {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("unexpected peer cleanup error: {error}"),
            }
        }
    })
    .await
    .expect("rejected setup must release its owned transport");
}

fn start_setup(
    config: ProviderConfig,
) -> tokio::task::JoinHandle<Result<ResponsesWebsocketSession>> {
    tokio::spawn(async move {
        connect_responses_websocket(
            &config,
            None,
            &RestoredProtocolContext::default(),
            1,
            None,
            Instant::now(),
            None,
        )
        .await
    })
}

async fn expire_setup(
    setup: tokio::task::JoinHandle<Result<ResponsesWebsocketSession>>,
    peer: TcpStream,
) {
    // All real TCP/request observations happened with time running. The setup
    // timer already exists; advancing ten seconds expires that same deadline.
    tokio::time::pause();
    tokio::time::advance(websocket_io::WRITE_TIMEOUT).await;
    let completed = tokio::time::timeout(Duration::from_secs(1), setup)
        .await
        .expect("silent establishment must settle within the shared budget");
    let error = match completed.unwrap() {
        Ok(_) => panic!("silent establishment should not construct an owner"),
        Err(error) => error,
    };
    assert_setup_timeout(&error);
    tokio::time::resume();
    assert_peer_eof(peer).await;
}

#[tokio::test]
async fn silent_direct_upgrade_times_out_and_drops_tcp() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = setup_config(
        format!("http://{}/v1", listener.local_addr().unwrap()),
        None,
    );
    let setup = start_setup(config);
    let (mut peer, _) = listener.accept().await.unwrap();
    let request = read_headers(&mut peer).await;
    assert!(request.starts_with("GET /v1/responses HTTP/1.1\r\n"));
    assert!(!request.contains("response.create"));
    expire_setup(setup, peer).await;
}

#[tokio::test]
async fn silent_direct_tls_times_out_and_drops_tcp() {
    // Library tests bypass main(), which selects ring when the SDK also enables
    // aws-lc. Another test may already have installed the process-wide provider.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = setup_config(
        format!("https://{}/v1", listener.local_addr().unwrap()),
        None,
    );
    let setup = start_setup(config);
    let (mut peer, _) = listener.accept().await.unwrap();
    let first = tokio::time::timeout(Duration::from_secs(2), peer.read_u8())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        first, 0x16,
        "TLS ClientHello must precede the stalled TLS phase"
    );
    expire_setup(setup, peer).await;
}

#[tokio::test]
async fn silent_proxy_connect_times_out_and_drops_tunnel() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = setup_config(
        "http://127.0.0.1:9/v1".into(),
        Some(format!("http://{}", listener.local_addr().unwrap())),
    );
    let setup = start_setup(config);
    let (mut peer, _) = listener.accept().await.unwrap();
    let request = read_headers(&mut peer).await;
    assert!(request.starts_with("CONNECT 127.0.0.1:9 HTTP/1.1\r\n"));
    expire_setup(setup, peer).await;
}

#[tokio::test]
async fn proxy_connect_success_then_silent_upgrade_uses_same_deadline() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = setup_config(
        "http://127.0.0.1:9/v1".into(),
        Some(format!("http://{}", listener.local_addr().unwrap())),
    );
    let setup = start_setup(config);
    let (mut peer, _) = listener.accept().await.unwrap();
    assert!(read_headers(&mut peer).await.starts_with("CONNECT "));
    peer.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .unwrap();
    assert!(read_headers(&mut peer)
        .await
        .starts_with("GET /v1/responses HTTP/1.1\r\n"));
    expire_setup(setup, peer).await;
}
