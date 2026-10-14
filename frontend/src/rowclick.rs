//! AGS-29: **a double-click on a nav row opens or closes it, and nothing
//! else.** Owner, 2026-10-13: "double click shouldn't open. that means single
//! click needs to be debounced." AGS-25 had chosen the opposite (a double-
//! click's first click followed the row's link), so expanding a row also
//! opened its ticket.
//!
//! A nav row is a link (`<a href>`), so a plain click on a row **with
//! children** is held: the browser's default is prevented, and the ticket
//! opens only if no second click comes within
//! [`DOUBLE_CLICK_MS`](crate::bindings::DOUBLE_CLICK_MS) — the bindings
//! table's, beside the sequence timeout, and named in its `select.only` and
//! `nav.toggle` rows. A double-click (`nav.toggle`) opens or closes the row;
//! the route never changes. A **leaf** has nothing to toggle and opens at
//! once. Everything else is as it was and immediate: Shift/Ctrl/⌘+click
//! select (ACT-3), middle-click a new tab, Alt+click a chip's filter
//! (FILTER-25), right-click the menu, the `▸`, Enter, j/k. Any of them, the
//! pointer leaving the row, or a click on another row drops a held click —
//! the latest click wins.
//!
//! AGS-30: the toggle is **every even click of a quick run** (`detail` 2, 4,
//! 6 …), read off the `click` event — the browser fires `dblclick` only when
//! a run reaches 2, so a second double-click straight after the first was
//! lost. The `▸` toggles on every odd click, so each double-click there is
//! one toggle too.
//!
//! AGS-32: the run is **menger's own count**, not the event's `detail`:
//! Chromium on Linux counts no further than three (a 4th quick click is
//! `detail` 1 again), so the second double-click's second click looked like
//! a fresh click, was held, and opened the ticket. A click on the same row
//! (or the same `▸`) within [`RUN_GAP_MS`] of the run's last click is its
//! next; anything else starts a run ([`next_count`]). `detail` is read only
//! for 0 — a click no pointer made.
//!
//! The rule is [`step`], a pure function over what is held and what just
//! happened (the native tests below); [`feed`] keeps the one held click for
//! the page and says what to do.

use std::cell::RefCell;

/// A plain click on a row with children, waiting out the window.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Held {
    /// The ticket the row opens.
    pub(crate) row: String,
    /// The click's `timeStamp` — which click a lapsing window is for.
    pub(crate) at: f64,
}

/// What happened to a nav row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Ev<'a> {
    /// A plain click (`select.only`) on `row`: `count` its place in the run
    /// (AGS-32's own count: 2 for a double-click's second click; 0 for one no
    /// pointer made), `parent` whether the row has children to open or close.
    Click { row: &'a str, at: f64, count: i32, parent: bool },
    /// The `dblclick` event (`nav.toggle`) on `row` — nothing of its own
    /// since AGS-30: the run's even click toggles.
    DoubleClick { row: &'a str, parent: bool },
    /// The window of the click at `at` ran out.
    Lapse { at: f64 },
    /// The pointer left `row`.
    Leave { row: &'a str },
    /// Any other click on a row: a modifier, the `▸`.
    Other,
}

/// What the row does about it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    /// Let the browser follow the link (a leaf's click).
    Follow,
    /// Prevent the link, and wait out the window.
    Hold,
    /// Prevent the link, and do nothing (a double-click's second click).
    Swallow,
    /// Open this ticket now (the window ran out).
    Open(String),
    /// Open or close this row (an even click of a run; prevent its default).
    Toggle(String),
    /// Nothing.
    Nothing,
}

/// The held click after `ev`, and what to do (module doc).
pub(crate) fn step(held: Option<&Held>, ev: Ev<'_>) -> (Option<Held>, Act) {
    match ev {
        // A leaf opens at once; the latest click wins.
        Ev::Click { parent: false, .. } => (None, Act::Follow),
        // No pointer pressed it (a `click()` from a script, a screen
        // reader's activation): nothing can follow, so nothing waits.
        Ev::Click { count: 0, .. } => (None, Act::Follow),
        // A run of quick clicks never navigates. AGS-30: every even click of
        // the run toggles — the browser fires `dblclick` only when a run
        // reaches 2 (`click:1 click:2 dblclick click:3 click:4`), so a quick
        // second double-click is clicks 3 and 4 and no `dblclick` at all.
        Ev::Click { row, count, .. } if count > 1 => (None, if count % 2 == 0 { Act::Toggle(row.to_string()) } else { Act::Swallow }),
        Ev::Click { row, at, .. } => (Some(Held { row: row.to_string(), at }), Act::Hold),
        // The `dblclick` itself: its toggle was the even click's (AGS-30).
        Ev::DoubleClick { .. } => (None, Act::Nothing),
        Ev::Lapse { at } => match held {
            Some(h) if h.at == at => (None, Act::Open(h.row.clone())),
            // A dropped click's window, or an older one's: the held click stays.
            _ => (held.cloned(), Act::Nothing),
        },
        Ev::Leave { row } => match held {
            Some(h) if h.row == row => (None, Act::Nothing),
            _ => (held.cloned(), Act::Nothing),
        },
        Ev::Other => (None, Act::Nothing),
    }
}

/// AGS-30: whether a click on the `▸` with this `detail` toggles — every odd
/// click of a run (and one no pointer made), so each double-click is one.
pub(crate) fn glyph_toggles(detail: i32) -> bool {
    detail <= 0 || detail % 2 == 1
}

/// AGS-32: how close a click must follow the run's last to be its next —
/// the usual system double-click time.
pub(crate) const RUN_GAP_MS: f64 = 500.0;

/// AGS-32: the last click of a run — on what (a row's id, or `▸` and its id),
/// when, and its place in the run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Run {
    pub(crate) on: String,
    pub(crate) at: f64,
    pub(crate) n: i32,
}

