//! AGS-18: the keyboard layer — the whole app without a mouse.
//!
//! Three pieces live here: the **global key map** (a single `window`
//! keydown listener: `/` and Ctrl/⌘+K open the jump palette, `?` the
//! cheat sheet, `Escape` closes an overlay, `g h` goes to the board, and
//! j/k/Enter move and open board rows), the **two overlays** (jump
//! palette and cheat sheet — real `role="dialog"` boxes that trap Tab and
//! hand focus back where they found it), and the small **roving-tabindex
//! helpers** the nav tree (src/nav.rs) shares with the board.
//!
//! Two rules shape the design:
//!
//! - **Focus is real focus.** Nothing here paints a pretend cursor: the
//!   focused row is the document's `activeElement`, carries the sole
//!   `tabindex="0"` of its widget (WAI-ARIA roving tabindex), and wears a
//!   token-painted ring that is visibly distinct from the *selected* row
//!   (which is the route's ticket, tinted with an inset accent bar).
//!   That is also why the tab stop is moved imperatively rather than
//!   through the reactive graph: which row has focus is a property of the
//!   document, not of the ticket store, and keeping it out of the signals
//!   means a focus move patches two attributes instead of re-running an
//!   attribute closure per row.
//! - **Keys never fire under someone else's cursor.** The global listener
//!   stands down inside an `<input>`/`<textarea>`/`<select>`/
//!   contenteditable, inside a `[role=dialog]` (the overlays own their
//!   keys) and inside a `[role=menu]` (AGS-16's ticket menu owns its
//!   own arrows/Enter/Escape).
//!
//! "Menu key / Shift+F10 opens the context menu on the focused ticket" —
//! AGS-18's one-line follow-up — landed with AGS-20 where it belongs, in
//! AGS-16's menu module (src/menu.rs); the focus this module owns is what
//! it opens on, and [`remember_focus`] / [`restore_focus`] are what hand
//! it back.
//!
//! MFE-12: the board mounts a window, so "the rows" are no longer the DOM.
//! j/k walk the row store's order — the board's order by construction —
//! and focus the next row where it is mounted; where it is not (`k` from
//! nowhere is the last ticket, ten thousand rows down), the board scrolls
//! it in and it takes focus once the window has drawn it
//! ([`focus_board_row`]). A focused row that the window leaves behind (a
//! wheel scroll, a scrollbar drag) takes the focus with it, so the board
//! remembers which row that was, and j/k resume there rather than at the
//! top. The palette was always the store's, not the DOM's.
//!
//! SCALE-25: **the palette asks the server.** It filtered every row the
//! client held on every keystroke — right only once the whole store had
//! been walked, and the last reader of "every row" besides the board. Now it
//! asks `tickets.find` (SCALE-6), the palette's own rule answered by the
//! server ([`palette_match`] stays here, unchanged, as the rule's statement
//! and the server's test oracle): the head of the store when it opens
//! (`find { query: "" }`), then each query once the typing pauses
//! ([`DEBOUNCE_MS`]), the 50-row cap as `find`'s `limit`, and the count as
//! the answer's rows plus its `remaining`. Every question is numbered and
//! only the answer to the newest is shown ([`Found`]), so an answer that
//! comes back late never replaces a newer one. While a question is out the
//! list shows what the last answer has that the new query still matches —
//! exactly the head of the new answer when the query only grew ([`narrows`]),
//! nothing otherwise — and Enter on an empty list waits for the answer and
//! opens its first row. With no connection it says so and shows no matches.
//! Its rows are its own: they are not put in the row cache, and the ticket
//! opened from one is fetched by its page, as any.
//!
//! SCALE-26: **the board's order is arithmetic, and its rows may be on their
//! way.** j/k walked the board's row list — every ticket the walk had found.
//! There is no such list now: the board holds a window (src/board.rs), and a
//! position is a ticket by `extents` alone. So a key steps a *position* — the
//! focused row's, by its id — and focuses the ticket at the next one, which
//! the window may not hold yet: the board scrolls it in, asks for it, and the
//! row takes focus when it is drawn ([`focus_board_row`], polled for up to
//! three seconds). Presses made meanwhile step on from that row, not from the
//! one still focused, so a held-down j crosses the window's edge without
//! losing a press. Home and End, once a board row has focus (or the window
//! left it behind), go to the board's first and last rows the same way —
//! before, they were the page's own scroll, which carried the focused row out
//! of the window and dropped the focus.

use std::cell::{Cell, RefCell};

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::{board, dom, navigate_to_ticket, session, st_class};
use menger_tickets_api::{TicketSummary, TicketsEvent};

/// How long a `g` stays armed as the first key of a `g …` sequence.
const GO_WINDOW_MS: f64 = 1500.0;

/// How many matches the jump palette renders at once. The list is
/// rebuilt per keystroke, so this is a rendering budget, not a filter
/// limit anyone can observe on a store of menger's size. SCALE-25: and the
/// `limit` it asks `find` for.
const PALETTE_MAX: usize = 50;

/// SCALE-25: how long the typing must pause before the palette asks `find`
/// — short enough to feel immediate, long enough that a word typed at speed
/// is one question, not one per letter (a non-empty `find` counts its
/// matches across the store: 52–155 ms at 10^6, SCALE-21).
pub(crate) const DEBOUNCE_MS: i32 = 80;

/// Which overlay is up. Exactly one at a time — the signal *is* the
/// mutual exclusion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Overlay {
    None,
    Palette,
    Cheats,
}

/// The cheat sheet's rows, and the app's key map in one place: what `?`
/// shows is this table, so the documentation cannot drift from a
/// separate hand-written list (a native test pins that every key the
/// handlers implement appears here).
pub(crate) const SHORTCUTS: &[(&str, &str)] = &[
    ("Tab", "into the nav tree, the board rows, and the controls"),
    ("j / ↓", "next row (nav tree, board)"),
    ("k / ↑", "previous row"),
    ("l / →", "expand a group, or step into it"),
    ("h / ←", "collapse a group, or step out to it"),
    ("Home / End", "first / last row (nav tree, board)"),
    ("Enter", "open the focused ticket"),
    ("/ or Ctrl+K", "jump to a ticket (⌘K on a Mac)"),
    ("g h", "go to the board"),
    ("?", "this list"),
    ("Esc", "close the palette or this list"),
];

