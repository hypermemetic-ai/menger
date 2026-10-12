//! WAVE-4: each ticket's place in the order of work, on demand.
//!
//! A ticket's **wave** is derived from `blocked_by` alone, over the whole
//! graph, by MNG-4's rule ([`satisfied`]): unfinished with every blocker
//! satisfied is wave 1; otherwise 1 + the highest wave among its
//! unsatisfied blockers. A `Complete` / `Superseded` ticket is `Done`; one
//! that names a blocker that is no ticket — or waits, at any depth, on one
//! that does — is `MissingBlocker`. `parent` orders nothing, and a ticket's
//! own `Blocked` status is a marker on its card, not an edge.
//!
//! [`place`] is the rule: it reads the cache's rows under the read lock its
//! caller holds, follows blockers into any shortcode whether or not it is
//! asked for, and memoizes each placement for the rest of the one call — so
//! a call costs the ticket plus its blocker closure, each ticket placed
//! once. The walk is iterative (an explicit stack, no recursion limit), and
//! terminates on a `blocked_by` cycle — which `create` refuses, so only a
//! hand edit makes one — by placing every ticket on or behind the cycle
//! `MissingBlocker`: like a missing blocker, it is a wait that no status
//! change can end.
//!
//! WAVE-6: the placements are kept now, on the rows ([`super::placements`]),
//! and [`waves`] reads them as they stand — its answer unchanged, its cost
//! the yielded tickets alone; `waiting_on` is still worked out per row.
//! [`place`] stays the rule: the store places every ticket by it at open,
//! and it is what the kept placements are held equal to.

use super::{satisfied, OrderKey, Row, Rows};
use crate::types::{Kind, Placement, Status, TicketSummary};
use std::collections::{BTreeSet, HashMap, HashSet};

/// The kinds `waves` places when none are asked for — work (WAVE-1
/// decision 9; FILTER-6's built-in "work").
pub const DEFAULT_KINDS: [Kind; 3] = [Kind::Build, Kind::Spike, Kind::Gap];

/// One placed ticket: its row, where it sits, and what it waits on.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub summary: TicketSummary,
    pub placement: Placement,
    /// Its unsatisfied direct blockers, in `blocked_by` order, distinct —
    /// ids that name no ticket included; empty for `Done`.
    pub waiting_on: Vec<String>,
}

/// A whole `waves` answer: the placed tickets in yield order, then the
/// tally `wave_counts` carries.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Waves {
    pub placed: Vec<Placed>,
    /// `waves[i]`: how many of `placed` sit in wave `i + 1`.
    pub waves: Vec<u64>,
    pub missing: u64,
    pub done: u64,
}

/// Where a status sorts inside a wave: the work under way first, then
/// what can start, then what waits (WAVE-1 decision 10). `Complete`,
/// `Superseded` and `Doc` are never in a wave; they sort last if asked.
fn rank(status: Status) -> u8 {
    match status {
        Status::Coding => 0,
        Status::InReview => 1,
        Status::Ready => 2,
        Status::Pending => 3,
        Status::Blocked => 4,
        Status::Idea => 5,
        Status::Epic => 6,
        Status::Complete | Status::Superseded | Status::Canceled | Status::Doc => 7,
    }
}

/// `t`'s unsatisfied direct blockers: each distinct id of its `blocked_by`,
/// in order, that is not a `Complete` or `Superseded` ticket.
fn waiting_on(rows: Rows<'_>, t: &Row) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for b in &t.blocked_by {
        if out.iter().any(|o| o == b) || rows.get(b).is_some_and(|r| satisfied(r.status)) {
            continue;
        }
        out.push(b.clone());
    }
    out
}

/// What a ticket's unsatisfied blockers come to, gathered one at a time.
/// WAVE-6: the kept placements' pass gathers with it too.
#[derive(Clone, Copy, Default)]
pub(super) struct Behind {
    /// The highest wave among them, 0 with none.
    highest: u32,
    /// One of them is no ticket, or is `MissingBlocker` itself.
    pub(super) missing: bool,
}

