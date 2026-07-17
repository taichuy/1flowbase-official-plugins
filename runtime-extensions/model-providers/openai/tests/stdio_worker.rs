use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{
    accept, accept_hdr,
    handshake::server::{Request, Response},
    protocol::{frame::coding::CloseCode, CloseFrame},
    Message,
};

fn read_http_request(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut header_end = None;
    let mut body_length = None;

    loop {
        let read = stream.read(&mut chunk).expect("request should be readable");
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);

        if header_end.is_none() {
            header_end = buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|offset| offset + 4);
            if let Some(end) = header_end {
                let headers = String::from_utf8_lossy(&buffer[..end]);
                body_length = headers.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                });
            }
        }

        if let (Some(end), Some(length)) = (header_end, body_length) {
            if buffer.len() >= end + length {
                return;
            }
        }
        if header_end.is_some() && body_length.is_none() {
            return;
        }
    }
}

fn start_two_response_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (mut first_stream, _) = listener.accept().expect("first request should connect");
        read_http_request(&mut first_stream);
        let error_body = concat!(
            r#"{"error":{"message":"OpenAI codex passthrough requires a non-empty instructions field"}}"#,
            "\n",
            r#"data: {"type":"response.failed"}"#,
            "\n\n"
        );
        write!(
            first_stream,
            "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json; charset=utf-8\r\nx-request-id: req_mixed_body\r\nx-api-key: should-not-leak\r\nset-cookie: should-not-leak\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            error_body.len(),
            error_body
        )
        .expect("error response should be writable");

        let (mut second_stream, _) = listener.accept().expect("second request should connect");
        read_http_request(&mut second_stream);
        let response_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_ok\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2},\"output\":[]}}\n\n"
        );
        write!(
            second_stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        )
        .expect("success response should be writable");
    });

    (address, handle)
}

fn start_websocket_reject_then_sse_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (mut first_stream, _) = listener.accept().expect("websocket request should connect");
        read_http_request(&mut first_stream);
        let error_body = r#"{"error":{"message":"websocket unavailable"}}"#;
        write!(
            first_stream,
            "HTTP/1.1 405 Method Not Allowed\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            error_body.len(),
            error_body
        )
        .expect("websocket rejection should be writable");

        let (mut second_stream, _) = listener.accept().expect("fallback request should connect");
        read_http_request(&mut second_stream);
        let response_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"response_id\":\"resp_fallback\",\"delta\":\"fallback ok\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_fallback\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2},\"output\":[]}}\n\n"
        );
        write!(
            second_stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        )
        .expect("fallback response should be writable");
    });

    (address, handle)
}

fn start_websocket_created_close_then_sse_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (first_stream, _) = listener.accept().expect("websocket request should connect");
        let mut websocket = accept(first_stream).expect("websocket handshake should succeed");
        let _ = websocket
            .read()
            .expect("response.create should be readable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.created","response":{"id":"resp_ws"}}"#.into(),
            ))
            .expect("response.created should be writable");
        websocket
            .close(None)
            .expect("websocket close should be writable");

        let (mut second_stream, _) = listener.accept().expect("fallback request should connect");
        read_http_request(&mut second_stream);
        let response_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"response_id\":\"resp_fallback\",\"delta\":\"fallback after close\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_fallback\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2},\"output\":[]}}\n\n"
        );
        write!(
            second_stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        )
        .expect("fallback response should be writable");
    });

    (address, handle)
}

fn start_websocket_function_call_done_then_close_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("websocket request should connect");
        let mut websocket = accept(stream).expect("websocket handshake should succeed");
        let _ = websocket
            .read()
            .expect("first response.create should be readable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_previous","delta":"first"}"#.into(),
            ))
            .expect("first delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_previous","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("first completion should be writable");

        let processed = websocket
            .read()
            .expect("response.processed should be readable")
            .into_text()
            .expect("response.processed should be text");
        assert_eq!(
            serde_json::from_str::<Value>(&processed).expect("response.processed should be JSON"),
            json!({
                "type": "response.processed",
                "response_id": "resp_previous"
            })
        );
        let request = websocket
            .read()
            .expect("continuation response.create should be readable")
            .into_text()
            .expect("continuation request should be text");
        assert!(
            request.contains("\"previous_response_id\":\"resp_previous\""),
            "continuation should carry the response cursor: {request}"
        );
        websocket
            .send(Message::Text(
                r#"{"type":"response.created","response":{"id":"resp_tool_close"}}"#.into(),
            ))
            .expect("response.created should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_item.added","item":{"type":"function_call","call_id":"call_lookup","name":"lookup","arguments":""}}"#.into(),
            ))
            .expect("function call item should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.function_call_arguments.done","item_id":"call_lookup","call_id":"call_lookup","arguments":"{\"query\":\"refund\"}"}"#.into(),
            ))
            .expect("function call arguments should be writable");
        websocket
            .close(None)
            .expect("websocket close should be writable");
    });

    (address, handle)
}

