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
    use tokio_tungstenite::tungstenite::{accept_with_config, Message};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let upstream = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut socket = accept_with_config(stream, Some(Default::default())).unwrap();
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
fn physical_identity_cannot_be_rebound_to_different_credentials() {
    let mut owners = Owners::default();
    let (first, _) = owners
        .route(&request("key-a", Some("same-logical"), None))
        .unwrap();
    owners.owner(&first);
    assert!(owners
        .route(&request("key-b", Some("same-logical"), None))
        .is_err());
    let command = ProviderStdioRequest {
        method: "transport_session".into(),
        input: json!({"logical_session_id":"same-logical", "generation":1,
            "worker_incarnation":9,"action":"close","deadline_unix_ms":4_102_444_800_000_i64}),
    };
    assert_eq!(owners.route(&command).unwrap().0, first);
    assert_eq!(owners.entries.len(), 1);
}

#[test]
fn physical_control_keeps_original_owners_across_configurations_and_workers() {
    let mut owners = Owners::default();
    let mut expected = Vec::new();
    for (generation, worker, credential) in [(1, 9, "key-a"), (2, 9, "key-b"), (1, 10, "key-a")] {
        let mut invocation = request(credential, Some("same-logical"), None);
        invocation.input["run_context"][TRANSPORT_SESSION_CONTEXT_KEY]["generation"] =
            json!(generation);
        invocation.input["run_context"][TRANSPORT_SESSION_CONTEXT_KEY]["worker_incarnation"] =
            json!(worker);
        expected.push((generation, worker, owners.route(&invocation).unwrap().0));
    }
    assert_ne!(expected[0].2, expected[1].2);
    assert_ne!(expected[0].2, expected[2].2);
    for (generation, worker, key) in &expected {
        let command = ProviderStdioRequest {
            method: "transport_session".into(),
            input: json!({"logical_session_id":"same-logical", "generation":generation,
                "worker_incarnation":worker,"action":"close","deadline_unix_ms":4_102_444_800_000_i64}),
        };
        // Repeated close must still reach the original ledger, not a newer owner.
        assert_eq!(&owners.route(&command).unwrap().0, key);
        assert_eq!(&owners.route(&command).unwrap().0, key);
    }
    for (generation, worker) in [(99, Some(9)), (1, Some(99)), (1, None)] {
        let command = ProviderStdioRequest {
            method: "transport_session".into(),
            input: json!({"logical_session_id":"same-logical", "generation":generation,
                "worker_incarnation":worker,"action":"close","deadline_unix_ms":4_102_444_800_000_i64}),
        };
        if worker.is_some() {
            assert!(owners.route(&command).is_err());
        } else {
            let missing = owners.route(&command).unwrap().0;
            assert!(expected.iter().all(|(_, _, key)| key != &missing));
        }
    }
}

#[test]
fn control_error_serialization_preserves_identity_rejection() {
    let command = ProviderStdioRequest {
        method: "transport_session".into(),
        input: json!({"logical_session_id":"unknown", "generation":1,
            "worker_incarnation":9,"action":"close","deadline_unix_ms":4_102_444_800_000_i64}),
    };
    let error = Owners::default().route(&command).unwrap_err();
    let response = error_response(error.context("control dispatch"));
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["kind"], "provider_transport_unavailable");
    assert!(response["result"].is_null());
    assert_eq!(
        error_response(anyhow!("malformed control"))["error"]["kind"],
        "provider_invalid_response"
    );
}

#[test]
fn same_configuration_generations_retain_response_history_owner() {
    let mut owners = Owners::default();
    let mut invocation = request("key-a", Some("same-logical"), None);
    let first = owners.route(&invocation).unwrap().0;
    invocation.input["run_context"][TRANSPORT_SESSION_CONTEXT_KEY]["generation"] = json!(2);
    assert_eq!(owners.route(&invocation).unwrap().0, first);
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
    let command = ProviderStdioRequest {
        method: "transport_session".into(),
        input: json!({"logical_session_id":"bound-logical", "generation":1,
            "worker_incarnation":9,"action":"close","deadline_unix_ms":4_102_444_800_000_i64}),
    };
    assert!(owners.route(&command).is_err());
}
