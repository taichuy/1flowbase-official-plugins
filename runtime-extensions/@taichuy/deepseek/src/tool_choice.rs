use anyhow::{bail, Result};
use serde_json::{json, Value};

/// Lower the AI Native choice contract or a configured Chat Completions choice.
/// JSON text remains supported for provider parameters configured through the UI.
pub(super) fn to_chat_wire(value: &Value) -> Result<Value> {
    let decoded;
    let value = if let Some(text) = value.as_str() {
        let text = text.trim();
        if matches!(text, "auto" | "none" | "required") {
            return Ok(json!(text));
        }
        decoded = serde_json::from_str::<Value>(text)
            .map_err(|_| anyhow::anyhow!("tool_choice must be a supported choice or valid JSON"))?;
        &decoded
    } else {
        value
    };

    if let Some(choice) = value.as_str() {
        if matches!(choice, "auto" | "none" | "required") {
            return Ok(value.clone());
        }
    }
    if let Some(choice) = value.as_object() {
        match choice.get("type").and_then(Value::as_str) {
            Some(kind @ ("auto" | "none" | "required")) if choice.len() == 1 => {
                return Ok(json!(kind));
            }
            Some("tool") if choice.len() == 2 => {
                if let Some(name) = choice.get("name").and_then(nonempty_name) {
                    return Ok(json!({"type": "function", "function": {"name": name}}));
                }
            }
            Some("function") if choice.len() == 2 => {
                if let Some(function) = choice.get("function").and_then(Value::as_object) {
                    if function.len() == 1 && function.get("name").and_then(nonempty_name).is_some()
                    {
                        return Ok(value.clone());
                    }
                }
            }
            _ => {}
        }
    }
    bail!("tool_choice must be auto, none, required, a Native {{type: tool, name}} choice, or a Chat Completions {{type: function, function: {{name}}}} choice");
}

fn nonempty_name(value: &Value) -> Option<&str> {
    value.as_str().filter(|name| !name.trim().is_empty())
}
