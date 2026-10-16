//! COMMENT-3: annotation mode — outline the page, click to comment, pins.
//!
//! `a` (or `A`), or Annotate in the header, turns it on; `a` again, or Esc,
//! turns it off. While it is on:
//!
//! - **Every element is outlined** — the app's by a class on `#app`
//!   (index.html's `#app.annotating *`), a framed body's by the frame's SDK
//!   (`menger:annotate { on }`, src/frame.rs) — and the cursor is a
//!   crosshair.
//! - **Hovering** an element draws a brighter box over it with a label that
//!   names it ([`crate::anchor::label_of`]); hovering a component in a body
//!   opens its info card (COMP-9's tooltip, which leaves plain hover: the SDK
//!   shows it in this mode only).
//! - **A click is the mode's alone**: every pointer and mouse event outside
//!   the mode's own chrome is stopped before the app sees it, its default
//!   prevented (no link followed, no control opened, no focus moved) — in a
//!   frame, by the SDK, the same way. The click opens a **comment box** at
//!   the element; Save keeps it (`comments.add`) with its anchor — `route`,
//!   a stable `path`, `label`, `excerpt`, and `ticket` when the element
//!   belongs to one (src/anchor.rs); Esc, or Cancel, drops it.
//! - **Pins**: each element with open comments wears a numbered pin at its
//!   corner ([`pins`]); a click lists its comments, each with Resolve. A
//!   comment whose element is no longer found (the body was amended under
//!   it) stays, marked "moved", pinned to its ticket's heading.
//!
//! Off, the class goes, the frames are told, and everything the mode drew —
//! the hover box, the pins, a box, a list — goes with it: nothing of the
//! mode is left on the page.
//!
//! **The frame bridge** (src/frame.rs, the SDK): the frame's origin is opaque,
//! so the host cannot read its DOM. The SDK posts what is hovered and what is
//! clicked as a chain of nodes ([`crate::anchor::Node`]) with the element's
//! rect and text; the host decides its label, its candidates for a path
//! ([`crate::anchor::candidates`]), and asks the frame which of them select
//! the element alone (`menger:probe` → `menger:probed`); for pins, it asks
//! where the anchored elements are (`menger:locate` → `menger:located`: a
//! rect and the text, or nothing). Everything the mode draws over a frame is
//! drawn by the host, at the frame's rect plus the element's.
//!
//! **Links back** (`#/…?comment=<id>`, from `#/comments`): the page the
//! comment was left on opens, and its element is scrolled to and flashed
//! ([`reveal`]).
//!
//! What decides — the pins' grouping and numbering — is pure and pinned by
//! native tests; the rest is the DOM.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use leptos::prelude::*;
use menger_tickets_api::{Anchor, Comment, CommentState};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::anchor::{self, Node, FRAME_ROOT};
use crate::{comments, dom, fmt_relative, fmt_unix, frame, now_unix, Route};

// ---------------------------------------------------------------------
// Pins (pure)

/// Where an anchor stands on the page now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Spot {
    /// Its element is there.
    Found,
    /// It is not: the comment is "moved".
    Missing,
    /// Not known yet (a frame has not answered).
    Pending,
}

/// What a pin marks.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PinAt {
    /// The element at this path.
    Element(String),
    /// The heading of this ticket (none: the page's) — comments moved.
    Heading(Option<String>),
}

/// One pin: its number, its comments (oldest first), what it marks.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Pin {
    pub(crate) n: usize,
    pub(crate) ids: Vec<u64>,
    pub(crate) at: PinAt,
}

impl Pin {
    pub(crate) fn key(&self) -> String {
        match &self.at {
            PinAt::Element(p) => p.clone(),
            PinAt::Heading(t) => format!("moved:{}", t.as_deref().unwrap_or("")),
        }
    }

    pub(crate) fn moved(&self) -> bool {
        matches!(self.at, PinAt::Heading(_))
    }
}

/// The pins on the page `route`: the open comments left on it, one pin per
/// element (by path) — those whose element is gone, one per ticket, at its
/// heading — numbered from 1 in the order of each pin's oldest comment, so a
/// new comment never renumbers the pins already there. A comment whose
/// place is not known yet has no pin until it is.
pub(crate) fn pins(route: &str, comments: &[Comment], mut spot: impl FnMut(&Anchor) -> Spot) -> Vec<Pin> {
    let mut groups: Vec<(PinAt, Vec<u64>)> = Vec::new();
    // APP-22: a comment on the ticket as a whole is drawn in its Conversation,
    // not pinned to anything.
    for c in comments.iter().filter(|c| c.state == CommentState::Open && c.anchor.route == route && !crate::conversation::is_whole(c)) {
        let at = match spot(&c.anchor) {
            Spot::Found => PinAt::Element(c.anchor.path.clone()),
            Spot::Missing => PinAt::Heading(c.anchor.ticket.clone()),
            Spot::Pending => continue,
        };
        match groups.iter_mut().find(|(a, _)| *a == at) {
            Some((_, ids)) => ids.push(c.id.get()),
            None => groups.push((at, vec![c.id.get()])),
        }
    }
    for (_, ids) in &mut groups {
        ids.sort_unstable();
    }
    groups.sort_by_key(|(_, ids)| ids[0]);
    groups.into_iter().enumerate().map(|(i, (at, ids))| Pin { n: i + 1, ids, at }).collect()
}

/// Where a box of `w` × `h` goes for something at `r`: under it, else above
/// it, inside a viewport of `vw` × `vh` with a margin.
pub(crate) fn place(r: Rect, w: f64, h: f64, vw: f64, vh: f64) -> (f64, f64) {
    const M: f64 = 8.0;
    let x = r.x.min(vw - w - M).max(M);
    let below = r.y + r.h + 6.0;
    let y = if below + h <= vh - M || r.y - h - 6.0 < M { below.min(vh - h - M).max(M) } else { r.y - h - 6.0 };
    (x, y)
}

// ---------------------------------------------------------------------
// The mode's state

