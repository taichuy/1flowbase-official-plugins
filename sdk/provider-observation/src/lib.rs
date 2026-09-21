//! Shared provider plaintext observation boundary. Credentials, headers and URLs are excluded.
use anyhow::Result;
use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    rc::Rc,
    sync::{Arc, Mutex},
};

/// This permissive outer envelope is compatible with old stdio hosts and providers.
/// The strict business input schema stays unchanged.
#[derive(serde::Deserialize)]
pub struct StdioRequestWire {
    method: String,
    #[serde(default)]
    input: serde_json::Value,
    #[serde(default)]
    host_capabilities: Vec<String>,
}

impl StdioRequestWire {
    pub fn into_parts(mut self) -> (String, serde_json::Value) {
        let wire = &mut self;
        if let Some(input) = wire.input.as_object_mut() {
            // Never accept business-input claims about what the surrounding host supports.
            input.remove("host_capabilities");
            if wire.method == "invoke"
                && matches!(
                    input.get("operation").and_then(serde_json::Value::as_str),
                    None | Some("generate")
                )
                && wire
                    .host_capabilities
                    .iter()
                    .any(|capability| capability == "protocol_observation_v1")
            {
                input.insert(
                    "host_capabilities".into(),
                    serde_json::json!(["protocol_observation_v1"]),
                );
            }
        }
        (self.method, self.input)
    }
}

/// Only the stdio decoder injects this temporary marker for the trusted streaming entry.
pub fn enabled(input: &serde_json::Value) -> bool {
    input
        .get("host_capabilities")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|capabilities| {
            capabilities
                .iter()
                .any(|capability| capability.as_str() == Some("protocol_observation_v1"))
        })
}

pub fn take_enabled(input: &mut serde_json::Value) -> bool {
    let enabled = enabled(input);
    if let Some(input) = input.as_object_mut() {
        input.remove("host_capabilities");
    }
    enabled
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProtocolObservation {
    pub protocol: String,
    pub transport: String,
    pub direction: String,
    pub kind: String,
    pub body: String,
    pub encoding: String,
    pub status: Option<u16>,
}

/// Both limits apply to retained encoded observations, including metadata.
#[derive(Clone, Copy)]
pub struct Limits {
    pub messages: usize,
    pub bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            messages: 128,
            bytes: 1024 * 1024,
        }
    }
}
struct Buffer {
    events: VecDeque<(ProtocolObservation, usize)>,
    bytes: usize,
    dropped: u64,
    transport: Option<String>,
    limits: Limits,
}
struct Context {
    protocol: Arc<Mutex<String>>,
    buffer: Arc<Mutex<Buffer>>,
    ready: Arc<tokio::sync::Notify>,
}
tokio::task_local! { static OBSERVATION: Context; }

pub fn record(transport: &str, direction: &str, kind: &str, bytes: &[u8], status: Option<u16>) {
    let _ = OBSERVATION.try_with(|context| {
        let protocol = if transport == "websocket" {
            "openai.responses".to_owned()
        } else {
            context
                .protocol
                .lock()
                .expect("observation protocol")
                .clone()
        };
        let mut buffer = context.buffer.lock().expect("observation buffer");
        buffer.transport = Some(transport.to_owned());
        let utf8 = std::str::from_utf8(bytes);
        let encoded_len = if utf8.is_ok() {
            bytes.len()
        } else {
            bytes.len().saturating_add(2) / 3 * 4
        };
        let size = encoded_len
            .saturating_add(protocol.len())
            .saturating_add(transport.len())
            .saturating_add(direction.len())
            .saturating_add(kind.len())
            .saturating_add(128);
        if buffer.events.len() >= buffer.limits.messages
            || size > buffer.limits.bytes.saturating_sub(buffer.bytes)
        {
            buffer.dropped = buffer.dropped.saturating_add(1);
            return;
        }
        let (body, encoding) = match utf8 {
            Ok(text) => (text.to_owned(), "utf8"),
            Err(_) => (encode_base64(bytes), "base64"),
        };
        buffer.events.push_back((
            ProtocolObservation {
                protocol,
                transport: transport.into(),
                direction: direction.into(),
                kind: kind.into(),
                body,
                encoding: encoding.into(),
                status,
            },
            size,
        ));
        buffer.bytes += size;
        context.ready.notify_one();
    });
}

