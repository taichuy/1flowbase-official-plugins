use super::*;

#[test]
fn original_objects_null_and_missing_fields_survive_all_parsers() {
    for original in [
        json!({"code":"context_length_exceeded","type":"invalid_request_error","param":null,"message":"line one\nline two","limit":128000,"future":{"nested":[1,null]}}),
        json!({"code":"future_supplier_code","message":"untouched","type":null,"future":true}),
        json!({"message":null,"param":["future"]}),
        Value::Null,
    ] {
        let body = json!({"error":original}).to_string();
        let parsed = parse_http(&body).unwrap();
        let http = error(parsed.get("error"), &body, Some(400), Some(&body));
        let websocket = websocket(&json!({"type":"error","error":original}).to_string()).unwrap();
        let response = json!({"error":original});
        let failed = failed(Some(&response));
        for typed in [http.clone(), websocket, failed] {
            assert_eq!(typed.kind, ProviderRuntimeErrorKind::ProviderUpstreamError);
            let details = typed.provider_details.as_ref().unwrap();
            assert_eq!(details.get("upstream_error"), Some(&original));
            assert_eq!(details["semantic_terminal"], true);
            let safe = crate::recovery_diagnostics::safe_error(&anyhow::Error::new(typed.clone()));
            assert_eq!(safe, typed);
        }
        assert_eq!(http.provider_details.as_ref().unwrap()["raw_body"], body);
        assert_eq!(http.provider_details.as_ref().unwrap()["status_code"], 400);
    }
    let missing = failed(Some(&json!({})));
    assert!(missing
        .provider_details
        .unwrap()
        .get("upstream_error")
        .is_none());
}

#[test]
fn only_real_http_status_is_recorded_and_body_is_exact() {
    for body in [
        "",
        "plain\nbody\n",
        "<html>error</html>",
        " \r\n\t ",
        "  {\"error\":null} \n",
    ] {
        let parsed = parse_http(body);
        let error = error(
            parsed.as_ref().and_then(|value| value.get("error")),
            body,
            Some(503),
            Some(body),
        );
        let details = error.provider_details.unwrap();
        assert_eq!(details["raw_body"], body);
        assert_eq!(details["status_code"], 503);
        assert_eq!(details["semantic_terminal"], false);
    }
    let event = websocket(r#"{"type":"error","status":400,"error":{"message":"bad"}}"#).unwrap();
    assert!(event.provider_details.unwrap().get("status_code").is_none());
    for status in [429, 500, 503] {
        assert_eq!(
            error(None, "busy", Some(status), None)
                .provider_details
                .unwrap()["semantic_terminal"],
            false
        );
    }
}

#[test]
fn mixed_json_sse_preserves_the_error_and_full_body() {
    let original = json!({"message":"instructions required","code":null,"extra":{"x":1}});
    let body = format!(
        "{}\ndata: {{\"type\":\"response.failed\"}}\n\n",
        json!({"error":original})
    );
    let parsed = parse_http(&body).unwrap();
    let typed = error(parsed.get("error"), &body, Some(400), Some(&body));
    assert_eq!(typed.message, "instructions required");
    assert_eq!(
        typed.provider_details.as_ref().unwrap()["upstream_error"],
        original
    );
    assert_eq!(typed.provider_details.as_ref().unwrap()["raw_body"], body);
}
