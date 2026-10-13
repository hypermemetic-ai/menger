//! ACT-3: **one selection, app-wide** — the nav selects like a file list
//! (owner, 2026-10-13: "I also want shift click and multi select of the nav
//! entries"), and the board and the flat nav select into the same set.
//!
//! - **A selection is ids** ([`Picked`]): the tickets chosen, in the order
//!   they were chosen, the *anchor* (the last plain or Ctrl/Cmd click) and the
//!   *base* (what was chosen when the anchor was set, which a range adds to).
//!   Nothing in it is a row: a row the windowed tree unmounts (MFE-12,
//!   SCALE-24), a node that closes, a filter that hides it, a route change or
//!   a live reorder leave it selected. It is kept in memory only — a reload
//!   starts empty.
//! - **The gestures are pure functions over the drawn order** — the ids of
//!   the rows the view draws, top to bottom, across groups and depths (a
//!   closed node's children are not drawn, so a range never reaches them):
//!   a click is [`Picked::only`]; Shift+click [`Picked::ranged`], from the
//!   anchor to the row; Ctrl/Cmd+click [`Picked::toggled`]; Shift+↑/↓ (and
//!   Shift+j/k) [`Picked::extended`] from the focused row; Space toggles the
//!   focused row; Ctrl/Cmd+A [`Picked::all`] the drawn rows; Esc
//!   [`Picked::default`]. Which a mouse click is: [`Gesture::of`].
//! - **A shortcode's row stands for its root ticket** (AGS-23: the row *is*
//!   the root's, or, under the lens, a heading keyed by it — FILTER-23); a
//!   shortcode with no root stands for no ticket and is not selectable.
//!   Every selectable row carries the id it stands for as `data-pick`
//!   ([`PICK`]).
//! - **One signal for the app** ([`install`], [`signal`]): the nav's rows,
//!   the flat list's and the board's each draw `aria-selected` and the
//!   `picked` tint (`--sel`) from it ([`is_picked`]). The menu (ACT-4) reads
//!   it with [`ids`]: a right-click on a selected row acts on the selection,
//!   on another row on that row alone — the selection kept either way. Until
//!   ACT-4 lands, the ticket menu acts on the row it opened on.
//! - **The nav's foot says it** ([`foot_line`]): "3 selected · clear", and
//!   how many of them the nav's filter hides ([`hidden_of`]: `member { ids,
//!   filter }` — not admitted, and no row drawn for it, which keeps
//!   FILTER-24's open ticket counted as shown).
//! - **A range is capped** at [`RANGE_MAX`] rows — `member`'s most ids, and
//!   twice what ACT-2's `update_many` takes by filter. A Shift+click across
//!   more of the board takes the [`RANGE_MAX`] nearest the anchor.

use std::cell::Cell;
use std::collections::HashSet;

use leptos::prelude::*;

/// The attribute a selectable row carries: the id of the ticket it stands for.
pub(crate) const PICK: &str = "data-pick";

/// The most rows one range selects ([`Picked::ranged`]).
pub(crate) const RANGE_MAX: usize = 1000;

/// The selection: which tickets, in the order chosen; the anchor a range
/// runs from; and the base a range adds to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Picked {
    ids: Vec<String>,
    set: HashSet<String>,
    anchor: Option<String>,
    base: Vec<String>,
}

impl Picked {
    fn of(ids: Vec<String>, anchor: Option<String>, base: Vec<String>) -> Picked {
        let mut seen = HashSet::new();
        let ids: Vec<String> = ids.into_iter().filter(|id| seen.insert(id.clone())).collect();
        Picked { set: seen, ids, anchor, base }
    }

    /// The ids selected, in the order they were chosen.
    pub(crate) fn ids(&self) -> &[String] {
        &self.ids
    }

    pub(crate) fn contains(&self, id: &str) -> bool {
        self.set.contains(id)
    }