// ---------------------------------------------------------------------
// Matching (pure — the part that is worth a native test)

/// The jump palette's filter: a case-insensitive substring of the code
/// or the title (v1, as specified). Case folding is ASCII — ticket codes
/// are ASCII by construction and the Unicode case tables cost the wasm
/// ~7 KiB (AGS-15 removed the last one). An empty query matches
/// everything, so the palette opens on the whole store.
pub(crate) fn palette_match(code: &str, title: &str, query: &str) -> bool {
    let q = query.trim();
    q.is_empty() || contains_ignore_ascii_case(code, q) || contains_ignore_ascii_case(title, q)
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    // UTF-8 is self-synchronizing, so a byte-window scan cannot match
    // across a character boundary by accident.
    n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// SCALE-25: whether every ticket `query` matches is one `before` matched —
/// true when the query only grew (the trimmed `before` is inside the trimmed
/// `query`). Then the rows of `before`'s answer that `query` matches are
/// the head of `query`'s own answer, in order: a match of `query` that sorts
/// before the last row `before` answered with was a match of `before`, so it
/// is among those rows.
pub(crate) fn narrows(before: &str, query: &str) -> bool {
    // An empty `before` (the palette's opening question) matched everything;
    // the scan itself is never handed an empty needle.
    let before = before.trim();
    before.is_empty() || contains_ignore_ascii_case(query.trim(), before)
}

/// SCALE-25: what the palette shows — the newest answer it took, and
/// whether a newer question is out. Questions are numbered as they are
/// asked; an answer is taken only if it is to the newest, so one that comes
/// back after a newer question was asked is dropped, whatever order the
/// server answers in.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Found {
    /// The query the rows answer — `None` before any answer.
    pub(crate) query: Option<String>,
    pub(crate) rows: Vec<TicketSummary>,
    /// The answer's rows plus its `remaining`: every match.
    pub(crate) total: u64,
    /// The newest question's number, and the shown answer's.
    pub(crate) asked: u32,
    pub(crate) took: u32,
    /// The newest question found no connection: nothing is shown as current.
    pub(crate) offline: bool,
}

impl Found {
    /// A new question; its number.
    pub(crate) fn ask(&mut self) -> u32 {
        self.asked = self.asked.wrapping_add(1);
        self.asked
    }

    /// Whether an answer to the newest question is still to come.
    pub(crate) fn pending(&self) -> bool {
        self.took != self.asked && !self.offline
    }

    /// The answer to question `n`: taken if `n` is still the newest.
    pub(crate) fn take(&mut self, n: u32, query: String, rows: Vec<TicketSummary>, remaining: u64) -> bool {
        if n != self.asked {
            return false;
        }
        self.total = rows.len() as u64 + remaining;
        self.query = Some(query);
        self.rows = rows;
        self.took = n;
        self.offline = false;
        true
    }

    /// Question `n` found no connection.
    pub(crate) fn lost(&mut self, n: u32) {
        if n == self.asked {
            self.offline = true;
        }
    }

