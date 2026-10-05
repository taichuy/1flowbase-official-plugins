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
    assert_eq!(usage.total_tokens, Some(5080));
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

#[test]
fn anthropic_total_includes_cache_reads_and_writes_once() {
    let usage = normalize_usage(&serde_json::json!({
        "input_tokens": 283719, "output_tokens": 2813,
        "cache_read_input_tokens": 494000, "cache_creation_input_tokens": 49400,
        "cache_creation": {"ephemeral_5m_input_tokens": 30000, "ephemeral_1h_input_tokens": 19400}
    }));
    assert_eq!(usage.total_tokens, Some(829932));
    assert_eq!(usage.input_tokens, Some(283719));
    assert_eq!(usage.input_cache_miss_tokens, Some(283719));
    assert_eq!(usage.cache_read_tokens, Some(494000));
    assert_eq!(usage.cache_write_tokens, Some(49400));
}

#[test]
fn anthropic_stream_total_uses_latest_counters_without_accumulating_snapshots() {
    let mut usage = normalize_usage(&serde_json::json!({
        "input_tokens": 10, "output_tokens": 1,
        "cache_read_input_tokens": 50, "cache_creation_input_tokens": 5000
    }));
    assert_eq!(usage.total_tokens, Some(5061));
    for _ in 0..2 {
        merge_usage(
            &mut usage,
            normalize_usage(&serde_json::json!({"output_tokens": 20})),
        );
        assert_eq!(usage.total_tokens, Some(5080));
    }
    merge_usage(
        &mut usage,
        normalize_usage(&serde_json::json!({
            "cache_read_input_tokens": 60, "cache_creation_input_tokens": 0
        })),
    );
    assert_eq!(usage.total_tokens, Some(90));
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(20));
    assert_eq!(usage.cache_write_tokens, Some(0));
}

#[test]
fn anthropic_total_preserves_missing_counts_and_zero_counts() {
    for raw in [
        serde_json::json!({"input_tokens": 10, "cache_read_input_tokens": 50}),
        serde_json::json!({"output_tokens": 20, "cache_read_input_tokens": 50}),
        serde_json::json!({"cache_creation_input_tokens": 5000}),
    ] {
        assert_eq!(normalize_usage(&raw).total_tokens, None);
    }
    for (raw, total) in [
        (
            serde_json::json!({"input_tokens": 0, "output_tokens": 0, "cache_read_input_tokens": 50}),
            50,
        ),
        (
            serde_json::json!({"input_tokens": 0, "output_tokens": 0}),
            0,
        ),
        (
            serde_json::json!({"input_tokens": 10, "output_tokens": 20}),
            30,
        ),
    ] {
        assert_eq!(normalize_usage(&raw).total_tokens, Some(total));
    }
}

#[test]
fn anthropic_total_does_not_wrap_or_retain_stale_total_on_overflow() {
    let mut usage = normalize_usage(&serde_json::json!({"input_tokens": 1, "output_tokens": 2}));
    merge_usage(
        &mut usage,
        normalize_usage(&serde_json::json!({"cache_read_input_tokens": u64::MAX})),
    );
    assert_eq!(usage.total_tokens, None);
    assert_eq!(usage.cache_read_tokens, Some(u64::MAX));
}
