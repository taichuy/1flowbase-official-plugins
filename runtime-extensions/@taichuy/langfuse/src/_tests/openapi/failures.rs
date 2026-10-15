use super::*;

#[tokio::test]
async fn spec_404_returns_original_response_without_alternate_endpoint_probe_or_execute() {
    let mut fixture = Fixture {
        spec_status: 404,
        spec: br#"{"message":"endpoint unavailable","details":{"original":true}}"#.to_vec(),
        ..Fixture::default()
    };
    let result = fixture
        .run("openapi_execute", json!({"operationId":"trace_list"}))
        .await;
    assert_eq!(result["success"], false);
    assert_eq!(result["error"]["status"], 404);
    assert_eq!(
        result["error"]["details"]["response"]["body"]["details"]["original"],
        true
    );
    assert_eq!(fixture.requests.len(), 1);
    assert_safe(&result);
}
#[tokio::test]
async fn malformed_yaml_duplicate_operation_ids_and_bad_refs_fail_without_execution() {
    let mut cases = vec![Fixture {
        spec: b"paths: [invalid".to_vec(),
        ..Fixture::default()
    }];
    let mut spec = document();
    spec["paths"]["/duplicate"] = json!({"post":{"operationId":"trace_list","responses":{}}});
    cases.push(with_document(spec));
    for reference in [
        "#/components/schemas/Missing",
        "https://foreign.example/spec.yml#/Bad",
        "#/components/schemas/~bad",
        "#/components/schemas/%41",
    ] {
        let mut spec = document();
        spec["components"]["schemas"]["Child"]["properties"]["parent"]["$ref"] = json!(reference);
        cases.push(with_document(spec));
    }
    for mut fixture in cases {
        let result = fixture
            .run("openapi_execute", json!({"operationId":"trace_list"}))
            .await;
        assert_eq!(result["success"], false);
        assert_eq!(fixture.requests.len(), 1);
        assert_safe(&result);
    }
}
#[tokio::test]
async fn missing_unknown_and_unsafe_path_values_or_unsupported_encodings_never_execute() {
    for body in [
        json!({"operationId":"missing"}),
        json!({"operationId":"trace_get"}),
        json!({"operationId":"trace_get","path":{"traceId":"."}}),
        json!({"operationId":"trace_get","path":{"traceId":".."}}),
        json!({"operationId":"trace_get","path":{"traceId":["array"]}}),
        json!({"operationId":"trace_get","path":{"traceId":"ok","other":"extra"}}),
        json!({"operationId":"trace_list","query":{"unknown":"extra"}}),
        json!({"operationId":"trace_list","query":{"tags":[{"object":"unsupported"}]}}),
    ] {
        let mut fixture = Fixture::default();
        let result = fixture.run("openapi_execute", body).await;
        assert_eq!(result["success"], false);
        assert_eq!(fixture.requests.len(), 1);
    }
    for style in ["label", "matrix"] {
        let mut spec = document();
        spec["components"]["parameters"]["TraceId"]["style"] = json!(style);
        let mut fixture = with_document(spec);
        let result = fixture
            .run(
                "openapi_execute",
                json!({"operationId":"trace_get","path":{"traceId":"test"}}),
            )
            .await;
        assert_eq!(
            result["error"]["code"],
            "langfuse_openapi_parameter_encoding"
        );
        assert_eq!(fixture.requests.len(), 1);
    }
    for path in [
        "https://foreign.example/escape",
        "//foreign.example/escape",
        "/../escape",
        "/api/./escape",
        "/api/public?override=1",
    ] {
        let mut spec = document();
        spec["paths"][path] = json!({"get":{"operationId":"escape","responses":{}}});
        let mut fixture = with_document(spec);
        let result = fixture
            .run("openapi_execute", json!({"operationId":"escape"}))
            .await;
        assert_eq!(result["success"], false);
        assert_eq!(fixture.requests.len(), 1);
    }
    let mut spec = document();
    spec["paths"]["/api/public/traces"]["get"]["parameters"][1]["style"] = json!("deepObject");
    let mut fixture = with_document(spec);
    let result = fixture
        .run(
            "openapi_execute",
            json!({"operationId":"trace_list","query":{"tags":["test"]}}),
        )
        .await;
    assert_eq!(
        result["error"]["code"],
        "langfuse_openapi_parameter_encoding"
    );
    assert_eq!(fixture.requests.len(), 1);
}
#[tokio::test]
async fn caller_authorization_headers_method_and_absolute_url_override_rejected_before_callbacks() {
    for field in ["authorization", "headers", "url", "method"] {
        let mut fixture = Fixture::default();
        let mut body = json!({"operationId":"trace_list"});
        body[field] = json!("injected");
        let result = fixture.run("openapi_execute", body).await;
        assert_eq!(result["error"]["code"], "langfuse_openapi_input");
        assert!(fixture.requests.is_empty());
    }
}
#[tokio::test]
async fn schema_reference_cycles_are_kept_while_parameter_reference_cycles_fail_explicitly() {
    let mut spec = document();
    spec["components"]["parameters"]["TraceId"] = json!({"$ref":"#/components/parameters/TraceId"});
    let mut fixture = with_document(spec);
    let result = fixture
        .run("openapi_get", json!({"operationId":"trace_get"}))
        .await;
    assert_eq!(result["error"]["code"], "langfuse_openapi_ref_cycle");
    assert_eq!(fixture.requests.len(), 1);
}
#[tokio::test]
async fn host_transport_failure_preserves_safe_code_status_and_details() {
    let result=invoke("openapi_discover",json!({"instance_id":INSTANCE}),|service,request| match service {
        MultiplexHostService::PluginDataV1=>json!({"result":{"results":[{"result":"optional_row","row":{"values":{"instance_id":{"type":"string","value":INSTANCE},"name":{"type":"string","value":"fixture"},"base_url":{"type":"string","value":"https://selected.example/"}}}}],"replayed":false}}),
        MultiplexHostService::PluginCredentialV1=>json!({"result":{"value":{"public_key":"public-fixture","secret_key":"secret-fixture"}}}),
        MultiplexHostService::HttpEgressV1=>{assert!(request["url"].as_str().unwrap().ends_with("/generated/api/openapi.yml"));json!({"error":{"code":"network_egress_denied","status":403,"details":{"reason":"safe-policy-reason"}}})},
        _=>panic!("unexpected callback"),
    }).await;
    assert_eq!(result["error"]["code"], "network_egress_denied");
    assert_eq!(result["error"]["status"], 403);
    assert_eq!(result["error"]["details"]["reason"], "safe-policy-reason");
    assert_safe(&result);
}
