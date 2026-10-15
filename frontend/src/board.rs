//! SCALE-26: the board's rows, by window — `extents` and pages; the walk ends.
//!
//! Until SCALE-26 the board was a window (MFE-12) over a list the session
//! filled by walking every page of the store: `live` came after the last of
//! them — 43.2 s at 10^6 — and again after every reconnect and every
//! `lagged`, and every row stayed in wasm memory (682 MB of it at 10^6).
//! After SCALE-24 and SCALE-25 the board was the walk's last reader. Now it
//! holds what it shows, and asks for the rest when it is shown:
//!
//! - **Where a ticket sits is arithmetic.** `extents` (SCALE-16) is each
//!   shortcode's last sequence, in the store's order, and sequences are dense
//!   (MNG-11: ids are allocated in order, never reused, and nothing deletes),
//!   so shortcode E holds rows `before(E) .. before(E) + last(E)`, and row k
//!   of the board is `E-(k - before(E) + 1)` ([`Extents`]). Their sum is the
//!   board's extent: the scrollbar, `aria-rowcount`, the dock's count.
//! - **A position is a cursor.** The page that starts at row k is
//!   `list { after: <row k - 1>, limit, summary }` — `after` is a position
//!   (MNG-9), so a far jump costs one page wherever it lands (~2 ms at 10^6,
//!   SCALE-20), and row 0 is asked from the top. The cursor is the row
//!   *before* the position: `after` naming a ticket that does not exist is
//!   `list`'s error, not "start at the next one" (SCALE-18). Only a
//!   hand-edited store has holes; an error marks the cursor's row absent and
//!   the next ask walks back one ([`Held::absent`]).
//! - **What is held is near what is shown.** The board asks for the window's
//!   rows it does not hold — one request in flight, the missing span in one
//!   page — and holds them in the row cache (SCALE-24) through its own
//!   [`cache::Holder`]; whenever the window moves it gives back every row
//!   more than [`KEEP`] rows from it. However far the board is scrolled, and
//!   however fast, it holds the window and two margins: under 2,400 rows.
//! - **`watch` keeps what is held.** A `created` moves the extents — the
//!   count and the scrollbar move with them — and the row is held only when
//!   it lands next to a row the board holds, inside a held stretch
//!   ([`Board::created`]); otherwise nothing is fetched and nothing is held.
//!   Every other event is `cache::put`: applied to the row if some view
//!   holds it, dropped if none does.
//! - **A `lagged`, or a new connection, asks again for what is shown**
//!   ([`Board::refresh`]): the extents, then the window's rows in one page,
//!   applied to the signals already drawn — so the rows keep their DOM — and
//!   every row outside the window given back (it may be stale, and it is not
//!   on screen). No walk: a reconnect at 10^6 is five requests.
//!
//! The row pitch, the scaled layout above 15 M px and the window's margins
//! are MFE-12's (`board_layout` in main.rs); this module only decides which
//! of the window's rows are in hand.
//!
//! FILTER-12: **the board under the lens.** It shows its effective filter —
//! global ∧ board (FILTER-11) — and with none it is everything above,
//! request for request. With one:
//!
//! - **Where a match sits is arithmetic over `counts { filter }`** — each
//!   shortcode's matches in the store's order ([`Matches`]): their sum is
//!   the extent (the scrollbar, `aria-rowcount`, the dock's count), and
//!   position p is the rank-th match of shortcode E by prefix sums.
//! - **A position is asked by position**: the window's missing stretch is
//!   `list { filter, at, limit, summary }`, whose rows are the matches at
//!   `first`, `first + 1`, …; held by rank ([`Ranked`]). A page that does
//!   not fit the counts (its rows in another shortcode, its total not
//!   theirs) says the counts are stale: they are asked again. A deep link
//!   is `list { filter, after: X, limit: 0 }`'s `first` ([`Board::locate`]).
//! - **Live by membership**: a `watch { filter, membership: true }`
//!   ([`session::Membership`]), opened before the previous filter's is
//!   dropped. `entered` raises its shortcode's count, every held row after
//!   it moves a rank, and the row is held when it lands beside a held one
//!   (from the row cache, or the row `watch` just carried); `left` lowers
//!   it and drops the row; `recount` sets the counts and asks the window
//!   again, rows keeping their DOM; `lagged` or a reconnect asks the counts,
//!   then the window. Every other event reaches rows through the session's
//!   `watch`, as before.
//! - **The extents stay the store's order**, asked as before: COMP-9's
//!   checks, and every ticket's count beside the matches.

use std::collections::{BTreeMap, BTreeSet};

use leptos::prelude::*;
use menger_tickets_api::{TicketSummary, TicketsEvent};
use wasm_bindgen_futures::spawn_local;

use crate::session::{self, Client};
use crate::{board_layout, cache, Geom, BOARD_PITCH_GUESS};

/// Rows the board keeps holding beyond each edge of its window once the
/// window is in hand: a scroll back over them is a re-render, not a fetch.
/// The window itself is the viewport plus MFE-12's 100-row margins — about
/// 250 rows — so the board holds at most ~2,250 rows wherever it has been.
pub(crate) const KEEP: usize = 1_000;

// ---------------------------------------------------------------------
// Where every ticket sits — pure, so a native test can pin it.

/// The store's order as `extents` gives it: each shortcode's last sequence,
/// in shortcode order, and how many rows come before each.
///
/// ACT-5: and the sequences each run no longer holds — deleted tickets,
/// whose numbers are never reused (`extents`' `holes`) — which no row is:
/// a run's row k is its k-th sequence that is not a hole.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Extents {
    runs: Vec<(String, u32)>,
    /// Per run, its holes, ascending.
    holes: Vec<Vec<u32>>,
    before: Vec<usize>,
    n: usize,
}

impl Extents {
    /// From `extents`' answer. A shortcode with no ticket has no rows; the
    /// order is the store's (`String`'s, the `(epic, seq)` key's first half),
    /// which the server already sends — sorted here so a lookup can rely on it.
    #[cfg(test)]
    pub(crate) fn new(runs: Vec<(String, u32)>) -> Self {
        Self::holed(runs.into_iter().map(|(e, l)| (e, l, Vec::new())).collect())
    }

    /// ACT-5: [`Extents::new`] with each run's holes.
    pub(crate) fn holed(mut runs: Vec<(String, u32, Vec<u32>)>) -> Self {
        runs.retain(|(_, last, _)| *last > 0);
        runs.sort_by(|a, b| a.0.cmp(&b.0));
        runs.dedup_by(|b, a| {
            let same = a.0 == b.0;
            if same {
                a.1 = a.1.max(b.1);
            }
            same
        });
        let mut holes = Vec::with_capacity(runs.len());
        let runs = runs
            .into_iter()
            .map(|(e, last, mut h)| {
                h.retain(|s| *s >= 1 && *s <= last);
                h.sort_unstable();
                h.dedup();
                holes.push(h);
                (e, last)
            })
            .collect();
        let mut e = Self { runs, holes, before: Vec::new(), n: 0 };
        e.sum();
        e
    }

    fn sum(&mut self) {
        let mut acc = 0usize;
        self.before = self
            .runs
            .iter()
            .zip(&self.holes)
            .map(|((_, last), holes)| {
                let b = acc;
                acc += *last as usize - holes.len();
                b
            })
            .collect();
        self.n = acc;
    }

    /// ACT-5: a ticket deleted — `E-seq` is a hole now, and every position
    /// after it one less. True when that moved anything.
    pub(crate) fn gone(&mut self, epic: &str, seq: u32) -> bool {
        let Ok(i) = self.runs.binary_search_by(|(e, _)| e.as_str().cmp(epic)) else { return false };
        if seq == 0 || seq > self.runs[i].1 {
            return false;
        }
        match self.holes[i].binary_search(&seq) {
            Ok(_) => return false,
            Err(at) => self.holes[i].insert(at, seq),
        }
        self.sum();
        true
    }

    /// Every row of the board: the store's ticket count.
    pub(crate) fn n(&self) -> usize {
        self.n
    }

    /// Where `E-seq` sits — `None` for a shortcode or a sequence the
    /// extents do not reach.
    pub(crate) fn pos(&self, epic: &str, seq: u32) -> Option<usize> {
        let i = self.runs.binary_search_by(|(e, _)| e.as_str().cmp(epic)).ok()?;
        let holes = &self.holes[i];
        let below = match holes.binary_search(&seq) {
            Ok(_) => return None,
            Err(below) => below,
        };
        (seq >= 1 && seq <= self.runs[i].1).then(|| self.before[i] + seq as usize - 1 - below)
    }

    /// Where the ticket `id` sits, by its own key (`{epic}-{seq}`).
    pub(crate) fn pos_of(&self, id: &str) -> Option<usize> {
        let (epic, seq) = crate::kids::key(id)?;
        self.pos(epic, seq)
    }

    /// COMP-9: whether a shortcode holds a ticket.
    pub(crate) fn has_shortcode(&self, epic: &str) -> bool {
        self.runs.binary_search_by(|(e, _)| e.as_str().cmp(epic)).is_ok()
    }

    /// The key of row `p`.
    pub(crate) fn key_at(&self, p: usize) -> Option<(&str, u32)> {
        if p >= self.n {
            return None;
        }
        let i = self.before.partition_point(|b| *b <= p) - 1;
        // ACT-5: the run's (p − before)-th sequence that is not a hole.
        let mut seq = (p - self.before[i]) as u32 + 1;
        for h in &self.holes[i] {
            if *h <= seq {
                seq += 1;
            } else {
                break;
            }
        }
        Some((self.runs[i].0.as_str(), seq))
    }

    /// The id of row `p`.
    pub(crate) fn id_at(&self, p: usize) -> Option<String> {
        self.key_at(p).map(|(e, s)| format!("{e}-{s}"))
    }

    /// A ticket born — `created`, or a page row past what the extents knew:
    /// its shortcode reaches `seq` now. True when that moved anything (an
    /// event the extents already counted moves nothing, so neither a
    /// re-asked `extents` nor a page that raced the event counts it twice).
    pub(crate) fn born(&mut self, epic: &str, seq: u32) -> bool {
        if seq == 0 {
            return false;
        }
        match self.runs.binary_search_by(|(e, _)| e.as_str().cmp(epic)) {
            // ACT-5: a hole filled — an undeleted ticket.
            Ok(i) if seq <= self.runs[i].1 => match self.holes[i].binary_search(&seq) {
                Ok(at) => {
                    self.holes[i].remove(at);
                }
                Err(_) => return false,
            },
            Ok(i) => self.runs[i].1 = seq,
            Err(i) => {
                self.runs.insert(i, (epic.to_string(), seq));
                self.holes.insert(i, Vec::new());
            }
        }
        self.sum();
        true
    }
}

/// A row's place in the store's total order — MNG-9's `(epic, seq)`.
type Key = (String, u32);

fn owned((e, s): (&str, u32)) -> Key {
    (e.to_string(), s)
}

/// What the board holds: its rows by key (the id as the server spelled it,
/// and the row cache's signal), and the rows a page passed over — holes, in
/// a hand-edited store; forgotten whenever the extents are asked again.
#[derive(Clone, Default)]
pub(crate) struct Held {
    rows: BTreeMap<Key, (String, cache::RowSignal)>,
    absent: BTreeSet<Key>,
}