impl Behind {
    pub(super) fn take(&mut self, p: Placement) {
        match p {
            Placement::Wave { n } => self.highest = self.highest.max(n),
            Placement::MissingBlocker => self.missing = true,
            // An unsatisfied blocker is never done (`Done` *is* satisfied).
            Placement::Done => {}
        }
    }

    pub(super) fn placement(self) -> Placement {
        if self.missing {
            Placement::MissingBlocker
        } else {
            Placement::Wave {
                n: self.highest.saturating_add(1),
            }
        }
    }
}

/// One ticket on the walk's stack: the next of its `blocked_by` to look at,
/// and what the ones looked at so far come to.
struct Frame<'a> {
    row: &'a Row,
    next: usize,
    behind: Behind,
}

/// `t`'s placement, every placement the walk works out on the way kept in
/// `memo` (by id) for the rest of the call. Iterative: a blocker not yet
/// placed is pushed and placed first, so a chain of any length costs a
/// stack of rows, never a stack of calls.
pub(super) fn place<'a>(rows: Rows<'a>, t: &'a Row, memo: &mut HashMap<&'a str, Placement>) -> Placement {
    if satisfied(t.status) {
        return Placement::Done;
    }
    if let Some(p) = memo.get(t.id.as_str()) {
        return *p;
    }
    let mut stack = vec![Frame { row: t, next: 0, behind: Behind::default() }];
    // The ids on the stack — a blocker found among them closes a cycle.
    let mut open: HashSet<&'a str> = HashSet::from([t.id.as_str()]);
    loop {
        let frame = stack.last_mut().expect("the walk ends when its first frame is placed");
        if let Some(b) = frame.row.blocked_by.get(frame.next) {
            frame.next += 1;
            match rows.get(b) {
                None => frame.behind.missing = true,
                Some(r) if satisfied(r.status) => {}
                Some(r) => {
                    if let Some(p) = memo.get(r.id.as_str()) {
                        frame.behind.take(*p);
                    } else if open.contains(r.id.as_str()) {
                        frame.behind.missing = true;
                    } else {
                        open.insert(r.id.as_str());
                        stack.push(Frame { row: r, next: 0, behind: Behind::default() });
                    }
                }
            }
            continue;
        }
        let done = stack.pop().expect("a frame was in hand");
        let p = done.behind.placement();
        memo.insert(done.row.id.as_str(), p);
        open.remove(done.row.id.as_str());
        match stack.last_mut() {
            Some(below) => below.behind.take(p),
            None => return p,
        }
    }
}

/// The `waves` answer for the shortcodes `epics` (upper case) and the kinds
/// `kinds` — never a doc, whatever is asked: a doc has no lifecycle, so no
/// place in the order of work. The yielded tickets are those shortcodes'
/// tickets of those kinds; their blockers are followed wherever they are.
///
/// Yield order: every `Wave` by `n`, inside a wave by [`rank`] and then the
/// store's order `(epic, seq)`; then every `MissingBlocker`; then every
/// `Done` — both in the store's order.
///
/// WAVE-6: each ticket's placement is the one the store keeps on its row; a
/// row that has none kept (none the store makes: only a doc has none, and a
/// doc is never yielded) is placed by the rule, as before.
pub(super) fn waves(rows: Rows<'_>, epics: &BTreeSet<String>, kinds: &[Kind]) -> Waves {
    let tickets = epics.iter().flat_map(|epic| {
        let first = OrderKey { epic: epic.clone(), seq: 0 };
        let last = OrderKey { epic: epic.clone(), seq: u32::MAX };
        rows.order.range(first..=last).map(|(_, t)| t)
    });
    of(rows, tickets, kinds, &|_| true)
}

/// The sort key of a placed ticket: (lane, wave, rank, last) — `last` puts
/// a `waves_to` target after everything else in its lane. The walks are in
/// the store's order and the sort is stable, so that is the tie-break.
type Key = (u8, u32, u8, bool);

