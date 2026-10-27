# WeChat Chat Channels

[简体中文](README.md)

Version 3.0.0 is a breaking communications upgrade: `read_file` now takes a file message’s `message_id`, and history tools default to and allow at most 5 real messages per page. Upgrade Host to the unified communications timeline contract first. The same QR-bound WeChat user can send texts and files, and receive the Agent Flow's final text reply. The plugin contributes distinct `1flowbase.notification.send` and `1flowbase.notification.receive` capabilities. It owns Tencent iLink protocol adaptation; Host owns continuous polling, durable inbox deduplication, cursor commits, selected file-table imports, Agent Flow execution and final-reply delivery.

## Send logs

Version 3.1.0 opts into the Host send log through the type descriptor’s `log` mapping. It requires a Host that supports this declaration; Cargo.toml pins the SDK source revision. The complete body comes from business input `/text`. With no separate title, the Host uses its first 255 Unicode characters. Business result fields `/accepted` and `/delivery_id` become JSON extra information. Before calling the provider, the Host persists the validated message projection after checking for credentials, so a failed attempt retains its message. Authorization secrets, secret patches and runtime envelopes are excluded. Missing content in historical logs is not backfilled.

## Configure

1. Upgrade Host to a release supporting the communications receive/download contract, durable inbox and channel MCP provisioning. The manifest version floor alone does not guarantee these capabilities. Install the official `1flowbase/wechat-chat-channels` runtime extension, assign it to the workspace and grant `notification.send` and `notification.receive`.
2. Create a channel of type `1flowbase/wechat-chat-channels/private`. Select its Agent Flow and file table. The plugin's provider configuration is empty; credentials stay Host-managed.
3. Begin authorization with action `qr`, scan and confirm in WeChat. Host polls authorization until `authorized`; restart if the QR expires. Incoming messages and replies remain restricted to the scanner. Other senders cannot rebind this private channel.
4. Enable the channel. Host continuously runs bounded Receive calls, even when the settings page is closed. Incoming text and image, voice, document and video attachments become durable inbox records. Host downloads attachments into the selected file table before invoking the configured Agent Flow; it delivers only the final reply.
5. Host provisions a separate MCP instance per channel. The plugin declares `history_messages`, `history_files` and `read_file`, plus a default editable system prompt explaining progressive history and file retrieval. Use the session conversation_id for history tools, pass returned next_cursor as cursor for more results, and paginate from newer to older messages. Each page defaults to and allows at most 5 real messages. history_messages returns text and file messages; history_files returns file messages. Both expose `id`, `type` (text/file) and `direction` (inbound/outbound). Pass a file message’s `id` as `message_id` to read_file, which returns the typed `message` and file content. IDs are opaque; Host enforces the current conversation and sender scope. Users may configure other MCP tools on their Agent Flow.

## Cursor and reply window

Only Receive calls `getupdates`. Successful polls return an opaque **candidate cursor**, including empty polls. Host commits it after durable inbox persistence; the plugin never puts a cursor into managed secret patches. An oversized provider message batch is drained through the opaque pending cursor without dropping messages. Replayed messages keep provider identities so Host deduplicates them. Invalid or malformed batches fail without returning a cursor.

Authorization maintenance and sends use Host's cached window and context; neither consumes updates. A valid bound-user message opens or renews a 24-hour window from its Tencent creation timestamp. Expiration preserves account authorization. Send another message to renew it. Older authorized instances without timestamp evidence remain unverified until Receive supplies evidence. Disabled channels do not poll.

Explicit successful iLink send acknowledgement produces `succeeded` with `{"accepted":true,"delivery_id":"..."}`. A nonempty `message_id` may acknowledge success when default zero status fields are absent; it never overrides an explicit error or malformed status. Acceptance means provider acceptance, not user receipt. `delivery_id` is the submitted `client_id`.

- Missing authorization: `authorization_required`. Provider `-14`: `authorization_invalid`, clearing account credentials and requiring another scan.
- Definite `-2` / prepare failure: `recipient_not_ready`, preserving authorization.
- No verified message: `subscription_required`; expired window: `subscription_expired`; unavailable timestamp evidence: `subscription_unavailable`. These block before an external send.
- Other definite provider errors: `provider_rejected`.
- Ambiguous send, timeout or connection loss: `unknown / provider_result_unknown`. Sends are never automatically retried by the plugin.

## Media and network boundary

Download references stay private to Host; they contain provider attachment identity and an opaque media locator/key, never model-visible URLs. Media uses the exact HTTPS `novac2c.cdn.weixin.qq.com/c2c/download` endpoint with a provider-issued `encrypted_query_param`. Redirects, arbitrary hosts, other paths, credentials in URLs and unexpected ports are rejected. Bot bearer tokens are never sent to CDN.

The provider encrypts media with AES-128-ECB and PKCS#7 padding. The plugin uses the already locked `aws-lc-rs` **1.16.3** as a direct dependency; no new third-party package is introduced. It accepts the provider's base64 raw-key and base64 hex-key encodings. ECB is not authenticated encryption; the implementation checks HTTPS, ciphertext block alignment, final padding, HTTP Content-Range and declared plaintext length when supplied. The protocol supplies no authenticated media digest, so these checks cannot prove origin integrity independently of HTTPS.

Host requests byte offsets and at most 48 KiB of decoded plaintext per call. CDN Range requests include aligned AES blocks and one lookahead block, so padding is removed only at the actual file end. There is no total-file-size cap and no whole-file buffering. When a CDN ignores Range and returns HTTP 200, the plugin streams and discards the prefix, retaining only the requested blocks and stopping after a suffix byte proves there is more content. This avoids a file-size limit but re-downloads the prefix for later chunks; real CDN interoperability and performance still require account-backed validation.

## Validation and source evidence

Mock fixtures cover QR binding, sender isolation, duplicate/replay identities, cursor overflow, empty/malformed polls, invalid authorization, cached-only sends, media references, AES download, large-file and unaligned chunks, unsafe locators and corrupt padding/length. Root's centralized QA runs the suite against the new SDK; fixtures alone are not a live-WeChat acceptance result.

Protocol patterns were checked against [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent/blob/0e427d788d253630d38a3a51cd76849e73cd6925/gateway/platforms/weixin.py) (MIT, Python; 256,843 Stars and 56,194 forks observed 2026-10-25). Its explicit item mapping, AES key encodings and CDN locator flow match this task. The implementation adapts those protocol patterns to Host-owned persistence and bounded typed chunks; it does not copy Hermes's whole-file download or cursor persistence.

The SDK is pinned to `a29f82200efbb3e3b37afc60c9d6a24009d6a9ad`, which includes the send-log declaration contract. No local dependency paths are committed.
