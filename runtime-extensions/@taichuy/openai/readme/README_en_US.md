# OpenAI Provider

[简体中文](README_zh_Hans.md)

`openai` is an official 1flowbase model provider runtime extension for OpenAI's Responses API.

The runtime is packaged with plugin manifest v1 and invoked through the host `stdio_json_multiplex_v1` contract.

OpenAI 0.2.72 requires host 0.5.4 or later to validate full native-context HTTP recovery receipts. Official installation and updates use the existing minimum-host-version check; older 0.5.3 hosts are incompatible with this recovery path. Compatibility overrides and direct loading bypass that default check and are outside the supported deployment path for this release.

It targets:

- `GET /models`
- `POST /responses`

The plugin keeps the host boundary stable:

- 1flowbase owns installation, assignment, provider instances, secret storage, and runtime governance.
- The host passes the 1flowbase native provider invocation shape as the only internal truth.
- This plugin owns the OpenAI Responses wire conversion, model discovery, usage normalization, and error shaping.

## Supported Configuration

- `base_url`
- `api_key`
- `organization`
- `project`
- `validate_model`
- `transport_mode` (optional: `auto`, `responses_websocket`, or `http_sse`)

The default base URL is `https://api.openai.com/v1`.

## Provider-Level Parameter Schema

The plugin declares request parameters that map to the Responses API:

- `reasoning_effort`
- `temperature`
- `top_p`
- `max_output_tokens`
- `response_format`
- `tool_choice`
- `store`

Native 1flowbase tool calls are converted inside the plugin to Responses `function_call` input items, and native tool result messages are converted to `function_call_output` input items. Native function tool definitions are converted to Responses function tools.

The runtime also forwards Codex-style Responses fields when the host passes them through the provider invocation contract: `parallel_tool_calls`, `include`, `service_tier`, `prompt_cache_key`, and `metadata`.

Streaming defaults to HTTP SSE. `transport_mode` can explicitly select `responses_websocket` or `auto`; in `auto` mode the runtime tries the Responses WebSocket transport first, keeps the upstream connection inside the provider worker, and falls back to HTTP SSE when the WebSocket handshake is unavailable. For native continuations, completed invocation-scoped raw history supports one cursor-free full-context WebSocket rebuild. If that rebuilt request fails before any semantic output, auto mode may send the same complete request over HTTP SSE within the host attempt and deadline budget. A predecessor cursor with only new tool outputs is never sent to HTTP. Explicit connection-bound host provenance, missing or foreign history, policy/protocol/authentication errors, forced WebSocket mode and committed semantic output prohibit this recovery. Both transports use a 5-minute idle timeout, matching Codex's long-running stream posture: active streams can keep flowing, but a silent upstream connection fails instead of hanging forever.

For LLM nodes, `responses_transport_policy` defaults to `inherit`, following the client's HTTP or WebSocket transport when known. `force_http_sse` and `force_websocket` select the upstream transport explicitly. Historical `use_responses_websocket=false` follows the client; `true` forces WebSocket. When no client transport is known, the provider-instance `transport_mode` remains the fallback.

## Static Models

The provider uses hybrid discovery. It can fetch the live OpenAI model catalog from `GET /models`, and it also ships current default model descriptors for:

- `gpt-5.2`
- `gpt-5.1`
- `gpt-5-mini`

Static token prices are intentionally omitted; pricing metadata is marked as dynamic.

## Packaging

1. Build the runtime binary:
   `cargo build --manifest-path Cargo.toml --release --target x86_64-unknown-linux-musl`
2. Package the plugin with the host CLI:
   `node ../1flowbase/scripts/node/plugin.js package . --out ./dist --runtime-binary ./target/x86_64-unknown-linux-musl/release/openai-provider --target x86_64-unknown-linux-musl`

Explicit native `responses_websocket` requests fail on WebSocket handshake errors; they never silently create an HTTP response cursor.
Semantic and native WebSocket turns send `response.create` without synthesizing a `response.processed` acknowledgement.

## Upstream error facts

Supplier failures use `provider_upstream_error`. `provider_details.upstream_error`
contains the original inner `error` value, including nulls and unknown fields;
when the supplier omits `error`, this fact is omitted too. HTTP failures also
include the actual `status_code` and exact `raw_body`. Event failures do not
synthesize an HTTP status. Structured WebSocket and SSE error terminals stop
stream processing and set `semantic_terminal`; local recovery receipts and
diagnostics are attached beside these supplier facts.

## Native recovery in 0.2.72

Valid string input is recorded as a user/input_text item only in completed replay history; the existing native wire conversion stays unchanged. Complete raw output items, including encrypted reasoning and function calls, retain their order and opaque fields. Recovery uses the accepted callback output without repeating completed tools. The invocation-owned rebuilt body is released when the request ends; this change adds no history table or whole-session ephemeral cache.

The typed `one_full_context_rebuild` receipt reports the actual final transport and zero-based upstream attempt. It asserts complete scoped context with the old cursor removed. Absence of a cursor alone does not authorize a native HTTP retry.
