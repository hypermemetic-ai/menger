//! CHAT-31: **one gesture for every phone sheet** — the queue's (APP-38),
//! the phone bar's tree and filter (APP-50) and the sections' sheet with
//! its chat (LOOK-3, CHAT-4). On an iPhone a drag on a sheet's handle used
//! to scroll the page behind it (the owner: "that slider should work and
//! right now they usually scroll the page behind (queue does this)"): the
//! handle's pointer listeners lost the touch to the page's pan, and the
//! queue's, the tree's and the filter's handles had none.
//!
//! - **The handle and the head drag the sheet** ([`Grip`] draws the handle
//!   and names the head): it follows the finger down, and is dismissed past
//!   a third of its height or on a flick down ([`dismisses`]); short of
//!   that it springs back.
//! - **The content scrolls inside the sheet**; at its top a downward drag
//!   becomes a sheet drag ([`mode`], [`takes_over`]) — the iOS sheet.
//! - **The page behind never moves** while a sheet is up: a drag that
//!   would reach the document is held (a non-passive `touchmove`,
//!   prevented), the sheet's scrollers contain their overscroll, and the
//!   document is locked (`html[data-sheet-lock]`) with its scroll held
//!   where the sheet found it ([`lock`]) — so closing leaves the page where
//!   it was.
//!
//! One set of listeners on the document, installed once ([`install`]);
//! each sheet registers itself through its [`Grip`]. A mouse drags the
//! head too (pointer events, not a touch's). At 821 px and up there are no
//! sheets and nothing here acts. What decides is pure and pinned by native
//! tests; the rest is the DOM.

use std::cell::{Cell, RefCell};

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::{dom, motion, phonebar};

/// Dragged down this share of its height, a sheet is dismissed.
pub(crate) const DISMISS_SHARE: f64 = 1.0 / 3.0;

/// A flick: released moving down at least this fast (px per ms)…
pub(crate) const FLICK_SPEED: f64 = 0.5;

/// …having come at least this far (px) — a tap's jitter is no flick.
pub(crate) const FLICK_MIN_PX: f64 = 24.0;

/// The speed is read over the drag's last this-many ms.
pub(crate) const FLICK_WINDOW_MS: f64 = 100.0;

/// Where a touch landed, with an open sheet up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Landed {
    /// The handle or the sheet's head.
    Head,
    /// The sheet's content: its nearest scroller, if any, at its top / at
    /// its end (none that scrolls is both).
    Body { top: bool, end: bool },
    /// Outside the sheet — its scrim, the bar, the page.
    Behind,
}

/// What a touch does, chosen at its first move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The sheet follows the finger; the browser does nothing.
    Drag,
    /// The browser's own: the content scrolls (or a row of pills sideways).
    Native,
    /// Nothing moves — above all not the page behind.
    Hold,
}

/// What a touch that landed at `landed` does, first moved by `dx`, `dy`
/// (px; `dy` > 0 down). None: it has not moved.
pub(crate) fn mode(landed: Landed, dx: f64, dy: f64) -> Option<Mode> {
    if dx == 0.0 && dy == 0.0 {
        return None;
    }
    let across = dx.abs() > dy.abs();
    Some(match landed {
        Landed::Behind => Mode::Hold,
        // Sideways inside the sheet: a row of pills or tabs scrolls.
        Landed::Head | Landed::Body { .. } if across => Mode::Native,
        Landed::Head => Mode::Drag,
        Landed::Body { top, .. } if dy > 0.0 => if top { Mode::Drag } else { Mode::Native },
        Landed::Body { end, .. } => if end { Mode::Hold } else { Mode::Native },
    })
}

/// Whether a touch scrolling the content natively is taken over as a sheet
/// drag: the content back at its `top`, the finger still going down (`dy`
/// this move), and the browser still letting it be stopped (`cancelable`).
pub(crate) fn takes_over(top: bool, dy: f64, cancelable: bool) -> bool {
    top && dy > 0.0 && cancelable
}

/// Where the sheet is drawn, dragged `dy` px: never above where it rests.
pub(crate) fn offset(dy: f64) -> f64 {
    dy.max(0.0)
}

/// The finger's speed down (px per ms) over the last [`FLICK_WINDOW_MS`]
/// of `samples` (`(ms, y)`, oldest first); 0 with fewer than two.
pub(crate) fn speed(samples: &[(f64, f64)]) -> f64 {
    let Some(&(t1, y1)) = samples.last() else { return 0.0 };
    let Some(&(t0, y0)) = samples.iter().find(|(t, _)| t1 - t <= FLICK_WINDOW_MS) else { return 0.0 };
    if t1 - t0 <= 0.0 {
        return 0.0;
    }
    (y1 - y0) / (t1 - t0)
}

