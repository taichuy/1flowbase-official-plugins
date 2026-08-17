# Model pricing catalog

This directory is the sole upstream source for official USD model-pricing templates.
Rules use stable IDs. Consumers upsert matching IDs and never delete local rules merely
because a later catalog omits them. The catalog publishes explicit zero-cost defaults for
every static LLM model shipped by an official runtime extension so billing upgrades do not
disable an existing model. Reviewed vendor prices replace these defaults as new catalog
versions; prices must not be guessed.

Run `node scripts/model-pricing-catalog.mjs --check` before review. Immutable signed
releases are created from tags named `model-pricing-v<catalog_version>`; the release
workflow signs the canonical `rules` bytes with the repository Ed25519 key.
