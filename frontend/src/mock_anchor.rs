//! APP-45: annotation inside mockups — an anchor that names a mockup and an
//! element in its page, and the keyboard path for choosing one.
//!
//! A mockup's devices are frames the host draws (src/mockup.rs,
//! src/present.rs, src/mocksize.rs): several `<iframe>`s showing one page at
//! several widths, none stamped with a ticket of its own (an `<m-mockup>`'s)
//! or all stamped with the same one (a `view: mockup` body's) — so a framed
//! path's first half (`iframe[data-ticket="…"]`, src/anchor.rs) cannot say
//! which. A comment left inside one names **the mockup**, not the frame:
//!
//! ```text
//! mockup:APP-45#2@1440 >> body > main > h1          the body's 2nd <m-mockup> (a fence)
//! mockup:APP-45#3=DESIGN-49@390 >> #cta             its 3rd, which shows DESIGN-49's page
//! mockup:DESIGN-49@768 >> section:nth-of-type(2)     DESIGN-49's own body, `view: mockup`
//! ```
//!
//! — the ticket whose body holds it, the placeholder's number in that body
//! (none: the body itself is the mockup), the referenced page when it is a
//! `ref`, and the device width it was made at; then [`anchor::FRAME_SEP`]
//! and the element's selector in the page, chosen as in any framed body.
//! The first half is no CSS selector: [`find_device`] reads it and finds
//! the device frame showing that mockup now — the one at the same width,
//! else the nearest shown — so a comment re-finds its element at any width
//! (and wherever a later view, APP-44's compare, draws the same mockup and
//! marks it as src/mockup.rs does). A `ref` whose placeholder moved in the
//! body is found by its ref. Every older anchor reads as before: a path
//! that does not begin `mockup:` is a CSS selector, as ever.
//!
//! **The keyboard path**: in annotation mode, with a mockup on the page,
//! `]` / `[` step from device to device, ↓ ↑ to the next or previous
//! element, → into the first child, ← out to the parent, and Enter opens
//! the comment box on the element chosen — the frame walks its own
//! document (`menger:walk { step }`, the SDK and the page shim) and says
//! what is chosen as a hover does (`menger:hover`), so the host draws it
//! the same way.
//!
//! What decides — the anchor's form, read and written, and which device a
//! comment is drawn on — is pure and pinned by native tests; the rest is
//! DOM.

use std::cell::RefCell;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::anchor::{self, FRAME_SEP};
use crate::bindings::{self, Context};
use crate::{annotate, dom, frame, keys, mockup};

/// What a mockup anchor's first half begins with.
pub(crate) const PREFIX: &str = "mockup:";

/// A mockup, as an anchor names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MockRef {
    /// The ticket whose body holds the mockup (or is it).
    pub(crate) ticket: String,
    /// The placeholder's number in that body; `None`: the body itself, as
    /// `view: mockup`.
    pub(crate) mock: Option<u32>,
    /// The ticket whose page the placeholder shows, for a `ref`.
    pub(crate) reference: Option<String>,
    /// The device width (CSS px) the comment was left at.
    pub(crate) width: u32,
}

fn id_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 160 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/'))
}

impl MockRef {
    /// `mockup:T#n=R@w` — the first half of a path.
    pub(crate) fn text(&self) -> String {
        let mut s = format!("{PREFIX}{}", self.ticket);
        if let Some(n) = self.mock {
            s.push_str(&format!("#{n}"));
            if let Some(r) = &self.reference {
                s.push('=');
                s.push_str(r);
            }
        }
        s.push_str(&format!("@{}", self.width));
        s
    }

    /// The first half of a path, read — `None` for anything else (a CSS
    /// selector: every anchor older than APP-45).
    pub(crate) fn parse(s: &str) -> Option<MockRef> {
        let rest = s.trim().strip_prefix(PREFIX)?;
        let (head, width) = rest.rsplit_once('@')?;
        let width: u32 = width.parse().ok().filter(|w| (1..=100_000).contains(w))?;
        let (ticket, mock, reference) = match head.split_once('#') {
            None => (head, None, None),
            Some((t, m)) => {
                let (n, r) = match m.split_once('=') {
                    Some((n, r)) => (n, Some(r)),
                    None => (m, None),
                };
                let n: u32 = n.parse().ok()?;
                if r.is_some_and(|r| !id_ok(r)) {
                    return None;
                }
                (t, Some(n), r.map(str::to_string))
            }
        };
        id_ok(ticket).then(|| MockRef { ticket: ticket.to_string(), mock, reference, width })
    }

