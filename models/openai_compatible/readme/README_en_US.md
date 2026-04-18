# OpenAI-Compatible API Provider

`openai_compatible` is an official 1flowbase provider plugin for services that expose an OpenAI-compatible API surface.

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
