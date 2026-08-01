# Anthropic Provider

`anthropic` is an official 1flowbase model provider runtime extension for Anthropic's Messages API.

The runtime is packaged with plugin manifest v1 and invoked through the host `stdio_json` contract.

It targets:

- `GET /v1/models`
- `POST /v1/messages`

The plugin keeps the host boundary stable:

- 1flowbase owns installation, assignment, provider instances, secret storage, and runtime governance.
- The host passes the 1flowbase native provider invocation shape as the only internal truth.
- This plugin owns the Anthropic Messages wire conversion, usage normalization, and error shaping.

## Supported Configuration

- `base_url`
- `api_key`
- `validate_model`
- `anthropic_version`

The default base URL is `https://api.anthropic.com`, and the default Anthropic API version is `2023-06-01`.

## Provider-Level Parameter Schema

The plugin declares Anthropic Messages request parameters in host-visible order:

- `thinking_type`
- `thinking_budget_tokens`
- `temperature`
- `top_p`
- `top_k`
- `max_output_tokens` (sent upstream as `max_tokens`)
- `tool_choice`

Native 1flowbase tool calls are converted inside the plugin to Anthropic `tool_use` content blocks, and native tool result messages are converted to `tool_result` content blocks.

## Model Discovery

The provider uses hybrid discovery. It can fetch the live Anthropic model catalog from `GET /v1/models`, and it also ships current default model descriptors for:

- `claude-opus-4-1-20250805`
- `claude-sonnet-4-20250514`
- `claude-3-5-haiku-20241022`

Static token prices are intentionally omitted; pricing metadata is marked as dynamic.
