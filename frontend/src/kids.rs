//! SCALE-4: the ticket page's children, by page.
//!
//! Until SCALE-4 a row carried `children` — every child id — and the page
//! rendered that list straight off the row, kept live because
//! `children_changed` carried the whole parent. The row carries only
//! `child_count` now, and the event is the parent's id, the child and the
//! new count. So the page asks for its children the way any client does
//! (`list { parent, summary, limit }`, `after = next` for more) when it
//! opens, and keeps what it holds live by applying each `added` to it —
//! the intended usage the ticket names. `child_count` decides whether
//! there is anything to ask for, and how many are left.
//!
//! MFE-17: and each `removed` — a `set_parent` moves a child away, and the
//! old parent's page lets it go (`child_removed`, which the app dropped
//! until now).
//!
//! This is the list's state and the rules that move it — pure, so a
//! native test can pin them. The fetching is `session::load_children`; the
//! deltas reach it through `session`'s registry of open lists.
//!
//! SCALE-24: the rows a page fetches are the page's own holds in the row
//! cache (src/cache.rs), given back when it closes — no longer inserted into
//! the board's list. The nav's children by page (src/nav.rs) are a sibling
//! of this list, not this list: the nav has to reach any child of 163,530
//! by scrolling, where this page reaches them a "more" at a time.
//!
//! FILTER-16: **under the global filter** the list is the children the
//! filter admits — `list { parent, filter, summary, limit }`, the same pages
//! — and the page says how many it hides: `child_count` less the matches
//! (the first page's rows plus its `remaining`), "n hidden by the filter",
//! which shows every child on a click. A delta cannot tell whether the
//! filter admits a child: one born or moved in waits in `unsure` for
//! `member`; the membership watch's `entered` and `left` (a child's own
//! write moved it in or out) move it in place; and every delta moves
//! `stale`, on which the page counts the matches again (`list { parent,
//! filter, limit: 0 }`, from the cells). With the global filter empty the
//! list is what it always was. [`Members`] is what the page knows of the
//! filter's verdict on the tickets it names: itself (the notice under the
//! title), its blockers and unlocks (dimmed when outside), and — every
//! child shown — its children.

use std::collections::HashMap;

/// One open page's children: what it holds, in the store's `(epic, seq)`
/// order, and where the next page starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Kids {
    /// The children held — every page fetched so far, plus each one born
    /// or moved in among them since.
    pub(crate) ids: Vec<String>,
    /// The cursor for the next page; `None` once the list is whole.
    pub(crate) next: Option<String>,
    /// The first page has answered. Until it has, a delta is not applied:
    /// the page, answered after it, already reflects it.
    pub(crate) loaded: bool,
    /// FILTER-16: the global filter the page is under (its wire text) —
    /// `None`: none, and the list is every child, as it always was.
    pub(crate) filter: Option<String>,
    /// FILTER-16: every child shown, the filter notwithstanding (the
    /// "hidden by the filter" line, clicked).
    pub(crate) all: bool,
    /// FILTER-16: under a filter, how many children it admits — `None`
    /// until counted.
    pub(crate) matched: Option<u64>,
    /// FILTER-16: under a filtered list, the children born or moved in
    /// whose membership `member` is to say, in arrival order.
    pub(crate) unsure: Vec<String>,
    /// FILTER-16: moves whenever the matches may have moved — what the page
    /// counts them again on.
    pub(crate) stale: u32,
    /// FILTER-16: the server did not take the filter — its words.
    pub(crate) refused: Option<String>,
}

/// An id's place in the store's order — `{epic}-{seq}`, which every id the
/// server allocates is (MNG-9). One that is not shaped like that has no
/// place to be inserted at. SCALE-24: the nav's lists place children by it
/// too, and turn a position into a cursor with it.
pub(crate) fn key(id: &str) -> Option<(&str, u32)> {
    id.rsplit_once('-')
        .and_then(|(epic, seq)| Some((epic, seq.parse::<u32>().ok()?)))
}

impl Kids {
    /// FILTER-16: a list not yet asked for, under `filter` (`None`: every
    /// child, as always), every child shown when `all`.
    pub(crate) fn under(filter: Option<String>, all: bool) -> Self {
        Kids { filter, all, ..Kids::default() }
    }

    /// FILTER-16: the filter the list is asked under — the page's, unless
    /// every child is shown.
    pub(crate) fn list_filter(&self) -> Option<&str> {
        self.filter.as_deref().filter(|_| !self.all)
    }

