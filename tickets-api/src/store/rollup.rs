use super::stored::{Changes, CrossRow, GroupRow, Mark};
use super::{Group, Row, Rows, Status};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Tickets counted: how many, and how many `Complete` (the nav's
/// `done` — `Superseded` is not done). SDLC-11: and the docs, apart — a
/// doc has no completion, so it is in neither `total` nor `done` (a
/// scope's done-ness ignores doc children), and counts in `docs` alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Tally {
    total: u64,
    done: u64,
    docs: u64,
}

impl Tally {
    /// One completion, gained or lost — all a status change moves.
    const DONE: Self = Self {
        total: 0,
        done: 1,
        docs: 0,
    };

    /// One ticket: a doc whatever its status says (only a hand edit gives
    /// one a status but `Doc`), work otherwise.
    fn of(t: &Row) -> Self {
        if t.kind.has_lifecycle() {
            Self {
                total: 1,
                done: u64::from(t.status == Status::Complete),
                docs: 0,
            }
        } else {
            Self {
                total: 0,
                done: 0,
                docs: 1,
            }
        }
    }

    /// Nothing counted: a row that draws no ticket.
    fn is_empty(self) -> bool {
        self == Self::default()
    }

    fn shift(&mut self, by: Tally, gain: bool) {
        if gain {
            self.total += by.total;
            self.done += by.done;
            self.docs += by.docs;
        } else {
            self.total -= by.total;
            self.done -= by.done;
            self.docs -= by.docs;
        }
    }

    /// As stored: `(total, done, docs)`.
    fn stored(self) -> (u64, u64, u64) {
        (self.total, self.done, self.docs)
    }

    fn from_stored((total, done, docs): (u64, u64, u64)) -> Self {
        Self { total, done, docs }
    }
}

/// One shortcode's half of the roll-up.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Shortcode {
    /// Its lowest-seq `scope` ticket — SDLC-11: or `doc`, which may root a
    /// shortcode as a scope may. Fixed once set: kinds never change and
    /// every later ticket has a higher seq.
    root: Option<String>,
    /// The trees whose top has no parent — drawn in this row whatever
    /// moves elsewhere.
    base: Tally,
    /// How many such tops: 1 in every shortcode made since MNG-14 (its
    /// root), more only in a legacy prefix with parentless tickets. A
    /// move of the only one takes `base` whole instead of walking it.
    base_tops: u64,
    /// Settled: AGS-20's sub-shortcode host — the ticket this row nests
    /// under — or `None` for a top-level row.
    host: Option<String>,
    /// Settled: the group whose subtree holds `host`, i.e. the row this
    /// one is drawn inside.
    up: Option<String>,
    /// The tickets drawn under this row before any nested group: `base`
    /// plus the cross trees homed here. Empty — no work and no doc — means
    /// the nav draws no row for this shortcode.
    own: Tally,
    /// `own` plus every nested group's — the row's `done/total`.
    roll: Tally,
}

/// A cross top's tree: its tally, and the group it is drawn in.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Cross {
    tally: Tally,
    home: String,
}

/// Where a tree's tally is kept: a shortcode's pool, or a cross top's
/// own entry.
#[derive(Debug, PartialEq, Eq)]
enum Bucket {
    Base(String),
    Cross(String),
}

impl Bucket {
    fn of(top: &Row, exit: Option<&Row>) -> Self {
        match exit {
            Some(_) => Self::Cross(top.id.clone()),
            None => Self::Base(top.epic.clone()),
        }
    }
}

/// SCALE-28: the roll-ups as stored ([`Rollups::stored`]): a row per
/// shortcode, one per cross top, one per mark.
pub(super) type Stored = (BTreeMap<String, GroupRow>, BTreeMap<String, CrossRow>, BTreeSet<(Mark, String)>);

/// The groups drawn before a reshaping write, with their depth, to
/// diff the settled shape against.
type Snapshot = BTreeMap<String, (Group, usize)>;

/// What [`Rollups::reparenting`] decided before the rows changed, for
/// [`Rollups::reparented`] to finish once they have.
pub(super) struct Reparenting {
    before: Snapshot,
    /// Set when the moved ticket was on a hand-edited parent cycle:
    /// moving it frees the rest of the cycle, so everything is rebuilt.
    rebuild: Option<String>,
}

