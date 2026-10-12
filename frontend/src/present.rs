//! MENGER-15: how a framed body is shown — **full screen**, and **as a
//! website mockup**.
//!
//! Every framed body (src/frame.rs) sits in a [`Framed`] box: a slim bar
//! over the frame with a full-screen control (and `z`, src/keys.rs; `z`
//! inside a focused body too — the SDK and the page shim post it). Full
//! screen is the same box, the same `<iframe>` — never a new frame, origin
//! or window: its sandbox and CSP are unchanged — made to fill the viewport:
//! the Fullscreen API on the box when the browser allows it (a gesture is
//! under way), and always a fixed overlay over the app (`.framed.is-full`),
//! which is all a deep link gets. Esc leaves it (the browser's own Esc in
//! native full screen; the app's key, or the frame's posted Esc, in the
//! overlay), as does the bar's exit. Full, the bar names the ticket — its
//! id and title. On the ticket page the route says so: `#/ticket/ID?view=full`
//! opens the ticket straight into it, and entering or leaving rewrites the
//! query in place (no history entry, no hashchange).
//!
//! **A body declares its view** ([`declared`]) — in its frontmatter:
//!
//! ```yaml
//! ---
//! view: mockup            # inline (default) | full | mockup
//! widths: 390, 768, 1440  # a mockup's devices: W or WxH, at most 6
//! ---
//! ```
//!
//! or, for a whole HTML document that should stay a plain file, in its
//! head: `<meta name="menger-view" content="mockup">` and `<meta
//! name="menger-widths" content="390,768x1024,1440">` (frontmatter wins).
//! `full` opens the ticket page full screen, its exit reading "Back to the
//! ticket"; `mockup` draws the page in device chrome at each width.
//!
//! **A mockup** renders the body once per device, each frame `W` CSS px
//! wide and `H` tall (a device's own viewport: its media queries see `W`),
//! scaled to fit — side by side at one common scale (sizes stay true to one
//! another), or one device at a time — with a light/dark toggle and a
//! reduced-motion toggle that the framed page sees as its
//! `prefers-color-scheme` / `prefers-reduced-motion` (src/frame.rs
//! [`frame::Emulate`]; frontend/sdk/menger-page.js), applied live. Full
//! screen applies to a mockup too: the devices scale to the screen.
//!
//! What decides — the declaration, the devices, the scale, the route's
//! query — is pure and pinned by native tests; the rest is DOM.

use std::cell::{Cell, RefCell};

use js_sys::{Function, Promise, Reflect};
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::dom;
use crate::frame::{Emulate, Frame, Scheme};

/// How a body asks to be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Mode {
    /// In the page, as every framed body (the default).
    #[default]
    Inline,
    /// Full screen when its ticket page opens.
    Full,
    /// In device chrome at each of its widths.
    Mockup,
}

impl Mode {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().trim_matches(['"', '\'']).to_ascii_lowercase().as_str() {
            "inline" | "page" => Some(Mode::Inline),
            "full" | "fullscreen" | "full-screen" => Some(Mode::Full),
            "mockup" | "mockups" => Some(Mode::Mockup),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Mode::Inline => "inline",
            Mode::Full => "full",
            Mode::Mockup => "mockup",
        }
    }
}

/// One mockup device: its viewport, in CSS px.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Device {
    pub(crate) w: u32,
    pub(crate) h: u32,
}

impl Device {
    /// A width alone: a phone's, a tablet's or a desktop's height.
    fn of_width(w: u32) -> Self {
        let h = if w < 600 {
            844
        } else if w < 1100 {
            1024
        } else {
            900
        };
        Device { w, h }
    }

    fn kind(self) -> &'static str {
        if self.w < 600 {
            "phone"
        } else if self.w < 1100 {
            "tablet"
        } else {
            "desktop"
        }
    }
}

/// The widths a mockup shows when the body names none: a phone, a tablet,
/// a desktop.
pub(crate) const DEFAULT_WIDTHS: [u32; 3] = [390, 768, 1440];

/// The most devices a mockup draws.
const MAX_DEVICES: usize = 6;

/// What a body declares: its view, and a mockup's devices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Decl {
    pub(crate) mode: Mode,
    pub(crate) devices: Vec<Device>,
}

