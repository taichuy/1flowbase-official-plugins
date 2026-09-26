use super::*;

fn request(api_key: &str, logical: Option<&str>, previous: Option<&str>) -> ProviderStdioRequest {
    let mut run_context = json!({});
    if let Some(logical) = logical {
        run_context[TRANSPORT_SESSION_CONTEXT_KEY] = json!({
            "logical_session_id": logical,
            "generation": 1,
            "worker_incarnation": 9,
            "task_id": "fixture-task",
            "state": "active",
            "physical_deadline_unix_ms": 4_102_444_800_000_i64
        });
    }
    ProviderStdioRequest {
        method: "invoke".into(),
        input: json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "fixture",
            "provider_code": "openai",
            "protocol": "openai_responses",
            "model": "fixture-model",
            "provider_config": {"api_key": api_key, "base_url": "http://127.0.0.1:1"},
            "previous_response_id": previous,
            "messages": [],
            "run_context": run_context
        }),
    }
}

#[test]
fn trusted_observation_envelope_routes_generate_and_preserves_handler_marker() {
    let business = request("key-a", Some("logical-a"), None).input;
    for capabilities in [json!([]), json!(["protocol_observation_v1"])] {
        let decoded: ProviderStdioRequest = serde_json::from_value(json!({
            "method": "invoke",
            "host_capabilities": capabilities,
            "input": business,
        }))
        .unwrap();
        let capture_enabled = protocol_observation::enabled(&decoded.input);
        assert_eq!(
            capture_enabled,
            capabilities == json!(["protocol_observation_v1"])
        );
        let mut owners = Owners::default();
        assert!(owners.route(&decoded).unwrap().1.is_some());
        assert!(is_streaming_generate(&decoded));
        assert_eq!(
            protocol_observation::enabled(&decoded.input),
            capture_enabled
        );
        assert_eq!(
            routed_invocation_input(&decoded.input).unwrap().operation,
            ProviderWireOperation::Generate
        );
    }
}

#[test]
fn observation_marker_does_not_relax_business_input_schema() {
    let mut business = request("key-a", Some("logical-a"), None).input;
    business["unknown_business_field"] = json!(true);
    let decoded: ProviderStdioRequest = serde_json::from_value(json!({
        "method": "invoke",
        "host_capabilities": ["protocol_observation_v1"],
        "input": business,
    }))
    .unwrap();
    assert!(protocol_observation::enabled(&decoded.input));
    assert!(Owners::default().route(&decoded).is_err());
    assert!(!is_streaming_generate(&decoded));
}

#[tokio::test(flavor = "current_thread")]
async fn routed_observation_reaches_the_real_streaming_handler() {
    use std::{net::TcpListener, thread};
    use tokio_tungstenite::tungstenite::{accept, Message};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let upstream = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut socket = accept(stream).unwrap();
        let sent = socket.read().unwrap().into_text().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&sent).unwrap()["type"],
            "response.create"
        );
        socket
            .send(Message::Text(
                json!({"type":"response.created","response":{"id":"resp-fixture","status":"in_progress","output":[]}})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        socket
            .send(Message::Text(
                json!({"type":"response.completed","response":{"id":"resp-fixture","status":"completed","output":[]}})
                    .to_string()
                    .into(),
            ))
            .unwrap();
    });

    let mut business = request("fixture-secret", Some("logical-fixture"), None).input;
    business["provider_config"]["base_url"] = json!(base_url);
    business["provider_config"]["transport_mode"] = json!("responses_websocket");
    business["messages"] = json!([{"role":"user","content":"fixture"}]);
    let decoded: ProviderStdioRequest = serde_json::from_value(json!({
        "method": "invoke",
        "host_capabilities": ["protocol_observation_v1"],
        "input": business,
    }))
    .unwrap();
    assert!(Owners::default().route(&decoded).is_ok());
    assert!(is_streaming_generate(&decoded));
    let mut events = Vec::new();
    let result = OpenAiProviderRuntime::default()
        .handle_invoke_request_streaming(decoded.input, |event| {
            events.push(event.clone());
            Ok(())
        })
        .await
        .unwrap();
    upstream.join().unwrap();
    assert_eq!(result.response_id.as_deref(), Some("resp-fixture"));
    assert!(events.iter().any(|event| matches!(event,
        ProviderStreamEvent::ProtocolObservation { kind, .. } if kind == "request_prepared"
    )));
    assert!(events.iter().any(|event| matches!(event,
        ProviderStreamEvent::ProtocolObservation { kind, .. } if kind == "stream_end"
    )));
}

