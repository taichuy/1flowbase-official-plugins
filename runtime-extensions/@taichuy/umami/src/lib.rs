mod hosts;
mod remote;
mod store;

use runtime_extension_sdk::MultiplexEmitter;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
};
use tokio::sync::Mutex;

#[derive(Debug)]
pub(crate) struct Failure(pub &'static str);
pub(crate) type Result<T> = std::result::Result<T, Failure>;

#[derive(Default, Deserialize)]
pub(crate) struct Payload {
    #[serde(default)]
    pub path: Value,
    #[serde(default)]
    pub query: Value,
    #[serde(default)]
    pub body: Value,
}
#[derive(Deserialize)]
struct Invocation {
    method: String,
    input: InvocationInput,
}
#[derive(Deserialize)]
struct InvocationInput {
    handler: String,
    input_payload: Payload,
}

/// Only locks live in the worker. Tokens belong exclusively to Host ephemeral storage.
#[derive(Clone, Default)]
pub struct Runtime {
    locks: Rc<RefCell<HashMap<String, Weak<Mutex<()>>>>>,
}
impl Runtime {
    fn host_lock(&self, key: &str) -> Rc<Mutex<()>> {
        let mut locks = self.locks.borrow_mut();
        locks.retain(|_, value| value.strong_count() > 0);
        if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Rc::new(Mutex::new(()));
        locks.insert(key.to_owned(), Rc::downgrade(&lock));
        lock
    }

    pub async fn dispatch(&self, request: Value, emitter: MultiplexEmitter) -> Value {
        let result = async {
            let request: Invocation =
                serde_json::from_value(request).map_err(|_| Failure("invalid_input"))?;
            if request.method != "execute" {
                return Err(Failure("invalid_input"));
            }
            let input = request.input.input_payload;
            let key = input
                .path
                .get("host_id")
                .and_then(Value::as_str)
                .or_else(|| input.body.get("host_id").and_then(Value::as_str))
                .unwrap_or("hosts");
            let lock = self.host_lock(key);
            // Intentionally async: serialize same-host mutation/authentication and reread
            // Host ephemeral storage after acquiring the lock to coalesce login attempts.
            let _guard = lock.lock().await;
            let ctx = remote::Context::new(emitter);
            match request.input.handler.as_str() {
                "hosts.list" => hosts::list(&ctx, input).await,
                "hosts.create" => hosts::save(&ctx, input, false).await,
                "hosts.update" => hosts::save(&ctx, input, true).await,
                "hosts.delete" => hosts::delete(&ctx, input).await,
                "hosts.test_draft" => hosts::test_draft(&ctx, input).await,
                "hosts.test" => hosts::test(&ctx, input).await,
                handler if handler.starts_with("upstream.") => {
                    remote::upstream(&ctx, &handler[9..], input).await
                }
                _ => Err(Failure("invalid_input")),
            }
        }
        .await;
        match result {
            Ok(value) => json!({"ok":true,"result":value}),
            Err(Failure(code)) => {
                json!({"ok":true,"result":{"success":false,"error":{"code":code}}})
            }
        }
    }
}

pub(crate) fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or(Failure("invalid_input"))
}

#[cfg(test)]
#[path = "_tests/runtime.rs"]
mod tests;
