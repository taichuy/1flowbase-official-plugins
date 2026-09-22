# GPT-6 pricing notes

The GPT-6 Astra, Sol, and Luna pricing sources use USD per million tokens. Each has a 1,050,000-token context window and a 128,000-token maximum output. If request input exceeds 272,000 tokens, the rates for all input and cache meters double, and the output rate increases by 50%. The threshold applies to the whole request.

Batch and Flex each cost 50% of the corresponding standard rates; Fast costs 200%. These mode multipliers also apply after the large-input adjustment. Pricing source schema v2 has no mode condition, so the JSON sources represent standard API pricing only. Context and output limits are model capabilities, not pricing fields in this schema.
