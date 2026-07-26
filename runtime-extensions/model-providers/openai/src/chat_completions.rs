use super::*;

const PASSTHROUGH_PARAMETERS: &[&str] = &[
    "temperature",
    "top_p",
    "tool_choice",
    "store",
    "parallel_tool_calls",
    "service_tier",
    "metadata",
    "reasoning_effort",
];

pub(super) async fn invoke<F>(
    config: &ProviderConfig,
    input: &ProviderInvocationInput,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let body = build_body(input)?;
    let response = build_http_client(config)?
        .request(Method::POST, build_url(config, "/chat/completions")?)
        .headers(build_stream_headers(
            config,
            input.client_protocol_envelope.as_ref(),
        )?)
        .json(&body)
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, config))?;
    read_stream(response, input.model.clone(), on_event).await
}

fn build_body(input: &ProviderInvocationInput) -> Result<Value> {
    input.ensure_generate_operation()?;
    if input.native_transport.is_some() {
        bail!("OpenAI Chat Completions does not support native Responses transport");
    }
    if input.previous_response_id.is_some() {
        bail!("OpenAI Chat Completions does not support previous_response_id");
    }
    if !input.required_capabilities.is_empty()
        || input.system.iter().any(|block| {
            matches!(
                block,
                NativePromptBlock::Text {
                    cache_control: Some(_),
                    ..
                }
            )
        })
        || input.request_context.end_user_reference.is_some()
    {
        bail!("OpenAI Chat Completions does not support the requested semantic capabilities");
    }
    let model = input.model.trim();
    if model.is_empty() {
        bail!("model is required");
    }

    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model.to_string()));
    body.insert("messages".to_string(), Value::Array(build_messages(input)));
    body.insert("stream".to_string(), Value::Bool(true));
    body.insert(
        "stream_options".to_string(),
        json!({ "include_usage": true }),
    );
    if !input.tools.is_empty() {
        body.insert("tools".to_string(), Value::Array(input.tools.clone()));
    }
    if let Some(response_format) = input
        .response_format
        .clone()
        .and_then(normalize_response_text_format)
        .or_else(|| {
            parameter_value(input, "response_format").and_then(normalize_response_text_format)
        })
    {
        body.insert("response_format".to_string(), response_format);
    }
    if let Some(max_output_tokens) = parameter_value(input, "max_output_tokens") {
        body.insert("max_completion_tokens".to_string(), max_output_tokens);
    }
    for key in PASSTHROUGH_PARAMETERS {
        if let Some(value) = parameter_value(input, key) {
            body.insert((*key).to_string(), value);
        }
    }
    Ok(Value::Object(body))
}

fn build_messages(input: &ProviderInvocationInput) -> Vec<Value> {
    let mut messages = Vec::new();
    if let Some(system) = input.system_text() {
        messages.push(json!({ "role": "developer", "content": system }));
    }
    for message in &input.messages {
        let mut item = Map::new();
        item.insert(
            "role".to_string(),
            Value::String(chat_role(message.role).to_string()),
        );
        item.insert("content".to_string(), chat_content(message));
        if let Some(name) = message
            .name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            item.insert("name".to_string(), Value::String(name.to_string()));
        }
        if let Some(tool_call_id) = message
            .tool_call_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            item.insert(
                "tool_call_id".to_string(),
                Value::String(tool_call_id.to_string()),
            );
        }
        if let Some(tool_calls) = chat_tool_calls(message.tool_calls.as_ref()) {
            item.insert("tool_calls".to_string(), Value::Array(tool_calls));
        }
        messages.push(Value::Object(item));
    }
    messages
}

fn chat_role(role: ProviderMessageRole) -> &'static str {
    match role {
        ProviderMessageRole::System => "developer",
        ProviderMessageRole::User => "user",
        ProviderMessageRole::Assistant => "assistant",
        ProviderMessageRole::Tool => "tool",
    }
}