fn start_websocket_close_then_reconnect_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (first_stream, _) = listener.accept().expect("first websocket should connect");
        let mut websocket = accept(first_stream).expect("first websocket handshake should succeed");
        let _ = websocket
            .read()
            .expect("first response.create should be readable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_previous","delta":"first"}"#.into(),
            ))
            .expect("first delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_previous","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("first completion should be writable");
        websocket
            .close(None)
            .expect("first websocket close should be writable");

        let (mut second_stream, _) = listener
            .accept()
            .expect("second continuation request should connect");
        second_stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("second request read timeout");
        let mut prefix = [0_u8; 4];
        let read = second_stream
            .peek(&mut prefix)
            .expect("second request should be peekable");
        if read >= 4 && &prefix == b"POST" {
            read_http_request(&mut second_stream);
            let error_body = r#"{"error":{"message":"previous_response_id is only supported on Responses WebSocket v2"}}"#;
            write!(
                second_stream,
                "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                error_body.len(),
                error_body
            )
            .expect("fallback error should be writable");
            return;
        }

        let mut websocket =
            accept(second_stream).expect("continuation websocket handshake should succeed");
        let request = websocket
            .read()
            .expect("continuation response.create should be readable")
            .into_text()
            .expect("continuation request should be text");
        assert!(
            request.contains("\"previous_response_id\":\"resp_previous\""),
            "continuation websocket request should carry the response cursor: {request}"
        );
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_final","delta":"reconnected ok"}"#.into(),
            ))
            .expect("continuation delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_final","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("continuation completion should be writable");
    });

    (address, handle)
}

fn start_websocket_unseen_cursor_close_then_reconnect_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (first_stream, _) = listener.accept().expect("first websocket should connect");
        let mut websocket = accept(first_stream).expect("first websocket handshake should succeed");
        let request = websocket
            .read()
            .expect("first response.create should be readable")
            .into_text()
            .expect("first request should be text");
        assert!(
            request.contains("\"previous_response_id\":\"resp_previous\""),
            "first request should carry the response cursor: {request}"
        );
        websocket
            .close(None)
            .expect("first websocket close should be writable");

        let (second_stream, _) = listener
            .accept()
            .expect("retry websocket should connect after stream close");
        let mut websocket =
            accept(second_stream).expect("retry websocket handshake should succeed");
        let request = websocket
            .read()
            .expect("retry response.create should be readable")
            .into_text()
            .expect("retry request should be text");
        assert!(
            request.contains("\"previous_response_id\":\"resp_previous\""),
            "retry request should keep the response cursor: {request}"
        );
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_retry","delta":"retry ok"}"#.into(),
            ))
            .expect("retry delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_retry","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("retry completion should be writable");
    });

    (address, handle)
}

fn start_websocket_proxy_failure_then_fresh_turn_state_retry_server(
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("websocket should connect");
        let mut websocket = accept_with_turn_state(stream, "sticky-turn-1");
        let _ = websocket
            .read()
            .expect("first response.create should be readable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_previous","delta":"first"}"#.into(),
            ))
            .expect("first delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_previous","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("first completion should be writable");
        let _ = websocket
            .read()
            .expect("response.processed should be readable");
        let request = websocket
            .read()
            .expect("continuation response.create should be readable")
            .into_text()
            .expect("continuation request should be text");
        assert!(
            request.contains("\"previous_response_id\":\"resp_previous\""),
            "continuation should carry the response cursor: {request}"
        );
        websocket
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Error,
                reason: "upstream websocket proxy failed".into(),
            })))
            .expect("proxy failure close should be writable");

        let (retry_stream, _) = listener
            .accept()
            .expect("retry websocket should connect before fallback");
        let mut websocket = accept_hdr(retry_stream, |request: &Request, response: Response| {
            let got = request
                .headers()
                .get("x-codex-turn-state")
                .and_then(|value| value.to_str().ok());
            assert_eq!(
                got, None,
                "retry after proxy failure should not replay stale turn state"
            );
            Ok(response)
        })
        .expect("retry websocket handshake should succeed");
        let request = websocket
            .read()
            .expect("retry response.create should be readable")
            .into_text()
            .expect("retry request should be text");
        assert!(
            request.contains("\"previous_response_id\":\"resp_previous\""),
            "retry should carry the response cursor: {request}"
        );
        websocket
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Error,
                reason: "upstream websocket proxy failed".into(),
            })))
            .expect("first retry proxy failure close should be writable");

        let (retry_stream, _) = listener
            .accept()
            .expect("second retry websocket should connect");
        let mut websocket = accept_hdr(retry_stream, |request: &Request, response: Response| {
            let got = request
                .headers()
                .get("x-codex-turn-state")
                .and_then(|value| value.to_str().ok());
            assert_eq!(
                got, None,
                "second retry after proxy failure should not replay stale turn state"
            );
            Ok(response)
        })
        .expect("second retry websocket handshake should succeed");
        let request = websocket
            .read()
            .expect("second retry response.create should be readable")
            .into_text()
            .expect("second retry request should be text");
        assert!(
            request.contains("\"previous_response_id\":\"resp_previous\""),
            "second retry should carry the response cursor: {request}"
        );
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_retry","delta":"retry after proxy"}"#.into(),
            ))
            .expect("retry delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_retry","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("retry completion should be writable");
    });

    (address, handle)
}

