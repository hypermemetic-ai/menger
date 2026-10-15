//! WAVE-6: every ticket's placement, kept.
//!
//! WAVE-4 placed tickets on demand ([`super::waves`]); the owner wants them
//! precomputed — "every additional ticket should trigger an update" (WAVE-1
//! decision 9). So each non-doc row carries its [`Placement`], equal at all
//! times to WAVE-4's rule over the whole graph, and the writes that can
//! move one keep it:
//!
//! - **a birth** — the newborn is placed from its blockers, and a forward
//!   reference to its id that it resolves moves the tickets that named it
//!   (they were `missing_blocker`) and theirs;
//! - **a status change across `Complete` / `Superseded`** — the ticket goes
//!   to `done` or back to a wave, and its dependents (found from `unlocks`,
//!   the mirror the store keeps of who names it — never a scan) may move.
//!
//! - SDLC-13: **a kind change across the doc line** — a doc made work is
//!   placed (it has no edges, so nothing downstream moves), work made a doc
//!   is placed nowhere (it is refused while in the graph) — or with a status
//!   across `Complete` / `Superseded`, as a status change.
//!
//! Nothing else moves one: `blocked_by` is fixed at birth, a status change
//! that stays on one side of the line changes no placement, and a move, an
//! amend or a link touches neither.
//!
//! [`replace`] is that write's pass, run before it commits, over the rows as
//! the write will leave them ([`Rows::with`]), under the write-serial lock.
//! It re-places the written ticket, then walks downstream along `unlocks`,
//! re-placing a ticket only when one of its blockers moved and going on past
//! it only when it moved too — it stops where nothing changes. The order
//! matters: a ticket must be placed after every blocker of it that moves, or
//! it reads a stale wave. The kept waves are that order: a ticket's wave is
//! higher than each of its unsatisfied blockers', and a status change only
//! removes waits (a completion) or adds ones that leave the written ticket
//! (a reopening — and the written ticket goes first). So the walk takes the
//! tickets in the order of their kept waves, lowest first, from a heap; the
//! `missing_blocker` lane has no wave, and comes last — nothing that waits on
//! a missing ticket is anything but missing — and is placed by pulling: a
//! blocker kept missing and not yet re-placed is re-placed first, by WAVE-4's
//! own walk (iterative, and a `blocked_by` cycle is missing, as there).
//!
//! What only a hand edit makes — a `blocked_by` cycle that a reopening closes
//! (`create` refuses one), a doc in the graph (`create` refuses that too) —
//! the walk notices (a ticket already re-placed has a blocker move under it;
//! a doc waits on a ticket that moved) and gives way to a pass over the whole
//! store with WAVE-4's rule, which is right by definition. Logged.
//!
//! At open the placements are computed from the rows ([`place_all`]): one
//! pass of WAVE-4's walk over every ticket, memoized — nothing is stored, so
//! nothing can be stale after a restart (WAVE-3 measures what it costs).

use super::waves::{place, Behind};
use super::{satisfied, OrderKey, Row, Rows};
use crate::address::ShortcodeId;
use crate::types::{Placement, PlacementChange};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// One ticket whose kept placement a write moves.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Shift {
    pub(super) key: OrderKey,
    pub(super) id: String,
    pub(super) placement: Placement,
}

impl Shift {
    /// As `placements_changed` carries it, with the shortcode `watch { epic
    /// }` filters it by.
    pub(super) fn change(&self) -> (ShortcodeId, PlacementChange) {
        (
            self.key.shortcode.clone(),
            PlacementChange {
                id: self.id.clone(),
                placement: self.placement,
            },
        )
    }
}

/// What one write's pass worked out.
#[derive(Debug, Default)]
pub(super) struct Replaced {
    /// The written ticket's placement as the write leaves it — `None` for a
    /// doc.
    pub(super) placement: Option<Placement>,
    /// Every placement the write moves, in the order the walk settled them:
    /// the written ticket's first (but a newborn's, which is on its
    /// `created`), then each one downstream that moved.
    pub(super) shifts: Vec<Shift>,
    /// How many tickets the pass placed — the written one included — for
    /// WAVE-3's measurements and the tests' check that it stops.
    pub(super) placed: usize,
    /// The walk gave way to a pass over the whole store (a hand edit).
    pub(super) whole: bool,
}

/// Where a kept placement sorts the walk: its wave; a missing ticket after
/// every wave; the written ticket (`done` kept, or none yet) first.
fn rank(p: Option<Placement>) -> u64 {
    match p {
        Some(Placement::Wave { n }) => u64::from(n),
        Some(Placement::MissingBlocker) => u64::MAX,
        Some(Placement::Done) | None => 0,
    }
}

