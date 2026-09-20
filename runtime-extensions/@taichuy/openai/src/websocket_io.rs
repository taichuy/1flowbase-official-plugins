//! One task owns each physical socket, including while no invocation is running.
use super::*;
use std::sync::Mutex;
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore};

const EVENT_COUNT: usize = 64;
const COMMAND_COUNT: usize = 8;
const BYTE_LIMIT: usize = 16 * 1024 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn config() -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(BYTE_LIMIT))
        .max_frame_size(Some(BYTE_LIMIT))
        .max_write_buffer_size(BYTE_LIMIT + 1024)
}

#[derive(Debug, Clone, Copy)]
enum StreamEnd {
    Error,
    Close,
    Eof,
}

#[derive(Debug, Clone)]
pub(super) struct Failure {
    pub kind: &'static str,
    pub close_code: Option<u16>,
    pub category: &'static str,
    pub io_kind: Option<&'static str>,
    pub phase: &'static str,
    pub idle_duration_ms: u64,
    end: StreamEnd,
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Responses websocket {}", self.kind)?;
        if let Some(code) = self.close_code {
            write!(f, " (close code {code})")?;
        }
        Ok(())
    }
}
impl std::error::Error for Failure {}
impl Failure {
    fn new(kind: &'static str) -> Self {
        Self {
            kind,
            close_code: None,
            category: "transport_disconnected",
            io_kind: None,
            phase: "idle",
            idle_duration_ms: 0,
            end: StreamEnd::Error,
        }
    }
    fn invalidates_buffered_events(&self) -> bool {
        matches!(
            self.kind,
            "queue_count_limit"
                | "queue_bytes_limit"
                | "capacity"
                | "write_buffer_full"
                | "owner_cancelled"
                | "owner_stopped"
        )
    }
    fn stream_end(self) -> Option<Result<Message>> {
        match self.end {
            StreamEnd::Close => Some(Ok(Message::Close(None))),
            StreamEnd::Eof => None,
            StreamEnd::Error => Some(Err(self.into())),
        }
    }
    fn websocket(error: WebSocketError) -> Self {
        let io_kind = match &error {
            WebSocketError::Io(error) => Some(recovery_diagnostics::io_kind(error.kind())),
            _ => None,
        };
        let kind = match error {
            WebSocketError::ConnectionClosed => "connection_closed",
            WebSocketError::AlreadyClosed => "already_closed",
            WebSocketError::Io(_) => "io",
            WebSocketError::Tls(_) => "tls",
            WebSocketError::Capacity(_) => "capacity",
            WebSocketError::Protocol(_) => "protocol",
            WebSocketError::WriteBufferFull(_) => "write_buffer_full",
            WebSocketError::Utf8 => "utf8",
            WebSocketError::AttackAttempt => "attack_attempt",
            WebSocketError::Url(_) => "url",
            WebSocketError::Http(_) => "http",
            WebSocketError::HttpFormat(_) => "http_format",
        };
        Self {
            io_kind,
            ..Self::new(kind)
        }
    }
}