    /// In words, for the comment box and the list: "mockup 2 · 1440 px".
    pub(crate) fn words(&self) -> String {
        match (self.mock, &self.reference) {
            (Some(n), Some(r)) => format!("mockup {n} ({r}) · {} px", self.width),
            (Some(n), None) => format!("mockup {n} · {} px", self.width),
            (None, _) => format!("mockup · {} px", self.width),
        }
    }
}

/// The mockup a whole path names, if it names one.
pub(crate) fn of_path(path: &str) -> Option<MockRef> {
    anchor::split(path).0.and_then(MockRef::parse)
}

/// Which of the devices shown — `(index, width)` — a comment made at
/// `width` is drawn on: the same width, else the nearest (the narrower on
/// a tie).
pub(crate) fn nearest(shown: &[(usize, u32)], width: u32) -> Option<usize> {
    shown.iter().min_by_key(|(_, w)| (w.abs_diff(width), *w)).map(|(i, _)| *i)
}

// -- the DOM half -------------------------------------------------------------

fn query_all(root: &web_sys::Element, sel: &str) -> Vec<web_sys::Element> {
    let Ok(list) = root.query_selector_all(sel) else { return Vec::new() };
    (0..list.length()).filter_map(|i| list.item(i)).filter_map(|n| n.dyn_into::<web_sys::Element>().ok()).collect()
}

fn doc_all(sel: &str) -> Vec<web_sys::Element> {
    dom::window().document().and_then(|d| d.document_element()).map(|r| query_all(&r, sel)).unwrap_or_default()
}

/// A device's frame — the one on screen when a re-render has two.
fn device_frame(device: &web_sys::Element) -> Option<web_sys::HtmlIFrameElement> {
    let frames: Vec<web_sys::HtmlIFrameElement> =
        query_all(device, &format!("iframe.{}", frame::FRAME_CLASS)).into_iter().filter_map(|f| f.dyn_into().ok()).collect();
    frames.iter().find(|f| f.get_attribute("data-state").as_deref() == Some("live")).or(frames.first()).cloned()
}

fn width_of(device: &web_sys::Element) -> Option<u32> {
    device.get_attribute("data-w").and_then(|w| w.parse().ok())
}

/// The box holding mockup `m`'s devices now.
fn holder(m: &MockRef) -> Option<web_sys::Element> {
    let t = anchor::quote(&m.ticket);
    let live = |els: Vec<web_sys::Element>| {
        els.iter().find(|e| !e.has_attribute("data-pending")).or(els.first()).cloned()
    };
    match m.mock {
        Some(n) => {
            let at = live(doc_all(&format!(".{}[{}={t}][data-mock=\"{n}\"]", mockup::BOX_CLASS, mockup::TICKET_ATTR)));
            // A `ref` whose placeholder moved in the body: found by its ref.
            match (&m.reference, at) {
                (Some(r), at) if at.as_ref().and_then(|e| e.get_attribute(mockup::REF_ATTR)).as_deref() != Some(r.as_str()) => {
                    live(doc_all(&format!(".{}[{}={t}][{}={}]", mockup::BOX_CLASS, mockup::TICKET_ATTR, mockup::REF_ATTR, anchor::quote(r))))
                }
                (_, at) => at,
            }
        }
        None => doc_all(&format!(".framed[data-view=\"mockup\"] iframe.{}[data-ticket={t}]", frame::FRAME_CLASS))
            .into_iter()
            .find(|f| f.closest(&format!(".{}", mockup::BOX_CLASS)).ok().flatten().is_none())
            .and_then(|f| f.closest(".framed").ok().flatten()),
    }
}

/// The device frame showing mockup `m` now, at its width or the nearest.
pub(crate) fn find_device(m: &MockRef) -> Option<web_sys::HtmlIFrameElement> {
    let devices: Vec<web_sys::Element> = query_all(&holder(m)?, ".device:not(.hidden)");
    let shown: Vec<(usize, u32)> = devices.iter().enumerate().filter_map(|(i, d)| width_of(d).map(|w| (i, w))).collect();
    device_frame(devices.get(nearest(&shown, m.width)?)?)
}

