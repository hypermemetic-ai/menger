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
//!
//! APP-58: and the keyboard slides *over* the view. iOS pans the visual
//! viewport down to bring the field into view (and, where the document
//! scrolls, may scroll it too), so anything left at the layout viewport's
//! top slides up and away with the pan. While a field inside a pinned view
//! — the full-screen chat, or a sheet above the bar — is typed in:
//!
//! - the views are pinned to the visual viewport (`--kb-top`, `--kb-h`),
//!   read again every frame while it moves ([`follow`]), so the header holds
//!   still at the visible area's top as the pan runs, not after it;
//! - the document stays where it was ([`hold_scroll`]): a scroll iOS makes
//!   to centre the field is put back, and the page under the view is where
//!   it was when the keyboard goes;
//! - a drag outside anything that scrolls moves nothing ([`guard_drag`]),
//!   so the pinned view cannot be dragged off the visible area.
//!
//! APP-61: but a touch in a field is the field's when it is editing text:
//! one that lands on (or within a handle's reach of) the focused field's
//! selection drags a handle, one that rests before it moves is iOS's caret
//! loupe or a held selection, and one still within [`SLOP`] of where it
//! landed has not chosen yet ([`owner`]). Cancelling those moves is what
//! froze a selection's handles in the composer. A swipe across the field,
//! or a drag on the header, is still held.
//!
//! APP-62: and the layout viewport is measured, never read off
//! `innerHeight`. WebKit on iOS reports `window.innerHeight` as the
//! *visual* viewport's height, so with the keyboard up it shrinks with
//! it; sizing the view `min(vv.height, innerHeight − offsetTop)` then
//! took the pan off twice (`vv.height − offsetTop`: an iPhone's 460 px
//! visible area panned 276 down left a 184 px chat, its header and
//! composer, and the page showing below). What a `position: fixed` view
//! is laid out in is read off a fixed probe (`#kbprobe`, `inset-block: 0`),
//! and the visible area is the visual viewport's own rect — top
//! `offsetTop`, height `vv.height` — whatever `innerHeight` says.

use std::cell::Cell;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::dom;

/// Less than this between the layout and the visual height is the
/// browser's own bars moving, not a keyboard.
const KEYBOARD_MIN_PX: f64 = 120.0;

/// Never a visible area shorter than this: the chat's head and composer.
const MIN_VIEW_PX: f64 = 160.0;

/// What the viewport says, at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Viewport {
    /// The layout viewport's height as a `position: fixed` view sees it
    /// (`#kbprobe`; `innerHeight` only where there is no probe — on iOS
    /// `innerHeight` is the visual viewport's, APP-62).
    pub layout_h: f64,
    /// `visualViewport.height`, `.offsetTop`, `.scale`.
    pub vv_h: f64,
    pub vv_top: f64,
    pub scale: f64,
    /// The tallest `layout_h` seen at this width: the height with no
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
    if !v.editing || v.layout_h <= 0.0 || v.vv_h <= 0.0 || v.scale > 1.01 {
        return None;
    }
    // Overlaid (iOS): the layout viewport keeps its height, the visual one
    // shrinks. Resized (Android): the layout viewport itself shrank.
    let overlaid = v.layout_h - v.vv_h >= KEYBOARD_MIN_PX;
    let resized = v.tallest - v.layout_h >= KEYBOARD_MIN_PX;
    if !overlaid && !resized {
        return None;
    }
    // APP-62: the visible area is the visual viewport's own rect, in the
    // layout viewport's coordinates (where a fixed view is placed): its
    // top is the pan, its height the visual height — never the layout
    // height less the pan, which takes the pan off twice where the layout
    // height reads short (iOS's innerHeight). Below the visible area's
    // bottom to the layout viewport's is the keyboard's inset.
    let top = v.vv_top.max(0.0);
    let height = v.vv_h.max(MIN_VIEW_PX);
    Some(Keyboard { inset: (v.layout_h - top - v.vv_h).max(0.0).round(), top: top.round(), height: height.round() })
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

/// The views pinned to the visual viewport while their field is typed in
/// (APP-58): the full-screen chat and every sheet placed above the bar.
const PINNED: &str = "#chatfull, #tdock-sheet, #lens-top, #nav, #palette, #qtray";

/// Frames to keep reading the viewport after something moved it (about
/// half a second: the keyboard's and the pan's animations).
const FOLLOW_FRAMES: u32 = 30;

/// How many more frames to follow: a change starts the count again, a
/// still frame takes one off.
pub(crate) fn frames_left(left: u32, changed: bool) -> u32 {
    if changed { left.max(FOLLOW_FRAMES) } else { left.saturating_sub(1) }
}

