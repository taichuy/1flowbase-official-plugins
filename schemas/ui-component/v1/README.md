# UI Component Catalog v1 schemas

`source.schema.json` defines each human-maintained canonical record. The publisher adds
only `source_locator` and `source_checksum`; `component.schema.json` defines that published
shape used in Seed and page outputs. Code fields are opaque non-empty strings and are never
parsed or resolved.

`catalog-source.schema.json` versions the inventory. `seed.schema.json`,
`index.schema.json`, `page.schema.json`, and `search-index.schema.json` define deterministic
consumer outputs. `release.schema.json` defines evidence signed over the exact Seed bytes.

The v1 sync contract is `authoritative_source_group_replace`: `component_code` is identity,
`source` and `group` bound the replacement set, and `version` is the record update version.