/// The mockup a device frame shows, as an anchor names it — `None` for a
/// frame in no mockup, or one whose body is no ticket's (a draft).
pub(crate) fn of_frame(f: &web_sys::Element) -> Option<MockRef> {
    let device = f.closest(".device").ok().flatten()?;
    let width = width_of(&device)?;
    match f.closest(&format!(".{}", mockup::BOX_CLASS)).ok().flatten() {
        Some(b) => Some(MockRef {
            ticket: b.get_attribute(mockup::TICKET_ATTR).filter(|t| id_ok(t))?,
            mock: Some(b.get_attribute("data-mock")?.parse().ok()?),
            reference: b.get_attribute(mockup::REF_ATTR).filter(|r| id_ok(r)),
            width,
        }),
        None => Some(MockRef { ticket: f.get_attribute("data-ticket").filter(|t| id_ok(t))?, mock: None, reference: None, width }),
    }
}

/// Whether a frame is a mockup's device (its own scroller, scaled).
pub(crate) fn is_device(f: &web_sys::Element) -> bool {
    f.closest(".device").ok().flatten().is_some()
}

/// src/anchor.rs: the frame a path's first half names — a mockup's device,
/// or `None` when it names no mockup (then it is a CSS selector).
pub(crate) fn frame_for(first: &str) -> Option<Option<web_sys::HtmlIFrameElement>> {
    MockRef::parse(first).map(|m| find_device(&m))
}

/// A link back: the device shown, centred in the app.
pub(crate) fn scroll_to_device(f: &web_sys::Element) -> bool {
    let Some(device) = f.closest(".device").ok().flatten() else { return false };
    let opts = web_sys::ScrollIntoViewOptions::new();
    opts.set_block(web_sys::ScrollLogicalPosition::Center);
    device.scroll_into_view_with_scroll_into_view_options(&opts);
    true
}

/// The comment box's line: where, in words.
pub(crate) fn where_words(path: &str) -> Option<String> {
    of_path(path).map(|m| m.words())
}

// -- the keyboard path ------------------------------------------------------------

thread_local! {
    /// The device the keys walk in: the last one walked or hovered.
    static WALKING: RefCell<Option<web_sys::HtmlIFrameElement>> = const { RefCell::new(None) };
}

/// The keyboard path's bindings, and what each asks of the frame (`None`:
/// the next or previous device).
const WALK: [(&str, Option<&str>, i32); 7] = [
    ("annotate.walk-next", Some("next"), 0),
    ("annotate.walk-prev", Some("prev"), 0),
    ("annotate.walk-in", Some("in"), 0),
    ("annotate.walk-out", Some("out"), 0),
    ("annotate.walk-pick", Some("pick"), 0),
    ("annotate.device-next", None, 1),
    ("annotate.device-prev", None, -1),
];

/// Every mockup device frame shown on the page, in document order.
fn devices() -> Vec<web_sys::HtmlIFrameElement> {
    doc_all(".framed[data-view=\"mockup\"] .device:not(.hidden)").iter().filter_map(device_frame).collect()
}

/// A device frame was hovered: the keys walk there next.
pub(crate) fn hovered(f: &web_sys::HtmlIFrameElement) {
    if is_device(f) {
        WALKING.with(|w| *w.borrow_mut() = Some(f.clone()));
    }
}

fn act(from: Option<&web_sys::HtmlIFrameElement>, step: Option<&str>, by: i32) -> bool {
    let all = devices();
    if all.is_empty() {
        return false;
    }
    let held = from.cloned().or_else(|| WALKING.with(|w| w.borrow().clone()));
    let at = held.as_ref().and_then(|h| all.iter().position(|f| f.is_same_node(Some(h))));
    let (target, step) = match (step, at) {
        (Some(step), Some(i)) => (all[i].clone(), step),
        // Not walking yet: the first device, from its start.
        (Some(_), None) => (all[0].clone(), "start"),
        (None, at) => {
            let n = all.len() as i32;
            let i = at.map_or(if by > 0 { 0 } else { n - 1 }, |i| (i as i32 + by).rem_euclid(n));
            let f = all[i as usize].clone();
            scroll_to_device(&f);
            (f, "start")
        }
    };
    WALKING.with(|w| *w.borrow_mut() = Some(target.clone()));
    frame::post_walk(&target, step);
    true
}

/// A key the page shim posted from inside a device (`menger:key`): one of
/// the walk's, walked there.
pub(crate) fn frame_key(f: &web_sys::HtmlIFrameElement, key: &str) -> bool {
    if !annotate::is_on() || !is_device(f) {
        return false;
    }
    let Some(id) = bindings::key_hit(key, Context::Annotate) else { return false };
    WALK.iter().find(|(w, _, _)| *w == id).is_some_and(|(_, step, by)| act(Some(f), *step, *by))
}

