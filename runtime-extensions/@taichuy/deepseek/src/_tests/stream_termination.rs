use crate::{
    build_stream_termination_metadata, is_sse_done_line, normalize_finish_reason,
    ProviderFinishReason, StreamTermination,
};

#[test]
fn unknown_raw_finish_reason_is_retained_with_done_termination_evidence() {
    let metadata = build_stream_termination_metadata(
        Some("insufficient_system_resource"),
        StreamTermination::Done,
    );

    assert_eq!(
        normalize_finish_reason(Some("insufficient_system_resource"), &[]),
        ProviderFinishReason::Unknown
    );
    assert_eq!(
        metadata["raw_finish_reason"],
        "insufficient_system_resource"
    );
    assert_eq!(metadata["raw_finish_reason_status"], "unrecognized");
    assert_eq!(metadata["transport_termination"], "done");
}

#[test]
fn missing_finish_reason_is_recorded_as_eof_not_as_an_unknown_value() {
    let metadata = build_stream_termination_metadata(None, StreamTermination::Eof);

    assert_eq!(
        normalize_finish_reason(None, &[]),
        ProviderFinishReason::Unknown
    );
    assert_eq!(metadata["raw_finish_reason"], serde_json::Value::Null);
    assert_eq!(metadata["raw_finish_reason_status"], "missing");
    assert_eq!(metadata["transport_termination"], "eof");
}

#[test]
fn normal_stop_and_tool_calls_keep_their_recognized_terminal_semantics() {
    assert_eq!(
        normalize_finish_reason(Some("stop"), &[]),
        ProviderFinishReason::Stop
    );
    assert_eq!(
        normalize_finish_reason(Some("tool_calls"), &[]),
        ProviderFinishReason::ToolCall
    );
    assert_eq!(
        build_stream_termination_metadata(Some("stop"), StreamTermination::Done)
            ["raw_finish_reason_status"],
        "recognized"
    );
}

#[test]
fn stream_error_is_recorded_as_distinct_termination_evidence() {
    assert_eq!(
        build_stream_termination_metadata(None, StreamTermination::Error)["transport_termination"],
        "error"
    );
}

#[test]
fn done_sentinel_is_distinguished_from_ordinary_eof() {
    assert!(is_sse_done_line("data: [DONE]"));
    assert!(is_sse_done_line(" data: [DONE] "));
    assert!(!is_sse_done_line("data: {\"choices\": []}"));
}
