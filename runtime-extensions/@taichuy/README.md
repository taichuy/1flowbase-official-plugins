# Official Runtime Extensions

Model provider runtime extensions live in this tree.

Each provider should have its own directory here, for example `openai_compatible/`.

Managed notification contributors also live here, including `wechat-chat-channels/`.
They use the Host's shared notification dispatcher and do not declare a model
provider slot. Setup documentation is available in each plugin's bilingual README.