#[cfg(test)]
thread_local! {
    /// How many writes on this thread gave way to the whole store — what the
    /// property test holds at zero for everything the store itself makes.
    pub(super) static WHOLE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Why the walk gave way to a pass over the whole store.
#[derive(Debug)]
struct Whole(&'static str);

/// One frame of a pull: a ticket, the next of its `blocked_by` to look at,
/// and what its unsatisfied blockers come to so far.
struct Frame<'a> {
    row: &'a Row,
    next: usize,
    behind: Behind,
}

struct Pass<'a> {
    rows: Rows<'a>,
    born: bool,
    /// Every ticket the walk has placed, with its placement as the write
    /// leaves it.
    new: HashMap<&'a str, Placement>,
    /// The tickets waiting to be placed, lowest kept rank first, then in the
    /// order they were found.
    heap: BinaryHeap<Reverse<(u64, u64, usize)>>,
    queued: Vec<&'a Row>,
    shifts: Vec<Shift>,
}

impl<'a> Pass<'a> {
    fn push(&mut self, r: &'a Row) {
        let at = self.queued.len() as u64;
        self.heap.push(Reverse((rank(r.placement), at, self.queued.len())));
        self.queued.push(r);
    }

    /// `r` is placed at `p`: kept, and — when that moves it — recorded, and
    /// every ticket that waits on it queued.
    fn settle(&mut self, r: &'a Row, p: Placement, written: bool) -> Result<(), Whole> {
        self.new.insert(r.id.as_str(), p);
        if !written && r.placement == Some(p) {
            return Ok(());
        }
        if !(written && self.born) {
            self.shifts.push(Shift {
                key: OrderKey::of(r),
                id: r.id.clone(),
                placement: p,
            });
        }
        // An entry named twice (a hand edit) queues its ticket twice; the
        // second is passed over when it comes up, placed already.
        for id in &r.unlocks {
            // An entry that names no ticket, or a ticket that does not name
            // `r` (a hand edit either way), does not wait on it.
            let Some(d) = self.rows.get(id) else { continue };
            if satisfied(d.status) || !d.blocked_by.iter().any(|b| *b == r.id) {
                continue;
            }
            if !d.kind.has_lifecycle() {
                return Err(Whole("a doc waits on a ticket whose placement moved"));
            }
            if self.new.contains_key(d.id.as_str()) {
                return Err(Whole("a ticket already placed waits on one that moved after it: a blocked_by cycle"));
            }
            self.push(d);
        }
        Ok(())
    }

    /// `t`'s placement as the write leaves it. `trusted`: every kept
    /// placement it reads is right as it stands — true of the written
    /// ticket's blockers, which nothing this write does can move (they are
    /// upstream of it). Otherwise a blocker kept missing and not yet placed
    /// may be one the write moves, and is placed first (pulled), each pulled
    /// ticket settled as it is.
    fn place(&mut self, t: &'a Row, trusted: bool) -> Result<Placement, Whole> {
        if satisfied(t.status) {
            return Ok(Placement::Done);
        }
        // Most tickets need nothing pulled: every blocker is placed already
        // or kept right as it stands. One look at each, nothing allocated;
        // the walk below only when a blocker must be pulled first.
        let mut behind = Behind::default();
        let mut pull = false;
        for b in &t.blocked_by {
            match self.rows.get(b) {
                None => behind.missing = true,
                Some(r) if satisfied(r.status) => {}
                Some(r) if r.id == t.id => behind.missing = true, // names itself
                Some(r) => match (self.new.get(r.id.as_str()), r.placement) {
                    (Some(p), _) => behind.take(*p),
                    (None, Some(p @ Placement::Wave { .. })) => behind.take(p),
                    (None, Some(p @ Placement::MissingBlocker)) if trusted => behind.take(p),
                    _ => {
                        pull = true;
                        break;
                    }
                },
            }
        }
        if !pull {
            return Ok(behind.placement());
        }
        let mut stack = vec![Frame {
            row: t,
            next: 0,
            behind: Behind::default(),
        }];
        let mut open: HashSet<&'a str> = HashSet::from([t.id.as_str()]);
        loop {
            let frame = stack.last_mut().expect("the walk ends when its first frame is placed");
            if let Some(b) = frame.row.blocked_by.get(frame.next) {
                frame.next += 1;
                match self.rows.get(b) {
                    None => frame.behind.missing = true,
                    Some(r) if satisfied(r.status) => {}
                    Some(r) => {
                        if let Some(p) = self.new.get(r.id.as_str()) {
                            frame.behind.take(*p);
                        } else if open.contains(r.id.as_str()) {
                            // On the pull's own stack: a cycle, missing as
                            // WAVE-4 has it.
                            frame.behind.missing = true;
                        } else {
                            match r.placement {
                                Some(p @ Placement::Wave { .. }) => frame.behind.take(p),
                                Some(p @ Placement::MissingBlocker) if trusted => frame.behind.take(p),
                                Some(Placement::MissingBlocker) => {
                                    open.insert(r.id.as_str());
                                    stack.push(Frame {
                                        row: r,
                                        next: 0,
                                        behind: Behind::default(),
                                    });
                                }
                                Some(Placement::Done) => return Err(Whole("an unfinished ticket was kept done")),
                                None if !r.kind.has_lifecycle() => return Err(Whole("a ticket waits on a doc")),
                                None => return Err(Whole("a ticket had no placement kept")),
                            }
                        }
                    }
                }
                continue;
            }
            let done = stack.pop().expect("a frame was in hand");
            let p = done.behind.placement();
            open.remove(done.row.id.as_str());
            match stack.last_mut() {
                None => return Ok(p),
                Some(below) => {
                    below.behind.take(p);
                    self.settle(done.row, p, false)?;
                }
            }
        }
    }

