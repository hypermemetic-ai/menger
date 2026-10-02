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
    pub(crate) fn added(&mut self, id: &str) -> bool {
        if !self.loaded || self.ids.iter().any(|x| x == id) || !self.within(id) {
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
    pub(crate) fn removed(&mut self, id: &str) -> bool {
        let before = self.ids.len();
        self.ids.retain(|x| x != id);
        self.ids.len() != before
    }

    /// How many children are not held yet, given the parent's count — what
    /// the "more" control offers. Zero once the list is whole.
    pub(crate) fn unshown(&self, child_count: u64) -> u64 {
        if self.next.is_none() {
            0
        } else {
            child_count.saturating_sub(self.ids.len() as u64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Kids;

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
}
