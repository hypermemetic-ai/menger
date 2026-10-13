//! AGS-16: the ticket menu — right-click any rendered ticket, anywhere,
//! for the ticket-level actions menger has today: **Open**, **Open in new
//! tab**, **Copy code**, **Set status ▸** (the seven, the current one
//! marked). manifest's ticket menu is the pattern; the behavior is
//! ported, not the code.
//!
//! Attachment is one attribute, not per-view wiring: every element that
//! renders a ticket carries `data-ticket-id` — the board's `<tr>`, the
//! nav's leaf `<a>`, the ticket page's `<h2>`, each resolved blockers /
//! unlocks `<li>` — and ONE document-level `contextmenu` listener, owned
//! by the [`TicketMenu`] component the App mounts once, looks for the
//! nearest such ancestor. Found → the native menu is suppressed and this
//! one opens at the cursor; not found → the browser's own menu, untouched
//! (plain page text keeps it). A future surface joins by carrying the
//! attribute; nothing else needs to know the menu exists.
//!
//! AGS-20 adds the keyboard's door to the same menu (AGS-18's one-line
//! follow-up): **Menu** / **Shift+F10** opens it on
//! `document.activeElement.closest('[data-ticket-id], [data-nav-id]')` —
//! the focused ticket, wherever it is — positioned at that element's rect
//! instead of a cursor, and focus goes back to that row when the menu
//! closes.
//!
//! Exactly one menu by construction: the open state is a single
//! `Option<Target>` signal, so a second right-click replaces the target
//! (a fresh subtree at the new cursor) — it can never add one. Escape,
//! a mousedown outside the menu (capture phase: any click anywhere), any
//! scroll, a route change, or choosing an item closes it. Arrow keys move
//! focus through the items (wrapping), Enter activates the focused one
//! natively (they are real `<a>`/`<button>` elements), ArrowRight/Left
//! open and close the status list; the menu is placed at the cursor and
//! then shifted left/up as needed to stay inside the viewport
//! ([`clamp`]), re-measured when the status list opens.
//!
//! Set status goes through the same `request_set_status` the board's
//! select uses: a failure lands in the app's error banner; the change
//! itself arrives back over the watch stream and repaints every host
//! (the mark in an open menu follows too — it reads the row's signal).
//! Copy code tries the async clipboard API, then the legacy
//! `execCommand("copy")` path (insecure contexts have no
//! `navigator.clipboard`), and if neither works shows the id selected in
//! a field so a manual copy is one keystroke; every outcome is confirmed
//! by a short-lived toast. No creation affordance lives here (MFE-1
//! story 2, retracted).
//!
//! MFE-12: the element a menu was opened on can now leave the DOM — the
//! board mounts a window, and a closed nav node unmounts its children.
//! Nothing here needs it to stay: the open menu holds the ticket's *id*
//! and reads everything it shows and sets through the row store, and the
//! scroll that would carry a board row out of the window closes the menu
//! first. A keyboard-opened menu hands focus back to its row through
//! [`keys::restore_focus`], which finds the row again by id if the window
//! remounted it, and otherwise leaves j/k to resume from it.
//!
//! SCALE-24: the row store it reads is the row cache (src/cache.rs), by id —
//! where every view that draws a ticket holds it — not the board's list,
//! which no longer holds every ticket the nav can show.
//!
//! SCALE-26: and it holds its ticket there while it is open — fetching it
//! when no view holds it (a ticket page's unlock not asked for yet): nothing
//! walks the store into the cache any more.
//!
//! WAVE-5: opened on a shortcode's row in the nav — the root's row, which
//! carries `data-shortcode` — it offers that shortcode's **Wave board**
//! (`#/waves/<SHORTCODE>`) after Open in new tab.
//!
//! FILTER-22: a tag the app draws inside a ticket element — a row's id,
//! status, kind or severity chip — opens the tag's filter menu instead
//! (src/tags.rs); a right-click anywhere else on the element opens this one.
//!
//! MENGER-34: a framed body is its own document, out of the listener's
//! reach, so its SDK (frontend/sdk/menger-sdk.js, 1.12) finds the chip
//! itself — the nearest `[data-ticket-id]` or `m-ticket[ref]` — and posts
//! `menger:ticket-menu { id, x, y, keyboard }` out; the bridge (src/frame.rs)
//! checks it as every frame message is checked, maps the point through the
//! frame's box ([`frame_point`]) and opens this same menu there
//! ([`open_from_frame`]). Focus moving into a frame (the window's `blur`)
//! closes it, as an outside mousedown does on the app's own document.
//!
//! ACT-7: **Nav ▸** — opened in the nav (a right-click, or Menu / Shift+F10,
//! on any of its rows), the menu carries a submenu that shapes the tree:
//! expand or collapse the row's subtree, all of its shortcode from the root,
//! collapse the others, expand or collapse everything, reveal the open
//! ticket (src/shape.rs says which items where, and what each does; the nav,
//! src/nav.rs, does them — [`nav::at`], [`nav::act`]). Elsewhere in the app
//! the menu has no Nav ▸. A right-click on the nav's empty space or its
//! header — or on a row that stands for no ticket, a rootless shortcode's or
//! a heading — opens the same menu holding only the Nav items
//! ([`nav_menu_view`]). On the flat list only the reveal means anything.
//! When ACT-4's one menu lands, Nav ▸ is one of its groups.
//!
//! ACT-4: **one menu for a ticket, wherever it is drawn** — its nav row, its
//! id chip, a board row, a card, the ticket page's heading, a relation, a
//! framed chip (owner, 2026-10-13: "the nav entry and the chip itself to have
//! the same menu"). It holds the ticket actions, every update, the filter
//! items and Nav ▸ together; what it holds for a target is a pure function
//! (src/menu/model.rs: one ticket or a selection, the chip right-clicked, the
//! reader's grants). A chip inside a ticket element no longer opens FILTER-22's
//! menu: its items are this menu's filter group, a status, kind, severity or
//! label chip's first. A right-click on a selected row acts on the selection
//! (ACT-3) when it holds more than one — "N tickets", the one-ticket items
//! stepping aside — and every update is one write: `update` for one ticket,
//! `update_many` (ACT-2) for many (src/menu/write.rs). Move under… and
//! Blocked by… pick a ticket with the palette's find, Rename… and Link
//! commits… ask in place (src/menu/forms.rs); Delete… confirms inside the
//! app, listing every ticket and what moves, then offers Undo while the
//! tombstone keeps them (src/menu/delete.rs). A shortcode's row in the nav is
//! its root ticket's menu, plus the shortcode's Wave board.