pub type EventSink<'a, E> = Box<dyn FnMut(&E) -> Result<()> + 'a>;

/// Business events are delivered directly to the one physical sink. They never enter the
/// observation queue and cannot be displaced by logging. The separate queue is bounded;
/// there is no task/channel per record and no blocking send on the observation path.
pub async fn capture<'a, E: 'a, T, F, Fut>(
    protocol: String,
    on_event: F,
    map: fn(ProtocolObservation) -> E,
    invoke: impl FnOnce(EventSink<'a, E>) -> Fut,
) -> Result<T>
where
    F: FnMut(&E) -> Result<()> + 'a,
    Fut: Future<Output = Result<T>>,
{
    capture_with_limits(protocol, on_event, map, invoke, Limits::default()).await
}

pub async fn capture_with_limits<'a, E: 'a, T, F, Fut>(
    protocol: String,
    on_event: F,
    map: fn(ProtocolObservation) -> E,
    invoke: impl FnOnce(EventSink<'a, E>) -> Fut,
    limits: Limits,
) -> Result<T>
where
    F: FnMut(&E) -> Result<()> + 'a,
    Fut: Future<Output = Result<T>>,
{
    let sink = Rc::new(RefCell::new(on_event));
    let business_sink = sink.clone();
    let buffer = Arc::new(Mutex::new(Buffer {
        events: VecDeque::new(),
        bytes: 0,
        dropped: 0,
        transport: None,
        limits,
    }));
    let ready = Arc::new(tokio::sync::Notify::new());
    let final_protocol = Arc::new(Mutex::new(protocol.clone()));
    let context = Context {
        protocol: final_protocol.clone(),
        buffer: buffer.clone(),
        ready: ready.clone(),
    };
    let future = OBSERVATION.scope(context, async move {
        invoke(Box::new(move |event| (business_sink.borrow_mut())(event))).await
    });
    tokio::pin!(future);
    let result = loop {
        tokio::select! {
            biased;
            result = &mut future => break result,
            _ = ready.notified() => {
                // One observation per scheduling turn. Always poll business work first.
                let event = { let mut buffer = buffer.lock().expect("observation buffer");
                    buffer.events.pop_front().map(|(event, size)| { buffer.bytes -= size; event }) };
                if let Some(event) = event { (sink.borrow_mut())(&map(event))?; }
                if !buffer.lock().expect("observation buffer").events.is_empty() { ready.notify_one(); }
            }
        }
    };
    // Invocation has finished: flush at most the fixed budget, then an out-of-band
    // integrity marker which cannot itself be displaced by a full observation queue.
    let (events, dropped, transport) = {
        let mut buffer = buffer.lock().expect("observation buffer");
        (
            std::mem::take(&mut buffer.events),
            buffer.dropped,
            buffer.transport.clone(),
        )
    };
    let final_protocol = final_protocol.lock().expect("observation protocol").clone();
    for (event, _) in events {
        (sink.borrow_mut())(&map(event))?;
    }
    if let Some(transport) = transport {
        let final_protocol = if transport == "websocket" {
            "openai.responses".to_owned()
        } else {
            final_protocol
        };
        if dropped > 0 {
            (sink.borrow_mut())(&map(ProtocolObservation { protocol: final_protocol, transport,
                direction: "received".into(), kind: "capture_integrity".into(),
                body: serde_json::json!({"dropped_count": dropped, "reason": "observation_capacity_exceeded"}).to_string(),
                encoding: "utf8".into(), status: None }))?;
        } else if result.is_ok() {
            (sink.borrow_mut())(&map(ProtocolObservation {
                protocol: final_protocol,
                transport,
                direction: "received".into(),
                kind: "stream_end".into(),
                body: String::new(),
                encoding: "utf8".into(),
                status: None,
            }))?;
        }
    }
    result
}

pub trait ObserveRequest {
    fn send_observed(self) -> impl Future<Output = reqwest::Result<reqwest::Response>> + Send;
}
impl ObserveRequest for reqwest::RequestBuilder {
    async fn send_observed(self) -> reqwest::Result<reqwest::Response> {
        let (client, request) = self.build_split();
        let request = request?;
        let protocol = if request.url().path().ends_with("/chat/completions") {
            Some("openai.chat_completions")
        } else if request.url().path().ends_with("/responses") {
            Some("openai.responses")
        } else if request.url().path().ends_with("/messages") {
            Some("anthropic.messages")
        } else if request.url().path().contains(":streamGenerateContent") {
            Some("gemini.generate_content")
        } else {
            None
        };
        if let Some(protocol) = protocol {
            let _ = OBSERVATION.try_with(|context| {
                *context.protocol.lock().expect("observation protocol") = protocol.into()
            });
        }
        // These are exactly the serialized bytes passed to reqwest, including protocol restoration.
        // No credential-bearing headers or query strings are included.
        if let Some(bytes) = request.body().and_then(reqwest::Body::as_bytes) {
            record("http", "prepared", "request_prepared", bytes, None);
        }
        let response = client.execute(request).await?;
        record(
            "http",
            "received",
            "response_head",
            &[],
            Some(response.status().as_u16()),
        );
        Ok(response)
    }
}

pub trait ObserveResponse {
    fn observed_text(self) -> impl Future<Output = reqwest::Result<String>> + Send;
}
impl ObserveResponse for reqwest::Response {
    async fn observed_text(self) -> reqwest::Result<String> {
        let bytes = self.bytes().await?;
        record("http", "received", "response_body", &bytes, None);
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

pub fn observe_chunk<B: AsRef<[u8]>>(chunk: &reqwest::Result<B>) {
    if let Ok(bytes) = chunk {
        record("sse", "received", "response_body", bytes.as_ref(), None);
    }
}

// Lossless encoding for non-UTF8 network fragments.
fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as usize;
        let b = chunk.get(1).copied().unwrap_or(0) as usize;
        let c = chunk.get(2).copied().unwrap_or(0) as usize;
        output.push(TABLE[a >> 2] as char);
        output.push(TABLE[((a & 3) << 4) | (b >> 4)] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((b & 15) << 2) | (c >> 6)] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[c & 63] as char
        } else {
            '='
        });
    }
    output
}

