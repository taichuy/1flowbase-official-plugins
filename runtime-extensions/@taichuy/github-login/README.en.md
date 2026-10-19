# GitHub sign-in

[中文](README.md)

Official managed authentication provider for `1flowbase/github-login`. It uses a GitHub OAuth App with state and PKCE S256, requesting only `user:email`. Host owns encrypted configuration, stable identity binding, registration transactions and sessions. This worker verifies GitHub identities and first-registration account-age eligibility.

## Setup

1. Install the plugin and create a GitHub login entry in Authentication Center.
2. Create an OAuth App under GitHub **Settings → Developer settings → OAuth Apps**. Copy Host's read-only callback URL into **Authorization callback URL** exactly.
3. Save the OAuth App client ID and client secret in Authentication Center, then enable the entry.
4. Optionally allow new users and set the minimum GitHub account age in whole years (default 0). These registration settings do not affect existing identity bindings.

The UI calls `ctx.auth.redirectToExternalLogin()`. Host builds `/api/public/auth/external/start?login_entry_id=<current-entry-id>`. No credentials or private configuration are exposed as public variables. Tokens, authorization codes, secrets and verifier values are never returned in profiles or errors.

Identity uses GitHub's immutable numeric user ID, independent of username changes. Profiles contain only login, name, avatar URL, creation time, verified primary email when available, and email verification status. Host requires email only during first registration. Missing email does not invalidate an existing binding.

Age checks use UTC calendar anniversaries; a February 29 anniversary falls on February 28 in non-leap years. Missing, invalid or future creation times deny registration even with a zero-year minimum. The worker never creates users.

Run `cargo test --manifest-path runtime-extensions/@taichuy/github-login/Cargo.toml` for directed multiplex Host HTTP fixtures and registration boundary cases. HTTP uses SDK Host egress exclusively. Existing package discovery and the provider release workflow handle packaging, signing and registry publication. Pin the SDK to a Host commit exporting the authentication contract before release; local patch configurations must remain uncommitted.
