//! APP-57: **a held ticket opens the one menu** — a touch screen's right
//! click (owner, 2026-10-18: "holding a ticket chip on mobile should open the
//! rightclick menu"). iOS Safari fires no `contextmenu` on a link; it shows
//! its own callout or preview instead, so the menu was out of a phone's reach.
//!
//! A touch that lands on what a right-click opens the menu on (the parent
//! module's `at`: a ticket element, a chip linking to a ticket, the nav) and
//! stays within [`SLOP`] of where it landed for [`HOLD_MS`] opens the menu
//! there; at ≤ 820 the menu is a sheet from the bottom (`menu::place`). The
//! lift after a hold is swallowed — `touchend`'s `preventDefault`, which
//! keeps the engine from making a click of it, and a click that comes anyway
//! within [`SWALLOW_MS`] — so the chip held is not followed. A touch that
//! moves past the slop, a second finger, a scroll anywhere, or the touch
//! ending first is no hold: a scroll scrolls and a tap still opens the
//! ticket. The stylesheet keeps iOS's callout and text selection off the
//! chips on a touch screen (`-webkit-touch-callout`, `user-select`).
//!
//! Android fires its own `contextmenu` on a long press; that opens the menu
//! as a right-click does, and [`claim`] makes it this hold's (no second
//! open, the lift swallowed). A body's frame does the same in its SDK
//! (frontend/sdk/menger-sdk.js) and asks for the menu as its right-click
//! does, `menger:ticket-menu`.
//!
//! [`moved`] and [`swallows`] are pure, pinned by native tests below; the
//! listeners ([`install`]) are DOM.

use std::cell::{Cell, RefCell};

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::{annotate, dom};

/// How long a touch rests before it is a hold — the platforms' own long
/// press is about this.
pub(crate) const HOLD_MS: i32 = 500;
/// How far (CSS px) a held touch may drift and still be a hold; past it the
/// touch is a scroll or a drag.
pub(crate) const SLOP: f64 = 10.0;
/// How long after a hold's lift a click is still the lift's.
pub(crate) const SWALLOW_MS: f64 = 800.0;

/// The touch in progress over something the menu opens on.
struct Press {
    /// Which press this is: the timer opens the menu only if it is still
    /// the one pending.
    gen: u32,
    x: f64,
    y: f64,
    el: web_sys::Element,
    /// The menu opened on it: its lift is swallowed.
    fired: bool,
}

thread_local! {
    static PRESS: RefCell<Option<Press>> = const { RefCell::new(None) };
    static GEN: Cell<u32> = const { Cell::new(0) };
    /// Until when (ms since the epoch) a click is a hold's lift.
    static SWALLOW_UNTIL: Cell<f64> = const { Cell::new(0.0) };
}

/// Whether a touch that landed at `(x0, y0)` and is now at `(x, y)` has
/// moved too far to be a hold.
pub(crate) fn moved(x0: f64, y0: f64, x: f64, y: f64) -> bool {
    (x - x0).hypot(y - y0) > SLOP
}

/// Whether a click at `now` is a hold's lift, swallowed until `until`.
pub(crate) fn swallows(now: f64, until: f64) -> bool {
    now < until
}

/// A touch is down on something the menu opens on — Android's own long
/// press (`contextmenu`) arriving now is this hold.
pub(super) fn during() -> bool {
    PRESS.with(|p| p.borrow().is_some())
}

/// The menu opened on the touch in progress by another door (Android's
/// `contextmenu`): no timer opens it again, and the lift is swallowed.
pub(super) fn claim() {
    PRESS.with(|p| {
        if let Some(p) = p.borrow_mut().as_mut() {
            p.fired = true;
            p.gen = p.gen.wrapping_add(1);
        }
    });
}

fn cancel() {
    PRESS.with(|p| p.borrow_mut().take());
}

/// The lift of a press: a fired one swallows what follows it.
fn lift(ev: &web_sys::Event) {
    let Some(p) = PRESS.with(|p| p.borrow_mut().take()) else { return };
    if p.fired {
        ev.prevent_default();
        SWALLOW_UNTIL.with(|s| s.set(js_sys::Date::now() + SWALLOW_MS));
    }
}

fn point(ev: &web_sys::TouchEvent) -> Option<(f64, f64)> {
    let t = ev.touches().get(0)?;
    Some((f64::from(t.client_x()), f64::from(t.client_y())))
}

/// A field takes its own long press (selection, the caret, paste).
fn typed_in(el: &web_sys::Element) -> bool {
    el.closest("input, textarea, select, [contenteditable]:not([contenteditable='false'])").ok().flatten().is_some()
}

fn listen(target: &web_sys::EventTarget, name: &str, f: &js_sys::Function, passive: bool) {
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);
    opts.set_passive(passive);
    let _ = target.add_event_listener_with_callback_and_add_event_listener_options(name, f, &opts);
}

