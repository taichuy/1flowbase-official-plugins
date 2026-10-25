//! Browser resources are local to one Host-authenticated actor/workspace worker.
//! V8 pages never leave the SDK's current-thread LocalSet.
mod model;
mod page;
use model::*;
use obscura_browser::BrowserContext;
use runtime_extension_sdk::MultiplexEmitter;
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc, time::Duration};
use tokio::sync::{watch, Mutex, Notify};

#[derive(Default)]
struct State {
    owner: Option<(String, String)>,
    browsers: BTreeMap<String, Browser>,
    maintenance_started: bool,
    changed: Rc<Notify>,
}
#[derive(Clone, Default)]
pub struct Service {
    state: Rc<RefCell<State>>,
}
impl Service {
    pub fn new() -> Self {
        Self::default()
    }
    pub async fn dispatch(&self, request: Value, emitter: MultiplexEmitter) -> Value {
        let result = async {
            let invocation: Invocation =
                serde_json::from_value(request).map_err(|_| "invalid_invocation")?;
            let c = &invocation.input.call_context;
            self.authorize(c)?;
            self.start_maintenance();
            let remaining = u64::try_from(c.deadline_unix_ms)
                .map_err(|_| "deadline_exceeded")?
                .checked_sub(now())
                .filter(|n| *n > 0)
                .ok_or("deadline_exceeded")?;
            tokio::time::timeout(
                Duration::from_millis(remaining),
                self.execute(
                    &invocation.input.handler,
                    &invocation.input.input_payload,
                    Some(&emitter),
                    Some(u64::try_from(c.deadline_unix_ms).map_err(|_| "deadline_exceeded")?),
                ),
            )
            .await
            .map_err(|_| "deadline_exceeded")?
        }
        .await;
        match result {
            Ok(output) => json!({"ok":true,"result":output}),
            // Use the Host's CapabilityStdioResponse failure path before any
            // HTTP/MCP success-output mapping (for example screenshot /data).
            // Codes are static safe identifiers; page/provider messages stay private.
            Err(code) => json!({"ok":false,"error":{"message":code}}),
        }
    }
    fn authorize(&self, c: &CallContext) -> Result<()> {
        if c.actor_id.trim().is_empty() || c.workspace_id.trim().is_empty() {
            return Err("missing_call_context");
        }
        if u64::try_from(c.deadline_unix_ms).map_err(|_| "deadline_exceeded")? <= now() {
            return Err("deadline_exceeded");
        }
        let mut state = self.state.borrow_mut();
        let owner = (c.actor_id.clone(), c.workspace_id.clone());
        match &state.owner {
            Some(existing) if existing != &owner => Err("scope_mismatch"),
            Some(_) => Ok(()),
            None => {
                state.owner = Some(owner);
                Ok(())
            }
        }
    }
    fn start_maintenance(&self) {
        if self.state.borrow().maintenance_started {
            return;
        }
        self.state.borrow_mut().maintenance_started = true;
        let weak = Rc::downgrade(&self.state);
        tokio::task::spawn_local(async move {
            loop {
                let Some(state) = weak.upgrade() else {
                    break;
                };
                let changed = state.borrow().changed.clone();
                let notified = changed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                let deadline = {
                    let mut state = state.borrow_mut();
                    state.browsers.retain(|_, b| {
                        b.activity.active.get() > 0
                            || b.activity.touched.get().elapsed()
                                < Duration::from_millis(b.idle_timeout_ms)
                    });
                    state
                        .browsers
                        .values()
                        .filter(|b| b.activity.active.get() == 0)
                        .filter_map(|b| {
                            b.activity
                                .touched
                                .get()
                                .checked_add(Duration::from_millis(b.idle_timeout_ms))
                        })
                        .min()
                };
                drop(state);
                match deadline {
                    Some(deadline) => {
                        tokio::select! { _=&mut notified => {}, _=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>{} }
                    }
                    None => notified.await,
                }
            }
        });
    }