    pub(crate) fn len(&self) -> usize {
        self.ids.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Where a range runs from: the last plain or Ctrl/Cmd click.
    pub(crate) fn anchor(&self) -> Option<&str> {
        self.anchor.as_deref()
    }

    /// A plain click: that row alone, and the anchor.
    pub(crate) fn only(id: &str) -> Picked {
        Picked::of(vec![id.to_string()], Some(id.to_string()), Vec::new())
    }

    /// Ctrl/Cmd+click, Space: the row in or out; it becomes the anchor, and
    /// what else is selected is what a range from it adds to.
    pub(crate) fn toggled(&self, id: &str) -> Picked {
        let mut ids = self.ids.clone();
        if self.contains(id) {
            ids.retain(|x| x != id);
        } else {
            ids.push(id.to_string());
        }
        let base = ids.iter().filter(|x| *x != id).cloned().collect();
        Picked::of(ids, Some(id.to_string()), base)
    }

    /// Shift+click: the base and every drawn row from the anchor to `to`,
    /// both included, in `order` — at most [`RANGE_MAX`], the nearest the
    /// anchor. An anchor that is not drawn (scrolled out of a window,
    /// filtered away, never set) has no range: `to` is added and becomes it.
    pub(crate) fn ranged(&self, order: &[String], to: &str) -> Picked {
        let at = |id: &str| order.iter().position(|x| x == id);
        let (Some(a), Some(t)) = (self.anchor.as_deref().and_then(at), at(to)) else {
            let mut ids = self.ids.clone();
            ids.push(to.to_string());
            let base = ids.iter().filter(|x| *x != to).cloned().collect();
            return Picked::of(ids, Some(to.to_string()), base);
        };
        let span: Vec<String> = if a <= t {
            order[a..=t].iter().take(RANGE_MAX).cloned().collect()
        } else {
            order[t..=a].iter().rev().take(RANGE_MAX).cloned().collect()
        };
        let mut ids = self.base.clone();
        ids.extend(span);
        Picked::of(ids, self.anchor.clone(), self.base.clone())
    }

    /// Shift+↑/↓: the range from the anchor to `to`, the focused row (`from`)
    /// standing in for an anchor that is not drawn — the selection so far is
    /// then what the range adds to.
    pub(crate) fn extended(&self, order: &[String], from: &str, to: &str) -> Picked {
        let drawn = self.anchor.as_deref().is_some_and(|a| order.iter().any(|x| x == a));
        if drawn {
            return self.ranged(order, to);
        }
        let base: Vec<String> = self.ids.iter().filter(|x| *x != from).cloned().collect();
        let mut ids = base.clone();
        ids.push(from.to_string());
        Picked::of(ids, Some(from.to_string()), base).ranged(order, to)
    }

    /// Ctrl/Cmd+A: every drawn row.
    pub(crate) fn all(order: &[String]) -> Picked {
        Picked::of(order.to_vec(), order.first().cloned(), Vec::new())
    }
}

/// Which selection a mouse click makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gesture {
    /// A plain click: opens the ticket, as before; the selection is the row.
    Plain,
    /// Shift (with or without Ctrl/Cmd): a range; opens nothing.
    Range,
    /// Ctrl or Cmd: a toggle; opens nothing.
    Toggle,
}

impl Gesture {
    pub(crate) fn of(shift: bool, ctrl: bool, meta: bool) -> Gesture {
        if shift {
            Gesture::Range
        } else if ctrl || meta {
            Gesture::Toggle
        } else {
            Gesture::Plain
        }
    }

    pub(crate) fn of_mouse(ev: &web_sys::MouseEvent) -> Gesture {
        Gesture::of(ev.shift_key(), ev.ctrl_key(), ev.meta_key())
    }

    /// The selection after this click on `id`, the view drawing `order`.
    pub(crate) fn apply(self, sel: &Picked, order: &[String], id: &str) -> Picked {
        match self {
            Gesture::Plain => Picked::only(id),
            Gesture::Range => sel.ranged(order, id),
            Gesture::Toggle => sel.toggled(id),
        }
    }
}

/// The drawn order as rows give it — a ticket drawn twice counts once, where
/// it is drawn first.
pub(crate) fn drawn_order(raw: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    raw.into_iter().filter(|id| seen.insert(id.clone())).collect()
}

/// How many selected tickets the nav's filter hides: not admitted
/// (`admitted`, `member`'s answer) and not drawn (FILTER-24 draws the open
/// ticket whatever the filter says). With no filter, none.
pub(crate) fn hidden_of(ids: &[String], admitted: Option<&HashSet<String>>, drawn: &HashSet<String>) -> usize {
    match admitted {
        None => 0,
        Some(a) => ids.iter().filter(|id| !a.contains(*id) && !drawn.contains(*id)).count(),
    }
}

/// The nav's foot: `3 selected` and, when the filter hides some, `· 2
/// hidden`; nothing with nothing selected.
pub(crate) fn foot_line(n: usize, hidden: usize) -> Option<String> {
    if n == 0 {
        return None;
    }
    Some(if hidden == 0 { format!("{n} selected") } else { format!("{n} selected · {hidden} hidden") })
}

// ---------------------------------------------------------------------
// The app's selection

thread_local! {
    static SEL: Cell<Option<RwSignal<Picked>>> = const { Cell::new(None) };
}

/// The app's one selection, made at its start (src/main.rs's `App`).
pub(crate) fn install() -> RwSignal<Picked> {
    let sig = RwSignal::new(Picked::default());
    SEL.with(|s| s.set(Some(sig)));
    sig
}

/// The app's selection — made here if [`install`] has not run.
pub(crate) fn signal() -> RwSignal<Picked> {
    SEL.with(|s| s.get()).unwrap_or_else(install)
}

