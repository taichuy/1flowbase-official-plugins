use super::*;
use std::time::Duration;

// Withhold both API responses until both reads have arrived. Serial code cannot
// pass this barrier; this tests overlap without a wall-clock speed assertion.
#[tokio::test]
async fn profile_and_email_reads_overlap_and_both_must_succeed() {
    for rejected in [None, Some("/user"), Some("/user/emails")] {
        tokio::task::LocalSet::new().run_until(async {
            let (host, worker) = tokio::io::duplex(8192);
            let (worker_read, worker_write) = tokio::io::split(worker);
            let runner = tokio::task::spawn_local(serve_io(worker_read, worker_write, crate::dispatch));
            let (host_read, mut writer) = tokio::io::split(host);
            let mut reader = BufReader::new(host_read);
            let call = MultiplexEnvelope::new(MultiplexHostMessage::Call {
                call_id: "1".into(),
                request: json!({"method":"execute","input":{
                    "handler":"authenticate","config_payload":config(),"input_payload":complete()}}),
            });
            writer.write_all(format!("{}\n",serde_json::to_string(&call).unwrap()).as_bytes()).await.unwrap();
            let token = next(&mut reader).await;
            answer(&mut writer, token, http(json!({"access_token":TOKEN}))).await;
            let first = next(&mut reader).await;
            let second = next(&mut reader).await;
            let mut paths = Vec::new();
            for message in [first, second] {
                let MultiplexWorkerMessage::Callback { ref request, ref service, .. } = message else { panic!("expected API read") };
                assert!(matches!(service, MultiplexHostService::HttpEgressV1));
                assert_eq!(request["headers"]["authorization"], format!("Bearer {TOKEN}"));
                let url = url::Url::parse(request["url"].as_str().unwrap()).unwrap();
                let path = url.path();
                paths.push(path.to_owned());
                let response = if rejected == Some(path) {
                    json!({"result":{"status":503,"headers":{},"body_base64":STANDARD.encode(format!("{SECRET}-{TOKEN}-{CODE}"))}})
                } else if path == "/user" {
                    http(json!({"id":42,"login":"fixture","created_at":"2010-01-01T00:00:00Z"}))
                } else {
                    assert_eq!(path,"/user/emails");
                    http(json!([{"email":"primary@example.test","primary":true,"verified":true}]))
                };
                answer(&mut writer, message, response).await;
            }
            paths.sort();
            assert_eq!(paths, ["/user", "/user/emails"]);
            let MultiplexWorkerMessage::Response {response,..} = next(&mut reader).await else {panic!("expected response")};
            assert_eq!(response["ok"], rejected.is_none());
            if rejected.is_none() {
                assert_eq!(response["result"]["subject_value"],"42");
                assert_eq!(response["result"]["profile"]["email"],"primary@example.test");
            } else {
                assert_eq!(response["error"]["message"],"github_http_rejected");
            }
            assert_safe(&response);
            writer.shutdown().await.unwrap();
            runner.await.unwrap().unwrap();
        }).await;
    }
}

async fn next(
    reader: &mut BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
) -> MultiplexWorkerMessage {
    let mut line = String::new();
    let count = tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .expect("both API reads must be sent before either response is returned")
        .unwrap();
    assert!(count > 0);
    serde_json::from_str::<MultiplexEnvelope<MultiplexWorkerMessage>>(&line)
        .unwrap()
        .message
}

async fn answer(
    writer: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
    message: MultiplexWorkerMessage,
    response: Value,
) {
    let MultiplexWorkerMessage::Callback {
        call_id,
        callback_id,
        ..
    } = message
    else {
        panic!("expected HTTP callback")
    };
    let frame = MultiplexEnvelope::new(MultiplexHostMessage::CallbackResult {
        call_id,
        callback_id,
        response,
    });
    writer
        .write_all(format!("{}\n", serde_json::to_string(&frame).unwrap()).as_bytes())
        .await
        .unwrap();
}
