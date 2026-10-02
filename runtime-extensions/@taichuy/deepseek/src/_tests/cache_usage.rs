use crate::*;

#[test]
fn normalize_usage_reads_commandcode_cached_tokens() {
    let usage = normalize_usage(&json!({
        "prompt_tokens": 3833,
        "completion_tokens": 21,
        "total_tokens": 3854,
        "prompt_tokens_details": {"cached_tokens": 3712},
        "completion_tokens_details": {"reasoning_tokens": 19}
    }));
    assert_eq!(usage.input_tokens, Some(3833));
    assert_eq!(usage.input_cache_hit_tokens, Some(3712));
    assert_eq!(usage.cache_read_tokens, Some(3712));
    assert_eq!(usage.input_cache_miss_tokens, None);
    assert_eq!(usage.output_tokens, Some(21));
    assert_eq!(usage.reasoning_tokens, Some(19));
    assert_eq!(usage.total_tokens, Some(3854));
}

#[test]
fn normalize_usage_preserves_native_cache_values_including_zero() {
    for native_hit in [0, 40] {
        let usage = normalize_usage(&json!({
            "prompt_tokens": 100,
            "prompt_cache_hit_tokens": native_hit,
            "prompt_cache_miss_tokens": 100 - native_hit,
            "prompt_tokens_details": {"cached_tokens": 80}
        }));
        assert_eq!(usage.input_cache_hit_tokens, Some(native_hit));
        assert_eq!(usage.cache_read_tokens, Some(native_hit));
        assert_eq!(usage.input_cache_miss_tokens, Some(100 - native_hit));
    }
}

#[test]
fn normalize_usage_distinguishes_missing_cache_from_reported_zero() {
    for details in [Value::Null, json!({}), json!({"cached_tokens": -1})] {
        let usage = normalize_usage(&json!({"prompt_tokens_details": details}));
        assert_eq!(usage.input_cache_hit_tokens, None);
        assert_eq!(usage.cache_read_tokens, None);
    }
    let usage = normalize_usage(&json!({
        "prompt_cache_hit_tokens": null,
        "prompt_tokens_details": {"cached_tokens": 0}
    }));
    assert_eq!(usage.input_cache_hit_tokens, Some(0));
    assert_eq!(usage.cache_read_tokens, Some(0));
}

#[tokio::test]
async fn commandcode_stream_preserves_cached_tokens_in_snapshot_and_result() {
    let (base_url, capture) = tests::capture_streaming_chat_request_with_usage(json!({
        "prompt_tokens": 3833, "completion_tokens": 21, "total_tokens": 3854,
        "prompt_tokens_details": {"cached_tokens": 3712}
    }));
    let mut events = Vec::new();
    let result = handle_invoke_request_streaming(
        json!({
            "contract_version": "1flowbase.provider/v2",
            "provider_instance_id": "provider-test", "provider_code": "deepseek",
            "protocol": "openai_compatible", "model": "deepseek/deepseek-v4.1-flash",
            "provider_config": {"base_url": base_url, "api_key": "test-key"},
            "messages": [{"role": "user", "content": "Reply OK."}]
        }),
        |event| {
            events.push(event.clone());
            Ok(())
        },
    )
    .await
    .expect("streaming invoke should succeed");
    capture.join().expect("capture thread should finish");
    assert_eq!(result.usage.input_cache_hit_tokens, Some(3712));
    assert_eq!(result.usage.cache_read_tokens, Some(3712));
    assert!(events.iter().any(|event| matches!(event,
        ProviderStreamEvent::UsageSnapshot { usage }
            if usage.input_cache_hit_tokens == Some(3712)
                && usage.cache_read_tokens == Some(3712))));
}