/// The app's keys in annotation mode — before the page's own (a modal
/// context), and only while a mockup is shown and nothing has focus that
/// owns its keys (the comment box, a field, a menu). Called once by the App.
pub(crate) fn install() {
    let w = dom::window();
    let down = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        if ev.default_prevented() || !annotate::is_on() || !keys::no_overlay() {
            return;
        }
        if ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).is_some_and(|el| keys::defer_to_focus(&el)) {
            return;
        }
        let Some((_, step, by)) = WALK.iter().find(|(id, _, _)| bindings::is(&ev, Context::Annotate, id)) else { return };
        if act(None, *step, *by) {
            ev.prevent_default();
            ev.stop_immediate_propagation();
        }
    });
    let _ = w.add_event_listener_with_callback_and_bool("keydown", down.as_ref().unchecked_ref(), true);
    down.forget();
}

/// The first half and the rest of a path, joined — for the frame a device.
pub(crate) fn path_in(m: &MockRef, inner: &str) -> String {
    format!("{}{FRAME_SEP}{inner}", m.text())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(ticket: &str, mock: Option<u32>, reference: Option<&str>, width: u32) -> MockRef {
        MockRef { ticket: ticket.into(), mock, reference: reference.map(str::to_string), width }
    }

    /// The anchor's first half, written and read back: a fence, a `ref`,
    /// and a body that is itself the mockup.
    #[test]
    fn a_mockup_anchor_names_the_ticket_the_mockup_and_the_width() {
        for (r, text) in [
            (m("APP-45", Some(2), None, 1440), "mockup:APP-45#2@1440"),
            (m("APP-45", Some(3), Some("DESIGN-49"), 390), "mockup:APP-45#3=DESIGN-49@390"),
            (m("DESIGN-49", None, None, 768), "mockup:DESIGN-49@768"),
        ] {
            assert_eq!(r.text(), text);
            assert_eq!(MockRef::parse(text), Some(r.clone()), "{text}");
            let path = path_in(&r, "body > main > h1:nth-of-type(1)");
            assert_eq!(of_path(&path), Some(r));
            assert_eq!(anchor::split(&path).1, "body > main > h1:nth-of-type(1)");
        }
        assert_eq!(m("A-1", Some(1), None, 390).words(), "mockup 1 · 390 px");
        assert_eq!(m("A-1", Some(3), Some("DESIGN-49"), 1440).words(), "mockup 3 (DESIGN-49) · 1440 px");
        assert_eq!(m("A-1", None, None, 768).words(), "mockup · 768 px");
    }

    /// Every anchor older than APP-45 reads as it did: its first half is a
    /// CSS selector, never a mockup; and a malformed one is none.
    #[test]
    fn older_anchors_and_malformed_ones_name_no_mockup() {
        for path in [
            "#t-status",
            "iframe[data-ticket=\"RETURNS-2\"] >> body > table",
            "conversation",
            "[data-tag=\"ticket DEMAND-1\"]",
        ] {
            assert_eq!(of_path(path), None, "{path}");
        }
        for bad in [
            "mockup:",
            "mockup:APP-45",
            "mockup:APP-45@",
            "mockup:APP-45@0",
            "mockup:APP-45@wide",
            "mockup:#1@390",
            "mockup:APP-45#x@390",
            "mockup:APP-45#1=@390",
            "mockup:APP 45#1@390",
            "mockup:APP-45#1=D\"]@390",
        ] {
            assert_eq!(MockRef::parse(bad), None, "{bad}");
        }
        // A selector that happens to hold the word is still a selector.
        assert_eq!(MockRef::parse("[data-x=\"mockup:A-1@3\"]"), None);
    }

    /// A comment is drawn on the device at its width, else the nearest
    /// shown — the narrower on a tie; with none shown, nowhere.
    #[test]
    fn a_comment_is_drawn_at_its_width_else_the_nearest() {
        let shown = [(0, 390), (1, 768), (2, 1440)];
        assert_eq!(nearest(&shown, 1440), Some(2));
        assert_eq!(nearest(&shown, 390), Some(0));
        assert_eq!(nearest(&shown, 1280), Some(2));
        assert_eq!(nearest(&shown, 500), Some(0));
        assert_eq!(nearest(&shown, 579), Some(0), "a tie goes to the narrower");
        assert_eq!(nearest(&[(4, 1440)], 390), Some(4));
        assert_eq!(nearest(&[], 390), None);
    }
}
