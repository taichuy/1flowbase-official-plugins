# Model pricing catalog

[中文说明](README.zh-CN.md)

This directory is the sole upstream source for official USD model-pricing templates.
Pricing sources live at `@<provider_code>/<model-key>/pricing.json`.
The directory provider must match `provider_code`; `upstream_model_id` is authoritative.
There is exactly one current standard configuration for each provider/model pair.
Installed user pricing is independently owned: consumers must not overwrite manual edits.

A source uses `1flowbase.model-pricing-source/v2`: identity, schedule and four default
`*_token_unit_size` / `*_token_unit_price` pairs are top-level. Prices are decimal
strings. `rules` is a required array (empty means defaults only), containing only
`when` and sparse `overrides`. Canonical fields use meters `input`, `output`,
`cache_hit`, and `cache_write`. Matching rules apply in array order; later matching
rules replace only their stated fields. No rating-policy sidecar or defaults in rules.

Conditions support `input_tokens: {operator: "gt" | "gte", value: integer}`,
`cache_write_ttl_seconds`, RFC3339 `effective_from` / `effective_to`, IANA `timezone`,
`weekday_mask` (1..127), and paired `local_time_start` / `local_time_end` in HH:MM:SS.
Local windows require an explicit timezone and use an inclusive start / exclusive end.
Other schedule fields inherit the top-level configuration. TTL conditions only override
cache-write fields. Actual TTL evidence is required; an unmatched TTL keeps the default.
See `schemas/model-pricing/v2/` and `scripts/model-pricing-validation.mjs`.

The publisher deterministically generates `catalog/v1/index.json`, `pages/<page>.json`,
`search-index.json`, `catalog.json`, `dist/catalog-seed.json`, and maintenance state.
Published pages contain at most 20 model configurations, matching the console default page size.
Read the index total before fetching only the required pages; synchronization stages one page at a time.
All payload schema constants are v2. The physical `catalog/v1/` URL is retained to avoid
changing discovery endpoints; it does not indicate the payload schema version.

## Migrating v1

Run `node scripts/convert-model-pricing-v2.mjs [repo-root]`, then
`node scripts/model-pricing-catalog.mjs`. The offline converter rejects unknown shapes,
leaves valid v2 source files untouched, and preserves legacy prices without model-specific
price constants. For old sources without separate cache-write pricing, the legacy input
rate is copied to cache-write defaults and conditional overrides. This preserves existing
billing behavior; it is not a newly claimed vendor quote.

Complete daily partitions become a default plus differing windows. Contiguous date
periods become an initial default plus later date overrides. Increasing-priority revisions
become the latest standard (Astra and Fable); superseded standards remain in historical
releases, not the current model source. Input tiers preserve gt/gte boundaries. The
approved Fable sample uses 300-second write pricing as its default and overrides 3600 seconds.

Migration of immutable state is allowed only once from v1: the converter verifies the old
aggregate checksum, every original source checksum and state ID, then compares each
converted configuration with the converted published content. It updates only equivalent
surviving IDs' checksums and retains retired IDs. It cannot bless arbitrary price changes.
After migration, published IDs remain immutable; revisions require a new ID. The old
schemas and legacy fixture remain solely as offline conversion/history evidence.

Run `node scripts/model-pricing-catalog.mjs --check` before review. Tests:
`node --test scripts/_tests/model-pricing-catalog.test.mjs scripts/_tests/convert-model-pricing-v2.test.mjs`.
Signed releases use tags `model-pricing-v<catalog_version>`; the release workflow signs
canonical aggregate `rules` bytes with Ed25519. Coding plans, credits, subscriptions,
and unpublished prices are outside this catalog.

## Hourly community price sync

The `model-pricing-sync` GitHub Action runs on `main` every hour at minute 17 (UTC),
and supports manual dispatch. It reads [Models.dev](https://models.dev/api.json) first,
then supplements missing or invalid individual fields from the matching native provider
in [LiteLLM](https://github.com/BerriAI/litellm/blob/main/model_prices_and_context_window.json).
No SDK or gateway dependency is installed; the converter uses Node.js built-ins.
Upstream MIT notices are retained in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

The sync discovers the union of provider/model identities in both upstream catalogs,
including new providers and models. Local files no longer act as an allowlist. Existing
aliases remain: `kimi` maps to international `moonshotai` / `moonshot`, `google` to
`google` / `gemini`, and `zai` to `zai`. Other providers retain their upstream identity.
Rates never cross provider or regional boundaries. Community-managed sources absent
from both snapshots are removed; local/manual sources, `zero/any`, disabled sources,
and explicit `extensions.price_sync: false` configurations are retained.

`_maintenance/upstream-models.json` mirrors the complete identity set and upstream
pricing, marking each model `synchronized` or `unavailable` with a reason. Publishing a
standard price configuration requires all four valid, nonnegative default rates.
Missing values are never replaced with zero or the input rate; explicit zero is valid.
Image, audio, video and other meters unsupported by the four-token-meter contract remain
in the inventory without fabricated standard prices. A complete inventory does not imply
that every model has a usable standard quote. Existing sources with incomplete new quotes
are retained but marked unavailable in the inventory, rather than verified as current.

Rates use decimal USD per million tokens. The sync also imports explicit context tiers,
preferring Models.dev `cost.tiers` and otherwise using LiteLLM's exact
`_above_<N>k_tokens` thresholds. It never guesses a threshold from the legacy
`context_over_200k` field name. LiteLLM's documented one-hour and twelve-hour TTL buckets, including combined context
conditions, are imported. Previously synchronized rules are removed when upstream withdraws
them. Other manual TTL, compound conditions and time-window rates remain and require separate review. Batch, flex, priority and
other service tiers unsupported by the condition contract are not imported.

Default-rate or context-rule changes receive a new UUID and observation time, preserving
historical identities. Previous vendor verification metadata is removed. Unchanged
configurations retain their IDs, timestamps and bytes; replaying the same snapshots is idle.
Additions, updates or removals advance the catalog version and regenerate indexes,
pages, search, aggregate, seed and maintenance state. A failed write restores the entire
pricing tree, including new/deleted sources and generated pages.

Both snapshots must download successfully; malformed data and network failures fail the
job. Requests retry at most three times with a 60-second timeout per attempt. The Action
summary and `model-pricing-sync-report` artifact expose discovery, additions, updates,
removals and unavailable quotes. GitHub may delay scheduled runs. Signed tagged releases
remain a separate operation; hourly synchronization does not create signed release assets.

Preview live data without writing:

```bash
node scripts/model-pricing/sync.mjs --dry-run
```

Replay previously downloaded snapshots (both files are required):

```bash
node scripts/model-pricing/sync.mjs --dry-run \
  --models-dev-file /tmp/models-dev.json --litellm-file /tmp/litellm.json
```

Run the synchronization and catalog tests:

```bash
node --test scripts/model-pricing/_tests/*.test.mjs \
  scripts/_tests/model-pricing-catalog.test.mjs scripts/_tests/convert-model-pricing-v2.test.mjs
node scripts/model-pricing-catalog.mjs --check
```

Search metadata is also split into shards of at most 100 entries at the default page size. The index declares each shard’s providers and pages: provider filters download only matching shards; other searches read bounded metadata shards. The complete search snapshot remains for existing consumers.