    /// The rows to show for what is typed now: the answer, once it is the
    /// newest question's; while one is out, the rows of the last answer the
    /// typed query still matches if it only grew from that answer's query
    /// (the head of the coming answer), else none; offline, none.
    pub(crate) fn shown(&self, typed: &str) -> Vec<TicketSummary> {
        if self.offline {
            return Vec::new();
        }
        if !self.pending() {
            return self.rows.clone();
        }
        match &self.query {
            Some(q) if narrows(q, typed) => self
                .rows
                .iter()
                .filter(|t| palette_match(&t.id, &t.title, typed))
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------
// Focus plumbing shared with the nav tree and the board

/// Every element matching `selector`, in document order.
pub(crate) fn query_all(selector: &str) -> Vec<web_sys::HtmlElement> {
    let Some(doc) = dom::window().document() else {
        return Vec::new();
    };
    let Ok(list) = doc.query_selector_all(selector) else {
        return Vec::new();
    };
    (0..list.length())
        .filter_map(|i| list.get(i).and_then(|n| n.dyn_into().ok()))
        .collect()
}

/// Rendered (not inside a collapsed group, not behind `#app.nonav`).
pub(crate) fn is_visible(el: &web_sys::HtmlElement) -> bool {
    el.offset_parent().is_some()
}

/// Focus a row and bring it into view — `nearest`, so a row that is
/// already on screen never jumps.
pub(crate) fn focus_row(el: &web_sys::HtmlElement) {
    let _ = el.focus();
    let opts = web_sys::ScrollIntoViewOptions::new();
    opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
    el.scroll_into_view_with_scroll_into_view_options(&opts);
}

/// Roving tabindex: `el` becomes the widget's single tab stop (all of
/// `all` are the widget's items), then takes focus.
pub(crate) fn focus_and_rove(all: &[web_sys::HtmlElement], el: &web_sys::HtmlElement) {
    rove(all, el);
    focus_row(el);
}

/// The tab stop only — no focus move (used to seed a freshly rendered
/// widget so Tab can reach it).
pub(crate) fn rove(all: &[web_sys::HtmlElement], el: &web_sys::HtmlElement) {
    for other in all {
        let stop = if other.is_same_node(Some(el)) { "0" } else { "-1" };
        let _ = other.set_attribute("tabindex", stop);
    }
}

/// Give a widget a tab stop if it has none *visible* — the selected row
/// when it is on screen, else the first row. Called a tick after the
/// rows change, so Tab always lands somewhere sensible, and a tab stop
/// that scrolled out of existence (its group collapsed, its row gone) is
/// replaced rather than stranded.
pub(crate) fn ensure_tab_stop(item_selector: &str, preferred: Option<&str>) {
    let all = query_all(item_selector);
    let visible: Vec<web_sys::HtmlElement> = all.iter().filter(|el| is_visible(el)).cloned().collect();
    let Some(first) = visible.first() else { return };
    if visible
        .iter()
        .any(|el| el.get_attribute("tabindex").as_deref() == Some("0"))
    {
        return;
    }
    let pick = preferred
        .map(query_all)
        .and_then(|els| els.into_iter().find(is_visible))
        .unwrap_or_else(|| first.clone());
    rove(&all, &pick);
}

// ---------------------------------------------------------------------
// Overlay state: opening remembers where focus was, closing hands it back

thread_local! {
    /// When the `g` of a `g …` sequence was pressed (ms since epoch).
    static GO_ARMED_AT: Cell<f64> = const { Cell::new(f64::NEG_INFINITY) };
    /// Where focus was when the overlay opened.
    static FOCUS_BEFORE_OVERLAY: RefCell<Option<web_sys::HtmlElement>> = const { RefCell::new(None) };
    /// MFE-12: the board row that last had focus, until focus lands
    /// anywhere else — what j/k resume from when the window has unmounted
    /// that row (and focus fell to `<body>` with it).
    static BOARD_FOCUS: RefCell<Option<String>> = const { RefCell::new(None) };

    /// SCALE-26: the board row a step is on its way to (not drawn yet), and
    /// when the step was taken — where the next press steps from.
    static BOARD_TARGET: RefCell<Option<(String, f64)>> = const { RefCell::new(None) };

    /// SCALE-26: the focused board row's element, and the one observer that
    /// gives it focus back when the window's `<For>` moves it ([`keep_board_focus`]).
    static BOARD_FOCUS_EL: RefCell<Option<web_sys::HtmlElement>> = const { RefCell::new(None) };
    static BOARD_OBSERVER: RefCell<Option<web_sys::MutationObserver>> = const { RefCell::new(None) };
    /// SCALE-26: the observer is handing focus back — not a new focus, so a
    /// step on its way stays on its way.
    static REFOCUS: Cell<bool> = const { Cell::new(false) };
}

/// How long a step waits for its row to be drawn: the scroll's frame, then
/// the window's page (a few ms at 10^6, more over a slow link).
const BOARD_STEP_WAIT_MS: f64 = 3000.0;

/// MFE-12: a board row took focus (the board's `focusin`). SCALE-26: a step
/// on its way ends there — it landed, or a click went somewhere else — and
/// the row's element is what [`keep_board_focus`] gives focus back to.
pub(crate) fn note_board_focus(id: String, row: Option<web_sys::HtmlElement>) {
    if !REFOCUS.with(Cell::get) {
        BOARD_TARGET.with(|t| *t.borrow_mut() = None);
    }
    if row.is_some() {
        BOARD_FOCUS_EL.with(|f| *f.borrow_mut() = row);
    }
    BOARD_FOCUS.with(|c| *c.borrow_mut() = Some(id));
}

/// SCALE-26: focus stays on the board's row while its window moves — the
/// nav's `keep_focus` (SCALE-24), for the board. When the window grows at its
/// top (a step up across its edge, a scroll back) the keyed `<For>` puts the
/// rows that stay back behind the new ones, and an element moved in the DOM
/// loses focus: the next j/k would start from nowhere. So the board's
/// `<tbody>` is watched, and a focused row that a mutation batch moved while
/// focus fell to `<body>` gets it back, without scrolling, in the observer's
/// microtask — before the next keystroke. A row blurred any other way (a
/// click elsewhere, `blur()`) is not moved, and stays blurred. One observer
/// for the app's lifetime, pointed at each `<tbody>` the board mounts.
pub(crate) fn keep_board_focus(tbody: &web_sys::Element) {
    let obs = BOARD_OBSERVER.with(|o| {
        o.borrow_mut()
            .get_or_insert_with(|| {
                let cb = Closure::<dyn FnMut(js_sys::Array, web_sys::MutationObserver)>::new(
                    |records: js_sys::Array, _: web_sys::MutationObserver| {
                        let Some(el) = BOARD_FOCUS_EL.with(|f| f.borrow().clone()) else { return };
                        let Some(doc) = dom::window().document() else { return };
                        let resting = doc.active_element().is_none_or(|a| a.tag_name() == "BODY");
                        if !resting || !el.is_connected() {
                            return;
                        }
                        // Taken out and put back — in one batch, or in two
                        // (the second only adds it).
                        let moved = records.iter().any(|r| {
                            let Ok(r) = r.dyn_into::<web_sys::MutationRecord>() else { return false };
                            [r.removed_nodes(), r.added_nodes()]
                                .iter()
                                .any(|nodes| (0..nodes.length()).filter_map(|i| nodes.get(i)).any(|n| n.is_same_node(Some(&el))))
                        });
                        if moved {
                            let opts = web_sys::FocusOptions::new();
                            opts.set_prevent_scroll(true);
                            REFOCUS.with(|r| r.set(true));
                            let _ = el.focus_with_options(&opts);
                            REFOCUS.with(|r| r.set(false));
                        }
                    },
                );
                let obs = web_sys::MutationObserver::new(cb.as_ref().unchecked_ref()).expect("MutationObserver");
                cb.forget(); // lives as long as the app
                obs
            })
            .clone()
    });
    let init = web_sys::MutationObserverInit::new();
    init.set_child_list(true);
    let _ = obs.observe_with_options(tbody, &init);
}

/// SCALE-26: the row a step is still on its way to, if one is.
fn board_target() -> Option<String> {
    let now = js_sys::Date::now();
    BOARD_TARGET.with(|t| t.borrow().as_ref().filter(|(_, at)| now - at < BOARD_STEP_WAIT_MS).map(|(id, _)| id.clone()))
}

/// Remember where focus is, so [`restore_focus`] can hand it back when
/// whatever took it goes away. Shared by the overlays and (AGS-20) the
/// keyboard-opened ticket menu: the two are mutually exclusive by
/// construction — the global map stands down inside `[role=menu]`, and
/// the menu's own opener stands down inside `[role=dialog]` — so one slot
/// is enough.
pub(crate) fn remember_focus() {
    let el = dom::window()
        .document()
        .and_then(|d| d.active_element())
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    FOCUS_BEFORE_OVERLAY.with(|c| *c.borrow_mut() = el);
}

/// Hand focus back, a tick later — the dialog (or menu) has to leave the
/// DOM first, or the browser's own "focus fell out of a removed subtree"
/// fixup lands on `<body>` after us. A no-op when nothing was remembered.
pub(crate) fn restore_focus() {
    let el = FOCUS_BEFORE_OVERLAY.with(|c| c.borrow_mut().take());
    spawn_local(async move {
        dom::sleep_ms(0).await;
        let Some(el) = el else { return };
        if el.is_connected() {
            focus_row(&el);
        } else if let Some(id) = el
            .get_attribute("data-ticket-id")
            .filter(|_| el.tag_name() == "TR")
        {
            // MFE-12: a board row the window unmounted while the overlay
            // (or the menu) was up. The same ticket's row, if the window
            // has it again; otherwise j/k resume from it.
            note_board_focus(id.clone(), None);
            if let Some(row) = mounted_board_row(&query_all("#board tr[data-ticket-id]"), &id) {
                focus_row(row);
            }
        }
    });
}

pub(crate) fn open_overlay(overlay: RwSignal<Overlay>, which: Overlay) {
    if overlay.get_untracked() == Overlay::None {
        remember_focus();
    }
    overlay.set(which);
}

pub(crate) fn close_overlay(overlay: RwSignal<Overlay>) {
    if overlay.get_untracked() == Overlay::None {
        return;
    }
    overlay.set(Overlay::None);
    restore_focus();
}

// ---------------------------------------------------------------------
// The global listener

/// `g …` sequences. Returns true when the key was swallowed by the
/// sequence — either arming it (`g`) or completing it (`g h` → the
/// board). The nav tree asks first, so `h` keeps its collapse meaning
/// there while `g h` still works from inside the tree.
pub(crate) fn consume_go_prefix(key: &str) -> bool {
    let now = js_sys::Date::now();
    let armed = GO_ARMED_AT.with(|c| now - c.get() < GO_WINDOW_MS);
    match key {
        "g" => {
            GO_ARMED_AT.with(|c| c.set(now));
            true
        }
        "h" if armed => {
            GO_ARMED_AT.with(|c| c.set(f64::NEG_INFINITY));
            let _ = dom::window().location().set_hash("/");
            true
        }
        _ => {
            if armed {
                GO_ARMED_AT.with(|c| c.set(f64::NEG_INFINITY));
            }
            false
        }
    }
}

/// One `window` keydown listener for the app's lifetime — the same
/// shape as the router's `hashchange`. MFE-12: it holds the row store,
/// which is what the board's j/k walk now. SCALE-26: the board's model —
/// positions by `extents`, not a list of rows.
pub(crate) fn install(overlay: RwSignal<Overlay>, board: board::Board) {
    let cb = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        global_keydown(&ev, overlay, board);
    });
    dom::window()
        .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())
        .expect("keydown listener");
    cb.forget(); // lives as long as the app

