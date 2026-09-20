use super::*;

#[test]
fn managed_http_fallback_shares_actual_attempt_budget_with_handshake_failure() {
    for budget in [1, 2] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            read_http_request(&mut first);
            first.write_all(b"HTTP/1.1 405 Method Not Allowed\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").unwrap();
            drop(first);
            let mut actual = 1;
            if budget == 2 {
                let (mut second, _) = listener.accept().unwrap();
                read_http_request(&mut second);
                actual += 1;
                let body = "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_fallback\",\"output\":[]}}\n\n";
                write!(second, "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
            listener.set_nonblocking(true).unwrap();
            (listener, actual)
        });
        let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut request: Value =
            serde_json::from_str(&invoke_line(&base, "responses_websocket")).unwrap();
        request["input"]["run_context"]["provider_recovery"] = json!({
            "policy":{"type":"semantic_mapped","budget":{"max_inner_attempts":budget,"absolute_deadline_unix_ms":4102444800000_i64}},
            "transport_epoch":17,"initial_commit_level":"lifecycle_only"
        });
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        let mut failure = None;
        let result = loop {
            let line = next_json_line(&mut stdout);
            if line["type"] == "error" {
                failure = Some(line.clone());
            }
            if line["type"] == "result" {
                break line;
            }
        };
        let receipt = if budget == 1 {
            let error = failure
                .as_ref()
                .expect("one attempt cannot issue HTTP fallback");
            &error["error"]["provider_details"]["1flowbase_provider_recovery"]
        } else {
            assert!(failure.is_none(), "{failure:?}");
            &result["result"]["provider_metadata"]["1flowbase_provider_recovery"]
        };
        assert_eq!(receipt["attempt"], budget - 1);
        if budget == 2 {
            assert_eq!(receipt["transport"], "provider_http");
        }
        let (listener, actual) = server.join().unwrap();
        assert_eq!(actual, budget);
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
    }
}