    /// FILTER-16: a page answered to a question asked under `asked` (the
    /// list's filter then): taken only if that is still the list's — the
    /// first page under a filter counts the matches (its rows plus its
    /// `remaining`). Whether it was taken.
    pub(crate) fn page_under(&mut self, asked: Option<&str>, first: bool, ids: Vec<String>, next: Option<String>, remaining: u64) -> bool {
        if asked != self.list_filter() {
            return false;
        }
        if first && asked.is_some() {
            self.matched = Some(ids.len() as u64 + remaining);
        }
        self.refused = None;
        self.page(first, ids, next);
        true
    }

    /// FILTER-16: the server refused the question asked under `asked`: the
    /// list is empty, with its words.
    pub(crate) fn refuse(&mut self, asked: Option<&str>, why: String) -> bool {
        if asked != self.list_filter() {
            return false;
        }
        self.ids.clear();
        self.next = None;
        self.loaded = true;
        self.refused = Some(why);
        true
    }

    /// FILTER-16: the matches counted (`list { parent, filter, limit: 0 }`).
    pub(crate) fn counted(&mut self, matched: u64) {
        self.matched = Some(matched);
    }

    fn touch(&mut self) {
        self.stale = self.stale.wrapping_add(1);
    }

    /// A page of children answered. The first page (no cursor asked for)
    /// replaces what was held — a reconnect re-asks from the top — and a
    /// later one extends it.
    pub(crate) fn page(&mut self, first: bool, ids: Vec<String>, next: Option<String>) {
        if first {
            self.ids = ids;
        } else {
            for id in ids {
                if !self.ids.contains(&id) {
                    self.ids.push(id);
                }
            }
        }
        self.next = next;
        self.loaded = true;
    }

    /// Whether `id`'s place is inside what is held: anywhere once the list
    /// is whole, else at or before the last child held — past that, the
    /// page that reaches it will bring it.
    fn within(&self, id: &str) -> bool {
        match (&self.next, self.ids.last()) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(_), Some(last)) => match (key(id), key(last)) {
                (Some(k), Some(l)) => k <= l,
                _ => false,
            },
        }
    }

    /// `children_changed { added }` for this parent: `added` goes in at its
    /// place in the store's order, if that is inside what is held and it is
    /// not held already. Returns whether the list changed.
    ///
    /// FILTER-16: under a filtered list it waits in `unsure` for `member`
    /// ([`Kids::admit`]); under any filter the matches are counted again.
    pub(crate) fn added(&mut self, id: &str) -> bool {
        if !self.loaded {
            return false;
        }
        if self.list_filter().is_some() {
            if self.ids.iter().any(|x| x == id) || self.unsure.iter().any(|x| x == id) {
                return false;
            }
            self.unsure.push(id.to_string());
            return true;
        }
        let moved = self.insert(id);
        if self.filter.is_some() {
            self.touch();
            return true;
        }
        moved
    }

    /// FILTER-16: `member`'s word on an `unsure` child — admitted, it goes
    /// in at its place (as `added` puts one); either way it is sure now, and
    /// an admitted one moves the count.
    pub(crate) fn admit(&mut self, id: &str, matched: bool) -> bool {
        let before = self.unsure.len();
        self.unsure.retain(|x| x != id);
        if self.unsure.len() == before {
            return false;
        }
        if matched {
            self.insert(id);
            self.touch();
        }
        true
    }

    /// FILTER-16: the membership watch's `entered` for a child of this
    /// parent — a write of its own moved it into the filter: a filtered list
    /// takes it at its place; the matches are counted again.
    pub(crate) fn entered(&mut self, id: &str) -> bool {
        if !self.loaded || self.filter.is_none() {
            return false;
        }
        if self.list_filter().is_some() {
            self.unsure.retain(|x| x != id);
            self.insert(id);
        }
        self.touch();
        true
    }

    /// FILTER-16: `left` for a child of this parent — out of a filtered
    /// list; the matches are counted again.
    pub(crate) fn left(&mut self, id: &str) -> bool {
        if !self.loaded || self.filter.is_none() {
            return false;
        }
        if self.list_filter().is_some() {
            self.ids.retain(|x| x != id);
            self.unsure.retain(|x| x != id);
        }
        self.touch();
        true
    }

    /// `id` in at its place in the store's order, if that is inside what
    /// is held and it is not held already. Whether it went in.
    fn insert(&mut self, id: &str) -> bool {
        if self.ids.iter().any(|x| x == id) || !self.within(id) {
            return false;
        }
        let at = match key(id) {
            Some(k) => self.ids.partition_point(|x| key(x).is_some_and(|xk| xk < k)),
            None => self.ids.len(),
        };
        self.ids.insert(at, id.to_string());
        true
    }

    /// MFE-17: `child_removed { removed }` for this parent — the child was
    /// moved under another — leaves the list, if held. Returns whether the
    /// list changed.
    ///
    /// FILTER-16: under a filter, the matches are counted again.
    pub(crate) fn removed(&mut self, id: &str) -> bool {
        let before = self.ids.len();
        self.ids.retain(|x| x != id);
        self.unsure.retain(|x| x != id);
        let moved = self.ids.len() != before;
        if self.filter.is_some() && self.loaded {
            self.touch();
            return true;
        }
        moved
    }

    /// How many children are not held yet, given the parent's count — what
    /// the "more" control offers. Zero once the list is whole. FILTER-16:
    /// under a filtered list, of the matches.
    pub(crate) fn unshown(&self, child_count: u64) -> u64 {
        if self.next.is_none() {
            0
        } else {
            let total = if self.list_filter().is_some() { self.matched.unwrap_or(child_count) } else { child_count };
            total.saturating_sub(self.ids.len() as u64)
        }
    }

    /// FILTER-16: how many children the filter hides, given the parent's
    /// count — zero with no filter, or before the matches are counted.
    pub(crate) fn hidden(&self, child_count: u64) -> u64 {
        match (&self.filter, self.matched) {
            (Some(_), Some(m)) => child_count.saturating_sub(m),
            _ => 0,
        }
    }
}