fn chat_content(message: &ProviderMessage) -> Value {
    let Some(content) = message.content_blocks.as_ref() else {
        return Value::String(message.content.clone());
    };
    let parts = responses_content_items_from_value(content)
        .into_iter()
        .filter_map(|part| match part.get("type").and_then(Value::as_str) {
            Some("input_text") => Some(json!({
                "type": "text",
                "text": part.get("text").cloned().unwrap_or(Value::Null),
            })),
            Some("input_image") => Some(json!({
                "type": "image_url",
                "image_url": {
                    "url": part.get("image_url").cloned().unwrap_or(Value::Null),
                    "detail": part.get("detail").cloned().unwrap_or(json!("auto")),
                },
            })),
            _ => None,
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        Value::String(message.content.clone())
    } else {
        Value::Array(parts)
    }
}

fn chat_tool_calls(raw: Option<&Value>) -> Option<Vec<Value>> {
    let calls = raw.and_then(Value::as_array)?;
    Some(
        calls
            .iter()
            .enumerate()
            .filter_map(|(index, call)| {
                if call.get("function").is_some() {
                    return Some(call.clone());
                }
                let name = call.get("name").and_then(Value::as_str)?;
                let id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("call_{index}"));
                let arguments = call
                    .get("arguments")
                    .map(response_tool_arguments)
                    .unwrap_or_else(|| "{}".to_string());
                Some(json!({
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": arguments },
                }))
            })
            .collect(),
    )
}

#[derive(Debug, Default)]
struct ChatToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl ChatToolCallBuilder {
    fn finish(self, index: usize) -> Result<ProviderToolCall> {
        let id = self
            .id
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("Chat Completions tool call {index} is missing id"))?;
        let name = self
            .name
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                anyhow!("Chat Completions tool call {index} is missing function.name")
            })?;
        let arguments = if self.arguments.is_empty() {
            json!({})
        } else {
            serde_json::from_str(&self.arguments).with_context(|| {
                format!("Chat Completions tool call {id} returned invalid JSON arguments")
            })?
        };
        Ok(ProviderToolCall {
            id,
            name,
            arguments,
        })
    }
}

#[derive(Debug, Default)]
struct ChatStreamState {
    events: Vec<ProviderStreamEvent>,
    text: String,
    tool_calls: Vec<ChatToolCallBuilder>,
    usage: ProviderUsage,
    finish_reason: Option<ProviderFinishReason>,
    response_id: Option<String>,
    response_model: Option<String>,
    created: Option<u64>,
    system_fingerprint: Option<String>,
}

async fn read_stream<F>(
    response: reqwest::Response,
    request_model: String,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let status = response.status();
    if !status.is_success() {
        return Err(provider_upstream_error_from_response(response)
            .await?
            .into());
    }
    let headers = response.headers().clone();
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut state = ChatStreamState::default();

    while let Some(chunk) = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next())
        .await
        .context("idle timeout waiting for Chat Completions SSE")?
    {
        let chunk = chunk?;
        buffer.extend_from_slice(&chunk);
        while let Some((index, delimiter_len)) = find_sse_event_boundary_bytes(&buffer) {
            let block = String::from_utf8(buffer[..index].to_vec())
                .context("OpenAI returned non-UTF-8 Chat Completions SSE data")?;
            buffer.drain(..index + delimiter_len);
            process_sse_block(&block, &mut state, on_event)?;
        }
    }
    if !buffer.is_empty() {
        let remaining = String::from_utf8(buffer)
            .context("OpenAI returned non-UTF-8 trailing Chat Completions SSE data")?;
        if !remaining.trim().is_empty() {
            process_sse_block(&remaining, &mut state, on_event)?;
        }
    }

    finalize_stream(state, request_model, &headers, on_event)
}

fn find_sse_event_boundary_bytes(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(lf), Some(crlf)) if lf < crlf => Some((lf, 2)),
        (Some(_), Some(crlf)) => Some((crlf, 4)),
        (Some(lf), None) => Some((lf, 2)),
        (None, Some(crlf)) => Some((crlf, 4)),
        (None, None) => None,
    }
}