impl Default for Decl {
    fn default() -> Self {
        Decl { mode: Mode::Inline, devices: DEFAULT_WIDTHS.iter().map(|w| Device::of_width(*w)).collect() }
    }
}

/// `390, 768x1024, 1440` (or `[390, 768, 1440]`) — each `W` or `WxH`, a
/// width 240–3840 and a height 240–4320; anything else is dropped. None
/// left: `None` (the defaults stand).
pub(crate) fn parse_widths(s: &str) -> Option<Vec<Device>> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    let devices: Vec<Device> = s
        .split([',', ' '])
        .map(|w| w.trim().trim_matches(['"', '\'']))
        .filter(|w| !w.is_empty())
        .filter_map(|w| {
            let w = w.to_ascii_lowercase();
            let w = w.trim_end_matches("px");
            let (w, h) = match w.split_once(['x', '×']) {
                Some((w, h)) => (w.trim().parse::<u32>().ok()?, Some(h.trim().trim_end_matches("px").parse::<u32>().ok()?)),
                None => (w.parse::<u32>().ok()?, None),
            };
            if !(240..=3840).contains(&w) || h.is_some_and(|h| !(240..=4320).contains(&h)) {
                return None;
            }
            Some(h.map_or_else(|| Device::of_width(w), |h| Device { w, h }))
        })
        .take(MAX_DEVICES)
        .collect();
    (!devices.is_empty()).then_some(devices)
}

/// A frontmatter's `key: value`, for a top-level key.
fn fm_value<'a>(fm: &'a str, key: &str) -> Option<&'a str> {
    fm.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        (k == key).then(|| v.split(" #").next().unwrap_or(v).trim())
    })
}

/// A whole document's `<meta name="NAME" content="…">`, anywhere in it.
fn meta_content<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let lower_needle = format!("name=\"{name}\"");
    let at = src.find(&lower_needle).or_else(|| src.find(&format!("name='{name}'")).or_else(|| src.find(&format!("name={name}"))))?;
    let open = src[..at].rfind('<')?;
    if !src[open..].get(..5).is_some_and(|t| t.eq_ignore_ascii_case("<meta")) {
        return None;
    }
    let close = at + src[at..].find('>')?;
    let tag = &src[open..close];
    let c = tag.find("content=")? + "content=".len();
    let rest = &tag[c..];
    let (q, rest) = match rest.chars().next()? {
        q @ ('"' | '\'') => (Some(q), &rest[1..]),
        _ => (None, rest),
    };
    let end = match q {
        Some(q) => rest.find(q)?,
        None => rest.find(|c: char| c.is_ascii_whitespace() || c == '/').unwrap_or(rest.len()),
    };
    Some(&rest[..end])
}

/// What a body declares — from its frontmatter (`view:`, `widths:`), else,
/// for a whole document, its `<meta name="menger-view">` and
/// `<meta name="menger-widths">`. Anything unreadable is the default.
pub(crate) fn declared(frontmatter: Option<&str>, src: &str) -> Decl {
    let mut decl = Decl::default();
    let page = crate::md::is_page(src);
    let view = frontmatter
        .and_then(|fm| fm_value(fm, "view"))
        .and_then(Mode::parse)
        .or_else(|| page.then(|| meta_content(src, "menger-view")).flatten().and_then(Mode::parse));
    if let Some(mode) = view {
        decl.mode = mode;
    }
    let widths = frontmatter
        .and_then(|fm| fm_value(fm, "widths"))
        .and_then(parse_widths)
        .or_else(|| page.then(|| meta_content(src, "menger-widths")).flatten().and_then(parse_widths));
    if let Some(devices) = widths {
        decl.devices = devices;
    }
    decl
}

/// The scale that fits `devices` (all of them side by side, `gap` px
/// apart) into `avail_w` px — and, when the height counts (full screen),
/// `avail_h` px — never above 1.
pub(crate) fn fit(devices: &[Device], gap: f64, avail_w: f64, avail_h: Option<f64>) -> f64 {
    if devices.is_empty() || avail_w <= 0.0 {
        return 1.0;
    }
    let total_w: f64 = devices.iter().map(|d| f64::from(d.w)).sum::<f64>();
    let gaps = gap * (devices.len() as f64 - 1.0);
    let mut s = ((avail_w - gaps).max(1.0) / total_w).min(1.0);
    if let Some(h) = avail_h.filter(|h| *h > 0.0) {
        let tallest = devices.iter().map(|d| f64::from(d.h)).fold(0.0, f64::max);
        s = s.min(h / tallest);
    }
    s.max(0.05)
}