/// FILTER-16: the line under a ticket page's children while the global
/// filter hides some: "n hidden by the filter" (a click shows every child);
/// every child shown, "n outside the filter — hide them". None while it
/// hides none.
pub(crate) fn hidden_line(hidden: u64, all: bool) -> Option<String> {
    match (hidden, all) {
        (0, _) => None,
        (n, false) => Some(format!("{n} hidden by the filter")),
        (n, true) => Some(format!("{n} outside the filter — hide them")),
    }
}

/// FILTER-16: what a ticket page knows of the global filter's verdict on
/// the tickets it names — `member`'s answers, kept by the membership
/// watch's `entered` and `left` for the ids it knows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Members {
    known: HashMap<String, bool>,
}

impl Members {
    /// The ids of `want` not known yet — what to ask `member`, in order,
    /// once each.
    pub(crate) fn unknown(&self, want: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for id in want {
            if !self.known.contains_key(id) && !out.contains(id) {
                out.push(id.clone());
            }
        }
        out
    }

    /// `member`'s answer.
    pub(crate) fn answered(&mut self, answer: impl IntoIterator<Item = (String, bool)>) {
        self.known.extend(answer);
    }

    /// `entered` (`true`) or `left` (`false`) for `id`, if it is one the
    /// page asked of; whether it moved.
    pub(crate) fn moved(&mut self, id: &str, inside: bool) -> bool {
        match self.known.get_mut(id) {
            Some(m) if *m != inside => {
                *m = inside;
                true
            }
            _ => false,
        }
    }

    /// Whether the filter is known not to admit `id`.
    pub(crate) fn outside(&self, id: &str) -> bool {
        self.known.get(id) == Some(&false)
    }
}

#[cfg(test)]
mod tests {
    use super::{hidden_line, Kids, Members};

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// A delta waits for the first page: until it answers, the page itself
    /// is the truth, and a delta applied first would be applied twice.
    #[test]
    fn nothing_moves_before_the_first_page() {
        let mut k = Kids::default();
        assert!(!k.added("PAR-3"));
        assert!(!k.removed("PAR-3"));
        assert_eq!(k, Kids::default());
        k.page(true, ids(&["PAR-2"]), None);
        assert!(k.loaded);
        assert!(k.added("PAR-3"));
        assert_eq!(k.ids, ids(&["PAR-2", "PAR-3"]));
    }

