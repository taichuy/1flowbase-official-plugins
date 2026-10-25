//! Run with nextest: each engine test must own its V8 process.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::{AtomicBool, Ordering},
};
use tokio::task::LocalSet;

const HTML: &str = r#"<!doctype html><html><head><title>Fixture</title></head><body>
<label for="name">Name</label><input id="name"><button onclick="document.querySelector('#out').textContent=document.querySelector('#name').value">Save</button>
<div id="out">Initial</div><button id="replace" onclick="document.querySelector('#name').outerHTML='<input id=name>'">Replace</button>
<script>setTimeout(()=>{document.querySelector('#out').textContent='timer';fetch('/data').then(r=>r.text()).then(t=>document.querySelector('#out').textContent=t)},80)</script>
</body></html>"#;
struct Fixture {
    url: String,
    stop: Arc<AtomicBool>,
}
impl Fixture {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        std::thread::spawn(move || {
                            stream
                                .set_read_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            let mut bytes = [0u8; 4096];
                            let n = stream.read(&mut bytes).unwrap_or(0);
                            let request = String::from_utf8_lossy(&bytes[..n]);
                            if request.starts_with("GET /slow ") {
                                std::thread::sleep(Duration::from_secs(2));
                            }
                            let (mime, body) = if request.starts_with("GET /data ") {
                                ("text/plain", "fetched")
                            } else {
                                ("text/html", HTML)
                            };
                            let response=format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                            let _ = stream.write_all(response.as_bytes());
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
        });
        Self { url, stop }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
async fn browser(s: &Service, name: &str) -> String {
    let value = s
        .execute(
            "browser-obscura.browsers.create",
            &json!({"body":{"name":name}}),
            None,
            None,
        )
        .await
        .unwrap();
    let id = value["browser"]["browser_id"].as_str().unwrap().to_string();
    // Only this deterministic loopback fixture explicitly opts into private networking.
    s.state.borrow_mut().browsers.get_mut(&id).unwrap().context = Arc::new(
        BrowserContext::with_storage_and_network(id.clone(), None, false, None, None, true),
    );
    id
}
async fn call(s: &Service, browser: &str, tab: &str, action: &str, body: Value) -> Result<Value> {
    s.execute(
        &format!("browser-obscura.{action}"),
        &json!({"path":{"browser_id":browser,"tab_id":tab},"body":body}),
        None,
        None,
    )
    .await
}
async fn tab(s: &Service, b: &str, url: &str) -> String {
    call(s, b, "", "tabs.create", json!({"url":url}))
        .await
        .unwrap()["tab"]["tab_id"]
        .as_str()
        .unwrap()
        .into()
}
async fn eval_in(s: &Service, b: &str, t: &str, script: &str) -> Value {
    let tab = s.state.borrow().browsers[b].tabs[t].clone();
    tab.interrupt.notify_one();
    let _wake = TabWake(tab.interrupt.clone());
    let mut tab = tab.inner.lock().await;
    tab.page.evaluate_script(script).unwrap()
}
#[tokio::test(flavor = "current_thread")]
async fn real_context_storage_tabs_and_browser_isolation() {
    LocalSet::new().run_until(async {
        let f=Fixture::new();let s=Service::new();let a=browser(&s,"Alpha").await;let b=browser(&s,"Beta").await;
        let t1=tab(&s,&a,&f.url).await;
        eval_in(&s,&a,&t1,"(()=>{localStorage.setItem('key','shared');sessionStorage.setItem('key','private');document.cookie='k=one';return true})()").await;
        let t2=tab(&s,&a,&f.url).await;let t3=tab(&s,&b,&f.url).await;
        assert_eq!(eval_in(&s,&a,&t2,"[localStorage.getItem('key'),sessionStorage.getItem('key'),document.cookie]").await,json!(["shared",null,"k=one"]));
        assert_eq!(eval_in(&s,&b,&t3,"[localStorage.getItem('key'),document.cookie]").await,json!([null,""]));
        assert!(s.state.borrow().browsers[&a].context.storage_dir.is_none());
        let list=s.execute("browser-obscura.browsers.list",&json!({"query":{"search":"ALP","page":1,"page_size":1}}),None,None).await.unwrap();
        assert_eq!(list["total"],1);assert_eq!(list["items"][0]["name"],"Alpha");
    }).await;
}
#[tokio::test(flavor = "current_thread")]
async fn snapshot_actions_node_identity_and_png() {
    LocalSet::new().run_until(async {
        let f=Fixture::new();let s=Service::new();let b=browser(&s,"UI").await;let t=tab(&s,&b,&f.url).await;
        let snap=call(&s,&b,&t,"page.snapshot",json!({})).await.unwrap();
        let input=snap["elements"].as_array().unwrap().iter().find(|e|e["tag"]=="input").unwrap();
        call(&s,&b,&t,"page.fill",json!({"ref":input["ref"],"snapshot_id":snap["snapshot_id"],"text":"Alice"})).await.unwrap();
        assert_eq!(eval_in(&s,&b,&t,"document.querySelector('#name').value").await,"Alice");
        assert_eq!(call(&s,&b,&t,"page.click",json!({"ref":input["ref"],"snapshot_id":snap["snapshot_id"]})).await,Err("stale_snapshot"));
        let snap=call(&s,&b,&t,"page.snapshot",json!({})).await.unwrap();
        let input=snap["elements"].as_array().unwrap().iter().find(|e|e["tag"]=="input").unwrap();
        eval_in(&s,&b,&t,"(()=>{const old=document.querySelector('#name');old.replaceWith(old.cloneNode(true));return true})()").await;
        assert_eq!(call(&s,&b,&t,"page.fill",json!({"ref":input["ref"],"snapshot_id":snap["snapshot_id"],"text":"bad"})).await,Err("stale_reference"));
        let shot=call(&s,&b,&t,"page.screenshot",json!({"width":320,"height":200})).await.unwrap();
        use base64::Engine;
        let png=base64::engine::general_purpose::STANDARD.decode(shot["data"].as_str().unwrap()).unwrap();
        assert_eq!(&png[..8],b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()),320);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()),200);
        assert_eq!(shot["metadata"]["tab_id"],t);
        let snap=call(&s,&b,&t,"page.snapshot",json!({})).await.unwrap();
        let field=snap["elements"].as_array().unwrap().iter().find(|e|e["tag"]=="input").unwrap();
        let a=call(&s,&b,&t,"page.fill",json!({"ref":field["ref"],"snapshot_id":snap["snapshot_id"],"text":"First"}));
        let z=call(&s,&b,&t,"page.fill",json!({"ref":field["ref"],"snapshot_id":snap["snapshot_id"],"text":"Second"}));
        let (a,z)=tokio::join!(a,z);
        assert!(a.is_ok());assert_eq!(z,Err("stale_snapshot"));
    }).await;
}
#[tokio::test(flavor = "current_thread")]
async fn autonomous_timer_fetch_wait_and_stale_navigation() {
    LocalSet::new()
        .run_until(async {
            let f = Fixture::new();
            let s = Service::new();
            let b = browser(&s, "Live").await;
            let t = tab(&s, &b, &f.url).await;
            let snap = call(&s, &b, &t, "page.snapshot", json!({})).await.unwrap();
            // No tool drives the page during this interval: the managed background loop must.
            tokio::time::sleep(Duration::from_millis(500)).await;
            assert_eq!(
                eval_in(&s, &b, &t, "document.querySelector('#out').textContent").await,
                "fetched"
            );
            call(
                &s,
                &b,
                &t,
                "page.wait",
                json!({"text":"fetched","selector":"#out","timeout_ms":1000}),
            )
            .await
            .unwrap();
            assert_eq!(
                call(&s, &b, &t, "page.wait", json!({"timeout_ms":10})).await,
                Err("wait_condition_required")
            );
            call(&s, &b, &t, "page.navigate", json!({"url":f.url}))
                .await
                .unwrap();
            let reference = &snap["elements"][0]["ref"];
            assert_eq!(
                call(
                    &s,
                    &b,
                    &t,
                    "page.click",
                    json!({"ref":reference,"snapshot_id":snap["snapshot_id"]})
                )
                .await,
                Err("stale_snapshot")
            );
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn cancelled_navigation_and_close_are_never_successful() {
    LocalSet::new()
        .run_until(async {
            let f = Fixture::new();
            let s = Service::new();
            let b = browser(&s, "Cancel").await;
            let t = tab(&s, &b, &f.url).await;
            let body = json!({"url":format!("{}/slow",f.url)});
            let result = tokio::time::timeout(
                Duration::from_millis(20),
                call(&s, &b, &t, "page.navigate", body),
            )
            .await;
            assert!(result.is_err());
            let info = call(&s, &b, &t, "tabs.list", json!({})).await.unwrap();
            assert_eq!(info["items"][0]["status"], "failed");
            let s2 = s.clone();
            let b2 = b.clone();
            let t2 = t.clone();
            let url = format!("{}/slow", f.url);
            let nav = tokio::task::spawn_local(async move {
                call(&s2, &b2, &t2, "page.navigate", json!({"url":url})).await
            });
            tokio::time::sleep(Duration::from_millis(20)).await;
            call(&s, &b, &t, "browsers.close", json!({})).await.unwrap();
            assert_eq!(nav.await.unwrap(), Err("browser_closed"));
            assert!(s.state.borrow().browsers.is_empty());
        })
        .await;
}
#[test]
fn trusted_scope_is_required_and_cannot_change() {
    let s = Service::new();
    assert!(serde_json::from_value::<Invocation>(
        json!({"input":{"handler":"x","input_payload":{}}})
    )
    .is_err());
    let c = |actor: &str, workspace: &str| CallContext {
        actor_id: actor.into(),
        workspace_id: workspace.into(),
        deadline_unix_ms: (now() + 10000) as i64,
    };
    assert_eq!(s.authorize(&c("", "w")), Err("missing_call_context"));
    assert!(s.authorize(&c("a", "w")).is_ok());
    assert_eq!(s.authorize(&c("b", "w")), Err("scope_mismatch"));
    assert_eq!(s.authorize(&c("a", "other")), Err("scope_mismatch"));
}

#[tokio::test(flavor = "current_thread")]
async fn visible_ax_snapshot_and_enter_form_default_action() {
    LocalSet::new().run_until(async {
        let f=Fixture::new();let s=Service::new();let b=browser(&s,"Forms").await;let t=tab(&s,&b,&f.url).await;
        eval_in(&s,&b,&t,r#"(()=>{document.body.innerHTML=`<button style="display:none">Hidden CSS</button><div aria-hidden="true"><button>Hidden ARIA</button></div><input type="password" value="private-password" aria-label="Password"><form action="/submitted" onsubmit="document.querySelector('#submitted').textContent='submitted';event.preventDefault()"><label for="field">Visible field</label><input id="field" name="q"><button type="submit">Submit</button></form><div id="submitted"></div>`;return true})()"#).await;
        let snap=call(&s,&b,&t,"page.snapshot",json!({})).await.unwrap();
        assert!(!snap.to_string().contains("Hidden CSS"));assert!(!snap.to_string().contains("Hidden ARIA"));assert!(!snap.to_string().contains("private-password"));
        let field=snap["elements"].as_array().unwrap().iter().find(|e|e["name"]=="Visible field").unwrap();
        call(&s,&b,&t,"page.press_key",json!({"key":"Enter","ref":field["ref"],"snapshot_id":snap["snapshot_id"]})).await.unwrap();
        assert_eq!(eval_in(&s,&b,&t,"document.querySelector('#submitted').textContent").await,"submitted");
        eval_in(&s,&b,&t,"(()=>{document.querySelector('#submitted').textContent='';document.querySelector('#field').addEventListener('keydown',e=>e.preventDefault());return true})()").await;
        let snap=call(&s,&b,&t,"page.snapshot",json!({})).await.unwrap();
        let field=snap["elements"].as_array().unwrap().iter().find(|e|e["name"]=="Visible field").unwrap();
        call(&s,&b,&t,"page.press_key",json!({"key":"Enter","ref":field["ref"],"snapshot_id":snap["snapshot_id"]})).await.unwrap();
        assert_eq!(eval_in(&s,&b,&t,"document.querySelector('#submitted').textContent").await,"");
        // An unrelated future timer must not block an immediate snapshot command.
        eval_in(&s,&b,&t,"(()=>{setTimeout(()=>{},60000);return true})()").await;
        tokio::time::timeout(Duration::from_millis(500),call(&s,&b,&t,"page.snapshot",json!({}))).await.unwrap().unwrap();
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn click_type_scroll_watchdog_errors_and_tab_close() {
    LocalSet::new().run_until(async {
        let f=Fixture::new();let s=Service::new();let b=browser(&s,"Actions").await;let t=tab(&s,&b,&f.url).await;
        call(&s,&b,&t,"page.wait",json!({"text":"fetched","timeout_ms":2000})).await.unwrap();
        let snap=call(&s,&b,&t,"page.snapshot",json!({})).await.unwrap();
        let field=snap["elements"].as_array().unwrap().iter().find(|e|e["name"]=="Name").unwrap();
        call(&s,&b,&t,"page.type",json!({"ref":field["ref"],"snapshot_id":snap["snapshot_id"],"text":"Typed"})).await.unwrap();
        let snap=call(&s,&b,&t,"page.snapshot",json!({})).await.unwrap();
        let save=snap["elements"].as_array().unwrap().iter().find(|e|e["name"]=="Save").unwrap();
        call(&s,&b,&t,"page.click",json!({"ref":save["ref"],"snapshot_id":snap["snapshot_id"]})).await.unwrap();
        assert_eq!(eval_in(&s,&b,&t,"document.querySelector('#out').textContent").await,"Typed");
        call(&s,&b,&t,"page.scroll",json!({"x":0,"y":100})).await.unwrap();
        eval_in(&s,&b,&t,"(()=>{document.querySelector('button').onclick=()=>{throw new Error('sensitive-page-error')};return true})()").await;
        let snap=call(&s,&b,&t,"page.snapshot",json!({})).await.unwrap();
        let save=snap["elements"].as_array().unwrap().iter().find(|e|e["name"]=="Save").unwrap();
        // Event dispatch reports some page listener exceptions as browser console
        // errors, so force the element's public click getter to throw synchronously.
        eval_in(&s,&b,&t,"(()=>{Object.defineProperty(document.querySelector('button'),'click',{get(){throw new Error('sensitive-page-error')}});return true})()").await;
        assert_eq!(call(&s,&b,&t,"page.click",json!({"ref":save["ref"],"snapshot_id":snap["snapshot_id"]})).await,Err("javascript_error"));
        call(&s,&b,&t,"page.navigate",json!({"url":f.url})).await.unwrap();
        eval_in(&s,&b,&t,"(()=>{Object.defineProperty(document.querySelector('button'),'click',{get(){while(true){}}});return true})()").await;
        let snap=call(&s,&b,&t,"page.snapshot",json!({})).await.unwrap();
        let save=snap["elements"].as_array().unwrap().iter().find(|e|e["name"]=="Save").unwrap();
        assert_eq!(call(&s,&b,&t,"page.click",json!({"ref":save["ref"],"snapshot_id":snap["snapshot_id"],"timeout_ms":50})).await,Err("javascript_timeout"));
        call(&s,&b,&t,"tabs.close",json!({})).await.unwrap();
        assert_eq!(call(&s,&b,&t,"page.snapshot",json!({})).await,Err("tab_not_found"));
    }).await;
}
