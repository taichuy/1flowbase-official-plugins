use crate::dsml::{DsmlParsingOutcome, DsmlStreamDecoder, DsmlToolCall};
use crate::{
    build_chat_completion_body, finalize_dsml_stream, merge_tool_call_deltas,
    parse_dsml_tool_calls_enabled, ProviderFinishReason, ProviderInvocationInput,
    ProviderStreamEvent, ToolCallBuilder,
};
use serde_json::json;

const COMPLETE_DSML: &str = "\n\n<｜DSML｜tool_calls>\n<｜DSML｜invoke name=\"lookup\">\n<｜DSML｜parameter name=\"query\" string=\"true\">refund status</｜DSML｜parameter>\n<｜DSML｜parameter name=\"limit\" string=\"false\">3</｜DSML｜parameter>\n</｜DSML｜invoke>\n<｜DSML｜invoke name=\"notify\">\n<｜DSML｜parameter name=\"urgent\" string=\"false\">true</｜DSML｜parameter>\n</｜DSML｜invoke>\n</｜DSML｜tool_calls>";

#[test]
fn ac_003_complete_dsml_split_at_every_character_becomes_tool_calls() {
    let mut decoder = DsmlStreamDecoder::default();
    let input = format!("I will check.{COMPLETE_DSML}");
    let mut visible = String::new();

    for character in input.chars() {
        if let Some(delta) = decoder.push(&character.to_string()) {
            visible.push_str(&delta);
        }
    }
    let resolution = decoder.finish(false);
    visible.push_str(&resolution.trailing_text);

    assert_eq!(visible, "I will check.");
    assert_eq!(resolution.outcome, DsmlParsingOutcome::Parsed);
    assert_eq!(
        resolution.tool_calls,
        vec![
            DsmlToolCall {
                name: "lookup".to_string(),
                arguments: json!({"query": "refund status", "limit": 3}),
            },
            DsmlToolCall {
                name: "notify".to_string(),
                arguments: json!({"urgent": true}),
            },
        ]
    );
}

#[test]
fn ac_005_malformed_dsml_becomes_output_protocol_failure() {
    let malformed = "<think>inspect schema</think><think>inspect schema</｜｜DSML｜｜parameter>\n</invoke>\n</｜｜DSML｜｜tool_calls>";
    let mut decoder = DsmlStreamDecoder::default();
    let mut visible = String::new();
    for character in malformed.chars() {
        if let Some(delta) = decoder.push(&character.to_string()) {
            visible.push_str(&delta);
        }
    }
    let resolution = decoder.finish(false);
    visible.push_str(&resolution.trailing_text);

    assert_eq!(
        visible,
        "<think>inspect schema</think><think>inspect schema"
    );
    assert_eq!(resolution.outcome, DsmlParsingOutcome::InvalidProtocol);
    assert!(resolution.tool_calls.is_empty());
    assert_eq!(
        resolution.protocol_failure.unwrap().error_code,
        "invalid_marker"
    );
}

#[test]
fn ac_005_truncated_matching_envelope_becomes_output_protocol_failure() {
    let truncated = "answer\n\n<｜DSML｜tool_calls>\n<｜DSML｜invoke name=\"lookup\">\n";
    let mut decoder = DsmlStreamDecoder::default();
    let mut visible = decoder.push(truncated).unwrap_or_default();
    let resolution = decoder.finish(false);
    visible.push_str(&resolution.trailing_text);

    assert_eq!(visible, "answer");
    assert_eq!(resolution.outcome, DsmlParsingOutcome::InvalidProtocol);
    assert!(resolution.tool_calls.is_empty());
    let failure = resolution.protocol_failure.unwrap();
    assert_eq!(failure.error_code, "incomplete_envelope");
    assert_eq!(
        failure.candidate,
        "\n\n<｜DSML｜tool_calls>\n<｜DSML｜invoke name=\"lookup\">\n"
    );
}

#[test]
fn ac_004_structured_tool_calls_take_precedence_without_consuming_dsml_text() {
    let mut decoder = DsmlStreamDecoder::default();
    let mut visible = decoder.push(COMPLETE_DSML).unwrap_or_default();
    let resolution = decoder.finish(true);
    visible.push_str(&resolution.trailing_text);

    assert_eq!(visible, COMPLETE_DSML);
    assert_eq!(
        resolution.outcome,
        DsmlParsingOutcome::StructuredToolCallsPrecedence
    );
    assert!(resolution.tool_calls.is_empty());
}

