//! FILTER-13: a filter compiled with its lineage — `under` — and `tree`.
//!
//! A ticket's lineage (FILTER-10) is its own id, its strict ancestors' ids
//! ([`tree::ancestors`]), its shortcode and the shortcodes hosting it (the
//! roll-ups' settled hosts, MNG-14). `under X` holds where `X` is in it:
//!
//! - for a shortcode `X`, it is a fact about the ticket's shortcode — `X`
//!   is it, or hosts it — so it is compiled into the per-shortcode tables
//!   exactly as `in` is, every shortcode `X` reaches named;
//! - for a ticket `X` (or an id a hand edit names as a parent), it holds on
//!   `X`'s subtree. Each such name is an *atom*, and a filter is compiled
//!   once per set of atoms that hold ([`Compiled::admit`]). In a shortcode
//!   where each atom holds for all of its tickets or for none — known from
//!   the atom's tally, never by looking — one table answers, as before
//!   ([`Mode::Constant`]); in one where an atom's subtree covers part of it,
//!   its tickets fall into regions by which atoms hold (subtrees are nested
//!   or apart), each answered by its own table, the subtrees taken from the
//!   children index ([`subtree`]: one union per internal descendant).
//!
//! `tree` ([`Store::tree_page`]) answers a parent's children, or a
//! shortcode's top row(s), that match or hold a match — (C(P) ∩ M) ∪
//! ⋃ H(P, g) over the cells `g` the filter admits — each with whether it
//! matches and how many matches are strictly below it (a sum over its
//! tally). Under `under X`, every child of P is under the same atoms except
//! the few on the way down to an `X` below P; those are worked out one by
//! one from the tallies of the `X`s below them. A parent on a hand-edited
//! cycle (whose subtree holds its own ancestors) is answered by looking at
//! every ticket of each child's subtree.

use super::cells::{self, Admit, View};
use super::children::Names;
use super::filtered::{self, Lens};
use super::tree::{self, g_cell, g_epic};
use super::*;
use roaring::RoaringBitmap;
use std::borrow::Cow;
use std::cell::{OnceCell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

/// At most this many distinct names one filter's `under` asks for.
pub const UNDERS_MAX: usize = 64;

/// One `under` name that names a ticket, or an id some ticket names as its
/// parent (a hand edit): its truth is a subtree's.
struct Atom<'c> {
    name: String,
    row: Option<&'c Row>,
    tally: &'c [(u32, u32)],
    /// Its subtree, itself included, per shortcode — taken when a
    /// shortcode holds part of it.
    sub: OnceCell<BTreeMap<String, RoaringBitmap>>,
}

/// How the atoms hold across one shortcode's tickets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    /// The same for every ticket: these atoms hold, the others do not.
    Constant(u64),
    /// Some atom holds for part of the shortcode only.
    Partial,
}

/// A filter compiled with its lineage against the cache, under the read
/// lock — for one read.
pub(super) struct Compiled<'c> {
    cache: &'c Cache,
    lens: Lens,
    status: Option<Status>,
    atoms: Vec<Atom<'c>>,
    /// The shortcodes judged on their own: `in`'s, and every one `under`
    /// reaches through its own name or a host's.
    named: Vec<String>,
    /// The shortcodes hosting each shortcode, innermost first — the lineage
    /// past a ticket's own shortcode (only kept when the filter asks
    /// `under`).
    hosts: HashMap<String, Vec<String>>,
    admits: RefCell<HashMap<u64, Rc<Admit>>>,
    /// FILTER-23: a shortcode's tickets whose cell the tables for a set of
    /// atoms admit, as one bitmap — built once per read, however many lists
    /// of that shortcode a walk reads.
    admitted: RefCell<HashMap<(u64, String), Rc<RoaringBitmap>>>,
    /// FILTER-24: `tree`'s `keep` — a ticket the filter does not admit,
    /// answered as if it did ([`Compiled::keep`]).
    kept: Option<&'c Row>,
}

/// The ticket an `under` name names, and its entry in the children index —
/// as the language compares names, whatever the case: the name as given
/// (the language upper-cases it), else, for `EPIC-SEQ`, the id in the
/// shortcode of that name in whatever case a hand edit wrote it.
fn named_id<'c>(cache: &'c Cache, name: &str) -> (Option<&'c Row>, Option<&'c children::Kids>) {
    let (row, kids) = (cache.get(name), cache.children.kids(name));
    if row.is_some() || kids.is_some() {
        return (row, kids);
    }
    let Some((epic, seq)) = name.rsplit_once('-') else { return (None, None) };
    let Some((actual, _)) = cache.cells.shortcodes().find(|(e, _)| e.eq_ignore_ascii_case(epic) && *e != epic) else {
        return (None, None);
    };
    let id = format!("{actual}-{seq}");
    (cache.get(&id), cache.children.kids(&id))
}

/// The shortcodes hosting `epic`, innermost first — its settled host's
/// shortcode, that one's, and so on.
pub(super) fn host_chain(cache: &Cache, epic: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = epic.to_string();
    while let Some(h) = cache.rollups.host(&cur) {
        let Some(row) = cache.get(h) else { break };
        if row.epic == epic || out.contains(&row.epic) {
            break;
        }
        out.push(row.epic.clone());
        cur = row.epic.clone();
    }
    out
}

/// The subtree of `name` — the ticket `row` (when it is one) and every
/// ticket whose lineage holds it — per shortcode, from the children index:
/// each internal descendant's lists OR-ed in. A list kept as a bitmap names
/// its internal children by its holders; a vector's are looked at (at most
/// [`super::children::HOLD_OVER`]).
pub(super) fn subtree(cache: &Cache, name: &str, row: Option<&Row>) -> BTreeMap<String, RoaringBitmap> {
    let mut out: BTreeMap<String, RoaringBitmap> = BTreeMap::new();
    if let Some(r) = row {
        out.entry(r.epic.clone()).or_default().insert(r.seq);
    }
    let mut stack = vec![name.to_string()];
    let mut seen: HashSet<String> = HashSet::from([name.to_string()]);
    while let Some(p) = stack.pop() {
        let Some(kids) = cache.children.kids(&p) else { continue };
        for (e, seqs) in kids.lists() {
            *out.entry(e.to_string()).or_default() |= &*seqs.bitmap();
            let inner: Box<dyn Iterator<Item = u32>> = match seqs.many() {
                Some(many) => {
                    let mut u = RoaringBitmap::new();
                    for (_, h) in &many.holders {
                        u |= h;
                    }
                    Box::new(u.into_iter())
                }
                None => seqs.iter(),
            };
            for s in inner {
                let Some(c) = cache.order.get(&OrderKey { epic: e.to_string(), seq: s }) else { continue };
                if cache.children.kids(&c.id).is_some() && seen.insert(c.id.clone()) {
                    stack.push(c.id.clone());
                }
            }
        }
    }
    out
}

/// A filter's tables by shortcode index, for one set of atoms — looked up
/// once a shortcode, then indexed.
struct Tables<'x> {
    admit: &'x Admit,
    names: &'x Names,
    memo: RefCell<Vec<Option<&'x [bool]>>>,
}

impl<'x> Tables<'x> {
    fn new(admit: &'x Admit, names: &'x Names) -> Self {
        Self { admit, names, memo: RefCell::new(vec![None; names.len()]) }
    }

