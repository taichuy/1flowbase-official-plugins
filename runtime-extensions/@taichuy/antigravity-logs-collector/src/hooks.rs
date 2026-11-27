//! Isolated Antigravity plugin hook registration and non-blocking event enqueue.
use crate::{canonical_transcript_for_path, AntigravityAdapter};
use agent_logs_collector::{
    atomic_write, collect_pending, enqueue_source, hash, validate_source, CheckpointLock,
    CollectOptions, Config, SourceAdapter,
};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const PLUGIN_NAME: &str = "1flowbase-agent-logs";
pub const PLUGIN_DISPLAY_NAME: &str = "1flowbase Agent Logs";
pub const EVENTS: &[&str] = &["SessionStart", "PostInvocation", "Stop"];
const ACCEPTED_EVENTS: &[&str] = &[
    "SessionStart",
    "PreInvocation",
    "PostInvocation",
    "PreToolUse",
    "PostToolUse",
    "Stop",
];

#[derive(Deserialize)]
struct Input {
    #[serde(rename = "hookEventName")]
    hook_event_name: Option<String>,
    #[serde(rename = "transcriptPath")]
    transcript_path: Option<PathBuf>,
    #[serde(rename = "artifactDirectoryPath")]
    artifact_directory_path: Option<PathBuf>,
}

pub fn absolute(path: &Path) -> Result<PathBuf> {
    ensure!(
        path.is_absolute(),
        "Client home and collector config must be absolute"
    );
    ensure!(
        !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir)),
        "Parent components are not allowed"
    );
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) => ensure!(
                !meta.file_type().is_symlink(),
                "Symbolic links are not allowed"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Cannot inspect path"),
        }
    }
    Ok(path.to_owned())
}

fn reject_unsafe_cmd_chars(path: &Path) -> Result<String> {
    let text = path.to_string_lossy().into_owned();
    ensure!(
        !text.contains(['\r', '\n', '\0', '"']),
        "Hook command paths must not contain quotes or control characters"
    );
    Ok(text)
}

pub fn hook_script_relpath(group_key: &str) -> String {
    format!("scripts\\{group_key}.cmd")
}

pub fn render_windows_cmd_wrapper(binary: &Path, config: &Path) -> Result<String> {
    let bin = reject_unsafe_cmd_chars(binary)?.replace('%', "%%");
    let cfg = reject_unsafe_cmd_chars(config)?.replace('%', "%%");
    Ok(format!("@echo off\r\n\"{bin}\" hook --config \"{cfg}\"\r\n"))
}

pub fn hook_command(binary: &Path, config: &Path, group_key: &str) -> Result<String> {
    if cfg!(windows) {
        // Relative quote-free .cmd path avoids cmd.exe /c quote-escaping bugs across Go/Node runners.
        reject_unsafe_cmd_chars(binary)?;
        reject_unsafe_cmd_chars(config)?;
        Ok(hook_script_relpath(group_key))
    } else {
        let quote = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"));
        Ok(format!("{} hook --config {}", quote(binary), quote(config)))
    }
}

pub fn hook_group_key(config: &Path) -> String {
    let digest = hash(config.to_string_lossy().as_bytes());
    format!("{PLUGIN_NAME}-{}", &digest[..12])
}

fn plugin_manifest() -> Value {
    json!({
        "name": PLUGIN_NAME,
        "displayName": PLUGIN_DISPLAY_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Collects local Antigravity conversation transcripts and sends them to 1flowbase Agent Logs."
    })
}

pub fn merge_hooks(
    document: &mut Value,
    group_key: &str,
    command: &str,
    install: bool,
) -> Result<()> {
    let object = document
        .as_object_mut()
        .context("Antigravity plugin hooks.json must be a JSON object")?;
    object.remove(group_key);
    if install {
        let mut group = serde_json::Map::new();
        group.insert("enabled".into(), json!(true));
        for event in EVENTS {
            group.insert(
                (*event).into(),
                json!([{
                    "type": "command",
                    "command": command,
                    "timeout": 10
                }]),
            );
        }
        object.insert(group_key.to_owned(), Value::Object(group));
    }
    Ok(())
}