#[test]
fn ac_002_plain_trailing_newlines_are_not_lost() {
    let plain = "ordinary answer\n\n";
    let mut decoder = DsmlStreamDecoder::default();
    let mut visible = decoder.push(plain).unwrap_or_default();
    let resolution = decoder.finish(false);
    visible.push_str(&resolution.trailing_text);

    assert_eq!(visible, plain);
    assert_eq!(resolution.outcome, DsmlParsingOutcome::NoMatchPassthrough);
}

#[test]
fn ac_003_complete_no_argument_tool_call_is_supported() {
    let input = "<｜DSML｜tool_calls>\n<｜DSML｜invoke name=\"get_context\">\n\n</｜DSML｜invoke>\n</｜DSML｜tool_calls>";
    let mut decoder = DsmlStreamDecoder::default();
    let visible = decoder.push(input).unwrap_or_default();
    let resolution = decoder.finish(false);

    assert!(visible.is_empty());
    assert_eq!(resolution.outcome, DsmlParsingOutcome::Parsed);
    assert_eq!(
        resolution.tool_calls,
        vec![DsmlToolCall {
            name: "get_context".to_string(),
            arguments: json!({}),
        }]
    );
}

#[test]
fn ac_003_mixed_text_preserves_suffix_after_the_canonical_tool_delta() {
    let input = format!("Before{COMPLETE_DSML}\nAfter");
    let mut decoder = DsmlStreamDecoder::default();
    let mut text = decoder.push(&input).unwrap_or_default();
    let mut events = Vec::new();
    let mut tool_calls = Vec::new();

    let outcome = finalize_dsml_stream(
        Some(decoder),
        &json!("resp_mixed"),
        false,
        &mut text,
        &mut events,
        &mut tool_calls,
    );

    assert_eq!(outcome, Some(DsmlParsingOutcome::Parsed));
    assert_eq!(text, "Before\nAfter");
    assert_eq!(tool_calls.len(), 2);
    assert!(matches!(
        events.first(),
        Some(ProviderStreamEvent::ToolCallDelta { .. })
    ));
    assert!(matches!(
        events.last(),
        Some(ProviderStreamEvent::TextDelta { delta }) if delta == "\nAfter"
    ));
}

#[test]
fn ac_005_multiple_dsml_envelopes_are_an_ambiguous_protocol_failure() {
    let input = format!("{COMPLETE_DSML}{COMPLETE_DSML}");
    let mut decoder = DsmlStreamDecoder::default();
    let mut visible = decoder.push(&input).unwrap_or_default();
    let resolution = decoder.finish(false);
    visible.push_str(&resolution.trailing_text);

    assert!(visible.is_empty());
    assert_eq!(resolution.outcome, DsmlParsingOutcome::InvalidProtocol);
    assert!(resolution.tool_calls.is_empty());
    assert_eq!(
        resolution.protocol_failure.unwrap().error_code,
        "ambiguous_envelope"
    );
}

#[test]
fn ac_004_incomplete_standard_tool_call_delta_still_blocks_dsml_parsing() {
    let mut decoder = DsmlStreamDecoder::default();
    let mut text = decoder.push(COMPLETE_DSML).unwrap_or_default();
    let mut events = Vec::new();
    let mut builders = Vec::new();
    merge_tool_call_deltas(
        Some(&json!([{
            "index": 0,
            "id": "call_incomplete"
        }])),
        &mut builders,
        &mut events,
    );
    let structured_tool_calls_seen = !builders.is_empty();
    let mut tool_calls = builders
        .into_iter()
        .filter_map(ToolCallBuilder::into_tool_call)
        .collect::<Vec<_>>();

    let outcome = finalize_dsml_stream(
        Some(decoder),
        &json!("resp_precedence"),
        structured_tool_calls_seen,
        &mut text,
        &mut events,
        &mut tool_calls,
    );

    assert_eq!(
        outcome,
        Some(DsmlParsingOutcome::StructuredToolCallsPrecedence)
    );
    assert_eq!(text, COMPLETE_DSML);
    assert!(tool_calls.is_empty());
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ProviderStreamEvent::ToolCallDelta { .. }))
            .count(),
        1
    );
}