    /// Whether tree cell `g` is admitted.
    fn admits(&self, g: u32) -> bool {
        let ix = g_epic(g) as usize;
        let mut memo = self.memo.borrow_mut();
        if ix >= memo.len() {
            memo.resize(ix + 1, None);
        }
        let t = *memo[ix].get_or_insert_with(|| self.admit.of(self.names.name(ix as u16)));
        t.get(g_cell(g) as usize).copied().unwrap_or(false)
    }

    /// How many of a tally's descendants are admitted.
    fn sum(&self, tally: impl IntoIterator<Item = (u32, u64)>) -> u64 {
        tally.into_iter().filter(|(g, _)| self.admits(*g)).map(|(_, n)| n).sum()
    }
}

impl<'c> Compiled<'c> {
    /// `lens` against the cache as it stands, `list`'s `status` folded in.
    pub(super) fn new(cache: &'c Cache, lens: &Lens, status: Option<Status>) -> Self {
        let mut atoms = Vec::new();
        for u in lens.unders() {
            let (row, kids) = named_id(cache, u);
            if row.is_none() && kids.is_none() {
                continue;
            }
            atoms.push(Atom {
                name: u.clone(),
                row,
                tally: kids.map_or(&[][..], |k| &k.tally[..]),
                sub: OnceCell::new(),
            });
        }
        let mut named: BTreeSet<String> = lens.named().iter().cloned().collect();
        let mut hosts = HashMap::new();
        if !lens.unders().is_empty() {
            for (epic, _) in cache.cells.shortcodes() {
                let chain = host_chain(cache, epic);
                let reached = |u: &String| u.eq_ignore_ascii_case(epic) || chain.iter().any(|h| h.eq_ignore_ascii_case(u));
                if lens.unders().iter().any(reached) {
                    named.insert(epic.to_string());
                }
                if !chain.is_empty() {
                    hosts.insert(epic.to_string(), chain);
                }
            }
        }
        Self {
            cache,
            lens: lens.clone(),
            status,
            atoms,
            named: named.into_iter().collect(),
            hosts,
            admits: RefCell::new(HashMap::new()),
            admitted: RefCell::new(HashMap::new()),
            kept: None,
        }
    }

    /// FILTER-23: `epic`'s tickets whose cell `admit(mask)` admits, as one
    /// bitmap (the union of those cells' bitmaps) — once per read.
    fn admitted_in(&self, mask: u64, epic: &str) -> Rc<RoaringBitmap> {
        let key = (mask, epic.to_string());
        if let Some(m) = self.admitted.borrow().get(&key) {
            return Rc::clone(m);
        }
        let admit = self.admit(mask);
        let table = admit.of(epic);
        let mut m = RoaringBitmap::new();
        for (_, b) in self.cache.cells.held_in(epic).into_iter().flatten().filter(|(c, _)| table.get(*c as usize).copied().unwrap_or(false)) {
            m |= b;
        }
        let m = Rc::new(m);
        self.admitted.borrow_mut().insert(key, Rc::clone(&m));
        m
    }