impl Held {
    fn has(&self, k: (&str, u32)) -> bool {
        self.rows.contains_key(&owned(k))
    }

    fn missing(&self, k: (&str, u32)) -> bool {
        let k = owned(k);
        !self.rows.contains_key(&k) && !self.absent.contains(&k)
    }
}

/// One stretch of the window as the board draws it: a row it holds, or rows
/// it does not hold yet — `(first, how many)` — drawn as one line each.
#[derive(Clone, PartialEq)]
pub(crate) enum Slot {
    Row(String, cache::RowSignal),
    Gap(usize, usize),
}

impl Slot {
    /// The `<For>`'s key: a row is its id, so it keeps its `<tr>` while it is
    /// in the window; a gap is where it starts and how long it is (its height
    /// is fixed when it is drawn).
    pub(crate) fn key(&self) -> String {
        match self {
            Slot::Row(id, _) => id.clone(),
            Slot::Gap(at, len) => format!("?{at}+{len}"),
        }
    }
}

/// The window `lo..hi` as rows and gaps, in order.
pub(crate) fn slots(ext: &Extents, held: &Held, lo: usize, hi: usize) -> Vec<Slot> {
    let mut out = Vec::new();
    let hi = hi.min(ext.n());
    let Some(first) = ext.key_at(lo).filter(|_| lo < hi) else { return out };
    let mut p = lo;
    for ((epic, seq), (id, sig)) in held.rows.range(owned(first)..) {
        let Some(q) = ext.pos(epic, *seq) else { continue };
        if q >= hi {
            break;
        }
        if q > p {
            out.push(Slot::Gap(p, q - p));
        }
        out.push(Slot::Row(id.clone(), sig.clone()));
        p = q + 1;
    }
    if p < hi {
        out.push(Slot::Gap(p, hi - p));
    }
    out
}

/// FILTER-12: the window `lo..hi` under a filter as rows and gaps, in order —
/// the rows held at their positions ([`Ranked::window`]).
pub(crate) fn ranked_slots(m: &Matches, r: &Ranked<(String, cache::RowSignal)>, lo: usize, hi: usize) -> Vec<Slot> {
    let hi = hi.min(m.n());
    let mut out = Vec::new();
    if lo >= hi {
        return out;
    }
    let mut p = lo;
    for (q, _, (id, sig)) in r.window(m, lo, hi) {
        if q > p {
            out.push(Slot::Gap(p, q - p));
        }
        out.push(Slot::Row(id.clone(), sig.clone()));
        p = q + 1;
    }
    if p < hi {
        out.push(Slot::Gap(p, hi - p));
    }
    out
}

/// The first and the last row of `lo..hi` the board neither holds nor knows
/// to be a hole — what one page must bring.
pub(crate) fn missing(ext: &Extents, held: &Held, lo: usize, hi: usize) -> Option<(usize, usize)> {
    let hi = hi.min(ext.n());
    let first = (lo..hi).find(|&p| ext.key_at(p).is_some_and(|k| held.missing(k)))?;
    let last = (first..hi).rev().find(|&p| ext.key_at(p).is_some_and(|k| held.missing(k)))?;
    Some((first, last))
}

/// The cursor for the page that starts at row `p`: the row before it — the
/// id the board holds there, or the one the extents name — stepping back
/// over rows known to be holes; `None` (the top) for row 0.
pub(crate) fn cursor(ext: &Extents, held: &Held, p: usize) -> Option<String> {
    let mut q = p.min(ext.n());
    while q > 0 {
        q -= 1;
        let k = owned(ext.key_at(q)?);
        if held.absent.contains(&k) {
            continue;
        }
        return Some(match held.rows.get(&k) {
            Some((id, _)) => id.clone(),
            None => format!("{}-{}", k.0, k.1),
        });
    }
    None
}

/// What a page asked for after `after` (`None`: from the top) says about the
/// rows it passed over: every row between the cursor and the page's last
/// that it did not bring is a hole. Worked out from the cursor's key, not
/// from where the board wanted the page to start — a birth before it may
/// have moved every position since — and pure arithmetic over the extents,
/// so a dense store never has any.
fn holes(ext: &Extents, after: Option<&str>, page: &[Key]) -> Vec<Key> {
    let from = match after {
        None => 0,
        Some(id) => match ext.pos_of(id) {
            Some(p) => p + 1,
            None => return Vec::new(),
        },
    };
    let brought: BTreeSet<&Key> = page.iter().collect();
    let Some(end) = page.iter().filter_map(|(e, s)| ext.pos(e, *s)).max() else { return Vec::new() };
    (from..end)
        .filter_map(|p| ext.key_at(p).map(owned))
        .filter(|k| !brought.contains(k))
        .collect()
}

// ---------------------------------------------------------------------
// FILTER-12: where every match sits — pure, so a native test can pin it.

/// The board's arithmetic under a filter: `counts { filter }` — each
/// shortcode's number of matches, in the store's order — and how many come
/// before each. SCALE-26's [`Extents`] with counts in place of last
/// sequences: their sum is the board's extent, and position `p` is the
/// `rank`-th match of shortcode E where `start(E) <= p < start(E) + held(E)`
/// ([`Matches::locate`]). Which sequence that is, the server knows
/// (`list { filter, at: p }` selects it); the board knows it of the rows it
/// holds ([`Ranked`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Matches {
    runs: Vec<(String, usize)>,
    before: Vec<usize>,
    n: usize,
}

impl Matches {
    /// From `counts`' answer: a shortcode with no match has no run; sorted
    /// into the store's order (the server's already), a shortcode said twice
    /// counted once, its last word.
    pub(crate) fn new(counts: impl IntoIterator<Item = (String, u64)>) -> Self {
        let mut runs: Vec<(String, usize)> = Vec::new();
        for (epic, held) in counts {
            match runs.iter_mut().find(|(e, _)| *e == epic) {
                Some(run) => run.1 = held as usize,
                None => runs.push((epic, held as usize)),
            }
        }
        runs.retain(|(_, held)| *held > 0);
        runs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut m = Self { runs, before: Vec::new(), n: 0 };
        m.sum();
        m
    }

    fn sum(&mut self) {
        let mut acc = 0usize;
        self.before = self
            .runs
            .iter()
            .map(|(_, held)| {
                let b = acc;
                acc += *held;
                b
            })
            .collect();
        self.n = acc;
    }

    fn find(&self, epic: &str) -> Result<usize, usize> {
        self.runs.binary_search_by(|(e, _)| e.as_str().cmp(epic))
    }

    /// Every match: the board's extent under the filter.
    pub(crate) fn n(&self) -> usize {
        self.n
    }

    /// Each shortcode's matches, in the store's order (FILTER-14's nav sums
    /// them along the hosts).
    #[allow(dead_code)] // FILTER-14
    pub(crate) fn runs(&self) -> &[(String, usize)] {
        &self.runs
    }

    /// Shortcode `epic`'s matches.
    pub(crate) fn held(&self, epic: &str) -> usize {
        self.find(epic).map_or(0, |i| self.runs[i].1)
    }

    /// How many matches come before `epic`'s — where its first sits, or
    /// would.
    #[allow(dead_code)] // FILTER-14
    pub(crate) fn start(&self, epic: &str) -> usize {
        match self.find(epic) {
            Ok(i) => self.before[i],
            Err(i) => self.before.get(i).copied().unwrap_or(self.n),
        }
    }

    /// Position `p` as (shortcode, rank among its matches) — `None` past the
    /// end.
    pub(crate) fn locate(&self, p: usize) -> Option<(&str, usize)> {
        if p >= self.n {
            return None;
        }
        let i = self.before.partition_point(|b| *b <= p) - 1;
        Some((self.runs[i].0.as_str(), p - self.before[i]))
    }

    /// Where the `rank`-th match of `epic` sits — `None` past its last.
    pub(crate) fn pos(&self, epic: &str, rank: usize) -> Option<usize> {
        let i = self.find(epic).ok()?;
        (rank < self.runs[i].1).then(|| self.before[i] + rank)
    }

    /// `entered`: one more match in `epic` (a new shortcode lands at its
    /// place in the order).
    pub(crate) fn entered(&mut self, epic: &str) {
        match self.find(epic) {
            Ok(i) => self.runs[i].1 += 1,
            Err(i) => self.runs.insert(i, (epic.to_string(), 1)),
        }
        self.sum();
    }

    /// `left`: one fewer in `epic` — never below none.
    pub(crate) fn left(&mut self, epic: &str) {
        if let Ok(i) = self.find(epic) {
            self.runs[i].1 -= 1;
            if self.runs[i].1 == 0 {
                self.runs.remove(i);
            }
            self.sum();
        }
    }

    /// `recount`: `epic`'s matches are `held`, as the write left them.
    pub(crate) fn recount(&mut self, epic: &str, held: u64) {
        let held = held as usize;
        match (self.find(epic), held) {
            (Ok(i), 0) => {
                self.runs.remove(i);
            }
            (Ok(i), h) => self.runs[i].1 = h,
            (Err(_), 0) => return,
            (Err(i), h) => self.runs.insert(i, (epic.to_string(), h)),
        }
        self.sum();
    }
}

/// What the board holds under a filter: its rows by key, each with its rank
/// among its shortcode's matches — it sits at `start(epic) + rank`. `T` is
/// what a row carries (the id and the row cache's signal, in the app).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ranked<T> {
    rows: BTreeMap<Key, (T, usize)>,
}

impl<T> Default for Ranked<T> {
    fn default() -> Self {
        Self { rows: BTreeMap::new() }
    }
}

/// Where an `entered` ticket landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Landed {
    /// It was held already — a member before: nothing moved.
    Held,
    /// Its position, when the rows held around it say it (`None`: they do
    /// not), and its rank there; `beside`: a row the board holds is next to
    /// it — inside a held stretch, so it is held too.
    At { pos: Option<usize>, rank: Option<usize>, beside: bool },
}

/// A page that does not fit what the board knows: its rows are not where
/// the counts put them, or its total is not theirs — the counts are stale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stale;

impl<T> Ranked<T> {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// Where the row held under `k` sits.
    pub(crate) fn pos_of(&self, m: &Matches, k: &Key) -> Option<usize> {
        let (_, rank) = self.rows.get(k)?;
        m.pos(&k.0, *rank)
    }

    /// The rows of shortcode `epic`, in its order.
    fn of(&self, epic: &str) -> impl DoubleEndedIterator<Item = (&Key, &(T, usize))> {
        let lo: Key = (epic.to_string(), 0);
        let hi: Key = (epic.to_string(), u32::MAX);
        self.rows.range(lo..=hi)
    }

    /// The row held at position `p`.
    pub(crate) fn at(&self, m: &Matches, p: usize) -> Option<(&Key, &T)> {
        let (epic, rank) = m.locate(p)?;
        self.of(epic).find(|(_, (_, r))| *r == rank).map(|(k, (t, _))| (k, t))
    }

