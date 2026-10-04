use super::*;

pub(super) fn restore_protocol_context_body(
    typed_body: Value,
    envelope: Option<&ProtocolContextEnvelope>,
) -> Result<Value> {
    Ok(restore_protocol_context_body_with_receipt(typed_body, envelope)?.body)
}

#[derive(Debug, Default, Serialize)]
pub(super) struct ProtocolBodyRestorationReceipt {
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) reconstructed_source_fields: BTreeSet<String>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) semantic_delta_fields: BTreeSet<String>,
    #[serde(skip_serializing_if = "is_false")]
    model_mapped: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl ProtocolBodyRestorationReceipt {
    fn is_empty(&self) -> bool {
        self.reconstructed_source_fields.is_empty()
            && self.semantic_delta_fields.is_empty()
            && !self.model_mapped
    }
}

pub(super) struct RestoredProtocolContextBody {
    pub(super) body: Value,
    pub(super) receipt: ProtocolBodyRestorationReceipt,
}

pub(super) fn restore_protocol_context_body_with_receipt(
    mut typed_body: Value,
    envelope: Option<&ProtocolContextEnvelope>,
) -> Result<RestoredProtocolContextBody> {
    let mut receipt = ProtocolBodyRestorationReceipt::default();
    let Some(envelope) = matching_protocol_context(envelope)? else {
        return Ok(RestoredProtocolContextBody {
            body: typed_body,
            receipt,
        });
    };
    let body = typed_body
        .as_object_mut()
        .context("typed Anthropic request body must be an object")?;
    for (name, value) in &envelope.body {
        if !protocol_context_field_is_safe(name) {
            bail!("protocol context contains a reserved body field");
        }
        if ANTHROPIC_TYPED_BODY_FIELDS.contains(&name.as_str()) || body.contains_key(name) {
            bail!("protocol context collides with a typed Anthropic body field");
        }
        validate_protocol_context_value(value)?;
        if name == "context_management" && !value.is_object() {
            bail!("protocol context context_management must be an object");
        }
        body.insert(name.clone(), value.clone());
    }

    if let Some(source_body) = envelope
        .source_request
        .as_ref()
        .and_then(|request| request.body.as_ref())
    {
        let source_body = source_body
            .as_object()
            .context("protocol context source request body must be an object")?;
        for name in source_body.keys() {
            if !protocol_context_field_is_safe(name) {
                bail!("protocol context source request body contains a reserved root field");
            }
        }
        if source_body.get("model") != body.get("model") {
            receipt.model_mapped = true;
        }
        // The Native ingress extracts every system message into the system blocks
        // and appends the latest user query after history. Restore the original
        // ordering only when that exact projection still matches. Inline system
        // messages require the joint system guard; an independent root-system
        // edit must remain intact while unchanged messages can still be restored.
        let message_restoration = restore_source_message_order(body, source_body);
        let restored_order = message_restoration.is_some();
        let restored_system = message_restoration == Some(true);
        if restored_order {
            receipt
                .reconstructed_source_fields
                .insert("messages".to_string());
            if restored_system && source_body.contains_key("system") {
                receipt
                    .reconstructed_source_fields
                    .insert("system".to_string());
            }
        }
        for (name, source_value) in source_body {
            // Messages have exactly one restoration owner: the closed Native
            // projection above. Generic content equivalence must never bypass
            // its newline, ordering, tool-query or per-turn effort guards.
            if name == "messages" {
                if !restored_order {
                    receipt.semantic_delta_fields.insert(name.clone());
                }
                continue;
            }
            if restored_system && name == "system" {
                continue;
            }
            if name == "model" {
                continue;
            }
            let Some(typed_value) = body.get(name) else {
                if ANTHROPIC_TYPED_BODY_FIELDS.contains(&name.as_str()) {
                    receipt.semantic_delta_fields.insert(name.clone());
                }
                continue;
            };
            if anthropic_source_field_matches_typed(name, source_value, typed_value) {
                body.insert(name.clone(), source_value.clone());
                receipt.reconstructed_source_fields.insert(name.clone());
            } else {
                receipt.semantic_delta_fields.insert(name.clone());
            }
        }
    }

    Ok(RestoredProtocolContextBody {
        body: typed_body,
        receipt,
    })
}

