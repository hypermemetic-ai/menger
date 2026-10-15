//! FILTER-13: the kept tree — what a filter asks of a subtree without
//! walking it (FILTER-3 §6.5).
//!
//! Three things, kept per parent in the children index's own entry
//! ([`Children`], [`Kids`]), so the internal tickets are indexed once:
//!
//! - **C(P)**, the children — a sorted vector a shortcode, or past
//!   [`HOLD_OVER`] a Roaring bitmap ([`Seqs`]);
//! - **T(P)**, the tally: how many of P's descendants are in each *tree
//!   cell* `g` — a shortcode and a cell (FILTER-4's), packed as one `u32`
//!   ([`g`]). The shortcode is in the key because a filter's `in` and
//!   `under` read it, and a subtree crosses shortcodes where it hosts one;
//! - **H(P, g)**, the holders: for a list kept as a bitmap, the children
//!   whose own tally holds `g` ([`Many::holders`]). A vector's are its
//!   children's tallies, read when asked (at most [`HOLD_OVER`] of them).
//!
//! "Descendant" is the lineage's (FILTER-10's `under`): a ticket's strict
//! ancestors are its parent, that one's parent, … — each once, the chain
//! ending where it meets one already met or the ticket itself (a
//! hand-edited cycle), or an id that names no ticket ([`ancestors`]).
//!
//! The SCALE-32 shape: what a write does to them is worked out before it
//! commits ([`Plan`] → [`Move`]): a birth counts its cell (and, where a
//! hand edit had named its id as a parent, its subtree's tally) in each
//! ancestor; a cell move moves one count in each ancestor of each ticket it
//! moves, 2d entries; a `set_parent` takes the moved subtree's tally — its
//! own cell plus its tally — off the old chain and puts it on the new one.
//! No write walks a subtree's tickets. A tally that crosses 0 flips one
//! holder bit in its parent's list; a list that crosses [`HOLD_OVER`] has
//! its holders built from its children's tallies, or dropped. Stored by the
//! write's own transaction ([`Move::store`]: form 5's `tree_tallies` and
//! `tree_holders`), taken by the cache after ([`take`]), read at start
//! ([`super::stored`]), and compared with a scan ([`build`]) by
//! `--verify-derived`. A `set_parent` that breaks a hand-edited cycle builds
//! the whole tree again, as the roll-ups do.

use crate::address::ShortcodeId;
use super::children::{Children, Seqs, HOLD_OVER};
use super::stored::Changes;
use super::{OrderKey, Row, Rows};
use roaring::RoaringBitmap;
use std::collections::{BTreeMap, HashMap};

/// A tree cell: a shortcode's index ([`super::children::Names`]) and a
/// cell id, packed — the shortcode in the high 16 bits.
pub(super) fn g(ix: u16, cell: u32) -> u32 {
    debug_assert!(cell <= 0xFFFF, "a cell id fits 16 bits");
    (u32::from(ix) << 16) | cell
}

/// The shortcode index of a tree cell.
pub(super) fn g_epic(g: u32) -> u16 {
    (g >> 16) as u16
}

/// The cell id of a tree cell.
pub(super) fn g_cell(g: u32) -> u32 {
    g & 0xFFFF
}

/// `g`'s count in a tally (sorted by `g`), 0 when absent.
pub(super) fn tally_get(tally: &[(u32, u32)], g: u32) -> u32 {
    tally.binary_search_by_key(&g, |(x, _)| *x).map_or(0, |i| tally[i].1)
}

/// Sets `g`'s count in a tally, keeping it sorted and its 0s out, and
/// growing it one entry at a time (a tally gains a cell rarely).
fn tally_set(tally: &mut Vec<(u32, u32)>, g: u32, n: u32) {
    match tally.binary_search_by_key(&g, |(x, _)| *x) {
        Ok(i) if n == 0 => {
            tally.remove(i);
        }
        Ok(i) => tally[i].1 = n,
        Err(_) if n == 0 => {}
        Err(i) => {
            tally.reserve_exact(1);
            tally.insert(i, (g, n));
        }
    }
}

