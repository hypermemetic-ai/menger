//! CHAT-54: **what a tap does to a phone's keyboard**, one rule for the
//! whole app. While a text box has the keyboard up (`data-kb`, APP-56), a tap
//! outside the box does one of two things, never a third:
//!
//! 1. on something that acts ([`ACTS`]) the action runs on that one tap
//!    (CHAT-50: its `mousedown` is prevented, so nothing blurs and nothing
//!    reflows before the click), and the action decides the keyboard — the
//!    ones in [`KEEPS`] leave the box focused, every other drops it after its
//!    click;
//! 2. on anything else (a message's text, empty space, a header) the tap
//!    blurs the box and the keyboard drops. Nothing more.
//!
//! A drag or a scroll is not a tap ([`crate::media::is_tap`]), so scrolling
//! leaves the keyboard as it is. The draft is the box's value and is never
//! touched. The table is here and nowhere else: a view that needs its action
//! to keep the keyboard adds a selector to [`KEEPS`] (or `data-kb-keep` on
//! the element), it does not prevent a `mousedown` of its own.
use std::cell::Cell;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::dom;

/// What acts on a tap: the page does something, the tap is not just a touch.
pub(crate) const ACTS: &str = "a, button, img, summary, select, label, [role=button], [role=menuitem], [role=tab], [role=option], [role=link], [data-act], [data-ticket-id], .tchip";

/// The actions that keep the box focused and the keyboard up: Send, the
/// image picker and the pen (and the pad it draws on), a quote, the jump to
/// the newest message, the composer's own tokens, thumbs and command menu.
pub(crate) const KEEPS: &str = ".csend, .cattach, .cpick, .dpad, .cthumbs, .ctok, .cmenu, .tjump, [data-act=quote], [data-kb-keep]";

/// A field that takes the keyboard itself: a tap on one is its own business.
const FIELDS: &str = "input, textarea, [contenteditable]:not([contenteditable=false])";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tap {
    /// Not ours: no keyboard up, or the tap is on a field.
    Leave,
    /// An action that keeps the keyboard: the box stays focused.
    Keep,
    /// An action that drops it: it runs, then the box blurs.
    Drop,
    /// Anything else: the box blurs.
    Blur,
}

/// The rule, pure: what a tap does, given whether a box has the keyboard
/// up, whether the tap is on a field, on something that acts, and on one
/// that keeps the keyboard.
pub(crate) fn decide(typing: bool, in_field: bool, acts: bool, keeps: bool) -> Tap {
    match (typing && !in_field, acts, keeps) {
        (false, ..) => Tap::Leave,
        (true, true, true) => Tap::Keep,
        (true, true, false) => Tap::Drop,
        (true, false, _) => Tap::Blur,
    }
}

fn typing() -> Option<web_sys::HtmlElement> {
    let doc = dom::window().document()?;
    doc.document_element()?.has_attribute("data-kb").then_some(())?;
    let a = doc.active_element()?.dyn_into::<web_sys::HtmlElement>().ok()?;
    a.matches(FIELDS).unwrap_or(false).then_some(a)
}

fn read(target: Option<web_sys::EventTarget>) -> (Tap, Option<web_sys::HtmlElement>) {
    let Some(box_) = typing() else { return (Tap::Leave, None) };
    let el = target.and_then(|t| t.dyn_into::<web_sys::Element>().ok());
    let on = |sel: &str| el.as_ref().is_some_and(|e| e.closest(sel).ok().flatten().is_some());
    (decide(true, on(FIELDS), on(ACTS), on(KEEPS)), Some(box_))
}

pub(crate) fn install() {
    let w = dom::window();
    let Some(doc) = w.document() else { return };
    let drop_after = Cell::new(false);
    let drop_after = std::rc::Rc::new(drop_after);
    // The action's own tap: the box keeps its focus through the click.
    let d = drop_after.clone();
    let down = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| match read(ev.target()).0 {
        Tap::Keep => ev.prevent_default(),
        Tap::Drop => {
            ev.prevent_default();
            d.set(true);
        }
        _ => {}
    });
    let _ = doc.add_event_listener_with_callback_and_bool("mousedown", down.as_ref().unchecked_ref(), true);
    down.forget(); // lives as long as the app
    // …and a dropping action blurs once it has run.
    let d = drop_after.clone();
    let click = Closure::<dyn FnMut()>::new(move || {
        if d.replace(false) {
            let _ = dom::window().set_timeout_with_callback_and_timeout_and_arguments_0(
                Closure::once_into_js(|| {
                    if let Some(b) = typing() {
                        let _ = b.blur();
                    }
                })
                .unchecked_ref(),
                0,
            );
        }
    });
    let _ = doc.add_event_listener_with_callback("click", click.as_ref().unchecked_ref());
    click.forget();
    // Anything else: a tap (not a drag) blurs.
    let from = std::rc::Rc::new(Cell::new(None::<(f64, f64, f64)>));
    let f = from.clone();
    let start = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |ev: web_sys::TouchEvent| {
        let t = ev.touches();
        f.set((t.length() == 1).then(|| t.get(0)).flatten().map(|p| (f64::from(p.client_x()), f64::from(p.client_y()), js_sys::Date::now())));
    });
    let _ = doc.add_event_listener_with_callback_and_bool("touchstart", start.as_ref().unchecked_ref(), true);
    start.forget();
    let end = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |ev: web_sys::TouchEvent| {
        let Some((x0, y0, at0)) = from.take() else { return };
        let Some(p) = ev.changed_touches().get(0) else { return };
        if !crate::media::is_tap(x0, y0, at0, f64::from(p.client_x()), f64::from(p.client_y()), js_sys::Date::now()) {
            return;
        }
        if let (Tap::Blur, Some(b)) = read(ev.target()) {
            let _ = b.blur();
        }
    });
    let _ = doc.add_event_listener_with_callback("touchend", end.as_ref().unchecked_ref());
    end.forget();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table() {
        assert_eq!(decide(false, false, true, true), Tap::Leave, "no keyboard: not ours");
        assert_eq!(decide(true, true, false, false), Tap::Leave, "a field takes its own tap");
        assert_eq!(decide(true, false, true, true), Tap::Keep, "send, the picker, the pen, a quote");
        assert_eq!(decide(true, false, true, false), Tap::Drop, "a chip, a link, a menu item");
        assert_eq!(decide(true, false, false, false), Tap::Blur, "text, space, a header");
    }
}
