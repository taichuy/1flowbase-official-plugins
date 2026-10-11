//! Native Claude Code JSONL facts. Transport and checkpointing belong to the SDK.
use agent_logs_collector::{
    AgentLogEvent, AgentLogEventKind as Kind, AgentLogUsage, AgentLogUsageBasis, Position,
    SourceAdapter,
};
use anyhow::{ensure, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub mod hooks;
pub struct ClaudeAdapter {
    pub client_home: PathBuf,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaudeContext {
    session: String,
    turn: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}
fn string(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}
fn text(v: &Value) -> Option<String> {
    if v.is_null() {
        None
    } else if let Some(s) = v.as_str() {
        Some(s.to_owned())
    } else {
        Some(v.to_string())
    }
}
/// Only a genuine user prompt owns a turn. Tool results and injected/meta/compact
/// context have user roles too, but must retain the current prompt's attribution.
pub fn prompt_uuid(row: &Value) -> Option<String> {
    if row["type"] != "user" || row["isMeta"] == true || row["isCompactSummary"] == true {
        return None;
    }
    let content = &row["message"]["content"];
    if content
        .as_array()
        .is_some_and(|blocks| blocks.iter().any(|b| b["type"] == "tool_result"))
    {
        return None;
    }
    if content.is_string()
        || content.as_array().is_some_and(|blocks| {
            blocks
                .iter()
                .any(|b| b["type"] == "text" || b["type"] == "image")
        })
    {
        string(&row["uuid"])
    } else {
        None
    }
}
fn base(row: &Value, context: &ClaudeContext, timestamp: String) -> AgentLogEvent {
    AgentLogEvent {
        event_id: String::new(),
        source_session_id: context.session.clone(),
        source_task_id: context.turn.clone().unwrap_or_default(),
        parent_source_task_id: None,
        sequence: 0,
        occurred_at: timestamp,
        kind: Kind::Context,
        content: None,
        phase: None,
        name: None,
        call_id: None,
        model_id: context.model.clone(),
        reasoning_effort: context.effort.clone(),
        provider_code: None,
        usage: None,
        inherited: row["isCompactSummary"] == true,
        raw: row.clone(),
    }
}
fn count(v: &Value, key: &str) -> Option<i64> {
    v[key]
        .as_i64()
        .filter(|n| *n >= 0 && *n <= 9_007_199_254_740_991)
}
impl SourceAdapter for ClaudeAdapter {
    type Context = ClaudeContext;
    fn source_client(&self) -> &'static str {
        "claude_code"
    }
    fn roots(&self, source: &Path) -> Vec<PathBuf> {
        let has_native_entry = |name: &str, directory: bool| {
            std::fs::symlink_metadata(source.join(name)).is_ok_and(|meta| {
                !meta.file_type().is_symlink()
                    && if directory {
                        meta.is_dir()
                    } else {
                        meta.is_file()
                    }
            })
        };
        if source == self.client_home
            || has_native_entry("projects", true)
            || has_native_entry("settings.json", false)
        {
            vec![source.join("projects")]
        } else {
            vec![source.to_owned()]
        }
    }
    fn create_context(&self, first: &Value) -> Result<ClaudeContext> {
        let session = string(&first["sessionId"]);
        // Claude can precede its first conversation row with a file snapshot.
        ensure!(
            session.is_some()
                || matches!(
                    first["type"].as_str(),
                    Some("file-history-snapshot" | "queue-operation")
                ),
            "Not a native Claude transcript"
        );
        Ok(ClaudeContext {
            session: session.unwrap_or_default(),
            turn: None,
            model: None,
            effort: None,
        })
    }
    fn convert(
        &self,
        position: &Position,
        context: &mut ClaudeContext,
    ) -> Result<Vec<AgentLogEvent>> {
        let row = &position.line;
        let kind = row["type"].as_str().unwrap_or("");
        if let Some(session) = string(&row["sessionId"]) {
            ensure!(
                context.session.is_empty() || context.session == session,
                "Claude transcript session identity changed"
            );
            context.session = session;
        }
        if let Some(turn) = prompt_uuid(row) {
            if context.turn.as_ref() != Some(&turn) {
                context.model = None;
                context.effort = None;
            }
            context.turn = Some(turn);
        }
        if let Some(effort) =
            string(&row["effort"]["level"]).or_else(|| string(&row["reasoningEffort"]))
        {
            context.effort = Some(effort);
        }
        if let Some(model) = string(&row["message"]["model"]) {
            context.model = Some(model);
        }
        // Unknown metadata without a source timestamp is not fabricated into an event.
        let Some(timestamp) = string(&row["timestamp"]) else {
            return Ok(vec![]);
        };
        time::OffsetDateTime::parse(&timestamp, &time::format_description::well_known::Rfc3339)
            .map_err(|_| anyhow::anyhow!("Claude source timestamp is invalid"))?;
        if context.session.is_empty() {
            return Ok(vec![]);
        }
        let event = base(row, context, timestamp);
        let mut facts = Vec::new();
        if matches!(kind, "user" | "assistant") {
            let message = &row["message"];
            let role = if kind == "user" {
                Kind::User
            } else {
                Kind::Assistant
            };
            match &message["content"] {
                Value::String(content) => {
                    let mut fact = event.clone();
                    fact.kind = role;
                    fact.content = Some(content.clone());
                    facts.push(fact);
                }
                Value::Array(blocks) => {
                    for block in blocks {
                        let mut fact = event.clone();
                        match block["type"].as_str().unwrap_or("") {
                            "text" => {
                                fact.kind = role;
                                fact.content = text(&block["text"]);
                                if kind == "assistant"
                                    && matches!(
                                        message["stop_reason"].as_str(),
                                        Some("end_turn" | "stop_sequence")
                                    )
                                {
                                    fact.phase = Some("final_answer".into());
                                }
                            }
                            "thinking" => {
                                fact.kind = Kind::Assistant;
                                fact.phase = Some("reasoning".into());
                                fact.content = text(&block["thinking"]);
                            }
                            "redacted_thinking" => {
                                fact.kind = Kind::Assistant;
                                fact.phase = Some("redacted_reasoning".into());
                            }
                            "tool_use" => {
                                fact.kind = Kind::ToolCall;
                                fact.name = string(&block["name"]);
                                fact.call_id = string(&block["id"]);
                                fact.content = text(&block["input"]);
                            }
                            "tool_result" => {
                                fact.kind = Kind::ToolResult;
                                fact.call_id = string(&block["tool_use_id"]);
                                fact.content = text(&block["content"]);
                                if block["is_error"] == true {
                                    fact.phase = Some("error".into());
                                }
                            }
                            _ => {
                                fact.content = text(block);
                            }
                        }
                        facts.push(fact);
                    }
                }
                _ => {}
            }
            if message["usage"].is_object() {
                let usage = &message["usage"];
                let mut fact = event.clone();
                fact.kind = Kind::Usage;
                fact.usage = Some(AgentLogUsage {
                    // Claude may persist multiple blocks for the same response id.
                    // Preserve the native message usage snapshot, never sum snapshots.
                    basis: AgentLogUsageBasis::Cumulative,
                    response_id: string(&message["id"]),
                    input_tokens: count(usage, "input_tokens"),
                    output_tokens: count(usage, "output_tokens"),
                    input_cache_hit_tokens: count(usage, "cache_read_input_tokens"),
                    cache_write_tokens: count(usage, "cache_creation_input_tokens"),
                    total_tokens: count(usage, "total_tokens"),
                });
                facts.push(fact);
            }
            if matches!(
                message["stop_reason"].as_str(),
                Some("end_turn" | "stop_sequence")
            ) {
                let mut fact = event;
                fact.kind = Kind::TaskEnd;
                fact.phase = string(&message["stop_reason"]);
                facts.push(fact);
            }
        } else {
            let mut fact = event;
            if kind == "system" && row["subtype"] == "turn_duration" {
                fact.kind = Kind::TaskEnd;
                fact.phase = Some("turn_duration".into());
            }
            fact.name = string(&row["subtype"]).or_else(|| Some(kind.into()));
            fact.content = text(&row["content"]);
            facts.push(fact);
        }
        Ok(facts)
    }
}
#[cfg(test)]
#[path = "_tests/adapter.rs"]
mod tests;