fn start_websocket_previous_response_unavailable_full_context_server(
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("websocket should connect");
        let mut websocket = accept(stream).expect("websocket handshake should succeed");
        let request = websocket
            .read()
            .expect("first response.create should be readable")
            .into_text()
            .expect("first request should be text");
        assert!(
            !request.contains("\"previous_response_id\""),
            "initial request should not carry a response cursor: {request}"
        );
        websocket
            .send(Message::Text(
                r#"{"type":"response.created","response":{"id":"resp_previous"}}"#.into(),
            ))
            .expect("response.created should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_item.added","item":{"type":"function_call","call_id":"call_lookup","name":"lookup","arguments":""}}"#.into(),
            ))
            .expect("function call item should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.function_call_arguments.done","item_id":"call_lookup","call_id":"call_lookup","arguments":"{\"query\":\"refund\"}"}"#.into(),
            ))
            .expect("function call arguments should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_previous","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[{"type":"function_call","call_id":"call_lookup","name":"lookup","arguments":"{\"query\":\"refund\"}"}]}}"#.into(),
            ))
            .expect("first completion should be writable");

        let _ = websocket
            .read()
            .expect("response.processed should be readable");
        let request = websocket
            .read()
            .expect("continuation response.create should be readable")
            .into_text()
            .expect("continuation request should be text");
        assert!(
            request.contains("\"previous_response_id\":\"resp_previous\""),
            "first continuation should use the response cursor: {request}"
        );
        assert!(
            request.contains("\"type\":\"function_call_output\"")
                && request.contains("\"call_id\":\"call_lookup\""),
            "continuation should carry only the tool output delta: {request}"
        );
        websocket
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Policy,
                reason: "previous_response_id resp_previous is no longer available".into(),
            })))
            .expect("unavailable cursor close should be writable");

        let (retry_stream, _) = listener
            .accept()
            .expect("full-context retry websocket should connect");
        let mut websocket =
            accept(retry_stream).expect("full-context retry handshake should succeed");
        let request = websocket
            .read()
            .expect("full-context response.create should be readable")
            .into_text()
            .expect("full-context request should be text");
        let request_json: Value =
            serde_json::from_str(&request).expect("full-context request should be JSON");
        assert_eq!(request_json["type"], "response.create");
        assert!(
            request_json.get("previous_response_id").is_none(),
            "full-context retry must not reuse the unavailable cursor: {request}"
        );
        assert_eq!(request_json["input"][0]["role"], "user");
        assert_eq!(request_json["input"][0]["content"], "hello");
        assert_eq!(request_json["input"][1]["type"], "function_call");
        assert_eq!(request_json["input"][1]["call_id"], "call_lookup");
        assert_eq!(request_json["input"][1]["name"], "lookup");
        assert_eq!(
            request_json["input"][1]["arguments"],
            r#"{"query":"refund"}"#
        );
        assert_eq!(request_json["input"][2]["type"], "function_call_output");
        assert_eq!(request_json["input"][2]["call_id"], "call_lookup");
        assert_eq!(request_json["input"][2]["output"], "tool result");
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_recovered","delta":"full context recovered"}"#.into(),
            ))
            .expect("full-context delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_recovered","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("full-context completion should be writable");
    });

    (address, handle)
}

