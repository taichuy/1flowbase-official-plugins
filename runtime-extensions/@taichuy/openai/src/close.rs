use super::*;

const RETENTION: Duration = Duration::from_secs(60);
const CAPACITY: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub(crate) struct CloseIdentity {
    pub logical_session_id: String,
    pub generation: u64,
    pub worker_incarnation: u64,
}
impl CloseIdentity {
    pub fn command(command: &TransportSessionCommand) -> Option<Self> {
        Some(Self {
            logical_session_id: command.logical_session_id.clone(),
            generation: command.generation,
            worker_incarnation: command.worker_incarnation?,
        })
    }
    pub fn directive(directive: &TransportSessionDirective) -> Option<Self> {
        Some(Self {
            logical_session_id: directive.logical_session_id.clone(),
            generation: directive.generation,
            worker_incarnation: directive.worker_incarnation?,
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NoAckReason {
    Timeout,
    TransportError,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ClosureEvidence {
    source: &'static str,
    identity: CloseIdentity,
    local_released: bool,
    peer_close_acknowledged: Option<bool>,
    no_ack_reason: Option<NoAckReason>,
}
#[derive(Default)]
pub(crate) struct CloseLedger {
    entries: HashMap<CloseIdentity, Entry>,
}
struct Entry {
    active: bool,
    had_resource: bool,
    no_ack_reason: Option<NoAckReason>,
    released_at: Instant,
    age: Duration,
    completed: Option<TransportSessionReceipt>,
}
impl CloseLedger {
    pub fn abandon_active(&mut self, now: Instant) {
        for entry in self.entries.values_mut() {
            if entry.active {
                entry.active = false;
                entry.released_at = now;
                entry
                    .no_ack_reason
                    .get_or_insert(NoAckReason::TransportError);
            }
        }
    }

    pub fn prune(&mut self, now: Instant) {
        self.entries.retain(|_, entry| {
            entry.active || now.saturating_duration_since(entry.released_at) < RETENTION
        });
    }
    /// Reserve evidence capacity before attempting a handshake. No active evidence is evicted.
    pub fn reserve(&mut self, identity: CloseIdentity, now: Instant) -> Result<()> {
        self.prune(now);
        if let Some(entry) = self.entries.get_mut(&identity) {
            if entry.completed.is_some() {
                bail!("transport generation is already closed");
            }
            entry.released_at = now;
            return Ok(());
        }
        if self.entries.len() >= CAPACITY {
            bail!("transport closure evidence capacity reached");
        }
        self.entries.insert(
            identity,
            Entry {
                active: false,
                had_resource: false,
                no_ack_reason: None,
                released_at: now,
                age: Duration::ZERO,
                completed: None,
            },
        );
        Ok(())
    }
    pub fn activated(&mut self, identity: &CloseIdentity) {
        let entry = self
            .entries
            .get_mut(identity)
            .expect("connection admission reserved evidence");
        entry.active = true;
        entry.had_resource = true;
    }
    pub fn released(
        &mut self,
        identity: &CloseIdentity,
        reason: Option<NoAckReason>,
        age: Duration,
        now: Instant,
    ) {
        let entry = self
            .entries
            .get_mut(identity)
            .expect("active evidence cannot be evicted");
        entry.active = false;
        entry.age = entry.age.max(age);
        entry.released_at = now;
        // Every socket incarnation in this generation contributes to the aggregate.
        // A later successful close must not erase an earlier missing ACK.
        if entry.no_ack_reason.is_none() {
            entry.no_ack_reason = reason;
        }
    }
    pub fn completed(&self, identity: &CloseIdentity) -> Option<TransportSessionReceipt> {
        self.entries.get(identity)?.completed.clone()
    }
    pub fn complete(
        &mut self,
        identity: &CloseIdentity,
        command: &TransportSessionCommand,
        now: Instant,
    ) -> Option<TransportSessionReceipt> {
        self.complete_with_reason(
            identity,
            match command.action {
                TransportSessionAction::Drain => TransportSessionCloseReason::RequestedDrain,
                TransportSessionAction::Close => TransportSessionCloseReason::RequestedClose,
            },
            now,
        )
    }

    pub fn complete_failure(
        &mut self,
        identity: &CloseIdentity,
        now: Instant,
    ) -> Option<TransportSessionReceipt> {
        self.complete_with_reason(identity, TransportSessionCloseReason::TransportFault, now)
    }

    fn complete_with_reason(
        &mut self,
        identity: &CloseIdentity,
        close_reason: TransportSessionCloseReason,
        now: Instant,
    ) -> Option<TransportSessionReceipt> {
        let entry = self.entries.get_mut(identity)?;
        if entry.active {
            return None;
        }
        if let Some(receipt) = &entry.completed {
            return Some(receipt.clone());
        }
        let reason = if entry.had_resource {
            entry.no_ack_reason
        } else {
            Some(NoAckReason::Unknown)
        };
        let ack = if !entry.had_resource {
            None
        } else {
            Some(reason.is_none())
        };
        let receipt = TransportSessionReceipt {
            generation: identity.generation,
            reused: entry.had_resource,
            physical_state: PhysicalTransportState::Closed,
            connection_age_ms: u64::try_from(entry.age.as_millis())
                .unwrap_or(u64::MAX)
                .min(86_400_000),
            ttl_remaining_ms: 0,
            close_reason: Some(close_reason),
            close_acknowledged: ack,
            closure_evidence: Some(ClosureEvidence {
                source: "provider_local_release",
                identity: identity.clone(),
                local_released: true,
                peer_close_acknowledged: ack,
                no_ack_reason: reason,
            }),
        };
        entry.released_at = now;
        entry.completed = Some(receipt.clone());
        Some(receipt)
    }
}
impl OpenAiProviderRuntime {
    /// Only the outer invocation outcome closes the generation. Individual socket
    /// failures inside the retry loop must remain eligible for same-epoch reconnect.
    pub(crate) fn attach_final_closure(
        &mut self,
        identity: Option<&CloseIdentity>,
        error: anyhow::Error,
    ) -> anyhow::Error {
        let Some(identity) = identity else {
            return error;
        };
        if self.close_worker_incarnation != Some(identity.worker_incarnation) {
            return error;
        }
        let Some(mut runtime_error) = error.downcast_ref::<ProviderRuntimeError>().cloned() else {
            return error;
        };
        let Some(receipt) = self
            .close_ledger
            .complete_failure(identity, self.websocket_clock.now())
        else {
            return error;
        };
        let Ok(value) = serde_json::to_value(receipt) else {
            return error;
        };
        let details = runtime_error
            .provider_details
            .get_or_insert_with(|| json!({}));
        let Some(details) = details.as_object_mut() else {
            return error;
        };
        details.insert(TRANSPORT_SESSION_RECEIPT_METADATA_KEY.into(), value);
        anyhow::Error::new(runtime_error)
    }
}

pub(crate) fn missing_receipt(command: &TransportSessionCommand) -> TransportSessionReceipt {
    TransportSessionReceipt {
        generation: command.generation,
        reused: false,
        physical_state: PhysicalTransportState::Faulted,
        connection_age_ms: 0,
        ttl_remaining_ms: 0,
        close_reason: Some(TransportSessionCloseReason::RequestedClose),
        close_acknowledged: None,
        closure_evidence: None,
    }
}
pub(crate) fn unix_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

/// The timeout covers send, flush, reading ACK, and responding to pings. The
/// session is owned by the timeout future, so every return first drops the socket.
pub(crate) async fn release_socket(
    mut session: ResponsesWebsocketSession,
    timeout: Duration,
) -> Option<NoAckReason> {
    session.state = WebsocketConnectionState::Closing;
    session.stream.close(timeout).await
}

pub(super) async fn release_stream<S>(mut stream: S, timeout: Duration) -> Option<NoAckReason>
where
    S: futures_util::Sink<Message>
        + futures_util::Stream<
            Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>,
        > + Unpin,
{
    let result = tokio::time::timeout(timeout, async move {
        stream
            .send(Message::Close(None))
            .await
            .map_err(|_| NoAckReason::TransportError)?;
        stream
            .flush()
            .await
            .map_err(|_| NoAckReason::TransportError)?;
        while let Some(message) = stream.next().await {
            match message {
                Ok(Message::Close(_)) => return Ok(()),
                Ok(Message::Ping(payload)) => stream
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|_| NoAckReason::TransportError)?,
                Ok(_) => {}
                Err(_) => return Err(NoAckReason::TransportError),
            }
        }
        Err(NoAckReason::Unknown)
    })
    .await;
    match result {
        Ok(Ok(())) => None,
        Ok(Err(reason)) => Some(reason),
        Err(_) => Some(NoAckReason::Timeout),
    }
}

#[cfg(test)]
#[path = "_tests/close.rs"]
mod tests;
