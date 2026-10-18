//! APP-56: a phone's on-screen keyboard, followed.
//!
//! iOS lays a keyboard over the page without shrinking the layout
//! viewport: what shrinks is the *visual* viewport, and iOS scrolls it to
//! centre the focused field. A composer fixed above the phone nav (`#pbar`)
//! is then left mid-screen, with the nav and a strip of the page between it
//! and the keyboard. Android (and iOS with `interactive-widget`) resizes the
//! layout viewport instead, and the nav rides up on the keyboard.
//!
//! So while a field is being typed in and the keyboard is up, `<html>`
//! carries `data-kb` and three lengths read off `window.visualViewport`:
//! `--kb-inset` (how far the layout viewport's bottom lies under the
//! keyboard's top), `--kb-top` and `--kb-h` (the visible area). The
//! stylesheet hides `#pbar`, sets `--pbar-h` to `--kb-inset` — so every
//! sheet placed above the bar sits on the keyboard instead — and fits the
//! full-screen chat to the visible area. Without the keyboard all of it
//! comes off and the layout is as before.

use std::cell::Cell;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::dom;

/// Less than this between the layout and the visual height is the
/// browser's own bars moving, not a keyboard.
const KEYBOARD_MIN_PX: f64 = 120.0;

/// What the viewport says, at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Viewport {
    /// `window.innerHeight`: the layout viewport.
    pub inner_h: f64,
    /// `visualViewport.height`, `.offsetTop`, `.scale`.
    pub vv_h: f64,
    pub vv_top: f64,
    pub scale: f64,
    /// The tallest `inner_h` seen at this width: the height with no
    /// keyboard, for a browser that resizes the layout viewport.
    pub tallest: f64,
    /// A field that takes typing has focus.
    pub editing: bool,
}

/// Where the keyboard's top is, as the stylesheet reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Keyboard {
    /// From the layout viewport's bottom up to the keyboard's top.
    pub inset: f64,
    /// The visible area: its top in the layout viewport, its height.
    pub top: f64,
    pub height: f64,
}

/// The keyboard, if one is up: a field is focused and the visible area is
/// well short of the page's height. A pinch-zoomed page is left alone (its
/// visual viewport is small because it is zoomed).
pub(crate) fn keyboard(v: Viewport) -> Option<Keyboard> {
    if !v.editing || v.inner_h <= 0.0 || v.vv_h <= 0.0 || v.scale > 1.01 {
        return None;
    }
    // Overlaid (iOS): the layout viewport keeps its height, the visual one
    // shrinks. Resized (Android): the layout viewport itself shrank.
    let overlaid = v.inner_h - v.vv_h >= KEYBOARD_MIN_PX;
    let resized = v.tallest - v.inner_h >= KEYBOARD_MIN_PX;
    if !overlaid && !resized {
        return None;
    }
    let top = v.vv_top.clamp(0.0, v.inner_h);
    let height = v.vv_h.min(v.inner_h - top);
    Some(Keyboard { inset: (v.inner_h - top - height).max(0.0).round(), top: top.round(), height: height.round() })
}

/// Whether `el` takes typing (and so opens a keyboard): a textarea, a text
/// input, an editable element.
fn takes_typing(el: &web_sys::Element) -> bool {
    match el.tag_name().to_ascii_lowercase().as_str() {
        "textarea" => !el.has_attribute("readonly"),
        "input" => {
            let ty = el.get_attribute("type").unwrap_or_default().to_ascii_lowercase();
            typed_input(&ty) && !el.has_attribute("readonly")
        }
        _ => el.dyn_ref::<web_sys::HtmlElement>().is_some_and(|h| h.is_content_editable()),
    }
}

/// An `<input type>` that opens a keyboard.
fn typed_input(ty: &str) -> bool {
    matches!(ty, "" | "text" | "search" | "email" | "url" | "tel" | "password" | "number")
}

thread_local! {
    static TALLEST: Cell<(f64, f64)> = const { Cell::new((0.0, 0.0)) };
    static SHOWN: Cell<Option<Keyboard>> = const { Cell::new(None) };
}

fn num(r: Result<JsValue, JsValue>) -> f64 {
    r.ok().and_then(|v| v.as_f64()).unwrap_or(0.0)
}

