# OpenAI-Compatible API Provider

`openai_compatible` is an official 1flowbase model provider runtime extension for services that expose an OpenAI-compatible API surface.

The runtime is packaged with plugin manifest v1 and invoked through the host `stdio_json` contract.

It is not limited to OpenAI's hosted service. It targets providers that expose:

- `GET /models`
- `POST /chat/completions`

The plugin keeps the host boundary stable:

- 1flowbase owns installation, assignment, provider instances, secret storage, and runtime governance.
- This plugin owns protocol translation, model discovery, usage normalization, and error shaping.

## Supported Configuration

- `base_url`
- `api_key`
- `organization`
- `project`
- `api_version`
- `default_headers`
- `validate_model`

`default_headers` accepts a JSON object string and is merged into every outbound request before the standard authorization headers are applied.

## Model Discovery

The plugin uses `hybrid` discovery:

- static default model: `gpt-4o-mini`
- dynamic refresh: `GET /models`

This means the host can show one safe default option immediately and replace or extend it with the live catalog after validation.

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
