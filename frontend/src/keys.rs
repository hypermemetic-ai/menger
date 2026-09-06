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
//! Not implemented here, deliberately: "Menu key / Shift+F10 opens the
//! context menu on the focused ticket" — a one-line hook that belongs to
//! AGS-16's menu, to be added when the two land together.

use std::cell::{Cell, RefCell};

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::{navigate_to_ticket, net, st_class, Row};

/// How long a `g` stays armed as the first key of a `g …` sequence.
const GO_WINDOW_MS: f64 = 1500.0;

/// How many matches the jump palette renders at once. The list is
/// rebuilt per keystroke, so this is a rendering budget, not a filter
/// limit anyone can observe on a store of menger's size.
const PALETTE_MAX: usize = 50;

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
    ("Home / End", "first / last row of the tree"),
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

// ---------------------------------------------------------------------
// Focus plumbing shared with the nav tree and the board

/// Every element matching `selector`, in document order.
pub(crate) fn query_all(selector: &str) -> Vec<web_sys::HtmlElement> {
    let Some(doc) = net::window().document() else {
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
}

fn remember_focus() {
    let el = net::window()
        .document()
        .and_then(|d| d.active_element())
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    FOCUS_BEFORE_OVERLAY.with(|c| *c.borrow_mut() = el);
}

/// Hand focus back, a tick later — the dialog has to leave the DOM first,
/// or the browser's own "focus fell out of a removed subtree" fixup
/// lands on `<body>` after us.
fn restore_focus() {
    let el = FOCUS_BEFORE_OVERLAY.with(|c| c.borrow_mut().take());
    spawn_local(async move {
        net::sleep_ms(0).await;
        if let Some(el) = el {
            focus_row(&el);
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
            let _ = net::window().location().set_hash("/");
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
/// shape as the router's `hashchange`.
pub(crate) fn install(overlay: RwSignal<Overlay>) {
    let cb = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        global_keydown(&ev, overlay);
    });
    net::window()
        .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())
        .expect("keydown listener");
    cb.forget(); // lives as long as the app
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

fn global_keydown(ev: &web_sys::KeyboardEvent, overlay: RwSignal<Overlay>) {
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
    if board_key(&key, target.as_ref()) {
        ev.prevent_default();
    }
}

/// Board rows: j/k (and, once a row has focus, ↑/↓) move; Enter opens.
/// Returns true when the key was used. Rows are read from the DOM — the
/// board is a keyed `<For>` over the same store, so document order *is*
/// row order, and nothing has to be mirrored into a second structure.
fn board_key(key: &str, target: Option<&web_sys::Element>) -> bool {
    let rows = query_all("#board tr[data-ticket-id]");
    if rows.is_empty() {
        return false;
    }
    let cur = target
        .and_then(|t| t.closest("#board tr[data-ticket-id]").ok().flatten())
        .and_then(|el| rows.iter().position(|r| r.is_same_node(Some(&el))));
    let last = rows.len() - 1;
    let next = match key {
        // A letter with no row focused starts at the near end; an arrow
        // with no row focused is the page's own scroll, left alone.
        "j" => Some(cur.map_or(0, |i| (i + 1).min(last))),
        "k" => Some(cur.map_or(last, |i| i.saturating_sub(1))),
        "ArrowDown" => cur.map(|i| (i + 1).min(last)),
        "ArrowUp" => cur.map(|i| i.saturating_sub(1)),
        "Enter" => {
            let Some(i) = cur else { return false };
            let Some(id) = rows[i].get_attribute("data-ticket-id") else {
                return false;
            };
            navigate_to_ticket(&id);
            return true;
        }
        _ => None,
    };
    match next {
        Some(i) => {
            focus_and_rove(&rows, &rows[i]);
            true
        }
        None => false,
    }
}

/// Seed the board's tab stop after a render (see [`ensure_tab_stop`]).
/// The board has no selected row — the first one is the way in.
pub(crate) fn ensure_board_tab_stop() {
    ensure_tab_stop("#board tr[data-ticket-id]", None);
}

// ---------------------------------------------------------------------
// The overlays

/// Keep Tab inside the open dialog (both overlays are modal). Focus
/// cycles through the dialog's own focusable elements and never reaches
/// the page behind it.
fn trap_tab(shift: bool) {
    let Some(doc) = net::window().document() else { return };
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
pub(crate) fn Overlays(rows: RwSignal<Vec<Row>>, overlay: RwSignal<Overlay>) -> impl IntoView {
    view! {
        {move || match overlay.get() {
            Overlay::None => ().into_any(),
            Overlay::Palette => view! { <Palette rows overlay /> }.into_any(),
            Overlay::Cheats => view! { <Cheats overlay /> }.into_any(),
        }}
    }
}

/// The jump palette: an input over a listbox of matches. The input keeps
/// focus (ARIA combobox pattern — the highlighted row is named by
/// `aria-activedescendant`, never focused), so typing and moving are the
/// same gesture.
#[component]
fn Palette(rows: RwSignal<Vec<Row>>, overlay: RwSignal<Overlay>) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let hi = RwSignal::new(0usize);
    let results = move || {
        let q = query.get();
        rows.with(|v| {
            v.iter()
                .filter(|r| r.ticket.with(|t| palette_match(&r.id, &t.title, &q)))
                .take(PALETTE_MAX)
                .cloned()
                .collect::<Vec<Row>>()
        })
    };
    let total = move || {
        let q = query.get();
        rows.with(|v| {
            v.iter()
                .filter(|r| r.ticket.with(|t| palette_match(&r.id, &t.title, &q)))
                .count()
        })
    };
    let open_at = move |i: usize| {
        let hit = results().get(i).map(|r| r.id.clone());
        if let Some(id) = hit {
            close_overlay(overlay);
            navigate_to_ticket(&id);
        }
    };

    let input_ref = NodeRef::<leptos::html::Input>::new();
    input_ref.on_load(|el: web_sys::HtmlInputElement| {
        let _ = el.focus();
    });

    let on_key = move |ev: web_sys::KeyboardEvent| {
        let key = ev.key();
        let len = results().len();
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
                    }
                />
                <ul id="palette-list" role="listbox" aria-label="matching tickets">
                    {move || {
                        let found = results();
                        if found.is_empty() {
                            return view! { <li class="empty">"no ticket matches"</li> }.into_any();
                        }
                        let cur = hi.get().min(found.len() - 1);
                        found
                            .into_iter()
                            .enumerate()
                            .map(|(i, r)| {
                                let ticket = r.ticket;
                                let id = r.id.clone();
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
                                        <span class="label">
                                            {move || ticket.with(|t| t.title.clone())}
                                        </span>
                                        <span class=move || {
                                            format!("tag {}", ticket.with(|t| st_class(&t.status)))
                                        }>{move || ticket.with(|t| t.status.clone())}</span>
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
                            let (shown, all) = (results().len(), total());
                            if all > shown {
                                format!("{shown} of {all} matches")
                            } else {
                                format!("{all} matches")
                            }
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
    use super::{palette_match, SHORTCUTS};

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
