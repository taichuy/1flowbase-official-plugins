# Gemini Provider

This runtime extension connects 1flowbase to Gemini-compatible `v1beta` GenerateContent APIs.

By default it calls Google AI Studio:

- `GET /v1beta/models`
- `POST /v1beta/models/{model}:streamGenerateContent?alt=sse`

Set `base_url` to a Gemini-compatible proxy, such as a local sub2api gateway, when needed.
