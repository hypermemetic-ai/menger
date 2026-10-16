//! APP-29: **`<m-mockup>`** — a website mockup as a component of a body,
//! with prose, decisions and other components around it, several per body.
//! Owner: *"we should be able to include text where we put the mock. the
//! mock should be another component that fits in."*
//!
//! ````markdown
//! ```mockup widths=390,1440
//! <!doctype html>…the page…
//! ```
//!
//! <m-mockup ref="DESIGN-49"></m-mockup>              <!-- another ticket's page -->
//! <m-mockup ref="DESIGN-49" widths="390"></m-mockup>
//! ````
//!
//! A fence holds the page inline: md.rs makes it an `<m-mockup source="…">`
//! placeholder, the page in an escaped attribute (never parsed in the body).
//! `ref` names a ticket whose body is a whole HTML document (md::is_page),
//! drawn at the widths the element names, else the ones that page declares
//! (`view: mockup` + `widths:`, or `<meta name="menger-widths">`), else
//! [`present::DEFAULT_WIDTHS`] ([`present::mock_devices`]).
//!
//! **The design is `<m-cite>`'s (src/cite.rs): the host draws the mockup,
//! beside the body's frame — never inside it.** A body is a sandboxed frame
//! with an opaque origin: `/app/frame` cannot nest in it, and a srcdoc
//! mockup inside it would lose the host's handshake, the page shim's
//! emulation, the theme and the links. So the SDK's `<m-mockup>` is a
//! placeholder: it reserves room and posts `menger:mockup { mock, ref,
//! widths, source, rect }` (`menger:unmock { mock }` when it leaves). The
//! host then, in the app's own DOM:
//!
//! 1. reads what to draw ([`parse`]) — the page the placeholder carries
//!    (the body's own text: handing it over reveals nothing new), or, for a
//!    `ref`, the ticket **over the viewer's own socket** (`tickets.get`,
//!    the viewer's reach; hidden ≡ not found — [`cite::classify`]), which
//!    must be a whole document ([`page_of`]). The citing frame never
//!    receives the other ticket's body (IDENTITY-36);
//! 2. lays a box (`div.mmock`) over the placeholder, clamped to the frame
//!    ([`cite::place`]), and mounts in it the same [`present::Framed`] box a
//!    `view: mockup` body gets — one device frame per width side by side,
//!    All / per device, Light / Dark, Reduced motion, ⤢ Full screen — each
//!    device a [`crate::frame::Frame`] with `emulate` and `fixed`, stamped
//!    with no ticket (nothing in a mock answers an input);
//! 3. answers the placeholder **only the box's height** — `menger:mocked {
//!    mock, height }` ([`mocked_reply`]) — so the prose below it moves down.
//!
//! **Live.** An `amended` of a referenced ticket redraws its mockups in
//! place (the devices' frames re-render without a flash); a ticket created
//! under a referenced id that was none is fetched. The citing frame's
//! re-render, resize and promotion move its boxes as they move cites'.
//!
//! **Failure is a named state** ([`state_text`]), in place of the devices:
//! an unknown or unreadable ticket, a ticket that is no whole page, a
//! malformed element, one past [`MAX_PER_FRAME`], one inside a mock.
//!
//! What decides is pure and pinned by native tests; the rest is DOM.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;

use js_sys::{Function, Reflect};
use leptos::prelude::*;
use menger_tickets_api::{Ticket, TicketsEvent};
use serde::Serialize;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::spawn_local;

use crate::cite::{self, Fetched, Rect};
use crate::present::{self, Decl, Mode};
use crate::{bridge, dom, frame, md, session};

/// The most mockups one body draws: each is up to six frames, and a `ref`
/// a read over the viewer's socket.
pub(crate) const MAX_PER_FRAME: usize = 16;

/// The class a mockup's box wears, in the body frame's container.
pub(crate) const BOX_CLASS: &str = "mmock";

/// What a placeholder draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// A fence's page, carried in the placeholder.
    Inline(String),
    /// Another ticket's whole-document body, by id.
    Ref(String),
}

