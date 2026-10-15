use super::*;

#[tokio::test]
async fn discover_filters_all_matching_operations_without_flattening_or_extra_requests() {
    let mut fixture = Fixture::default();
    let result = fixture.run("openapi_discover", json!({})).await;
    assert_eq!(result["success"], true);
    assert_eq!(result["instance_id"], INSTANCE);
    assert_eq!(result["operations"].as_array().unwrap().len(), 3);
    assert_eq!(fixture.requests.len(), 1);
    assert_safe(&result);
    let mut fixture = Fixture::default();
    let result = fixture
        .run(
            "openapi_discover",
            json!({"search":"GET TRACE","tag":"Trace"}),
        )
        .await;
    assert_eq!(result["operations"].as_array().unwrap().len(), 2);
    let mut fixture = Fixture::default();
    let result = fixture
        .run("openapi_discover", json!({"tag":"AnnotationQueues"}))
        .await;
    assert_eq!(
        result["operations"][0]["operationId"],
        "annotationQueues_createQueue"
    );
    let mut spec = document();
    for n in 0..140 {
        spec["paths"][format!("/api/public/generated/{n}")] =
            json!({"get":{"operationId":format!("generated_{n}"),"parameters":[],"responses":{}}});
    }
    let mut fixture = with_document(spec);
    let result = fixture.run("openapi_discover", json!({})).await;
    assert_eq!(result["operations"].as_array().unwrap().len(), 143);
}
#[tokio::test]
async fn get_full_document_and_operation_preserve_types_cycles_and_inherited_parameter_override() {
    let mut fixture = Fixture::default();
    let result = fixture.run("openapi_get", json!({})).await;
    assert_eq!(result["document"], document());
    assert_eq!(fixture.requests.len(), 1);
    let mut fixture = Fixture::default();
    let result = fixture
        .run("openapi_get", json!({"operationId":"trace_list"}))
        .await;
    assert_eq!(
        result["operation"]["parameters"][0]["schema"]["nullable"],
        true
    );
    let parameters = result["parameters"].as_array().unwrap();
    assert_eq!(
        parameters.iter().filter(|p| p["name"] == "limit").count(),
        1
    );
    assert_eq!(
        parameters.iter().find(|p| p["name"] == "limit").unwrap()["description"],
        "Operation level overrides inherited parameter."
    );
    assert_eq!(
        result["components"]["schemas"]["Child"]["properties"]["parent"]["$ref"],
        "#/components/schemas/TraceWithFullDetails"
    );
    assert_eq!(
        result["components"]["schemas"].as_object().unwrap().len(),
        2
    );
    assert!(result["components"]["schemas"].get("Unrelated").is_none());
    let mut fixture = Fixture::default();
    let result = fixture
        .run("openapi_get", json!({"operationId":"trace_get"}))
        .await;
    assert_eq!(result["parameters"][0]["name"], "traceId");
    assert_eq!(result["parameters"][0]["required"], true);
    assert_safe(&result);
}
#[tokio::test]
async fn execute_encodes_path_as_single_segment_and_uses_selected_base_ignoring_servers() {
    for id in [INSTANCE, OTHER] {
        let mut fixture = Fixture::default();
        let result=fixture.run("openapi_execute",json!({"instance_id":id,"operationId":"trace_get","path":{"traceId":"a/b ?#%\\世界"}})).await;
        assert_eq!(result["success"], true);
        assert_eq!(fixture.requests.len(), 2);
        let url = fixture.requests[1]["url"].as_str().unwrap();
        assert!(
            url.contains("/api/public/traces/a%2Fb%20%3F%23%25%5C%E4%B8%96%E7%95%8C"),
            "{url}"
        );
        assert!(url.starts_with(if id == INSTANCE {
            "https://selected.example/prefix/"
        } else {
            "https://other.example/root/"
        }));
        assert_eq!(fixture.requests[1]["method"], "GET");
        assert_safe(&result);
    }
}
#[tokio::test]
async fn execute_query_form_scalars_default_exploded_arrays_and_comma_arrays() {
    let mut fixture = Fixture::default();
    let result=fixture.run("openapi_execute",json!({"operationId":"trace_list","query":{"tags":["a b","x&y"],"environment":["prod","test"],"limit":500,"name":"A & B"}})).await;
    assert_eq!(result["success"], true);
    let url = url::Url::parse(fixture.requests[1]["url"].as_str().unwrap()).unwrap();
    let pairs = url.query_pairs().into_owned().collect::<Vec<_>>();
    assert_eq!(
        pairs
            .iter()
            .filter(|(key, _)| key == "tags")
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>(),
        vec!["a b", "x&y"]
    );
    assert!(pairs.contains(&("environment".into(), "prod,test".into())));
    assert!(pairs.contains(&("limit".into(), "500".into())));
    assert!(pairs.contains(&("name".into(), "A & B".into())));
}
#[tokio::test]
async fn json_body_including_null_and_all_upstream_statuses_headers_and_raw_bytes_survive() {
    for body in [
        json!({"name":"Queue 世界","nested":[1,true,null]}),
        Value::Null,
    ] {
        for status in [200, 403, 404, 429, 500] {
            let upstream = json!({"error":{"code":"upstream_error","extension":{"raw":true}}});
            let mut fixture = Fixture {
                upstream: json_response(status, upstream.clone()),
                ..Fixture::default()
            };
            let result = fixture
                .run(
                    "openapi_execute",
                    json!({"operationId":"annotationQueues_createQueue","body":body}),
                )
                .await;
            assert_eq!(result["success"], true);
            assert_eq!(result["response"]["status"], status);
            assert_eq!(result["response"]["headers"]["x-upstream"], "retained");
            assert_eq!(result["response"]["body"], upstream);
            assert_eq!(
                STANDARD
                    .decode(result["response"]["body_base64"].as_str().unwrap())
                    .unwrap(),
                serde_json::to_vec(&upstream).unwrap()
            );
            let request = &fixture.requests[1];
            assert_eq!(request["method"], "POST");
            assert_eq!(request["headers"]["content-type"], "application/json");
            let bytes = STANDARD
                .decode(request["body_base64"].as_str().unwrap())
                .unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), body);
            assert_eq!(fixture.requests.len(), 2);
            assert_safe(&result);
        }
    }
    let bytes = [0, 0xff, b'X', b'\n'];
    let mut fixture = Fixture {
        upstream: response(503, &bytes),
        ..Fixture::default()
    };
    let result = fixture
        .run("openapi_execute", json!({"operationId":"trace_list"}))
        .await;
    assert_eq!(result["response"]["status"], 503);
    assert_eq!(
        STANDARD
            .decode(result["response"]["body_base64"].as_str().unwrap())
            .unwrap(),
        bytes
    );
    assert!(result["response"].get("body").is_none());
    assert_eq!(fixture.requests.len(), 2);
}

