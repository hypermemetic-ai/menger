use super::{OrderKey, Row};
use roaring::RoaringBitmap;
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Bound;
use std::sync::Arc;

/// FILTER-13: a parent's children in one shortcode are a sorted vector up to
/// this many, and a Roaring bitmap past it — one that keeps its holders
/// ([`Many::holders`]). A vector is four bytes a child and no more; a bitmap
/// costs a few hundred bytes before its first member, and then two bytes or
/// one bit a child — so the many small parents of a nested tree stay
/// vectors, and a shortcode root's 163,530 children are a bitmap of 20 KB.
///
/// The unit tests take 4, so that every walk crosses it both ways.
pub(super) const HOLD_OVER: usize = if cfg!(test) { 4 } else { 256 };

pub(super) struct Children {
    /// Parent id → its children. Keyed by the id a ticket's `parent`
    /// names, whether or not that id is a ticket — `list { parent }` has
    /// always answered by the field, and a hand-edited database can name
    /// a parent that does not exist (or not yet).
    of: HashMap<String, Kids>,
    /// FILTER-13: every shortcode a child is in, interned once — what a
    /// list is keyed by, and what a tally's cell names its shortcode by.
    names: Names,
}

/// FILTER-13: the shortcodes, interned — a name once, an index into it
/// wherever a cell of the kept tree names one. Only grows.
#[derive(Default)]
pub(super) struct Names {
    all: Vec<Arc<str>>,
    ix: HashMap<Arc<str>, u16>,
}

impl Names {
    /// `epic`'s index, interning it on first sight.
    pub(super) fn intern(&mut self, epic: &str) -> u16 {
        if let Some(i) = self.ix.get(epic) {
            return *i;
        }
        let i = u16::try_from(self.all.len()).expect("fewer than 65,536 shortcodes");
        let name: Arc<str> = Arc::from(epic);
        self.all.push(Arc::clone(&name));
        self.ix.insert(name, i);
        i
    }

    /// `epic`'s index, when it has one.
    pub(super) fn get(&self, epic: &str) -> Option<u16> {
        self.ix.get(epic).copied()
    }

    /// The shortcode at `i`.
    pub(super) fn name(&self, i: u16) -> &str {
        &self.all[i as usize]
    }

    /// The shared name of `epic`, interning it.
    fn shared(&mut self, epic: &str) -> Arc<str> {
        let i = self.intern(epic);
        Arc::clone(&self.all[i as usize])
    }

    /// How many there are.
    pub(super) fn len(&self) -> usize {
        self.all.len()
    }
}

/// One parent's children in one shortcode, ascending: a sorted vector, or —
/// past [`HOLD_OVER`] — a bitmap with its holders.
pub(super) enum Seqs {
    Few(Vec<u32>),
    Many(Box<Many>),
}

/// FILTER-13: a list of more than [`HOLD_OVER`] children, and its holders
/// H(P, g): for each cell `g` (a shortcode and a cell, [`super::tree`]) some
/// child's subtree holds below it, the children whose tally holds it — so
/// "which of P's children hold a match" is a union of the holders of the
/// cells a filter admits, and no child is looked at.
#[derive(Default)]
pub(super) struct Many {
    pub(super) seqs: RoaringBitmap,
    /// By cell, ascending; a cell no child's tally holds has no entry.
    pub(super) holders: Vec<(u32, RoaringBitmap)>,
}

impl Seqs {
    pub(super) fn len(&self) -> u64 {
        match self {
            Self::Few(v) => v.len() as u64,
            Self::Many(m) => m.seqs.len(),
        }
    }

