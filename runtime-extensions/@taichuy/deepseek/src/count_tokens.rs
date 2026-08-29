use std::sync::OnceLock;

use serde_json::{json, Map, Value};
use tokenizers::Tokenizer;

const BYTES_PER_ESTIMATED_TOKEN: u64 = 4;
const BEGIN_OF_SENTENCE: &str = "<｜begin▁of▁sentence｜>";
const END_OF_SENTENCE: &str = "<｜end▁of▁sentence｜>";
const USER: &str = "<｜User｜>";
const ASSISTANT: &str = "<｜Assistant｜>";
const TOOL_CALLS_BEGIN: &str = "<｜tool▁calls▁begin｜>";
const TOOL_CALL_BEGIN: &str = "<｜tool▁call▁begin｜>";
const TOOL_SEPARATOR: &str = "<｜tool▁sep｜>";
const TOOL_CALL_END: &str = "<｜tool▁call▁end｜>";
const TOOL_CALLS_END: &str = "<｜tool▁calls▁end｜>";
const TOOL_OUTPUTS_BEGIN: &str = "<｜tool▁outputs▁begin｜>";
const TOOL_OUTPUT_BEGIN: &str = "<｜tool▁output▁begin｜>";
const TOOL_OUTPUT_END: &str = "<｜tool▁output▁end｜>";
const TOOL_OUTPUTS_END: &str = "<｜tool▁outputs▁end｜>";
const TOKENIZER_JSON: &[u8] = include_bytes!("assets/deepseek_v4_tokenizer.json");

static DEEPSEEK_V4_TOKENIZER: OnceLock<Result<Tokenizer, String>> = OnceLock::new();

pub(crate) fn count(wire_body: Value) -> Value {
    let mut unknown_block_count = 0;
    let observable_body = scrub_unknown_media(wire_body, &mut unknown_block_count);
    if unknown_block_count > 0 {
        return provider_estimate(observable_body, unknown_block_count);
    }

    let Some(rendered_prompt) = render_prompt(&observable_body) else {
        return provider_estimate(observable_body, 0);
    };
    let tokenizer = DEEPSEEK_V4_TOKENIZER
        .get_or_init(|| Tokenizer::from_bytes(TOKENIZER_JSON).map_err(|error| error.to_string()));
    let Ok(tokenizer) = tokenizer else {
        return provider_estimate(observable_body, 0);
    };
    let Ok(encoding) = tokenizer.encode(rendered_prompt, false) else {
        return provider_estimate(observable_body, 0);
    };

    let unmodeled_tool_definition_count = observable_body
        .get("tools")
        .and_then(Value::as_array)
        .map_or(0, Vec::len) as u64;
    json!({
        "operation": "count_tokens",
        "input_tokens": encoding.len() as u64,
        "method": "deepseek_v4_tokenizer",
        "coverage": if unmodeled_tool_definition_count == 0 { "complete" } else { "partial" },
        "unknown_block_count": 0,
        "unmodeled_tool_definition_count": unmodeled_tool_definition_count,
    })
}

