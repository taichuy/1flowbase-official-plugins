# WeChat Chat Channels

[简体中文](README.md)

Version 3.0.0 is a breaking communications upgrade: `read_file` now takes a file message’s `message_id`, and history tools default to and allow at most 5 real messages per page. Upgrade Host to the unified communications timeline contract first. The same QR-bound WeChat user can send texts and files, and receive the Agent Flow's final text reply. The plugin contributes distinct `1flowbase.notification.send` and `1flowbase.notification.receive` capabilities. It owns Tencent iLink protocol adaptation; Host owns continuous polling, durable inbox deduplication, cursor commits, selected file-table imports, Agent Flow execution and final-reply delivery.


Version 3.1.3 declares `bound_user_id` as an encrypted account identity through `authorization.managed_identity_keys`. A compatible Host permits it only in the identity fields of a correlated standard send receipt, avoiding a false unknown outcome after acceptance. Tokens such as bot_token/context_token and all other output locations remain fully protected. A matching Host/SDK is required.

## Send logs

Version 3.1.0 opts into the Host send log through the type descriptor’s `log` mapping. It requires a Host that supports this declaration; Cargo.toml pins the SDK source revision. The complete body comes from business input `/text`. With no separate title, the Host uses its first 255 Unicode characters. Business result fields `/accepted` and `/delivery_id` become JSON extra information. Before calling the provider, the Host persists the validated message projection after checking for credentials, so a failed attempt retains its message. Authorization secrets, secret patches and runtime envelopes are excluded. Missing content in historical logs is not backfilled.

Version 3.1.2 declares `receive.supports_send_receipts` so the Host can correlate send history across entrypoints. When the Host supplies its send-log UUID as `request_id`, the plugin sends a stable `client_id` of `1flowbase-{request_id}`. On success, its typed `receipt` echoes that request ID, the actual WeChat recipient/conversation, and `client_message_id`. The provider does not supply a message identity, so `provider_message_id` is omitted. The business result remains `{accepted:true, delivery_id:client_id}`. Acceptance does not confirm delivery or reading. With no request ID from an older Host, the plugin keeps generating a random client ID and omits the receipt. Timeouts and ambiguous results never trigger automatic retries. This requires a Host/SDK with the send-receipt contract; release packaging builds the installable artifact.

## Configure

1. Upgrade Host to a release supporting the communications receive/download contract, durable inbox and channel MCP provisioning. The manifest version floor alone does not guarantee these capabilities. Install the official `1flowbase/wechat-chat-channels` runtime extension, assign it to the workspace and grant `notification.send` and `notification.receive`.
2. Create a channel of type `1flowbase/wechat-chat-channels/private`. Select its Agent Flow and file table. The plugin's provider configuration is empty; credentials stay Host-managed.
3. Begin authorization with action `qr`, scan and confirm in WeChat. Host polls authorization until `authorized`; restart if the QR expires. Incoming messages and replies remain restricted to the scanner. Other senders cannot rebind this private channel.
4. Enable the channel. Host continuously runs bounded Receive calls, even when the settings page is closed. Incoming text and image, voice, document and video attachments become durable inbox records. Host downloads attachments into the selected file table before invoking the configured Agent Flow; it delivers only the final reply.
5. The plugin owns one MCP instance with both ID and name `wechat-chat-channels`. Host creates or forcibly replaces the complete built-in graph on activation and upgrade, removing stale plugin tools. Existing channel bindings migrate their built-in selection while retaining user-added MCP instances. Pass `instance_id=Host session.channel_id` to each tool; Host enforces workspace and sender scope. Tools include `history_messages` and `history_files` (default/maximum 5 items, follow next_cursor), literal keyword `search_messages`, `file_info` (actual file IDs, MIME, size and authorized URLs), chunked `read_file`, and explicit `view_image` returning native MCP image content. Images are never automatically injected. When file processing is needed, AI may discover the separately user-authorized terminal MCP, call `/terminal/terminal_info`, then use file_table_id and file_record_id (as terminal record_id) to download to an absolute temporary path and process it. Preview follows file management authorization and preview policy; no public links are automatically granted.

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


### Native WeChat typing status

The receive descriptor advertises `supports_activity`. The Host sends `set_activity` with a
committed inbound destination and either `replying` or `idle`. The plugin obtains an iLink
`getconfig` ticket, caches it privately through Host-managed secrets for 600 seconds, and calls
`sendtyping` with status 1 or 2. Stop also refreshes expired tickets. Status failures return
`applied: false` and never create a chat message or block the report.
This candidate requires the matching Host / SDK with `NotificationActivityState`. Local candidate
builds override the SDK dependency with that source; the SDK revision must be updated before release.


## Received quotations and history retrieval

The optional `NotificationReceivedMessage.reply_to` contains a `NotificationMessageReference` with optional supplier `provider_message_id`, `title`, and quoted `text`. At least one must be nonempty. IDs retain the 1024-byte identity limit; titles allow up to 1024 bytes and quoted text up to 65536 bytes, within the existing 512 KiB receive-batch budget. Omission preserves the previous receive shape. The wire reference cannot assert a Host message ID, role, credentials, or private download locator. Providers extract quotations separately from the original command text.

Host persists the quotation in the receive transaction. It resolves a unique original message only within the same workspace, channel, sender, and conversation, using an inbound supplier message ID or a stored send receipt's provider/client ID. A verified match adds the Host message_id and original text; otherwise the supplier snapshot is retained without guessing from titles or text. Quoted content reaches the model as user data, never as system instructions or fabricated file attachments. Shared history exposes the same reply_to field for console display.

History tools return at most five messages per page. When looking for an earlier report, follow next_cursor until it is found or history is exhausted; absence from the first page does not establish absence from history. The received-message send_status projection includes the actual channel send associated with that input. Command processing and report sending remain separate outcomes. Existing unknown outcomes without durable receipts remain unknown.