    /// The bitmap of these sequences — the list's own for a bitmap, built
    /// for a vector (at most [`HOLD_OVER`] pushes).
    pub(super) fn bitmap(&self) -> Cow<'_, RoaringBitmap> {
        match self {
            Self::Few(v) => Cow::Owned(RoaringBitmap::from_sorted_iter(v.iter().copied()).expect("children are sorted")),
            Self::Many(m) => Cow::Borrowed(&m.seqs),
        }
    }

    /// The holders, when the list keeps them.
    pub(super) fn many(&self) -> Option<&Many> {
        match self {
            Self::Few(_) => None,
            Self::Many(m) => Some(m),
        }
    }

    pub(super) fn many_mut(&mut self) -> Option<&mut Many> {
        match self {
            Self::Few(_) => None,
            Self::Many(m) => Some(m),
        }
    }

    /// Every sequence, ascending.
    pub(super) fn iter(&self) -> Box<dyn Iterator<Item = u32> + '_> {
        match self {
            Self::Few(v) => Box::new(v.iter().copied()),
            Self::Many(m) => Box::new(m.seqs.iter()),
        }
    }

    /// The sequences in `from..=to`, ascending.
    fn range(&self, from: u32, to: u32) -> Box<dyn Iterator<Item = u32> + '_> {
        match self {
            Self::Few(v) => {
                let a = v.partition_point(|s| *s < from);
                let b = v.partition_point(|s| *s <= to);
                Box::new(v[a.min(b)..b].iter().copied())
            }
            Self::Many(m) => Box::new(m.seqs.range(from..=to)),
        }
    }

    /// How many lie in `from..=to` — a binary search, or two ranks.
    fn count(&self, from: u32, to: u32) -> u64 {
        match self {
            Self::Few(v) => {
                let a = v.partition_point(|s| *s < from);
                let b = v.partition_point(|s| *s <= to);
                b.saturating_sub(a) as u64
            }
            Self::Many(m) => {
                let below = from.checked_sub(1).map_or(0, |f| m.seqs.rank(f));
                m.seqs.rank(to) - below
            }
        }
    }

    /// Adds `seq`; whether it was new. A vector past [`HOLD_OVER`] becomes a
    /// bitmap with no holders yet — the write that grew it brings them
    /// ([`super::tree`]).
    fn insert(&mut self, seq: u32) -> bool {
        let added = match self {
            Self::Few(v) => match v.binary_search(&seq) {
                Ok(_) => false,
                Err(at) => {
                    v.insert(at, seq);
                    true
                }
            },
            Self::Many(m) => m.seqs.insert(seq),
        };
        self.settle();
        added
    }

    /// Appends `seq`, which is above every one held (a build in the store's
    /// order), or inserts it.
    fn push(&mut self, seq: u32) {
        let pushed = match self {
            Self::Few(v) if v.last().is_none_or(|l| *l < seq) => {
                v.push(seq);
                true
            }
            Self::Few(_) => false,
            Self::Many(m) => m.seqs.push(seq),
        };
        if pushed {
            self.settle();
        } else {
            self.insert(seq);
        }
    }

    /// Takes `seq` out; whether it was there. A bitmap back at
    /// [`HOLD_OVER`] becomes a vector, its holders dropped.
    fn remove(&mut self, seq: u32) -> bool {
        let removed = match self {
            Self::Few(v) => match v.binary_search(&seq) {
                Ok(at) => {
                    v.remove(at);
                    true
                }
                Err(_) => false,
            },
            Self::Many(m) => m.seqs.remove(seq),
        };
        self.settle();
        removed
    }

    /// The representation its length calls for.
    fn settle(&mut self) {
        let n = self.len() as usize;
        match self {
            Self::Few(v) if n > HOLD_OVER => {
                let seqs = RoaringBitmap::from_sorted_iter(v.iter().copied()).expect("children are sorted");
                *self = Self::Many(Box::new(Many { seqs, holders: Vec::new() }));
            }
            Self::Many(m) if n <= HOLD_OVER => {
                let v: Vec<u32> = m.seqs.iter().collect();
                *self = Self::Few(v);
            }
            _ => {}
        }
    }

    /// The same sequences, whatever the representation.
    fn same(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

/// One parent's children, in the store's order: per shortcode (in
/// shortcode order), the sequences of its tickets that hang here,
/// ascending. A shortcode root's children are almost all of one shortcode
/// — its own (MNG-14) — so this is one list and a few short ones beside it
/// (sub-shortcode roots it hosts). FILTER-13: beside them, the parent's
/// descendants tallied by cell ([`super::tree`]) — kept here, where the
/// parent already has its one entry, so the kept tree costs no second
/// index of the internal tickets.
#[derive(Default)]
pub(super) struct Kids {
    lists: Vec<(Arc<str>, Seqs)>,
    /// Every list's length, summed — the parent's `child_count`.
    count: u64,
    /// FILTER-13: T(P) — how many of the parent's descendants are in each
    /// cell `g`, by `g` ascending, none 0.
    pub(super) tally: Vec<(u32, u32)>,
}

impl Kids {
    /// The lists whose shortcode can hold a key inside `(start, end)`, each
    /// with the stretch of sequences that can, inclusive: the epic bounds
    /// are the keys' (a key bound excluded still admits the rest of its
    /// shortcode).
    fn cut<'a>(&'a self, start: Bound<&'a OrderKey>, end: Bound<&'a OrderKey>) -> impl Iterator<Item = (&'a str, &'a Seqs, u32, u32)> + 'a {
        use Bound::{Excluded, Included, Unbounded};
        // A span never ends before it starts (`Span::of`); were one asked
        // for, it holds nothing — never a panic under the read lock.
        let inverted = matches!(
            (start, end),
            (Included(a) | Excluded(a), Included(b) | Excluded(b)) if a.epic > b.epic
        );
        let (lo, hi) = (
            match start {
                Unbounded => None,
                Included(k) | Excluded(k) => Some(k.epic.as_str()),
            },
            match end {
                Unbounded => None,
                Included(k) | Excluded(k) => Some(k.epic.as_str()),
            },
        );
        let lists = if inverted { &self.lists[..0] } else { &self.lists[..] };
        lists.iter().filter_map(move |(e, seqs)| {
            let e: &str = e;
            if lo.is_some_and(|lo| e < lo) || hi.is_some_and(|hi| e > hi) {
                return None;
            }
            let from = match start {
                Included(k) if k.epic == e => Some(k.seq),
                Excluded(k) if k.epic == e => k.seq.checked_add(1),
                _ => Some(0),
            }?;
            let to = match end {
                Included(k) if k.epic == e => Some(k.seq),
                Excluded(k) if k.epic == e => k.seq.checked_sub(1),
                _ => Some(u32::MAX),
            }?;
            (from <= to).then_some((e, seqs, from, to))
        })
    }

    fn at(&self, epic: &str) -> Result<usize, usize> {
        self.lists.binary_search_by(|(e, _)| (**e).cmp(epic))
    }

    /// The children in `epic`.
    pub(super) fn list(&self, epic: &str) -> Option<&Seqs> {
        self.at(epic).ok().map(|i| &self.lists[i].1)
    }

    pub(super) fn list_mut(&mut self, epic: &str) -> Option<&mut Seqs> {
        self.at(epic).ok().map(|i| &mut self.lists[i].1)
    }

    /// Every list, in shortcode order.
    pub(super) fn lists(&self) -> impl Iterator<Item = (&str, &Seqs)> {
        self.lists.iter().map(|(e, s)| (&**e, s))
    }

    pub(super) fn lists_mut(&mut self) -> impl Iterator<Item = (&str, &mut Seqs)> {
        self.lists.iter_mut().map(|(e, s)| (&**e, s))
    }

    fn add(&mut self, names: &mut Names, key: &OrderKey) {
        let i = match self.at(&key.epic) {
            Ok(i) => i,
            Err(i) => {
                self.lists.insert(i, (names.shared(&key.epic), Seqs::Few(Vec::new())));
                i
            }
        };
        // A newborn is its shortcode's highest sequence, so this is a push
        // but for a child moved here by `set_parent`.
        if self.lists[i].1.insert(key.seq) {
            self.count += 1;
        }
    }

    fn remove(&mut self, key: &OrderKey) {
        let Ok(i) = self.at(&key.epic) else { return };
        if self.lists[i].1.remove(key.seq) {
            self.count -= 1;
            if self.lists[i].1.len() == 0 {
                self.lists.remove(i);
            }
        }
    }

    /// The same children (the tally and holders aside, which the kept
    /// tree's own comparison reads).
    fn same_children(&self, other: &Self) -> bool {
        self.count == other.count
            && self.lists.len() == other.lists.len()
            && self.lists.iter().zip(&other.lists).all(|((a, x), (b, y))| a == b && x.same(y))
    }
}