fn accept_with_turn_state(
    stream: TcpStream,
    turn_state: &'static str,
) -> tokio_tungstenite::tungstenite::WebSocket<TcpStream> {
    accept_hdr(stream, |_request: &Request, mut response: Response| {
        response
            .headers_mut()
            .insert("x-codex-turn-state", turn_state.parse().unwrap());
        Ok(response)
    })
    .expect("websocket handshake should succeed")
}

fn accept_expect_turn_state(
    stream: TcpStream,
    expected: &'static str,
) -> tokio_tungstenite::tungstenite::WebSocket<TcpStream> {
    accept_hdr(stream, |request: &Request, response: Response| {
        let got = request
            .headers()
            .get("x-codex-turn-state")
            .and_then(|value| value.to_str().ok());
        assert_eq!(
            got,
            Some(expected),
            "continuation reconnect should replay the sticky turn state"
        );
        Ok(response)
    })
    .expect("websocket handshake should succeed")
}

fn accept_record_turn_state(
    stream: TcpStream,
    response_turn_state: Option<&'static str>,
    observed: Arc<Mutex<Vec<Option<String>>>>,
) -> tokio_tungstenite::tungstenite::WebSocket<TcpStream> {
    accept_hdr(stream, move |request: &Request, mut response: Response| {
        let got = request
            .headers()
            .get("x-codex-turn-state")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        observed
            .lock()
            .expect("observed turn states mutex should lock")
            .push(got);
        if let Some(turn_state) = response_turn_state {
            response
                .headers_mut()
                .insert("x-codex-turn-state", turn_state.parse().unwrap());
        }
        Ok(response)
    })
    .expect("websocket handshake should succeed")
}

fn start_websocket_turn_state_reconnect_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (first_stream, _) = listener.accept().expect("first websocket should connect");
        let mut websocket = accept_with_turn_state(first_stream, "sticky-turn-1");
        let _ = websocket
            .read()
            .expect("first response.create should be readable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_previous","delta":"first"}"#.into(),
            ))
            .expect("first delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_previous","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("first completion should be writable");
        websocket
            .close(None)
            .expect("first websocket close should be writable");

        let (second_stream, _) = listener
            .accept()
            .expect("continuation reconnect should connect");
        let mut websocket = accept_expect_turn_state(second_stream, "sticky-turn-1");
        let request = websocket
            .read()
            .expect("continuation response.create should be readable")
            .into_text()
            .expect("continuation request should be text");
        assert!(
            request.contains("\"previous_response_id\":\"resp_previous\""),
            "continuation websocket request should carry the response cursor: {request}"
        );
        assert!(
            request.contains("\"type\":\"function_call_output\"")
                && request.contains("\"call_id\":\"call_lookup\""),
            "continuation websocket request should carry the tool output delta: {request}"
        );
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_final","delta":"sticky ok"}"#.into(),
            ))
            .expect("continuation delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_final","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("continuation completion should be writable");
    });

    (address, handle)
}

fn start_websocket_rotating_turn_state_reconnect_server() -> (
    String,
    thread::JoinHandle<()>,
    Arc<Mutex<Vec<Option<String>>>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let server_observed = Arc::clone(&observed);
    let handle = thread::spawn(move || {
        let (first_stream, _) = listener.accept().expect("first websocket should connect");
        let mut websocket = accept_record_turn_state(
            first_stream,
            Some("sticky-turn-1"),
            Arc::clone(&server_observed),
        );
        let _ = websocket
            .read()
            .expect("first response.create should be readable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_previous","delta":"first"}"#.into(),
            ))
            .expect("first delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_previous","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("first completion should be writable");
        websocket
            .close(None)
            .expect("first websocket close should be writable");

        let (second_stream, _) = listener
            .accept()
            .expect("first continuation reconnect should connect");
        let mut websocket = accept_record_turn_state(
            second_stream,
            Some("sticky-turn-rotated"),
            Arc::clone(&server_observed),
        );
        let request = websocket
            .read()
            .expect("first continuation response.create should be readable")
            .into_text()
            .expect("first continuation request should be text");
        assert!(
            request.contains("\"previous_response_id\":\"resp_previous\""),
            "first continuation should carry the response cursor: {request}"
        );
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_second","delta":"second"}"#.into(),
            ))
            .expect("second delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_second","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("second completion should be writable");
        websocket
            .close(None)
            .expect("second websocket close should be writable");

        let (third_stream, _) = listener
            .accept()
            .expect("second continuation reconnect should connect");
        let mut websocket =
            accept_record_turn_state(third_stream, None, Arc::clone(&server_observed));
        let request = websocket
            .read()
            .expect("second continuation response.create should be readable")
            .into_text()
            .expect("second continuation request should be text");
        assert!(
            request.contains("\"previous_response_id\":\"resp_second\""),
            "second continuation should carry the response cursor: {request}"
        );
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_final","delta":"final"}"#.into(),
            ))
            .expect("final delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_final","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("final completion should be writable");
    });

    (address, handle, observed)
}

