use super::*;
use runtime_extension_sdk::{serve, MultiplexEmitter};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use tokio::sync::Mutex;

const OWNER_IDLE_RETENTION: Duration = Duration::from_secs(60 * 60);

struct Owner {
    runtime: Rc<Mutex<OpenAiProviderRuntime>>,
    last_used: Instant,
    retain_until: Option<Instant>,
}

fn routed_invocation_input(input: &Value) -> Result<ProviderInvocationInput> {
    let mut business_input = input.clone();
    // The outer stdio envelope may inject this trusted observation marker.
    // Routing reads the strict business schema, while the original request
    // retains the marker for handle_invoke_request_streaming.
    protocol_observation::take_enabled(&mut business_input);
    Ok(serde_json::from_value(business_input)?)
}

fn is_streaming_generate(request: &ProviderStdioRequest) -> bool {
    request.method == "invoke"
        && request.input.get("operation").and_then(Value::as_str) != Some("count_tokens")
        && routed_invocation_input(&request.input)
            .is_ok_and(|input| input.operation == ProviderWireOperation::Generate)
}

#[derive(Default)]
struct Owners {
    entries: HashMap<String, Owner>,
    // The control command has no provider config. An ambiguous logical ID must
    // never be routed to a different credential or endpoint.
    logical: HashMap<String, Vec<String>>,
    responses: HashMap<(String, String), (Option<String>, Instant)>,
    next_anonymous: u64,
    gc_started: bool,
}

impl Owners {
    fn prune(&mut self) {
        let now = Instant::now();
        self.entries.retain(|_, owner| {
            Rc::strong_count(&owner.runtime) > 1
                || owner.retain_until.map_or_else(
                    || now.saturating_duration_since(owner.last_used) < OWNER_IDLE_RETENTION,
                    |deadline| now < deadline,
                )
        });
        self.logical.retain(|_, keys| {
            keys.retain(|key| self.entries.contains_key(key));
            !keys.is_empty()
        });
        self.responses.retain(|_, (key, observed_at)| {
            now.saturating_duration_since(*observed_at) < OWNER_IDLE_RETENTION
                && key
                    .as_ref()
                    .map_or(true, |key| self.entries.contains_key(key))
        });
    }

    fn route(&mut self, request: &ProviderStdioRequest) -> Result<(String, Option<String>)> {
        self.prune();
        if request.method == "transport_session" {
            let command: TransportSessionCommand = serde_json::from_value(request.input.clone())?;
            command.validate()?;
            let keys = self.logical.get(&command.logical_session_id);
            return match keys {
                Some(keys) if keys.len() == 1 => Ok((keys[0].clone(), None)),
                Some(_) => bail!("transport session identity is ambiguous across configurations"),
                None => Ok((self.anonymous_key(), None)),
            };
        }
        if request.method != "invoke"
            || request.input.get("operation").and_then(Value::as_str) == Some("count_tokens")
        {
            return Ok((self.anonymous_key(), None));
        }
        let input = routed_invocation_input(&request.input)?;
        if input.operation != ProviderWireOperation::Generate {
            return Ok((self.anonymous_key(), None));
        }
        let config = provider_config_for_invocation(&input)?;
        let directive = transport_session_directive(&input)?;
        let scope = websocket_history_scope(&config, &input);
        if let Some(directive) = directive {
            // One Host-sealed physical session spans semantic and native turns.
            // History scopes intentionally separate those formats; runtime ownership must not.
            let key = websocket_session_key(&config, &input, Some(&directive));
            let remaining_ms = directive
                .physical_deadline_unix_ms
                .saturating_sub(close::unix_time_ms())
                .max(0) as u64;
            let retain_until = Instant::now()
                + Duration::from_millis(remaining_ms.min(24 * 60 * 60 * 1_000))
                + Duration::from_secs(60);
            self.owner(&key);
            if let Some(owner) = self.entries.get_mut(&key) {
                owner.retain_until = Some(
                    owner
                        .retain_until
                        .map_or(retain_until, |old| old.max(retain_until)),
                );
            }
            let keys = self
                .logical
                .entry(directive.logical_session_id)
                .or_default();
            if !keys.contains(&key) {
                keys.push(key.clone());
            }
            return Ok((key, Some(scope)));
        }
        let previous = input.previous_response_id.as_deref().or_else(|| {
            input
                .native_transport
                .as_ref()
                .and_then(|native| responses_body_previous_response_id(&native.wire_body))
        });
        if let Some(previous) = previous {
            if let Some((Some(key), _)) = self.responses.get(&(scope.clone(), previous.to_owned()))
            {
                return Ok((key.clone(), Some(scope)));
            }
        }
        Ok((self.anonymous_key(), Some(scope)))
    }

    fn anonymous_key(&mut self) -> String {
        self.next_anonymous = self.next_anonymous.wrapping_add(1);
        format!("anonymous:{}", self.next_anonymous)
    }