/// A rectangle in the app's viewport (px).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) w: f64,
    pub(crate) h: f64,
}

impl Rect {
    fn of(r: &web_sys::DomRect) -> Self {
        Rect { x: r.left(), y: r.top(), w: r.width(), h: r.height() }
    }

    /// A rect in a frame's document, in the app's viewport — APP-45: scaled
    /// as the frame is drawn (a mockup's device), and cut to what the frame
    /// shows (none of it: nothing, so no pin).
    fn in_frame(self, frame: &web_sys::HtmlIFrameElement) -> Self {
        let f = frame.get_bounding_client_rect();
        let ow = f64::from(frame.offset_width());
        let s = if ow > 0.0 && f.width() > 0.0 { f.width() / ow } else { 1.0 };
        let (cl, ct) = (f64::from(frame.client_left()), f64::from(frame.client_top()));
        let r = Rect { x: f.left() + (cl + self.x) * s, y: f.top() + (ct + self.y) * s, w: self.w * s, h: self.h * s };
        let (vx, vy) = (f.left() + cl * s, f.top() + ct * s);
        let (vr, vb) = (vx + f64::from(frame.client_width()) * s, vy + f64::from(frame.client_height()) * s);
        if r.x > vr || r.y > vb || r.x + r.w < vx || r.y + r.h < vy {
            return Rect { x: r.x, y: r.y, w: 0.0, h: 0.0 };
        }
        let (x, y) = (r.x.max(vx), r.y.max(vy));
        Rect { x, y, w: (r.x + r.w).min(vr) - x, h: (r.y + r.h).min(vb) - y }
    }

    fn style(self) -> String {
        format!("left:{}px;top:{}px;width:{}px;height:{}px", self.x, self.y, self.w.max(0.0), self.h.max(0.0))
    }

    fn visible(self) -> bool {
        self.w > 0.0 || self.h > 0.0
    }
}

/// The hover box: where, and the element's name.
#[derive(Clone, Debug, PartialEq)]
struct Hover {
    rect: Rect,
    label: String,
}

/// A comment being written: what it will be anchored to.
#[derive(Clone, Debug, PartialEq)]
struct Draft {
    /// Moves with every new box — what the box's view is keyed by, so a
    /// scroll that moves it never re-renders what is typed.
    seq: u32,
    anchor: Anchor,
    error: Option<String>,
    saving: bool,
}

/// A pin, and where it is drawn.
#[derive(Clone, Debug, PartialEq)]
struct Placed {
    pin: Pin,
    x: f64,
    y: f64,
}

/// The mode, every handle `Copy`.
#[derive(Clone, Copy)]
pub(crate) struct Annot {
    pub(crate) on: RwSignal<bool>,
    hover: RwSignal<Option<Hover>>,
    draft: RwSignal<Option<Draft>>,
    /// Where the box's element is now.
    draft_at: RwSignal<Option<Rect>>,
    placed: RwSignal<Vec<Placed>>,
    /// The pin whose list is open (its key).
    open: RwSignal<Option<String>>,
    flash: RwSignal<Option<Rect>>,
}

/// What a hover box or a box is drawn over.
enum Target {
    App(web_sys::Element),
    Frame { frame: web_sys::HtmlIFrameElement, rect: Rect },
}

impl Target {
    fn rect(&self) -> Option<Rect> {
        match self {
            Target::App(el) => el.is_connected().then(|| Rect::of(&el.get_bounding_client_rect())),
            Target::Frame { frame, rect } => frame.is_connected().then(|| rect.in_frame(frame)),
        }
    }
}

/// A click in a frame, waiting for the frame to say which selectors pick
/// its element alone.
struct Pick {
    frame: web_sys::HtmlIFrameElement,
    chain: Vec<Node>,
    rect: Rect,
    text: String,
    candidates: Vec<String>,
}

thread_local! {
    static ANNOT: Cell<Option<Annot>> = const { Cell::new(None) };
    static HOVERED: RefCell<Option<Target>> = const { RefCell::new(None) };
    static DRAFTED: RefCell<Option<Target>> = const { RefCell::new(None) };
    /// Clicks in frames, by (channel, the SDK's id).
    static PICKS: RefCell<HashMap<(String, u32), Pick>> = RefCell::new(HashMap::new());
    /// Where frames said anchored elements are: by path, the frame's
    /// channel and its answer (a rect and the text, or nothing).
    static LOCATED: RefCell<HashMap<String, (String, Option<(Rect, String)>)>> = RefCell::new(HashMap::new());
    /// What was asked of a frame: by path, its channel and when.
    static ASKED: RefCell<HashMap<String, (String, f64)>> = RefCell::new(HashMap::new());
    /// `menger:locate` asks out, by id: the frame's channel and the paths.
    static LOCATING: RefCell<HashMap<u32, (String, Vec<String>)>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u32> = const { Cell::new(0) };
    static SCHEDULED: Cell<bool> = const { Cell::new(false) };
    /// When the page last moved (the mode turned on, the route changed): a
    /// frame not there yet is not yet "moved".
    static SETTLING_SINCE: Cell<f64> = const { Cell::new(0.0) };
    static FLASHES: Cell<u32> = const { Cell::new(0) };
    static DRAFTS: Cell<u32> = const { Cell::new(0) };
}

/// How long after the page moved a frame that is not there is waited for.
const SETTLE_MS: f64 = 2500.0;

/// How long a frame's answer about an element is trusted before it is asked
/// again.
const ASK_AGAIN_MS: f64 = 1500.0;

/// How often, while the mode is on, the pins are placed again (a page still
/// loading, a body re-rendered).
const TICK_MS: i32 = 400;

fn next_id() -> u32 {
    NEXT.with(|n| {
        let v = n.get().wrapping_add(1);
        n.set(v);
        v
    })
}

pub(crate) fn get() -> Option<Annot> {
    ANNOT.with(Cell::get)
}

/// Whether the mode is on (untracked).
pub(crate) fn is_on() -> bool {
    get().is_some_and(|a| a.on.get_untracked())
}

