//! UI-6: **the wasm entry** — what `index.html` calls once `engine.js` has
//! loaded `engine.wasm`: `boot(version)`, the hash the page was served for.
//!
//! It reads the stamped socket port (`<meta name="menger-ws-port">`),
//! opens the page's one WebSocket (signed in by the session's cookie; a
//! reconnect loop of 1 s), reads `/app/<version>/manifest.json`, fetches
//! the version's components, asks `ui.slots.list`, opens `ui.watch`
//! (UI-9: `live { version }` swaps, `slot_changed` remounts a section),
//! mounts the route's component into the body, follows the hash, and
//! draws the section menu on a right-click. Behind the `dom` feature, on
//! wasm32.

use crate::live::Clock;
use crate::router::Router;
use crate::signals::Runtime;
use crate::swap::{App, Assignment, ComponentEntry, Files, Manifest, MountId};
use crate::transport::ws::{sleep, WsWire};
use crate::transport::{Wire, WireItem};
use crate::tree::dom::Dom;
use crate::tree::Renderer;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// The page's clock: milliseconds since the epoch.
#[derive(Clone, Copy, Debug)]
pub struct PageClock;

impl Clock for PageClock {
    fn now_ms(&self) -> u64 {
        js_sys::Date::now() as u64
    }
    fn schedule(&self, after_ms: u64, f: Box<dyn FnOnce()>) {
        // Freed once it has run.
        let c = Closure::once_into_js(move || f());
        let _ = window().set_timeout_with_callback_and_timeout_and_arguments_0(c.unchecked_ref(), after_ms.min(i32::MAX as u64) as i32);
    }
}

type PageApp = App<Dom, WsWire, PageClock>;

/// Everything the page holds.
struct Page {
    app: Rc<PageApp>,
    wire: WsWire,
    dom: Dom,
    root: web_sys::Node,
    current: RefCell<Option<MountId>>,
    manifests: RefCell<HashMap<String, Manifest>>,
    fetching: RefCell<BTreeSet<(String, String)>>,
}

fn window() -> web_sys::Window {
    web_sys::window().expect("a window")
}

fn say(line: &str) {
    web_sys::console::log_1(&line.into());
}

/// The socket's URL: over https the origin's `/ws`, else the stamped port
/// on the page's host.
fn socket_url(document: &web_sys::Document) -> String {
    let loc = window().location();
    let host = loc.hostname().unwrap_or_else(|_| "127.0.0.1".into());
    if loc.protocol().ok().as_deref() == Some("https:") {
        return format!("wss://{}/ws", loc.host().unwrap_or(host));
    }
    let port = document
        .query_selector(r#"meta[name="menger-ws-port"]"#)
        .ok()
        .flatten()
        .and_then(|m| m.get_attribute("content"))
        .filter(|p| p.bytes().all(|b| b.is_ascii_digit()) && !p.is_empty())
        .unwrap_or_else(|| "4455".into());
    format!("ws://{host}:{port}")
}

/// `url`'s body as text.
async fn fetch_text(url: &str) -> Result<String, String> {
    let resp = wasm_bindgen_futures::JsFuture::from(window().fetch_with_str(url)).await.map_err(|e| format!("{url}: {e:?}"))?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| format!("{url}: no response"))?;
    if !resp.ok() {
        return Err(format!("{url}: {}", resp.status()));
    }
    let text = wasm_bindgen_futures::JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))?;
    text.as_string().ok_or_else(|| format!("{url}: no text"))
}

impl Page {
    /// Version `version`'s manifest (kept: a version's files never change).
    async fn manifest(&self, version: &str) -> Result<Manifest, String> {
        if let Some(m) = self.manifests.borrow().get(version) {
            return Ok(m.clone());
        }
        let m = Manifest::parse(&fetch_text(&format!("/app/{version}/manifest.json")).await?)?;
        self.manifests.borrow_mut().insert(version.into(), m.clone());
        Ok(m)
    }

    /// Version `version`'s components `names`, fetched.
    async fn components(&self, version: &str, names: &[String]) -> Files {
        let mut files = Files::default();
        for name in names {
            let tree = fetch_text(&format!("/app/{version}/components/{name}.json")).await;
            let rhai = fetch_text(&format!("/app/{version}/components/{name}.rhai")).await;
            match (tree, rhai) {
                (Ok(t), Ok(h)) => files.put_version(version, name, &t, &h),
                (Err(e), _) | (_, Err(e)) => say(&format!("{name} of {version}: {e}")),
            }
        }
        files
    }

