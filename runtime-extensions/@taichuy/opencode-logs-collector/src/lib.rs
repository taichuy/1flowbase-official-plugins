//! Native OpenCode V1 completed message/part facts and V2 canonical event boundaries.
pub mod hooks;
use agent_logs_collector::{
    AgentLogEvent, AgentLogEventKind as Kind, AgentLogUsage, AgentLogUsageBasis, Position,
    SourceAdapter,
};
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

pub struct OpenCodeAdapter;
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenCodeContext {
    session: String,
    turn: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    effort: Option<String>,
    #[serde(default)]
    messages: BTreeMap<String, MessageContext>,
    #[serde(default)]
    emitted: BTreeMap<String, String>,
    #[serde(default)]
    step_text: Option<String>,
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
fn source_time(value: &Value) -> Result<time::OffsetDateTime> {
    if let Some(ms) = value.as_i64() {
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
            .context("Invalid source timestamp")
    } else if let Some(value) = value.as_str() {
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .context("Invalid source timestamp")
    } else {
        anyhow::bail!("Native event lacks timestamp");
    }
}
fn timestamp(value: &Value) -> Result<String> {
    Ok(source_time(value)?.format(&time::format_description::well_known::Rfc3339)?)
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
            messages: BTreeMap::new(),
            emitted: BTreeMap::new(),
            step_text: None,
        })
    }
    fn convert(
        &self,
        record: &Position,
        context: &mut Self::Context,
    ) -> Result<Vec<AgentLogEvent>> {
        if matches!(
            record.line["type"].as_str(),
            Some("message.updated" | "message.part.updated")
        ) {
            return self.convert_v1(record, context);
        }
        let line = &record.line;
        let kind = line["type"].as_str().unwrap_or("");
        let properties = &line["properties"];
        if kind == "collector.system.transform" {
            ensure!(
                properties["sessionID"].as_str() == Some(&context.session),
                "System hook session differs from journal"
            );
            let occurred_at = timestamp(&properties["timestamp"])?;
            let content = properties["system"]
                .as_array()
                .context("System hook lacks native array")?
                .iter()
                .map(|entry| entry.as_str().context("Invalid native system text"))
                .collect::<Result<Vec<_>>>()?
                .join("\n");
            return Ok(vec![AgentLogEvent {
                event_id: String::new(),
                source_session_id: context.session.clone(),
                source_task_id: context.turn.clone().unwrap_or_default(),
                parent_source_task_id: None,
                sequence: 0,
                occurred_at,
                kind: Kind::System,
                content: Some(content),
                phase: Some("request_system_snapshot".into()),
                name: None,
                call_id: None,
                model_id: string(&properties["model"]["id"]),
                provider_code: string(&properties["model"]["providerID"]),
                reasoning_effort: None,
                usage: None,
                inherited: false,
                raw: line.clone(),
            }]);
        }
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
            context.step_text = None;
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
                append_text(&mut context.step_text, event.content.as_deref());
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
                    // V1 Session.getUsage and V2 publisher store disjoint non-cached input.
                    input_tokens: inclusive_input(
                        count(&tokens["input"]),
                        count(&tokens["cache"]["read"]),
                        count(&tokens["cache"]["write"]),
                    ),
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
                    event.content = context.step_text.clone();
                    event.phase = Some(
                        if event
                            .content
                            .as_ref()
                            .is_some_and(|text| !text.trim().is_empty())
                        {
                            "final_answer"
                        } else {
                            "completed"
                        }
                        .into(),
                    );
                }
            }
            "session.next.step.failed" => {
                // Native runner publishes this after transport retries and overflow recovery.
                event.kind = Kind::TaskEnd;
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

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageContext {
    role: String,
    turn: String,
    model: Option<String>,
    provider: Option<String>,
    effort: Option<String>,
    created: Value,
    completed: bool,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    real_prompt: bool,
    #[serde(default)]
    parent: Option<String>,
}
impl OpenCodeAdapter {
    fn convert_v1(
        &self,
        record: &Position,
        context: &mut OpenCodeContext,
    ) -> Result<Vec<AgentLogEvent>> {
        let line = &record.line;
        let p = &line["properties"];
        let is_info = line["type"] == "message.updated";
        let native = if is_info { &p["info"] } else { &p["part"] };
        ensure!(
            native["sessionID"].as_str() == Some(&context.session),
            "Message session differs from journal"
        );
        ensure!(
            p.get("sessionID").unwrap_or(&native["sessionID"]).as_str() == Some(&context.session),
            "Native event session differs from journal"
        );
        let id = string(&native["id"])
            .filter(|id| !id.is_empty())
            .context("Message/part lacks ID")?;
        if is_info {
            let role = native["role"].as_str().context("Message lacks role")?;
            let previous = context.messages.get(&id);
            let turn = if role == "user" {
                previous
                    .map(|message| message.turn.clone())
                    .unwrap_or_default()
            } else {
                let parent = native["parentID"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .context("Assistant lacks parent message")?;
                // The native parent ID remains an exact source identity when joining
                // mid-session. It does not establish a real user prompt.
                context
                    .messages
                    .get(parent)
                    .map(|message| message.turn.clone())
                    .filter(|turn| !turn.is_empty())
                    .unwrap_or_else(|| parent.to_owned())
            };
            let message = MessageContext {
                role: role.into(),
                turn,
                real_prompt: previous.is_some_and(|message| message.real_prompt),
                parent: string(&native["parentID"]),
                model: string(&native["modelID"]).or_else(|| string(&native["model"]["modelID"])),
                provider: string(&native["providerID"])
                    .or_else(|| string(&native["model"]["providerID"])),
                effort: string(&native["variant"]).or_else(|| string(&native["model"]["variant"])),
                created: native["time"]["created"].clone(),
                completed: native["time"]["completed"].is_number(),
                text: context
                    .messages
                    .get(&id)
                    .and_then(|previous| previous.text.clone()),
            };
            timestamp(&message.created)?;
            context.messages.insert(id.clone(), message.clone());
            let at = if message.completed {
                &native["time"]["completed"]
            } else {
                &message.created
            };
            let mut event = v1_event(line, context, &message, timestamp(at)?);
            if !message.completed {
                let key = format!("{id}:metadata");
                let signature =
                    agent_logs_collector::hash(serde_json::to_string(native)?.as_bytes());
                if context.emitted.get(&key) == Some(&signature) {
                    return Ok(vec![]);
                }
                context.emitted.insert(key, signature);
                return Ok(vec![event]);
            }
            let key = format!("{id}:completed");
            if !claim(context, key, native)? {
                return Ok(vec![]);
            }
            let tokens = &native["tokens"];
            let mut result = Vec::new();
            if let Some(usage) = native_usage(tokens, Some(id)) {
                event.kind = Kind::Usage;
                event.usage = Some(usage);
                result.push(event.clone());
                event.usage = None;
            }
            let failed = !native["error"].is_null();
            // V1 retries before halt; cleanup marks the assistant completed afterward.
            // Recoverable context overflow leaves error unset and continues via compaction.
            event.kind = if failed || native["finish"] == "stop" {
                Kind::TaskEnd
            } else {
                Kind::Context
            };
            event.phase = if failed {
                Some(
                    if native["error"]["name"] == "MessageAbortedError" {
                        "cancelled"
                    } else {
                        "error"
                    }
                    .into(),
                )
            } else {
                string(&native["finish"])
            };
            event.content = text(&native["error"]["data"]["message"])
                .or_else(|| text(&native["error"]["message"]));
            if event.kind == Kind::TaskEnd
                && !failed
                && message
                    .text
                    .as_ref()
                    .is_some_and(|text| !text.trim().is_empty())
            {
                event.content = message.text.clone();
                event.phase = Some("final_answer".into());
            }
            result.push(event);
            return Ok(result);
        }
        let message_id = native["messageID"]
            .as_str()
            .context("Part lacks message ID")?;
        let Some(mut message) = context.messages.get(message_id).cloned() else {
            // Do not invent attribution when joining an already-running historical session.
            return Ok(vec![]);
        };
        let part_type = native["type"].as_str().unwrap_or("");
        if message.role == "user" {
            // Metadata alone proves no origin. Only an observed synthetic part
            // may alias this unknown user message to the active real prompt.
            if native["synthetic"] == true && !message.real_prompt && message.turn.is_empty() {
                message.turn = context.turn.clone().unwrap_or_default();
                for child in context.messages.values_mut() {
                    if child.parent.as_deref() == Some(message_id) {
                        child.turn = message.turn.clone();
                    }
                }
            }
            if matches!(part_type, "text" | "file" | "agent" | "subtask")
                && native["synthetic"] != true
                && native["ignored"] != true
                && !message.real_prompt
            {
                message.real_prompt = true;
                message.turn = message_id.to_owned();
                for child in context.messages.values_mut() {
                    if child.parent.as_deref() == Some(message_id) {
                        child.turn = message.turn.clone();
                    }
                }
                let created = source_time(&message.created)?;
                // A late old part retains its own identity without stealing the active prompt.
                let newer = context
                    .turn
                    .as_ref()
                    .and_then(|id| context.messages.get(id))
                    .map(|current| source_time(&current.created))
                    .transpose()?
                    .is_some_and(|current| current > created);
                if !newer {
                    // Keep user aliases and unsettled assistants for exact parentID joins.
                    // Settled assistant bodies are no longer needed by a newer prompt.
                    context
                        .messages
                        .retain(|_, stored| stored.role == "user" || !stored.completed);
                    let messages = &context.messages;
                    context.emitted.retain(|key, _| {
                        messages.keys().any(|id| key.starts_with(&format!("{id}:")))
                    });
                    context.turn = Some(message_id.to_owned());
                    context.model = None;
                    context.provider = None;
                    context.effort = None;
                }
            }
            context
                .messages
                .insert(message_id.to_owned(), message.clone());
        }
        let mut result = Vec::new();
        match part_type {
            "text" | "reasoning" => {
                if native["ignored"] == true {
                    return Ok(vec![]);
                }
                let at = if message.role == "user" {
                    if p["time"].is_number() {
                        &p["time"]
                    } else {
                        &message.created
                    }
                } else {
                    if !native["time"]["end"].is_number() {
                        return Ok(vec![]);
                    }
                    &native["time"]["end"]
                };
                let key = format!("{message_id}:part:{id}:text");
                if !claim(context, key, native)? {
                    return Ok(vec![]);
                }
                let mut event = v1_event(line, context, &message, timestamp(at)?);
                event.kind = if message.role == "user" {
                    Kind::User
                } else {
                    Kind::Assistant
                };
                event.content = text(&native["text"]);
                if part_type == "reasoning" {
                    event.phase = Some("reasoning".into());
                }
                if native["synthetic"] == true {
                    event.kind = Kind::Context;
                    event.phase = Some("synthetic".into());
                }
                if message.role == "assistant" && part_type == "text" && native["synthetic"] != true
                {
                    if let Some(stored) = context.messages.get_mut(message_id) {
                        append_text(&mut stored.text, event.content.as_deref());
                    }
                }
                result.push(event);
            }
            "tool" => {
                let state = &native["state"];
                let status = state["status"].as_str().unwrap_or("");
                if !matches!(status, "running" | "completed" | "error") {
                    return Ok(vec![]);
                }
                let mut event =
                    v1_event(line, context, &message, timestamp(&state["time"]["start"])?);
                event.kind = Kind::ToolCall;
                event.name = string(&native["tool"]);
                event.call_id = string(&native["callID"]);
                event.content = state.get("input").map(Value::to_string);
                let call = serde_json::json!({"callID":native["callID"],"tool":native["tool"],"input":state["input"]});
                if claim(context, format!("{message_id}:part:{id}:call"), &call)? {
                    result.push(event.clone());
                }
                if matches!(status, "completed" | "error") {
                    event.occurred_at = timestamp(&state["time"]["end"])?;
                    event.kind = Kind::ToolResult;
                    event.content = text(&state["output"]).or_else(|| text(&state["error"]));
                    if status == "error" {
                        event.phase = Some("error".into());
                    }
                    if claim(context, format!("{message_id}:part:{id}:result"), state)? {
                        result.push(event);
                    }
                }
            }
            _ => (),
        }
        Ok(result)
    }
}
fn v1_event(
    raw: &Value,
    context: &OpenCodeContext,
    message: &MessageContext,
    occurred_at: String,
) -> AgentLogEvent {
    AgentLogEvent {
        event_id: String::new(),
        source_session_id: context.session.clone(),
        source_task_id: message.turn.clone(),
        parent_source_task_id: None,
        sequence: 0,
        occurred_at,
        kind: Kind::Context,
        content: None,
        phase: None,
        name: None,
        call_id: None,
        model_id: message.model.clone(),
        reasoning_effort: message.effort.clone(),
        provider_code: message.provider.clone(),
        usage: None,
        inherited: false,
        raw: raw.clone(),
    }
}
fn claim(context: &mut OpenCodeContext, key: String, fact: &Value) -> Result<bool> {
    let digest = agent_logs_collector::hash(serde_json::to_string(fact)?.as_bytes());
    if let Some(previous) = context.emitted.get(&key) {
        ensure!(previous == &digest, "Conflicting completed native fact");
        return Ok(false);
    }
    context.emitted.insert(key, digest);
    Ok(true)
}
fn native_usage(tokens: &Value, response_id: Option<String>) -> Option<AgentLogUsage> {
    let usage = AgentLogUsage {
        basis: AgentLogUsageBasis::Delta,
        response_id,
        // V1 Session.getUsage and V2 publisher store disjoint non-cached input.
        input_tokens: inclusive_input(
            count(&tokens["input"]),
            count(&tokens["cache"]["read"]),
            count(&tokens["cache"]["write"]),
        ),
        output_tokens: count(&tokens["output"]),
        input_cache_hit_tokens: count(&tokens["cache"]["read"]),
        cache_write_tokens: count(&tokens["cache"]["write"]),
        total_tokens: count(&tokens["total"]),
    };
    (usage.input_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.input_cache_hit_tokens.is_some()
        || usage.cache_write_tokens.is_some()
        || usage.total_tokens.is_some())
    .then_some(usage)
}

#[cfg(test)]
#[path = "_tests/adapter.rs"]
mod tests;

fn append_text(target: &mut Option<String>, value: Option<&str>) {
    if let Some(value) = value {
        if let Some(existing) = target {
            existing.push_str(value);
        } else {
            *target = Some(value.to_owned());
        }
    }
}

fn inclusive_input(uncached: Option<i64>, read: Option<i64>, write: Option<i64>) -> Option<i64> {
    uncached?
        .checked_add(read?)?
        .checked_add(write?)
        .filter(|value| (0..=9_007_199_254_740_991).contains(value))
}