use js_sys::Reflect;
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};

use std::cell::Cell;
use std::sync::Arc;

mod delete;
mod forms;
pub(crate) mod model;
mod view;
mod write;

pub(crate) use view::TicketMenu;
use view::Toaster;

use crate::lens::{self, Level};
use crate::shape::{self, Item};
use crate::tags::Tag;
use crate::{cache, dom, keys, nav, selection, session, signin, st_class, tags, waves};
use menger_tickets_api::{AttrKind, KnowNothing, TicketSummary};
use model::{Entry, Facts, Form, Grants, Mark, Sub};

thread_local! {
    /// MENGER-34: the one menu's open state, for the frame bridge to open
    /// it through ([`open_from_frame`]) — set when the menu mounts.
    static OPEN: Cell<Option<RwSignal<Option<Target>>>> = const { Cell::new(None) };
}

/// MENGER-34: a ticket id as a frame wrote it, if it is one — `SHORTCODE-N`,
/// trimmed, upper case (as the server reads it); anything else is nothing.
pub(crate) fn frame_ticket(raw: &str) -> Option<String> {
    let v = raw.trim();
    (v.len() <= 64 && AttrKind::Ticket.check(v, &KnowNothing).is_ok()).then(|| v.to_ascii_uppercase())
}

/// MENGER-34: a point in a frame's document, `(x, y)`, on the app's
/// viewport — the frame's box at `(left, top)`, `width` wide on screen for
/// `client_width` CSS px inside (a mockup's frame is scaled).
pub(crate) fn frame_point(left: f64, top: f64, width: f64, client_width: f64, x: f64, y: f64) -> (f64, f64) {
    let s = if client_width > 0.0 && width > 0.0 { width / client_width } else { 1.0 };
    (left + x * s, top + y * s)
}

