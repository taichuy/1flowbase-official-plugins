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
        for (name, source_value) in source_body {
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
        "messages" => canonical_anthropic_messages(source) == canonical_anthropic_messages(typed),
        "system" => canonical_anthropic_content(source) == canonical_anthropic_content(typed),
        "tools" => canonical_anthropic_tools(source) == canonical_anthropic_tools(typed),
        _ => source == typed,
    }
}

fn canonical_anthropic_messages(value: &Value) -> Value {
    let Value::Array(messages) = value else {
        return value.clone();
    };
    Value::Array(
        messages
            .iter()
            .map(|message| {
                let Some(object) = message.as_object() else {
                    return message.clone();
                };
                let mut canonical = object.clone();
                if let Some(content) = object.get("content") {
                    canonical.insert("content".to_string(), canonical_anthropic_content(content));
                }
                Value::Object(canonical)
            })
            .collect(),
    )
}

fn canonical_anthropic_content(value: &Value) -> Value {
    let parts = match value {
        Value::String(text) => vec![json!({"type": "text", "text": text})],
        Value::Array(parts) => parts.clone(),
        _ => return value.clone(),
    };
    let mut canonical: Vec<Value> = Vec::new();
    for part in parts {
        let mut part = canonical_anthropic_block(&part);
        let text = part
            .get("type")
            .and_then(Value::as_str)
            .filter(|kind| *kind == "text")
            .and_then(|_| part.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(text) = text {
            if let Some(previous) = canonical.last_mut().and_then(Value::as_object_mut) {
                if previous.get("type").and_then(Value::as_str) == Some("text") {
                    let previous_text = previous
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    previous.insert(
                        "text".to_string(),
                        Value::String(format!("{previous_text}{text}")),
                    );
                    continue;
                }
            }
        }
        if let Some(object) = part.as_object_mut() {
            object.remove("cache_control");
        }
        canonical.push(part);
    }
    Value::Array(canonical)
}

fn canonical_anthropic_block(value: &Value) -> Value {
    let Value::Object(object) = value else {
        return value.clone();
    };
    let mut canonical = object.clone();
    canonical.remove("cache_control");
    if canonical.get("type").and_then(Value::as_str) == Some("tool_result") {
        if let Some(content) = canonical.get("content").cloned() {
            canonical.insert("content".to_string(), canonical_anthropic_content(&content));
        }
    }
    Value::Object(canonical)
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