/// The strict ancestors of the ticket `id`, whose parent is `parent`: that
/// parent, its parent, and so on — each once. The chain ends at an id that
/// names no ticket (it is the last), at a ticket with no parent, or where
/// it would meet one already met or `id` itself (a hand-edited cycle).
pub(super) fn ancestors<'a>(rows: Rows<'a>, id: &str, parent: Option<&'a str>) -> Vec<&'a str> {
    let mut out: Vec<&'a str> = Vec::new();
    let mut cur = parent;
    while let Some(p) = cur {
        if p == id || out.contains(&p) {
            break;
        }
        out.push(p);
        cur = rows.get(p).and_then(|r| r.parent.as_deref());
    }
    out
}

/// Whether the ticket `id`, whose parent is `parent`, is on a parent cycle
/// — its chain comes back to it (only a hand edit makes one).
pub(super) fn on_cycle(rows: Rows<'_>, id: &str, parent: Option<&str>) -> bool {
    let mut seen: Vec<&str> = Vec::new();
    let mut cur = parent;
    while let Some(p) = cur {
        if p == id {
            return true;
        }
        if seen.contains(&p) {
            return false;
        }
        seen.push(p);
        cur = rows.get(p).and_then(|r| r.parent.as_deref());
    }
    false
}

/// Every tally and every holder, from the rows and the children index as
/// they stand — a build's scan, a seed's, and what `--verify-derived`
/// compares the stored form with. `cell_of` is a row's cell id.
///
/// Each parent's children are counted by cell once (a pass over the rows),
/// and that count is added to the parent's tally and to each of its strict
/// ancestors': a ticket's cell lands in every ancestor of it, which is the
/// definition, at a cost of the parents times their depth. A child of P on
/// a cycle through P ends P's chain ([`ancestors`]) and is not its own
/// descendant: its own cell is taken back out of its tally.
pub(super) fn build(rows: Rows<'_>, children: &mut Children, cell_of: impl Fn(&Row) -> u32) {
    let mut direct: HashMap<&str, HashMap<u32, u32>> = HashMap::new();
    for t in rows.tickets() {
        let Some(p) = t.parent.as_deref() else { continue };
        let gg = g(children.names_mut().intern(&t.shortcode), cell_of(t));
        *direct.entry(p).or_default().entry(gg).or_insert(0) += 1;
    }
    let mut tally: HashMap<&str, HashMap<u32, u32>> = HashMap::new();
    for (p, d) in &direct {
        let chain = ancestors(rows, p, rows.get(p).and_then(|r| r.parent.as_deref()));
        for a in std::iter::once(*p).chain(chain.iter().copied()) {
            let t = tally.entry(a).or_default();
            for (gg, n) in d {
                *t.entry(*gg).or_insert(0) += n;
            }
        }
        if let Some(lr) = chain.last().and_then(|l| rows.get(l)).filter(|r| r.parent.as_deref() == Some(*p)) {
            let last = lr.id.as_str();
            let gg = g(children.names_mut().intern(&lr.shortcode), cell_of(lr));
            let n = tally.get_mut(last).and_then(|t| t.get_mut(&gg)).expect("counted above");
            *n -= 1;
        }
    }
    for (a, t) in tally {
        let kids = children.kids_mut(a).expect("an ancestor has a child");
        let mut v: Vec<(u32, u32)> = t.into_iter().filter(|(_, n)| *n > 0).collect();
        v.sort_unstable();
        v.shrink_to_fit();
        kids.tally = v;
    }
    // H(P, g): for each internal ticket in a list kept as a bitmap, its bit
    // in each cell its tally holds.
    let mut holders: HashMap<(&str, &ShortcodeId), BTreeMap<u32, RoaringBitmap>> = HashMap::new();
    for (c, kids) in children.every() {
        let Some(row) = rows.get(c) else { continue };
        let Some(p) = row.parent.as_deref() else { continue };
        if children.in_shortcode(p, &row.shortcode).and_then(Seqs::many).is_none() {
            continue;
        }
        let h = holders.entry((p, &row.shortcode)).or_default();
        for (gg, _) in &kids.tally {
            h.entry(*gg).or_default().insert(row.seq);
        }
    }
    for ((p, e), h) in holders {
        let many = children
            .kids_mut(p)
            .and_then(|k| k.list_mut(e))
            .and_then(Seqs::many_mut)
            .expect("a list kept as a bitmap");
        many.holders = h.into_iter().collect();
    }
}