/// MENGER-34: open the menu on `id` at `(x, y)` on the app's viewport, as
/// a right-click (or, `keyboard`, Menu / Shift+F10) inside a frame asked —
/// replacing any menu open, as a second right-click does.
pub(crate) fn open_from_frame(id: String, x: f64, y: f64, keyboard: bool) {
    let Some(target) = OPEN.with(Cell::get) else { return };
    if keyboard {
        keys::remember_focus();
    }
    let ids = model::target_ids(&id, &selection::ids());
    target.set(Some(Target { ids, shortcode: None, x, y, nav: None, chip: None, level: Level::Global }));
}

/// ACT-4: the ticket element `el` is (or sits in) — what the one menu opens
/// on. FILTER-22's tag menu stands down inside one.
pub(crate) fn host_of(el: &web_sys::Element) -> Option<web_sys::Element> {
    el.closest("[data-ticket-id]").ok().flatten()
}

/// ACT-4: whose filter the menu's filter items build, for a right-click on
/// `el` inside `host`: the chip's surface, else the host's, else the global
/// filter (the ticket page, a Needs-you row, a framed chip).
fn level_at(host: &web_sys::Element) -> Level {
    host.closest("[data-tag-level]")
        .ok()
        .flatten()
        .and_then(|s| s.get_attribute("data-tag-level"))
        .and_then(|n| Level::of_name(&n))
        .unwrap_or(Level::Global)
}

/// ACT-4: the menu's target on `host`, opened at `(x, y)` — the selection
/// when the host is in it and it holds more than one.
fn target_on(host: &web_sys::Element, el: &web_sys::Element, x: f64, y: f64, nav: Option<nav::At>) -> Option<Target> {
    let id = host.get_attribute("data-ticket-id").or_else(|| host.get_attribute("data-nav-id"))?;
    let (chip, level) = match tags::tag_at(el) {
        Some((tag, level)) => (Some(tag), level),
        None => (None, level_at(host)),
    };
    Some(Target { ids: model::target_ids(&id, &selection::ids()), shortcode: host.get_attribute("data-shortcode"), x, y, nav, chip, level })
}

/// The menu's DOM id — the one element the document-level listeners
/// test containment against.
const MENU_ID: &str = "tmenu";
/// Margin the menu keeps from the viewport's edges when clamped.
const EDGE: f64 = 4.0;
/// How long a confirmation toast stays: a plain "copied" is glanced at;
/// the manual-copy fallback holds its selected id until the user acts.
const TOAST_MS: i32 = 2200;
const TOAST_MANUAL_MS: i32 = 12_000;
/// ACT-4: how long a delete's Undo is offered (the tombstone keeps the
/// tickets 30 days; the toast is the quick way back).
const TOAST_UNDO_MS: i32 = 15_000;

/// MENGER-31: the menu's wave items for a ticket, each its words, its link
/// and the attribute a harness finds it by. The owner's reading (MENGER-31's
/// `reading` input, recommended): **Wave board for <parent>** — the parent's
/// subtree's board, this ticket highlighted; absent for a root — and **Waves
/// to <ticket>**, the board whose final state it is. One function, so the
/// one-item variant (the parent's board cut off at this ticket) is a change
/// here and a scope there, not in every surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WaveItem {
    pub(crate) words: String,
    pub(crate) href: String,
    /// `data-wave`: `parent` or `to`.
    pub(crate) kind: &'static str,
}

pub(crate) fn wave_items(id: &str, parent: Option<&str>) -> Vec<WaveItem> {
    let mut out = Vec::new();
    if let Some(p) = parent {
        let href = waves::scoped_href(&waves::Scope::Under(p.to_string()), Some(id));
        out.push(WaveItem { words: format!("Wave board for {p}"), href, kind: "parent" });
    }
    let href = waves::scoped_href(&waves::Scope::To(id.to_string()), None);
    out.push(WaveItem { words: format!("Waves to {id}"), href, kind: "to" });
    out
}

/// What was right-clicked, and where.
#[derive(Clone, PartialEq)]
struct Target {
    /// The tickets — ACT-4: one, or the selection it was opened on; ACT-7:
    /// none for the nav's own menu (its empty space, a row that stands for no
    /// ticket).
    ids: Vec<String>,
    /// WAVE-5: the shortcode the element stands for, when it is a
    /// shortcode's row in the nav (`data-shortcode`).
    shortcode: Option<String>,
    x: f64,
    y: f64,
    /// ACT-7: opened in the nav — where, for its Nav ▸ items.
    nav: Option<nav::At>,
    /// ACT-4: the chip right-clicked inside the element (FILTER-22's tag).
    chip: Option<Tag>,
    /// ACT-4: whose filter the filter items build.
    level: Level,
}

