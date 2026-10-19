//! APP-63: after a reconnect the app reloads, and comes back where it was.
//!
//! Owner, 2026-10-18 (chat 6): *"when reconnecting we need to refetch what
//! we may have missed. likely that means just reloading the page,
//! unfortunately"*.
//!
//! **Why a reload, not a re-ask.** The session already re-asks a great deal
//! on a new connection (session.rs: `watch`, the groups, the extents, the
//! board's window, the open page; the queue, inputs, comments, chats,
//! deployments, themes and orgs each `refresh`). But what a view fetched
//! for itself it fetches again only if it was written to — and not every
//! one was: an open chat asks for its messages once per mount
//! (`chat_pane`'s `started`), so a message posted while the socket was
//! down never arrived until the page was reloaded. Every view that holds
//! what it asked for is another place for the same gap, and every view
//! written later another. A reload is the one path that asks everything
//! again by construction: the same path as the first load. It also brings
//! a page that outlived a deploy onto the release the server now serves.
//!
//! **What it keeps.** The route (the hash survives a reload), the drafts
//! (the chat composer's, `menger.chat.draft.<id>`, and the comment boxes',
//! COMMENT-9, are in localStorage already) and, through a snapshot in
//! sessionStorage taken the moment before ([`Snapshot`]), the scroll — the
//! window's and every scrolled element's but a chat's messages, which land
//! on what is new by design — and every typed text field's value, focus
//! and caret, so a box no store keeps (a note, a filter, a request for
//! changes) is filled again. A field is filled only when it comes back
//! empty and of the same kind ([`fill`]): a box the app filled itself is
//! never overwritten.
//!
//! **When.** Only on a *re*connect — a page whose session has connected
//! before ([`connected`]) — and only once the server answers: `signin::gate`
//! has asked `/login/me` (an expired sign-in goes to the sign-in, as it did)
//! and a new socket has opened. While the server is down nothing reloads;
//! the session retries with a backoff ([`backoff_ms`]). A reloaded page's
//! first connection is a first one, so a reload needs a connection that
//! came up, went down and came up again. And at most [`MAX_RELOADS`] in
//! [`WINDOW_SECS`] ([`may_reload`]): past it the page stays and the
//! session's own re-ask is what it has.
//!
//! What decides — the backoff, the budget, which fields and scrollers are
//! kept, whether a field is filled — is pure and pinned by native tests;
//! the rest is the browser.

use std::cell::Cell;

use serde::{Deserialize, Serialize};

/// Where the snapshot waits for the reloaded page (sessionStorage: this
/// tab only).
pub(crate) const SNAPSHOT: &str = "menger.resume";
/// When this tab reloaded for a reconnect: a JSON list of unix seconds.
pub(crate) const TIMES: &str = "menger.resume.times";
/// The most reloads in [`WINDOW_SECS`].
pub(crate) const MAX_RELOADS: usize = 3;
pub(crate) const WINDOW_SECS: f64 = 120.0;
/// A snapshot older than this is not restored (ms).
pub(crate) const SNAPSHOT_TTL_MS: f64 = 60_000.0;
/// How long the reloaded page keeps trying to put things back (ms): the
/// board's window and a ticket's body arrive after the first paint.
const RESTORE_MS: f64 = 10_000.0;
const RESTORE_TICK_MS: i32 = 120;
/// Scrolled elements that land themselves: a chat's messages open on what
/// is new (CHAT-8), which is the point of coming back.
const OWN_LANDING: &[&str] = &["pmsgs"];

/// The retry gap after the `failures`-th session in a row that never
/// connected: 1 s, 2 s, 4 s, 8 s, then 15 s. A session that connected
/// resets it, so a restart still costs about a second.
pub(crate) fn backoff_ms(failures: u32) -> i32 {
    match failures {
        0 => 1_000,
        1 => 2_000,
        2 => 4_000,
        3 => 8_000,
        _ => 15_000,
    }
}

