//! COMP-14: **`<m-cite>`** — a live window into a section of another
//! ticket, cited and not copied.
//!
//! ```html
//! <m-cite ref="AUTH-10" field="findings"></m-cite>   <!-- [data-field="findings"] -->
//! <m-cite ref="AUTH-10#the-table"></m-cite>          <!-- the element id="the-table" -->
//! ```
//!
//! **The design: the host draws the window, beside the citing frame — never
//! inside it.** A cited section's HTML (and every script in it) must not run
//! in the citing frame: that would hand one shortcode's body the other's
//! reach (IDENTITY-36), and the citer's script could read the cited text. So
//! the SDK's `<m-cite>` element is only a **placeholder**: it reserves room
//! in the citing body and tells the host where it is — `menger:cite { cite,
//! ref, field, rect }` (and `menger:uncite { cite }` when it leaves). The
//! host then, in the app's own DOM:
//!
//! 1. asks the server for the cited ticket **over the viewer's own socket**
//!    (`tickets.get`) — the viewer's reach decides, never the citing frame's
//!    scope ([`classify`]): a ticket, `not_found` (no such ticket, or — under
//!    login — one outside the viewer's reach: the server never says which),
//!    or a refusal;
//! 2. lays a window (`div.mcite`) over the placeholder, clamped to the citing
//!    frame's box ([`place`]): a header — the cited ticket's id, live status
//!    and title, and the section's label, a link that opens the ticket at the
//!    section (`#/ticket/AUTH-10?field=findings`, [`route`]) — over a view;
//! 3. renders the cited ticket's **whole body** in a frame of its own in that
//!    view — the same sandbox, CSP and SDK as every frame (IDENTITY-28
//!    unchanged) — stamped as the **cited** ticket's ([`stamps`]): its reads
//!    held to the cited shortcode, never the citer's; no `data-ticket`, so the
//!    host answers no write from it (inputs are display only — answered in
//!    their own ticket); its init block's `cite` tells its SDK which section
//!    to show: the SDK hides everything but that element and its ancestors,
//!    and reports the section's top and bottom (`menger:size { top, found }`),
//!    so the host shows exactly the section, at its natural height up to
//!    [`MAX_VIEW`] px, then scrolling inside the window;
//! 4. answers the citing frame **only the window's height** —
//!    `menger:cited { cite, height }` ([`cited_reply`]) — so the placeholder
//!    takes the room the window needs. Nothing else of the cited ticket
//!    reaches the citing frame: no title, status, body or state.
//!
//! **Live.** Every `watch` event the session hears passes [`heard`]: an
//! `amended` of a cited ticket re-renders its windows (no reload, no flash —
//! the frame's own re-render), a status change repaints the header, and a
//! ticket created under a cited id fetches it.
//!
//! **Failure is a named state** ([`state_text`]): an unknown or unreadable
//! ticket (`AUTH-10: no ticket you can read`), a refusal (`you cannot read
//! AUTH-10`), a missing section (`AUTH-10 has no section `findings``), a
//! malformed cite, and a cite nested deeper than [`MAX_DEPTH`]. A state that
//! is not a shown section carries nothing of the ticket but the id the
//! citer wrote.
//!
//! **Limits.** The citing frame learns the window's height (it lays out
//! around it) — so a script there can tell roughly how tall a section the
//! viewer can read is, and whether the window shows one; never its text.
//! A body opens at most [`MAX_PER_FRAME`] windows.
//! Cited windows nest [`MAX_DEPTH`] deep (a cycle stops there). The whole
//! cited body loads in the window's frame — its scripts and diagrams run,
//! hidden but for the section. Heading anchors are not addressed: md.rs
//! gives headings no ids; an id the body writes (`<h2 id=…>`) is one.
//!
//! What decides — parsing a cite, classifying the answer, the placement, the
//! stamps, the reply, the states — is pure and pinned by native tests; the
//! rest is DOM.

use std::cell::RefCell;
use std::collections::HashMap;

use menger_tickets_api::{Ticket, TicketsEvent};
use serde::Serialize;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::inspect::{section_name_ok, Anchor};
use crate::{bridge, dom, frame, md, session};

/// The tallest a window's view of a section grows (CSS px); a taller
/// section scrolls inside it.
pub(crate) const MAX_VIEW: f64 = 480.0;

/// How deep windows nest: a citing body (depth 0) shows a window (1), whose
/// section may show one more (2); a cite inside that draws its state. A
/// cycle (A cites B cites A) stops here too.
pub(crate) const MAX_DEPTH: u32 = 2;

/// The most windows one frame's body may open: each is a read over the
/// viewer's socket and a frame, so a body cannot have the host fetch
/// without end. A cite past it draws its line ([`over_cap`]).
pub(crate) const MAX_PER_FRAME: usize = 64;

/// The line of a cite past [`MAX_PER_FRAME`] in one body, when `open` are
/// already open there.
pub(crate) fn over_cap(open: usize) -> Option<String> {
    (open >= MAX_PER_FRAME).then(|| format!("m-cite: a body opens at most {MAX_PER_FRAME} windows"))
}

