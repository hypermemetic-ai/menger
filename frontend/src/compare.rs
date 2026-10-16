//! APP-44: **a mockup compare** — any mockups on a page, side by side, at
//! one shared size and theme, still live. Owner, 2026-10-16: *"allow me to
//! do a side by side of various mocks. I would like to be able to do this
//! dynamically, picking whichever I want on the page"*.
//!
//! - Every mockup's bar ([`crate::present::Framed`]) on a ticket page has
//!   **Compare** (a toggle; `c` while focus is in the mockup's box — its
//!   bar, its devices' labels and handles; a key pressed inside a mocked
//!   page is the page's). Toggling adds the mockup to the page's **compare
//!   set** ([`toggled`]); a pill at the bottom left of the page says
//!   `compare N · open · clear` — clear of the queue control (APP-38, the
//!   bottom right) and the section dock (the right edge; at a phone's width
//!   its pill, bottom right, above the queue's).
//! - **Open** lays the set side by side in one full-window view: a column
//!   each, its description (APP-42) over it, at one shared device (390, 768,
//!   1440 or a typed `W` / `W×H`, as APP-33's) and one shared Light / Dark /
//!   Reduced motion — each column a live frame the viewer uses directly.
//!   Columns reorder by drag or ←/→ on a focused column head ([`moved`],
//!   [`nudged`]), drop with ×, and more are added from the picker: every
//!   mockup on the page (title, ticket, its number), and — by ref — another
//!   ticket's (`DESIGN-49`, a whole-page body; `DESIGN-49#m2`, its second
//!   mockup), read over the viewer's own socket (IDENTITY-36, as
//!   src/mockup.rs reads a `ref`: hidden ≡ not found, drawn by the host).
//! - Past what fits, the columns scroll sideways inside the view ([`layout`]);
//!   the page never does. At a phone's width the view is one column and a
//!   switcher.
//! - The set is the **viewer's**: kept per page in localStorage
//!   ([`storage_key`], tolerant of a storage that throws), never in the
//!   body. While the view is open the route carries it —
//!   `#/ticket/APP-37?compare=APP-37#m1,APP-37#m3` ([`from_hash`],
//!   [`with_hash`]) — so the link opens the same comparison.
//! - A member that is not there (a mockup number past the body's, a ticket
//!   the viewer cannot read, a ref that is no page) is a **named state** in
//!   its column ([`Why::text`]), never a crash.
//!
//! A mockup is named as its box names itself (APP-33's `remember`): `T#mN`,
//! the Nth `<m-mockup>` of ticket T's body, numbered as the body's frame
//! numbers them — in document order, which is the order its renderer
//! (md.rs, the frame's own) writes the placeholders; a ` ```mockup ` fence,
//! an element holding its page and an `<m-mockup ref>` alike — and `T`, a
//! `view: mockup` (whole-page) body. The column reads the body the same way:
//! rendered by md.rs, the Nth placeholder taken from an inert `<template>`.
//!
//! Columns keep their frames while they move: the column list is drawn in a
//! fixed order and placed by CSS `order`, so a reorder never re-inserts an
//! `<iframe>` (which would reload it, losing what the viewer did in it).
//!
//! Motion: the view opens with a short fade under the Motion setting (the
//! global rules, LOOK-8); nothing runs forever. What decides is pure and
//! pinned by native tests; the rest is DOM.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use leptos::prelude::*;
use menger_tickets_api::TicketsEvent;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context};
use crate::cite::{self, Fetched};
use crate::frame::{Emulate, Frame, Scheme};
use crate::mockup::{self, Posted, Source};
use crate::present::{self, Caption, Device};
use crate::{bridge, dom, md, mocksize, session};

// -- the set ------------------------------------------------------------------

/// The most mockups one comparison holds.
pub(crate) const MAX: usize = 8;

/// One mockup, as a page names it: ticket `T`'s `m`th `<m-mockup>`
/// (`T#mN`), or `T` itself — a `view: mockup` body, a whole page.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Item {
    pub(crate) ticket: String,
    pub(crate) mock: Option<u32>,
}

impl Item {
    /// `APP-37#m1`, `app-37 #M1`, `APP-37%23m1`, `APP-37` — the ticket
    /// upper-cased; anything else is none.
    pub(crate) fn parse(s: &str) -> Option<Item> {
        let s = s.trim().replace("%23", "#");
        let (t, m) = match s.split_once('#') {
            Some((t, m)) => (t.trim(), Some(m.trim())),
            None => (s.as_str(), None),
        };
        let ticket = t.to_ascii_uppercase();
        if ticket.is_empty() || !cite::ticket_ok(&ticket) {
            return None;
        }
        let mock = match m {
            None => None,
            Some(m) => {
                let n = m.strip_prefix(['m', 'M'])?.parse::<u32>().ok()?;
                if !(1..=999).contains(&n) {
                    return None;
                }
                Some(n)
            }
        };
        Some(Item { ticket, mock })
    }

    /// As the route, the storage and a box's name spell it.
    pub(crate) fn key(&self) -> String {
        match self.mock {
            Some(n) => format!("{}#m{n}", self.ticket),
            None => self.ticket.clone(),
        }
    }
}

/// The set with `item` in it (at the end) — or out of it, when it was.
/// Past [`MAX`] an add changes nothing.
pub(crate) fn toggled(set: &[Item], item: &Item) -> Vec<Item> {
    if set.contains(item) {
        set.iter().filter(|x| *x != item).cloned().collect()
    } else {
        added(set, item)
    }
}

/// The set with `item` at its end — unchanged when it holds it or is full.
pub(crate) fn added(set: &[Item], item: &Item) -> Vec<Item> {
    let mut v = set.to_vec();
    if !v.contains(item) && v.len() < MAX {
        v.push(item.clone());
    }
    v
}

/// The set with the member at `from` moved to `to` (clamped).
pub(crate) fn moved(set: &[Item], from: usize, to: usize) -> Vec<Item> {
    let mut v = set.to_vec();
    if from >= v.len() {
        return v;
    }
    let x = v.remove(from);
    let to = to.min(v.len());
    v.insert(to, x);
    v
}

/// ←/→ on a focused column head: the member at `at` one place left
/// (`delta` −1) or right (+1) — and where it now is. At an end, nothing.
pub(crate) fn nudged(set: &[Item], at: usize, delta: i32) -> (Vec<Item>, usize) {
    if at >= set.len() {
        return (set.to_vec(), at);
    }
    let to = (at as i64 + i64::from(delta)).clamp(0, set.len() as i64 - 1) as usize;
    (moved(set, at, to), to)
}

/// The set as kept and routed: `APP-37#m1,APP-37#m3`.
pub(crate) fn encode(set: &[Item]) -> String {
    set.iter().map(Item::key).collect::<Vec<_>>().join(",")
}