/// Does the route's query ask for full screen (`?view=full`)?
pub(crate) fn wants_full(hash: &str) -> bool {
    hash.split_once('?').is_some_and(|(_, q)| q.split('&').any(|p| p == "view=full"))
}

/// The hash with `view=full` in its query (`on`) or out of it, the rest of
/// the query kept in order.
pub(crate) fn with_full(hash: &str, on: bool) -> String {
    let (path, query) = hash.split_once('?').unwrap_or((hash, ""));
    let mut params: Vec<&str> = query.split('&').filter(|p| !p.is_empty() && *p != "view=full").collect();
    if on {
        params.push("view=full");
    }
    if params.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", params.join("&"))
    }
}

// -- the DOM half ------------------------------------------------------------

thread_local! {
    /// Which box is full screen (its number), if any.
    static FULL: ArcRwSignal<Option<u32>> = ArcRwSignal::new(None);
    static NEXT: Cell<u32> = const { Cell::new(0) };
    /// The ticket page's box — what `z` turns.
    static PAGE: Cell<Option<u32>> = const { Cell::new(None) };
    /// Whether the full box is in the browser's own full screen.
    static NATIVE: Cell<bool> = const { Cell::new(false) };
    static LISTENING: Cell<bool> = const { Cell::new(false) };
    /// Each mounted box's element, by number.
    static ROOTS: RefCell<Vec<(u32, web_sys::Element)>> = const { RefCell::new(Vec::new()) };
}

fn full() -> ArcRwSignal<Option<u32>> {
    FULL.with(Clone::clone)
}

/// `z`: the full box leaves full screen; else the ticket page's box enters
/// it. Whether anything moved.
pub(crate) fn toggle_page() -> bool {
    let f = full();
    if f.get_untracked().is_some() {
        f.set(None);
        return true;
    }
    match PAGE.with(Cell::get) {
        Some(id) => {
            f.set(Some(id));
            true
        }
        None => false,
    }
}

/// `z` posted by a frame: its own box turns.
pub(crate) fn toggle_from(frame: &web_sys::HtmlIFrameElement) {
    let id = frame
        .closest(".framed[data-framed]")
        .ok()
        .flatten()
        .and_then(|el| el.get_attribute("data-framed"))
        .and_then(|n| n.parse::<u32>().ok());
    let f = full();
    match (f.get_untracked(), id) {
        (Some(_), _) => f.set(None),
        (None, Some(id)) => f.set(Some(id)),
        (None, None) => {}
    }
}

/// Esc: leave full screen, if a box is in it. Whether one was.
pub(crate) fn escape() -> bool {
    let f = full();
    if f.get_untracked().is_some() {
        f.set(None);
        return true;
    }
    false
}

/// Call `method` on `target` (a Fullscreen API method); a rejected promise
/// is caught (a deep link has no gesture to ask with: the overlay stands).
fn call(target: &JsValue, method: &str) {
    let Ok(f) = Reflect::get(target, &JsValue::from_str(method)) else { return };
    let Ok(f) = f.dyn_into::<Function>() else { return };
    if let Ok(p) = f.call0(target) {
        if let Ok(p) = p.dyn_into::<Promise>() {
            let noop = Closure::<dyn FnMut(JsValue)>::new(|_| {});
            let _ = p.catch(&noop);
            noop.forget();
        }
    }
}

