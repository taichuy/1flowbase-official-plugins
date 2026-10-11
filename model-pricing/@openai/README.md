# GPT-6 and GPT-6.1 pricing notes

GPT-6.1 Sol pricing was verified on 2026-10-11 against the [official model page](https://developers.openai.com/api/docs/models/gpt-6.1-sol).
Prices are USD per million tokens for the first-party standard API.

| Model ID | Input | Output | Cache read | Cache write |
| --- | --- | --- | --- | --- |
| `gpt-6.1-sol` | $2 | $10 | $0.10 | $2.50 |
| `gpt-6.1-sol`, input over 272,000 tokens | $4 | $15 | $0.20 | $5 |

GPT-6.1 Sol and the existing GPT-6 Astra, Sol, and Luna sources have a 1,050,000-token context window and a 128,000-token maximum output. If request input exceeds 272,000 tokens, all input and cache rates double, and the output rate increases by 50%. The threshold applies to the whole request; exactly 272,000 tokens uses standard rates. Existing GPT-6 configurations retain their published prices.

Batch and Flex each cost 50% of the corresponding standard rates; Fast costs 200%. GPT-6.1 Sol also supports Ultrafast at 600% of standard rates, and regional processing adds a 10% premium where available. Mode multipliers also apply after the large-input adjustment. Schema v2 has no mode or inference-geography condition, so the JSON sources represent standard API pricing only. Context and output limits are model capabilities, not pricing fields in this schema.

For GPT-6.1 Sol, `effective_from` records this catalog addition on 2026-10-11, not the vendor release date.