impl Children {
    /// From the tickets, in the store's order — at load (after every
    /// in-place migration) and after a seed. A walk of what is already
    /// sorted, so each list is built by pushing: O(N), once, and a run
    /// of siblings (every ticket of a shortcode, under its root) costs one
    /// lookup of its parent, not one each.
    pub(super) fn build<'a>(tickets: impl Iterator<Item = &'a Row>) -> Self {
        let mut c = Self::filling();
        let mut run: Option<(&str, &str, Vec<u32>)> = None;
        for t in tickets {
            let Some(parent) = t.parent.as_deref() else {
                continue;
            };
            match &mut run {
                Some((p, e, seqs)) if *p == parent && *e == t.epic => seqs.push(t.seq),
                _ => {
                    if let Some((p, e, seqs)) = run.take() {
                        for s in seqs {
                            c.push(p, e, s);
                        }
                    }
                    run = Some((parent, &t.epic, vec![t.seq]));
                }
            }
        }
        if let Some((p, e, seqs)) = run {
            for s in seqs {
                c.push(p, e, s);
            }
        }
        c
    }

    /// SCALE-28: an empty index to [`Self::push`] the database's own into.
    pub(super) fn filling() -> Self {
        Self {
            of: HashMap::new(),
            names: Names::default(),
        }
    }

    /// SCALE-28: one entry of the database's index on `(parent, epic, seq)`,
    /// read in that order — at start, instead of a [`Self::build`] from the
    /// tickets. In that order every parent's children arrive together, each
    /// shortcode's ascending, so a child is a push; the parent's and the
    /// shortcode's names are only copied when they change.
    pub(super) fn push(&mut self, parent: &str, epic: &str, seq: u32) {
        let kids = match self.of.get_mut(parent) {
            Some(kids) => kids,
            None => self.of.entry(parent.to_string()).or_default(),
        };
        let i = match kids.lists.last() {
            Some((e, _)) if **e == *epic => kids.lists.len() - 1,
            _ => match kids.at(epic) {
                Ok(i) => i,
                Err(i) => {
                    kids.lists.insert(i, (self.names.shared(epic), Seqs::Few(Vec::new())));
                    i
                }
            },
        };
        let before = kids.lists[i].1.len();
        kids.lists[i].1.push(seq);
        kids.count += kids.lists[i].1.len() - before;
    }

    /// How many parents have a child — for the startup log and the verdict.
    pub(super) fn parents(&self) -> usize {
        self.of.len()
    }

    /// SCALE-28: every parent whose children differ between the two, in id
    /// order — what `--verify-derived` reports of the index.
    pub(super) fn differences(&self, other: &Self) -> Vec<String> {
        let mut out: Vec<String> = self
            .of
            .iter()
            .filter(|(p, kids)| other.of.get(*p).is_none_or(|o| !kids.same_children(o)))
            .map(|(p, _)| p.clone())
            .chain(other.of.keys().filter(|p| !self.of.contains_key(*p)).cloned())
            .collect();
        out.sort();
        out
    }

    /// `parent`'s children as `(epic, seq)` keys, in the store's order.
    pub(super) fn of(&self, parent: &str) -> Vec<(String, u32)> {
        self.of.get(parent).map_or_else(Vec::new, |kids| {
            kids.lists
                .iter()
                .flat_map(|(e, seqs)| seqs.iter().map(move |s| (e.to_string(), s)))
                .collect()
        })
    }

    /// How many tickets name `parent` — its `child_count`. A lookup.
    pub(super) fn count(&self, parent: &str) -> u64 {
        self.of.get(parent).map_or(0, |kids| kids.count)
    }

    /// Every parent and its count — what a build hands the rows.
    pub(super) fn counts(&self) -> impl Iterator<Item = (&str, u64)> {
        self.of.iter().map(|(p, kids)| (p.as_str(), kids.count))
    }

    /// FILTER-13: `parent`'s entry — its lists and its tally.
    pub(super) fn kids(&self, parent: &str) -> Option<&Kids> {
        self.of.get(parent)
    }

    pub(super) fn kids_mut(&mut self, parent: &str) -> Option<&mut Kids> {
        self.of.get_mut(parent)
    }

    /// FILTER-13: every parent and its entry.
    pub(super) fn every(&self) -> impl Iterator<Item = (&str, &Kids)> {
        self.of.iter().map(|(p, k)| (p.as_str(), k))
    }

    pub(super) fn every_mut(&mut self) -> impl Iterator<Item = (&str, &mut Kids)> {
        self.of.iter_mut().map(|(p, k)| (p.as_str(), k))
    }

    /// FILTER-13: the shortcodes, interned.
    pub(super) fn names(&self) -> &Names {
        &self.names
    }

    pub(super) fn names_mut(&mut self) -> &mut Names {
        &mut self.names
    }

    /// The sequences of `parent`'s children in shortcode `epic`, ascending
    /// — a subtree walk's step, which never leaves the shortcode.
    pub(super) fn in_shortcode(&self, parent: &str, epic: &str) -> Option<&Seqs> {
        self.of.get(parent).and_then(|kids| kids.list(epic))
    }

    /// `parent`'s children inside `(start, end)`, in the store's order —
    /// what a page of `list { parent }` walks: a binary search to the
    /// cursor, then the page.
    pub(super) fn range<'a>(
        &'a self,
        parent: &str,
        start: Bound<&'a OrderKey>,
        end: Bound<&'a OrderKey>,
    ) -> impl Iterator<Item = OrderKey> + 'a {
        self.of
            .get(parent)
            .into_iter()
            .flat_map(move |kids| kids.cut(start, end))
            .flat_map(|(epic, seqs, from, to)| {
                seqs.range(from, to).map(move |seq| OrderKey {
                    epic: epic.to_string(),
                    seq,
                })
            })
    }

    /// How many of `parent`'s children lie inside `(start, end)` — a
    /// binary search (or two ranks) per shortcode the parent's children are
    /// in, never a walk of the children. What makes `remaining` on a page
    /// of children cost the page.
    pub(super) fn len(&self, parent: &str, start: Bound<&OrderKey>, end: Bound<&OrderKey>) -> u64 {
        self.of.get(parent).map_or(0, |kids| {
            kids.cut(start, end).map(|(_, seqs, from, to)| seqs.count(from, to)).sum()
        })
    }

    /// `key` now names `parent` — a birth, or the arriving half of a
    /// `set_parent`. Returns the parent's new count.
    pub(super) fn add(&mut self, parent: &str, key: &OrderKey) -> u64 {
        let kids = match self.of.get_mut(parent) {
            Some(kids) => kids,
            None => self.of.entry(parent.to_string()).or_default(),
        };
        kids.add(&mut self.names, key);
        kids.count
    }

    /// `key` no longer names `parent` — the leaving half of a
    /// `set_parent`. Returns the parent's new count.
    pub(super) fn remove(&mut self, parent: &str, key: &OrderKey) -> u64 {
        let Some(kids) = self.of.get_mut(parent) else {
            return 0;
        };
        kids.remove(key);
        let count = kids.count;
        if count == 0 {
            self.of.remove(parent);
        }
        count
    }

    /// Every parent and its children as `(epic, seq)` keys, in the store's
    /// order, sorted by parent — for the tests' comparison with a scan.
    #[cfg(test)]
    pub(super) fn all(&self) -> std::collections::BTreeMap<String, Vec<(String, u32)>> {
        self.of.keys().map(|p| (p.clone(), self.of(p))).collect()
    }
}