    /// Fetches whatever the engine asks for, and hands it over.
    fn pump(self: &Rc<Self>) {
        for (v, n) in self.app.needs() {
            if !self.fetching.borrow_mut().insert((v.clone(), n.clone())) {
                continue;
            }
            let me = self.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let files = me.components(&v, std::slice::from_ref(&n)).await;
                me.fetching.borrow_mut().remove(&(v, n));
                me.app.provide(&files);
                me.pump();
            });
        }
    }

    /// The route the hash names, mounted in place of the last.
    fn route(self: &Rc<Self>) {
        let hash = window().location().hash().unwrap_or_default();
        let router = Router::new(&self.app.manifest().routes);
        if let Some(old) = self.current.borrow_mut().take() {
            let el = self.app.element_of(old);
            self.app.unmount(old);
            self.dom.remove(&self.root, &el);
        }
        let Some(route) = router.resolve(&hash) else {
            say(&format!("no route for {hash}"));
            return;
        };
        match self.app.mount(&self.root, &route.component, route.props) {
            Ok(id) => *self.current.borrow_mut() = Some(id),
            Err(e) => say(&format!("{}: {e}", route.component)),
        }
        self.pump();
    }

    /// `ui.slots.list`: the assignments.
    async fn assignments(self: &Rc<Self>) {
        match self.wire.ask("ui.slots.list", json!({})).await {
            Ok(items) => {
                let list = items
                    .iter()
                    .filter(|i| i["type"] == "assignment")
                    .filter_map(|i| {
                        Some(Assignment {
                            layout: i["layout"].as_str()?.into(),
                            slot: i["slot"].as_str()?.into(),
                            component: i["component"].as_str()?.into(),
                            version: i["version"].as_str()?.into(),
                        })
                    })
                    .collect();
                self.app.set_assignments(list);
                self.pump();
            }
            Err(e) => say(&format!("ui.slots.list: {e}")),
        }
    }

    /// `ui.watch`, opened (again after a reconnect).
    fn watch(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        self.wire.open("ui.watch", json!({}), Box::new(move |item| {
            let Some(page) = me.upgrade() else { return };
            match item {
                WireItem::Data(ev) => page.heard(ev),
                WireItem::Refused(m) => say(&format!("ui.watch: {m}")),
                _ => {}
            }
        }));
    }

    fn heard(self: &Rc<Self>, ev: Value) {
        match ev["type"].as_str() {
            Some("live") => {
                let Some(version) = ev["version"].as_str().map(str::to_string) else { return };
                if version == self.app.version() {
                    return;
                }
                let me = self.clone();
                wasm_bindgen_futures::spawn_local(async move { me.swap_to(&version).await });
            }
            Some("slot_changed") => {
                let (layout, slot) = (ev["layout"].as_str().unwrap_or_default().to_string(), ev["slot"].as_str().unwrap_or_default().to_string());
                match (ev["component"].as_str(), ev["version"].as_str()) {
                    (Some(c), v) => self.app.slot_changed(Assignment { layout, slot, component: c.into(), version: v.map(str::to_string).unwrap_or_else(|| self.app.version()) }),
                    (None, _) => self.app.slot_reset(&layout, &slot),
                }
                self.pump();
            }
            Some("lagged") => {
                let me = self.clone();
                wasm_bindgen_futures::spawn_local(async move { me.assignments().await });
            }
            _ => {}
        }
    }

    /// A push: the new manifest, the components whose hashes changed
    /// fetched, then the swap.
    async fn swap_to(self: &Rc<Self>, version: &str) {
        let m = match self.manifest(version).await {
            Ok(m) => m,
            Err(e) => return say(&format!("the version {version}: {e}")),
        };
        let old = self.app.manifest();
        let changed: Vec<String> = m.components.iter().filter(|(n, e)| old.components.get(*n) != Some(*e)).map(|(n, _)| n.clone()).collect();
        // The changed ones now; the rest when something mounts them (pump).
        let files = self.components(version, &changed).await;
        self.app.provide(&files);
        let report = self.app.swap(version, m);
        say(&format!("ui: {version} live — {} remounted, {} kept on their last good version", report.remounted.len(), report.failed.len()));
        self.pump();
    }
}

thread_local! {
    static PAGE: RefCell<Option<Rc<Page>>> = const { RefCell::new(None) };
}

/// Boots the engine on the page for the version `version` (its hash, as
/// `index.html` was served for it).
#[wasm_bindgen]
pub fn boot(version: String) {
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = run(version).await {
            web_sys::console::error_1(&format!("ui: {e}").into());
        }
    });
}

