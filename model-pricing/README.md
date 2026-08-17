# Model pricing catalog

This directory is the sole upstream source for official USD model-pricing templates.
Rules use stable IDs. Consumers upsert matching IDs and never delete user-owned local rules
merely because a later catalog omits them. Model pricing is maintained independently from
runtime-extension manifests: providers are vendors, not plugins. The initial catalog contains
one `zero / any` global fallback so billing upgrades remain usable without manufacturing one
zero-price rule per plugin model. Reviewed vendor prices are added as exact provider/model
rules and take precedence over the fallback; prices must not be guessed.

Run `node scripts/model-pricing-catalog.mjs --check` before review. Immutable signed
releases are created from tags named `model-pricing-v<catalog_version>`; the release
workflow signs the canonical `rules` bytes with the repository Ed25519 key.
