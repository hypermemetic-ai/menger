//! ACT-7: **what is open in the nav, as a shape** (owner, 2026-10-13: "give
//! me a collapse and expand all option on right click … expand all or expand
//! all from the shortcode for instance").
//!
//! Until ACT-7 the nav's expansion was a set of keys (AGS-12's
//! `menger.nav.expanded`): a node was open when its key was in it. That set
//! cannot say "everything under THEME-4" — the nav does not know what is
//! under THEME-4 until it asks (SCALE-24: nothing walks the store), and
//! "everything" is 650 keys on prod, 10^6 at scale. So the expansion is a
//! [`Shape`]: a default for the whole tree ([`Shape::all`]) and a mark per
//! key the reader touched ([`Mark`]), each stamped with a clock tick:
//!
//! - **A plain mark** opens or closes its own row (a click on ▸, h/l, a
//!   double-click, the reveal of a deep link) — what the set held before.
//! - **A deep mark** also says what every row under it inherits — open
//!   ([`Op::ExpandSubtree`], the shortcode's root for [`Op::ExpandShortcode`])
//!   or closed. Nothing under it is enumerated: a node mounts, reads its
//!   ancestors' keys (the chain it is drawn under, hosted shortcodes
//!   included) and folds the shape over them ([`Shape::inherited`]).
//! - **The newer word wins along a chain.** A mark under a deep mark counts
//!   only when it is newer than the deep mark (a higher tick): expanding
//!   THEME-4's subtree overrides whatever was closed inside it before, and a
//!   row closed inside it afterwards stays closed. Expand all and Collapse
//!   all reset every mark ([`Shape::default`] is the nav's opening state:
//!   shortcodes only).
//! - **Collapse others** keeps open only the chain down to the row
//!   ([`Op::CollapseOthers`]) — the row too, if it was open.
//!
//! Every op is a pure function of the shape ([`Shape::apply`]); the nav
//! (src/nav.rs) passes the row's key and its chain from the DOM, and the
//! menu (src/menu.rs) offers them under **Nav ▸** ([`items`]). The shape is
//! kept where the set was — the same localStorage key, a set from before
//! read as plain marks ([`Shape::load`]), so a reload keeps it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What the reader said of one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Mark {
    /// The row itself open.
    #[serde(rename = "o")]
    pub(crate) open: bool,
    /// A deep mark: what every row under it inherits.
    #[serde(rename = "k", default, skip_serializing_if = "Option::is_none")]
    pub(crate) kids: Option<bool>,
    /// When it was said — the newer word wins along a chain.
    #[serde(rename = "t")]
    pub(crate) at: u64,
}

/// What a row inherits from the rows above it: open or closed, and from
/// which tick (0: the tree's default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Inh {
    pub(crate) open: bool,
    pub(crate) at: u64,
}

/// The nav's expansion: the tree's default, the marks, and the clock.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Shape {
    /// Every row open (Expand all) — otherwise every row closed.
    #[serde(default)]
    pub(crate) all: bool,
    #[serde(default)]
    pub(crate) marks: BTreeMap<String, Mark>,
    #[serde(default)]
    pub(crate) clock: u64,
}

/// What the localStorage key held: AGS-12's set of open keys, or a shape.
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    Keys(Vec<String>),
    Shape(Shape),
}

/// One of the Nav ▸ items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    /// The row and every row under it, at any depth, open.
    ExpandSubtree,
    /// The row and every row under it closed.
    CollapseSubtree,
    /// The row's shortcode from its root down — the shortcodes it hosts too.
    ExpandShortcode,
    CollapseShortcode,
    /// Every row closed but the chain down to the row.
    CollapseOthers,
    /// The whole tree.
    ExpandAll,
    /// The nav's opening state: shortcodes only.
    CollapseAll,
    /// The chain to the open ticket opens; its row is brought into view and
    /// takes focus (the nav's, not the shape's).
    Reveal,
}