fn start_websocket_same_session_continuation_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = format!("http://{}", listener.local_addr().expect("listener addr"));
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("websocket should connect");
        let mut websocket = accept(stream).expect("websocket handshake should succeed");
        let request = websocket
            .read()
            .expect("first response.create should be readable")
            .into_text()
            .expect("first request should be text");
        assert!(
            request.contains("\"type\":\"response.create\""),
            "first websocket request should be response.create: {request}"
        );
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_previous","delta":"first"}"#.into(),
            ))
            .expect("first delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_previous","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("first completion should be writable");

        let processed = websocket
            .read()
            .expect("response.processed should be readable")
            .into_text()
            .expect("response.processed should be text");
        assert_eq!(
            serde_json::from_str::<Value>(&processed).expect("response.processed should be JSON"),
            json!({
                "type": "response.processed",
                "response_id": "resp_previous"
            })
        );
        let request = websocket
            .read()
            .expect("continuation response.create should be readable")
            .into_text()
            .expect("continuation request should be text");
        assert!(
            request.contains("\"type\":\"response.create\""),
            "continuation should follow response.processed: {request}"
        );
        assert!(
            request.contains("\"previous_response_id\":\"resp_previous\""),
            "continuation should carry the response cursor: {request}"
        );
        websocket
            .send(Message::Text(
                r#"{"type":"response.output_text.delta","response_id":"resp_final","delta":"continued"}"#.into(),
            ))
            .expect("continuation delta should be writable");
        websocket
            .send(Message::Text(
                r#"{"type":"response.completed","response":{"id":"resp_final","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"output":[]}}"#.into(),
            ))
            .expect("continuation completion should be writable");
    });

    (address, handle)
}

fn invoke_line(base_url: &str, transport_mode: &str) -> String {
    serde_json::to_string(&json!({
        "method": "invoke",
        "input": {
            "contract_version": "1flowbase.provider/v2",
            "model": "gpt-5.3-codex-spark",
            "provider_config": {
                "base_url": base_url,
                "api_key": "test-key",
                "transport_mode": transport_mode
            },
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        }
    }))
    .expect("invoke request should serialize")
}

fn invoke_line_with_previous_response_id(
    base_url: &str,
    transport_mode: &str,
    previous_response_id: &str,
) -> String {
    serde_json::to_string(&json!({
        "method": "invoke",
        "input": {
            "contract_version": "1flowbase.provider/v2",
            "model": "gpt-5.3-codex-spark",
            "previous_response_id": previous_response_id,
            "provider_config": {
                "base_url": base_url,
                "api_key": "test-key",
                "transport_mode": transport_mode
            },
            "messages": [
                {
                    "role": "tool",
                    "tool_call_id": "call_lookup",
                    "content": "tool result"
                }
            ]
        }
    }))
    .expect("invoke request should serialize")
}

fn next_json_line(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .expect("stdout should be readable");
        assert!(read > 0, "provider worker exited before the next JSON line");
        if !line.trim().is_empty() {
            return serde_json::from_str(line.trim()).expect("stdout line should be JSON");
        }
    }
}

#[test]
fn websocket_previous_response_reconnect_replays_turn_state() {
    let (base_url, server) = start_websocket_turn_state_reconnect_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "responses_websocket"))
        .expect("first request should write");
    stdin.flush().expect("first request should flush");

    loop {
        let line = next_json_line(&mut stdout);
        if line["type"].as_str() == Some("result") {
            assert_eq!(line["result"]["response_id"], "resp_previous");
            break;
        }
    }

    writeln!(
        stdin,
        "{}",
        invoke_line_with_previous_response_id(&base_url, "responses_websocket", "resp_previous")
    )
    .expect("continuation request should write");
    stdin.flush().expect("continuation request should flush");

    let mut saw_text_delta = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("text_delta") => {
                saw_text_delta = true;
                assert_eq!(line["delta"], "sticky ok");
            }
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "sticky ok");
                assert_eq!(line["result"]["response_id"], "resp_final");
                break;
            }
            Some("error") => panic!("continuation reconnect should keep sticky turn state: {line}"),
            _ => {}
        }
    }
    assert!(saw_text_delta);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}