    /// The tables for the atoms `mask` holds — compiled once per read.
    pub(super) fn admit(&self, mask: u64) -> Rc<Admit> {
        if let Some(a) = self.admits.borrow().get(&mask) {
            return Rc::clone(a);
        }
        let a = Rc::new(Admit::compile(self.cache.cells.keys(), self.named.iter().cloned(), |epic, key| {
            if self.status.is_some_and(|s| key.status() != s) {
                return false;
            }
            let mut lineage: Vec<&str> = Vec::new();
            if !epic.is_empty() {
                lineage.push(epic);
                lineage.extend(self.hosts.get(epic).into_iter().flatten().map(String::as_str));
            }
            lineage.extend(self.atoms.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, a)| a.name.as_str()));
            self.lens.judge(epic, key, &lineage)
        }));
        self.admits.borrow_mut().insert(mask, Rc::clone(&a));
        a
    }

    /// How many tickets `epic` holds.
    fn held(&self, epic: &str) -> u64 {
        self.cache.cells.held_in(epic).map_or(0, |h| h.iter().map(|(_, b)| b.len()).sum())
    }

    /// How many of `epic`'s tickets are in the atom's subtree — from its
    /// tally and itself.
    fn count_in(&self, a: &Atom<'_>, epic: &str) -> u64 {
        let mine = u64::from(a.row.is_some_and(|r| r.epic == epic));
        let Some(ix) = self.cache.children.names().get(epic) else { return mine };
        mine + a.tally.iter().filter(|(g, _)| g_epic(*g) == ix).map(|(_, n)| u64::from(*n)).sum::<u64>()
    }

    /// How the atoms hold across `epic`.
    pub(super) fn mode(&self, epic: &str) -> Mode {
        if self.atoms.is_empty() {
            return Mode::Constant(0);
        }
        let held = self.held(epic);
        let mut whole = 0u64;
        for (i, a) in self.atoms.iter().enumerate() {
            match self.count_in(a, epic) {
                0 => {}
                n if n == held => whole |= 1 << i,
                _ => return Mode::Partial,
            }
        }
        Mode::Constant(whole)
    }

    fn sub<'a>(&self, a: &'a Atom<'c>) -> &'a BTreeMap<String, RoaringBitmap> {
        a.sub.get_or_init(|| subtree(self.cache, a.row.map_or(a.name.as_str(), |r| r.id.as_str()), a.row))
    }

    /// `epic`'s tickets by which atoms hold for them: each region, and the
    /// atoms that hold there. Subtrees are nested or apart, so a region is
    /// a subtree less the subtrees inside it, and its atoms are those whose
    /// subtree holds it; outside them all, only the atoms whole here hold.
    fn regions(&self, epic: &str) -> Vec<(u64, RoaringBitmap)> {
        let Some(held) = self.cache.cells.held_in(epic) else { return Vec::new() };
        let n = self.held(epic);
        let mut whole = 0u64;
        let mut sets: Vec<(RoaringBitmap, u64)> = Vec::new();
        for (i, a) in self.atoms.iter().enumerate() {
            match self.count_in(a, epic) {
                0 => continue,
                k if k == n => {
                    whole |= 1 << i;
                    continue;
                }
                _ => {}
            }
            let u = self.sub(a).get(epic).cloned().unwrap_or_default();
            match sets.iter_mut().find(|(s, _)| *s == u) {
                Some((_, m)) => *m |= 1 << i,
                None => sets.push((u, 1 << i)),
            }
        }
        let mut out = Vec::new();
        let mut covered = RoaringBitmap::new();
        for (j, (dj, mj)) in sets.iter().enumerate() {
            covered |= dj;
            let (mut mask, mut region) = (whole | mj, dj.clone());
            for (k, (dk, mk)) in sets.iter().enumerate() {
                if k == j {
                    continue;
                }
                if dj.is_subset(dk) {
                    mask |= mk;
                } else if dk.is_subset(dj) {
                    region -= dk;
                }
            }
            out.push((mask, region));
        }
        let mut all = RoaringBitmap::new();
        for (_, b) in held {
            all |= b;
        }
        out.push((whole, all - covered));
        out
    }

    /// The matches in `epic`, per cell: `(cell, how many)`.
    pub(super) fn cell_counts(&self, epic: &str) -> Vec<(u32, u64)> {
        let Some(held) = self.cache.cells.held_in(epic) else { return Vec::new() };
        match self.mode(epic) {
            Mode::Constant(m) => {
                let admit = self.admit(m);
                let table = admit.of(epic);
                held.iter().filter(|(c, _)| table.get(*c as usize).copied().unwrap_or(false)).map(|(c, b)| (*c, b.len())).collect()
            }
            Mode::Partial => {
                let mut by: BTreeMap<u32, u64> = BTreeMap::new();
                for (mask, region) in self.regions(epic) {
                    let admit = self.admit(mask);
                    let table = admit.of(epic);
                    for (c, b) in held.iter().filter(|(c, _)| table.get(*c as usize).copied().unwrap_or(false)) {
                        *by.entry(*c).or_insert(0) += b.intersection_len(&region);
                    }
                }
                by.into_iter().filter(|(_, n)| *n > 0).collect()
            }
        }
    }

    /// The matches in `epic`, as one bitmap.
    pub(super) fn matches_in(&self, epic: &str) -> RoaringBitmap {
        let Some(held) = self.cache.cells.held_in(epic) else { return RoaringBitmap::new() };
        let admitted = |mask: u64, within: Option<&RoaringBitmap>| -> RoaringBitmap {
            let admit = self.admit(mask);
            let table = admit.of(epic);
            let mut out = RoaringBitmap::new();
            for (_, b) in held.iter().filter(|(c, _)| table.get(*c as usize).copied().unwrap_or(false)) {
                match within {
                    Some(w) => out |= b & w,
                    None => out |= b,
                }
            }
            out
        };
        match self.mode(epic) {
            Mode::Constant(m) => admitted(m, None),
            Mode::Partial => {
                let mut out = RoaringBitmap::new();
                for (mask, region) in self.regions(epic) {
                    out |= admitted(mask, Some(&region));
                }
                out
            }
        }
    }

    /// The matches of every shortcode — of `only`, when one is given — as
    /// a [`View`]: a shortcode the atoms hold for whole or not at all is
    /// its admitted cells' bitmaps, as FILTER-4's; one they cut is one
    /// bitmap of its own.
    pub(super) fn view(&self, only: Option<&str>) -> View<'c> {
        let by_epic = self
            .cache
            .cells
            .shortcodes()
            .filter(|(epic, _)| only.is_none_or(|o| o == *epic))
            .filter_map(|(epic, held)| {
                let bms: Vec<Cow<'c, RoaringBitmap>> = match self.mode(epic) {
                    Mode::Constant(m) => {
                        let admit = self.admit(m);
                        let table = admit.of(epic);
                        held.iter()
                            .filter(|(c, _)| table.get(*c as usize).copied().unwrap_or(false))
                            .map(|(_, b)| Cow::Borrowed(b))
                            .collect()
                    }
                    Mode::Partial => {
                        let m = self.matches_in(epic);
                        if m.is_empty() {
                            Vec::new()
                        } else {
                            vec![Cow::Owned(m)]
                        }
                    }
                };
                (!bms.is_empty()).then_some((epic, bms))
            })
            .collect();
        View::from_parts(by_epic)
    }

    /// The lineage of `t` past its ids: its shortcode, then those hosting it.
    fn shortcode_lineage<'t>(&'t self, epic: &'t str) -> impl Iterator<Item = &'t str> + 't {
        std::iter::once(epic).chain(self.hosts.get(epic).into_iter().flatten().map(String::as_str))
    }

    /// `t`'s cell id.
    fn cell(&self, t: &Row) -> u32 {
        self.cache.cells.id(self.cache.key(t)).expect("every ticket's cell is interned")
    }

    /// Whether the filter admits `t`, by its whole lineage — the reference
    /// evaluator's question, asked of one ticket (a hand-edited cycle's).
    fn judge_row(&self, t: &Row) -> bool {
        let key = self.cache.key(t);
        if self.status.is_some_and(|s| key.status() != s) {
            return false;
        }
        let rows = self.cache.rows();
        let mut lineage: Vec<&str> = vec![t.id.as_str()];
        lineage.extend(tree::ancestors(rows, &t.id, t.parent.as_deref()));
        lineage.extend(self.shortcode_lineage(&t.epic));
        self.lens.judge(&t.epic, key, &lineage)
    }

    /// `t`'s node by looking at every ticket of its subtree — for a ticket
    /// on a hand-edited cycle, whose subtree holds its own ancestors.
    fn node_by_looking(&self, t: &Row) -> (bool, u64) {
        let mut below = 0;
        for (e, bm) in subtree(self.cache, &t.id, Some(t)) {
            for s in bm {
                if e == t.epic && s == t.seq {
                    continue;
                }
                if let Some(r) = self.cache.order.get(&OrderKey { epic: e.clone(), seq: s }) {
                    below += u64::from(self.judge_row(r));
                }
            }
        }
        (self.judge_row(t), below)
    }

    /// The atoms whose name is among `ids` — the ticket itself and its
    /// strict ancestors: they hold over its whole subtree.
    fn holding(&self, ids: &[&str]) -> u64 {
        let mut m = 0u64;
        for (i, a) in self.atoms.iter().enumerate() {
            if ids.iter().any(|id| id.eq_ignore_ascii_case(&a.name)) {
                m |= 1 << i;
            }
        }
        m
    }

    /// The atoms that are tickets strictly below `t` — with each one's
    /// strict ancestors.
    fn below(&self, t: &Row) -> Vec<(usize, Vec<&'c str>)> {
        let rows = self.cache.rows();
        self.atoms
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let x = a.row?;
                let chain = tree::ancestors(rows, &x.id, x.parent.as_deref());
                chain.contains(&t.id.as_str()).then_some((i, chain))
            })
            .collect()
    }

    /// `t`'s node — whether it matches, how many matches are strictly
    /// below it — when the atoms `held` hold over its subtree and the atoms
    /// `inside` are tickets strictly below it (each with its ancestors): its
    /// tally split into regions by those subtrees, each region summed under
    /// the atoms that hold there.
    fn node(&self, t: &Row, held: u64, inside: &[(usize, Vec<&'c str>)]) -> (bool, u64) {
        let names = self.cache.children.names();
        let admit = self.admit(held);
        let matched = admit.admits(&t.epic, self.cell(t)).unwrap_or(false);
        let tally_of = |id: &str| -> BTreeMap<u32, i64> {
            self.cache.children.kids(id).map_or_else(BTreeMap::new, |k| k.tally.iter().map(|(g, n)| (*g, i64::from(*n))).collect())
        };
        if inside.is_empty() {
            let tables = Tables::new(&admit, names);
            return (matched, tables.sum(tally_of(&t.id).into_iter().map(|(g, n)| (g, n as u64))));
        }
        // Each inner atom's subtree tally: its tally and its own cell.
        let whole = |i: usize| -> BTreeMap<u32, i64> {
            let x = self.atoms[i].row.expect("an atom below a ticket is a ticket");
            let mut s = tally_of(&x.id);
            let ix = names.get(&x.epic).expect("a ticket below another has its shortcode interned");
            *s.entry(tree::g(ix, self.cell(x))).or_insert(0) += 1;
            s
        };
        let name = |i: usize| self.atoms[i].row.map(|r| r.id.as_str()).unwrap_or("");
        // `j` is inside `i`: `i`'s ticket is among `j`'s ancestors.
        let within = |j: &(usize, Vec<&str>), i: usize| j.1.contains(&name(i));
        let less = |acc: &mut BTreeMap<u32, i64>, by: &BTreeMap<u32, i64>| {
            for (g, n) in by {
                *acc.entry(*g).or_insert(0) -= n;
            }
        };
        let mut regions: Vec<(u64, BTreeMap<u32, i64>)> = Vec::new();
        let mut outside = tally_of(&t.id);
        for a in inside {
            let i = a.0;
            let mut region = whole(i);
            let mut mask = held | (1 << i);
            for b in inside.iter().filter(|b| b.0 != i) {
                if within(a, b.0) {
                    mask |= 1 << b.0;
                } else if within(b, i) && !inside.iter().any(|c| c.0 != i && c.0 != b.0 && within(c, i) && within(b, c.0)) {
                    less(&mut region, &whole(b.0));
                }
            }
            if !inside.iter().any(|b| b.0 != i && within(a, b.0)) {
                less(&mut outside, &whole(i));
            }
            regions.push((mask, region));
        }
        regions.push((held, outside));
        let mut below = 0;
        for (mask, tally) in regions {
            let admit = self.admit(mask);
            let tables = Tables::new(&admit, names);
            below += tables.sum(tally.into_iter().filter(|(_, n)| *n > 0).map(|(g, n)| (g, n as u64)));
        }
        (matched, below)
    }

    /// `t`'s node, by its own lineage: a ticket asked about on its own (a
    /// shortcode's root, a rootless shortcode's tickets).
    fn node_alone(&self, t: &Row) -> (bool, u64) {
        let rows = self.cache.rows();
        if tree::on_cycle(rows, &t.id, t.parent.as_deref()) {
            return self.node_by_looking(t);
        }
        let mut ids: Vec<&str> = vec![t.id.as_str()];
        ids.extend(tree::ancestors(rows, &t.id, t.parent.as_deref()));
        self.node(t, self.holding(&ids), &self.below(t))
    }

    /// FILTER-23: whether `t` matches, by its own lineage — [`Self::node_alone`]'s
    /// `matched`, without its count.
    fn matches_alone(&self, t: &Row) -> bool {
        let rows = self.cache.rows();
        if tree::on_cycle(rows, &t.id, t.parent.as_deref()) {
            return self.judge_row(t);
        }
        let mut ids: Vec<&str> = vec![t.id.as_str()];
        ids.extend(tree::ancestors(rows, &t.id, t.parent.as_deref()));
        self.admit(self.holding(&ids)).admits(&t.epic, self.cell(t)).unwrap_or(false)
    }
}

/// FILTER-13: what `tree` asks — a parent's children, or a shortcode's top
/// level — under a filter. FILTER-23: `matches_only`, the nav that draws
/// only matches — no ticket that only holds them ([`Store::tree_page`]).
///
/// FILTER-24: `keep` — with `matches_only`, a ticket drawn as if it matched
/// (the nav's open ticket, whatever the filter): placed where a match would
/// be, its nearest matches answered under it.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeQuery {
    pub parent: Option<String>,
    pub epic: Option<String>,
    pub lens: Lens,
    pub matches_only: bool,
    pub keep: Option<String>,
}

