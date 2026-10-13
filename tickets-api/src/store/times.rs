//! FILTER-27: **the times** — when each ticket was born, and since when it
//! has held its current status: what `created <window>` and `for over|under
//! <length>` are answered from.
//!
//! Kept beside the transition log ([`super::history::Log`]), whose interned
//! shortcodes they share, as two sorted indexes over `(moment, shortcode,
//! seq)` and each ticket's entry:
//!
//! - **born** — `created_unix`, which no write moves;
//! - **since** — the moment of the ticket's latest status transition (its
//!   birth's, for a ticket born since the log began: every status write
//!   records one, and the log's append moves the entry in the same critical
//!   section); for a ticket the log says nothing of — one from before the
//!   log — its last update or, if earlier, the moment the log began (a
//!   status change since then would be on record), and never before its
//!   birth. Such an entry is a lower bound ("status since at least …"):
//!   `exact` is false.
//!
//! - **touched** (FILTER-30) — the ticket's last touch: the later of its
//!   row's last write (`updated_unix`, which every write to the row moves
//!   and nothing derived does — a roll-up, a count, a placement leaves it)
//!   and its last **activity**: a comment left on it, an answer to or a
//!   clear of one of its inputs. Activity is kept apart ([`Times::touch`]),
//!   so a row written again keeps it, and read in at open from the comments
//!   and the inputs' histories ([`Log::touch_all`](super::history::Log)).
//!
//! A window is two binary searches over the index and the tickets inside it,
//! never a scan of the tickets. Both are rebuilt from the database at open
//! ([`Times::build`], from the rows and the log just read) and checked by
//! `--verify-derived` ([`Times::check`]): each entry what the rows and the
//! log say (and, FILTER-30, the comments and answers), each index holding
//! each ticket once.

use super::history::Clock;
use crate::filter::{Held, Window};
use crate::types::{Status, TicketSummary};
use roaring::RoaringBitmap;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// One ticket's times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Timed {
    pub(crate) born: i64,
    pub(crate) since: i64,
    /// FILTER-30: its last touch — its row's last write or, if later, its
    /// last comment or answer.
    pub(crate) touched: i64,
    /// Its current status, as [`Status::ALL`]'s place.
    pub(crate) status: u8,
    /// `since` is a transition's moment (not a pre-log bound).
    pub(crate) exact: bool,
}

/// The indexes and the entries they sort — tickets by `(epic, seq)`, the
/// shortcode as the log interns it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Times {
    born: BTreeSet<(i64, u32, u32)>,
    since: BTreeSet<(i64, u32, u32)>,
    /// FILTER-30: by last touch.
    touched: BTreeSet<(i64, u32, u32)>,
    of: HashMap<(u32, u32), Timed>,
    /// FILTER-30: each ticket's last comment or answer — what a row written
    /// again does not say, kept across its rewrites (and a ticket's delete).
    active: HashMap<(u32, u32), i64>,
}

fn status_ix(s: Status) -> u8 {
    Status::ALL.iter().position(|x| *x == s).expect("a status") as u8
}

/// What a ticket's times are, from its row, its latest status transition's
/// moment (`None`: the log has none of it) and when the log began — its
/// touch its row's alone ([`Times::put`] adds its activity).
pub(crate) fn timed(row: &TicketSummary, latest: Option<i64>, began: Option<i64>) -> Timed {
    let (since, exact) = match latest {
        Some(at) => (at, true),
        None => {
            let updated = if row.updated_unix > 0 { row.updated_unix } else { row.created_unix };
            let bound = began.map_or(updated, |b| updated.min(b));
            (bound.max(row.created_unix), false)
        }
    };
    let written = if row.updated_unix > 0 { row.updated_unix } else { row.created_unix };
    Timed { born: row.created_unix, since, touched: written.max(row.created_unix), status: status_ix(row.status), exact }
}

impl Times {
    /// `(epic, seq)`'s entry.
    pub(crate) fn get(&self, epic: u32, seq: u32) -> Option<Timed> {
        self.of.get(&(epic, seq)).copied()
    }

    /// An empty set of times keeping `active` (FILTER-30) — what a rebuild
    /// from the rows starts from.
    pub(crate) fn with_active(active: HashMap<(u32, u32), i64>) -> Self {
        Self { active, ..Self::default() }
    }

    /// FILTER-30: the activity kept, to carry across a rebuild.
    pub(crate) fn take_active(&mut self) -> HashMap<(u32, u32), i64> {
        std::mem::take(&mut self.active)
    }

    /// `(epic, seq)`'s entry set to `t` — its touch no earlier than its
    /// activity (FILTER-30) — its index places moved.
    pub(crate) fn put(&mut self, epic: u32, seq: u32, mut t: Timed) {
        if let Some(a) = self.active.get(&(epic, seq)) {
            t.touched = t.touched.max(*a);
        }
        if let Some(old) = self.of.insert((epic, seq), t) {
            if old == t {
                return;
            }
            self.born.remove(&(old.born, epic, seq));
            self.since.remove(&(old.since, epic, seq));
            self.touched.remove(&(old.touched, epic, seq));
        }
        self.born.insert((t.born, epic, seq));
        self.since.insert((t.since, epic, seq));
        self.touched.insert((t.touched, epic, seq));
    }

    /// FILTER-30: activity on `(epic, seq)` at `at` — a comment left on it,
    /// an answer to or a clear of one of its inputs: kept, and its touch
    /// moved when later.
    pub(crate) fn touch(&mut self, epic: u32, seq: u32, at: i64) {
        let a = self.active.entry((epic, seq)).or_insert(at);
        *a = (*a).max(at);
        if let Some(t) = self.get(epic, seq) {
            self.put(epic, seq, t);
        }
    }

