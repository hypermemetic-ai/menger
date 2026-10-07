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

use js_sys::Reflect;
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};

use crate::{cache, dom, keys, session, st_class};
use menger_tickets_api::Status;

/// The menu's DOM id — the one element the document-level listeners
/// test containment against.
const MENU_ID: &str = "tmenu";
/// Margin the menu keeps from the viewport's edges when clamped.
const EDGE: f64 = 4.0;
/// How long a confirmation toast stays: a plain "copied" is glanced at;
/// the manual-copy fallback holds its selected id until the user acts.
const TOAST_MS: i32 = 2200;
const TOAST_MANUAL_MS: i32 = 12_000;

/// What was right-clicked, and where.
#[derive(Clone, PartialEq)]
struct Target {
    id: String,
    /// WAVE-5: the shortcode the element stands for, when it is a
    /// shortcode's row in the nav (`data-shortcode`).
    shortcode: Option<String>,
    x: f64,
    y: f64,
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
struct Toast {
    text: String,
    /// The id to show selected in a field (the manual fallback).
    manual: Option<String>,
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
fn items(root: &web_sys::Element) -> Vec<web_sys::HtmlElement> {
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
/// changes (the status list opening).
fn place(el: &web_sys::HtmlElement, x: f64, y: f64) {
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
fn focused_ticket() -> Option<(String, Option<String>, f64, f64)> {
    let active = document().active_element()?;
    if active.closest("[role=dialog]").ok().flatten().is_some() {
        return None;
    }
    let host = active
        .closest("[data-ticket-id], [data-nav-id]")
        .ok()
        .flatten()?;
    let id = host
        .get_attribute("data-ticket-id")
        .or_else(|| host.get_attribute("data-nav-id"))?;
    let rect = host.get_bounding_client_rect();
    Some((id, host.get_attribute("data-shortcode"), rect.left(), rect.bottom()))
}

/// The document-level listeners, installed once for the app's lifetime
/// (the component is mounted once): open on a right-click over a ticket
/// element — or on Menu / Shift+F10 over the focused one — and close on
/// Escape / outside mousedown / scroll / route change.
fn install_listeners(target: RwSignal<Option<Target>>) {
    let doc = document();
    let win = dom::window();

    let on_context = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        let host = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest("[data-ticket-id]").ok().flatten());
        if let Some((id, shortcode)) = host.and_then(|el| Some((el.get_attribute("data-ticket-id")?, el.get_attribute("data-shortcode")))) {
            // Only here: a right-click anywhere else keeps the native menu.
            ev.prevent_default();
            target.set(Some(Target { id, shortcode, x: ev.client_x() as f64, y: ev.client_y() as f64 }));
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
            if let Some((id, shortcode, x, y)) = focused_ticket() {
                ev.prevent_default();
                ev.stop_propagation();
                keys::remember_focus();
                target.set(Some(Target { id, shortcode, x, y }));
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
    let ms = if t.manual.is_some() { TOAST_MANUAL_MS } else { TOAST_MS };
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

/// Mounted once by the App, outside every ticket element. Renders the
/// open menu (if any) and the confirmation toast (if any); owns the
/// document-level listeners.
#[component]
pub(crate) fn TicketMenu(last_error: RwSignal<Option<String>>) -> impl IntoView {
    let target = RwSignal::new(None::<Target>);
    let toast = RwSignal::new(None::<Toast>);
    let toast_gen = StoredValue::new(0u32);
    install_listeners(target);

    view! {
        {move || target.get().map(|t| menu_view(t, last_error, target, toast, toast_gen))}
        {move || toast.get().map(|t| toast_view(t, toast))}
    }
}

/// One open menu: header (code chip · status chip · title), the four
/// items, and the inline status list under **Set status** — SDLC-11: three
/// items for a doc, which has no status to set. Built fresh
/// per open, so a second right-click starts with the list closed and the
/// keyboard cursor on the menu itself.
fn menu_view(
    t: Target,
    last_error: RwSignal<Option<String>>,
    target: RwSignal<Option<Target>>,
    toast: RwSignal<Option<Toast>>,
    toast_gen: StoredValue<u32>,
) -> impl IntoView {
    let Target { id, shortcode, x, y } = t;
    // SCALE-26: the menu holds the ticket it is open on, and asks for it when
    // no view does (no walk puts every ticket in the cache any more).
    let holder = cache::Holder::new();
    if !holder.hold_id(&id) {
        session::load_rows(vec![id.clone()], holder);
    }
    let ticket = {
        let id = id.clone();
        Memo::new(move |_| cache::find(&id))
    };
    // Live through the row's signal: a status change arriving over watch
    // while the menu is open moves the mark.
    let status = Memo::new(move |_| ticket.get().map(|s| s.with(|x| x.status)));
    // SDLC-11: a doc has no lifecycle, so the menu offers it no status
    // moves — no Set status at all. Until the row is held, none either.
    let moves = Memo::new(move |_| ticket.get().is_some_and(|s| s.with(|x| x.kind.has_lifecycle())));
    let title = move || ticket.get().map(|s| s.with(|x| x.title.clone())).unwrap_or_default();
    let sub = RwSignal::new(false);
    let root = NodeRef::<leptos::html::Div>::new();
    let close = move || close_menu(target);
    let href = format!("#/ticket/{id}");

    // Place once mounted, focus the menu so the arrow keys work at once,
    // and re-place when the status list changes the height.
    let focused = StoredValue::new(false);
    Effect::new(move |_| {
        let Some(el) = root.get() else { return };
        sub.track();
        place(&el, x, y);
        if !focused.get_value() {
            focused.set_value(true);
            focus(&el);
        }
    });

    let on_key = move |ev: web_sys::KeyboardEvent| {
        let Some(el) = root.get() else { return };
        let root_el: &web_sys::Element = &el;
        let key = ev.key();
        let active = document().active_element();
        let list = items(root_el);
        let current = list.iter().position(|i| active.as_ref() == Some(i.as_ref()));
        let on_parent = list
            .get(current.unwrap_or(usize::MAX))
            .is_some_and(|i| i.has_attribute("aria-haspopup"));
        match key.as_str() {
            "ArrowRight" if on_parent => {
                sub.set(true);
                // The list mounts on the next render tick; focus its
                // first row once it exists.
                spawn_local(async move {
                    dom::sleep_ms(0).await;
                    let first = root
                        .get_untracked()
                        .and_then(|el| items(&el).into_iter().find(|i| i.get_attribute("role").as_deref() == Some("menuitemradio")));
                    if let Some(first) = first {
                        focus(&first);
                    }
                });
                ev.prevent_default();
            }
            "ArrowLeft" if sub.get_untracked() => {
                sub.set(false);
                if let Some(parent) = items(root_el).into_iter().find(|i| i.has_attribute("aria-haspopup")) {
                    focus(&parent);
                }
                ev.prevent_default();
            }
            "Tab" => close(),
            k => {
                if let Some(n) = step(current, list.len(), k) {
                    focus(&list[n]);
                    ev.prevent_default();
                }
            }
        }
    };

    let copy = {
        let id = id.clone();
        move |_| {
            let id = id.clone();
            close();
            spawn_local(async move {
                let t = match copy_text(&id).await {
                    Copied::Api | Copied::Legacy => Toast { text: format!("copied {id}"), manual: None },
                    Copied::Manual => Toast {
                        text: "clipboard unavailable — copy it from here:".into(),
                        manual: Some(id),
                    },
                };
                show_toast(toast, toast_gen, t);
            });
        }
    };

    let status_items = {
        let id = id.clone();
        move || {
            let id = id.clone();
            // MFE-9: the closed vocabulary from the type that owns it, so
            // the chips and what `set_status` sends cannot disagree.
            // SDLC-11: the lifecycle — `Doc` is held, never moved to.
            Status::LIFECYCLE
                .into_iter()
                .map(|s| {
                    let id = id.clone();
                    let current = move || status.get() == Some(s);
                    view! {
                        <button
                            class="item"
                            role="menuitemradio"
                            aria-checked=move || current().to_string()
                            data-status=s.as_str()
                            on:click=move |_| {
                                if !current() {
                                    session::request_set_status(id.clone(), s, last_error, || {});
                                }
                                close();
                            }
                        >
                            <span class="mark" aria-hidden="true">{move || if current() { "✓" } else { "" }}</span>
                            <span class=st_class(s.as_str())>{s.as_str()}</span>
                        </button>
                    }
                })
                .collect_view()
        }
    };

    view! {
        <div
            id=MENU_ID
            role="menu"
            tabindex="-1"
            node_ref=root
            data-ticket=id.clone()
            aria-label=format!("ticket {id}")
            style=format!("left: {x}px; top: {y}px")
            on:contextmenu=|ev| ev.prevent_default()
            on:keydown=on_key
        >
            <div class="h">
                <span class="code">{id.clone()}</span>
                <span class=move || {
                    format!("tag {}", st_class(status.get().map_or("", |s| s.as_str())))
                }>{move || status.get().map(|s| s.to_string()).unwrap_or_default()}</span>
                <span class="title">{title}</span>
            </div>
            <a class="item" role="menuitem" href=href.clone() on:click=move |_| close()>"Open"</a>
            <a class="item" role="menuitem" href=href target="_blank" rel="noopener" on:click=move |_| close()>
                "Open in new tab"
            </a>
            // WAVE-5: a shortcode's row offers the shortcode's wave board.
            {shortcode.map(|sc| view! {
                <a class="item" role="menuitem" href=format!("#/waves/{sc}") data-wave-board=sc.clone() on:click=move |_| close()>
                    "Wave board"
                </a>
            })}
            <button class="item" role="menuitem" on:click=copy>"Copy code"</button>
            {move || {
                let status_items = status_items.clone();
                moves.get().then(|| {
                    view! {
                        <button
                            class="item"
                            role="menuitem"
                            aria-haspopup="menu"
                            aria-expanded=move || sub.get().to_string()
                            on:click=move |_| sub.update(|s| *s = !*s)
                        >
                            "Set status "
                            <span class="arrow" aria-hidden="true">{move || if sub.get() { "▾" } else { "▸" }}</span>
                        </button>
                        {move || sub.get().then(|| view! { <div class="sub" role="group">{status_items()}</div> })}
                    }
                })
            }}
        </div>
    }
}

/// The confirmation toast. The manual fallback carries the id in a
/// read-only field, selected on mount, so Ctrl/Cmd+C finishes the copy.
fn toast_view(t: Toast, toast: RwSignal<Option<Toast>>) -> impl IntoView {
    let field = NodeRef::<leptos::html::Input>::new();
    if t.manual.is_some() {
        field.on_load(|el: web_sys::HtmlInputElement| {
            let _ = el.focus();
            el.select();
        });
    }
    let manual = t.manual.clone();
    view! {
        <div id="toast" role="status" class:manual=t.manual.is_some()>
            <span class="msg">{t.text}</span>
            {manual.map(|id| view! { <input class="code" readonly value=id node_ref=field /> })}
            <button on:click=move |_| toast.set(None)>"dismiss"</button>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::{clamp, step, EDGE};

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
