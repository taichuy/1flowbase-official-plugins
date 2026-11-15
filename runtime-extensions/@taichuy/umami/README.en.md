# Umami

[中文](README.md)

Manage shared Umami connections in 1flowbase, then use the registered HTTP interfaces or the dedicated MCP instance to operate Umami.

## Setup

1. Install and activate the plugin, then open **Umami** in Settings (`/settings/umami`). The Host must include `PluginEphemeralV1` and MCP support for whole request bodies.
2. Create a connection with a name, base URL, username, and password. Use `https://analytics.example.com`, or include a deployment base path such as `https://example.com/analytics`. Do not enter the `/websites` page URL.
3. Optionally test the draft before saving. Testing does not save the draft or password. The backend authenticates changed connection details independently when saving.
4. Leave the password blank when editing to keep the saved password. Disabled connections cannot execute upstream operations. Updating or deleting a connection clears its temporary authentication.
5. Connect your MCP client to the automatically created `umami` instance. Call `hosts_list` to obtain a `host_id`, then discover the operation groups.

The settings page uses the Host DataTable, request bridge, and operation permissions. System root has access; other users need the corresponding current role permissions. Lists expose only whether a password is configured, never the password, credential identifier, or authentication token.

## Authentication and storage

- Connection metadata lives in the plugin-owned `umami_hosts` collection and is accessed through `PluginDataV1`.
- Passwords use encrypted Host credential storage through `PluginCredentialClient`. The plugin does not connect directly to databases, Redis, or local key files.
- Tokens live exclusively in Host ephemeral storage with a one-hour cache TTL. They survive worker restarts, while cache loss or expiration causes a new login. Tokens are opaque; the plugin does not decode encrypted JWTs to infer an expiration date.
- After an upstream 401, the plugin verifies the same token. A valid session means insufficient permissions. A confirmed invalid session permits one new login and one retry. It does not blindly replay requests after 403, network failures, or server errors.
- Requests for the same connection in one shared worker coalesce authentication. Workers belonging to different users do not guarantee a single global login. Changing credentials or the endpoint creates a new credential revision so the new connection cannot reuse an old cached token.
- Automatic login and renewal for accounts with two-factor authentication are unsupported. The plugin reports this explicitly and does not consume verification or recovery codes. The corresponding upstream API operations remain in the complete interface inventory.
- If the account password changes in Umami, update the saved connection as well. Umami remains responsible for authorizing its API operations.

## Complete API and MCP coverage

The inventory is pinned to Umami **3.5.0**, commit `b18d5144dfe7342edeeca6f61d13ba3baabed9ad`:

- 186 current `/api` operations, including management, analytics, authentication, collection, and internal APIs; `OPTIONS` is excluded.
- 16 legacy report operations retained by upstream rewrites.
- 6 plugin connection-management operations.

The plugin registers **208 interfaces and 208 MCP tools**. External Umami Cloud gateway APIs absent from this source tree, static pages, short-link and pixel URLs, and the `/mcp` protocol endpoint are outside this REST inventory. Different deployed versions may return 404 or different results; the plugin preserves upstream status and response data.

HTTP routes follow this pattern:

```text
/api/console/managed-services/umami/hosts/{host_id}/api/...
```

Parameters retain their upstream names, such as `websiteId`, `startAt`, and `pageSize`. MCP exposes ordinary object fields directly. Array and union request bodies use a `body` argument representing the entire HTTP body. Results contain `success`, upstream `status`, and original `data`; text responses preserve text and `content_type`, while non-UTF-8 bytes use `encoding: base64`.

The login operation authenticates the saved connection and returns authentication state and user information; its token stays in ephemeral storage. Protocol-level tokens returned by SSO and two-factor verification also stay in temporary storage. Explicit credential-management operations, such as API-key creation, retain their business results, with sensitive fields excluded from ordinary operation events.

Activation after installation, reactivation, and upgrades create or force-reset the `umami` MCP instance, tools, groups, and descriptions from the packaged template. Manual edits to that managed instance will be overwritten; create a separate instance for custom configuration. Connection records, passwords, and unrelated instances are retained.

## Development and verification

The new plugin declares the existing packages below and reuses the local cache. The Host gains no new third-party dependencies, and no new packages were downloaded. Exact resolution is recorded in `Cargo.lock`.

| Package | Version | Purpose and scope |
| --- | --- | --- |
| runtime-extension-sdk | 0.6.3 / Git revision in Cargo.toml | Plugin worker and Host data, credential, ephemeral, and HTTP callbacks |
| tokio | 1.54.1 | Async worker and per-connection locks |
| serde / serde_json | 1.0.229 / 1.0.154 | Plugin wire types and JSON |
| base64 | 0.22.1 | HTTP callback byte encoding |
| url | 2.5.8 | Endpoint, path, and query encoding |
| uuid | 1.29.0 | Connection and credential revision identifiers |

The development extractor reuses TypeScript 5.9.3 and Zod 4.4.3 already installed in the Host. It does not install Umami server dependencies or introduce `@umami/api-client` or `@umami/mcp`.

```bash
node scripts/extract-upstream.cjs /path/to/umami /path/to/1flowbase/web _assets/api/upstream-contracts.json
node scripts/generate.mjs
node scripts/generate.mjs --check
node --test scripts/_tests/*.test.mjs
cargo test --locked
```

Extraction never executes Umami route handlers. Routes and contracts must match individually. The date enum is extracted from source; callbacks used only for runtime timezone validation are not executed. Snapshots retain the version, commit, original routes, and source files. Every packaged MCP file has a SHA-256 digest. `ui/_tests/run.mjs` exercises the real page using existing Host frontend modules and local Chrome, without building the entire frontend.

Use a temporary Cargo patch to the assembly SDK for local development; do not commit absolute paths. Production dependencies must pin a reachable Host revision containing the required capabilities. To release a new plugin version, update `version` in the generator and regenerate. Existing official-repository CI/release workflows verify platform release artifacts.

Upstream contract snapshots are derived from MIT-licensed Umami. Its license is included in `_assets/api/LICENSE.umami`. Plugin code uses the repository's Apache-2.0 license.
