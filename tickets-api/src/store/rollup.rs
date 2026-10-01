use super::{Group, Kind, Rows, Status, Ticket};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Tickets counted: how many, and how many `Complete` (the nav's
/// `done` — `Superseded` is not done).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Tally {
    total: u64,
    done: u64,
}

impl Tally {
    /// One completion, gained or lost — all a status change moves.
    const DONE: Self = Self { total: 0, done: 1 };

    fn of(t: &Ticket) -> Self {
        Self {
            total: 1,
            done: u64::from(t.status == Status::Complete),
        }
    }

    fn shift(&mut self, by: Tally, gain: bool) {
        if gain {
            self.total += by.total;
            self.done += by.done;
        } else {
            self.total -= by.total;
            self.done -= by.done;
        }
    }
}

/// One shortcode's half of the roll-up.
#[derive(Debug, Default)]
struct Shortcode {
    /// Its lowest-seq `scope` ticket. Fixed once set: kinds never
    /// change and every later ticket has a higher seq.
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
    /// plus the cross trees homed here. Zero means the nav draws no
    /// row for this shortcode.
    own: Tally,
    /// `own` plus every nested group's — the row's `done/total`.
    roll: Tally,
}

/// A cross top's tree: its tally, and the group it is drawn in.
#[derive(Debug)]
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
    fn of(top: &Ticket, exit: Option<&Ticket>) -> Self {
        match exit {
            Some(_) => Self::Cross(top.id.clone()),
            None => Self::Base(top.epic.clone()),
        }
    }
}

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
        type Top<'a> = (&'a Ticket, Option<&'a Ticket>);
        let mut sibling: Option<(&Ticket, Top<'_>)> = None;
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
            if t.kind == Kind::Scope && s.root.is_none() {
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
    fn trace(&mut self, rows: Rows<'_>, from: &Ticket) {
        fn up<'a>(rows: Rows<'a>, t: &Ticket) -> Option<&'a Ticket> {
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

    /// How many shortcodes the store holds (drawn or not).
    pub(super) fn len(&self) -> usize {
        self.shortcodes.len()
    }

    /// Every drawn group, in shortcode order.
    pub(super) fn groups(&self) -> Vec<Group> {
        self.shortcodes
            .keys()
            .filter_map(|epic| self.view(epic))
            .collect()
    }

    /// A newborn (already in `rows`) is counted where it is drawn.
    pub(super) fn created(&mut self, rows: Rows<'_>, t: &Ticket) -> Vec<Group> {
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
        let new_root = t.kind == Kind::Scope && s.root.is_none();
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

    /// A status change (already in `rows`): only a crossing of
    /// `Complete` moves anything.
    pub(super) fn status_changed(
        &mut self,
        rows: Rows<'_>,
        t: &Ticket,
        previous: Status,
    ) -> Vec<Group> {
        let gain = t.status == Status::Complete;
        if gain == (previous == Status::Complete) {
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
        child: &Ticket,
        new_parent: &Ticket,
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
    fn top<'a>(&self, rows: Rows<'a>, t: &'a Ticket) -> (&'a Ticket, Option<&'a Ticket>) {
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
    fn cut(&self, top: &Ticket, parent: &Ticket) -> bool {
        self.shortcodes
            .get(&top.epic)
            .and_then(|s| s.host.as_deref())
            == Some(parent.id.as_str())
    }

    /// The shortcode whose row `t` is drawn under: up to its tree's
    /// top, and across into the next shortcode for as long as the top
    /// nests under a ticket there.
    fn home<'a>(&self, rows: Rows<'a>, t: &'a Ticket) -> &'a str {
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
    fn count(&mut self, rows: Rows<'_>, t: &Ticket, by: Tally, gain: bool) -> String {
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
    /// reached without leaving its shortcode — walking the store's
    /// `children` mirror, each entry checked against the child's own
    /// `parent`.
    fn subtree<'a>(&self, rows: Rows<'a>, t: &'a Ticket) -> Tally {
        let mut tally = Tally::default();
        let mut stack = vec![t];
        while let Some(x) = stack.pop() {
            tally.shift(Tally::of(x), true);
            stack.extend(x.children.iter().filter_map(|c| rows.get(c)).filter(|c| {
                c.parent.as_deref() == Some(x.id.as_str())
                    && c.epic == x.epic
                    && !self.on_cycle.contains(c.id.as_str())
            }));
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
            if own.total == 0 {
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
    fn nests_inside<'a>(&self, rows: Rows<'a>, host: &'a Ticket, epic: &str) -> bool {
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
    /// shortcode's).
    fn view(&self, epic: &str) -> Option<Group> {
        let s = self.shortcodes.get(epic)?;
        (s.own.total > 0).then(|| Group {
            epic: epic.to_string(),
            total: s.roll.total,
            done: s.roll.done,
            root: s.root.clone(),
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
                        root: s.root.clone(),
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
