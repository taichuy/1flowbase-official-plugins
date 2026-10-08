# Native Codex logs collector

[中文](README.md)

This Rust collector runs on the **computer and user account that runs Codex**. It converts existing and newly written Codex logs into `1flowbase.agent-logs/v1` and uploads them to the selected application's existing logs, conversation and client trajectory views.

Users do not need Node.js, a Rust toolchain or a repository checkout. The package uses `collector-manifest.json`; 1flowbase retains the signed package through the extension catalog without executing it on the server.

## Install

First install the collector distribution into the current 1flowbase node from the application’s **Collector CLI** page. This retains assets and does not mean that a client is running. Select Codex, optionally paste an application Key or generate one, then use **Copy command**. The copied command passes the real Key through `FLOWBASE_AGENT_LOGS_API_KEY`; the preview masks it. If no Key is supplied, the installer asks in the local terminal. It saves the Key in private user configuration and sends it only to the configured 1flowbase endpoint as a Bearer header. Keys can be managed or revoked on the application's API page.

The Linux/macOS command has this structure; replace the endpoint and application ID:

```bash
installer="$(mktemp)"
curl -fsSL 'https://YOUR_HOST/api/public/client-collectors/taichuy/codex-logs-collector/0.2.1/assets/install.sh' -o "$installer"
bash "$installer" --endpoint 'https://YOUR_HOST/api/logs/v1/events' --installation-id 'YOUR_APPLICATION_ID' --version '0.2.1' --release-base 'https://YOUR_HOST/api/public/client-collectors/taichuy/codex-logs-collector/0.2.1/assets'
rm -f "$installer"
```

On Windows, copy the PowerShell command from the application page. Its parameters are `-Endpoint`, `-InstallationId`, `-Version` and required `-ReleaseBase`. Windows uses a task for the current user; Linux uses user systemd and macOS uses a LaunchAgent. Collection starts immediately and resumes when this user logs in after a reboot.

If Linux has no user systemd session, installation reports background startup failure explicitly. Rerun it in a supported user session, or configure with `--no-start` and run native `watch` under your own process supervisor. No sudo is required. Installation never modifies Codex configuration or source logs.

## Scope and recovery

The default source is the install-time `CODEX_HOME`, or `~/.codex` if unset (the `.codex` directory in the Windows user profile). Only `sessions` and `archived_sessions` are collected, including existing history and subsequent records. `--source PATH` selects a Codex home directory, a specific rollout directory or a rollout file.

Only newline-committed JSONL records are uploaded. Network errors, HTTP failures and incomplete acknowledgments retain the checkpoint. Recovery replays the same event identities. Moving a rollout into archives does not count it twice. Changed or truncated acknowledged content rejects that collection attempt and retains its checkpoint. Records without an explicit source turn wait for source attribution; the collector does not invent tasks.

Each application ID has an independent installation and source identity. Reinstalling or upgrading retains configuration and checkpoints. Reconfiguration can rotate the key but cannot silently rebind endpoint or source. Use another `--installation-id` for a different deployment or source binding.

## Historical import, batching and upgrades

Version 0.2.0 uses stateful file tracking in the shared SDK. A checkpoint atomically saves the acknowledged source byte position, the minimal parsing context at that position and any required source attribution claim. Each collection still verifies acknowledged historical prefixes, but history with a saved context no longer undergoes repeated JSON decoding or format conversion. Only new complete records are converted. Strict prefix verification still reads historical bytes; its I/O cost does not depend solely on the amount of new data.

Requests are batched by their complete serialized size, with a default target of **1 MiB (1048576 bytes)**. This includes `raw`, message content, JSON escaping, metadata and separators. The target is configurable and does not cap the total number of records. An event larger than the target is sent intact as a singleton. Files take turns sending batches while retaining each file's source order. Only one request awaits durable acknowledgment at a time; checkpoints never advance before acknowledgment.

For manual operation, adjust the target with:

```text
codex-logs-collector import --config /path/to/config.json --batch-bytes 262144
codex-logs-collector watch --config /path/to/config.json --batch-bytes 262144
```