/// A kept or routed set read back: each readable member once, in order, at
/// most [`MAX`]; anything unreadable dropped (`%2C` reads as a comma).
pub(crate) fn decode(s: &str) -> Vec<Item> {
    let s = s.replace("%2C", ",").replace("%2c", ",");
    let mut v: Vec<Item> = Vec::new();
    for part in s.split(',') {
        if let Some(i) = Item::parse(part) {
            if !v.contains(&i) && v.len() < MAX {
                v.push(i);
            }
        }
    }
    v
}

/// The route's `compare=` — `Some` (the members it names) when the query
/// holds one.
pub(crate) fn from_hash(hash: &str) -> Option<Vec<Item>> {
    let (_, q) = hash.split_once('?')?;
    // The value runs to the next `&`: a member's own `#` is part of it.
    q.split('&').find_map(|p| p.strip_prefix("compare=")).map(decode)
}

/// The hash with the comparison in its query (`Some`, non-empty) or out of
/// it, the rest of the query kept in order.
pub(crate) fn with_hash(hash: &str, set: Option<&[Item]>) -> String {
    let (path, query) = hash.split_once('?').unwrap_or((hash, ""));
    let mut params: Vec<String> = query.split('&').filter(|p| !p.is_empty() && !p.starts_with("compare=")).map(str::to_string).collect();
    if let Some(set) = set.filter(|s| !s.is_empty()) {
        params.push(format!("compare={}", encode(set)));
    }
    if params.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", params.join("&"))
    }
}

/// The localStorage key a page's set is kept under.
pub(crate) fn storage_key(ticket: &str) -> String {
    format!("menger.compare:{}", ticket.to_ascii_uppercase())
}

/// The localStorage key the view's shared device is kept under.
pub(crate) const DEVICE_KEY: &str = "menger.compare-device";

/// The pill's count.
pub(crate) fn pill_text(n: usize) -> String {
    format!("compare {n}")
}

// -- what a column shows -------------------------------------------------------

/// Why a column shows a line instead of its mockup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Why {
    /// The ticket is none the viewer can read (hidden ≡ not found).
    NoTicket(String),
    Unreadable(String),
    /// The body holds fewer mockups than the number named.
    NoMock { ticket: String, n: u32, have: usize },
    /// A whole-page member (or a ref) whose body is no whole page.
    NotPage(String),
    /// The placeholder does not say a mockup (src/mockup.rs's reason).
    Invalid(String),
}

impl Why {
    /// The named state, as the column says it — nothing of a ticket but
    /// the id the set (or the body) wrote.
    pub(crate) fn text(&self) -> String {
        match self {
            Why::NoTicket(t) => format!("{t}: no ticket you can read"),
            Why::Unreadable(t) => format!("you cannot read {t}"),
            Why::NoMock { ticket, n, have: 0 } => format!("{ticket} has no mockup {n} — its body holds none"),
            Why::NoMock { ticket, n, have: 1 } => format!("{ticket} has no mockup {n} — its body holds one"),
            Why::NoMock { ticket, n, have } => format!("{ticket} has no mockup {n} — its body holds {have}"),
            Why::NotPage(t) => format!("{t} is no whole HTML page — a mockup shows a body that starts <!doctype html>"),
            Why::Invalid(why) => why.clone(),
        }
    }
}

/// The `n`th (1-based) of a body's placeholders — or why there is none.
pub(crate) fn nth<'a>(ticket: &str, posted: &'a [Posted], n: u32) -> Result<&'a Posted, Why> {
    posted
        .get((n as usize).wrapping_sub(1))
        .ok_or_else(|| Why::NoMock { ticket: ticket.to_string(), n, have: posted.len() })
}

/// A line naming a mockup in the picker: its title, else its description's
/// first line, else `mockup N` — clipped. (A column's head passes the title
/// alone: the description is under it.)
pub(crate) fn label(caption: &Caption, n: Option<u32>) -> String {
    let words = caption
        .title
        .clone()
        .or_else(|| caption.description.as_deref().and_then(|d| d.lines().map(str::trim).find(|l| !l.is_empty())).map(str::to_string));
    let s = match (words, n) {
        (Some(w), _) => w,
        (None, Some(n)) => format!("mockup {n}"),
        (None, None) => "the page".to_string(),
    };
    match s.char_indices().nth(LABEL_MAX) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s,
    }
}

/// The longest label, in characters.
pub(crate) const LABEL_MAX: usize = 80;

// -- the layout ----------------------------------------------------------------

/// The gap between columns, px.
pub(crate) const GAP: f64 = 16.0;
/// The narrowest a column gets before the row scrolls sideways, px.
pub(crate) const MIN_COL: f64 = 300.0;
/// A column's head and caption over its screen, px.
pub(crate) const COL_CHROME: f64 = 92.0;
/// A screen's border, left and right together, px.
pub(crate) const SCREEN_EDGE: f64 = 4.0;
/// At or under this stage width the view is one column and a switcher.
pub(crate) const PHONE_W: f64 = 600.0;

/// How the columns sit in a stage `w` × `h` px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Layout {
    /// One column's width, px.
    pub(crate) col_w: f64,
    /// The device's scale in its column — never above 1.
    pub(crate) scale: f64,
    /// Whether the columns run past the stage (it scrolls sideways).
    pub(crate) scrolls: bool,
    /// One column at a time, with a switcher.
    pub(crate) single: bool,
}

/// `n` columns of `device` in a stage `w` × `h`: each its share of the
/// width, at least [`MIN_COL`] (then the row scrolls); the device scaled to
/// fit its column's width and the stage's height under the column's head.
pub(crate) fn layout(w: f64, h: f64, n: usize, device: Device) -> Layout {
    let single = w <= PHONE_W;
    let n = if single { 1 } else { n.max(1) };
    let gaps = GAP * (n as f64 - 1.0);
    let share = ((w - gaps) / n as f64).max(1.0);
    let col_w = if single { w.max(1.0) } else { share.max(MIN_COL) };
    let by_w = (col_w - SCREEN_EDGE).max(1.0) / f64::from(device.w.max(1));
    let by_h = if h > COL_CHROME + 40.0 { (h - COL_CHROME) / f64::from(device.h.max(1)) } else { by_w };
    let scale = by_w.min(by_h).clamp(0.05, 1.0);
    let scrolls = !single && col_w * n as f64 + gaps > w + 0.5;
    Layout { col_w, scale, scrolls, single }
}

// -- the DOM half --------------------------------------------------------------

/// A body's placeholders as read: when it was updated, how long it was, what
/// it holds.
type Parsed = (i64, usize, Rc<Vec<Posted>>);

thread_local! {
    /// The ticket page the set is for.
    static PAGE: ArcRwSignal<Option<String>> = ArcRwSignal::new(None);
    static SET: ArcRwSignal<Vec<Item>> = ArcRwSignal::new(Vec::new());
    static OPEN: ArcRwSignal<bool> = ArcRwSignal::new(false);
    /// Bumped when a read lands or a read ticket changes: columns re-resolve.
    static REV: ArcRwSignal<u32> = ArcRwSignal::new(0);
    /// Bumped when the page's mockup boxes change (APP-46's *pick this*
    /// rides on them).
    static BOXREV: ArcRwSignal<u32> = ArcRwSignal::new(0);
    /// What the viewer's socket answered, by ticket id.
    static READ: RefCell<HashMap<String, Fetched>> = RefCell::new(HashMap::new());
    static ASKED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Each read body's placeholders, by ticket — kept while it is unchanged.
    static PARSED: RefCell<HashMap<String, Parsed>> = RefCell::new(HashMap::new());
}