    /// The rows held in `lo..hi`, by position — each position once (a row
    /// whose rank a `recount` left stale yields to the one before it).
    pub(crate) fn window(&self, m: &Matches, lo: usize, hi: usize) -> Vec<(usize, &Key, &T)> {
        let mut out: Vec<(usize, &Key, &T)> = Vec::new();
        for (k, (t, rank)) in &self.rows {
            let Some(p) = m.pos(&k.0, *rank) else { continue };
            if p < lo || p >= hi || out.last().is_some_and(|(q, _, _)| *q >= p) {
                continue;
            }
            out.push((p, k, t));
        }
        out
    }

    /// The first and the last position of `lo..hi` the board does not hold —
    /// what one page must bring.
    pub(crate) fn missing(&self, m: &Matches, lo: usize, hi: usize) -> Option<(usize, usize)> {
        let hi = hi.min(m.n());
        let held: BTreeSet<usize> = self.window(m, lo, hi).into_iter().map(|(p, _, _)| p).collect();
        let first = (lo..hi).find(|p| !held.contains(p))?;
        let last = (first..hi).rev().find(|p| !held.contains(p))?;
        Some((first, last))
    }

    /// `entered { epic, seq }`, applied: one more match in its shortcode, and
    /// every held row of the shortcode after it one rank further. Where it
    /// lands is known when the rows around it are held: the row before it in
    /// its shortcode and the one at the next rank (or the shortcode's last);
    /// the first of the shortcode at rank 0; or a shortcode with no match yet.
    pub(crate) fn entered(&mut self, m: &mut Matches, epic: &str, seq: u32) -> Landed {
        let key: Key = (epic.to_string(), seq);
        if self.rows.contains_key(&key) {
            return Landed::Held;
        }
        let count = m.held(epic);
        let before = self.of(epic).rev().find(|(k, _)| k.1 < seq).map(|(_, (_, r))| *r);
        let after = self.of(epic).find(|(k, _)| k.1 > seq).map(|(_, (_, r))| *r);
        let rank = match (before, after) {
            _ if count == 0 => Some(0),
            (Some(b), _) if b + 1 == count => Some(b + 1),
            (Some(b), Some(a)) if a == b + 1 => Some(b + 1),
            (None, Some(0)) => Some(0),
            _ => None,
        };
        m.entered(epic);
        for (k, (_, r)) in self.rows.iter_mut() {
            if k.0 == epic && k.1 > seq {
                *r += 1;
            }
        }
        let pos = rank.and_then(|r| m.pos(epic, r));
        let beside = pos.is_some_and(|p| {
            let held = |q: usize| self.at(m, q).is_some();
            (p > 0 && held(p - 1)) || held(p + 1)
        });
        Landed::At { pos, rank, beside }
    }

    /// Hold `t` under `k` at `rank` (what an `entered` found beside a held
    /// row, or a page brought).
    pub(crate) fn insert(&mut self, k: Key, t: T, rank: usize) {
        self.rows.insert(k, (t, rank));
    }

    /// `left { epic, seq }`, applied: one fewer match in its shortcode, its
    /// row dropped (handed back, to be released), and every held row of the
    /// shortcode after it one rank nearer.
    pub(crate) fn left(&mut self, m: &mut Matches, epic: &str, seq: u32) -> Option<T> {
        m.left(epic);
        let gone = self.rows.remove(&(epic.to_string(), seq)).map(|(t, _)| t);
        for (k, (_, r)) in self.rows.iter_mut() {
            if k.0 == epic && k.1 > seq {
                *r = r.saturating_sub(1);
            }
        }
        gone
    }

    /// `recount { epic, held }`, applied to the counts. The ranks of the
    /// shortcode's rows may be stale until the window is asked again; they
    /// keep their rows (and their DOM) meanwhile, and [`Ranked::window`]
    /// draws each position once.
    pub(crate) fn recount(&mut self, m: &mut Matches, epic: &str, held: u64) {
        m.recount(epic, held);
    }

    /// A page `list { filter, at: first }` answered, `total` being its
    /// `first + rows + remaining`: each row is the match at `first + i`. The
    /// counts must agree — each row in the shortcode the counts place there,
    /// the total theirs — or the page is [`Stale`] and nothing is placed.
    /// Placed: the rows ranked (a row held already re-ranked, keeping its
    /// payload), and every other held row that claimed one of the page's
    /// positions, or sits between its first and last row, handed back — it
    /// is not a match there any more.
    pub(crate) fn place(&mut self, m: &Matches, first: usize, total: Option<usize>, page: Vec<(Key, T)>) -> Result<Placed<T>, Stale> {
        let keys: Vec<Key> = page.iter().map(|(k, _)| k.clone()).collect();
        let ranks = Self::fits(m, first, total, &keys)?;
        let span = first..first + page.len();
        let keys: BTreeSet<Key> = page.iter().map(|(k, _)| k.clone()).collect();
        let (lo, hi) = match (keys.first(), keys.last()) {
            (Some(a), Some(b)) => (a.clone(), b.clone()),
            _ => return Ok(Placed { fresh: false, gone: Vec::new() }),
        };
        let stale: Vec<Key> = self
            .rows
            .iter()
            .filter(|(k, (_, rank))| {
                !keys.contains(*k) && ((**k >= lo && **k <= hi) || m.pos(&k.0, *rank).is_some_and(|p| span.contains(&p)))
            })
            .map(|(k, _)| k.clone())
            .collect();
        let gone = stale.into_iter().filter_map(|k| self.rows.remove(&k)).map(|(t, _)| t).collect();
        let mut fresh = false;
        for ((k, t), rank) in page.into_iter().zip(ranks) {
            match self.rows.get_mut(&k) {
                Some(held) => held.1 = rank,
                None => {
                    fresh = true;
                    self.rows.insert(k, (t, rank));
                }
            }
        }
        Ok(Placed { fresh, gone })
    }

    /// Whether a page of `keys` at `first`, of `total` matches, fits the
    /// counts — each row in the shortcode they place there, the total
    /// theirs: the rows' ranks, or [`Stale`].
    pub(crate) fn fits(m: &Matches, first: usize, total: Option<usize>, keys: &[Key]) -> Result<Vec<usize>, Stale> {
        if total.is_some_and(|t| t != m.n()) {
            return Err(Stale);
        }
        keys.iter()
            .enumerate()
            .map(|(i, k)| match m.locate(first + i) {
                Some((epic, rank)) if epic == k.0 => Ok(rank),
                _ => Err(Stale),
            })
            .collect()
    }

    /// Whether a row is held outside `lo..hi` (or placed nowhere).
    pub(crate) fn outside(&self, m: &Matches, lo: usize, hi: usize) -> bool {
        self.rows.iter().any(|(k, (_, rank))| m.pos(&k.0, *rank).is_none_or(|p| p < lo || p >= hi))
    }

    /// Every row outside `lo..hi` (or placed nowhere) taken out and handed
    /// back.
    pub(crate) fn keep(&mut self, m: &Matches, lo: usize, hi: usize) -> Vec<T> {
        let gone: Vec<Key> = self
            .rows
            .iter()
            .filter(|(k, (_, rank))| m.pos(&k.0, *rank).is_none_or(|p| p < lo || p >= hi))
            .map(|(k, _)| k.clone())
            .collect();
        gone.into_iter().filter_map(|k| self.rows.remove(&k)).map(|(t, _)| t).collect()
    }

    /// Every row taken out and handed back.
    pub(crate) fn clear(&mut self) -> Vec<T> {
        std::mem::take(&mut self.rows).into_values().map(|(t, _)| t).collect()
    }
}

/// What placing a page did: whether it held a row the board did not, and
/// the rows it displaced (to be released).
#[derive(Debug, PartialEq)]
pub(crate) struct Placed<T> {
    pub(crate) fresh: bool,
    pub(crate) gone: Vec<T>,
}

// ---------------------------------------------------------------------
// The board's rows, live

/// One page of the store, asked of the server.
enum Answer {
    Page(Vec<TicketSummary>),
    /// An in-band refusal — the cursor names no ticket (a hole).
    Refused,
    /// No socket, or it died on the way.
    Offline(String),
}

async fn ask(client: &Client, after: Option<String>, limit: usize) -> Answer {
    let Ok(after) = crate::typed::opt(after) else { return Answer::Refused };
    match client
        .list_all(None, None, None, after, Some(limit.max(1) as u32), Some(true), None, None, None, crate::session::zone())
        .await
    {
        Ok(events) => {
            let mut rows = Vec::new();
            let mut paged = false;
            for ev in events {
                match ev {
                    TicketsEvent::Summary { summary } => rows.push(summary),
                    TicketsEvent::Page { .. } => paged = true,
                    TicketsEvent::Error { .. } => return Answer::Refused,
                    _ => {}
                }
            }
            if paged {
                Answer::Page(rows)
            } else {
                Answer::Refused
            }
        }
        Err(e) => Answer::Offline(session::describe(&e)),
    }
}

/// FILTER-12: what `counts { filter }` answered.
enum Counted {
    Counts(Vec<(String, u64)>),
    /// An in-band refusal — a filter the server does not take.
    Refused(String),
    Offline(String),
}

async fn count(client: &Client, filter: &str) -> Counted {
    let filter = match crate::typed::typed(filter) {
        Ok(f) => f,
        Err(why) => return Counted::Refused(why),
    };
    match client.counts_all(Some(filter), None, crate::session::zone()).await {
        Ok(events) => {
            let mut runs = Vec::new();
            for ev in events {
                match ev {
                    TicketsEvent::Count { shortcode: epic, held, .. } => runs.push((epic.to_string(), held)),
                    TicketsEvent::Error { message } => return Counted::Refused(message),
                    _ => {}
                }
            }
            Counted::Counts(runs)
        }
        Err(e) => Counted::Offline(session::describe(&e)),
    }
}

/// FILTER-12: one page of the matches, from position `at`.
enum Window {
    /// Its rows; `first`, the position of the first; `total`, every match
    /// (`first + rows + remaining`).
    Page { rows: Vec<TicketSummary>, first: usize, total: usize },
    Refused(String),
    Offline(String),
}

async fn ask_at(client: &Client, filter: &str, at: usize, limit: usize) -> Window {
    let filter = match crate::typed::typed(filter) {
        Ok(f) => f,
        Err(why) => return Window::Refused(why),
    };
    match client
        .list_all(None, None, None, None, Some(limit.max(1) as u32), Some(true), Some(filter), Some(at as u64), None, crate::session::zone())
        .await
    {
        Ok(events) => {
            let mut rows = Vec::new();
            for ev in events {
                match ev {
                    TicketsEvent::Summary { summary } => rows.push(summary),
                    TicketsEvent::Page { remaining, first, .. } => {
                        let first = first.map_or(at, |f| f as usize);
                        let total = first + rows.len() + remaining as usize;
                        return Window::Page { rows, first, total };
                    }
                    TicketsEvent::Error { message } => return Window::Refused(message),
                    _ => {}
                }
            }
            Window::Refused("list: no page".into())
        }
        Err(e) => Window::Offline(session::describe(&e)),
    }
}

