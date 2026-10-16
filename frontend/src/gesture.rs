//! IDENTITY-36 (INPUT-4, INPUT-9): **a gesture on THIS frame** — what a
//! frame's write (`menger.input.submit`, `menger.input.clear`) waits for.
//!
//! The app window's own transient activation (`navigator.userActivation`)
//! is on for ~5 s after ANY click or key anywhere in the app — so a body
//! that polled `menger:submit` used to record an answer, source `app`, on
//! the viewer's next unrelated click. The host now asks for more, all of it
//! read from the host's own side, where nothing in a frame can reach:
//!
//! - **activation** — the app's transient activation is on; a browser
//!   without the API cannot say, and is refused (fail closed);
//! - **focus** — the app's document has focus and its focused element is
//!   this very `<iframe>`: a click or key in a frame moves focus into it, and
//!   any click in the app moves it back out;
//! - **engagement** — since the app's last own gesture (a `pointerdown` or
//!   `keydown` the host heard: one in the app, never in a frame), the
//!   pointer came onto this frame (`pointerover` with the frame as target),
//!   or a Tab in the app carried focus into it (keyboard users);
//! - **whose activation** — the live activation can be the app's own only
//!   while its last own gesture is younger than [`ACTIVATION_MS`]; then
//!   focus must have entered this frame after that gesture (the app
//!   window's `blur`, the frame focused) — a click in the frame does that.
//!
//! APP-35: **the app's own activation, waited out.** A press in the app that
//! leaves focus in the frame — its scrollbar, dragged to scroll the body
//! down to an input; a control that keeps focus where it is — makes the
//! app's activation live for [`ACTIVATION_MS`] while no blur says focus came
//! back to the frame, so a real click on Submit right after it was refused,
//! and the next one, seconds later, recorded. Such a write is now neither
//! refused nor taken at once: the host reads it again when the app's own
//! activation has run out ([`Verdict::After`]); an activation still live
//! then is a gesture in a frame since, and the rest of the reading is
//! asked again as at the arrival. A body that polls submit gets nothing it
//! could not have by polling at that moment itself.
//!
//! What remains (said, not hidden): a frame that moves focus into itself
//! by script while the pointer rests over it, within 5 s of a click
//! elsewhere in the app, passes — the browser does not tell the host
//! whether a frame's focus came from a click or from its own `focus()`.
//!
//! The decision ([`on_this_frame`]) is pure and pinned by native tests;
//! the listeners that feed it ([`install`]) and the reading at a write
//! ([`on_frame`]) are DOM.

use std::cell::RefCell;

use js_sys::Reflect;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::dom;

/// HTML's transient activation duration as the browsers ship it (Chromium
/// and Firefox: 5 s) — how long the app's own gesture keeps its window
/// activated.
pub(crate) const ACTIVATION_MS: f64 = 5_000.0;

/// APP-35: how long past the app's own activation the host waits before it
/// reads a write again — the browsers' timers and this one are not one clock.
pub(crate) const SETTLE_MS: f64 = 150.0;

/// A Tab in the app counts as carrying focus into a frame when focus lands
/// there within this long.
pub(crate) const TAB_MS: f64 = 1_000.0;

/// What the host has seen, for one frame (times in ms, one clock).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Seen {
    /// The app's last own gesture — a `pointerdown` or `keydown` it heard.
    pub(crate) host: Option<f64>,
    /// Since when the pointer has been over this frame; `None`: it is not.
    pub(crate) hovered: Option<f64>,
    /// When focus last went into this frame, and whether a Tab in the app
    /// carried it there.
    pub(crate) entered: Option<(f64, bool)>,
}

/// The moment of a write.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Now {
    pub(crate) at: f64,
    /// The app's transient activation; `None`: the browser cannot say.
    pub(crate) active: Option<bool>,
    /// The app has focus, and its focused element is this frame.
    pub(crate) focused: bool,
}

/// What the host makes of a write, read the moment it arrives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Verdict {
    /// A viewer's gesture on this frame is under way.
    Yes,
    /// It is not.
    No,
    /// APP-35: all of it holds, but the live activation may still be the
    /// app's own (its last gesture younger than [`ACTIVATION_MS`], focus not
    /// seen entering this frame since): read again at this time, when the
    /// app's own has run out.
    After(f64),
}

/// Whether a viewer's gesture on this frame is under way (module doc).
pub(crate) fn verdict(seen: &Seen, now: &Now) -> Verdict {
    if now.active != Some(true) || !now.focused {
        return Verdict::No;
    }
    let host = seen.host.unwrap_or(f64::NEG_INFINITY);
    let by_pointer = seen.hovered.is_some_and(|since| since > host);
    let by_tab = seen.entered.is_some_and(|(at, tab)| tab && at >= host);
    if !(by_pointer || by_tab) {
        return Verdict::No;
    }
    let host_still_active = now.at - host < ACTIVATION_MS;
    if !host_still_active || seen.entered.is_some_and(|(at, _)| at >= host) {
        Verdict::Yes
    } else {
        Verdict::After(host + ACTIVATION_MS + SETTLE_MS)
    }
}

