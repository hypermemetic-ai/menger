use super::{OrderKey, Ticket};
use std::collections::{BTreeMap, HashMap};
use std::ops::Bound;

pub(super) struct Children {
    /// Parent id → its children. Keyed by the id a ticket's `parent`
    /// names, whether or not that id is a ticket — `list { parent }` has
    /// always answered by the field, and a hand-edited database can name
    /// a parent that does not exist (or not yet).
    of: HashMap<String, Kids>,
}

/// One parent's children, in the store's order: per shortcode (in
/// shortcode order), the sequences of its tickets that hang here,
/// ascending. A shortcode root's children are almost all of one shortcode
/// — its own (MNG-14) — so this is one vector of `u32`s and a few short
/// ones beside it (sub-shortcode roots it hosts): four bytes a child,
/// where AGS-19's list on the row was an id string a child, on the row,
/// in the database and in every event about the parent.
#[derive(Default)]
struct Kids {
    by_epic: BTreeMap<String, Vec<u32>>,
    /// Every vector's length, summed — the parent's `child_count`.
    count: u64,
}

impl Kids {
    /// The vectors whose shortcode can hold a key inside `(start, end)`,
    /// each cut to the stretch of it that does: the epic bounds are the
    /// keys' (a key bound excluded still admits the rest of its
    /// shortcode), the cut inside a vector is a binary search.
    fn cut<'a>(
        &'a self,
        start: Bound<&'a OrderKey>,
        end: Bound<&'a OrderKey>,
    ) -> impl Iterator<Item = (&'a str, &'a [u32])> + 'a {
        use Bound::{Excluded, Included, Unbounded};
        let epic = |b: Bound<&'a OrderKey>| match b {
            Unbounded => Unbounded,
            Included(k) | Excluded(k) => Included(k.epic.as_str()),
        };
        // A span never ends before it starts (`Span::of`); were one asked
        // for, it holds nothing — never a panic under the read lock.
        let inverted = matches!(
            (start, end),
            (Included(a) | Excluded(a), Included(b) | Excluded(b)) if a.epic > b.epic
        );
        let epics = (!inverted).then(|| self.by_epic.range::<str, _>((epic(start), epic(end))));
        epics.into_iter().flatten().map(move |(e, seqs)| {
                let from = match start {
                    Included(k) if k.epic == *e => seqs.partition_point(|s| *s < k.seq),
                    Excluded(k) if k.epic == *e => seqs.partition_point(|s| *s <= k.seq),
                    _ => 0,
                };
                let to = match end {
                    Included(k) if k.epic == *e => seqs.partition_point(|s| *s <= k.seq),
                    Excluded(k) if k.epic == *e => seqs.partition_point(|s| *s < k.seq),
                    _ => seqs.len(),
                };
                (e.as_str(), &seqs[from.min(to)..to])
        })
    }

    fn add(&mut self, key: &OrderKey) {
        let seqs = self.by_epic.entry(key.epic.clone()).or_default();
        // A newborn is its shortcode's highest sequence, so this is a push
        // but for a child moved here by `set_parent`.
        let at = seqs.partition_point(|s| *s < key.seq);
        if seqs.get(at) != Some(&key.seq) {
            seqs.insert(at, key.seq);
            self.count += 1;
        }
    }

    fn remove(&mut self, key: &OrderKey) {
        let Some(seqs) = self.by_epic.get_mut(&key.epic) else {
            return;
        };
        if let Ok(at) = seqs.binary_search(&key.seq) {
            seqs.remove(at);
            self.count -= 1;
            if seqs.is_empty() {
                self.by_epic.remove(&key.epic);
            }
        }
    }
}

impl Children {
    /// From the tickets, in the store's order — at load (after every
    /// in-place migration) and after a seed. A walk of what is already
    /// sorted, so each vector is built by pushing: O(N), once, and a run
    /// of siblings (every ticket of a shortcode, under its root) costs one
    /// lookup of its parent, not one each.
    pub(super) fn build<'a>(tickets: impl Iterator<Item = &'a Ticket>) -> Self {
        let mut of: HashMap<String, Kids> = HashMap::new();
        let mut run: Option<(&str, &str, Vec<u32>)> = None;
        let flush = |of: &mut HashMap<String, Kids>, (parent, epic, seqs): (&str, &str, Vec<u32>)| {
            let kids = of.entry(parent.to_string()).or_default();
            kids.count += seqs.len() as u64;
            kids.by_epic.entry(epic.to_string()).or_default().extend(seqs);
        };
        for t in tickets {
            let Some(parent) = t.parent.as_deref() else {
                continue;
            };
            match &mut run {
                Some((p, e, seqs)) if *p == parent && *e == t.epic => seqs.push(t.seq),
                _ => {
                    if let Some(done) = run.take() {
                        flush(&mut of, done);
                    }
                    run = Some((parent, &t.epic, vec![t.seq]));
                }
            }
        }
        if let Some(done) = run {
            flush(&mut of, done);
        }
        Self { of }
    }

    /// How many tickets name `parent` — its `child_count`. A lookup.
    pub(super) fn count(&self, parent: &str) -> u64 {
        self.of.get(parent).map_or(0, |kids| kids.count)
    }

    /// Every parent and its count — what a build hands the rows.
    pub(super) fn counts(&self) -> impl Iterator<Item = (&str, u64)> {
        self.of.iter().map(|(p, kids)| (p.as_str(), kids.count))
    }

    /// The sequences of `parent`'s children in shortcode `epic`, ascending
    /// — a subtree walk's step, which never leaves the shortcode.
    pub(super) fn in_shortcode(&self, parent: &str, epic: &str) -> &[u32] {
        self.of
            .get(parent)
            .and_then(|kids| kids.by_epic.get(epic))
            .map_or(&[], Vec::as_slice)
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
            .flat_map(|(epic, seqs)| {
                seqs.iter().map(move |&seq| OrderKey {
                    epic: epic.to_string(),
                    seq,
                })
            })
    }

    /// How many of `parent`'s children lie inside `(start, end)` — a
    /// binary search per shortcode the parent's children are in, never a
    /// walk of the children. What makes `remaining` on a page of children
    /// cost the page.
    pub(super) fn len(&self, parent: &str, start: Bound<&OrderKey>, end: Bound<&OrderKey>) -> u64 {
        self.of.get(parent).map_or(0, |kids| {
            kids.cut(start, end).map(|(_, seqs)| seqs.len() as u64).sum()
        })
    }

    /// `key` now names `parent` — a birth, or the arriving half of a
    /// `set_parent`. Returns the parent's new count.
    pub(super) fn add(&mut self, parent: &str, key: &OrderKey) -> u64 {
        let kids = self.of.entry(parent.to_string()).or_default();
        kids.add(key);
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
    pub(super) fn all(&self) -> BTreeMap<String, Vec<(String, u32)>> {
        self.of
            .iter()
            .map(|(p, kids)| {
                let keys = kids
                    .by_epic
                    .iter()
                    .flat_map(|(e, seqs)| seqs.iter().map(move |s| (e.clone(), *s)))
                    .collect();
                (p.clone(), keys)
            })
            .collect()
    }
}