/// `a`, or Annotate: on, or off.
pub(crate) fn toggle() {
    if let Some(a) = get() {
        a.on.update(|on| *on = !*on);
    }
}

/// Esc: closes what is open — a pin's list, then a comment box — else
/// leaves the mode. Whether it did anything.
pub(crate) fn escape() -> bool {
    let Some(a) = get() else { return false };
    if a.open.get_untracked().is_some() {
        a.open.set(None);
        return true;
    }
    if a.draft.get_untracked().is_some() {
        cancel(a);
        return true;
    }
    if a.on.get_untracked() {
        a.on.set(false);
        return true;
    }
    false
}

/// The page's route, as anchors spell it.
fn route_now() -> String {
    crate::route_text(&crate::parse_route(&dom::window().location().hash().unwrap_or_default()))
}

/// Everything the mode drew, gone.
fn clear(a: Annot) {
    HOVERED.with(|h| *h.borrow_mut() = None);
    DRAFTED.with(|d| *d.borrow_mut() = None);
    PICKS.with(|p| p.borrow_mut().clear());
    LOCATED.with(|l| l.borrow_mut().clear());
    ASKED.with(|l| l.borrow_mut().clear());
    LOCATING.with(|l| l.borrow_mut().clear());
    a.hover.set(None);
    a.draft.set(None);
    a.draft_at.set(None);
    a.placed.set(Vec::new());
    a.open.set(None);
    a.flash.set(None);
}

fn cancel(a: Annot) {
    DRAFTED.with(|d| *d.borrow_mut() = None);
    a.draft.set(None);
    a.draft_at.set(None);
}

/// Place everything again on the next frame.
pub(crate) fn schedule() {
    if SCHEDULED.with(|s| s.replace(true)) {
        return;
    }
    request_animation_frame(|| {
        SCHEDULED.with(|s| s.set(false));
        reposition();
    });
}

fn element_text(el: &web_sys::Element) -> String {
    let t = el.dyn_ref::<web_sys::HtmlElement>().map(|h| h.inner_text()).or_else(|| el.text_content()).unwrap_or_default();
    t.chars().take(600).collect()
}

// ---------------------------------------------------------------------
// Placing: the hover box, the comment box, the pins

/// The heading a moved comment is pinned to: its ticket's (the ticket page's
/// title, or wherever the page draws that ticket), else the page's first.
fn heading_rect(ticket: Option<&str>) -> Option<Rect> {
    let doc = dom::window().document()?;
    let q = |s: &str| doc.query_selector(s).ok().flatten();
    let el = ticket
        .and_then(|t| {
            let t = anchor::quote(t);
            q(&format!("#ticket-page h2[data-ticket-id={t}]")).or_else(|| q(&format!("#app [data-ticket-id={t}]")))
        })
        .or_else(|| q("#app main h2"))
        .or_else(|| q("#app main"))?;
    let r = Rect::of(&el.get_bounding_client_rect());
    r.visible().then_some(r)
}

/// Where an anchor's element is now — the app's at once; a frame's from what
/// it said (asking it, `asks`, when it has not said, or said long ago).
fn spot_of(anchor: &Anchor, asks: &mut HashMap<String, (web_sys::HtmlIFrameElement, Vec<String>)>) -> (Spot, Option<Rect>) {
    match anchor::split(&anchor.path) {
        (None, path) => match anchor::find_in_app(path) {
            Some(el) => (Spot::Found, Some(Rect::of(&el.get_bounding_client_rect()))),
            None => (Spot::Missing, None),
        },
        (Some(frame_sel), _) => {
            let Some(frame) = anchor::find_frame(frame_sel) else {
                let settling = js_sys::Date::now() - SETTLING_SINCE.with(Cell::get) < SETTLE_MS;
                return (if settling { Spot::Pending } else { Spot::Missing }, None);
            };
            let channel = frame.get_attribute("data-channel").unwrap_or_default();
            let known = LOCATED.with(|l| l.borrow().get(&anchor.path).filter(|(c, _)| *c == channel).map(|(_, a)| a.clone()));
            let asked = ASKED.with(|a| a.borrow().get(&anchor.path).is_some_and(|(c, at)| *c == channel && js_sys::Date::now() - at < ASK_AGAIN_MS));
            if !asked {
                ASKED.with(|a| a.borrow_mut().insert(anchor.path.clone(), (channel.clone(), js_sys::Date::now())));
                let entry = asks.entry(channel).or_insert_with(|| (frame.clone(), Vec::new()));
                if !entry.1.contains(&anchor.path) {
                    entry.1.push(anchor.path.clone());
                }
            }
            match known {
                None => (Spot::Pending, None),
                Some(None) => (Spot::Missing, None),
                Some(Some((rect, text))) if anchor::still_there(&anchor.path, &anchor.excerpt, Some(&text)) => (Spot::Found, Some(rect.in_frame(&frame))),
                Some(Some(_)) => (Spot::Missing, None),
            }
        }
    }
}