#[derive(Debug)]
struct Buffered {
    message: Message,
    _bytes: OwnedSemaphorePermit,
}
#[derive(Debug)]
struct Command {
    buffered: Buffered,
    ack: oneshot::Sender<Result<(), Failure>>,
}
#[derive(Debug)]
pub(super) struct SocketOwner {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<Buffered>,
    command_bytes: Arc<Semaphore>,
    terminal: Arc<Mutex<Health>>,
    close: Option<oneshot::Sender<(Duration, oneshot::Sender<Option<close::NoAckReason>>)>>,
    task: tokio::task::JoinHandle<()>,
    peer_ack: Arc<std::sync::atomic::AtomicBool>,
}
#[derive(Debug)]
struct Health {
    failure: Option<Failure>,
    phase: &'static str,
    idle_since: Instant,
}
fn retain_failure(terminal: &Mutex<Health>, mut failure: Failure) -> Failure {
    let mut health = terminal.lock().unwrap();
    failure.phase = health.phase;
    failure.idle_duration_ms = if health.phase == "idle" {
        health.idle_since.elapsed().as_millis().min(86_400_000) as u64
    } else {
        0
    };
    health.failure.get_or_insert(failure).clone()
}
/// Dropping an incomplete invocation invalidates its socket; stale frames cannot be reused.
pub(super) struct Activity {
    terminal: Arc<Mutex<Health>>,
    abort: tokio::task::AbortHandle,
    completed: bool,
}
impl Activity {
    pub fn complete(mut self) {
        let mut health = self.terminal.lock().unwrap();
        health.phase = "idle";
        health.idle_since = Instant::now();
        self.completed = true;
    }
}
impl Drop for Activity {
    fn drop(&mut self) {
        if !self.completed {
            retain_failure(&self.terminal, Failure::new("owner_cancelled"));
            self.abort.abort();
        }
    }
}
fn buffer(message: Message, bytes: &Arc<Semaphore>) -> Result<Buffered, Failure> {
    let size = message.len();
    if size > BYTE_LIMIT {
        return Err(Failure::new("queue_bytes_limit"));
    }
    let permit = bytes
        .clone()
        .try_acquire_many_owned(size as u32)
        .map_err(|_| Failure::new("queue_bytes_limit"))?;
    Ok(Buffered {
        message,
        _bytes: permit,
    })
}
impl SocketOwner {
    pub fn new<S>(mut socket: S) -> Self
    where
        S: futures_util::Sink<Message, Error = WebSocketError>
            + futures_util::Stream<Item = Result<Message, WebSocketError>>
            + Unpin
            + Send
            + 'static,
    {
        let (commands, mut rx) = mpsc::channel::<Command>(COMMAND_COUNT);
        let (tx, events) = mpsc::channel(EVENT_COUNT);
        let (close, mut close_rx) =
            oneshot::channel::<(Duration, oneshot::Sender<Option<close::NoAckReason>>)>();
        let terminal = Arc::new(Mutex::new(Health {
            failure: None,
            phase: "idle",
            idle_since: Instant::now(),
        }));
        let first = terminal.clone();
        let peer_ack = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let acknowledged = peer_ack.clone();
        let event_bytes = Arc::new(Semaphore::new(BYTE_LIMIT));
        let task = tokio::spawn(async move {
            let failure = loop {
                tokio::select! {
                    biased;
                    request = &mut close_rx => {
                        if let Ok((timeout, ack)) = request {
                            let result = close::release_stream(socket, timeout).await;
                            let _ = ack.send(result);
                        }
                        break Failure::new("owner_stopped");
                    }
                    command = rx.recv() => {
                        let Some(command) = command else { break Failure::new("owner_cancelled") };
                        let result = write(&mut socket, command.buffered.message).await;
                        // Keep the byte permit until the write completes.
                        let failure = result.as_ref().err().cloned();
                        if let Some(error) = &failure { retain_failure(&first, error.clone()); }
                        let _ = command.ack.send(result);
                        if let Some(error) = failure { break error; }
                    }
                    incoming = socket.next() => {
                        match incoming {
                            Some(Ok(Message::Ping(_))) => {
                                // tungstenite queued the matching Pong; flush it exactly once.
                                if let Err(error) = flush(&mut socket).await { break error; }
                            }
                            Some(Ok(Message::Pong(_))) => {}
                            Some(Ok(Message::Close(frame))) => {
                                let failure = Failure { end: StreamEnd::Close, category: recovery_diagnostics::close_category(frame.as_ref()), close_code: frame.as_ref().map(|f| u16::from(f.code)), ..Failure::new("connection_closed") };
                                retain_failure(&first, failure.clone());
                                if flush(&mut socket).await.is_ok() { acknowledged.store(true, std::sync::atomic::Ordering::Release); }
                                break failure;
                            }
                            Some(Ok(message)) => {
                                let buffered = match buffer(message, &event_bytes) { Ok(v) => v, Err(e) => break e };
                                if let Err(error) = tx.try_send(buffered) {
                                    break Failure::new(if matches!(error, mpsc::error::TrySendError::Full(_)) { "queue_count_limit" } else { "owner_cancelled" });
                                }
                            }
                            Some(Err(error)) => break Failure::websocket(error),
                            None => break Failure { end: StreamEnd::Eof, ..Failure::new("connection_closed") },
                        }
                    }
                }
            };
            retain_failure(&first, failure);
        });
        Self {
            commands,
            events,
            command_bytes: Arc::new(Semaphore::new(BYTE_LIMIT)),
            terminal,
            close: Some(close),
            task,
            peer_ack,
        }
    }
    pub fn failure(&self) -> Option<Failure> {
        self.terminal.lock().unwrap().failure.clone()
    }
    pub fn activity(&self) -> Activity {
        self.terminal.lock().unwrap().phase = "active";
        Activity {
            terminal: self.terminal.clone(),
            abort: self.task.abort_handle(),
            completed: false,
        }
    }
    fn invalidate(&self, failure: Failure) -> anyhow::Error {
        let failure = retain_failure(&self.terminal, failure);
        self.task.abort();
        failure.into()
    }
    pub async fn send(&self, message: Message) -> Result<()> {
        if let Some(error) = self.failure() {
            return Err(error.into());
        }
        let buffered = buffer(message, &self.command_bytes).map_err(|e| self.invalidate(e))?;
        let (ack, result) = oneshot::channel();
        self.commands
            .try_send(Command { buffered, ack })
            .map_err(|e| {
                self.invalidate(Failure::new(
                    if matches!(e, mpsc::error::TrySendError::Full(_)) {
                        "queue_count_limit"
                    } else {
                        "owner_stopped"
                    },
                ))
            })?;
        result
            .await
            .unwrap_or_else(|_| {
                Err(self
                    .failure()
                    .unwrap_or_else(|| Failure::new("owner_stopped")))
            })
            .map_err(Into::into)
    }
    pub async fn next(&mut self) -> Option<Result<Message>> {
        // Resource/cancellation faults invalidate the mailbox. Transport end is
        // ordered after all frames already read, even when the consumer is slow.
        if let Some(failure) = self.failure() {
            if failure.invalidates_buffered_events() {
                return Some(Err(failure.into()));
            }
            if let Ok(event) = self.events.try_recv() {
                return Some(Ok(event.message));
            }
            return failure.stream_end();
        }
        let event = self.events.recv().await;
        if let Some(failure) = self.failure() {
            if failure.invalidates_buffered_events() {
                return Some(Err(failure.into()));
            }
            if event.is_none() {
                return failure.stream_end();
            }
        }
        event.map(|event| Ok(event.message))
    }
    pub async fn close(mut self, timeout: Duration) -> Option<close::NoAckReason> {
        self.terminal.lock().unwrap().phase = "closing";
        let (ack, result) = oneshot::channel();
        if self.close.take().unwrap().send((timeout, ack)).is_err() {
            let _ = tokio::time::timeout(timeout, &mut self.task).await;
            return if self.peer_ack.load(std::sync::atomic::Ordering::Acquire) {
                None
            } else {
                Some(close::NoAckReason::TransportError)
            };
        }
        match tokio::time::timeout(timeout, result).await {
            Ok(Ok(reason)) => reason,
            Ok(Err(_)) => Some(close::NoAckReason::TransportError),
            Err(_) => Some(close::NoAckReason::Timeout),
        }
    }
}
impl Drop for SocketOwner {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn write<S: futures_util::Sink<Message, Error = WebSocketError> + Unpin>(
    socket: &mut S,
    message: Message,
) -> Result<(), Failure> {
    tokio::time::timeout(WRITE_TIMEOUT, socket.send(message))
        .await
        .map_err(|_| Failure::new("write_timeout"))?
        .map_err(Failure::websocket)
}
async fn flush<S: futures_util::Sink<Message, Error = WebSocketError> + Unpin>(
    socket: &mut S,
) -> Result<(), Failure> {
    tokio::time::timeout(WRITE_TIMEOUT, socket.flush())
        .await
        .map_err(|_| Failure::new("write_timeout"))?
        .map_err(Failure::websocket)
}

#[cfg(test)]
#[path = "_tests/websocket_io.rs"]
mod tests;