fn finalize_stream<F>(
    mut state: ChatStreamState,
    request_model: String,
    headers: &HeaderMap,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let finish_reason = state
        .finish_reason
        .clone()
        .ok_or_else(|| anyhow!("Chat Completions stream closed before a terminal finish_reason"))?;
    let response_id = state
        .response_id
        .clone()
        .ok_or_else(|| anyhow!("Chat Completions stream did not provide a response id"))?;
    let tool_calls = state
        .tool_calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| call.finish(index))
        .collect::<Result<Vec<_>>>()?;
    if finish_reason == ProviderFinishReason::ToolCall && tool_calls.is_empty() {
        bail!("Chat Completions finished with tool_calls but emitted no tool call");
    }

    let mut terminal_events = Vec::new();
    if state.usage.has_any_value() {
        terminal_events.push(ProviderStreamEvent::UsageSnapshot {
            usage: state.usage.clone(),
        });
    }
    for call in &tool_calls {
        terminal_events.push(ProviderStreamEvent::ToolCallCommit { call: call.clone() });
    }
    terminal_events.push(ProviderStreamEvent::Finish {
        reason: finish_reason.clone(),
    });
    emit_new_events(&terminal_events, on_event)?;
    state.events.extend(terminal_events);

    Ok(RuntimeInvocationEnvelope {
        events: state.events,
        result: ProviderInvocationResult {
            final_content: (!state.text.is_empty()).then_some(state.text),
            response_id: Some(response_id.clone()),
            tool_calls,
            mcp_calls: Vec::new(),
            usage: state.usage,
            finish_reason: Some(finish_reason),
            provider_metadata: json!({
                "request_model": request_model,
                "transport": "chat_completions_sse",
                "response_id": response_id,
                "response_model": state.response_model,
                "created": state.created,
                "system_fingerprint": state.system_fingerprint,
                "upstream_request_id": header_text(headers, "x-request-id"),
            }),
        },
    })
}

fn process_sse_block<F>(block: &str, state: &mut ChatStreamState, on_event: &mut F) -> Result<()>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let Some(data) = response_sse_block_data(block) else {
        return Ok(());
    };
    let data = data.trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let payload: Value = serde_json::from_str(data)
        .with_context(|| "OpenAI returned invalid Chat Completions SSE JSON")?;
    let event_start = state.events.len();
    process_payload(&payload, state)?;
    emit_new_events(&state.events[event_start..], on_event)
}

fn process_payload(payload: &Value, state: &mut ChatStreamState) -> Result<()> {
    let object = payload
        .as_object()
        .ok_or_else(|| anyhow!("Chat Completions SSE payload must be an object"))?;
    if let Some(error) = object.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Chat Completions stream returned an error");
        bail!("{message}");
    }
    if let Some(kind) = object.get("object").and_then(Value::as_str) {
        if kind != "chat.completion.chunk" {
            bail!("unknown Chat Completions SSE object: {kind}");
        }
    }
    capture_stable_text(object.get("id"), "id", &mut state.response_id)?;
    capture_stable_text(object.get("model"), "model", &mut state.response_model)?;
    capture_stable_u64(object.get("created"), "created", &mut state.created)?;
    capture_stable_text(
        object.get("system_fingerprint"),
        "system_fingerprint",
        &mut state.system_fingerprint,
    )?;
    if let Some(usage) = object.get("usage").filter(|value| !value.is_null()) {
        state.usage = normalize_chat_usage(usage)?;
    }

    let Some(choices) = object.get("choices") else {
        if object.get("usage").is_some() {
            return Ok(());
        }
        bail!("Chat Completions SSE payload is missing choices");
    };
    let choices = choices
        .as_array()
        .ok_or_else(|| anyhow!("Chat Completions SSE choices must be an array"))?;
    if choices.is_empty() {
        if object.get("usage").is_some() {
            return Ok(());
        }
        bail!("Chat Completions SSE choices cannot be empty without usage");
    }
    if choices.len() != 1 {
        bail!("Chat Completions SSE must contain exactly one choice");
    }
    let choice = choices[0]
        .as_object()
        .ok_or_else(|| anyhow!("Chat Completions SSE choice must be an object"))?;
    if choice
        .get("index")
        .and_then(Value::as_u64)
        .is_some_and(|index| index != 0)
    {
        bail!("Chat Completions SSE choice index must be 0");
    }

    let terminal_was_seen = state.finish_reason.is_some();
    if let Some(reason) = choice.get("finish_reason").filter(|value| !value.is_null()) {
        let reason = reason
            .as_str()
            .ok_or_else(|| anyhow!("Chat Completions finish_reason must be a string"))?;
        let mapped = map_finish_reason(reason)?;
        if let Some(previous) = state.finish_reason.as_ref() {
            if previous != &mapped {
                bail!("Chat Completions emitted conflicting terminal finish_reason values");
            }
        }
        state.finish_reason = Some(mapped);
    }

    let Some(delta) = choice.get("delta") else {
        if choice.get("finish_reason").is_some() {
            return Ok(());
        }
        bail!("Chat Completions choice is missing delta");
    };
    let delta = delta
        .as_object()
        .ok_or_else(|| anyhow!("Chat Completions choice delta must be an object"))?;
    let has_semantic_delta = delta.keys().any(|key| {
        matches!(
            key.as_str(),
            "content"
                | "refusal"
                | "reasoning_content"
                | "reasoning"
                | "reasoning_delta"
                | "tool_calls"
        )
    });
    if terminal_was_seen && has_semantic_delta {
        bail!("Chat Completions emitted semantic delta after terminal finish_reason");
    }
    if let Some(role) = delta.get("role").filter(|value| !value.is_null()) {
        let role = role
            .as_str()
            .ok_or_else(|| anyhow!("Chat Completions delta role must be text"))?;
        if role != "assistant" {
            bail!("unexpected Chat Completions delta role: {role}");
        }
    }
    for key in delta.keys() {
        if !matches!(
            key.as_str(),
            "role"
                | "content"
                | "refusal"
                | "reasoning_content"
                | "reasoning"
                | "reasoning_delta"
                | "tool_calls"
        ) {
            bail!("unknown Chat Completions semantic delta field: {key}");
        }
    }
    append_text_delta(delta.get("content"), &mut state.text, &mut state.events)?;
    append_text_delta(delta.get("refusal"), &mut state.text, &mut state.events)?;
    append_reasoning_delta(delta, &mut state.events)?;
    merge_tool_call_deltas(delta.get("tool_calls"), state)?;
    Ok(())
}