fn reposition() {
    let Some(a) = get() else { return };
    if !a.on.get_untracked() {
        return;
    }
    let hover = HOVERED.with(|h| h.borrow().as_ref().map(Target::rect));
    match hover {
        Some(Some(r)) => a.hover.maybe_update(|h| match h {
            Some(h) if h.rect != r => {
                h.rect = r;
                true
            }
            _ => false,
        }),
        Some(None) => {
            HOVERED.with(|h| *h.borrow_mut() = None);
            a.hover.set(None);
        }
        None => {}
    }
    if let Some(r) = DRAFTED.with(|d| d.borrow().as_ref().and_then(Target::rect)) {
        if a.draft_at.get_untracked() != Some(r) {
            a.draft_at.set(Some(r));
        }
    }

    let route = route_now();
    let held: Vec<Comment> = comments::signal().with_untracked(|h| h.as_ref().map(|h| h.list().to_vec())).unwrap_or_default();
    let mut asks = HashMap::new();
    let mut rects: HashMap<String, Rect> = HashMap::new();
    let found = pins(&route, &held, |anchor| {
        let (spot, rect) = spot_of(anchor, &mut asks);
        if let Some(r) = rect {
            rects.insert(anchor.path.clone(), r);
        }
        spot
    });
    for (channel, (frame, paths)) in asks {
        let inner: Vec<String> = paths.iter().map(|p| anchor::split(p).1.to_string()).collect();
        let id = next_id();
        LOCATING.with(|l| l.borrow_mut().insert(id, (channel.clone(), paths)));
        frame::post_locate(&frame, &channel, id, &inner);
    }
    let mut placed = Vec::new();
    for pin in found {
        let r = match &pin.at {
            PinAt::Element(path) => rects.get(path).copied(),
            PinAt::Heading(t) => heading_rect(t.as_deref()),
        };
        let Some(r) = r.filter(|r| r.visible()) else { continue };
        let (x, y) = if pin.moved() { (r.x + r.w - 26.0, r.y - 6.0) } else { (r.x + r.w - 11.0, r.y - 11.0) };
        placed.push(Placed { pin, x: x.max(2.0), y: y.max(2.0) });
    }
    if a.placed.with_untracked(|p| *p != placed) {
        a.placed.set(placed);
    }
    // A list whose pin has gone (its last comment resolved) closes.
    if let Some(open) = a.open.get_untracked() {
        if !a.placed.with_untracked(|p| p.iter().any(|x| x.pin.key() == open)) {
            a.open.set(None);
        }
    }
}

// ---------------------------------------------------------------------
// The app's own elements

fn exempt(el: &web_sys::Element) -> bool {
    el.closest("#annot-layer, #annotate").ok().flatten().is_some()
}

fn hover_app(a: Annot, el: web_sys::Element) {
    if exempt(&el) {
        return;
    }
    if el.tag_name().eq_ignore_ascii_case("iframe") {
        // The frame says what is under the pointer inside it.
        return;
    }
    if HOVERED.with(|h| matches!(&*h.borrow(), Some(Target::App(x)) if x.is_same_node(Some(&el)))) {
        return;
    }
    let Some(chain) = anchor::chain_of(&el) else {
        HOVERED.with(|h| *h.borrow_mut() = None);
        a.hover.set(None);
        return;
    };
    let label = anchor::label_of(&chain, &element_text(&el));
    let rect = Rect::of(&el.get_bounding_client_rect());
    HOVERED.with(|h| *h.borrow_mut() = Some(Target::App(el)));
    a.hover.set(Some(Hover { rect, label }));
}

fn open_box(a: Annot, anchor: Anchor, target: Target) {
    let rect = target.rect();
    DRAFTED.with(|d| *d.borrow_mut() = Some(target));
    let seq = DRAFTS.with(|d| {
        let v = d.get().wrapping_add(1);
        d.set(v);
        v
    });
    a.open.set(None);
    a.draft_at.set(rect);
    a.draft.set(Some(Draft { seq, anchor, error: None, saving: false }));
}

fn pick_app(a: Annot, el: web_sys::Element) {
    if el.tag_name().eq_ignore_ascii_case("iframe") {
        return;
    }
    let Some(chain) = anchor::chain_of(&el) else { return };
    let route = route_now();
    let text = element_text(&el);
    let anchor = Anchor {
        path: anchor::path_in_app(&el, &chain),
        label: anchor::label_of(&chain, &text),
        excerpt: anchor::excerpt_of(&text),
        ticket: anchor::ticket_of(&route, &chain),
        route,
        version: None,
    };
    open_box(a, anchor, Target::App(el));
}

/// The pointer and mouse events the mode takes for itself.
const TAKEN: [&str; 8] = ["pointerdown", "pointerup", "mousedown", "mouseup", "click", "dblclick", "auxclick", "contextmenu"];

fn install_listeners(a: Annot) {
    let w = dom::window();
    let Some(doc) = w.document() else { return };
    let over = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        if !a.on.get_untracked() {
            return;
        }
        if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
            hover_app(a, el);
        }
    });
    let _ = doc.add_event_listener_with_callback_and_bool("mouseover", over.as_ref().unchecked_ref(), true);
    over.forget();
    let out = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        if a.on.get_untracked() && ev.related_target().is_none() && HOVERED.with(|h| matches!(&*h.borrow(), Some(Target::App(_)))) {
            HOVERED.with(|h| *h.borrow_mut() = None);
            a.hover.set(None);
        }
    });
    let _ = doc.add_event_listener_with_callback_and_bool("mouseout", out.as_ref().unchecked_ref(), true);
    out.forget();
    for kind in TAKEN {
        let take = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            if !a.on.get_untracked() {
                return;
            }
            let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
            if exempt(&el) {
                return;
            }
            ev.stop_immediate_propagation();
            if kind.starts_with("pointer") {
                return; // its compatibility mouse events are stopped below
            }
            ev.prevent_default();
            // AGS-28: a click is `annotate.pick`; the rest are only taken.
            let picked = ev.dyn_ref::<web_sys::MouseEvent>().is_some_and(|m| crate::bindings::hit_mouse(m, crate::bindings::Context::Annotate) == Some("annotate.pick"));
            if kind == "click" && picked {
                pick_app(a, el);
            }
        });
        let _ = w.add_event_listener_with_callback_and_bool(kind, take.as_ref().unchecked_ref(), true);
        take.forget();
    }
    let moved = Closure::<dyn FnMut()>::new(|| {
        if is_on() {
            schedule();
        }
    });
    let _ = w.add_event_listener_with_callback_and_bool("scroll", moved.as_ref().unchecked_ref(), true);
    let _ = w.add_event_listener_with_callback("resize", moved.as_ref().unchecked_ref());
    moved.forget();
}