/// One write's change to the kept tree, worked out before it commits.
#[derive(Default)]
pub(super) struct Move {
    /// Each tally entry it changes, as it leaves it: (parent, shortcode,
    /// cell, count) — 0: gone.
    tallies: Vec<(String, ShortcodeId, u32, u32)>,
    /// Each holder bitmap it changes in a list that keeps them across the
    /// write: (parent, the list's shortcode, the cell's shortcode, cell),
    /// who leaves it and who joins it.
    flips: Vec<(String, ShortcodeId, ShortcodeId, u32, RoaringBitmap, RoaringBitmap)>,
    /// Each list that becomes a bitmap: its holders, whole.
    built: Vec<(String, ShortcodeId, Vec<(ShortcodeId, u32, RoaringBitmap)>)>,
    /// Each list whose stored holders all go (it becomes a vector, or is
    /// built afresh), before any is written.
    cleared: Vec<(String, ShortcodeId)>,
    /// Each stored holder container it rewrites, as it is after: `None`
    /// when left empty.
    holder_rows: Vec<(HolderKey, Option<(u64, Vec<u8>)>)>,
    /// A move off a hand-edited cycle: the whole kept tree, built again from
    /// the rows as the write leaves them.
    rebuilt: Option<Box<Children>>,
}

/// A stored holder container: (parent, the list's shortcode, the cell's
/// shortcode, cell, block).
pub(super) type HolderKey = (String, ShortcodeId, ShortcodeId, u32, u32);

/// The stored form of the kept tree.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Stored {
    /// (parent, shortcode, cell) → count.
    pub(super) tallies: BTreeMap<(String, ShortcodeId, u32), u32>,
    /// Each holder container → (card, bits).
    pub(super) holders: BTreeMap<HolderKey, (u64, Vec<u8>)>,
}

/// The kept tree as stored — every tally entry and every holder container.
pub(super) fn stored(children: &Children) -> Stored {
    let names = children.names();
    let mut s = Stored::default();
    for (p, kids) in children.every() {
        for (gg, n) in &kids.tally {
            s.tallies.insert((p.to_string(), names.name(g_epic(*gg)).clone(), g_cell(*gg)), *n);
        }
        for (e, seqs) in kids.lists() {
            let Some(many) = seqs.many() else { continue };
            for (gg, bm) in &many.holders {
                for (hi, card, bits) in super::cells::containers(bm) {
                    s.holders.insert(
                        (p.to_string(), e.clone(), names.name(g_epic(*gg)).clone(), g_cell(*gg), hi),
                        (card, bits),
                    );
                }
            }
        }
    }
    s
}

impl Move {
    /// The rows the write's own transaction writes for it.
    pub(super) fn store(&self, into: &mut Changes) {
        if let Some(rebuilt) = &self.rebuilt {
            into.tree_all = Some(stored(rebuilt));
            return;
        }
        into.tree_cleared.extend(self.cleared.iter().cloned());
        into.tree_holders.extend(self.holder_rows.iter().cloned());
        into.tree_tallies
            .extend(self.tallies.iter().map(|(p, e, c, n)| ((p.clone(), e.clone(), *c), *n)));
    }

    /// The kept tree built again — a move off a hand-edited cycle.
    pub(super) fn rebuilt(children: Children) -> Self {
        Self {
            rebuilt: Some(Box::new(children)),
            ..Self::default()
        }
    }

    /// How many tally entries it moves — for the tests and the logs.
    #[cfg(test)]
    pub(super) fn tallies_moved(&self) -> usize {
        self.tallies.len()
    }
}

