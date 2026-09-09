use super::*;

// AC3: optional fields preserve the existing ABI and survive wire snapshots.
#[test]
fn usage_wire_preserves_optional_cache_write_buckets() {
    let legacy: ProviderUsage =
        serde_json::from_value(serde_json::json!({"input_tokens": 10})).unwrap();
    assert_eq!(legacy.cache_write_by_ttl_seconds, None);
    assert_eq!(legacy.input_cache_miss_tokens, None);
    let usage: ProviderUsage = serde_json::from_value(serde_json::json!({
        "cache_write_tokens": 5000,
        "cache_write_by_ttl_seconds": {"300": 3000, "3600": 2000},
        "input_cache_miss_tokens": 10
    }))
    .unwrap();
    assert!(usage.has_any_value());
    let wire = serde_json::to_value(&usage).unwrap();
    assert_eq!(
        wire["cache_write_by_ttl_seconds"],
        serde_json::json!({"300": 3000, "3600": 2000})
    );
    assert_eq!(
        serde_json::from_value::<ProviderUsage>(wire).unwrap(),
        usage
    );
}

#[test]
fn chat_cached_input_is_read_and_output_cached_tokens_are_never_write() {
    let usage = normalize_chat_usage(&serde_json::json!({
        "prompt_tokens": 100, "completion_tokens": 10,
        "prompt_tokens_details": {"cached_tokens": 40},
        "completion_tokens_details": {"cached_tokens": 7}
    }));
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.input_cache_miss_tokens, None);
    assert_eq!(usage.cache_read_tokens, Some(40));
    assert_eq!(usage.cache_write_tokens, None);
    assert_eq!(usage.cache_write_by_ttl_seconds, None);
}

#[test]
fn responses_cached_input_is_read_without_inventing_write_usage() {
    let usage = normalize_usage(&serde_json::json!({
        "input_tokens": 100, "output_tokens": 10,
        "input_tokens_details": {"cached_tokens": 40},
        "output_tokens_details": {"cached_tokens": 7}
    }));
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.cache_read_tokens, Some(40));
    assert_eq!(usage.cache_write_tokens, None);
    assert_eq!(usage.cache_write_by_ttl_seconds, None);
}

#[test]
fn chat_input_cache_writes_are_distinct_from_cached_reads() {
    let usage = normalize_chat_usage(&serde_json::json!({
        "prompt_tokens": 100,
        "prompt_tokens_details": {"cached_tokens": 40, "cache_write_tokens": 30},
        "completion_tokens_details": {"cached_tokens": 7}
    }));
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.cache_read_tokens, Some(40));
    assert_eq!(usage.cache_write_tokens, Some(30));
    assert_eq!(usage.input_cache_miss_tokens, None);
    assert_eq!(usage.cache_write_by_ttl_seconds, None);
}

#[test]
fn responses_input_cache_writes_follow_official_prompt_caching_example() {
    let usage = normalize_usage(&serde_json::json!({
        "input_tokens": 15000,
        "input_tokens_details": {"cached_tokens": 12000, "cache_write_tokens": 3000}
    }));
    assert_eq!(usage.input_tokens, Some(15000));
    assert_eq!(usage.cache_read_tokens, Some(12000));
    assert_eq!(usage.cache_write_tokens, Some(3000));
    assert_eq!(usage.input_cache_miss_tokens, None);
    assert_eq!(usage.cache_write_by_ttl_seconds, None);
}