fn page() -> ArcRwSignal<Option<String>> {
    PAGE.with(Clone::clone)
}
fn set_sig() -> ArcRwSignal<Vec<Item>> {
    SET.with(Clone::clone)
}
fn open_sig() -> ArcRwSignal<bool> {
    OPEN.with(Clone::clone)
}
fn rev() -> ArcRwSignal<u32> {
    REV.with(Clone::clone)
}

/// src/mockup.rs: a box was drawn — a column of it may now carry its bar.
pub(crate) fn boxes_moved() {
    BOXREV.with(|r| r.update(|n| *n = n.wrapping_add(1)));
}

/// APP-46's *compare these* on an option's look (the box `k`): every look of
/// that decision, side by side — the set becomes them, in the body's order.
pub(crate) fn open_decision(k: &str) {
    let items: Vec<Item> = mockup::decision_of(k)
        .into_iter()
        .map(|(t, n)| Item { ticket: t.to_ascii_uppercase(), mock: Some(n) })
        .take(MAX)
        .collect();
    let on_page = |i: &Item| page().with_untracked(|p| p.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(&i.ticket)));
    if items.is_empty() || !items.iter().all(on_page) {
        return;
    }
    commit(items);
    open();
}

fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

fn load(ticket: &str) -> Vec<Item> {
    storage().and_then(|s| s.get_item(&storage_key(ticket)).ok().flatten()).map(|v| decode(&v)).unwrap_or_default()
}

fn keep(ticket: &str, set: &[Item]) {
    let Some(s) = storage() else { return };
    let k = storage_key(ticket);
    let _ = if set.is_empty() { s.remove_item(&k) } else { s.set_item(&k, &encode(set)) };
}

/// The route's query, rewritten in place (no history entry, no hashchange).
fn route(set: Option<&[Item]>) {
    let w = dom::window();
    let hash = w.location().hash().unwrap_or_default();
    if !hash.starts_with("#/ticket/") {
        return;
    }
    let next = with_hash(&hash, set);
    if next != hash {
        if let Ok(h) = w.history() {
            let _ = h.replace_state_with_url(&JsValue::NULL, "", Some(&next));
        }
    }
}

/// The set, changed: kept, and routed while the view is open.
fn commit(next: Vec<Item>) {
    let s = set_sig();
    if s.with_untracked(|v| *v == next) {
        return;
    }
    if let Some(t) = page().get_untracked() {
        keep(&t, &next);
    }
    if open_sig().get_untracked() {
        route(Some(&next));
    }
    s.set(next);
}

/// A ticket page opened: its set is the route's (which opens the view), else
/// the one kept for it.
fn enter(ticket: &str) {
    let t = ticket.to_ascii_uppercase();
    let hash = dom::window().location().hash().unwrap_or_default();
    page().set(Some(t.clone()));
    match from_hash(&hash) {
        Some(set) => {
            keep(&t, &set);
            set_sig().set(set.clone());
            open_sig().set(!set.is_empty());
        }
        None => {
            set_sig().set(load(&t));
            open_sig().set(false);
        }
    }
}

fn leave(ticket: &str) {
    let p = page();
    if p.get_untracked().is_some_and(|t| t.eq_ignore_ascii_case(ticket)) {
        p.set(None);
        open_sig().set(false);
        set_sig().set(Vec::new());
    }
}

fn open() {
    let set = set_sig().get_untracked();
    if set.is_empty() {
        return;
    }
    route(Some(&set));
    open_sig().set(true);
}

fn close() {
    route(None);
    open_sig().set(false);
    // Focus back where the view was opened from.
    if let Some(b) = dom::window()
        .document()
        .and_then(|d| d.query_selector(".cmp-pill .cmp-open").ok().flatten())
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = b.focus();
    }
}

/// Whether `item` is a mockup of the page open now — a box elsewhere (a
/// board note, the editor's preview) has no comparison to join.
fn of_page(item: &Item) -> bool {
    page().with(|p| p.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(&item.ticket)))
}

/// `c` (or the bar's Compare): the mockup named `key` in or out of the set.
pub(crate) fn toggle_key(key: &str) -> bool {
    let Some(item) = Item::parse(key) else { return false };
    if !page().with_untracked(|p| p.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(&item.ticket))) {
        return false;
    }
    commit(toggled(&set_sig().get_untracked(), &item));
    true
}

/// The mockup box focus is in, if any — its name (`data-compare`).
pub(crate) fn box_key(target: &web_sys::Element) -> Option<String> {
    target.closest(".framed[data-compare]").ok().flatten().and_then(|b| b.get_attribute("data-compare"))
}

/// The bar's Compare toggle for the mockup named `key`, while `mockup` holds
/// and the box is on its own ticket's page.
pub(crate) fn toggle_button(key: String, mockup: Memo<bool>) -> impl IntoView {
    let item = Item::parse(&key);
    let set = set_sig();
    move || {
        let item = item.clone()?;
        if !(mockup.get() && of_page(&item)) {
            return None;
        }
        let on = {
            let (set, item) = (set.clone(), item.clone());
            move || set.with(|s| s.contains(&item)).to_string()
        };
        let key = item.key();
        Some(view! {
            <button class="framed-btn framed-compare" aria-pressed=on title="Compare this mockup side by side with others (c, focus in its box)"
                on:click=move |_| { toggle_key(&key); }>
                "Compare"
            </button>
        })
    }
}

/// A ticket's read, asked once and kept; tracked by [`REV`].
fn fetched(id: &str) -> Option<Fetched> {
    rev().track();
    if let Some(f) = READ.with(|r| r.borrow().get(id).cloned()) {
        return Some(f);
    }
    let first = ASKED.with(|a| {
        let mut a = a.borrow_mut();
        let new = !a.iter().any(|x| x == id);
        if new {
            a.push(id.to_string());
        }
        new
    });
    if first {
        let id = id.to_string();
        spawn_local(async move {
            let answer = match session::client() {
                Some(client) => match crate::typed::typed(&id) {
                    Ok(tid) => client.get_all(tid).await.map_err(|e| session::describe(&e)),
                    Err(why) => Err(why),
                },
                None => Err("not connected".to_string()),
            };
            let f = cite::classify(answer);
            READ.with(|r| r.borrow_mut().insert(id.clone(), f));
            ASKED.with(|a| a.borrow_mut().retain(|x| *x != id));
            rev().update(|n| *n = n.wrapping_add(1));
        });
    }
    None
}

