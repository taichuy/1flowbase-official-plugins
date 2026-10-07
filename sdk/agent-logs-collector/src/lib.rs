//! Standalone source scheduling, identity, durable checkpoints and canonical HTTP delivery.
mod collector;
mod config;
mod source;
pub use collector::{collect, CollectReport, SourceAdapter};
pub use config::{configure, Config};
pub use control_plane_contracts::ports::runtime::agent_logs::{
    AgentLogEvent, AgentLogEventKind, AgentLogUsage, AgentLogUsageBasis, AgentLogsBatch,
    AgentLogsReceipt, AGENT_LOGS_SCHEMA_VERSION,
};
pub use source::{atomic_write, hash, CheckpointLock, Position};

#[cfg(test)]
#[path = "_tests/collector.rs"]
mod tests;
