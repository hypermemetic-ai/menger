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
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Extents {
    runs: Vec<(String, u32)>,
    before: Vec<usize>,
    n: usize,
}

impl Extents {
    /// From `extents`' answer. A shortcode with no ticket has no rows; the
    /// order is the store's (`String`'s, the `(epic, seq)` key's first half),
    /// which the server already sends — sorted here so a lookup can rely on it.
    pub(crate) fn new(mut runs: Vec<(String, u32)>) -> Self {
        runs.retain(|(_, last)| *last > 0);
        runs.sort_by(|a, b| a.0.cmp(&b.0));
        runs.dedup_by(|b, a| {
            let same = a.0 == b.0;
            if same {
                a.1 = a.1.max(b.1);
            }
            same
        });
        let mut e = Self { runs, before: Vec::new(), n: 0 };
        e.sum();
        e
    }

    fn sum(&mut self) {
        let mut acc = 0usize;
        self.before = self
            .runs
            .iter()
            .map(|(_, last)| {
                let b = acc;
                acc += *last as usize;
                b
            })
            .collect();
        self.n = acc;
    }

    /// Every row of the board: the store's ticket count.
    pub(crate) fn n(&self) -> usize {
        self.n
    }

    /// Where `E-seq` sits — `None` for a shortcode or a sequence the
    /// extents do not reach.
    pub(crate) fn pos(&self, epic: &str, seq: u32) -> Option<usize> {
        let i = self.runs.binary_search_by(|(e, _)| e.as_str().cmp(epic)).ok()?;
        (seq >= 1 && seq <= self.runs[i].1).then(|| self.before[i] + seq as usize - 1)
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
        Some((self.runs[i].0.as_str(), (p - self.before[i]) as u32 + 1))
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
            Ok(i) if seq <= self.runs[i].1 => return false,
            Ok(i) => self.runs[i].1 = seq,
            Err(i) => self.runs.insert(i, (epic.to_string(), seq)),
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
    match client
        .list_all(None, None, None, after, Some(limit.max(1) as u32), Some(true))
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

/// The board's model of the store: the extents, the rows it holds, and the
/// geometry its view last measured (MFE-12's [`Geom`]) — from which the
/// window is worked out whether or not the board is on screen. Made once by
/// the App; the session writes the extents and asks for what is shown, the
/// board's view reads.
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
        }
    }

    /// The extents are in hand (tracked).
    pub(crate) fn ready(self) -> bool {
        self.ext.with(Option::is_some)
    }

    /// The board's extent — every ticket (tracked).
    pub(crate) fn n(self) -> usize {
        self.ext.with(|e| e.as_ref().map_or(0, Extents::n))
    }

    pub(crate) fn n_untracked(self) -> usize {
        self.ext.with_untracked(|e| e.as_ref().map_or(0, Extents::n))
    }

    /// Where `id` sits (tracked: a birth before it moves it).
    pub(crate) fn pos_of(self, id: &str) -> Option<usize> {
        self.ext.with(|e| e.as_ref()?.pos_of(id))
    }

    pub(crate) fn pos_of_untracked(self, id: &str) -> Option<usize> {
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
    pub(crate) fn id_at(self, p: usize) -> Option<String> {
        self.ext.with_untracked(|e| {
            let e = e.as_ref()?;
            let k = owned(e.key_at(p)?);
            self.held.with_untracked(|h| h.rows.get(&k).map(|(id, _)| id.clone())).or_else(|| e.id_at(p))
        })
    }

    /// The window `lo..hi` as the view draws it (tracked).
    pub(crate) fn slots(self, lo: usize, hi: usize) -> Vec<Slot> {
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
    pub(crate) fn set_extents(self, runs: Vec<(String, u32)>) {
        let ext = Extents::new(runs);
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
        let grew = self.ext.with_untracked(|e| e.as_ref().is_some_and(|e| e.pos(&t.epic, t.seq).is_none()));
        if grew {
            self.ext.update(|e| {
                if let Some(e) = e {
                    e.born(&t.epic, t.seq);
                }
            });
        }
        let near = self.ext.with_untracked(|e| {
            let e = e.as_ref()?;
            let p = e.pos(&t.epic, t.seq)?;
            self.held.with_untracked(|h| {
                if h.has((t.epic.as_str(), t.seq)) {
                    return Some(false); // held already: `put` below is the whole story
                }
                let beside = |q: usize| e.key_at(q).is_some_and(|k| h.has(k));
                Some((p > 0 && beside(p - 1)) || beside(p + 1))
            })
        });
        if near == Some(true) {
            if let Some(sig) = self.holder.with_value(|h| h.hold(t.clone())) {
                self.held.update(|h| {
                    h.rows.insert((t.epic.clone(), t.seq), (t.id.clone(), sig));
                });
            }
        } else {
            cache::put(t.clone());
        }
    }

    /// A page asked for after `after` came back: hold its rows (an update for
    /// the ones held already), extend the extents by any row past them, and
    /// note the rows it passed over. True when it told the board anything.
    fn place(self, rows: Vec<TicketSummary>, after: Option<&str>) -> bool {
        let past: Vec<(String, u32)> = self.ext.with_untracked(|e| {
            e.as_ref().map_or_else(Vec::new, |e| rows.iter().filter(|t| e.pos(&t.epic, t.seq).is_none()).map(|t| (t.epic.clone(), t.seq)).collect())
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
        let keys: Vec<(Key, String)> = rows.iter().map(|t| ((t.epic.clone(), t.seq), t.id.clone())).collect();
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
}

#[cfg(test)]
mod tests {
    use super::{cursor, holes, missing, slots, Extents, Held, Slot};
    use leptos::prelude::*;
    use menger_tickets_api::{Kind, Status, TicketSummary};

    fn ext() -> Extents {
        // Out of order on purpose, an empty shortcode, a multi-digit seq.
        Extents::new(vec![("MFE".into(), 3), ("AGS".into(), 12), ("EMPTY".into(), 0), ("ZZ".into(), 1)])
    }

    fn row(id: &str) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').expect("EPIC-N");
        TicketSummary {
            id: id.to_string(),
            epic: epic.to_string(),
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
            h.rows.insert((t.epic.clone(), t.seq), (t.id.clone(), ArcRwSignal::new(t)));
        }
        h
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
