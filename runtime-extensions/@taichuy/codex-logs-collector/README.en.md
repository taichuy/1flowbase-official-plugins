# Native Codex logs collector

[中文](README.md)

This Rust collector runs on the **computer and user account that runs Codex**. It converts existing and newly written Codex logs into `1flowbase.agent-logs/v1` and uploads them to the selected application's existing logs, conversation and client trajectory views.

Users do not need Node.js, a Rust toolchain or a repository checkout. The package uses `collector-manifest.json`; 1flowbase retains the signed package through the extension catalog without executing it on the server.

## Install

First install the collector distribution into the current 1flowbase node from the application’s **Collector CLI** page. This retains assets and does not mean that a client is running. Then open **Collector CLI** in an Agent Logs application, select Codex and copy the command for your operating system. Create or manage an application API Key on the API page. The install command contains no key. The installer asks for it in your local terminal and saves it in private user configuration. The credential is sent only to the configured 1flowbase endpoint as a Bearer header.

The Linux/macOS command has this structure; replace the endpoint and application ID:

```bash
installer="$(mktemp)"
curl -fsSL 'https://YOUR_HOST/api/public/client-collectors/taichuy/codex-logs-collector/0.1.0/assets/install.sh' -o "$installer"
bash "$installer" --endpoint 'https://YOUR_HOST/api/logs/v1/events' --installation-id 'YOUR_APPLICATION_ID' --version '0.1.0' --release-base 'https://YOUR_HOST/api/public/client-collectors/taichuy/codex-logs-collector/0.1.0/assets'
rm -f "$installer"
```

On Windows, copy the PowerShell command from the application page. Its parameters are `-Endpoint`, `-InstallationId`, `-Version` and required `-ReleaseBase`. Windows uses a task for the current user; Linux uses user systemd and macOS uses a LaunchAgent. Collection starts immediately and resumes when this user logs in after a reboot.

If Linux has no user systemd session, installation reports background startup failure explicitly. Rerun it in a supported user session, or configure with `--no-start` and run native `watch` under your own process supervisor. No sudo is required. Installation never modifies Codex configuration or source logs.

## Scope and recovery

The default source is the install-time `CODEX_HOME`, or `~/.codex` if unset (the `.codex` directory in the Windows user profile). Only `sessions` and `archived_sessions` are collected, including existing history and subsequent records. `--source PATH` selects a Codex home directory, a specific rollout directory or a rollout file.

Only newline-committed JSONL records are uploaded. Network errors, HTTP failures and incomplete acknowledgments retain the checkpoint. Recovery replays the same event identities. Moving a rollout into archives does not count it twice. Changed or truncated acknowledged content rejects that collection attempt and retains its checkpoint. Records without an explicit source turn wait for source attribution; the collector does not invent tasks.

Each application ID has an independent installation and source identity. Reinstalling or upgrading retains configuration and checkpoints. Reconfiguration can rotate the key but cannot silently rebind endpoint or source. Use another `--installation-id` for a different deployment or source binding.

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

The shared `sdk/agent-logs-collector` owns reading, uploading, ACK validation and recovery. This plugin owns Codex format mapping and CLI behavior. The SDK uses canonical Rust DTOs from a pinned main-repository Git revision rather than copying protocol definitions. Source tests live in SDK/plugin `src/_tests`; installer and publication tests live in `scripts/collectors/_tests`.
