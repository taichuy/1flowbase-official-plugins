# Model pricing catalog

This directory is the sole upstream source for official USD model-pricing templates.
Rules use stable IDs. Consumers upsert matching IDs and never delete local rules merely
because a later catalog omits them. The initial catalog is intentionally empty: vendor
prices must be added with a cited source and review rather than guessed.

Run `node scripts/model-pricing-catalog.mjs --check` before review. Immutable signed
releases are created from tags named `model-pricing-v<catalog_version>`; the release
workflow signs the canonical `rules` bytes with the repository Ed25519 key.