#[test]
fn websocket_continuation_reconnect_keeps_original_turn_state() {
    let (base_url, server, observed_turn_states) =
        start_websocket_rotating_turn_state_reconnect_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "responses_websocket"))
        .expect("first request should write");
    stdin.flush().expect("first request should flush");

    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("result") => {
                assert_eq!(line["result"]["response_id"], "resp_previous");
                break;
            }
            Some("error") => panic!("first websocket request should succeed: {line}"),
            _ => {}
        }
    }

    writeln!(
        stdin,
        "{}",
        invoke_line_with_previous_response_id(&base_url, "responses_websocket", "resp_previous")
    )
    .expect("first continuation request should write");
    stdin
        .flush()
        .expect("first continuation request should flush");

    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("result") => {
                assert_eq!(line["result"]["response_id"], "resp_second");
                break;
            }
            Some("error") => panic!("first continuation should succeed: {line}"),
            _ => {}
        }
    }

    writeln!(
        stdin,
        "{}",
        invoke_line_with_previous_response_id(&base_url, "responses_websocket", "resp_second")
    )
    .expect("second continuation request should write");
    stdin
        .flush()
        .expect("second continuation request should flush");

    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "final");
                assert_eq!(line["result"]["response_id"], "resp_final");
                break;
            }
            Some("error") => panic!("second continuation should succeed: {line}"),
            _ => {}
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");

    let observed = observed_turn_states
        .lock()
        .expect("observed turn states mutex should lock")
        .clone();
    assert_eq!(
        observed,
        vec![
            None,
            Some("sticky-turn-1".to_string()),
            Some("sticky-turn-1".to_string())
        ]
    );
}

#[test]
fn websocket_continuation_sends_response_processed_before_next_request() {
    let (base_url, server) = start_websocket_same_session_continuation_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "responses_websocket"))
        .expect("first request should write");
    stdin.flush().expect("first request should flush");

    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("result") => {
                assert_eq!(line["result"]["response_id"], "resp_previous");
                break;
            }
            Some("error") => panic!("first websocket request should succeed: {line}"),
            _ => {}
        }
    }

    writeln!(
        stdin,
        "{}",
        invoke_line_with_previous_response_id(&base_url, "responses_websocket", "resp_previous")
    )
    .expect("continuation request should write");
    stdin.flush().expect("continuation request should flush");

    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "continued");
                assert_eq!(line["result"]["response_id"], "resp_final");
                break;
            }
            Some("error") => panic!("continuation should succeed on the same websocket: {line}"),
            _ => {}
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}

#[test]
fn websocket_previous_response_reconnects_instead_of_http_fallback() {
    let (base_url, server) = start_websocket_close_then_reconnect_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "responses_websocket"))
        .expect("first request should write");
    stdin.flush().expect("first request should flush");

    loop {
        let line = next_json_line(&mut stdout);
        if line["type"].as_str() == Some("result") {
            assert_eq!(line["result"]["final_content"], "first");
            assert_eq!(line["result"]["response_id"], "resp_previous");
            break;
        }
    }

    writeln!(
        stdin,
        "{}",
        invoke_line_with_previous_response_id(&base_url, "responses_websocket", "resp_previous")
    )
    .expect("continuation request should write");
    stdin.flush().expect("continuation request should flush");

    let mut saw_text_delta = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("text_delta") => {
                saw_text_delta = true;
                assert_eq!(line["delta"], "reconnected ok");
            }
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "reconnected ok");
                assert_eq!(line["result"]["response_id"], "resp_final");
                assert_eq!(
                    line["result"]["provider_metadata"]["transport"],
                    "responses_websocket"
                );
                break;
            }
            Some("error") => panic!("continuation should not fall back to HTTP SSE: {line}"),
            _ => {}
        }
    }
    assert!(saw_text_delta);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}

#[test]
fn websocket_previous_response_retries_stream_close_without_seen_cursor() {
    let (base_url, server) = start_websocket_unseen_cursor_close_then_reconnect_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(
        stdin,
        "{}",
        invoke_line_with_previous_response_id(&base_url, "responses_websocket", "resp_previous")
    )
    .expect("request should write");
    stdin.flush().expect("request should flush");

    let mut saw_text_delta = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("text_delta") => {
                saw_text_delta = true;
                assert_eq!(line["delta"], "retry ok");
            }
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "retry ok");
                assert_eq!(line["result"]["response_id"], "resp_retry");
                assert_eq!(
                    line["result"]["provider_metadata"]["transport"],
                    "responses_websocket"
                );
                break;
            }
            Some("error") => panic!("websocket cursor stream close should reconnect: {line}"),
            _ => {}
        }
    }
    assert!(saw_text_delta);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}

