use crate::{
    atomic_write, hash,
    source::{files, reject_symlink_path, Lines, RawPosition},
    AgentLogEvent, AgentLogsBatch, AgentLogsReceipt, CheckpointLock, Config, Position,
    AGENT_LOGS_SCHEMA_VERSION,
};
use anyhow::{ensure, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

/// Context snapshots contain only adapter state needed to convert the next record.
pub trait SourceAdapter {
    type Context: Clone + Serialize + DeserializeOwned;
    /// Bump when the serialized context contract changes. Unknown versions fail closed.
    fn context_version(&self) -> u32 {
        1
    }
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
/// Soft size target for the exact canonical JSON envelope. Single oversized facts are intact.
pub const DEFAULT_BATCH_TARGET_BYTES: usize = 1024 * 1024;
#[derive(Clone, Copy, Debug)]
pub struct CollectOptions {
    pub batch_target_bytes: usize,
}
impl Default for CollectOptions {
    fn default() -> Self {
        Self {
            batch_target_bytes: DEFAULT_BATCH_TARGET_BYTES,
        }
    }
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
    #[serde(default)]
    context: Option<Snapshot>,
    #[serde(default)]
    scan: Option<Cursor>,
    #[serde(default)]
    legacy: bool,
    #[serde(default)]
    pending: Option<Pending>,
    #[serde(default)]
    claim: Option<Claim>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    offset: u64,
    prefix_hash: Option<String>,
    context: Option<Snapshot>,
}
impl Checkpoint {
    fn cursor(&self) -> Cursor {
        Cursor {
            offset: self.offset,
            prefix_hash: self.prefix_hash.clone(),
            context: self.context.clone(),
        }
    }
    fn set_cursor(&mut self, cursor: Cursor) {
        self.offset = cursor.offset;
        self.prefix_hash = cursor.prefix_hash;
        self.context = cursor.context;
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u32,
    value: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    start: u64,
    prefix_hash: Option<String>,
    context: Snapshot,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    end: u64,
    prefix_hash: String,
    task: String,
    parent: Option<String>,
}
#[derive(Debug)]
pub struct CollectReport {
    pub uploaded: usize,
    pub unattributed_files: usize,
    pub source_id: String,
}
fn persist(path: &Path, state: &State) -> Result<()> {
    let bytes = serde_json::to_vec(state).context("Cannot encode checkpoint")?;
    if fs::read(path).is_ok_and(|existing| existing == bytes) {
        return Ok(());
    }
    atomic_write(path, &bytes)
}
fn snapshot<A: SourceAdapter>(adapter: &A, context: &A::Context) -> Result<Snapshot> {
    Ok(Snapshot {
        version: adapter.context_version(),
        value: serde_json::to_value(context)?,
    })
}
fn restore<A: SourceAdapter>(adapter: &A, snapshot: &Snapshot) -> Result<A::Context> {
    ensure!(
        snapshot.version == adapter.context_version(),
        "Unsupported adapter context version"
    );
    serde_json::from_value(snapshot.value.clone()).context("Invalid adapter context checkpoint")
}
// This is the v1 digest, retained exactly: nonblank bytes (without newline), then decimal end.
fn advance(digest: &mut Sha256, raw: &RawPosition) {
    digest.update(&raw.bytes);
    digest.update(raw.end.to_string().as_bytes());
}
fn prefix(digest: &Sha256) -> String {
    format!("{:x}", digest.clone().finalize())
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
struct Runner<C> {
    path: PathBuf,
    identity: String,
    checkpoint: Checkpoint,
    context: C,
    digest: Sha256,
    acknowledged: Cursor,
}
/// Validate all acknowledged/scanned historical bytes, without JSON parsing in v2.
/// This intentionally retains O(history) I/O to detect arbitrary prefix changes.
async fn prepare<A: SourceAdapter>(
    file: &Path,
    observed: Option<&String>,
    state: &State,
    adapter: &A,
) -> Result<Option<Runner<A::Context>>> {
    let mut lines = Lines::open(file)?;
    let Some(first) = lines.next().transpose()? else {
        ensure!(
            observed.is_none(),
            "Acknowledged source was truncated; checkpoint retained"
        );
        return Ok(None);
    };
    let mut header = format!("{}:", adapter.source_client()).into_bytes();
    header.extend_from_slice(&first.bytes);
    let identity = hash(header);
    ensure!(
        observed.is_none_or(|id| id == &identity),
        "Acknowledged source header changed; checkpoint retained"
    );
    let mut cp = state.files.get(&identity).cloned().unwrap_or_default();
    let acknowledged = cp.cursor();
    if let Some(scan) = cp.scan.take() {
        cp.set_cursor(scan);
    }
    let mut digest = Sha256::new();
    let legacy = cp.context.is_none() && cp.offset > 0;
    if legacy {
        ensure!(
            cp.legacy || state.version == 1,
            "Missing adapter context checkpoint"
        );
        // v1 replays once, but only after its complete historical digest has been checked.
    }
    let mut verified = cp.offset == 0;
    let validation_end = cp.claim.as_ref().map_or(cp.offset, |claim| claim.end);
    let mut claim_verified = cp.claim.is_none();
    let mut cursor_digest = Sha256::new();
    for (index, raw) in std::iter::once(Ok(first)).chain(lines).enumerate() {
        if index % 256 == 0 {
            tokio::task::yield_now().await;
        }
        let raw = raw?;
        if raw.end > validation_end {
            break;
        }
        advance(&mut digest, &raw);
        if raw.end == cp.offset {
            ensure!(
                cp.prefix_hash.as_deref() == Some(prefix(&digest).as_str()),
                "Acknowledged source prefix changed; checkpoint retained"
            );
            verified = true;
            cursor_digest = digest.clone();
        }
        if let Some(claim) = &cp.claim {
            if raw.end == claim.end {
                ensure!(
                    claim.prefix_hash == prefix(&digest),
                    "Pending claim source prefix changed; checkpoint retained"
                );
                claim_verified = true;
            }
        }
        if raw.end == validation_end {
            break;
        }
    }
    ensure!(
        verified && claim_verified,
        "Acknowledged source prefix changed or truncated; checkpoint retained"
    );
    digest = cursor_digest;
    let context = if let Some(saved) = &cp.context {
        restore(adapter, saved)?
    } else {
        let mut records = Lines::open(file)?;
        let first = records
            .next()
            .transpose()?
            .context("Missing source header")?
            .decode()?;
        let mut recovered = adapter.create_context(&first.line)?;
        if legacy {
            adapter.convert(&first, &mut recovered)?;
            for (index, raw) in records.enumerate() {
                if index % 256 == 0 {
                    tokio::task::yield_now().await;
                }
                let raw = raw?;
                if raw.end > cp.offset {
                    break;
                }
                adapter.convert(&raw.decode()?, &mut recovered)?;
            }
        }
        cp.context = Some(snapshot(adapter, &recovered)?);
        recovered
    };
    if !cp.paths.contains(&file.to_owned()) {
        cp.paths.push(file.to_owned());
    }
    cp.legacy = false;
    let mut acknowledged = if legacy { cp.cursor() } else { acknowledged };
    if acknowledged.context.is_none() && acknowledged.offset == 0 {
        acknowledged.context = cp.context.clone();
    }
    Ok(Some(Runner {
        path: file.to_owned(),
        identity,
        checkpoint: cp,
        context,
        digest,
        acknowledged,
    }))
}
fn envelope(state: &State, events: Vec<AgentLogEvent>) -> AgentLogsBatch {
    AgentLogsBatch {
        schema_version: AGENT_LOGS_SCHEMA_VERSION.into(),
        source_id: state.source_id.clone(),
        source_client: state.source_client.clone(),
        events,
    }
}
fn durable_checkpoint<C>(runner: &Runner<C>) -> Checkpoint {
    let mut saved = runner.checkpoint.clone();
    let scan = saved.cursor();
    saved.scan = if scan.offset != runner.acknowledged.offset {
        Some(scan)
    } else {
        None
    };
    saved.set_cursor(runner.acknowledged.clone());
    saved
}
fn save_runner<C>(config: &Config, state: &mut State, runner: &Runner<C>) -> Result<()> {
    state
        .files
        .insert(runner.identity.clone(), durable_checkpoint(runner));
    persist(&config.state_path, state)
}
/// One scheduler visit yields at most one upload. File handles are reopened per visit.
async fn visit<A: SourceAdapter>(
    config: &Config,
    adapter: &A,
    options: CollectOptions,
    client: &reqwest::Client,
    state: &mut State,
    runner: &mut Runner<A::Context>,
    uploaded: &mut usize,
) -> Result<bool> {
    let mut batch = Vec::new();
    let mut encoded_bytes = serde_json::to_vec(&envelope(state, Vec::new()))?.len();
    let mut lines = Lines::at(&runner.path, runner.checkpoint.offset)?;
    let mut exhausted = true;
    let mut count = 0;
    while let Some(raw) = lines.next() {
        count += 1;
        if count % 256 == 0 {
            tokio::task::yield_now().await;
        }
        let raw = raw?;
        let before = runner.checkpoint.clone();
        let before_context = runner.context.clone();
        let before_digest = runner.digest.clone();
        advance(&mut runner.digest, &raw);
        let position = raw.decode()?;
        let converted = adapter.convert(&position, &mut runner.context)?;
        runner.checkpoint.offset = position.end;
        runner.checkpoint.prefix_hash = Some(prefix(&runner.digest));
        runner.checkpoint.context = Some(snapshot(adapter, &runner.context)?);
        if let Some(mut event) = converted {
            if let Some(claim) = &before.claim {
                if event.source_task_id.is_empty() && position.end <= claim.end {
                    event.source_task_id = claim.task.clone();
                    event.parent_source_task_id = claim.parent.clone();
                }
            }
            if event.source_task_id.is_empty() {
                // Flush owned facts before starting an unowned interval.
                if !batch.is_empty() {
                    runner.checkpoint = before;
                    runner.context = before_context;
                    runner.digest = before_digest;
                    exhausted = false;
                    break;
                }
                if runner.checkpoint.pending.is_none() {
                    runner.checkpoint.pending = Some(Pending {
                        start: before.offset,
                        prefix_hash: before.prefix_hash,
                        context: snapshot(adapter, &before_context)?,
                    });
                }
            } else if let Some(pending) = runner.checkpoint.pending.take() {
                // Persist the explicit claim before replay. Crucially restore ORIGINAL context,
                // not the context of this future claiming record.
                runner.checkpoint.claim = Some(Claim {
                    end: position.end,
                    prefix_hash: prefix(&runner.digest),
                    task: event.source_task_id,
                    parent: event.parent_source_task_id,
                });
                runner.checkpoint.offset = pending.start;
                runner.checkpoint.prefix_hash = pending.prefix_hash;
                runner.checkpoint.context = Some(pending.context.clone());
                runner.context = restore(adapter, &pending.context)?;
                runner.digest = Sha256::new();
                for (index, raw) in Lines::open(&runner.path)?.enumerate() {
                    if index % 256 == 0 {
                        tokio::task::yield_now().await;
                    }
                    let raw = raw?;
                    if raw.end > pending.start {
                        break;
                    }
                    advance(&mut runner.digest, &raw);
                }
                save_runner(config, state, runner)?;
                lines = Lines::at(&runner.path, pending.start)?;
                continue;
            } else {
                event.event_id = hash(format!("{}:{}", runner.identity, position.start));
                event.sequence = i64::try_from(position.start)
                    .ok()
                    .and_then(|n| n.checked_add(1))
                    .context("Source byte position exceeds canonical sequence")?;
                let event_bytes = serde_json::to_vec(&event)?.len();
                let candidate_bytes = encoded_bytes + event_bytes + usize::from(!batch.is_empty());
                if !batch.is_empty() && candidate_bytes > options.batch_target_bytes {
                    runner.checkpoint = before;
                    runner.context = before_context;
                    runner.digest = before_digest;
                    exhausted = false;
                    break;
                }
                encoded_bytes = candidate_bytes;
                batch.push(event);
                if encoded_bytes >= options.batch_target_bytes {
                    if runner
                        .checkpoint
                        .claim
                        .as_ref()
                        .is_some_and(|claim| position.end >= claim.end)
                    {
                        runner.checkpoint.claim = None;
                    }
                    exhausted = false;
                    break;
                }
            }
        }
        if runner
            .checkpoint
            .claim
            .as_ref()
            .is_some_and(|claim| position.end >= claim.end)
        {
            runner.checkpoint.claim = None;
        }
    }
    if !batch.is_empty() {
        let envelope = envelope(state, batch);
        debug_assert_eq!(serde_json::to_vec(&envelope)?.len(), encoded_bytes);
        upload(client, config, &envelope).await?;
        *uploaded += envelope.events.len();
        runner.acknowledged = runner.checkpoint.cursor();
    }
    save_runner(config, state, runner)?;
    Ok(exhausted)
}
pub async fn collect<A: SourceAdapter>(config: &Config, adapter: &A) -> Result<CollectReport> {
    collect_with_options(config, adapter, CollectOptions::default()).await
}
pub async fn collect_with_options<A: SourceAdapter>(
    config: &Config,
    adapter: &A,
    options: CollectOptions,
) -> Result<CollectReport> {
    config.validate()?;
    ensure!(
        options.batch_target_bytes > 0,
        "Batch byte target must be positive"
    );
    let _lock = CheckpointLock::acquire(&config.state_path)?;
    let mut state = match fs::read(&config.state_path) {
        Ok(bytes) => serde_json::from_slice::<State>(&bytes)
            .map_err(|_| anyhow::anyhow!("Invalid checkpoint"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => State {
            version: 2,
            source_id: uuid::Uuid::new_v4().to_string(),
            source_client: adapter.source_client().into(),
            endpoint: config.endpoint.clone(),
            source_path: config.source_path.clone(),
            files: BTreeMap::new(),
        },
        Err(_) => return Err(anyhow::anyhow!("Cannot read checkpoint")),
    };
    ensure!(
        (state.version == 1 || state.version == 2)
            && !state.source_id.is_empty()
            && state.source_client == adapter.source_client()
            && state.endpoint == config.endpoint
            && state.source_path == config.source_path,
        "Checkpoint identity differs; use a separate config directory"
    );
    // A new installation ID must exist before its first request; existing state is
    // untouched until source integrity checks and any migration succeed.
    if !config.state_path.exists() {
        persist(&config.state_path, &state)?;
    }
    if state.version == 1 {
        for checkpoint in state.files.values_mut() {
            checkpoint.legacy = true;
        }
    }
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
    let path_index: BTreeMap<PathBuf, String> = state
        .files
        .iter()
        .flat_map(|(id, cp)| cp.paths.iter().cloned().map(move |path| (path, id.clone())))
        .collect();
    // Inventory by immutable raw header, with O(1) index lookup for previously observed paths.
    let mut identities: BTreeMap<String, Vec<(PathBuf, u64)>> = BTreeMap::new();
    for path in selected {
        let mut records = Lines::open(&path)?;
        let Some(first) = records.next().transpose()? else {
            ensure!(
                !path_index.contains_key(&path),
                "Acknowledged source was truncated; checkpoint retained"
            );
            continue;
        };
        let mut header = format!("{}:", adapter.source_client()).into_bytes();
        header.extend_from_slice(&first.bytes);
        let identity = hash(header);
        ensure!(
            path_index.get(&path).is_none_or(|old| old == &identity),
            "Acknowledged source header changed; checkpoint retained"
        );
        let length = fs::metadata(&path).context("Cannot inspect source")?.len();
        identities.entry(identity).or_default().push((path, length));
    }
    let mut queue = VecDeque::new();
    let mut identities = identities.into_iter().collect::<Vec<_>>();
    identities.sort_by(|a, b| {
        a.1.iter()
            .map(|entry| &entry.0)
            .min()
            .cmp(&b.1.iter().map(|entry| &entry.0).min())
    });
    for (identity, mut aliases) in identities {
        aliases.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let primary = aliases[0].0.clone();
        // Every shorter snapshot must be an exact committed prefix of the primary.
        // Two open readers at most; divergent same-header aliases fail before any upload.
        for (alias, _) in aliases.iter().skip(1) {
            let mut primary_records = Lines::open(&primary)?;
            for (index, raw) in Lines::open(alias)?.enumerate() {
                if index % 256 == 0 {
                    tokio::task::yield_now().await;
                }
                let raw = raw?;
                let reference = primary_records
                    .next()
                    .transpose()?
                    .context("Source identity aliases diverged")?;
                ensure!(
                    raw.start == reference.start
                        && raw.end == reference.end
                        && raw.bytes == reference.bytes,
                    "Source identity aliases diverged; checkpoint retained"
                );
            }
        }
        if let Some(mut runner) =
            prepare(&primary, path_index.get(&primary), &state, adapter).await?
        {
            ensure!(
                runner.identity == identity,
                "Source header changed during collection"
            );
            for (path, _) in aliases {
                if !runner.checkpoint.paths.contains(&path) {
                    runner.checkpoint.paths.push(path);
                }
            }
            queue.push_back(runner);
        }
    }
    // Missing legacy files remain marked for strict lazy migration when they reappear.
    for runner in &queue {
        state
            .files
            .insert(runner.identity.clone(), durable_checkpoint(runner));
    }
    state.version = 2;
    persist(&config.state_path, &state)?;
    let mut uploaded = 0;
    let mut unattributed_files = 0;
    while let Some(mut runner) = queue.pop_front() {
        if visit(
            config,
            adapter,
            options,
            &client,
            &mut state,
            &mut runner,
            &mut uploaded,
        )
        .await?
        {
            if runner.checkpoint.pending.is_some() {
                unattributed_files += 1;
            }
        } else {
            queue.push_back(runner);
        }
    }
    Ok(CollectReport {
        uploaded,
        unattributed_files,
        source_id: state.source_id,
    })
}
