use crate::{
    atomic_write, hash,
    source::{files, reject_symlink_path, Lines},
    AgentLogEvent, AgentLogsBatch, AgentLogsReceipt, CheckpointLock, Config, Position,
    AGENT_LOGS_SCHEMA_VERSION,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

/// Agent-specific format conversion. Empty source_task_id means explicitly unowned,
/// and remains pending until a later source-declared turn claims it.
pub trait SourceAdapter {
    type Context;
    fn source_client(&self) -> &'static str;
    fn roots(&self, source: &Path) -> Vec<PathBuf> {
        vec![source.to_owned()]
    }
    fn create_context(&self, first: &Value) -> Result<Self::Context>;
    fn convert(
        &self,
        record: &Position,
        context: &mut Self::Context,
    ) -> Result<Option<AgentLogEvent>>;
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u32,
    source_id: String,
    source_client: String,
    endpoint: String,
    source_path: PathBuf,
    files: BTreeMap<String, Checkpoint>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    offset: u64,
    prefix_hash: Option<String>,
    #[serde(default)]
    paths: Vec<PathBuf>,
}
struct Entry {
    event: AgentLogEvent,
    end: u64,
    prefix: String,
}
#[derive(Debug)]
pub struct CollectReport {
    pub uploaded: usize,
    pub unattributed_files: usize,
    pub source_id: String,
}
fn persist(path: &Path, state: &State) -> Result<()> {
    atomic_write(
        path,
        &serde_json::to_vec(state).context("Cannot encode checkpoint")?,
    )
}
async fn upload(
    client: &reqwest::Client,
    config: &Config,
    envelope: &AgentLogsBatch,
) -> Result<()> {
    envelope
        .validate()
        .map_err(|_| anyhow::anyhow!("Invalid canonical source event; checkpoint retained"))?;
    let response = client
        .post(&config.endpoint)
        .bearer_auth(&config.api_key)
        .json(envelope)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Upload failed; checkpoint retained"))?;
    ensure!(
        response.status().is_success(),
        "Upload rejected (HTTP {}); checkpoint retained",
        response.status().as_u16()
    );
    // Only canonical ApiSuccess.data is a complete durable acknowledgment.
    let value: Value = response
        .json()
        .await
        .map_err(|_| anyhow::anyhow!("Invalid upload receipt; checkpoint retained"))?;
    let receipt: AgentLogsReceipt =
        serde_json::from_value(value.get("data").cloned().unwrap_or(Value::Null))
            .map_err(|_| anyhow::anyhow!("Invalid upload receipt; checkpoint retained"))?;
    ensure!(
        receipt
            .accepted_events
            .checked_add(receipt.duplicate_events)
            == Some(envelope.events.len()),
        "Incomplete upload receipt; checkpoint retained"
    );
    Ok(())
}
async fn flush(
    client: &reqwest::Client,
    config: &Config,
    state: &mut State,
    identity: &str,
    source_file: &Path,
    batch: &mut Vec<Entry>,
    uploaded: &mut usize,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let envelope = AgentLogsBatch {
        schema_version: AGENT_LOGS_SCHEMA_VERSION.into(),
        source_id: state.source_id.clone(),
        source_client: state.source_client.clone(),
        events: batch.iter().map(|entry| entry.event.clone()).collect(),
    };
    upload(client, config, &envelope).await?;
    let last = batch.last().expect("nonempty batch");
    let mut paths = state
        .files
        .get(identity)
        .map(|checkpoint| checkpoint.paths.clone())
        .unwrap_or_default();
    if !paths.contains(&source_file.to_owned()) {
        paths.push(source_file.to_owned());
    }
    state.files.insert(
        identity.to_owned(),
        Checkpoint {
            offset: last.end,
            prefix_hash: Some(last.prefix.clone()),
            paths,
        },
    );
    persist(&config.state_path, state)?;
    *uploaded += batch.len();
    batch.clear();
    Ok(())
}
pub async fn collect<A: SourceAdapter>(config: &Config, adapter: &A) -> Result<CollectReport> {
    config.validate()?;
    let _lock = CheckpointLock::acquire(&config.state_path)?;
    let mut state = match fs::read(&config.state_path) {
        Ok(bytes) => serde_json::from_slice::<State>(&bytes)
            .map_err(|_| anyhow::anyhow!("Invalid checkpoint"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => State {
            version: 1,
            source_id: uuid::Uuid::new_v4().to_string(),
            source_client: adapter.source_client().into(),
            endpoint: config.endpoint.clone(),
            source_path: config.source_path.clone(),
            files: BTreeMap::new(),
        },
        Err(_) => return Err(anyhow::anyhow!("Cannot read checkpoint")),
    };
    ensure!(
        state.version == 1
            && !state.source_id.is_empty()
            && state.source_client == adapter.source_client()
            && state.endpoint == config.endpoint
            && state.source_path == config.source_path,
        "Checkpoint identity differs; use a separate config directory"
    );
    // Installation identity is durably committed even if first HTTP request fails.
    persist(&config.state_path, &state)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .context("Cannot initialize collector transport")?;
    let mut selected = Vec::new();
    for root in adapter.roots(&config.source_path) {
        reject_symlink_path(&root)?;
        files(&root, &mut selected)?;
    }
    selected.sort();
    selected.dedup();
    let mut uploaded = 0;
    let mut unattributed_files = 0;
    for file in selected {
        let observed_identity = state
            .files
            .iter()
            .find(|(_, checkpoint)| checkpoint.paths.contains(&file))
            .map(|(identity, _)| identity.clone());
        let mut context = None;
        let mut identity = String::new();
        let mut checkpoint = Checkpoint::default();
        let mut verified = false;
        let mut digest = Sha256::new();
        let mut batch = Vec::new();
        let mut pending: Vec<Entry> = Vec::new();
        for (record_index, position) in Lines::open(&file)?.enumerate() {
            // Let watch cancellation run even during long acknowledged/unowned prefixes.
            if record_index % 256 == 0 {
                tokio::task::yield_now().await;
            }
            let position = position?;
            if context.is_none() {
                context = Some(adapter.create_context(&position.line)?);
                let mut header = format!("{}:", adapter.source_client()).into_bytes();
                header.extend_from_slice(&position.bytes);
                identity = hash(header);
                ensure!(
                    observed_identity
                        .as_ref()
                        .is_none_or(|observed| observed == &identity),
                    "Acknowledged source header changed; checkpoint retained"
                );
                checkpoint = state.files.get(&identity).cloned().unwrap_or_default();
                verified = checkpoint.offset == 0;
            }
            digest.update(&position.bytes);
            digest.update(position.end.to_string().as_bytes());
            let prefix = format!("{:x}", digest.clone().finalize());
            let converted =
                adapter.convert(&position, context.as_mut().expect("initialized context"))?;
            if position.end <= checkpoint.offset {
                if position.end == checkpoint.offset {
                    ensure!(
                        checkpoint.prefix_hash.as_deref() == Some(prefix.as_str()),
                        "Acknowledged source prefix changed; checkpoint retained"
                    );
                    verified = true;
                }
                continue;
            }
            ensure!(
                verified,
                "Acknowledged source prefix changed or truncated; checkpoint retained"
            );
            let Some(mut event) = converted else {
                continue;
            };
            event.event_id = hash(format!("{identity}:{}", position.start));
            event.sequence = i64::try_from(position.start)
                .ok()
                .and_then(|n| n.checked_add(1))
                .context("Source byte position exceeds canonical sequence")?;
            let entry = Entry {
                event,
                end: position.end,
                prefix,
            };
            if entry.event.source_task_id.is_empty() {
                pending.push(entry);
                continue;
            }
            for mut buffered in pending.drain(..) {
                buffered.event.source_task_id = entry.event.source_task_id.clone();
                buffered.event.parent_source_task_id = entry.event.parent_source_task_id.clone();
                batch.push(buffered);
                if batch.len() >= 100 {
                    flush(
                        &client,
                        config,
                        &mut state,
                        &identity,
                        &file,
                        &mut batch,
                        &mut uploaded,
                    )
                    .await?;
                }
            }
            batch.push(entry);
            if batch.len() >= 100 {
                flush(
                    &client,
                    config,
                    &mut state,
                    &identity,
                    &file,
                    &mut batch,
                    &mut uploaded,
                )
                .await?;
            }
        }
        ensure!(
            (context.is_none() && observed_identity.is_none()) || verified,
            "Acknowledged source was truncated; checkpoint retained"
        );
        flush(
            &client,
            config,
            &mut state,
            &identity,
            &file,
            &mut batch,
            &mut uploaded,
        )
        .await?;
        if let Some(checkpoint) = state.files.get_mut(&identity) {
            if !checkpoint.paths.contains(&file) {
                checkpoint.paths.push(file.clone());
                persist(&config.state_path, &state)?;
            }
        }
        if !pending.is_empty() {
            unattributed_files += 1;
        }
    }
    Ok(CollectReport {
        uploaded,
        unattributed_files,
        source_id: state.source_id,
    })
}