fn anthropic_source_field_matches_typed(name: &str, source: &Value, typed: &Value) -> bool {
    match name {
        "system" => anthropic_system_blocks(source, true).is_some_and(|source_blocks| {
            Some(source_blocks) == anthropic_system_blocks(typed, false)
        }),
        "tools" => canonical_anthropic_tools(source) == canonical_anthropic_tools(typed),
        "thinking" => canonical_anthropic_thinking(source) == canonical_anthropic_thinking(typed),
        _ => source == typed,
    }
}

// Native keeps root system object-block boundaries and text verbatim. Only
// source string shorthand is trimmed/dropped by Anthropic ingress. Cache hints
// can be restored independently; adjacent blocks must never be concatenated.
fn anthropic_system_blocks(value: &Value, source: bool) -> Option<Vec<Value>> {
    let values = match value {
        Value::String(_) => vec![value.clone()],
        Value::Array(values) => values.clone(),
        _ => return None,
    };
    let mut blocks = Vec::new();
    for value in values {
        let mut block = match value {
            Value::String(text) => {
                let text = if source { text.trim().to_owned() } else { text };
                json!({"type":"text", "text":text})
            }
            Value::Object(mut object) => {
                object.remove("cache_control");
                if object.get("type").and_then(Value::as_str) != Some("text") {
                    return None;
                }
                Value::Object(object)
            }
            _ => return None,
        };
        let text = block.get("text")?.as_str()?;
        if source && text.trim().is_empty() {
            continue;
        }
        if let Some(object) = block.as_object_mut() {
            object.remove("cache_control");
        }
        blocks.push(block);
    }
    Some(blocks)
}

// Display controls the source protocol presentation, while the typed reasoning
// mode/budget remain authoritative. Do not overwrite a changed mode or budget.
fn canonical_anthropic_thinking(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("display");
    }
    value
}

fn restore_source_message_order(
    typed: &mut Map<String, Value>,
    source: &Map<String, Value>,
) -> Option<bool> {
    let Some(messages) = source.get("messages").and_then(Value::as_array) else {
        return None;
    };
    // A restored per-turn effort must not shadow an intentional Native effort
    // change in the root output configuration.
    if messages
        .iter()
        .any(|message| message.get("output_config").is_some())
        && source.get("output_config") != typed.get("output_config")
    {
        return None;
    }
    let Some(last_user) = messages
        .iter()
        .rposition(|message| message["role"] == "user")
    else {
        return None;
    };
    let mut system = match source.get("system") {
        Some(value) => anthropic_system_blocks(value, true)?,
        None => Vec::new(),
    };
    let Some(projected) = native_source_messages(messages, last_user, &mut system) else {
        return None;
    };
    let expected_system = system;
    let typed_system = match typed.get("system") {
        Some(value) => anthropic_system_blocks(value, false)?,
        None => Vec::new(),
    };
    let Some(typed_messages) = typed.get("messages") else {
        return None;
    };
    let system_matches = expected_system == typed_system;
    let has_inline_system = messages.iter().any(|message| message["role"] == "system");
    if (has_inline_system && !system_matches)
        || projected_messages_without_cache_hints(&Value::Array(build_anthropic_messages(
            &projected,
        ))) != projected_messages_without_cache_hints(typed_messages)
    {
        return None;
    }
    typed.insert("messages".to_string(), Value::Array(messages.clone()));
    if system_matches {
        if let Some(system) = source.get("system") {
            typed.insert("system".to_string(), system.clone());
        } else {
            typed.remove("system");
        }
    }
    Some(system_matches)
}