/// …and once the write has committed (its children already moved in the
/// index), the cache takes it.
pub(super) fn take(children: &mut Children, m: Move) {
    if let Some(rebuilt) = m.rebuilt {
        *children = *rebuilt;
        return;
    }
    let mut ix: HashMap<ShortcodeId, u16> = HashMap::new();
    {
        let names = children.names_mut();
        let all = m
            .tallies
            .iter()
            .map(|(_, e, _, _)| e)
            .chain(m.flips.iter().map(|(_, _, e, _, _, _)| e))
            .chain(m.built.iter().flat_map(|(_, _, h)| h.iter().map(|(e, _, _)| e)));
        for e in all {
            if !ix.contains_key(e) {
                ix.insert(e.clone(), names.intern(e));
            }
        }
    }
    for (a, e, cell, n) in m.tallies {
        if let Some(kids) = children.kids_mut(&a) {
            tally_set(&mut kids.tally, g(ix[&e], cell), n);
        }
    }
    for (p, e, h) in m.built {
        let mut holders: Vec<(u32, RoaringBitmap)> = h.into_iter().map(|(ge, c, bm)| (g(ix[&ge], c), bm)).collect();
        holders.sort_by_key(|(gg, _)| *gg);
        if let Some(many) = children.kids_mut(&p).and_then(|k| k.list_mut(&e)).and_then(Seqs::many_mut) {
            many.holders = holders;
        } else {
            debug_assert!(false, "{p}'s list in {e} became a bitmap with the write");
        }
    }
    for (p, e, ge, cell, leave, join) in m.flips {
        let Some(many) = children.kids_mut(&p).and_then(|k| k.list_mut(&e)).and_then(Seqs::many_mut) else {
            debug_assert!(false, "{p}'s list in {e} keeps its holders across the write");
            continue;
        };
        let gg = g(ix[&ge], cell);
        let at = match many.holders.binary_search_by_key(&gg, |(x, _)| *x) {
            Ok(i) => i,
            Err(i) => {
                many.holders.insert(i, (gg, RoaringBitmap::new()));
                i
            }
        };
        let bm = &mut many.holders[at].1;
        *bm -= &leave;
        *bm |= &join;
        if bm.is_empty() {
            many.holders.remove(at);
        }
    }
}

/// What one write does to the kept tree, gathered as the write is worked
/// out — counts moved along chains, children joining or leaving a list —
/// and then made a [`Move`] ([`Plan::finish`]) against the cache as it
/// stands, under the write-serial lock.
pub(super) struct Plan<'a> {
    children: &'a Children,
    rows: Rows<'a>,
    /// Per parent, the counts of its children that move: (child, its
    /// shortcode, cell, by). Grouped so a fan-out of siblings walks their
    /// parent's chain once.
    by_parent: HashMap<&'a str, Vec<(&'a str, &'a ShortcodeId, u32, i64)>>,
    /// Per ancestor, per (shortcode, cell): how its tally moves.
    deltas: HashMap<&'a str, BTreeMap<(&'a ShortcodeId, u32), i64>>,
    /// Children that join a parent's list, and that leave one.
    joins: Vec<(&'a str, &'a Row)>,
    leaves: Vec<(&'a str, &'a Row)>,
    /// The rows of children who join (they are not the cache's yet).
    joining: HashMap<(&'a ShortcodeId, u32), &'a Row>,
}

impl<'a> Plan<'a> {
    /// A plan against `children`, over `rows` as the write leaves them.
    pub(super) fn new(children: &'a Children, rows: Rows<'a>) -> Self {
        Self {
            children,
            rows,
            by_parent: HashMap::new(),
            deltas: HashMap::new(),
            joins: Vec::new(),
            leaves: Vec::new(),
            joining: HashMap::new(),
        }
    }

    /// `t`'s own count in `(epic, cell)` moves by `by` in each of its strict
    /// ancestors — a birth (+1), a cell move (−1 from, +1 to).
    pub(super) fn count(&mut self, t: &'a Row, cell: u32, by: i64) {
        if let Some(p) = t.parent.as_deref() {
            self.by_parent.entry(p).or_default().push((t.id.as_str(), &t.shortcode, cell, by));
        }
    }

