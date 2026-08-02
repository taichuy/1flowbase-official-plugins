# Official Agent Flow Templates

This directory stores official AgentFlow workflow templates and the generated
catalog consumed by 1flowbase.

## Layout

- `@<organization>/<workflow_id>/template.json`: one exported Agent Flow template.
- `catalog/v1/**`: generated discovery catalog maintained by the shared extension publisher.
- `releases/v1/catalog.json`: generated, history-preserving signed release catalog.

Each `template.json` is the exact release artifact and must contain
`template_id` (UUID), `release_version` (integer >= 1),
`exported_from_system_version`, `exported_at` (RFC3339), `application`,
`flow_document`, and `dependencies`. The workflow never derives a business
version: changing artifact bytes requires increasing `release_version`, while
reusing the same template ID and version with a different SHA-256 is rejected.

Published release-catalog versions remain enumerable and point only to immutable
GitHub Release assets. Each version records the SHA-256 of the original
artifact bytes plus `algorithm: ed25519`, `key_id`, and a base64 signature over
those same bytes. Do not edit the generated catalog by hand.

Production signing requires the Actions secrets
`OFFICIAL_PLUGIN_SIGNING_PRIVATE_KEY_PEM` (the shared official Ed25519 PKCS#8 PEM) and
`OFFICIAL_PLUGIN_SIGNING_KEY_ID`. The corresponding Ed25519 public key must be
distributed through the 1flowbase trusted-key configuration and pinned to that
key ID; it must not be learned from the catalog being verified. Private keys
must never be committed. The deterministic key embedded in the test file is a
fixture only and is not trusted for releases.
