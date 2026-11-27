# Antigravity Native Logs Collector

[简体中文](README.md)

Native Rust client collector for Antigravity App, IDE, and CLI. It converts append-only local Antigravity JSONL transcripts into `1flowbase.agent-logs/v1` batches and uploads them to a target 1flowbase Agent Logs application.

No Node.js, Rust toolchain, or `agy` CLI is required on the client machine. Transport, byte-offset checkpoints, ACKs, and retries are handled by the shared `agent-logs-collector` SDK. No resident daemon is installed and historical conversations are never imported automatically.

## Installation

Install the collector distribution into your 1flowbase node from the Agent Logs **Collector CLI** page, choose **Antigravity**, prepare an Application API Key, and run the generated Shell or Windows PowerShell command.

```sh
antigravity-logs-collector configure --endpoint https://HOST/api/logs/v1/events --config /PRIVATE/antigravity-logs/config.json --source /HOME/.gemini --key-stdin
antigravity-logs-collector install-hooks --config /PRIVATE/antigravity-logs/config.json --client-home /HOME/.gemini
antigravity-logs-collector flush --config /PRIVATE/antigravity-logs/config.json
```

Default client home is `GEMINI_HOME` or `~/.gemini`. `install-hooks` writes an isolated native Antigravity plugin under `~/.gemini/config/plugins/1flowbase-agent-logs/` (`plugin.json` and `hooks.json`) without modifying `~/.gemini/config/config.json` or global `~/.gemini/config/hooks.json`. Each installation owns a config-hashed hook group; uninstall removes only its own entry and cleans up the plugin directory when empty.

## Non-blocking Hook Delivery

Because Antigravity runs lifecycle hooks synchronously, `SessionStart`, `PostInvocation`, and `Stop` invoke the native collector executable directly (no PowerShell wrapper on Windows):

1. Resolve the active conversation transcript from `transcriptPath` (or `artifactDirectoryPath`), preferring untruncated `transcript_full.jsonl` with fallback to `transcript.jsonl` (strictly one file per conversation);
2. Persist the validated path into the private `.pending` queue;
3. Spawn a detached, short-lived, windowless `flush` child process (with a 250 ms settle wait on `Stop`) and immediately write `{}` to stdout so the agent loop is never blocked;
4. Failed uploads retain checkpoints and pending entries for the next hook or manual `flush`. Historical backfill requires explicit `import`.