    // MFE-12: focus landing anywhere but on a board row ends the board's
    // claim to it (`BOARD_FOCUS`); a row of the board renews it itself.
    let on_focus = Closure::<dyn FnMut(web_sys::Event)>::new(|ev: web_sys::Event| {
        let on_board = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest("#board tr[data-ticket-id]").ok().flatten())
            .is_some();
        if !on_board {
            BOARD_FOCUS.with(|c| *c.borrow_mut() = None);
            BOARD_TARGET.with(|t| *t.borrow_mut() = None);
            BOARD_FOCUS_EL.with(|f| *f.borrow_mut() = None);
        }
    });
    if let Some(doc) = dom::window().document() {
        let _ = doc.add_event_listener_with_callback("focusin", on_focus.as_ref().unchecked_ref());
    }
    on_focus.forget(); // lives as long as the app
}

/// True where a keystroke belongs to whatever already has focus: a text
/// field, a `<select>`, a dialog (the overlays run their own key maps),
/// or a menu (AGS-16).
fn defer_to_focus(el: &web_sys::Element) -> bool {
    matches!(
        el.tag_name().as_str(),
        "INPUT" | "TEXTAREA" | "SELECT" | "OPTION"
    ) || el
        .closest("[role=dialog], [role=menu], [contenteditable='true']")
        .ok()
        .flatten()
        .is_some()
}

fn global_keydown(ev: &web_sys::KeyboardEvent, overlay: RwSignal<Overlay>, board: board::Board) {
    if ev.default_prevented() || ev.is_composing() {
        return;
    }
    let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
    let key = ev.key();
    let cmd = ev.ctrl_key() || ev.meta_key();

    // Escape reaches here only when focus escaped the dialog (a click on
    // the scrim, a removed row); the dialog's own handler is the normal
    // path. Cheap safety net, so Escape always closes an overlay.
    if key == "Escape" && overlay.get_untracked() != Overlay::None {
        ev.prevent_default();
        close_overlay(overlay);
        return;
    }
    if let Some(el) = &target {
        if defer_to_focus(el) {
            return;
        }
    }
    if overlay.get_untracked() != Overlay::None {
        return;
    }
    if cmd {
        // ⌘K / Ctrl+K is the palette; every other chord stays the
        // browser's (Ctrl+R, ⌘L, …).
        if key.eq_ignore_ascii_case("k") {
            ev.prevent_default();
            open_overlay(overlay, Overlay::Palette);
        }
        return;
    }
    if ev.alt_key() {
        return;
    }
    match key.as_str() {
        "/" => {
            ev.prevent_default(); // Firefox's quick-find would eat it
            open_overlay(overlay, Overlay::Palette);
            return;
        }
        "?" => {
            ev.prevent_default();
            open_overlay(overlay, Overlay::Cheats);
            return;
        }
        _ => {}
    }
    if consume_go_prefix(&key) {
        ev.prevent_default();
        return;
    }
    if board_key(&key, target.as_ref(), board) {
        ev.prevent_default();
    }
}