/// What to do with the document's scroll as the keyboard comes and goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Hold {
    /// Nothing to do.
    Keep,
    /// The keyboard came up over a pinned view: remember where the page is.
    Lock(f64),
    /// It went: put the page back where it was.
    Restore(f64),
}

/// `was` / `now`: a keyboard over a pinned view, before and after; `y` the
/// document's scroll now, `locked` the one remembered.
pub(crate) fn hold(was: bool, now: bool, y: f64, locked: Option<f64>) -> Hold {
    match (was, now, locked) {
        (false, true, _) => Hold::Lock(y),
        (true, false, Some(at)) | (true, true, Some(at)) if (y - at).abs() > 0.5 => Hold::Restore(at),
        _ => Hold::Keep,
    }
}

/// How far (CSS px) a touch in a field may drift before it has chosen:
/// within it, it may still become the field's.
const SLOP: f64 = 10.0;

/// How long (ms) a touch in a field rests before a move is the field's —
/// under iOS's own long press (the loupe, a held selection), so its first
/// move is never cancelled.
const REST_MS: f64 = 300.0;

/// How far (CSS px) outside the focused field a selection's handle reaches:
/// its knob hangs past the text line, over the field's edge.
const HANDLE_PX: f64 = 32.0;

/// Whose a touch's move is while the keyboard is up over a pinned view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Owner {
    /// The field's: a handle, the loupe, a held selection — never cancelled.
    Field,
    /// Not chosen yet: let through, asked again on the next move.
    Open,
    /// The page's: held unless it is in something that scrolls.
    Page,
}

/// What a touch is, from how it began (`in_field`: on a field that takes
/// typing; `handles`: within reach of the focused field's selection) and
/// where it is now — `rested` ms after it landed, `moved` px from there.
pub(crate) fn owner(in_field: bool, handles: bool, rested: f64, moved: f64) -> Owner {
    if handles || (in_field && rested >= REST_MS) {
        Owner::Field
    } else if in_field && moved <= SLOP {
        Owner::Open
    } else {
        Owner::Page
    }
}

/// The touch in progress (APP-61): where and when it landed, what it landed
/// on, and its owner once chosen — a choice holds for the rest of the touch.
#[derive(Clone, Copy)]
struct Press {
    at: f64,
    x: f64,
    y: f64,
    in_field: bool,
    handles: bool,
    chosen: Option<Owner>,
}

thread_local! {
    static PRESS: Cell<Option<Press>> = const { Cell::new(None) };
    static TALLEST: Cell<(f64, f64)> = const { Cell::new((0.0, 0.0)) };
    static SHOWN: Cell<Option<Keyboard>> = const { Cell::new(None) };
    /// APP-58: the document's scroll when the keyboard came up over a
    /// pinned view; `None` while no such keyboard is up.
    static LOCKED: Cell<Option<f64>> = const { Cell::new(None) };
    /// Frames left to follow, and the frame callback.
    static FOLLOW: Cell<u32> = const { Cell::new(0) };
    static FRAME: std::cell::RefCell<Option<js_sys::Function>> = const { std::cell::RefCell::new(None) };
}

fn num(r: Result<JsValue, JsValue>) -> f64 {
    r.ok().and_then(|v| v.as_f64()).unwrap_or(0.0)
}

/// Whether the focused field is inside a view pinned to the keyboard.
fn in_pinned(doc: &web_sys::Document) -> bool {
    doc.active_element().and_then(|el| el.closest(PINNED).ok().flatten()).is_some()
}

/// APP-62: the probe's id — a `position: fixed` box from the layout
/// viewport's top to its bottom (`index.html`), invisible and inert.
const PROBE: &str = "kbprobe";

/// The layout viewport's height as a fixed view is laid out in it: the
/// probe's. `innerHeight` is not it on iOS (it follows the visual
/// viewport there).
fn layout_height(doc: &web_sys::Document) -> Option<f64> {
    let h = doc.get_element_by_id(PROBE)?.get_bounding_client_rect().height();
    (h > 0.0).then_some(h)
}

/// Once: the probe, at the end of `<body>`.
fn mount_probe(doc: &web_sys::Document) {
    if doc.get_element_by_id(PROBE).is_some() {
        return;
    }
    let (Ok(el), Some(body)) = (doc.create_element("div"), doc.body()) else { return };
    el.set_id(PROBE);
    let _ = el.set_attribute("aria-hidden", "true");
    let _ = body.append_child(&el);
}