async fn run(version: String) -> Result<(), String> {
    let document = window().document().ok_or("no document")?;
    let root: web_sys::Node = document.body().ok_or("no body")?.into();
    let wire = WsWire::new(&socket_url(&document));
    wire.run().await;
    let dom = Dom::new(document.clone());
    let app = App::new(&Runtime::new(), dom.clone(), wire.clone(), PageClock);
    let page = Rc::new(Page {
        app: app.clone(),
        wire: wire.clone(),
        dom,
        root,
        current: RefCell::new(None),
        manifests: RefCell::new(HashMap::new()),
        fetching: RefCell::new(BTreeSet::new()),
    });
    PAGE.with(|p| *p.borrow_mut() = Some(page.clone()));

    let manifest = page.manifest(&version).await?;
    let names: Vec<String> = manifest.components.keys().cloned().collect();
    let files = page.components(&version, &names).await;
    app.provide(&files);
    app.load(&version, manifest);
    app.set_navigate(Rc::new(|route: &str| {
        let _ = window().location().set_hash(route);
    }));

    // The watch first, so nothing said while the rest is asked is missed.
    page.watch();
    page.assignments().await;
    page.route();

    // The hash.
    let p = page.clone();
    let on_hash = Closure::<dyn FnMut()>::new(move || p.route());
    window().add_event_listener_with_callback("hashchange", on_hash.as_ref().unchecked_ref()).map_err(|e| format!("{e:?}"))?;
    on_hash.forget();

    // A reconnect: every `live` asked again, the watch and the assignments.
    let p = Rc::downgrade(&page);
    wire.on_reconnect(Rc::new(move || {
        if let Some(page) = p.upgrade() {
            page.app.cache().reconnected();
            page.watch();
            let pg = page.clone();
            wasm_bindgen_futures::spawn_local(async move { pg.assignments().await });
        }
    }));

    // The menu.
    let p = page.clone();
    let on_menu = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |e: web_sys::MouseEvent| menu_event(&p, e));
    document.add_event_listener_with_callback("contextmenu", on_menu.as_ref().unchecked_ref()).map_err(|e| format!("{e:?}"))?;
    on_menu.forget();

    // The cache's keep: swept every second.
    let p = Rc::downgrade(&page);
    wasm_bindgen_futures::spawn_local(async move {
        while let Some(page) = p.upgrade() {
            page.app.cache().sweep();
            drop(page);
            sleep(1_000).await;
        }
    });
    Ok(())
}

// ── the section menu ──────────────────────────────────────────────────────

const MENU_STYLE: &str = "position:fixed;z-index:2147483647;min-width:14rem;max-width:24rem;padding:.25rem 0;\
background:Canvas;color:CanvasText;border:1px solid GrayText;border-radius:6px;box-shadow:0 6px 24px rgba(0,0,0,.25);\
font:13px/1.4 system-ui,sans-serif";
const HEAD_STYLE: &str = "padding:.25rem .75rem;font-size:11px;opacity:.7;text-transform:uppercase;letter-spacing:.04em";
const ITEM_STYLE: &str = "display:block;width:100%;text-align:left;padding:.3rem .75rem;border:0;background:none;color:inherit;font:inherit;cursor:pointer";

fn close_menu(document: &web_sys::Document) {
    if let Ok(Some(m)) = document.query_selector("[data-engine-menu]") {
        m.remove();
    }
}

fn menu_event(page: &Rc<Page>, e: web_sys::MouseEvent) {
    let Some(target) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
    if target.closest("[data-engine-menu]").ok().flatten().is_some() {
        return;
    }
    let Some(section) = target.closest("[data-slot]").ok().flatten() else { return };
    e.prevent_default();
    let layout = section.get_attribute("data-layout").unwrap_or_default();
    let slot = section.get_attribute("data-slot").unwrap_or_default();
    let (x, y) = (e.client_x(), e.client_y());
    let page = page.clone();
    wasm_bindgen_futures::spawn_local(async move { open_menu(&page, &layout, &slot, x, y).await });
}

/// Every stored version (newest first) with its manifest's entry for
/// `component`.
async fn versions_of(page: &Rc<Page>, component: &str) -> Vec<(String, Option<ComponentEntry>, String)> {
    let items = page.wire.ask("ui.versions", json!({})).await.unwrap_or_default();
    let mut out = Vec::new();
    for v in items.iter().filter(|i| i["type"] == "version") {
        let Some(hash) = v["hash"].as_str() else { continue };
        let entry = page.manifest(hash).await.ok().and_then(|m| m.components.get(component).cloned());
        let label = format!("{} · {}", &hash[..hash.len().min(10)], v["commit"].as_str().unwrap_or(""));
        out.push((hash.to_string(), entry, label));
    }
    out
}