/// [`verdict`], answered now: [`Verdict::After`] is not yet a gesture.
pub(crate) fn on_this_frame(seen: &Seen, now: &Now) -> bool {
    verdict(seen, now) == Verdict::Yes
}

/// APP-35: the reading of a write that [`verdict`] put off, taken again at
/// its time — a gesture only if it is one then, with nothing to wait for (a
/// press in the app since puts it off again: that is no gesture).
pub(crate) fn settled(first: Verdict, again: impl FnOnce() -> Verdict) -> bool {
    match first {
        Verdict::Yes => true,
        Verdict::No => false,
        Verdict::After(_) => again() == Verdict::Yes,
    }
}

#[derive(Default)]
struct Track {
    host: Option<f64>,
    last_tab: Option<f64>,
    /// `(channel, since)`.
    hovered: Option<(String, f64)>,
    /// `(channel, at, by_tab)`.
    entered: Option<(String, f64, bool)>,
}

thread_local! {
    static TRACK: RefCell<Track> = RefCell::new(Track::default());
}

fn now_ms() -> f64 {
    js_sys::Date::now()
}

/// The frame (`iframe.<class>`) an event's target is, by its channel.
fn frame_channel(target: Option<web_sys::EventTarget>, class: &str) -> Option<String> {
    let el = target?.dyn_into::<web_sys::Element>().ok()?;
    (el.tag_name().eq_ignore_ascii_case("iframe") && el.class_list().contains(class)).then(|| el.get_attribute("data-channel"))?
}

/// The listeners that feed [`on_frame`] — on the app's own window, for the
/// app's lifetime; `class` is the frames' class. Called once, by the
/// frames' listener.
pub(crate) fn install(class: &'static str) {
    let w = dom::window();
    let listen = |kind: &str, f: Box<dyn FnMut(web_sys::Event)>| {
        let cb = Closure::<dyn FnMut(web_sys::Event)>::wrap(f);
        let _ = w.add_event_listener_with_callback_and_bool(kind, cb.as_ref().unchecked_ref(), true);
        cb.forget();
    };
    // The app's own gestures: never one in a frame (those go to the frame).
    for kind in ["pointerdown", "keydown"] {
        listen(
            kind,
            Box::new(move |ev: web_sys::Event| {
                let t = now_ms();
                let tab = crate::bindings::moves_focus(&ev);
                TRACK.with(|tr| {
                    let mut tr = tr.borrow_mut();
                    tr.host = Some(t);
                    if tab {
                        tr.last_tab = Some(t);
                    }
                });
            }),
        );
    }
    // Where the pointer is: onto a frame (the frame is the target), onto
    // anything else in the app, or out of the window.
    listen(
        "pointerover",
        Box::new(move |ev: web_sys::Event| {
            let over = frame_channel(ev.target(), class);
            TRACK.with(|tr| {
                let mut tr = tr.borrow_mut();
                match over {
                    Some(ch) if tr.hovered.as_ref().is_some_and(|(c, _)| *c == ch) => {}
                    Some(ch) => tr.hovered = Some((ch, now_ms())),
                    None => tr.hovered = None,
                }
            });
        }),
    );
    listen(
        "pointerout",
        Box::new(move |ev: web_sys::Event| {
            let to = Reflect::get(&ev, &JsValue::from_str("relatedTarget")).ok().filter(|v| !v.is_null() && !v.is_undefined());
            if to.is_none() {
                TRACK.with(|tr| tr.borrow_mut().hovered = None);
            }
        }),
    );
    // Focus leaving the app's document: into a frame, when the focused
    // element is one once the blur has settled.
    let blur = Closure::<dyn FnMut()>::new(move || {
        let settle = Closure::once_into_js(move || {
            let doc = dom::window().document();
            let into = doc.and_then(|d| d.active_element()).and_then(|el| frame_channel(Some(el.into()), class));
            let t = now_ms();
            TRACK.with(|tr| {
                let mut tr = tr.borrow_mut();
                let tab = tr.last_tab.is_some_and(|k| t - k < TAB_MS) && tr.last_tab >= tr.host;
                tr.entered = into.map(|ch| (ch, t, tab));
            });
        });
        let _ = dom::window().set_timeout_with_callback_and_timeout_and_arguments_0(settle.unchecked_ref(), 0);
    });
    let _ = w.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref());
    blur.forget();
}