/// ACT-4: the ids selected now, in the order chosen (untracked).
#[allow(dead_code)] // ACT-4's menu
pub(crate) fn ids() -> Vec<String> {
    signal().with_untracked(|p| p.ids.clone())
}

/// A row's `aria-selected` and tint: whether `id` is selected. Each row's
/// memo re-runs on a change of the selection; only rows whose answer moved
/// touch the DOM.
pub(crate) fn is_picked(id: String) -> Memo<bool> {
    let sel = signal();
    Memo::new(move |_| sel.with(|p| p.contains(&id)))
}

/// Change the selection; nothing is written when it is the same.
pub(crate) fn set(next: Picked) {
    let sel = signal();
    if sel.with_untracked(|p| *p != next) {
        sel.set(next);
    }
}

/// The selection now (untracked).
pub(crate) fn now() -> Picked {
    signal().with_untracked(Clone::clone)
}

/// Esc: clears a selection there is — `false` when there was none, so the
/// key goes on to whatever else it closes.
pub(crate) fn clear() -> bool {
    let sel = signal();
    if sel.with_untracked(Picked::is_empty) {
        return false;
    }
    sel.set(Picked::default());
    true
}

/// The board's order between two of its rows, `from` and `to` (SCALE-26: a
/// position is arithmetic — every id in between is known unfiltered, and
/// under a filter the ones the board holds): the ids from one to the other,
/// at most [`RANGE_MAX`] from `from`. A row the board cannot place gives
/// just `to`.
pub(crate) fn board_span(board: crate::board::Board, from: &str, to: &str) -> Vec<String> {
    let (Some(a), Some(t)) = (board.pos_of_untracked(from), board.pos_of_untracked(to)) else {
        return vec![to.to_string()];
    };
    let (lo, hi) = if a <= t { (a, t.min(a + RANGE_MAX)) } else { (t.max(a.saturating_sub(RANGE_MAX)), a) };
    (lo..=hi).filter_map(|p| board.id_at(p)).collect()
}

/// A click on a board row (`id`), as the nav's rows take one: the board's
/// order runs from the anchor when the board can place it.
pub(crate) fn board_click(board: crate::board::Board, gesture: Gesture, id: &str) {
    let sel = now();
    let from = sel.anchor().filter(|a| board.pos_of_untracked(a).is_some()).unwrap_or(id).to_string();
    set(gesture.apply(&sel, &board_span(board, &from, id), id));
}

/// Shift+j/k on the board: the selection from the anchor (or the focused
/// row `from`) to `to`.
pub(crate) fn board_extend(board: crate::board::Board, from: &str, to: &str) {
    let sel = now();
    let start = sel.anchor().filter(|a| board.pos_of_untracked(a).is_some()).unwrap_or(from).to_string();
    set(sel.extended(&board_span(board, &start, to), from, to));
}

