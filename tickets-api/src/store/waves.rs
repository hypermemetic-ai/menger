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
//! Nothing here is stored or kept: [`place`] reads the cache's rows under
//! the read lock its caller holds, follows blockers into any shortcode
//! whether or not it is asked for, and memoizes each placement for the rest
//! of the one call — so a call costs the tickets it yields plus their
//! blocker closure, each ticket placed once. The walk is iterative (an
//! explicit stack, no recursion limit), and terminates on a `blocked_by`
//! cycle — which `create` refuses, so only a hand edit makes one — by
//! placing every ticket on or behind the cycle `MissingBlocker`: like a
//! missing blocker, it is a wait that no status change can end.

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
        Status::Complete | Status::Superseded | Status::Doc => 7,
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
#[derive(Clone, Copy, Default)]
struct Behind {
    /// The highest wave among them, 0 with none.
    highest: u32,
    /// One of them is no ticket, or is `MissingBlocker` itself.
    missing: bool,
}

impl Behind {
    fn take(&mut self, p: Placement) {
        match p {
            Placement::Wave { n } => self.highest = self.highest.max(n),
            Placement::MissingBlocker => self.missing = true,
            // An unsatisfied blocker is never done (`Done` *is* satisfied).
            Placement::Done => {}
        }
    }

    fn placement(self) -> Placement {
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
pub(super) fn waves(rows: Rows<'_>, epics: &BTreeSet<String>, kinds: &[Kind]) -> Waves {
    let mut memo: HashMap<&str, Placement> = HashMap::new();
    // (lane, wave, rank) — the sort key; the walk is already in the store's
    // order, and the sort is stable, so that is the tie-break.
    let mut keyed: Vec<((u8, u32, u8), Placed)> = Vec::new();
    for epic in epics {
        let first = OrderKey { epic: epic.clone(), seq: 0 };
        let last = OrderKey { epic: epic.clone(), seq: u32::MAX };
        for t in rows.order.range(first..=last).map(|(_, t)| t) {
            if t.kind == Kind::Doc || !kinds.contains(&t.kind) {
                continue;
            }
            let placement = place(rows, t, &mut memo);
            let key = match placement {
                Placement::Wave { n } => (0, n, rank(t.status)),
                Placement::MissingBlocker => (1, 0, 0),
                Placement::Done => (2, 0, 0),
            };
            let waiting_on = match placement {
                Placement::Done => Vec::new(),
                _ => waiting_on(rows, t),
            };
            keyed.push((key, Placed { summary: t.clone(), placement, waiting_on }));
        }
    }
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