/// SCALE-28: `Clone` — a write works out what the roll-ups become on a
/// copy, before it commits, so it can store the rows that differ in the
/// same transaction ([`Rollups::changes`]); the cache takes the copy once
/// the commit has landed. A copy is one entry per shortcode and per cross
/// top: the cost of `groups`, not of the tickets.
#[derive(Clone)]
pub(super) struct Rollups {
    /// In shortcode order — plan()'s order, which decides between two
    /// sub-shortcodes that would nest inside each other.
    shortcodes: BTreeMap<String, Shortcode>,
    cross: HashMap<String, Cross>,
    /// Tickets on a parent cycle (hand-edited only; almost always empty).
    on_cycle: HashSet<String>,
    /// Ids named as a parent that name no ticket (hand-edited only).
    dangling: HashSet<String>,
}

impl Rollups {
    /// Rolls up every ticket once — at load, and after a seed — in one
    /// pass in the store's order, then [`Self::settle`].
    ///
    /// A ticket's top is its parent's top (or itself), so the work per
    /// ticket is its parent's: checking the parent's chain for a cycle
    /// and walking it to its top. Most tickets share their parent with
    /// the ticket before them — since MNG-14, their shortcode's root —
    /// so that is done once per run of siblings, not once per ticket,
    /// and a shortcode's tickets, being contiguous, are pooled into one
    /// entry as they pass.
    pub(super) fn build(rows: Rows<'_>) -> Self {
        let mut r = Self {
            shortcodes: BTreeMap::new(),
            cross: HashMap::new(),
            on_cycle: HashSet::new(),
            dangling: HashSet::new(),
        };
        type Top<'a> = (&'a Row, Option<&'a Row>);
        let mut sibling: Option<(&Row, Top<'_>)> = None;
        let mut here: Option<(&str, Shortcode)> = None;
        for t in rows.tickets() {
            if here.as_ref().is_none_or(|(epic, _)| *epic != t.epic) {
                if let Some((epic, s)) = here.take() {
                    r.shortcodes.insert(epic.to_string(), s);
                }
                here = Some((&t.epic, Shortcode::default()));
            }
            let parent = match t.parent.as_deref() {
                None => None,
                Some(p) => match sibling {
                    Some((last, top)) if last.id == p => Some((last, top)),
                    _ => match rows.get(p) {
                        None => {
                            r.dangling.insert(p.to_string());
                            None
                        }
                        Some(parent) => {
                            // Any cycle through `t` passes through its
                            // parent: tracing from there finds it before
                            // anything is placed by it.
                            r.trace(rows, parent);
                            sibling = Some((parent, r.top(rows, parent)));
                            sibling
                        }
                    },
                },
            };
            // `top`, unrolled by one step around the memo.
            let (top, exit) = match parent {
                Some(_) if r.on_cycle.contains(t.id.as_str()) => (t, None),
                Some((p, _)) if p.epic != t.epic => (t, Some(p)),
                Some((_, top)) => top,
                None => (t, None),
            };
            let tally = Tally::of(t);
            let s = &mut here.as_mut().expect("entered above").1;
            if exit.is_some() {
                match r.cross.get_mut(&top.id) {
                    Some(c) => c.tally.shift(tally, true),
                    None => {
                        r.cross.insert(
                            top.id.clone(),
                            Cross {
                                tally,
                                home: String::new(),
                            },
                        );
                    }
                }
            } else {
                // A tree never leaves its shortcode: the top's is t's.
                s.base.shift(tally, true);
                if top.id == t.id {
                    s.base_tops += 1;
                }
            }
            // (epic, seq) order: the first scope met is the lowest-seq.
            // SDLC-11: or doc — a doc collection's root.
            if t.kind.roots_a_shortcode() && s.root.is_none() {
                s.root = Some(t.id.clone());
            }
        }
        if let Some((epic, s)) = here {
            r.shortcodes.insert(epic.to_string(), s);
        }
        r.settle(rows);
        r
    }

    /// Brent's cycle finding along `from`'s chain of parents: a cycle met
    /// joins `on_cycle`. `create` and `set_parent` refuse to make one and
    /// the AGS-19 backfill skips one, so only a database edited by hand
    /// holds any; plan() draws each member as a root of its shortcode,
    /// and so do the roll-ups. No allocation, and O(the chain) when — as
    /// always outside a hand edit — there is no cycle to find.
    fn trace(&mut self, rows: Rows<'_>, from: &Row) {
        fn up<'a>(rows: Rows<'a>, t: &Row) -> Option<&'a Row> {
            t.parent.as_deref().and_then(|p| rows.get(p))
        }
        let (mut power, mut length) = (1usize, 1usize);
        let mut tortoise = from;
        let Some(mut hare) = up(rows, from) else {
            return;
        };
        while !std::ptr::eq(tortoise, hare) {
            if power == length {
                tortoise = hare;
                power *= 2;
                length = 0;
            }
            match up(rows, hare) {
                Some(next) => hare = next,
                None => return,
            }
            length += 1;
        }
        // `hare` is on a cycle of `length` tickets.
        if self.on_cycle.contains(hare.id.as_str()) {
            return;
        }
        let mut x = hare;
        for _ in 0..length {
            self.on_cycle.insert(x.id.clone());
            x = up(rows, x).expect("on the cycle");
        }
    }

    /// SCALE-28: from what is stored — at start, instead of [`Self::build`].
    /// Every field is stored as kept, the settled shape included (host, the
    /// group a row is drawn inside, each roll-up), so nothing is settled
    /// again: what was kept is what is read.
    pub(super) fn from_stored(
        groups: BTreeMap<String, GroupRow>,
        cross: BTreeMap<String, CrossRow>,
        marks: BTreeSet<(Mark, String)>,
    ) -> Self {
        let tally = Tally::from_stored;
        let mut r = Self {
            shortcodes: groups
                .into_iter()
                .map(|(epic, g)| {
                    let s = Shortcode {
                        root: g.root,
                        base: tally(g.base),
                        base_tops: g.base_tops,
                        host: g.host,
                        up: g.up,
                        own: tally(g.own),
                        roll: tally(g.roll),
                    };
                    (epic, s)
                })
                .collect(),
            cross: cross
                .into_iter()
                .map(|(id, c)| (id, Cross { tally: tally(c.tally), home: c.home }))
                .collect(),
            on_cycle: HashSet::new(),
            dangling: HashSet::new(),
        };
        for (mark, id) in marks {
            match mark {
                Mark::Cycle => r.on_cycle.insert(id),
                Mark::Dangling => r.dangling.insert(id),
            };
        }
        r
    }

    /// SCALE-28: as stored — a row per shortcode, one per cross top, one per
    /// mark.
    pub(super) fn stored(&self) -> Stored {
        let groups = self.shortcodes.iter().map(|(e, s)| (e.clone(), Self::row(s))).collect();
        let cross = self.cross.iter().map(|(id, c)| (id.clone(), Self::cross_row(c))).collect();
        let marks = self
            .on_cycle
            .iter()
            .map(|id| (Mark::Cycle, id.clone()))
            .chain(self.dangling.iter().map(|id| (Mark::Dangling, id.clone())))
            .collect();
        (groups, cross, marks)
    }

    fn row(s: &Shortcode) -> GroupRow {
        GroupRow {
            root: s.root.clone(),
            host: s.host.clone(),
            up: s.up.clone(),
            base: s.base.stored(),
            base_tops: s.base_tops,
            own: s.own.stored(),
            roll: s.roll.stored(),
        }
    }

    fn cross_row(c: &Cross) -> CrossRow {
        CrossRow {
            tally: c.tally.stored(),
            home: c.home.clone(),
        }
    }

    /// SCALE-28: the stored rows that differ between these roll-ups and
    /// `after` — what a write that turns one into the other writes in its
    /// transaction. A comparison per shortcode and per cross top, and a
    /// row written only where one moved: a create writes its tree's group
    /// and the few around it, as many as its `group_changed` events (and
    /// those whose settled state moved without a drawn number moving).
    pub(super) fn changes(&self, after: &Self, into: &mut Changes) {
        for (epic, s) in &after.shortcodes {
            if self.shortcodes.get(epic) != Some(s) {
                into.groups.push((epic.clone(), Some(Self::row(s))));
            }
        }
        for epic in self.shortcodes.keys().filter(|e| !after.shortcodes.contains_key(*e)) {
            into.groups.push((epic.clone(), None));
        }
        for (id, c) in &after.cross {
            if self.cross.get(id) != Some(c) {
                into.cross.push((id.clone(), Some(Self::cross_row(c))));
            }
        }
        for id in self.cross.keys().filter(|id| !after.cross.contains_key(*id)) {
            into.cross.push((id.clone(), None));
        }
        for (mark, was, now) in [
            (Mark::Cycle, &self.on_cycle, &after.on_cycle),
            (Mark::Dangling, &self.dangling, &after.dangling),
        ] {
            into.marks.extend(now.difference(was).map(|id| ((mark, id.clone()), true)));
            into.marks.extend(was.difference(now).map(|id| ((mark, id.clone()), false)));
        }
    }

    /// How many shortcodes the store holds (drawn or not).
    pub(super) fn len(&self) -> usize {
        self.shortcodes.len()
    }

    /// `epic`'s scope root — its lowest-seq `scope` — drawn or not (SCALE-3:
    /// where `create` and `set_parent` find it).
    pub(super) fn root(&self, epic: &str) -> Option<&str> {
        self.shortcodes.get(epic)?.root.as_deref()
    }

    /// FILTER-13: the ticket `epic`'s row nests under — its settled host,
    /// drawn or not — or `None` for a top-level shortcode: what a lineage's
    /// "shortcodes hosting it" follow.
    pub(super) fn host(&self, epic: &str) -> Option<&str> {
        self.shortcodes.get(epic)?.host.as_deref()
    }

    /// AGS-23: `epic`'s row as it stands, if the nav draws one — what a
    /// write reports that moves no count but changes the root's own row.
    pub(super) fn group(&self, epic: &str) -> Option<Group> {
        self.view(epic)
    }

    /// Every drawn group, in shortcode order.
    pub(super) fn groups(&self) -> Vec<Group> {
        self.shortcodes
            .keys()
            .filter_map(|epic| self.view(epic))
            .collect()
    }

    /// A newborn (already in `rows`) is counted where it is drawn.
    pub(super) fn created(&mut self, rows: Rows<'_>, t: &Row) -> Vec<Group> {
        if self.dangling.contains(&t.id) {
            let before = self.snapshot();
            let why = format!("{} was named as a parent before it existed", t.id);
            return self.rebuild(rows, before, &why);
        }
        let parent = t.parent.as_deref().and_then(|p| rows.get(p));
        // A parent in another shortcode makes the newborn a cross top —
        // MNG-14 allows that only to a new shortcode's root, i.e. a new
        // sub-shortcode: a reshaping, settled whole.
        let before = parent
            .is_some_and(|p| p.epic != t.epic)
            .then(|| self.snapshot());
        let s = self.shortcodes.entry(t.epic.clone()).or_default();
        let new_root = t.kind.roots_a_shortcode() && s.root.is_none();
        if new_root {
            s.root = Some(t.id.clone());
        }
        let tally = Tally::of(t);
        if let Some(before) = before {
            self.cross.insert(
                t.id.clone(),
                Cross {
                    tally,
                    home: String::new(),
                },
            );
            self.settle(rows);
            return self.diff(before);
        }
        let home = self.count(rows, t, tally, true);
        if parent.is_none() {
            // Born with no parent: the top of a new tree in the pool.
            self.shortcodes
                .get_mut(&t.epic)
                .expect("entered above")
                .base_tops += 1;
        }
        let mut moved = self.carry(&home, tally, true);
        // A new root moves no count outside its own tree, but the
        // shortcode's row now names it.
        if new_root && !moved.iter().any(|g| g.epic == t.epic) {
            moved.extend(self.view(&t.epic));
        }
        moved
    }

    /// Whether a status change from `previous` to `now` moves any count:
    /// only a crossing of `Complete` does (SCALE-28: so a write that moves
    /// none copies nothing and stores nothing).
    pub(super) fn crosses(previous: Status, now: Status) -> bool {
        (now == Status::Complete) != (previous == Status::Complete)
    }

    /// A status change (already in `rows`): only a crossing of
    /// `Complete` moves anything.
    pub(super) fn status_changed(
        &mut self,
        rows: Rows<'_>,
        t: &Row,
        previous: Status,
    ) -> Vec<Group> {
        let gain = t.status == Status::Complete;
        if !Self::crosses(previous, t.status) {
            return Vec::new();
        }
        let home = self.count(rows, t, Tally::DONE, gain);
        self.carry(&home, Tally::DONE, gain)
    }

    /// A `set_parent`, before the rows change: the child's tree — the
    /// child and its descendants inside its shortcode, which the move
    /// leaves intact — is taken from the bucket it is counted in and
    /// given to the one it is joining. Settling waits for the rows.
    pub(super) fn reparenting(
        &mut self,
        rows: Rows<'_>,
        child: &Row,
        new_parent: &Row,
    ) -> Reparenting {
        let before = self.snapshot();
        if self.on_cycle.contains(&child.id) {
            return Reparenting {
                before,
                rebuild: Some(format!("{} was moved off a parent cycle", child.id)),
            };
        }
        let (top, exit) = self.top(rows, child);
        let from = Bucket::of(top, exit);
        // Under a parent in its own shortcode the child joins that
        // parent's tree (whose top is not the child: that would be the
        // cycle `set_parent` refuses); under one in another shortcode
        // it is a cross top of its own.
        let to = if new_parent.epic == child.epic {
            let (top, exit) = self.top(rows, new_parent);
            Bucket::of(top, exit)
        } else {
            Bucket::Cross(child.id.clone())
        };
        let base_top = top.id == child.id && exit.is_none();
        if from != to {
            let tally = if top.id == child.id && exit.is_some() {
                // A cross top's entry is exactly its tree.
                self.cross
                    .remove(&child.id)
                    .expect("a cross top has an entry")
                    .tally
            } else {
                let pool = &self.shortcodes[&child.epic];
                let tally = if base_top && pool.base_tops == 1 {
                    pool.base
                } else {
                    self.subtree(rows, child)
                };
                self.bucket(&from).shift(tally, false);
                tally
            };
            match &to {
                Bucket::Cross(id) if !self.cross.contains_key(id) => {
                    self.cross.insert(
                        id.clone(),
                        Cross {
                            tally,
                            home: String::new(),
                        },
                    );
                }
                _ => self.bucket(&to).shift(tally, true),
            }
        }
        if base_top {
            // It has a parent now: no longer a top of the pool.
            self.shortcodes
                .get_mut(&child.epic)
                .expect("every epic has a shortcode")
                .base_tops -= 1;
        }
        Reparenting {
            before,
            rebuild: None,
        }
    }

    /// …and once the rows have changed: settle the shape and report
    /// every group whose row moved.
    pub(super) fn reparented(&mut self, rows: Rows<'_>, r: Reparenting) -> Vec<Group> {
        if let Some(why) = r.rebuild {
            return self.rebuild(rows, r.before, &why);
        }
        self.settle(rows);
        self.diff(r.before)
    }

    /// The top of `t`'s tree — the ancestor where following `parent`
    /// leaves the shortcode, names no ticket, or meets a cycle — and,
    /// when it leaves the shortcode, the parent it leaves for.
    fn top<'a>(&self, rows: Rows<'a>, t: &'a Row) -> (&'a Row, Option<&'a Row>) {
        let mut x = t;
        // `on_cycle` is exact, so the chain ends; bounded regardless,
        // so a broken invariant is a wrong count, never a hang under
        // the write lock.
        for _ in 0..rows.len() {
            if self.on_cycle.contains(x.id.as_str()) {
                return (x, None);
            }
            match x.parent.as_deref().and_then(|p| rows.get(p)) {
                None => return (x, None),
                Some(p) if p.epic != x.epic => return (x, Some(p)),
                Some(p) => x = p,
            }
        }
        (x, None)
    }

    /// Whether plan() cuts `top`'s edge to `parent` (in another
    /// shortcode): `parent` is the host of `top`'s shortcode, so the
    /// tree is drawn in its own row rather than under the host.
    fn cut(&self, top: &Row, parent: &Row) -> bool {
        self.shortcodes
            .get(&top.epic)
            .and_then(|s| s.host.as_deref())
            == Some(parent.id.as_str())
    }

    /// The shortcode whose row `t` is drawn under: up to its tree's
    /// top, and across into the next shortcode for as long as the top
    /// nests under a ticket there.
    fn home<'a>(&self, rows: Rows<'a>, t: &'a Row) -> &'a str {
        let mut x = t;
        // Each step crosses a parent edge out of a distinct cross top,
        // and the parent graph is acyclic: this ends within that many.
        for _ in 0..=self.cross.len() {
            let (top, exit) = self.top(rows, x);
            match exit {
                Some(p) if !self.cut(top, p) => x = p,
                _ => return &top.epic,
            }
        }
        &x.epic
    }

    /// Counts `by` into (or out of) the bucket `t`'s tree is kept in,
    /// and says which group that tree is drawn in.
    fn count(&mut self, rows: Rows<'_>, t: &Row, by: Tally, gain: bool) -> String {
        let (top, exit) = self.top(rows, t);
        if exit.is_some() {
            let c = self
                .cross
                .get_mut(&top.id)
                .expect("a cross top has an entry");
            c.tally.shift(by, gain);
            return c.home.clone();
        }
        self.shortcodes
            .get_mut(&top.epic)
            .expect("every epic has a shortcode")
            .base
            .shift(by, gain);
        top.epic.clone()
    }

    fn bucket(&mut self, b: &Bucket) -> &mut Tally {
        match b {
            Bucket::Base(epic) => {
                &mut self
                    .shortcodes
                    .get_mut(epic)
                    .expect("every epic has a shortcode")
                    .base
            }
            Bucket::Cross(id) => {
                &mut self
                    .cross
                    .get_mut(id)
                    .expect("a cross top has an entry")
                    .tally
            }
        }
    }

    /// Moves `by` into (or out of) `home`'s own count and the roll-up
    /// of `home` and every group it is drawn inside; returns each as it
    /// now stands, innermost first.
    fn carry(&mut self, home: &str, by: Tally, gain: bool) -> Vec<Group> {
        let chain = self.chain(home);
        for (i, epic) in chain.iter().enumerate() {
            let s = self.shortcodes.get_mut(epic).expect("in the chain");
            if i == 0 {
                s.own.shift(by, gain);
            }
            s.roll.shift(by, gain);
        }
        chain.iter().filter_map(|epic| self.view(epic)).collect()
    }

    /// `epic`, then the group its row is drawn inside, and so on out to
    /// a top-level row.
    fn chain(&self, epic: &str) -> Vec<String> {
        let mut out = vec![epic.to_string()];
        // Acyclic by settle's nesting check; bounded regardless.
        while out.len() <= self.shortcodes.len() {
            let last = out.last().expect("starts non-empty");
            match self.shortcodes.get(last).and_then(|s| s.up.clone()) {
                Some(up) => out.push(up),
                None => break,
            }
        }
        out
    }

    /// The tally of `t`'s tree from `t` down — `t` and every descendant
    /// reached without leaving its shortcode — walking the cache's
    /// children index (SCALE-4; it was the row's `children` mirror, each
    /// entry checked against the child's own `parent` — the index is
    /// built from that field, so there is nothing left to check), one
    /// shortcode's slice of it per step.
    fn subtree<'a>(&self, rows: Rows<'a>, t: &'a Row) -> Tally {
        let mut tally = Tally::default();
        let mut stack = vec![t];
        while let Some(x) = stack.pop() {
            tally.shift(Tally::of(x), true);
            stack.extend(
                rows.children_in_shortcode(x)
                    .filter(|c| !self.on_cycle.contains(c.id.as_str())),
            );
        }
        tally
    }

    /// Re-derives the shape from the pools and the cross tops: plan()'s
    /// sub-shortcodes, where each cross tree is drawn, where each
    /// sub-shortcode's row is drawn, and every roll-up.
    fn settle(&mut self, rows: Rows<'_>) {
        for s in self.shortcodes.values_mut() {
            s.host = None;
            s.up = None;
        }
        // plan(): in shortcode order, a root whose parent is in another
        // shortcode makes its shortcode a sub-shortcode hosted there —
        // unless, with the ones accepted before it, that would draw the
        // group inside itself.
        let epics: Vec<String> = self.shortcodes.keys().cloned().collect();
        for epic in &epics {
            let Some(root) = self.shortcodes[epic]
                .root
                .as_deref()
                .and_then(|r| rows.get(r))
            else {
                continue;
            };
            // A root on a parent cycle has no parent as plan() sees it.
            if self.on_cycle.contains(&root.id) {
                continue;
            }
            let Some(host) = root.parent.as_deref().and_then(|p| rows.get(p)) else {
                continue;
            };
            if host.epic == *epic {
                continue; // ordinary nesting inside the shortcode
            }
            self.shortcodes.get_mut(epic).expect("listed").host = Some(host.id.clone());
            if self.nests_inside(rows, host, epic) {
                self.shortcodes.get_mut(epic).expect("listed").host = None;
            }
        }
        for s in self.shortcodes.values_mut() {
            s.own = s.base;
        }
        let tops: Vec<String> = self.cross.keys().cloned().collect();
        for id in &tops {
            let top = rows.get(id).expect("a cross top is a ticket");
            let home = self.home(rows, top).to_string();
            let tally = self.cross[id].tally;
            self.shortcodes
                .get_mut(&home)
                .expect("a home is a shortcode")
                .own
                .shift(tally, true);
            self.cross.get_mut(id).expect("listed").home = home;
        }
        // A sub-shortcode's row is drawn in the group holding its host.
        for epic in &epics {
            let Some(host) = self.shortcodes[epic]
                .host
                .as_deref()
                .and_then(|h| rows.get(h))
            else {
                continue;
            };
            let up = self.home(rows, host).to_string();
            self.shortcodes.get_mut(epic).expect("listed").up = Some(up);
        }
        for s in self.shortcodes.values_mut() {
            s.roll = s.own;
        }
        for epic in &epics {
            let own = self.shortcodes[epic].own;
            if own.is_empty() {
                continue;
            }
            for up in self.chain(epic).into_iter().skip(1) {
                self.shortcodes
                    .get_mut(&up)
                    .expect("in the chain")
                    .roll
                    .shift(own, true);
            }
        }
    }

    /// plan()'s `nests_inside`: would drawing `epic`'s row under `host`
    /// put the group inside itself? Walks the drawn tree up from
    /// `host`, a tree top at a time; meeting a row of `epic` is the
    /// loop.
    fn nests_inside<'a>(&self, rows: Rows<'a>, host: &'a Row, epic: &str) -> bool {
        let mut cur = host;
        let mut seen: HashSet<&str> = HashSet::new();
        loop {
            if !seen.insert(cur.id.as_str()) {
                return true; // a loop that cannot be drawn: refuse
            }
            let (top, exit) = self.top(rows, cur);
            match exit {
                Some(p) if !self.cut(top, p) => cur = p,
                _ => {
                    if top.epic == epic {
                        return true;
                    }
                    match self
                        .shortcodes
                        .get(&top.epic)
                        .and_then(|s| s.host.as_deref())
                        .and_then(|h| rows.get(h))
                    {
                        Some(h) => cur = h,
                        None => return false,
                    }
                }
            }
        }
    }

    /// The group `epic`'s row shows, or `None` when the nav draws no
    /// row for it (every one of its tickets is drawn under another
    /// shortcode's). SDLC-11: a row of docs alone is a row.
    fn view(&self, epic: &str) -> Option<Group> {
        let s = self.shortcodes.get(epic)?;
        (!s.own.is_empty()).then(|| Group {
            epic: epic.to_string(),
            total: s.roll.total,
            done: s.roll.done,
            docs: s.roll.docs,
            root: s.root.clone(),
            // AGS-23: the cache dresses it (`Cache::dress`) — the
            // roll-ups keep no ticket's fields.
            root_row: None,
            host: s.host.clone(),
        })
    }

    fn snapshot(&self) -> Snapshot {
        self.shortcodes
            .keys()
            .filter_map(|epic| {
                let group = self.view(epic)?;
                Some((epic.clone(), (group, self.chain(epic).len())))
            })
            .collect()
    }

    /// Every group whose row differs from `before` — new rows included,
    /// and a row that is no longer drawn as one last `total: 0` —
    /// innermost first (as the incremental paths report), then in
    /// shortcode order.
    fn diff(&self, before: Snapshot) -> Vec<Group> {
        let mut moved: Vec<(usize, Group)> = Vec::new();
        for (epic, s) in &self.shortcodes {
            match (before.get(epic), self.view(epic)) {
                (Some((was, _)), Some(now)) if *was == now => {}
                (_, Some(now)) => moved.push((self.chain(epic).len(), now)),
                (Some((_, depth)), None) => moved.push((
                    *depth,
                    Group {
                        epic: epic.clone(),
                        total: 0,
                        done: 0,
                        docs: 0,
                        root: s.root.clone(),
                        root_row: None,
                        host: None,
                    },
                )),
                (None, None) => {}
            }
        }
        moved.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.epic.cmp(&b.1.epic)));
        moved.into_iter().map(|(_, group)| group).collect()
    }

    fn rebuild(&mut self, rows: Rows<'_>, before: Snapshot, why: &str) -> Vec<Group> {
        tracing::warn!(
            "roll-ups rebuilt from all {} tickets: {why} (a database edited by hand)",
            rows.len()
        );
        *self = Self::build(rows);
        self.diff(before)
    }
}