/// The class a window wears, in the citing frame's container.
pub(crate) const WINDOW_CLASS: &str = "mcite";

/// Which element of the cited body a cite shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    /// The first element carrying `data-field="<name>"`.
    Field(String),
    /// The element with that id.
    Id(String),
}

impl Section {
    /// How the header names it: `findings`, `#the-table`.
    pub(crate) fn label(&self) -> String {
        match self {
            Section::Field(n) => n.clone(),
            Section::Id(n) => format!("#{n}"),
        }
    }

    /// The route anchor that opens the ticket at it.
    pub(crate) fn anchor(&self) -> Anchor {
        match self {
            Section::Field(n) => Anchor::Field(n.clone()),
            Section::Id(n) => Anchor::Id(n.clone()),
        }
    }
}

/// A cite: the ticket, and the section of its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cite {
    pub(crate) ticket: String,
    pub(crate) section: Section,
}

/// A ticket id as a cite names one: `EPIC-N`.
fn ticket_ok(id: &str) -> bool {
    let Some((epic, n)) = id.rsplit_once('-') else { return false };
    !epic.is_empty()
        && epic.len() <= 64
        && epic.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !n.is_empty()
        && n.len() <= 10
        && n.chars().all(|c| c.is_ascii_digit())
}

/// `<m-cite ref field>` as written, as a [`Cite`] — or why not, which the
/// window shows: `ref` is a ticket id, with the section either `field`
/// (a `data-field` name) or after a `#` in `ref` (an element id), not both.
pub(crate) fn parse(reference: &str, field: Option<&str>) -> Result<Cite, String> {
    let reference = reference.trim();
    let field = field.map(str::trim).filter(|f| !f.is_empty());
    let (id, hash) = match reference.split_once('#') {
        Some((id, h)) => (id.trim(), Some(h.trim())),
        None => (reference, None),
    };
    if id.is_empty() {
        return Err("m-cite: name the ticket — ref=\"AUTH-10\"".into());
    }
    let id = id.to_ascii_uppercase();
    if !ticket_ok(&id) {
        return Err(format!("m-cite: ref=\"{}\" is no ticket id (EPIC-N)", clip(reference)));
    }
    let section = match (field, hash) {
        (Some(_), Some(_)) => return Err(format!("m-cite: {id} — give field=\"…\" or ref=\"{id}#id\", not both")),
        (None, None) | (None, Some("")) => {
            return Err(format!("m-cite: {id} — name a section: field=\"…\" or ref=\"{id}#id\""))
        }
        (Some(f), None) if section_name_ok(f) => Section::Field(f.to_string()),
        (None, Some(h)) if section_name_ok(h) => Section::Id(h.to_string()),
        (Some(bad), None) | (None, Some(bad)) => {
            return Err(format!(
                "m-cite: {id} — \"{}\" is no section name (letters, digits, _ - . : only)",
                clip(bad)
            ))
        }
    };
    Ok(Cite { ticket: id, section })
}

/// At most 64 characters of what a body wrote, for a message.
fn clip(s: &str) -> String {
    s.chars().take(64).collect()
}

/// The route the window's header opens: the cited ticket at the section.
pub(crate) fn route(cite: &Cite) -> String {
    let anchor = cite.section.anchor();
    let (key, value) = anchor.pair();
    format!("#/ticket/{}?{key}={value}", cite.ticket)
}

/// What the server answered the viewer's `get`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Fetched {
    Ticket(Box<Ticket>),
    /// `not_found`: no such ticket — or, under login, one outside the
    /// viewer's reach (the server answers both alike, and says nothing of
    /// whether it exists).
    NoTicket,
    /// A refusal (an in-band error, a refused call, no connection): the
    /// reason is not kept — it is not the citer's to show.
    Unreadable,
}

/// The viewer's `get` answer as [`Fetched`].
pub(crate) fn classify(answer: Result<Vec<TicketsEvent>, String>) -> Fetched {
    let Ok(events) = answer else { return Fetched::Unreadable };
    for ev in events {
        match ev {
            TicketsEvent::Ticket { ticket } => return Fetched::Ticket(Box::new(ticket)),
            TicketsEvent::NotFound { .. } => return Fetched::NoTicket,
            TicketsEvent::Error { .. } => return Fetched::Unreadable,
            _ => {}
        }
    }
    Fetched::Unreadable
}

/// Where a window stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum State {
    /// Asked; nothing to show yet.
    Loading,
    /// The section, drawn.
    Shown,
    NoTicket,
    Unreadable,
    /// The ticket's body has no such section (once its frame settled).
    NoSection,
    /// The tag does not say a cite ([`parse`]'s reason).
    Invalid(String),
    /// Nested deeper than [`MAX_DEPTH`].
    TooDeep,
}