    fn owner(&mut self, key: &str) -> Rc<Mutex<OpenAiProviderRuntime>> {
        let owner = self.entries.entry(key.to_owned()).or_insert_with(|| Owner {
            runtime: Rc::new(Mutex::new(OpenAiProviderRuntime::default())),
            last_used: Instant::now(),
            retain_until: None,
        });
        owner.last_used = Instant::now();
        owner.runtime.clone()
    }

    fn record(&mut self, key: &str, scope: Option<String>, response_id: Option<String>) {
        if let Some(owner) = self.entries.get_mut(key) {
            owner.last_used = Instant::now();
        }
        if let (Some(scope), Some(response_id)) = (scope, response_id) {
            let entry = self
                .responses
                .entry((scope, response_id))
                .or_insert_with(|| (Some(key.to_owned()), Instant::now()));
            if entry.0.as_deref() != Some(key) {
                entry.0 = None; // A colliding upstream cursor has no safe owner.
            }
            entry.1 = Instant::now();
        }
    }
}

struct Inflight<'a> {
    runtime: &'a mut OpenAiProviderRuntime,
    finished: bool,
}

impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.runtime.abandon_inflight_transport();
        }
    }
}

pub async fn serve_multiplex_worker() -> Result<()> {
    let owners = Rc::new(RefCell::new(Owners::default()));
    serve(move |raw, emitter| {
        let owners = owners.clone();
        async move { handle(raw, emitter, owners).await }
    })
    .await?;
    Ok(())
}

async fn handle(raw: Value, emitter: MultiplexEmitter, owners: Rc<RefCell<Owners>>) -> Value {
    if !owners.borrow().gc_started {
        owners.borrow_mut().gc_started = true;
        let weak = Rc::downgrade(&owners);
        tokio::task::spawn_local(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let Some(owners) = weak.upgrade() else { break };
                owners.borrow_mut().prune();
            }
        });
    }
    let request =
        serde_json::from_value::<ProviderStdioRequest>(raw).unwrap_or(ProviderStdioRequest {
            method: "invalid".into(),
            input: Value::Null,
        });
    let (key, scope) = match owners.borrow_mut().route(&request) {
        Ok(route) => route,
        Err(error) => {
            return if request.method == "invoke"
                && !matches!(
                    request.input.get("operation").and_then(Value::as_str),
                    Some("count_tokens" | "compact")
                ) {
                streaming_failure(error, &emitter)
            } else {
                error_response(error)
            };
        }
    };
    let owner = owners.borrow_mut().owner(&key);
    let mut runtime = owner.lock().await;
    let mut inflight = Inflight {
        runtime: &mut runtime,
        finished: false,
    };
    let streaming = is_streaming_generate(&request);
    let (response, response_id) = if streaming {
        let result = inflight
            .runtime
            .handle_invoke_request_streaming(request.input, |event| {
                emitter.try_event(serde_json::to_value(event)?)?;
                Ok(())
            })
            .await;
        match result {
            Ok(result) => {
                let response_id = result.response_id.clone();
                (
                    serde_json::to_value(result).unwrap_or(Value::Null),
                    response_id,
                )
            }
            Err(error) => (streaming_failure(error, &emitter), None),
        }
    } else {
        let response = inflight
            .runtime
            .handle_request(request)
            .await
            .unwrap_or_else(|error| {
                error
                    .downcast_ref::<ProviderRuntimeError>()
                    .cloned()
                    .map(ProviderStdioResponse::runtime_error)
                    .unwrap_or_else(|| {
                        ProviderStdioResponse::error("provider_invalid_response", error.to_string())
                    })
            });
        (serde_json::to_value(response).unwrap_or(Value::Null), None)
    };
    inflight.finished = true;
    drop(inflight);
    drop(runtime);
    let mut owners = owners.borrow_mut();
    owners.record(&key, scope, response_id);
    response
}

fn error_response(error: anyhow::Error) -> Value {
    serde_json::to_value(ProviderStdioResponse::error(
        "provider_invalid_response",
        error.to_string(),
    ))
    .unwrap_or(Value::Null)
}

fn streaming_failure(error: anyhow::Error, emitter: &MultiplexEmitter) -> Value {
    let runtime_error = error
        .downcast_ref::<ProviderRuntimeError>()
        .cloned()
        .unwrap_or_else(|| ProviderRuntimeError::normalize("invoke", error.to_string(), None));
    let _ = emitter.try_event(
        serde_json::to_value(ProviderStreamEvent::Error {
            error: runtime_error.clone(),
        })
        .unwrap_or(Value::Null),
    );
    serde_json::to_value(ProviderInvocationResult {
        final_content: None,
        response_id: None,
        tool_calls: Vec::new(),
        mcp_calls: Vec::new(),
        usage: ProviderUsage::default(),
        finish_reason: Some(ProviderFinishReason::Error),
        provider_metadata: runtime_error.failure_metadata(),
    })
    .unwrap_or(Value::Null)
}

#[cfg(test)]
#[path = "_tests/multiplex_worker.rs"]
mod tests;