/// FILTER-13: one row `tree` answers: the ticket, whether it matches, and
/// how many matches are strictly below it.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub summary: TicketSummary,
    pub matched: bool,
    pub below: u64,
}

/// FILTER-13: one page of `tree`.
#[derive(Debug, Clone, PartialEq)]
pub struct TreePage {
    pub nodes: Vec<Node>,
    /// The cursor for the next page — the last row's id; for an empty page
    /// the cursor it was cut with — or `None` when nothing is left.
    pub next: Option<String>,
    /// How many rows are left after this page.
    pub remaining: u64,
}

/// The rows a tree read answers, before the page is cut: per shortcode,
/// in the store's order, the sequences that match or hold a match; and the
/// nodes already worked out. FILTER-23: per shortcode, which of those rows
/// match (`hit`); the hosted shortcodes' roots drawn after them (`tail`, in
/// shortcode order); and whether the rows come from more than one parent
/// (`alone`: each row's node is then worked out by its own lineage).
#[derive(Default)]
struct Rowed<'c> {
    by_epic: Vec<(&'c str, RoaringBitmap)>,
    known: HashMap<(&'c str, u32), (bool, u64)>,
    hit: HashMap<&'c str, RoaringBitmap>,
    tail: Vec<&'c Row>,
    alone: bool,
}

/// FILTER-23: where a hosted shortcode's row is drawn in the nav that shows
/// only matches — under this ticket's row (its host, or the nearest of the
/// host's ancestors that matches or roots a shortcode), or at the top of
/// this shortcode (a legacy shortcode with no root, whose tickets are its
/// top level).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place<'c> {
    Ticket(&'c str),
    Top(&'c str),
}

impl Store {
    /// FILTER-13: `tree { parent | epic, filter, after, limit }` — the rows
    /// the nav would show at that address unfiltered (a parent's children,
    /// as `list { parent }` gives them; or a shortcode's top level: its
    /// root, or — a shortcode with none — its tickets, as `list { epic }`
    /// gives them), those that match or hold a match, in the store's order;
    /// each with `matched` and `below`. Paged as `list` pages: strictly
    /// after `after` (a ticket id, any ticket: a position), at most
    /// `limit`; `remaining` exact.
    ///
    /// FILTER-23: `matches_only` — the nav that draws only matches. At a
    /// parent: the matches nearest below it — its children that match and,
    /// in place of a child that does not, that child's own nearest matches,
    /// at any depth — in the store's order; then the shortcodes whose row
    /// is drawn there ([`Place`]), in shortcode order, each its root's node
    /// whether or not it holds a match. A hosted shortcode's root is its
    /// shortcode's row, never one of the matches: nothing below it is
    /// answered at a parent above it. At a shortcode: its root when the
    /// root matches; when it does not, what its root's address answers (the
    /// top of the shortcode); a shortcode with no root, its tickets that
    /// match, then the shortcodes drawn at its top. A cursor that is one of
    /// the shortcodes' roots is a place among them; any other, a place
    /// among the matches.
    ///
    /// FILTER-24: `keep`, with `matches_only` — a ticket that does not match
    /// is answered as if it did ([`Compiled::keep`]): a row where a match
    /// would be, with its own node (`matched: false`), and its nearest
    /// matches answered under it rather than here. Every other answer is
    /// the one without it.
    pub fn tree_page(&self, q: &TreeQuery, after: Option<&str>, limit: Option<u32>) -> Result<TreePage, StoreError> {
        let up = |s: Option<&str>| s.map(|s| s.trim().to_ascii_uppercase());
        let (parent, epic) = (up(q.parent.as_deref()), up(q.epic.as_deref()));
        let after = up(after);
        let cache = self.tickets.read().expect("store lock poisoned");
        let cursor = match after.as_deref() {
            None => None,
            Some(id) => Some(cache.at.get(id).cloned().ok_or_else(|| StoreError::CursorNotFound(id.to_string()))?),
        };
        let mut c = Compiled::new(&cache, &q.lens, None);
        if let Some(id) = q.keep.as_deref().filter(|_| q.matches_only) {
            c.keep(id);
        }
        let rowed = match (parent.as_deref(), epic.as_deref(), q.matches_only) {
            (Some(p), None, false) => c.children_rows(p),
            (None, Some(e), false) => c.top_rows(e),
            (Some(p), None, true) => c.nearest_rows(p, &c.placed()),
            (None, Some(e), true) => c.top_matches(e),
            _ => return Err(StoreError::TreeAddress),
        };
        Ok(c.page(rowed, cursor.as_ref(), after, limit))
    }
}

impl<'c> Compiled<'c> {
    /// A parent's children that match or hold a match. All but the few on
    /// the way to an `under` ticket below the parent are under the same
    /// atoms — those over the parent — so their rows are bitmaps: the
    /// list's members whose cell is admitted, and the holders of every
    /// admitted cell (a list of more than [`super::children::HOLD_OVER`]);
    /// a shorter list's children are each looked up. The few are worked
    /// out one by one ([`Compiled::node`]).
    fn children_rows(&self, p: &str) -> Rowed<'c> {
        let cache = self.cache;
        let mut rowed = Rowed::default();
        let Some(kids) = cache.children.kids(p) else { return rowed };
        let rows = cache.rows();
        let prow = cache.get(p);
        let pparent = prow.and_then(|r| r.parent.as_deref());
        let cycle = prow.is_some() && tree::on_cycle(rows, p, pparent);
        let mut chain: Vec<&str> = vec![p];
        chain.extend(tree::ancestors(rows, p, pparent));
        let held = self.holding(&chain);
        // The children on the way down to an atom below the parent.
        let mut special: BTreeMap<(&'c str, u32), (&'c Row, u64)> = BTreeMap::new();
        for (i, a) in self.atoms.iter().enumerate() {
            if held & (1 << i) != 0 {
                continue;
            }
            let Some(x) = a.row else { continue };
            let xchain = tree::ancestors(rows, &x.id, x.parent.as_deref());
            let Some(at) = xchain.iter().position(|id| *id == p) else { continue };
            let q = if at == 0 { x } else { cache.get(xchain[at - 1]).expect("an ancestor below a ticket is a ticket") };
            let entry = special.entry((q.epic.as_str(), q.seq)).or_insert((q, 0));
            if at == 0 {
                entry.1 |= 1 << i;
            }
        }
        let names = cache.children.names();
        let admit = self.admit(held);
        let tables = Tables::new(&admit, names);
        let bulk = |r: &Row| -> (bool, u64) {
            let matched = tables.admits(tree::g(names.get(&r.epic).expect("a child's shortcode is interned"), self.cell(r)));
            let below = cache.children.kids(&r.id).map_or(0, |k| tables.sum(k.tally.iter().map(|(g, n)| (*g, u64::from(*n)))));
            (matched, below)
        };
        for (e, seqs) in kids.lists() {
            let mut q = RoaringBitmap::new();
            // FILTER-23: which of the rows match.
            let mut hit = RoaringBitmap::new();
            match seqs.many().filter(|_| !cycle) {
                Some(many) => {
                    hit = &many.seqs & &*self.admitted_in(held, e);
                    q = hit.clone();
                    for (g, h) in &many.holders {
                        if tables.admits(*g) {
                            q |= h;
                        }
                    }
                }
                None => {
                    for s in seqs.iter() {
                        if special.contains_key(&(e, s)) {
                            continue;
                        }
                        let Some(r) = cache.order.get(&OrderKey { epic: e.to_string(), seq: s }) else { continue };
                        let node = if cycle { self.node_by_looking(r) } else { bulk(r) };
                        if node.0 || node.1 > 0 {
                            q.insert(s);
                        }
                        if node.0 {
                            hit.insert(s);
                        }
                        rowed.known.insert((e, s), node);
                    }
                }
            }
            for ((_, s), (r, also)) in special.range((e, 0)..=(e, u32::MAX)) {
                q.remove(*s);
                hit.remove(*s);
                let node = if cycle { self.node_by_looking(r) } else { self.node(r, held | also, &self.below(r)) };
                if node.0 || node.1 > 0 {
                    q.insert(*s);
                }
                if node.0 {
                    hit.insert(*s);
                }
                rowed.known.insert((e, *s), node);
            }
            if !q.is_empty() {
                rowed.by_epic.push((e, q));
            }
            if !hit.is_empty() {
                rowed.hit.insert(e, hit);
            }
        }
        rowed
    }

    /// FILTER-23: the rows at parent `p` in the nav that draws only matches
    /// ([`Store::tree_page`]): the matches nearest below `p`, then the hosted
    /// shortcodes `placed` at `p`. The walk reads `p`'s children as
    /// [`Self::children_rows`] does — a long list's matches and holders as
    /// bitmaps, a short one's children looked up — keeps those that match,
    /// and goes on into those that only hold a match, each once. It never
    /// goes below a match, below a ticket that holds none, or into a hosted
    /// shortcode (its root is that shortcode's row): what it reads is the
    /// tickets that hold matches without matching, between `p` and the
    /// matches — never the matches' own subtrees.
    fn nearest_rows(&self, p: &str, placed: &[(&'c Row, Place<'c>)]) -> Rowed<'c> {
        let cache = self.cache;
        let hosted = self.hosted_roots();
        let rows = cache.rows();
        let prow = cache.get(p);
        let mut chain: Vec<&str> = vec![p];
        chain.extend(prow.map(|r| tree::ancestors(rows, &r.id, r.parent.as_deref())).unwrap_or_default());
        let held = self.holding(&chain);
        // An `under` ticket strictly below `p` cuts its subtree into regions.
        let cut = self.atoms.iter().enumerate().any(|(i, a)| {
            held & (1 << i) == 0 && a.row.is_some_and(|x| tree::ancestors(rows, &x.id, x.parent.as_deref()).contains(&p))
        });
        if !cut && !prow.is_some_and(|r| tree::on_cycle(rows, &r.id, r.parent.as_deref())) {
            let by = self.nearest_fast(p, held, &hosted);
            let mut rowed = Rowed { by_epic: by.into_iter().collect(), alone: true, ..Rowed::default() };
            self.splice_kept(&mut rowed, Place::Ticket(p));
            self.place_tail(&mut rowed, placed, |at| matches!(at, Place::Ticket(id) if id == p));
            return rowed;
        }
        let mut by: BTreeMap<&'c str, RoaringBitmap> = BTreeMap::new();
        let mut known: HashMap<(&'c str, u32), (bool, u64)> = HashMap::new();
        let mut stack: Vec<&str> = vec![p];
        let mut seen: HashSet<&str> = HashSet::from([p]);
        while let Some(x) = stack.pop() {
            let rowed = self.children_rows(x);
            for (e, q) in &rowed.by_epic {
                let none = RoaringBitmap::new();
                let boundary = hosted.get(e).unwrap_or(&none);
                let hit = rowed.hit.get(e).map_or_else(RoaringBitmap::new, |h| h - boundary);
                let down = (q - &hit) - boundary;
                for s in &hit {
                    if let Some(n) = rowed.known.get(&(*e, s)) {
                        known.insert((*e, s), *n);
                    }
                }
                *by.entry(*e).or_default() |= hit;
                for s in &down {
                    let Some(r) = cache.order.get(&OrderKey { epic: e.to_string(), seq: s }) else { continue };
                    if seen.insert(r.id.as_str()) {
                        stack.push(r.id.as_str());
                    }
                }
            }
        }
        let mut rowed = Rowed { by_epic: by.into_iter().filter(|(_, b)| !b.is_empty()).collect(), known, alone: true, ..Rowed::default() };
        self.splice_kept(&mut rowed, Place::Ticket(p));
        self.place_tail(&mut rowed, placed, |at| matches!(at, Place::Ticket(id) if id == p));
        rowed
    }

    /// FILTER-23: [`Self::nearest_rows`]'s walk when no ticket's own lineage
    /// is needed — no `under` names a ticket strictly below `p` (every ticket
    /// below it is then under the atoms `held` over `p`), and `p` is on no
    /// hand-edited cycle (so nothing below it is: a cycle below `p` would run
    /// through it). One table then answers every ticket: a list's matches are
    /// its members in the shortcode's admitted bitmap, its children holding a
    /// match a long list's holders of the admitted cells (a short one's,
    /// those whose tally holds one) — and a ticket whose matches below are
    /// all its own children (its tally's admitted sum is how many of them
    /// match) is not looked into. Each ticket it goes through costs its tally
    /// and its lists, never a lineage.
    fn nearest_fast(&self, p: &str, held: u64, hosted: &HashMap<&'c str, RoaringBitmap>) -> BTreeMap<&'c str, RoaringBitmap> {
        let cache = self.cache;
        let admit = self.admit(held);
        let tables = Tables::new(&admit, cache.children.names());
        let none = RoaringBitmap::new();
        let mut memo: HashMap<&'c str, Rc<RoaringBitmap>> = HashMap::new();
        let mut by: BTreeMap<&'c str, RoaringBitmap> = BTreeMap::new();
        let mut stack: Vec<&'c children::Kids> = cache.children.kids(p).into_iter().collect();
        while let Some(kids) = stack.pop() {
            let below = tables.sum(kids.tally.iter().map(|(g, n)| (*g, u64::from(*n))));
            let mut mine = 0u64;
            let mut hits: Vec<(&'c str, &'c children::Seqs, RoaringBitmap)> = Vec::new();
            for (e, seqs) in kids.lists() {
                let m = Rc::clone(memo.entry(e).or_insert_with(|| self.admitted_in(held, e)));
                let boundary = hosted.get(e).unwrap_or(&none);
                let hit: RoaringBitmap = match seqs.many() {
                    Some(many) => (&many.seqs & &*m) - boundary,
                    None => seqs.iter().filter(|s| m.contains(*s) && !boundary.contains(*s)).collect(),
                };
                mine += hit.len();
                hits.push((e, seqs, hit));
            }
            for (e, seqs, hit) in hits {
                if below > mine {
                    let boundary = hosted.get(e).unwrap_or(&none);
                    let down: Vec<u32> = match seqs.many() {
                        Some(many) => {
                            let mut q = RoaringBitmap::new();
                            for (g, h) in &many.holders {
                                if tables.admits(*g) {
                                    q |= h;
                                }
                            }
                            ((q - &hit) - boundary).iter().collect()
                        }
                        None => seqs.iter().filter(|s| !hit.contains(*s) && !boundary.contains(*s)).collect(),
                    };
                    let mut key = OrderKey { epic: e.to_string(), seq: 0 };
                    for s in down {
                        key.seq = s;
                        let Some(k) = cache.order.get(&key).and_then(|r| cache.children.kids(&r.id)) else { continue };
                        if k.tally.iter().any(|(g, _)| tables.admits(*g)) {
                            stack.push(k);
                        }
                    }
                }
                if !hit.is_empty() {
                    *by.entry(e).or_default() |= hit;
                }
            }
        }
        by
    }

    /// FILTER-23: a shortcode's top in the nav that draws only matches: its
    /// root when the root matches; else what the root's address answers; a
    /// shortcode with no root, its tickets that match, then the shortcodes
    /// drawn at its top.
    fn top_matches(&self, epic: &str) -> Rowed<'c> {
        let cache = self.cache;
        let Some((name, _)) = cache.cells.shortcodes().find(|(e, _)| *e == epic) else { return Rowed::default() };
        let placed = self.placed();
        match cache.rollups.root(epic).and_then(|r| cache.get(r)) {
            Some(root) => {
                let node = self.node_alone(root);
                if !node.0 {
                    return self.nearest_rows(&root.id, &placed);
                }
                let mut rowed = Rowed::default();
                rowed.by_epic.push((name, RoaringBitmap::from_iter([root.seq])));
                rowed.known.insert((name, root.seq), node);
                rowed
            }
            None => {
                let m = self.matches_in(name);
                let mut rowed = Rowed { alone: true, ..Rowed::default() };
                if !m.is_empty() {
                    rowed.by_epic.push((name, m));
                }
                self.splice_kept(&mut rowed, Place::Top(name));
                self.place_tail(&mut rowed, &placed, |at| at == Place::Top(name));
                rowed
            }
        }
    }

    /// FILTER-24: keep `id` — answered as if it matched — when it names a
    /// ticket that does not match, is no shortcode's root (a root is its
    /// shortcode's row already) and is on no hand-edited cycle.
    pub(super) fn keep(&mut self, id: &str) {
        let cache = self.cache;
        let Some(k) = filtered::keep_row(cache, id) else { return };
        if cache.rollups.root(&k.epic) == Some(k.id.as_str())
            || tree::on_cycle(cache.rows(), &k.id, k.parent.as_deref())
            || self.matches_alone(k)
        {
            return;
        }
        self.kept = Some(k);
    }

    /// FILTER-24: whether `t` is the kept ticket.
    fn is_kept(&self, t: &Row) -> bool {
        self.kept.is_some_and(|k| k.id == t.id)
    }

    /// FILTER-24: the rows answered at `here`, with the kept ticket where a
    /// match would be. At a ticket: when it is the kept one's ancestor with
    /// nothing between them that matches or roots a hosted shortcode (what
    /// the walk down from here stops at), the kept row is added in the
    /// store's order and its nearest matches — answered here in its place
    /// until now — are taken out: they are its own rows. At the top of a
    /// shortcode with no root (every match of it, flat): added when it is of
    /// that shortcode.
    fn splice_kept(&self, rowed: &mut Rowed<'c>, here: Place<'_>) {
        let Some(k) = self.kept else { return };
        match here {
            Place::Top(e) => {
                if k.epic != e {
                    return;
                }
            }
            Place::Ticket(p) => {
                let cache = self.cache;
                let mut cur = k;
                let mut seen: HashSet<&str> = HashSet::from([k.id.as_str()]);
                loop {
                    // A parent that names no ticket (a hand edit) has
                    // children all the same.
                    let Some(up) = cur.parent.as_deref() else { return };
                    if up == p {
                        break;
                    }
                    let Some(t) = cache.get(up) else { return };
                    if !seen.insert(t.id.as_str()) {
                        return;
                    }
                    // What the walk down from `p` stops at: a match, or a
                    // hosted shortcode's root (its shortcode's row).
                    let hosted = cache.rollups.host(&t.epic).is_some() && cache.rollups.root(&t.epic) == Some(t.id.as_str());
                    if hosted || self.matches_alone(t) {
                        return;
                    }
                    cur = t;
                }
                let mine = self.nearest_rows(&k.id, &[]);
                for (e, q) in &mine.by_epic {
                    if let Some((_, b)) = rowed.by_epic.iter_mut().find(|(x, _)| x == e) {
                        *b -= q;
                    }
                }
                rowed.by_epic.retain(|(_, b)| !b.is_empty());
            }
        }
        match rowed.by_epic.binary_search_by(|(e, _)| (*e).cmp(k.epic.as_str())) {
            Ok(i) => {
                rowed.by_epic[i].1.insert(k.seq);
            }
            Err(i) => rowed.by_epic.insert(i, (k.epic.as_str(), RoaringBitmap::from_iter([k.seq]))),
        }
        rowed.known.insert((k.epic.as_str(), k.seq), self.node_alone(k));
    }

    /// FILTER-23: the hosted shortcodes' roots, by shortcode — each its
    /// shortcode's row, never a match among another's.
    fn hosted_roots(&self) -> HashMap<&'c str, RoaringBitmap> {
        let cache = self.cache;
        let mut out: HashMap<&'c str, RoaringBitmap> = HashMap::new();
        for (epic, _) in cache.cells.shortcodes() {
            if cache.rollups.host(epic).is_none() {
                continue;
            }
            if let Some(root) = cache.rollups.root(epic).and_then(|r| cache.get(r)) {
                out.entry(root.epic.as_str()).or_default().insert(root.seq);
            }
        }
        out
    }

    /// FILTER-23: where each hosted shortcode's row is drawn in the nav that
    /// draws only matches, in shortcode order: from its host up, the first
    /// ticket that matches or roots a shortcode (a row the nav draws: a
    /// match, or a shortcode's row); past a ticket with no parent, the top
    /// of its shortcode. A host chain that ends at an id naming no ticket,
    /// or comes back on itself (hand edits), places it nowhere.
    fn placed(&self) -> Vec<(&'c Row, Place<'c>)> {
        let cache = self.cache;
        let mut out = Vec::new();
        for (epic, _) in cache.cells.shortcodes() {
            let Some(host) = cache.rollups.host(epic) else { continue };
            let Some(root) = cache.rollups.root(epic).and_then(|r| cache.get(r)) else { continue };
            let mut cur = cache.get(host);
            let mut seen: HashSet<&str> = HashSet::new();
            let place = loop {
                let Some(t) = cur else { break None };
                if !seen.insert(t.id.as_str()) {
                    break None;
                }
                if cache.rollups.root(&t.epic) == Some(t.id.as_str()) || self.is_kept(t) || self.matches_alone(t) {
                    break Some(Place::Ticket(t.id.as_str()));
                }
                match t.parent.as_deref() {
                    None => break Some(Place::Top(t.epic.as_str())),
                    Some(p) => cur = cache.get(p),
                }
            };
            if let Some(place) = place {
                out.push((root, place));
            }
        }
        out
    }

    /// FILTER-23: the roots of the shortcodes `placed` where `here` says,
    /// after the rows — each with its node.
    fn place_tail(&self, rowed: &mut Rowed<'c>, placed: &[(&'c Row, Place<'c>)], here: impl Fn(Place<'c>) -> bool) {
        for (root, at) in placed {
            if here(*at) {
                rowed.known.insert((root.epic.as_str(), root.seq), self.node_alone(root));
                rowed.tail.push(root);
            }
        }
    }

    /// A shortcode's top level: its root, when it has one; its tickets in
    /// the store's order, when it has none (a store from before MNG-12).
    fn top_rows(&self, epic: &str) -> Rowed<'c> {
        let cache = self.cache;
        let mut rowed = Rowed::default();
        let Some((name, _)) = cache.cells.shortcodes().find(|(e, _)| *e == epic) else { return rowed };
        let tops: Vec<&'c Row> = match cache.rollups.root(epic).and_then(|r| cache.get(r)) {
            Some(root) => vec![root],
            None => {
                let from = OrderKey { epic: epic.to_string(), seq: 0 };
                let to = OrderKey { epic: epic.to_string(), seq: u32::MAX };
                cache.order.range(from..=to).map(|(_, t)| t).collect()
            }
        };
        let mut q = RoaringBitmap::new();
        for t in tops {
            let node = self.node_alone(t);
            if node.0 || node.1 > 0 {
                q.insert(t.seq);
            }
            rowed.known.insert((name, t.seq), node);
        }
        if !q.is_empty() {
            rowed.by_epic.push((name, q));
        }
        rowed
    }

    /// The page: the rows strictly after `cursor`, at most `limit`, each
    /// with its node; `remaining` the rows after it; `next` as `list`'s.
    fn page(&self, rowed: Rowed<'c>, cursor: Option<&OrderKey>, after: Option<String>, limit: Option<u32>) -> TreePage {
        let take = limit.map_or(usize::MAX, |n| n as usize);
        let mut picked: Vec<(&'c str, u32)> = Vec::new();
        let mut total: u64 = 0;
        // FILTER-23: a cursor that is one of the tail's roots is a place in
        // the tail, after every row.
        let in_tail = cursor.and_then(|k| rowed.tail.iter().position(|r| r.epic == k.epic && r.seq == k.seq));
        for (e, q) in rowed.by_epic.iter().filter(|_| in_tail.is_none()) {
            let lo = match cursor {
                Some(k) if k.epic.as_str() > *e => continue,
                Some(k) if k.epic == *e => match k.seq.checked_add(1) {
                    Some(lo) => lo,
                    None => continue,
                },
                _ => 0,
            };
            let below = lo.checked_sub(1).map_or(0, |b| q.rank(b));
            total += q.len() - below;
            if picked.len() < take {
                picked.extend(q.range(lo..).take(take - picked.len()).map(|s| (*e, s)));
            }
        }
        let tail = &rowed.tail[in_tail.map_or(0, |i| i + 1)..];
        total += tail.len() as u64;
        picked.extend(tail.iter().take(take.saturating_sub(picked.len())).map(|r| (r.epic.as_str(), r.seq)));
        let names = self.cache.children.names();
        let rows: Vec<&'c Row> = picked
            .iter()
            .filter_map(|(e, s)| self.cache.order.get(&OrderKey { epic: e.to_string(), seq: *s }))
            .collect();
        // A long list's rows were not each worked out: they are under the
        // atoms over their parent — one parent for every such row.
        let mask = rows
            .iter()
            .filter(|_| !rowed.alone)
            .find(|r| !rowed.known.contains_key(&(r.epic.as_str(), r.seq)))
            .and_then(|r| r.parent.as_deref())
            .map_or(0, |p| {
                let mut chain: Vec<&str> = vec![p];
                chain.extend(tree::ancestors(self.cache.rows(), p, self.cache.get(p).and_then(|x| x.parent.as_deref())));
                self.holding(&chain)
            });
        let admit = self.admit(mask);
        let tables = Tables::new(&admit, names);
        let mut nodes = Vec::with_capacity(rows.len());
        for r in rows {
            let (matched, below) = match rowed.known.get(&(r.epic.as_str(), r.seq)) {
                Some(n) => *n,
                // FILTER-23: rows from many parents — each by its lineage.
                None if rowed.alone => self.node_alone(r),
                None => {
                    let matched = tables.admits(tree::g(names.get(&r.epic).expect("a child's shortcode is interned"), self.cell(r)));
                    let below = self.cache.children.kids(&r.id).map_or(0, |k| tables.sum(k.tally.iter().map(|(g, n)| (*g, u64::from(*n)))));
                    (matched, below)
                }
            };
            nodes.push(Node { summary: r.clone(), matched, below });
        }
        let remaining = total.saturating_sub(nodes.len() as u64);
        let next = if remaining == 0 { None } else { nodes.last().map(|n| n.summary.id.clone()).or(after) };
        TreePage { nodes, next, remaining }
    }
}

