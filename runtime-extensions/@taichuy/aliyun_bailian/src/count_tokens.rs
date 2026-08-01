use serde_json::{json, Map, Value};

const BYTES_PER_ESTIMATED_TOKEN: u64 = 4;

// The package has no licensed tokenizer asset with a pinned version and digest.
// Estimate the selected Bailian wire instead of presenting an unverified tokenizer as exact.

pub(crate) fn provider_estimate(wire_body: Value) -> Value {
    let mut unknown_block_count = 0;
    let observable_body = scrub_unknown_media(wire_body, &mut unknown_block_count);
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
        let result = provider_estimate(json!({
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
