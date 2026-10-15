# WeChat private notifications

Authorize with a Tencent iLink QR code and send text notifications to **the WeChat user who scanned and confirmed that authorization**. This is a Rust RuntimeExtension contributing to `1flowbase.notification.send`. No separate WeChat service, listener, or database is needed.

[简体中文](README.md)

The Host must provide `notifications.instances.authorization`; the version floor does not replace this capability requirement.

## Use

1. Install `1flowbase/wechat-notification` from the official runtime catalog, assign it to the workspace, and grant the contribution `notification.send`.
2. Create a notification instance of type `1flowbase/wechat-notification/private`. User configuration is empty.
3. Start authorization with `begin` and action `qr`, scan the displayed QR code in WeChat, and confirm. The Host calls `poll` until `authorized`. Start again if the code expires.
4. Send `{"text":"Notification text"}`. The scanner is the only recipient; messages from other peers never change recipient binding or context.

The Host's enabled/disabled setting is independent of authorization. `cancel` removes a pending QR challenge. `maintain` refreshes the authorized account's cursor/context during one call, without a background service. This extension supports private text only.

## Outcomes

An explicit successful iLink response produces `succeeded` with `{"accepted":true,"delivery_id":"..."}`. Success can be acknowledged by `ret: 0` or an HTTP-success response containing only a nonempty `message_id`, with default status fields omitted. A message ID never overrides an explicit error or a malformed status field. Acceptance means **the provider accepted the request**, not user receipt or a read acknowledgement. `delivery_id` is the iLink `client_id` used for the request.

- No authorization: `authorization_required`. Token/session `-14`: `authorization_invalid`, clearing account credentials and requiring another scan.
- Definite `-2` / prepare failed: `recipient_not_ready`, preserving `authorized`. The user can message the account before a new notification is initiated.
- Other definite provider errors: `provider_rejected`.
- Send timeout, connection loss, or a response that cannot prove acceptance: `unknown / provider_result_unknown`. No automatic send retries; consider duplicate delivery before initiating a new notification.

Every send first attempts one `getupdates` request, matching context only to the bound scanner. Its client timeout is one second, following the first-party client's cancellable long-poll behavior without inventing a server-side short-poll field. Preparation timeouts or temporary read failures preserve existing context and still attempt sending. An authorized send is attempted even without inbound context; iLink decides recipient readiness.

## Secrets and network boundaries

QR credentials, bot token/id, bound user, base URL, cursor, and context are returned only in `managed_secret_patch` for encrypted Host persistence and injected by the Host as `managed_secrets`. The extension writes no local credential files and logs no credentials or raw provider responses. The public authorization view contains only status and QR display information. Starting a new scan clears the old account state.

Production starts at `https://ilinkai.weixin.qq.com`. Provider base URLs and QR redirects must be HTTPS under `*.weixin.qq.com`, with no URL credentials, additional path/query, or non-443 port. Automatic HTTP redirects are disabled. User configuration cannot supply an arbitrary endpoint.

## Build and verification entry points

The Host SDK is pinned to revision `948f41cad5b2f951c748a0a0104464588b95d88d`. `execute_authorizable_notification` validates the typed input/result; `serve` owns the shared dispatcher. The extension implements no independent stdio protocol. The manifest binary path is `bin/wechat-notification`.

```bash
cargo test --manifest-path runtime-extensions/@taichuy/wechat-notification/Cargo.toml
cargo build --release --manifest-path runtime-extensions/@taichuy/wechat-notification/Cargo.toml
```

Injected iLink mocks in `src/_tests/behavior.rs` cover QR states, Host secret recovery, redirect/expiry/cancel, direct sends without context, scanner binding, invalid credentials, rejection, and unknown results. Mocks do not establish real QR authorization, Tencent availability, or actual WeChat delivery. External verification requires installing the real package, authorizing with a real WeChat user, sending a notification, and checking receipt in WeChat.

## Protocol references

First-party [Tencent/openclaw-weixin](https://github.com/Tencent/openclaw-weixin) API types, headers, and optional-context sends; the local Hermes Weixin adapter (MIT) was used to cross-check QR states and failure classification. This implementation is independently written and does not reproduce substantial upstream code.