/// The reloads still inside the window at `now`.
pub(crate) fn recent(times: &[f64], now: f64) -> Vec<f64> {
    times.iter().copied().filter(|t| now - t < WINDOW_SECS && *t <= now + 1.0).collect()
}

/// Whether one more reload fits the budget.
pub(crate) fn may_reload(times: &[f64], now: f64) -> bool {
    recent(times, now).len() < MAX_RELOADS
}

/// A text field's value, as the page held it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Field {
    pub(crate) path: String,
    /// `textarea`, or `input`.
    pub(crate) tag: String,
    pub(crate) placeholder: String,
    pub(crate) value: String,
    pub(crate) focused: bool,
    pub(crate) caret: Option<(u32, u32)>,
}

/// An element's scroll.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Scroll {
    pub(crate) path: String,
    pub(crate) top: f64,
    pub(crate) left: f64,
}

/// What the page was, the moment before it reloaded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    /// `Date.now()`, ms.
    pub(crate) at: f64,
    /// The route (`location.hash`).
    pub(crate) hash: String,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) scrolls: Vec<Scroll>,
    pub(crate) fields: Vec<Field>,
}

impl Snapshot {
    /// Whether the page now loading is the one it was taken of.
    pub(crate) fn usable(&self, now: f64, hash: &str) -> bool {
        now - self.at < SNAPSHOT_TTL_MS && now >= self.at - 1_000.0 && self.hash == hash
    }
}

/// Whether a field of `tag` / `kind` (an input's `type`) holds typing worth
/// keeping: a textarea, or a text-like input. Never a password.
pub(crate) fn keeps(tag: &str, kind: &str) -> bool {
    match tag {
        "textarea" => true,
        "input" => matches!(kind, "" | "text" | "search" | "url" | "email"),
        _ => false,
    }
}

/// Whether a scrolled element with these classes is put back.
pub(crate) fn keeps_scroll(classes: &str) -> bool {
    !classes.split_whitespace().any(|c| OWN_LANDING.contains(&c))
}

/// What to do with a saved field and the one found at its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fill {
    /// It came back empty: put the value in.
    Put,
    /// It holds the value already (the app's own draft store).
    Done,
    /// It is another field, or holds something else: leave it.
    Skip,
}

/// [`Fill`] for `saved`, against a field of `tag` with `placeholder` that
/// holds `value`.
pub(crate) fn fill(saved: &Field, tag: &str, placeholder: &str, value: &str) -> Fill {
    if saved.tag != tag || saved.placeholder != placeholder {
        Fill::Skip
    } else if value == saved.value {
        Fill::Done
    } else if value.is_empty() {
        Fill::Put
    } else {
        Fill::Skip
    }
}

thread_local! {
    /// How many sessions on this page have connected.
    static CONNECTS: Cell<u32> = const { Cell::new(0) };
}

/// A session's socket is up.
pub(crate) fn connected() {
    CONNECTS.with(|c| c.set(c.get() + 1));
}

/// How many sessions on this page have connected — the session loop's
/// backoff resets when it moves.
pub(crate) fn connects() -> u32 {
    CONNECTS.with(Cell::get)
}

pub(crate) use web::{install, reconnected};

/// The browser half.
mod web {
    use super::*;
    use crate::dom;
    use std::rc::Rc;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use web_sys::{Element, HtmlInputElement, HtmlTextAreaElement};

    fn storage() -> Option<web_sys::Storage> {
        dom::window().session_storage().ok().flatten()
    }

    fn now_ms() -> f64 {
        js_sys::Date::now()
    }

    /// A new socket is up. On a reconnect, within the budget: the snapshot
    /// is kept and the page reloads — `true`, and the caller asks nothing
    /// more of this page. Otherwise `false`, and the session goes on.
    pub(crate) fn reconnected() -> bool {
        if connects() == 0 {
            return false; // the page's first connection: nothing was missed
        }
        let now = now_ms() / 1000.0;
        let st = storage();
        let times: Vec<f64> = st
            .as_ref()
            .and_then(|s| s.get_item(TIMES).ok().flatten())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let mut times = recent(&times, now);
        if !may_reload(&times, now) {
            leptos::logging::log!("reconnected: {} reloads in {WINDOW_SECS} s — staying, the session asks again", times.len());
            return false;
        }
        times.push(now);
        if let Some(s) = st.as_ref() {
            let _ = s.set_item(TIMES, &serde_json::to_string(&times).unwrap_or_default());
            if let Ok(json) = serde_json::to_string(&take()) {
                let _ = s.set_item(SNAPSHOT, &json);
            }
        }
        leptos::logging::log!("reconnected: reloading to catch up (APP-63)");
        dom::window().location().reload().is_ok()
    }