fn append_text_delta(
    value: Option<&Value>,
    text: &mut String,
    events: &mut Vec<ProviderStreamEvent>,
) -> Result<()> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let delta = value
        .as_str()
        .ok_or_else(|| anyhow!("Chat Completions text delta must be a string"))?;
    if !delta.is_empty() {
        text.push_str(delta);
        events.push(ProviderStreamEvent::TextDelta {
            delta: delta.to_string(),
        });
    }
    Ok(())
}

fn append_reasoning_delta(
    delta: &Map<String, Value>,
    events: &mut Vec<ProviderStreamEvent>,
) -> Result<()> {
    let Some(value) = delta
        .get("reasoning_content")
        .or_else(|| delta.get("reasoning"))
        .or_else(|| delta.get("reasoning_delta"))
        .filter(|value| !value.is_null())
    else {
        return Ok(());
    };
    let reasoning = value
        .as_str()
        .ok_or_else(|| anyhow!("Chat Completions reasoning delta must be a string"))?;
    if !reasoning.is_empty() {
        events.push(ProviderStreamEvent::ReasoningDelta {
            delta: reasoning.to_string(),
        });
    }
    Ok(())
}

fn merge_tool_call_deltas(raw: Option<&Value>, state: &mut ChatStreamState) -> Result<()> {
    let Some(raw) = raw.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let calls = raw
        .as_array()
        .ok_or_else(|| anyhow!("Chat Completions tool_calls delta must be an array"))?;
    if calls.is_empty() {
        bail!("Chat Completions tool_calls delta cannot be empty");
    }
    for call in calls {
        let call = call
            .as_object()
            .ok_or_else(|| anyhow!("Chat Completions tool call delta must be an object"))?;
        let index = call
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("Chat Completions tool call delta is missing index"))?
            as usize;
        for key in call.keys() {
            if !matches!(key.as_str(), "index" | "id" | "type" | "function") {
                bail!("unknown Chat Completions tool call delta field: {key}");
            }
        }
        if let Some(call_type) = call.get("type").filter(|value| !value.is_null()) {
            if call_type.as_str() != Some("function") {
                bail!("Chat Completions tool call delta type must be function");
            }
        }
        while state.tool_calls.len() <= index {
            state.tool_calls.push(ChatToolCallBuilder::default());
        }
        let builder = &mut state.tool_calls[index];
        if let Some(id) = call.get("id").filter(|value| !value.is_null()) {
            let id = id
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("Chat Completions tool call id must be non-empty text"))?;
            if builder.id.as_deref().is_some_and(|previous| previous != id) {
                bail!("Chat Completions tool call {index} changed id");
            }
            builder.id = Some(id.to_string());
        }
        if let Some(function) = call.get("function").filter(|value| !value.is_null()) {
            let function = function.as_object().ok_or_else(|| {
                anyhow!("Chat Completions tool call function delta must be an object")
            })?;
            for key in function.keys() {
                if !matches!(key.as_str(), "name" | "arguments") {
                    bail!("unknown Chat Completions function delta field: {key}");
                }
            }
            if let Some(name) = function.get("name").filter(|value| !value.is_null()) {
                let name = name
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        anyhow!("Chat Completions tool call function name must be non-empty text")
                    })?;
                if builder
                    .name
                    .as_deref()
                    .is_some_and(|previous| previous != name)
                {
                    bail!("Chat Completions tool call {index} changed function name");
                }
                builder.name = Some(name.to_string());
            }
            if let Some(arguments) = function.get("arguments").filter(|value| !value.is_null()) {
                let arguments = arguments.as_str().ok_or_else(|| {
                    anyhow!("Chat Completions tool call arguments delta must be a string")
                })?;
                builder.arguments.push_str(arguments);
            }
        }
        if call.get("id").is_none() && call.get("function").is_none() {
            bail!("Chat Completions tool call delta has no semantic fields");
        }
        state.events.push(ProviderStreamEvent::ToolCallDelta {
            call_id: builder
                .id
                .clone()
                .unwrap_or_else(|| format!("tool_call_{}", index + 1)),
            delta: Value::Object(call.clone()),
        });
    }
    Ok(())
}