/// The line a window shows instead of a section — `None` while it shows
/// one. `reference` is what the citer wrote; nothing here comes from the
/// cited ticket.
pub(crate) fn state_text(reference: &str, cite: Option<&Cite>, state: &State) -> Option<String> {
    let who = cite.map_or_else(|| clip(reference.trim()), |c| c.ticket.clone());
    match state {
        State::Shown => None,
        State::Loading => Some(format!("{who}…")),
        State::NoTicket => Some(format!("{who}: no ticket you can read")),
        State::Unreadable => Some(format!("you cannot read {who}")),
        State::NoSection => Some(match cite.map(|c| &c.section) {
            Some(Section::Field(n)) => format!("{who} has no section `{n}`"),
            Some(Section::Id(n)) => format!("{who} has no element `#{n}`"),
            None => format!("{who} has no such section"),
        }),
        State::Invalid(why) => Some(why.clone()),
        State::TooDeep => Some(format!("{who}: cites nest {MAX_DEPTH} deep at most — open it to read on")),
    }
}

/// The window's header: the ticket's chip (id, status, title) and the
/// section's label, linking to [`route`]. Only a ticket the viewer read
/// fills `status` and `title`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Head {
    pub(crate) id: String,
    pub(crate) status: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) section: String,
    pub(crate) route: Option<String>,
}

/// The header for `cite` (or, malformed, what was written) given what the
/// viewer read.
pub(crate) fn head(reference: &str, cite: Option<&Cite>, fetched: Option<&Fetched>) -> Head {
    let Some(cite) = cite else {
        return Head { id: clip(reference.trim()), status: None, title: None, section: String::new(), route: None };
    };
    let (status, title) = match fetched {
        Some(Fetched::Ticket(t)) => (Some(t.status.as_str().to_string()), Some(t.title.clone())),
        _ => (None, None),
    };
    Head { id: cite.ticket.clone(), status, title, section: cite.section.label(), route: Some(route(cite)) }
}

/// What a cited window's frame is stamped with — all of it the **cited**
/// ticket's, nothing the citer's (it is not an argument).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stamps {
    /// `<iframe data-ticket>` — none: the host answers no write from a
    /// window (its inputs are display only).
    pub(crate) data_ticket: Option<String>,
    /// `<iframe data-scope>` — the cited ticket's shortcode: its reads are
    /// the cited body's, as in its own ticket (IDENTITY-36).
    pub(crate) scope: Option<String>,
    /// `menger.input.ticket` in its init block — whose inputs it shows.
    pub(crate) init_ticket: Option<String>,
    /// `<iframe data-cite-depth>`.
    pub(crate) depth: u32,
}

/// The stamps of the frame that shows `cited` in a window opened at `depth`.
pub(crate) fn stamps(cited: &str, depth: u32) -> Stamps {
    Stamps {
        data_ticket: None,
        scope: bridge::scope_of(Some(cited)),
        init_ticket: Some(cited.to_string()),
        depth,
    }
}

/// A rectangle in a frame's document (CSS px), as the SDK reports one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) w: f64,
    pub(crate) h: f64,
}

/// Where a window goes in the citing frame's container.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Place {
    pub(crate) top: f64,
    pub(crate) left: f64,
    pub(crate) width: f64,
    /// The placeholder is laid out (not hidden, not zero-wide).
    pub(crate) shown: bool,
}

/// The window over the placeholder `rect` of a frame at (`frame_left`,
/// `frame_top`) in its container, `frame_w` × `frame_h` — clamped to the
/// frame's box, so a citing body can only place a window over itself.
pub(crate) fn place(rect: Rect, frame_left: f64, frame_top: f64, frame_w: f64, frame_h: f64) -> Place {
    let fin = |v: f64| if v.is_finite() { v } else { 0.0 };
    let (fw, fh) = (fin(frame_w).max(0.0), fin(frame_h).max(0.0));
    let x = fin(rect.x).clamp(0.0, fw);
    let y = fin(rect.y).clamp(0.0, fh);
    let width = fin(rect.w).clamp(0.0, fw - x);
    Place { top: fin(frame_top) + y, left: fin(frame_left) + x, width, shown: width >= 1.0 }
}

/// The view's height for a section from `top` to `bottom` of its frame's
/// document: its natural height, at most [`MAX_VIEW`].
pub(crate) fn view_height(top: f64, bottom: f64) -> f64 {
    let h = if top.is_finite() && bottom.is_finite() { bottom - top } else { 0.0 };
    h.clamp(0.0, MAX_VIEW).ceil()
}

/// `menger:cited`'s payload past its type and channel — the one thing a
/// citing frame is told: how tall its window is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct CitedReply {
    pub(crate) cite: u32,
    pub(crate) height: u32,
}

/// The reply for cite `n`, its window `height` px tall.
pub(crate) fn cited_reply(n: u32, height: f64) -> CitedReply {
    let h = if height.is_finite() { height.clamp(0.0, 50_000.0).ceil() as u32 } else { 0 };
    CitedReply { cite: n, height: h }
}

