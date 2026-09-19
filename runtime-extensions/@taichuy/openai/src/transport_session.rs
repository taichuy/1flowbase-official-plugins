use super::*;

pub(crate) const TRANSPORT_SESSION_CONTEXT_KEY: &str = "physical_transport_session";
pub(crate) const TRANSPORT_SESSION_RECEIPT_METADATA_KEY: &str =
    "1flowbase_physical_transport_session";
pub(crate) const INVOCATION_TIMING_RECEIPT_METADATA_KEY: &str =
    "1flowbase_provider_invocation_timing";
const INVOCATION_TIMING_SCHEMA_VERSION: u8 = 1;
const MAX_OPAQUE_ID_BYTES: usize = 256;
const MAX_CONNECTION_LIFETIME_MS: u64 = 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LogicalSessionState {
    Active,
    Waiting,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransportSessionDirective {
    pub logical_session_id: String,
    pub generation: u64,
    #[serde(default)]
    pub worker_incarnation: Option<u64>,
    pub task_id: String,
    pub state: LogicalSessionState,
    pub physical_deadline_unix_ms: i64,
}

impl TransportSessionDirective {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_opaque_id("logical_session_id", &self.logical_session_id)?;
        if self.worker_incarnation == Some(0) {
            bail!("worker incarnation must be positive");
        }
        if self.generation == 0 {
            bail!("transport session directive generation must be positive");
        }
        validate_opaque_id("task_id", &self.task_id)?;
        if self.physical_deadline_unix_ms <= 0 {
            bail!("transport session physical_deadline_unix_ms must be positive");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TransportSessionAction {
    Drain,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransportSessionCommand {
    pub logical_session_id: String,
    pub generation: u64,
    #[serde(default)]
    pub worker_incarnation: Option<u64>,
    pub action: TransportSessionAction,
    pub deadline_unix_ms: i64,
}

impl TransportSessionCommand {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_opaque_id("logical_session_id", &self.logical_session_id)?;
        if self.worker_incarnation == Some(0) {
            bail!("worker incarnation must be positive");
        }
        if self.generation == 0 {
            bail!("transport session generation must be positive");
        }
        if self.deadline_unix_ms <= 0 {
            bail!("transport session deadline must be positive");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PhysicalTransportState {
    Ready,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TransportSessionCloseReason {
    RequestedDrain,
    RequestedClose,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransportSessionReceipt {
    pub generation: u64,
    pub reused: bool,
    pub physical_state: PhysicalTransportState,
    pub connection_age_ms: u64,
    pub ttl_remaining_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<TransportSessionCloseReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_acknowledged: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closure_evidence: Option<close::ClosureEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InvocationTerminationKind {
    Completed,
    UpstreamError,
    TransportError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationTimingReceipt {
    pub schema_version: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_ms: Option<u64>,
    pub upstream_ms: u64,
    pub termination_kind: InvocationTerminationKind,
}

pub(crate) fn transport_session_directive(
    input: &ProviderInvocationInput,
) -> Result<Option<TransportSessionDirective>> {
    let Some(value) = input.run_context.get(TRANSPORT_SESSION_CONTEXT_KEY) else {
        return Ok(None);
    };
    let directive: TransportSessionDirective = serde_json::from_value(value.clone())
        .context("physical transport session directive is invalid")?;
    directive.validate()?;
    if directive.physical_deadline_unix_ms <= unix_time_ms() {
        bail!("physical transport session invocation deadline has expired");
    }
    Ok(Some(directive))
}

pub(crate) fn ready_receipt(
    session: &ResponsesWebsocketSession,
    contract_generation: u64,
    now: Instant,
    reused: bool,
    policy: WebsocketLifecyclePolicy,
) -> TransportSessionReceipt {
    let age = now.saturating_duration_since(session.created_at);
    TransportSessionReceipt {
        generation: contract_generation,
        reused,
        physical_state: PhysicalTransportState::Ready,
        connection_age_ms: bounded_millis(age),
        ttl_remaining_ms: bounded_millis(policy.hard_max_age.saturating_sub(age)),
        close_reason: None,
        close_acknowledged: None,
        closure_evidence: None,
    }
}

pub(crate) fn attach_receipt(metadata: &mut Value, receipt: TransportSessionReceipt) -> Result<()> {
    let object = metadata
        .as_object_mut()
        .ok_or_else(|| anyhow!("provider_metadata must be an object"))?;
    object.insert(
        TRANSPORT_SESSION_RECEIPT_METADATA_KEY.to_string(),
        serde_json::to_value(receipt)?,
    );
    Ok(())
}

pub(crate) fn attach_invocation_timing_receipt(
    metadata: &mut Value,
    connect: Option<Duration>,
    upstream: Duration,
) -> Result<()> {
    let object = metadata
        .as_object_mut()
        .ok_or_else(|| anyhow!("provider_metadata must be an object"))?;
    object.insert(
        INVOCATION_TIMING_RECEIPT_METADATA_KEY.to_string(),
        serde_json::to_value(InvocationTimingReceipt {
            schema_version: INVOCATION_TIMING_SCHEMA_VERSION,
            connect_ms: connect.map(bounded_millis),
            upstream_ms: bounded_millis(upstream),
            termination_kind: InvocationTerminationKind::Completed,
        })?,
    );
    Ok(())
}

fn bounded_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis())
        .unwrap_or(u64::MAX)
        .min(MAX_CONNECTION_LIFETIME_MS)
}

fn unix_time_ms() -> i64 {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}

fn validate_opaque_id(field: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_OPAQUE_ID_BYTES {
        bail!("transport session {field} is outside the bounded contract");
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        bail!("transport session {field} must be an opaque URL-safe identifier");
    }
    Ok(())
}

// A failure carries only observed diagnostics, never a fabricated Ready receipt.
impl ProviderRuntimeError {
    pub fn failure_metadata(&self) -> Value {
        let mut metadata = Map::new();
        if let Some(details) = self.provider_details.as_ref().and_then(Value::as_object) {
            for key in [
                recovery::RECOVERY_RECEIPT_METADATA_KEY,
                INVOCATION_TIMING_RECEIPT_METADATA_KEY,
            ] {
                if let Some(value) = details.get(key) {
                    metadata.insert(key.to_string(), value.clone());
                }
            }
        }
        Value::Object(metadata)
    }
}

pub(crate) fn attach_failed_timing(
    error: anyhow::Error,
    connect: Option<Duration>,
    upstream: Duration,
) -> anyhow::Error {
    let Some(mut runtime_error) = error.downcast_ref::<ProviderRuntimeError>().cloned() else {
        return error;
    };
    let termination_kind = if matches!(
        runtime_error.kind,
        ProviderRuntimeErrorKind::ProviderTransportUnavailable
            | ProviderRuntimeErrorKind::EndpointUnreachable
    ) {
        InvocationTerminationKind::TransportError
    } else {
        InvocationTerminationKind::UpstreamError
    };
    let details = runtime_error
        .provider_details
        .get_or_insert_with(|| json!({}));
    if let Some(details) = details.as_object_mut() {
        details.insert(
            INVOCATION_TIMING_RECEIPT_METADATA_KEY.to_string(),
            serde_json::to_value(InvocationTimingReceipt {
                schema_version: INVOCATION_TIMING_SCHEMA_VERSION,
                connect_ms: connect.map(bounded_millis),
                upstream_ms: bounded_millis(upstream),
                termination_kind,
            })
            .expect("typed timing receipt must serialize"),
        );
    }
    anyhow::Error::new(runtime_error)
}