/// Reads the viewport now and stamps (or clears) `<html>`; whether it
/// changed anything.
fn apply() -> bool {
    let w = dom::window();
    let Some(vv) = w.visual_viewport() else { return false };
    let Some(doc) = w.document() else { return false };
    let inner_w = num(w.inner_width());
    let layout_h = layout_height(&doc).unwrap_or_else(|| num(w.inner_height()));
    // The tallest height is per width: a rotation starts it again.
    let tallest = TALLEST.with(|c| {
        let (width, tallest) = c.get();
        let t = if width == inner_w { tallest.max(layout_h) } else { layout_h };
        c.set((inner_w, t));
        t
    });
    let editing = doc.active_element().is_some_and(|el| takes_typing(&el));
    let kb = keyboard(Viewport { layout_h, vv_h: vv.height(), vv_top: vv.offset_top(), scale: vv.scale(), tallest, editing });
    // APP-58: a keyboard over a pinned view holds the document still.
    let pinned = kb.is_some() && in_pinned(&doc);
    let locked = LOCKED.with(Cell::get);
    match hold(locked.is_some(), pinned, w.scroll_y().unwrap_or(0.0), locked) {
        Hold::Lock(y) => LOCKED.with(|c| c.set(Some(y))),
        Hold::Restore(y) => {
            w.scroll_to_with_x_and_y(w.scroll_x().unwrap_or(0.0), y);
            if !pinned {
                LOCKED.with(|c| c.set(None));
            }
        }
        Hold::Keep if !pinned => LOCKED.with(|c| c.set(None)),
        Hold::Keep => {}
    }
    if SHOWN.with(Cell::get) == kb {
        return false;
    }
    SHOWN.with(|c| c.set(kb));
    let Some(html) = doc.document_element().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) else { return true };
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
    true
}

/// APP-58: something moved the viewport (focus, the keyboard, a pan):
/// read it every frame until it has been still for [`FOLLOW_FRAMES`], so a
/// pinned view tracks iOS's pan as it runs. It places; it animates nothing.
fn follow() {
    let idle = FOLLOW.with(|c| {
        let idle = c.get() == 0;
        c.set(c.get().max(FOLLOW_FRAMES));
        idle
    });
    if idle {
        next_frame();
    }
}

fn next_frame() {
    FRAME.with(|f| {
        if let Some(cb) = f.borrow().as_ref() {
            let _ = dom::window().request_animation_frame(cb);
        }
    });
}

fn frame() {
    let changed = apply();
    let left = FOLLOW.with(|c| {
        let n = frames_left(c.get(), changed);
        c.set(n);
        n
    });
    if left > 0 {
        next_frame();
    }
}

/// APP-58: a scroll of the document while the keyboard is up over a pinned
/// view is put back — iOS scrolls it to centre the field, which the pinned
/// view already shows.
fn hold_scroll() {
    if LOCKED.with(Cell::get).is_some() {
        apply();
        follow();
    }
}

/// Whether `el` or an ancestor below the pinned view scrolls vertically
/// (and has somewhere to go): a drag there is the list's, not the page's.
fn scrolls(mut el: Option<web_sys::Element>) -> bool {
    let w = dom::window();
    while let Some(e) = el {
        if e.matches(PINNED).unwrap_or(false) {
            return false;
        }
        if e.scroll_height() > e.client_height() + 1 {
            if e.tag_name().eq_ignore_ascii_case("textarea") {
                return true;
            }
            let oy = w.get_computed_style(&e).ok().flatten().and_then(|s| s.get_property_value("overflow-y").ok()).unwrap_or_default();
            if oy == "auto" || oy == "scroll" {
                return true;
            }
        }
        el = e.parent_element();
    }
    false
}

/// The field `el` is in, if it takes typing.
fn field_of(el: &web_sys::Element) -> Option<web_sys::Element> {
    el.closest("textarea, input, [contenteditable]:not([contenteditable='false'])").ok().flatten().filter(takes_typing)
}

/// Whether the focused field holds a selection (not a caret) and `(x, y)`
/// is within a handle's reach of it.
fn near_selection(doc: &web_sys::Document, x: f64, y: f64) -> bool {
    let Some(field) = doc.active_element().filter(takes_typing) else { return false };
    let selected = if let Some(t) = field.dyn_ref::<web_sys::HtmlTextAreaElement>() {
        t.selection_start().ok().flatten() != t.selection_end().ok().flatten()
    } else if let Some(i) = field.dyn_ref::<web_sys::HtmlInputElement>() {
        i.selection_start().ok().flatten() != i.selection_end().ok().flatten()
    } else {
        dom::window().get_selection().ok().flatten().is_some_and(|s| !s.is_collapsed() && field.contains(s.anchor_node().as_ref()))
    };
    let r = field.get_bounding_client_rect();
    selected && x >= r.left() - HANDLE_PX && x <= r.right() + HANDLE_PX && y >= r.top() - HANDLE_PX && y <= r.bottom() + HANDLE_PX
}