// Mirror the closed Anthropic ingress -> Native prompt context transformation,
// then reuse the provider's output codec. Text joining, result-only query text,
// reasoning blocks and media precedence must match that transformation exactly;
// no content, ordering or whitespace is ignored to make the guard pass.
fn native_source_messages(
    messages: &[Value],
    last_user: usize,
    system: &mut Vec<Value>,
) -> Option<Vec<ProviderMessage>> {
    let mut projected = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if index == last_user {
            continue;
        }
        let role = message.get("role")?.as_str()?;
        let content = message.get("content")?;
        let text = source_history_text(content)?;
        if role == "system" {
            if !text.trim().is_empty() {
                system.push(json!({"type":"text", "text":text}));
            }
            continue;
        }
        if role == "assistant" {
            let mut provider = source_provider_message(ProviderMessageRole::Assistant, text);
            if let Some(blocks) = content.as_array() {
                if blocks.iter().any(|block| {
                    matches!(
                        block["type"].as_str(),
                        Some("thinking" | "redacted_thinking")
                    )
                }) {
                    let mut reasoning = Vec::new();
                    for block in blocks {
                        match block["type"].as_str()? {
                            "text" => {
                                let text = block["text"].as_str()?.trim();
                                if !text.is_empty() { reasoning.push(json!({"type":"text", "text":text})); }
                            }
                            "thinking" => {
                                let mut part = json!({"type":"reasoning", "text":block["thinking"].as_str()?});
                                if let Some(signature) = block.get("signature") { part["signature"] = signature.clone(); }
                                reasoning.push(part);
                            }
                            "redacted_thinking" => reasoning.push(json!({"type":"reasoning_redacted", "data":block["data"].as_str()?})),
                            _ => {}
                        }
                    }
                    provider.content_blocks = Some(Value::Array(reasoning));
                }
                let calls = blocks.iter().filter(|block| block["type"] == "tool_use")
                    .map(|block| json!({"id":block["id"], "name":block["name"], "arguments":block.get("input").cloned().unwrap_or_else(|| json!({}))}))
                    .collect::<Vec<_>>();
                if !calls.is_empty() {
                    provider.tool_calls = Some(Value::Array(calls));
                }
            }
            projected.push(provider);
        } else if role == "user" {
            let tools = source_tool_results(content)?;
            let has_tools = !tools.is_empty();
            projected.extend(tools);
            let media = source_media(content);
            if !text.trim().is_empty() || !media.is_empty() || !has_tools {
                let mut provider = source_provider_message(ProviderMessageRole::User, text);
                if !media.is_empty() {
                    provider.content_blocks = Some(Value::Array(media));
                }
                projected.push(provider);
            }
        } else {
            return None;
        }
    }
    let content = messages[last_user].get("content")?;
    projected.extend(source_tool_results(content)?);
    let media = source_media(content);
    if !media.is_empty() {
        let mut provider = source_provider_message(ProviderMessageRole::User, String::new());
        provider.content_blocks = Some(Value::Array(media));
        projected.push(provider);
    }
    let query = source_current_user_text(content)?;
    if !query.trim().is_empty() {
        projected.push(source_provider_message(ProviderMessageRole::User, query));
    }
    Some(projected)
}

fn source_provider_message(role: ProviderMessageRole, content: String) -> ProviderMessage {
    ProviderMessage {
        role,
        content,
        name: None,
        tool_call_id: None,
        is_error: None,
        tool_calls: None,
        content_blocks: None,
    }
}

fn source_history_text(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }
    Some(
        content
            .as_array()?
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn source_tool_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    if let Some(blocks) = content.as_array() {
        let text = blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if blocks.iter().all(|block| block["type"] == "text") || !text.trim().is_empty() {
            return text;
        }
        if blocks
            .iter()
            .any(|block| matches!(block["type"].as_str(), Some("image" | "document")))
        {
            return String::new();
        }
    }
    content.to_string()
}