/// `<m-mockup ref source>` as posted, as a [`Source`] — or why not, which
/// the box shows. One of the two, never both, never neither.
pub(crate) fn parse(reference: Option<&str>, source: Option<&str>) -> Result<Source, String> {
    let reference = reference.map(str::trim).filter(|r| !r.is_empty());
    match (reference, source) {
        (Some(_), Some(_)) => Err("m-mockup: give ref=\"ID\" or a ```mockup fence's page, not both".into()),
        (None, None) => Err("m-mockup: name a page — ref=\"DESIGN-49\", or write a ```mockup fence".into()),
        (None, Some(page)) => Ok(Source::Inline(page.to_string())),
        (Some(r), None) => {
            let id = r.to_ascii_uppercase();
            if cite::ticket_ok(&id) {
                Ok(Source::Ref(id))
            } else {
                Err(format!("m-mockup: ref=\"{}\" is no ticket id (EPIC-N)", clip(r)))
            }
        }
    }
}

fn clip(s: &str) -> String {
    s.chars().take(64).collect()
}

/// Where a box stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum State {
    /// A `ref` asked; nothing to draw yet.
    Loading,
    /// The devices, drawn.
    Shown,
    NoTicket,
    Unreadable,
    /// The referenced ticket's body is no whole HTML document.
    NotPage,
    /// The element does not say a mockup ([`parse`]'s reason).
    Invalid(String),
    /// A placeholder inside a mock's own page draws nothing.
    Nested,
}

/// The line a box shows instead of its devices — `None` while it shows
/// them. Nothing here comes from a ticket but the id the body wrote.
pub(crate) fn state_text(source: Option<&Source>, state: &State) -> Option<String> {
    let who = match source {
        Some(Source::Ref(id)) => id.clone(),
        _ => "m-mockup".to_string(),
    };
    match state {
        State::Shown => None,
        State::Loading => Some(format!("{who}…")),
        State::NoTicket => Some(format!("{who}: no ticket you can read")),
        State::Unreadable => Some(format!("you cannot read {who}")),
        State::NotPage => Some(format!("{who} is no whole HTML page — a mockup shows a body that starts <!doctype html>")),
        State::Invalid(why) => Some(why.clone()),
        State::Nested => Some("m-mockup: a mock's own page draws no mockup".into()),
    }
}

/// A referenced ticket's body as a mockup's page and what it declares —
/// `None` when it is no whole document.
pub(crate) fn page_of(body: &str) -> Option<(String, Decl)> {
    let (fm, src) = md::split_frontmatter(body);
    md::is_page(src).then(|| (src.to_string(), present::declared(fm, src)))
}

/// What the box draws, from its source and (for a `ref`) what was read: the
/// page and its declaration, or the state instead.
pub(crate) fn resolve(source: &Source, fetched: Option<&Fetched>) -> Result<(String, Option<Decl>), State> {
    match (source, fetched) {
        (Source::Inline(page), _) => Ok((page.clone(), None)),
        (Source::Ref(_), None) => Err(State::Loading),
        (Source::Ref(_), Some(Fetched::NoTicket)) => Err(State::NoTicket),
        (Source::Ref(_), Some(Fetched::Unreadable)) => Err(State::Unreadable),
        (Source::Ref(_), Some(Fetched::Ticket(t))) => page_of(&t.body).map(|(p, d)| (p, Some(d))).ok_or(State::NotPage),
    }
}

/// The declaration the mounted box is given: a mockup, at `widths` (the
/// element's) or the page's declared devices.
pub(crate) fn decl_for(widths: Option<&str>, declared: Option<&Decl>) -> Decl {
    Decl { mode: Mode::Mockup, devices: present::mock_devices(widths, declared) }
}

/// `menger:mocked`'s payload past its type and channel — the one thing the
/// body's frame is told: how tall its mockup is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct MockedReply {
    pub(crate) mock: u32,
    pub(crate) height: u32,
}

/// The reply for mockup `n`, its box `height` px tall.
pub(crate) fn mocked_reply(n: u32, height: f64) -> MockedReply {
    let h = if height.is_finite() { height.clamp(0.0, 50_000.0).ceil() as u32 } else { 0 };
    MockedReply { mock: n, height: h }
}