/// FILTER-12: what a page `list { filter, after: X, limit: 0 }` says: X's
/// position among the matches (its `first`, less one when X is a match —
/// the page after X starts right after it).
async fn ask_place(client: &Client, filter: &str, id: &str) -> Option<usize> {
    let (id, filter) = (crate::typed::typed(id).ok()?, crate::typed::typed(filter).ok()?);
    let events = client
        .list_all(None, None, None, Some(id), Some(0), Some(true), Some(filter), None, None, crate::session::zone())
        .await
        .ok()?;
    events.into_iter().find_map(|ev| match ev {
        TicketsEvent::Page { first, .. } => first.map(|f| f as usize),
        _ => None,
    })
}

/// FILTER-12: the filter the board shows — its effective filter's groups as
/// the wire's text — and whether they admit nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Lensing {
    pub(crate) text: String,
    pub(crate) nothing: bool,
}

impl Lensing {
    /// The board's lensing for its effective filter: `None` with no group
    /// (SCALE-26's board — the display part filters nothing).
    pub(crate) fn of(f: &menger_tickets_api::filter::Filter) -> Option<Lensing> {
        (!f.groups.is_empty()).then(|| Lensing { text: crate::counts::wire_text(f), nothing: f.admits_nothing().is_some() })
    }
}

/// A row the board holds under a filter: its id and its signal.
type Row = (String, cache::RowSignal);

/// The board's model of the store: the extents, the rows it holds, and the
/// geometry its view last measured (MFE-12's [`Geom`]) — from which the
/// window is worked out whether or not the board is on screen. Made once by
/// the App; the session writes the extents and asks for what is shown, the
/// board's view reads.
///
/// FILTER-12: under a filter (the board's effective filter non-empty) the
/// board is a window over its matches instead: `counts { filter }` is its
/// arithmetic ([`Matches`]), its rows are held by rank ([`Ranked`]) and
/// asked by position (`list { filter, at }`), and a membership watch moves
/// both. The extents stay the store's order (COMP-9's checks, the header's
/// count of every ticket); with no filter everything is as SCALE-26 left it.
#[derive(Clone, Copy)]
pub(crate) struct Board {
    ext: RwSignal<Option<Extents>>,
    held: RwSignal<Held>,
    pub(crate) geom: RwSignal<Geom>,
    holder: StoredValue<cache::Holder>,
    /// Moves with every [`Board::refresh`]: an answer to an older question
    /// is not applied.
    gen: StoredValue<u32>,
    /// The generation of the request in flight — one at a time.
    flight: StoredValue<Option<u32>>,
    /// An answer that changed nothing re-runs nothing — except when it came
    /// back to a newer generation: then the board asks again.
    retry: Trigger,
    /// Asks in a row that told the board nothing, for one window: a store
    /// that disagrees with its extents cannot spin the board on one gap.
    strikes: StoredValue<((usize, usize), u8)>,
    /// FILTER-12: the filter shown — `None`: the store, unfiltered.
    lens: RwSignal<Option<Lensing>>,
    /// FILTER-12: `counts { filter }`, once answered.
    matches: RwSignal<Option<Matches>>,
    /// FILTER-12: the rows held under the filter, by rank.
    ranked: RwSignal<Ranked<Row>>,
    /// FILTER-12: moves with every membership event — a page asked before
    /// one is not placed (its positions may have moved).
    moved: StoredValue<u64>,
    /// FILTER-12: the board's membership watch, while it has a filter.
    member: StoredValue<Option<session::Membership>>,
    /// FILTER-12: what the server said of a filter it did not take.
    refused: RwSignal<Option<String>>,
    /// FILTER-12: counts asked again because a page did not fit them, since
    /// the last page that did.
    resyncs: StoredValue<u8>,
}

impl Board {
    pub(crate) fn new() -> Self {
        let viewport = crate::dom::window().inner_height().ok().and_then(|h| h.as_f64()).unwrap_or(900.0);
        Self {
            ext: RwSignal::new(None),
            held: RwSignal::new(Held::default()),
            geom: RwSignal::new(Geom { above: 0.0, viewport, pitch: BOARD_PITCH_GUESS }),
            holder: StoredValue::new(cache::Holder::new()),
            gen: StoredValue::new(0),
            flight: StoredValue::new(None),
            retry: Trigger::new(),
            strikes: StoredValue::new(((0, 0), 0)),
            lens: RwSignal::new(None),
            matches: RwSignal::new(None),
            ranked: RwSignal::new(Ranked::default()),
            moved: StoredValue::new(0),
            member: StoredValue::new(None),
            refused: RwSignal::new(None),
            resyncs: StoredValue::new(0),
        }
    }

    /// FILTER-12: the filter the board shows, if any (tracked).
    pub(crate) fn lensing(self) -> Option<Lensing> {
        self.lens.get()
    }

    /// FILTER-12: whether the board shows a filter's matches (tracked).
    pub(crate) fn filtered(self) -> bool {
        self.lens.with(Option::is_some)
    }

    pub(crate) fn filtered_untracked(self) -> bool {
        self.lens.with_untracked(Option::is_some)
    }

    /// FILTER-12: what the server said of a filter it did not take (tracked).
    pub(crate) fn refused(self) -> Option<String> {
        self.refused.get()
    }

    /// The extents are in hand (tracked) — under a filter, its counts.
    pub(crate) fn ready(self) -> bool {
        if self.filtered() {
            self.matches.with(Option::is_some)
        } else {
            self.ext.with(Option::is_some)
        }
    }

    /// The board's extent — every ticket (tracked); under a filter, every
    /// match.
    pub(crate) fn n(self) -> usize {
        if self.filtered() {
            self.matches.with(|m| m.as_ref().map_or(0, Matches::n))
        } else {
            self.ext.with(|e| e.as_ref().map_or(0, Extents::n))
        }
    }

    pub(crate) fn n_untracked(self) -> usize {
        if self.filtered_untracked() {
            self.matches.with_untracked(|m| m.as_ref().map_or(0, Matches::n))
        } else {
            self.ext.with_untracked(|e| e.as_ref().map_or(0, Extents::n))
        }
    }

    /// FILTER-12: every ticket in the store — the extents' sum, filter or
    /// none (tracked; `None` before the extents are in).
    pub(crate) fn store_n(self) -> Option<usize> {
        self.ext.with(|e| e.as_ref().map(Extents::n))
    }

    /// Where `id` sits (tracked: a birth before it moves it). Under a
    /// filter, a row the board holds.
    pub(crate) fn pos_of(self, id: &str) -> Option<usize> {
        if self.filtered() {
            let k = owned(crate::kids::key(id)?);
            return self.matches.with(|m| self.ranked.with(|r| r.pos_of(m.as_ref()?, &k)));
        }
        self.ext.with(|e| e.as_ref()?.pos_of(id))
    }

    pub(crate) fn pos_of_untracked(self, id: &str) -> Option<usize> {
        if self.filtered_untracked() {
            let k = owned(crate::kids::key(id)?);
            return self.matches.with_untracked(|m| self.ranked.with_untracked(|r| r.pos_of(m.as_ref()?, &k)));
        }
        self.ext.with_untracked(|e| e.as_ref()?.pos_of(id))
    }

    /// COMP-9: whether a shortcode holds a ticket — `None` before the
    /// extents are in.
    pub(crate) fn has_shortcode_untracked(self, epic: &str) -> Option<bool> {
        self.ext.with_untracked(|e| e.as_ref().map(|e| e.has_shortcode(epic)))
    }

    /// COMP-9: whether a ticket has the id `id` (ids are allocated
    /// highest-plus-one and nothing deletes — SCALE-16) — `None` before the
    /// extents are in.
    pub(crate) fn has_ticket_untracked(self, id: &str) -> Option<bool> {
        self.ext.with_untracked(|e| e.as_ref().map(|e| e.pos_of(id).is_some()))
    }

    /// The row pitch the board's view last measured (px).
    pub(crate) fn pitch(self) -> f64 {
        self.geom.with_untracked(|g| g.pitch())
    }

    /// The id of row `p` — held or not: the keyboard walks every row.
    /// FILTER-12: under a filter, held — which match sits at a position the
    /// board does not hold, only the server knows (`None`).
    pub(crate) fn id_at(self, p: usize) -> Option<String> {
        if self.filtered_untracked() {
            return self.matches.with_untracked(|m| self.ranked.with_untracked(|r| r.at(m.as_ref()?, p).map(|(_, (id, _))| id.clone())));
        }
        self.ext.with_untracked(|e| {
            let e = e.as_ref()?;
            let k = owned(e.key_at(p)?);
            self.held.with_untracked(|h| h.rows.get(&k).map(|(id, _)| id.clone())).or_else(|| e.id_at(p))
        })
    }

    /// The window `lo..hi` as the view draws it (tracked).
    pub(crate) fn slots(self, lo: usize, hi: usize) -> Vec<Slot> {
        if self.filtered() {
            return self.matches.with(|m| match m {
                Some(m) => self.ranked.with(|r| ranked_slots(m, r, lo, hi)),
                None => Vec::new(),
            });
        }
        self.ext.with(|e| match e {
            Some(e) => self.held.with(|h| slots(e, h, lo, hi)),
            None => Vec::new(),
        })
    }

    /// The window the board's view mounts for `n` rows at the geometry it
    /// last measured — MFE-12's, worked out here so the session can ask for
    /// it before the view has drawn a row.
    fn window(self) -> (usize, usize) {
        let n = self.n_untracked();
        let l = board_layout(n, self.geom.get_untracked());
        (l.start.min(n), l.end.min(n))
    }

    /// `extents` answered: the store's order, as it is now. Holes are asked
    /// about again; a row the extents no longer reach (a reconnect to another
    /// store) is given back.
    pub(crate) fn set_extents(self, runs: Vec<(String, u32, Vec<u32>)>) {
        let ext = Extents::holed(runs);
        let gone: Vec<String> = self.held.with_untracked(|h| {
            h.rows.iter().filter(|((e, s), _)| ext.pos(e, *s).is_none()).map(|(_, (id, _))| id.clone()).collect()
        });
        self.ext.set(Some(ext));
        self.held.update(|h| {
            h.absent.clear();
            h.rows.retain(|_, (id, _)| !gone.contains(id));
        });
        self.holder.with_value(|holder| gone.iter().for_each(|id| holder.release(id)));
    }

    /// `created`: the store grew by one. The extents move — and with them the
    /// count, the scrollbar and every position after it — and the row is
    /// held if it lands inside a stretch the board holds (next to a row it
    /// holds); anywhere else nothing is fetched and nothing is held.
    pub(crate) fn created(self, t: &TicketSummary) {
        let grew = self.ext.with_untracked(|e| e.as_ref().is_some_and(|e| e.pos(t.shortcode.as_str(), t.seq).is_none()));
        if grew {
            self.ext.update(|e| {
                if let Some(e) = e {
                    e.born(t.shortcode.as_str(), t.seq);
                }
            });
        }
        let near = self.ext.with_untracked(|e| {
            let e = e.as_ref()?;
            let p = e.pos(t.shortcode.as_str(), t.seq)?;
            self.held.with_untracked(|h| {
                if h.has((t.shortcode.as_str(), t.seq)) {
                    return Some(false); // held already: `put` below is the whole story
                }
                let beside = |q: usize| e.key_at(q).is_some_and(|k| h.has(k));
                Some((p > 0 && beside(p - 1)) || beside(p + 1))
            })
        });
        if near == Some(true) {
            if let Some(sig) = self.holder.with_value(|h| h.hold(t.clone())) {
                self.held.update(|h| {
                    h.rows.insert((t.shortcode.to_string(), t.seq), (t.id.to_string(), sig));
                });
            }
        } else {
            cache::put(t.clone());
        }
    }