fn normalize_chat_usage(raw: &Value) -> Result<ProviderUsage> {
    let object = raw
        .as_object()
        .ok_or_else(|| anyhow!("Chat Completions usage must be an object"))?;
    Ok(ProviderUsage {
        input_tokens: optional_u64(object.get("prompt_tokens"), "usage.prompt_tokens")?,
        output_tokens: optional_u64(object.get("completion_tokens"), "usage.completion_tokens")?,
        reasoning_tokens: optional_u64(
            object
                .get("completion_tokens_details")
                .and_then(|details| details.get("reasoning_tokens")),
            "usage.completion_tokens_details.reasoning_tokens",
        )?,
        cache_read_tokens: optional_u64(
            object
                .get("prompt_tokens_details")
                .and_then(|details| details.get("cached_tokens")),
            "usage.prompt_tokens_details.cached_tokens",
        )?,
        cache_write_tokens: None,
        total_tokens: optional_u64(object.get("total_tokens"), "usage.total_tokens")?,
    })
}

fn optional_u64(value: Option<&Value>, field: &str) -> Result<Option<u64>> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    value
        .as_u64()
        .map(Some)
        .ok_or_else(|| anyhow!("Chat Completions {field} must be a non-negative integer"))
}

fn map_finish_reason(reason: &str) -> Result<ProviderFinishReason> {
    match reason {
        "stop" => Ok(ProviderFinishReason::Stop),
        "length" => Ok(ProviderFinishReason::Length),
        "tool_calls" | "function_call" => Ok(ProviderFinishReason::ToolCall),
        "content_filter" => Ok(ProviderFinishReason::ContentFilter),
        reason => bail!("unknown Chat Completions finish_reason: {reason}"),
    }
}

fn capture_stable_text(
    value: Option<&Value>,
    field: &str,
    target: &mut Option<String>,
) -> Result<()> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let value = value
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("Chat Completions {field} must be non-empty text"))?;
    if target.as_deref().is_some_and(|previous| previous != value) {
        bail!("Chat Completions stream changed {field}");
    }
    *target = Some(value.to_string());
    Ok(())
}

