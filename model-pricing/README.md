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

Only existing provider/model sources are updated. All four valid nonnegative rates
must be available; otherwise the whole model is skipped without deletion or partial updates.
An explicit zero is valid, while a missing field is never assumed to be zero or the input rate.
Prices are converted to decimal strings in USD per 1,000,000 tokens. LiteLLM's per-token
rates are scaled with decimal arithmetic. Provider identity is checked to avoid borrowing
relay prices. `kimi` maps to international `moonshotai` / `moonshot`, not `moonshotai-cn`;
`google` maps to `google` / `gemini`, and `zai` to `zai` / `z_ai`.
Unmapped providers (including `zero/any`), disabled or non-current configurations, and
sources with `extensions.price_sync: false` are skipped.

The sync changes only the four default rates. Existing context, time-window and cache-TTL
rules, ordering and other configuration remain intact; it neither imports nor recalculates
conditional rules. Those rules continue to require separate review when defaults change.
Changed configurations receive a new UUID and use the sync observation time as
`effective_from`; this is not a claimed vendor price-change date. Their `extensions.source`
becomes `community_pricing`, and `extensions.price_sync` records the observation time and
source URL/provider/model/field for each meter. Previous vendor verification metadata is
removed from changed configurations. Numerically unchanged prices retain their ID,
timestamps, provenance and bytes, so idle runs do not create commits.

On change, `catalog_version` advances and all catalog, search, seed and maintenance
artifacts are regenerated and checked before the workflow commits to `main`. Historical
IDs remain in maintenance state. Both upstream snapshots must download successfully;
network/JSON/invalid-catalog errors fail the job before syncing, and transient requests
retry at most three times with a 60-second timeout per attempt. Logs and the Action summary
identify updated, unchanged and skipped models. Signed tagged releases remain a separate
existing release operation; hourly sync does not create tags or signed release assets.

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