    /// A whole list takes a birth or a move-in at its place in the store's
    /// order — numeric seq, epic before seq — and a second delta for the
    /// same child changes nothing.
    #[test]
    fn a_whole_list_takes_each_child_at_its_place() {
        let mut k = Kids::default();
        k.page(true, ids(&["AAA-1", "PAR-3", "PAR-10", "SUB-1"]), None);
        assert!(k.added("PAR-9"));
        assert!(k.added("ZED-1"));
        assert!(k.added("AA-4"));
        assert!(!k.added("PAR-9"), "held already");
        assert_eq!(k.ids, ids(&["AA-4", "AAA-1", "PAR-3", "PAR-9", "PAR-10", "SUB-1", "ZED-1"]));
    }

    /// MFE-17: a child moved away leaves the list, once; one not held (past
    /// the held pages, or never here) changes nothing — and moving it back
    /// puts it at its place again.
    #[test]
    fn a_child_moved_away_leaves_the_list() {
        let mut k = Kids::default();
        k.page(true, ids(&["AAA-1", "PAR-3", "PAR-5", "SUB-1"]), None);
        assert!(k.removed("PAR-3"));
        assert!(!k.removed("PAR-3"));
        assert!(!k.removed("NOPE-1"));
        assert_eq!(k.ids, ids(&["AAA-1", "PAR-5", "SUB-1"]));
        assert!(k.added("PAR-3"));
        assert_eq!(k.ids, ids(&["AAA-1", "PAR-3", "PAR-5", "SUB-1"]));
    }

    /// A list held in part takes a child that falls inside what it holds
    /// and leaves one past it to the page that reaches it — which then
    /// brings it once; `unshown` is the count less what is held.
    #[test]
    fn a_partial_list_takes_only_what_falls_inside_it() {
        let mut k = Kids::default();
        k.page(true, ids(&["MAP-2", "MAP-3", "MAP-5"]), Some("MAP-5".into()));
        assert_eq!(k.unshown(10), 7);
        assert!(k.added("MAP-4"), "inside: between two held");
        assert!(!k.added("MAP-11"), "past the last held: the next page brings it");
        assert_eq!(k.unshown(11), 7);
        k.page(false, ids(&["MAP-6", "MAP-11"]), None);
        assert_eq!(k.ids, ids(&["MAP-2", "MAP-3", "MAP-4", "MAP-5", "MAP-6", "MAP-11"]));
        assert_eq!(k.unshown(11), 0, "whole");
        // A reconnect re-asks from the top and replaces what was held.
        k.page(true, ids(&["MAP-2"]), Some("MAP-2".into()));
        assert_eq!(k.ids, ids(&["MAP-2"]));
    }

    /// FILTER-16: under the global filter the list is the matches, and the
    /// line says how many it hides — `child_count` less the first page's
    /// rows and `remaining`; a click shows every child; a page asked under
    /// another filter (or none) than the list's now is dropped.
    #[test]
    fn under_a_filter_the_line_counts_what_it_hides() {
        let mut k = Kids::under(Some("kind build".into()), false);
        assert_eq!(k.list_filter(), Some("kind build"));
        assert!(!k.page_under(None, true, ids(&["AGS-2"]), None, 0), "asked with no filter: not this list's");
        assert!(k.page_under(Some("kind build"), true, ids(&["AGS-3", "AGS-5", "AGS-6"]), Some("AGS-6".into()), 15));
        assert_eq!(k.matched, Some(18));
        assert_eq!(k.hidden(22), 4);
        assert_eq!(hidden_line(k.hidden(22), false).as_deref(), Some("4 hidden by the filter"));
        assert_eq!(k.unshown(22), 15, "more: of the matches, not of every child");
        assert!(k.page_under(Some("kind build"), false, ids(&["AGS-7"]), None, 0));
        assert_eq!(k.matched, Some(18), "a later page does not count");
        assert_eq!(k.unshown(22), 0);
        assert_eq!(hidden_line(0, false), None, "nothing hidden, no line");
        assert_eq!(hidden_line(1, false).as_deref(), Some("1 hidden by the filter"));

        // The click: every child, the count kept, the line turned over.
        let mut all = Kids::under(Some("kind build".into()), true);
        assert_eq!(all.list_filter(), None, "every child is asked with no filter");
        assert!(all.page_under(None, true, ids(&["AGS-2", "AGS-3"]), Some("AGS-3".into()), 20));
        assert_eq!(all.matched, None, "an unfiltered page counts no match");
        all.counted(18);
        assert_eq!(all.hidden(22), 4);
        assert_eq!(all.unshown(22), 20);
        assert_eq!(hidden_line(all.hidden(22), true).as_deref(), Some("4 outside the filter — hide them"));

        // No filter: no line, and the list is today's.
        let mut plain = Kids::under(None, false);
        assert!(plain.page_under(None, true, ids(&["AGS-2"]), None, 0));
        assert_eq!((plain.hidden(22), plain.matched), (0, None));
        assert_eq!(plain, { let mut d = Kids::default(); d.page(true, ids(&["AGS-2"]), None); d });

        // Refused: the server's words, nothing listed.
        let mut r = Kids::under(Some("under X".into()), false);
        assert!(r.refuse(Some("under X"), "under comes with FILTER-13".into()));
        assert!(r.loaded && r.ids.is_empty() && r.refused.is_some());
    }

