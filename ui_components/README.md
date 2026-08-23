# UI component catalog

`ui_components/` is the sole official source for maintained UI component samples. A
canonical record lives at `@<source>/<group>/<record>.json`. Only committed records in
that inventory are published; npm exports and installed modules are never discovered.

Each record has a stable `component_code`, display metadata, opaque `import_code` and
`source_code`, official ownership (`origin`, `source`, and `group`), upstream identity and
version, a record version, search keywords, and an update timestamp. The publisher checks
the JSON contract and grouping identity, but deliberately does not parse, compile,
evaluate, or resolve either code field. Maintainers are responsible for sample usability.

The deterministic publisher writes:

- `catalog/v1/index.json` with pagination, search, download, and update metadata
- `catalog/v1/pages/<page>.json` in `component_code` order
- `catalog/v1/search-index.json`
- `dist/catalog-seed.json`, whose component and semantic SHA-256 digests are self-verifying
- `_maintenance/catalog-state.json`

Consumers identify records by `component_code`. An official sync applies
`authoritative_source_group_replace`: replace the selected `source` / `group` inventory,
including deletion of absent official records, without changing user-owned custom records.
The catalog supplies raw component content only; activation, package installation, and
runtime availability remain outside this contract.

After editing metadata or a canonical record, rebuild locally:

```bash
node scripts/ui-component-catalog-publisher.mjs
node scripts/ui-component-catalog-publisher.mjs --check
```

Immutable releases use `ui-component-catalog-v<catalog_version>` tags. The release asset is
`ui-component-catalog-v<catalog_version>.json`; CI publishes its SHA-256 digest, Ed25519
signature, and signed record. Published evidence is registered in
`releases/v1/catalog.json`.

Schemas are maintained under `schemas/ui-component/v1/`. This catalog intentionally does
not use the generic Extension Catalog entry schema because raw import/source content and
source-group replacement are UI-component-specific semantics.
