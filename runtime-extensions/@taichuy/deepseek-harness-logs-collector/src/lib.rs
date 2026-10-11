//! Adapter for journaled native DeepSeek Harness v4 Session events.
pub mod hooks;
use agent_logs_collector::{
    AgentLogEvent, AgentLogEventKind as Kind, AgentLogUsage, AgentLogUsageBasis, Position,
    SourceAdapter,
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
pub struct DeepSeekHarnessAdapter;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionContext {
    session: String,
    turn: Option<String>,
    parent: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    effort: Option<String>,
    #[serde(default)]
    final_text: Option<String>,
}
fn string(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}
fn identity(v: &Value) -> Option<String> {
    string(v).or_else(|| v.as_u64().map(|n| n.to_string()))
}
fn usage(v: &Value) -> AgentLogUsage {
    let count = |key: &str| {
        v[key]
            .as_i64()
            .filter(|n| (0..=9_007_199_254_740_991).contains(n))
    };
    let cache_count = |key: &str| {
        if v.get(key).is_none() {
            // Zero omission applies only to a present valid native usage record.
            count("inputTokens").map(|_| 0)
        } else {
            count(key)
        }
    };
    AgentLogUsage {
        basis: AgentLogUsageBasis::Delta,
        response_id: None,
        // DSH TokenUsage is disjoint; omitted optional cache fields mean zero
        // (token-meter/usage-projection and llm-pi-ai/mapUsage). Present null/invalid is unknown.
        input_tokens: inclusive_input(
            count("inputTokens"),
            cache_count("cacheReadTokens"),
            cache_count("cacheWriteTokens"),
        ),
        output_tokens: count("outputTokens"),
        input_cache_hit_tokens: cache_count("cacheReadTokens"),
        cache_write_tokens: cache_count("cacheWriteTokens"),
        total_tokens: count("totalTokens"),
    }
}
impl SourceAdapter for DeepSeekHarnessAdapter {
    type Context = SessionContext;
    fn source_client(&self) -> &'static str {
        "deepseek-harness"
    }
    fn create_context(&self, first: &Value) -> Result<SessionContext> {
        ensure!(
            first["type"] == "collector_session",
            "Expected private collector_session journal header"
        );
        let session = string(&first["session_id"])
            .ok_or_else(|| anyhow::anyhow!("Missing source session id"))?;
        Ok(SessionContext {
            session,
            turn: None,
            parent: None,
            model: None,
            provider: None,
            effort: None,
            final_text: None,
        })
    }
    fn convert(&self, row: &Position, ctx: &mut SessionContext) -> Result<Vec<AgentLogEvent>> {
        let raw = &row.line;
        if raw["type"] == "collector_session" {
            return Ok(vec![]);
        }
        ensure!(
            raw["type"] == "dsh_event",
            "Unexpected native journal record"
        );
        ensure!(
            raw["session"]["id"] == ctx.session,
            "Native session does not match journal identity"
        );
        let e = &raw["event"];
        let d = &e["data"];
        let ty = e["type"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing native event type"))?;
        if ty == "turn/start" {
            ctx.final_text = None;
            ctx.turn = identity(&d["turn_id"]).or_else(|| identity(&d["turn"]));
            ctx.parent = identity(&d["parent_turn_id"]).or_else(|| identity(&d["parentTaskId"]));
            // Request headers are change-driven and remain active across turns.
        } else if let Some(id) = identity(&d["turn_id"]).or_else(|| identity(&d["turn"])) {
            ctx.turn = Some(id);
            if let Some(parent) =
                identity(&d["parent_turn_id"]).or_else(|| identity(&d["parentTaskId"]))
            {
                ctx.parent = Some(parent);
            }
        }
        if ty == "request/header" {
            let cfg = &d["header"]["config"];
            ctx.model = string(&cfg["model"]);
            ctx.provider = string(&cfg["provider"]);
            ctx.effort = string(&cfg["reasoningEffort"]);
        }
        let message = if ty == "user/message" {
            d
        } else {
            &d["message"]
        };
        if matches!(
            ty,
            "tool/call" | "tool/ptc-dispatch-start" | "assistant/attempt"
        ) {
            ctx.final_text = None;
        }
        if ty == "assistant/message" {
            // A candidate belongs only to the latest complete assistant message
            // in this turn. Tool-bearing messages and interrupted prefixes are
            // intermediate output, not a completed final answer.
            ctx.final_text = message["content"].as_array().and_then(|blocks| {
                if d["interrupted"] == true || blocks.iter().any(|b| b["type"] == "tool-call") {
                    return None;
                }
                let text = blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<String>();
                (!text.trim().is_empty()).then_some(text)
            });
            ctx.model = string(&message["source"]["model"]).or_else(|| ctx.model.clone());
            ctx.provider = string(&message["source"]["provider"]).or_else(|| ctx.provider.clone());
        }
        let millis = e["time"]
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or_else(|| anyhow::anyhow!("Missing native event timestamp"))?;
        let occurred_at =
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000)?
                .format(&Rfc3339)?;
        let base = AgentLogEvent {
            event_id: String::new(),
            source_session_id: ctx.session.clone(),
            source_task_id: ctx.turn.clone().unwrap_or_default(),
            parent_source_task_id: ctx.parent.clone(),
            sequence: 0,
            occurred_at,
            kind: Kind::Context,
            content: None,
            phase: None,
            name: None,
            call_id: None,
            model_id: ctx.model.clone(),
            provider_code: ctx.provider.clone(),
            reasoning_effort: ctx.effort.clone(),
            usage: None,
            inherited: false,
            raw: raw.clone(),
        };
        let mut facts = vec![];
        match ty {
            "turn/start" => {
                let mut f = base.clone();
                f.kind = Kind::Context;
                f.phase = Some("turn/start".into());
                facts.push(f);
            }
            "turn/end" => {
                let mut f = base.clone();
                f.kind = Kind::TaskEnd;
                let reason = d["reason"]["kind"].as_str();
                f.phase = match reason {
                    Some("aborted") => Some("cancelled".into()),
                    Some("completed") => {
                        if let Some(text) = ctx.final_text.take() {
                            f.content = Some(text);
                            Some("final_answer".into())
                        } else {
                            Some("completed".into())
                        }
                    }
                    _ => string(&d["reason"]["kind"]),
                };
                facts.push(f);
            }
            "tool/call" | "tool/ptc-dispatch-start" => {
                let mut f = base.clone();
                f.kind = Kind::ToolCall;
                f.name = string(&d["name"]);
                f.call_id = string(&d["callId"]).or_else(|| string(&d["subCallId"]));
                f.content = string(&d["arguments"])
                    .or_else(|| (!d["arguments"].is_null()).then(|| d["arguments"].to_string()));
                facts.push(f);
            }
            "tool/result" | "tool/ptc-dispatch" => {
                let mut f = base.clone();
                f.kind = Kind::ToolResult;
                f.call_id = string(&message["toolCallId"]).or_else(|| string(&d["subCallId"]));
                f.name = string(&d["name"]);
                f.phase = Some(
                    if message["isError"] == true || d["isError"] == true {
                        "error"
                    } else {
                        "completed"
                    }
                    .into(),
                );
                let blocks = if ty == "tool/ptc-dispatch" {
                    &d["content"]
                } else {
                    &message["content"]
                };
                f.content = blocks.as_array().map(|a| {
                    a.iter()
                        .filter_map(|b| b["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                });
                facts.push(f);
            }
            "user/message" | "assistant/message" | "system/message" | "developer/message" => {
                if let Some(blocks) = message["content"].as_array() {
                    for block in blocks {
                        let mut f = base.clone();
                        f.kind = match block["type"].as_str().unwrap_or("") {
                            "reasoning" => {
                                f.phase = Some("reasoning".into());
                                Kind::Assistant
                            }
                            // The dedicated tool/call event owns tool lifecycle, avoiding duplicate calls.
                            "tool-call" => continue,
                            _ => match ty {
                                "user/message" if d["source"]["kind"] == "user" => Kind::User,
                                "user/message" => Kind::Context,
                                "assistant/message" => Kind::Assistant,
                                _ => Kind::System,
                            },
                        };
                        if ty == "user/message" && d["source"]["kind"] != "user" {
                            f.kind = Kind::Context;
                            f.name = string(&d["source"]["kind"]);
                            f.phase = string(&d["source"]["form"]);
                        }
                        f.content = string(&block["text"]);
                        if d["interrupted"] == true {
                            f.phase = Some(
                                if block["type"] == "reasoning" {
                                    "interrupted_reasoning"
                                } else {
                                    "interrupted"
                                }
                                .into(),
                            );
                        }
                        facts.push(f);
                    }
                }
                if ty == "assistant/message" && d["usage"].is_object() {
                    let mut f = base.clone();
                    f.kind = Kind::Usage;
                    f.usage = Some(usage(&d["usage"]));
                    facts.push(f);
                }
            }
            "assistant/attempt" => {
                // Attempt streams are diagnostic output only, never fabricated surface messages.
                // Compact runs are joined per block/type, while full exact timing remains in raw.
                let mut blocks = std::collections::BTreeMap::<(u64, String), String>::new();
                if let Some(stream) = d["stream"].as_array() {
                    for run in stream {
                        let ty = run["type"].as_str().unwrap_or("");
                        if matches!(ty, "text-chunks" | "reasoning-chunks") {
                            let text = blocks
                                .entry((run["index"].as_u64().unwrap_or(0), ty.into()))
                                .or_default();
                            if let Some(texts) = run["texts"].as_array() {
                                for delta in texts {
                                    if let Some(s) = delta.as_str() {
                                        text.push_str(s);
                                    }
                                }
                            }
                        } else if ty == "chunk" {
                            let chunk = &run["chunk"];
                            let cty = chunk["type"].as_str().unwrap_or("");
                            if matches!(cty, "text-delta" | "reasoning-delta") {
                                let key = if cty == "text-delta" {
                                    "text-chunks"
                                } else {
                                    "reasoning-chunks"
                                };
                                blocks
                                    .entry((chunk["index"].as_u64().unwrap_or(0), key.into()))
                                    .or_default()
                                    .push_str(chunk["text"].as_str().unwrap_or(""));
                            } else if cty == "usage" {
                                let mut f = base.clone();
                                f.kind = Kind::Usage;
                                f.usage = Some(usage(&chunk["usage"]));
                                f.phase = Some("attempt".into());
                                facts.push(f);
                            }
                        }
                    }
                }
                for ((_, ty), content) in blocks {
                    let mut f = base.clone();
                    f.kind = Kind::Assistant;
                    f.content = Some(content);
                    f.phase = Some(
                        if ty == "reasoning-chunks" {
                            "attempt_reasoning"
                        } else {
                            "attempt"
                        }
                        .into(),
                    );
                    facts.push(f);
                }
            }
            _ => {}
        }
        // Every record retains exact raw native facts, even when its type has no normalized mapping.
        if facts.is_empty() {
            facts.push(base);
        }
        if ty == "turn/end" {
            ctx.final_text = None;
            ctx.turn = None;
            ctx.parent = None;
        }
        Ok(facts)
    }
}
#[cfg(test)]
mod tests;

fn inclusive_input(uncached: Option<i64>, read: Option<i64>, write: Option<i64>) -> Option<i64> {
    uncached?
        .checked_add(read?)?
        .checked_add(write?)
        .filter(|value| (0..=9_007_199_254_740_991).contains(value))
}
