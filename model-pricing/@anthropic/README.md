# Claude 5.5 pricing notes

Verified on 2026-10-02 against [Anthropic pricing](https://platform.claude.com/docs/en/about-claude/pricing) and [model IDs](https://platform.claude.com/docs/en/about-claude/models/overview).
Prices are USD per million tokens for the first-party standard API with global routing.

| Model ID | Input | Output | Cache read / refresh | 5-minute cache write | 1-hour cache write |
| --- | --- | --- | --- | --- | --- |
| `claude-opus-5-5` | $4 | $20 | $0.20 | $5 | $8 |
| `claude-sonnet-5-5` | $2 | $10 | $0.20 | $2.50 | $4 |

The default cache-write rate covers the 300-second TTL. An explicit 3600-second TTL overrides only the cache-write rate. Cache reads cost 0.05 times base input for Opus 5.5 and 0.1 times base input for Sonnet 5.5. The full 1M-token context window uses standard pricing, so no input-length surcharge rule is needed.

Batch API pricing is 50% of standard rates (Sonnet 5.5 input/output: $1/$5 per million tokens). US-only inference (`inference_geo: "us"`) multiplies all token categories, including cache reads and writes, by 1.1. Schema v2 has no batch-mode or inference-geography condition; these modifiers are documented here and are not applied by these standard API JSON configurations.

`effective_from` records this catalog addition on 2026-10-02, not the vendor release date. Existing model configurations remain unchanged. No unpublished Haiku 5.5 pricing is included.
