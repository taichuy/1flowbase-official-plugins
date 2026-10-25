use super::*;
use runtime_extension_sdk::{MultiplexEnvelope, MultiplexHostMessage, MultiplexWorkerMessage};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn send(w: &mut (impl tokio::io::AsyncWrite + Unpin), message: MultiplexHostMessage) {
    let bytes = format!(
        "{}\n",
        serde_json::to_string(&MultiplexEnvelope::new(message)).unwrap()
    );
    w.write_all(bytes.as_bytes()).await.unwrap();
}
async fn read(r: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> MultiplexWorkerMessage {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(3), r.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str::<MultiplexEnvelope<MultiplexWorkerMessage>>(&line)
        .unwrap()
        .message
}
fn request(handler: &str, payload: Value) -> Value {
    json!({"input":{"handler":format!("browser-obscura.{handler}"),"input_payload":payload,
        "call_context":{"actor_id":"actor","workspace_id":"workspace","deadline_unix_ms":now()+10000}}})
}
#[tokio::test(flavor = "current_thread")]
async fn sdk_lease_released_on_explicit_close_and_idle_expiry() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (host, worker) = tokio::io::duplex(65536);
            let (worker_read, worker_write) = tokio::io::split(worker);
            let (host_read, mut host_write) = tokio::io::split(host);
            let service = Service::new();
            let inspect = service.clone();
            let runner = tokio::task::spawn_local(runtime_extension_sdk::serve_io(
                worker_read,
                worker_write,
                move |r, e| {
                    let service = service.clone();
                    async move { service.dispatch(r, e).await }
                },
            ));
            let mut reader = BufReader::new(host_read);
            send(
                &mut host_write,
                MultiplexHostMessage::WorkerCapabilities {
                    capabilities: vec!["runtime_worker_retention/v1".into()],
                },
            )
            .await;
            for i in 0..2 {
                send(
                    &mut host_write,
                    MultiplexHostMessage::Call {
                        call_id: format!("{}", i * 2 + 1),
                        request: request(
                            "browsers.create",
                            json!({"body":{"name":"Lease","idle_timeout_ms":1000}}),
                        ),
                    },
                )
                .await;
                assert!(matches!(
                    read(&mut reader).await,
                    MultiplexWorkerMessage::ResourceRetention {
                        retained_resources: 1,
                        ..
                    }
                ));
                let created = match read(&mut reader).await {
                    MultiplexWorkerMessage::Response { response, .. } => response,
                    _ => panic!("expected response"),
                };
                assert_eq!(created["result"]["success"], true);
                let id = created["result"]["browser"]["browser_id"].as_str().unwrap();
                if i == 0 {
                    send(
                        &mut host_write,
                        MultiplexHostMessage::Call {
                            call_id: "2".into(),
                            request: request("browsers.close", json!({"path":{"browser_id":id}})),
                        },
                    )
                    .await;
                    // The SDK may flush the resource release before or after Response.
                    let mut release = false;
                    let mut response = false;
                    for _ in 0..2 {
                        match read(&mut reader).await {
                            MultiplexWorkerMessage::ResourceRetention {
                                retained_resources: 0,
                                ..
                            } => release = true,
                            MultiplexWorkerMessage::Response { response: r, .. } => {
                                assert_eq!(r["result"]["closed"], true);
                                response = true
                            }
                            other => panic!("unexpected {other:?}"),
                        }
                    }
                    assert!(release && response);
                } else {
                    assert!(matches!(
                        read(&mut reader).await,
                        MultiplexWorkerMessage::ResourceRetention {
                            retained_resources: 0,
                            ..
                        }
                    ));
                    assert!(inspect.state.borrow().browsers.is_empty());
                }
            }
            host_write.shutdown().await.unwrap();
            assert!(runner.await.unwrap().is_ok());
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn sdk_fails_closed_without_context_or_retention_capability() {
    tokio::task::LocalSet::new().run_until(async {
        let (host,worker)=tokio::io::duplex(65536);
        let (wr,ww)=tokio::io::split(worker);let (hr,mut hw)=tokio::io::split(host);
        let s=Service::new();let inspect=s.clone();
        let runner=tokio::task::spawn_local(runtime_extension_sdk::serve_io(wr,ww,move |r,e|{let s=s.clone();async move{s.dispatch(r,e).await}}));
        let mut reader=BufReader::new(hr);
        let invalid=json!({"input":{"handler":"browser-obscura.browsers.create","input_payload":{"body":{"name":"No context"}}}});
        for (id,request,expected) in [("1",invalid,"invalid_invocation"),("2",request("browsers.create",json!({"body":{"name":"No retention"}})),"retention_unavailable")] {
            send(&mut hw,MultiplexHostMessage::Call{call_id:id.into(),request}).await;
            match read(&mut reader).await {
                MultiplexWorkerMessage::Response{response,..}=>{assert_eq!(response["ok"],true);assert_eq!(response["result"]["success"],false);assert_eq!(response["result"]["error"]["code"],expected);},
                other=>panic!("unexpected {other:?}"),
            }
        }
        assert!(inspect.state.borrow().browsers.is_empty());
        hw.shutdown().await.unwrap();assert!(runner.await.unwrap().is_ok());
    }).await;
}