/// The mounted board row for ticket `id`, if the window holds it.
fn mounted_board_row<'a>(mounted: &'a [web_sys::HtmlElement], id: &str) -> Option<&'a web_sys::HtmlElement> {
    mounted
        .iter()
        .find(|r| r.get_attribute("data-ticket-id").as_deref() == Some(id))
}

/// Board rows: j/k (and, once a row has focus, ↑/↓) move; Enter opens.
/// Returns true when the key was used. MFE-12: the order walked is the
/// row store's — the board's order by construction, so nothing is
/// mirrored into a second structure — not the DOM's, which holds only the
/// window; the row a key lands on may not be mounted yet, and
/// [`focus_board_row`] brings it in. SCALE-26: the order is the extents'
/// (a row's position is its id's), the row may not be fetched yet either, a
/// step still on its way is where the next one starts, and Home/End are the
/// board's ends once a row has focus.
fn board_key(key: &str, target: Option<&web_sys::Element>, board: board::Board) -> bool {
    // SCALE-26: whether the board is on screen is its `<tbody>`, not its
    // mounted rows — a window run ahead of its pages (k held down past what
    // has been fetched) mounts none for a moment, and the presses made then
    // are steps like any other.
    if dom::window().document().and_then(|d| d.get_element_by_id("board")).is_none() {
        return false; // no board on screen
    }
    let mounted = query_all("#board tr[data-ticket-id]");
    let focused = target
        .and_then(|t| t.closest("#board tr[data-ticket-id]").ok().flatten())
        .and_then(|el| el.get_attribute("data-ticket-id"));
    // Focus on nothing (`<body>`) because the window unmounted the
    // focused row: the letters carry on from that row. A row still
    // mounted that lost focus to a click elsewhere is the plain "no row
    // focused" case, as before.
    let resting = target.is_none_or(|t| matches!(t.tag_name().as_str(), "BODY" | "HTML"));
    let parked = BOARD_FOCUS
        .with(|c| c.borrow().clone())
        .filter(|id| resting && mounted_board_row(&mounted, id).is_none());
    // A step on its way (a click into a row, or focus anywhere else, ends it).
    let pending = board_target();
    let n = board.n_untracked();
    let at = |id: &Option<String>| id.as_deref().and_then(|id| board.pos_of_untracked(id));
    let cur = at(&pending).or_else(|| at(&focused));
    let resume = cur.or_else(|| at(&parked));
    if n == 0 {
        return false;
    }
    let last = n - 1;
    let next = match key {
        // A letter with no row focused starts at the near end; an arrow
        // with no row focused is the page's own scroll, left alone.
        "j" => Some(resume.map_or(0, |i| (i + 1).min(last))),
        "k" => Some(resume.map_or(last, |i| i.saturating_sub(1))),
        "ArrowDown" => cur.map(|i| (i + 1).min(last)),
        "ArrowUp" => cur.map(|i| i.saturating_sub(1)),
        // SCALE-26: the board's ends, once a row has focus — with none, the
        // page's own Home/End, as before.
        "Home" => resume.map(|_| 0),
        "End" => resume.map(|_| last),
        "Enter" => {
            let Some(id) = focused else { return false };
            navigate_to_ticket(&id);
            return true;
        }
        _ => None,
    };
    match next {
        Some(i) => {
            focus_board_row(&mounted, i, board);
            true
        }
        None => false,
    }
}

/// Focus store row `index` on the board (MFE-12): at once where the window
/// holds it; otherwise scroll it in and focus it when the window has drawn
/// it — polled a frame at a time, because a scroll's event lands on the
/// next frame and the window moves on that event. SCALE-26: and the row may
/// still have to be fetched, so the wait is longer, the row it waits for is
/// remembered (the next press steps on from it), and a newer step ends it.
fn focus_board_row(mounted: &[web_sys::HtmlElement], index: usize, board: board::Board) {
    let Some(id) = board.id_at(index) else { return };
    if let Some(el) = mounted_board_row(mounted, &id) {
        BOARD_TARGET.with(|t| *t.borrow_mut() = None);
        focus_board_el(mounted, el, index, board);
        return;
    }
    let since = js_sys::Date::now();
    BOARD_TARGET.with(|t| *t.borrow_mut() = Some((id.clone(), since)));
    crate::scroll_board_to(index, board.n_untracked(), board.pitch());
    spawn_local(async move {
        while js_sys::Date::now() - since < BOARD_STEP_WAIT_MS {
            dom::sleep_ms(16).await;
            let mine = BOARD_TARGET.with(|t| t.borrow().as_ref().is_some_and(|(want, at)| *want == id && *at == since));
            if !mine {
                return; // a newer step, or a focus elsewhere
            }
            let all = query_all("#board tr[data-ticket-id]");
            if let Some(el) = mounted_board_row(&all, &id) {
                BOARD_TARGET.with(|t| *t.borrow_mut() = None);
                focus_board_el(&all, el, index, board);
                return;
            }
        }
        BOARD_TARGET.with(|t| {
            let mut t = t.borrow_mut();
            if t.as_ref().is_some_and(|(want, at)| *want == id && *at == since) {
                *t = None;
            }
        });
    });
}

/// SCALE-26: a board row takes focus without the browser's own scroll into
/// view, which assumes the rows stay where they are when it scrolls — and on
/// a board laid out above 15 M px they do not (the scroll range is mapped
/// onto the store, MFE-12: a scroll of one pitch moves the rows by ~2.5 at
/// 10^6), so the row it scrolled to was somewhere else, often unmounted, and
/// focus fell to `<body>` (j stopped dead 25 rows down at 10^6, before this
/// build too). The board scrolls itself to the row by its own mapping
/// (`scroll_board_to`, the least that shows it — a no-op when it is on
/// screen), and the row is focused where it is.
fn focus_board_el(all: &[web_sys::HtmlElement], el: &web_sys::HtmlElement, index: usize, board: board::Board) {
    rove(all, el);
    crate::scroll_board_to(index, board.n_untracked(), board.pitch());
    let opts = web_sys::FocusOptions::new();
    opts.set_prevent_scroll(true);
    let _ = el.focus_with_options(&opts);
}

