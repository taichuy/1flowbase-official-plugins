//! Native event durability for clients without an append-only transcript.
//! Publishing a complete journal revision keeps crash tails invisible to readers.
use crate::{atomic_write, enqueue_source, hash, CheckpointLock, Config, SourceAdapter};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

pub fn append_record<A: SourceAdapter>(
    config: &Config,
    adapter: &A,
    session_id: &str,
    value: &Value,
) -> Result<PathBuf> {
    config.validate()?;
    ensure!(
        !session_id.trim().is_empty(),
        "Native event requires a session identity"
    );
    ensure!(value.is_object(), "Native event must be an object");
    let path = config
        .source_path
        .join(format!("{}.jsonl", hash(session_id)));
    // Serialize journal revision and notification with collectors. Enqueue first so
    // a process dying after atomic publication still leaves a durable retry trigger.
    let _lock = CheckpointLock::wait(&config.state_path)?;
    crate::validate_source(config, adapter, &path)?;
    let header = json!({"type":"collector_session","session_id":session_id});
    let mut bytes = match fs::read(&path) {
        Ok(bytes) => {
            ensure!(
                bytes.last() == Some(&b'\n'),
                "Native journal has an incomplete tail"
            );
            let first = bytes
                .split(|byte| *byte == b'\n')
                .next()
                .unwrap_or_default();
            ensure!(
                serde_json::from_slice::<Value>(first)? == header,
                "Native journal identity differs"
            );
            bytes
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut bytes = serde_json::to_vec(&header)?;
            bytes.push(b'\n');
            bytes
        }
        Err(error) => return Err(error).context("Cannot read native journal"),
    };
    serde_json::to_writer(&mut bytes, value)?;
    bytes.push(b'\n');
    enqueue_source(config, adapter, &path)?;
    atomic_write(&path, &bytes)?;
    Ok(path)
}
