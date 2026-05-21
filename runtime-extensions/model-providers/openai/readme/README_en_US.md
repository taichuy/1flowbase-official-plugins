# OpenAI Provider

`openai` is an official 1flowbase model provider runtime extension for OpenAI's Responses API.

The runtime is packaged with plugin manifest v1 and invoked through the host `stdio_json_worker` contract.

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

Streaming prefers the Responses WebSocket transport in `auto` mode, keeps the upstream connection inside the provider worker, acknowledges completed responses with `response.processed`, and falls back to HTTP SSE when the WebSocket handshake is unavailable. Both transports use a 5-minute idle timeout, matching Codex's long-running stream posture: active streams can keep flowing, but a silent upstream connection fails instead of hanging forever.

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
