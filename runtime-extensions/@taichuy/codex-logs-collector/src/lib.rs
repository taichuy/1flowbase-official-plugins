//! Codex private rollout format adapter; delivery belongs to the shared SDK.
use agent_logs_collector::{
    AgentLogEvent, AgentLogEventKind as Kind, AgentLogUsage, AgentLogUsageBasis, Position,
    SourceAdapter,
};
use anyhow::{ensure, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct CodexAdapter {
    pub codex_home: PathBuf,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexContext {
    session: String,
    provider: Option<String>,
    model: Option<String>,
    turn: Option<String>,
    parent: Option<String>,
    inherited_before: Option<i64>,
}
fn string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}
fn text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => Some(
            items
                .iter()
                .map(|item| item.get("text").and_then(Value::as_str).unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => Some(value.to_string()),
    }
}
fn usage(value: &Value, basis: AgentLogUsageBasis, response_id: Option<String>) -> AgentLogUsage {
    let count = |key: &str| {
        value[key]
            .as_i64()
            .filter(|v| *v >= 0 && *v <= 9_007_199_254_740_991)
    };
    AgentLogUsage {
        basis,
        response_id,
        input_tokens: count("input_tokens"),
        output_tokens: count("output_tokens"),
        input_cache_hit_tokens: count("cached_input_tokens"),
        cache_write_tokens: count("cache_write_input_tokens"),
        total_tokens: count("total_tokens"),
    }
}
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64().is_some_and(|n| n != 0.0),
        Value::String(v) => !v.is_empty(),
        _ => true,
    }
}
impl SourceAdapter for CodexAdapter {
    type Context = CodexContext;
    fn source_client(&self) -> &'static str {
        "codex"
    }
    fn roots(&self, source: &Path) -> Vec<PathBuf> {
        // Configuration persists the selected path, not the install-time environment.
        // Service environments may have a different CODEX_HOME; inspect only directory
        // entry metadata so Codex's unrelated history.jsonl is never parsed as rollout.
        let has_entry = |name: &str, directory: bool| {
            std::fs::symlink_metadata(source.join(name)).is_ok_and(|meta| {
                !meta.file_type().is_symlink()
                    && if directory {
                        meta.is_dir()
                    } else {
                        meta.is_file()
                    }
            })
        };
        let explicit_file = source
            .extension()
            .is_some_and(|extension| extension == "jsonl");
        let codex_root = source == self.codex_home
            || has_entry("config.toml", false)
            || has_entry("history.jsonl", false)
            || has_entry("sessions", true)
            || has_entry("archived_sessions", true);
        if !explicit_file && codex_root {
            vec![source.join("sessions"), source.join("archived_sessions")]
        } else {
            vec![source.to_owned()]
        }
    }
    fn create_context(&self, first: &Value) -> Result<Self::Context> {
        ensure!(
            first["type"] == "session_meta",
            "Codex rollout must begin with session_meta and id"
        );
        let meta = &first["payload"];
        let session = string(&meta["id"])
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Codex rollout must begin with session_meta and id"))?;
        Ok(CodexContext {
            session,
            provider: string(&meta["model_provider"]),
            model: None,
            turn: None,
            parent: None,
            inherited_before: meta["subagent_history_start_ordinal"]
                .as_i64()
                .or_else(|| meta["forked_from_ordinal_exclusive"].as_i64()),
        })
    }
    fn convert(
        &self,
        record: &Position,
        context: &mut Self::Context,
    ) -> Result<Option<AgentLogEvent>> {
        let line = &record.line;
        let p = &line["payload"];
        let inherited = truthy(&line["metadata"]["inherited_user_message"])
            || truthy(&p["inherited"])
            || context
                .inherited_before
                .zip(line["ordinal"].as_i64())
                .is_some_and(|(before, ordinal)| ordinal < before);
        let explicit = if inherited {
            None
        } else {
            string(&p["turn_id"])
                .or_else(|| string(&p["internal_chat_message_metadata_passthrough"]["turn_id"]))
        };
        if explicit.as_ref().is_some_and(|s| !s.is_empty()) {
            context.turn = explicit.clone();
        }
        let line_type = line["type"].as_str().unwrap_or("");
        let payload_type = p["type"].as_str().unwrap_or("");
        if !inherited
            && (line_type == "turn_context"
                || (line_type == "event_msg"
                    && matches!(payload_type, "task_started" | "turn_started")))
        {
            context.turn = string(&p["turn_id"]).or_else(|| context.turn.clone());
            context.parent = string(&p["parent_turn_id"]).or_else(|| {
                string(&p["root_turn_id"]).filter(|root| Some(root) != context.turn.as_ref())
            });
            context.model = string(&p["model"]).or_else(|| context.model.clone());
            context.provider = string(&p["model_provider"]).or_else(|| context.provider.clone());
        }
        if !inherited && line_type == "token_usage_record" {
            context.turn = string(&p["turn_id"]).or_else(|| context.turn.clone());
            if let Some(root) = string(&p["root_turn_id"])
                .filter(|root| !root.is_empty() && Some(root) != context.turn.as_ref())
            {
                context.parent = Some(root);
            }
        }
        let occurred_at = string(&line["timestamp"])
            .ok_or_else(|| anyhow::anyhow!("Rollout event lacks a valid source timestamp"))?;
        time::OffsetDateTime::parse(&occurred_at, &time::format_description::well_known::Rfc3339)
            .map_err(|_| anyhow::anyhow!("Rollout event lacks a valid source timestamp"))?;
        let mut event = AgentLogEvent {
            event_id: String::new(),
            source_session_id: context.session.clone(),
            source_task_id: explicit
                .or_else(|| context.turn.clone())
                .unwrap_or_default(),
            parent_source_task_id: context.parent.clone(),
            sequence: 0,
            occurred_at,
            kind: Kind::Context,
            content: None,
            phase: None,
            name: None,
            call_id: None,
            model_id: context.model.clone(),
            provider_code: context.provider.clone(),
            usage: None,
            inherited,
            raw: line.clone(),
        };
        match line_type {
            "response_item" => match payload_type {
                "message" => {
                    event.kind = match p["role"].as_str().unwrap_or("") {
                        "user" => Kind::User,
                        "assistant" => Kind::Assistant,
                        "system" | "developer" => Kind::System,
                        _ => Kind::Context,
                    };
                    event.content = text(&p["content"]);
                    event.phase = string(&p["phase"]);
                }
                "function_call"
                | "custom_tool_call"
                | "local_shell_call"
                | "web_search_call"
                | "image_generation_call" => {
                    event.kind = Kind::ToolCall;
                    event.name = string(&p["name"]).or_else(|| Some(payload_type.into()));
                    event.call_id = string(&p["call_id"]).or_else(|| string(&p["id"]));
                    event.content = text(
                        p.get("arguments")
                            .filter(|v| !v.is_null())
                            .or_else(|| p.get("input").filter(|v| !v.is_null()))
                            .or_else(|| p.get("action"))
                            .unwrap_or(&Value::Null),
                    );
                }
                "function_call_output" | "custom_tool_call_output" | "local_shell_call_output" => {
                    event.kind = Kind::ToolResult;
                    event.call_id = string(&p["call_id"]);
                    event.content = text(&p["output"]);
                }
                _ => {}
            },
            "token_usage_record" => {
                event.kind = Kind::Usage;
                event.usage = Some(usage(
                    &p["usage"],
                    AgentLogUsageBasis::Delta,
                    string(&p["response_id"]).filter(|s| !s.is_empty()),
                ));
            }
            "event_msg" => match payload_type {
                "token_count" if !p["info"]["total_token_usage"].is_null() => {
                    event.kind = Kind::Usage;
                    event.usage = Some(usage(
                        &p["info"]["total_token_usage"],
                        AgentLogUsageBasis::Cumulative,
                        None,
                    ));
                }
                "user_message" | "agent_message" => {
                    event.content = text(&p["message"]);
                    event.phase = string(&p["phase"]);
                }
                "task_complete" | "turn_complete" | "turn_aborted" => {
                    event.kind = Kind::TaskEnd;
                    if payload_type == "turn_aborted" {
                        event.phase = Some("cancelled".into());
                    } else if let Some(final_text) =
                        string(&p["last_agent_message"]).filter(|s| !s.trim().is_empty())
                    {
                        event.phase = Some("final_answer".into());
                        event.content = Some(final_text);
                    }
                }
                _ => {}
            },
            _ => {}
        }
        if !inherited && event.kind == Kind::TaskEnd {
            context.turn = None;
            context.parent = None;
        }
        Ok(Some(event))
    }
}
#[cfg(test)]
#[path = "_tests/adapter.rs"]
mod tests;