/// How a copy ended (see module doc): the async API, the legacy command,
/// or neither — the id is offered selected for a manual copy.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Copied {
    Api,
    Legacy,
    Manual,
}

#[derive(Clone, PartialEq)]
pub(super) struct Toast {
    text: String,
    /// The id to show selected in a field (the manual fallback).
    manual: Option<String>,
    /// ACT-4: the tickets a delete removed — the toast's Undo brings them back.
    undo: Option<Vec<String>>,
}

/// Where a `w`×`h` box opened at the cursor (`x`, `y`) goes so it stays
/// inside a `vw`×`vh` viewport: shifted left / up by the overflow, keeping
/// [`EDGE`] from the far edges, never past the near ones. A box larger
/// than the viewport pins [`EDGE`] from the near edge (its own scrolling
/// takes over).
pub(crate) fn clamp(x: f64, y: f64, w: f64, h: f64, vw: f64, vh: f64) -> (f64, f64) {
    let left = x.min(vw - w - EDGE).max(EDGE.min(x)).max(0.0);
    let top = y.min(vh - h - EDGE).max(EDGE.min(y)).max(0.0);
    (left, top)
}

/// Keyboard cursor over `len` items: the index to focus for `key` given
/// the currently focused item (`None` = focus is on the menu itself, not
/// an item). Arrows wrap; Home/End jump. `None` out means the key is not
/// a cursor key (or there is nothing to move over).
pub(crate) fn step(current: Option<usize>, len: usize, key: &str) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match key {
        "ArrowDown" => Some(current.map_or(0, |i| (i + 1) % len)),
        "ArrowUp" => Some(current.map_or(len - 1, |i| (i + len - 1) % len)),
        "Home" => Some(0),
        "End" => Some(len - 1),
        _ => None,
    }
}

fn document() -> web_sys::Document {
    dom::window().document().expect("document")
}

fn menu_element() -> Option<web_sys::Element> {
    document().get_element_by_id(MENU_ID)
}