/// Whether a gesture is under way (`navigator.userActivation.isActive`) —
/// only then is native full screen worth asking for.
fn activation() -> bool {
    let nav: JsValue = dom::window().navigator().into();
    Reflect::get(&nav, &JsValue::from_str("userActivation"))
        .ok()
        .filter(|a| a.is_object())
        .and_then(|a| Reflect::get(&a, &JsValue::from_str("isActive")).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// MENGER-34: whether the browser's own full screen is on — nothing of the
/// app's outside the box (the ticket menu) is drawn over it.
pub(crate) fn native() -> bool {
    fullscreen_element().is_some()
}

fn fullscreen_element() -> Option<web_sys::Element> {
    let doc: JsValue = dom::window().document()?.into();
    Reflect::get(&doc, &JsValue::from_str("fullscreenElement")).ok()?.dyn_into::<web_sys::Element>().ok()
}

/// The browser left its full screen (its own Esc): so does the box.
fn install() {
    if LISTENING.with(|l| l.replace(true)) {
        return;
    }
    let Some(doc) = dom::window().document() else { return };
    let cb = Closure::<dyn FnMut()>::new(|| {
        if fullscreen_element().is_some() {
            NATIVE.with(|n| n.set(true));
        } else if NATIVE.with(|n| n.replace(false)) {
            full().set(None);
            refocus_dialog();
        }
    });
    let _ = doc.add_event_listener_with_callback("fullscreenchange", cb.as_ref().unchecked_ref());
    cb.forget();
}

/// Leaving the browser's full screen hands focus back to the element that
/// held it — the frame, for a link clicked in it — after a dialog that
/// opened meanwhile (MENGER-14's leave dialog) took it: the dialog takes it
/// again, so its keys (Esc, Enter, Tab) stay its own.
fn refocus_dialog() {
    let Some(doc) = dom::window().document() else { return };
    let in_dialog = doc.active_element().and_then(|a| a.closest("[role=dialog]").ok().flatten()).is_some();
    if in_dialog {
        return;
    }
    let target = doc
        .query_selector("[role=dialog] [data-act=open], [role=dialog] button")
        .ok()
        .flatten()
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok());
    if let Some(el) = target {
        let _ = el.focus();
    }
}

/// The route's query, rewritten in place — no history entry, no hashchange.
fn set_route_full(on: bool) {
    let w = dom::window();
    let hash = w.location().hash().unwrap_or_default();
    if wants_full(&hash) == on || !hash.starts_with("#/ticket/") {
        return;
    }
    let next = with_full(&hash, on);
    if let Ok(h) = w.history() {
        let _ = h.replace_state_with_url(&JsValue::NULL, "", Some(&next));
    }
}

/// The box a framed body sits in (see the module doc): the bar, then the
/// frame — or, for `view: mockup`, its devices. `title` is the ticket
/// page's (a box elsewhere — a board note, the editor's preview — has
/// none): it names the ticket in the full-screen bar, and makes this the
/// box `z` and `?view=full` (and `view: full`) open.
#[component]
pub(crate) fn Framed(
    #[prop(into)] html: Signal<String>,
    #[prop(into)] decl: Signal<Decl>,
    ticket: Option<String>,
    #[prop(optional)] scope: Option<String>,
    #[prop(optional)] title: Option<Signal<String>>,
) -> impl IntoView {
    install();
    let id = NEXT.with(|n| {
        let v = n.get().wrapping_add(1);
        n.set(v);
        v
    });
    let on_page = title.is_some();
    let root = NodeRef::<leptos::html::Div>::new();
    let stage = NodeRef::<leptos::html::Div>::new();
    let f = full();
    let is_full = {
        let f = f.clone();
        Memo::new(move |_| f.get() == Some(id))
    };
    if on_page {
        PAGE.with(|p| p.set(Some(id)));
    }
    root.on_load(move |el: web_sys::HtmlDivElement| {
        ROOTS.with(|r| r.borrow_mut().push((id, el.into())));
    });
    on_cleanup(move || {
        ROOTS.with(|r| r.borrow_mut().retain(|(n, _)| *n != id));
        if PAGE.with(Cell::get) == Some(id) {
            PAGE.with(|p| p.set(None));
        }
        let f = full();
        if f.get_untracked() == Some(id) {
            f.set(None);
        }
    });
    // The ticket page opens full when the route or the body says so — once,
    // as it mounts.
    if on_page {
        let declared_full = decl.with_untracked(|d| d.mode == Mode::Full);
        if declared_full || wants_full(&dom::window().location().hash().unwrap_or_default()) {
            f.set(Some(id));
        }
    }
    // Entering and leaving: the browser's full screen when a gesture allows,
    // the route's query on the ticket page.
    Effect::new(move |was: Option<bool>| {
        let now = is_full.get();
        if was == Some(now) || (was.is_none() && !now) {
            return now;
        }
        let el = ROOTS.with(|r| r.borrow().iter().find(|(n, _)| *n == id).map(|(_, e)| e.clone()));
        if now {
            if let Some(el) = el.filter(|_| activation()) {
                call(&el.into(), "requestFullscreen");
            }
        } else if fullscreen_element().is_some() {
            if let Some(doc) = dom::window().document() {
                call(&doc.into(), "exitFullscreen");
            }
        }
        if on_page {
            set_route_full(now);
        }
        now
    });
    let toggle = {
        let f = f.clone();
        move |_| {
            if f.get_untracked() == Some(id) {
                f.set(None);
            } else {
                f.set(Some(id));
            }
        }
    };
    let declared_full = move || decl.with(|d| d.mode == Mode::Full);
    let label = move || match (is_full.get(), declared_full() && on_page) {
        (true, true) => "← Back to the ticket",
        (true, false) => "Exit full screen",
        (false, _) => "⤢ Full screen",
    };
    let mode = move || decl.with(|d| d.mode.as_str());
    let mockup = Memo::new(move |_| decl.with(|d| (d.mode == Mode::Mockup).then(|| d.devices.clone())));
    let ticket_id = ticket.clone();
    let heading = move || {
        let t = title?;
        Some(view! {
            <span class="framed-id code">{ticket_id.clone()}</span>
            <span class="framed-title">{move || t.get()}</span>
        })
    };
    // A mockup's toggles, kept while the box is mounted.
    let app_scheme = crate::frame::painted_scheme();
    let scheme = RwSignal::new(app_scheme);
    let reduce = RwSignal::new(prefers_reduced_motion());
    let solo = RwSignal::new(None::<usize>);
    let size = RwSignal::new((0.0f64, 0.0f64));
    stage.on_load(move |el: web_sys::HtmlDivElement| observe(el, size));
    let body = move || match mockup.get() {
        None => {
            let ticket = ticket.clone();
            let scope = scope.clone().unwrap_or_default();
            view! { <Frame html ticket scope /> }.into_any()
        }
        Some(devices) => {
            let ticket = ticket.clone();
            let scope = scope.clone().unwrap_or_default();
            mockup_view(html, devices, ticket, scope, scheme, reduce, solo, size, is_full).into_any()
        }
    };
    let controls = move || {
        mockup.with(|m| m.clone()).map(|devices| {
            let buttons = devices
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    let label = format!("{}", d.w);
                    let tip = format!("{} × {} ({})", d.w, d.h, d.kind());
                    view! {
                        <button class="framed-btn" title=tip aria-pressed=move || (solo.get() == Some(i)).to_string() on:click=move |_| solo.set(Some(i))>{label}</button>
                    }
                })
                .collect_view();
            view! {
                <span class="framed-group" role="group" aria-label="devices">
                    <button class="framed-btn" title="every device, side by side" aria-pressed=move || solo.get().is_none().to_string() on:click=move |_| solo.set(None)>"All"</button>
                    {buttons}
                </span>
                <span class="framed-group" role="group" aria-label="colour scheme">
                    <button class="framed-btn" aria-pressed=move || (scheme.get() == Scheme::Light).to_string() on:click=move |_| scheme.set(Scheme::Light)>"Light"</button>
                    <button class="framed-btn" aria-pressed=move || (scheme.get() == Scheme::Dark).to_string() on:click=move |_| scheme.set(Scheme::Dark)>"Dark"</button>
                </span>
                <button class="framed-btn framed-motion" title="prefers-reduced-motion, as the page sees it" aria-pressed=move || reduce.get().to_string() on:click=move |_| reduce.update(|r| *r = !*r)>
                    "Reduced motion"
                </button>
            }
        })
    };
    view! {
        <div class="framed" class:is-full=move || is_full.get() data-framed=id.to_string() data-view=mode node_ref=root>
            <div class="framed-bar">
                {move || if is_full.get() { heading() } else { None }}
                <span class="framed-gap"></span>
                {controls}
                <button class="framed-btn framed-full" title="Full screen (z) — Esc leaves it" aria-pressed=move || is_full.get().to_string() on:click=toggle>
                    {label}
                </button>
            </div>
            <div class="framed-stage" node_ref=stage>{body}</div>
        </div>
    }
}