    /// ACT-5: `deleted`: each ticket is a hole in the extents now — the
    /// count, the scrollbar and every position after it move — and its row
    /// is given back. (Under a filter, the membership watch's `left` says it.)
    pub(crate) fn deleted(self, ids: &[String]) {
        let keys: Vec<(String, u32)> = ids.iter().filter_map(|id| crate::kids::key(id).map(|(e, s)| (e.to_string(), s))).collect();
        self.ext.update(|e| {
            if let Some(e) = e {
                for (epic, seq) in &keys {
                    e.gone(epic, *seq);
                }
            }
        });
        let held: Vec<String> = self.held.with_untracked(|h| keys.iter().filter_map(|k| h.rows.get(k).map(|(id, _)| id.clone())).collect());
        self.held.update(|h| {
            for k in &keys {
                h.rows.remove(k);
                h.absent.remove(k);
            }
        });
        self.holder.with_value(|holder| held.iter().for_each(|id| holder.release(id)));
    }

    /// A page asked for after `after` came back: hold its rows (an update for
    /// the ones held already), extend the extents by any row past them, and
    /// note the rows it passed over. True when it told the board anything.
    fn place(self, rows: Vec<TicketSummary>, after: Option<&str>) -> bool {
        let past: Vec<(String, u32)> = self.ext.with_untracked(|e| {
            e.as_ref().map_or_else(Vec::new, |e| rows.iter().filter(|t| e.pos(t.shortcode.as_str(), t.seq).is_none()).map(|t| (t.shortcode.to_string(), t.seq)).collect())
        });
        if !past.is_empty() {
            self.ext.update(|e| {
                if let Some(e) = e {
                    for (epic, seq) in &past {
                        e.born(epic, *seq);
                    }
                }
            });
        }
        let keys: Vec<(Key, String)> = rows.iter().map(|t| ((t.shortcode.to_string(), t.seq), t.id.to_string())).collect();
        let fresh = self.held.with_untracked(|h| keys.iter().any(|(k, _)| !h.rows.contains_key(k)));
        let page: Vec<Key> = keys.iter().map(|(k, _)| k.clone()).collect();
        let gaps = self.ext.with_untracked(|e| e.as_ref().map_or_else(Vec::new, |e| holes(e, after, &page)));
        if !self.holder.with_value(|h| h.hold_page(rows)) {
            return false;
        }
        let gone: Vec<String> = self.held.with_untracked(|h| gaps.iter().filter_map(|k| h.rows.get(k).map(|(id, _)| id.clone())).collect());
        let new_gaps = self.held.with_untracked(|h| gaps.iter().any(|k| !h.absent.contains(k)));
        if fresh || new_gaps || !gone.is_empty() {
            self.held.update(|h| {
                for (k, id) in keys {
                    h.absent.remove(&k);
                    if let Some(sig) = cache::get(&id) {
                        h.rows.entry(k).or_insert((id, sig));
                    }
                }
                for k in gaps {
                    h.rows.remove(&k);
                    h.absent.insert(k);
                }
            });
        }
        self.holder.with_value(|h| gone.iter().for_each(|id| h.release(id)));
        fresh || new_gaps || !gone.is_empty()
    }

    /// Give back every row outside `keep` (and any the extents no longer
    /// reach). Touches the rows' signal only when something goes.
    fn keep(self, lo: usize, hi: usize) {
        let gone: Vec<(Key, String)> = self.ext.with_untracked(|e| {
            let Some(e) = e.as_ref() else { return Vec::new() };
            self.held.with_untracked(|h| {
                h.rows
                    .iter()
                    .filter(|((epic, seq), _)| e.pos(epic, *seq).is_none_or(|p| p < lo || p >= hi))
                    .map(|(k, (id, _))| (k.clone(), id.clone()))
                    .collect()
            })
        });
        if gone.is_empty() {
            return;
        }
        self.held.update(|h| {
            for (k, _) in &gone {
                h.rows.remove(k);
            }
        });
        self.holder.with_value(|h| gone.iter().for_each(|(_, id)| h.release(id)));
    }

    /// Ask again for what is shown — the session's answer to a new
    /// connection and to a `lagged`, after it has the extents again: the
    /// window's rows in one page, applied in place (the rows keep their DOM),
    /// and every row outside the window given back, since it may be stale.
    /// An answer still out from before is not applied. `Err` only when the
    /// socket died.
    pub(crate) async fn refresh(self, client: &Client) -> Result<(), String> {
        // FILTER-12: under a filter, its counts and then its window.
        if self.filtered_untracked() {
            return self.refresh_lensed(client, true).await;
        }
        let gen = self.gen.get_value().wrapping_add(1);
        self.gen.set_value(gen);
        self.flight.set_value(Some(gen));
        self.strikes.set_value(((0, 0), 0));
        let (lo, hi) = self.window();
        let result = if hi > lo {
            let after = self.ext.with_untracked(|e| self.held.with_untracked(|h| e.as_ref().and_then(|e| cursor(e, h, lo))));
            match ask(client, after.clone(), hi - lo).await {
                Answer::Page(rows) => {
                    self.place(rows, after.as_deref());
                    Ok(())
                }
                // A hole under the cursor: the board's own asking walks back.
                Answer::Refused => Ok(()),
                Answer::Offline(e) => Err(e),
            }
        } else {
            Ok(())
        };
        if self.gen.get_value() == gen {
            let (lo, hi) = self.window();
            self.keep(lo, hi);
        }
        if self.flight.get_value() == Some(gen) {
            self.flight.set_value(None);
        }
        self.retry.notify();
        result
    }

    /// The board's asking, for the App's lifetime: whenever the window, the
    /// extents or what is held move, what is far from the window is given
    /// back and the window's missing rows are asked for — one page, one
    /// request in flight.
    pub(crate) fn drive(self) {
        Effect::new(move |_| {
            // FILTER-12: under a filter, its window over the matches.
            if let Some(lens) = self.lens.get() {
                self.drive_lensed(lens);
                return;
            }
            self.retry.track();
            let Some(n) = self.ext.with(|e| e.as_ref().map(Extents::n)) else { return };
            let l = board_layout(n, self.geom.get());
            let (lo, hi) = (l.start.min(n), l.end.min(n));
            self.held.track();
            // Far from the window is far, whether or not the window is in
            // hand yet: a scroll that never stops long enough for a page to
            // land still gives back what it leaves behind.
            self.keep(lo.saturating_sub(KEEP), hi + KEEP);
            if self.flight.get_value().is_some() {
                return;
            }
            if self.strikes.with_value(|(w, k)| *w == (lo, hi) && *k >= 3) {
                return; // until the window, or the connection, moves
            }
            let gap = self.ext.with_untracked(|e| self.held.with_untracked(|h| e.as_ref().and_then(|e| missing(e, h, lo, hi))));
            let Some((from, last)) = gap else { return };
            let after = self.ext.with_untracked(|e| self.held.with_untracked(|h| e.as_ref().and_then(|e| cursor(e, h, from))));
            let gen = self.gen.get_value();
            self.flight.set_value(Some(gen));
            spawn_local(async move {
                let answer = match session::client() {
                    Some(client) => ask(&client, after.clone(), last + 1 - from).await,
                    None => Answer::Offline("not connected".into()),
                };
                if self.flight.try_get_value() == Some(Some(gen)) {
                    self.flight.set_value(None);
                }
                if self.gen.try_get_value() != Some(gen) {
                    self.retry.notify(); // a refresh went out meanwhile; a no-op once the app is gone
                    return;
                }
                let told = match answer {
                    Answer::Page(rows) => self.place(rows, after.as_deref()),
                    // The cursor named no ticket: a hole. The next ask walks
                    // back over it.
                    Answer::Refused => {
                        let hole = self.ext.with_untracked(|e| {
                            let e = e.as_ref()?;
                            let p = after.as_deref().and_then(|id| e.pos_of(id))?;
                            e.key_at(p).map(owned)
                        });
                        match hole {
                            Some(k) => {
                                self.held.update(|h| {
                                    h.absent.insert(k);
                                });
                                true
                            }
                            None => false,
                        }
                    }
                    // The session sees the socket go and asks again when it is back.
                    Answer::Offline(_) => return,
                };
                self.strikes.update_value(|(w, k)| {
                    if *w != (lo, hi) || told {
                        *w = (lo, hi);
                        *k = 0;
                    }
                    if !told {
                        *k += 1;
                    }
                });
                if !told {
                    self.retry.notify();
                }
            });
        });
    }

    // -----------------------------------------------------------------
    // FILTER-12: under a filter

    /// The board follows its effective filter (global ∧ board) for the App's
    /// lifetime: set now — before the session asks for anything — and again
    /// whenever it changes.
    pub(crate) fn follow(self, effective: Signal<menger_tickets_api::filter::Filter>) {
        self.relens(effective.with_untracked(Lensing::of));
        Effect::new(move |_| {
            let next = effective.with(Lensing::of);
            if self.lens.with_untracked(|l| *l != next) {
                self.relens(next);
            }
        });
    }

    /// A new filter, or none: what was held for the old one given back; the
    /// membership watch for the new one opened before the old one is
    /// dropped; then its counts and its window — now, on the connection
    /// there is, or by the session once one is up. No filter: SCALE-26's
    /// board, its window asked after the extents again.
    fn relens(self, next: Option<Lensing>) {
        let was = self.lens.get_untracked();
        if was == next {
            return;
        }
        self.gen.update_value(|g| *g = g.wrapping_add(1));
        self.flight.set_value(None);
        self.strikes.set_value(((0, 0), 0));
        self.resyncs.set_value(0);
        if was.is_none() {
            let gone: Vec<String> = self.held.with_untracked(|h| h.rows.values().map(|(id, _)| id.clone()).collect());
            self.held.set(Held::default());
            self.holder.with_value(|h| gone.iter().for_each(|id| h.release(id)));
        }
        let gone = self.ranked.try_update(Ranked::clear).unwrap_or_default();
        self.holder.with_value(|h| gone.iter().for_each(|(id, _)| h.release(id)));
        self.matches.set(None);
        self.refused.set(None);
        let watch = next.as_ref().filter(|l| !l.nothing).map(|l| l.text.clone());
        match (self.member.get_value(), watch) {
            (Some(m), Some(text)) => m.replace(text),
            (Some(m), None) => {
                m.close();
                self.member.set_value(None);
            }
            (None, Some(text)) => self.member.set_value(Some(session::Membership::open(text, move |ev| self.heard(ev)))),
            (None, None) => {}
        }
        let lensed = next.is_some();
        self.lens.set(next);
        if lensed {
            if let Some(client) = session::client() {
                spawn_local(async move {
                    let _ = self.refresh_lensed(&client, true).await;
                });
            }
        }
    }