/// The mode, for the app's lifetime: its state, its listeners, and what it
/// follows — the frames told when it turns, the pins placed again when the
/// comments or the route move. Called once by the App.
pub(crate) fn install(route: RwSignal<Route>) -> Annot {
    let a = Annot {
        on: RwSignal::new(false),
        hover: RwSignal::new(None),
        draft: RwSignal::new(None),
        draft_at: RwSignal::new(None),
        placed: RwSignal::new(Vec::new()),
        open: RwSignal::new(None),
        flash: RwSignal::new(None),
    };
    ANNOT.with(|c| c.set(Some(a)));
    install_listeners(a);
    Effect::new(move |was: Option<bool>| {
        let on = a.on.get();
        if was.is_none() && !on {
            return on;
        }
        frame::broadcast_annotate(on);
        if on {
            a.flash.set(None);
            SETTLING_SINCE.with(|s| s.set(js_sys::Date::now()));
            comments::want();
            schedule();
            spawn_local(async move {
                while a.on.try_get_untracked() == Some(true) {
                    dom::sleep_ms(TICK_MS).await;
                    if is_on() {
                        reposition();
                    }
                }
            });
        } else {
            clear(a);
        }
        on
    });
    let held = comments::signal();
    Effect::new(move |_| {
        held.track();
        if is_on() {
            schedule();
        }
    });
    Effect::new(move |seen: Option<Route>| {
        let now = route.get();
        if seen.is_some_and(|s| s != now) {
            SETTLING_SINCE.with(|s| s.set(js_sys::Date::now()));
            HOVERED.with(|h| *h.borrow_mut() = None);
            a.hover.set(None);
            a.flash.set(None);
            if a.draft.get_untracked().is_some() {
                cancel(a);
            }
            a.open.set(None);
            if is_on() {
                schedule();
            }
        }
        // A link back from #/comments: the comment's element, shown.
        let hash = dom::window().location().hash().unwrap_or_default();
        if let Some(id) = comment_named(&hash) {
            reveal(a, id);
        }
        now
    });
    a
}

/// `comment=<id>` in a route's query.
pub(crate) fn comment_named(hash: &str) -> Option<u64> {
    crate::lens::pairs(crate::lens::query_of(hash))
        .into_iter()
        .find(|p| p.key == "comment")
        .and_then(|p| p.value)
        .and_then(|v| v.parse().ok())
}

// ---------------------------------------------------------------------
// The frames (called by src/frame.rs)

/// A frame's `menger:hover`: the element under the pointer in it, or none.
pub(crate) fn frame_hover(frame: web_sys::HtmlIFrameElement, chain: Option<Vec<Node>>, rect: Rect, text: String) {
    let Some(a) = get().filter(|a| a.on.get_untracked()) else { return };
    let Some(chain) = chain.filter(|c| !c.is_empty()) else {
        let mine = HOVERED.with(|h| matches!(&*h.borrow(), Some(Target::Frame { frame: f, .. }) if f.is_same_node(Some(&frame))));
        if mine {
            HOVERED.with(|h| *h.borrow_mut() = None);
            a.hover.set(None);
        }
        return;
    };
    let label = anchor::label_of(&chain, &text);
    let at = rect.in_frame(&frame);
    HOVERED.with(|h| *h.borrow_mut() = Some(Target::Frame { frame, rect }));
    a.hover.set(Some(Hover { rect: at, label }));
}

/// A frame's `menger:pick`: a click on an element in it — the frame is
/// asked which of its candidates select it alone.
pub(crate) fn frame_pick(frame: web_sys::HtmlIFrameElement, channel: &str, id: u32, chain: Vec<Node>, rect: Rect, text: String) {
    if !is_on() || chain.is_empty() {
        return;
    }
    let candidates = anchor::candidates(&chain, FRAME_ROOT);
    frame::post_probe(&frame, channel, id, &candidates);
    PICKS.with(|p| p.borrow_mut().insert((channel.to_string(), id), Pick { frame, chain, rect, text, candidates }));
}

/// A frame's `menger:probed`: which candidates select the picked element
/// alone — the box opens, anchored to the first.
pub(crate) fn frame_probed(channel: &str, id: u32, ok: Vec<bool>) {
    let Some(a) = get().filter(|a| a.on.get_untracked()) else { return };
    let Some(pick) = PICKS.with(|p| p.borrow_mut().remove(&(channel.to_string(), id))) else { return };
    let mut verdicts = ok.into_iter();
    let inner = anchor::choose(&pick.candidates, |_| verdicts.next().unwrap_or(false));
    let route = route_now();
    let frame_chain = anchor::chain_of(&pick.frame).unwrap_or_default();
    let anchor = Anchor {
        path: anchor::join(&anchor::frame_path(&pick.frame), &inner),
        label: anchor::label_of(&pick.chain, &pick.text),
        excerpt: anchor::excerpt_of(&pick.text),
        // APP-45: inside a mockup, the ticket that holds it.
        ticket: crate::mock_anchor::of_frame(&pick.frame).map(|m| m.ticket).or_else(|| anchor::ticket_of(&route, &frame_chain)),
        route,
        version: None,
    };
    open_box(a, anchor, Target::Frame { frame: pick.frame, rect: pick.rect });
}

/// A frame's `menger:located`: where the asked paths' elements are in it.
pub(crate) fn frame_located(channel: &str, id: u32, found: Vec<Option<(Rect, String)>>) {
    let Some((asked_on, paths)) = LOCATING.with(|l| l.borrow_mut().remove(&id)) else { return };
    if asked_on != channel {
        return;
    }
    LOCATED.with(|l| {
        let mut l = l.borrow_mut();
        for (path, f) in paths.into_iter().zip(found) {
            l.insert(path, (channel.to_string(), f));
        }
    });
    schedule();
}

/// A frame loaded, or its document moved (it reported a new size): what it
/// said about where elements are is stale.
pub(crate) fn frame_changed(channel: &str) {
    if !is_on() {
        return;
    }
    LOCATED.with(|l| l.borrow_mut().retain(|_, (c, _)| c != channel));
    ASKED.with(|l| l.borrow_mut().retain(|_, (c, _)| c != channel));
    schedule();
}