impl Op {
    /// The item's attribute — what a harness finds it by (`data-nav-op`).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Op::ExpandSubtree => "expand-subtree",
            Op::CollapseSubtree => "collapse-subtree",
            Op::ExpandShortcode => "expand-shortcode",
            Op::CollapseShortcode => "collapse-shortcode",
            Op::CollapseOthers => "collapse-others",
            Op::ExpandAll => "expand-all",
            Op::CollapseAll => "collapse-all",
            Op::Reveal => "reveal",
        }
    }
}

/// Where the Nav ▸ menu was opened — on a row of the tree (its label, and
/// whether it opens onto anything; the row's shortcode), on the flat list,
/// or on the nav's empty space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Row { label: String, has_kids: bool, shortcode: Option<String> },
    Flat,
    Space,
}

/// One item of the Nav ▸ menu: its words and its op, or a separator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    Do(Op, String),
    Rule,
}

/// The Nav ▸ items for where the menu opened. A row of the tree has them
/// all (its subtree's only when it has one; its shortcode's when it has a
/// shortcode); the nav's empty space only those that need no row; the flat
/// list only the reveal — nothing else there means anything. `reveal` is
/// whether a ticket is open.
pub(crate) fn items(place: &Place, reveal: bool) -> Vec<Item> {
    let mut out = Vec::new();
    if let Place::Row { label, has_kids, shortcode } = place {
        if *has_kids {
            out.push(Item::Do(Op::ExpandSubtree, format!("Expand {label}'s subtree")));
            out.push(Item::Do(Op::CollapseSubtree, format!("Collapse {label}'s subtree")));
        }
        if let Some(sc) = shortcode {
            out.push(Item::Do(Op::ExpandShortcode, format!("Expand all of {sc}")));
            out.push(Item::Do(Op::CollapseShortcode, format!("Collapse all of {sc}")));
        }
        out.push(Item::Do(Op::CollapseOthers, "Collapse others".into()));
        out.push(Item::Rule);
    }
    if *place != Place::Flat {
        out.push(Item::Do(Op::ExpandAll, "Expand all".into()));
        out.push(Item::Do(Op::CollapseAll, "Collapse all".into()));
    }
    if reveal {
        if !out.is_empty() {
            out.push(Item::Rule);
        }
        out.push(Item::Do(Op::Reveal, "Reveal the open ticket".into()));
    }
    out
}

impl Shape {
    /// The shape the stored text says — AGS-12's set read as plain marks
    /// (each open, tick 1), nothing stored or unreadable the default.
    pub(crate) fn load(raw: Option<&str>) -> Shape {
        match raw.and_then(|r| serde_json::from_str::<Stored>(r).ok()) {
            Some(Stored::Shape(s)) => s,
            Some(Stored::Keys(keys)) => {
                let marks: BTreeMap<String, Mark> = keys.into_iter().map(|k| (k, Mark { open: true, kids: None, at: 1 })).collect();
                let clock = u64::from(!marks.is_empty());
                Shape { all: false, marks, clock }
            }
            None => Shape::default(),
        }
    }