    async fn execute(
        &self,
        handler: &str,
        payload: &Value,
        emitter: Option<&MultiplexEmitter>,
        deadline_unix_ms: Option<u64>,
    ) -> Result<Value> {
        let body = payload.get("body").unwrap_or(&Value::Null);
        let path = payload.get("path").unwrap_or(&Value::Null);
        let query = payload.get("query").unwrap_or(&Value::Null);
        match handler {
            "browser-obscura.browsers.list" => {
                let page = query_number(query, "page", 1, 1)?;
                let page_size = query_number(query, "page_size", 20, 1)?;
                let state = self.state.borrow();
                let search = query
                    .get("search")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_lowercase();
                let matched: Vec<_> = state
                    .browsers
                    .values()
                    .filter(|b| b.name.to_lowercase().contains(&search))
                    .collect();
                let offset = usize::try_from(
                    page.checked_sub(1)
                        .and_then(|p| p.checked_mul(page_size))
                        .ok_or("invalid_input")?,
                )
                .map_err(|_| "invalid_input")?;
                let page_limit = usize::try_from(page_size).map_err(|_| "invalid_input")?;
                let items: Vec<_> = matched
                    .iter()
                    .skip(offset)
                    .take(page_limit)
                    .map(|b| b.summary())
                    .collect();
                return Ok(
                    json!({"success":true,"items":items,"page":page,"page_size":page_size,"total":matched.len()}),
                );
            }
            "browser-obscura.browsers.create" => {
                let name = string(body, "name")?.trim();
                if name.is_empty() {
                    return Err("invalid_input");
                }
                // Default five minutes; caller chooses any representable positive duration.
                let idle_timeout_ms = number(body, "idle_timeout_ms", 300_000, 1)?;
                tokio::time::Instant::now()
                    .checked_add(Duration::from_millis(idle_timeout_ms))
                    .ok_or("invalid_input")?;
                let lease = match emitter {
                    Some(e) => Some(
                        e.retain_resource()
                            .await
                            .map_err(|_| "retention_unavailable")?,
                    ),
                    None => None,
                };
                let browser_id = id();
                let browser = Browser {
                    context: Arc::new(BrowserContext::new(browser_id.clone())),
                    browser_id: browser_id.clone(),
                    name: name.into(),
                    created: now(),
                    activity: Activity::new(self.state.borrow().changed.clone()),
                    idle_timeout_ms,
                    tabs: BTreeMap::new(),
                    closed: watch::channel(false).0,
                    _lease: lease,
                };
                let output = json!({"success":true,"browser":browser.summary()});
                self.state.borrow_mut().browsers.insert(browser_id, browser);
                self.state.borrow().changed.notify_one();
                return Ok(output);
            }
            _ => {}
        }
        let browser_id = string(path, "browser_id")?;
        if handler == "browser-obscura.browsers.close" {
            self.state
                .borrow_mut()
                .browsers
                .remove(browser_id)
                .ok_or("browser_not_found")?;
            return Ok(json!({"success":true,"closed":true}));
        }
        let (mut closed, tabs, context, browser_summary, _activity) = {
            let mut state = self.state.borrow_mut();
            let browser = state
                .browsers
                .get_mut(browser_id)
                .ok_or("browser_not_found")?;
            let activity = browser.activity.start();
            (
                browser.closed.subscribe(),
                browser.tabs.clone(),
                browser.context.clone(),
                browser.summary(),
                activity,
            )
        };
        if handler == "browser-obscura.browsers.get" || handler == "browser-obscura.tabs.list" {
            let mut items = Vec::new();
            for tab in tabs.values() {
                tab.interrupt.notify_one();
                let tab = tokio::select! { biased; _ = closed.changed() => return Err("browser_closed"), tab = tab.inner.lock() => tab };
                items.push(tab.summary(browser_id));
            }
            return Ok(if handler.ends_with(".get") {
                json!({"success":true,"browser":browser_summary,"tabs":items})
            } else {
                json!({"success":true,"total":items.len(),"items":items})
            });
        }
        if handler == "browser-obscura.tabs.create" {
            let mut page = Tab::new(context);
            if let Some(url) = body.get("url") {
                let url = url.as_str().ok_or("invalid_input")?;
                let timeout = deadline_unix_ms
                    .map(|d| d.saturating_sub(now()).min(30_000))
                    .unwrap_or(30_000);
                if timeout == 0 {
                    return Err("deadline_exceeded");
                }
                tokio::select! { biased;
                    _ = closed.changed() => return Err("browser_closed"),
                    result = page::navigate(&mut page,url,timeout) => result?,
                }
            }
            // Publish only a fully created tab. Cancellation during navigation drops
            // the owned page rather than leaving an unreachable tab in the browser.
            let tab_id = page.page.id.clone();
            let summary = page.summary(browser_id);
            let tab = TabHandle {
                closed: page.closed.clone(),
                inner: Rc::new(Mutex::new(page)),
                interrupt: Rc::new(Notify::new()),
            };
            self.state
                .borrow_mut()
                .browsers
                .get_mut(browser_id)
                .ok_or("browser_closed")?
                .tabs
                .insert(tab_id, tab.clone());
            drive_tab(&tab, closed.clone());
            return Ok(json!({"success":true,"tab":summary}));
        }
        let tab_id = string(path, "tab_id")?;
        let tab = tabs.get(tab_id).ok_or("tab_not_found")?.clone();
        if handler == "browser-obscura.tabs.close" {
            // Signal immediately, before waiting on an in-flight navigation's lock.
            self.state
                .borrow_mut()
                .browsers
                .get_mut(browser_id)
                .ok_or("browser_closed")?
                .tabs
                .remove(tab_id);
            // The independent cancellation sender is held by the task below.
            tab.closed.send_replace(true);
            return Ok(json!({"success":true,"closed":true}));
        }
        let mut tab_closed = tab.closed.subscribe();
        tab.interrupt.notify_one();
        let _wake = TabWake(tab.interrupt.clone());
        tokio::select! {
            biased;
            _ = closed.changed() => Err("browser_closed"),
            _ = tab_closed.changed() => Err("tab_closed"),
            result = async {
                let mut tab = tab.inner.lock().await;
                page::execute(&mut tab,browser_id,handler,body,deadline_unix_ms).await
            } => result,
        }
    }
}
fn drive_tab(tab: &TabHandle, mut browser_closed: watch::Receiver<bool>) {
    let weak = Rc::downgrade(&tab.inner);
    let interrupt = tab.interrupt.clone();
    let mut tab_closed = tab.closed.subscribe();
    tokio::task::spawn_local(async move {
        loop {
            let Some(tab) = weak.upgrade() else {
                break;
            };
            let mut page = tokio::select! {biased;
                _=browser_closed.changed()=>break,
                _=tab_closed.changed()=>break,
                page=tab.lock()=>page,
            };
            if matches!(page.status.get(), Status::Failed) {
                drop(page);
                drop(tab);
                tokio::select! {biased; _=browser_closed.changed()=>break, _=tab_closed.changed()=>break, _=interrupt.notified()=>{} }
                continue;
            }
            if page.page.has_pending_navigation() {
                // A document-triggered navigation is ordered before the next tab
                // command. Only close cancels it; ordinary polling must not abort it.
                let result = tokio::select! {biased;
                    _=browser_closed.changed()=>break,
                    _=tab_closed.changed()=>break,
                    result=page::turn(&mut page)=>result,
                };
                if result.is_err() {
                    page.status.set(Status::Failed);
                }
                continue;
            }
            let idle = tokio::select! {biased;
                _=browser_closed.changed()=>break,
                _=tab_closed.changed()=>break,
                _=interrupt.notified()=>false,
                result=page::autonomous_turn(&mut page)=>match result {Ok(idle)=>idle,Err(_)=>{page.status.set(Status::Failed);false}},
            };
            drop(page);
            drop(tab);
            if idle {
                // No pending timers/network remain. Only a command can introduce
                // new page work; don't repeatedly poll an idle V8 isolate.
                tokio::select! {biased; _=browser_closed.changed()=>break, _=tab_closed.changed()=>break, _=interrupt.notified()=>{} }
            }
            tokio::task::yield_now().await;
        }
    });
}

#[cfg(test)]
#[path = "_tests/engine.rs"]
mod tests;

#[cfg(test)]
#[path = "_tests/lifecycle.rs"]
mod lifecycle_tests;
