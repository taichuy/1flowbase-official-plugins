use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

use gemini_provider::{handle_request, ProviderStdioRequest};
use serde_json::{json, Value};

fn fake_count_tokens_upstream(response_body: &'static str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
    let address = format!("http://{}", listener.local_addr().expect("fixture address"));
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("fixture request");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = vec![0; 8192];
        let length = stream.read(&mut bytes).expect("read fixture request");
        sender
            .send(String::from_utf8_lossy(&bytes[..length]).to_string())
            .unwrap();
        write!(stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(), response_body
        ).unwrap();
    });
    (address, receiver)
}

fn request(base_url: &str) -> ProviderStdioRequest {
    ProviderStdioRequest {
        method: "invoke".to_string(),
        input: json!({
            "operation": "count_tokens",
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "fixture",
            "provider_code": "gemini",
            "protocol": "gemini",
            "model": "unknown-fixture-model",
            "provider_config": {"base_url": base_url, "api_key": "fixture-secret"},
            "messages": [{"role":"user","content":"hello"}],
            "system": [{"type":"text","text":"instructions"}],
            "required_capabilities": ["count_tokens"],
            "tools": [{"functionDeclarations":[{"name":"weather"}]}],
            "mcp_bindings": [],
            "response_format": null,
            "model_parameters": {"temperature":0.8,"max_output_tokens":512},
            "client_protocol_envelope": {
                "source_protocol": "anthropic_messages",
                "source_request": {
                    "authentication": "x_api_key",
                    "body": {"model":"foreign-model"}
                },
                "query": {"future":["ignored"]},
                "headers": {
                    "x-client-name": ["CanonicalClient"],
                    "authorization": ["Bearer must-not-win"]
                },
                "body": {"future_field":true}
            },
            "trace_context": {},
            "run_context": {}
        }),
    }
}

#[tokio::test]
async fn official_count_tokens_uses_model_method_body_and_auth() {
    let (base_url, captured) = fake_count_tokens_upstream(r#"{"totalTokens":29}"#);
    let response = handle_request(request(&base_url))
        .await
        .expect("CountTokens response");
    assert_eq!(
        response.result,
        json!({
            "operation":"count_tokens", "input_tokens":29, "method":"upstream_api",
            "coverage":"complete", "unknown_block_count":0
        })
    );
    let captured = captured.recv_timeout(Duration::from_secs(5)).unwrap();
    let (headers, body) = captured.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("POST /v1beta/models/unknown-fixture-model:countTokens HTTP/1.1"));
    assert!(headers
        .to_ascii_lowercase()
        .contains("x-goog-api-key: fixture-secret"));
    assert!(headers
        .to_ascii_lowercase()
        .contains("x-client-name: canonicalclient"));
    assert!(!headers
        .to_ascii_lowercase()
        .contains("authorization: bearer must-not-win"));
    let body: Value = serde_json::from_str(body).unwrap();
    assert!(body.get("contents").is_some());
    assert!(body.get("systemInstruction").is_some());
    assert!(body.get("tools").is_some());
    assert!(body.get("generationConfig").is_none());
    assert!(body.get("toolConfig").is_none());
    assert!(body.get("safetySettings").is_none());
}

#[tokio::test]
async fn malformed_upstream_count_tokens_is_a_plugin_failure_fixture() {
    let (base_url, captured) = fake_count_tokens_upstream("{}");
    let error = handle_request(request(&base_url))
        .await
        .expect_err("missing totalTokens must fail");
    captured
        .recv_timeout(Duration::from_secs(5))
        .expect("malformed fixture request must remain captured until the response is sent");
    assert!(error.to_string().contains("totalTokens"));
}