// ---------------------------------------------------------------------------
// `watch { filter }` with `under`: a ticket's membership is its cell's and
// its lineage's, and a re-parent — or a shortcode re-hosted — moves the
// lineage of a whole subtree without moving a cell. What a write did to such
// a watch is worked out by the write itself, before it commits and after it
// applies, under the write-serial lock — so it is exact whatever a lagging
// subscriber reads later — and only while such a watch is open.

/// FILTER-13: one open `watch { filter }` whose filter asks `under`.
pub struct UnderWatch {
    id: u64,
    lens: Lens,
    epic: Option<String>,
}

impl UnderWatch {
    pub(super) fn id(&self) -> u64 {
        self.id
    }
}

/// FILTER-13: the open `watch { filter }`s whose filter asks `under` —
/// weakly held: a watch that ends drops out.
#[derive(Default)]
pub(crate) struct UnderWatches {
    next: std::sync::atomic::AtomicU64,
    open: StdMutex<Vec<std::sync::Weak<UnderWatch>>>,
}

impl UnderWatches {
    /// Registers a watch of `lens` (in `epic` alone, when one is asked).
    pub(super) fn open(&self, lens: Lens, epic: Option<String>) -> Arc<UnderWatch> {
        let id = self.next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let w = Arc::new(UnderWatch { id, lens, epic });
        self.open.lock().expect("watches lock poisoned").push(Arc::downgrade(&w));
        w
    }

