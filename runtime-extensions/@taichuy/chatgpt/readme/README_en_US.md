# ChatGPT Subscription Provider

`chatgpt` is an official 1flowbase model-provider RuntimeExtension for a user's
ChatGPT subscription. It uses the ChatGPT Codex backend at
`https://chatgpt.com/backend-api/codex` and is invoked through the
`stdio_json_worker` contract.

## Authentication and secrets

Start with the generic provider-auth action **Sign in with Device Code**. The
plugin returns a verification URL, user code, expiry and polling interval; the
host persists only the plugin-declared managed-secret patch. **Paste OAuth
callback URL** is a compatibility path for browser PKCE completion.

The plugin owns OAuth exchange, refresh-token rotation, account identity and
expiry checks. 1flowbase only dispatches the generic auth operation and stores
managed secrets per provider instance. OAuth codes, tokens and transient PKCE /
device grants never belong in normal instance configuration.

## Models and runtime behavior

Models are discovered dynamically from `GET /models`. The ChatGPT `slug` is
kept unchanged as `model_id`; no static alias or fallback mapping is shipped.
The plugin uses a short ETag cache and retains the last valid catalog through a
temporary upstream failure.

Responses stream over HTTP SSE by default. An LLM node may select Responses
WebSocket with the provider-declared `use_responses_websocket` parameter.
WebSocket fallback is only permitted before visible output and is never used to
downgrade an established WebSocket continuation cursor.

Hosted `web_search` is passed through as a ChatGPT Responses Hosted Tool. The
upstream service executes it; neither the host nor this plugin implements a
local search executor.

## Network boundary

The plugin sends Codex identity headers and honors the optional per-instance
HTTP proxy. It bounds connection establishment, enables response compression,
and keeps normal TLS verification enabled. A provider-instance Cloudflare jar
accepts only documented infrastructure cookies on HTTPS ChatGPT hosts; ChatGPT
account, session and authentication cookies are rejected.

## Packaging

Build the runtime binary with the package's `Cargo.toml`. Cross-platform
archives, checksums and official-registry publication are produced by the
official provider release workflow; they are not generated from a developer's
local subscription credentials.
