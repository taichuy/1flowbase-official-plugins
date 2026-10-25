use obscura_browser::{BrowserContext, Page};
use obscura_dom::NodeId;
pub(crate) use runtime_extension_sdk::ManagedServiceCallContext as CallContext;
use runtime_extension_sdk::MultiplexResourceLease;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    cell::Cell,
    collections::BTreeMap,
    rc::Rc,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{watch, Mutex, Notify};

pub(crate) type Result<T> = std::result::Result<T, &'static str>;
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub(crate) fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub(crate) fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("invalid_input")
}
pub(crate) fn number(value: &Value, key: &str, default: u64, min: u64) -> Result<u64> {
    let n = match value.get(key) {
        None => default,
        Some(v) => v.as_u64().ok_or("invalid_input")?,
    };
    if n < min {
        return Err("invalid_input");
    }
    Ok(n)
}
#[derive(Deserialize)]
pub(crate) struct Invocation {
    pub input: InvocationInput,
}
#[derive(Deserialize)]
pub(crate) struct InvocationInput {
    pub handler: String,
    pub input_payload: Value,
    pub call_context: CallContext,
}
#[derive(Clone, Copy)]
pub(crate) enum Status {
    Ready,
    Loading,
    Failed,
}
pub(crate) struct Tab {
    pub page: Page,
    pub status: Rc<Cell<Status>>,
    pub snapshot_id: Option<String>,
    pub refs: BTreeMap<String, NodeId>,
    pub closed: watch::Sender<bool>,
    pub script_deadline_unix_ms: u64,
}
impl Tab {
    pub fn new(context: Arc<BrowserContext>) -> Self {
        let mut page = Page::new(id(), context);
        page.navigate_blank();
        Self {
            page,
            status: Rc::new(Cell::new(Status::Ready)),
            snapshot_id: None,
            refs: BTreeMap::new(),
            closed: watch::channel(false).0,
            script_deadline_unix_ms: u64::MAX,
        }
    }
    pub fn invalidate(&mut self) {
        self.snapshot_id = None;
        self.refs.clear();
    }
    pub fn summary(&self, browser_id: &str) -> Value {
        json!({"browser_id":browser_id,"tab_id":self.page.id,"title":self.page.title,"url":self.page.url_string(),
            "status": match self.status.get() { Status::Ready => "ready", Status::Loading => "loading", Status::Failed => "failed" },
            "snapshot_id":self.snapshot_id})
    }
}
pub(crate) struct Browser {
    pub browser_id: String,
    pub name: String,
    pub created: u64,
    pub activity: Rc<Activity>,
    pub idle_timeout_ms: u64,
    pub context: Arc<BrowserContext>,
    pub tabs: BTreeMap<String, TabHandle>,
    pub closed: watch::Sender<bool>,
    pub _lease: Option<MultiplexResourceLease>,
}
impl Browser {
    pub fn summary(&self) -> Value {
        json!({"browser_id":self.browser_id,"name":self.name,"status":"running","created_at_unix_ms":self.created,
            "last_activity_at_unix_ms":self.activity.at.get(),"tab_count":self.tabs.len(),"idle_timeout_ms":self.idle_timeout_ms})
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        self.closed.send_replace(true);
    }
}
/// Cancellation must leave a failed tab, never a successful navigation receipt.
pub(crate) struct OperationGuard {
    status: Rc<Cell<Status>>,
    finished: bool,
}
impl OperationGuard {
    pub fn start(tab: &Tab) -> Self {
        tab.status.set(Status::Loading);
        Self {
            status: tab.status.clone(),
            finished: false,
        }
    }
    pub fn finish(mut self) {
        self.status.set(Status::Ready);
        self.finished = true;
    }
}
impl Drop for OperationGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.status.set(Status::Failed);
        }
    }
}

#[derive(Clone)]
pub(crate) struct TabHandle {
    pub inner: Rc<Mutex<Tab>>,
    pub closed: watch::Sender<bool>,
    pub interrupt: Rc<Notify>,
}

pub(crate) struct Activity {
    pub active: Cell<usize>,
    pub touched: Cell<Instant>,
    pub at: Cell<u64>,
    pub changed: Rc<Notify>,
}
impl Activity {
    pub fn new(changed: Rc<Notify>) -> Rc<Self> {
        Rc::new(Self {
            active: Cell::new(0),
            touched: Cell::new(Instant::now()),
            at: Cell::new(now()),
            changed,
        })
    }
    pub fn start(self: &Rc<Self>) -> ActivityGuard {
        self.active.set(self.active.get() + 1);
        self.touch();
        ActivityGuard(self.clone())
    }
    fn touch(&self) {
        self.touched.set(Instant::now());
        self.at.set(now());
        self.changed.notify_one();
    }
}
pub(crate) struct ActivityGuard(Rc<Activity>);
impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.0.active.set(self.0.active.get() - 1);
        self.0.touch();
    }
}

/// HTTP query values are strings; direct canonical invocations may provide numbers.
pub(crate) fn query_number(value: &Value, key: &str, default: u64, min: u64) -> Result<u64> {
    match value.get(key).and_then(Value::as_str) {
        Some(s) => {
            let n = s.parse::<u64>().map_err(|_| "invalid_input")?;
            if n < min {
                Err("invalid_input")
            } else {
                Ok(n)
            }
        }
        None => number(value, key, default, min),
    }
}

pub(crate) struct TabWake(pub Rc<Notify>);
impl Drop for TabWake {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
