use super::*;

#[derive(Clone, Default)]
pub(super) struct ToolUseBuilder {
    id: String,
    name: String,
    input_json: String,
}

impl ToolUseBuilder {
    pub(super) fn into_tool_call(self) -> Result<ProviderToolCall> {
        let arguments = if self.input_json.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&self.input_json)
                .context("Anthropic tool input JSON was incomplete")?
        };
        Ok(ProviderToolCall {
            id: self.id,
            name: self.name,
            arguments,
        })
    }
}

pub(super) struct StreamBlock {
    item: Value,
    text: String,
    closed: bool,
}

pub(super) struct AnthropicStreamParseState<'a> {
    pub(super) text: &'a mut String,
    pub(super) tool_builders: &'a mut BTreeMap<usize, ToolUseBuilder>,
    pub(super) blocks: &'a mut BTreeMap<usize, StreamBlock>,
    pub(super) usage: &'a mut ProviderUsage,
    pub(super) finish_reason: &'a mut ProviderFinishReason,
    pub(super) message_id: &'a mut Value,
    pub(super) saw_message_stop: &'a mut bool,
}

fn output_item(phase: ProviderOutputItemPhase, index: usize, item: Value) -> ProviderStreamEvent {
    ProviderStreamEvent::OutputItem {
        phase,
        output_index: index,
        item,
    }
}

fn close_block(
    index: usize,
    blocks: &mut BTreeMap<usize, StreamBlock>,
    tool_builders: &BTreeMap<usize, ToolUseBuilder>,
    events: &mut Vec<ProviderStreamEvent>,
) -> Result<()> {
    let Some(block) = blocks.get_mut(&index).filter(|block| !block.closed) else {
        return Ok(());
    };
    match block.item["type"].as_str() {
        Some("message") => {
            block.item["content"] = json!([{"type":"output_text","text":block.text}]);
        }
        Some("reasoning") => {
            block.item["summary"] = json!([{"type":"summary_text","text":block.text}]);
        }
        Some("function_call") => {
            if let Some(builder) = tool_builders.get(&index) {
                let call = builder.clone().into_tool_call()?;
                block.item["arguments"] = Value::String(if builder.input_json.trim().is_empty() {
                    call.arguments.to_string()
                } else {
                    builder.input_json.clone()
                });
            }
        }
        _ => {}
    }
    block.item["status"] = json!("completed");
    events.push(output_item(
        ProviderOutputItemPhase::Done,
        index,
        block.item.clone(),
    ));
    block.closed = true;
    Ok(())
}

