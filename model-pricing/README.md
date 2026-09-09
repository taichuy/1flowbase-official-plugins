# Model pricing catalog

This directory is the sole upstream source for official USD model-pricing templates.
Providers are vendors, not plugins. Human-maintained sources live at
`@<provider_code>/<model-key>/pricing.json`; the file's `upstream_model_id` is authoritative,
so model IDs containing path separators do not leak into repository paths. The directory
provider and the source `provider_code` must match.

The publisher deterministically generates:

- `catalog/v1/index.json`
- `catalog/v1/pages/<page>.json`
- `catalog/v1/search-index.json`
- `_maintenance/catalog-state.json`
- `catalog/v1/catalog.json` and `dist/catalog-seed.json` compatibility snapshots

Rules use immutable IDs. After an ID has been published, changing any of its pricing content is
rejected; a price revision must use a new rule ID and effective range. Consumers insert missing
IDs and never update or delete an installed local rule. The initial catalog contains one
`zero / any` global fallback. Reviewed vendor prices are added as exact provider/model rules and
take precedence over the fallback; prices must not be guessed.

Time-of-day prices remain separate physical rules selected by effective range, timezone,
weekday, and local window. Conditional standard API prices use the versioned
`rating_policy`; v1 permits deterministic input-token tiers. V2 `token_pricing`
uses one positive integer `unit_size`, complete input/output/cache-hit decimal rates,
and cache-write rates with either `unit_price` or `by_ttl_seconds`. Optional strictly
ascending input tiers replace all four rates for the full request at the highest
matching threshold. TTL buckets are positive integer seconds; missing write TTL
evidence must not be guessed. New revisions have higher priority and a later
effective date, leaving published IDs and historical pricing unchanged. Coding plans, credits,
subscriptions, and unpublished prices are not part of this USD catalog.

Run `node scripts/model-pricing-catalog.mjs` after editing a source and
`node scripts/model-pricing-catalog.mjs --check` before review. Immutable signed
releases are created from tags named `model-pricing-v<catalog_version>`; the release
workflow signs the canonical `rules` bytes with the repository Ed25519 key.