async fn open_menu(page: &Rc<Page>, layout: &str, slot: &str, x: i32, y: i32) {
    let document = page.dom.document.clone();
    close_menu(&document);
    let component = page.app.menu(layout, slot, &[]).component;
    let stored = versions_of(page, &component).await;
    let listed: Vec<(String, Option<ComponentEntry>)> = stored.iter().map(|(h, e, _)| (h.clone(), e.clone())).collect();
    let menu = page.app.menu(layout, slot, &listed);
    let Ok(m) = document.create_element("div") else { return };
    let _ = m.set_attribute("data-engine-menu", "");
    let _ = m.set_attribute("role", "menu");
    let _ = m.set_attribute("style", &format!("{MENU_STYLE};left:{x}px;top:{y}px"));
    let add = |parent: &web_sys::Element, tag: &str, attrs: &[(&str, &str)], text: &str| -> Option<web_sys::Element> {
        let el = document.create_element(tag).ok()?;
        for (k, v) in attrs {
            let _ = el.set_attribute(k, v);
        }
        el.set_text_content(Some(text));
        let _ = parent.append_child(&el);
        Some(el)
    };
    add(&m, "div", &[("style", HEAD_STYLE)], &format!("{layout} · {slot}: components"));
    for name in &menu.components {
        if let Some(b) = add(&m, "button", &[("data-component", name), ("style", ITEM_STYLE), ("role", "menuitem")], name) {
            let (page, layout, slot, name, live) = (page.clone(), layout.to_string(), slot.to_string(), name.clone(), page.app.version());
            on_click(&b, move || pick(&page, &layout, &slot, &name, &live));
        }
    }
    if !menu.versions.is_empty() {
        add(&m, "div", &[("style", HEAD_STYLE)], &format!("{}: other versions", menu.component));
    }
    for v in &menu.versions {
        let label = stored.iter().find(|(h, _, _)| h == v).map(|(_, _, l)| l.clone()).unwrap_or_else(|| v.clone());
        if let Some(b) = add(&m, "button", &[("data-version", v), ("style", ITEM_STYLE), ("role", "menuitem")], &label) {
            let (page, layout, slot, name, v) = (page.clone(), layout.to_string(), slot.to_string(), menu.component.clone(), v.clone());
            on_click(&b, move || pick(&page, &layout, &slot, &name, &v));
        }
    }
    if let Some(body) = document.body() {
        let _ = body.append_child(&m);
    }
    // A click elsewhere, or Escape, closes it.
    let doc = document.clone();
    let away = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        let inside = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).and_then(|t| t.closest("[data-engine-menu]").ok().flatten()).is_some();
        let escape = e.dyn_ref::<web_sys::KeyboardEvent>().is_some_and(|k| k.key() == "Escape");
        if (!inside && e.type_() == "mousedown") || escape {
            close_menu(&doc);
        }
    });
    let _ = document.add_event_listener_with_callback("mousedown", away.as_ref().unchecked_ref());
    let _ = document.add_event_listener_with_callback("keydown", away.as_ref().unchecked_ref());
    away.forget();
}

fn on_click(el: &web_sys::Element, f: impl Fn() + 'static) {
    let c = Closure::<dyn FnMut()>::new(f);
    let _ = el.add_event_listener_with_callback("click", c.as_ref().unchecked_ref());
    c.forget();
}

/// A pick: `ui.slots.set`; the section remounts when `ui.watch` says so; a
/// refusal is shown in the menu.
fn pick(page: &Rc<Page>, layout: &str, slot: &str, component: &str, version: &str) {
    let page = page.clone();
    let params = json!({ "layout": layout, "slot": slot, "component": component, "version": version });
    wasm_bindgen_futures::spawn_local(async move {
        let refusal = match page.wire.ask("ui.slots.set", params).await {
            Ok(items) => items.iter().find(|i| i["type"] != "set").map(|i| i["message"].as_str().map(str::to_string).unwrap_or_else(|| i.to_string())),
            Err(m) => Some(m),
        };
        let document = page.dom.document.clone();
        match refusal {
            None => close_menu(&document),
            Some(why) => {
                if let Ok(Some(m)) = document.query_selector("[data-engine-menu]") {
                    if let Ok(Some(old)) = m.query_selector("[data-refused]") {
                        old.remove();
                    }
                    if let Ok(el) = document.create_element("div") {
                        let _ = el.set_attribute("data-refused", "");
                        let _ = el.set_attribute("role", "alert");
                        let _ = el.set_attribute("style", "padding:.4rem .75rem;color:#b3261e;border-top:1px solid GrayText");
                        el.set_text_content(Some(&format!("refused: {why}")));
                        let _ = m.append_child(&el);
                    }
                }
            }
        }
    });
}