/// The viewer's own `prefers-reduced-motion` — a mockup's toggle starts there.
fn prefers_reduced_motion() -> bool {
    dom::window()
        .match_media("(prefers-reduced-motion: reduce)")
        .ok()
        .flatten()
        .is_some_and(|m| m.matches())
}

/// Keep `size` the stage's inner size, as it changes. MENGER-20: the stage
/// leaving the page (its ticket closed) is one more change, seen after
/// `size` has been disposed with its box — then the observer lets go of the
/// stage instead of reading the dead signal, which panicked on every ticket
/// left.
fn observe(el: web_sys::HtmlDivElement, size: RwSignal<(f64, f64)>) {
    // false once the box is gone
    let measure = {
        let el = el.clone();
        move || {
            let Some(was) = size.try_get_untracked() else { return false };
            let w = f64::from(el.client_width());
            let h = f64::from(el.client_height());
            if was != (w, h) {
                size.set((w, h));
            }
            true
        }
    };
    measure();
    let Ok(ctor) = Reflect::get(&dom::window(), &JsValue::from_str("ResizeObserver")) else { return };
    let Ok(ctor) = ctor.dyn_into::<Function>() else { return };
    // A ResizeObserver's callback is given the observer itself.
    let cb = Closure::<dyn FnMut(JsValue, JsValue)>::new(move |_entries: JsValue, ro: JsValue| {
        if !measure() {
            if let Ok(disconnect) = Reflect::get(&ro, &JsValue::from_str("disconnect")).and_then(|f| f.dyn_into::<Function>()) {
                let _ = disconnect.call0(&ro);
            }
        }
    });
    let Ok(ro) = Reflect::construct(&ctor, &js_sys::Array::of1(cb.as_ref())) else { return };
    cb.forget();
    if let Ok(observe) = Reflect::get(&ro, &JsValue::from_str("observe")).and_then(|f| f.dyn_into::<Function>()) {
        let _ = observe.call1(&ro, &el);
    }
}