// -- the DOM half -------------------------------------------------------------

/// One window: the cite `n` of the citing frame on `channel`.
struct Entry {
    citer: web_sys::HtmlIFrameElement,
    channel: String,
    n: u32,
    reference: String,
    field: Option<String>,
    cite: Result<Cite, String>,
    depth: u32,
    overlay: web_sys::HtmlElement,
    rect: Rect,
    state: State,
    fetched: Option<Fetched>,
    /// The last height told to the citer.
    told: Option<u32>,
    /// Bumped by every fetch: only the newest answer lands.
    asked: u32,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<String, Entry>> = RefCell::new(HashMap::new());
}

fn key(channel: &str, n: u32) -> String {
    format!("{channel}:{n}")
}

fn doc() -> Option<web_sys::Document> {
    dom::window().document()
}

fn el(tag: &str, class: &str) -> Option<web_sys::HtmlElement> {
    let e = doc()?.create_element(tag).ok()?;
    let _ = e.set_attribute("class", class);
    e.dyn_into::<web_sys::HtmlElement>().ok()
}

fn child(parent: &web_sys::Element, selector: &str) -> Option<web_sys::HtmlElement> {
    parent.query_selector(selector).ok().flatten().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
}

fn style(e: &web_sys::HtmlElement, prop: &str, value: &str) {
    let _ = e.style().set_property(prop, value);
}

/// A window's skeleton: its header, its state line, its view (an `.mframe`
/// the section's frame loads into).
fn build(key: &str, channel: &str, n: u32, depth: u32) -> Option<web_sys::HtmlElement> {
    let w = el("div", WINDOW_CLASS)?;
    let _ = w.set_attribute("data-for", channel);
    let _ = w.set_attribute("data-cite", &n.to_string());
    let _ = w.set_attribute("data-key", key);
    let _ = w.set_attribute("data-depth", &depth.to_string());
    let _ = w.set_attribute("role", "figure");
    let head = el("a", "mcite-head")?;
    let state = el("p", "mcite-state")?;
    let view = el("div", "mcite-view")?;
    let inner = el("div", "mframe")?;
    let _ = view.append_child(&inner);
    let _ = w.append_child(&head);
    let _ = w.append_child(&state);
    let _ = w.append_child(&view);
    Some(w)
}

/// Paint the header and the state line from the entry.
fn paint(e: &Entry) {
    let cite = e.cite.as_ref().ok();
    let h = head(&e.reference, cite, e.fetched.as_ref());
    if let Some(a) = child(&e.overlay, ":scope > .mcite-head") {
        a.set_text_content(None);
        match &h.route {
            Some(r) => {
                let _ = a.set_attribute("href", r);
            }
            None => {
                let _ = a.remove_attribute("href");
            }
        }
        let push = |class: &str, text: &str| {
            if let Some(s) = el("span", class) {
                s.set_text_content(Some(text));
                let _ = a.append_child(&s);
            }
        };
        push("mcite-id", &h.id);
        if let Some(st) = &h.status {
            push(&crate::st_class(st), st);
        }
        if let Some(t) = &h.title {
            push("mcite-title", t);
        }
        if !h.section.is_empty() {
            push("mcite-sec", &format!("§ {}", h.section));
        }
        let label = match (&h.title, h.section.is_empty()) {
            (Some(t), false) => format!("{} — {t}, section {}", h.id, h.section),
            (None, false) => format!("{}, section {}", h.id, h.section),
            _ => h.id.clone(),
        };
        let _ = a.set_attribute("aria-label", &format!("cited from {label}"));
    }
    let text = state_text(&e.reference, cite, &e.state);
    if let Some(p) = child(&e.overlay, ":scope > .mcite-state") {
        p.set_text_content(text.as_deref());
        p.set_hidden(text.is_none());
    }
    if let Some(v) = child(&e.overlay, ":scope > .mcite-view") {
        // A section shows only once drawn; a failure shows only its line.
        v.set_hidden(!matches!(e.state, State::Shown | State::Loading) || e.fetched.as_ref().is_some_and(|f| !matches!(f, Fetched::Ticket(_))));
    }
    let _ = e.overlay.set_attribute("data-state", match e.state {
        State::Shown => "shown",
        State::Loading => "loading",
        _ => "failed",
    });
}

