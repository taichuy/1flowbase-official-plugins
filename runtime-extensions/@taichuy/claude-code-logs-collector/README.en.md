# Claude Code logs collector

Version 0.1.1 uses Claude Code command hooks and the shared agent-logs SDK. Each invocation exits after durable source enqueue and delivery; no resident process or automatic history import is installed.

```sh
claude-code-logs-collector configure --endpoint https://HOST/api/logs/v1/events --config /PRIVATE/claude-logs/config.json --source /HOME/.claude --key-stdin
claude-code-logs-collector install-hooks --config /PRIVATE/claude-logs/config.json --client-home /HOME/.claude
claude-code-logs-collector flush --config /PRIVATE/claude-logs/config.json
```

Supply the ingestion key on stdin. `CLAUDE_CONFIG_DIR`, otherwise `~/.claude`, is the default client home. `configure`, `import`, `flush`, `install-hooks`, `uninstall-hooks`, and `hook` use `--config`; `--client-home` selects the native settings directory. `hook` consumes Claude's JSON on stdin and returns `{}` on success. `import` is an explicit historical backfill. A custom source directory or JSONL file restricts what hooks may enqueue.

Installation merges only handlers tagged with this config identity in `settings.json`, with an exact private backup in the config directory. Existing matchers, handlers, model, permissions and other settings are retained. It preserves `disableAllHooks`; enabled native hooks and any required Claude trust are prerequisites. Uninstall removes only this installation's handlers. Hook commands are synchronous so CLI shutdown cannot abandon initial enqueue. Windows uses an encoded PowerShell command; Unix arguments are shell quoted.

SessionStart, UserPromptSubmit, PostToolUse, PostToolUseFailure, Stop, StopFailure, SubagentStop and SessionEnd select the current transcript. SubagentStop uses `agent_transcript_path`, never the parent's `transcript_path`. The source is queued before the bounded 250 ms settling grace period or network. Stop-family and SessionEnd handlers enqueue again after delivery because Claude may write the final transcript record after hooks finish. The next lifecycle event or manual `flush` retries queued tails; no terminal fact is invented for a missing tail. Delivery failures retain SDK checkpoints and pending work.

The adapter accepts native `sessionId`, user `uuid`, assistant `message.content`, model, response ID and source timestamps. Initial file-history snapshots/queue metadata need not have a session ID. A real user prompt establishes the task ID; user-role tool results, meta messages and compact summaries do not replace it. Every content block becomes a fact, including every tool call/result and thinking block. Thinking uses `assistant` plus `phase=reasoning`, redacted thinking uses `redacted_reasoning`; complete raw source rows retain signatures, redacted payloads and nontext blocks. No token total or provider is inferred. Native message usage is a cumulative snapshot scoped by response ID, including cache read/write tokens, so repeated blocks are not described as independent token deltas. `reasoningEffort` or `effort.level` is emitted only if present in the transcript.

Only native assistant `stop_reason=end_turn|stop_sequence` or system `subtype=turn_duration` produces `task_end`. A Stop hook is an attempted stop that another hook can block, and never proves completion. If the current Claude version emits no terminal or effort transcript field, those facts remain absent; the actual wire request must independently prove effort in protocol validation. This collector does not infer a final-answer declaration from an arbitrary last assistant block.

Schema reference: [official Claude Code hooks](https://code.claude.com/docs/en/hooks). The documentation explicitly states transcript writes are asynchronous and documents `agent_transcript_path` and preservation-compatible `statusMessage` command handlers. Development used installed Claude Code 2.1.289. Private user histories are not fixtures; tests contain only synthetic native records. Central integration QA owns actual client/protocol runs.

Native source classification: isMeta and isCompactSummary text remain context rather than real user input; compact facts retain inherited attribution. isApiErrorMessage remains diagnostic context with an error terminal, contributes no synthetic usage, and is never a final answer. <synthetic> cannot replace a real model. A following turn_duration retains error semantics. System prompt text remains unknown when absent from native facts; attachments or injected text are not treated as proof.