/// Every `<m-mockup>` placeholder a body's frame holds, in its order — the
/// body rendered as the frame renders it, read from an inert `<template>`
/// (nothing in it loads or runs).
fn placeholders(ticket: &menger_tickets_api::Ticket) -> Rc<Vec<Posted>> {
    let id = ticket.id.to_string();
    if let Some(p) = PARSED.with(|p| {
        p.borrow().get(&id).filter(|(u, l, _)| *u == ticket.updated_unix && *l == ticket.body.len()).map(|(_, _, v)| v.clone())
    }) {
        return p;
    }
    let (_, src) = md::split_frontmatter(&ticket.body);
    let mut out = Vec::new();
    if !md::is_page(src) {
        let html = md::render_body_framed(src, &|_| None).html;
        if let Some(t) = dom::window()
            .document()
            .and_then(|d| d.create_element("template").ok())
            .and_then(|e| e.dyn_into::<web_sys::HtmlTemplateElement>().ok())
        {
            t.set_inner_html(&html);
            if let Ok(list) = t.content().query_selector_all(md::MOCKUP_TAG) {
                for i in 0..list.length() {
                    let Some(el) = list.get(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
                    out.push(Posted {
                        reference: el.get_attribute("ref"),
                        widths: el.get_attribute("widths"),
                        source: el.get_attribute("source"),
                        title: el.get_attribute("title"),
                        description: el.get_attribute("description"),
                        // A column's *pick this* rides on the page's box
                        // (mockup::box_of), never on this read.
                        option: None,
                    });
                }
            }
        }
    }
    let v = Rc::new(out);
    PARSED.with(|p| p.borrow_mut().insert(id, (ticket.updated_unix, ticket.body.len(), v.clone())));
    v
}

/// What a column draws.
#[derive(Clone, Debug, PartialEq)]
enum Col {
    Loading,
    Shown { page: String, caption: Caption, scope: String },
    Failed(Why),
}

fn read_ticket(id: &str) -> Result<Option<Box<menger_tickets_api::Ticket>>, Why> {
    match fetched(id) {
        None => Ok(None),
        Some(Fetched::Ticket(t)) => Ok(Some(t)),
        Some(Fetched::NoTicket) => Err(Why::NoTicket(id.to_string())),
        Some(Fetched::Unreadable) => Err(Why::Unreadable(id.to_string())),
    }
}

/// A whole-page ticket's page and caption.
fn whole(id: &str, own: Caption) -> Col {
    match read_ticket(id) {
        Ok(None) => Col::Loading,
        Err(why) => Col::Failed(why),
        Ok(Some(t)) => match mockup::page_of(&t.body) {
            Some((page, decl)) => {
                Col::Shown { page, caption: own.or(Some(&decl.caption)), scope: bridge::scope_of(Some(id)).unwrap_or_default() }
            }
            None => Col::Failed(Why::NotPage(id.to_string())),
        },
    }
}

/// What `item` draws now — tracked: a read landing or a body amended
/// re-resolves it.
fn resolve(item: &Item) -> Col {
    let Some(n) = item.mock else { return whole(&item.ticket, Caption::default()) };
    let t = match read_ticket(&item.ticket) {
        Ok(None) => return Col::Loading,
        Err(why) => return Col::Failed(why),
        Ok(Some(t)) => t,
    };
    let posted = placeholders(&t);
    let p = match nth(&item.ticket, &posted, n) {
        Ok(p) => p.clone(),
        Err(why) => return Col::Failed(why),
    };
    let own = Caption::of(p.title.as_deref(), p.description.as_deref());
    match mockup::parse(p.reference.as_deref(), p.source.as_deref()) {
        Err(why) => Col::Failed(Why::Invalid(why)),
        Ok(Source::Inline(page)) => Col::Shown { page, caption: own, scope: bridge::scope_of(Some(&item.ticket)).unwrap_or_default() },
        Ok(Source::Ref(id)) => whole(&id, own),
    }
}

/// The picker's rows: every mockup of the page's ticket.
fn listing(ticket: &str) -> Result<Option<Vec<(Item, String)>>, Why> {
    let Some(t) = read_ticket(ticket)? else { return Ok(None) };
    let (fm, src) = md::split_frontmatter(&t.body);
    if md::is_page(src) {
        let d = present::declared(fm, src);
        return Ok(Some(vec![(Item { ticket: ticket.to_string(), mock: None }, label(&d.caption, None))]));
    }
    let posted = placeholders(&t);
    Ok(Some(
        posted
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let n = i as u32 + 1;
                let mut l = label(&Caption::of(p.title.as_deref(), p.description.as_deref()), Some(n));
                if let Some(r) = p.reference.as_deref().filter(|r| !r.trim().is_empty()) {
                    l = format!("{l} (ref {})", r.trim());
                }
                (Item { ticket: ticket.to_string(), mock: Some(n) }, l)
            })
            .collect(),
    ))
}

/// Every `watch` event: a ticket a column reads, amended or created, is read
/// again in place — its frames re-render without a flash.
pub(crate) fn heard(ev: &TicketsEvent) {
    let (ticket, created) = match ev {
        TicketsEvent::Amended { ticket, .. } => (ticket, false),
        TicketsEvent::Created { ticket } => (ticket, true),
        _ => return,
    };
    let id = ticket.id.to_string();
    // A ticket no column reads stays unread; a created one may be a ref a
    // column named before it existed.
    let known = READ.with(|r| r.borrow().contains_key(&id));
    if !known && !created {
        return;
    }
    READ.with(|r| r.borrow_mut().insert(id.clone(), Fetched::Ticket(Box::new(ticket.clone()))));
    PARSED.with(|p| p.borrow_mut().remove(&id));
    rev().update(|n| *n = n.wrapping_add(1));
}

fn load_device() -> Device {
    storage()
        .and_then(|s| s.get_item(DEVICE_KEY).ok().flatten())
        .and_then(|v| mocksize::decode(Some(&v)))
        .and_then(|v| v.first().copied())
        .unwrap_or(Device { w: 390, h: 844 })
}

fn keep_device(d: Device) {
    if let Some(s) = storage() {
        let _ = s.set_item(DEVICE_KEY, &mocksize::encode(&[d]));
    }
}

/// The ticket page's pill and, when open, the view.
#[component]
pub(crate) fn Tray(ticket: String) -> impl IntoView {
    enter(&ticket);
    let t = ticket.clone();
    on_cleanup(move || leave(&t));
    let set = set_sig();
    let is_open = open_sig();
    let n = {
        let set = set.clone();
        move || set.with(Vec::len)
    };
    let n2 = n.clone();
    let hidden = {
        let is_open = is_open.clone();
        move || n2() == 0 || is_open.get()
    };
    let label = {
        let n = n.clone();
        move || format!("{} — open them side by side", pill_text(n()))
    };
    view! {
        <div class="cmp-pill" role="group" aria-label=label hidden=hidden>
            <span class="cmp-n">{move || pill_text(n())}</span>
            <button class="cmp-open" title="Open the picked mockups side by side" on:click=move |_| open()>"open"</button>
            <button class="cmp-clear" title="Empty the comparison" on:click=move |_| commit(Vec::new())>"clear"</button>
        </div>
        {move || is_open.get().then(|| view! { <CompareView ticket=ticket.clone() /> })}
    }
}