/// Seed the board's tab stop after a render (see [`ensure_tab_stop`]).
/// The board has no selected row: the way in is the row focus last left,
/// while the window holds it, else the first row on screen. MFE-12: not
/// the window's first row — that one sits a margin above the viewport, and
/// Tab would scroll the board out from under the reader to reach it.
pub(crate) fn ensure_board_tab_stop() {
    let all = query_all("#board tr[data-ticket-id]");
    if all.is_empty()
        || all
            .iter()
            .any(|el| el.get_attribute("tabindex").as_deref() == Some("0"))
    {
        return;
    }
    let remembered = BOARD_FOCUS.with(|c| c.borrow().clone());
    let on_screen = || {
        let top = all[0]
            .closest(".content")
            .ok()
            .flatten()
            .map_or(0.0, |s| s.get_bounding_client_rect().top());
        all.iter().find(|el| el.get_bounding_client_rect().bottom() > top + 1.0)
    };
    let pick = remembered
        .and_then(|id| mounted_board_row(&all, &id))
        .or_else(on_screen)
        .unwrap_or(&all[0]);
    rove(&all, pick);
}

// ---------------------------------------------------------------------
// The overlays

/// Keep Tab inside the open dialog (both overlays are modal). Focus
/// cycles through the dialog's own focusable elements and never reaches
/// the page behind it.
fn trap_tab(shift: bool) {
    let Some(doc) = dom::window().document() else { return };
    let Ok(Some(dialog)) = doc.query_selector("[role=dialog]") else {
        return;
    };
    let Ok(list) = dialog.query_selector_all(
        "a[href], button:not([disabled]), input:not([disabled]), select, textarea, [tabindex]:not([tabindex='-1'])",
    ) else {
        return;
    };
    let items: Vec<web_sys::HtmlElement> = (0..list.length())
        .filter_map(|i| list.get(i).and_then(|n| n.dyn_into().ok()))
        .collect();
    if items.is_empty() {
        return;
    }
    let active = doc.active_element();
    let cur = active
        .and_then(|a| items.iter().position(|el| el.is_same_node(Some(&a))))
        .unwrap_or(0);
    let n = items.len();
    let next = if shift { (cur + n - 1) % n } else { (cur + 1) % n };
    let _ = items[next].focus();
}

/// Whichever overlay is up, mounted once beside the app's columns.
#[component]
pub(crate) fn Overlays(overlay: RwSignal<Overlay>) -> impl IntoView {
    view! {
        {move || match overlay.get() {
            Overlay::None => ().into_any(),
            Overlay::Palette => view! { <Palette overlay /> }.into_any(),
            Overlay::Cheats => view! { <Cheats overlay /> }.into_any(),
        }}
    }
}

/// SCALE-25: ask `find` question `n` (`query`), and take the answer if it is
/// still the newest; with no connection, say so. `then` runs when an answer
/// is taken (Enter, pressed while it was on its way).
fn ask_find(found: RwSignal<Found>, n: u32, query: String, then: impl FnOnce() + 'static) {
    spawn_local(async move {
        let Some(client) = session::client() else {
            found.try_update(|f| f.lost(n));
            return;
        };
        match client.find_all(query.clone(), None, Some(PALETTE_MAX as u32)).await {
            Ok(events) => {
                let mut rows = Vec::new();
                let mut remaining = 0;
                for ev in events {
                    match ev {
                        TicketsEvent::Summary { summary } => rows.push(summary),
                        TicketsEvent::Page { remaining: r, .. } => remaining = r,
                        _ => {}
                    }
                }
                if found.try_update(|f| f.take(n, query, rows, remaining)) == Some(true) {
                    then();
                }
            }
            Err(_) => {
                found.try_update(|f| f.lost(n));
            }
        }
    });
}