/// The app's transient activation; `None`: no `navigator.userActivation`.
/// Read through `Reflect`, as the clipboard is: web-sys gates the binding.
fn host_active() -> Option<bool> {
    let navigator = dom::window().navigator();
    let ua = Reflect::get(&navigator, &JsValue::from_str("userActivation")).ok().filter(JsValue::is_object)?;
    Reflect::get(&ua, &JsValue::from_str("isActive")).ok()?.as_bool()
}

/// Whether a viewer's gesture on `frame` is under way, now — read the
/// moment its write arrives ([`settle`] waits out a [`Verdict::After`]).
pub(crate) fn on_frame(frame: &web_sys::HtmlIFrameElement) -> Verdict {
    let Some(channel) = frame.get_attribute("data-channel") else { return Verdict::No };
    let doc = dom::window().document();
    let focused = doc.as_ref().is_some_and(|d| d.has_focus().unwrap_or(false))
        && doc.and_then(|d| d.active_element()).is_some_and(|el| el.is_same_node(Some(frame)));
    let now = Now { at: now_ms(), active: host_active(), focused };
    let seen = TRACK.with(|tr| {
        let tr = tr.borrow();
        Seen {
            host: tr.host,
            hovered: tr.hovered.as_ref().filter(|(c, _)| *c == channel).map(|(_, since)| *since),
            entered: tr.entered.as_ref().filter(|(c, _, _)| *c == channel).map(|(_, at, tab)| (*at, *tab)),
        }
    });
    verdict(&seen, &now)
}

/// APP-35: `first`, the reading at the write's arrival, made final — a
/// [`Verdict::After`] is slept out and `frame` read again then.
pub(crate) async fn settle(frame: &web_sys::HtmlIFrameElement, first: Verdict) -> bool {
    if let Verdict::After(at) = first {
        let wait = (at - now_ms()).clamp(0.0, ACTIVATION_MS + SETTLE_MS);
        dom::sleep_ms(wait.ceil() as i32).await;
    }
    settled(first, || on_frame(frame))
}