/// Lay the window over its placeholder.
fn position(e: &Entry) {
    let f = &e.citer;
    let p = place(
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

/// Tell the citer how tall its window is, when that changed.
fn tell(e: &mut Entry) {
    let h = e.overlay.get_bounding_client_rect().height();
    let reply = cited_reply(e.n, if e.overlay.hidden() { 0.0 } else { h });
    if e.told == Some(reply.height) {
        return;
    }
    e.told = Some(reply.height);
    frame::post_cited(&e.citer, &e.channel, &reply);
}

fn with_entry(key: &str, f: impl FnOnce(&mut Entry)) {
    WINDOWS.with(|w| {
        if let Some(e) = w.borrow_mut().get_mut(key) {
            f(e);
        }
    });
}

/// Windows whose citing frame left the document go with it.
fn prune() {
    WINDOWS.with(|w| {
        w.borrow_mut().retain(|_, e| {
            let alive = e.citer.is_connected() && e.overlay.is_connected();
            if !alive {
                e.overlay.remove();
            }
            alive
        })
    });
}

/// `menger:cite` from the frame `citer` on `channel`: its cite `n` says
/// `reference` / `field` and sits at `rect`. A new or changed cite is
/// fetched; a moved one is laid again.
pub(crate) fn asked(
    citer: web_sys::HtmlIFrameElement,
    channel: String,
    n: u32,
    reference: String,
    field: Option<String>,
    rect: Rect,
) {
    prune();
    let k = key(&channel, n);
    let same = WINDOWS.with(|w| {
        w.borrow().get(&k).map(|e| e.reference == reference && e.field == field)
    });
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
            if let Some(old) = WINDOWS.with(|w| w.borrow_mut().remove(&k)) {
                old.overlay.remove();
            }
        }
        None => {}
    }
    let Some(container) = citer.parent_element() else { return };
    let depth = citer.get_attribute("data-cite-depth").and_then(|d| d.parse::<u32>().ok()).unwrap_or(0) + 1;
    let Some(overlay) = build(&k, &channel, n, depth) else { return };
    if citer.get_attribute("data-state").as_deref() == Some("pending") {
        let _ = overlay.set_attribute("data-pending", "");
    }
    let _ = container.append_child(&overlay);
    let open = WINDOWS.with(|w| w.borrow().values().filter(|e| e.channel == channel).count());
    let cite = match over_cap(open) {
        Some(why) => Err(why),
        None => parse(&reference, field.as_deref()),
    };
    let state = match &cite {
        Err(why) => State::Invalid(why.clone()),
        Ok(_) if depth > MAX_DEPTH => State::TooDeep,
        Ok(_) => State::Loading,
    };
    let fetch = matches!(state, State::Loading);
    let mut e = Entry {
        citer,
        channel,
        n,
        reference,
        field,
        cite,
        depth,
        overlay,
        rect,
        state,
        fetched: None,
        told: None,
        asked: 0,
    };
    paint(&e);
    position(&e);
    tell(&mut e);
    WINDOWS.with(|w| w.borrow_mut().insert(k.clone(), e));
    if fetch {
        fetch_into(k);
    }
}

/// Ask the server, over the viewer's own socket, for the window's ticket.
fn fetch_into(k: String) {
    let Some((id, n)) = WINDOWS.with(|w| {
        let mut w = w.borrow_mut();
        let e = w.get_mut(&k)?;
        e.asked = e.asked.wrapping_add(1);
        Some((e.cite.as_ref().ok()?.ticket.clone(), e.asked))
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
        let fetched = classify(answer);
        let current = WINDOWS.with(|w| w.borrow().get(&k).is_some_and(|e| e.asked == n));
        if current {
            land(&k, fetched);
        }
    });
}

/// What the viewer read lands in the window: a ticket's body loads into its
/// view; anything else is its state.
fn land(k: &str, fetched: Fetched) {
    with_entry(k, |e| {
        e.state = match &fetched {
            Fetched::Ticket(_) => State::Loading,
            Fetched::NoTicket => State::NoTicket,
            Fetched::Unreadable => State::Unreadable,
        };
        if let (Fetched::Ticket(t), Ok(cite)) = (&fetched, &e.cite) {
            show(e, cite.clone(), t);
        } else if let Some(inner) = child(&e.overlay, ".mcite-view > .mframe") {
            // Nothing of a ticket stays once it cannot be read.
            inner.set_inner_html("");
        }
        e.fetched = Some(fetched);
        paint(e);
        tell(e);
    });
}

/// Load `ticket`'s body into the window's view, as a frame of the cited
/// ticket's own ([`stamps`]) told to show `cite.section`.
fn show(e: &Entry, cite: Cite, ticket: &Ticket) {
    let Some(inner) = child(&e.overlay, ".mcite-view > .mframe") else { return };
    let src = md::split_frontmatter(&ticket.body).1;
    let html = md::render_body_framed(src, &|_| None).html;
    let s = stamps(&ticket.id, e.depth);
    frame::load_cited(&inner, &html, &s, &cite.section, &key(&e.channel, e.n));
}