/// The jump palette: an input over a listbox of matches. The input keeps
/// focus (ARIA combobox pattern — the highlighted row is named by
/// `aria-activedescendant`, never focused), so typing and moving are the
/// same gesture.
#[component]
fn Palette(overlay: RwSignal<Overlay>) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let hi = RwSignal::new(0usize);
    // SCALE-25: the server's answers, numbered; what is shown is derived
    // from the newest one and what is typed now.
    let found = RwSignal::new(Found::default());
    let results = Memo::new(move |_| {
        let q = query.get();
        found.with(|f| f.shown(&q))
    });
    // Enter pressed while the answer was on its way, with nothing to open.
    let enter_waits = StoredValue::new(false);
    let open_at = move |i: usize| {
        let hit = results.with_untracked(|r| r.get(i).map(|t| t.id.clone()));
        if let Some(id) = hit {
            close_overlay(overlay);
            navigate_to_ticket(&id);
        }
    };
    let answered = move || {
        if enter_waits.try_get_value() == Some(true) {
            enter_waits.set_value(false);
            let at = hi.get_untracked();
            let len = results.with_untracked(Vec::len);
            if len > 0 {
                open_at(at.min(len - 1));
            }
        }
    };
    // Opening is the head of the store, asked at once.
    let first = found.try_update(Found::ask).unwrap_or(0);
    ask_find(found, first, String::new(), answered);
    // The connection: lost, the palette says so and shows nothing as
    // current; back, it asks the newest question again.
    let link = session::link();
    Effect::new(move |was: Option<bool>| {
        let up = link.get();
        if !up {
            found.update(|f| {
                let n = f.ask();
                f.lost(n);
            });
        } else if was == Some(false) {
            let n = found.try_update(Found::ask).unwrap_or(0);
            ask_find(found, n, query.get_untracked(), answered);
        }
        up
    });

    let input_ref = NodeRef::<leptos::html::Input>::new();
    input_ref.on_load(|el: web_sys::HtmlInputElement| {
        let _ = el.focus();
    });

    let on_key = move |ev: web_sys::KeyboardEvent| {
        let key = ev.key();
        let len = results.with_untracked(Vec::len);
        let cur = hi.get_untracked().min(len.saturating_sub(1));
        match key.as_str() {
            "Escape" => {
                ev.prevent_default();
                close_overlay(overlay);
            }
            "Enter" => {
                ev.prevent_default();
                if len > 0 {
                    open_at(cur);
                } else if found.with_untracked(Found::pending) {
                    // SCALE-25: the answer is on its way — open its first.
                    enter_waits.set_value(true);
                }
            }
            "ArrowDown" => {
                ev.prevent_default();
                if len > 0 {
                    hi.set((cur + 1) % len);
                }
            }
            "ArrowUp" => {
                ev.prevent_default();
                if len > 0 {
                    hi.set((cur + len - 1) % len);
                }
            }
            "Home" => {
                ev.prevent_default();
                hi.set(0);
            }
            "End" => {
                ev.prevent_default();
                hi.set(len.saturating_sub(1));
            }
            "Tab" => {
                ev.prevent_default();
                trap_tab(ev.shift_key());
            }
            _ => {}
        }
        ev.stop_propagation();
    };

    view! {
        <div class="overlay" on:click=move |_| close_overlay(overlay)>
            <div
                id="palette"
                class="dialog"
                role="dialog"
                aria-modal="true"
                aria-label="Jump to a ticket"
                on:click=|ev| ev.stop_propagation()
                on:keydown=on_key
            >
                <input
                    id="palette-input"
                    node_ref=input_ref
                    type="text"
                    role="combobox"
                    aria-expanded="true"
                    aria-controls="palette-list"
                    aria-autocomplete="list"
                    aria-label="Jump to a ticket"
                    aria-activedescendant=move || format!("pal-{}", hi.get())
                    autocomplete="off"
                    spellcheck="false"
                    placeholder="jump to a ticket — code or title"
                    prop:value=move || query.get()
                    on:input=move |ev| {
                        query.set(event_target_value(&ev));
                        hi.set(0);
                        // SCALE-25: a question per pause in the typing — and
                        // every answer to an earlier one is no longer wanted.
                        let n = found.try_update(Found::ask).unwrap_or(0);
                        spawn_local(async move {
                            dom::sleep_ms(DEBOUNCE_MS).await;
                            if found.try_with_untracked(|f| f.asked) == Some(n) {
                                if let Some(q) = query.try_get_untracked() {
                                    ask_find(found, n, q, answered);
                                }
                            }
                        });
                    }
                />
                <ul
                    id="palette-list"
                    role="listbox"
                    aria-label="matching tickets"
                    aria-busy=move || found.with(Found::pending).to_string()
                >
                    {move || {
                        let shown = results.get();
                        if shown.is_empty() {
                            let word = found.with(|f| {
                                if f.offline {
                                    "offline — reconnecting…"
                                } else if f.pending() {
                                    "searching…"
                                } else {
                                    "no ticket matches"
                                }
                            });
                            return view! { <li class="empty">{word}</li> }.into_any();
                        }
                        let cur = hi.get().min(shown.len() - 1);
                        shown
                            .into_iter()
                            .enumerate()
                            .map(|(i, t)| {
                                let id = t.id.clone();
                                let (title, status) = (t.title.clone(), t.status);
                                view! {
                                    <li
                                        id=format!("pal-{i}")
                                        role="option"
                                        aria-selected=if i == cur { "true" } else { "false" }
                                        class:hi=i == cur
                                        data-pal-id=id.clone()
                                        on:click=move |_| open_at(i)
                                    >
                                        <span class="code">{id.clone()}</span>
                                        <span class="label">{title}</span>
                                        <span class=format!("tag {}", st_class(status.as_str()))>{status.to_string()}</span>
                                    </li>
                                }
                            })
                            .collect_view()
                            .into_any()
                    }}
                </ul>
                <div class="hint">
                    <span id="palette-count">
                        {move || {
                            // SCALE-25: the server's count — the answer's rows
                            // plus its `remaining` — once it is the answer to
                            // what is typed.
                            found.with(|f| {
                                if f.offline {
                                    "offline".to_string()
                                } else if f.pending() {
                                    "… matches".to_string()
                                } else {
                                    let (shown, all) = (f.rows.len() as u64, f.total);
                                    if all > shown {
                                        format!("{shown} of {all} matches")
                                    } else {
                                        format!("{all} matches")
                                    }
                                }
                            })
                        }}
                    </span>
                    " · ↑↓ move · Enter opens · Esc closes"
                </div>
            </div>
        </div>
    }
}