    /// The page as it is: the route, its scroll, its typed fields.
    fn take() -> Snapshot {
        let w = dom::window();
        let doc = w.document().expect("document");
        let active = doc.active_element();
        let mut fields = Vec::new();
        let mut scrolls = Vec::new();
        if let Ok(all) = doc.query_selector_all("textarea, input") {
            for i in 0..all.length() {
                let Some(el) = all.item(i).and_then(|n| n.dyn_into::<Element>().ok()) else { continue };
                let focused = active.as_ref() == Some(&el);
                if let Some(f) = field_of(&el, focused) {
                    fields.push(f);
                }
            }
        }
        if let Ok(all) = doc.query_selector_all("body *") {
            for i in 0..all.length() {
                let Some(el) = all.item(i).and_then(|n| n.dyn_into::<Element>().ok()) else { continue };
                let (top, left) = (el.scroll_top() as f64, el.scroll_left() as f64);
                if (top > 0.0 || left > 0.0) && keeps_scroll(&el.class_name()) {
                    scrolls.push(Scroll { path: path_of(&el), top, left });
                }
            }
        }
        Snapshot {
            at: now_ms(),
            hash: w.location().hash().unwrap_or_default(),
            x: w.scroll_x().unwrap_or(0.0),
            y: w.scroll_y().unwrap_or(0.0),
            scrolls,
            fields,
        }
    }

    fn field_of(el: &Element, focused: bool) -> Option<Field> {
        let tag = el.tag_name().to_ascii_lowercase();
        let (value, placeholder, caret, kind, off) = if let Some(t) = el.dyn_ref::<HtmlTextAreaElement>() {
            let caret = t.selection_start().ok().flatten().zip(t.selection_end().ok().flatten());
            (t.value(), t.placeholder(), caret, String::new(), t.disabled() || t.read_only())
        } else if let Some(t) = el.dyn_ref::<HtmlInputElement>() {
            let caret = t.selection_start().ok().flatten().zip(t.selection_end().ok().flatten());
            (t.value(), t.placeholder(), caret, t.type_().to_ascii_lowercase(), t.disabled() || t.read_only())
        } else {
            return None;
        };
        if off || value.is_empty() || !keeps(&tag, &kind) {
            return None;
        }
        Some(Field { path: path_of(el), tag, placeholder, value, focused, caret })
    }

    /// A selector for `el`: from its nearest ancestor with an id (or the
    /// body), each step its tag and place among its kind.
    fn path_of(el: &Element) -> String {
        let mut steps = Vec::new();
        let mut cur = Some(el.clone());
        while let Some(e) = cur {
            let tag = e.tag_name().to_ascii_lowercase();
            if tag == "body" || tag == "html" {
                steps.push("body".to_string());
                break;
            }
            let id = e.id();
            if !id.is_empty() && !id.contains('"') && !id.contains('\\') {
                steps.push(format!("[id=\"{id}\"]"));
                break;
            }
            let mut n = 1;
            let mut sib = e.previous_element_sibling();
            while let Some(s) = sib {
                if s.tag_name() == e.tag_name() {
                    n += 1;
                }
                sib = s.previous_element_sibling();
            }
            steps.push(format!("{tag}:nth-of-type({n})"));
            cur = e.parent_element();
        }
        steps.reverse();
        steps.join(" > ")
    }

    fn find(path: &str) -> Option<Element> {
        dom::window().document()?.query_selector(path).ok().flatten()
    }