    /// The watches still open.
    fn live(&self) -> Vec<Arc<UnderWatch>> {
        let mut open = self.open.lock().expect("watches lock poisoned");
        open.retain(|w| w.strong_count() > 0);
        open.iter().filter_map(std::sync::Weak::upgrade).collect()
    }
}

/// FILTER-13: what one write moved for each open `under` watch — by its id,
/// its events (after its `epic`), in the store's order, or its recounts.
#[derive(Debug, Default)]
pub struct UnderMoves {
    pub(super) by: HashMap<u64, Vec<filtered::Membership>>,
}

/// What a write touches that `under` reads, worked out before it commits:
/// the tickets whose cell it moves, a subtree it moves, and the shortcodes
/// whose hosts it changes (every ticket of those).
#[derive(Default)]
pub(super) struct Touched {
    by_epic: BTreeMap<String, RoaringBitmap>,
    whole: BTreeSet<String>,
}

impl Touched {
    /// The tickets a write's cell moves move.
    pub(super) fn of_moves(moves: Option<&cells::Moves>) -> Self {
        let mut t = Self::default();
        for m in moves.into_iter().flat_map(|m| &m.moves) {
            t.by_epic.entry(m.epic.to_string()).or_default().insert(m.seq);
        }
        t
    }

    /// And the subtree of `name` (the ticket `row`, when it is one), as it
    /// stands.
    pub(super) fn subtree(&mut self, cache: &Cache, name: &str, row: Option<&Row>) {
        for (e, bm) in subtree(cache, name, row) {
            *self.by_epic.entry(e).or_default() |= bm;
        }
    }

