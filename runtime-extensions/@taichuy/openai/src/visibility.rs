//! Delay only recognized empty Responses scaffolding until it is safe to publish.
use super::{ProviderStreamEvent, Result};
use serde_json::{json, Value};

pub(crate) const MAX_EVENTS: usize = 1024;
pub(crate) const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    kind: Option<&'static str>,
    count: usize,
    bytes: usize,
}

impl std::fmt::Display for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Responses visibility boundary")
    }
}

impl Snapshot {
    pub(crate) fn annotate(&self, value: &mut Value) {
        value["semantic_event_kind"] = json!(self.kind);
        value["buffered_scaffold_events"] = json!(self.count);
        value["buffered_scaffold_bytes"] = json!(self.bytes);
    }
}

#[derive(Default)]
pub(crate) struct Visibility {
    pending: Vec<ProviderStreamEvent>,
    bytes: usize,
    first_semantic: Option<&'static str>,
}

impl Visibility {
    pub(crate) fn observe(&mut self, payload: &Value) {
        if self.first_semantic.is_none() {
            self.first_semantic = semantic_kind(payload);
        }
    }

    pub(crate) fn committed(&self) -> bool {
        self.first_semantic.is_some()
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            kind: self.first_semantic,
            count: self.pending.len(),
            bytes: self.bytes,
        }
    }

    pub(crate) fn publish<F>(
        &mut self,
        events: &mut Vec<ProviderStreamEvent>,
        all_events: &mut Vec<ProviderStreamEvent>,
        sink: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&ProviderStreamEvent) -> Result<()>,
    {
        if self.committed() {
            self.flush(all_events, sink)?;
            for event in events.iter() {
                sink(event)?;
            }
            record_emitted(events, all_events);
            return Ok(());
        }
        let bytes = events.iter().try_fold(0usize, |total, event| {
            serde_json::to_vec(event).map(|encoded| total.saturating_add(encoded.len()))
        })?;
        if self.pending.len().saturating_add(events.len()) > MAX_EVENTS
            || self.bytes.saturating_add(bytes) > MAX_BYTES
        {
            // Refuse rather than exposing abandoned slots or retaining unbounded data.
            let mut error = super::recovery_diagnostics::transport_error(
                "visibility_buffer_limit",
                "policy_rejected",
                None,
            )
            .downcast::<super::ProviderRuntimeError>()?;
            error.kind = super::ProviderRuntimeErrorKind::ProviderInvalidResponse;
            return Err(error.into());
        }
        self.bytes += bytes;
        self.pending.append(events);
        Ok(())
    }

    pub(crate) fn flush<F>(
        &mut self,
        all_events: &mut Vec<ProviderStreamEvent>,
        sink: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&ProviderStreamEvent) -> Result<()>,
    {
        for event in &self.pending {
            sink(event)?;
        }
        record_emitted(&mut self.pending, all_events);
        self.bytes = 0;
        Ok(())
    }
}

// HTTP raw NativeEvents have always been sink-only; retain the existing result
// envelope projection while buffering them in the same publication order.
fn record_emitted(
    events: &mut Vec<ProviderStreamEvent>,
    all_events: &mut Vec<ProviderStreamEvent>,
) {
    all_events.extend(
        events
            .drain(..)
            .filter(|event| !matches!(event, ProviderStreamEvent::NativeEvent { .. })),
    );
}

fn keys_only(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.keys().all(|key| keys.contains(&key.as_str())))
}

fn absent_or_empty_array(value: &Value, key: &str) -> bool {
    value
        .get(key)
        .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty))
}

fn empty_item(item: &Value) -> bool {
    if item
        .get("status")
        .is_some_and(|status| status != "in_progress")
        || item.get("id").is_some_and(|id| !id.is_string())
    {
        return false;
    }
    match item["type"].as_str() {
        Some("message") => {
            keys_only(item, &["type", "id", "role", "status", "content"])
                && item.get("role").is_none_or(|role| role == "assistant")
                && absent_or_empty_array(item, "content")
        }
        Some("reasoning") => {
            keys_only(item, &["type", "id", "status", "summary", "content"])
                && absent_or_empty_array(item, "summary")
                && absent_or_empty_array(item, "content")
        }
        _ => false,
    }
}

fn empty_part(part: &Value) -> bool {
    matches!(
        part["type"].as_str(),
        Some("output_text" | "summary_text" | "reasoning_text")
    ) && keys_only(part, &["type", "text", "annotations", "logprobs"])
        && part.get("text").is_some_and(|text| text == "")
        && absent_or_empty_array(part, "annotations")
        && absent_or_empty_array(part, "logprobs")
}

/// Only returns a closed category; upstream strings never become diagnostic values.
pub(crate) fn semantic_kind(payload: &Value) -> Option<&'static str> {
    let kind = payload["type"].as_str().unwrap_or_default();
    match kind {
        "response.created" | "response.in_progress"
            if payload.get("response").is_none_or(|response| {
                response.is_object() && absent_or_empty_array(response, "output")
            }) =>
        {
            None
        }
        "response.completed" | "response.done" => Some("other"),
        "response.output_item.added"
            if keys_only(
                payload,
                &[
                    "type",
                    "sequence_number",
                    "response_id",
                    "output_index",
                    "item",
                ],
            ) && empty_item(&payload["item"]) =>
        {
            None
        }
        "response.content_part.added" | "response.reasoning_summary_part.added"
            if keys_only(
                payload,
                &[
                    "type",
                    "sequence_number",
                    "response_id",
                    "item_id",
                    "output_index",
                    "content_index",
                    "summary_index",
                    "part",
                ],
            ) && empty_part(&payload["part"]) =>
        {
            None
        }
        "response.output_text.delta" => Some("text_delta"),
        "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
            Some("reasoning_delta")
        }
        "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
            Some("tool_call_delta")
        }
        "response.output_item.added" | "response.output_item.done" => {
            let added = kind == "response.output_item.added";
            Some(match (payload["item"]["type"].as_str(), added) {
                (Some("message"), true) => "message_added",
                (Some("message"), false) => "message_done",
                (Some("reasoning"), true) => "reasoning_added",
                (Some("reasoning"), false) => "reasoning_done",
                (
                    Some(
                        "function_call"
                        | "custom_tool_call"
                        | "tool_search_call"
                        | "tool_search_output"
                        | "additional_tools"
                        | "file_search_call"
                        | "program"
                        | "shell_call"
                        | "mcp_list_tools"
                        | "mcp_call"
                        | "mcp_approval_request",
                    ),
                    true,
                ) => "tool_item_added",
                (
                    Some(
                        "function_call"
                        | "custom_tool_call"
                        | "tool_search_call"
                        | "tool_search_output"
                        | "additional_tools"
                        | "file_search_call"
                        | "program"
                        | "shell_call"
                        | "mcp_list_tools"
                        | "mcp_call"
                        | "mcp_approval_request",
                    ),
                    false,
                ) => "tool_item_done",
                _ => "unknown_item",
            })
        }
        "response.content_part.added"
        | "response.content_part.done"
        | "response.reasoning_summary_part.added"
        | "response.reasoning_summary_part.done"
        | "response.output_text.done"
        | "response.reasoning_text.done"
        | "response.reasoning_summary_text.done"
        | "response.function_call_arguments.done"
        | "response.custom_tool_call_input.done" => Some("raw_responses_delta"),
        _ => Some("other"),
    }
}

#[cfg(test)]
#[path = "_tests/visibility.rs"]
mod tests;