/// Whether a sheet `height` px tall, released dragged down `dy` px moving
/// at `speed` (px per ms, down), is dismissed: past a third of it, or a
/// flick down.
pub(crate) fn dismisses(dy: f64, height: f64, speed: f64) -> bool {
    dy >= height * DISMISS_SHARE || (speed >= FLICK_SPEED && dy >= FLICK_MIN_PX)
}

// -- the sheets ---------------------------------------------------------------

/// A sheet, as its [`Grip`] registered it.
#[derive(Clone, Copy)]
struct Entry {
    /// The sheet's element id: what moves.
    id: &'static str,
    /// What of it is its head (a selector inside it).
    head: &'static str,
    open: Signal<bool>,
    dismiss: Callback<()>,
}

/// A touch or a mouse drag under way.
struct Gesture {
    sheet: Entry,
    landed: Landed,
    /// The content's scroller the touch landed in.
    scroller: Option<web_sys::Element>,
    mode: Option<Mode>,
    x0: f64,
    y0: f64,
    /// Where the sheet drag began (the start, or where it took over).
    from: f64,
    last: f64,
    samples: Vec<(f64, f64)>,
}

thread_local! {
    static SHEETS: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    static NOW: RefCell<Option<Gesture>> = const { RefCell::new(None) };
    /// The sheets holding the document, and where they hold it.
    static LOCKS: Cell<u32> = const { Cell::new(0) };
    static HELD: Cell<Option<f64>> = const { Cell::new(None) };
}

fn is_phone() -> bool {
    phonebar::is_phone(dom::window().inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(f64::MAX))
}

/// The open sheet, if one is up (one at a time, APP-50).
fn open_sheet() -> Option<Entry> {
    if !is_phone() {
        return None;
    }
    SHEETS.with(|s| s.borrow().iter().copied().find(|e| e.open.try_get_untracked().unwrap_or(false)))
}

fn element(id: &str) -> Option<web_sys::HtmlElement> {
    dom::window().document()?.get_element_by_id(id)?.dyn_into().ok()
}

/// The nearest element from `el` up to `stop` (included) that scrolls
/// vertically and has somewhere to go — a textarea too.
fn scroller(mut el: Option<web_sys::Element>, stop: Option<&web_sys::Element>) -> Option<web_sys::Element> {
    let w = dom::window();
    while let Some(e) = el {
        if e.scroll_height() > e.client_height() + 1 {
            if e.tag_name().eq_ignore_ascii_case("textarea") {
                return Some(e);
            }
            let oy = w.get_computed_style(&e).ok().flatten().and_then(|s| s.get_property_value("overflow-y").ok()).unwrap_or_default();
            if oy == "auto" || oy == "scroll" {
                return Some(e);
            }
        }
        if stop.is_some_and(|s| s == &e) {
            return None;
        }
        el = e.parent_element();
    }
    None
}

/// The content's state: at its top, at its end (none that scrolls: both).
fn ends(s: Option<&web_sys::Element>) -> (bool, bool) {
    s.map_or((true, true), |e| (e.scroll_top() <= 0, e.scroll_top() + e.client_height() >= e.scroll_height() - 1))
}

/// Where `target` is, for the open `sheet`. None: behind it, inside
/// something of its own that scrolls (a menu over the sheet) — not ours.
fn landed(sheet: &Entry, target: Option<web_sys::Element>) -> Option<(Landed, Option<web_sys::Element>)> {
    let el = element(sheet.id)?;
    let el: &web_sys::Element = el.as_ref();
    let Some(t) = target.clone().filter(|t| el.contains(Some(t))) else {
        return match scroller(target, None) {
            Some(_) => None,
            None => Some((Landed::Behind, None)),
        };
    };
    if t.closest(sheet.head).ok().flatten().is_some_and(|h| el.contains(Some(&h))) {
        return Some((Landed::Head, None));
    }
    let s = scroller(Some(t), Some(el));
    let (top, end) = ends(s.as_ref());
    Some((Landed::Body { top, end }, s))
}

/// Draw the sheet `dy` px down from where it rests, following the finger.
fn place(id: &str, dy: f64) {
    if let Some(p) = element(id) {
        let st = p.style();
        let _ = st.set_property("transition", "none");
        let _ = st.set_property("translate", &format!("0 {}px", offset(dy)));
    }
}