/// The id a row stands for, if it is selectable.
pub(crate) fn pick_of(el: &web_sys::Element) -> Option<String> {
    el.get_attribute(PICK)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn ids(p: &Picked) -> Vec<&str> {
        p.ids().iter().map(String::as_str).collect()
    }

    #[test]
    fn a_click_selects_that_row_and_anchors_there() {
        let p = Picked::only("A-2");
        assert_eq!(ids(&p), ["A-2"]);
        assert_eq!(p.anchor(), Some("A-2"));
        assert_eq!(Gesture::Plain.apply(&p, &[], "B-1"), Picked::only("B-1"));
    }

    #[test]
    fn shift_click_selects_from_the_anchor_in_the_drawn_order_either_way() {
        // across groups and depths: the order is what the nav draws
        let o = order(&["A-1", "A-2", "A-5", "B-1", "B-3", "C-1"]);
        let p = Picked::only("A-2").ranged(&o, "B-3");
        assert_eq!(ids(&p), ["A-2", "A-5", "B-1", "B-3"]);
        assert_eq!(p.anchor(), Some("A-2"), "a range keeps its anchor");
        // a second shift-click replaces the range, from the same anchor
        let q = p.ranged(&o, "A-1");
        assert_eq!(ids(&q), ["A-2", "A-1"]);
        assert!(q.contains("A-1") && !q.contains("B-1"));
    }

    #[test]
    fn a_closed_node_s_children_are_not_drawn_so_not_ranged() {
        // A-2 is closed: its children A-3, A-4 are not in the drawn order
        let o = order(&["A-1", "A-2", "A-5"]);
        let p = Picked::only("A-1").ranged(&o, "A-5");
        assert_eq!(ids(&p), ["A-1", "A-2", "A-5"]);
        assert!(!p.contains("A-3"));
    }

    #[test]
    fn ctrl_click_toggles_and_a_range_adds_to_what_it_left() {
        let o = order(&["A-1", "A-2", "A-3", "A-4", "A-5", "A-6"]);
        let p = Picked::only("A-1").toggled("A-4");
        assert_eq!(ids(&p), ["A-1", "A-4"]);
        assert_eq!(p.anchor(), Some("A-4"));
        let p = p.ranged(&o, "A-6");
        assert_eq!(ids(&p), ["A-1", "A-4", "A-5", "A-6"], "the base (A-1) stays; the range runs from A-4");
        // toggled off: the anchor moves there, and it is out
        let q = p.toggled("A-5");
        assert_eq!(ids(&q), ["A-1", "A-4", "A-6"]);
        assert_eq!(q.anchor(), Some("A-5"));
        assert_eq!(Gesture::Toggle.apply(&q, &o, "A-1").ids(), ["A-4", "A-6"]);
    }

    #[test]
    fn an_anchor_not_drawn_makes_no_range_the_row_is_added() {
        let o = order(&["B-1", "B-2", "B-3"]);
        let p = Picked::only("A-9").ranged(&o, "B-2");
        assert_eq!(ids(&p), ["A-9", "B-2"]);
        assert_eq!(p.anchor(), Some("B-2"));
        // from nothing at all
        assert_eq!(ids(&Picked::default().ranged(&o, "B-3")), ["B-3"]);
    }

    #[test]
    fn shift_arrows_extend_from_the_focused_row() {
        let o = order(&["A-1", "A-2", "A-3", "A-4"]);
        // no anchor drawn: the focused row anchors, what was selected stays
        let p = Picked::only("Z-1").extended(&o, "A-2", "A-3");
        assert_eq!(ids(&p), ["Z-1", "A-2", "A-3"]);
        assert_eq!(p.anchor(), Some("A-2"));
        let p = p.extended(&o, "A-3", "A-4");
        assert_eq!(ids(&p), ["Z-1", "A-2", "A-3", "A-4"]);
        // and back up past the anchor
        let p = p.extended(&o, "A-4", "A-3").extended(&o, "A-3", "A-2").extended(&o, "A-2", "A-1");
        assert_eq!(ids(&p), ["Z-1", "A-2", "A-1"]);
        // an anchor drawn: a range from it
        let q = Picked::only("A-1").extended(&o, "A-1", "A-2");
        assert_eq!(ids(&q), ["A-1", "A-2"]);
    }

    #[test]
    fn select_all_is_every_drawn_row_and_esc_clears() {
        let o = drawn_order(order(&["A-1", "A-2", "A-1", "B-1"]));
        assert_eq!(o, ["A-1", "A-2", "B-1"], "a row drawn twice counts once");
        let p = Picked::all(&o);
        assert_eq!(p.len(), 3);
        assert!(Picked::default().is_empty());
    }

    #[test]
    fn a_range_is_capped_nearest_the_anchor() {
        let o: Vec<String> = (1..=RANGE_MAX + 50).map(|i| format!("A-{i}")).collect();
        let p = Picked::only("A-1").ranged(&o, o.last().unwrap());
        assert_eq!(p.len(), RANGE_MAX);
        assert!(p.contains("A-1") && !p.contains(o.last().unwrap()));
        let q = Picked::only(o.last().unwrap()).ranged(&o, "A-1");
        assert_eq!(q.len(), RANGE_MAX);
        assert!(q.contains(o.last().unwrap()) && !q.contains("A-1"));
    }

    #[test]
    fn which_gesture_a_click_is() {
        assert_eq!(Gesture::of(false, false, false), Gesture::Plain);
        assert_eq!(Gesture::of(true, false, false), Gesture::Range);
        assert_eq!(Gesture::of(true, true, false), Gesture::Range);
        assert_eq!(Gesture::of(false, true, false), Gesture::Toggle);
        assert_eq!(Gesture::of(false, false, true), Gesture::Toggle, "Cmd on a Mac");
    }

    #[test]
    fn the_foot_counts_the_selection_and_what_the_filter_hides() {
        let sel = order(&["A-1", "A-2", "A-3"]);
        let drawn: HashSet<String> = ["A-1".to_string(), "A-3".to_string()].into();
        assert_eq!(hidden_of(&sel, None, &drawn), 0, "no filter hides nothing");
        let admitted: HashSet<String> = ["A-1".to_string()].into();
        // A-3 is not admitted but drawn (the open ticket, FILTER-24)
        assert_eq!(hidden_of(&sel, Some(&admitted), &drawn), 1);
        assert_eq!(foot_line(0, 0), None);
        assert_eq!(foot_line(3, 0).as_deref(), Some("3 selected"));
        assert_eq!(foot_line(3, 2).as_deref(), Some("3 selected · 2 hidden"));
    }
}
