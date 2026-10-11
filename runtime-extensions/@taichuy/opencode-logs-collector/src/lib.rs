//! Canonical durable OpenCode session.next events. Deltas and legacy projections are ignored.
pub mod hooks;
use agent_logs_collector::{
    AgentLogEvent, AgentLogEventKind as Kind, AgentLogUsage, AgentLogUsageBasis, Position,
    SourceAdapter,
};
use anyhow::{ensure, Context, Result};
use serde_json::Value;

pub struct OpenCodeAdapter;
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenCodeContext {
    session: String,
    turn: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    effort: Option<String>,
}
fn string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}
fn count(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .filter(|v| *v >= 0 && *v <= 9_007_199_254_740_991)
}
fn text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_owned());
    }
    value.as_array().map(|items| {
        items
            .iter()
            .filter_map(|item| item["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    })
}
fn timestamp(value: &Value) -> Result<String> {
    let time = if let Some(ms) = value.as_i64() {
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
            .context("Invalid source timestamp")?
    } else if let Some(value) = value.as_str() {
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .context("Invalid source timestamp")?
    } else {
        anyhow::bail!("Native event lacks timestamp");
    };
    Ok(time.format(&time::format_description::well_known::Rfc3339)?)
}
impl SourceAdapter for OpenCodeAdapter {
    type Context = OpenCodeContext;
    fn source_client(&self) -> &'static str {
        "opencode"
    }
    fn create_context(&self, first: &Value) -> Result<Self::Context> {
        ensure!(
            first["type"] == "collector_session",
            "OpenCode journal requires collector_session header"
        );
        let session = string(&first["session_id"])
            .filter(|s| !s.is_empty())
            .context("Missing journal session ID")?;
        Ok(OpenCodeContext {
            session,
            turn: None,
            model: None,
            provider: None,
            effort: None,
        })
    }
    fn convert(
        &self,
        record: &Position,
        context: &mut Self::Context,
    ) -> Result<Vec<AgentLogEvent>> {
        let line = &record.line;
        let kind = line["type"].as_str().unwrap_or("");
        let properties = &line["properties"];
        if !kind.starts_with("session.next.") || kind.ends_with(".delta") {
            return Ok(vec![]);
        }
        ensure!(
            properties["sessionID"].as_str() == Some(&context.session),
            "Native event session differs from journal"
        );
        ensure!(
            line["id"].as_str().is_some_and(|s| !s.is_empty()),
            "Native event lacks ID"
        );
        let occurred_at = timestamp(&properties["timestamp"])?;
        if kind == "session.next.prompted" {
            context.turn = string(&properties["messageID"]).filter(|s| !s.is_empty());
            ensure!(context.turn.is_some(), "Prompt lacks message ID");
            context.model = None;
            context.provider = None;
            context.effort = None;
        }
        if kind == "session.next.step.started" {
            context.model = string(&properties["model"]["id"]);
            context.provider = string(&properties["model"]["providerID"]);
            context.effort = string(&properties["model"]["variant"]);
        }
        let mut event = AgentLogEvent {
            event_id: String::new(),
            source_session_id: context.session.clone(),
            source_task_id: context.turn.clone().unwrap_or_default(),
            parent_source_task_id: None,
            sequence: 0,
            occurred_at,
            kind: Kind::Context,
            content: None,
            phase: None,
            name: None,
            call_id: None,
            model_id: context.model.clone(),
            reasoning_effort: context.effort.clone(),
            provider_code: context.provider.clone(),
            usage: None,
            inherited: false,
            raw: line.clone(),
        };
        let mut result = Vec::new();
        match kind {
            "session.next.prompted" => {
                event.kind = Kind::User;
                event.content = text(&properties["prompt"]["text"]);
            }
            "session.next.text.ended" => {
                event.kind = Kind::Assistant;
                event.content = text(&properties["text"]);
            }
            "session.next.reasoning.ended" => {
                event.kind = Kind::Assistant;
                event.phase = Some("reasoning".into());
                event.content = text(&properties["text"]);
            }
            "session.next.tool.called" => {
                event.kind = Kind::ToolCall;
                event.name = string(&properties["tool"]);
                event.call_id = string(&properties["callID"]);
                event.content = properties.get("input").map(Value::to_string);
            }
            "session.next.tool.success" | "session.next.tool.failed" => {
                event.kind = Kind::ToolResult;
                event.call_id = string(&properties["callID"]);
                event.content = text(&properties["content"])
                    .or_else(|| properties.get("result").map(Value::to_string))
                    .or_else(|| text(&properties["error"]["message"]));
                if kind.ends_with("failed") {
                    event.phase = Some("error".into());
                }
            }
            "session.next.step.ended" => {
                let tokens = &properties["tokens"];
                event.kind = Kind::Usage;
                event.usage = Some(AgentLogUsage {
                    basis: AgentLogUsageBasis::Delta,
                    response_id: string(&properties["assistantMessageID"]),
                    input_tokens: count(&tokens["input"]),
                    output_tokens: count(&tokens["output"]),
                    input_cache_hit_tokens: count(&tokens["cache"]["read"]),
                    cache_write_tokens: count(&tokens["cache"]["write"]),
                    total_tokens: count(&tokens["total"]),
                });
                if event.usage.as_ref().is_some_and(|usage| {
                    usage.input_tokens.is_some()
                        || usage.output_tokens.is_some()
                        || usage.input_cache_hit_tokens.is_some()
                        || usage.cache_write_tokens.is_some()
                        || usage.total_tokens.is_some()
                }) {
                    result.push(event.clone());
                }
                event.usage = None;
                event.kind = Kind::Context;
                event.phase = string(&properties["finish"]);
                if properties["finish"] == "stop" {
                    event.kind = Kind::TaskEnd;
                    event.phase = Some("completed".into());
                }
            }
            "session.next.step.failed" => {
                event.phase = Some("error".into());
                event.content = text(&properties["error"]["message"]);
            }
            "session.next.context.updated" | "session.next.synthetic" => {
                event.content = text(&properties["text"]);
            }
            "session.next.step.started" => (),
            _ => return Ok(vec![]),
        }
        result.push(event);
        Ok(result)
    }
}
#[cfg(test)]
#[path = "_tests/adapter.rs"]
mod tests;