/// The menu's focusable items in document order — only what is mounted,
/// so the status list counts exactly while it is open.
pub(crate) fn items(root: &web_sys::Element) -> Vec<web_sys::HtmlElement> {
    let Ok(list) = root.query_selector_all("[role='menuitem'], [role='menuitemradio']") else {
        return Vec::new();
    };
    (0..list.length())
        .filter_map(|i| list.item(i))
        .filter_map(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .collect()
}

fn focus(el: &web_sys::HtmlElement) {
    let _ = el.focus();
}

/// Measure the mounted menu and move it inside the viewport (see
/// [`clamp`]). Called once it is in the DOM and again whenever its height
/// changes (the status list opening). FILTER-22: the tag menu's too.
pub(crate) fn place(el: &web_sys::HtmlElement, x: f64, y: f64) {
    let rect = el.get_bounding_client_rect();
    let win = dom::window();
    let dim = |v: Result<JsValue, JsValue>| v.ok().and_then(|v| v.as_f64()).unwrap_or(f64::INFINITY);
    let (vw, vh) = (dim(win.inner_width()), dim(win.inner_height()));
    let (left, top) = clamp(x, y, rect.width(), rect.height(), vw, vh);
    let style = el.style();
    let _ = style.set_property("left", &format!("{left}px"));
    let _ = style.set_property("top", &format!("{top}px"));
}

/// Close the menu, handing focus back where it was if the menu was opened
/// from the keyboard (AGS-20). Every close path goes through here, so
/// Escape, an outside click, a scroll, a route change and choosing an item
/// all end the same way.
fn close_menu(target: RwSignal<Option<Target>>) {
    if target.with_untracked(Option::is_none) {
        return;
    }
    target.set(None);
    keys::restore_focus();
}

/// The focused ticket and where its row sits — AGS-18's follow-up: the
/// keyboard's own way to ask for this menu. `data-ticket-id` is the menu's
/// attachment everywhere; `data-nav-id` is accepted too so a nav row that
/// ever loses the former still answers. Nothing inside an overlay dialog
/// counts (those own their keys), and the menu opens at the element's
/// rect rather than a cursor that does not exist.
///
/// ACT-7: a focused row of the nav opens it too — a row that stands for no
/// ticket (a rootless shortcode's, a heading) with the nav's items alone.
fn focused_ticket() -> Option<Target> {
    let active = document().active_element()?;
    if active.closest("[role=dialog]").ok().flatten().is_some() {
        return None;
    }
    let nav = nav::at(&active).filter(offers);
    let host = match active.closest("[data-ticket-id], [data-nav-id]").ok().flatten() {
        Some(host) => host,
        None if nav.is_some() => active.closest("#nav [role=treeitem]").ok().flatten()?,
        None => return None,
    };
    let rect = host.get_bounding_client_rect();
    match target_on(&host, &active, rect.left(), rect.bottom(), nav.clone()) {
        Some(t) => Some(Target { chip: None, ..t }),
        None => Some(Target { ids: Vec::new(), shortcode: None, x: rect.left(), y: rect.bottom(), nav, chip: None, level: Level::Global }),
    }
}

/// The document-level listeners, installed once for the app's lifetime
/// (the component is mounted once): open on a right-click over a ticket
/// element — or on Menu / Shift+F10 over the focused one — and close on
/// Escape / outside mousedown / scroll / route change.
fn install_listeners(target: RwSignal<Option<Target>>) {
    let doc = document();
    let win = dom::window();

    let on_context = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        // ACT-7: in the nav, the menu carries Nav ▸ — and its empty space,
        // its header and a row that stands for no ticket open the nav's
        // items alone.
        let nav = nav::at(&el).filter(offers);
        let (x, y) = (ev.client_x() as f64, ev.client_y() as f64);
        // ACT-4: anywhere on a ticket element — its chips too — this menu.
        if let Some(t) = host_of(&el).and_then(|host| target_on(&host, &el, x, y, nav.clone())) {
            // Only here: a right-click anywhere else keeps the native menu.
            ev.prevent_default();
            target.set(Some(t));
        } else if tags::tag_at(&el).is_some() {
            // FILTER-22: a tag outside any ticket is the tag menu's.
        } else if nav.is_some() {
            ev.prevent_default();
            target.set(Some(Target { ids: Vec::new(), shortcode: None, x, y, nav, chip: None, level: Level::Nav }));
        }
    });
    let _ = doc.add_event_listener_with_callback("contextmenu", on_context.as_ref().unchecked_ref());
    on_context.forget();

    let on_down = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        if target.with_untracked(Option::is_none) {
            return;
        }
        let inside = menu_element().is_some_and(|m| {
            ev.target()
                .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
                .is_some_and(|n| m.contains(Some(&n)))
        });
        if !inside {
            close_menu(target);
        }
    });
    // Capture phase: no element can swallow the mousedown before this
    // sees it, so a click on anything outside the menu closes it.
    let _ = doc.add_event_listener_with_callback_and_bool(
        "mousedown",
        on_down.as_ref().unchecked_ref(),
        true,
    );
    on_down.forget();

    let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        let key = ev.key();
        if key == "Escape" {
            close_menu(target);
            return;
        }
        // AGS-18's follow-up (AGS-20): the keyboard opens this menu on
        // whatever ticket has focus, at that row's rect. `preventDefault`
        // is what stops the engine from also firing its own `contextmenu`
        // for the same keystroke and re-opening us at its coordinates.
        if key == "ContextMenu" || (key == "F10" && ev.shift_key()) {
            if let Some(t) = focused_ticket() {
                ev.prevent_default();
                ev.stop_propagation();
                keys::remember_focus();
                target.set(Some(t));
            }
        }
    });
    let _ = doc.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    on_key.forget();

    // `scroll` does not bubble; capturing on the document sees every
    // scrolling box (the content column, the nav's tree) at once.
    let on_scroll = Closure::<dyn FnMut()>::new(move || close_menu(target));
    let _ = doc.add_event_listener_with_callback_and_bool(
        "scroll",
        on_scroll.as_ref().unchecked_ref(),
        true,
    );
    on_scroll.forget();

    let on_route = Closure::<dyn FnMut()>::new(move || close_menu(target));
    let _ = win.add_event_listener_with_callback("hashchange", on_route.as_ref().unchecked_ref());
    on_route.forget();

    // MENGER-34: a mousedown inside a frame never reaches this document;
    // focus leaving for the frame (or the window) is its trace here.
    let on_blur = Closure::<dyn FnMut()>::new(move || close_menu(target));
    let _ = win.add_event_listener_with_callback("blur", on_blur.as_ref().unchecked_ref());
    on_blur.forget();
}