/// The line of a placeholder past [`MAX_PER_FRAME`], when `open` are open.
pub(crate) fn over_cap(open: usize) -> Option<String> {
    (open >= MAX_PER_FRAME).then(|| format!("m-mockup: a body draws at most {MAX_PER_FRAME} mockups"))
}

// -- the DOM half -------------------------------------------------------------

/// One mockup: placeholder `n` of the frame on `channel`.
struct Entry {
    citer: web_sys::HtmlIFrameElement,
    channel: String,
    n: u32,
    /// What the placeholder posted, to tell a change from a move.
    posted: (Option<String>, Option<String>, Option<String>),
    widths: Option<String>,
    source: Result<Source, String>,
    overlay: web_sys::HtmlElement,
    rect: Rect,
    state: State,
    fetched: Option<Fetched>,
    told: Option<u32>,
    asked: u32,
    /// The page and declaration the mounted box reads.
    html: ArcRwSignal<String>,
    decl: ArcRwSignal<Decl>,
    /// The mounted box (an UnmountHandle — dropped, it unmounts).
    mounted: Option<Box<dyn Any>>,
    observer: Option<JsValue>,
}

impl Entry {
    fn remove(self) {
        if let Some(ro) = &self.observer {
            if let Ok(f) = Reflect::get(ro, &JsValue::from_str("disconnect")).and_then(|f| f.dyn_into::<Function>()) {
                let _ = f.call0(ro);
            }
        }
        drop(self.mounted);
        self.overlay.remove();
    }
}

thread_local! {
    static BOXES: RefCell<HashMap<String, Entry>> = RefCell::new(HashMap::new());
}

fn key(channel: &str, n: u32) -> String {
    format!("{channel}:m{n}")
}

fn el(tag: &str, class: &str) -> Option<web_sys::HtmlElement> {
    let e = dom::window().document()?.create_element(tag).ok()?;
    let _ = e.set_attribute("class", class);
    e.dyn_into::<web_sys::HtmlElement>().ok()
}

fn child(parent: &web_sys::Element, selector: &str) -> Option<web_sys::HtmlElement> {
    parent.query_selector(selector).ok().flatten().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
}

fn style(e: &web_sys::HtmlElement, prop: &str, value: &str) {
    let _ = e.style().set_property(prop, value);
}

/// A box's skeleton: its state line, and where the devices mount.
fn build(key: &str, channel: &str, n: u32) -> Option<web_sys::HtmlElement> {
    let w = el("div", BOX_CLASS)?;
    let _ = w.set_attribute("data-for", channel);
    let _ = w.set_attribute("data-mock", &n.to_string());
    let _ = w.set_attribute("data-key", key);
    let _ = w.set_attribute("role", "figure");
    let _ = w.set_attribute("aria-label", "website mockup");
    let state = el("p", "mmock-state")?;
    let host = el("div", "mmock-host")?;
    let _ = w.append_child(&state);
    let _ = w.append_child(&host);
    Some(w)
}

fn with_entry(key: &str, f: impl FnOnce(&mut Entry)) {
    BOXES.with(|b| {
        if let Some(e) = b.borrow_mut().get_mut(key) {
            f(e);
        }
    });
}

/// Paint the state line; mount the devices once there is a page, unmount
/// them when there is none.
fn paint(e: &mut Entry) {
    let source = e.source.as_ref().ok();
    let text = state_text(source, &e.state);
    if let Some(p) = child(&e.overlay, ":scope > .mmock-state") {
        p.set_text_content(text.as_deref());
        p.set_hidden(text.is_none());
    }
    let _ = e.overlay.set_attribute(
        "data-state",
        match e.state {
            State::Shown => "shown",
            State::Loading => "loading",
            _ => "failed",
        },
    );
    if e.state != State::Shown {
        e.mounted = None;
        return;
    }
    if e.mounted.is_some() {
        return;
    }
    let Some(host) = child(&e.overlay, ":scope > .mmock-host") else { return };
    let (html, decl) = (e.html.clone(), e.decl.clone());
    // The reads a fragment's SDK may ask are held as a cite's frame's are:
    // to the page's own ticket's shortcode — the referenced one, or the body's.
    let scope = match &e.source {
        Ok(Source::Ref(id)) => bridge::scope_of(Some(id)),
        _ => e.citer.get_attribute("data-scope"),
    }
    .unwrap_or_default();
    let handle = leptos::mount::mount_to(host, move || {
        let html = Signal::derive(move || html.get());
        let decl = Signal::derive(move || decl.get());
        view! { <present::Framed html decl ticket=None scope /> }
    });
    e.mounted = Some(Box::new(handle));
}