    /// FILTER-16: deltas under a filtered list — a birth waits for
    /// `member`; admitted, it goes in at its place; `entered` and `left`
    /// move a child in and out; each moves `stale` (the count is asked
    /// again); with no filter the deltas are SCALE-4's and MFE-17's.
    #[test]
    fn under_a_filter_deltas_wait_for_the_verdict() {
        let mut k = Kids::under(Some("status Ready".into()), false);
        assert!(!k.added("PAR-4"), "before the first page, nothing");
        k.page_under(Some("status Ready"), true, ids(&["PAR-2", "PAR-9"]), None, 0);
        let s0 = k.stale;
        assert!(k.added("PAR-4"));
        assert_eq!(k.ids, ids(&["PAR-2", "PAR-9"]), "not shown until member says");
        assert_eq!(k.unsure, ids(&["PAR-4"]));
        assert!(!k.added("PAR-4"), "once");
        assert!(k.admit("PAR-4", true));
        assert_eq!(k.ids, ids(&["PAR-2", "PAR-4", "PAR-9"]));
        assert!(k.stale != s0, "admitted: counted again");
        assert!(k.added("PAR-5"));
        let s1 = k.stale;
        assert!(k.admit("PAR-5", false));
        assert_eq!((k.ids.len(), k.stale), (3, s1), "not admitted: not shown, the count unmoved");
        assert!(!k.admit("PAR-5", true), "only an unsure child");

        assert!(k.left("PAR-2"));
        assert_eq!(k.ids, ids(&["PAR-4", "PAR-9"]));
        assert!(k.entered("PAR-3"));
        assert_eq!(k.ids, ids(&["PAR-3", "PAR-4", "PAR-9"]));
        let s2 = k.stale;
        assert!(k.removed("PAR-3"));
        assert_eq!(k.ids, ids(&["PAR-4", "PAR-9"]));
        assert!(k.stale != s2, "a move away is counted again");

        // Every child shown: deltas as always, the count asked again.
        let mut all = Kids::under(Some("status Ready".into()), true);
        all.page_under(None, true, ids(&["PAR-2"]), None, 0);
        let s3 = all.stale;
        assert!(all.added("PAR-3"));
        assert_eq!(all.ids, ids(&["PAR-2", "PAR-3"]));
        assert!(all.stale != s3);
        assert!(all.left("PAR-2"));
        assert_eq!(all.ids, ids(&["PAR-2", "PAR-3"]), "every child stays shown");

        // No filter: entered / left are not this list's.
        let mut plain = Kids::default();
        plain.page(true, ids(&["PAR-2"]), None);
        assert!(!plain.entered("PAR-3") && !plain.left("PAR-2"));
        assert_eq!(plain.stale, 0);
    }

    /// FILTER-16: the page's verdicts — asked once per id, answered, moved
    /// by `entered` / `left` only for the ids it knows.
    #[test]
    fn members_know_what_was_asked() {
        let mut m = Members::default();
        let want = ids(&["SCOPE-1", "B-1", "U-1", "B-1"]);
        assert_eq!(m.unknown(&want), ids(&["SCOPE-1", "B-1", "U-1"]));
        m.answered([("SCOPE-1".to_string(), false), ("B-1".to_string(), true), ("U-1".to_string(), false)]);
        assert!(m.unknown(&want).is_empty());
        assert!(m.outside("SCOPE-1") && !m.outside("B-1") && m.outside("U-1"));
        assert!(!m.outside("NEVER-1"), "not known: not marked");
        assert!(m.moved("U-1", true) && !m.outside("U-1"));
        assert!(m.moved("B-1", false) && m.outside("B-1"));
        assert!(!m.moved("B-1", false), "no move");
        assert!(!m.moved("NEVER-1", false), "an id it never asked of");
        assert!(!m.outside("NEVER-1"));
    }
}