fn source_tool_results(content: &Value) -> Option<Vec<ProviderMessage>> {
    let Some(blocks) = content.as_array() else {
        return Some(Vec::new());
    };
    let mut results = Vec::new();
    for block in blocks.iter().filter(|block| block["type"] == "tool_result") {
        let content = block.get("content").cloned().unwrap_or_else(|| json!(""));
        let mut provider =
            source_provider_message(ProviderMessageRole::Tool, source_tool_text(&content));
        provider.tool_call_id = Some(block["tool_use_id"].as_str()?.to_owned());
        provider.is_error = block.get("is_error").and_then(Value::as_bool);
        if let Some(parts) = content.as_array() {
            let normalized = parts
                .iter()
                .filter_map(|part| match part["type"].as_str() {
                    Some("text") => part["text"]
                        .as_str()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(|text| json!({"type":"text", "text":text})),
                    Some("image" | "document") => Some(part.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if !normalized.is_empty() {
                provider.content_blocks = Some(Value::Array(normalized));
            }
        }
        results.push(provider);
    }
    Some(results)
}

fn source_media(content: &Value) -> Vec<Value> {
    content
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|block| matches!(block["type"].as_str(), Some("image" | "document")))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn source_current_user_text(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }
    let blocks = content.as_array()?;
    let visible = blocks.iter().any(|block| {
        block["type"] == "text"
            && block["text"]
                .as_str()
                .is_some_and(|text| !text.trim().is_empty())
    });
    let mut text = String::new();
    for block in blocks {
        let is_text = block["type"] == "text";
        let part = match block["type"].as_str()? {
            "text" => block["text"].as_str()?.to_owned(),
            "tool_result" if !visible => block
                .get("content")
                .map(source_tool_text)
                .unwrap_or_default(),
            _ => continue,
        };
        if is_text || !part.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&part);
        }
    }
    Some(text)
}

// This is already provider wire on both sides, so no text normalization or
// regrouping is permitted. Only block cache hints are outside Native semantics.
fn projected_messages_without_cache_hints(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(messages) = value.as_array_mut() {
        for message in messages {
            if let Some(content) = message.get_mut("content") {
                remove_content_cache_hints(content);
            }
        }
    }
    value
}

fn remove_content_cache_hints(content: &mut Value) {
    let Some(blocks) = content.as_array_mut() else {
        return;
    };
    for block in blocks {
        let Some(object) = block.as_object_mut() else {
            continue;
        };
        object.remove("cache_control");
        if object.get("type").and_then(Value::as_str) == Some("tool_result") {
            if let Some(content) = object.get_mut("content") {
                remove_content_cache_hints(content);
            }
        }
    }
}

fn canonical_anthropic_tools(value: &Value) -> Value {
    let Value::Array(tools) = value else {
        return value.clone();
    };
    Value::Array(
        tools
            .iter()
            .map(|tool| {
                let Some(object) = tool.as_object() else {
                    return tool.clone();
                };
                let mut canonical = object.clone();
                canonical.remove("cache_control");
                canonical.remove("eager_input_streaming");
                Value::Object(canonical)
            })
            .collect(),
    )
}

pub(super) fn attach_matching_protocol_context_receipt(
    provider_metadata: &mut Value,
    envelope: Option<&ProtocolContextEnvelope>,
    receipt: &ProtocolBodyRestorationReceipt,
) -> Result<()> {
    if matching_protocol_context(envelope)?.is_none() || receipt.is_empty() {
        return Ok(());
    }
    let metadata = provider_metadata
        .as_object_mut()
        .context("Anthropic provider metadata must be an object")?;
    if metadata.contains_key("provider_request_translation") {
        bail!("Anthropic provider metadata contains reserved request translation receipt");
    }
    metadata.insert(
        "provider_request_translation".to_string(),
        serde_json::to_value(receipt).context("serializing protocol reconstruction receipt")?,
    );
    Ok(())
}

fn validate_protocol_context_value(value: &Value) -> Result<()> {
    match value {
        Value::Array(values) => {
            for value in values {
                validate_protocol_context_value(value)?;
            }
        }
        Value::Object(object) => {
            for (name, value) in object {
                if !protocol_context_field_is_safe(name) {
                    bail!("protocol context contains a nested reserved body field");
                }
                validate_protocol_context_value(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
#[path = "_tests/source_request_fidelity.rs"]
mod source_request_fidelity;