// ---------------------------------------------------------------------
// Copy code

/// `navigator.clipboard.writeText(text)` as a promise, or None where the
/// async clipboard does not exist (insecure context, old engine). Reached
/// through `Reflect`: web-sys's `Clipboard` binding is behind its
/// unstable-APIs cfg, and this is one call.
fn clipboard_write(text: &str) -> Option<js_sys::Promise> {
    let nav = dom::window().navigator();
    let clip = Reflect::get(&nav, &"clipboard".into()).ok()?;
    if clip.is_undefined() || clip.is_null() {
        return None;
    }
    let write: js_sys::Function = Reflect::get(&clip, &"writeText".into()).ok()?.dyn_into().ok()?;
    write.call1(&clip, &JsValue::from_str(text)).ok()?.dyn_into().ok()
}

/// The pre-async-clipboard path: select the text in an off-screen
/// textarea and `document.execCommand("copy")`. Works from a user gesture
/// in insecure contexts where `navigator.clipboard` is absent.
fn legacy_copy(text: &str) -> bool {
    let doc = document();
    let Some(body) = doc.body() else { return false };
    let Ok(ta) = doc
        .create_element("textarea")
        .and_then(|e| e.dyn_into::<web_sys::HtmlTextAreaElement>().map_err(Into::into))
    else {
        return false;
    };
    ta.set_value(text);
    let _ = ta.set_attribute("readonly", "");
    let _ = ta.set_attribute("aria-hidden", "true");
    // `HtmlElement::style()` by explicit deref: tachys's element
    // extension trait also offers a `style` on the textarea type.
    let style = web_sys::HtmlElement::style(&ta);
    let _ = style.set_property("position", "fixed");
    let _ = style.set_property("top", "0");
    let _ = style.set_property("opacity", "0");
    if body.append_child(&ta).is_err() {
        return false;
    }
    ta.select();
    let ok = Reflect::get(&doc, &"execCommand".into())
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&doc, &JsValue::from_str("copy")).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let _ = body.remove_child(&ta);
    ok
}

async fn copy_text(text: &str) -> Copied {
    if let Some(p) = clipboard_write(text) {
        if JsFuture::from(p).await.is_ok() {
            return Copied::Api;
        }
    }
    if legacy_copy(text) {
        return Copied::Legacy;
    }
    Copied::Manual
}

/// Show a toast, replacing any current one; it clears itself after its
/// time unless a newer toast has taken over (the generation counter).
fn show_toast(toast: RwSignal<Option<Toast>>, gen: StoredValue<u32>, t: Toast) {
    let g = gen.get_value().wrapping_add(1);
    gen.set_value(g);
    let ms = if t.manual.is_some() {
        TOAST_MANUAL_MS
    } else if t.undo.is_some() {
        TOAST_UNDO_MS
    } else {
        TOAST_MS
    };
    toast.set(Some(t));
    spawn_local(async move {
        dom::sleep_ms(ms).await;
        if gen.get_value() == g {
            toast.set(None);
        }
    });
}

// ---------------------------------------------------------------------
// Views

/// ACT-7: whether the nav has anything to offer where the menu opened — the
/// flat list with no ticket open has nothing, so no Nav ▸ (and no menu of
/// the nav's own).
fn offers(at: &nav::At) -> bool {
    !shape::items(&at.place, at.reveal).is_empty()
}

/// ACT-7: the Nav ▸ items for where the menu opened ([`shape::items`]),
/// each a button that does its op and closes the menu. The reveal moves
/// focus to the open ticket's row, so the menu hands focus back first.
fn nav_items(at: nav::At, target: RwSignal<Option<Target>>) -> AnyView {
    shape::items(&at.place, at.reveal)
        .into_iter()
        .map(|item| match item {
            Item::Rule => view! { <div class="rule" role="separator"></div> }.into_any(),
            Item::Do(op, words) => {
                let at = at.clone();
                view! {
                    <button
                        class="item"
                        role="menuitem"
                        data-nav-op=op.name()
                        on:click=move |_| {
                            close_menu(target);
                            nav::act(op, &at);
                        }
                    >
                        {words}
                    </button>
                }
                .into_any()
            }
        })
        .collect_view()
        .into_any()
}

