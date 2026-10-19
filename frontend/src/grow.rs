//! APP-27: an answer box grows with what is typed.
//!
//! Every box the owner types an answer or a reply into — the tray's text
//! reply and its Request changes, the ticket page's Conversation, the
//! annotation comment — carries `class="grow"` and is handed to [`follow`].
//! The stylesheet's `textarea.grow` caps it at half the viewport, past which
//! it scrolls; each box's own rule gives its resting height as a
//! `min-height` (its rows, in `lh`). Where the engine has CSS
//! `field-sizing: content` the stylesheet does the growing and this module
//! does nothing; where it does not (Firefox), [`fit`] sets the height from
//! `scrollHeight` on every value — typed, restored from a draft, or cleared
//! after a send — so one helper serves every box.

use leptos::html::Textarea;
use leptos::prelude::*;
use wasm_bindgen::{JsCast, JsValue};

thread_local! {
    /// Whether the engine sizes a `field-sizing: content` box itself.
    static NATIVE: bool = native();
}

fn native() -> bool {
    let Ok(css) = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("CSS")) else { return false };
    let Some(supports) = js_sys::Reflect::get(&css, &JsValue::from_str("supports"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
    else {
        return false;
    };
    supports
        .call2(&css, &JsValue::from_str("field-sizing"), &JsValue::from_str("content"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// The box at the height of what it holds: from its resting height (its
/// `min-height`, else its rows) up to the stylesheet's cap, then it scrolls.
/// A no-op where `field-sizing: content` does it, and for a box not laid
/// out (detached or hidden: it is fitted on its next value).
pub(crate) fn fit(el: &web_sys::HtmlTextAreaElement) {
    if NATIVE.with(|n| *n) {
        return;
    }
    // `HtmlElement::style()` by explicit path: tachys's element extension
    // trait also offers a `style` on the textarea type.
    let style = web_sys::HtmlElement::style(el);
    if !el.is_connected() || el.offset_height() == 0 {
        let _ = style.remove_property("height");
        return;
    }
    // At `auto` the box is its resting height, and scrollHeight what it holds.
    let _ = style.set_property("height", "auto");
    let edge = (el.offset_height() - el.client_height()).max(0);
    let _ = style.set_property("height", &format!("{}px", el.scroll_height() + edge));
}

/// Keep `area` fitted to its value: `value` reads the signal the box's
/// `prop:value` reads, so a value set from outside re-sizes it as typing does.
/// The fit waits a frame: this effect may run before the one that writes the
/// new value into the box (a box cleared after a send would measure the old).
pub(crate) fn follow<T: 'static>(area: NodeRef<Textarea>, value: impl Fn() -> T + 'static) {
    Effect::new(move |_| {
        value();
        if let Some(el) = area.get() {
            fit(&el);
            request_animation_frame(move || fit(&el));
        }
    });
}

/// APP-93: whether a box's text runs past one line at its present width.
/// One line is the box's line height and its vertical padding; the box is
/// measured at its resting height so a height set by [`fit`] does not read
/// as text.
fn over_one_line(el: &web_sys::HtmlTextAreaElement) -> bool {
    let style = web_sys::HtmlElement::style(el);
    let _ = style.set_property("height", "auto");
    let Some(cs) = web_sys::window().and_then(|w| w.get_computed_style(el).ok().flatten()) else { return false };
    let px = |p: &str| cs.get_property_value(p).ok().and_then(|v| v.trim_end_matches("px").parse::<f64>().ok());
    let line = px("line-height").unwrap_or_else(|| px("font-size").unwrap_or(16.0) * 1.35);
    let pad = px("padding-top").unwrap_or(0.0) + px("padding-bottom").unwrap_or(0.0);
    f64::from(el.scroll_height()) > line * 1.5 + pad
}

/// APP-93: the phone's one-row composer card (`.ccard`, the box beside its
/// buttons) gives the box the whole row once its text would wrap beside
/// them — the card's `wrap` class, which the stylesheet reads to drop the
/// buttons to a row under it. The box is measured at the narrow width (the
/// class off) and the class set in the same task, so no frame is painted
/// in between and a text that fits one line beside the buttons goes back
/// beside them. A no-op where the card is laid out as a column (a desk):
/// the class changes nothing there.
pub(crate) fn wrap<T: 'static>(area: NodeRef<Textarea>, value: impl Fn() -> T + 'static) {
    let lay = move |el: &web_sys::HtmlTextAreaElement| {
        let Some(card) = el.parent_element() else { return };
        let list = card.class_list();
        let _ = list.remove_1("wrap");
        if !el.is_connected() || el.offset_height() == 0 {
            fit(el);
            return;
        }
        let wraps = el.value().contains('\n') || over_one_line(el);
        let _ = list.toggle_with_force("wrap", wraps);
        fit(el);
        if NATIVE.with(|n| *n) {
            let _ = web_sys::HtmlElement::style(el).remove_property("height");
        }
    };
    Effect::new(move |_| {
        value();
        if let Some(el) = area.get() {
            lay(&el);
            request_animation_frame(move || lay(&el));
        }
    });
}