    pub(crate) fn stored(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// The row's own mark, if it is newer than what the row inherits.
    fn mark(&self, key: &str, inh: Inh) -> Option<Mark> {
        self.marks.get(key).copied().filter(|m| m.at > inh.at)
    }

    /// What the tree's top rows inherit.
    pub(crate) fn top(&self) -> Inh {
        Inh { open: self.all, at: 0 }
    }

    /// What the rows under `key` inherit, given what `key` inherits.
    pub(crate) fn below(&self, key: &str, inh: Inh) -> Inh {
        match self.mark(key, inh) {
            Some(Mark { kids: Some(open), at, .. }) => Inh { open, at },
            _ => inh,
        }
    }

    /// What a row drawn under `chain` (its ancestors' keys, the top first)
    /// inherits.
    pub(crate) fn inherited<S: AsRef<str>>(&self, chain: &[S]) -> Inh {
        chain.iter().fold(self.top(), |inh, k| self.below(k.as_ref(), inh))
    }

    /// Whether the row `key`, drawn under `chain`, is open.
    pub(crate) fn is_open<S: AsRef<str>>(&self, chain: &[S], key: &str) -> bool {
        let inh = self.inherited(chain);
        self.mark(key, inh).map_or(inh.open, |m| m.open)
    }

    /// The row `key`, drawn under `chain`, opened or closed — and nothing
    /// else: a deep mark keeps what it says of the rows under it; a plain
    /// mark that would say what the row inherits anyway goes.
    pub(crate) fn set_open<S: AsRef<str>>(&mut self, chain: &[S], key: &str, open: bool) {
        let inh = self.inherited(chain);
        match self.mark(key, inh) {
            Some(m) if m.kids.is_some() => {
                self.marks.insert(key.to_string(), Mark { open, ..m });
            }
            _ if open == inh.open => {
                self.marks.remove(key);
            }
            _ => {
                let at = self.tick();
                self.marks.insert(key.to_string(), Mark { open, kids: None, at });
            }
        }
    }

    /// The row opened or closed the other way.
    #[cfg(test)]
    pub(crate) fn toggle<S: AsRef<str>>(&mut self, chain: &[S], key: &str) {
        let now = self.is_open(chain, key);
        self.set_open(chain, key, !now);
    }

    /// The row `key` open, wherever it is drawn — the reveal of a deep link,
    /// `f`'s filter to a ticket — where the chain is not at hand: a newer
    /// plain mark (a deep one keeps what it says below).
    pub(crate) fn force_open(&mut self, key: &str) {
        let at = self.tick();
        let kids = self.marks.get(key).and_then(|m| m.kids);
        self.marks.insert(key.to_string(), Mark { open: true, kids, at });
    }

    /// Whether `force_open` would change anything: the row has a mark that
    /// already opens it. (A row open by inheritance is not known here.)
    pub(crate) fn marked_open(&self, key: &str) -> bool {
        self.marks.get(key).is_some_and(|m| m.open)
    }

    /// One Nav ▸ item. `key` is the row the op is about — the row itself,
    /// or, for a shortcode's, the shortcode's root (its code when it has
    /// none); `chain` the keys above the row, the top first; `open` whether
    /// the row is open. The reveal is the nav's: the shape is unchanged.
    pub(crate) fn apply<S: AsRef<str>>(&mut self, op: Op, key: &str, chain: &[S], open: bool) {
        match op {
            Op::ExpandSubtree | Op::ExpandShortcode | Op::CollapseSubtree | Op::CollapseShortcode => {
                let to = matches!(op, Op::ExpandSubtree | Op::ExpandShortcode);
                let at = self.tick();
                self.marks.insert(key.to_string(), Mark { open: to, kids: Some(to), at });
            }
            Op::CollapseOthers => {
                let keep: Vec<String> = chain.iter().map(|k| k.as_ref().to_string()).chain(open.then(|| key.to_string())).collect();
                *self = Shape::default();
                for k in keep {
                    let at = self.tick();
                    self.marks.insert(k, Mark { open: true, kids: None, at });
                }
            }
            Op::ExpandAll => *self = Shape { all: true, ..Shape::default() },
            Op::CollapseAll => *self = Shape::default(),
            Op::Reveal => {}
        }
    }

    /// AGS-23's migration on a shape: a mark on a shortcode's code moves to
    /// its root's id (`root_of`), keeping what it says. `None` when nothing
    /// moves.
    pub(crate) fn rekeyed(&self, root_of: impl Fn(&str) -> Option<String>) -> Option<Shape> {
        let moves: Vec<(String, String)> = self
            .marks
            .keys()
            .filter(|k| !k.contains('-'))
            .filter_map(|k| root_of(k).map(|r| (k.clone(), r)))
            .collect();
        if moves.is_empty() {
            return None;
        }
        let mut next = self.clone();
        for (code, root) in moves {
            if let Some(m) = next.marks.remove(&code) {
                next.marks.insert(root, m);
            }
        }
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::{items, Item, Op, Place, Shape};

    const NONE: &[&str] = &[];

    /// A small tree, by chain: A-1 (a shortcode's root) › A-2 › A-3, and
    /// B-1 hosted under A-2 (drawn under A-2, after its children) › B-2.
    fn open(s: &Shape, chain: &[&str], key: &str) -> bool {
        s.is_open(chain, key)
    }

    fn every(s: &Shape) -> [bool; 5] {
        [
            open(s, NONE, "A-1"),
            open(s, &["A-1"], "A-2"),
            open(s, &["A-1", "A-2"], "A-3"),
            open(s, &["A-1", "A-2"], "B-1"),
            open(s, &["A-1", "A-2", "B-1"], "B-2"),
        ]
    }

    /// The nav opens on shortcodes only; a click opens one row, a second
    /// closes it and leaves no mark behind.
    #[test]
    fn the_default_is_closed_and_a_toggle_is_one_row() {
        let mut s = Shape::default();
        assert_eq!(every(&s), [false; 5]);
        s.toggle(NONE, "A-1");
        assert_eq!(every(&s), [true, false, false, false, false]);
        s.toggle(NONE, "A-1");
        assert_eq!(s, Shape { clock: 1, ..Shape::default() }, "no mark left");
    }

    /// Expand X's subtree: X and everything under it, hosted shortcodes
    /// included — and it overrides what was closed inside before; a row
    /// closed inside it afterwards stays closed, and only that row.
    #[test]
    fn a_subtree_opens_at_any_depth_and_the_newer_word_wins() {
        let mut s = Shape::default();
        s.set_open(&["A-1"], "A-2", false); // nothing: it is closed already
        s.toggle(NONE, "A-1");
        s.apply(Op::ExpandSubtree, "A-2", &["A-1"], false);
        assert_eq!(every(&s), [true, true, true, true, true]);
        s.toggle(&["A-1", "A-2"], "B-1");
        assert_eq!(every(&s), [true, true, true, false, true], "B-1 closed; B-2 keeps what it inherits");
        // Collapse the subtree: every row in it closes, B-1's later word
        // included (the deep mark is newer).
        s.apply(Op::CollapseSubtree, "A-2", &["A-1"], true);
        assert_eq!(every(&s), [true, false, false, false, false]);
        // Opening A-2 again by hand opens A-2 alone: its rows still inherit
        // closed.
        s.toggle(&["A-1"], "A-2");
        assert_eq!(every(&s), [true, true, false, false, false]);
    }

    /// Expand all of a shortcode is its root's subtree; Collapse all of it
    /// closes the root and everything under.
    #[test]
    fn a_shortcode_is_its_roots_subtree() {
        let mut s = Shape::default();
        s.apply(Op::ExpandShortcode, "A-1", NONE, false);
        assert_eq!(every(&s), [true; 5]);
        assert!(!open(&s, NONE, "C-1"), "another shortcode stays as it was");
        s.apply(Op::CollapseShortcode, "A-1", NONE, true);
        assert_eq!(every(&s), [false; 5]);
    }

    /// Expand all opens every row, and a row closed afterwards is closed;
    /// Collapse all is the nav's opening state.
    #[test]
    fn all_and_none() {
        let mut s = Shape::default();
        s.toggle(NONE, "A-1");
        s.apply(Op::ExpandAll, "", NONE, false);
        assert_eq!(every(&s), [true; 5]);
        assert!(open(&s, NONE, "Z-9"), "a row the nav has not drawn yet");
        assert!(s.marks.is_empty());
        s.toggle(&["A-1"], "A-2");
        assert_eq!(every(&s), [true, false, true, true, true], "closed: A-2 alone (its rows still inherit open)");
        s.apply(Op::CollapseAll, "", NONE, false);
        assert_eq!(s, Shape::default());
    }

    /// Collapse others: every row closes but the chain down to the row —
    /// the row itself as it was.
    #[test]
    fn collapse_others_keeps_the_chain() {
        let mut s = Shape::default();
        s.apply(Op::ExpandAll, "", NONE, false);
        s.apply(Op::CollapseOthers, "A-2", &["A-1"], true);
        assert_eq!(every(&s), [true, true, false, false, false]);
        assert!(!open(&s, NONE, "C-1"));
        let mut t = Shape::default();
        t.apply(Op::ExpandAll, "", NONE, false);
        t.apply(Op::CollapseOthers, "A-3", &["A-1", "A-2"], false);
        assert_eq!(every(&t), [true, true, false, false, false], "a closed row stays closed");
    }

    /// The reveal and `f` open a row without its chain at hand; a deep mark
    /// keeps what it says below.
    #[test]
    fn force_open_keeps_a_deep_mark() {
        let mut s = Shape::default();
        s.apply(Op::CollapseSubtree, "A-1", NONE, true);
        s.force_open("A-1");
        assert!(s.marked_open("A-1"));
        assert_eq!(every(&s), [true, false, false, false, false]);
        s.apply(Op::Reveal, "A-1", NONE, true);
        assert_eq!(every(&s), [true, false, false, false, false], "the reveal is the nav's");
    }

    /// AGS-12's set reads as plain marks; a shape survives its round trip;
    /// nothing, or garbage, is the default.
    #[test]
    fn the_shape_is_kept_where_the_set_was() {
        let s = Shape::load(Some(r#"["A-1","PRE"]"#));
        assert!(open(&s, NONE, "A-1") && open(&s, NONE, "PRE") && !open(&s, &["A-1"], "A-2"));
        let mut t = s.clone();
        t.apply(Op::ExpandSubtree, "A-2", &["A-1"], false);
        assert_eq!(Shape::load(Some(&t.stored())), t);
        assert_eq!(Shape::load(None), Shape::default());
        assert_eq!(Shape::load(Some("{nope")), Shape::default());
        assert_eq!(Shape::load(Some("[]")), Shape::default());
    }

    /// AGS-23's migration, on marks: a code moves to its root's id with
    /// what it says; once.
    #[test]
    fn a_code_mark_moves_to_its_root() {
        let mut s = Shape::load(Some(r#"["SUP","PRE","SUP-4"]"#));
        s.apply(Op::ExpandSubtree, "DOCS", NONE, false);
        let root = |c: &str| match c {
            "SUP" => Some("SUP-1".to_string()),
            "DOCS" => Some("DOCS-1".to_string()),
            _ => None,
        };
        let now = s.rekeyed(root).expect("moves");
        assert_eq!(now.marks.keys().collect::<Vec<_>>(), ["DOCS-1", "PRE", "SUP-1", "SUP-4"]);
        assert_eq!(now.marks["DOCS-1"].kids, Some(true));
        assert_eq!(now.rekeyed(root), None);
    }

    /// The Nav ▸ items per place: a row has all of them (its subtree's only
    /// when it has children); the empty space those that need no row; the
    /// flat list only the reveal; no open ticket, no reveal.
    #[test]
    fn the_items_per_place() {
        let ops = |v: Vec<Item>| {
            v.into_iter()
                .map(|i| match i {
                    Item::Do(op, _) => op.name(),
                    Item::Rule => "--",
                })
                .collect::<Vec<_>>()
        };
        let row = Place::Row { label: "THEME-4".into(), has_kids: true, shortcode: Some("THEME".into()) };
        assert_eq!(
            ops(items(&row, true)),
            [
                "expand-subtree",
                "collapse-subtree",
                "expand-shortcode",
                "collapse-shortcode",
                "collapse-others",
                "--",
                "expand-all",
                "collapse-all",
                "--",
                "reveal"
            ]
        );
        let words: Vec<String> = items(&row, false).into_iter().filter_map(|i| if let Item::Do(_, w) = i { Some(w) } else { None }).collect();
        assert_eq!(words[..4], ["Expand THEME-4's subtree", "Collapse THEME-4's subtree", "Expand all of THEME", "Collapse all of THEME"]);
        let leaf = Place::Row { label: "THEME-9".into(), has_kids: false, shortcode: Some("THEME".into()) };
        assert_eq!(ops(items(&leaf, false))[..2], ["expand-shortcode", "collapse-shortcode"]);
        assert_eq!(ops(items(&Place::Space, true)), ["expand-all", "collapse-all", "--", "reveal"]);
        assert_eq!(ops(items(&Place::Space, false)), ["expand-all", "collapse-all"]);
        assert_eq!(ops(items(&Place::Flat, true)), ["reveal"]);
        assert!(items(&Place::Flat, false).is_empty());
    }
}