pub(super) fn process_anthropic_sse_data(
    data: &str,
    events: &mut Vec<ProviderStreamEvent>,
    state: AnthropicStreamParseState<'_>,
) -> Result<()> {
    let AnthropicStreamParseState {
        text,
        tool_builders,
        blocks,
        usage,
        finish_reason,
        message_id,
        saw_message_stop,
    } = state;
    if data.is_empty() || data == "[DONE]" || *saw_message_stop {
        return Ok(());
    }
    let payload: Value = serde_json::from_str(data)?;
    let index = payload.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    match payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "message_start" => {
            if let Some(message) = payload.get("message") {
                if let Some(id) = message.get("id") {
                    *message_id = id.clone();
                }
                *usage = normalize_usage(message.get("usage").unwrap_or(&Value::Null));
            }
        }
        "content_block_start" => {
            // A supplier index identifies one block for the lifetime of this message.
            if blocks.contains_key(&index) {
                return Ok(());
            }
            let Some(block) = payload.get("content_block") else {
                return Ok(());
            };
            let id = format!(
                "{}_block_{index}",
                message_id.as_str().unwrap_or("anthropic")
            );
            let (item, initial_text) = match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let builder = ToolUseBuilder {
                        id: block.get("id").map(value_to_string).unwrap_or_default(),
                        name: block.get("name").map(value_to_string).unwrap_or_default(),
                        input_json: block
                            .get("input")
                            .filter(|value| !is_empty_tool_input(value))
                            .map(Value::to_string)
                            .unwrap_or_default(),
                    };
                    let item = json!({"id":builder.id,"type":"function_call","call_id":builder.id,
                        "name":builder.name,"arguments":"","status":"in_progress"});
                    tool_builders.insert(index, builder);
                    (item, String::new())
                }
                Some("text") => (
                    json!({"id":id,"type":"message","role":"assistant",
                    "content":[],"status":"in_progress"}),
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
                Some("thinking") => (
                    json!({"id":id,"type":"reasoning","summary":[],
                    "status":"in_progress"}),
                    block
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
                _ => return Ok(()),
            };
            events.push(output_item(
                ProviderOutputItemPhase::Added,
                index,
                item.clone(),
            ));
            if let Some(builder) = tool_builders
                .get(&index)
                .filter(|builder| !builder.input_json.is_empty())
            {
                events.push(ProviderStreamEvent::ToolCallDelta {
                    call_id: builder.id.clone(),
                    delta: Value::String(builder.input_json.clone()),
                });
            }
            // Initial nonempty content is supplier output, just like subsequent deltas.
            if !initial_text.is_empty() {
                if item["type"] == "message" {
                    text.push_str(&initial_text);
                    events.push(ProviderStreamEvent::TextDelta {
                        delta: initial_text.clone(),
                    });
                } else {
                    events.push(ProviderStreamEvent::ReasoningDelta {
                        delta: initial_text.clone(),
                    });
                }
            }
            if let Some(signature) = block
                .get("signature")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                events.push(ProviderStreamEvent::ReasoningSignatureDelta {
                    signature: signature.into(),
                });
            }
            blocks.insert(
                index,
                StreamBlock {
                    item,
                    text: initial_text,
                    closed: false,
                },
            );
        }
        "content_block_delta" => {
            if blocks.get(&index).is_some_and(|block| block.closed) {
                return Ok(());
            }
            let delta = payload.get("delta").unwrap_or(&Value::Null);
            match delta
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "text_delta" => {
                    if let Some(value) = delta.get("text").and_then(Value::as_str) {
                        text.push_str(value);
                        if let Some(block) = blocks.get_mut(&index) {
                            block.text.push_str(value);
                        }
                        events.push(ProviderStreamEvent::TextDelta {
                            delta: value.to_string(),
                        });
                    }
                }
                "input_json_delta" => {
                    if let Some(value) = delta.get("partial_json").and_then(Value::as_str) {
                        if let Some(builder) = tool_builders.get_mut(&index) {
                            builder.input_json.push_str(value);
                            events.push(ProviderStreamEvent::ToolCallDelta {
                                call_id: builder.id.clone(),
                                delta: Value::String(value.to_string()),
                            });
                        }
                    }
                }
                "thinking_delta" => {
                    if let Some(value) = delta.get("thinking").and_then(Value::as_str) {
                        if let Some(block) = blocks.get_mut(&index) {
                            block.text.push_str(value);
                        }
                        events.push(ProviderStreamEvent::ReasoningDelta {
                            delta: value.to_string(),
                        });
                    }
                }
                "signature_delta" => {
                    if let Some(value) = delta.get("signature").and_then(Value::as_str) {
                        events.push(ProviderStreamEvent::ReasoningSignatureDelta {
                            signature: value.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        "content_block_stop" => close_block(index, blocks, tool_builders, events)?,
        "message_delta" => {
            if let Some(reason) = payload
                .get("delta")
                .and_then(|delta| delta.get("stop_reason"))
                .and_then(Value::as_str)
            {
                *finish_reason = normalize_stop_reason(reason);
            }
            if let Some(snapshot) = payload.get("usage") {
                merge_usage(usage, normalize_usage(snapshot));
            }
        }
        "message_stop" => {
            // Explicit message completion also closes blocks from older compatible suppliers
            // that omit individual stop frames. No completion is inferred from timing.
            for index in blocks.keys().copied().collect::<Vec<_>>() {
                close_block(index, blocks, tool_builders, events)?;
            }
            *saw_message_stop = true;
        }
        "error" => {
            return Err(
                upstream_error::from_parts(None, &HeaderMap::new(), data.to_string()).into(),
            )
        }
        _ => {}
    }
    Ok(())
}