/// AGS-32: a click on `on` at `at`'s place in its run.
pub(crate) fn next_count(last: Option<&Run>, on: &str, at: f64) -> i32 {
    match last {
        Some(r) if r.on == on && at >= r.at && at - r.at <= RUN_GAP_MS => r.n + 1,
        _ => 1,
    }
}

thread_local! {
    static HELD: RefCell<Option<Held>> = const { RefCell::new(None) };
    static RUN: RefCell<Option<Run>> = const { RefCell::new(None) };
}

/// AGS-32: counts a click on `on` into the page's run and says its place —
/// 0 for a click no pointer made (`detail` 0), which starts no run.
fn counted(on: &str, at: f64, detail: i32) -> i32 {
    if detail <= 0 {
        RUN.with(|r| *r.borrow_mut() = None);
        return 0;
    }
    RUN.with(|r| {
        let mut r = r.borrow_mut();
        let n = next_count(r.as_ref(), on, at);
        *r = Some(Run { on: on.to_string(), at, n });
        n
    })
}

/// A plain click on a nav row: counted into its run (AGS-32), then [`step`].
pub(crate) fn click(row: &str, at: f64, detail: i32, parent: bool) -> Act {
    let count = counted(row, at, detail);
    feed(Ev::Click { row, at, count, parent })
}

/// AGS-32: whether a click on a row's `▸` toggles — every odd click of its
/// own run ([`glyph_toggles`]).
pub(crate) fn glyph(row: &str, at: f64, detail: i32) -> bool {
    glyph_toggles(counted(&["▸", row].concat(), at, detail))
}

/// [`step`] over the page's one held click.
pub(crate) fn feed(ev: Ev<'_>) -> Act {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let (next, act) = step(h.as_ref(), ev);
        *h = next;
        act
    })
}

#[cfg(test)]
mod tests {
    use super::{step, Act, Ev, Held};

