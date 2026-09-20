use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const RECOVERY_DIRECTIVE_CONTEXT_KEY: &str = "provider_recovery";
pub(crate) const RECOVERY_RECEIPT_METADATA_KEY: &str = "1flowbase_provider_recovery";
/// Typed details slot that keeps the failure which preceded a bounded recovery
/// attempt visible next to the final recovery outcome.
pub(crate) const RECOVERY_ORIGINAL_ERROR_METADATA_KEY: &str =
    "1flowbase_provider_recovery_original_error";
pub(crate) const STANDALONE_MAX_INNER_ATTEMPTS: u16 = 3;
const MAX_RECOVERY_INNER_ATTEMPTS: u16 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommitLevel {
    LifecycleOnly,
    SemanticCommitted,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryPolicyKind {
    SemanticMapped,
    NativeOpaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryBudget {
    pub max_inner_attempts: u16,
    pub absolute_deadline_unix_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RecoveryPolicy {
    SemanticMapped { budget: RecoveryBudget },
    NativeOpaque { budget: RecoveryBudget },
}

impl RecoveryPolicy {
    const fn kind(self) -> RecoveryPolicyKind {
        match self {
            Self::SemanticMapped { .. } => RecoveryPolicyKind::SemanticMapped,
            Self::NativeOpaque { .. } => RecoveryPolicyKind::NativeOpaque,
        }
    }

    const fn budget(self) -> RecoveryBudget {
        match self {
            Self::SemanticMapped { budget } | Self::NativeOpaque { budget } => budget,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CursorBinding {
    Durable,
    ConnectionBound {
        transport_epoch: u64,
        socket_incarnation: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CursorProvenance {
    pub binding: CursorBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderRecoveryDirective {
    pub policy: RecoveryPolicy,
    pub transport_epoch: u64,
    pub initial_commit_level: CommitLevel,
    #[serde(default)]
    pub cursor_provenance: Option<CursorProvenance>,
}

impl ProviderRecoveryDirective {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let budget = self.policy.budget();
        if self.transport_epoch == 0 {
            return Err("transport epoch must be positive".to_string());
        }
        if budget.max_inner_attempts == 0 || budget.max_inner_attempts > MAX_RECOVERY_INNER_ATTEMPTS
        {
            return Err(format!(
                "recovery max_inner_attempts must contain 1 through {MAX_RECOVERY_INNER_ATTEMPTS} attempts"
            ));
        }
        if budget.absolute_deadline_unix_ms <= 0 {
            return Err("recovery absolute_deadline_unix_ms must be positive".to_string());
        }
        if let Some(CursorProvenance {
            binding:
                CursorBinding::ConnectionBound {
                    transport_epoch,
                    socket_incarnation,
                },
        }) = self.cursor_provenance
        {
            if transport_epoch != self.transport_epoch {
                return Err("connection-bound cursor cannot cross a transport epoch".to_string());
            }
            if socket_incarnation == 0 {
                return Err("socket incarnation must be positive".to_string());
            }
        }
        Ok(())
    }

    pub(crate) fn constraints(&self) -> RecoveryConstraints {
        let budget = self.policy.budget();
        RecoveryConstraints {
            policy: self.policy.kind(),
            max_inner_attempts: budget.max_inner_attempts,
            absolute_deadline_unix_ms: Some(budget.absolute_deadline_unix_ms),
            initial_commit_level: self.initial_commit_level,
        }
    }
}

pub(crate) fn directive_from_run_context(
    run_context: &std::collections::BTreeMap<String, Value>,
) -> Result<Option<ProviderRecoveryDirective>, String> {
    run_context
        .get(RECOVERY_DIRECTIVE_CONTEXT_KEY)
        .map(|value| {
            let directive: ProviderRecoveryDirective = serde_json::from_value(value.clone())
                .map_err(|_| "provider recovery directive is invalid".to_string())?;
            directive.validate()?;
            Ok(directive)
        })
        .transpose()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryDisposition {
    SameEpochReconnect,
    PreCommitHttpFallback,
    OneFullContextRebuild,
    LogicalInvocationRetry,
    TerminalInterruption,
    SemanticTerminal,
}

impl RecoveryDisposition {
    /// A terminal disposition claims no reconnect and no resumption, so its
    /// receipt needs no socket incarnation to stay truthful.
    pub(crate) const fn is_terminal(self) -> bool {
        matches!(self, Self::TerminalInterruption | Self::SemanticTerminal)
    }
}

/// Bounded full-jitter backoff between provider-internal recovery attempts.
/// The attempt budget belongs to the AI Native directive; this only spaces the
/// attempts it already authorized and never extends the deadline.
const WEBSOCKET_INNER_RETRY_BASE_MS: u64 = 100;
const WEBSOCKET_INNER_RETRY_CAP_MS: u64 = 500;

pub(crate) fn inner_retry_delay_ms(attempt: u16, random_sample: u64) -> u64 {
    let exponent = u32::from(attempt).min(16);
    let upper_bound = WEBSOCKET_INNER_RETRY_BASE_MS
        .saturating_mul(1_u64 << exponent)
        .min(WEBSOCKET_INNER_RETRY_CAP_MS);
    random_sample % (upper_bound + 1)
}

/// Jitter source for [`inner_retry_delay_ms`]. Nanosecond wall-clock noise is
/// sufficient here: the value only de-correlates co-scheduled reconnects.
pub(crate) fn inner_retry_random_sample() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    u64::from(nanos)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryTransport {
    AiNativeWebSocket,
    ProviderHttp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Closed D1 wire taxonomy; not every reason is Provider-originated in D2.
pub(crate) enum RecoveryReason {
    TransportDisconnected,
    TransportRejected,
    ProtocolError,
    DeadlineExceeded,
    FencingRejected,
    BudgetExhausted,
    SemanticCompleted,
    SemanticFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderRecoveryReceipt {
    pub attempt: u16,
    pub transport: RecoveryTransport,
    pub transport_epoch: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket_incarnation: Option<u64>,
    pub commit_level: CommitLevel,
    pub disposition: RecoveryDisposition,
    pub reason: RecoveryReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecoveryTransition {
    pub attempt: u16,
    pub commit_level: CommitLevel,
    pub disposition: RecoveryDisposition,
    pub reason: RecoveryReason,
}

pub(crate) fn attach_recovery_receipt(
    metadata: &mut Value,
    receipt: ProviderRecoveryReceipt,
) -> Result<(), String> {
    let object = metadata
        .as_object_mut()
        .ok_or_else(|| "provider_metadata must be an object for a recovery receipt".to_string())?;
    object.insert(
        RECOVERY_RECEIPT_METADATA_KEY.to_string(),
        serde_json::to_value(receipt).expect("ProviderRecoveryReceipt must always serialize"),
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoverySignal {
    TransportDisconnected,
    TransportRejected,
    PreviousResponseUnavailable,
    ProxyFailed,
    ProtocolError,
    SemanticTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorState {
    None,
    OpaqueUnowned,
    ConnectionBound {
        same_epoch: bool,
        owner_available: bool,
        turn_state_available: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecoveryFacts {
    pub signal: RecoverySignal,
    pub cursor: CursorState,
    pub full_context_available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecoveryConstraints {
    pub policy: RecoveryPolicyKind,
    pub max_inner_attempts: u16,
    pub absolute_deadline_unix_ms: Option<i64>,
    pub initial_commit_level: CommitLevel,
}

impl RecoveryConstraints {
    pub(crate) const fn standalone(policy: RecoveryPolicyKind) -> Self {
        Self {
            policy,
            max_inner_attempts: STANDALONE_MAX_INNER_ATTEMPTS,
            absolute_deadline_unix_ms: None,
            initial_commit_level: CommitLevel::LifecycleOnly,
        }
    }
}

/// The only Provider recovery transition table. Managed and standalone calls
/// differ only in the constraints supplied to this state machine.
#[derive(Debug, Clone)]
pub(crate) struct RecoveryFsm {
    constraints: RecoveryConstraints,
    attempt: u16,
    commit_level: CommitLevel,
    full_context_rebuild_used: bool,
}

impl RecoveryFsm {
    pub(crate) const fn new(constraints: RecoveryConstraints) -> Self {
        Self {
            constraints,
            attempt: 0,
            commit_level: constraints.initial_commit_level,
            full_context_rebuild_used: false,
        }
    }

    pub(crate) fn observe_semantic_event(&mut self) {
        if self.commit_level == CommitLevel::LifecycleOnly {
            self.commit_level = CommitLevel::SemanticCommitted;
        }
    }

    pub(crate) fn observe_terminal(&mut self) {
        self.commit_level = CommitLevel::Terminal;
    }

    pub(crate) fn decide(&mut self, facts: RecoveryFacts) -> RecoveryDisposition {
        if facts.signal == RecoverySignal::SemanticTerminal {
            self.observe_terminal();
            return RecoveryDisposition::SemanticTerminal;
        }
        if self.commit_level != CommitLevel::LifecycleOnly {
            self.observe_terminal();
            return RecoveryDisposition::TerminalInterruption;
        }
        if self
            .constraints
            .absolute_deadline_unix_ms
            .is_some_and(|deadline| deadline <= unix_time_ms())
            || self.attempt >= self.constraints.max_inner_attempts
        {
            self.observe_terminal();
            return RecoveryDisposition::TerminalInterruption;
        }

        let disposition = match facts.signal {
            RecoverySignal::PreviousResponseUnavailable
                if self.constraints.policy == RecoveryPolicyKind::SemanticMapped
                    && facts.full_context_available
                    && !self.full_context_rebuild_used =>
            {
                self.full_context_rebuild_used = true;
                RecoveryDisposition::OneFullContextRebuild
            }
            RecoverySignal::PreviousResponseUnavailable => {
                RecoveryDisposition::TerminalInterruption
            }
            RecoverySignal::TransportDisconnected => match facts.cursor {
                CursorState::ConnectionBound {
                    same_epoch: true,
                    owner_available: true,
                    turn_state_available: true,
                } => RecoveryDisposition::SameEpochReconnect,
                CursorState::ConnectionBound { .. } => RecoveryDisposition::TerminalInterruption,
                CursorState::OpaqueUnowned
                    if self.constraints.policy == RecoveryPolicyKind::SemanticMapped =>
                {
                    RecoveryDisposition::PreCommitHttpFallback
                }
                CursorState::OpaqueUnowned => RecoveryDisposition::TerminalInterruption,
                CursorState::None
                    if self.constraints.policy == RecoveryPolicyKind::SemanticMapped =>
                {
                    RecoveryDisposition::PreCommitHttpFallback
                }
                CursorState::None => RecoveryDisposition::TerminalInterruption,
            },
            RecoverySignal::ProxyFailed => match facts.cursor {
                CursorState::ConnectionBound {
                    same_epoch: true,
                    owner_available: true,
                    ..
                } => RecoveryDisposition::SameEpochReconnect,
                _ => RecoveryDisposition::TerminalInterruption,
            },
            RecoverySignal::TransportRejected | RecoverySignal::ProtocolError => {
                if self.constraints.policy == RecoveryPolicyKind::SemanticMapped
                    && !matches!(facts.cursor, CursorState::ConnectionBound { .. })
                {
                    RecoveryDisposition::PreCommitHttpFallback
                } else {
                    RecoveryDisposition::TerminalInterruption
                }
            }
            RecoverySignal::SemanticTerminal => unreachable!("handled above"),
        };
        if disposition == RecoveryDisposition::TerminalInterruption {
            self.observe_terminal();
        } else {
            self.attempt = self.attempt.saturating_add(1);
        }
        disposition
    }

    pub(crate) fn decide_transition(&mut self, facts: RecoveryFacts) -> RecoveryTransition {
        let attempt = self.attempt;
        let deadline_expired = self
            .constraints
            .absolute_deadline_unix_ms
            .is_some_and(|deadline| deadline <= unix_time_ms());
        let budget_exhausted = self.attempt >= self.constraints.max_inner_attempts;
        let committed_before = self.commit_level != CommitLevel::LifecycleOnly;
        let disposition = self.decide(facts);
        let reason = if deadline_expired {
            RecoveryReason::DeadlineExceeded
        } else if budget_exhausted {
            RecoveryReason::BudgetExhausted
        } else if committed_before || facts.signal == RecoverySignal::SemanticTerminal {
            RecoveryReason::SemanticFailed
        } else {
            match facts.signal {
                RecoverySignal::TransportDisconnected
                | RecoverySignal::PreviousResponseUnavailable
                | RecoverySignal::ProxyFailed => RecoveryReason::TransportDisconnected,
                RecoverySignal::TransportRejected => RecoveryReason::TransportRejected,
                RecoverySignal::ProtocolError => RecoveryReason::ProtocolError,
                RecoverySignal::SemanticTerminal => RecoveryReason::SemanticFailed,
            }
        };
        RecoveryTransition {
            attempt,
            commit_level: self.commit_level,
            disposition,
            reason,
        }
    }
}

fn unix_time_ms() -> i64 {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fsm(policy: RecoveryPolicyKind) -> RecoveryFsm {
        RecoveryFsm::new(RecoveryConstraints::standalone(policy))
    }

    #[test]
    fn managed_and_standalone_constraints_use_the_same_transition_table() {
        let facts = RecoveryFacts {
            signal: RecoverySignal::TransportRejected,
            cursor: CursorState::None,
            full_context_available: false,
        };
        let mut standalone = fsm(RecoveryPolicyKind::SemanticMapped);
        let mut managed = RecoveryFsm::new(RecoveryConstraints {
            policy: RecoveryPolicyKind::SemanticMapped,
            max_inner_attempts: 2,
            absolute_deadline_unix_ms: None,
            initial_commit_level: CommitLevel::LifecycleOnly,
        });
        assert_eq!(standalone.decide(facts), managed.decide(facts));
        assert_eq!(
            managed.decide(facts),
            RecoveryDisposition::PreCommitHttpFallback
        );
    }

    #[test]
    fn lifecycle_does_not_commit_but_semantic_output_blocks_replay() {
        let facts = RecoveryFacts {
            signal: RecoverySignal::TransportDisconnected,
            cursor: CursorState::None,
            full_context_available: false,
        };
        let mut machine = fsm(RecoveryPolicyKind::SemanticMapped);
        assert_eq!(
            machine.decide(facts),
            RecoveryDisposition::PreCommitHttpFallback
        );
        machine.observe_semantic_event();
        assert_eq!(
            machine.decide(facts),
            RecoveryDisposition::TerminalInterruption
        );
    }

    #[test]
    fn connection_bound_cursor_only_reconnects_with_same_epoch_owner_and_turn_state() {
        let mut machine = fsm(RecoveryPolicyKind::SemanticMapped);
        let mut facts = RecoveryFacts {
            signal: RecoverySignal::TransportDisconnected,
            cursor: CursorState::ConnectionBound {
                same_epoch: true,
                owner_available: true,
                turn_state_available: true,
            },
            full_context_available: true,
        };
        assert_eq!(
            machine.decide(facts),
            RecoveryDisposition::SameEpochReconnect
        );
        facts.cursor = CursorState::ConnectionBound {
            same_epoch: false,
            owner_available: true,
            turn_state_available: true,
        };
        assert_eq!(
            machine.decide(facts),
            RecoveryDisposition::TerminalInterruption
        );
    }

    #[test]
    fn unavailable_previous_response_rebuilds_full_context_once() {
        let facts = RecoveryFacts {
            signal: RecoverySignal::PreviousResponseUnavailable,
            cursor: CursorState::ConnectionBound {
                same_epoch: true,
                owner_available: true,
                turn_state_available: true,
            },
            full_context_available: true,
        };
        let mut machine = fsm(RecoveryPolicyKind::SemanticMapped);
        assert_eq!(
            machine.decide(facts),
            RecoveryDisposition::OneFullContextRebuild
        );
        assert_eq!(
            machine.decide(facts),
            RecoveryDisposition::TerminalInterruption
        );
    }

    #[test]
    fn native_opaque_never_crosses_to_http() {
        let mut machine = fsm(RecoveryPolicyKind::NativeOpaque);
        assert_eq!(
            machine.decide(RecoveryFacts {
                signal: RecoverySignal::TransportRejected,
                cursor: CursorState::None,
                full_context_available: false,
            }),
            RecoveryDisposition::TerminalInterruption
        );
    }

    #[test]
    fn disconnected_without_cursor_respects_native_and_mapped_policy() {
        for deadline in [None, Some(4_102_444_800_000)] {
            for (policy, expected, commit_level, consumed) in [
                (
                    RecoveryPolicyKind::NativeOpaque,
                    RecoveryDisposition::TerminalInterruption,
                    CommitLevel::Terminal,
                    0,
                ),
                (
                    RecoveryPolicyKind::SemanticMapped,
                    RecoveryDisposition::PreCommitHttpFallback,
                    CommitLevel::LifecycleOnly,
                    1,
                ),
            ] {
                let mut machine = RecoveryFsm::new(RecoveryConstraints {
                    policy,
                    max_inner_attempts: 3,
                    absolute_deadline_unix_ms: deadline,
                    initial_commit_level: CommitLevel::LifecycleOnly,
                });
                let transition = machine.decide_transition(RecoveryFacts {
                    signal: RecoverySignal::TransportDisconnected,
                    cursor: CursorState::None,
                    full_context_available: false,
                });
                assert_eq!(
                    transition.disposition, expected,
                    "policy={policy:?}, deadline={deadline:?}"
                );
                assert_eq!(transition.commit_level, commit_level);
                assert_eq!(transition.reason, RecoveryReason::TransportDisconnected);
                assert_eq!(transition.attempt, 0);
                assert_eq!(machine.attempt, consumed);
                assert_eq!(machine.constraints.absolute_deadline_unix_ms, deadline);
            }
        }
    }

    #[test]
    fn managed_directive_consumes_the_d1_json_wire_without_provider_decisions() {
        let value = serde_json::json!({
            "policy": {
                "type": "semantic_mapped",
                "budget": {
                    "max_inner_attempts": 3,
                    "absolute_deadline_unix_ms": 4_102_444_800_000_i64
                }
            },
            "transport_epoch": 17,
            "initial_commit_level": "lifecycle_only",
            "cursor_provenance": {
                "binding": {
                    "type": "connection_bound",
                    "transport_epoch": 17,
                    "socket_incarnation": 4
                }
            }
        });
        let mut context = std::collections::BTreeMap::new();
        context.insert(RECOVERY_DIRECTIVE_CONTEXT_KEY.to_string(), value);
        let directive = directive_from_run_context(&context).unwrap().unwrap();
        assert_eq!(directive.transport_epoch, 17);
        assert_eq!(directive.constraints().max_inner_attempts, 3);
    }

    #[test]
    fn recovery_receipt_contains_only_closed_typed_state() {
        let mut metadata = serde_json::json!({});
        attach_recovery_receipt(
            &mut metadata,
            ProviderRecoveryReceipt {
                attempt: 1,
                transport: RecoveryTransport::AiNativeWebSocket,
                transport_epoch: 17,
                socket_incarnation: Some(5),
                commit_level: CommitLevel::LifecycleOnly,
                disposition: RecoveryDisposition::SameEpochReconnect,
                reason: RecoveryReason::TransportDisconnected,
            },
        )
        .unwrap();
        let encoded = metadata.to_string();
        assert!(encoded.contains("1flowbase_provider_recovery"));
        for forbidden in ["cursor", "turn_state", "prompt", "response_id"] {
            assert!(!encoded.contains(forbidden));
        }
    }

    #[test]
    fn expired_managed_deadline_cannot_multiply_inner_attempts() {
        let mut machine = RecoveryFsm::new(RecoveryConstraints {
            policy: RecoveryPolicyKind::SemanticMapped,
            max_inner_attempts: 16,
            absolute_deadline_unix_ms: Some(1),
            initial_commit_level: CommitLevel::LifecycleOnly,
        });
        let transition = machine.decide_transition(RecoveryFacts {
            signal: RecoverySignal::TransportDisconnected,
            cursor: CursorState::None,
            full_context_available: false,
        });
        assert_eq!(
            transition.disposition,
            RecoveryDisposition::TerminalInterruption
        );
        assert_eq!(transition.reason, RecoveryReason::DeadlineExceeded);
        assert_eq!(transition.commit_level, CommitLevel::Terminal);
    }

    #[test]
    fn d4_managed_and_standalone_execute_the_same_mapped_transition_matrix() {
        let cases = [
            (
                RecoveryFacts {
                    signal: RecoverySignal::TransportRejected,
                    cursor: CursorState::None,
                    full_context_available: false,
                },
                RecoveryDisposition::PreCommitHttpFallback,
            ),
            (
                RecoveryFacts {
                    signal: RecoverySignal::TransportDisconnected,
                    cursor: CursorState::ConnectionBound {
                        same_epoch: true,
                        owner_available: true,
                        turn_state_available: true,
                    },
                    full_context_available: false,
                },
                RecoveryDisposition::SameEpochReconnect,
            ),
            (
                RecoveryFacts {
                    signal: RecoverySignal::PreviousResponseUnavailable,
                    cursor: CursorState::ConnectionBound {
                        same_epoch: true,
                        owner_available: true,
                        turn_state_available: true,
                    },
                    full_context_available: true,
                },
                RecoveryDisposition::OneFullContextRebuild,
            ),
        ];

        for (facts, expected) in cases {
            let mut standalone = fsm(RecoveryPolicyKind::SemanticMapped);
            let mut managed = RecoveryFsm::new(RecoveryConstraints {
                policy: RecoveryPolicyKind::SemanticMapped,
                max_inner_attempts: STANDALONE_MAX_INNER_ATTEMPTS,
                absolute_deadline_unix_ms: None,
                initial_commit_level: CommitLevel::LifecycleOnly,
            });
            assert_eq!(standalone.decide(facts), expected);
            assert_eq!(managed.decide(facts), expected);
        }
    }

    #[test]
    fn d4_connection_bound_cursor_never_crosses_epoch_or_http() {
        for cursor in [
            CursorState::ConnectionBound {
                same_epoch: false,
                owner_available: true,
                turn_state_available: true,
            },
            CursorState::ConnectionBound {
                same_epoch: true,
                owner_available: false,
                turn_state_available: true,
            },
            CursorState::ConnectionBound {
                same_epoch: true,
                owner_available: true,
                turn_state_available: false,
            },
        ] {
            let mut machine = fsm(RecoveryPolicyKind::SemanticMapped);
            let disposition = machine.decide(RecoveryFacts {
                signal: RecoverySignal::TransportDisconnected,
                cursor,
                full_context_available: true,
            });
            assert_eq!(disposition, RecoveryDisposition::TerminalInterruption);
            assert_ne!(disposition, RecoveryDisposition::PreCommitHttpFallback);
        }
    }

    #[test]
    fn d4_unavailable_cursor_rebuild_is_single_use_even_with_remaining_budget() {
        let mut machine = RecoveryFsm::new(RecoveryConstraints {
            policy: RecoveryPolicyKind::SemanticMapped,
            max_inner_attempts: 8,
            absolute_deadline_unix_ms: None,
            initial_commit_level: CommitLevel::LifecycleOnly,
        });
        let unavailable = RecoveryFacts {
            signal: RecoverySignal::PreviousResponseUnavailable,
            cursor: CursorState::ConnectionBound {
                same_epoch: true,
                owner_available: true,
                turn_state_available: true,
            },
            full_context_available: true,
        };
        assert_eq!(
            machine.decide(unavailable),
            RecoveryDisposition::OneFullContextRebuild
        );
        assert_eq!(
            machine.decide(unavailable),
            RecoveryDisposition::TerminalInterruption
        );
    }

    #[test]
    fn d4_native_opaque_and_post_commit_paths_never_select_provider_http() {
        let retryable = RecoveryFacts {
            signal: RecoverySignal::TransportRejected,
            cursor: CursorState::None,
            full_context_available: true,
        };
        let mut native = fsm(RecoveryPolicyKind::NativeOpaque);
        assert_eq!(
            native.decide(retryable),
            RecoveryDisposition::TerminalInterruption
        );

        let mut mapped = fsm(RecoveryPolicyKind::SemanticMapped);
        mapped.observe_semantic_event();
        assert_eq!(
            mapped.decide(retryable),
            RecoveryDisposition::TerminalInterruption
        );

        let mut terminal = fsm(RecoveryPolicyKind::SemanticMapped);
        assert_eq!(
            terminal.decide(RecoveryFacts {
                signal: RecoverySignal::SemanticTerminal,
                cursor: CursorState::None,
                full_context_available: true,
            }),
            RecoveryDisposition::SemanticTerminal
        );
    }

    #[test]
    fn d4_attempt_cap_is_absolute_and_never_resets_after_recoverable_transitions() {        let mut machine = RecoveryFsm::new(RecoveryConstraints {
            policy: RecoveryPolicyKind::SemanticMapped,
            max_inner_attempts: 2,
            absolute_deadline_unix_ms: None,
            initial_commit_level: CommitLevel::LifecycleOnly,
        });
        let reconnect = RecoveryFacts {
            signal: RecoverySignal::TransportDisconnected,
            cursor: CursorState::ConnectionBound {
                same_epoch: true,
                owner_available: true,
                turn_state_available: true,
            },
            full_context_available: false,
        };
        assert_eq!(
            machine.decide_transition(reconnect).disposition,
            RecoveryDisposition::SameEpochReconnect
        );
        assert_eq!(
            machine.decide_transition(reconnect).disposition,
            RecoveryDisposition::SameEpochReconnect
        );
        let exhausted = machine.decide_transition(reconnect);
        assert_eq!(
            exhausted.disposition,
            RecoveryDisposition::TerminalInterruption
        );
        assert_eq!(exhausted.reason, RecoveryReason::BudgetExhausted);
        assert_eq!(exhausted.commit_level, CommitLevel::Terminal);
    }

    #[test]
    fn inner_retry_backoff_is_bounded_exponential_and_full_jitter() {
        // Full jitter: the sample selects within [0, upper_bound(attempt)].
        assert_eq!(inner_retry_delay_ms(0, 0), 0);
        assert_eq!(inner_retry_delay_ms(0, 100), 100);
        assert_eq!(inner_retry_delay_ms(0, 101), 0);
        assert_eq!(inner_retry_delay_ms(1, 200), 200);
        assert_eq!(inner_retry_delay_ms(1, 201), 0);
        // The cap holds for arbitrarily large attempt counters and samples.
        assert_eq!(inner_retry_delay_ms(u16::MAX, u64::MAX), u64::MAX % 501);
        assert!(inner_retry_delay_ms(u16::MAX, u64::MAX) <= WEBSOCKET_INNER_RETRY_CAP_MS);
        assert!(inner_retry_delay_ms(3, u64::MAX) <= WEBSOCKET_INNER_RETRY_CAP_MS);
    }

    #[test]
    fn terminal_dispositions_are_the_only_ones_needing_no_socket_incarnation() {
        assert!(RecoveryDisposition::TerminalInterruption.is_terminal());
        assert!(RecoveryDisposition::SemanticTerminal.is_terminal());
        for recoverable in [
            RecoveryDisposition::SameEpochReconnect,
            RecoveryDisposition::OneFullContextRebuild,
            RecoveryDisposition::LogicalInvocationRetry,
            RecoveryDisposition::PreCommitHttpFallback,
        ] {
            assert!(!recoverable.is_terminal(), "{recoverable:?}");
        }
    }
}
