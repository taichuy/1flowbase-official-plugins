# DeepSeek Harness logs collector 0.1.0

Native Cordis plugin, verified against DeepSeek Harness 0.2.1-alpha.2 source d743267388. The executable embeds its plugin; no daemon or package manager installation is needed. Configure and install never import prior sessions and never change persistence compression.

```sh
deepseek-harness-logs-collector configure --endpoint https://HOST/api/logs/v1/events --config /absolute/collector.json --source /absolute/private-journal --key-stdin
deepseek-harness-logs-collector install-hooks --config /absolute/collector.json --client-home /absolute/dsh-home
deepseek-harness-logs-collector flush --config /absolute/collector.json
deepseek-harness-logs-collector uninstall-hooks --config /absolute/collector.json --client-home /absolute/dsh-home
```

Provide the ingestion key on stdin. The config and journal are private SDK files. Omit --source to use DSH_HOME/1flowbase-logs/journal (or ~/.dsh/1flowbase-logs/journal). Select the same home used by the client. Imports are explicit and accept only collector journals, never the client's compressed persistence store.

Registration appends one marked YAML patch to cordis.patch.yml and installs 1flowbase-logs/plugin.mjs. Other patch text, including !!js expressions, is retained. Reinstallation replaces only the marked block; uninstall removes it and removes the generated plugin only when its SHA256 receipt still matches. Config, journal and delivery checkpoints remain available for explicit retry. Ambiguous marker blocks, unowned plugin files and JSON-array patch files fail closed. For a JSON-array home patch, supply the embedded plugin with a separate manually selected --patch overlay rather than rewriting user configuration.

session/event is fire-and-forget. Each callback uses spawnSync to run record --config PATH --session ID with native JSON on stdin. SDK append_record syncs the source before enqueue; normal events do not perform HTTP. turn/end adds --flush because turn boundaries do not automatically flush in this Harness. Awaited session/flush also invokes flush. Delivery failures remain pending for the next explicit/native flush and emit a generic diagnostic without private event/config text.

The adapter emits each message block separately; reasoning remains distinct from assistant text. Dedicated tool/call owns lifecycle identity; embedded assistant tool-call blocks remain in raw without duplicating lifecycle events. Tool results, nested PTC calls, interrupted prefixes, exact terminal reasons, native seq/time/surface metadata, session parent lineage, and compact assistant streams remain in raw. Failed assistant attempts aggregate text/reasoning deltas by block and retain their attempt phase; they do not create terminal facts. Usage is emitted only when reported. Native inputTokens is uncached input. Canonical input_tokens includes inputTokens, cacheReadTokens and cacheWriteTokens; the cache counters retain their independent native meaning. totalTokens is never invented. reasoningTokens remains in raw because the existing canonical usage contract has no corresponding field.

Source task identity uses explicit turn_id, otherwise native numeric turn; explicit parent_turn_id/parentTaskId stays attached through the turn. Session parentSession is lineage in raw and is not a parent task identity. Request headers and message source preserve provider/model/reasoningEffort. No session ending is inferred from process shutdown, missing turn/end or incomplete streams.

References: packages/core/session/src/types.ts and index.ts, packages/llm/llm/src/{types,message,assistant-stream}.ts, packages/boot/app-boot/src/{profile,profile-context,index}.ts in the referenced Harness. Fixture and Rust tests cover atomic multi-block facts, tool identity, usage absence/cache counts, attempt aggregation, terminal error facts and preservation of native patch expressions. Builds and acceptance are centralized in the multi-client test batch.

For Anthropic gateway connections, the DSH provider baseURL is the service root without /v1. Set the custom gpt-6-luna model to reasoningEfforts: {max: max} and compat: {forceAdaptiveThinking: true} to send adaptive thinking with output_config.effort=max. Chat/Responses use a baseURL ending in /v1.