fn key(placement: Placement, status: Status, last: bool) -> Key {
    match placement {
        Placement::Wave { n } => (0, n, rank(status), last),
        Placement::MissingBlocker => (1, 0, 0, last),
        Placement::Done => (2, 0, 0, last),
    }
}

fn placed(rows: Rows<'_>, t: &Row, placement: Placement) -> Placed {
    let waiting_on = match placement {
        Placement::Done => Vec::new(),
        _ => waiting_on(rows, t),
    };
    Placed { summary: t.clone(), placement, waiting_on }
}

/// MENGER-31: the `waves` answer for the tickets `tickets` (in the store's
/// order) of the kinds `kinds` that `admit` lets through — never a doc. The
/// rule, the order and the tally are [`waves`]': each ticket where the store
/// keeps it. `waves_under` is this over a ticket's subtree.
pub(super) fn of<'a>(rows: Rows<'a>, tickets: impl Iterator<Item = &'a Row>, kinds: &[Kind], admit: &dyn Fn(&Row) -> bool) -> Waves {
    let mut memo: HashMap<&str, Placement> = HashMap::new();
    let mut keyed: Vec<(Key, Placed)> = Vec::new();
    for t in tickets {
        if t.kind == Kind::Doc || !kinds.contains(&t.kind) || !admit(t) {
            continue;
        }
        let placement = t.placement.unwrap_or_else(|| place(rows, t, &mut memo));
        keyed.push((key(placement, t.status, false), placed(rows, t, placement)));
    }
    finish(keyed)
}

/// MENGER-31: a `waves_to` answer — the wave board whose final state is one
/// ticket — and the `blocked_by` cycles among what it waits on.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WavesTo {
    pub waves: Waves,
    /// Each cycle among the open tickets of the answer (a strongly connected
    /// set of two or more, or one that names itself): its ids in the store's
    /// order; the cycles in the order of their first id.
    pub cycles: Vec<Vec<String>>,
}

/// Whether a ticket's open descendants are part of what it is: a scope's
/// or an execution's are (the work it is made of); a build's are not.
fn gathers(kind: Kind) -> bool {
    matches!(kind, Kind::Scope | Kind::Execution)
}

/// MENGER-31: what must happen before `target` — its open descendants when
/// it is a scope or an execution (`descendants`, in the store's order), and
/// every ticket that it or any of those waits on, transitively, over
/// `blocked_by`. A done blocker is in the answer, `Done`, and not followed
/// past (what it waited on holds nothing back now); an id that names no
/// ticket is followed nowhere (its waiter is `missing_blocker`).
///
/// Each ticket is placed where the store keeps it — every blocker of an open
/// one is in the set, so its kept wave is its wave here too (no second wave
/// rule); the target is alone in the last wave: one past the highest wave
/// of the rest, `missing_blocker` when anything it waits on is, `Done` when
/// it is done (and then nothing else is in the answer). `kinds` filters what
/// is yielded, not what is followed; the target is always yielded. The
/// yield order is [`waves`]', the target last in its lane. `admit` keeps a
/// ticket out of the answer (it is still followed and still counts).
pub(super) fn to<'a>(
    rows: Rows<'a>,
    target: &'a Row,
    descendants: impl Iterator<Item = &'a Row>,
    kinds: &[Kind],
    admit: &dyn Fn(&Row) -> bool,
) -> WavesTo {
    let mut members: Vec<&'a Row> = vec![target];
    let mut seen: HashSet<&'a str> = HashSet::from([target.id.as_str()]);
    let mut queue: Vec<&'a Row> = Vec::new();
    if !satisfied(target.status) {
        queue.push(target);
        if gathers(target.kind) {
            for d in descendants {
                if d.kind != Kind::Doc && !satisfied(d.status) && seen.insert(d.id.as_str()) {
                    members.push(d);
                    queue.push(d);
                }
            }
        }
    }
    let mut at = 0;
    while let Some(&t) = queue.get(at) {
        at += 1;
        for b in &t.blocked_by {
            let Some(r) = rows.get(b) else { continue };
            if r.kind == Kind::Doc || !seen.insert(r.id.as_str()) {
                continue;
            }
            members.push(r);
            if !satisfied(r.status) {
                queue.push(r);
            }
        }
    }

    // The store's order, the tie-break inside a lane.
    members[1..].sort_by(|a, b| (a.epic.as_str(), a.seq).cmp(&(b.epic.as_str(), b.seq)));
    let mut memo: HashMap<&str, Placement> = HashMap::new();
    let mut highest = 0u32;
    let mut missing = target.blocked_by.iter().any(|b| rows.get(b).is_none());
    let mut keyed: Vec<(Key, Placed)> = Vec::new();
    for t in members.iter().skip(1) {
        let placement = t.placement.unwrap_or_else(|| place(rows, t, &mut memo));
        match placement {
            Placement::Wave { n } => highest = highest.max(n),
            Placement::MissingBlocker => missing = true,
            Placement::Done => {}
        }
        if kinds.contains(&t.kind) && admit(t) {
            keyed.push((key(placement, t.status, false), placed(rows, t, placement)));
        }
    }
    let last = if satisfied(target.status) {
        Placement::Done
    } else if missing {
        Placement::MissingBlocker
    } else {
        Placement::Wave { n: highest.saturating_add(1) }
    };
    keyed.push((key(last, target.status, true), placed(rows, target, last)));

    let open: Vec<&'a Row> = members.iter().copied().filter(|t| !satisfied(t.status)).collect();
    WavesTo { waves: finish(keyed), cycles: cycles(&open) }
}

