# OpenCode logs collector

OpenCode's default V1 runtime emits native `message.updated` and `message.part.updated` events. The embedded plugin synchronously records these facts into the shared SDK's private per-session journal. V2 `session.next.*` canonical events are also supported. Installation starts no daemon and reads no database or historical conversations.

```sh
opencode-logs-collector configure --endpoint http://localhost:7800/api/public/applications/APP/agent-logs --source /absolute/collector/sessions --config /absolute/collector/config.json --key-stdin
opencode-logs-collector install-hooks --config /absolute/collector/config.json --client-home /absolute/opencode-config
opencode-logs-collector flush --config /absolute/collector/config.json
opencode-logs-collector uninstall-hooks --config /absolute/collector/config.json --client-home /absolute/opencode-config
```

Supply the ingestion key on stdin. `--client-home` is the OpenCode configuration directory, including task-isolated `OPENCODE_CONFIG_DIR`. Only one hash-named `plugins/*.js` file is owned; existing config and other plugins are preserved. Each installation uses its own journal/checkpoint directory.

The native callback uses `spawnSync` argument arrays to finish durable recording before returning. Idle/dispose only flush facts already recorded, never manufacture a successful result. Failed delivery remains queued for the next flush. Live deltas are ignored.

V1 user text uses the native part-notification timestamp, falling back to its message creation time when absent. Assistant text/reasoning is accepted only with native `part.time.end`. Running tools supply call/input; completed/error tools supply result and also the call/input if its running notification was missed. Completed assistant metadata supplies source usage and finish; `finish=stop` marks the retained actual text as the final answer. Other finishes continue the task or preserve error context. Raw source facts are retained.

The default CLI uses V1 provider config, even in current source. Example model:
```json
{
  "provider": {
    "gateway": {
      "npm": "@ai-sdk/openai-compatible",
      "options": {
        "baseURL": "http://localhost:7800/v1",
        "apiKey": "{env:QA_GATEWAY_KEY}"
      },
      "models": {
        "gpt-6-luna": {
          "reasoning": true,
          "limit": { "context": 262144, "output": 8192 },
          "variants": { "max": { "reasoningEffort": "max" } }
        }
      }
    }
  }
}
```

For Chat use `@ai-sdk/openai-compatible`; Responses use `@ai-sdk/openai`, both with `variants.max.reasoningEffort="max"`. Anthropic uses `@ai-sdk/anthropic` and `variants.max.effort="max"`. Run `opencode run --format json -m gateway/gpt-6-luna --variant max`. Verify protocol and effort on the gateway. `OPENCODE_EXPERIMENTAL_NATIVE_LLM` changes only model transport, not Session/config version; the default AI SDK path supports these controls. A variant label alone does not prove wire effort.

Focused verification: `cargo test --manifest-path runtime-extensions/@taichuy/opencode-logs-collector/Cargo.toml`. Integration additionally requires real default-client continuation, tool interaction and ingestion receipts. Fixtures are synthetic; old installed binaries require separate compatibility verification.

Version 0.1.1 retains synthetic parts as Context. User metadata alone does not establish a human turn; a non-synthetic, non-ignored text/file/agent/subtask part establishes it. Mixed messages retain real text. Known user aliases resolve assistant.parentID to the real turn. New real input prunes settled assistant bodies while retaining user aliases and unsettled assistant attribution. Late old parts cannot replace the active turn. When joining mid-session, an unseen native parentID remains exact unclassified attribution without an invented user message. Parts without message metadata retain the existing no-fabrication boundary; the journal is preserved. Only text/reasoning and tool lifecycle project surface text; other part content is not guessed.

The official experimental.chat.system.transform hook records the native system array as a System/raw request_system_snapshot without changing it or other user configuration. Calls without sessionID are omitted. This is the pre-request value at this plugin's position; later plugins may transform it, so it is not a claim about the final wire payload.