#[test]
fn websocket_proxy_failure_after_cursor_retries_without_stale_turn_state() {
    let (base_url, server) = start_websocket_proxy_failure_then_fresh_turn_state_retry_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "responses_websocket"))
        .expect("first request should write");
    stdin.flush().expect("first request should flush");

    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("result") => {
                assert_eq!(line["result"]["response_id"], "resp_previous");
                break;
            }
            Some("error") => panic!("first websocket request should succeed: {line}"),
            _ => {}
        }
    }

    writeln!(
        stdin,
        "{}",
        invoke_line_with_previous_response_id(&base_url, "responses_websocket", "resp_previous")
    )
    .expect("continuation request should write");
    stdin.flush().expect("continuation request should flush");

    let mut saw_text_delta = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("text_delta") => {
                saw_text_delta = true;
                assert_eq!(line["delta"], "retry after proxy");
            }
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "retry after proxy");
                assert_eq!(
                    line["result"]["provider_metadata"]["transport"],
                    "responses_websocket"
                );
                break;
            }
            Some("error") => panic!("proxy failure should reconnect without stale state: {line}"),
            _ => {}
        }
    }
    assert!(saw_text_delta);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}

#[test]
fn websocket_previous_response_unavailable_retries_with_full_context() {
    let (base_url, server) = start_websocket_previous_response_unavailable_full_context_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "responses_websocket"))
        .expect("first request should write");
    stdin.flush().expect("first request should flush");

    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("result") => {
                assert_eq!(line["result"]["response_id"], "resp_previous");
                assert_eq!(line["result"]["finish_reason"], "tool_call");
                assert_eq!(line["result"]["tool_calls"][0]["id"], "call_lookup");
                break;
            }
            Some("error") => panic!("first websocket request should succeed: {line}"),
            _ => {}
        }
    }

    writeln!(
        stdin,
        "{}",
        invoke_line_with_previous_response_id(&base_url, "responses_websocket", "resp_previous")
    )
    .expect("continuation request should write");
    stdin.flush().expect("continuation request should flush");

    let mut saw_text_delta = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("text_delta") => {
                saw_text_delta = true;
                assert_eq!(line["delta"], "full context recovered");
            }
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "full context recovered");
                assert_eq!(line["result"]["response_id"], "resp_recovered");
                assert_eq!(
                    line["result"]["provider_metadata"]["transport"],
                    "responses_websocket"
                );
                break;
            }
            Some("error") => {
                panic!("unavailable cursor should recover with full-context retry: {line}")
            }
            _ => {}
        }
    }
    assert!(saw_text_delta);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}

#[test]
fn websocket_previous_response_can_fallback_to_sse_without_prior_websocket_session() {
    let (base_url, server) = start_websocket_reject_then_sse_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(
        stdin,
        "{}",
        invoke_line_with_previous_response_id(&base_url, "responses_websocket", "resp_previous")
    )
    .expect("request should write");
    stdin.flush().expect("request should flush");

    let mut saw_text_delta = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("text_delta") => {
                saw_text_delta = true;
                assert_eq!(line["delta"], "fallback ok");
            }
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "fallback ok");
                assert_eq!(line["result"]["response_id"], "resp_fallback");
                assert_eq!(line["result"]["provider_metadata"]["transport"], "http_sse");
                break;
            }
            Some("error") => panic!("HTTP-only provider should still be able to fallback: {line}"),
            _ => {}
        }
    }
    assert!(saw_text_delta);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}