fn render_prompt(wire_body: &Value) -> Option<String> {
    let messages = wire_body.get("messages")?.as_array()?;
    let system_prompt = messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .filter_map(|message| message_content(message.get("content")?))
        .collect::<Vec<_>>()
        .join("\n\n");

    let mut prompt = String::from(BEGIN_OF_SENTENCE);
    prompt.push_str(&system_prompt);
    let mut in_tool_outputs = false;
    let mut wrote_tool_call = false;
    let mut wrote_tool_output = false;

    for message in messages {
        let role = message.get("role").and_then(Value::as_str)?;
        if role == "system" {
            continue;
        }
        match role {
            "user" => {
                in_tool_outputs = false;
                prompt.push_str(USER);
                prompt.push_str(&message_content(message.get("content")?)?);
            }
            "assistant" if message.get("content").is_none_or(Value::is_null) => {
                in_tool_outputs = false;
                let tool_calls = message.get("tool_calls")?.as_array()?;
                for tool_call in tool_calls {
                    let subsequent_tool_call = wrote_tool_call;
                    if !subsequent_tool_call {
                        prompt.push_str(ASSISTANT);
                        prompt.push_str(TOOL_CALLS_BEGIN);
                    } else {
                        prompt.push('\n');
                    }
                    prompt.push_str(TOOL_CALL_BEGIN);
                    prompt.push_str(
                        tool_call
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("function"),
                    );
                    prompt.push_str(TOOL_SEPARATOR);
                    let function = tool_call.get("function")?;
                    prompt.push_str(function.get("name")?.as_str()?);
                    prompt.push('\n');
                    prompt.push_str("```json\n");
                    prompt.push_str(&json_text(function.get("arguments")?));
                    prompt.push_str("\n```");
                    prompt.push_str(TOOL_CALL_END);
                    wrote_tool_call = true;
                    if subsequent_tool_call {
                        prompt.push_str(TOOL_CALLS_END);
                        prompt.push_str(END_OF_SENTENCE);
                    }
                }
            }
            "assistant" => {
                let content = message_content(message.get("content")?)?;
                if in_tool_outputs {
                    prompt.push_str(TOOL_OUTPUTS_END);
                    prompt.push_str(&content);
                } else {
                    prompt.push_str(ASSISTANT);
                    prompt.push_str(&content);
                }
                prompt.push_str(END_OF_SENTENCE);
                in_tool_outputs = false;
            }
            "tool" => {
                in_tool_outputs = true;
                if !wrote_tool_output {
                    prompt.push_str(TOOL_OUTPUTS_BEGIN);
                } else {
                    prompt.push('\n');
                }
                prompt.push_str(TOOL_OUTPUT_BEGIN);
                prompt.push_str(&message_content(message.get("content")?)?);
                prompt.push_str(TOOL_OUTPUT_END);
                wrote_tool_output = true;
            }
            _ => return None,
        }
    }
    if in_tool_outputs {
        prompt.push_str(TOOL_OUTPUTS_END);
    } else {
        prompt.push_str(ASSISTANT);
    }
    Some(prompt)
}

fn message_content(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => Some(
            blocks
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .or_else(|| block.get("content").and_then(Value::as_str))
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Value::Null => None,
        other => Some(json_text(other)),
    }
}

fn json_text(value: &Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default())
}

fn provider_estimate(observable_body: Value, unknown_block_count: u64) -> Value {
    let byte_count = serde_json::to_vec(&observable_body)
        .map(|bytes| bytes.len() as u64)
        .unwrap_or(0);
    let input_tokens =
        byte_count.saturating_add(BYTES_PER_ESTIMATED_TOKEN - 1) / BYTES_PER_ESTIMATED_TOKEN;
    json!({
        "operation": "count_tokens",
        "input_tokens": input_tokens,
        "method": "provider_estimate",
        "coverage": if unknown_block_count == 0 { "complete" } else { "partial" },
        "unknown_block_count": unknown_block_count,
    })
}

fn scrub_unknown_media(value: Value, unknown_block_count: &mut u64) -> Value {
    match value {
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| scrub_unknown_media(value, unknown_block_count))
                .collect(),
        ),
        Value::Object(object) if is_unknown_media_block(&object) => {
            *unknown_block_count = unknown_block_count.saturating_add(1);
            json!({ "type": "unknown_media" })
        }
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, scrub_unknown_media(value, unknown_block_count)))
                .collect(),
        ),
        other => other,
    }
}

fn is_unknown_media_block(object: &Map<String, Value>) -> bool {
    const MEDIA_KEYS: &[&str] = &[
        "image_url",
        "input_audio",
        "inline_data",
        "file_data",
        "file_id",
        "source",
    ];
    if MEDIA_KEYS.iter().any(|key| object.contains_key(*key)) {
        return true;
    }
    object
        .get("type")
        .and_then(Value::as_str)
        .map(|kind| {
            let kind = kind.to_ascii_lowercase();
            ["image", "audio", "video", "file", "document"]
                .iter()
                .any(|media| kind.contains(media))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_is_total_but_observable_as_partial() {
        let result = count(json!({
            "messages": [{"role":"user","content":[
                {"type":"text","text":"hello"},
                {"type":"image_url","image_url":{"url":"data:image/png;base64,AAAA"}}
            ]}]
        }));
        assert_eq!(result["operation"], "count_tokens");
        assert_eq!(result["method"], "provider_estimate");
        assert_eq!(result["coverage"], "partial");
        assert_eq!(result["unknown_block_count"], 1);
        assert!(result["input_tokens"].as_u64().is_some());
    }
}