/// ACT-7: the nav's own menu — opened on its empty space, its header, or a
/// row that stands for no ticket: a header naming the place, and the Nav
/// items alone (those that need no row, off a row). Placed, focused and
/// walked by the keys as the ticket menu is.
fn nav_menu_view(t: Target, target: RwSignal<Option<Target>>) -> impl IntoView {
    let Target { x, y, nav, .. } = t;
    let Some(at) = nav else { return ().into_any() };
    let root = NodeRef::<leptos::html::Div>::new();
    let focused = StoredValue::new(false);
    Effect::new(move |_| {
        let Some(el) = root.get() else { return };
        place(&el, x, y);
        if !focused.get_value() {
            focused.set_value(true);
            focus(&el);
        }
    });
    let on_key = move |ev: web_sys::KeyboardEvent| {
        let Some(el) = root.get() else { return };
        let list = items(&el);
        let active = document().active_element();
        let current = list.iter().position(|i| active.as_ref() == Some(i.as_ref()));
        match ev.key().as_str() {
            "Tab" => close_menu(target),
            k => {
                if let Some(n) = step(current, list.len(), k) {
                    focus(&list[n]);
                    ev.prevent_default();
                }
            }
        }
    };
    let label = match &at.place {
        shape::Place::Row { label, .. } => label.clone(),
        shape::Place::Flat => "the flat list".to_string(),
        shape::Place::Space => "the tree".to_string(),
    };
    view! {
        <div
            id=MENU_ID
            role="menu"
            tabindex="-1"
            node_ref=root
            data-nav-menu=""
            aria-label="nav"
            style=format!("left: {x}px; top: {y}px")
            on:contextmenu=|ev| ev.prevent_default()
            on:keydown=on_key
        >
            <div class="h">
                <span class="code">"Nav"</span>
                <span class="title">{label}</span>
            </div>
            {nav_items(at, target)}
        </div>
    }
    .into_any()
}

