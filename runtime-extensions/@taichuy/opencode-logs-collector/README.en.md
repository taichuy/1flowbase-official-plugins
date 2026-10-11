# OpenCode logs collector

OpenCode 1.18.4 native `session.next.*` events are recorded synchronously into a private per-session journal and delivered using the shared agent-logs collector SDK. No daemon, database scan, or historical import runs during installation.

```sh
opencode-logs-collector configure --endpoint http://localhost:7800/api/public/applications/APP/agent-logs --source /absolute/collector/sessions --config /absolute/collector/config.json --key-stdin
opencode-logs-collector install-hooks --config /absolute/collector/config.json --client-home /absolute/opencode-config
opencode-logs-collector flush --config /absolute/collector/config.json
opencode-logs-collector uninstall-hooks --config /absolute/collector/config.json --client-home /absolute/opencode-config
```

Supply the ingestion key on stdin. `--client-home` is the OpenCode configuration directory, including a task-isolated `OPENCODE_CONFIG_DIR`. The installer owns only one hash-named `plugins/*.js` file; existing config and plugins stay intact. Each installation must use a separate config directory for its journal/checkpoint.

The embedded plugin uses `spawnSync` with argument arrays to record committed events before the unawaited native callback returns. It records prompted messages, request steps, ended text/reasoning, tool calls/results and request usage. Live deltas and legacy projected duplicate events are ignored. Idle/dispose only flush the journal; they never manufacture completion or usage. Delivery failures remain pending for the next flush. Journal replay uses SDK checkpoints; it does not re-read client history.

A native prompt message ID identifies the imported task. `step.ended` with finish `stop` supplies task completion; `tool-calls` continues that task. Other finishes and failures remain factual context until a source completion exists. Native timestamps, raw events, model/provider and model variant are preserved. The variant is a client label, so verify actual wire reasoning separately.

For current OpenCode source, define a model variant `max` with request-body overrides: Chat `{"reasoning_effort":"max"}`, Responses `{"reasoning":{"effort":"max"}}`, Anthropic `{"output_config":{"effort":"max"}}`. Native OpenAI effort schema excludes max; the configured HTTP body overlay sends these values without changing client source. Select the correct native/provider API and `--variant max`; assert gateway wire evidence. Old installed binaries may use a different API and are unsupported until separately verified.

Focused verification: `cargo test --manifest-path runtime-extensions/@taichuy/opencode-logs-collector/Cargo.toml`. Native plugin/source integration requires a real current-source OpenCode run and ingestion receipts. Fixtures contain only synthetic data.