    fn click(row: &str, at: f64, count: i32, parent: bool) -> Ev<'_> {
        Ev::Click { row, at, count, parent }
    }

    /// Runs `evs` from nothing held; the acts, in order.
    fn run(evs: &[Ev<'_>]) -> Vec<Act> {
        let mut held: Option<Held> = None;
        evs.iter()
            .map(|ev| {
                let (next, act) = step(held.as_ref(), *ev);
                held = next;
                act
            })
            .collect()
    }

    #[test]
    fn a_single_click_on_a_row_with_children_opens_after_the_window() {
        assert_eq!(run(&[click("A-1", 10.0, 1, true), Ev::Lapse { at: 10.0 }]), [Act::Hold, Act::Open("A-1".into())]);
    }

    #[test]
    fn a_double_click_toggles_and_opens_nothing() {
        let acts = run(&[
            click("A-1", 10.0, 1, true),
            click("A-1", 120.0, 2, true),
            Ev::DoubleClick { row: "A-1", parent: true },
            Ev::Lapse { at: 10.0 },
        ]);
        assert_eq!(acts, [Act::Hold, Act::Toggle("A-1".into()), Act::Nothing, Act::Nothing]);
        assert!(!acts.iter().any(|a| matches!(a, Act::Open(_) | Act::Follow)), "a double-click never navigates");
    }

    /// A double-click slower than the window but within the system's: the
    /// first click has opened the ticket; the second does not open it again,
    /// and the double-click still toggles.
    #[test]
    fn a_slow_double_click_still_toggles_once() {
        let acts = run(&[
            click("A-1", 10.0, 1, true),
            Ev::Lapse { at: 10.0 },
            click("A-1", 300.0, 2, true),
            Ev::DoubleClick { row: "A-1", parent: true },
        ]);
        assert_eq!(acts, [Act::Hold, Act::Open("A-1".into()), Act::Toggle("A-1".into()), Act::Nothing]);
    }

    /// AGS-30: a quick run of 2, 4, 6 clicks toggles 1, 2, 3 times and never
    /// navigates — no `dblclick` comes after the first.
    #[test]
    fn every_even_click_of_a_run_toggles() {
        for n in [2, 4, 6] {
            let evs: Vec<Ev<'_>> = (1..=n).map(|c| click("A-1", 10.0 * c as f64, c, true)).chain([Ev::Lapse { at: 10.0 }]).collect();
            let acts = run(&evs);
            assert_eq!(acts.iter().filter(|a| matches!(a, Act::Toggle(_))).count(), (n / 2) as usize, "{n} clicks");
            assert!(!acts.iter().any(|a| matches!(a, Act::Open(_) | Act::Follow)), "{n} clicks never navigate");
        }
    }

    /// AGS-32: menger counts the run — Chromium's wrapped details (1, 2, 3,
    /// 1) and Firefox's (1, 2, 3, 4) are the same run of four; a pause past
    /// the gap, or another row, starts a new one.
    #[test]
    fn the_run_is_counted_by_time_and_row_not_detail() {
        use super::{next_count, Run, RUN_GAP_MS};
        let mut last: Option<Run> = None;
        let mut ns = vec![];
        for at in [0.0, 120.0, 260.0, 380.0] {
            let n = next_count(last.as_ref(), "A-1", at);
            last = Some(Run { on: "A-1".into(), at, n });
            ns.push(n);
        }
        assert_eq!(ns, [1, 2, 3, 4]);
        // The four as a row's acts: two toggles, no navigation.
        let acts = run(&ns.iter().enumerate().map(|(i, n)| click("A-1", i as f64 * 120.0, *n, true)).collect::<Vec<_>>());
        assert_eq!(acts.iter().filter(|a| matches!(a, Act::Toggle(_))).count(), 2);
        assert!(!acts.iter().any(|a| matches!(a, Act::Open(_) | Act::Follow)));
        // A pause past the gap, another row: a new run.
        assert_eq!(next_count(last.as_ref(), "A-1", 380.0 + RUN_GAP_MS + 1.0), 1);
        assert_eq!(next_count(last.as_ref(), "B-1", 400.0), 1);
    }

    /// AGS-30: the `▸`'s rule — every odd click of a run toggles.
    #[test]
    fn the_toggle_glyph_turns_on_every_odd_click() {
        let turns = |n: i32| (1..=n).filter(|d| super::glyph_toggles(*d)).count();
        assert_eq!((turns(1), turns(2), turns(4), turns(6)), (1, 1, 2, 3));
        assert!(super::glyph_toggles(0), "a click no pointer made");
    }

    #[test]
    fn a_leaf_opens_at_once_and_ignores_a_double_click() {
        assert_eq!(run(&[click("A-2", 10.0, 1, false)]), [Act::Follow]);
        assert_eq!(
            run(&[click("A-2", 10.0, 1, false), click("A-2", 100.0, 2, false), Ev::DoubleClick { row: "A-2", parent: false }]),
            [Act::Follow, Act::Follow, Act::Nothing]
        );
    }

    #[test]
    fn a_click_no_pointer_made_opens_at_once() {
        assert_eq!(run(&[click("A-1", 10.0, 1, true), click("A-1", 20.0, 0, true), Ev::Lapse { at: 10.0 }]), [Act::Hold, Act::Follow, Act::Nothing]);
    }

    #[test]
    fn the_latest_click_wins() {
        // Another row with children: its click is held, the first one's dropped.
        let acts = run(&[click("A-1", 10.0, 1, true), click("B-1", 60.0, 1, true), Ev::Lapse { at: 10.0 }, Ev::Lapse { at: 60.0 }]);
        assert_eq!(acts, [Act::Hold, Act::Hold, Act::Nothing, Act::Open("B-1".into())]);
        // A leaf: opens, and the held click is dropped.
        let acts = run(&[click("A-1", 10.0, 1, true), click("A-3", 60.0, 1, false), Ev::Lapse { at: 10.0 }]);
        assert_eq!(acts, [Act::Hold, Act::Follow, Act::Nothing]);
        // A modifier click, the ▸: the held click is dropped.
        assert_eq!(run(&[click("A-1", 10.0, 1, true), Ev::Other, Ev::Lapse { at: 10.0 }]), [Act::Hold, Act::Nothing, Act::Nothing]);
    }

    #[test]
    fn the_pointer_leaving_the_row_drops_its_click() {
        assert_eq!(
            run(&[click("A-1", 10.0, 1, true), Ev::Leave { row: "A-1" }, Ev::Lapse { at: 10.0 }]),
            [Act::Hold, Act::Nothing, Act::Nothing]
        );
        // Leaving another row drops nothing.
        assert_eq!(
            run(&[click("A-1", 10.0, 1, true), Ev::Leave { row: "B-1" }, Ev::Lapse { at: 10.0 }]),
            [Act::Hold, Act::Nothing, Act::Open("A-1".into())]
        );
    }

    /// The window is under the system's double-click time (~500 ms), so a
    /// held click feels quick — and the bindings table says how long it is.
    #[test]
    fn the_window_is_one_constant_the_table_names() {
        use crate::bindings::{BINDINGS, DOUBLE_CLICK_MS};
        assert!((150.0..500.0).contains(&DOUBLE_CLICK_MS));
        let said = format!("{DOUBLE_CLICK_MS} ms");
        for id in ["select.only", "nav.toggle"] {
            let b = BINDINGS.iter().find(|b| b.id == id).unwrap();
            assert!(b.does.contains(&said), "{id} should name the window ({said}): {:?}", b.does);
        }
    }
}
