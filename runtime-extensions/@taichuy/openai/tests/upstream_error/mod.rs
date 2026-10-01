use super::*;

#[test]
fn worker_preserves_exact_http_bodies_and_stops_on_structured_sse_failures() {
    let original = json!({"code":"future_supplier_code","type":null,"param":null,
        "message":"supplier line one\nline two", "future":{"nested":[1,null]}});
    let json_body = format!(" \r\n{}\n ", json!({"error":original}));
    let failed_body = format!(
        "data: {}\n\ndata: {}\n\n",
        json!({"type":"response.failed","response":{"id":"rejected","error":original}}),
        json!({"type":"response.completed","response":{"id":"must_not_complete","output":[]}})
    );
    for (status, content_type, body, expected) in [
        (
            "400 Bad Request",
            "application/json",
            json_body,
            Some(original.clone()),
        ),
        (
            "400 Bad Request",
            "text/plain",
            "plain supplier error\n\n".into(),
            None,
        ),
        (
            "400 Bad Request",
            "text/html",
            "<html>supplier error</html>\n".into(),
            None,
        ),
        ("400 Bad Request", "text/plain", " \r\n\t ".into(), None),
        (
            "400 Bad Request",
            "application/json",
            "{\"error\":null}".into(),
            Some(Value::Null),
        ),
        (
            "200 OK",
            "text/event-stream",
            failed_body,
            Some(original.clone()),
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fixture_body = body.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_http_request(&mut stream);
            write!(stream, "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{fixture_body}", fixture_body.len()).unwrap();
        });
        let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = MultiplexStdin::new(child.stdin.take().unwrap());
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        writeln!(stdin, "{}", invoke_line(&base, "http_sse")).unwrap();
        stdin.flush().unwrap();
        let error = next_json_line(&mut stdout);
        assert_eq!(error["type"], "error", "{error}");
        assert_eq!(error["error"]["kind"], "provider_upstream_error");
        let details = &error["error"]["provider_details"];
        assert_eq!(details.get("upstream_error"), expected.as_ref());
        if status.starts_with("400") {
            assert_eq!(details["status_code"], 400);
            assert_eq!(details["raw_body"], body);
        } else {
            assert!(details.get("status_code").is_none());
        }
        let result = next_json_line(&mut stdout);
        assert_eq!(result["type"], "result");
        assert_eq!(result["result"]["finish_reason"], "error");
        assert_ne!(result["result"]["response_id"], "must_not_complete");
        let _ = child.kill();
        let _ = child.wait();
        server.join().unwrap();
    }
}