/// The drag let go: dismissed (the sheet leaves by its own transition, the
/// drag's offset kept until it has) or sprung back.
fn release(sheet: Entry, dy: f64, speed: f64) {
    let Some(p) = element(sheet.id) else { return };
    let height = p.get_bounding_client_rect().height();
    let st = p.style();
    let gone = dismisses(dy, height, speed);
    if gone {
        let _ = st.remove_property("transition");
        sheet.dismiss.run(());
    } else {
        // Springs back (LOOK-8: at once under Motion none).
        if motion::current() != motion::Motion::None {
            let _ = st.set_property("transition", "translate .2s cubic-bezier(.2, .9, .3, 1)");
        }
        let _ = st.set_property("translate", "0 0");
    }
    let clear = Closure::once_into_js(move || {
        if let Some(p) = element(sheet.id) {
            let _ = p.style().remove_property("translate");
            let _ = p.style().remove_property("transition");
        }
    });
    let _ = dom::window().set_timeout_with_callback_and_timeout_and_arguments_0(clear.unchecked_ref(), if gone { 340 } else { 220 });
}

fn touch_at(ev: &web_sys::TouchEvent) -> Option<(f64, f64)> {
    let t = ev.touches().get(0).or_else(|| ev.changed_touches().get(0))?;
    Some((f64::from(t.client_x()), f64::from(t.client_y())))
}

/// Begin a gesture at `(x, y)` on `target`; a `mouse` one only on a head,
/// and it drags at once.
fn begin(target: Option<web_sys::Element>, x: f64, y: f64, mouse: bool) -> bool {
    let Some(sheet) = open_sheet() else { return false };
    let Some((at, scroller)) = landed(&sheet, target) else { return false };
    if mouse && at != Landed::Head {
        return false;
    }
    let now = js_sys::Date::now();
    let mode = mouse.then_some(Mode::Drag);
    NOW.with(|g| *g.borrow_mut() = Some(Gesture { sheet, landed: at, scroller, mode, x0: x, y0: y, from: y, last: y, samples: vec![(now, y)] }));
    true
}

/// The gesture moved to `(x, y)`: whether the browser must not act on it.
fn moved(x: f64, y: f64, cancelable: bool) -> bool {
    NOW.with(|g| {
        let mut g = g.borrow_mut();
        let Some(g) = g.as_mut() else { return false };
        let step = y - g.last;
        g.last = y;
        let now = js_sys::Date::now();
        g.samples.push((now, y));
        g.samples.retain(|(t, _)| now - t <= FLICK_WINDOW_MS * 2.0);
        if g.mode.is_none() {
            let at = match g.landed {
                Landed::Body { .. } => {
                    let (top, end) = ends(g.scroller.as_ref());
                    Landed::Body { top, end }
                }
                l => l,
            };
            g.mode = mode(at, x - g.x0, y - g.y0);
        }
        if g.mode == Some(Mode::Native) && matches!(g.landed, Landed::Body { .. }) && takes_over(ends(g.scroller.as_ref()).0, step, cancelable) {
            g.mode = Some(Mode::Drag);
            g.from = y - step;
        }
        match g.mode {
            Some(Mode::Drag) => {
                place(g.sheet.id, y - g.from);
                true
            }
            Some(Mode::Hold) => true,
            _ => false,
        }
    })
}

/// The gesture ended (`cancel`: the browser took it).
fn ended(cancel: bool) {
    let Some(mut g) = NOW.with(|g| g.borrow_mut().take()) else { return };
    if g.mode == Some(Mode::Drag) {
        let dy = if cancel { 0.0 } else { g.last - g.from };
        // Let go where it stopped: a finger held still before it lifts is
        // no flick.
        g.samples.push((js_sys::Date::now(), g.last));
        release(g.sheet, dy, speed(&g.samples));
    }
}

fn on_touchstart(ev: web_sys::TouchEvent) {
    ended(true);
    if ev.touches().length() != 1 {
        return;
    }
    let Some((x, y)) = touch_at(&ev) else { return };
    begin(ev.target().and_then(|t| t.dyn_into().ok()), x, y, false);
}

fn on_touchmove(ev: web_sys::TouchEvent) {
    if ev.touches().length() != 1 {
        return;
    }
    let Some((x, y)) = touch_at(&ev) else { return };
    if moved(x, y, ev.cancelable()) && ev.cancelable() {
        ev.prevent_default();
    }
}

