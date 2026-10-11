use crate::ClaudeAdapter;
use agent_logs_collector::{
    atomic_write, collect_pending, enqueue_source, hash, validate_source, CheckpointLock,
    CollectOptions, Config,
};
use anyhow::{ensure, Context, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "StopFailure",
    "SubagentStop",
    "SessionEnd",
];
pub fn selected_transcript(input: &Value) -> Result<Option<PathBuf>> {
    let event = input["hook_event_name"]
        .as_str()
        .context("Claude hook lacks event name")?;
    ensure!(EVENTS.contains(&event), "Unsupported Claude hook event");
    let selected = if event == "SubagentStop" {
        &input["agent_transcript_path"]
    } else {
        &input["transcript_path"]
    };
    Ok(selected
        .as_str()
        .filter(|p| !p.is_empty())
        .map(PathBuf::from))
}
pub fn absolute(path: &Path) -> Result<PathBuf> {
    let input = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut result = PathBuf::new();
    for part in input.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            part => result.push(part.as_os_str()),
        }
    }
    Ok(result)
}
pub async fn run(config: &Config, adapter: &ClaudeAdapter, options: CollectOptions) -> Result<()> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .context("Cannot read Claude hook input")?;
    let input: Value = serde_json::from_slice(&bytes).context("Invalid Claude hook input")?;
    let event = input["hook_event_name"]
        .as_str()
        .context("Claude hook lacks event name")?;
    let selected = selected_transcript(&input)?;
    let mut path = None;
    if let Some(selected) = selected {
        match validate_source(config, adapter, &selected) {
            Ok(selected) => {
                enqueue_source(config, adapter, &selected)?;
                path = Some(selected);
            }
            Err(error) => eprintln!("Claude transcript skipped: {error}"),
        }
    }
    if matches!(
        event,
        "Stop" | "StopFailure" | "SubagentStop" | "SessionEnd"
    ) {
        // Native transcripts are asynchronous. This short per-hook grace period
        // never claims that a missing record or stop_reason has been observed.
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let report = collect_pending(config, adapter, options).await?;
    if matches!(
        event,
        "Stop" | "StopFailure" | "SubagentStop" | "SessionEnd"
    ) {
        // Writes can happen after all Stop handlers finish. Keep the transcript
        // queued for the next lifecycle event/manual flush even after this ACK.
        if let Some(path) = path {
            enqueue_source(config, adapter, &path)?;
        }
    }
    if report.pending_sources > 0 {
        eprintln!(
            "Claude logs: {} sources queued for retry",
            report.pending_sources
        );
    }
    Ok(())
}
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
fn command(binary: &Path, config: &Path) -> String {
    if cfg!(windows) {
        let quote = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "''"));
        let script = format!("& {} hook --config {}", quote(binary), quote(config));
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        format!(
            "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    } else {
        format!("{} hook --config {}", quote(binary), quote(config))
    }
}
pub fn merge_settings(document: &mut Value, tag: &str, command: &str, install: bool) -> Result<()> {
    let settings = document
        .as_object_mut()
        .context("Claude settings must be an object; left unchanged")?;
    if !install && !settings.contains_key("hooks") {
        return Ok(());
    }
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("Claude hooks must be an object; left unchanged")?;
    for event in EVENTS {
        if let Some(groups) = hooks.get_mut(*event) {
            let groups = groups
                .as_array_mut()
                .context("Invalid Claude hook groups; left unchanged")?;
            for group in groups.iter_mut() {
                let handlers = group
                    .get_mut("hooks")
                    .and_then(Value::as_array_mut)
                    .context("Invalid Claude hook handlers; left unchanged")?;
                handlers.retain(|handler| handler["statusMessage"].as_str() != Some(tag));
            }
            groups.retain(|group| group["hooks"].as_array().is_some_and(|v| !v.is_empty()));
        }
        if install {
            let handler = json!({"type":"command", "command":command, "timeout": if *event == "SessionEnd" { 5 } else { 30 }, "statusMessage":tag});
            hooks
                .entry(*event)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .context("Invalid Claude hook event")?
                .push(json!({"hooks":[handler]}));
        }
    }
    Ok(())
}
/// Only this installation's handlers are replaced; all unrelated settings and
/// handlers are preserved, with an exact private backup before any mutation.
pub fn register(config: &Path, home: &Path, install: bool) -> Result<()> {
    let config = absolute(config)?;
    if install {
        Config::load(&config)?;
    }
    let path = absolute(home)?.join("settings.json");
    let _lock = CheckpointLock::acquire(&path)?;
    let existing = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("Cannot read Claude settings"),
    };
    if !install && existing.is_none() {
        return Ok(());
    }
    let mut document: Value = match &existing {
        Some(bytes) => {
            serde_json::from_slice(bytes).context("Invalid Claude settings JSON; left unchanged")?
        }
        None => json!({}),
    };
    let tag = format!(
        "1flowbase agent logs {}",
        hash(config.to_string_lossy().as_bytes())
    );
    let binary = std::env::current_exe().context("Cannot locate collector executable")?;
    merge_settings(&mut document, &tag, &command(&binary, &config), install)?;
    let bytes = serde_json::to_vec_pretty(&document)?;
    if existing.as_deref() != Some(bytes.as_slice()) {
        if let Some(original) = existing {
            let backup = config
                .parent()
                .context("Config needs parent directory")?
                .join(format!("claude-settings-backup-{}.json", hash(&original)));
            if !backup.exists() {
                atomic_write(&backup, &original)?;
            }
        }
        atomic_write(&path, &bytes)?;
    }
    println!(
        "Claude hooks {}. Other settings retained; historical import is explicit.",
        if install { "installed" } else { "removed" }
    );
    Ok(())
}
#[cfg(test)]
#[path = "_tests/hooks.rs"]
mod tests;
