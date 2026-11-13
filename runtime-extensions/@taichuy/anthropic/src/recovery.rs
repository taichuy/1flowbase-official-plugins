//! Attest one HTTP attempt; the host owns retry admission, budgets and backoff.
use super::*;
use extension_contracts::{
    CommitLevel, ProviderRecoveryDirective, ProviderRecoveryReceipt, RecoveryDisposition,
    RecoveryReason, RecoveryTransport, PROVIDER_RECOVERY_DIRECTIVE_CONTEXT_KEY,
    PROVIDER_RECOVERY_RECEIPT_METADATA_KEY,
};

#[derive(Default)]
pub(super) struct HttpRecovery {
    directive: Option<ProviderRecoveryDirective>,
    semantic: bool,
    terminal: bool,
}

impl HttpRecovery {
    pub(super) fn from_context(context: &BTreeMap<String, Value>) -> Result<Self> {
        let directive = context
            .get(PROVIDER_RECOVERY_DIRECTIVE_CONTEXT_KEY)
            .map(|value| serde_json::from_value::<ProviderRecoveryDirective>(value.clone()))
            .transpose()
            .context("invalid provider recovery directive")?;
        if let Some(directive) = &directive {
            directive.validate().map_err(anyhow::Error::msg)?;
        }
        Ok(Self {
            directive,
            ..Self::default()
        })
    }

    // Inspect source events as well as emitted events: tool inputs in a start
    // block and reasoning signatures are semantic even without a text delta.
    pub(super) fn observe_data(&mut self, data: &str) -> Result<()> {
        if data.is_empty() || data == "[DONE]" {
            return Ok(());
        }
        let payload: Value = serde_json::from_str(data)?;
        match payload["type"].as_str() {
            Some("content_block_start") => {
                let block = &payload["content_block"];
                self.semantic |= match block["type"].as_str() {
                    Some("tool_use") => true,
                    Some("text" | "thinking" | "redacted_thinking") => {
                        ["text", "thinking", "signature", "data"]
                            .iter()
                            .any(|field| {
                                block[*field]
                                    .as_str()
                                    .is_some_and(|value| !value.is_empty())
                            })
                    }
                    // An unknown future block must not become permission to replay.
                    _ => true,
                };
            }
            Some("content_block_delta") => {
                // Unknown content deltas are conservative semantic boundaries.
                self.semantic = true;
            }
            Some("message_stop") => self.terminal = true,
            Some("message_start" | "message_delta" | "content_block_stop" | "ping") => {}
            Some(_) => self.semantic = true,
            None => bail!("invalid Anthropic SSE event type"),
        }
        Ok(())
    }

    pub(super) fn disconnected(&self, original: anyhow::Error) -> anyhow::Error {
        let Some(directive) = &self.directive else {
            return original;
        };
        let terminal = self.semantic
            || self.terminal
            || directive.initial_commit_level != CommitLevel::LifecycleOnly;
        let receipt = ProviderRecoveryReceipt {
            attempt: 0,
            transport: RecoveryTransport::ProviderHttp,
            transport_epoch: directive.transport_epoch,
            socket_incarnation: None,
            commit_level: if terminal {
                CommitLevel::Terminal
            } else {
                CommitLevel::LifecycleOnly
            },
            disposition: if terminal {
                RecoveryDisposition::TerminalInterruption
            } else {
                RecoveryDisposition::LogicalInvocationRetry
            },
            reason: RecoveryReason::TransportDisconnected,
        };
        // Never emit an invalid or regressive receipt that could authorize replay.
        if receipt.validate_against(directive).is_err() {
            return original;
        }
        let mut error = original
            .downcast_ref::<ProviderRuntimeError>()
            .cloned()
            .unwrap_or_else(|| ProviderRuntimeError {
                kind: ProviderRuntimeErrorKind::EndpointUnreachable,
                message: original.to_string(),
                provider_summary: None,
                provider_details: None,
            });
        let original_wire = serde_json::to_value(&error).expect("runtime error serializes");
        let details = error.provider_details.get_or_insert_with(|| json!({}));
        let Some(details) = details.as_object_mut() else {
            return original;
        };
        if details.contains_key(PROVIDER_RECOVERY_RECEIPT_METADATA_KEY)
            || details.contains_key("1flowbase_provider_recovery_original_error")
        {
            return original;
        }
        details.insert(
            PROVIDER_RECOVERY_RECEIPT_METADATA_KEY.to_string(),
            serde_json::to_value(receipt).expect("typed recovery receipt serializes"),
        );
        details.insert(
            "1flowbase_provider_recovery_original_error".to_string(),
            original_wire,
        );
        error.into()
    }
}

#[cfg(test)]
#[path = "_tests/recovery.rs"]
mod tests;