/// Reads the viewport now and stamps (or clears) `<html>`.
fn apply() {
    let w = dom::window();
    let Some(vv) = w.visual_viewport() else { return };
    let Some(doc) = w.document() else { return };
    let (inner_w, inner_h) = (num(w.inner_width()), num(w.inner_height()));
    // The tallest height is per width: a rotation starts it again.
    let tallest = TALLEST.with(|c| {
        let (width, tallest) = c.get();
        let t = if width == inner_w { tallest.max(inner_h) } else { inner_h };
        c.set((inner_w, t));
        t
    });
    let editing = doc.active_element().is_some_and(|el| takes_typing(&el));
    let kb = keyboard(Viewport { inner_h, vv_h: vv.height(), vv_top: vv.offset_top(), scale: vv.scale(), tallest, editing });
    if SHOWN.with(Cell::get) == kb {
        return;
    }
    SHOWN.with(|c| c.set(kb));
    let Some(html) = doc.document_element().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) else { return };
    // A chat's messages read at their end stay there as the keyboard moves
    // the composer: the newest line sits right above it.
    let held = at_end(&doc);
    let style = html.style();
    match kb {
        Some(k) => {
            let _ = style.set_property("--kb-inset", &format!("{}px", k.inset));
            let _ = style.set_property("--kb-top", &format!("{}px", k.top));
            let _ = style.set_property("--kb-h", &format!("{}px", k.height));
            let _ = html.set_attribute("data-kb", "");
        }
        None => {
            let _ = html.remove_attribute("data-kb");
            for p in ["--kb-inset", "--kb-top", "--kb-h"] {
                let _ = style.remove_property(p);
            }
        }
    }
    for el in held {
        el.set_scroll_top(el.scroll_height());
    }
}

/// The chats' message lists scrolled to (within a line of) their end.
fn at_end(doc: &web_sys::Document) -> Vec<web_sys::Element> {
    let Ok(list) = doc.query_selector_all(".pmsgs") else { return Vec::new() };
    (0..list.length())
        .filter_map(|i| list.item(i)?.dyn_into::<web_sys::Element>().ok())
        .filter(|el| el.client_height() > 0 && el.scroll_height() - el.scroll_top() - el.client_height() <= 24)
        .collect()
}

/// Once, at the app's start: follows the visual viewport and focus for as
/// long as the page lives. A browser without `visualViewport` gets nothing.
pub(crate) fn install() {
    let w = dom::window();
    let Some(vv) = w.visual_viewport() else { return };
    let cb = Closure::<dyn FnMut()>::new(apply);
    let f: &js_sys::Function = cb.as_ref().unchecked_ref();
    let _ = vv.add_event_listener_with_callback("resize", f);
    let _ = vv.add_event_listener_with_callback("scroll", f);
    let _ = w.add_event_listener_with_callback("resize", f);
    // Focus moves before the keyboard does; on its way out, the next field
    // may take it in the same turn, so it is read after that.
    let _ = w.add_event_listener_with_callback("focusin", f);
    cb.forget(); // lives as long as the app
    let out = Closure::<dyn FnMut()>::new(|| {
        let _ = dom::window().set_timeout_with_callback_and_timeout_and_arguments_0(
            Closure::once_into_js(apply).unchecked_ref(),
            0,
        );
    });
    let _ = w.add_event_listener_with_callback("focusout", out.as_ref().unchecked_ref());
    out.forget();
    apply();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An iPhone 13's Safari: 664 tall with its bars, a 336 keyboard.
    fn phone(vv_h: f64, vv_top: f64, editing: bool) -> Viewport {
        Viewport { inner_h: 664.0, vv_h, vv_top, scale: 1.0, tallest: 664.0, editing }
    }

    #[test]
    fn no_keyboard_without_a_field_or_without_the_shrink() {
        assert_eq!(keyboard(phone(664.0, 0.0, true)), None);
        assert_eq!(keyboard(phone(328.0, 0.0, false)), None);
        // The browser's own bars moving is not a keyboard.
        assert_eq!(keyboard(phone(600.0, 0.0, true)), None);
    }

    #[test]
    fn an_overlaid_keyboard_puts_the_bottom_on_its_top() {
        // iOS: the visual viewport 328 tall at the top.
        assert_eq!(keyboard(phone(328.0, 0.0, true)), Some(Keyboard { inset: 336.0, top: 0.0, height: 328.0 }));
        // iOS scrolled it down 200 to centre the field: the keyboard's top
        // is still the visible area's bottom.
        assert_eq!(keyboard(phone(328.0, 200.0, true)), Some(Keyboard { inset: 136.0, top: 200.0, height: 328.0 }));
        // Past the layout viewport's bottom (iOS overscroll): no negative inset.
        assert_eq!(keyboard(phone(328.0, 400.0, true)), Some(Keyboard { inset: 0.0, top: 400.0, height: 264.0 }));
    }

    #[test]
    fn a_resized_layout_is_a_keyboard_at_the_bottom() {
        // Android: the layout viewport itself shrank to the visible area.
        let v = Viewport { inner_h: 380.0, vv_h: 380.0, vv_top: 0.0, scale: 1.0, tallest: 720.0, editing: true };
        assert_eq!(keyboard(v), Some(Keyboard { inset: 0.0, top: 0.0, height: 380.0 }));
    }

    #[test]
    fn a_zoomed_page_is_left_alone() {
        let v = Viewport { scale: 2.0, ..phone(332.0, 100.0, true) };
        assert_eq!(keyboard(v), None);
    }

    #[test]
    fn typed_inputs_open_a_keyboard_and_others_do_not() {
        for t in ["", "text", "search", "email", "url", "tel", "password", "number"] {
            assert!(typed_input(t), "{t}");
        }
        for t in ["checkbox", "radio", "range", "color", "button", "submit", "file", "date"] {
            assert!(!typed_input(t), "{t}");
        }
    }
}