/// The gap between devices side by side, px.
const GAP: f64 = 24.0;
/// A device's label and chrome, above and around its screen, px.
const CHROME_H: f64 = 84.0;
/// A device's bezel, left and right of its screen (at most), px.
const BEZEL_W: f64 = 20.0;

/// A mockup's devices: each a frame of the body at its viewport, scaled.
#[allow(clippy::too_many_arguments)]
fn mockup_view(
    html: Signal<String>,
    devices: Vec<Device>,
    ticket: Option<String>,
    scope: String,
    scheme: RwSignal<Scheme>,
    reduce: RwSignal<bool>,
    solo: RwSignal<Option<usize>>,
    size: RwSignal<(f64, f64)>,
    is_full: Memo<bool>,
) -> impl IntoView {
    let emulate = Signal::derive(move || Emulate { scheme: Some(scheme.get()), reduce: Some(reduce.get()) });
    let all = devices.clone();
    let scale = Memo::new(move |_| {
        let (w, h) = size.get();
        let shown: Vec<Device> = match solo.get() {
            Some(i) => all.get(i).copied().into_iter().collect(),
            None => all.clone(),
        };
        let avail_h = is_full.get().then_some(h - CHROME_H);
        // The stage's padding, and each device's bezel, are not scaled.
        fit(&shown, GAP, w - 12.0 - BEZEL_W * shown.len() as f64, avail_h)
    });
    devices
        .into_iter()
        .enumerate()
        .map(|(i, d)| {
            let ticket = ticket.clone();
            let scope = scope.clone();
            let shown = move || solo.get().is_none_or(|s| s == i);
            let screen = move || {
                let s = scale.get();
                format!("width:{:.1}px;height:{:.1}px", f64::from(d.w) * s, f64::from(d.h) * s)
            };
            let inner = move || format!("width:{}px;height:{}px;transform:scale({:.4})", d.w, d.h, scale.get());
            let label = format!("{} × {}", d.w, d.h);
            view! {
                <figure class="device" data-kind=d.kind() data-w=d.w.to_string() class:hidden=move || !shown()>
                    <figcaption class="device-label">
                        <span>{d.kind()}</span>
                        <span class="code">{label}</span>
                        <span class="device-scale">{move || format!("{:.0}%", scale.get() * 100.0)}</span>
                    </figcaption>
                    <div class="device-screen" style=screen>
                        <div class="device-inner" style=inner>
                            <Frame html ticket scope emulate fixed=true />
                        </div>
                    </div>
                </figure>
            }
        })
        .collect_view()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_declares_its_view_in_its_frontmatter() {
        let d = declared(Some("title: x\nview: mockup\nwidths: 390, 768x1000, 1440\n"), "<p>x</p>");
        assert_eq!(d.mode, Mode::Mockup);
        assert_eq!(d.devices, vec![Device { w: 390, h: 844 }, Device { w: 768, h: 1000 }, Device { w: 1440, h: 900 }]);
        assert_eq!(declared(Some("view: full"), "x").mode, Mode::Full);
        assert_eq!(declared(Some("view: \"Mockup\"  # the landing"), "x").mode, Mode::Mockup);
        assert_eq!(declared(Some("widths: [320, 1920]"), "x").devices, vec![Device::of_width(320), Device::of_width(1920)]);
        // Nothing, or nothing readable: inline, the three defaults.
        for fm in [None, Some("view: sideways"), Some("  view: mockup"), Some("widths: wide")] {
            let d = declared(fm, "x");
            assert_eq!(d, Decl::default(), "{fm:?}");
        }
        assert_eq!(Decl::default().devices.iter().map(|d| d.w).collect::<Vec<_>>(), DEFAULT_WIDTHS);
    }

    #[test]
    fn a_whole_document_may_declare_it_in_its_head() {
        let page = "<!doctype html><html><head><meta name=\"menger-view\" content=\"mockup\"><meta name='menger-widths' content='375,1280x800'></head><body></body></html>";
        let d = declared(None, page);
        assert_eq!((d.mode, d.devices), (Mode::Mockup, vec![Device::of_width(375), Device { w: 1280, h: 800 }]));
        // The frontmatter wins; a fragment's meta is no declaration.
        assert_eq!(declared(Some("view: full"), page).mode, Mode::Full);
        assert_eq!(declared(None, "<div><meta name=\"menger-view\" content=\"mockup\"></div>").mode, Mode::Inline);
    }

    #[test]
    fn widths_outside_reason_are_dropped() {
        assert_eq!(parse_widths("100, 390, 99999, 768x10, 1024x768px"), Some(vec![Device::of_width(390), Device { w: 1024, h: 768 }]));
        assert_eq!(parse_widths("1,2,3"), None);
        assert_eq!(parse_widths("400 500 600 700 800 900 1000 1100").map(|d| d.len()), Some(MAX_DEVICES));
        assert_eq!(Device::of_width(390).kind(), "phone");
        assert_eq!(Device::of_width(768).kind(), "tablet");
        assert_eq!(Device::of_width(1440).kind(), "desktop");
    }

    #[test]
    fn devices_side_by_side_share_one_scale_that_fits() {
        let d: Vec<Device> = DEFAULT_WIDTHS.iter().map(|w| Device::of_width(*w)).collect();
        // 2598 px of devices and two 24 px gaps into 1000 px.
        let s = fit(&d, 24.0, 1000.0, None);
        assert!((s - (952.0 / 2598.0)).abs() < 1e-9, "{s}");
        // Never larger than life; the height counts in full screen.
        assert_eq!(fit(&d[..1], 24.0, 4000.0, None), 1.0);
        assert!((fit(&d[2..], 24.0, 4000.0, Some(450.0)) - 0.5).abs() < 1e-9);
        assert_eq!(fit(&[], 24.0, 100.0, None), 1.0);
    }

    #[test]
    fn the_route_says_full_screen_in_its_query() {
        assert!(wants_full("#/ticket/A-1?view=full"));
        assert!(wants_full("#/ticket/A-1?component=m-x&view=full"));
        assert!(!wants_full("#/ticket/A-1") && !wants_full("#/ticket/A-1?view=fuller"));
        assert_eq!(with_full("#/ticket/A-1", true), "#/ticket/A-1?view=full");
        assert_eq!(with_full("#/ticket/A-1?view=full", false), "#/ticket/A-1");
        assert_eq!(with_full("#/ticket/A-1?field=x&view=full&g=1", false), "#/ticket/A-1?field=x&g=1");
        assert_eq!(with_full("#/ticket/A-1?field=x", true), "#/ticket/A-1?field=x&view=full");
        assert_eq!(with_full("#/ticket/A-1?view=full", true), "#/ticket/A-1?view=full");
    }
}
