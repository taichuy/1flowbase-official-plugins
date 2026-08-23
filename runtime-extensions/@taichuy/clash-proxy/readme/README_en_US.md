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

For every lease the wrapper starts the signed, bundled Mihomo Alpha core with a fresh
`127.0.0.1` mixed HTTP listener and an ephemeral configuration file. Releasing a lease stops the
core and removes that file. TUN, system proxy changes, public listeners, and runtime downloads are
not supported.

The bundled core is GPL-3.0. Each signed release includes its SHA-256, GPL notice, and
corresponding-source pointer in `_meta/official-release.json`.