    /// A membership event: `entered` and `left` move the counts and the rows
    /// held (a row entering beside a held one is held — from the row cache,
    /// or the row `watch` just carried; elsewhere nothing is fetched); a
    /// `recount` sets the counts and asks the window again, the rows keeping
    /// their DOM; a `lagged` asks the counts, then the window. `Opened` asks
    /// nothing: the session asks for what is shown on every connection, and
    /// a new filter asks for itself.
    fn heard(self, ev: &session::Member) {
        use session::Member;
        if !self.filtered_untracked() {
            return;
        }
        match ev {
            Member::Entered { id, epic, seq } => {
                self.moved.update_value(|n| *n = n.wrapping_add(1));
                let Some(mut m) = self.matches.get_untracked() else { return };
                let landed = self.ranked.try_update(|r| r.entered(&mut m, epic, *seq));
                self.matches.set(Some(m));
                if let Some(Landed::At { rank: Some(rank), beside: true, .. }) = landed {
                    let sig = if self.holder.with_value(|h| h.hold_id(id)) {
                        cache::get(id)
                    } else {
                        session::recent(id).and_then(|t| self.holder.with_value(|h| h.hold(t)))
                    };
                    if let Some(sig) = sig {
                        self.ranked.update(|r| r.insert((epic.clone(), *seq), (id.clone(), sig), rank));
                    }
                }
            }
            Member::Left { epic, seq, .. } => {
                self.moved.update_value(|n| *n = n.wrapping_add(1));
                let Some(mut m) = self.matches.get_untracked() else { return };
                let gone = self.ranked.try_update(|r| r.left(&mut m, epic, *seq)).flatten();
                self.matches.set(Some(m));
                if let Some((id, _)) = gone {
                    self.holder.with_value(|h| h.release(&id));
                }
            }
            Member::Recount { epic, held } => {
                self.moved.update_value(|n| *n = n.wrapping_add(1));
                let Some(mut m) = self.matches.get_untracked() else { return };
                self.ranked.update(|r| r.recount(&mut m, epic, *held));
                self.matches.set(Some(m));
                self.again(false);
            }
            Member::Lagged => self.again(true),
            Member::Refused(message) => self.refused.set(Some(message.clone())),
            Member::Opened => {}
        }
    }

    /// Ask again, on the connection there is — the counts first when
    /// `counts`.
    fn again(self, counts: bool) {
        if let Some(client) = session::client() {
            spawn_local(async move {
                let _ = self.refresh_lensed(&client, counts).await;
            });
        }
    }

    /// The board under its filter asked again: its counts (when `counts`,
    /// or when it has none), then the window's rows in one page, placed over
    /// the rows held (they keep their DOM); every row outside the window
    /// given back. A filter that admits nothing asks nothing. An answer to
    /// an older question is not applied. `Err` only when the socket died.
    async fn refresh_lensed(self, client: &Client, counts: bool) -> Result<(), String> {
        let Some(lens) = self.lens.get_untracked() else { return Ok(()) };
        let gen = self.gen.get_value().wrapping_add(1);
        self.gen.set_value(gen);
        self.flight.set_value(Some(gen));
        self.strikes.set_value(((0, 0), 0));
        let mut result = self.ask_lensed(client, &lens, gen, counts).await;
        // A page the counts did not place: they are stale — once more, the
        // counts first.
        if result == Ok(false) && !counts && self.gen.get_value() == gen {
            result = self.ask_lensed(client, &lens, gen, true).await;
        }
        if self.gen.get_value() == gen {
            let (lo, hi) = self.window();
            self.keep_lensed(lo, hi);
        }
        if self.flight.get_value() == Some(gen) {
            self.flight.set_value(None);
        }
        self.retry.notify();
        result.map(|_| ())
    }

    /// The asking behind [`Board::refresh_lensed`]: `Ok(false)` when the
    /// window's page did not fit the counts.
    async fn ask_lensed(self, client: &Client, lens: &Lensing, gen: u32, counts: bool) -> Result<bool, String> {
        if lens.nothing {
            self.matches.set(Some(Matches::default()));
            return Ok(true);
        }
        if counts || self.matches.with_untracked(Option::is_none) {
            match count(client, &lens.text).await {
                Counted::Counts(runs) => {
                    if self.gen.get_value() != gen {
                        return Ok(true);
                    }
                    self.matches.set(Some(Matches::new(runs)));
                    self.refused.set(None);
                }
                Counted::Refused(message) => {
                    if self.gen.get_value() == gen {
                        self.matches.set(Some(Matches::default()));
                        self.refused.set(Some(message));
                    }
                    return Ok(true);
                }
                Counted::Offline(e) => return Err(e),
            }
        }
        let (lo, hi) = self.window();
        if hi <= lo {
            return Ok(true);
        }
        let moved = self.moved.get_value();
        match ask_at(client, &lens.text, lo, hi - lo).await {
            Window::Page { rows, first, total } => {
                if self.gen.get_value() != gen || self.moved.get_value() != moved {
                    return Ok(true); // the board's own asking fills what moved
                }
                Ok(self.place_lensed(rows, first, total).is_ok())
            }
            Window::Refused(message) => {
                self.refused.set(Some(message));
                Ok(true)
            }
            Window::Offline(e) => Err(e),
        }
    }

    /// A page of the matches came back: held, ranked where the counts put
    /// it, and the rows it displaced given back — `Ok(true)` when it told
    /// the board anything; [`Stale`] (nothing held) when it does not fit.
    fn place_lensed(self, rows: Vec<TicketSummary>, first: usize, total: usize) -> Result<bool, Stale> {
        let keys: Vec<Key> = rows.iter().map(|t| (t.shortcode.to_string(), t.seq)).collect();
        self.matches.with_untracked(|m| Ranked::<Row>::fits(m.as_ref().ok_or(Stale)?, first, Some(total), &keys))?;
        self.resyncs.set_value(0);
        if !self.holder.with_value(|h| h.hold_page(rows.clone())) {
            return Ok(false);
        }
        let page: Vec<(Key, Row)> = rows.into_iter().filter_map(|t| cache::get(&t.id).map(|sig| ((t.shortcode.to_string(), t.seq), (t.id.into_string(), sig)))).collect();
        let placed = self.matches.with_untracked(|m| {
            let m = m.as_ref()?;
            self.ranked.try_update(|r| r.place(m, first, Some(total), page))
        });
        match placed {
            Some(Ok(p)) => {
                self.holder.with_value(|h| p.gone.iter().for_each(|(id, _)| h.release(id)));
                Ok(p.fresh || !p.gone.is_empty())
            }
            Some(Err(s)) => Err(s),
            None => Ok(false),
        }
    }

    /// Give back every row held under the filter outside `lo..hi`. Touches
    /// the rows' signal only when something goes.
    fn keep_lensed(self, lo: usize, hi: usize) {
        let any = self.matches.with_untracked(|m| m.as_ref().is_some_and(|m| self.ranked.with_untracked(|r| r.outside(m, lo, hi))));
        if !any {
            return;
        }
        let gone = self.matches.with_untracked(|m| {
            let m = m.as_ref()?;
            self.ranked.try_update(|r| r.keep(m, lo, hi))
        });
        self.holder.with_value(|h| gone.into_iter().flatten().for_each(|(id, _)| h.release(&id)));
    }

    /// [`Board::drive`] under a filter: what is far from the window given
    /// back, and the window's missing positions asked for — `list { filter,
    /// at, limit, summary }`, one page, one request in flight. A page asked
    /// before a membership event moved the positions is asked again; one the
    /// counts do not place asks the counts again (twice at most, until a
    /// page fits).
    fn drive_lensed(self, lens: Lensing) {
        self.retry.track();
        let Some(n) = self.matches.with(|m| m.as_ref().map(Matches::n)) else { return };
        let l = board_layout(n, self.geom.get());
        let (lo, hi) = (l.start.min(n), l.end.min(n));
        self.ranked.track();
        self.keep_lensed(lo.saturating_sub(KEEP), hi + KEEP);
        if self.flight.get_value().is_some() {
            return;
        }
        if self.strikes.with_value(|(w, k)| *w == (lo, hi) && *k >= 3) {
            return; // until the window, or the connection, moves
        }
        let gap = self.matches.with_untracked(|m| self.ranked.with_untracked(|r| r.missing(m.as_ref()?, lo, hi)));
        let Some((from, last)) = gap else { return };
        let gen = self.gen.get_value();
        let moved = self.moved.get_value();
        self.flight.set_value(Some(gen));
        spawn_local(async move {
            let answer = match session::client() {
                Some(client) => ask_at(&client, &lens.text, from, last + 1 - from).await,
                None => Window::Offline("not connected".into()),
            };
            if self.flight.try_get_value() == Some(Some(gen)) {
                self.flight.set_value(None);
            }
            if self.gen.try_get_value() != Some(gen) || self.moved.try_get_value() != Some(moved) {
                self.retry.notify(); // asked again; a no-op once the app is gone
                return;
            }
            let told = match answer {
                Window::Page { rows, first, total } => match self.place_lensed(rows, first, total) {
                    Ok(told) => told,
                    Err(Stale) => {
                        if self.resyncs.get_value() < 2 {
                            self.resyncs.update_value(|n| *n += 1);
                            self.again(true);
                            return;
                        }
                        false
                    }
                },
                Window::Refused(message) => {
                    self.refused.set(Some(message));
                    false
                }
                // The session sees the socket go and asks again when it is back.
                Window::Offline(_) => return,
            };
            self.strikes.update_value(|(w, k)| {
                if *w != (lo, hi) || told {
                    *w = (lo, hi);
                    *k = 0;
                }
                if !told {
                    *k += 1;
                }
            });
            if !told {
                self.retry.notify();
            }
        });
    }