    /// And every shortcode whose chain of hosts differs between the
    /// roll-ups before (the cache's) and `after` over `rows` (the rows as the
    /// write leaves them) — of the shortcodes the store holds, and `extra`.
    pub(super) fn hosts(&mut self, cache: &Cache, after: &rollup::Rollups, rows: Rows<'_>, extra: Option<&str>) {
        let chain = |r: &rollup::Rollups, rows: Rows<'_>, epic: &str| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            let mut cur = epic.to_string();
            while let Some(h) = r.host(&cur) {
                let Some(row) = rows.get(h) else { break };
                if row.epic == epic || out.contains(&row.epic) {
                    break;
                }
                out.push(row.epic.clone());
                cur = row.epic.clone();
            }
            out
        };
        let epics: BTreeSet<&str> = cache.cells.shortcodes().map(|(e, _)| e).chain(extra).collect();
        for e in epics {
            if chain(&cache.rollups, cache.rows(), e) != chain(after, rows, e) {
                self.whole.insert(e.to_string());
            }
        }
    }

    /// Each shortcode touched, and what of it — `None`: all of it.
    fn areas(&self) -> Vec<(&str, Option<&RoaringBitmap>)> {
        let mut out: Vec<(&str, Option<&RoaringBitmap>)> = Vec::new();
        let epics: BTreeSet<&str> = self.by_epic.keys().map(String::as_str).chain(self.whole.iter().map(String::as_str)).collect();
        for e in epics {
            out.push((e, if self.whole.contains(e) { None } else { self.by_epic.get(e) }));
        }
        out
    }
}

