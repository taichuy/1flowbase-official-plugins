# WeChat private notifications

Authorize with a Tencent iLink QR code and send text notifications to **the WeChat user who scanned and confirmed that authorization**. This is a Rust RuntimeExtension contributing to `1flowbase.notification.send`. No separate WeChat service, listener, or database is needed.

[简体中文](README.md)

The Host must provide `notifications.instances.authorization`; the version floor does not replace this capability requirement.

## Use

1. Install `1flowbase/wechat-notification` from the official runtime catalog, assign it to the workspace, and grant the contribution `notification.send`.
2. Create a notification instance of type `1flowbase/wechat-notification/private`. User configuration is empty.
3. Start authorization with `begin` and action `qr`, scan the displayed QR code in WeChat, and confirm. The Host calls `poll` until `authorized`. Start again if the code expires.
4. The bound user sends any message to the bot to open a 24-hour notification window. Each newer user message renews the window from its Tencent creation time. Then send `{"text":"Notification text"}`. Other peers and bot messages never renew the window or change recipient binding/context.

Enabled/disabled, account authorization, and the delivery window are independent. Expiration preserves account authorization; send another message to renew without scanning again. `cancel` removes a pending QR challenge. `maintain` and each send synchronize inbound messages during one call, without a background service. No continuous capture is claimed while the page is closed and no send is running. Delayed synchronization still uses the original message time. This extension sends private text only.

Upgrade the Host before installing 1.1.0: it must support `NotificationAuthorizationView.delivery_window`, Host-injected `NotificationRuntimeInput.delivery_window`, and the subscription failure codes. An older authorized instance without time evidence remains unverified; it is never assigned a fabricated subscription time.

## Outcomes

An explicit successful iLink response produces `succeeded` with `{"accepted":true,"delivery_id":"..."}`. Success can be acknowledged by `ret: 0` or an HTTP-success response containing only a nonempty `message_id`, with default status fields omitted. A message ID never overrides an explicit error or a malformed status field. Acceptance means **the provider accepted the request**, not user receipt or a read acknowledgement. `delivery_id` is the iLink `client_id` used for the request.

- No authorization: `authorization_required`. Token/session `-14`: `authorization_invalid`, clearing account credentials and requiring another scan.
- Definite `-2` / prepare failed: `recipient_not_ready`, preserving `authorized`. The user can message the account before a new notification is initiated.
- No verifiable user message yet: `subscription_required`. Expired delivery window: `subscription_expired`. Both block before an external send; the Host records the failure. Send any message to the bot, then refresh or initiate a new notification.
- Failed synchronization, missing/invalid message time, or an older instance without time evidence: `subscription_unavailable`. This does not claim the user never sent a message. A previously verified active window remains usable until its original deadline, but failed synchronization never renews it.
- Other definite provider errors: `provider_rejected`.
- Send timeout, connection loss, or a response that cannot prove acceptance: `unknown / provider_result_unknown`. No automatic send retries; consider duplicate delivery before initiating a new notification.

Every send attempts one `getupdates` request. Only the bound sender's `message_type=1` messages qualify. Take the largest valid `create_time_ms` in milliseconds and expire 24 hours later; any item type qualifies without text or context. Old/duplicate/reordered messages never move the deadline backward. Missing, malformed, or future timestamps never use poll time as a substitute. Valid getupdates responses may omit default zero status fields.

The existing one-second client cancellation of a long poll remains; a timeout is unavailable evidence, not an empty message batch. Only a still-valid verified window permits using cached context or sending without context. Public times/status travel in typed `delivery_window`, separately from managed secrets; the frontend never calculates eligibility.

## Secrets and network boundaries

QR credentials, bot token/id, bound user, base URL, cursor, and context are returned only in `managed_secret_patch` for encrypted Host persistence and injected by the Host as `managed_secrets`. The extension writes no local credential files and logs no credentials or raw provider responses. The public authorization view contains only status and QR display information. Starting a new scan clears the old account state.

Production starts at `https://ilinkai.weixin.qq.com`. Provider base URLs and QR redirects must be HTTPS under `*.weixin.qq.com`, with no URL credentials, additional path/query, or non-443 port. Automatic HTTP redirects are disabled. User configuration cannot supply an arbitrary endpoint.

## Build and verification entry points

The Host SDK is pinned to revision `dc2aa7cad430fe211746ac5506e83dfd6a40cb68`. `execute_authorizable_notification` validates the typed input/result; `serve` owns the shared dispatcher. The extension implements no independent stdio protocol. The manifest binary path is `bin/wechat-notification`.

```bash
cargo test --manifest-path runtime-extensions/@taichuy/wechat-notification/Cargo.toml
cargo build --release --manifest-path runtime-extensions/@taichuy/wechat-notification/Cargo.toml
```

Injected iLink mocks in `src/_tests/behavior.rs` cover QR states, Host secret recovery, redirect/expiry/cancel, direct sends without context, scanner binding, invalid credentials, rejection, and unknown results. Mocks do not establish real QR authorization, Tencent availability, or actual WeChat delivery. External verification requires installing the real package, authorizing with a real WeChat user, sending a notification, and checking receipt in WeChat.

## Protocol references

First-party [Tencent/openclaw-weixin](https://github.com/Tencent/openclaw-weixin) API types, headers, and optional-context sends; the local Hermes Weixin adapter (MIT) was used to cross-check QR states and failure classification. This implementation is independently written and does not reproduce substantial upstream code.