fn point(ev: &web_sys::TouchEvent) -> Option<(f64, f64)> {
    let t = ev.touches().get(0)?;
    Some((f64::from(t.client_x()), f64::from(t.client_y())))
}

/// APP-61: a touch lands — what it landed on is kept for its moves.
fn press(ev: web_sys::TouchEvent) {
    let (Some((x, y)), Some(doc)) = (point(&ev), dom::window().document()) else {
        PRESS.with(|p| p.set(None));
        return;
    };
    let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
    let in_field = target.as_ref().and_then(field_of).is_some();
    let handles = ev.touches().length() == 1 && near_selection(&doc, x, y);
    PRESS.with(|p| p.set(Some(Press { at: js_sys::Date::now(), x, y, in_field, handles, chosen: None })));
}

/// Whose this move is, chosen once per touch (a touch whose landing went
/// unheard is the page's).
fn owner_now(ev: &web_sys::TouchEvent) -> Owner {
    let Some(mut p) = PRESS.with(Cell::get) else { return Owner::Page };
    if let Some(o) = p.chosen {
        return o;
    }
    let (x, y) = point(ev).unwrap_or((p.x, p.y));
    let o = owner(p.in_field, p.handles, js_sys::Date::now() - p.at, (x - p.x).hypot(y - p.y));
    if o != Owner::Open {
        p.chosen = Some(o);
        PRESS.with(|c| c.set(Some(p)));
    }
    o
}

