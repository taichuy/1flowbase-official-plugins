//! Native Antigravity JSONL transcript adapter; delivery belongs to the shared SDK.
use agent_logs_collector::{
    atomic_write, AgentLogEvent, AgentLogEventKind as Kind, AgentLogUsage, AgentLogUsageBasis,
    CheckpointLock, Position, SourceAdapter,
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::{Path, PathBuf},
};

pub mod hooks;

const SURFACE_DIRS: &[&str] = &["antigravity", "antigravity-ide", "antigravity-cli"];

pub struct AntigravityAdapter {
    pub client_home: PathBuf,
    pub parent_links_path: Option<PathBuf>,
}

impl AntigravityAdapter {
    pub fn new(client_home: PathBuf, parent_links_path: Option<PathBuf>) -> Self {
        Self {
            client_home,
            parent_links_path,
        }
    }

    fn load_parent_task(&self, conversation_id: &str) -> Option<String> {
        let path = self.parent_links_path.as_ref()?;
        let bytes = fs::read(path).ok()?;
        let map: BTreeMap<String, String> = serde_json::from_slice(&bytes).ok()?;
        map.get(conversation_id).cloned()
    }

    fn record_parent_link(&self, child_conversation_id: &str, parent_task_id: &str) {
        let Some(path) = self.parent_links_path.as_ref() else {
            return;
        };
        if !valid_conversation_id(child_conversation_id) || parent_task_id.is_empty() {
            return;
        }
        let Ok(_lock) = CheckpointLock::wait(path) else {
            return;
        };
        let mut map: BTreeMap<String, String> = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        if map.get(child_conversation_id).map(String::as_str) == Some(parent_task_id) {
            return;
        }
        map.insert(child_conversation_id.to_owned(), parent_task_id.to_owned());
        if let Ok(bytes) = serde_json::to_vec(&map) {
            let _ = atomic_write(path, &bytes);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingTool {
    call_id: String,
    name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntigravityContext {
    session: String,
    turn: Option<String>,
    parent: Option<String>,
    model: Option<String>,
    #[serde(default)]
    pending_tools: VecDeque<PendingTool>,
}

fn valid_conversation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(idx, byte)| {
            byte.is_ascii_alphanumeric() || (idx > 0 && matches!(byte, b'-' | b'_'))
        })
}

fn is_regular_non_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|meta| !meta.file_type().is_symlink() && meta.is_file())
}

fn is_dir_non_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|meta| !meta.file_type().is_symlink() && meta.is_dir())
}

/// Prefer untruncated `transcript_full.jsonl`; fall back to `transcript.jsonl`.
/// Strictly selects at most one canonical file per conversation directory.
pub fn select_conversation_transcript(logs_dir: &Path) -> Option<PathBuf> {
    let full = logs_dir.join("transcript_full.jsonl");
    if is_regular_non_symlink(&full) {
        return Some(full);
    }
    let compact = logs_dir.join("transcript.jsonl");
    if is_regular_non_symlink(&compact) {
        return Some(compact);
    }
    None
}

/// Upgrade a hook-supplied `transcript.jsonl` path to sibling `transcript_full.jsonl` when present.
pub fn canonical_transcript_for_path(path: &Path) -> PathBuf {
    if path.file_name().and_then(|n| n.to_str()) == Some("transcript.jsonl") {
        if let Some(parent) = path.parent() {
            let full = parent.join("transcript_full.jsonl");
            if is_regular_non_symlink(&full) {
                return full;
            }
        }
    }
    path.to_owned()
}

fn collect_brain_transcripts(brain_dir: &Path, output: &mut Vec<PathBuf>) {
    if !is_dir_non_symlink(brain_dir) {
        return;
    }
    let Ok(entries) = fs::read_dir(brain_dir) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(valid_conversation_id)
                && is_dir_non_symlink(path)
        })
        .collect();
    dirs.sort();
    for conv_dir in dirs {
        let logs_dir = conv_dir.join(".system_generated").join("logs");
        if is_dir_non_symlink(&logs_dir) {
            if let Some(transcript) = select_conversation_transcript(&logs_dir) {
                output.push(transcript);
            }
        }
    }
}