/// The open `under` watches' matches among what a write touches, before it
/// commits — what [`UnderBefore::after`] compares with.
pub(super) struct UnderBefore {
    watches: Vec<Arc<UnderWatch>>,
    touched: Touched,
    before: Vec<BTreeMap<String, RoaringBitmap>>,
}

impl Store {
    /// FILTER-13: before a write commits (the write-serial lock held, the
    /// cache as it stands): when a `watch { filter }` asking `under` is open,
    /// what the write touches and which of it each such watch's filter
    /// matches. `None`, and nothing worked out, when none is open.
    pub(super) fn under_before(&self, cache: &Cache, touched: impl FnOnce() -> Touched) -> Option<UnderBefore> {
        let watches = self.unders.live();
        if watches.is_empty() {
            return None;
        }
        let touched = touched();
        let before = watches
            .iter()
            .map(|w| {
                let c = Compiled::new(cache, &w.lens, None);
                touched
                    .areas()
                    .into_iter()
                    .map(|(e, area)| {
                        let m = c.matches_in(e);
                        (e.to_string(), match area {
                            Some(a) => m & a,
                            None => m,
                        })
                    })
                    .collect()
            })
            .collect();
        Some(UnderBefore { watches, touched, before })
    }
}

impl UnderBefore {
    /// …and once the write is in the cache (the lock still held): each
    /// watch's tickets that entered and left, in the store's order — or,
    /// past [`cells::RECOUNT_OVER`], its count of each shortcode they are
    /// in, as the write left it. `None` when no watch's membership moved.
    pub(super) fn after(self, tickets: &RwLock<Cache>) -> Option<Change> {
        use filtered::Membership;
        let cache = tickets.read().expect("store lock poisoned");
        let mut by: HashMap<u64, Vec<Membership>> = HashMap::new();
        for (w, before) in self.watches.iter().zip(&self.before) {
            let c = Compiled::new(&cache, &w.lens, None);
            let mut moved: Vec<(&str, u32, bool)> = Vec::new();
            let mut epics: Vec<&str> = Vec::new();
            for (e, area) in self.touched.areas() {
                if w.epic.as_deref().is_some_and(|x| x != e) {
                    continue;
                }
                let m = c.matches_in(e);
                let now = match area {
                    Some(a) => m & a,
                    None => m,
                };
                let was = before.get(e).cloned().unwrap_or_default();
                let n = moved.len();
                let entered = &now - &was;
                let left = &was - &now;
                let mut all: Vec<(u32, bool)> = entered.iter().map(|s| (s, true)).chain(left.iter().map(|s| (s, false))).collect();
                all.sort_unstable();
                moved.extend(all.into_iter().map(|(s, inn)| (e, s, inn)));
                if moved.len() > n {
                    epics.push(e);
                }
            }
            if moved.is_empty() {
                continue;
            }
            let events = if moved.len() > cells::RECOUNT_OVER {
                epics.into_iter().map(|e| Membership::Recount { epic: e.to_string(), held: c.matches_in(e).len() }).collect()
            } else {
                moved
                    .into_iter()
                    .map(|(e, seq, inn)| {
                        let key = OrderKey { epic: e.to_string(), seq };
                        let id = cache.order.get(&key).map_or_else(|| format!("{e}-{seq}"), |t| t.id.clone());
                        let epic = e.to_string();
                        if inn {
                            Membership::Entered { id, epic, seq }
                        } else {
                            Membership::Left { id, epic, seq }
                        }
                    })
                    .collect()
            };
            by.insert(w.id, events);
        }
        (!by.is_empty()).then(|| Change::UnderMoved { moves: Arc::new(UnderMoves { by }) })
    }
}