fn on_pointerdown(ev: web_sys::PointerEvent) {
    if ev.pointer_type() == "touch" || !crate::bindings::is_main(&ev) {
        return;
    }
    let target: Option<web_sys::Element> = ev.target().and_then(|t| t.dyn_into().ok());
    if begin(target.clone(), f64::from(ev.client_x()), f64::from(ev.client_y()), true) {
        // No text selection or native drag to cancel the pointer.
        ev.prevent_default();
        if let Some(t) = target {
            let _ = t.set_pointer_capture(ev.pointer_id());
        }
    }
}

fn on_pointermove(ev: web_sys::PointerEvent) {
    if ev.pointer_type() != "touch" {
        moved(f64::from(ev.client_x()), f64::from(ev.client_y()), true);
    }
}

/// The document held where the sheet found it: a scroll that moved it
/// while a sheet is up (iOS's own, past the lock) is put back.
fn hold() {
    let Some(y) = HELD.with(Cell::get) else { return };
    // CHAT-29: the full-screen chat over the page is the page on a phone —
    // it scrolls the document; a dock's sheet left up under it holds nothing.
    if crate::chat_full::is_up() {
        return;
    }
    let w = dom::window();
    if (w.scroll_y().unwrap_or(y) - y).abs() > 0.5 {
        w.scroll_to_with_x_and_y(w.scroll_x().unwrap_or(0.0), y);
    }
}

/// A sheet came up (`on`) or went down: the first locks the document and
/// holds its scroll, the last lets it go — where it was.
fn lock(on: bool) {
    let n = LOCKS.with(Cell::get);
    let n = if on { n + 1 } else { n.saturating_sub(1) };
    LOCKS.with(|c| c.set(n));
    let w = dom::window();
    let Some(html) = w.document().and_then(|d| d.document_element()) else { return };
    match (n, HELD.with(Cell::get)) {
        // Let go as it is: what the sheet's choice scrolled to (a section
        // picked, a page opened) is the reader's.
        (0, Some(_)) => {
            HELD.with(|c| c.set(None));
            let _ = html.remove_attribute("data-sheet-lock");
        }
        (1.., None) => {
            HELD.with(|c| c.set(Some(w.scroll_y().unwrap_or(0.0))));
            let _ = html.set_attribute("data-sheet-lock", "");
        }
        _ => {}
    }
}

/// Once, at the app's start: the listeners every sheet's gesture runs on.
pub(crate) fn install() {
    let w = dom::window();
    let Some(doc) = w.document() else { return };
    let listen = |name: &str, f: &js_sys::Function, passive: bool| {
        let opts = web_sys::AddEventListenerOptions::new();
        opts.set_capture(true);
        opts.set_passive(passive);
        let _ = doc.add_event_listener_with_callback_and_add_event_listener_options(name, f, &opts);
    };
    let start = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(on_touchstart);
    listen("touchstart", start.as_ref().unchecked_ref(), true);
    start.forget();
    // Not passive: a drag the sheet takes, or one that would reach the page,
    // is the browser's no more.
    let mv = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(on_touchmove);
    listen("touchmove", mv.as_ref().unchecked_ref(), false);
    mv.forget();
    let end = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(|_| ended(false));
    listen("touchend", end.as_ref().unchecked_ref(), true);
    end.forget();
    let cancel = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(|_| ended(true));
    listen("touchcancel", cancel.as_ref().unchecked_ref(), true);
    cancel.forget();
    let down = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(on_pointerdown);
    listen("pointerdown", down.as_ref().unchecked_ref(), false);
    down.forget();
    let pmv = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(on_pointermove);
    listen("pointermove", pmv.as_ref().unchecked_ref(), true);
    pmv.forget();
    let up = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(|ev: web_sys::PointerEvent| {
        if ev.pointer_type() != "touch" {
            ended(ev.type_() == "pointercancel");
        }
    });
    listen("pointerup", up.as_ref().unchecked_ref(), true);
    listen("pointercancel", up.as_ref().unchecked_ref(), true);
    up.forget();
    let held = Closure::<dyn FnMut()>::new(hold);
    let _ = w.add_event_listener_with_callback("scroll", held.as_ref().unchecked_ref());
    held.forget();
}