    fn run(&mut self, written: &'a Row) -> Result<(), Whole> {
        let p = self.place(written, true)?;
        self.settle(written, p, true)?;
        while let Some(Reverse((_, _, i))) = self.heap.pop() {
            let t = self.queued[i];
            if self.new.contains_key(t.id.as_str()) {
                continue; // placed already: pulled, or queued twice
            }
            let p = self.place(t, false)?;
            self.settle(t, p, false)?;
        }
        Ok(())
    }
}

/// The re-placement pass of one write — a birth (`born`, the newborn
/// `written`) or a status change across the satisfied line (`written` with
/// its new status) — over `rows`, the rows as the write leaves them (the
/// written one among them, its kept placement still the old one). A doc is
/// placed nowhere and moves nothing.
pub(super) fn replace(rows: Rows<'_>, written: &Row, born: bool) -> Replaced {
    if !written.kind.has_lifecycle() {
        return Replaced::default();
    }
    let started = std::time::Instant::now();
    let mut pass = Pass {
        rows,
        born,
        new: HashMap::new(),
        heap: BinaryHeap::new(),
        queued: Vec::new(),
        shifts: Vec::new(),
    };
    let replaced = match pass.run(written) {
        Ok(()) => Replaced {
            placement: pass.new.get(written.id.as_str()).copied(),
            placed: pass.new.len(),
            shifts: pass.shifts,
            whole: false,
        },
        Err(Whole(why)) => {
            #[cfg(test)]
            WHOLE.with(|w| w.set(w.get() + 1));
            tracing::warn!(
                "WAVE-6: re-placing after a write to {}: {why} (only a hand edit makes one) — placing \
                 every ticket again",
                written.id
            );
            whole(rows, written, born)
        }
    };
    // Target `wave6`, not the module's: a debug line a write would be noise in
    // the server's default filter (`menger=debug`). WAVE-3 reads the fields.
    tracing::debug!(
        target: "wave6",
        placed = replaced.placed as u64,
        moved = replaced.shifts.len() as u64,
        whole = replaced.whole,
        micros = started.elapsed().as_micros() as u64,
        "WAVE-6: a write to {} placed {} ticket(s){} and moved {}",
        written.id,
        replaced.placed,
        if replaced.whole { " (the whole store)" } else { "" },
        replaced.shifts.len()
    );
    replaced
}

/// Every placement the rows as the write leaves them give, by WAVE-4's rule
/// over the whole store, against what is kept — what the walk gives way to.
fn whole(rows: Rows<'_>, written: &Row, born: bool) -> Replaced {
    let mut memo = HashMap::new();
    let mut out = Replaced {
        whole: true,
        ..Replaced::default()
    };
    for t in rows.tickets() {
        if !t.kind.has_lifecycle() {
            continue;
        }
        let p = place(rows, t, &mut memo);
        out.placed += 1;
        if t.id == written.id {
            out.placement = Some(p);
            if born {
                continue;
            }
        }
        if t.placement != Some(p) {
            out.shifts.push(Shift {
                key: OrderKey::of(t),
                id: t.id.clone(),
                placement: p,
            });
        }
    }
    out
}

/// Every ticket's placement from the rows alone — at open, and after a
/// seed: WAVE-4's walk over every ticket but the docs, one memo for the
/// whole pass, so each is placed once. In the rows' order, `None` for a doc.
pub(super) fn place_all(rows: Rows<'_>) -> Vec<Option<Placement>> {
    let mut memo = HashMap::with_capacity(rows.len());
    rows.tickets()
        .map(|t| t.kind.has_lifecycle().then(|| place(rows, t, &mut memo)))
        .collect()
}
