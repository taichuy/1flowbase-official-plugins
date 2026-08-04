# Runtime Extensions

Runtime extensions are executed by the host but implement provider-specific behavior.

Current subtrees:

- `@taichuy/<provider_code>/` for official model provider runtime extensions

Every runtime manifest declares a required `publisher_namespace`. Official manifests
use `1flowbase`; this publisher identity determines the runtime catalog organization
and ID independently of the repository owner and display/legal `vendor` metadata.
Manifests also publish `slot_codes` and may publish `keywords` (defaulting to an empty
list) for catalog classification and search.