/// The full-window view: the shared controls, then the columns.
#[component]
fn CompareView(ticket: String) -> impl IntoView {
    let set = set_sig();
    let device = RwSignal::new(load_device());
    let scheme = RwSignal::new(crate::frame::painted_scheme());
    let reduce = RwSignal::new(present::prefers_reduced_motion());
    let emulate = Signal::derive(move || Emulate { scheme: Some(scheme.get()), reduce: Some(reduce.get()) });
    let size = RwSignal::new((0.0f64, 0.0f64));
    let stage = NodeRef::<leptos::html::Div>::new();
    stage.on_load(move |el: web_sys::HtmlDivElement| present::observe(el, size));
    let root = NodeRef::<leptos::html::Div>::new();
    root.on_load(move |el: web_sys::HtmlDivElement| {
        if let Ok(Some(b)) = el.query_selector(".cmp-close") {
            if let Ok(b) = b.dyn_into::<web_sys::HtmlElement>() {
                let _ = b.focus();
            }
        }
    });
    let count = {
        let set = set.clone();
        Memo::new(move |_| set.with(Vec::len))
    };
    let lay = Memo::new(move |_| {
        let (w, h) = size.get();
        layout(w, h, count.get(), device.get())
    });
    // The column shown alone at a phone's width.
    let active = RwSignal::new(0usize);
    Effect::new(move |_| {
        let n = count.get();
        if active.get_untracked() >= n && n > 0 {
            active.set(n - 1);
        }
    });
    let dragging = RwSignal::new(None::<Item>);
    let keys = move |ev: web_sys::KeyboardEvent| {
        if bindings::hit(&ev, Context::Compare) == Some("compare.close") {
            ev.prevent_default();
            ev.stop_propagation();
            close();
        }
    };
    let set_c = set.clone();
    let columns = move || {
        // A fixed order (by name): a reorder moves a column by CSS `order`,
        // never its frame in the DOM.
        let mut v = set_c.get();
        v.sort_by_key(Item::key);
        v
    };
    let set_s = set.clone();
    let switcher = move || {
        let set = set_s.clone();
        lay.get().single.then(|| {
            let n = set.with(Vec::len);
            let set2 = set.clone();
            let title = move || set2.with(|s| s.get(active.get()).map(Item::key)).unwrap_or_default();
            view! {
                <div class="cmp-switch" role="group" aria-label="which mockup">
                    <button class="framed-btn" aria-label="the previous mockup" disabled=move || active.get() == 0
                        on:click=move |_| active.update(|a| *a = a.saturating_sub(1))>"‹"</button>
                    <span class="cmp-switch-at"><span class="code">{move || format!("{} of {n}", active.get() + 1)}</span>" "{title}</span>
                    <button class="framed-btn" aria-label="the next mockup" disabled=move || n <= active.get() + 1
                        on:click=move |_| active.update(|a| *a = (*a + 1).min(n.saturating_sub(1)))>"›"</button>
                </div>
            }
        })
    };
    view! {
        <div class="cmp-view" role="dialog" aria-modal="true" aria-label="Mockups side by side" node_ref=root on:keydown=keys
            class:is-dragging=move || dragging.get().is_some() class:is-single=move || lay.get().single>
            <div class="cmp-bar">
                <span class="cmp-heading">"Compare "<span class="code">{move || count.get()}</span></span>
                <span class="framed-gap"></span>
                {size_controls(device)}
                <span class="framed-group" role="group" aria-label="colour scheme">
                    <button class="framed-btn" aria-pressed=move || (scheme.get() == Scheme::Light).to_string() on:click=move |_| scheme.set(Scheme::Light)>"Light"</button>
                    <button class="framed-btn" aria-pressed=move || (scheme.get() == Scheme::Dark).to_string() on:click=move |_| scheme.set(Scheme::Dark)>"Dark"</button>
                </span>
                <button class="framed-btn" title="prefers-reduced-motion, as every page sees it" aria-pressed=move || reduce.get().to_string()
                    on:click=move |_| reduce.update(|r| *r = !*r)>"Reduced motion"</button>
                {picker(ticket.clone())}
                <button class="framed-btn cmp-close" title="Close (Esc) — the set stays" on:click=move |_| close()>"Close"</button>
            </div>
            {switcher}
            <div class="cmp-stage" node_ref=stage class:scrolls=move || lay.get().scrolls>
                <For each=columns key=Item::key children=move |item: Item| column(item, device, emulate, lay, active, dragging) />
            </div>
        </div>
    }
}

/// The shared device: 390 · 768 · 1440, or a size typed.
fn size_controls(device: RwSignal<Device>) -> impl IntoView {
    let refused = RwSignal::new(None::<String>);
    let input = NodeRef::<leptos::html::Input>::new();
    let pick = move |d: Device| {
        device.set(d);
        keep_device(d);
    };
    let field_keys = move |ev: web_sys::KeyboardEvent| {
        ev.stop_propagation();
        match bindings::hit(&ev, Context::Field) {
            Some("form.submit") => {
                ev.prevent_default();
                let text = input.get_untracked().map(|i| i.value()).unwrap_or_default();
                match mocksize::parse_size(&text) {
                    Ok((w, h)) => {
                        refused.set(None);
                        pick(h.map_or_else(|| Device::of_width(w), |h| Device { w, h }));
                    }
                    Err(why) => refused.set(Some(why)),
                }
            }
            Some("form.cancel") => {
                ev.prevent_default();
                refused.set(None);
                if let Some(i) = input.get_untracked() {
                    i.set_value("");
                }
            }
            _ => {
                if refused.get_untracked().is_some() {
                    refused.set(None);
                }
            }
        }
    };
    let buttons = present::DEFAULT_WIDTHS
        .iter()
        .map(|w| {
            let d = Device::of_width(*w);
            view! {
                <button class="framed-btn" title=format!("{} ({})", mocksize::says(d), d.kind()) aria-pressed=move || (device.get() == d).to_string()
                    on:click=move |_| pick(d)>{w.to_string()}</button>
            }
        })
        .collect_view();
    view! {
        <span class="framed-group" role="group" aria-label="the shared device">{buttons}</span>
        <span class="framed-field">
            <input class="framed-input code cmp-size" type="text" node_ref=input placeholder=move || mocksize::says(device.get())
                aria-label="Every column's size: W or W×H, then Enter" title="Every column's size: W or W×H, then Enter"
                aria-invalid=move || refused.get().is_some().to_string() on:keydown=field_keys />
            <span class="framed-refused" role="alert" hidden=move || refused.get().is_none()>{move || refused.get()}</span>
        </span>
    }
}

