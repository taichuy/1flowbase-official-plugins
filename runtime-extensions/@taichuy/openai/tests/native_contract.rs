//! Root #2028 AC-007/010/012: real stdio worker and real WS upstream.
use std::{io::{BufRead, BufReader, Write}, net::TcpListener, process::{Command, Stdio}, thread, time::Duration};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{accept_hdr, Message};

fn input(base: &str, body: Value, current: bool) -> Value {
    let mut capabilities=vec!["responses.native_passthrough"];
    if current { capabilities.push("responses.native_output.v1"); }
    json!({"method":"invoke","input":{
        "contract_version":"1flowbase.provider/v2","provider_instance_id":"fixture",
        "provider_code":"openai","protocol":"openai_responses","model":"fixture",
        "provider_config":{"base_url":base,"api_key":"fixture","transport_mode":"responses_websocket"},
        "required_capabilities":capabilities,
        "client_protocol_envelope":{"source_protocol":"openai_responses","headers":{"session-id":["fixture-session"],"thread-id":["fixture-thread"],"openai-beta":["responses_websockets=2026-02-06"],"x-codex-turn-state":["client-turn"]}},
        "native_transport":{"protocol":"openai_responses","wire_body":body,"digest":"fixture","size_bytes":1}
    }})
}

#[test]
fn old_host_is_rejected_by_real_worker_before_network() {
    let mut child=Command::new(env!("CARGO_BIN_EXE_openai-provider")).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    writeln!(child.stdin.take().unwrap(),"{}",input("http://127.0.0.1:1",json!({"input":[]}),false)).unwrap();
    let output=child.wait_with_output().unwrap();
    let lines: Vec<Value>=String::from_utf8(output.stdout).unwrap().lines().map(|s|serde_json::from_str(s).unwrap()).collect();
    assert_eq!(lines[0]["type"],"error");
    assert!(lines[0].to_string().contains("responses.native_output.v1"));
    assert_eq!(lines.last().unwrap()["result"]["finish_reason"],"error");
}