/// The listeners, installed once for the app's lifetime beside the menu's
/// own. `opens(el, x, y)` says whether the menu opens on `el`;
/// `open(el, x, y)` opens it there.
pub(super) fn install(opens: impl Fn(&web_sys::Element, f64, f64) -> bool + 'static, open: impl Fn(&web_sys::Element, f64, f64) + 'static) {
    let doc: web_sys::EventTarget = dom::window().document().expect("document").into();
    let win: web_sys::EventTarget = dom::window().into();
    let open = std::rc::Rc::new(open);

    let on_start = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |ev: web_sys::TouchEvent| {
        cancel();
        if ev.touches().length() != 1 || annotate::is_on() {
            return;
        }
        let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        let Some((x, y)) = point(&ev) else { return };
        if typed_in(&el) || super::menu_element().is_some_and(|m| m.contains(Some(&el))) || !opens(&el, x, y) {
            return;
        }
        let gen = GEN.with(|g| {
            g.set(g.get().wrapping_add(1));
            g.get()
        });
        PRESS.with(|p| *p.borrow_mut() = Some(Press { gen, x, y, el, fired: false }));
        let open = open.clone();
        spawn_local(async move {
            dom::sleep_ms(HOLD_MS).await;
            let due = PRESS.with(|p| {
                let mut p = p.borrow_mut();
                let p = p.as_mut().filter(|p| p.gen == gen && !p.fired)?;
                p.fired = true;
                Some((p.el.clone(), p.x, p.y))
            });
            if let Some((el, x, y)) = due {
                open(&el, x, y);
            }
        });
    });
    listen(&doc, "touchstart", on_start.as_ref().unchecked_ref(), true);
    on_start.forget();

    let on_move = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |ev: web_sys::TouchEvent| {
        let Some((x, y)) = point(&ev) else { return };
        let gone = PRESS.with(|p| p.borrow().as_ref().is_some_and(|p| !p.fired && (ev.touches().length() != 1 || moved(p.x, p.y, x, y))));
        if gone {
            cancel();
        }
    });
    listen(&doc, "touchmove", on_move.as_ref().unchecked_ref(), true);
    on_move.forget();

    // Not passive: its preventDefault is what keeps the lift from clicking.
    let on_end = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |ev: web_sys::TouchEvent| lift(&ev));
    listen(&doc, "touchend", on_end.as_ref().unchecked_ref(), false);
    listen(&doc, "touchcancel", on_end.as_ref().unchecked_ref(), false);
    on_end.forget();

    // A click that comes anyway after a hold — WebKit's, Android's — is
    // taken before anything in the app sees it.
    let on_click = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        if swallows(js_sys::Date::now(), SWALLOW_UNTIL.with(Cell::get)) {
            SWALLOW_UNTIL.with(|s| s.set(0.0));
            ev.prevent_default();
            ev.stop_propagation();
        }
    });
    listen(&win, "click", on_click.as_ref().unchecked_ref(), false);
    on_click.forget();

    // A scroll anywhere (it does not bubble; capture sees every box) is no
    // hold — a fired one keeps its lift swallowed.
    let on_scroll = Closure::<dyn FnMut()>::new(move || {
        PRESS.with(|p| {
            let mut p = p.borrow_mut();
            if p.as_ref().is_some_and(|p| !p.fired) {
                p.take();
            }
        });
    });
    listen(&doc, "scroll", on_scroll.as_ref().unchecked_ref(), true);
    on_scroll.forget();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_may_drift_within_the_slop() {
        assert!(!moved(100.0, 100.0, 100.0, 100.0));
        assert!(!moved(100.0, 100.0, 106.0, 108.0), "10 px exactly is still a hold");
        assert!(moved(100.0, 100.0, 100.0, 111.0), "a scroll starting on the chip");
        assert!(moved(100.0, 100.0, 89.0, 100.0));
    }

    #[test]
    fn only_the_lifts_click_is_swallowed() {
        assert!(swallows(1_000.0, 1_000.0 + SWALLOW_MS));
        assert!(!swallows(1_000.0 + SWALLOW_MS, 1_000.0 + SWALLOW_MS));
        assert!(!swallows(5_000.0, 0.0), "no hold, no swallow: a tap opens its ticket");
    }

    /// iOS's callout and text selection are off the chips — the app's (on a
    /// touch screen; a conversation keeps its text) and a body's (the SDK's
    /// touch sheet) — and the menu is a sheet at a phone's width. WebKit off
    /// iOS drops `-webkit-touch-callout` from the CSSOM, so it is read here.
    #[test]
    fn a_chip_shows_no_callout_and_the_menu_is_a_sheet() {
        let css = crate::tokens_lint::app_css();
        // TICKETS-36: a link by uid is a chip too.
        assert!(css.contains(r##":is([data-ticket-id], a[href^="#/ticket/"], a[href^="#/t/"]) { -webkit-touch-callout: none; }"##));
        assert!(css.contains(r##":is([data-ticket-id]:not(section), a[href^="#/ticket/"], a[href^="#/t/"]) { -webkit-user-select: none; user-select: none; }"##));
        assert!(css.contains(":is(#tmenu, #tagmenu, #dmenu)[data-sheet] {"), "DEPLOY-47: the deployment menu is a sheet too");
        let sdk = include_str!("../../sdk/menger-sdk.js");
        assert!(sdk.contains("'[data-ticket-id], m-ticket { -webkit-touch-callout: none; -webkit-user-select: none; user-select: none; }'"));
    }

    #[test]
    fn a_hold_is_the_platforms_long_press() {
        assert_eq!(HOLD_MS, 500);
        assert!(SWALLOW_MS > f64::from(HOLD_MS));
    }
}