pub fn conversation_id_from_path(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?;
    if matches!(file_name, "transcript.jsonl" | "transcript_full.jsonl") {
        let logs = path.parent()?;
        if logs.file_name()?.to_str()? == "logs" {
            let sys = logs.parent()?;
            if sys.file_name()?.to_str()? == ".system_generated" {
                let conv = sys.parent()?.file_name()?.to_str()?;
                if valid_conversation_id(conv) {
                    return Some(conv.to_owned());
                }
            }
        }
    }
    let stem = path.file_stem()?.to_str()?;
    if !matches!(stem, "transcript" | "transcript_full") && valid_conversation_id(stem) {
        return Some(stem.to_owned());
    }
    None
}

fn string(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn count(value: &Value, key: &str) -> Option<i64> {
    value[key]
        .as_i64()
        .filter(|n| *n >= 0 && *n <= 9_007_199_254_740_991)
}

fn inclusive_input(raw_input: Option<i64>, cache_read: Option<i64>) -> Option<i64> {
    match (raw_input, cache_read) {
        (None, None) => None,
        (a, b) => a
            .unwrap_or(0)
            .checked_add(b.unwrap_or(0))
            .filter(|n| *n <= 9_007_199_254_740_991),
    }
}

fn clean_user_prompt(raw: &str) -> Option<String> {
    if let Some(start) = raw.find("<USER_REQUEST>") {
        let after = &raw[start + "<USER_REQUEST>".len()..];
        if let Some(end) = after.find("</USER_REQUEST>") {
            let inner = after[..end].trim();
            if !inner.is_empty() {
                return Some(inner.to_owned());
            }
        }
    }
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then_some(trimmed.to_owned())
}

fn extract_subagent_conversation_id(content: &str) -> Option<String> {
    // Antigravity invoke_subagent tool output embeds a JSON block containing `"conversationId": "<id>"`.
    let key = "\"conversationId\"";
    let start = content.find(key)?;
    let rest = &content[start + key.len()..];
    let colon = rest.find(':')?;
    let after_colon = rest[colon + 1..].trim_start();
    let quoted = after_colon.strip_prefix('"')?;
    let end = quoted.find('"')?;
    let id = &quoted[..end];
    valid_conversation_id(id).then_some(id.to_owned())
}

fn build_usage(row: &Value, response_id: String) -> Option<AgentLogUsage> {
    let raw_in = count(row, "input_tokens");
    let cache_read = count(row, "cache_read_tokens");
    let out = count(row, "output_tokens");
    if raw_in.is_none() && cache_read.is_none() && out.is_none() {
        return None;
    }
    let canonical_in = inclusive_input(raw_in, cache_read);
    let total = match (canonical_in, out) {
        (Some(i), Some(o)) => i.checked_add(o).filter(|n| *n <= 9_007_199_254_740_991),
        (Some(i), None) => Some(i),
        (None, Some(o)) => Some(o),
        (None, None) => None,
    };
    Some(AgentLogUsage {
        basis: AgentLogUsageBasis::Delta,
        response_id: Some(response_id),
        input_tokens: canonical_in,
        output_tokens: out,
        input_cache_hit_tokens: Some(cache_read.unwrap_or(0)),
        cache_write_tokens: Some(0),
        total_tokens: total,
    })
}

fn base_event(row: &Value, context: &AntigravityContext, occurred_at: String) -> AgentLogEvent {
    AgentLogEvent {
        event_id: String::new(),
        source_session_id: context.session.clone(),
        source_task_id: context.turn.clone().unwrap_or_default(),
        parent_source_task_id: context.parent.clone(),
        sequence: 0,
        occurred_at,
        kind: Kind::Context,
        content: None,
        phase: None,
        name: None,
        call_id: None,
        model_id: context.model.clone(),
        reasoning_effort: None,
        provider_code: None,
        usage: None,
        inherited: false,
        raw: row.clone(),
    }
}

impl SourceAdapter for AntigravityAdapter {
    type Context = AntigravityContext;

    fn source_client(&self) -> &'static str {
        "antigravity"
    }

    fn roots(&self, source: &Path) -> Vec<PathBuf> {
        if is_regular_non_symlink(source)
            && source.extension().is_some_and(|ext| ext == "jsonl")
        {
            return vec![source.to_owned()];
        }
        let mut results = Vec::new();
        let looks_like_gemini_home = source == self.client_home
            || SURFACE_DIRS
                .iter()
                .any(|dir| is_dir_non_symlink(&source.join(dir)))
            || is_dir_non_symlink(&source.join("config"));
        if looks_like_gemini_home {
            for surface in SURFACE_DIRS {
                collect_brain_transcripts(&source.join(surface).join("brain"), &mut results);
            }
            return results;
        }
        let nested_brain = source.join("brain");
        if is_dir_non_symlink(&nested_brain) {
            collect_brain_transcripts(&nested_brain, &mut results);
            return results;
        }
        collect_brain_transcripts(source, &mut results);
        results
    }

    fn source_identity_seed(&self, path: &Path, first: &[u8]) -> Vec<u8> {
        let mut seed = conversation_id_from_path(path)
            .unwrap_or_default()
            .into_bytes();
        seed.push(b':');
        seed.extend_from_slice(first);
        seed
    }

    fn create_context(&self, first: &Value) -> Result<Self::Context> {
        let session = string(&first["conversation_id"]).ok_or_else(|| {
            anyhow::anyhow!("Antigravity transcript requires path or conversation_id")
        })?;
        self.build_context(session, first)
    }

    fn create_context_for_path(&self, path: &Path, first: &Value) -> Result<Self::Context> {
        let session = conversation_id_from_path(path)
            .or_else(|| string(&first["conversation_id"]))
            .ok_or_else(|| anyhow::anyhow!("Cannot determine Antigravity conversation ID"))?;
        self.build_context(session, first)
    }

    fn convert(
        &self,
        position: &Position,
        context: &mut Self::Context,
    ) -> Result<Vec<AgentLogEvent>> {
        let row = &position.line;
        let step_type = row["type"].as_str().unwrap_or("");
        let step_index = row["step_index"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("Antigravity record missing integer step_index"))?;
        let Some(occurred_at) = string(&row["created_at"]) else {
            return Ok(vec![]);
        };
        time::OffsetDateTime::parse(
            &occurred_at,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|_| anyhow::anyhow!("Antigravity record has invalid RFC3339 created_at"))?;

        let status = row["status"].as_str().unwrap_or("");
        let mut facts = Vec::new();

        match step_type {
            "USER_INPUT" => {
                let raw_content = row["content"].as_str().unwrap_or("");
                let is_stream_interrupt = raw_content
                    .trim_start()
                    .starts_with("Error: The stream was interrupted");
                if is_stream_interrupt {
                    let mut event = base_event(row, context, occurred_at);
                    event.kind = Kind::Context;
                    event.content = Some(raw_content.to_owned());
                    facts.push(event);
                } else {
                    if context.turn.is_none() && context.parent.is_none() {
                        context.parent = self.load_parent_task(&context.session);
                    }
                    context.turn = Some(format!("{}:step:{step_index}", context.session));
                    context.pending_tools.clear();
                    let mut event = base_event(row, context, occurred_at);
                    event.kind = Kind::User;
                    event.content = clean_user_prompt(raw_content);
                    facts.push(event);
                }
            }
            "PLANNER_RESPONSE" => {
                if context.turn.is_none() {
                    if context.parent.is_none() {
                        context.parent = self.load_parent_task(&context.session);
                    }
                    // Subagent transcripts begin with SYSTEM_MESSAGE rather than USER_INPUT.
                    context.turn = Some(format!("{}:root", context.session));
                }
                if let Some(model) = string(&row["model"]) {
                    context.model = Some(model);
                }
                context.pending_tools.clear();

                let event = base_event(row, context, occurred_at);
                if let Some(thinking) = string(&row["thinking"]) {
                    let mut reasoning = event.clone();
                    reasoning.kind = Kind::Assistant;
                    reasoning.phase = Some("reasoning".into());
                    reasoning.content = Some(thinking);
                    facts.push(reasoning);
                }

                let tool_calls = row["tool_calls"].as_array();
                let has_tools = tool_calls.is_some_and(|calls| !calls.is_empty());
                let content = string(&row["content"]);

                if has_tools {
                    if let Some(progress_text) = content {
                        let mut assistant = event.clone();
                        assistant.kind = Kind::Assistant;
                        assistant.content = Some(progress_text);
                        facts.push(assistant);
                    }
                    if let Some(calls) = tool_calls {
                        for (idx, call) in calls.iter().enumerate() {
                            let name = string(&call["name"]).unwrap_or_else(|| "tool".into());
                            let call_id = string(&call["id"])
                                .unwrap_or_else(|| format!("step-{step_index}-tool-{idx}"));
                            context.pending_tools.push_back(PendingTool {
                                call_id: call_id.clone(),
                                name: name.clone(),
                            });
                            let mut tc = event.clone();
                            tc.kind = Kind::ToolCall;
                            tc.name = Some(name);
                            tc.call_id = Some(call_id);
                            tc.content = call
                                .get("args")
                                .filter(|v| !v.is_null())
                                .map(|v| v.to_string());
                            facts.push(tc);
                        }
                    }
                } else if let Some(answer) = content {
                    let is_done = status == "DONE";
                    let mut assistant = event.clone();
                    assistant.kind = Kind::Assistant;
                    assistant.content = Some(answer.clone());
                    if is_done {
                        assistant.phase = Some("final_answer".into());
                    }
                    facts.push(assistant);
                    if is_done {
                        let mut end = event.clone();
                        end.kind = Kind::TaskEnd;
                        end.phase = Some("final_answer".into());
                        end.content = Some(answer);
                        facts.push(end);
                    }
                }

                if let Some(usage) =
                    build_usage(row, format!("{}:step:{step_index}", context.session))
                {
                    let mut usage_event = event;
                    usage_event.kind = Kind::Usage;
                    usage_event.usage = Some(usage);
                    facts.push(usage_event);
                }
            }
            "GENERIC" | "VIEW_FILE" | "LIST_DIRECTORY" | "ASK_QUESTION" => {
                let pending = context.pending_tools.pop_front();
                let content = string(&row["content"]);
                let error = string(&row["error"]);
                let is_error = status == "ERROR" || error.is_some();

                if let (Some(tool), Some(text), Some(parent_task)) =
                    (pending.as_ref(), content.as_deref(), context.turn.as_deref())
                {
                    if tool.name == "invoke_subagent" {
                        if let Some(child_id) = extract_subagent_conversation_id(text) {
                            self.record_parent_link(&child_id, parent_task);
                        }
                    }
                }

                let mut event = base_event(row, context, occurred_at);
                event.kind = Kind::ToolResult;
                event.name = pending.as_ref().map(|t| t.name.clone());
                event.call_id = Some(
                    pending
                        .map(|t| t.call_id)
                        .unwrap_or_else(|| format!("step-{step_index}")),
                );
                event.content = match (content, error) {
                    (Some(c), Some(e)) => Some(format!("{c}\n{e}")),
                    (Some(c), None) => Some(c),
                    (None, Some(e)) => Some(e),
                    (None, None) => None,
                };
                if is_error {
                    event.phase = Some("error".into());
                }
                facts.push(event);
            }
            "ERROR_MESSAGE" => {
                if context.turn.is_none() {
                    context.turn = Some(format!("{}:root", context.session));
                }
                let mut event = base_event(row, context, occurred_at);
                event.kind = Kind::TaskEnd;
                event.phase = Some("error".into());
                event.content = string(&row["error"]).or_else(|| string(&row["content"]));
                facts.push(event);
            }
            "CHECKPOINT" => {
                let mut ctx = base_event(row, context, occurred_at.clone());
                ctx.kind = Kind::Context;
                ctx.inherited = true;
                ctx.content = string(&row["content"]);
                facts.push(ctx);
                if let Some(usage) =
                    build_usage(row, format!("{}:checkpoint:{step_index}", context.session))
                {
                    let mut usage_event = base_event(row, context, occurred_at);
                    usage_event.kind = Kind::Usage;
                    usage_event.usage = Some(usage);
                    facts.push(usage_event);
                }
            }
            "SYSTEM_MESSAGE" => {
                let mut event = base_event(row, context, occurred_at);
                event.kind = Kind::System;
                event.content = string(&row["content"]);
                facts.push(event);
            }
            "CONVERSATION_HISTORY" => {
                let mut event = base_event(row, context, occurred_at);
                event.kind = Kind::Context;
                event.inherited = true;
                event.content = string(&row["content"]);
                facts.push(event);
            }
            _ => {
                let mut event = base_event(row, context, occurred_at);
                event.kind = Kind::Context;
                event.content = string(&row["content"]);
                facts.push(event);
            }
        }

        Ok(facts)
    }
}

impl AntigravityAdapter {
    fn build_context(&self, session: String, first: &Value) -> Result<AntigravityContext> {
        ensure!(
            first["step_index"].is_i64()
                && first["type"].is_string()
                && first["created_at"].is_string(),
            "Not a native Antigravity transcript"
        );
        let parent = self.load_parent_task(&session);
        Ok(AntigravityContext {
            session,
            turn: None,
            parent,
            model: None,
            pending_tools: VecDeque::new(),
        })
    }
}

#[cfg(test)]
#[path = "_tests/adapter.rs"]
mod tests;