#[test]
fn paired_worker_preserves_items_and_accepts_both_tool_result_types() {
    for (kind,field,result_kind,raw) in [
        ("custom_tool_call","input","custom_tool_call_output","text(await tools.exec_command({cmd:'cat fixture'}));"),
        ("function_call","arguments","function_call_output","{\"path\":\"fixture\"}")
    ] {
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();
        let base=format!("http://{}",listener.local_addr().unwrap());
        let items=vec![
            json!({"id":"rs_1","type":"reasoning","summary":[],"encrypted_content":"opaque-fixture"}),
            json!({"id":"msg_1","type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"reading"}]}),
            json!({"id":"tool_1","type":kind,"call_id":"call_1","name":"exec",field:raw,"status":"completed"})
        ];
        let upstream_items=items.clone();
        let server=thread::spawn(move || {
            let (stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut ws=accept_hdr(stream, |request: &tokio_tungstenite::tungstenite::handshake::server::Request, mut response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                assert_eq!(request.headers()["openai-beta"], "responses_websockets=2026-02-06");
                assert_eq!(request.headers().get_all("openai-beta").iter().count(), 1);
                assert_eq!(request.headers()["x-codex-turn-state"], "client-turn");
                response.headers_mut().insert("x-codex-turn-state", "provider-turn".parse().unwrap());
                Ok(response)
            }).unwrap();
            let first:Value=serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(first["input"][0]["type"],"additional_tools");
            ws.send(Message::Text(json!({"type":"response.created","response":{"id":"resp_1"}}).to_string().into())).unwrap();
            for (index,item) in upstream_items.iter().enumerate() {
                for event in ["response.output_item.added","response.output_item.done"] {
                    ws.send(Message::Text(json!({"type":event,"output_index":index,"item":item}).to_string().into())).unwrap();
                }
            }
            ws.send(Message::Text(json!({"type":"response.completed","response":{"id":"resp_1","output":upstream_items}}).to_string().into())).unwrap();
            let next:Value=serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(next["type"],"response.create", "native transport must not synthesize response.processed");
            assert_eq!(next["previous_response_id"],"resp_1");
            assert_eq!(next["input"],json!([{"type":result_kind,"call_id":"call_1","output":"nonce"}]));
            let final_item=json!({"id":"msg_final","type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"nonce"}]});
            for event in ["response.output_item.added","response.output_item.done"] {
                ws.send(Message::Text(json!({"type":event,"output_index":0,"item":final_item}).to_string().into())).unwrap();
            }
            ws.send(Message::Text(json!({"type":"response.completed","response":{"id":"resp_2","output":[final_item]}}).to_string().into())).unwrap();
        });
        let mut child=Command::new(env!("CARGO_BIN_EXE_openai-provider")).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut stdin=child.stdin.take().unwrap();let mut stdout=BufReader::new(child.stdout.take().unwrap());
        let mut turn=|body:Value| {writeln!(stdin,"{}",input(&base,body,true)).unwrap();stdin.flush().unwrap();let mut events=vec![];loop {let mut line=String::new();assert!(stdout.read_line(&mut line).unwrap()>0);let value:Value=serde_json::from_str(&line).unwrap();let done=value["type"]=="result";events.push(value);if done{return events;}}};
        let first=turn(json!({"input":[{"type":"additional_tools","tools":[]}]}));
        let actual:Vec<Value>=first.iter().filter(|v|v["type"]=="output_item"&&v["phase"]=="done").map(|v|v["item"].clone()).collect();
        assert_eq!(actual,items);
        assert_eq!(first.last().unwrap()["result"]["provider_metadata"]["transport"],"responses_websocket");
        let second=turn(json!({"previous_response_id":"resp_1","input":[{"type":result_kind,"call_id":"call_1","output":"nonce"}]}));
        assert_eq!(second.last().unwrap()["result"]["final_content"],"nonce");
        assert!(second.iter().any(|v|v["type"]=="output_item"&&v["phase"]=="done"&&v["item"]["phase"]=="final_answer"));
        drop(stdin);assert!(child.wait().unwrap().success());server.join().unwrap();
    }
}

#[test]
fn paired_worker_preserves_native_incomplete_terminal() {
    let listener=TcpListener::bind("127.0.0.1:0").unwrap();
    let base=format!("http://{}",listener.local_addr().unwrap());
    let item=json!({"id":"msg_partial","type":"message","role":"assistant","phase":"final_answer","status":"incomplete","content":[{"type":"output_text","text":"partial"}]});
    let expected=item.clone();
    let server=thread::spawn(move || {
        let (stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut ws=tokio_tungstenite::tungstenite::accept(stream).unwrap();
        ws.read().unwrap();
        for event in ["response.output_item.added","response.output_item.done"] {
            ws.send(Message::Text(json!({"type":event,"output_index":0,"item":item}).to_string().into())).unwrap();
        }
        ws.send(Message::Text(json!({"type":"response.incomplete","response":{"id":"resp_partial","status":"incomplete","output":[item],"incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":2,"output_tokens":3,"total_tokens":5}}}).to_string().into())).unwrap();
    });
    let mut child=Command::new(env!("CARGO_BIN_EXE_openai-provider")).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    writeln!(child.stdin.take().unwrap(),"{}",input(&base,json!({"input":[]}),true)).unwrap();
    let output=child.wait_with_output().unwrap();
    assert!(output.status.success());
    let lines:Vec<Value>=String::from_utf8(output.stdout).unwrap().lines().map(|s|serde_json::from_str(s).unwrap()).collect();
    assert!(!lines.iter().any(|v|v["type"]=="error"));
    assert_eq!(lines.last().unwrap()["result"]["finish_reason"],"length");
    assert_eq!(lines.last().unwrap()["result"]["final_content"],"partial");
    assert_eq!(lines.last().unwrap()["result"]["usage"]["total_tokens"],5);
    assert!(lines.iter().any(|v|v["type"]=="output_item"&&v["phase"]=="done"&&v["item"]==expected));
    server.join().unwrap();
}

// AC-008: an explicitly selected native WS transport must never create an HTTP cursor.
#[test]
fn explicit_native_websocket_handshake_failure_does_not_invoke_http() {
    let listener=TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base=format!("http://{}",listener.local_addr().unwrap());
    let (stop_tx,stop_rx)=std::sync::mpsc::channel();
    let server=thread::spawn(move || {
        let mut requests=vec![];
        loop {
            match listener.accept() {
                Ok((mut stream,_)) => {
                    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                    let mut reader=BufReader::new(stream.try_clone().unwrap());
                    let mut first=String::new();reader.read_line(&mut first).unwrap();
                    requests.push(first);
                    loop {let mut line=String::new();reader.read_line(&mut line).unwrap();if line=="\r\n"||line.is_empty(){break;}}
                    stream.write_all(b"HTTP/1.1 426 Upgrade Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                }
                Err(e) if e.kind()==std::io::ErrorKind::WouldBlock => {
                    if stop_rx.recv_timeout(Duration::from_millis(10)).is_ok(){break;}
                }
                Err(e)=>panic!("{e}"),
            }
        }
        requests
    });
    let mut child=Command::new(env!("CARGO_BIN_EXE_openai-provider")).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    writeln!(child.stdin.take().unwrap(),"{}",input(&base,json!({"input":[]}),true)).unwrap();
    let output=child.wait_with_output().unwrap();
    stop_tx.send(()).unwrap();
    let requests=server.join().unwrap();
    assert!(!requests.is_empty());
    assert!(requests.iter().all(|r|r.starts_with("GET ")),"HTTP model fallback observed: {requests:?}");
    let lines:Vec<Value>=String::from_utf8(output.stdout).unwrap().lines().map(|s|serde_json::from_str(s).unwrap()).collect();
    assert!(lines.iter().any(|v|v["type"]=="error"));
    assert_eq!(lines.last().unwrap()["result"]["finish_reason"],"error");
}
