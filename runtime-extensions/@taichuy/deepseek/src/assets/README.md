# DeepSeek V4 tokenizer provenance

- Source archive: `https://cdn.deepseek.com/api-docs/deepseek_v4_tokenizer.zip`
- Source page: `https://api-docs.deepseek.com/quick_start/token_usage`
- Retrieved: `2026-08-29`
- Archive SHA-256: `e7310d1dafe0a86d8a5629fe78a7c763760f651db9b8682718a1781dcd6fe495`
- Embedded `tokenizer.json` SHA-256: `89085f12ef79460ac5f66d1119325ddfc694b4ab209d80bbd81d35f081dc9614`

The runtime embeds the pinned JSON at compile time. It never downloads tokenizer
data while serving `count_tokens` requests. DeepSeek's returned API usage remains
the source of truth for billing.