`--batch-bytes` must be a positive integer. It does not change configuration, source identity or event identities. Background services use the default target unless explicitly configured otherwise. Facts awaiting an explicit source turn are replayed from source byte ranges and minimal context, rather than accumulating complete events in memory. Their scan cursor is stored separately and does not advance the server-acknowledged cursor. Attribution requires a source-declared turn; tools, models, providers and raw facts retain the context of their original records. Replay depends on source logs remaining readable; the collector does not delete them.

To upgrade from 0.1.0, install the 0.2.0 distribution in 1flowbase, then run that version's local installation command. Keep the installation ID, configuration and `state.json`. On first reading a legacy file checkpoint, the collector validates history using its original digest algorithm, reconstructs the parsing context and migrates it while preserving `source_id`, acknowledged offsets and stable event IDs. Unavailable legacy files retain their checkpoints and migrate when they return, without blocking collection of other files. Older executables cannot read the new checkpoints; do not mix executable versions or delete checkpoints to downgrade. The existing 0.1.0 release remains immutable, and previously retained packages remain available at their original version.

## Local files and manual operation

Linux/macOS defaults to `${XDG_DATA_HOME:-$HOME/.local/share}/1flowbase/collectors/codex/APPLICATION_ID/`. Windows uses `%LOCALAPPDATA%\1flowbase\collectors\codex\APPLICATION_ID\`. The directory contains `bin`, private `config.json` and `state.json`. Do not publish configuration contents.

```text
codex-logs-collector --help
codex-logs-collector --version
codex-logs-collector import --config /path/to/config.json
codex-logs-collector watch --config /path/to/config.json
```

A running background collector exclusively owns its checkpoint. Stop that service before running manual import/watch. The operating system releases the lock after a process crash; deleting checkpoints is unnecessary.

To uninstall, rerun the same installer using `--uninstall --installation-id APPLICATION_ID` on Linux/macOS, or `-Uninstall -InstallationId APPLICATION_ID` on Windows. It removes the application's service and executable while retaining configuration, checkpoints and Codex logs. If you overrode install/config paths, supply those same paths when uninstalling.

## Releases and development

The `collector-release` workflow tests source code and builds Linux musl, macOS and Windows binaries for amd64 and arm64. Each release publishes one host-independent signed distribution tar.gz, containing six opaque native archives, public installers, bilingual README files, SHA-256 checksums, an Ed25519 checksum signature, a public key and source SHA metadata. The root manifest records every asset name, digest and size. The existing extension catalog advertises its outer SHA-256 and Ed25519 signature. Installers require the version-pinned asset base copied from 1flowbase and fetch the binary and checksums from that same node; no GitHub fallback is used. Removing the retained package disables new downloads. Installers verify archive checksums. Downloads must respond directly; HTTP redirects are refused. Signatures support independent verification and release auditing; checksum verification is not presented as signature verification.

The shared `sdk/agent-logs-collector` owns file identity indexing, prefix verification, context checkpoints, source-range replay, byte batching, round-robin scheduling, uploading, ACK validation and recovery. This plugin owns Codex mapping, minimal serializable context and CLI behavior. The SDK uses canonical Rust DTOs from a pinned main-repository Git revision rather than copying protocol definitions. The architecture draws on [Vector's file source](https://github.com/vectordotdev/vector/tree/master/lib/file-source) and [Fluent Bit's tail input](https://github.com/fluent/fluent-bit/tree/master/plugins/in_tail), implemented with existing Rust standard-library facilities and dependencies rather than embedding either complete collector. Source tests live in SDK/plugin `src/_tests`; installer and publication tests live in `scripts/collectors/_tests`.

## 0.2.1: Turn identity and model fields

The collector uses actual Codex turn/task identifiers. Responses API passthrough metadata cannot create another turn. `turn_context.model` and `turn_context.effort` map to the canonical `model_id` and `reasoning_effort` fields, which 1flowbase projects into its log list. Client trajectories retain the complete original events. Records without an explicit Codex owner remain pending; the collector does not invent turn boundaries.

Existing 0.2.0 checkpoints remain readable; reasoning effort is an optional context field. The update applies to subsequent collection and does not rewrite already acknowledged server projections. To repair imported history, first delete the affected records through the 1flowbase application-log API, then reset the corresponding checkpoint in a controlled reimport. Upgrading or running import again does not resend acknowledged history. Retain configuration, source identity, original files and the checkpoint backup.
