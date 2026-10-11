# Claude 5.5 pricing notes

Verified on 2026-10-11 against [Anthropic pricing](https://platform.claude.com/docs/en/about-claude/pricing) and the [Haiku 5.5 model overview](https://platform.claude.com/docs/en/models/haiku-5-5/overview).
Prices are USD per million tokens for the first-party standard API with global routing.

| Model ID | Input | Output | Cache read / refresh | 5-minute cache write | 1-hour cache write |
| --- | --- | --- | --- | --- | --- |
| `claude-opus-5-5` | $4 | $20 | $0.20 | $5 | $8 |
| `claude-sonnet-5-5` | $2 | $10 | $0.10 | $2.50 | $4 |
| `claude-haiku-5-5`, input up to 100,000 tokens | $0.10 | $0.50 | $0.01 | $0.125 | $0.20 |
| `claude-haiku-5-5`, input over 100,000 tokens | $0.50 | $2.50 | $0.05 | $0.625 | $1 |

The default cache-write rate covers the 300-second TTL. An explicit 3600-second TTL overrides only the cache-write rate. Cache reads cost 0.05 times base input for Opus 5.5 and Sonnet 5.5, and 0.1 times base input for Haiku 5.5. Opus and Sonnet use standard rates across the full 1M-token context window.

Haiku's prompt-length threshold counts all input tokens, including cache reads and cache writes. Exactly 100,000 tokens uses the lower rates; above that threshold, higher prices apply to the whole request, including output and cache meters. Rules apply in array order: the 1-hour write override, the large-input override, then the combined large-input/1-hour override. This preserves the $1 write rate when both conditions match.

Batch API pricing is 50% of standard rates, including Haiku's long-context tier. Opus 5.5 Fast mode input/output costs $8/$40 per million tokens. US-only inference (`inference_geo: "us"`) multiplies all token categories, including cache reads and writes and Haiku's long-context tier, by 1.1. Schema v2 has no mode or inference-geography condition; these modifiers are documented here and are not applied by these standard API JSON configurations.

For Haiku 5.5 and the corrected Sonnet 5.5 configuration, `effective_from` records this catalog update on 2026-10-11, not the vendor release date. Sonnet receives a new immutable configuration ID for the cache-read correction from $0.20 to $0.10; the previous ID remains in maintenance history. Opus 5.5 pricing is unchanged.