#[test]
fn invoke_error_emits_result_line_and_keeps_worker_reusable() {
    let (base_url, server) = start_two_response_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "http_sse")).expect("first request should write");
    stdin.flush().expect("first request should flush");

    let error_line = next_json_line(&mut stdout);
    assert_eq!(error_line["type"], "error");
    assert_eq!(error_line["error"]["kind"], "provider_upstream_error");
    assert!(error_line["error"]["message"]
        .as_str()
        .expect("error message should be a string")
        .contains("OpenAI codex passthrough requires a non-empty instructions field"));
    assert_eq!(error_line["error"]["provider_details"]["status"], 400);
    assert_eq!(
        error_line["error"]["provider_details"]["content_type"],
        "application/json; charset=utf-8"
    );
    assert_eq!(
        error_line["error"]["provider_details"]["headers"]["x-request-id"],
        "req_mixed_body"
    );
    assert!(error_line["error"]["provider_details"]["headers"]
        .get("authorization")
        .is_none());
    assert!(error_line["error"]["provider_details"]["headers"]
        .get("x-api-key")
        .is_none());
    assert!(error_line["error"]["provider_details"]["headers"]
        .get("set-cookie")
        .is_none());
    assert!(error_line["error"]["provider_details"]["raw_body"]
        .as_str()
        .expect("raw_body should be a string")
        .contains("data: {\"type\":\"response.failed\"}"));

    let result_line = next_json_line(&mut stdout);
    assert_eq!(result_line["type"], "result");
    assert_eq!(result_line["result"]["finish_reason"], "error");

    writeln!(stdin, "{}", invoke_line(&base_url, "http_sse")).expect("second request should write");
    stdin.flush().expect("second request should flush");

    let mut saw_text_delta = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("text_delta") => {
                saw_text_delta = true;
                assert_eq!(line["delta"], "ok");
            }
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "ok");
                assert_eq!(line["result"]["response_id"], "resp_ok");
                break;
            }
            _ => {}
        }
    }
    assert!(saw_text_delta);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}

#[test]
fn websocket_transport_falls_back_to_sse_before_response_events() {
    let (base_url, server) = start_websocket_reject_then_sse_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "responses_websocket"))
        .expect("request should write");
    stdin.flush().expect("request should flush");

    let mut saw_text_delta = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("text_delta") => {
                saw_text_delta = true;
                assert_eq!(line["delta"], "fallback ok");
            }
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "fallback ok");
                assert_eq!(line["result"]["response_id"], "resp_fallback");
                assert_eq!(line["result"]["provider_metadata"]["transport"], "http_sse");
                break;
            }
            Some("error") => panic!("fallback should not emit an error line: {line}"),
            _ => {}
        }
    }
    assert!(saw_text_delta);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}

#[test]
fn websocket_transport_falls_back_to_sse_after_lifecycle_frame_without_output() {
    let (base_url, server) = start_websocket_created_close_then_sse_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "responses_websocket"))
        .expect("request should write");
    stdin.flush().expect("request should flush");

    let mut saw_text_delta = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("text_delta") => {
                saw_text_delta = true;
                assert_eq!(line["delta"], "fallback after close");
            }
            Some("result") => {
                assert_eq!(line["result"]["final_content"], "fallback after close");
                assert_eq!(line["result"]["response_id"], "resp_fallback");
                assert_eq!(line["result"]["provider_metadata"]["transport"], "http_sse");
                break;
            }
            Some("error") => panic!("fallback should not emit an error line: {line}"),
            _ => {}
        }
    }
    assert!(saw_text_delta);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}

#[test]
fn websocket_close_after_function_call_done_finalizes_tool_call() {
    let (base_url, server) = start_websocket_function_call_done_then_close_server();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openai-provider"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("openai provider binary should spawn");
    let mut stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "{}", invoke_line(&base_url, "responses_websocket"))
        .expect("first request should write");
    stdin.flush().expect("first request should flush");

    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("result") => {
                assert_eq!(line["result"]["response_id"], "resp_previous");
                break;
            }
            Some("error") => panic!("first websocket request should succeed: {line}"),
            _ => {}
        }
    }

    writeln!(
        stdin,
        "{}",
        invoke_line_with_previous_response_id(&base_url, "responses_websocket", "resp_previous")
    )
    .expect("request should write");
    stdin.flush().expect("request should flush");

    let mut saw_tool_call_commit = false;
    loop {
        let line = next_json_line(&mut stdout);
        match line["type"].as_str() {
            Some("tool_call_commit") => {
                saw_tool_call_commit = true;
                assert_eq!(line["call"]["id"], "call_lookup");
                assert_eq!(line["call"]["name"], "lookup");
                assert_eq!(line["call"]["arguments"]["query"], "refund");
            }
            Some("result") => {
                assert_eq!(line["result"]["response_id"], "resp_tool_close");
                assert_eq!(line["result"]["finish_reason"], "tool_call");
                assert_eq!(line["result"]["tool_calls"][0]["id"], "call_lookup");
                break;
            }
            Some("error") => panic!("function call close should finalize without error: {line}"),
            _ => {}
        }
    }
    assert!(saw_tool_call_commit);

    let _ = child.kill();
    let _ = child.wait();
    server.join().expect("server thread should finish");
}