/// Provider-local types remain local; only this mechanical wire adapter is expanded.
#[macro_export]
macro_rules! provider_adapter {
    () => {
        pub(crate) use $crate::{
            enabled, observe_chunk, record, take_enabled, ObserveRequest, ObserveResponse,
            StdioRequestWire,
        };
        impl From<StdioRequestWire> for crate::ProviderStdioRequest {
            fn from(wire: StdioRequestWire) -> Self {
                let (method, input) = wire.into_parts();
                Self { method, input }
            }
        }
        pub(crate) async fn capture<'a, T, F, Fut>(
            protocol: String,
            on_event: F,
            invoke: impl FnOnce($crate::EventSink<'a, crate::ProviderStreamEvent>) -> Fut,
        ) -> anyhow::Result<T>
        where
            F: FnMut(&crate::ProviderStreamEvent) -> anyhow::Result<()> + 'a,
            Fut: std::future::Future<Output = anyhow::Result<T>>,
        {
            $crate::capture(
                protocol,
                on_event,
                |event| crate::ProviderStreamEvent::ProtocolObservation {
                    protocol: event.protocol,
                    transport: event.transport,
                    direction: event.direction,
                    kind: event.kind,
                    body: event.body,
                    encoding: event.encoding,
                    status: event.status,
                },
                invoke,
            )
            .await
        }
    };
}

#[cfg(test)]
#[path = "_tests/capture.rs"]
mod tests;
