use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::{
    ensure_success_status, normalize_tool_input, read_json_response, value_to_string,
    BailianProtocol, ProviderFinishReason, ProviderInvocationResult, ProviderStreamEvent,
    ProviderToolCall, ProviderUsage, RuntimeInvocationEnvelope,
};

pub(crate) async fn read_chat_streaming_response<F>(
    response: reqwest::Response,
    request_model: String,
    protocol: BailianProtocol,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let status = response.status();
    if !status.is_success() {
        let payload = read_json_response(response).await?;
        return ensure_success_status(status, &payload)
            .and_then(|_| bail!("provider request failed"));
    }
    let mut state = StreamState::new(request_model, protocol);
    read_sse_lines(
        response,
        |payload| state.process_chat_payload(payload),
        on_event,
    )
    .await?;
    state.finish(on_event)
}

pub(crate) async fn read_responses_streaming_response<F>(
    response: reqwest::Response,
    request_model: String,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let status = response.status();
    if !status.is_success() {
        let payload = read_json_response(response).await?;
        return ensure_success_status(status, &payload)
            .and_then(|_| bail!("provider request failed"));
    }
    let mut state = StreamState::new(request_model, BailianProtocol::OpenAiResponses);
    read_sse_lines(
        response,
        |payload| state.process_responses_payload(payload),
        on_event,
    )
    .await?;
    state.finish(on_event)
}

pub(crate) async fn read_anthropic_streaming_response<F>(
    response: reqwest::Response,
    request_model: String,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let status = response.status();
    if !status.is_success() {
        let payload = read_json_response(response).await.unwrap_or(Value::Null);
        return ensure_success_status(status, &payload)
            .and_then(|_| bail!("provider request failed"));
    }
    let mut state = StreamState::new(request_model, BailianProtocol::AnthropicMessages);
    read_sse_lines(
        response,
        |payload| state.process_anthropic_payload(payload),
        on_event,
    )
    .await?;
    state.finish(on_event)
}

pub(crate) async fn read_dashscope_streaming_response<F>(
    response: reqwest::Response,
    request_model: String,
    on_event: &mut F,
) -> Result<RuntimeInvocationEnvelope>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
{
    let status = response.status();
    if !status.is_success() {
        let payload = read_json_response(response).await?;
        return ensure_success_status(status, &payload)
            .and_then(|_| bail!("provider request failed"));
    }
    let mut state = StreamState::new(request_model, BailianProtocol::DashScope);
    read_sse_lines(
        response,
        |payload| state.process_dashscope_payload(payload),
        on_event,
    )
    .await?;
    state.finish(on_event)
}

async fn read_sse_lines<F, P>(
    response: reqwest::Response,
    mut process_payload: P,
    on_event: &mut F,
) -> Result<()>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
    P: FnMut(&Value) -> Vec<ProviderStreamEvent>,
{
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    while let Some(chunk) = stream.next().await {
        buffer.push_str(&String::from_utf8_lossy(&chunk?));
        while let Some(index) = buffer.find('\n') {
            let mut line = buffer[..index].to_string();
            if line.ends_with('\r') {
                line.pop();
            }
            buffer.drain(..=index);
            process_sse_line(&line, &mut process_payload, on_event)?;
        }
    }
    if !buffer.trim().is_empty() {
        process_sse_line(&buffer, &mut process_payload, on_event)?;
    }
    Ok(())
}

fn process_sse_line<F, P>(line: &str, process_payload: &mut P, on_event: &mut F) -> Result<()>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
    P: FnMut(&Value) -> Vec<ProviderStreamEvent>,
{
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') || !line.starts_with("data:") {
        return Ok(());
    }
    let data = line.trim_start_matches("data:").trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let payload: Value =
        serde_json::from_str(data).with_context(|| "provider returned invalid SSE JSON")?;
    if let (Some(code), Some(message)) = (
        payload.get("code").and_then(Value::as_str),
        payload.get("message").and_then(Value::as_str),
    ) {
        bail!("provider stream error {code}: {message}");
    }
    for event in process_payload(&payload) {
        on_event(&event)?;
    }
    Ok(())
}