    /// Put `saved` back into the field at its path; whether it is settled.
    fn refill(saved: &Field) -> bool {
        let Some(el) = find(&saved.path) else { return false };
        let tag = el.tag_name().to_ascii_lowercase();
        let (value, placeholder) = if let Some(t) = el.dyn_ref::<HtmlTextAreaElement>() {
            (t.value(), t.placeholder())
        } else if let Some(t) = el.dyn_ref::<HtmlInputElement>() {
            (t.value(), t.placeholder())
        } else {
            return true;
        };
        match fill(saved, &tag, &placeholder, &value) {
            Fill::Skip => return true,
            Fill::Done => {}
            Fill::Put => {
                if let Some(t) = el.dyn_ref::<HtmlTextAreaElement>() {
                    t.set_value(&saved.value);
                } else if let Some(t) = el.dyn_ref::<HtmlInputElement>() {
                    t.set_value(&saved.value);
                }
                // Bubbling, so the app's own (delegated) `input` handler
                // takes it — its signal, its draft store.
                let init = web_sys::EventInit::new();
                init.set_bubbles(true);
                if let Ok(ev) = web_sys::Event::new_with_event_init_dict("input", &init) {
                    let _ = el.dispatch_event(&ev);
                }
            }
        }
        if saved.focused {
            if let Some(h) = el.dyn_ref::<web_sys::HtmlElement>() {
                let opts = web_sys::FocusOptions::new();
                opts.set_prevent_scroll(true);
                let _ = h.focus_with_options(&opts);
            }
            if let Some((a, b)) = saved.caret {
                if let Some(t) = el.dyn_ref::<HtmlTextAreaElement>() {
                    let _ = t.set_selection_range(a, b);
                } else if let Some(t) = el.dyn_ref::<HtmlInputElement>() {
                    let _ = t.set_selection_range(a, b);
                }
            }
        }
        true
    }

    /// Scroll `s`'s element back; whether it holds there.
    fn rescroll(s: &Scroll) -> bool {
        let Some(el) = find(&s.path) else { return false };
        if (el.scroll_top() as f64 - s.top).abs() > 2.0 || (el.scroll_left() as f64 - s.left).abs() > 2.0 {
            el.set_scroll_top(s.top as i32);
            el.set_scroll_left(s.left as i32);
        }
        (el.scroll_top() as f64 - s.top).abs() <= 2.0 && (el.scroll_left() as f64 - s.left).abs() <= 2.0
    }

    fn rescroll_window(x: f64, y: f64) -> bool {
        let w = dom::window();
        let at = |w: &web_sys::Window| (w.scroll_x().unwrap_or(0.0), w.scroll_y().unwrap_or(0.0));
        let (cx, cy) = at(&w);
        if (cx - x).abs() > 2.0 || (cy - y).abs() > 2.0 {
            w.scroll_to_with_x_and_y(x, y);
        }
        let (cx, cy) = at(&w);
        (cx - x).abs() <= 2.0 && (cy - y).abs() <= 2.0
    }