/// The confirmation toast. The manual fallback carries the id in a
/// read-only field, selected on mount, so Ctrl/Cmd+C finishes the copy.
/// ACT-4: a delete's toast carries Undo, which calls `undelete`.
fn toast_view(t: Toast, toaster: Toaster, last_error: RwSignal<Option<String>>) -> impl IntoView {
    let toast = toaster.toast;
    let field = NodeRef::<leptos::html::Input>::new();
    if t.manual.is_some() {
        field.on_load(|el: web_sys::HtmlInputElement| {
            let _ = el.focus();
            el.select();
        });
    }
    let manual = t.manual.clone();
    let undo = t.undo.clone().map(|ids| {
        let busy = RwSignal::new(false);
        let undo = move |_| {
            let ids = ids.clone();
            busy.set(true);
            spawn_local(async move {
                match write::undelete(ids.clone()).await {
                    Ok(()) => toaster.show(format!("restored {}", model::heading(&ids)), None),
                    Err(e) => {
                        busy.try_set(false);
                        last_error.set(Some(format!("undelete: {e}")));
                    }
                }
            });
        };
        view! { <button data-act="undo" disabled=move || busy.get() on:click=undo>"Undo"</button> }
    });
    view! {
        <div id="toast" role="status" class:manual=t.manual.is_some()>
            <span class="msg">{t.text}</span>
            {manual.map(|id| view! { <input class="code" readonly value=id node_ref=field /> })}
            {undo}
            <button on:click=move |_| toast.set(None)>"dismiss"</button>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::{clamp, frame_point, frame_ticket, step, wave_items, WaveItem, EDGE};

    /// MENGER-34: what a frame says is a ticket only when it has a ticket's
    /// form, read as the server reads it.
    #[test]
    fn a_frames_ticket_is_checked_by_form() {
        assert_eq!(frame_ticket("PANTRY-4").as_deref(), Some("PANTRY-4"));
        assert_eq!(frame_ticket(" menger-34 ").as_deref(), Some("MENGER-34"));
        for bad in ["", "PANTRY", "PANTRY-", "-4", "PAN TRY-4", "A-4x", "#/ticket/A-1", "<b>A-1</b>"] {
            assert_eq!(frame_ticket(bad), None, "{bad:?}");
        }
        assert_eq!(frame_ticket(&format!("{}-1", "A".repeat(80))), None, "too long");
    }

    /// MENGER-34: a frame's point lands at the frame's box plus the point,
    /// scaled as a mockup's frame is.
    #[test]
    fn a_frames_point_maps_through_its_box() {
        assert_eq!(frame_point(100.0, 50.0, 800.0, 800.0, 10.0, 20.0), (110.0, 70.0));
        assert_eq!(frame_point(100.0, 50.0, 400.0, 800.0, 10.0, 20.0), (105.0, 60.0), "drawn at half size");
        assert_eq!(frame_point(100.0, 50.0, 0.0, 0.0, 10.0, 20.0), (110.0, 70.0), "an unmeasured box: unscaled");
    }

    /// MENGER-31: the wave items per ticket — a ticket with a parent gets
    /// the parent's board (it highlighted) and the board to itself; a root,
    /// only the latter. The menu is one, so every surface that carries
    /// `data-ticket-id` (a board row, a nav row, a wave card, a Needs-you
    /// row, a palette result, the ticket page's heading) gets these.
    #[test]
    fn the_wave_items_per_ticket() {
        let words = |items: Vec<WaveItem>| items.into_iter().map(|w| (w.words, w.href, w.kind)).collect::<Vec<_>>();
        assert_eq!(
            words(wave_items("MENGER-31", Some("MENGER-2"))),
            [
                ("Wave board for MENGER-2".to_string(), "#/waves/under/MENGER-2?at=MENGER-31".to_string(), "parent"),
                ("Waves to MENGER-31".to_string(), "#/waves/to/MENGER-31".to_string(), "to"),
            ]
        );
        assert_eq!(
            words(wave_items("MENGER-1", None)),
            [("Waves to MENGER-1".to_string(), "#/waves/to/MENGER-1".to_string(), "to")],
            "a root has no parent's board"
        );
        // The links are the routes the app parses.
        for w in wave_items("A-2", Some("A-1")) {
            assert!(matches!(crate::parse_route(&w.href), crate::Route::WavesOf(_)), "{}", w.href);
        }
    }

    /// The menu opens at the cursor and is shifted only by its overflow.
    #[test]
    fn clamp_keeps_the_box_inside_the_viewport() {
        // fits: unchanged
        assert_eq!(clamp(100.0, 200.0, 220.0, 160.0, 1400.0, 900.0), (100.0, 200.0));
        // overflows right: shifted left to sit EDGE from the edge
        assert_eq!(clamp(1300.0, 200.0, 220.0, 160.0, 1400.0, 900.0), (1400.0 - 220.0 - EDGE, 200.0));
        // overflows bottom: shifted up
        assert_eq!(clamp(100.0, 850.0, 220.0, 160.0, 1400.0, 900.0), (100.0, 900.0 - 160.0 - EDGE));
        // both corners at once
        assert_eq!(clamp(1399.0, 899.0, 220.0, 160.0, 1400.0, 900.0), (1400.0 - 220.0 - EDGE, 900.0 - 160.0 - EDGE));
        // taller than the viewport: pinned EDGE from the top, never negative
        assert_eq!(clamp(100.0, 300.0, 220.0, 2000.0, 1400.0, 900.0), (100.0, EDGE));
        // a cursor at the very edge stays where it is (no push past it)
        assert_eq!(clamp(0.0, 0.0, 220.0, 160.0, 1400.0, 900.0), (0.0, 0.0));
    }

    /// Arrow keys wrap over the mounted items; entering from the menu
    /// itself lands on the first (down) or last (up) item.
    #[test]
    fn keyboard_cursor_wraps_and_ignores_other_keys() {
        assert_eq!(step(None, 4, "ArrowDown"), Some(0));
        assert_eq!(step(None, 4, "ArrowUp"), Some(3));
        assert_eq!(step(Some(0), 4, "ArrowDown"), Some(1));
        assert_eq!(step(Some(3), 4, "ArrowDown"), Some(0));
        assert_eq!(step(Some(0), 4, "ArrowUp"), Some(3));
        assert_eq!(step(Some(2), 11, "Home"), Some(0));
        assert_eq!(step(Some(2), 11, "End"), Some(10));
        assert_eq!(step(Some(1), 4, "Enter"), None);
        assert_eq!(step(Some(1), 4, "a"), None);
        assert_eq!(step(None, 0, "ArrowDown"), None);
    }
}