    /// A status transition of `(epic, seq)` into status `to` at `at` — the
    /// log's append. A ticket not held yet (a newborn whose row comes in the
    /// same critical section) is put when its row is.
    pub(crate) fn moved(&mut self, epic: u32, seq: u32, to: u8, at: i64) {
        if let Some(t) = self.get(epic, seq) {
            self.put(epic, seq, Timed { since: at, status: to, exact: true, ..t });
        }
    }

    /// The tickets `index` holds with `lo <= moment < hi`, per shortcode
    /// index, those `keep` keeps.
    fn range(
        index: &BTreeSet<(i64, u32, u32)>,
        lo: Option<i64>,
        hi: Option<i64>,
        mut keep: impl FnMut(u32, u32) -> bool,
    ) -> BTreeMap<u32, RoaringBitmap> {
        let mut out: BTreeMap<u32, RoaringBitmap> = BTreeMap::new();
        let start = (lo.unwrap_or(i64::MIN), 0, 0);
        for &(at, e, s) in index.range(start..) {
            if hi.is_some_and(|h| at >= h) {
                break;
            }
            if keep(e, s) {
                out.entry(e).or_default().insert(s);
            }
        }
        out
    }

    /// `created <window>`: the tickets born in `w` as `clock` reads it.
    pub(crate) fn created(&self, w: Option<&Window>, clock: &Clock) -> BTreeMap<u32, RoaringBitmap> {
        let Some((lo, hi)) = clock.bounds(w) else { return BTreeMap::new() };
        Self::range(&self.born, lo, hi, |_, _| true)
    }

    /// FILTER-30: `touched <window>`: the tickets last touched in `w`.
    pub(crate) fn touched(&self, w: Option<&Window>, clock: &Clock) -> BTreeMap<u32, RoaringBitmap> {
        let Some((lo, hi)) = clock.bounds(w) else { return BTreeMap::new() };
        Self::range(&self.touched, lo, hi, |_, _| true)
    }

    /// FILTER-30: the earliest last touch at or after `from` — where a
    /// relative window over touches next moves.
    pub(crate) fn first_touched_from(&self, from: i64) -> Option<i64> {
        self.touched.range((from, 0, 0)..).next().map(|(at, _, _)| *at)
    }

    /// `[status S] for over|under D`: the tickets in a status `h` asks of
    /// that have held it longer than `D` (over) or not (under), now.
    pub(crate) fn held(&self, h: &Held, clock: &Clock) -> BTreeMap<u32, RoaringBitmap> {
        let edge = clock.now().saturating_sub(h.seconds());
        let (lo, hi) = if h.over { (None, Some(edge)) } else { (Some(edge), None) };
        Self::range(&self.since, lo, hi, |e, s| {
            self.of.get(&(e, s)).is_some_and(|t| h.of.admits(Status::ALL[t.status as usize]))
        })
    }

    /// FILTER-27: the next moment after `now` at which some ticket's answer
    /// to `h` changes with no write — the first one not yet held for `D`
    /// crossing it; `None` when none will.
    pub(crate) fn next_held(&self, h: &Held, now: i64) -> Option<i64> {
        let edge = now.saturating_sub(h.seconds());
        let admits = |e: u32, s: u32| self.of.get(&(e, s)).is_some_and(|t| h.of.admits(Status::ALL[t.status as usize]));
        self.since
            .range((edge, 0, 0)..)
            .find(|(_, e, s)| admits(*e, *s))
            .map(|(at, _, _)| at.saturating_add(h.seconds()).saturating_add(1))
    }

    /// FILTER-27: the earliest birth at or after `from` — where a relative
    /// window over births next moves.
    pub(crate) fn first_born_from(&self, from: i64) -> Option<i64> {
        self.born.range((from, 0, 0)..).next().map(|(at, _, _)| *at)
    }

    /// `--verify-derived`: where these times differ from `want` (built
    /// afresh from the same rows and log) or from themselves, one line each;
    /// `name` names a shortcode index.
    pub(crate) fn check(&self, want: &Times, name: impl Fn(u32) -> String) -> Vec<String> {
        let mut out = Vec::new();
        let key = |e: u32, s: u32| format!("{}-{s}", name(e));
        for (&(e, s), t) in &want.of {
            match self.of.get(&(e, s)) {
                None => out.push(format!("times of {}: missing; want {t:?}", key(e, s))),
                Some(x) if x != t => out.push(format!("times of {}: {x:?}; want {t:?}", key(e, s))),
                _ => {}
            }
        }
        for &(e, s) in self.of.keys().filter(|k| !want.of.contains_key(k)) {
            out.push(format!("times of {}: held, but no ticket", key(e, s)));
        }
        if self.born.len() != self.of.len() || self.since.len() != self.of.len() || self.touched.len() != self.of.len() {
            out.push(format!(
                "times: {} entries, {} in the birth index, {} in the status-since index, {} in the touch index",
                self.of.len(),
                self.born.len(),
                self.since.len(),
                self.touched.len()
            ));
        }
        for t in self.of.values().filter(|t| t.since < t.born) {
            out.push(format!("times: a status held since {} by a ticket born {}", t.since, t.born));
        }
        for t in self.of.values().filter(|t| t.touched < t.born) {
            out.push(format!("times: last touched at {} by a ticket born {}", t.touched, t.born));
        }
        for (&(e, s), a) in &self.active {
            if self.of.get(&(e, s)).is_some_and(|t| t.touched < *a) {
                out.push(format!("times of {}: last touched before its last comment or answer ({a})", key(e, s)));
            }
        }
        out
    }

    /// Each entry's key and times — tests.
    #[cfg(test)]
    pub(crate) fn entries(&self) -> impl Iterator<Item = (&(u32, u32), &Timed)> {
        self.of.iter()
    }
}