    /// At startup: a snapshot left by [`reconnected`] for this page is put
    /// back — the fields as they come, the scroll until it holds or the
    /// reader moves it. Whether this page is such a reload (the boot's
    /// sheet stays down).
    pub(crate) fn install() -> bool {
        let Some(st) = storage() else { return false };
        let Some(json) = st.get_item(SNAPSHOT).ok().flatten() else { return false };
        let _ = st.remove_item(SNAPSHOT);
        let Ok(snap) = serde_json::from_str::<Snapshot>(&json) else { return false };
        let hash = dom::window().location().hash().unwrap_or_default();
        if !snap.usable(now_ms(), &hash) {
            return false;
        }
        // The reader's own move ends the scroll's restoring.
        let moved = Rc::new(Cell::new(false));
        let doc = dom::window().document().expect("document");
        let stop = {
            let moved = moved.clone();
            Closure::<dyn Fn()>::new(move || moved.set(true))
        };
        for ev in ["wheel", "touchstart", "keydown", "pointerdown"] {
            let _ = doc.add_event_listener_with_callback(ev, stop.as_ref().unchecked_ref());
        }
        stop.forget();
        wasm_bindgen_futures::spawn_local(async move {
            let start = now_ms();
            let mut fields: Vec<Field> = snap.fields.clone();
            let mut scrolls: Vec<(Scroll, u8)> = snap.scrolls.iter().cloned().map(|s| (s, 0)).collect();
            let mut window_held = 0u8;
            while now_ms() - start < RESTORE_MS {
                fields.retain(|f| !refill(f));
                if !moved.get() {
                    // Held for three ticks: the content under it has
                    // stopped growing.
                    window_held = if rescroll_window(snap.x, snap.y) { window_held + 1 } else { 0 };
                    for (s, held) in scrolls.iter_mut() {
                        *held = if rescroll(s) { *held + 1 } else { 0 };
                    }
                    scrolls.retain(|(_, h)| *h < 3);
                }
                let scroll_done = moved.get() || (window_held >= 3 && scrolls.is_empty());
                if fields.is_empty() && scroll_done {
                    break;
                }
                dom::sleep_ms(RESTORE_TICK_MS).await;
            }
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_holds() {
        let gaps: Vec<i32> = (0..7).map(backoff_ms).collect();
        assert_eq!(gaps, vec![1_000, 2_000, 4_000, 8_000, 15_000, 15_000, 15_000]);
    }

    #[test]
    fn at_most_three_reloads_in_two_minutes() {
        let now = 10_000.0;
        assert!(may_reload(&[], now));
        assert!(may_reload(&[now - 10.0, now - 5.0], now));
        assert!(!may_reload(&[now - 10.0, now - 5.0, now - 1.0], now), "a fourth inside the window: stay");
        assert!(may_reload(&[now - 300.0, now - 200.0, now - 121.0], now), "old ones are forgotten");
        assert_eq!(recent(&[now - 500.0, now - 3.0, now + 900.0], now), vec![now - 3.0], "a clock that moved back forgets the future");
    }

    #[test]
    fn a_snapshot_is_for_its_own_page_and_fresh() {
        let s = Snapshot { at: 1_000_000.0, hash: "#/ticket/A-1".into(), x: 0.0, y: 40.0, scrolls: vec![], fields: vec![] };
        assert!(s.usable(1_002_000.0, "#/ticket/A-1"));
        assert!(!s.usable(1_002_000.0, "#/board"), "another route");
        assert!(!s.usable(1_000_000.0 + SNAPSHOT_TTL_MS + 1.0, "#/ticket/A-1"), "stale");
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Snapshot>(&json).unwrap(), s);
    }

    #[test]
    fn which_fields_and_scrollers_are_kept() {
        assert!(keeps("textarea", ""));
        for t in ["", "text", "search", "url", "email"] {
            assert!(keeps("input", t), "{t}");
        }
        for t in ["password", "checkbox", "radio", "file", "hidden", "range", "color"] {
            assert!(!keeps("input", t), "{t}");
        }
        assert!(!keeps("select", ""));
        assert!(keeps_scroll("board rows"));
        assert!(!keeps_scroll("pmsgs turns"), "a chat lands on what is new");
    }

    #[test]
    fn a_field_is_filled_only_when_it_came_back_empty_and_the_same() {
        let f = Field { path: "body > textarea:nth-of-type(1)".into(), tag: "textarea".into(), placeholder: "Write a note".into(), value: "half a thought".into(), focused: true, caret: Some((4, 4)) };
        assert_eq!(fill(&f, "textarea", "Write a note", ""), Fill::Put);
        assert_eq!(fill(&f, "textarea", "Write a note", "half a thought"), Fill::Done, "its own store put it back");
        assert_eq!(fill(&f, "textarea", "Write a note", "something else"), Fill::Skip, "never overwritten");
        assert_eq!(fill(&f, "textarea", "Reply", ""), Fill::Skip, "another box at that path");
        assert_eq!(fill(&f, "input", "Write a note", ""), Fill::Skip);
    }
}
