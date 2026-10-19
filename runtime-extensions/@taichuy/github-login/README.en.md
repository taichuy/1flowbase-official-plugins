# GitHub sign-in

[中文](README.md)

Official managed authentication provider for `1flowbase/github-login`. It uses a GitHub OAuth App with state and PKCE S256, requesting only `user:email`. Host owns encrypted configuration, stable identity binding, registration transactions and sessions. This worker verifies GitHub identities and first-registration account-age eligibility.

## Setup

The Host must include managed external authentication support. The first compatible release target is **0.5.8**; the published **v0.5.7 does not include this feature**. For development, use a `dev` build containing these authentication changes. If it still reports version 0.5.7, use Extension Center's existing exact compatibility acknowledgement only after checking its source. An acknowledgement cannot add missing functionality to an older release.

1. Install the plugin and create a GitHub login entry in Authentication Center.
2. Create an OAuth App under GitHub **Settings → Developer settings → OAuth Apps**. Copy Host's read-only callback URL into **Authorization callback URL** exactly.
3. Save the OAuth App client ID and client secret in Authentication Center, then enable the entry.
4. Optionally allow new users and set the minimum GitHub account age in whole years (default 0). These registration settings do not affect existing identity bindings.

The UI calls `ctx.auth.redirectToExternalLogin()`. Host builds `/api/public/auth/external/start?login_entry_id=<current-entry-id>`. No credentials or private configuration are exposed as public variables. Tokens, authorization codes, secrets and verifier values are never returned in profiles or errors.

Identity uses GitHub's immutable numeric user ID, independent of username changes. Profiles contain only login, name, avatar URL, creation time, verified primary email when available, and email verification status. Host requires email only during first registration. Missing email does not invalidate an existing binding.

Age checks use UTC calendar anniversaries; a February 29 anniversary falls on February 28 in non-leap years. Missing, invalid or future creation times deny registration even with a zero-year minimum. The worker never creates users.

Run `cargo test --manifest-path runtime-extensions/@taichuy/github-login/Cargo.toml` for directed multiplex Host HTTP fixtures and registration boundary cases. HTTP uses SDK Host egress exclusively. Existing package discovery and the provider release workflow handle packaging, signing and registry publication. Pin the SDK to a Host commit exporting the authentication contract before release; local patch configurations must remain uncommitted.

## Dependency scope

This plugin is a new, independent Rust package and adds no third-party dependencies to the Host. It reuses `runtime-extension-sdk`, pinned by Git revision in `Cargo.toml` and `Cargo.lock`, for worker transport and Host HTTP egress. It does not implement its own worker protocol or network client.

| Direct dependency | Locked version | Purpose |
| --- | --- | --- |
| `tokio` | 1.53.3 | Async worker runtime and test IO |
| `serde` / `serde_json` | 1.0.229 / 1.0.154 | Typed configuration, authentication requests and JSON responses |
| `base64` | 0.22.1 | Body encoding for the Host HTTP wire contract |
| `url` | 2.5.8 | OAuth URL and form encoding |
| `time` | 0.3.55 | UTC dates and calendar-anniversary checks |