/// The cycles among `open` over `blocked_by` (an edge to a ticket outside it
/// ignored): Tarjan's strongly connected components, iteratively — each of
/// two or more, or of one that names itself — ids in the store's order.
fn cycles(open: &[&Row]) -> Vec<Vec<String>> {
    let ix: HashMap<&str, usize> = open.iter().enumerate().map(|(i, t)| (t.id.as_str(), i)).collect();
    let edges: Vec<Vec<usize>> =
        open.iter().map(|t| t.blocked_by.iter().filter_map(|b| ix.get(b.as_str()).copied()).collect()).collect();
    let n = open.len();
    let unvisited = usize::MAX;
    let (mut index, mut low, mut on_stack) = (vec![unvisited; n], vec![0usize; n], vec![false; n]);
    let mut stack: Vec<usize> = Vec::new();
    let mut next = 0usize;
    let mut found: Vec<Vec<usize>> = Vec::new();
    for root in 0..n {
        if index[root] != unvisited {
            continue;
        }
        // Each node on the walk, and the next of its edges to look at.
        let mut work: Vec<(usize, usize)> = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(top) = work.last_mut() {
            let v = top.0;
            if let Some(&w) = edges[v].get(top.1) {
                top.1 += 1;
                if index[w] == unvisited {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(u, _)) = work.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] == index[v] {
                let mut scc = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    scc.push(w);
                    if w == v {
                        break;
                    }
                }
                if scc.len() > 1 || edges[v].contains(&v) {
                    found.push(scc);
                }
            }
        }
    }
    let order = |i: usize| (open[i].epic.as_str(), open[i].seq);
    for scc in &mut found {
        scc.sort_by_key(|i| order(*i));
    }
    found.sort_by_key(|scc| order(scc[0]));
    found.into_iter().map(|scc| scc.into_iter().map(|i| open[i].id.clone()).collect()).collect()
}

/// The keyed tickets in yield order, and their tally.
fn finish(mut keyed: Vec<(Key, Placed)>) -> Waves {
    keyed.sort_by_key(|(key, _)| *key);

    let mut out = Waves::default();
    for (_, placed) in keyed {
        match placed.placement {
            Placement::Wave { n } => {
                let i = n as usize - 1;
                if out.waves.len() <= i {
                    out.waves.resize(i + 1, 0);
                }
                out.waves[i] += 1;
            }
            Placement::MissingBlocker => out.missing += 1,
            Placement::Done => out.done += 1,
        }
        out.placed.push(placed);
    }
    out
}