/// MENGER-34: the weaker reading, for what writes nothing (the ticket menu
/// a chip asks for): a gesture is live — the frame's own activation reaches
/// the app as it propagates to ancestors — and focus is in this very frame,
/// where a click or key in it puts it. A body's script, alone, has neither.
pub(crate) fn live_in(frame: &web_sys::HtmlIFrameElement) -> bool {
    let doc = dom::window().document();
    host_active() == Some(true)
        && doc.as_ref().is_some_and(|d| d.has_focus().unwrap_or(false))
        && doc.and_then(|d| d.active_element()).is_some_and(|el| el.is_same_node(Some(frame)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: f64 = 100_000.0;

    fn now(at: f64) -> Now {
        Now { at, active: Some(true), focused: true }
    }

    /// A click in the frame: the pointer came onto it, focus went in, the
    /// activation is on — whether the app's last own click was long ago or
    /// a moment before (its link opened the page).
    #[test]
    fn a_click_in_this_frame_is_a_gesture_on_it() {
        let seen = Seen { host: Some(T), hovered: Some(T + 800.0), entered: Some((T + 1_200.0, false)) };
        assert!(on_this_frame(&seen, &now(T + 1_300.0)), "a click right after one in the app");
        assert!(on_this_frame(&seen, &now(T + 60_000.0)), "a later click in the same frame, focus still there");
        let first = Seen { host: None, hovered: Some(T), entered: Some((T + 10.0, false)) };
        assert!(on_this_frame(&first, &now(T + 20.0)), "no gesture in the app yet");
        // A browser that fired no blur: once the app's own activation has
        // expired, the live one is the frame's.
        let no_blur = Seen { host: Some(T), hovered: Some(T + 100.0), entered: None };
        assert!(on_this_frame(&no_blur, &now(T + ACTIVATION_MS + 1.0)));
        assert!(!on_this_frame(&no_blur, &now(T + 3_000.0)), "inside the app's window, it cannot be told apart: refused");
    }

    /// The finding: a click anywhere else in the app is no gesture on this
    /// frame — focus is not in it, or the pointer never came onto it since,
    /// or it is the app's own activation still live.
    #[test]
    fn a_click_elsewhere_in_the_app_is_not() {
        // The click moved focus to the app: not focused.
        let seen = Seen { host: Some(T + 5_000.0), hovered: Some(T), entered: Some((T, false)) };
        assert!(!on_this_frame(&seen, &Now { at: T + 5_100.0, active: Some(true), focused: false }));
        // Focus kept in the frame (a mousedown the app prevented), the app
        // clicked since the pointer came onto it: no engagement since.
        assert!(!on_this_frame(&seen, &now(T + 5_100.0)));
        // A key in the app while the pointer rests over the frame, then the
        // frame takes focus by script: the key is newer than the hover.
        let steal = Seen { host: Some(T + 2_000.0), hovered: Some(T), entered: Some((T + 2_500.0, false)) };
        assert!(!on_this_frame(&steal, &now(T + 2_600.0)));
        // Focus in the frame from long ago, the pointer elsewhere: nothing.
        let away = Seen { host: Some(T), hovered: None, entered: Some((T + 100.0, false)) };
        assert!(!on_this_frame(&away, &now(T + 60_000.0)));
    }

    /// No activation, or a browser that cannot say: refused (fail closed).
    #[test]
    fn no_activation_or_no_api_is_no_gesture() {
        let seen = Seen { host: None, hovered: Some(T), entered: Some((T, false)) };
        for active in [Some(false), None] {
            assert!(!on_this_frame(&seen, &Now { at: T + 10.0, active, focused: true }), "{active:?}");
        }
        assert!(!on_this_frame(&Seen::default(), &now(T)), "nothing seen at all");
    }

    /// APP-35, the finding: focus in the frame from an earlier click in it
    /// (a decision answered above), a press in the app that leaves focus
    /// there (its scrollbar, dragged down to the input), the pointer back on
    /// the frame, a click on Submit a second later. It used to be refused —
    /// and the next click, past the app's activation, recorded. Now the
    /// write is read again once the app's own activation has run out: a
    /// gesture if one is still live then (the click on Submit's), with focus
    /// and the pointer still on this frame.
    #[test]
    fn a_click_right_after_a_press_in_the_app_that_kept_focus_waits_out_its_activation() {
        let seen = Seen { host: Some(T), hovered: Some(T + 300.0), entered: Some((T - 20_000.0, false)) };
        let first = verdict(&seen, &now(T + 1_200.0));
        assert_eq!(first, Verdict::After(T + ACTIVATION_MS + SETTLE_MS));
        assert!(!on_this_frame(&seen, &now(T + 1_200.0)), "not yet a gesture at the arrival");
        // Read again then: the activation is the click's in the frame.
        let then = now(T + ACTIVATION_MS + SETTLE_MS);
        assert!(settled(first, || verdict(&seen, &then)));
        // No blur ever seen for this frame: the same.
        let never = Seen { entered: None, ..seen };
        assert!(settled(verdict(&never, &now(T + 1_200.0)), || verdict(&never, &then)));
    }

    /// IDENTITY-36 kept: the press in the app was all there was — a frame
    /// that polled submit while the pointer crossed it is refused when read
    /// again, for the app's activation has run out with nothing after it;
    /// so is one whose focus or pointer has gone, or after another press in
    /// the app meanwhile (put off again: no gesture).
    #[test]
    fn a_put_off_write_with_no_gesture_in_the_frame_since_is_refused() {
        let seen = Seen { host: Some(T), hovered: Some(T + 300.0), entered: Some((T - 20_000.0, false)) };
        let first = verdict(&seen, &now(T + 400.0));
        assert!(matches!(first, Verdict::After(_)));
        let at = T + ACTIVATION_MS + SETTLE_MS;
        assert!(!settled(first, || verdict(&seen, &Now { at, active: Some(false), focused: true })), "activation expired: the app's own");
        assert!(!settled(first, || verdict(&seen, &Now { at, active: Some(true), focused: false })), "focus left the frame");
        let left = Seen { hovered: None, ..seen };
        assert!(!settled(first, || verdict(&left, &now(at))), "the pointer left it");
        let again = Seen { host: Some(T + 4_000.0), ..seen };
        assert!(!settled(first, || verdict(&again, &now(at))), "a press in the app meanwhile");
        // Refused outright stays refused, without a second reading.
        assert!(!settled(Verdict::No, || Verdict::Yes));
    }

    /// Keyboard: a Tab in the app carried focus in; then a key in the frame.
    #[test]
    fn a_tab_into_this_frame_engages_it() {
        let seen = Seen { host: Some(T), hovered: None, entered: Some((T + 30.0, true)) };
        assert!(on_this_frame(&seen, &now(T + 40.0)));
        assert!(on_this_frame(&seen, &now(T + 30_000.0)));
        // Focus that arrived without a Tab, the pointer elsewhere: no.
        let seen = Seen { host: Some(T), hovered: None, entered: Some((T + 30.0, false)) };
        assert!(!on_this_frame(&seen, &now(T + 40.0)));
        // A key in the app after the Tab: the frame no longer holds it.
        let seen = Seen { host: Some(T + 500.0), hovered: None, entered: Some((T + 30.0, true)) };
        assert!(!on_this_frame(&seen, &now(T + 600.0)));
    }
}
