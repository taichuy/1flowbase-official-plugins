# Clash / Mihomo Proxy

`clash-proxy` exposes selected proxies from an encrypted HTTPS Clash/Mihomo subscription through
the stable `1flowbase.network_egress_provider/v1` worker ABI. It rejects base64 payloads, proxy
providers, and V2Ray JSON.

The host passes the secret only at process start as a private config file. Its JSON value contains
one `subscription_url`; the provider fetches it over HTTPS and accepts only a YAML document with
one top-level `proxies` array. Supported fixed-v1 entry types are `ss`, `vmess`, `vless`, and
`trojan`; all entries need a safe display `name`, `server`, and `port`, plus their protocol
credential fields.

For every lease the wrapper starts the signed, bundled Mihomo Alpha core with a fresh
`127.0.0.1` mixed HTTP listener and an ephemeral configuration file. Releasing a lease stops the
core and removes that file. TUN, system proxy changes, public listeners, and runtime downloads are
not supported.

The bundled core is GPL-3.0. Each signed release includes its SHA-256, GPL notice, and
corresponding-source pointer in `_meta/official-release.json`.