#[test]
fn independent_sessions_and_scoped_response_owners() {
    let mut owners = Owners::default();
    let (first, first_scope) = owners
        .route(&request("key-a", Some("logical-a"), None))
        .unwrap();
    owners.owner(&first);
    let (second, _) = owners
        .route(&request("key-a", Some("logical-b"), None))
        .unwrap();
    assert_ne!(first, second);
    let first_scope = first_scope.unwrap();
    let command = ProviderStdioRequest {
        method: "transport_session".into(),
        input: json!({"logical_session_id":"logical-a", "generation":1,
            "worker_incarnation":9,"action":"close","deadline_unix_ms":4_102_444_800_000_i64}),
    };
    assert_eq!(owners.route(&command).unwrap().0, first);
    owners.record(&first, Some(first_scope), Some("resp-fixture".into()));
    let (continuation, _) = owners
        .route(&request("key-a", None, Some("resp-fixture")))
        .unwrap();
    // A no-directive call has a different physical scope; it cannot claim a
    // host-sealed native session merely by guessing a response ID.
    assert_ne!(continuation, first);
    let (foreign_credential, _) = owners
        .route(&request("key-b", None, Some("resp-fixture")))
        .unwrap();
    assert_ne!(foreign_credential, first);
    let (unsealed, unsealed_scope) = owners.route(&request("key-a", None, None)).unwrap();
    owners.owner(&unsealed);
    owners.record(&unsealed, unsealed_scope, Some("resp-unsealed".into()));
    let (continued, _) = owners
        .route(&request("key-a", None, Some("resp-unsealed")))
        .unwrap();
    assert_eq!(continued, unsealed);
}

#[test]
fn control_cannot_choose_between_different_credentials() {
    let mut owners = Owners::default();
    let (first, _) = owners
        .route(&request("key-a", Some("same-logical"), None))
        .unwrap();
    owners.owner(&first);
    let (second, _) = owners
        .route(&request("key-b", Some("same-logical"), None))
        .unwrap();
    owners.owner(&second);
    let command = ProviderStdioRequest {
        method: "transport_session".into(),
        input: json!({"logical_session_id":"same-logical", "generation":1,
            "worker_incarnation":9,"action":"close","deadline_unix_ms":4_102_444_800_000_i64}),
    };
    assert!(owners.route(&command).is_err());
}

#[test]
fn idle_owner_pruning_keeps_active_calls_and_releases_response_map() {
    let mut owners = Owners::default();
    let key = "anonymous:fixture";
    let active = owners.owner(key);
    owners.record(key, Some("scope".into()), Some("response".into()));
    let Some(expired) = Instant::now().checked_sub(OWNER_IDLE_RETENTION + Duration::from_secs(1))
    else {
        return;
    };
    owners.entries.get_mut(key).unwrap().last_used = expired;
    owners.prune();
    assert!(owners.entries.contains_key(key));
    drop(active);
    owners.prune();
    assert!(!owners.entries.contains_key(key));
    assert!(owners.responses.is_empty());
}

#[test]
fn bound_owner_survives_idle_window_until_physical_deadline() {
    let mut owners = Owners::default();
    let (key, _) = owners
        .route(&request("key-a", Some("bound-logical"), None))
        .unwrap();
    let Some(expired) = Instant::now().checked_sub(OWNER_IDLE_RETENTION + Duration::from_secs(1))
    else {
        return;
    };
    owners.entries.get_mut(&key).unwrap().last_used = expired;
    owners.prune();
    assert!(owners.entries.contains_key(&key));
    owners.entries.get_mut(&key).unwrap().retain_until =
        Some(Instant::now() - Duration::from_secs(1));
    owners.prune();
    assert!(!owners.entries.contains_key(&key));
}
