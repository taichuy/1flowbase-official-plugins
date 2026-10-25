use crate::model::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::time::Duration;

fn eval(tab: &mut Tab, script: &str) -> Result<Value> {
    // Serialization may invoke page-owned JavaScript. Keep the upstream
    // watchdog armed until evaluate_script has serialized the whole result.
    let budget = tab.script_deadline_unix_ms.saturating_sub(now());
    if budget == 0 {
        return Err("deadline_exceeded");
    }
    let watchdog = tab
        .page
        .js
        .as_mut()
        .ok_or("page_unavailable")?
        .arm_watchdog(Duration::from_millis(budget));
    let result = tab.page.evaluate_script(script);
    let fired = tab
        .page
        .js
        .as_mut()
        .ok_or("page_unavailable")?
        .disarm_watchdog(watchdog);
    if fired {
        return Err("javascript_timeout");
    }
    result.map_err(|_| "javascript_error")
}
fn quote(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization")
}
pub(crate) async fn turn(tab: &mut Tab) -> Result<()> {
    drive(tab).await
}
pub(crate) async fn autonomous_turn(tab: &mut Tab) -> Result<bool> {
    tab.page
        .run_autonomous_event_loop_turn()
        .await
        .map_err(|_| "page_event_loop_failed")
}
async fn drive(tab: &mut Tab) -> Result<()> {
    if tab.page.has_pending_navigation() {
        tab.invalidate();
        let guard = OperationGuard::start(tab);
        tab.page
            .process_pending_navigation()
            .await
            .map_err(|_| "navigation_failed")?;
        guard.finish();
    }
    // Commands service ready work without waiting on unrelated distant timers.
    // Upstream explicitly permits cancelling the autonomous future while parked.
    if let Ok(result) = tokio::time::timeout(
        Duration::from_millis(1),
        tab.page.run_autonomous_event_loop_turn(),
    )
    .await
    {
        result.map_err(|_| "page_event_loop_failed")?;
    }
    if tab.page.has_pending_navigation() {
        tab.invalidate();
        let guard = OperationGuard::start(tab);
        tab.page
            .process_pending_navigation()
            .await
            .map_err(|_| "navigation_failed")?;
        guard.finish();
    }
    Ok(())
}
pub(crate) async fn navigate(tab: &mut Tab, target: &str, timeout_ms: u64) -> Result<()> {
    let url = url::Url::parse(target).map_err(|_| "invalid_url")?;
    if !matches!(url.scheme(), "http" | "https") && target != "about:blank" {
        return Err("unsupported_url_scheme");
    }
    tab.invalidate();
    let guard = OperationGuard::start(tab);
    tab.page
        .set_navigation_timeout(Duration::from_millis(timeout_ms));
    tokio::time::timeout(Duration::from_millis(timeout_ms), tab.page.navigate(target))
        .await
        .map_err(|_| "navigation_timeout")?
        .map_err(|_| "navigation_failed")?;
    guard.finish();
    Ok(())
}
fn target(tab: &Tab, body: &Value) -> Result<String> {
    let snapshot = string(body, "snapshot_id")?;
    if tab.snapshot_id.as_deref() != Some(snapshot) {
        return Err("stale_snapshot");
    }
    let reference = string(body, "ref")?;
    let node = tab.refs.get(reference).ok_or("stale_reference")?;
    // Tokens include a random snapshot UUID. A replaced DOM node carrying a
    // copied attribute is rejected because its actual NodeId differs.
    let selector = format!("[data-1flowbase-ref=\"{reference}\"]");
    let actual = tab
        .page
        .with_dom(|dom| dom.query_selector(&selector).ok().flatten())
        .flatten();
    if actual.as_ref() != Some(node) {
        return Err("stale_reference");
    }
    Ok(format!("document.querySelector({})", quote(&selector)))
}
fn snapshot(tab: &mut Tab, browser_id: &str, body: &Value) -> Result<Value> {
    let max_chars =
        usize::try_from(number(body, "max_chars", 12000, 0)?).map_err(|_| "invalid_input")?;
    tab.invalidate();
    let snapshot_id = id();
    if !tab.page.has_js() {
        tab.snapshot_id = Some(snapshot_id.clone());
        return Ok(
            json!({"success":true,"browser_id":browser_id,"tab_id":tab.page.id,"snapshot_id":snapshot_id,"title":tab.page.title,"url":tab.page.url_string(),"text":"","elements":[]}),
        );
    }
    let script = format!(
        r#"(()=>{{
        const nodes=document.querySelectorAll('a[href],button,input:not([type=hidden]),select,textarea,[role],[onclick],[tabindex]');
        return Array.from(nodes).map((el,i)=>{{const ref={prefix}+'-'+i;el.setAttribute('data-1flowbase-ref',ref);return ref;}});
    }})()"#,
        prefix = quote(&snapshot_id)
    );
    let controls = eval(tab, &script)?;
    let styles = tab
        .page
        .js
        .as_ref()
        .map(|js| js.accessibility_styles())
        .unwrap_or_default();
    let context = tab
        .page
        .with_dom(|dom| {
            obscura_browser::accessibility::AccessibilityContext::for_document(dom, &styles)
        })
        .ok_or("snapshot_failed")?;
    let mut remaining = max_chars;
    let mut elements = Vec::new();
    for reference in controls.as_array().ok_or("snapshot_failed")? {
        let reference = reference.as_str().ok_or("snapshot_failed")?;
        let selector = format!("[data-1flowbase-ref=\"{reference}\"]");
        let candidate=tab.page.with_dom(|dom| {
            let node_id=dom.query_selector(&selector).ok().flatten()?;
            let ax=context.describe_node(dom,node_id)?;
            let node=dom.get_node(node_id)?;
            let tag=node.as_element()?.local.to_string();
            Some((node_id,json!({"ref":reference,"role":ax["role"]["value"].as_str().unwrap_or("generic"),"name":ax["name"]["value"].as_str().unwrap_or(""),"tag":tag})))
        }).flatten();
        let Some((node, mut control)) = candidate else {
            continue;
        };
        if let Some(name) = control["name"].as_str() {
            control["name"] = Value::String(name.chars().take(remaining).collect());
        }
        let cost = control.to_string().chars().count();
        if cost > remaining {
            break;
        }
        remaining -= cost;
        tab.refs.insert(reference.into(), node);
        elements.push(control);
    }
    let text = tab
        .page
        .with_dom(|dom| {
            let mut text = String::new();
            let mut chars = 0;
            for node_id in dom.descendants(dom.document()) {
                if context.is_hidden(node_id) {
                    continue;
                }
                let Some(node) = dom.get_node(node_id) else {
                    continue;
                };
                if let obscura_dom::NodeData::Text { contents } = &node.data {
                    for c in contents.chars() {
                        if chars >= remaining {
                            return text;
                        }
                        text.push(c);
                        chars += 1;
                    }
                    if chars < remaining {
                        text.push(' ');
                        chars += 1;
                    }
                }
            }
            text
        })
        .unwrap_or_default();
    tab.snapshot_id = Some(snapshot_id.clone());
    Ok(
        json!({"success":true,"browser_id":browser_id,"tab_id":tab.page.id,"snapshot_id":snapshot_id,
        "title":tab.page.title,"url":tab.page.url_string(),"text":text,"elements":elements}),
    )
}
pub(crate) async fn execute(
    tab: &mut Tab,
    browser_id: &str,
    handler: &str,
    body: &Value,
    deadline_unix_ms: Option<u64>,
) -> Result<Value> {
    let action = handler
        .strip_prefix("browser-obscura.page.")
        .ok_or("unknown_handler")?;
    if *tab.closed.borrow() {
        return Err("tab_closed");
    }
    // Failed/cancelled navigation cannot silently become a successful action.
    if matches!(tab.status.get(), Status::Failed) && action != "navigate" {
        return Err("page_failed");
    }
    let timeout_ms = number(body, "timeout_ms", 30_000, 1)?;
    tab.script_deadline_unix_ms = deadline_unix_ms
        .unwrap_or(u64::MAX)
        .min(now().saturating_add(timeout_ms));
    let timeout_ms = tab.script_deadline_unix_ms.saturating_sub(now());
    if timeout_ms == 0 {
        return Err("deadline_exceeded");
    }
    tab.page
        .set_navigation_timeout(Duration::from_millis(timeout_ms));
    if action == "navigate" {
        navigate(tab, string(body, "url")?, timeout_ms).await?;
    } else {
        turn(tab).await?;
        match action {
            "snapshot" => return snapshot(tab, browser_id, body),
            "screenshot" => {
                let width = number(body, "width", 1280, 1)?;
                let height = number(body, "height", 720, 1)?;
                obscura_browser::validate_capture_region(obscura_browser::CaptureRegion::new(
                    0.0,
                    0.0,
                    width as f32,
                    height as f32,
                    1.0,
                ))
                .map_err(|_| "invalid_input")?;
                tab.page.set_viewport((width as f32, height as f32));
                tab.page.prepare_screenshot_resources(timeout_ms).await;
                let png = tab
                    .page
                    .screenshot((width as f32, height as f32))
                    .ok_or("screenshot_failed")?;
                return Ok(
                    json!({"success":true,"data":STANDARD.encode(png),"mime_type":"image/png","metadata":{
                    "browser_id":browser_id,"tab_id":tab.page.id,"url":tab.page.url_string(),"title":tab.page.title,"width":width,"height":height}}),
                );
            }
            "wait" => {
                let selector = body
                    .get("selector")
                    .map(|v| v.as_str().filter(|s| !s.is_empty()).ok_or("invalid_input"))
                    .transpose()?;
                let text = body
                    .get("text")
                    .map(|v| v.as_str().filter(|s| !s.is_empty()).ok_or("invalid_input"))
                    .transpose()?;
                if selector.is_none() && text.is_none() {
                    return Err("wait_condition_required");
                }
                // The contract requires an explicit bounded condition, never sleep-only.
                let timeout_ms = number(body, "timeout_ms", 0, 1)?;
                let script = format!(
                    "Boolean(({}) && ({}))",
                    selector
                        .map(|s| format!("document.querySelector({})", quote(s)))
                        .unwrap_or("true".into()),
                    text.map(|s| format!(
                        "document.body && document.body.innerText.includes({})",
                        quote(s)
                    ))
                    .unwrap_or("true".into())
                );
                tokio::time::timeout(Duration::from_millis(timeout_ms), async {
                    loop {
                        turn(tab).await?;
                        if eval(tab, &script)? == Value::Bool(true) {
                            return Ok::<(), &'static str>(());
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .map_err(|_| "wait_timeout")??;
            }
            "click" | "fill" | "type" | "press_key" => {
                let element = if action == "press_key" && body.get("ref").is_none() {
                    if body.get("snapshot_id").is_some() {
                        return Err("invalid_input");
                    }
                    "(document.activeElement || document)".to_string()
                } else {
                    target(tab, body)?
                };
                let operation = match action {
                    "click" => "el.click();".into(),
                    "fill" | "type" => {
                        let text = quote(
                            body.get("text")
                                .and_then(Value::as_str)
                                .ok_or("invalid_input")?,
                        );
                        let value = if action == "type" {
                            format!("(el.value||'')+{text}")
                        } else {
                            text
                        };
                        format!("el.focus(); globalThis.__obscura_setFieldValue(el,'value',{value}); el.dispatchEvent(globalThis.__obscura_markTrusted(new Event('input',{{bubbles:true}}))); el.dispatchEvent(globalThis.__obscura_markTrusted(new Event('change',{{bubbles:true}})));")
                    }
                    _ => {
                        let key = quote(string(body, "key")?);
                        format!(
                            r#"
                        const key={key};
                        const accepted=el.dispatchEvent(globalThis.__obscura_markTrusted(new KeyboardEvent('keydown',{{key,bubbles:true,cancelable:true}})));
                        if(accepted && !el.disabled) {{
                            const tag=(el.localName||'').toLowerCase();
                            if(key==='Enter') {{
                                if(tag==='button'||tag==='a'||(tag==='input'&&['submit','button','image','reset'].includes(el.type))) el.click();
                                else if(tag==='input'&&el.form) {{
                                    const submit=Array.from(el.form.querySelectorAll('button,input')).find(n=>n.form===el.form&&!n.disabled&&((n.localName==='button'&&(n.type==='submit'||!n.getAttribute('type')))||(n.localName==='input'&&['submit','image'].includes(n.type))));
                                    if(submit) submit.click(); else el.form.requestSubmit();
                                }}
                            }} else if(key===' '&&(tag==='button'||(tag==='input'&&['checkbox','radio'].includes(el.type)))) el.click();
                        }}
                        el.dispatchEvent(globalThis.__obscura_markTrusted(new KeyboardEvent('keyup',{{key,bubbles:true,cancelable:true}})));
                    "#
                        )
                    }
                };
                let script=format!("(()=>{{ const el={element}; if(!el) throw new Error('Missing target'); {operation} return true; }})()");
                let guard = OperationGuard::start(tab);
                eval(tab, &script)?;
                // Every action may replace DOM or trigger navigation, so require a
                // fresh snapshot for subsequent ref-based actions.
                tab.invalidate();
                turn(tab).await?;
                guard.finish();
            }
            "scroll" => {
                let x = body
                    .get("x")
                    .and_then(Value::as_f64)
                    .filter(|x| x.is_finite())
                    .ok_or("invalid_input")?;
                let y = body
                    .get("y")
                    .and_then(Value::as_f64)
                    .filter(|y| y.is_finite())
                    .ok_or("invalid_input")?;
                eval(
                    tab,
                    &format!("(()=>{{window.scrollBy({x},{y});return true;}})()"),
                )?;
            }
            _ => return Err("unknown_handler"),
        }
    }
    Ok(json!({"success":true,"tab":tab.summary(browser_id)}))
}
