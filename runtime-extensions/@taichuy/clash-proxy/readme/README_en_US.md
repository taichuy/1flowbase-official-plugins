# Clash / Mihomo Proxy

`clash-proxy` exposes selected proxies from an encrypted HTTPS Clash/Mihomo subscription through
the stable `1flowbase.network_egress_provider/v1` worker ABI. It rejects base64 payloads, proxy
providers, and V2Ray JSON.

The host passes the secret only at process start as a private config file. Its JSON value contains
one `subscription_url`; the provider fetches it over HTTPS with the standard `clash.meta` client
identifier and projects the YAML document's `proxies` array into isolated egresses. Other
top-level subscription settings (rules, groups, DNS, listeners, and proxy providers) are ignored.
Node names may use Unicode or emoji and remain display-only. Each `provider_egress_key` is a
stable ASCII SHA-256 fingerprint of the complete node configuration, so a subscription reorder
does not change keys and equally named but differently configured nodes remain distinct. Exact
duplicate nodes are rejected.
Supported remote node types are `ss`, `ssr`, `socks5`, `http`, `vmess`, `vless`, `trojan`,
`hysteria`, `hysteria2`, `tuic`, `snell`, `shadowquic`, `anytls`, `gost-relay`, and `mieru`; their
Mihomo node options are preserved.

The wrapper keeps a bounded pool of at most four signed, bundled Mihomo Alpha cores, keyed by the
selected egress. Concurrent and consecutive leases for the same egress reuse its private
`127.0.0.1` mixed HTTP listener while retaining independent lease and cleanup tokens. Releasing the
last lease makes the core idle; an idle core is removed after 60 seconds or earlier when the pool
needs capacity. Provider shutdown stops every core and removes every ephemeral configuration file.
TUN, system proxy changes, public listeners, and runtime downloads are not supported.

The default capacity is derived from a conservative local-source budget, not from the 1 GiB
`RLIMIT_AS` value: a 2 GiB pool RSS budget minus a 256 MiB worker allowance, divided by a 384 MiB
per-core peak RSS allowance, yields four cores. The ignored Linux source-integration benchmark
starts a real bundled Mihomo under the 1 GiB address-space limit, probes its listener, records
`VmRSS`/`VmHWM`, and rejects a peak above that allowance. `RLIMIT_AS` is inherited per process and
is not an aggregate process-tree memory limit; the keyed pool's count bound is the aggregate
runtime guard.

The bundled core is GPL-3.0. Each signed release includes its SHA-256, GPL notice, and
corresponding-source pointer in `_meta/official-release.json`.