/// A sheet's handle, and the sheet registered with the gesture: `target`
/// its element id, `head` what of it drags it besides the handle (a
/// selector inside it), `open` whether it is up, `dismiss` what puts it
/// down. `class` is the handle's look, each sheet's own.
#[component]
pub(crate) fn Grip(target: &'static str, head: &'static str, #[prop(into)] open: Signal<bool>, dismiss: Callback<()>, class: &'static str) -> impl IntoView {
    SHEETS.with(|s| s.borrow_mut().push(Entry { id: target, head, open, dismiss }));
    // Up on a phone, it holds the document.
    let locked = StoredValue::new(false);
    Effect::new(move |_| {
        let up = open.get() && is_phone();
        if up != locked.get_value() {
            locked.set_value(up);
            lock(up);
        }
    });
    on_cleanup(move || {
        SHEETS.with(|s| s.borrow_mut().retain(|e| e.id != target));
        if locked.try_get_value().unwrap_or(false) {
            lock(false);
        }
    });
    view! { <div class=class data-grip=target aria-hidden="true"></div> }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The handle and the head drag the sheet down (and up only to where
    /// it rests); sideways, a row of pills in the head scrolls.
    #[test]
    fn the_head_drags_the_sheet() {
        assert_eq!(mode(Landed::Head, 0.0, 0.0), None, "not moved yet");
        assert_eq!(mode(Landed::Head, 1.0, 4.0), Some(Mode::Drag));
        assert_eq!(mode(Landed::Head, 0.0, -3.0), Some(Mode::Drag), "up too: held where it rests");
        assert_eq!(mode(Landed::Head, 6.0, 2.0), Some(Mode::Native), "sideways: the pills");
        assert_eq!(offset(-40.0), 0.0);
        assert_eq!(offset(120.0), 120.0);
    }

    /// The content scrolls inside; at its top a pull down drags the sheet,
    /// and at its end a push up moves nothing (never the page behind).
    #[test]
    fn the_content_scrolls_and_its_top_hands_over() {
        let mid = Landed::Body { top: false, end: false };
        let top = Landed::Body { top: true, end: false };
        let end = Landed::Body { top: false, end: true };
        let still = Landed::Body { top: true, end: true };
        assert_eq!(mode(mid, 0.0, 5.0), Some(Mode::Native));
        assert_eq!(mode(mid, 0.0, -5.0), Some(Mode::Native));
        assert_eq!(mode(top, 0.0, 5.0), Some(Mode::Drag));
        assert_eq!(mode(top, 0.0, -5.0), Some(Mode::Native));
        assert_eq!(mode(end, 0.0, -5.0), Some(Mode::Hold));
        assert_eq!(mode(end, 0.0, 5.0), Some(Mode::Native));
        assert_eq!(mode(still, 0.0, 5.0), Some(Mode::Drag), "nothing scrolls: it drags");
        assert_eq!(mode(still, 0.0, -5.0), Some(Mode::Hold));
        // Scrolled back to the top, still going down: the sheet's now —
        // while the browser lets it be taken.
        assert!(takes_over(true, 3.0, true));
        assert!(!takes_over(true, 3.0, false));
        assert!(!takes_over(false, 3.0, true));
        assert!(!takes_over(true, -3.0, true));
    }

    /// Outside the sheet — its scrim, the bar — a drag moves nothing: the
    /// page behind never scrolls.
    #[test]
    fn behind_the_sheet_nothing_moves() {
        assert_eq!(mode(Landed::Behind, 0.0, 12.0), Some(Mode::Hold));
        assert_eq!(mode(Landed::Behind, 0.0, -12.0), Some(Mode::Hold));
        assert_eq!(mode(Landed::Behind, 12.0, 0.0), Some(Mode::Hold));
    }

    /// 390 row: dismissed past a third of the sheet, or on a flick down;
    /// short of both it springs back.
    #[test]
    fn a_third_or_a_flick_dismisses() {
        assert!(!dismisses(10.0, 600.0, 0.0));
        assert!(!dismisses(199.0, 600.0, 0.1));
        assert!(dismisses(200.0, 600.0, 0.0));
        assert!(dismisses(80.0, 240.0, 0.0), "a short sheet: a third of it");
        assert!(dismisses(40.0, 600.0, 0.8), "a flick");
        assert!(!dismisses(10.0, 600.0, 2.0), "a tap's jitter is no flick");
        assert!(!dismisses(150.0, 600.0, -1.0), "flicked back up");
    }

    /// The speed is the last 100 ms of the drag's.
    #[test]
    fn the_speed_is_the_last_moments() {
        assert_eq!(speed(&[]), 0.0);
        assert_eq!(speed(&[(0.0, 10.0)]), 0.0);
        assert_eq!(speed(&[(0.0, 0.0), (50.0, 30.0), (100.0, 80.0)]), 0.8);
        // A slow start, then a quick last stretch: the last stretch.
        assert_eq!(speed(&[(0.0, 0.0), (400.0, 20.0), (450.0, 50.0), (500.0, 80.0)]), 0.6);
        // Held still at the end: no flick.
        assert_eq!(speed(&[(0.0, 0.0), (50.0, 100.0), (300.0, 100.0)]), 0.0);
    }
}
