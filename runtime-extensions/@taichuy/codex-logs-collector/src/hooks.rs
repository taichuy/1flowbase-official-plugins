use agent_logs_collector::{
    atomic_write, collect_pending, enqueue_source, hash, validate_source, CheckpointLock,
    CollectOptions, Config,
};
use anyhow::{ensure, Context, Result};
use base64::Engine;
use codex_logs_collector::CodexAdapter;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PostToolUse",
    "Stop",
    "SubagentStop",
    "Interrupt",
    "SessionEnd",
];
// Stop/Interrupt precede the terminal rollout event. Async handlers give Codex time
// to commit it. This is a bounded per-invocation drain, never a resident watcher.
const SETTLE_BUDGET: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
struct Input {
    hook_event_name: String,
    #[serde(default)]
    transcript_path: Option<PathBuf>,
    #[serde(default)]
    agent_transcript_path: Option<PathBuf>,
    #[serde(default)]
    turn_id: Option<String>,
}

async fn await_terminal(path: &Path, turn: &str) -> Result<bool> {
    let deadline = Instant::now() + SETTLE_BUDGET;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("Cannot open hook transcript"),
    };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        let count = reader
            .read_line(&mut line)
            .context("Cannot read hook transcript")?;
        if line.ends_with('\n') {
            if line.contains("task_complete")
                || line.contains("turn_complete")
                || line.contains("turn_aborted")
            {
                if let Ok(row) = serde_json::from_str::<Value>(&line) {
                    let payload = &row["payload"];
                    if row["type"] == "event_msg"
                        && matches!(
                            payload["type"].as_str(),
                            Some("task_complete" | "turn_complete" | "turn_aborted")
                        )
                        && payload["turn_id"].as_str() == Some(turn)
                    {
                        return Ok(true);
                    }
                }
            }
            line.clear();
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        if count == 0 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

pub async fn run(config: &Config, adapter: &CodexAdapter, options: CollectOptions) -> Result<()> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .context("Cannot read Codex hook input")?;
    let input: Input = serde_json::from_slice(&bytes).context("Invalid Codex hook input")?;
    ensure!(
        EVENTS.contains(&input.hook_event_name.as_str()),
        "Unsupported Codex hook event"
    );
    // SubagentStop's transcript_path belongs to the parent; never mistake it for the child.
    let selected = if input.hook_event_name == "SubagentStop" {
        input.agent_transcript_path
    } else {
        input.transcript_path
    };
    let mut unsettled = None;
    if let Some(path) = selected {
        // Custom source selections intentionally ignore hooks for other projects/files.
        let path = match validate_source(config, adapter, &path) {
            Ok(path) => path,
            Err(error) => {
                eprintln!("Agent logs transcript skipped: {error}");
                collect_pending(config, adapter, options).await?;
                return Ok(());
            }
        };
        // Durable before any waiting/network. A timeout or Codex exit cannot lose the trigger.
        enqueue_source(config, adapter, &path)?;
        if matches!(
            input.hook_event_name.as_str(),
            "Stop" | "SubagentStop" | "Interrupt"
        ) {
            if let Some(turn) = input.turn_id {
                if !await_terminal(&path, &turn).await? {
                    unsettled = Some(path);
                }
            }
        }
    }
    let report = collect_pending(config, adapter, options).await?;
    if let Some(path) = unsettled {
        // Other Stop hooks may continue the turn or take longer than our budget.
        // Preserve a retry instead of fabricating completion from hook payload text.
        enqueue_source(config, adapter, &path)?;
    }
    if report.pending_sources > 0 {
        eprintln!(
            "Agent logs: {} sources queued for the next hook or manual flush",
            report.pending_sources
        );
    }
    Ok(())
}

fn quote(value: &Path) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "'\\''"))
}
fn quote_windows(value: &Path) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "''"))
}

/// Merge only this installation's handlers. Existing hooks, notify and trust state
/// are owned by Codex/the user and are never replaced or automatically trusted.
pub fn register(config: &Path, home: &Path, install: bool) -> Result<()> {
    let config = super::absolute(config.to_owned())?;
    if install {
        Config::load(&config)?;
    }
    let home = super::absolute(home.to_owned())?;
    let path = home.join("hooks.json");
    let _lock = CheckpointLock::acquire(&path)?;
    let existing = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("Cannot read existing Codex hooks"),
    };
    if !install && existing.is_none() {
        return Ok(());
    }
    let mut document: Value = match &existing {
        Some(bytes) => {
            serde_json::from_slice(bytes).context("Invalid existing hooks.json; left unchanged")?
        }
        None => json!({"hooks": {}}),
    };
    let object = document
        .as_object_mut()
        .context("Codex hooks must be an object")?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("Codex hooks must contain an event object")?;
    let tag = format!(
        "1flowbase agent logs {}",
        hash(config.to_string_lossy().as_bytes())
    );
    for event in EVENTS {
        if let Some(groups) = hooks.get_mut(*event) {
            let groups = groups
                .as_array_mut()
                .context("Invalid Codex hook groups; left unchanged")?;
            for group in groups.iter_mut() {
                let handlers = group
                    .get_mut("hooks")
                    .and_then(Value::as_array_mut)
                    .context("Invalid Codex hook handlers; left unchanged")?;
                handlers.retain(|handler| handler["statusMessage"].as_str() != Some(&tag));
            }
            groups.retain(|group| {
                group["hooks"]
                    .as_array()
                    .is_some_and(|handlers| !handlers.is_empty())
            });
        }
    }
    if install {
        let binary = std::env::current_exe().context("Cannot locate collector executable")?;
        for event in EVENTS {
            let command = format!("{} hook --config {}", quote(&binary), quote(&config));
            // Codex invokes commandWindows through cmd.exe. Encoding the PowerShell
            // command protects spaces, Unicode, quotes and cmd's % / ! expansion.
            let windows_script = format!(
                "& {} hook --config {}",
                quote_windows(&binary),
                quote_windows(&config)
            );
            let windows_bytes: Vec<u8> = windows_script
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect();
            let command_windows = format!(
                "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
                base64::engine::general_purpose::STANDARD.encode(windows_bytes)
            );
            let handler = json!({
                "type": "command", "command": command, "commandWindows": command_windows,
                "async": *event != "SessionEnd",
                "timeout": if matches!(*event, "SessionEnd" | "Interrupt") { 3 } else { 120 },
                "statusMessage": tag,
            });
            hooks
                .entry(*event)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .context("Invalid Codex hook event")?
                .push(json!({"hooks": [handler]}));
        }
    }
    let encoded = serde_json::to_vec_pretty(&document)?;
    if existing.as_deref() != Some(encoded.as_slice()) {
        if let Some(bytes) = existing {
            let backup = config
                .parent()
                .context("Config needs a parent directory")?
                .join(format!("hooks-backup-{}.json", hash(&bytes)));
            if !backup.exists() {
                atomic_write(&backup, &bytes)?;
            }
        }
        atomic_write(&path, &encoded)?;
    }
    if install {
        println!("Codex hooks installed. Review and trust these handlers in Codex /hooks. No background service is registered. Use import for historical backfill.");
    } else {
        println!("This installation's Codex hooks removed; other handlers retained.");
    }
    Ok(())
}
