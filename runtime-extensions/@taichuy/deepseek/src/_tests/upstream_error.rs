use crate::*;
use std::io::{Read, Write};
use std::net::TcpListener;

fn serve(status: &str, content_type: &str, body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let response = format!("HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0_u8; 4096];
        let _ = stream.read(&mut buffer);
        stream.write_all(response.as_bytes()).unwrap();
    });
    url
}

async fn invoke(base_url: String) -> (ProviderInvocationResult, Vec<ProviderStreamEvent>) {
    let mut events = Vec::new();
    let result = handle_invoke_request_streaming(
        json!({
            "contract_version":"1flowbase.provider/v2", "provider_instance_id":"fixture",
            "provider_code":"deepseek", "protocol":"deepseek", "model":"deepseek-v4-pro",
            "provider_config":{"base_url":base_url,"api_key":"secret-canary"},
            "messages":[{"role":"user","content":"hello"}]
        }),
        |event| {
            events.push(event.clone());
            Ok(())
        },
    )
    .await
    .unwrap();
    (result, events)
}

fn upstream() -> Value {
    json!({"error":{"code":"context_length_exceeded","type":"invalid_request_error",
        "message":"context limit secret-canary","param":null,
        "extra":{"number":17,"credential_echo":"secret-canary"}}})
}

fn assert_error(result: ProviderInvocationResult, events: Vec<ProviderStreamEvent>, status: u16) {
    assert_eq!(result.finish_reason, Some(ProviderFinishReason::Error));
    assert_eq!(result.final_content, None);
    assert!(result.tool_calls.is_empty() && result.mcp_calls.is_empty());
    assert!(!events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::ToolCallCommit { .. }
            | ProviderStreamEvent::Finish {
                reason: ProviderFinishReason::Stop
            }
    )));
    let errors: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ProviderStreamEvent::Error { error } => Some(error),
            _ => None,
        })
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, "provider_upstream_error");
    let details = errors[0].provider_details.as_ref().unwrap();
    assert_eq!(details["status_code"], status);
    assert_eq!(details["upstream_error"]["code"], "context_length_exceeded");
    assert_eq!(details["upstream_error"]["param"], Value::Null);
    assert_eq!(details["upstream_error"]["extra"]["number"], 17);
    assert!(!serde_json::to_string(&errors[0])
        .unwrap()
        .contains("secret-canary"));
    if status == 200 {
        assert_eq!(details["semantic_terminal"], true);
    } else {
        assert!(details.get("semantic_terminal").is_none());
    }
}

#[tokio::test]
async fn http_error_preserves_original_code_extensions_and_redacts_values() {
    let (result, events) = invoke(serve(
        "400 Bad Request",
        "application/json",
        upstream().to_string(),
    ))
    .await;
    assert_error(result, events, 400);
}

#[tokio::test]
async fn sse_error_is_terminal_before_delta_after_tools_and_without_newline() {
    for (prefix, suffix) in [
        ("", "\n\ndata: [DONE]\n\n"),
        ("data: {\"choices\":[{\"delta\":{\"content\":\"partial\",\"reasoning_content\":\"think\",\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"effect\",\"arguments\":\"{}\"}}]}}]}\n\n",
         "\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"),
        ("", ""),
    ] {
        let body = format!("{prefix}data: {}{suffix}", upstream());
        let (result, events) = invoke(serve("200 OK", "text/event-stream", body)).await;
        assert_error(result, events, 200);
    }
}

#[test]
fn unstructured_http_error_does_not_invent_a_null_upstream_error() {
    let event =
        crate::upstream_error::event(&Value::Null, "plain upstream failure", "key", 502, false);
    let ProviderStreamEvent::Error { error } = event else {
        panic!("expected error")
    };
    let details = error.provider_details.unwrap();
    assert!(details.get("upstream_error").is_none());
    assert_eq!(details["raw_body"], "plain upstream failure");
    assert_eq!(details["status_code"], 502);
}