/// Lay the box over its placeholder.
fn position(e: &Entry) {
    let f = &e.citer;
    let p = cite::place(
        e.rect,
        f64::from(f.offset_left()),
        f64::from(f.offset_top()),
        f64::from(f.offset_width()),
        f64::from(f.offset_height()),
    );
    style(&e.overlay, "top", &format!("{}px", p.top));
    style(&e.overlay, "left", &format!("{}px", p.left));
    style(&e.overlay, "width", &format!("{}px", p.width));
    e.overlay.set_hidden(!p.shown);
}

/// Tell the body how tall its mockup is, when that changed — never while
/// the box is full screen (it is out of the flow then; its room stays).
fn tell(e: &mut Entry) {
    if e.overlay.query_selector(".framed.is-full").ok().flatten().is_some() {
        return;
    }
    let h = e.overlay.get_bounding_client_rect().height();
    let reply = mocked_reply(e.n, if e.overlay.hidden() { 0.0 } else { h });
    if e.told == Some(reply.height) {
        return;
    }
    e.told = Some(reply.height);
    frame::post_mocked(&e.citer, &e.channel, &reply);
}

/// Every size change of the box is told; the observer lets go once the box
/// is gone.
fn observe(k: &str, overlay: &web_sys::HtmlElement) -> Option<JsValue> {
    let ctor = Reflect::get(&dom::window(), &JsValue::from_str("ResizeObserver")).ok()?.dyn_into::<Function>().ok()?;
    let k = k.to_string();
    let cb = Closure::<dyn FnMut(JsValue, JsValue)>::new(move |_entries: JsValue, ro: JsValue| {
        let alive = BOXES.with(|b| b.borrow().contains_key(&k));
        if alive {
            with_entry(&k, tell);
        } else if let Ok(f) = Reflect::get(&ro, &JsValue::from_str("disconnect")).and_then(|f| f.dyn_into::<Function>()) {
            let _ = f.call0(&ro);
        }
    });
    let ro = Reflect::construct(&ctor, &js_sys::Array::of1(cb.as_ref())).ok()?;
    cb.forget();
    let observe = Reflect::get(&ro, &JsValue::from_str("observe")).ok()?.dyn_into::<Function>().ok()?;
    let _ = observe.call1(&ro, overlay);
    Some(ro)
}

/// Boxes whose body frame left the document go with it.
fn prune() {
    let gone: Vec<String> = BOXES.with(|b| {
        b.borrow().iter().filter(|(_, e)| !(e.citer.is_connected() && e.overlay.is_connected())).map(|(k, _)| k.clone()).collect()
    });
    for k in gone {
        if let Some(e) = BOXES.with(|b| b.borrow_mut().remove(&k)) {
            e.remove();
        }
    }
}

/// Land what the box draws — the page, or its state.
fn settle(e: &mut Entry) {
    let resolved = match &e.source {
        Ok(source) => resolve(source, e.fetched.as_ref()),
        Err(why) => Err(State::Invalid(why.clone())),
    };
    match resolved {
        Ok((page, declared)) => {
            let decl = decl_for(e.widths.as_deref(), declared.as_ref());
            if e.decl.get_untracked() != decl {
                e.decl.set(decl);
            }
            if e.html.get_untracked() != page {
                e.html.set(page);
            }
            e.state = State::Shown;
        }
        Err(state) => e.state = state,
    }
    paint(e);
    tell(e);
}

