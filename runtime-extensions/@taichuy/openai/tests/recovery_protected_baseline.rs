//! Source gate for the nine cycle-2 recovery regressions. The behavior tests remain in
//! `stdio_worker.rs`; this fixture makes accidental removal/renaming visible without changing
//! their protected expectations or running the worker suite during packet assembly.

const STDIO_WORKER: &str = include_str!("stdio_worker.rs");

#[test]
fn cycle2_protected_recovery_matrix_remains_present_and_unweakened() {
    let protected = [
        "websocket_previous_response_can_fallback_to_sse_without_prior_websocket_session",
        "websocket_continuation_reconnect_keeps_original_turn_state",
        "websocket_previous_response_retries_stream_close_without_seen_cursor",
        "websocket_previous_response_reconnect_replays_turn_state",
        "websocket_previous_response_reconnects_instead_of_http_fallback",
        "websocket_transport_falls_back_to_sse_before_response_events",
        "websocket_transport_falls_back_to_sse_after_lifecycle_frame_without_output",
        "websocket_previous_response_unavailable_retries_with_full_context",
        "websocket_proxy_failure_after_cursor_retries_without_stale_turn_state",
    ];

    for test_name in protected {
        assert_eq!(
            STDIO_WORKER.matches(&format!("fn {test_name}()")).count(),
            1,
            "protected cycle-2 fixture {test_name} must remain exactly once"
        );
    }

    for protected_expectation in [
        "HTTP-only provider should still be able to fallback",
        "continuation reconnect should keep sticky turn state",
        "websocket cursor stream close should reconnect",
        "continuation should not fall back to HTTP SSE",
        "unavailable cursor should recover with full-context retry",
        "proxy failure should reconnect without stale state",
        "provider_metadata\"][\"transport\"]",
    ] {
        assert!(
            STDIO_WORKER.contains(protected_expectation),
            "protected cycle-2 expectation must remain: {protected_expectation}"
        );
    }
}