/// Owns only `~/.gemini/config/plugins/1flowbase-agent-logs/`.
/// Never modifies `~/.gemini/config/config.json` or global `~/.gemini/config/hooks.json`.
pub fn register(config: &Path, client_home: &Path, install: bool) -> Result<()> {
    let config = absolute(config)?;
    if install {
        Config::load(&config)?;
    }
    let home = absolute(client_home)?;
    let plugin_dir = home.join("config").join("plugins").join(PLUGIN_NAME);
    let manifest_path = plugin_dir.join("plugin.json");
    let hooks_path = plugin_dir.join("hooks.json");

    if manifest_path.exists() {
        absolute(&manifest_path)?;
        let existing: Value = serde_json::from_slice(
            &fs::read(&manifest_path).context("Cannot read existing plugin.json")?,
        )
        .context("Invalid existing plugin.json; left unchanged")?;
        ensure!(
            existing["name"].as_str() == Some(PLUGIN_NAME),
            "Plugin directory belongs to a different plugin; left unchanged"
        );
    } else if !install && !hooks_path.exists() {
        return Ok(());
    }

    let _lock = CheckpointLock::acquire(&hooks_path)?;
    let existing_hooks = match fs::read(&hooks_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("Cannot read plugin hooks.json"),
    };
    let mut document: Value = match &existing_hooks {
        Some(bytes) => {
            serde_json::from_slice(bytes).context("Invalid plugin hooks.json; left unchanged")?
        }
        None => json!({}),
    };
    let group_key = hook_group_key(&config);
    let binary = std::env::current_exe().context("Cannot locate collector executable")?;
    let cmd = hook_command(&binary, &config, &group_key)?;
    merge_hooks(&mut document, &group_key, &cmd, install)?;

    let scripts_dir = plugin_dir.join("scripts");
    let wrapper_path = scripts_dir.join(format!("{group_key}.cmd"));

    if install {
        if cfg!(windows) {
            let wrapper = render_windows_cmd_wrapper(&binary, &config)?;
            atomic_write(&wrapper_path, wrapper.as_bytes())?;
        }
        atomic_write(&manifest_path, &serde_json::to_vec_pretty(&plugin_manifest())?)?;
        atomic_write(&hooks_path, &serde_json::to_vec_pretty(&document)?)?;
        println!("Antigravity plugin installed at {}", plugin_dir.display());
        return Ok(());
    }

    if wrapper_path.exists() {
        fs::remove_file(&wrapper_path).context("Cannot remove plugin hook wrapper")?;
    }
    let _ = fs::remove_dir(&scripts_dir);

    let empty = document.as_object().is_some_and(|map| map.is_empty());
    if empty {
        if hooks_path.exists() {
            fs::remove_file(&hooks_path).context("Cannot remove plugin hooks.json")?;
        }
        if manifest_path.exists() {
            fs::remove_file(&manifest_path).context("Cannot remove plugin.json")?;
        }
        drop(_lock);
        let mut lock_file = hooks_path.as_os_str().to_owned();
        lock_file.push(".lock");
        let _ = fs::remove_file(&lock_file);
        let _ = fs::remove_dir(&plugin_dir);
    } else {
        atomic_write(&hooks_path, &serde_json::to_vec_pretty(&document)?)?;
    }
    println!("Antigravity plugin hook removed.");
    Ok(())
}

fn match_authorized_root(
    config: &Config,
    adapter: &AntigravityAdapter,
    candidate: &Path,
) -> Option<PathBuf> {
    let preferred = canonical_transcript_for_path(candidate);
    let roots = adapter.roots(&config.source_path);
    if let Some(exact) = roots.iter().find(|root| root.as_path() == preferred.as_path()) {
        return Some(exact.clone());
    }
    #[cfg(windows)]
    {
        let target = preferred
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase();
        if let Some(matched) = roots.iter().find(|root| {
            root.to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase()
                == target
        }) {
            return Some(matched.clone());
        }
    }
    None
}

#[cfg(windows)]
fn clear_std_handle_inheritance() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(n_std_handle: u32) -> isize;
        fn SetHandleInformation(h_object: isize, dw_mask: u32, dw_flags: u32) -> i32;
    }
    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
    // STD_INPUT_HANDLE (-10), STD_OUTPUT_HANDLE (-11), STD_ERROR_HANDLE (-12)
    for std_handle in [0xFFFF_FFF6u32, 0xFFFF_FFF5u32, 0xFFFF_FFF4u32] {
        unsafe {
            let handle = GetStdHandle(std_handle);
            if handle != 0 && handle != -1 {
                let _ = SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

fn spawn_detached_flush(
    config_path: &Path,
    options: CollectOptions,
    settle_ms: u64,
) -> Result<()> {
    #[cfg(windows)]
    clear_std_handle_inheritance();

    let binary = std::env::current_exe().context("Cannot locate collector executable")?;
    let mut cmd = Command::new(binary);
    cmd.arg("flush")
        .arg("--config")
        .arg(config_path)
        .arg("--batch-bytes")
        .arg(options.batch_target_bytes.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if settle_ms > 0 {
        cmd.env("FLOWBASE_COLLECTOR_SETTLE_MS", settle_ms.to_string());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NO_WINDOW prevents console flashing and never blocks parent.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    let _child = cmd.spawn().context("Cannot spawn detached flush worker")?;
    Ok(())
}

pub async fn run(
    config: &Config,
    config_path: &Path,
    adapter: &AntigravityAdapter,
    options: CollectOptions,
) -> Result<()> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .context("Cannot read Antigravity hook input")?;
    let input: Input = if bytes.iter().all(|b| b.is_ascii_whitespace()) {
        Input {
            hook_event_name: Some("Stop".into()),
            transcript_path: None,
            artifact_directory_path: None,
        }
    } else {
        serde_json::from_slice(&bytes).context("Invalid Antigravity hook JSON")?
    };

    let event_name = input.hook_event_name.as_deref().unwrap_or("Stop");
    ensure!(
        ACCEPTED_EVENTS.contains(&event_name),
        "Unsupported Antigravity hook event: {event_name}"
    );

    let raw_candidate = input.transcript_path.or_else(|| {
        input.artifact_directory_path.map(|dir| {
            dir.join(".system_generated")
                .join("logs")
                .join("transcript.jsonl")
        })
    });

    if let Some(candidate) = raw_candidate {
        let resolved = match_authorized_root(config, adapter, &candidate)
            .unwrap_or_else(|| canonical_transcript_for_path(&candidate));
        match validate_source(config, adapter, &resolved) {
            Ok(valid) => {
                enqueue_source(config, adapter, &valid)?;
            }
            Err(error) => {
                eprintln!("Antigravity logs transcript skipped: {error}");
            }
        }
    }

    if std::env::var("FLOWBASE_COLLECTOR_SYNC_HOOK").as_deref() == Ok("1") {
        let report = collect_pending(config, adapter, options).await?;
        if report.pending_sources > 0 {
            eprintln!(
                "Antigravity logs: {} sources queued for retry",
                report.pending_sources
            );
        }
        return Ok(());
    }

    let settle_ms = if event_name == "Stop" { 250 } else { 0 };
    if let Err(error) = spawn_detached_flush(config_path, options, settle_ms) {
        eprintln!("Antigravity logs detached flush deferred: {error}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "_tests/hooks.rs"]
mod tests;
