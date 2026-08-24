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
duplicate nodes are rejected. A subscription may project at most 256 egresses so the provider's
listener and file-descriptor usage remains bounded.
Supported remote node types are `ss`, `ssr`, `socks5`, `http`, `vmess`, `vless`, `trojan`,
`hysteria`, `hysteria2`, `tuic`, `snell`, `shadowquic`, `anytls`, `gost-relay`, and `mieru`; their
Mihomo node options are preserved.

The wrapper starts at most one signed, bundled Mihomo Alpha core for each provider worker
generation. Every projected egress receives a private `127.0.0.1` mixed listener whose `proxy`
field is pinned directly to that node's stable internal fingerprint. Distinct nodes can therefore
serve concurrent leases without changing a shared global selection, and duplicate display names
remain safe. Concurrent leases retain independent lease and cleanup tokens. Releasing the final
lease makes the provider core idle; it is removed after 60 seconds. Provider shutdown stops the
core and removes every ephemeral configuration file. TUN, system proxy changes, public listeners,
ambient-proxy inheritance, and runtime downloads are not supported.

The ignored Linux source-integration benchmark starts one real bundled Mihomo under the 1 GiB
address-space limit, verifies every pinned listener, records `VmRSS`/`VmHWM`, and rejects a peak
above 384 MiB. A separate source-integration probe concurrently exercises HTTP and HTTPS through
multiple listeners and requires every observed exit to match the configured upstream proxy.

The bundled core is GPL-3.0. Each signed release includes its SHA-256, GPL notice, and
corresponding-source pointer in `_meta/official-release.json`.