/// `menger:size` from a window's frame (`data-cite-key` = `k`): the section
/// runs from `top` to `bottom` of its document; `found` whether the section
/// is there, `settled` whether the frame is done drawing.
pub(crate) fn sized(frame: &web_sys::HtmlIFrameElement, k: &str, top: f64, bottom: f64, found: bool, settled: bool) {
    let top = if top.is_finite() { top.max(0.0) } else { 0.0 };
    let el: &web_sys::HtmlElement = frame.unchecked_ref();
    style(el, "margin-top", &format!("-{top}px"));
    with_entry(k, |e| {
        if !matches!(e.fetched, Some(Fetched::Ticket(_))) {
            return;
        }
        if let Some(v) = child(&e.overlay, ":scope > .mcite-view") {
            style(&v, "height", &format!("{}px", view_height(top, bottom)));
        }
        let live = frame.get_attribute("data-state").as_deref() != Some("pending");
        let next = if found {
            State::Shown
        } else if settled {
            State::NoSection
        } else {
            e.state.clone()
        };
        // A re-render loading beside the live frame does not repaint the
        // window until it takes over.
        if live || matches!(e.state, State::Loading) {
            e.state = next;
        }
        paint(e);
        tell(e);
    });
}

/// `menger:uncite`: the placeholder left the citing body.
pub(crate) fn dropped(channel: &str, n: u32) {
    if let Some(e) = WINDOWS.with(|w| w.borrow_mut().remove(&key(channel, n))) {
        e.overlay.remove();
    }
}