/// APP-58: while the keyboard is up over a pinned view, a drag on what does
/// not scroll (the header, the composer's edge) would pan the visual
/// viewport and take the view with it: it moves nothing. APP-61: unless it
/// is the field's ([`owner`]).
fn guard_drag(ev: web_sys::TouchEvent) {
    if LOCKED.with(Cell::get).is_none() {
        return;
    }
    if owner_now(&ev) != Owner::Page {
        return;
    }
    let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
    if !scrolls(target) {
        ev.prevent_default();
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
    if let Some(doc) = w.document() {
        mount_probe(&doc);
    }
    let tick = Closure::<dyn FnMut()>::new(frame);
    FRAME.with(|f| *f.borrow_mut() = Some(tick.as_ref().unchecked_ref::<js_sys::Function>().clone()));
    tick.forget(); // lives as long as the app
    let cb = Closure::<dyn FnMut()>::new(|| {
        apply();
        follow();
    });
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
            Closure::once_into_js(|| {
                apply();
                follow();
            })
            .unchecked_ref(),
            0,
        );
    });
    let _ = w.add_event_listener_with_callback("focusout", out.as_ref().unchecked_ref());
    out.forget();
    // APP-58: the document held still, and a drag on what does not scroll
    // held too, while the keyboard is up over a pinned view.
    let held = Closure::<dyn FnMut()>::new(hold_scroll);
    let _ = w.add_event_listener_with_callback("scroll", held.as_ref().unchecked_ref());
    held.forget();
    let drag = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(guard_drag);
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_passive(false);
    // APP-61: where each touch landed, read before anything else hears it.
    let landed = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(press);
    let first = web_sys::AddEventListenerOptions::new();
    first.set_passive(true);
    first.set_capture(true);
    if let Some(doc) = w.document() {
        let _ = doc.add_event_listener_with_callback_and_add_event_listener_options("touchmove", drag.as_ref().unchecked_ref(), &opts);
        let _ = doc.add_event_listener_with_callback_and_add_event_listener_options("touchstart", landed.as_ref().unchecked_ref(), &first);
    }
    drag.forget();
    landed.forget();
    apply();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An iPhone 13's Safari: 664 tall with its bars, a 336 keyboard.
    fn phone(vv_h: f64, vv_top: f64, editing: bool) -> Viewport {
        Viewport { layout_h: 664.0, vv_h, vv_top, scale: 1.0, tallest: 664.0, editing }
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
        // Past the layout viewport's bottom (iOS overscroll): no negative
        // inset, and still the whole visible area (APP-62).
        assert_eq!(keyboard(phone(328.0, 400.0, true)), Some(Keyboard { inset: 0.0, top: 400.0, height: 328.0 }));
    }

    #[test]
    fn the_visible_area_is_the_visual_viewport_whatever_inner_height_says() {
        // APP-62, the owner's iPhone (440 × 956, Safari's bar at the top):
        // the layout viewport 834, the keyboard and its accessory bar 374,
        // the visible area 460, panned 276 down. WebKit's innerHeight was
        // the visual viewport's 460, and the old sizing made the chat
        // 460 − 276 = 184. The layout height is the fixed probe's: 834.
        let v = Viewport { layout_h: 834.0, vv_h: 460.0, vv_top: 276.0, scale: 1.0, tallest: 834.0, editing: true };
        assert_eq!(keyboard(v), Some(Keyboard { inset: 98.0, top: 276.0, height: 460.0 }));
        // No probe and innerHeight the visual one: still a keyboard (the
        // layout height fell from its tallest) and still the visible area.
        let v = Viewport { layout_h: 460.0, ..v };
        assert_eq!(keyboard(v).map(|k| (k.top, k.height)), Some((276.0, 460.0)));
        // A visible area shorter than the head and composer: never shorter
        // than them.
        let v = Viewport { layout_h: 390.0, vv_h: 120.0, vv_top: 0.0, scale: 1.0, tallest: 390.0, editing: true };
        assert_eq!(keyboard(v).map(|k| k.height), Some(MIN_VIEW_PX));
    }

    #[test]
    fn a_resized_layout_is_a_keyboard_at_the_bottom() {
        // Android: the layout viewport itself shrank to the visible area.
        let v = Viewport { layout_h: 380.0, vv_h: 380.0, vv_top: 0.0, scale: 1.0, tallest: 720.0, editing: true };
        assert_eq!(keyboard(v), Some(Keyboard { inset: 0.0, top: 0.0, height: 380.0 }));
    }

    #[test]
    fn a_zoomed_page_is_left_alone() {
        let v = Viewport { scale: 2.0, ..phone(332.0, 100.0, true) };
        assert_eq!(keyboard(v), None);
    }

    #[test]
    fn following_lasts_while_the_viewport_moves_then_stops() {
        // A move starts the count; still frames run it down to nothing.
        let mut left = frames_left(0, true);
        assert_eq!(left, FOLLOW_FRAMES);
        for _ in 0..FOLLOW_FRAMES - 1 {
            left = frames_left(left, false);
        }
        assert_eq!(left, 1);
        // Another move mid-way starts it again.
        assert_eq!(frames_left(left, true), FOLLOW_FRAMES);
        assert_eq!(frames_left(frames_left(left, false), false), 0);
    }

    #[test]
    fn the_document_is_held_while_the_keyboard_is_over_a_pinned_view() {
        // The keyboard comes up: where the page is, remembered.
        assert_eq!(hold(false, true, 420.0, None), Hold::Lock(420.0));
        // iOS scrolls the document to centre the field: put back.
        assert_eq!(hold(true, true, 560.0, Some(420.0)), Hold::Restore(420.0));
        assert_eq!(hold(true, true, 420.0, Some(420.0)), Hold::Keep);
        // The keyboard goes: the page is where it was.
        assert_eq!(hold(true, false, 300.0, Some(420.0)), Hold::Restore(420.0));
        assert_eq!(hold(true, false, 420.0, Some(420.0)), Hold::Keep);
        // No keyboard over a pinned view (a field in the page): left alone.
        assert_eq!(hold(false, false, 560.0, None), Hold::Keep);
    }

    /// APP-61: a selection's handle, the loupe and a held selection are the
    /// field's; a swipe across it, or a drag outside it, is the page's.
    #[test]
    fn a_touch_in_a_field_is_the_fields_when_it_edits_text() {
        // A handle: dragged at once, however far.
        assert_eq!(owner(true, true, 0.0, 0.0), Owner::Field);
        assert_eq!(owner(true, true, 20.0, 80.0), Owner::Field);
        // Its knob hangs past the field: still the handle's.
        assert_eq!(owner(false, true, 20.0, 40.0), Owner::Field);
        // Rested, then moved: the caret's loupe or a held selection.
        assert_eq!(owner(true, false, REST_MS, 0.0), Owner::Field);
        assert_eq!(owner(true, false, 600.0, 60.0), Owner::Field);
        // In the field, not chosen yet: a jitter is let through.
        assert_eq!(owner(true, false, 40.0, 3.0), Owner::Open);
        assert_eq!(owner(true, false, 120.0, SLOP), Owner::Open);
        // A swipe across the field before it rested: held, as APP-58.
        assert_eq!(owner(true, false, 80.0, SLOP + 1.0), Owner::Page);
        // Outside any field, with no selection: the page's, at once.
        assert_eq!(owner(false, false, 0.0, 0.0), Owner::Page);
        assert_eq!(owner(false, false, 900.0, 0.0), Owner::Page);
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