/// The picker: every mockup on the page, and another ticket's by ref.
fn picker(ticket: String) -> impl IntoView {
    let menu = RwSignal::new(false);
    let menu_ref = NodeRef::<leptos::html::Div>::new();
    let wrap = NodeRef::<leptos::html::Span>::new();
    let refused = RwSignal::new(None::<String>);
    let input = NodeRef::<leptos::html::Input>::new();
    let set = set_sig();
    let toggle = move |_| {
        let open = !menu.get_untracked();
        menu.set(open);
        if open {
            if let Some(first) = menu_ref.get_untracked().and_then(|m| m.query_selector("button").ok().flatten()) {
                if let Ok(b) = first.dyn_into::<web_sys::HtmlElement>() {
                    let _ = b.focus();
                }
            }
        }
    };
    let rows = move || {
        let set = set.clone();
        match listing(&ticket) {
            Ok(None) => view! { <p class="cmp-menu-note">"reading the page…"</p> }.into_any(),
            Err(why) => view! { <p class="cmp-menu-note">{why.text()}</p> }.into_any(),
            Ok(Some(v)) if v.is_empty() => view! { <p class="cmp-menu-note">"this page has no mockup"</p> }.into_any(),
            Ok(Some(v)) => v
                .into_iter()
                .map(|(item, l)| {
                    let set = set.clone();
                    let it = item.clone();
                    let on = Memo::new(move |_| set.with(|s| s.contains(&it)));
                    let at = item.key();
                    view! {
                        <button class="framed-menu-item" role="menuitemcheckbox" aria-checked=move || on.get().to_string() class:is-on=move || on.get()
                            title="In or out of the comparison"
                            on:click=move |_| commit(toggled(&set_sig().get_untracked(), &item))>
                            <span class="cmp-menu-l">{l}</span>
                            <span class="code">{at}</span>
                        </button>
                    }
                })
                .collect_view()
                .into_any(),
        }
    };
    let menu_keys = move |ev: web_sys::KeyboardEvent| {
        let Some(m) = menu_ref.get_untracked() else { return };
        let Ok(list) = m.query_selector_all("button") else { return };
        let n = list.length();
        let active = dom::window().document().and_then(|d| d.active_element());
        let at = (0..n).find(|i| list.get(*i).map(|x| Some(x.unchecked_into::<web_sys::Element>()) == active) == Some(true));
        let to = match bindings::hit(&ev, Context::Menu) {
            Some("dialog.close") => {
                ev.stop_propagation();
                ev.prevent_default();
                menu.set(false);
                return;
            }
            Some("list.next") => at.map_or(0, |i| (i + 1) % n.max(1)),
            Some("list.prev") => at.map_or(n.saturating_sub(1), |i| (i + n - 1) % n.max(1)),
            Some("list.first") => 0,
            Some("list.last") => n.saturating_sub(1),
            _ => return,
        };
        ev.prevent_default();
        if let Some(b) = list.get(to).and_then(|x| x.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = b.focus();
        }
    };
    let field_keys = move |ev: web_sys::KeyboardEvent| {
        ev.stop_propagation();
        match bindings::hit(&ev, Context::Field) {
            Some("form.submit") => {
                ev.prevent_default();
                let text = input.get_untracked().map(|i| i.value()).unwrap_or_default();
                match Item::parse(&text) {
                    Some(item) => {
                        let before = set_sig().get_untracked();
                        let after = added(&before, &item);
                        if after == before && !before.contains(&item) {
                            refused.set(Some(format!("a comparison holds at most {MAX}")));
                        } else {
                            refused.set(None);
                            commit(after);
                            if let Some(i) = input.get_untracked() {
                                i.set_value("");
                            }
                        }
                    }
                    None => refused.set(Some("a ticket (DESIGN-49) or one of its mockups (DESIGN-49#m2)".into())),
                }
            }
            Some("form.cancel") => {
                ev.prevent_default();
                menu.set(false);
            }
            _ => {
                if refused.get_untracked().is_some() {
                    refused.set(None);
                }
            }
        }
    };
    let left = move |ev: web_sys::FocusEvent| {
        let into = ev.related_target().and_then(|t| t.dyn_into::<web_sys::Node>().ok());
        let inside = wrap.get_untracked().is_some_and(|w| into.is_some_and(|n| w.contains(Some(&n))));
        if !inside {
            menu.set(false);
        }
    };
    view! {
        <span class="framed-sizes cmp-pick" node_ref=wrap on:focusout=left>
            <button class="framed-btn" aria-haspopup="menu" aria-expanded=move || menu.get().to_string() title="Add a mockup: this page's, or another ticket's by ref"
                on:click=toggle>"+ Add ▾"</button>
            <div class="framed-menu cmp-menu" role="menu" aria-label="mockups to compare" hidden=move || !menu.get() node_ref=menu_ref on:keydown=menu_keys>
                {rows}
                <label class="cmp-ref">
                    <span>"By ref"</span>
                    <input class="framed-input code" type="text" node_ref=input placeholder="DESIGN-49 or DESIGN-49#m2"
                        aria-label="Another ticket's mockup: its id (a whole page) or ID#mN, then Enter"
                        aria-invalid=move || refused.get().is_some().to_string() on:keydown=field_keys />
                </label>
                <span class="framed-refused" role="alert" hidden=move || refused.get().is_none()>{move || refused.get()}</span>
            </div>
        </span>
    }
}