    /// FILTER-12: where `id` sits among the board's matches — where the
    /// board holds it, else the server's word: `list { filter, after: id,
    /// limit: 0 }`'s `first` is how many matches there are up to it, so it
    /// sits at `first - 1` (or, not a match, the last match before it does).
    /// A deep link's position; unfiltered, the extents'.
    pub(crate) async fn locate(self, id: &str) -> Option<usize> {
        if let Some(p) = self.pos_of_untracked(id) {
            return Some(p);
        }
        let lens = self.lens.get_untracked()?;
        if lens.nothing {
            return None;
        }
        let client = session::client()?;
        let first = ask_place(&client, &lens.text, id).await?;
        first.checked_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{cursor, holes, missing, ranked_slots, slots, Extents, Held, Key, Landed, Lensing, Matches, Placed, Ranked, Slot, Stale};
    use menger_tickets_api::filter::Filter;
    use leptos::prelude::*;
    use menger_tickets_api::{Kind, Status, TicketSummary};

    // FILTER-12 ------------------------------------------------------

    fn k(id: &str) -> Key {
        let (e, s) = id.rsplit_once('-').expect("EPIC-N");
        (e.to_string(), s.parse().expect("seq"))
    }

    fn matches() -> Matches {
        // Out of order, a shortcode with none, one said twice (its last word).
        Matches::new(vec![("MFE".into(), 3), ("AGS".into(), 9), ("EMPTY".into(), 0), ("ZZ".into(), 1), ("AGS".into(), 12)])
    }

    /// FILTER-12: a position is (shortcode, rank) by prefix sums over the
    /// counts, and back — in the store's order, across shortcodes; nothing
    /// past the end or past a shortcode's last match.
    #[test]
    fn a_position_is_a_shortcode_and_a_rank_and_back() {
        let m = matches();
        assert_eq!(m.n(), 16);
        assert_eq!(m.runs().iter().map(|(e, h)| (e.as_str(), *h)).collect::<Vec<_>>(), [("AGS", 12), ("MFE", 3), ("ZZ", 1)]);
        assert_eq!(m.locate(0), Some(("AGS", 0)));
        assert_eq!(m.locate(11), Some(("AGS", 11)));
        assert_eq!(m.locate(12), Some(("MFE", 0)));
        assert_eq!(m.locate(15), Some(("ZZ", 0)));
        assert_eq!(m.locate(16), None);
        for p in 0..m.n() {
            let (e, r) = m.locate(p).unwrap();
            assert_eq!(m.pos(e, r), Some(p), "{p}");
        }
        assert_eq!(m.pos("MFE", 3), None, "past its last match");
        assert_eq!(m.pos("EMPTY", 0), None);
        assert_eq!(m.pos("NOPE", 0), None);
        assert_eq!((m.start("AGS"), m.start("MFE"), m.start("ZZ")), (0, 12, 15));
        assert_eq!(m.start("BAM"), 12, "a shortcode with none starts where it would");
        assert_eq!(m.start("ZZZ"), 16);
        assert_eq!(m.start("A"), 0);
        assert_eq!((m.held("AGS"), m.held("EMPTY"), m.held("NOPE")), (12, 0, 0));
        assert_eq!(Matches::default().locate(0), None);
        assert_eq!(Matches::new(Vec::new()).n(), 0);
    }

    /// FILTER-12: `entered` raises a shortcode's count (a new one lands at
    /// its place), `left` lowers it (never below none; at none the
    /// shortcode goes), `recount` sets it — and every position after moves.
    #[test]
    fn membership_moves_the_counts() {
        let mut m = matches();
        m.entered("MFE");
        assert_eq!((m.n(), m.start("ZZ"), m.locate(15)), (17, 16, Some(("MFE", 3))));
        m.entered("BAM");
        assert_eq!((m.n(), m.start("BAM"), m.start("MFE"), m.pos("BAM", 0)), (18, 12, 13, Some(12)));
        m.left("BAM");
        assert_eq!((m.n(), m.held("BAM"), m.start("MFE")), (17, 0, 12));
        m.left("NOPE");
        assert_eq!(m.n(), 17, "a shortcode with none loses nothing");
        m.recount("AGS", 5);
        assert_eq!((m.n(), m.start("MFE"), m.locate(5)), (10, 5, Some(("MFE", 0))));
        m.recount("NEW", 0);
        assert_eq!(m.n(), 10);
        m.recount("NEW", 2);
        assert_eq!((m.n(), m.start("NEW"), m.start("ZZ")), (12, 9, 11));
        m.recount("MFE", 0);
        assert_eq!((m.n(), m.held("MFE"), m.start("NEW")), (8, 0, 5));
    }

    /// A shortcode A whose matches are A-2, A-4, A-7, A-9, A-12 and B's are
    /// B-1, B-5, B-6; the board holds A-4, A-7, A-9 and B-1 (positions 1, 2,
    /// 3 and 5).
    fn held_ab() -> (Matches, Ranked<String>) {
        let m = Matches::new(vec![("A".into(), 5), ("B".into(), 3)]);
        let mut r = Ranked::default();
        for (id, rank) in [("A-4", 1), ("A-7", 2), ("A-9", 3), ("B-1", 0)] {
            r.insert(k(id), id.to_string(), rank);
        }
        (m, r)
    }

    fn positions(m: &Matches, r: &Ranked<String>) -> Vec<(usize, String)> {
        r.window(m, 0, m.n()).into_iter().map(|(p, _, t)| (p, t.clone())).collect()
    }

    /// FILTER-12: the window under a filter is the rows held at their
    /// positions, and what is missing in it — one page's worth.
    #[test]
    fn a_held_row_sits_at_its_shortcodes_start_plus_its_rank() {
        let (m, r) = held_ab();
        assert_eq!(positions(&m, &r), [(1, "A-4".into()), (2, "A-7".into()), (3, "A-9".into()), (5, "B-1".into())]);
        assert_eq!(r.pos_of(&m, &k("B-1")), Some(5));
        assert_eq!(r.pos_of(&m, &k("A-2")), None, "not held");
        assert_eq!(r.at(&m, 2).map(|(_, t)| t.as_str()), Some("A-7"));
        assert_eq!(r.at(&m, 0), None);
        assert_eq!(r.missing(&m, 0, 8), Some((0, 7)));
        assert_eq!(r.missing(&m, 1, 4), None);
        assert_eq!(r.missing(&m, 1, 6), Some((4, 4)));
        assert_eq!(r.missing(&m, 6, 99), Some((6, 7)), "clamped to the matches");
    }

    /// FILTER-12: `entered` and `left` applied to counts and held rows — a
    /// row after it in its shortcode moves a rank; one that lands between two
    /// held rows (or after its shortcode's held last, or first of a
    /// shortcode with none) is placed and, beside a held row, held; one the
    /// held rows cannot place moves only the counts; one held already moves
    /// nothing.
    #[test]
    fn entered_and_left_move_counts_and_held_rows() {
        let (mut m, mut r) = held_ab();
        // A-8, between A-7 (rank 2) and A-9 (rank 3): rank 3, beside A-7.
        let landed = r.entered(&mut m, "A", 8);
        assert_eq!(landed, Landed::At { pos: Some(3), rank: Some(3), beside: true });
        r.insert(k("A-8"), "A-8".into(), 3);
        assert_eq!(m.n(), 9);
        assert_eq!(positions(&m, &r), [(1, "A-4".into()), (2, "A-7".into()), (3, "A-8".into()), (4, "A-9".into()), (6, "B-1".into())]);
        // A-13: after A-9 (rank 4), but A-12 (rank 5) is not held — where
        // it lands is not known; the counts move, B-1 with them.
        assert_eq!(r.entered(&mut m, "A", 13), Landed::At { pos: None, rank: None, beside: false });
        assert_eq!((m.held("A"), r.pos_of(&m, &k("B-1"))), (7, Some(7)));
        // A-1: before every held A row, the first held not at rank 0.
        assert_eq!(r.entered(&mut m, "A", 1), Landed::At { pos: None, rank: None, beside: false });
        assert_eq!(positions(&m, &r)[..2], [(2, "A-4".into()), (3, "A-7".into())]);
        // Held already: a member before — nothing moves.
        assert_eq!(r.entered(&mut m, "A", 7), Landed::Held);
        assert_eq!(m.n(), 11);
        // A-7 leaves: dropped and handed back; A-8 and A-9 a rank nearer.
        assert_eq!(r.left(&mut m, "A", 7), Some("A-7".into()));
        assert_eq!(positions(&m, &r), [(2, "A-4".into()), (3, "A-8".into()), (4, "A-9".into()), (7, "B-1".into())]);
        // A-3 leaves, not held: A-4 and after a rank nearer.
        assert_eq!(r.left(&mut m, "A", 3), None);
        assert_eq!(positions(&m, &r), [(1, "A-4".into()), (2, "A-8".into()), (3, "A-9".into()), (6, "B-1".into())]);
        assert_eq!(m.n(), 9);
        // After A's held last when it is A's last: placed at the end of A,
        // beside it.
        let mut m2 = Matches::new(vec![("A".into(), 2), ("B".into(), 1)]);
        let mut r2: Ranked<String> = Ranked::default();
        r2.insert(k("A-5"), "A-5".into(), 1);
        assert_eq!(r2.entered(&mut m2, "A", 9), Landed::At { pos: Some(2), rank: Some(2), beside: true });
        // The first of a shortcode with none: rank 0, beside B's first held.
        r2.insert(k("B-1"), "B-1".into(), 0);
        assert_eq!(r2.entered(&mut m2, "AB", 4), Landed::At { pos: Some(3), rank: Some(0), beside: true });
        assert_eq!((m2.n(), r2.pos_of(&m2, &k("B-1"))), (5, Some(4)));
        // Before the held rank-0 row of its shortcode: rank 0.
        assert_eq!(r2.entered(&mut m2, "B", 0), Landed::At { pos: Some(4), rank: Some(0), beside: true });
        // Far from every held row: placed, not beside.
        let mut m3 = Matches::new(vec![("A".into(), 0), ("C".into(), 9)]);
        let mut r3: Ranked<String> = Ranked::default();
        r3.insert(k("C-40"), "C-40".into(), 8);
        assert_eq!(r3.entered(&mut m3, "A", 1), Landed::At { pos: Some(0), rank: Some(0), beside: false });
    }

    /// FILTER-12: `recount` sets the counts; the shortcode's held rows keep
    /// their rows (and their DOM) until the window is asked again, each
    /// position drawn once meanwhile.
    #[test]
    fn a_recount_sets_the_counts_and_keeps_the_rows() {
        let (mut m, mut r) = held_ab();
        r.recount(&mut m, "A", 2);
        assert_eq!(m.n(), 5);
        assert_eq!(r.len(), 4, "rows kept until the window is asked again");
        assert_eq!(positions(&m, &r), [(1, "A-4".into()), (2, "B-1".into())], "A-7, A-9 past A's count; B moved up");
        r.recount(&mut m, "B", 0);
        assert_eq!((m.n(), r.pos_of(&m, &k("B-1"))), (2, None));
    }

    /// FILTER-12: the board is filtered exactly when its effective filter
    /// has a group — the wire gets the groups' text (a display part filters
    /// nothing, and is not the server's); a clash asks nothing.
    #[test]
    fn the_board_is_filtered_by_its_effective_filters_groups() {
        let of = |t: &str| Lensing::of(&Filter::parse(t).expect("parses"));
        assert_eq!(of(""), None);
        assert_eq!(of("· tree"), None, "a display part alone is SCALE-26's board");
        assert_eq!(of("status Ready"), Some(Lensing { text: "status Ready".into(), nothing: false }));
        assert_eq!(of("not kind doc and status Ready · flat"), Some(Lensing { text: "not kind doc and status Ready".into(), nothing: false }));
        assert_eq!(of("status Ready and status Coding").map(|l| l.nothing), Some(true));
        let global = Filter::parse("not kind doc").unwrap();
        let own = Filter::parse("status Ready").unwrap();
        assert_eq!(Lensing::of(&global.and(&own)).map(|l| l.text), Some("not kind doc and status Ready".into()), "global ∧ board");
    }

    /// FILTER-12: the window drawn under a filter — the rows held at their
    /// positions, the stretches between them gaps of their length, nothing
    /// past the matches.
    #[test]
    fn a_filtered_window_is_rows_at_their_positions_and_gaps() {
        let m = Matches::new(vec![("A".into(), 5), ("B".into(), 3)]);
        let mut r = Ranked::default();
        for (id, rank) in [("A-4", 1), ("A-7", 2), ("B-1", 0)] {
            let t = row(id);
            r.insert(k(id), (id.to_string(), ArcRwSignal::new(t)), rank);
        }
        let keys = |lo, hi| ranked_slots(&m, &r, lo, hi).iter().map(Slot::key).collect::<Vec<_>>();
        assert_eq!(keys(0, 8), ["?0+1", "A-4", "A-7", "?3+2", "B-1", "?6+2"]);
        assert_eq!(keys(1, 3), ["A-4", "A-7"]);
        assert_eq!(keys(4, 99), ["?4+1", "B-1", "?6+2"], "clamped to the matches");
        assert!(keys(8, 20).is_empty());
        assert!(ranked_slots(&Matches::default(), &r, 0, 10).is_empty());
    }

    /// FILTER-12: a page at `first` — its rows are the matches there:
    /// ranked, held rows re-ranked, a held row it passed over (or whose
    /// position it took) handed back; a page whose rows the counts put in
    /// another shortcode, or whose total is not theirs, is stale and changes
    /// nothing.
    #[test]
    fn a_page_is_placed_where_the_counts_put_it_or_is_stale() {
        let (m, mut r) = held_ab();
        let page = |ids: &[&str]| ids.iter().map(|id| (k(id), id.to_string())).collect::<Vec<_>>();
        assert_eq!(r.place(&m, 4, None, page(&["A-12", "A-13"])), Err(Stale), "position 5 is B's");
        assert_eq!(r.place(&m, 0, Some(9), page(&["A-2"])), Err(Stale), "the total is 8");
        assert_eq!(r.len(), 4, "a stale page changes nothing");
        // A-7 is not a match any more: the page brings A-2, A-4, A-9.
        let placed = r.place(&m, 0, Some(8), page(&["A-2", "A-4", "A-9"])).unwrap();
        assert_eq!(placed, Placed { fresh: true, gone: vec!["A-7".to_string()] });
        assert_eq!(positions(&m, &r), [(0, "A-2".into()), (1, "A-4".into()), (2, "A-9".into()), (5, "B-1".into())]);
        // Again: nothing new.
        assert_eq!(r.place(&m, 1, Some(8), page(&["A-4", "A-9"])).unwrap(), Placed { fresh: false, gone: Vec::new() });
        // A row whose stale rank claims a position the page fills.
        r.insert(k("B-9"), "B-9".into(), 1);
        let placed = r.place(&m, 5, Some(8), page(&["B-1", "B-5"])).unwrap();
        assert_eq!(placed.gone, vec!["B-9".to_string()]);
        // Outside the window: given back.
        assert_eq!(r.keep(&m, 0, 2), vec!["A-9".to_string(), "B-1".to_string(), "B-5".to_string()]);
        assert_eq!(r.clear(), vec!["A-2".to_string(), "A-4".to_string()]);
        assert_eq!(r.len(), 0);
        assert_eq!(r.place(&m, 0, Some(8), Vec::new()).unwrap(), Placed { fresh: false, gone: Vec::new() });
    }

    fn ext() -> Extents {
        // Out of order on purpose, an empty shortcode, a multi-digit seq.
        Extents::new(vec![("MFE".into(), 3), ("AGS".into(), 12), ("EMPTY".into(), 0), ("ZZ".into(), 1)])
    }

    fn row(id: &str) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').expect("EPIC-N");
        TicketSummary {
            hidden_blocked_by: 0,
            hidden_unlocks: 0,
            parent_hidden: false,
            labels: Vec::new(),
            id: menger_tickets_api::TicketId::new(id),
            epic: menger_tickets_api::Code::stored(epic), shortcode: menger_tickets_api::ShortcodeId::new(epic), address: String::new(),
            seq: seq.parse().expect("seq"),
            title: String::new(),
            status: Status::Pending,
            kind: Kind::Build,
            severity: None,
            parent: None,
            child_count: 0,
            blocked_by: Vec::new(),
            unlocks: Vec::new(),
            commits: Vec::new(),
            path: None,
            placement: None,
            created_unix: 0,
            updated_unix: 0,
        }
    }

