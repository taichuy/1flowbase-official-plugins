use crate::remote::*;
use serde_json::{json, Value};

// Sanitized Bull job shape returned for nonempty OTLP in Langfuse v4.
// All identities, storage locations and credentials below are fixture-only.
pub(crate) fn queue_ack() -> Value {
    json!({
        "name":"otel-ingestion-job", "id":"42",
        "data":{
            "name":"otel-ingestion-job", "id":"fixture-job-id",
            "timestamp":"2026-10-15T10:00:00.125Z",
            "payload":{
                "data":{"fileKey":"otel/fixture-project/2026/10/15/10/00/fixture.json","publicKey":"fixture-public"},
                "authCheck":{"validKey":true,"scope":{"projectId":"fixture-project","accessLevel":"project","orgId":"fixture-org"}},
                "propagatedHeaders":{}, "ingestionVersion":"4"
            }
        },
        "opts":{"attempts":3,"backoff":{"type":"exponential","delay":5000},"removeOnComplete":true},
        "progress":0, "returnvalue":null, "stacktrace":null, "priority":0,
        "attemptsStarted":0, "attemptsMade":0, "stalledCounter":0,
        "timestamp":1792058400125u64, "queueQualifiedName":"bull:otel-ingestion-queue"
    })
}

#[test]
fn nonempty_queue_acceptance_requires_complete_positive_vendor_receipt() {
    // Actual saved nonempty HTTP200 response has stacktrace:null, not [].
    assert!(validate_ack(&queue_ack()).is_ok());
    let mut empty_trace = queue_ack();
    empty_trace["stacktrace"] = json!([]);
    assert!(validate_ack(&empty_trace).is_ok());
    let mut shard = queue_ack();
    shard["queueQualifiedName"] = json!("bull:otel-ingestion-queue-2");
    assert!(validate_ack(&shard).is_ok());
    let mut propagated_headers = queue_ack();
    propagated_headers["data"]["payload"]["propagatedHeaders"] =
        json!({"warning":"fixture-request-header","errors":"opaque request metadata"});
    assert!(validate_ack(&propagated_headers).is_ok());
    for (pointer, replacement) in [
        ("/name", json!("another-job")),
        ("/data/name", json!("another-job")),
        ("/id", json!(null)),
        ("/data/id", json!("")),
        ("/data/timestamp", json!("not-a-timestamp")),
        ("/data/payload/data/fileKey", json!(null)),
        ("/data/payload/data/publicKey", json!("")),
        ("/data/payload/authCheck/validKey", json!(false)),
        (
            "/data/payload/authCheck/scope/accessLevel",
            json!("organization"),
        ),
        ("/data/payload/authCheck/scope/projectId", json!(null)),
        ("/queueQualifiedName", json!("bull:unrelated-queue")),
        ("/timestamp", json!(0)),
        ("/opts", json!(null)),
        ("/progress", json!("partial")),
        ("/returnvalue", json!({"error":"failed"})),
        ("/stacktrace", json!(["private error body"])),
        ("/stacktrace", json!("invalid diagnostic")),
        ("/attemptsMade", json!(1)),
    ] {
        let mut ack = queue_ack();
        *ack.pointer_mut(pointer).unwrap() = replacement;
        let error = validate_ack(&ack).unwrap_err();
        assert_eq!(error.code, "langfuse_ack");
        assert!(error.details.is_none());
    }
    for field in [
        "partialSuccess",
        "partial_success",
        "error",
        "errors",
        "warning",
        "warnings",
        "failedReason",
    ] {
        let mut ack = queue_ack();
        ack[field] = json!("private rejection");
        assert!(validate_ack(&ack).is_err());
    }
    assert!(validate_ack(&json!({"name":"otel-ingestion-job","id":"42"})).is_err());
}
#[test]
fn acknowledgements_reject_partial_warning_and_malformed_counts() {
    assert!(validate_ack(&json!({})).is_ok());
    assert!(validate_ack(&json!({"partialSuccess":{"rejectedSpans":"0"}})).is_ok());
    for value in [
        json!({"partialSuccess":{"rejectedSpans":"2"}}),
        json!({"partialSuccess":{"errorMessage":"warning"}}),
        json!({"partialSuccess":{"rejectedSpans":-1}}),
        json!({"partialSuccess":{"warnings":["warning"]}}),
        json!({"partialSuccess":{"rejectedSpans":"0","rejected_spans":"2"}}),
        json!({"partialSuccess":null}),
        json!({"error":"not accepted"}),
        json!([]),
    ] {
        assert!(validate_ack(&value).is_err());
    }
}

#[test]
fn duplicate_versions_keep_latest_complete_raw_row_independent_of_page_order() {
    let older = json!({"id":"observation","traceId":"trace","startTime":"2026-10-15T00:00:00Z",
        "output":{"createdAt":"semantic nested field"},"metadata":{"1flowbase_agent_log":"identical capsule"},
        "createdAt":"2026-10-15T09:00:00Z","updatedAt":"2026-10-15T09:00:00Z"});
    let mut latest = older.clone();
    // RFC3339 instants, rather than lexical strings, determine the latest row.
    latest["createdAt"] = json!("2026-10-15T11:30:00+02:00");
    latest["updatedAt"] = json!("2026-10-15T11:30:00+02:00");
    let mut intermediate = older.clone();
    intermediate["updatedAt"] = json!("2026-10-15T09:15:00Z");
    for values in [
        vec![older.clone(), latest.clone(), intermediate.clone()],
        vec![latest.clone(), intermediate.clone(), older.clone()],
        vec![intermediate.clone(), older.clone(), latest.clone()],
    ] {
        assert_eq!(complete(values).unwrap(), vec![latest.clone()]);
    }
    let mut same_instant = latest.clone();
    same_instant["updatedAt"] = json!("2026-10-15T09:30:00.000Z");
    assert_eq!(
        complete(vec![latest.clone(), same_instant.clone()]).unwrap(),
        complete(vec![same_instant, latest]).unwrap()
    );
}

#[test]
fn duplicate_versions_reject_all_real_fact_changes() {
    let original = json!({"id":"observation","traceId":"trace","startTime":"2026-10-15T00:00:00Z",
        "endTime":"2026-10-15T00:01:00Z", "output":"complete content", "usageDetails":{"input":4},
        "metadata":{"1flowbase_agent_log":"capsule"},"createdAt":"2026-10-15T09:00:00Z","updatedAt":"2026-10-15T09:00:00Z"});
    for (pointer, replacement) in [
        ("/output", json!("changed")),
        ("/usageDetails/input", json!(5)),
        ("/metadata/1flowbase_agent_log", json!("changed capsule")),
        ("/startTime", json!("2026-10-15T00:00:01Z")),
        ("/endTime", json!("2026-10-15T00:01:01Z")),
        ("/traceId", json!("different trace")),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        changed["updatedAt"] = json!("2026-10-15T10:00:00Z");
        for values in [
            vec![original.clone(), changed.clone()],
            vec![changed, original.clone()],
        ] {
            assert_eq!(
                complete(values).unwrap_err().code,
                "langfuse_observation_conflict"
            );
        }
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