/// `a`/`A` pressed in a frame (`menger:key`, AGS-28's `annotate.toggle`):
/// the mode turns, unless an overlay is up. (Its Esc is src/frame.rs's.)
pub(crate) fn frame_toggle() {
    if crate::keys::no_overlay() {
        toggle();
    }
}

// ---------------------------------------------------------------------
// A link back: the comment's element, shown

/// `#/…?comment=<id>`: once the comment and its element are found (the page
/// may still be loading), scroll to it and flash it — or, gone, its
/// ticket's heading; then the query forgets the comment.
fn reveal(a: Annot, id: u64) {
    comments::want();
    spawn_local(async move {
        let since = js_sys::Date::now();
        while js_sys::Date::now() - since < 8000.0 {
            dom::sleep_ms(150).await;
            let Some(c) = comments::signal().with_untracked(|h| h.as_ref().and_then(|h| h.get(id).cloned())) else { continue };
            // APP-22: a whole-ticket comment's place is its row in the Conversation.
            let row = crate::conversation::is_whole(&c).then(|| crate::conversation::row_selector(id));
            let rect = match row.as_deref().map_or_else(|| anchor::split(&c.anchor.path), |r| (None, r)) {
                (None, path) => anchor::find_in_app(path).map(|el| {
                    let opts = web_sys::ScrollIntoViewOptions::new();
                    opts.set_block(web_sys::ScrollLogicalPosition::Center);
                    el.scroll_into_view_with_scroll_into_view_options(&opts);
                    move || Rect::of(&el.get_bounding_client_rect())
                }).map(|f| Box::new(f) as Box<dyn Fn() -> Rect>),
                (Some(fsel), _) => {
                    let Some(frame) = anchor::find_frame(fsel) else { continue };
                    if frame.get_attribute("data-state").as_deref() != Some("live") {
                        continue;
                    }
                    let channel = frame.get_attribute("data-channel").unwrap_or_default();
                    let known = LOCATED.with(|l| l.borrow().get(&c.anchor.path).filter(|(ch, _)| *ch == channel).map(|(_, x)| x.clone()));
                    match known {
                        None => {
                            let ask = next_id();
                            LOCATING.with(|l| l.borrow_mut().insert(ask, (channel.clone(), vec![c.anchor.path.clone()])));
                            // APP-45: a device scrolls its page to the element first.
                            frame::post_locate_revealing(&frame, &channel, ask, &[anchor::split(&c.anchor.path).1.to_string()]);
                            continue;
                        }
                        Some(Some((r, text))) if anchor::still_there(&c.anchor.path, &c.anchor.excerpt, Some(&text)) => {
                            if !crate::mock_anchor::scroll_to_device(&frame) {
                                frame::scroll_into_frame(&frame, (r.y - 80.0).max(0.0));
                            }
                            Some(Box::new(move || r.in_frame(&frame)) as Box<dyn Fn() -> Rect>)
                        }
                        Some(_) => None,
                    }
                }
            };
            let rect = match rect {
                Some(f) => {
                    dom::sleep_ms(60).await;
                    Some(f())
                }
                None if js_sys::Date::now() - since > 2500.0 => heading_rect(c.anchor.ticket.as_deref()),
                None => continue,
            };
            if let Some(r) = rect {
                flash(a, r);
            }
            break;
        }
        // The link has done its work: the query forgets the comment.
        let w = dom::window();
        let hash = w.location().hash().unwrap_or_default();
        if comment_named(&hash) == Some(id) {
            let q: Vec<crate::lens::Pair> = crate::lens::pairs(crate::lens::query_of(&hash)).into_iter().filter(|p| p.key != "comment").collect();
            let next = crate::lens::with_query(&hash, &crate::lens::join(&q));
            if let Ok(history) = w.history() {
                let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&next));
            }
        }
    });
}

fn flash(a: Annot, r: Rect) {
    let n = FLASHES.with(|f| {
        let v = f.get().wrapping_add(1);
        f.set(v);
        v
    });
    a.flash.set(Some(r));
    spawn_local(async move {
        dom::sleep_ms(1800).await;
        if FLASHES.with(Cell::get) == n {
            a.flash.try_set(None);
        }
    });
}

// ---------------------------------------------------------------------
// The layer

fn save(a: Annot, body: String) {
    let Some(d) = a.draft.get_untracked() else { return };
    if body.trim().is_empty() {
        a.draft.update(|d| {
            if let Some(d) = d {
                d.error = Some("Write the comment first.".into());
            }
        });
        return;
    }
    a.draft.update(|d| {
        if let Some(d) = d {
            d.saving = true;
            d.error = None;
        }
    });
    spawn_local(async move {
        let seq = d.seq;
        let r = comments::add(d.anchor, body).await;
        let still = a.draft.try_with_untracked(|d| d.as_ref().is_some_and(|d| d.seq == seq)) == Some(true);
        if !still {
            return;
        }
        match r {
            Ok(_) => {
                cancel(a);
                schedule();
            }
            Err(e) => a.draft.update(|d| {
                if let Some(d) = d {
                    d.saving = false;
                    d.error = Some(e);
                }
            }),
        }
    });
}

fn viewport() -> (f64, f64) {
    let w = dom::window();
    (w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1024.0), w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(768.0))
}