    fn held(ids: &[&str]) -> Held {
        let mut h = Held::default();
        for id in ids {
            let t = row(id);
            h.rows.insert((t.shortcode.to_string(), t.seq), (t.id.to_string(), ArcRwSignal::new(t)));
        }
        h
    }

    /// ACT-5: a deleted ticket is a hole — no row, every row after it one
    /// back, the count one less; a hole the extents name, the same; an
    /// undeleted one fills it again; a shortcode whose every ticket went has
    /// no row and keeps its numbers.
    #[test]
    fn a_hole_is_no_row_and_the_rows_close_over_it() {
        let mut e = Extents::holed(vec![("AGS".into(), 5, vec![2]), ("GONE".into(), 2, vec![1, 2]), ("MFE".into(), 3, vec![])]);
        assert_eq!(e.n(), 7);
        let ids: Vec<String> = (0..e.n()).filter_map(|p| e.id_at(p)).collect();
        assert_eq!(ids, ["AGS-1", "AGS-3", "AGS-4", "AGS-5", "MFE-1", "MFE-2", "MFE-3"]);
        for (p, id) in ids.iter().enumerate() {
            assert_eq!(e.pos_of(id), Some(p), "{id}");
        }
        assert_eq!(e.pos("AGS", 2), None, "a hole is no row");
        assert!(e.gone("AGS", 4));
        assert!(!e.gone("AGS", 4), "gone already");
        assert_eq!(e.n(), 6);
        assert_eq!(e.pos_of("AGS-5"), Some(2));
        assert_eq!(e.pos_of("MFE-1"), Some(3));
        assert!(e.born("AGS", 2), "undeleted: the hole filled");
        assert_eq!((e.n(), e.pos_of("AGS-2"), e.pos_of("AGS-3")), (7, Some(1), Some(2)));
        assert!(e.born("GONE", 3), "a new ticket past the deleted ones");
        assert_eq!((e.pos_of("GONE-3"), e.pos_of("MFE-1")), (Some(4), Some(5)));
    }

    /// SCALE-26: row k of the board is `E-j` by the extents alone, and back —
    /// in the store's order, across shortcodes, past nine; nothing outside.
    #[test]
    fn a_position_is_an_id_and_an_id_a_position() {
        let e = ext();
        assert_eq!(e.n(), 16);
        let order: Vec<String> = (0..e.n()).map(|p| e.id_at(p).unwrap()).collect();
        assert_eq!(order[0], "AGS-1");
        assert_eq!(order[9], "AGS-10");
        assert_eq!(order[11], "AGS-12");
        assert_eq!(order[12], "MFE-1");
        assert_eq!(order[15], "ZZ-1");
        for (p, id) in order.iter().enumerate() {
            assert_eq!(e.pos_of(id), Some(p), "{id}");
        }
        assert_eq!(e.id_at(16), None);
        assert_eq!(e.pos_of("AGS-13"), None);
        assert_eq!(e.pos_of("AGS-0"), None);
        assert_eq!(e.pos_of("EMPTY-1"), None);
        assert_eq!(e.pos_of("NOPE-1"), None);
        assert_eq!(e.pos_of("no-dash-seq"), None);
        assert_eq!(Extents::default().id_at(0), None);
    }

    /// A birth moves the count and every position after it — once: an event
    /// the extents already counted (a re-asked `extents`, a page that raced
    /// it) moves nothing. A new shortcode lands at its place in the order.
    #[test]
    fn a_birth_moves_what_comes_after_it_once() {
        let mut e = ext();
        assert!(e.born("AGS", 13));
        assert_eq!((e.n(), e.pos_of("AGS-13"), e.pos_of("MFE-1")), (17, Some(12), Some(13)));
        assert!(!e.born("AGS", 13), "counted already");
        assert!(!e.born("AGS", 2));
        assert!(e.born("BAM", 1));
        assert_eq!((e.n(), e.pos_of("BAM-1"), e.pos_of("MFE-1"), e.pos_of("ZZ-1")), (18, Some(13), Some(14), Some(17)));
        assert!(e.born("ZZ", 4), "events lost behind a lag: the shortcode reaches the newest");
        assert_eq!((e.n(), e.id_at(20).as_deref()), (21, Some("ZZ-4")));
        assert!(!e.born("ZZ", 0));
    }

    /// The window drawn: held rows in order, and the stretches between them
    /// as gaps of their length; the cursor for a gap is the row before it.
    #[test]
    fn a_window_is_rows_and_gaps_and_a_gap_is_asked_after_the_row_before_it() {
        let e = ext();
        let h = held(&["AGS-2", "AGS-3", "AGS-7", "MFE-1"]);
        let drawn: Vec<String> = slots(&e, &h, 0, 14).iter().map(Slot::key).collect();
        assert_eq!(drawn, ["?0+1", "AGS-2", "AGS-3", "?3+3", "AGS-7", "?7+5", "MFE-1", "?13+1"]);
        assert_eq!(slots(&e, &h, 1, 3).iter().map(Slot::key).collect::<Vec<_>>(), ["AGS-2", "AGS-3"]);
        assert!(slots(&e, &h, 16, 20).is_empty());
        assert_eq!(missing(&e, &h, 1, 3), None);
        assert_eq!(missing(&e, &h, 0, 14), Some((0, 13)));
        assert_eq!(missing(&e, &h, 1, 9), Some((3, 7 + 1)));
        assert_eq!(cursor(&e, &h, 0), None, "the top");
        assert_eq!(cursor(&e, &h, 3).as_deref(), Some("AGS-3"));
        assert_eq!(cursor(&e, &h, 12).as_deref(), Some("AGS-12"), "the last of the shortcode before");
        assert_eq!(cursor(&e, &h, 99).as_deref(), Some("ZZ-1"));
    }

    /// SCALE-18: `after` naming a hole is an error, so a hole is stepped over
    /// — the cursor walks back one — and a page that passed over a row tells
    /// the board it is a hole, so it is never asked for again.
    #[test]
    fn a_hole_is_walked_back_over_and_never_asked_for_again() {
        let e = ext();
        let mut h = held(&["AGS-1"]);
        h.absent.insert(("AGS".into(), 4));
        h.absent.insert(("AGS".into(), 3));
        assert_eq!(cursor(&e, &h, 4).as_deref(), Some("AGS-2"));
        assert_eq!(missing(&e, &h, 1, 5), Some((1, 4)));
        let page: Vec<(String, u32)> = [("AGS", 5), ("AGS", 7), ("AGS", 8)].iter().map(|(e, s)| (e.to_string(), *s)).collect();
        assert_eq!(holes(&e, Some("AGS-4"), &page), vec![("AGS".to_string(), 6)]);
        assert_eq!(holes(&e, Some("AGS-2"), &page), vec![("AGS".to_string(), 3), ("AGS".to_string(), 4), ("AGS".to_string(), 6)]);
        assert_eq!(holes(&e, None, &page[..1]), [1, 2, 3, 4].map(|s| ("AGS".to_string(), s)).to_vec(), "from the top");
        assert!(holes(&e, Some("AGS-4"), &[]).is_empty());
        assert!(holes(&e, Some("NOPE-1"), &page).is_empty(), "a cursor the extents do not place says nothing");
    }
}