fn capture_stable_u64(value: Option<&Value>, field: &str, target: &mut Option<u64>) -> Result<()> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let value = value
        .as_u64()
        .ok_or_else(|| anyhow!("Chat Completions {field} must be a non-negative integer"))?;
    if target.is_some_and(|previous| previous != value) {
        bail!("Chat Completions stream changed {field}");
    }
    *target = Some(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(payload: Value, state: &mut ChatStreamState) -> Result<()> {
        process_payload(&payload, state)
    }

    #[test]
    fn sse_boundary_detection_keeps_split_utf8_bytes_buffered() {
        let payload = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"中文🙂\"},\"finish_reason\":null}]}\n\n";
        let bytes = payload.as_bytes();
        let emoji_start = payload.find('🙂').unwrap();
        let mut buffer = bytes[..emoji_start + 1].to_vec();
        assert_eq!(find_sse_event_boundary_bytes(&buffer), None);

        buffer.extend_from_slice(&bytes[emoji_start + 1..]);
        let (boundary, delimiter_len) = find_sse_event_boundary_bytes(&buffer).unwrap();
        assert_eq!(delimiter_len, 2);
        assert_eq!(
            std::str::from_utf8(&buffer[..boundary]).unwrap(),
            payload.trim_end()
        );
    }

    #[test]
    fn repeated_identical_text_chunks_are_preserved_in_order() {
        let mut state = ChatStreamState::default();
        let chunk = json!({
            "id": "chatcmpl_repeat",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {"content": "same"}, "finish_reason": null}]
        });
        push(chunk.clone(), &mut state).unwrap();
        push(chunk, &mut state).unwrap();

        assert_eq!(state.text, "samesame");
        assert_eq!(
            state.events,
            vec![
                ProviderStreamEvent::TextDelta {
                    delta: "same".to_string()
                },
                ProviderStreamEvent::TextDelta {
                    delta: "same".to_string()
                }
            ]
        );
    }

    #[test]
    fn fragmented_tool_arguments_commit_as_one_typed_call() {
        let mut state = ChatStreamState::default();
        push(
            json!({
                "id": "chatcmpl_tool",
                "choices": [{"index": 0, "delta": {"tool_calls": [{
                    "index": 0,
                    "id": "call_weather",
                    "type": "function",
                    "function": {"name": "weather", "arguments": "{\"city\":"}
                }]}, "finish_reason": null}]
            }),
            &mut state,
        )
        .unwrap();
        push(
            json!({
                "id": "chatcmpl_tool",
                "choices": [{"index": 0, "delta": {"tool_calls": [{
                    "index": 0,
                    "function": {"arguments": "\"Paris\"}"}
                }]}, "finish_reason": null}]
            }),
            &mut state,
        )
        .unwrap();

        let call = state.tool_calls.pop().unwrap().finish(0).unwrap();
        assert_eq!(call.id, "call_weather");
        assert_eq!(call.name, "weather");
        assert_eq!(call.arguments, json!({"city": "Paris"}));
        assert_eq!(
            state
                .events
                .iter()
                .filter(|event| matches!(event, ProviderStreamEvent::ToolCallDelta { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn usage_and_terminal_frames_map_without_inference() {
        let mut state = ChatStreamState::default();
        push(
            json!({
                "id": "chatcmpl_terminal",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "length"}]
            }),
            &mut state,
        )
        .unwrap();
        push(
            json!({
                "id": "chatcmpl_terminal",
                "choices": [],
                "usage": {
                    "prompt_tokens": 3,
                    "completion_tokens": 5,
                    "total_tokens": 8,
                    "completion_tokens_details": {"reasoning_tokens": 2},
                    "prompt_tokens_details": {"cached_tokens": 1}
                }
            }),
            &mut state,
        )
        .unwrap();

        assert_eq!(state.finish_reason, Some(ProviderFinishReason::Length));
        assert_eq!(state.usage.input_tokens, Some(3));
        assert_eq!(state.usage.output_tokens, Some(5));
        assert_eq!(state.usage.reasoning_tokens, Some(2));
        assert_eq!(state.usage.cache_read_tokens, Some(1));
        assert_eq!(state.usage.total_tokens, Some(8));

        let mut emitted = Vec::new();
        let envelope = finalize_stream(
            state,
            "gpt-5.4-mini".to_string(),
            &HeaderMap::new(),
            &mut |event| {
                emitted.push(event.clone());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            envelope.result.finish_reason,
            Some(ProviderFinishReason::Length)
        );
        assert!(matches!(
            emitted.as_slice(),
            [
                ProviderStreamEvent::UsageSnapshot { .. },
                ProviderStreamEvent::Finish {
                    reason: ProviderFinishReason::Length
                }
            ]
        ));
    }

    #[test]
    fn unknown_semantic_frames_fail_explicitly() {
        let mut state = ChatStreamState::default();
        let error = push(
            json!({
                "id": "chatcmpl_unknown",
                "choices": [{"index": 0, "delta": {"future_semantic": "opaque"}, "finish_reason": null}]
            }),
            &mut state,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("unknown Chat Completions semantic delta field"));
    }

    #[test]
    fn invalid_fragmented_tool_arguments_fail_at_commit() {
        let builder = ChatToolCallBuilder {
            id: Some("call_invalid".to_string()),
            name: Some("lookup".to_string()),
            arguments: "{not-json}".to_string(),
        };
        let error = builder.finish(0).unwrap_err();

        assert!(error.to_string().contains("invalid JSON arguments"));
    }
}
