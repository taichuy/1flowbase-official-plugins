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
fn anthropic_cache_write_ttls_and_ordinary_input_survive_partial_snapshots() {
    let mut usage = normalize_usage(&serde_json::json!({
        "input_tokens": 10, "cache_read_input_tokens": 50, "cache_creation_input_tokens": 5000,
        "cache_creation": {"ephemeral_5m_input_tokens": 3000, "ephemeral_1h_input_tokens": 2000}
    }));
    assert_eq!(usage.input_cache_miss_tokens, Some(10));
    assert_eq!(usage.cache_read_tokens, Some(50));
    assert_eq!(usage.cache_write_tokens, Some(5000));
    merge_usage(
        &mut usage,
        normalize_usage(&serde_json::json!({"output_tokens": 20})),
    );
    assert_eq!(
        usage.cache_write_by_ttl_seconds.as_ref().unwrap()["300"],
        3000
    );
    assert_eq!(
        usage.cache_write_by_ttl_seconds.as_ref().unwrap()["3600"],
        2000
    );
    assert_eq!(usage.input_cache_miss_tokens, Some(10));
    merge_usage(
        &mut usage,
        normalize_usage(&serde_json::json!({
            "cache_creation": {"ephemeral_5m_input_tokens": 4000}
        })),
    );
    assert_eq!(
        usage.cache_write_by_ttl_seconds.as_ref().unwrap()["300"],
        4000
    );
    assert_eq!(
        usage.cache_write_by_ttl_seconds.as_ref().unwrap()["3600"],
        2000
    );
}

#[test]
fn missing_anthropic_ttl_is_not_guessed_and_write_only_usage_is_present() {
    let usage = normalize_usage(&serde_json::json!({"cache_creation_input_tokens": 5000}));
    assert!(usage.has_any_value());
    assert_eq!(usage.cache_write_tokens, Some(5000));
    assert_eq!(usage.cache_write_by_ttl_seconds, None);
    assert_eq!(usage.input_cache_miss_tokens, None);
}