/// `menger:mockup` from the frame `citer` on `channel`: its placeholder `n`
/// says `reference` / `widths` / `source` and sits at `rect`. A new or
/// changed one is drawn (a `ref` fetched); a moved one is laid again.
pub(crate) fn asked(
    citer: web_sys::HtmlIFrameElement,
    channel: String,
    n: u32,
    reference: Option<String>,
    widths: Option<String>,
    source: Option<String>,
    rect: Rect,
) {
    prune();
    let k = key(&channel, n);
    let posted = (reference, widths, source);
    let same = BOXES.with(|b| b.borrow().get(&k).map(|e| e.posted == posted));
    match same {
        Some(true) => {
            with_entry(&k, |e| {
                e.rect = rect;
                position(e);
                tell(e);
            });
            return;
        }
        Some(false) => {
            if let Some(old) = BOXES.with(|b| b.borrow_mut().remove(&k)) {
                old.remove();
            }
        }
        None => {}
    }
    let Some(container) = citer.parent_element() else { return };
    let Some(overlay) = build(&k, &channel, n) else { return };
    if citer.get_attribute("data-state").as_deref() == Some("pending") {
        let _ = overlay.set_attribute("data-pending", "");
    }
    let _ = container.append_child(&overlay);
    let open = BOXES.with(|b| b.borrow().values().filter(|e| e.channel == channel).count());
    // A mock's own page (a fragment, with the SDK) draws no mockup in turn.
    let nested = citer.closest(&format!(".{BOX_CLASS}")).ok().flatten().is_some();
    let source = match over_cap(open) {
        Some(why) => Err(why),
        None if nested => Err(state_text(None, &State::Nested).unwrap_or_default()),
        None => parse(posted.0.as_deref(), posted.2.as_deref()),
    };
    let fetch = matches!(source, Ok(Source::Ref(_)));
    let observer = observe(&k, &overlay);
    let mut e = Entry {
        citer,
        channel,
        n,
        widths: posted.1.clone(),
        posted,
        source,
        overlay,
        rect,
        state: State::Loading,
        fetched: None,
        told: None,
        asked: 0,
        html: ArcRwSignal::new(String::new()),
        decl: ArcRwSignal::new(Decl::default()),
        mounted: None,
        observer,
    };
    // Laid before it mounts, so the devices first measure the right width.
    position(&e);
    settle(&mut e);
    BOXES.with(|b| b.borrow_mut().insert(k.clone(), e));
    if fetch {
        fetch_into(k);
    }
}

/// Ask the server, over the viewer's own socket, for the box's ticket.
fn fetch_into(k: String) {
    let Some((id, n)) = BOXES.with(|b| {
        let mut b = b.borrow_mut();
        let e = b.get_mut(&k)?;
        e.asked = e.asked.wrapping_add(1);
        match e.source.as_ref().ok()? {
            Source::Ref(id) => Some((id.clone(), e.asked)),
            Source::Inline(_) => None,
        }
    }) else {
        return;
    };
    spawn_local(async move {
        let answer = match session::client() {
            Some(client) => match crate::typed::typed(&id) {
                Ok(id) => client.get_all(id).await.map_err(|e| session::describe(&e)),
                Err(why) => Err(why),
            },
            None => Err("not connected".to_string()),
        };
        let fetched = cite::classify(answer);
        with_entry(&k, |e| {
            if e.asked == n {
                e.fetched = Some(fetched);
                settle(e);
            }
        });
    });
}

/// `menger:unmock`: the placeholder left the body.
pub(crate) fn dropped(channel: &str, n: u32) {
    if let Some(e) = BOXES.with(|b| b.borrow_mut().remove(&key(channel, n))) {
        e.remove();
    }
}