/// The `?` cheat sheet: [`SHORTCUTS`] rendered, nothing else. Its close
/// button is the focus trap's one stop, and it takes focus on open so
/// Escape and Enter both work with no aim.
#[component]
fn Cheats(overlay: RwSignal<Overlay>) -> impl IntoView {
    let close_ref = NodeRef::<leptos::html::Button>::new();
    close_ref.on_load(|el: web_sys::HtmlButtonElement| {
        let _ = el.focus();
    });
    let on_key = move |ev: web_sys::KeyboardEvent| {
        match ev.key().as_str() {
            "Escape" => {
                ev.prevent_default();
                close_overlay(overlay);
            }
            "Tab" => {
                ev.prevent_default();
                trap_tab(ev.shift_key());
            }
            _ => {}
        }
        ev.stop_propagation();
    };

    view! {
        <div class="overlay" on:click=move |_| close_overlay(overlay)>
            <div
                id="cheats"
                class="dialog"
                role="dialog"
                aria-modal="true"
                aria-label="Keyboard shortcuts"
                on:click=|ev| ev.stop_propagation()
                on:keydown=on_key
            >
                <h2>"keyboard"</h2>
                <dl>
                    {SHORTCUTS
                        .iter()
                        .map(|(k, what)| {
                            view! {
                                <dt><kbd>{*k}</kbd></dt>
                                <dd>{*what}</dd>
                            }
                        })
                        .collect_view()}
                </dl>
                <button class="close" node_ref=close_ref on:click=move |_| close_overlay(overlay)>
                    "close (Esc)"
                </button>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::{narrows, palette_match, Found, SHORTCUTS};
    use menger_tickets_api::{Kind, Status, TicketSummary};

    /// The palette's v1 filter: a case-insensitive substring of either
    /// the code or the title, and an empty query keeps everything.
    #[test]
    fn palette_matches_code_or_title_case_insensitively() {
        // code, exactly and by prefix — "AGS-1" narrows to AGS-1, AGS-10…
        assert!(palette_match("AGS-1", "Keyboard navigation", "AGS-1"));
        assert!(palette_match("AGS-18", "Keyboard navigation", "AGS-1"));
        assert!(!palette_match("AGS-2", "Parent hierarchy", "AGS-1"));
        assert!(!palette_match("MFE-3", "The live board", "AGS-1"));
        // case folds both ways
        assert!(palette_match("AGS-18", "Keyboard navigation", "ags-1"));
        assert!(palette_match("ags-18", "keyboard", "AGS"));
        assert!(palette_match("MFE-7", "The Ticket Page", "ticket page"));
        // title substrings, anywhere in the string
        assert!(palette_match("AGS-18", "Keyboard navigation", "navig"));
        assert!(palette_match("AGS-18", "Keyboard navigation", "board"));
        assert!(!palette_match("AGS-18", "Keyboard navigation", "mouse"));
        // an empty (or blank) query is "everything"
        assert!(palette_match("AGS-18", "Keyboard navigation", ""));
        assert!(palette_match("AGS-18", "Keyboard navigation", "   "));
        // a query longer than both fields matches nothing, no panic
        assert!(!palette_match("A-1", "hi", "a much longer query"));
        // the query is trimmed, the fields are not re-tokenized: a
        // substring is a substring, spaces included
        assert!(palette_match("AGS-18", "Keyboard navigation", " keyboard nav "));
        assert!(!palette_match("AGS-18", "Keyboard navigation", "keyboardnav"));
        // non-ASCII text is carried through the scan without splitting a
        // character (ASCII case folding only, by design)
        assert!(palette_match("AGS-9", "manifest's palette — tokens", "—"));
        assert!(palette_match("AGS-9", "manifest's palette — tokens", "PALETTE"));
    }

    /// SCALE-25: a query narrows another when it only grew — then every
    /// ticket it matches, the other matched.
    #[test]
    fn a_query_narrows_only_when_it_grew() {
        assert!(narrows("", "map"));
        assert!(narrows("map", "map-12"));
        assert!(narrows("MAP", " map-1 "));
        assert!(narrows("ap-1", "map-12"));
        assert!(!narrows("map-12", "map-1"), "it shrank");
        assert!(!narrows("map", "mop"), "it changed");
    }

    fn hit(id: &str, title: &str) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').expect("EPIC-N");
        TicketSummary {
            id: id.to_string(),
            epic: epic.to_string(),
            seq: seq.parse().expect("seq"),
            title: title.to_string(),
            status: Status::Pending,
            kind: Kind::Build,
            severity: None,
            parent: None,
            child_count: 0,
            commits: Vec::new(),
            path: None,
            blocked_by: Vec::new(),
            unlocks: Vec::new(),
            created_unix: 0,
            updated_unix: 0,
        }
    }

    fn ids(rows: Vec<TicketSummary>) -> Vec<String> {
        rows.into_iter().map(|t| t.id).collect()
    }

    /// SCALE-25: only the newest question's answer is shown — one that comes
    /// back after a newer question went out is dropped, whatever the order —
    /// and the count is the answer's rows plus its `remaining`. While a
    /// question is out, a query that grew shows what the last answer had
    /// that it matches (the head of what is coming); one that changed shows
    /// nothing; and with no connection nothing is shown as current.
    #[test]
    fn only_the_newest_answer_is_shown() {
        let mut f = Found::default();
        let open = f.ask();
        assert!(f.pending() && f.shown("").is_empty());
        assert!(f.take(open, String::new(), vec![hit("AAA-1", "x"), hit("MAP-1", "a map"), hit("MAP-12", "y")], 997));
        assert_eq!((f.total, f.pending()), (1_000, false));
        assert_eq!(ids(f.shown("")), ["AAA-1", "MAP-1", "MAP-12"]);

        let map = f.ask();
        let map12 = f.ask();
        assert!(f.pending());
        assert_eq!(ids(f.shown("map-12")), ["MAP-12"], "the head of what is coming");
        assert_eq!(ids(f.shown("map")), ["MAP-1", "MAP-12"], "titles count, as the rule says");
        assert!(!f.take(map, "map".into(), vec![hit("MAP-1", "a map")], 40), "an older question's answer");
        assert!(f.pending(), "still waiting for the newest");
        assert!(f.take(map12, "map-12".into(), vec![hit("MAP-12", "y"), hit("MAP-120", "z")], 5));
        assert_eq!((f.total, f.pending()), (7, false));
        assert!(!f.take(map, "map".into(), Vec::new(), 0), "late, and older still");
        assert_eq!(ids(f.shown("map-12")), ["MAP-12", "MAP-120"]);

        let shorter = f.ask();
        assert!(f.shown("map-1").is_empty(), "it shrank: nothing until its own answer");
        f.lost(shorter);
        assert!(f.offline && !f.pending() && f.shown("map-1").is_empty());
        let again = f.ask();
        assert!(f.take(again, "map-1".into(), vec![hit("MAP-1", "a map")], 0));
        assert!(!f.offline);
    }

    /// What `?` shows is the key map: every key the handlers implement
    /// has a row, and every row says what it does.
    #[test]
    fn the_cheat_sheet_documents_the_key_map() {
        for (key, what) in SHORTCUTS {
            assert!(!key.trim().is_empty() && !what.trim().is_empty(), "{key}: {what}");
        }
        let keys: Vec<&str> = SHORTCUTS.iter().map(|(k, _)| *k).collect();
        for required in [
            "Tab",         // reaches the nav
            "j / ↓",       // move
            "k / ↑",
            "l / →",       // expand / into
            "h / ←",       // collapse / out
            "Home / End",
            "Enter",       // open
            "/ or Ctrl+K", // palette
            "g h",         // board
            "?",           // this list
            "Esc",         // close
        ] {
            assert!(keys.contains(&required), "cheat sheet is missing {required}: {keys:?}");
        }
    }
}