    /// `x`'s subtree — its own cell `cell` and its tally — counted `by` in
    /// each of `chain`: a move's two halves, and a birth whose id a hand
    /// edit had already named as a parent.
    pub(super) fn subtree(&mut self, x: &'a Row, cell: Option<u32>, chain: &[&'a str], by: i64) {
        let names = self.children.names();
        let tally = self.children.kids(&x.id).map(|k| k.tally.as_slice()).unwrap_or(&[]);
        for a in chain {
            let d = self.deltas.entry(*a).or_default();
            if let Some(cell) = cell {
                *d.entry((&x.shortcode, cell)).or_insert(0) += by;
            }
            for (gg, n) in tally {
                *d.entry((names.name(g_epic(*gg)), g_cell(*gg))).or_insert(0) += by * i64::from(*n);
            }
        }
    }

    /// `child` joins `parent`'s list.
    pub(super) fn join(&mut self, parent: &'a str, child: &'a Row) {
        self.joins.push((parent, child));
        self.joining.insert((&child.shortcode, child.seq), child);
    }

    /// `child` leaves `parent`'s list.
    pub(super) fn leave(&mut self, parent: &'a str, child: &'a Row) {
        self.leaves.push((parent, child));
    }

    /// Each grouped count along its parent's chain: every ancestor of the
    /// parent, and the parent — but not the child itself, should it end
    /// the chain (a cycle through the parent).
    fn spread(&mut self) {
        let rows = self.rows;
        for (p, moved) in std::mem::take(&mut self.by_parent) {
            // The parent's children's moves summed once — a fan-out of
            // siblings is one sum, then one walk up the chain.
            let mut sum: BTreeMap<(&'a ShortcodeId, u32), i64> = BTreeMap::new();
            for (_, epic, cell, by) in &moved {
                *sum.entry((epic, *cell)).or_insert(0) += by;
            }
            let chain = ancestors(rows, p, rows.get(p).and_then(|r| r.parent.as_deref()));
            for a in std::iter::once(p).chain(chain.iter().copied()) {
                let d = self.deltas.entry(a).or_default();
                for (k, by) in &sum {
                    *d.entry(*k).or_insert(0) += by;
                }
                // A child that ends the chain (a cycle through the parent)
                // is not its own descendant.
                for (child, epic, cell, by) in moved.iter().filter(|m| m.0 == a) {
                    let _ = child;
                    *d.entry((epic, *cell)).or_insert(0) -= by;
                }
            }
        }
    }

    /// `id`'s tally as the write leaves it, by (shortcode, cell).
    fn final_tally(&self, id: &str) -> Vec<((&'a ShortcodeId, u32), u32)> {
        let names = self.children.names();
        let mut out: BTreeMap<(&'a ShortcodeId, u32), i64> = BTreeMap::new();
        if let Some(k) = self.children.kids(id) {
            for (gg, n) in &k.tally {
                out.insert((names.name(g_epic(*gg)), g_cell(*gg)), i64::from(*n));
            }
        }
        if let Some(d) = self.deltas.get(id) {
            for (k, by) in d {
                *out.entry(*k).or_insert(0) += by;
            }
        }
        out.into_iter().filter(|(_, n)| *n > 0).map(|(k, n)| (k, n as u32)).collect()
    }

    /// The row at `(epic, seq)` as the write leaves it.
    fn row_at(&self, epic: &'a ShortcodeId, seq: u32) -> Option<&'a Row> {
        if let Some(r) = self.joining.get(&(epic, seq)) {
            return Some(r);
        }
        self.rows.order.get(&OrderKey { shortcode: epic.clone(), seq })
    }

    /// The [`Move`]: each tally as it is left, and what the holders do — a
    /// bit flipped where a tally crosses 0 (in the parent's list, when it
    /// keeps holders across the write), a moved child's bits moved with
    /// it, a list that crosses [`HOLD_OVER`] built or dropped — with the
    /// stored rows of each.
    pub(super) fn finish(mut self) -> Move {
        self.spread();
        let names = self.children.names();
        let mut m = Move::default();
        let mut crossings: Vec<(&'a str, &'a ShortcodeId, u32, bool)> = Vec::new();
        let mut deltas: Vec<(&'a str, &BTreeMap<(&'a ShortcodeId, u32), i64>)> = self.deltas.iter().map(|(a, d)| (*a, d)).collect();
        deltas.sort_by_key(|(a, _)| *a);
        for (a, d) in deltas {
            let kids = self.children.kids(a);
            for ((epic, cell), by) in d {
                if *by == 0 {
                    continue;
                }
                let now = kids
                    .and_then(|k| names.get(epic).map(|ix| tally_get(&k.tally, g(ix, *cell))))
                    .map_or(0, i64::from);
                let after = now + by;
                debug_assert!(after >= 0, "{a}'s tally of ({epic}, {cell}): {now} {by:+}");
                let after = after.max(0);
                m.tallies.push((a.to_string(), (*epic).clone(), *cell, after as u32));
                if (now == 0) != (after == 0) {
                    crossings.push((a, epic, *cell, after > 0));
                }
            }
        }
        // Each list whose length moves: is it a bitmap before, and after?
        let mut lengths: BTreeMap<(&'a str, &'a ShortcodeId), i64> = BTreeMap::new();
        for (p, c) in &self.joins {
            *lengths.entry((p, &c.shortcode)).or_insert(0) += 1;
        }
        for (p, c) in &self.leaves {
            *lengths.entry((p, &c.shortcode)).or_insert(0) -= 1;
        }
        let len_of = |p: &str, e: &ShortcodeId| self.children.in_shortcode(p, e).map_or(0, |s| s.len() as i64);
        let many_before = |p: &str, e: &ShortcodeId| len_of(p, e) as usize > HOLD_OVER;
        let many_after = |p: &str, e: &ShortcodeId| (len_of(p, e) + lengths.get(&(p, e)).copied().unwrap_or(0)) as usize > HOLD_OVER;
        let keeps = |p: &str, e: &ShortcodeId| many_before(p, e) && many_after(p, e);
        // Lists that become bitmaps: their holders from their children's
        // tallies as the write leaves them. Lists that become vectors: their
        // stored holders go.
        for ((p, e), _) in lengths.iter().filter(|(_, by)| **by != 0) {
            match (many_before(p, e), many_after(p, e)) {
                (false, true) => {
                    let mut members: Vec<u32> = self.children.in_shortcode(p, e).map(|s| s.iter().collect()).unwrap_or_default();
                    members.extend(self.joins.iter().filter(|(q, c)| q == p && c.shortcode == **e).map(|(_, c)| c.seq));
                    members.retain(|s| !self.leaves.iter().any(|(q, c)| q == p && c.shortcode == **e && c.seq == *s));
                    members.sort_unstable();
                    members.dedup();
                    let mut h: BTreeMap<(&'a ShortcodeId, u32), RoaringBitmap> = BTreeMap::new();
                    for s in members {
                        let Some(row) = self.row_at(e, s) else { continue };
                        for (k, _) in self.final_tally(&row.id) {
                            h.entry(k).or_default().insert(s);
                        }
                    }
                    m.cleared.push((p.to_string(), (*e).clone()));
                    for ((ge, c), bm) in &h {
                        for (hi, card, bits) in super::cells::containers(bm) {
                            m.holder_rows.push(((p.to_string(), (*e).clone(), (*ge).clone(), *c, hi), Some((card, bits))));
                        }
                    }
                    m.built.push((p.to_string(), (*e).clone(), h.into_iter().map(|((ge, c), bm)| (ge.clone(), c, bm)).collect()));
                }
                (true, false) => m.cleared.push((p.to_string(), (*e).clone())),
                _ => {}
            }
        }
        // The bits that flip in lists that keep their holders.
        let mut flips: BTreeMap<(&'a str, &'a ShortcodeId, &'a ShortcodeId, u32), (Vec<u32>, Vec<u32>)> = BTreeMap::new();
        for (a, epic, cell, on) in crossings {
            let Some(ar) = self.rows.get(a) else { continue };
            let Some(p) = ar.parent.as_deref() else { continue };
            if !keeps(p, &ar.shortcode) {
                continue;
            }
            let f = flips.entry((p, &ar.shortcode, epic, cell)).or_default();
            if on {
                f.1.push(ar.seq);
            } else {
                f.0.push(ar.seq);
            }
        }
        for (p, x) in &self.leaves {
            if !keeps(p, &x.shortcode) {
                continue;
            }
            for (gg, _) in self.children.kids(&x.id).map(|k| k.tally.as_slice()).unwrap_or(&[]) {
                flips.entry((p, &x.shortcode, names.name(g_epic(*gg)), g_cell(*gg))).or_default().0.push(x.seq);
            }
        }
        for (p, x) in &self.joins {
            if !keeps(p, &x.shortcode) {
                continue;
            }
            for (k, _) in self.final_tally(&x.id) {
                flips.entry((p, &x.shortcode, k.0, k.1)).or_default().1.push(x.seq);
            }
        }
        for ((p, e, ge, cell), (leave, join)) in flips {
            let leave = RoaringBitmap::from_iter(leave);
            let join = RoaringBitmap::from_iter(join);
            let empty = RoaringBitmap::new();
            let now = self
                .children
                .in_shortcode(p, e)
                .and_then(Seqs::many)
                .and_then(|many| {
                    let gg = g(names.get(ge)?, cell);
                    many.holders.binary_search_by_key(&gg, |(x, _)| *x).ok().map(|i| &many.holders[i].1)
                })
                .unwrap_or(&empty);
            // The blocks the flips touch, as they will be.
            let mut blocks: Vec<u32> = leave.iter().chain(join.iter()).map(|s| s >> 16).collect();
            blocks.sort_unstable();
            blocks.dedup();
            let mut mask = RoaringBitmap::new();
            for hi in &blocks {
                mask.insert_range((hi << 16)..=((hi << 16) | 0xFFFF));
            }
            let mut after = now & &mask;
            after -= &leave;
            after |= &join;
            let mut rows: BTreeMap<u32, (u64, Vec<u8>)> =
                super::cells::containers(&after).into_iter().map(|(hi, card, bits)| (hi, (card, bits))).collect();
            for hi in blocks {
                m.holder_rows.push(((p.to_string(), e.clone(), ge.clone(), cell, hi), rows.remove(&hi)));
            }
            m.flips.push((p.to_string(), e.clone(), ge.clone(), cell, leave, join));
        }
        m
    }
}

/// The differences between two stored forms of the kept tree, one line
/// each, every cell named as what it is (`describe`: the stored side's cell
/// table, then the scan's — the two intern cells in different orders, so
/// rows are compared by the cell, not its id).
pub(super) fn differences(
    stored: &Stored,
    scanned: &Stored,
    describe_stored: impl Fn(u32) -> Option<String>,
    describe_scanned: impl Fn(u32) -> Option<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    let name = |describe: &dyn Fn(u32) -> Option<String>, cell: u32| describe(cell).unwrap_or_else(|| format!("cell {cell}, which the cell table does not hold"));
    let mut tallies: BTreeMap<(String, String, String), (u32, u32)> = BTreeMap::new();
    for ((p, e, c), n) in &stored.tallies {
        tallies.insert((p.clone(), e.to_string(), name(&describe_stored, *c)), (*n, *c));
    }
    let want: BTreeMap<(String, String, String), u32> = scanned
        .tallies
        .iter()
        .map(|((p, e, c), n)| ((p.clone(), e.to_string(), name(&describe_scanned, *c)), *n))
        .collect();
    for ((p, e, k), (n, id)) in &tallies {
        match want.get(&(p.clone(), e.clone(), k.clone())) {
            Some(w) if w == n => {}
            Some(w) => out.push(format!("tree tally of {p}: {e} cell {id} {k}: stored {n}; a scan gives {w}")),
            None => out.push(format!("tree tally of {p}: {e} cell {id} {k}: stored {n}; a scan gives none")),
        }
    }
    for ((p, e, k), w) in &want {
        if !tallies.contains_key(&(p.clone(), e.clone(), k.clone())) {
            out.push(format!("tree tally of {p}: {e} {k}: none stored; a scan gives {w}"));
        }
    }
    let show = |(card, raw): (u64, &[u8])| match super::cells::decode(card, raw) {
        Ok(lows) if lows.len() <= 8 => format!("{card} members {lows:?}"),
        Ok(_) => format!("{card} members"),
        Err(e) => format!("{card} members, unreadable: {e}"),
    };
    type Key = (String, String, String, String, u32);
    let mut holders: BTreeMap<Key, ((u64, &[u8]), u32)> = BTreeMap::new();
    for ((p, l, e, c, hi), (card, raw)) in &stored.holders {
        holders.insert((p.clone(), l.to_string(), e.to_string(), name(&describe_stored, *c), *hi), ((*card, raw.as_slice()), *c));
    }
    let want: BTreeMap<Key, (u64, &[u8])> = scanned
        .holders
        .iter()
        .map(|((p, l, e, c, hi), (card, raw))| ((p.clone(), l.to_string(), e.to_string(), name(&describe_scanned, *c), *hi), (*card, raw.as_slice())))
        .collect();
    for ((p, l, e, k, hi), (row, id)) in &holders {
        match want.get(&(p.clone(), l.clone(), e.clone(), k.clone(), *hi)) {
            Some(w) if *w == *row => {}
            Some(w) => out.push(format!(
                "tree holders of {p}'s children in {l}: {e} cell {id} {k} container {hi}: stored {}; a scan gives {}",
                show(*row),
                show(*w)
            )),
            None => out.push(format!(
                "tree holders of {p}'s children in {l}: {e} cell {id} {k} container {hi}: stored {}; a scan gives none",
                show(*row)
            )),
        }
    }
    for ((p, l, e, k, hi), w) in &want {
        if !holders.contains_key(&(p.clone(), l.clone(), e.clone(), k.clone(), *hi)) {
            out.push(format!("tree holders of {p}'s children in {l}: {e} {k} container {hi}: none stored; a scan gives {}", show(*w)));
        }
    }
    out
}