/// The comment box at its element.
fn draft_view(a: Annot, seq: u32) -> impl IntoView {
    let anchor = a.draft.with_untracked(|d| d.as_ref().map(|d| d.anchor.clone())).unwrap_or_default();
    let body = RwSignal::new(String::new());
    let area = NodeRef::<leptos::html::Textarea>::new();
    crate::grow::follow(area, move || body.track());
    area.on_load(|el: web_sys::HtmlTextAreaElement| {
        let _ = el.focus();
    });
    let style = move || {
        let (vw, vh) = viewport();
        let r = a.draft_at.get().unwrap_or_default();
        let (x, y) = place(r, 352.0, 200.0, vw, vh);
        format!("left:{x}px;top:{y}px")
    };
    let error = move || a.draft.with(|d| d.as_ref().filter(|d| d.seq == seq).and_then(|d| d.error.clone()));
    let saving = move || a.draft.with(|d| d.as_ref().is_some_and(|d| d.seq == seq && d.saving));
    let mut where_ = match &anchor.ticket {
        Some(t) => format!("{} · {t}", anchor.route),
        None => anchor.route.clone(),
    };
    // APP-45: inside a mockup, which and at what width.
    if let Some(m) = crate::mock_anchor::where_words(&anchor.path) {
        where_ = format!("{where_} · {m}");
    }
    view! {
        <div
            id="annot-box"
            class="annot-box"
            role="dialog"
            aria-label="Leave a comment"
            data-path=anchor.path.clone()
            style=style
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                match crate::bindings::hit(&ev, crate::bindings::Context::Dialog) {
                    Some("dialog.close") => {
                        ev.prevent_default();
                        cancel(a);
                    }
                    Some("comment.save") => {
                        ev.prevent_default();
                        save(a, body.get_untracked());
                    }
                    _ => {}
                }
                ev.stop_propagation();
            }
        >
            <div class="annot-for">
                <span class="annot-what">{anchor.label.clone()}</span>
                <span class="annot-where" title=anchor.path.clone()>{where_}</span>
            </div>
            <textarea
                node_ref=area
                class="grow"
                rows="4"
                placeholder="Leave a comment — Ctrl+Enter saves, Esc cancels"
                prop:value=move || body.get()
                on:input=move |ev| body.set(event_target_value(&ev))
            ></textarea>
            {move || error().map(|e| view! { <p class="annot-err" role="alert">{e}</p> })}
            <div class="annot-acts">
                <button class="annot-cancel" on:click=move |_| cancel(a)>"Cancel"</button>
                <button class="annot-save" disabled=saving on:click=move |_| save(a, body.get_untracked())>
                    {move || if saving() { "Saving…" } else { "Save" }}
                </button>
            </div>
        </div>
    }
}