/// One column: its head (grip, label, name, ×), its caption, its screen.
fn column(item: Item, device: RwSignal<Device>, emulate: Signal<Emulate>, lay: Memo<Layout>, active: RwSignal<usize>, dragging: RwSignal<Option<Item>>) -> impl IntoView {
    let set = set_sig();
    let pos = {
        let (set, item) = (set.clone(), item.clone());
        Memo::new(move |_| set.with(|s| s.iter().position(|x| *x == item)).unwrap_or(0))
    };
    let col = {
        let item = item.clone();
        Memo::new(move |_| resolve(&item))
    };
    let shown = Memo::new(move |_| matches!(col.get(), Col::Shown { .. }));
    let html = Signal::derive(move || match col.get() {
        Col::Shown { page, .. } => page,
        _ => String::new(),
    });
    let caption = Memo::new(move |_| match col.get() {
        Col::Shown { caption, .. } => caption,
        _ => Caption::default(),
    });
    let n = item.mock;
    // The head names it by its title; its description is said under the
    // head, not twice.
    let title = move || label(&Caption { title: caption.get().title, description: None }, n);
    let key = item.key();
    let style = move || format!("order:{};width:{:.0}px", pos.get(), lay.get().col_w);
    let hidden = move || lay.get().single && active.get() != pos.get();
    let screen = move || {
        let (d, s) = (device.get(), lay.get().scale);
        format!("width:{:.1}px;height:{:.1}px", f64::from(d.w) * s, f64::from(d.h) * s)
    };
    let inner = move || {
        let d = device.get();
        format!("width:{}px;height:{}px;transform:scale({:.4})", d.w, d.h, lay.get().scale)
    };
    let head_keys = move |ev: web_sys::KeyboardEvent| {
        let delta = match bindings::hit(&ev, Context::Compare) {
            Some("compare.left") => -1,
            Some("compare.right") => 1,
            _ => return,
        };
        ev.prevent_default();
        ev.stop_propagation();
        let (next, to) = nudged(&set_sig().get_untracked(), pos.get_untracked(), delta);
        commit(next);
        if lay.get_untracked().single {
            active.set(to);
        }
    };
    let drag_item = item.clone();
    let start = move |ev: web_sys::DragEvent| {
        if let Some(dt) = ev.data_transfer() {
            let _ = dt.set_data("text/plain", &drag_item.key());
            dt.set_effect_allowed("move");
        }
        dragging.set(Some(drag_item.clone()));
    };
    let over = move |ev: web_sys::DragEvent| {
        if dragging.get_untracked().is_some() {
            ev.prevent_default();
        }
    };
    let drop_item = item.clone();
    let dropped = move |ev: web_sys::DragEvent| {
        let Some(from) = dragging.get_untracked() else { return };
        ev.prevent_default();
        dragging.set(None);
        let s = set_sig().get_untracked();
        let (Some(a), Some(b)) = (s.iter().position(|x| *x == from), s.iter().position(|x| *x == drop_item)) else { return };
        commit(moved(&s, a, b));
    };
    let gone = item.clone();
    let scope = Memo::new(move |_| match col.get() {
        Col::Shown { scope, .. } => scope,
        _ => String::new(),
    });
    let item_w = item.clone();
    let state = Memo::new(move |_| match col.get() {
        Col::Loading => Some(format!("{}…", item_w.key())),
        Col::Failed(why) => Some(why.text()),
        Col::Shown { .. } => None,
    });
    let failed = move || matches!(col.get(), Col::Failed(_));
    // APP-46: a look of the page's own body carries its box's bar — in a
    // decision, the option's name, mark and *pick this* (its answer path and
    // gesture check, unchanged); and *comment*.
    let boxed = {
        let item = item.clone();
        Memo::new(move |_| {
            BOXREV.with(|r| r.track());
            item.mock.and_then(|n| mockup::box_of(&item.ticket, n))
        })
    };
    let bar_ticket = item.ticket.clone();
    let acts = move || {
        boxed.get().map(|(k, c)| {
            let title = caption.get_untracked().title;
            let bar = crate::mockpick::column_bar(k, Some(bar_ticket.clone()), c, title);
            view! { <div class="cmp-acts framed-bar">{bar.run()}</div> }
        })
    };
    view! {
        <section class="cmp-col" style=style class:hidden=hidden data-compare-item=key.clone() aria-label=move || format!("{} — {}", key, title())
            on:dragover=over on:drop=dropped>
            <header class="cmp-head" tabindex="0" draggable="true" title="Drag, or ← → with this head focused, to move it"
                aria-roledescription="movable column" on:keydown=head_keys on:dragstart=start on:dragend=move |_| dragging.set(None)>
                <span class="cmp-grip" aria-hidden="true">"⋮⋮"</span>
                <span class="cmp-title">{title}</span>
                <span class="cmp-at code">{item.key()}</span>
                <button class="cmp-x" title="Drop it from the comparison" aria-label=format!("Drop {} from the comparison", item.key())
                    on:click=move |_| commit(set_sig().get_untracked().into_iter().filter(|x| *x != gone).collect())>"×"</button>
            </header>
            {acts}
            {move || caption.get().description.map(|d| {
                let html = md::render_body(&d).html;
                view! { <div class="cmp-caption md" inner_html=html></div> }
            })}
            <p class="cmp-state" class:failed=failed hidden=move || state.with(Option::is_none)>{move || state.get()}</p>
            {move || shown.get().then(|| {
                let scope = scope.get_untracked();
                view! {
                    <div class="cmp-screen" style=screen>
                        <div class="cmp-inner" style=inner>
                            <Frame html ticket=None scope emulate fixed=true />
                        </div>
                    </div>
                }
            })}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i(s: &str) -> Item {
        Item::parse(s).unwrap()
    }

    /// A member is `T#mN` or `T`, however it was spelled; anything else is
    /// none.
    #[test]
    fn a_member_is_a_ticket_and_its_mockup_number() {
        assert_eq!(i("APP-37#m1"), Item { ticket: "APP-37".into(), mock: Some(1) });
        assert_eq!(i(" app-37 #M3 "), Item { ticket: "APP-37".into(), mock: Some(3) });
        assert_eq!(i("APP-37%23m6"), Item { ticket: "APP-37".into(), mock: Some(6) });
        assert_eq!(i("design-49"), Item { ticket: "DESIGN-49".into(), mock: None });
        assert_eq!(i("APP-37#m12").key(), "APP-37#m12");
        assert_eq!(i("DESIGN-49").key(), "DESIGN-49");
        for bad in ["", "APP", "APP-37#", "APP-37#x1", "APP-37#m0", "APP-37#m", "APP-37#m1000", "#m1", "APP-37#m-1"] {
            assert_eq!(Item::parse(bad), None, "{bad:?}");
        }
    }

    /// Compare toggles a member in and out, at the end, at most eight.
    #[test]
    fn the_set_toggles_adds_and_holds_its_cap() {
        let a = i("APP-37#m1");
        let b = i("APP-37#m3");
        let s = toggled(&[], &a);
        assert_eq!(s, vec![a.clone()]);
        let s = toggled(&s, &b);
        assert_eq!(s, vec![a.clone(), b.clone()]);
        assert_eq!(toggled(&s, &a), vec![b.clone()]);
        assert_eq!(added(&s, &a), s, "already in");
        let full: Vec<Item> = (1..=MAX as u32).map(|n| Item { ticket: "APP-37".into(), mock: Some(n) }).collect();
        assert_eq!(added(&full, &i("DESIGN-49")), full);
        assert_eq!(toggled(&full, &i("DESIGN-49")).len(), MAX);
        assert_eq!(toggled(&full, &full[0]).len(), MAX - 1);
    }

    /// Drag moves a member to where it was dropped; ←/→ one place, never
    /// past an end, answering where it went.
    #[test]
    fn columns_reorder_by_drag_and_by_arrows() {
        let s: Vec<Item> = ["A-1#m1", "A-1#m2", "A-1#m3", "A-1#m4"].iter().map(|x| i(x)).collect();
        let keys = |v: &[Item]| v.iter().map(|x| x.mock.unwrap()).collect::<Vec<_>>();
        assert_eq!(keys(&moved(&s, 0, 2)), [2, 3, 1, 4]);
        assert_eq!(keys(&moved(&s, 3, 0)), [4, 1, 2, 3]);
        assert_eq!(keys(&moved(&s, 1, 99)), [1, 3, 4, 2]);
        assert_eq!(keys(&moved(&s, 9, 0)), [1, 2, 3, 4]);
        let (v, at) = nudged(&s, 1, 1);
        assert_eq!((keys(&v), at), (vec![1, 3, 2, 4], 2));
        let (v, at) = nudged(&s, 1, -1);
        assert_eq!((keys(&v), at), (vec![2, 1, 3, 4], 0));
        let (v, at) = nudged(&s, 0, -1);
        assert_eq!((keys(&v), at), (vec![1, 2, 3, 4], 0));
        let (v, at) = nudged(&s, 3, 1);
        assert_eq!((keys(&v), at), (vec![1, 2, 3, 4], 3));
        assert_eq!(nudged(&[], 0, 1), (vec![], 0));
    }

    /// Kept and routed as `T#mN,…`; read back tolerant — each once, in
    /// order, at most eight, the unreadable dropped.
    #[test]
    fn the_set_is_kept_as_a_list_of_names() {
        let s = vec![i("APP-37#m1"), i("APP-37#m3"), i("DESIGN-49")];
        assert_eq!(encode(&s), "APP-37#m1,APP-37#m3,DESIGN-49");
        assert_eq!(decode(&encode(&s)), s);
        assert_eq!(decode("APP-37%23m1%2CAPP-37%23m3"), s[..2].to_vec());
        assert_eq!(decode("APP-37#m1,,garbage,APP-37#m1,app-37#m3"), s[..2].to_vec());
        assert_eq!(decode(""), vec![]);
        let many = (1..=12).map(|n| format!("A-1#m{n}")).collect::<Vec<_>>().join(",");
        assert_eq!(decode(&many).len(), MAX);
        assert_eq!(storage_key("app-37"), "menger.compare:APP-37");
        assert_eq!(pill_text(3), "compare 3");
    }

    /// The route carries the set as `?compare=…`, beside the rest of the
    /// query, and loses it when the view closes.
    #[test]
    fn the_route_carries_the_comparison() {
        let s = vec![i("APP-37#m1"), i("APP-37#m3")];
        assert_eq!(from_hash("#/ticket/APP-37?compare=APP-37#m1,APP-37#m3"), Some(s.clone()));
        assert_eq!(from_hash("#/ticket/APP-37?view=full&compare=APP-37%23m1,APP-37%23m3&g=1"), Some(s.clone()));
        assert_eq!(from_hash("#/ticket/APP-37?compare="), Some(vec![]));
        assert_eq!(from_hash("#/ticket/APP-37"), None);
        assert_eq!(from_hash("#/ticket/APP-37?view=full"), None);
        assert_eq!(with_hash("#/ticket/APP-37", Some(&s)), "#/ticket/APP-37?compare=APP-37#m1,APP-37#m3");
        assert_eq!(with_hash("#/ticket/APP-37?view=full", Some(&s)), "#/ticket/APP-37?view=full&compare=APP-37#m1,APP-37#m3");
        assert_eq!(with_hash("#/ticket/APP-37?compare=A-1#m1&field=x", Some(&s)), "#/ticket/APP-37?field=x&compare=APP-37#m1,APP-37#m3");
        assert_eq!(with_hash("#/ticket/APP-37?field=x&compare=APP-37#m1", None), "#/ticket/APP-37?field=x");
        assert_eq!(with_hash("#/ticket/APP-37?compare=APP-37#m1", Some(&[])), "#/ticket/APP-37");
        // Round trip.
        assert_eq!(from_hash(&with_hash("#/ticket/APP-37?f=1", Some(&s))), Some(s));
    }

    /// A member that is not there is a named line — the number past the
    /// body's says how many it holds.
    #[test]
    fn a_missing_member_is_a_named_state() {
        let p = |t: &str| Posted { title: Some(t.into()), ..Posted::default() };
        let posted = vec![p("one"), p("two")];
        assert_eq!(nth("APP-37", &posted, 2).unwrap().title.as_deref(), Some("two"));
        assert_eq!(nth("APP-37", &posted, 9).unwrap_err(), Why::NoMock { ticket: "APP-37".into(), n: 9, have: 2 });
        assert_eq!(nth("APP-37", &posted, 0).unwrap_err().text(), "APP-37 has no mockup 0 — its body holds 2");
        assert_eq!(nth("APP-37", &posted[..1], 3).unwrap_err().text(), "APP-37 has no mockup 3 — its body holds one");
        assert_eq!(nth("APP-37", &[], 1).unwrap_err().text(), "APP-37 has no mockup 1 — its body holds none");
        assert_eq!(Why::NoTicket("SECRET-3".into()).text(), "SECRET-3: no ticket you can read");
        assert_eq!(Why::Unreadable("SECRET-3".into()).text(), "you cannot read SECRET-3");
        assert!(Why::NotPage("DESIGN-49".into()).text().starts_with("DESIGN-49 is no whole HTML page"));
    }

    /// The label: a title, else the description's first line, else its
    /// number — clipped.
    #[test]
    fn a_mockup_is_labelled_by_its_caption() {
        assert_eq!(label(&Caption::of(Some("Checkout"), Some("the cart")), Some(2)), "Checkout");
        assert_eq!(label(&Caption::of(None, Some("\n The bar, on the board \nmore")), Some(1)), "The bar, on the board");
        assert_eq!(label(&Caption::default(), Some(4)), "mockup 4");
        assert_eq!(label(&Caption::default(), None), "the page");
        let long = "x".repeat(LABEL_MAX + 10);
        assert_eq!(label(&Caption::of(Some(&long), None), None).chars().count(), LABEL_MAX + 1);
    }

    /// Columns share the stage, at least 300 px each (then it scrolls); the
    /// device fits its column's width and the stage's height; a phone's
    /// stage is one column.
    #[test]
    fn columns_share_the_stage_then_scroll() {
        let phone = Device { w: 390, h: 844 };
        let desk = Device { w: 1440, h: 900 };
        // Two phones in a 1400 × 860 stage: 692 px each, the height decides.
        let l = layout(1400.0, 860.0, 2, phone);
        assert!((l.col_w - 692.0).abs() < 1e-9 && !l.scrolls && !l.single, "{l:?}");
        assert!((l.scale - (860.0 - COL_CHROME) / 844.0).abs() < 1e-9, "{l:?}");
        // Four desktops: 338 px each, the width decides.
        let l = layout(1400.0, 860.0, 4, desk);
        assert!((l.col_w - 338.0).abs() < 1e-9 && !l.scrolls);
        assert!((l.scale - (338.0 - SCREEN_EDGE) / 1440.0).abs() < 1e-9);
        // Eight: each at the floor, the row scrolls sideways.
        let l = layout(1400.0, 860.0, 8, phone);
        assert_eq!(l.col_w, MIN_COL);
        assert!(l.scrolls);
        // Never larger than life.
        assert_eq!(layout(3000.0, 2000.0, 1, phone).scale, 1.0);
        // A phone's stage: one column, the whole width, no scrolling.
        let l = layout(358.0, 700.0, 4, phone);
        assert!(l.single && !l.scrolls && l.col_w == 358.0, "{l:?}");
        // Unmeasured: a sane scale, not a panic.
        let l = layout(0.0, 0.0, 0, phone);
        assert!(l.scale > 0.0 && l.scale <= 1.0);
    }
}
