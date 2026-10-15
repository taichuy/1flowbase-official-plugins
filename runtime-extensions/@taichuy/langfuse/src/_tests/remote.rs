use crate::remote::*;
use serde_json::json;
#[test]
fn acknowledgements_reject_partial_warning_and_malformed_counts() {
    assert!(validate_ack(&json!({})).is_ok());
    assert!(validate_ack(&json!({"partialSuccess":{"rejectedSpans":"0"}})).is_ok());
    for value in [
        json!({"partialSuccess":{"rejectedSpans":"2"}}),
        json!({"partialSuccess":{"errorMessage":"warning"}}),
        json!({"partialSuccess":{"rejectedSpans":-1}}),
        json!([]),
    ] {
        assert!(validate_ack(&value).is_err());
    }
}
#[test]
fn pagination_rejects_conflicting_cursor_and_missing_numbered_metadata() {
    assert_eq!(
        next_cursor(&json!({"meta":{"cursor":"opaque"}})).unwrap(),
        Some("opaque".into())
    );
    assert_eq!(
        next_cursor(&json!({"meta":{"nextCursor":null}})).unwrap(),
        None
    );
    for value in [
        json!({"meta":{"cursor":""}}),
        json!({"meta":{"cursor":"a","nextCursor":"b"}}),
        json!({"meta":{"unexpected":1}}),
    ] {
        assert!(next_cursor(&value).is_err());
    }
    assert!(numbered_more(&json!({"meta":{"page":1,"totalPages":2}}), 1, false).unwrap());
    assert!(numbered_more(&json!({"meta":{"page":1,"totalPages":2}}), 1, true).is_err());
    assert!(numbered_more(&json!({"meta":{"page":2,"totalPages":2}}), 1, false).is_err());
}
#[test]
fn remote_trace_binding_is_checked_before_import() {
    assert!(bound_items(&json!({"data":[{"traceId":"foreign"}]}), "selected").is_err());
    assert!(bound_items(&json!({"data":[{"traceId":"selected"}]}), "selected").is_ok());
}
