use super::*;

fn context(level: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([(
        "provider_recovery".to_owned(),
        json!({
            "policy":{"type":"semantic_mapped","budget":{"max_inner_attempts":3,
                "absolute_deadline_unix_ms":4102444800000_i64}},
            "transport_epoch":17, "initial_commit_level":level
        }),
    )])
}

fn receipt(error: &anyhow::Error) -> Value {
    error
        .downcast_ref::<ProviderRuntimeError>()
        .unwrap()
        .provider_details
        .as_ref()
        .unwrap()[PROVIDER_RECOVERY_RECEIPT_METADATA_KEY]
        .clone()
}

async fn read(body: &[u8], fail_sink: bool) -> Result<RuntimeInvocationEnvelope> {
    read_with_truncated_body(body, fail_sink, false).await
}

async fn read_with_truncated_body(
    body: &[u8],
    fail_sink: bool,
    truncated: bool,
) -> Result<RuntimeInvocationEnvelope> {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let body = body.to_vec();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        socket.read(&mut request).unwrap();
        write!(socket, "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len() + if truncated { 16 } else { 0 }).unwrap();
        socket.write_all(&body).unwrap();
    });
    let response = reqwest::get(format!("http://{address}")).await.unwrap();
    let result = read_streaming_message_with_recovery(
        response,
        "fixture".into(),
        1024,
        &mut |_| {
            if fail_sink {
                bail!("sink rejected event")
            } else {
                Ok(())
            }
        },
        HttpRecovery::from_context(&context("lifecycle_only")).unwrap(),
    )
    .await;
    server.join().unwrap();
    result
}

#[tokio::test]
async fn presemantic_eof_has_typed_http_logical_retry_receipt() {
    let error = read(
        b"data: {\"type\":\"message_start\",\"message\":{\"id\":\"upstream\"}}\n\n",
        false,
    )
    .await
    .unwrap_err();
    let value = receipt(&error);
    assert_eq!(value["transport"], "provider_http");
    assert_eq!(value["commit_level"], "lifecycle_only");
    assert_eq!(value["disposition"], "logical_invocation_retry");
    assert_eq!(value["transport_epoch"], 17);
    assert_eq!(value["attempt"], 0);
    let typed = error.downcast_ref::<ProviderRuntimeError>().unwrap();
    assert_eq!(
        typed.provider_details.as_ref().unwrap()["1flowbase_provider_recovery_original_error"]
            ["message"],
        typed.message
    );
}

#[tokio::test]
async fn semantic_events_and_terminal_stop_forbid_eof_replay() {
    for data in [
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t","name":"Read","input":{}}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"thought"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signed"}}),
    ] {
        let error = read(format!("data: {data}\n\n").as_bytes(), false)
            .await
            .unwrap_err();
        assert_eq!(receipt(&error)["disposition"], "terminal_interruption");
        assert_eq!(receipt(&error)["commit_level"], "terminal");
    }
    let mut state = HttpRecovery::from_context(&context("lifecycle_only")).unwrap();
    state.observe_data(r#"{"type":"message_stop"}"#).unwrap();
    assert_eq!(
        receipt(&state.disconnected(anyhow!("late disconnect")))["commit_level"],
        "terminal"
    );
}

#[tokio::test]
async fn malformed_json_invalid_utf8_sink_and_upstream_errors_never_authorize_retry() {
    for (data, sink) in [
        (b"data: {malformed}\n\n".as_slice(), false),
        (b"data: {}\n\n".as_slice(), false),
        (b"data: \xff\n\n".as_slice(), false),
        (b"data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n".as_slice(), true),
        (b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"original\"}}\n\n".as_slice(), false),
    ] {
        let error = read(data, sink).await.unwrap_err();
        assert!(error.downcast_ref::<ProviderRuntimeError>().and_then(|typed|
            typed.provider_details.as_ref()).and_then(|details|
            details.get(PROVIDER_RECOVERY_RECEIPT_METADATA_KEY)).is_none());
    }
}

#[test]
fn directive_absence_malformed_and_existing_commit_are_safe() {
    assert!(
        HttpRecovery::from_context(&BTreeMap::from([("provider_recovery".into(), json!({}))]))
            .is_err()
    );
    let original = HttpRecovery::default().disconnected(anyhow!("original"));
    assert_eq!(original.to_string(), "original");
    assert!(original.downcast_ref::<ProviderRuntimeError>().is_none());
    for level in ["semantic_committed", "terminal"] {
        let recovery = HttpRecovery::from_context(&context(level)).unwrap();
        assert_eq!(
            receipt(&recovery.disconnected(anyhow!("original")))["disposition"],
            "terminal_interruption"
        );
    }
}

#[tokio::test]
async fn premature_http_body_disconnect_is_recoverable_only_before_semantics() {
    for (data, expected) in [
        (json!({"type":"ping"}), "logical_invocation_retry"),
        (
            json!({"type":"content_block_delta","delta":{"type":"signature_delta","signature":"signed"}}),
            "terminal_interruption",
        ),
    ] {
        let error = read_with_truncated_body(format!("data: {data}\n\n").as_bytes(), false, true)
            .await
            .unwrap_err();
        assert_eq!(receipt(&error)["disposition"], expected);
    }
}

#[tokio::test]
async fn send_disconnect_uses_host_directive_without_internal_retry() {
    use std::{io::Read, net::TcpListener};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = [0; 16384];
        let _ = socket.read(&mut request).unwrap();
        // Close after accepting HTTP request, before sending response headers.
    });
    let input: ProviderInvocationInput = serde_json::from_value(json!({
        "contract_version":"1flowbase.provider/v2", "provider_instance_id":"fixture-provider",
        "provider_code":"anthropic", "protocol":"anthropic_messages", "model":"fixture",
        "provider_config":{"api_key":"test-key","base_url":format!("http://{address}")},
        "messages":[{"role":"user","content":"hello"}],
        "run_context":context("lifecycle_only")
    }))
    .unwrap();
    let error = invoke_message_with_event_sink(input, |_| Ok(()))
        .await
        .unwrap_err();
    server.join().unwrap();
    assert_eq!(receipt(&error)["disposition"], "logical_invocation_retry");
    assert_eq!(receipt(&error)["attempt"], 0);
}

#[tokio::test]
async fn empty_thinking_start_natural_eof_retains_lifecycle_only_recovery() {
    // Natural Claude proxy failure: lifecycle start frames followed by EOF,
    // before any thinking, signature, text or tool-input was committed.
    let body = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"natural\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":12,\"output_tokens\":0}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n"
    );
    let error = read(body.as_bytes(), false).await.unwrap_err();
    assert_eq!(receipt(&error)["disposition"], "logical_invocation_retry");
    assert_eq!(receipt(&error)["commit_level"], "lifecycle_only");
    for data in [
        json!({"type":"content_block_start","content_block":{"type":"thinking","thinking":"actual thought","signature":""}}),
        json!({"type":"content_block_start","content_block":{"type":"thinking","thinking":"","signature":"actual signature"}}),
        json!({"type":"content_block_start","content_block":{"type":"redacted_thinking","data":"opaque"}}),
        json!({"type":"future_event","data":"future semantics"}),
    ] {
        let error = read(format!("data: {data}\n\n").as_bytes(), false)
            .await
            .unwrap_err();
        assert_eq!(receipt(&error)["disposition"], "terminal_interruption");
    }
}
