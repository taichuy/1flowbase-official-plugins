# Extension Catalog v1

Every official extension category publishes the same static contract:

- `<category>/catalog/v1/index.json`
- `<category>/catalog/v1/pages/<page>.json`
- `<category>/catalog/v1/search-index.json`
- `<category>/_maintenance/catalog-state.json`

`index.json` and numbered pages are the client contract described by
`catalog.schema.json`. `search-index.json`, described by `search-index.schema.json`,
contains normalized list metadata and verified page locators so consumers can filter
the complete snapshot before applying pagination. `_maintenance/catalog-state.json`
is generator state only;
clients must not fetch or depend on it.

New sources use `<category>/@<organization>/<artifact>/catalog-entry.json` and the
`source-entry.schema.json` contract. The generator derives `id`, `category`,
`organization`, `artifact`, and `catalog_page` from the source path and pagination.
Canonical entries override an equally identified legacy publication entry.

Every page entry exposes `slot_codes` and `keywords` arrays. Canonical source entries
may omit them and receive empty arrays. Runtime entries instead project both arrays
from the runtime manifest through `official-registry.json`. Runtime identity is
`publisher_namespace/provider_code`; repository ownership and the display/legal
`vendor` field never determine catalog identity. The registry's `manifest_locator`
is generated from the actual repository-relative manifest path and is the sole source
locator used by runtime catalog publication; it is not derived from publisher identity.

The generator sorts by `id`, uses deterministic page numbers and cursors, and keeps
`generated_at` stable while the source fingerprint and page size are unchanged.
The search index uses the same ordering and source fingerprint, and each search record
names its page cursor, checksum, and locator. `index.json.search_index` checksums the
entire search index.
Empty categories still publish page 1 so every category has the same traversal
contract.

AgentFlow templates, MCP bundle manifests, the i18n source tree, and runtime
extension manifests are discovered only under canonical `@organization/artifact`
directories. MCP `catalog.json`, i18n Seed releases, and `official-registry.json`
contribute published download metadata but are not alternate source layouts or v1
client contracts.

Generate all categories with:

```bash
node scripts/update-extension-catalog.mjs
```

Detect drift without writes with:

```bash
node scripts/update-extension-catalog.mjs --check
```
