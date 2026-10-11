//! Durable event-triggered delivery. Each notification has its own file, so a trigger
//! arriving during an upload cannot be erased by acknowledgement of an older trigger.
use crate::{
    atomic_write, collector::collect_locked, config::absolute, source::reject_symlink_path,
    CheckpointLock, CollectOptions, Config, SourceAdapter,
};
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn directory(config: &Config) -> PathBuf {
    let mut path = config.state_path.as_os_str().to_owned();
    path.push(".pending");
    PathBuf::from(path)
}

/// A hook may select a file, never expand the installation's authorized source roots.
pub fn validate_source<A: SourceAdapter>(
    config: &Config,
    adapter: &A,
    path: &Path,
) -> Result<PathBuf> {
    config.validate()?;
    let path = absolute(path)?;
    reject_symlink_path(&path)?;
    ensure!(
        path.extension().is_some_and(|value| value == "jsonl"),
        "Expected a JSONL transcript"
    );
    ensure!(
        adapter.roots(&config.source_path).iter().any(|root| {
            if root.extension().is_some_and(|value| value == "jsonl") {
                path == *root
            } else {
                path.starts_with(root)
            }
        }),
        "Transcript is outside the configured source"
    );
    Ok(path)
}

pub fn enqueue_source<A: SourceAdapter>(config: &Config, adapter: &A, path: &Path) -> Result<()> {
    let path = validate_source(config, adapter, path)?;
    atomic_write(
        &directory(config).join(format!("{}.json", uuid::Uuid::new_v4())),
        &serde_json::to_vec(&path)?,
    )
}

#[derive(Default)]
pub struct PendingReport {
    pub uploaded: usize,
    pub pending_sources: usize,
}

/// No resident loop and no directory-wide source scan. Failed sources remain queued
/// for the next invocation; one failed transcript does not prevent other deliveries.
pub async fn collect_pending<A: SourceAdapter>(
    config: &Config,
    adapter: &A,
    options: CollectOptions,
) -> Result<PendingReport> {
    config.validate()?;
    // Every invoker persists its trigger before waiting. Even a killed waiting process
    // leaves work for a later hook, and concurrent successful invokers cannot lose it.
    let _lock = CheckpointLock::wait(&config.state_path)?;
    let pending = directory(config);
    reject_symlink_path(&pending)?;
    let entries = match fs::read_dir(&pending) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PendingReport::default())
        }
        Err(error) => return Err(error).context("Cannot read pending collector sources"),
    };
    let mut sources: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for entry in entries {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        reject_symlink_path(&entry.path())?;
        let source: PathBuf = serde_json::from_slice(&fs::read(entry.path())?)
            .context("Invalid pending source notification")?;
        let source = validate_source(config, adapter, &source)?;
        sources.entry(source).or_default().push(entry.path());
    }
    let mut report = PendingReport::default();
    for (source, notifications) in sources {
        if !source.is_file() {
            report.pending_sources += 1;
            continue;
        }
        match collect_locked(config, adapter, options, Some(&source)).await {
            Ok(collected) => {
                report.uploaded += collected.uploaded;
                if collected.unattributed_files > 0 {
                    report.pending_sources += 1;
                    continue;
                }
                for notification in notifications {
                    fs::remove_file(notification)?;
                }
            }
            Err(error) => {
                eprintln!("Agent logs delivery deferred: {error}");
                report.pending_sources += 1;
            }
        }
    }
    Ok(report)
}
