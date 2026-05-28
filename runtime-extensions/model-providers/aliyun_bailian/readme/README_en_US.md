# Alibaba Cloud Bailian Provider

`aliyun_bailian` is an official 1flowbase model provider runtime extension for Alibaba Cloud Model Studio / Bailian.

It supports the same shared API key while allowing each invocation or provider instance to choose a wire protocol:

- `openai_responses`: `POST /compatible-mode/v1/responses`
- `openai_chat`: `POST /compatible-mode/v1/chat/completions`
- `anthropic_messages`: `POST /apps/anthropic/v1/messages`
- `dashscope`: `POST /api/v1/services/aigc/text-generation/generation` or multimodal generation when image/video content is present

The generic OpenAI-compatible provider remains a pass-through provider. Bailian-specific request shaping, model defaults, image content mapping, and tool call conversion live in this provider.

## Supported Configuration

- `api_key`
- `base_url`
- `api_protocol`
- `validate_model`

For China Beijing, leave `base_url` empty or use `https://dashscope.aliyuncs.com`.
For protocol-specific endpoints, the provider derives the documented base path automatically.