#[tokio::test]
async fn local_reference_uri_fragments_and_json_pointer_escapes_preserve_component_names() {
    let mut spec = document();
    spec["components"]["schemas"]["Name /~+"] = json!({"type":"string","enum":["exact"]});
    spec["paths"]["/api/public/traces"]["get"]["responses"]["200"]["content"]["application/json"]
        ["schema"] = json!({"$ref":"#/components/schemas/Name%20~1~0+"});
    let mut fixture = with_document(spec);
    let result = fixture
        .run("openapi_get", json!({"operationId":"trace_list"}))
        .await;
    assert_eq!(result["success"], true);
    assert_eq!(
        result["components"]["schemas"]["Name /~+"]["enum"][0],
        "exact"
    );
}

#[tokio::test]
async fn path_scalar_control_bytes_and_literal_plus_are_encoded_without_normalization_loss() {
    for (value, encoded) in [
        (json!("a\n\r\tb+c"), "a%0A%0D%09b%2Bc"),
        (json!(42), "42"),
        (json!(true), "true"),
        (json!("%2e%2e"), "%252e%252e"),
    ] {
        let mut fixture = Fixture::default();
        let result = fixture
            .run(
                "openapi_execute",
                json!({"operationId":"trace_get","path":{"traceId":value}}),
            )
            .await;
        assert_eq!(result["success"], true);
        assert_eq!(
            fixture.requests[1]["url"],
            format!("https://selected.example/prefix/api/public/traces/{encoded}")
        );
    }
}

#[test]
fn manifest_registers_independent_fixed_mcp_operations_with_one_source_for_argument_fields() {
    let manifest: Value = serde_yaml::from_str(include_str!("../../../manifest.yaml")).unwrap();
    let operations = manifest["managed_service"]["operations"]
        .as_array()
        .unwrap();
    for (handler, method, path, name) in [
        ("instances_list", "GET", "/instances", "instances.list"),
        (
            "openapi_discover",
            "POST",
            "/openapi/discover",
            "openapi.discover",
        ),
        ("openapi_get", "POST", "/openapi/get", "openapi.get"),
        (
            "openapi_execute",
            "POST",
            "/openapi/execute",
            "openapi.execute",
        ),
    ] {
        let operation = operations
            .iter()
            .find(|op| op["interface_id"] == format!("langfuse.{handler}"))
            .unwrap();
        assert_eq!(operation["method"], method);
        assert_eq!(
            operation["path"],
            format!("/api/console/managed-services/langfuse{path}")
        );
        assert_eq!(operation["mcp"]["name"], name);
        let mut names = std::collections::BTreeSet::new();
        for location in ["path", "query", "body"] {
            for field in operation["input_schema"]["properties"][location]["properties"]
                .as_object()
                .unwrap()
                .keys()
            {
                assert!(
                    names.insert(field),
                    "MCP arguments must not collide across HTTP containers"
                );
            }
        }
        if handler != "instances_list" {
            assert!(
                operation["input_schema"]["properties"]["path"]["properties"]
                    .as_object()
                    .unwrap()
                    .is_empty()
            );
            assert!(names.iter().any(|name| name.as_str() == "instance_id"));
        }
    }
}
