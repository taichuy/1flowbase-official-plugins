# DeepSeek Provider

`deepseek` is an official 1flowbase model provider runtime extension for the dedicated DeepSeek API surface.

The runtime is packaged with plugin manifest v1 and invoked through the host `stdio_json` contract.

It targets:

- `GET /models`
- `GET /user/balance`
- `POST /chat/completions`

This task only scaffolds provider metadata and the minimal process contract. `invoke` intentionally returns an unsupported scaffold error until the streaming bridge is implemented.

## Supported Configuration

- `base_url`
- `api_key`
- `validate_model`

The default base URL is `https://api.deepseek.com`.

## Provider-Level Parameter Schema

The plugin declares DeepSeek-specific request parameters in the host-visible order:

- `thinking_type`
- `reasoning_effort`
- `temperature`
- `top_p`
- `max_tokens`
- `response_format`
- `stop`
- `tool_choice`
- `logprobs`
- `top_logprobs`
- `user_id`

Deprecated OpenAI-compatible fields such as `frequency_penalty` and `presence_penalty` are not exposed by this provider.

## Static Models

The bundled static catalog includes:

- `deepseek-v4-flash`
- `deepseek-v4-pro`

Both models declare 1M context, 384K max output, streaming, tool calling, structured output, and reasoning metadata. Static token prices are intentionally omitted; pricing metadata is marked as dynamic.