/// A frame took over in its container: the boxes of the frame it replaced
/// go, its own show and are laid again.
pub(crate) fn promoted(frame: &web_sys::HtmlIFrameElement) {
    let Some(channel) = frame.get_attribute("data-channel") else { return };
    let Some(container) = frame.parent_element() else { return };
    let Ok(list) = container.query_selector_all(&format!(":scope > .{BOX_CLASS}")) else { return };
    let mut gone = Vec::new();
    for i in 0..list.length() {
        let Some(w) = list.get(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
        if w.get_attribute("data-for").as_deref() == Some(channel.as_str()) {
            let _ = w.remove_attribute("data-pending");
        } else {
            match w.get_attribute("data-key") {
                Some(k) => gone.push(k),
                None => w.remove(),
            }
        }
    }
    for k in gone {
        if let Some(e) = BOXES.with(|b| b.borrow_mut().remove(&k)) {
            e.remove();
        }
    }
    BOXES.with(|b| {
        for e in b.borrow_mut().values_mut().filter(|e| e.channel == channel) {
            position(e);
            tell(e);
        }
    });
}

/// A body frame resized: its boxes are laid again.
pub(crate) fn relayout(channel: &str) {
    BOXES.with(|b| {
        for e in b.borrow_mut().values_mut().filter(|e| e.channel == channel) {
            position(e);
        }
    });
}

/// Every `watch` event the session hears: a referenced ticket amended
/// redraws its mockups; a ticket created under a referenced id that was
/// none is fetched.
pub(crate) fn heard(ev: &TicketsEvent) {
    let (ticket, created): (&Ticket, bool) = match ev {
        TicketsEvent::Amended { ticket, .. } => (ticket, false),
        TicketsEvent::Created { ticket } => (ticket, true),
        _ => return,
    };
    prune();
    let mut refetch = Vec::new();
    BOXES.with(|b| {
        for (k, e) in b.borrow_mut().iter_mut() {
            if !matches!(&e.source, Ok(Source::Ref(id)) if *id == ticket.id) {
                continue;
            }
            match &e.fetched {
                Some(Fetched::Ticket(_)) => {
                    e.fetched = Some(Fetched::Ticket(Box::new(ticket.clone())));
                    refetch.push((k.clone(), false));
                }
                _ if created || e.fetched.is_some() => refetch.push((k.clone(), true)),
                _ => {}
            }
        }
    });
    // Settled outside the map's borrow: a mount runs the box's effects.
    for (k, fetch) in refetch {
        if fetch {
            fetch_into(k);
        } else {
            with_entry(&k, settle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::present::Device;
    use menger_tickets_api::{Kind, Status};

    fn ticket(id: &str, body: &str) -> Ticket {
        let (epic, seq) = id.rsplit_once('-').unwrap();
        Ticket {
            hidden_blocked_by: 0,
            hidden_unlocks: 0,
            parent_hidden: false,
            labels: Vec::new(),
            id: menger_tickets_api::TicketId::new(id),
            epic: menger_tickets_api::Code::stored(epic),
            shortcode: menger_tickets_api::ShortcodeId::new(epic),
            address: String::new(),
            seq: seq.parse().unwrap(),
            title: "secret title".into(),
            status: Status::Doc,
            kind: Kind::Doc,
            severity: None,
            blocked_by: vec![],
            unlocks: vec![],
            parent: None,
            child_count: 0,
            commits: vec![],
            path: None,
            placement: None,
            body: body.into(),
            created_unix: 0,
            updated_unix: 0,
            moved_from: Vec::new(),
        }
    }

    const PAGE: &str = "<!doctype html><html><head><meta name=\"menger-widths\" content=\"375,1280x800\"></head><body>hi</body></html>";

    /// A placeholder carries a page or names a ticket — one, never both.
    #[test]
    fn a_placeholder_names_a_page_or_a_ticket() {
        assert_eq!(parse(None, Some("<p>x</p>")), Ok(Source::Inline("<p>x</p>".into())));
        assert_eq!(parse(Some(" design-49 "), None), Ok(Source::Ref("DESIGN-49".into())));
        // An empty ref is none; a fence's page may be empty and is still one.
        assert_eq!(parse(Some(""), Some("")), Ok(Source::Inline(String::new())));
        for (r, s, why) in [
            (Some("A-1"), Some("<p>"), "not both"),
            (None, None, "name a page"),
            (Some("DESIGN"), None, "no ticket id"),
            (Some("A-1#x"), None, "no ticket id"),
        ] {
            let err = parse(r, s).unwrap_err();
            assert!(err.starts_with("m-mockup: ") && err.contains(why), "{r:?}: {err}");
        }
    }

    /// Inline: drawn at once, at the fence's widths or the defaults.
    #[test]
    fn an_inline_page_is_drawn_at_its_widths() {
        let s = parse(None, Some(PAGE)).unwrap();
        let (page, declared) = resolve(&s, None).unwrap();
        assert_eq!((page.as_str(), declared), (PAGE, None));
        let d = decl_for(Some("390,1440"), None);
        assert_eq!((d.mode, d.devices), (Mode::Mockup, vec![Device { w: 390, h: 844 }, Device { w: 1440, h: 900 }]));
        assert_eq!(decl_for(None, None).devices, Decl::default().devices);
    }

    /// By ref: the ticket's whole-document body, at its own declared widths
    /// unless the element names some; frontmatter split off first.
    #[test]
    fn a_referenced_page_is_its_tickets_body_at_its_declared_widths() {
        let s = parse(Some("DESIGN-49"), None).unwrap();
        assert_eq!(resolve(&s, None), Err(State::Loading));
        let t = ticket("DESIGN-49", PAGE);
        let (page, declared) = resolve(&s, Some(&Fetched::Ticket(Box::new(t)))).unwrap();
        assert_eq!(page, PAGE);
        assert_eq!(decl_for(None, declared.as_ref()).devices, vec![Device { w: 375, h: 844 }, Device { w: 1280, h: 800 }]);
        assert_eq!(decl_for(Some("390"), declared.as_ref()).devices, vec![Device { w: 390, h: 844 }]);
        let fm = format!("---\nview: mockup\nwidths: 768\n---\n{PAGE}");
        let (page, declared) = page_of(&fm).unwrap();
        assert_eq!(page, PAGE);
        assert_eq!(decl_for(None, Some(&declared)).devices, vec![Device { w: 768, h: 1024 }]);
    }

    /// A ticket that cannot be read, or is no page, is its named line — and
    /// the line carries nothing of the ticket but the id the body wrote.
    #[test]
    fn a_missing_hidden_or_non_page_ref_is_a_named_notice() {
        let s = parse(Some("SECRET-3"), None).unwrap();
        assert_eq!(resolve(&s, Some(&Fetched::NoTicket)), Err(State::NoTicket));
        assert_eq!(resolve(&s, Some(&Fetched::Unreadable)), Err(State::Unreadable));
        let prose = ticket("SECRET-3", "# Layoffs\n\nnot a page");
        assert_eq!(resolve(&s, Some(&Fetched::Ticket(Box::new(prose)))), Err(State::NotPage));
        let line = |st: State| state_text(Some(&s), &st).unwrap();
        assert_eq!(line(State::NoTicket), "SECRET-3: no ticket you can read");
        assert_eq!(line(State::Unreadable), "you cannot read SECRET-3");
        assert!(line(State::NotPage).starts_with("SECRET-3 is no whole HTML page"));
        for st in [State::NoTicket, State::Unreadable, State::NotPage, State::Loading] {
            let l = line(st);
            assert!(!l.contains("Layoffs") && !l.contains("secret title"), "{l}");
        }
        assert_eq!(state_text(Some(&s), &State::Shown), None);
        let bad = parse(None, None).unwrap_err();
        assert_eq!(state_text(None, &State::Invalid(bad.clone())), Some(bad));
    }

    /// The body hears only a height; a body draws a bounded number.
    #[test]
    fn the_body_hears_only_a_height_and_draws_a_bounded_number() {
        let reply = serde_json::to_value(mocked_reply(3, 512.2)).unwrap();
        assert_eq!(reply, serde_json::json!({ "mock": 3, "height": 513 }));
        assert_eq!(mocked_reply(1, f64::NAN).height, 0);
        assert_eq!(mocked_reply(1, 1e9).height, 50_000);
        assert_eq!(over_cap(MAX_PER_FRAME - 1), None);
        assert_eq!(over_cap(MAX_PER_FRAME).as_deref(), Some("m-mockup: a body draws at most 16 mockups"));
    }
}