#[test]
fn ac_001_model_parameter_is_local_only_and_defaults_off() {
    let disabled = provider_input(json!({}));
    assert!(!parse_dsml_tool_calls_enabled(&disabled).expect("default should be valid"));

    let enabled = provider_input(json!({"parse_dsml_tool_calls": true}));
    assert!(parse_dsml_tool_calls_enabled(&enabled).expect("boolean should be valid"));
    let body = build_chat_completion_body(&enabled).expect("request body should build");
    assert!(body.get("parse_dsml_tool_calls").is_none());
}

#[test]
fn ac_001_model_parameter_rejects_non_boolean_values() {
    let input = provider_input(json!({"parse_dsml_tool_calls": "true"}));
    let error = parse_dsml_tool_calls_enabled(&input).expect_err("string must be rejected");
    assert_eq!(error.to_string(), "parse_dsml_tool_calls must be a boolean");
}

#[test]
fn ac_003_parsed_dsml_emits_canonical_delta_and_tool_call_finish() {
    let mut decoder = DsmlStreamDecoder::default();
    let mut text = decoder
        .push(&format!("Checking.{COMPLETE_DSML}"))
        .unwrap_or_default();
    let mut events = Vec::new();
    let mut tool_calls = Vec::new();

    let outcome = finalize_dsml_stream(
        Some(decoder),
        &json!("resp_1"),
        false,
        &mut text,
        &mut events,
        &mut tool_calls,
    );
    let finish_reason = if outcome == Some(DsmlParsingOutcome::Parsed) {
        ProviderFinishReason::ToolCall
    } else {
        ProviderFinishReason::Stop
    };

    assert_eq!(text, "Checking.");
    assert_eq!(finish_reason, ProviderFinishReason::ToolCall);
    assert_eq!(tool_calls.len(), 2);
    assert_eq!(tool_calls[0].id, "call_dsml_resp_1_1");
    assert_eq!(tool_calls[0].name, "lookup");
    assert_eq!(
        tool_calls[0].arguments,
        json!({"query": "refund status", "limit": 3})
    );
    assert!(matches!(
        &events[0],
        ProviderStreamEvent::ToolCallDelta { call_id, .. }
            if call_id == "call_dsml_resp_1_1"
    ));
}

#[test]
fn ac_005_invalid_dsml_emits_typed_failure_and_error_finish() {
    let mut decoder = DsmlStreamDecoder::default();
    let mut text = decoder
        .push("prefix</｜｜DSML｜｜tool_calls>")
        .unwrap_or_default();
    let mut events = Vec::new();
    let mut tool_calls = Vec::new();

    let outcome = finalize_dsml_stream(
        Some(decoder),
        &json!("resp_invalid"),
        false,
        &mut text,
        &mut events,
        &mut tool_calls,
    );
    let finish_reason = match outcome {
        Some(DsmlParsingOutcome::InvalidProtocol) => ProviderFinishReason::Error,
        _ => ProviderFinishReason::Stop,
    };

    assert_eq!(text, "prefix");
    assert!(tool_calls.is_empty());
    assert!(matches!(
        events.as_slice(),
        [ProviderStreamEvent::OutputProtocolFailure { failure }]
            if failure.protocol == "dsml"
                && failure.error_code == "invalid_marker"
                && failure.provider_details["candidate_preview"] == "</｜｜DSML｜｜tool_calls>"
    ));
    assert_eq!(finish_reason, ProviderFinishReason::Error);
}

fn provider_input(model_parameters: serde_json::Value) -> ProviderInvocationInput {
    serde_json::from_value(json!({
        "contract_version": "1flowbase.provider/v2",
        "provider_instance_id": "provider-test",
        "provider_code": "deepseek",
        "protocol": "openai_compatible",
        "model": "deepseek-v4-flash",
        "provider_config": {
            "base_url": "https://api.deepseek.com",
            "api_key": "test-key"
        },
        "messages": [{"role": "user", "content": "hello"}],
        "model_parameters": model_parameters
    }))
    .expect("fixture should satisfy provider input contract")
}