/// An open pin's comments (`ids`), each with Resolve — drawn again only when
/// they change; where it sits follows its pin.
fn list_view(a: Annot, key: String, ids: Vec<u64>, moved: bool) -> AnyView {
    let rows: Vec<Comment> =
        comments::signal().with_untracked(|h| h.as_ref().map(|h| ids.iter().filter_map(|id| h.get(*id).cloned()).collect())).unwrap_or_default();
    let tall = 60.0 + 90.0 * rows.len().min(4) as f64;
    let style = {
        let key = key.clone();
        move || {
            let (vw, vh) = viewport();
            let at = a.placed.with(|p| p.iter().find(|x| x.pin.key() == key).map(|x| (x.x, x.y))).unwrap_or_default();
            let (x, y) = place(Rect { x: at.0, y: at.1, w: 20.0, h: 20.0 }, 340.0, tall, vw, vh);
            format!("left:{x}px;top:{y}px")
        }
    };
    let n = rows.len();
    let head = format!("{n} open comment{}", if n == 1 { "" } else { "s" });
    let what = rows.first().map(|c| c.anchor.label.clone()).unwrap_or_default();
        view! {
            <div
                id="annot-list"
                class="annot-list"
                role="dialog"
                aria-label="Comments here"
                data-pin=key.clone()
                style=style
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if crate::bindings::is(&ev, crate::bindings::Context::Dialog, "dialog.close") {
                        ev.prevent_default();
                        ev.stop_propagation();
                        a.open.set(None);
                    }
                }
            >
                <div class="annot-for">
                    <span class="annot-what">{head}" · "{what}</span>
                    {moved.then(|| view! { <span class="annot-moved" title="its element is no longer on the page — the body changed">"moved"</span> })}
                    <button class="annot-x" title="close (Esc)" on:click=move |_| a.open.set(None)>"×"</button>
                </div>
                <ul>
                    {rows
                        .into_iter()
                        .map(|c| {
                            let id = c.id;
                            let busy = RwSignal::new(false);
                            let err = RwSignal::new(None::<String>);
                            view! {
                                <li data-comment-id=id.to_string()>
                                    <div class="annot-meta">
                                        <span class="cwho">{c.author.clone()}</span>
                                        <span class="cwhen" title=fmt_unix(c.at)>{fmt_relative(now_unix(), c.at)}</span>
                                        <span class="cid">"#"{id.get()}</span>
                                        {moved.then(|| view! { <span class="annot-moved">"moved"</span> })}
                                    </div>
                                    <div class="annot-body" data-links="comment" data-ticket=c.anchor.ticket.clone()><crate::highlight::CommentText text=c.body.clone() /></div>
                                    <div class="annot-acts">
                                        {move || err.get().map(|e| view! { <span class="annot-err">{e}</span> })}
                                        <button
                                            class="annot-resolve"
                                            disabled=move || busy.get()
                                            on:click=move |_| {
                                                busy.set(true);
                                                spawn_local(async move {
                                                    let r = comments::resolve(id.get()).await;
                                                    busy.try_set(false);
                                                    err.try_set(r.err());
                                                    schedule();
                                                });
                                            }
                                        >
                                            "Resolve"
                                        </button>
                                    </div>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
            </div>
        }
        .into_any()
}

/// What the mode draws, over everything, outside `#app` — mounted once.
#[component]
pub(crate) fn Layer() -> impl IntoView {
    let Some(a) = get() else { return ().into_any() };
    let draft_seq = Memo::new(move |_| a.draft.with(|d| d.as_ref().map(|d| d.seq)));
    let pin_keys = Memo::new(move |_| a.placed.with(|p| p.iter().map(|x| x.pin.clone()).collect::<Vec<Pin>>()));
    let open_pin = Memo::new(move |_| {
        let k = a.open.get()?;
        a.placed.with(|p| p.iter().find(|x| x.pin.key() == k).map(|x| (k.clone(), x.pin.ids.clone(), x.pin.moved())))
    });
    view! {
        <div id="annot-layer" class:on=move || a.on.get()>
            {move || a.hover.get().map(|h| view! {
                <div class="annot-hover" style=h.rect.style()>
                    <span class="annot-label">{h.label}</span>
                </div>
            })}
            {move || a.flash.get().map(|r| view! { <div class="annot-flash" style=r.style()></div> })}
            <For
                each=move || pin_keys.get()
                key=|p: &Pin| p.clone()
                children=move |pin: Pin| {
                    let key = pin.key();
                    let at = {
                        let key = key.clone();
                        move || a.placed.with(|p| p.iter().find(|x| x.pin.key() == key).map(|x| format!("left:{}px;top:{}px", x.x, x.y)))
                    };
                    let count = pin.ids.len();
                    let title = format!(
                        "{count} open comment{}{} — click to list",
                        if count == 1 { "" } else { "s" },
                        if pin.moved() { ", moved: the element is no longer on the page" } else { "" }
                    );
                    let k = key.clone();
                    view! {
                        <button
                            class="annot-pin"
                            class:moved=pin.moved()
                            data-pin=key.clone()
                            data-count=count.to_string()
                            title=title
                            style=at
                            on:click=move |_| a.open.update(|o| *o = if o.as_deref() == Some(k.as_str()) { None } else { Some(k.clone()) })
                        >
                            {pin.n}
                        </button>
                    }
                }
            />
            {move || open_pin.get().map(|(k, ids, moved)| list_view(a, k, ids, moved))}
            {move || draft_seq.get().map(|seq| draft_view(a, seq))}
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::tests::{comment, resolved};

    /// The pins on a page: its open comments, one pin per element, numbered
    /// by each pin's oldest comment; the moved ones at their ticket's
    /// heading, one pin per ticket; none for another page, a resolved
    /// comment, or a place not known yet.
    #[test]
    fn pins_group_a_pages_open_comments_by_element() {
        let page = "#/ticket/DEMAND-2";
        let row = "iframe[data-ticket=\"DEMAND-2\"] >> body > table > tbody > tr:nth-of-type(2)";
        let all = vec![
            comment(9, page, "#t-status", Some("DEMAND-2")),
            comment(7, page, row, Some("DEMAND-2")),
            comment(6, "#/", "#annotate", None),
            resolved(comment(5, page, "#t-parent", Some("DEMAND-2"))),
            comment(4, page, "#t-status", Some("DEMAND-2")),
            comment(3, page, "#gone", Some("DEMAND-2")),
            comment(2, page, "#also-gone", Some("DEMAND-2")),
            comment(1, page, "#pending", Some("DEMAND-2")),
        ];
        let spot = |a: &Anchor| match a.path.as_str() {
            "#gone" | "#also-gone" => Spot::Missing,
            "#pending" => Spot::Pending,
            _ => Spot::Found,
        };
        let got = pins(page, &all, spot);
        assert_eq!(
            got,
            vec![
                Pin { n: 1, ids: vec![2, 3], at: PinAt::Heading(Some("DEMAND-2".into())) },
                Pin { n: 2, ids: vec![4, 9], at: PinAt::Element("#t-status".into()) },
                Pin { n: 3, ids: vec![7], at: PinAt::Element(row.into()) },
            ]
        );
        assert!(got[0].moved() && !got[1].moved());
        assert_eq!(got[0].key(), "moved:DEMAND-2");
        assert_eq!(got[1].key(), "#t-status");
        // A new comment adds a pin after the others; none renumbers.
        let mut more = all.clone();
        more.insert(0, comment(10, page, "#t-kind", Some("DEMAND-2")));
        let after = pins(page, &more, spot);
        assert_eq!(after[..3], got[..]);
        assert_eq!(after[3], Pin { n: 4, ids: vec![10], at: PinAt::Element("#t-kind".into()) });
        // The board: only its own.
        assert_eq!(pins("#/", &all, spot), vec![Pin { n: 1, ids: vec![6], at: PinAt::Element("#annotate".into()) }]);
        // A moved comment on no ticket pins to the page's heading.
        let lone = vec![comment(1, "#/", "#gone", None)];
        assert_eq!(pins("#/", &lone, spot), vec![Pin { n: 1, ids: vec![1], at: PinAt::Heading(None) }]);
        assert!(pins("#/inputs", &all, spot).is_empty());
        // APP-22: a comment on the ticket as a whole is in its Conversation, never pinned.
        let mut whole = all.clone();
        whole.insert(0, comment(11, page, crate::conversation::PATH, Some("DEMAND-2")));
        assert_eq!(pins(page, &whole, spot), got);
    }

    /// A box goes under what it is for, inside the viewport — above it when
    /// there is no room below.
    #[test]
    fn a_box_goes_under_its_element_inside_the_viewport() {
        let r = Rect { x: 100.0, y: 100.0, w: 50.0, h: 20.0 };
        assert_eq!(place(r, 300.0, 200.0, 1200.0, 800.0), (100.0, 126.0));
        let low = Rect { y: 700.0, ..r };
        assert_eq!(place(low, 300.0, 200.0, 1200.0, 800.0), (100.0, 494.0), "above");
        let right = Rect { x: 1150.0, ..r };
        assert_eq!(place(right, 300.0, 200.0, 1200.0, 800.0).0, 892.0, "pulled in from the right");
        let tall = Rect { y: 150.0, h: 600.0, ..r };
        let (_, y) = place(tall, 300.0, 200.0, 1200.0, 800.0);
        assert!((8.0..=592.0).contains(&y), "never off screen: {y}");
    }

    #[test]
    fn a_link_back_names_its_comment() {
        assert_eq!(comment_named("#/ticket/DEMAND-2?comment=7"), Some(7));
        assert_eq!(comment_named("#/?g=kind%20build&comment=12"), Some(12));
        assert_eq!(comment_named("#/ticket/DEMAND-2"), None);
        assert_eq!(comment_named("#/?comment=x"), None);
    }
}
