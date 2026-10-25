# Obscura Browser for 1flowbase

Operate temporary browsers from AI conversations and manage the same browser instances and tabs in the console. The plugin links the Obscura Rust libraries directly. The host supervises a plugin process isolated by authenticated user and originating workspace.

## Install and connect

Use a host build that supports `actor_workspace_scope`, trusted `ManagedServiceCallContext`, and `managed_service.mcp_instance`. The base minimum is 0.5.8, but earlier 0.5.8 builds do not contain these contracts: deploy the corresponding host source first. Cargo.toml pins the exact SDK revision; CI checks the paired host and plugin SHAs.

1. Install and enable `browser-obscura` in Extension Center. The initial release target is Linux amd64 GNU.
2. Find `browser_obscura` in MCP management for the current workspace. Use the existing MCP authentication flow to connect your AI client or select the instance for an application conversation.
3. Open `/settings/browser-obscura`. Console and MCP requests must use the same user and workspace to share browser resources.

Activation creates the initial MCP instance, tools, bindings, and discovery policy. Re-enabling or upgrading preserves user edits and deletions. Import the example or use the host's export/import flow when you want another copy. Disabling, revoking, or uninstalling the plugin prevents further execution of its operations.

## Workflow

Discover tools progressively with `mcp_list` and `mcp_get`:

```text
browsers_create → tabs_create → page_snapshot
    → page_fill / page_click / page_press_key → fresh page_snapshot
    → page_screenshot → tabs_close / browsers_close
```

Every page operation identifies a `browser_id` and `tab_id`. Element actions also require the current `snapshot_id` and `ref`. Do not reuse references after navigation or page changes. A click or Enter can submit a form; inspect the page after a timeout or uncertain result instead of automatically repeating the action.

Tabs in one browser share its browser context. Separate browsers have isolated cookies and storage. Profiles are temporary: process exit invalidates old IDs, without restoring pages or replaying actions. Leaving the management page stops its requests but does not close browsers created by AI.

Browsers expire when idle; creation accepts `idle_timeout_ms`, with the service implementation defining the default. Activity renewal and in-flight protection are backend-owned. Explicitly closing a browser closes its tabs and releases the host resource lease.

The console supports search, pagination, creation, closure, tab details, navigation, and screenshots. HTTP returns complete PNG data; MCP delivers native image content with separate metadata.

Business failures use the host’s standard HTTP 422 response with a stable `code`; MCP preserves it as `target_code` without treating errors as images or replaying the operation.

## Editable MCP example

The `mcp/` directory is a standard `1flowbase.mcp.bundle/v2` bundle containing a checksummed manifest, tools, and instances. It uses separate `browser_obscura_example` identifiers so it does not overwrite the generated default instance. Recompute file SHA-256 values after editing files.

Zip `manifest.json`, `tools/`, and `instances/` from that directory and import it through the host MCP preview/import flow. The example contains no API keys, browser cookies, or user data. You can edit names, descriptions, input/output mappings, groups, and discovery policy; configuration never bypasses operation authorization.

## Scope and behavior

- The engine is pinned to `h4ckf0r0day/obscura@fdeac4421eaab3b1f0c26c71c2214fa5c8ad2fb9`, with render enabled and the same revision's taffy/cosmic-text patches. There is no CDP server or separate browser daemon.
- Supported operations are navigation, snapshots, click, fill/type, key presses, scrolling, conditional waits, and PNG screenshots. The plugin does not expose host commands, host files, cookie export, or cross-user takeover.
- Obscura's network restrictions remain in place. Production defaults do not allow private-network or file URL access. Deterministic fixtures explicitly allow their temporary local HTTP server without changing release defaults.
- Obscura does not implement all Chromium Web APIs, Service Worker behavior, or rendering features. Compatibility claims require evidence from the actual target sites.
- The host owns authentication, originating workspace, process supervision, cancellation, and MCP provisioning. The plugin owns pages, per-resource ordering, background page tasks, and cleanup. Forced plugin-process termination invalidates its temporary browsers.

## Development and verification

Cargo.lock records exact dependencies. Obscura crates are new direct engine dependencies. The runtime-extension-sdk, Tokio, Serde, base64, UUID, and URL libraries provide host integration, asynchronous execution, and data encoding. No separate browser-driver framework is added.

Run engine tests in isolated processes:

```bash
cargo nextest run --release --locked --all-targets
cargo build --release --locked --bin browser-obscura
```

Package the executable with the host's `scripts/node/plugin/cli.js package` command. `.github/workflows/browser-obscura-ci.yml` verifies exact paired source SHAs, runs real engine tests, and packages the binary. Evidence is written to `tmp/test-governance/browser-obscura/`. Central UI verification uses `ui/_tests/settings.browser.mjs` with a host page-debug temporary session and desktop/mobile viewports.

Tests being present does not mean they passed. Root Issue #2679 records candidate SHAs, QA, installation, and release evidence.

## License

The plugin and Obscura use Apache-2.0. Dependencies retain their own licenses; see LICENSE, NOTICE, and licenses/. No private commercial plugin source or assets are included.
