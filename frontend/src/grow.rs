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
