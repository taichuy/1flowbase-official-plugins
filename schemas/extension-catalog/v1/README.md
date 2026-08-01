# Extension Catalog v1

Every official extension category publishes the same static contract:

- `<category>/catalog/v1/index.json`
- `<category>/catalog/v1/pages/<page>.json`
- `<category>/_maintenance/catalog-state.json`

`index.json` and numbered pages are the client contract described by
`catalog.schema.json`. `_maintenance/catalog-state.json` is generator state only;
clients must not fetch or depend on it.

New sources use `<category>/@<organization>/<artifact>/catalog-entry.json` and the
`source-entry.schema.json` contract. The generator derives `id`, `category`,
`organization`, `artifact`, and `catalog_page` from the source path and pagination.
Canonical entries override an equally identified legacy publication entry.

The generator sorts by `id`, uses deterministic page numbers and cursors, and keeps
`generated_at` stable while the source fingerprint and page size are unchanged.
Empty categories still publish page 1 so every category has the same traversal
contract.

Legacy AgentFlow templates, MCP `catalog.json`, i18n catalog Seed releases, and the
runtime extension `official-registry.json` are read only by publisher adapters in
`scripts/extension-catalog.mjs`. They are not alternate v1 client contracts.

Generate all categories with:

```bash
node scripts/update-extension-catalog.mjs
```

Detect drift without writes with:

```bash
node scripts/update-extension-catalog.mjs --check
```
