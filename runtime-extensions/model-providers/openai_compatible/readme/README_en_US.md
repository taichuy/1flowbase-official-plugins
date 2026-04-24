# OpenAI-Compatible API Provider

`openai_compatible` is an official 1flowbase model provider runtime extension for services that expose an OpenAI-compatible API surface.

The runtime is packaged with plugin manifest v1 and invoked through the host `stdio_json` contract.

It is not limited to OpenAI's hosted service. It targets providers that expose:

- `GET /models`
- `POST /chat/completions`

The plugin keeps the host boundary stable:

- 1flowbase owns installation, assignment, provider instances, secret storage, and runtime governance.
- This plugin owns protocol translation, model discovery, usage normalization, and error shaping.
- The plugin exposes a provider-level parameter schema for common `POST /chat/completions` request parameters.
- Model metadata extraction stays explicit: `context_window` and `max_output_tokens` are read only from known upstream fields and remain `null` when absent.

## Supported Configuration

- `base_url`
- `api_key`
- `organization`
- `project`
- `api_version`
- `default_headers`
- `validate_model`

`default_headers` accepts a JSON object string and is merged into every outbound request before the standard authorization headers are applied.

## Provider-Level Parameter Schema

The plugin declares a provider-level parameter schema for the Chat Completions invocation bridge. It covers these request parameters from the compatible `POST /chat/completions` surface:

- Sampling and length: `temperature`, `top_p`, `n`, `max_tokens`, `max_completion_tokens`, `presence_penalty`, `frequency_penalty`, `stop`, `seed`
- Probability controls: `logit_bias`, `logprobs`, `top_logprobs`
- Output and tracking: `response_format`, `user`, `store`, `metadata`, `audio`, `modalities`, `reasoning_effort`
- Tool controls: `tools`, `tool_choice`, `parallel_tool_calls`

`model`, `messages`, and `stream` stay host-controlled. The runtime always sends `stream: false` to the upstream provider and normalizes the response into the 1flowbase runtime event envelope.

The host owns persistence, UI rendering, and per-model manual overrides. This plugin only declares the provider-level parameter contract and forwards supported invocation parameters.

## Model Discovery

The plugin uses `hybrid` discovery, but ships with no bundled static default models.

The active catalog is refreshed from `GET /models` after validation.

Model metadata extraction is intentionally explicit-only. The runtime reads:

- `context_window`, `context_length`, `input_token_limit`
- `max_output_tokens`, `output_token_limit`, `max_tokens`

If an upstream `/models` payload does not expose one of these fields as an integer, the plugin returns `null` instead of guessing.

During normalization the runtime only maps explicit upstream fields:

- Context aliases: `context_window`, `context_length`, `input_token_limit`
- Output aliases: `max_output_tokens`, `output_token_limit`, `max_tokens`

If the upstream payload does not expose one of those numeric fields, the plugin returns `null` instead of inferring a value.

## Local Demo

1. Start `plugin-runner`.
2. Run `node scripts/node/plugin.js demo dev /path/to/openai_compatible --port 4310`.
3. Open the demo page and point it at the running `plugin-runner`.
4. Use `Validate`, `List Models`, and `Invoke` to exercise the real provider contract.

## Packaging

This example shows the current model provider runtime extension packaging contract, not a provider-only manifest schema.

1. Build the runtime binary, for example:
   `cargo build --manifest-path Cargo.toml --release --target x86_64-unknown-linux-musl`
2. Package the plugin with the host CLI:
   `node ../1flowbase/scripts/node/plugin.js package . --out ./dist --runtime-binary ./target/x86_64-unknown-linux-musl/release/openai_compatible-provider --target x86_64-unknown-linux-musl`