/// A frame took over in its container (`frame::promote`): the windows of
/// the frame it replaced go, its own show and are laid again.
pub(crate) fn promoted(frame: &web_sys::HtmlIFrameElement) {
    let Some(channel) = frame.get_attribute("data-channel") else { return };
    let Some(container) = frame.parent_element() else { return };
    let Ok(list) = container.query_selector_all(&format!(":scope > .{WINDOW_CLASS}")) else { return };
    for i in 0..list.length() {
        let Some(w) = list.get(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
        if w.get_attribute("data-for").as_deref() == Some(channel.as_str()) {
            let _ = w.remove_attribute("data-pending");
        } else {
            if let Some(k) = w.get_attribute("data-key") {
                WINDOWS.with(|m| m.borrow_mut().remove(&k));
            }
            w.remove();
        }
    }
    WINDOWS.with(|m| {
        for e in m.borrow_mut().values_mut().filter(|e| e.channel == channel) {
            position(e);
            tell(e);
        }
    });
}

/// A citing frame resized: its windows are laid again.
pub(crate) fn relayout(channel: &str) {
    WINDOWS.with(|m| {
        for e in m.borrow_mut().values_mut().filter(|e| e.channel == channel) {
            position(e);
        }
    });
}

/// Every `watch` event the session hears: a cited ticket amended re-renders
/// its windows; its status or kind moved repaints their headers; a ticket
/// created under a cited id that was none is fetched.
pub(crate) fn heard(ev: &TicketsEvent) {
    let (ticket, created) = match ev {
        TicketsEvent::Amended { ticket, .. }
        | TicketsEvent::StatusChanged { ticket, .. }
        | TicketsEvent::KindChanged { ticket, .. } => (ticket, false),
        TicketsEvent::Created { ticket } => (ticket, true),
        _ => return,
    };
    let body_changed = matches!(ev, TicketsEvent::Amended { .. });
    let mut refetch = Vec::new();
    prune();
    WINDOWS.with(|m| {
        for (k, e) in m.borrow_mut().iter_mut() {
            let Ok(cite) = e.cite.clone() else { continue };
            if cite.ticket != ticket.id {
                continue;
            }
            match &e.fetched {
                Some(Fetched::Ticket(_)) => {
                    e.fetched = Some(Fetched::Ticket(Box::new(ticket.clone())));
                    if body_changed {
                        show(e, cite, ticket);
                    }
                    paint(e);
                    tell(e);
                }
                _ if created || e.fetched.is_some() => refetch.push(k.clone()),
                _ => {}
            }
        }
    });
    for k in refetch {
        fetch_into(k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::{Kind, Status};

    fn ticket(id: &str, title: &str, body: &str) -> Ticket {
        let (epic, seq) = id.rsplit_once('-').unwrap();
        Ticket {
            hidden_blocked_by: 0,
            hidden_unlocks: 0,
            parent_hidden: false,
            labels: Vec::new(),
            id: menger_tickets_api::TicketId::new(id),
            epic: menger_tickets_api::Code::stored(epic), shortcode: menger_tickets_api::ShortcodeId::new(epic), address: String::new(),
            seq: seq.parse().unwrap(),
            title: title.into(),
            status: Status::Ready,
            kind: Kind::Build,
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

    /// The two forms: `field`, and `#id` in `ref`; ids fold to uppercase.
    #[test]
    fn a_cite_names_a_ticket_and_one_section() {
        assert_eq!(
            parse("AUTH-10", Some("findings")),
            Ok(Cite { ticket: "AUTH-10".into(), section: Section::Field("findings".into()) })
        );
        assert_eq!(
            parse(" auth-10#the-table ", None),
            Ok(Cite { ticket: "AUTH-10".into(), section: Section::Id("the-table".into()) })
        );
        assert_eq!(parse("AUTH-10#x", Some("")).map(|c| c.section), Ok(Section::Id("x".into())));
        for (r, f, why) in [
            ("", Some("f"), "name the ticket"),
            ("AUTH", Some("f"), "no ticket id"),
            ("AUTH-x", Some("f"), "no ticket id"),
            ("AUTH-10/x", Some("f"), "no ticket id"),
            ("AUTH-10", None, "name a section"),
            ("AUTH-10#", None, "name a section"),
            ("AUTH-10#x", Some("f"), "not both"),
            ("AUTH-10", Some("a\"]"), "no section name"),
            ("AUTH-10#a b", None, "no section name"),
        ] {
            let err = parse(r, f).unwrap_err();
            assert!(err.starts_with("m-cite: ") && err.contains(why), "{r:?} {f:?}: {err}");
        }
    }

    /// The header opens the cited ticket at the section — a route the
    /// app's allow-list takes and reads the anchor back off.
    #[test]
    fn the_header_opens_the_ticket_at_the_section() {
        let c = parse("AUTH-10", Some("findings")).unwrap();
        assert_eq!(route(&c), "#/ticket/AUTH-10?field=findings");
        assert_eq!(crate::inspect::app_route(&route(&c)).as_deref(), Some("#/ticket/AUTH-10?field=findings"));
        assert_eq!(crate::inspect::anchor(&route(&c)), Some(Anchor::Field("findings".into())));
        let c = parse("AUTH-10#t", None).unwrap();
        assert_eq!(route(&c), "#/ticket/AUTH-10?anchor=t");
        assert_eq!(crate::inspect::anchor(&route(&c)), Some(Anchor::Id("t".into())));
    }

    /// Rendering a field: a ticket read fills the header — id, live status,
    /// title, the section's label — and the window shows the section.
    #[test]
    fn a_read_ticket_fills_the_header() {
        let c = parse("AUTH-10", Some("findings")).unwrap();
        let t = ticket("AUTH-10", "The review", "<m-field data-field=\"findings\">x</m-field>");
        let fetched = classify(Ok(vec![TicketsEvent::Ticket { ticket: t.clone() }]));
        assert_eq!(fetched, Fetched::Ticket(Box::new(t)));
        let h = head("AUTH-10", Some(&c), Some(&fetched));
        assert_eq!(
            h,
            Head {
                id: "AUTH-10".into(),
                status: Some("Ready".into()),
                title: Some("The review".into()),
                section: "findings".into(),
                route: Some("#/ticket/AUTH-10?field=findings".into()),
            }
        );
        assert_eq!(state_text("AUTH-10", Some(&c), &State::Shown), None);
    }

    /// Missing ticket, missing section, unreadable ticket, a malformed tag
    /// and a cite too deep: each its own named line, never a blank.
    #[test]
    fn every_failure_is_a_named_state() {
        let c = parse("AUTH-10", Some("findings")).unwrap();
        let line = |s: State| state_text("AUTH-10", Some(&c), &s).unwrap();
        assert_eq!(classify(Ok(vec![TicketsEvent::NotFound { id: "AUTH-10".parse().unwrap() }])), Fetched::NoTicket);
        assert_eq!(line(State::NoTicket), "AUTH-10: no ticket you can read");
        assert_eq!(line(State::NoSection), "AUTH-10 has no section `findings`");
        assert_eq!(line(State::Unreadable), "you cannot read AUTH-10");
        assert_eq!(line(State::TooDeep), "AUTH-10: cites nest 2 deep at most — open it to read on");
        assert_eq!(line(State::Loading), "AUTH-10…");
        let id = parse("AUTH-10#t", None).unwrap();
        assert_eq!(state_text("AUTH-10#t", Some(&id), &State::NoSection).unwrap(), "AUTH-10 has no element `#t`");
        let bad = parse("AUTH", Some("f")).unwrap_err();
        assert_eq!(state_text("AUTH", None, &State::Invalid(bad.clone())), Some(bad));
    }

    /// Acceptance 3: an unreadable ticket leaks nothing — the refusal's
    /// words are dropped, and neither the header nor the line carries a
    /// title, a status or a body; a refused call, no connection and a
    /// stream with no answer are all unreadable.
    #[test]
    fn an_unreadable_ticket_leaks_nothing() {
        let c = parse("SECRET-3", Some("plan")).unwrap();
        for answer in [
            Ok(vec![TicketsEvent::Error { message: "forbidden: SECRET-3 'Layoffs' is Coding".into() }]),
            Err("forbidden: grant tickets.get not held on SECRET".to_string()),
            Err("not connected".to_string()),
            Ok(vec![]),
        ] {
            let fetched = classify(answer);
            assert_eq!(fetched, Fetched::Unreadable);
            let h = head("SECRET-3", Some(&c), Some(&fetched));
            assert_eq!((h.status, h.title), (None, None));
            let line = state_text("SECRET-3", Some(&c), &State::Unreadable).unwrap();
            assert_eq!(line, "you cannot read SECRET-3");
            for secret in ["Layoffs", "Coding", "forbidden", "grant"] {
                assert!(!format!("{line} {} {}", h.id, h.section).contains(secret), "{secret}");
            }
        }
        // Not found (or outside the viewer's reach): the same — nothing but the id.
        let h = head("SECRET-3", Some(&c), Some(&Fetched::NoTicket));
        assert_eq!((h.status, h.title), (None, None));
        // And whatever the window shows, the citing frame hears only a height.
        let reply = serde_json::to_value(cited_reply(4, 31.2)).unwrap();
        assert_eq!(reply, serde_json::json!({ "cite": 4, "height": 32 }));
    }

    /// Acceptance 4: the window's frame is the **cited** ticket's — its reads
    /// held to the cited shortcode (a `get` of the citer's ticket is
    /// `not_found`, a `list` is `under` the cited shortcode), no
    /// `data-ticket` (no write is answered from it), its init naming the
    /// cited ticket — and the citer's own frame gains no read: `cite` is no
    /// method it may call.
    #[test]
    fn the_window_reads_as_the_cited_ticket_and_the_citer_reads_no_more() {
        let s = stamps("AUTH-10", 1);
        assert_eq!(
            s,
            Stamps { data_ticket: None, scope: Some("AUTH".into()), init_ticket: Some("AUTH-10".into()), depth: 1 }
        );
        let reach = bridge::Reach::only("AUTH");
        // ACT-6: a `get` is asked (an old id may name a ticket that moved
        // into the reach), and its answer held to the cited shortcode.
        let call = bridge::parse_call("get", r#"{"id":"MENGER-2"}"#).unwrap();
        assert!(matches!(bridge::scope_call(call, Some(&reach)).unwrap(), bridge::Scoped::Call(bridge::Call::Get { .. })));
        let held = bridge::held_to(vec![TicketsEvent::Ticket { ticket: ticket("MENGER-2", "t", "") }], &reach, Some("MENGER-2".into()));
        assert!(matches!(&held[..], [TicketsEvent::NotFound { id }] if id == "MENGER-2"), "{held:?}");
        match bridge::scope_call(bridge::parse_call("list", "{}").unwrap(), Some(&reach)).unwrap() {
            bridge::Scoped::Call(bridge::Call::List { filter, .. }) => assert_eq!(filter.as_deref(), Some("under AUTH")),
            other => panic!("{other:?}"),
        }
        // Writes from the window: refused before anything is read.
        let err = bridge::display_only(bridge::SUBMIT);
        assert!(err.contains("display only") && err.contains("nothing sent"), "{err}");
        // The citer's bridge is what it was: no `cite`, no new read.
        assert_eq!(bridge::READ_METHODS.len(), 10);
        for m in ["cite", "cited", "menger:cite"] {
            assert!(bridge::parse_call(m, "{}").unwrap_err().starts_with("read-only: "), "{m}");
        }
    }

    /// A window lies over its placeholder, clamped to the citing frame's
    /// box; a hidden placeholder hides it.
    #[test]
    fn a_window_stays_inside_its_citing_frame() {
        let r = |x, y, w, h| Rect { x, y, w, h };
        assert_eq!(place(r(10.0, 200.0, 300.0, 40.0), 0.0, 5.0, 600.0, 900.0), Place { top: 205.0, left: 10.0, width: 300.0, shown: true });
        // Wider than the frame, or off its edges: held inside.
        assert_eq!(place(r(500.0, 50.0, 900.0, 40.0), 0.0, 0.0, 600.0, 900.0).width, 100.0);
        assert_eq!(place(r(-80.0, -40.0, 100.0, 4.0), 0.0, 0.0, 600.0, 900.0), Place { top: 0.0, left: 0.0, width: 100.0, shown: true });
        assert_eq!(place(r(10.0, 1e9, 50.0, 4.0), 0.0, 0.0, 600.0, 900.0).top, 900.0);
        assert_eq!(place(r(f64::NAN, 0.0, f64::INFINITY, 0.0), 0.0, 0.0, 600.0, 900.0).left, 0.0);
        // display:none in the citer: zero wide, no window.
        assert!(!place(r(0.0, 0.0, 0.0, 0.0), 0.0, 0.0, 600.0, 900.0).shown);
    }

    /// A section at its natural height, up to the cap.
    #[test]
    fn the_view_is_the_sections_height_up_to_the_cap() {
        assert_eq!(view_height(12.0, 112.4), 101.0);
        assert_eq!(view_height(0.0, 5000.0), MAX_VIEW);
        assert_eq!(view_height(40.0, 10.0), 0.0);
        assert_eq!(view_height(f64::NAN, 10.0), 0.0);
        assert_eq!(cited_reply(1, f64::NAN).height, 0);
    }

    /// A body cannot have the host read without end.
    #[test]
    fn a_body_opens_a_bounded_number_of_windows() {
        assert_eq!(over_cap(MAX_PER_FRAME - 1), None);
        assert_eq!(over_cap(MAX_PER_FRAME).as_deref(), Some("m-cite: a body opens at most 64 windows"));
    }
}
