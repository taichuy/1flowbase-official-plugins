//! Plaintext transport observations. Never capture headers, URLs, or authentication exchanges.
use crate::ProviderStreamEvent;
use anyhow::Result;
use std::{
    future::Future,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

/// This permissive outer envelope is compatible with old stdio hosts and providers.
/// The strict business input schema stays unchanged.
#[derive(serde::Deserialize)]
pub(crate) struct StdioRequestWire {
    method: String,
    #[serde(default)]
    input: serde_json::Value,
    #[serde(default)]
    host_capabilities: Vec<String>,
}

impl From<StdioRequestWire> for crate::ProviderStdioRequest {
    fn from(mut wire: StdioRequestWire) -> Self {
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
        Self {
            method: wire.method,
            input: wire.input,
        }
    }
}

/// Only the stdio decoder injects this temporary marker for the trusted streaming entry.
pub(crate) fn enabled(input: &serde_json::Value) -> bool {
    input
        .get("host_capabilities")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|capabilities| {
            capabilities
                .iter()
                .any(|capability| capability.as_str() == Some("protocol_observation_v1"))
        })
}

pub(crate) fn take_enabled(input: &mut serde_json::Value) -> bool {
    let enabled = enabled(input);
    if let Some(input) = input.as_object_mut() {
        input.remove("host_capabilities");
    }
    enabled
}

type EventSink = Box<dyn FnMut(&ProviderStreamEvent) -> Result<()> + Send>;
struct Context {
    protocol: Mutex<String>,
    sender: mpsc::UnboundedSender<ProviderStreamEvent>,
    transport: Arc<Mutex<Option<String>>>,
}
tokio::task_local! { static OBSERVATION: Context; }

pub(crate) fn record(
    transport: &str,
    direction: &str,
    kind: &str,
    bytes: &[u8],
    status: Option<u16>,
) {
    let _ = OBSERVATION.try_with(|context| {
        *context.transport.lock().expect("observation transport") = Some(transport.to_owned());
        let (body, encoding) = match std::str::from_utf8(bytes) {
            Ok(text) => (text.to_owned(), "utf8"),
            Err(_) => (encode_base64(bytes), "base64"),
        };
        // The receiver is owned by capture until invocation and event drain complete.
        let _ = context
            .sender
            .send(ProviderStreamEvent::ProtocolObservation {
                protocol: if transport == "websocket" {
                    "openai.responses".into()
                } else {
                    context
                        .protocol
                        .lock()
                        .expect("observation protocol")
                        .clone()
                },
                transport: transport.into(),
                direction: direction.into(),
                kind: kind.into(),
                body,
                encoding: encoding.into(),
                status,
            });
    });
}

pub(crate) async fn capture<T, F, Fut>(
    protocol: String,
    mut on_event: F,
    invoke: impl FnOnce(EventSink) -> Fut,
) -> Result<T>
where
    F: FnMut(&ProviderStreamEvent) -> Result<()>,
    Fut: Future<Output = Result<T>>,
{
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let event_sender = sender.clone();
    let transport = Arc::new(Mutex::new(None));
    let context = Context {
        protocol: Mutex::new(protocol),
        sender,
        transport: transport.clone(),
    };
    let future = OBSERVATION.scope(context, async move {
        let result = invoke(Box::new(move |event| {
            event_sender
                .send(event.clone())
                .map_err(|_| anyhow::anyhow!("provider event receiver closed"))
        }))
        .await;
        let last_transport = transport.lock().expect("observation transport").clone();
        if let Some(transport) = last_transport {
            // Successful transport readers validate their protocol terminal before returning.
            // Failures deliberately omit stream_end, so the host reports incomplete evidence.
            if result.is_ok() {
                record(&transport, "received", "stream_end", &[], None);
            }
        }
        result
    });
    tokio::pin!(future);
    let result = loop {
        tokio::select! {
            biased;
            Some(event) = receiver.recv() => on_event(&event)?,
            result = &mut future => break result,
        }
    };
    while let Ok(event) = receiver.try_recv() {
        on_event(&event)?;
    }
    result
}

pub(crate) trait ObserveRequest {
    async fn send_observed(self) -> reqwest::Result<reqwest::Response>;
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
            record("http", "sent", "request", bytes, None);
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

pub(crate) trait ObserveResponse {
    async fn observed_text(self) -> reqwest::Result<String>;
}
impl ObserveResponse for reqwest::Response {
    async fn observed_text(self) -> reqwest::Result<String> {
        let bytes = self.bytes().await?;
        record("http", "received", "response_body", &bytes, None);
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

pub(crate) fn observe_chunk<B: AsRef<[u8]>>(chunk: &reqwest::Result<B>) {
    if let Ok(bytes) = chunk {
        record("sse", "received", "response_body", bytes.as_ref(), None);
    }
}

// Keep the transport module dependency-free across independently built provider packages.
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

#[cfg(test)]
#[path = "_tests/protocol_observation.rs"]
mod tests;