pub(super) struct StreamState {
    request_model: String,
    protocol: BailianProtocol,
    pub(super) text: String,
    events: Vec<ProviderStreamEvent>,
    pub(super) usage: ProviderUsage,
    pub(super) finish_reason: ProviderFinishReason,
    response_id: Option<String>,
    tool_builders: HashMap<usize, ToolCallBuilder>,
}

impl StreamState {
    pub(super) fn new(request_model: String, protocol: BailianProtocol) -> Self {
        Self {
            request_model,
            protocol,
            text: String::new(),
            events: Vec::new(),
            usage: ProviderUsage::default(),
            finish_reason: ProviderFinishReason::Unknown,
            response_id: None,
            tool_builders: HashMap::new(),
        }
    }

    fn process_chat_payload(&mut self, payload: &Value) -> Vec<ProviderStreamEvent> {
        if self.response_id.is_none() {
            self.response_id = payload
                .get("id")
                .map(value_to_string)
                .filter(|value| !value.is_empty());
        }
        if let Some(usage) = payload.get("usage").filter(|value| !value.is_null()) {
            self.usage = normalize_openai_usage(usage);
        }
        let mut events = Vec::new();
        let Some(choice) = payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
        else {
            return events;
        };
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = normalize_openai_finish_reason(reason);
        }
        let delta = choice.get("delta").unwrap_or(&Value::Null);
        if let Some(content) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            self.text.push_str(content);
            events.push(ProviderStreamEvent::TextDelta {
                delta: content.to_string(),
            });
        }
        if let Some(reasoning) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            events.push(ProviderStreamEvent::ReasoningDelta {
                delta: reasoning.to_string(),
            });
        }
        merge_chat_tool_deltas(
            delta.get("tool_calls"),
            &mut self.tool_builders,
            &mut events,
        );
        self.events.extend(events.clone());
        events
    }

    fn process_responses_payload(&mut self, payload: &Value) -> Vec<ProviderStreamEvent> {
        if let Some(id) = payload
            .get("response")
            .and_then(|value| value.get("id"))
            .or_else(|| payload.get("id"))
        {
            self.response_id = Some(value_to_string(id));
        }
        let mut events = Vec::new();
        match payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "response.output_text.delta" => {
                if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                    self.text.push_str(delta);
                    events.push(ProviderStreamEvent::TextDelta {
                        delta: delta.to_string(),
                    });
                }
            }
            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                    events.push(ProviderStreamEvent::ReasoningDelta {
                        delta: delta.to_string(),
                    });
                }
            }
            "response.function_call_arguments.done" => {
                if let Some(call) = provider_tool_call_from_response_payload(payload) {
                    let index = self.tool_builders.len();
                    self.tool_builders
                        .insert(index, ToolCallBuilder::from_call(call));
                }
            }
            "response.output_item.done" => {
                if let Some(call) = provider_tool_call_from_response_item(payload.get("item")) {
                    let index = self.tool_builders.len();
                    self.tool_builders
                        .insert(index, ToolCallBuilder::from_call(call));
                }
            }
            "response.completed" => {
                if let Some(response) = payload.get("response") {
                    self.usage =
                        normalize_responses_usage(response.get("usage").unwrap_or(&Value::Null));
                    if let Some(output) = response.get("output").and_then(Value::as_array) {
                        for item in output {
                            if let Some(call) = provider_tool_call_from_response_item(Some(item)) {
                                let index = self.tool_builders.len();
                                self.tool_builders
                                    .insert(index, ToolCallBuilder::from_call(call));
                            }
                        }
                    }
                    self.finish_reason = normalize_response_finish_reason(response);
                }
            }
            _ => {}
        }
        self.events.extend(events.clone());
        events
    }

    fn process_anthropic_payload(&mut self, payload: &Value) -> Vec<ProviderStreamEvent> {
        let mut events = Vec::new();
        match payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "message_start" => {
                if let Some(message) = payload.get("message") {
                    self.response_id = message.get("id").map(value_to_string);
                    self.usage =
                        normalize_anthropic_usage(message.get("usage").unwrap_or(&Value::Null));
                }
            }
            "content_block_start" => {
                let index = payload.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(block) = payload.get("content_block") {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        self.tool_builders.insert(
                            index,
                            ToolCallBuilder {
                                id: block.get("id").map(value_to_string).unwrap_or_default(),
                                name: block.get("name").map(value_to_string).unwrap_or_default(),
                                arguments: block
                                    .get("input")
                                    .map(Value::to_string)
                                    .unwrap_or_default(),
                            },
                        );
                    }
                }
            }
            "content_block_delta" => {
                let index = payload.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let delta = payload.get("delta").unwrap_or(&Value::Null);
                match delta
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                {
                    "text_delta" => {
                        if let Some(value) = delta.get("text").and_then(Value::as_str) {
                            self.text.push_str(value);
                            events.push(ProviderStreamEvent::TextDelta {
                                delta: value.to_string(),
                            });
                        }
                    }
                    "thinking_delta" => {
                        if let Some(value) = delta.get("thinking").and_then(Value::as_str) {
                            events.push(ProviderStreamEvent::ReasoningDelta {
                                delta: value.to_string(),
                            });
                        }
                    }
                    "input_json_delta" => {
                        if let Some(value) = delta.get("partial_json").and_then(Value::as_str) {
                            if let Some(builder) = self.tool_builders.get_mut(&index) {
                                builder.arguments.push_str(value);
                                events.push(ProviderStreamEvent::ToolCallDelta {
                                    call_id: builder.id.clone(),
                                    delta: Value::String(value.to_string()),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(reason) = payload
                    .get("delta")
                    .and_then(|value| value.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    self.finish_reason = normalize_anthropic_stop_reason(reason);
                }
                if let Some(usage) = payload.get("usage") {
                    merge_usage(&mut self.usage, normalize_anthropic_usage(usage));
                }
            }
            _ => {}
        }
        self.events.extend(events.clone());
        events
    }

    pub(super) fn process_dashscope_payload(
        &mut self,
        payload: &Value,
    ) -> Vec<ProviderStreamEvent> {
        let mut events = Vec::new();
        if let Some(request_id) = payload.get("request_id").and_then(Value::as_str) {
            self.response_id = Some(request_id.to_string());
        }
        if let Some(usage) = payload.get("usage") {
            self.usage = normalize_dashscope_usage(usage);
        }
        if let Some(reasoning) = dashscope_reasoning_delta(payload) {
            events.push(ProviderStreamEvent::ReasoningDelta { delta: reasoning });
        }
        if let Some(text) = dashscope_text_delta(payload) {
            self.text.push_str(&text);
            events.push(ProviderStreamEvent::TextDelta { delta: text });
        }
        if let Some(finish_reason) = dashscope_finish_reason(payload) {
            self.finish_reason = normalize_openai_finish_reason(finish_reason);
        }
        self.events.extend(events.clone());
        events
    }

    fn finish<F>(mut self, on_event: &mut F) -> Result<RuntimeInvocationEnvelope>
    where
        F: FnMut(&ProviderStreamEvent) -> Result<()>,
    {
        let tool_calls = self
            .tool_builders
            .into_values()
            .map(ToolCallBuilder::into_tool_call)
            .collect::<Vec<_>>();
        if !tool_calls.is_empty() {
            self.finish_reason = ProviderFinishReason::ToolCall;
        }
        let mut final_events = Vec::new();
        for call in &tool_calls {
            final_events.push(ProviderStreamEvent::ToolCallCommit { call: call.clone() });
        }
        if self.usage.has_any_value() {
            final_events.push(ProviderStreamEvent::UsageSnapshot {
                usage: self.usage.clone(),
            });
        }
        final_events.push(ProviderStreamEvent::Finish {
            reason: self.finish_reason.clone(),
        });
        for event in &final_events {
            on_event(event)?;
        }
        self.events.extend(final_events);
        Ok(RuntimeInvocationEnvelope {
            events: self.events,
            result: ProviderInvocationResult {
                final_content: (!self.text.is_empty()).then_some(self.text),
                response_id: self.response_id,
                tool_calls,
                mcp_calls: Vec::new(),
                usage: self.usage,
                finish_reason: Some(self.finish_reason),
                provider_metadata: json!({
                    "request_model": self.request_model,
                    "api_protocol": self.protocol.as_str(),
                }),
            },
        })
    }
}

#[derive(Debug, Clone, Default)]
struct ToolCallBuilder {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallBuilder {
    fn from_call(call: ProviderToolCall) -> Self {
        Self {
            id: call.id,
            name: call.name,
            arguments: call.arguments.to_string(),
        }
    }

    fn into_tool_call(self) -> ProviderToolCall {
        ProviderToolCall {
            id: if self.id.is_empty() {
                "tool_call_1".to_string()
            } else {
                self.id
            },
            name: if self.name.is_empty() {
                "unknown_tool".to_string()
            } else {
                self.name
            },
            arguments: serde_json::from_str(&self.arguments)
                .unwrap_or_else(|_| json!({ "raw": self.arguments })),
        }
    }
}

fn merge_chat_tool_deltas(
    tool_calls: Option<&Value>,
    builders: &mut HashMap<usize, ToolCallBuilder>,
    events: &mut Vec<ProviderStreamEvent>,
) {
    let Some(tool_calls) = tool_calls.and_then(Value::as_array) else {
        return;
    };
    for tool_call in tool_calls {
        let index = tool_call
            .get("index")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(builders.len());
        let builder = builders.entry(index).or_default();
        if let Some(id) = tool_call
            .get("id")
            .map(value_to_string)
            .filter(|value| !value.is_empty())
        {
            builder.id = id;
        }
        if let Some(function) = tool_call.get("function") {
            if let Some(name) = function
                .get("name")
                .map(value_to_string)
                .filter(|value| !value.is_empty())
            {
                builder.name = name;
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                builder.arguments.push_str(arguments);
            }
        }
        events.push(ProviderStreamEvent::ToolCallDelta {
            call_id: if builder.id.is_empty() {
                format!("tool_call_{}", index + 1)
            } else {
                builder.id.clone()
            },
            delta: tool_call.clone(),
        });
    }
}

fn provider_tool_call_from_response_payload(payload: &Value) -> Option<ProviderToolCall> {
    let id = payload
        .get("call_id")
        .or_else(|| payload.get("item_id"))
        .map(value_to_string)
        .filter(|value| !value.is_empty())?;
    let name = payload
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())?
        .to_string();
    let arguments = payload
        .get("arguments")
        .map(|value| normalize_tool_input(value.clone()))
        .unwrap_or_else(|| json!({}));
    Some(ProviderToolCall {
        id,
        name,
        arguments,
    })
}

fn provider_tool_call_from_response_item(item: Option<&Value>) -> Option<ProviderToolCall> {
    let item = item?;
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return None;
    }
    let id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .map(value_to_string)
        .filter(|value| !value.is_empty())?;
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())?
        .to_string();
    let arguments = item
        .get("arguments")
        .map(|value| normalize_tool_input(value.clone()))
        .unwrap_or_else(|| json!({}));
    Some(ProviderToolCall {
        id,
        name,
        arguments,
    })
}

fn dashscope_text_delta(payload: &Value) -> Option<String> {
    payload
        .get("output")
        .and_then(|output| output.get("text"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            dashscope_choice_message(payload)
                .and_then(|message| message.get("content"))
                .and_then(dashscope_content_text)
        })
}

fn dashscope_reasoning_delta(payload: &Value) -> Option<String> {
    dashscope_choice_message(payload)
        .and_then(|message| {
            message
                .get("reasoning_content")
                .or_else(|| message.get("reasoning"))
        })
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn dashscope_choice_message(payload: &Value) -> Option<&Value> {
    payload
        .get("output")
        .and_then(|output| output.get("choices"))
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|choice| choice.get("message"))
}

fn dashscope_content_text(content: &Value) -> Option<String> {
    match content {
        Value::String(value) if !value.is_empty() => Some(value.to_string()),
        Value::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    Value::String(value) => text.push_str(value),
                    Value::Object(object) => {
                        if let Some(value) = object
                            .get("text")
                            .or_else(|| object.get("content"))
                            .and_then(Value::as_str)
                        {
                            text.push_str(value);
                        }
                    }
                    _ => {}
                }
            }
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn dashscope_finish_reason(payload: &Value) -> Option<&str> {
    payload
        .get("output")
        .and_then(|output| output.get("finish_reason"))
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .get("output")
                .and_then(|output| output.get("choices"))
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|choice| choice.get("finish_reason"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "null" && *value != "none")
}

fn normalize_openai_usage(usage: &Value) -> ProviderUsage {
    ProviderUsage {
        input_tokens: number_or_none(usage.get("prompt_tokens")),
        output_tokens: number_or_none(usage.get("completion_tokens")),
        reasoning_tokens: usage
            .get("completion_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(number_or_none_ref),
        cache_read_tokens: usage
            .get("prompt_tokens_details")
            .and_then(|details| details.get("cached_tokens"))
            .and_then(number_or_none_ref),
        cache_write_tokens: None,
        total_tokens: number_or_none(usage.get("total_tokens")),
    }
}

fn normalize_responses_usage(usage: &Value) -> ProviderUsage {
    ProviderUsage {
        input_tokens: number_or_none(usage.get("input_tokens")),
        output_tokens: number_or_none(usage.get("output_tokens")),
        reasoning_tokens: usage
            .get("output_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(number_or_none_ref),
        cache_read_tokens: usage
            .get("input_tokens_details")
            .and_then(|details| details.get("cached_tokens"))
            .and_then(number_or_none_ref),
        cache_write_tokens: None,
        total_tokens: number_or_none(usage.get("total_tokens")),
    }
}

fn normalize_anthropic_usage(usage: &Value) -> ProviderUsage {
    let input_tokens = number_or_none(usage.get("input_tokens"));
    let output_tokens = number_or_none(usage.get("output_tokens"));
    ProviderUsage {
        input_tokens,
        output_tokens,
        reasoning_tokens: None,
        cache_read_tokens: number_or_none(usage.get("cache_read_input_tokens")),
        cache_write_tokens: number_or_none(usage.get("cache_creation_input_tokens")),
        total_tokens: input_tokens
            .zip(output_tokens)
            .map(|(left, right)| left + right),
    }
}

fn normalize_dashscope_usage(usage: &Value) -> ProviderUsage {
    ProviderUsage {
        input_tokens: number_or_none(usage.get("input_tokens")),
        output_tokens: number_or_none(usage.get("output_tokens")),
        reasoning_tokens: usage
            .get("output_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(number_or_none_ref),
        cache_read_tokens: None,
        cache_write_tokens: None,
        total_tokens: number_or_none(usage.get("total_tokens")),
    }
}

fn merge_usage(current: &mut ProviderUsage, snapshot: ProviderUsage) {
    current.input_tokens = snapshot.input_tokens.or(current.input_tokens);
    current.output_tokens = snapshot.output_tokens.or(current.output_tokens);
    current.reasoning_tokens = snapshot.reasoning_tokens.or(current.reasoning_tokens);
    current.cache_read_tokens = snapshot.cache_read_tokens.or(current.cache_read_tokens);
    current.cache_write_tokens = snapshot.cache_write_tokens.or(current.cache_write_tokens);
    current.total_tokens = current
        .input_tokens
        .zip(current.output_tokens)
        .map(|(left, right)| left + right)
        .or(snapshot.total_tokens)
        .or(current.total_tokens);
}

fn number_or_none(value: Option<&Value>) -> Option<u64> {
    value.and_then(number_or_none_ref)
}

fn number_or_none_ref(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_i64()
            .and_then(|raw| (raw >= 0).then_some(raw as u64))
    })
}

fn normalize_openai_finish_reason(reason: &str) -> ProviderFinishReason {
    match reason {
        "stop" => ProviderFinishReason::Stop,
        "length" => ProviderFinishReason::Length,
        "tool_calls" => ProviderFinishReason::ToolCall,
        "content_filter" => ProviderFinishReason::ContentFilter,
        _ => ProviderFinishReason::Unknown,
    }
}

fn normalize_response_finish_reason(response: &Value) -> ProviderFinishReason {
    match response.get("status").and_then(Value::as_str) {
        Some("completed") => ProviderFinishReason::Stop,
        Some("incomplete") => ProviderFinishReason::Length,
        _ => ProviderFinishReason::Unknown,
    }
}

fn normalize_anthropic_stop_reason(reason: &str) -> ProviderFinishReason {
    match reason {
        "end_turn" | "stop_sequence" => ProviderFinishReason::Stop,
        "max_tokens" => ProviderFinishReason::Length,
        "tool_use" => ProviderFinishReason::ToolCall,
        _ => ProviderFinishReason::Unknown,
    }
}
