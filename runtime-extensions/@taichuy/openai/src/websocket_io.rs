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

#[derive(Debug, Clone)]
pub(super) struct Failure {
    pub kind: &'static str,
    pub close_code: Option<u16>,
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
        }
    }
    fn websocket(error: WebSocketError) -> Self {
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
        Self::new(kind)
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
    terminal: Arc<Mutex<Option<Failure>>>,
    close: Option<oneshot::Sender<(Duration, oneshot::Sender<Option<close::NoAckReason>>)>>,
    task: tokio::task::JoinHandle<()>,
    peer_ack: Arc<std::sync::atomic::AtomicBool>,
}
fn retain_failure(terminal: &Mutex<Option<Failure>>, failure: Failure) -> Failure {
    terminal.lock().unwrap().get_or_insert(failure).clone()
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
        let terminal = Arc::new(Mutex::new(None));
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
                                let failure = Failure { kind: "connection_closed", close_code: frame.map(|f| u16::from(f.code)) };
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
                            None => break Failure::new("connection_closed"),
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
        self.terminal.lock().unwrap().clone()
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
        // Resource faults invalidate buffered success; orderly peer close preserves prior frames.
        if let Some(error) = self.failure() {
            if error.kind == "connection_closed" {
                if let Ok(event) = self.events.try_recv() {
                    return Some(Ok(event.message));
                }
            }
            return Some(Err(error.into()));
        }
        let event = self.events.recv().await;
        if let Some(error) = self.failure() {
            if error.kind != "connection_closed" || event.is_none() {
                return Some(Err(error.into()));
            }
        }
        event.map(|event| Ok(event.message))
    }
    pub async fn close(mut self, timeout: Duration) -> Option<close::NoAckReason> {
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
