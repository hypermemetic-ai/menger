//! MENGER-39: the **queue store** — the tickets put in front of the owner,
//! each with an urgency, a reason and (at `blocking`) the input that answers
//! it; and the queue's own history.
//!
//! **Behind the cache, as everything stored is.** Read whole at open
//! ([`load`]) into a [`Queue`]; every read answers from it — `queued` is its
//! entries in the queue's order ([`crate::queue::order`]), `history` takes a
//! ticket's records, and the filter cell `queued [urgency]`
//! ([`crate::filter::Event::Queued`]) is answered from the kept entries
//! ([`Queue::holds`]), never by testing a ticket. `asked` is the entries
//! that name an open input and that the reader did not queue
//! ([`Queue::asked`]). A write — `queue`,
//! `unqueue`, `snooze`, and a `close_input` of the named input — takes the
//! write path every write takes ([`Store::write`]): worked out under the
//! write-serial lock from the cache, committed in one transaction (the
//! entry and its history record), then put into the cache and broadcast.
//!
//! **One entry a ticket.** Queueing a queued ticket again raises its urgency
//! and never lowers it; the reason is the new call's, the inputs it names
//! are added to the entry's (APP-86), and a snooze ends.
//!
//! **Several questions** (APP-86): an entry names its inputs in the order
//! asked (`inputs`) and the one the caller is idle on (`blocking` — at
//! urgency `blocking`, required when there are several). Closing a named
//! input is progress; closing the last open one takes the entry out
//! ([`Closing`]). `input` keeps the one an older reader reads: the
//! blocking one, else the first.
//!
//! **A page of it reads what it shows** (V2-20): the cache keeps the entries
//! in the queue's order ([`Queue::in_order`]), so `queued` cuts a page from
//! the kept order, never by sorting the entries; the count beside it walks
//! that order once, and no row is cloned that the page does not hold.
//!
//! **Its history is a log of its own** (`queue_log`), not the transition
//! log: a transition is a status's or a kind's, which `changed`, `by` and
//! `--verify-derived` read as such — and an older release reading a
//! `transitions` row it cannot parse would refuse to open the file. Every
//! queueing, raise, requeue, snooze and unqueue is a [`QueueRecord`]; the
//! `history` read yields a ticket's after its transitions.
//!
//! **Stored** in two tables, `queue` (an entry per queued ticket) and
//! `queue_log` (a record per write) — gained, empty, on open ([`migrate`]).
//! A file v1 wrote keeps its `mockups` and `comment` columns, unread: v2
//! carries neither. A deleted ticket's entry stays in the file and out of
//! every read (its undo brings it back).

use crate::address::ShortcodeId;
use super::history::Act;
use super::lineage::{self, UnderBefore};
use super::{Cache, Change, OrderKey, Row, Store, StoreError};
use crate::access::{Editor, Permit};
use crate::queue::{QueueAction, QueueEntry, QueueRecord, Urgency, INPUTS_MAX, REASON_MAX, SNOOZE_MAX_MINUTES};
use crate::types::{InputState, TicketSummary, TransitionSource};
use roaring::RoaringBitmap;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

/// The tables.
pub(super) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS queue (
            ticket        TEXT PRIMARY KEY,
            urgency       TEXT NOT NULL,
            reason        TEXT NOT NULL,
            input         TEXT,
            inputs        TEXT,
            blocking      TEXT,
            actor         TEXT NOT NULL,
            name          TEXT,
            source        TEXT NOT NULL,
            queued_unix   INTEGER NOT NULL,
            asked_unix    INTEGER NOT NULL,
            snoozed_until INTEGER
        );
        CREATE TABLE IF NOT EXISTS queue_log (
            id         INTEGER PRIMARY KEY,
            ticket     TEXT NOT NULL,
            action     TEXT NOT NULL,
            urgency    TEXT NOT NULL,
            reason     TEXT,
            input      TEXT,
            inputs     TEXT,
            blocking   TEXT,
            at_unix    INTEGER NOT NULL,
            actor      TEXT NOT NULL,
            name       TEXT,
            source     TEXT NOT NULL,
            until_unix INTEGER
        );
        CREATE INDEX IF NOT EXISTS queue_log_by_ticket ON queue_log (ticket, id);
    ";

/// A file without the queue's tables gains them, empty, here — on every
/// open a no-op once they are there. Returns whether they were made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    let had: bool = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'queue_log'",
        [],
        |r| r.get::<_, i64>(0).map(|n| n > 0),
    )?;
    if had {
        return Ok(false);
    }
    conn.execute_batch(TABLES)?;
    tracing::warn!("MENGER-39: the file gained the queue's tables, empty, durably; every later open reads them");
    Ok(true)
}

/// APP-86: a file whose queue has no `inputs` / `blocking` gains them here,
/// in place, on both tables; on every open a no-op once they are there.
pub(crate) fn migrate_inputs(conn: &Connection) -> anyhow::Result<()> {
    for table in ["queue", "queue_log"] {
        for column in ["inputs", "blocking"] {
            if !super::db::has_column(conn, table, column)? {
                conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT"), [])?;
                tracing::warn!("APP-86: the {table} table gained its {column} column, durably");
            }
        }
    }
    Ok(())
}

/// APP-86: a list of names as kept — JSON, `NULL` when empty.
fn list_text(names: &[String]) -> Option<String> {
    (!names.is_empty()).then(|| serde_json::to_string(names).unwrap_or_default())
}

/// …and read back; `NULL` (one input or none, and every row before APP-86)
/// is empty — [`QueueEntry::named`] then reads `input`.
fn list_of(text: Option<String>) -> Vec<String> {
    text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Where an entry stands in the queue's order: most urgent first, then the
/// oldest, then the first queued within a second (its `queued` record's id),
/// then by id.
type Place = (Reverse<Urgency>, i64, u64, String);

/// The tickets a word holds for, per shortcode.
type Held = BTreeMap<ShortcodeId, RoaringBitmap>;

/// The most rows a page of `queued` holds unless asked, and at most.
pub const QUEUED_PAGE: usize = 200;
pub const QUEUED_PAGE_MAX: usize = 1_000;

/// The queue as the cache holds it: each queued ticket's entry, the entries
/// in the queue's order, and each ticket's history records, oldest first.
#[derive(Debug, Default)]
pub(crate) struct Queue {
    entries: HashMap<String, QueueEntry>,
    /// V2-20: what `queued [urgency]` holds for, built once between writes:
    /// every filter and every open watch of the word shares it, so a write
    /// beside 100 of them walks the entries once, not 200 times.
    held: Mutex<HashMap<Option<Urgency>, Arc<Held>>>,
    /// V2-20: what `asked` holds for, by reader, and the inputs' move it was
    /// read at: it turns on the entries and on the inputs they name.
    asked: Mutex<HashMap<String, (u64, Arc<Held>)>>,
    /// V2-20: the entries in the queue's order — what a page is cut from.
    order: BTreeSet<Place>,
    /// V2-20: each ticket's `queued` record's id: its [`Place`]'s third.
    ranks: HashMap<String, u64>,
    /// V2-20: the snoozes, by when each ends.
    snoozes: BTreeSet<(i64, String)>,
    log: HashMap<String, Vec<QueueRecord>>,
    next_id: u64,
}

impl Queue {
    /// `ticket`'s entry, when it is queued.
    pub(crate) fn get(&self, ticket: &str) -> Option<&QueueEntry> {
        self.entries.get(ticket)
    }

    /// `ticket`'s history records, oldest first.
    pub(crate) fn records(&self, ticket: &str) -> &[QueueRecord] {
        self.log.get(ticket).map_or(&[][..], Vec::as_slice)
    }

    /// The id the next record takes.
    pub(super) fn next_id(&self) -> u64 {
        self.next_id.max(1)
    }

    /// `e`'s place at `rank`.
    fn place(e: &QueueEntry, rank: u64) -> Place {
        (Reverse(e.urgency), e.queued_unix, rank, e.ticket.clone())
    }

    /// `ticket` out of the kept order, its entry staying.
    fn unplace(&mut self, ticket: &str) {
        self.forget_sets();
        if let Some(old) = self.entries.get(ticket) {
            let place = Self::place(old, self.ranks.get(ticket).copied().unwrap_or(0));
            self.order.remove(&place);
            if let Some(until) = old.snoozed_until_unix {
                self.snoozes.remove(&(until, ticket.to_string()));
            }
        }
    }

    /// The kept sets, gone with what they were read from.
    fn forget_sets(&mut self) {
        self.held.get_mut().expect("queue's kept sets poisoned").clear();
        self.asked.get_mut().expect("queue's kept sets poisoned").clear();
    }

    /// `e` into the entries and the kept order.
    fn insert(&mut self, e: QueueEntry) {
        self.forget_sets();
        let rank = self.ranks.get(&e.ticket).copied().unwrap_or(0);
        self.order.insert(Self::place(&e, rank));
        if let Some(until) = e.snoozed_until_unix {
            self.snoozes.insert((until, e.ticket.clone()));
        }
        self.entries.insert(e.ticket.clone(), e);
    }

    /// The cache's half of a committed write: the entry as it leaves it
    /// (`None`: out of the queue) and its record.
    pub(super) fn put(&mut self, ticket: &str, entry: Option<QueueEntry>, record: QueueRecord) {
        self.unplace(ticket);
        if record.action == QueueAction::Queued {
            self.ranks.insert(ticket.to_string(), record.id);
        }
        match entry {
            Some(e) => self.insert(e),
            None => {
                self.entries.remove(ticket);
                self.ranks.remove(ticket);
            }
        }
        self.next_id = record.id + 1;
        self.log.entry(ticket.to_string()).or_default().push(record);
    }

    /// The tickets `queued [urgency]` holds for, per shortcode — each queued
    /// (at `urgency`, when one is named) and placed by `at`. From the kept
    /// entries, never a scan of the tickets. A snooze does not take a ticket
    /// out: it is still queued, only off the alerting surfaces.
    pub(super) fn holds(&self, urgency: Option<Urgency>, at: &HashMap<String, OrderKey>) -> BTreeMap<ShortcodeId, RoaringBitmap> {
        let mut held = self.held.lock().expect("queue's kept sets poisoned");
        let set = held.entry(urgency).or_insert_with(|| {
            let mut out = Held::new();
            for e in self.entries.values().filter(|e| urgency.is_none_or(|u| e.urgency == u)) {
                if let Some(key) = at.get(&e.ticket) {
                    out.entry(key.shortcode.clone()).or_default().insert(key.seq);
                }
            }
            Arc::new(out)
        });
        set.as_ref().clone()
    }

    /// [`Queue::holds`] from the entries, kept set aside: what a test holds it to.
    #[cfg(test)]
    pub(super) fn holds_walked(&self, urgency: Option<Urgency>, at: &HashMap<String, OrderKey>) -> BTreeMap<ShortcodeId, RoaringBitmap> {
        let mut out = Held::new();
        for e in self.entries.values().filter(|e| urgency.is_none_or(|u| e.urgency == u)) {
            if let Some(key) = at.get(&e.ticket) {
                out.entry(key.shortcode.clone()).or_default().insert(key.seq);
            }
        }
        out
    }

    /// V2-20: the tickets `asked` holds for, for the reader `me`, per
    /// shortcode — an entry someone else queued that names an input still
    /// `open`: a question waiting on the reader, never one's own.
    pub(super) fn asked(&self, me: &str, inputs: &super::inputs::Inputs, at: &HashMap<String, OrderKey>) -> BTreeMap<ShortcodeId, RoaringBitmap> {
        let mut kept = self.asked.lock().expect("queue's kept sets poisoned");
        if let Some((moves, set)) = kept.get(me) {
            if *moves == inputs.moves() {
                return set.as_ref().clone();
            }
        }
        let mut out = Held::new();
        for e in self.entries.values().filter(|e| e.by != me) {
            let open = |i: &&str| inputs.get(&e.ticket, i).is_some_and(|x| x.state == InputState::Open);
            if e.named().iter().any(|i| open(i)) {
                if let Some(key) = at.get(&e.ticket) {
                    out.entry(key.shortcode.clone()).or_default().insert(key.seq);
                }
            }
        }
        kept.insert(me.to_string(), (inputs.moves(), Arc::new(out.clone())));
        out
    }

    /// ACT-5: only the entries of the tickets `keep` names — a deleted
    /// ticket's stay in the file, out of every read, for its undo.
    pub(super) fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        let gone: Vec<String> = self.entries.keys().filter(|t| !keep(t)).cloned().collect();
        gone.iter().for_each(|t| self.hide(t));
    }

    /// ACT-5: `ticket`'s entry out of every read, its record and its row in
    /// the file kept.
    pub(super) fn hide(&mut self, ticket: &str) {
        self.unplace(ticket);
        self.entries.remove(ticket);
    }

    /// The entries in the queue's order, from just after `after`.
    fn in_order<'a>(&'a self, after: Option<&Place>) -> impl Iterator<Item = (&'a Place, &'a QueueEntry)> + 'a {
        use std::ops::Bound::{Excluded, Unbounded};
        let from = after.map_or(Unbounded, |p| Excluded(p.clone()));
        self.order.range((from, Unbounded)).filter_map(|p| Some((p, self.entries.get(&p.3)?)))
    }

    /// How many snoozes are running at `now`.
    fn snoozed(&self, now: i64) -> usize {
        self.snoozes.range((now + 1, String::new())..).count()
    }
}

/// A place as a page's cursor: `<urgency>.<queued>.<rank>.<ticket>`, an
/// [`crate::vocab::Cursor`] (the ticket last: an id may hold a dot). It names
/// a position, so a page after a ticket that has left the queue goes on.
fn cursor_of(p: &Place) -> String {
    format!("{}.{}.{}.{}", 3 - p.0 .0 as u8, p.1, p.2, p.3)
}

/// …and read back; `None` for what no page answered.
fn place_of(text: &str) -> Option<Place> {
    let mut parts = text.splitn(4, '.');
    let u = parts.next()?.parse::<usize>().ok()?;
    let urgency = *Urgency::ALL.iter().rev().nth(u)?;
    Some((Reverse(urgency), parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.to_string()))
}

/// What `queued` asks.
#[derive(Debug, Clone, Default)]
pub struct QueuedAsk {
    /// Only this ticket's entry, when it has one and the reader sees it.
    pub id: Option<String>,
    /// Whether the snoozed ones (at `now`) are in it.
    pub snoozed: bool,
    /// How many rows; [`QUEUED_PAGE`] unless asked, at most [`QUEUED_PAGE_MAX`].
    pub limit: Option<usize>,
    /// Strictly after the place a page answered as its `next`.
    pub after: Option<String>,
}

/// A page of `queued`.
#[derive(Debug, Clone, Default)]
pub struct QueuedPage {
    pub rows: Vec<(QueueEntry, TicketSummary)>,
    /// Where the next page starts; `None` at the end.
    pub next: Option<String>,
    /// The rows past this page.
    pub remaining: u64,
    /// Every row in the caller's reach, on any page.
    pub count: u64,
    /// How many of those are snoozed at `now`.
    pub snoozed: u64,
}

impl Cache {
    /// MENGER-13: the tickets a history predicate holds for, per shortcode —
    /// MENGER-39: `queued` and `asked` from the queue's kept entries, the
    /// rest from the transition log.
    pub(super) fn holds(&self, h: &crate::filter::History, clock: &crate::store::Clock) -> BTreeMap<ShortcodeId, RoaringBitmap> {
        match &h.event {
            crate::filter::Event::Queued(u) => self.queue.holds(*u, &self.at),
            crate::filter::Event::Asked => self.queue.asked(&clock.me, &self.inputs, &self.at),
            _ => self.log.holds(h, clock),
        }
    }

    /// …and when time alone next moves what it holds for: never, for the
    /// queue.
    pub(super) fn next_boundary(&self, h: &crate::filter::History, clock: &crate::store::Clock) -> Option<i64> {
        match &h.event {
            crate::filter::Event::Queued(_) | crate::filter::Event::Asked => None,
            _ => self.log.next_boundary(h, clock),
        }
    }
}

/// The queue as stored.
pub(super) fn load(conn: &Connection) -> anyhow::Result<Queue> {
    let bad = |at: usize, e: String| rusqlite::Error::FromSqlConversionFailure(at, rusqlite::types::Type::Text, e.into());
    let mut q = Queue::default();
    let mut stmt = conn.prepare(
        "SELECT ticket, urgency, reason, input, actor, name, source, queued_unix, asked_unix, snoozed_until, inputs, blocking FROM queue",
    )?;
    let entries = stmt.query_map([], |r| {
        let input: Option<String> = r.get(3)?;
        Ok(QueueEntry {
            ticket: r.get(0)?,
            urgency: r.get::<_, String>(1)?.parse().map_err(|e| bad(1, e))?,
            reason: r.get(2)?,
            inputs: list_of(r.get(10)?),
            blocking: r.get(11)?,
            input,
            by: r.get(4)?,
            name: r.get(5)?,
            source: r.get::<_, String>(6)?.parse().map_err(|e| bad(6, e))?,
            queued_unix: r.get(7)?,
            asked_unix: r.get(8)?,
            snoozed_until_unix: r.get(9)?,
        })
    })?;
    let entries = entries.collect::<Result<Vec<_>, _>>()?;
    let mut stmt = conn.prepare(
        "SELECT id, ticket, action, urgency, reason, input, at_unix, actor, name, source, until_unix, inputs, blocking FROM queue_log ORDER BY id",
    )?;
    let records = stmt.query_map([], |r| {
        let input: Option<String> = r.get(5)?;
        Ok(QueueRecord {
            id: r.get::<_, i64>(0)? as u64,
            ticket: r.get(1)?,
            action: r.get::<_, String>(2)?.parse().map_err(|e| bad(2, e))?,
            urgency: r.get::<_, String>(3)?.parse().map_err(|e| bad(3, e))?,
            reason: r.get(4)?,
            inputs: list_of(r.get(11)?),
            blocking: r.get(12)?,
            input,
            at_unix: r.get(6)?,
            actor: r.get(7)?,
            name: r.get(8)?,
            source: r.get::<_, String>(9)?.parse().map_err(|e| bad(9, e))?,
            until_unix: r.get(10)?,
        })
    })?;
    for rec in records {
        let rec = rec?;
        q.next_id = rec.id + 1;
        if rec.action == QueueAction::Queued {
            q.ranks.insert(rec.ticket.clone(), rec.id);
        }
        q.log.entry(rec.ticket.clone()).or_default().push(rec);
    }
    entries.into_iter().for_each(|e| q.insert(e));
    Ok(q)
}

/// The entry `ticket` holds after a write (`None`: none), and its record,
/// inside the caller's transaction.
pub(super) fn store(tx: &Connection, ticket: &str, entry: Option<&QueueEntry>, record: &QueueRecord) -> rusqlite::Result<()> {
    match entry {
        Some(e) => {
            tx.prepare_cached(
                "INSERT OR REPLACE INTO queue (ticket, urgency, reason, input, actor, name, source, queued_unix, asked_unix, snoozed_until, inputs, blocking)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?
            .execute(params![
                e.ticket,
                e.urgency.as_str(),
                e.reason,
                e.input,
                e.by,
                e.name,
                e.source.as_str(),
                e.queued_unix,
                e.asked_unix,
                e.snoozed_until_unix,
                list_text(&e.inputs),
                e.blocking,
            ])?;
        }
        None => {
            tx.prepare_cached("DELETE FROM queue WHERE ticket = ?1")?.execute([ticket])?;
        }
    }
    tx.prepare_cached(
        "INSERT INTO queue_log (id, ticket, action, urgency, reason, input, at_unix, actor, name, source, until_unix, inputs, blocking)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?
    .execute(params![
        record.id as i64,
        record.ticket,
        record.action.as_str(),
        record.urgency.as_str(),
        record.reason,
        record.input,
        record.at_unix,
        record.actor,
        record.name,
        record.source.as_str(),
        record.until_unix,
        list_text(&record.inputs),
        record.blocking,
    ])?;
    Ok(())
}

/// A queue write as `watch` carries it — the wire's `queued`, `unqueued`
/// and `snoozed` ([`crate::types::TicketsEvent`]).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueueChange {
    /// Put in the queue, or queued again (`action`: queued, raised,
    /// requeued) — the entry as the write leaves it, and its ticket's row.
    Queued { entry: QueueEntry, ticket: TicketSummary, action: QueueAction },
    /// Out of the queue (`action`: unqueued, or closed — its named input was
    /// closed) — the entry as it was, who took it out and when.
    Unqueued { id: String, entry: QueueEntry, action: QueueAction, by: String, at_unix: i64 },
    /// Snoozed until the entry's `snoozed_until_unix`.
    Snoozed { entry: QueueEntry, ticket: TicketSummary },
}

/// A record of `action` on `entry` by `act` at `at`.
pub(super) fn record_of(id: u64, entry: &QueueEntry, action: QueueAction, act: &Act, at: i64) -> QueueRecord {
    QueueRecord {
        id,
        ticket: entry.ticket.clone(),
        action,
        urgency: entry.urgency,
        reason: matches!(action, QueueAction::Queued | QueueAction::Raised | QueueAction::Requeued).then(|| entry.reason.clone()),
        input: entry.input.clone(),
        inputs: entry.inputs.clone(),
        blocking: entry.blocking.clone(),
        at_unix: at,
        actor: act.actor.clone(),
        name: act.name.clone(),
        source: act.source,
        until_unix: (action == QueueAction::Snoozed).then_some(entry.snoozed_until_unix).flatten(),
    }
}

/// Whether `input` of `ticket` may name what answers a queue entry: one it
/// declares, not closed, removed or withdrawn.
fn check_input(cache: &Cache, ticket: &str, input: &str) -> Result<(), StoreError> {
    let (t, i) = (ticket.to_string(), input.to_string());
    let state = cache.inputs.get(ticket, input).map(|x| x.state).ok_or_else(|| StoreError::InputUnknown {
        ticket: t.clone(),
        input: i.clone(),
        known: cache.inputs.of(ticket).iter().map(|x| x.id.to_string()).collect(),
    })?;
    match state {
        InputState::Closed => Err(StoreError::InputClosed { ticket: t, input: i }),
        InputState::Removed => Err(StoreError::InputRemoved { ticket: t, input: i }),
        InputState::Withdrawn => Err(StoreError::InputWithdrawn { ticket: t, input: i }),
        InputState::Open | InputState::Answered => Ok(()),
    }
}

/// MENGER-39: what a `close_input` of `input` does to the queue — the entry
/// that names it taken out, worked out before the write commits. APP-86:
/// only when every other input it names is settled (closed, removed or
/// withdrawn) — one closed of several is progress, and nothing is written.
#[derive(Clone)]
pub(super) struct Closing {
    entry: QueueEntry,
    record: QueueRecord,
}

impl Closing {
    /// `ticket`'s entry, when it names `input`, as the close takes it out.
    pub(super) fn of(cache: &Cache, ticket: &str, input: &str, at: i64) -> Option<Self> {
        let entry = cache.queue.get(ticket).filter(|e| e.named().contains(&input))?.clone();
        let settled = |i: &str| {
            let state = cache.inputs.get(ticket, i).map(|x| x.state);
            i == input || state.is_none_or(|s| matches!(s, InputState::Closed | InputState::Removed | InputState::Withdrawn))
        };
        if !entry.named().into_iter().all(settled) {
            return None;
        }
        let act = Act { message: None, cause: None, ..Act::default() };
        let mut record = record_of(cache.queue.next_id(), &entry, QueueAction::Closed, &act, at);
        // The store's own act: the input's closing, not a caller's call.
        record.actor = crate::types::SYSTEM_ACTOR.into();
        record.source = TransitionSource::Cascade;
        record.reason = Some(match entry.named().len() {
            0 | 1 => format!("its input `{input}` was closed"),
            n => format!("its inputs were closed, `{input}` the last of {n}"),
        });
        Some(Self { entry, record })
    }

    /// …committed, inside the input's transaction.
    pub(super) fn commit(&self, tx: &Connection) -> rusqlite::Result<()> {
        store(tx, &self.entry.ticket, None, &self.record)
    }

    /// …in the cache, with the input.
    pub(super) fn apply(&self, cache: &mut Cache) {
        cache.queue.put(&self.entry.ticket, None, self.record.clone());
    }

    /// …and on `watch`.
    pub(super) fn change(self, shortcode: ShortcodeId) -> Change {
        let Closing { entry, record } = self;
        Change::Queue {
            shortcode,
            change: QueueChange::Unqueued { id: entry.ticket.clone(), entry, action: QueueAction::Closed, by: record.actor, at_unix: record.at_unix },
        }
    }
}

/// What a `queue` asks (MENGER-39; APP-86 several inputs).
#[derive(Debug, Clone)]
pub struct QueueAsk {
    pub urgency: Urgency,
    pub reason: String,
    /// The inputs whose answers are asked, in order (APP-86).
    pub inputs: Vec<String>,
    /// The one the caller is idle on — among them (APP-86).
    pub blocking: Option<String>,
}

impl Default for QueueAsk {
    fn default() -> Self {
        Self { urgency: Urgency::Whenever, reason: String::new(), inputs: Vec::new(), blocking: None }
    }
}

/// A `blocking` call that names one input is idle on it.
fn ask_blocking(urgency: Urgency, asked: &[String]) -> bool {
    urgency == Urgency::Blocking && asked.len() == 1
}

impl Store {
    /// A permit's key, or any address of a ticket, as the store keeps it.
    pub(crate) fn keyed(&self, id: &str) -> Result<String, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        match cache.at.contains_key(id) {
            true => Ok(id.to_string()),
            false => cache.resolve(id),
        }
    }

    /// MENGER-39: puts `id` in front of the owner — at `urgency`, for
    /// `reason`, answered by `input` (required at `blocking`). The one-input
    /// spelling of [`Store::queue_with`].
    pub async fn queue_by(&self, on: Permit<Editor>, urgency: Urgency, reason: &str, input: Option<&str>, act: Act) -> Result<QueueChange, StoreError> {
        let ask = QueueAsk { urgency, reason: reason.to_string(), inputs: input.into_iter().map(str::to_string).collect(), ..QueueAsk::default() };
        self.queue_with(on, ask, act).await
    }

    /// MENGER-39: puts `id` in front of the owner — at the ask's urgency,
    /// for its reason, answered by its inputs (each one the ticket declares
    /// and has not closed; APP-86: several, in order, and `blocking` the one
    /// the caller is idle on — among them, and required at urgency
    /// `blocking` when there are several). Queued already: the urgency is
    /// raised, never lowered, the reason the new one, the inputs added to
    /// the entry's, a snooze ended. One entry, one record, one write;
    /// answers the entry with its row and what the write was.
    pub async fn queue_with(&self, on: Permit<Editor>, ask: QueueAsk, act: Act) -> Result<QueueChange, StoreError> {
        // ORG-3: the ticket by any short form of its address.
        let id = self.resolve(on.id())?;
        let QueueAsk { urgency, reason, inputs, blocking } = ask;
        let reason = reason.trim().to_string();
        if reason.is_empty() {
            return Err(StoreError::Invalid("a queue entry says why: `reason` is blank".into()));
        }
        if reason.len() > REASON_MAX {
            return Err(StoreError::Invalid(format!("`reason` is {} bytes — at most {REASON_MAX}", reason.len())));
        }
        let mut asked: Vec<String> = Vec::new();
        for i in inputs.iter().map(|i| i.trim()).filter(|i| !i.is_empty()) {
            if !asked.iter().any(|a| a == i) {
                asked.push(i.to_string());
            }
        }
        let blocking = blocking.map(|b| b.trim().to_string()).filter(|b| !b.is_empty());
        if urgency == Urgency::Blocking && asked.is_empty() && blocking.is_none() {
            return Err(StoreError::Invalid(
                "`blocking` names the input that answers it — `input`, an input on the ticket (the caller is idle until it is answered)".into(),
            ));
        }
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (row, entry, record, action, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let row = cache.get(&id).cloned().ok_or_else(|| StoreError::NotFound(id.clone()))?;
            // ARCHIVE-2: an archived ticket asks nothing.
            if row.archived.is_some() {
                return Err(StoreError::Invalid(format!("{id} is archived: an archived ticket asks nothing — restore it first (nothing was written)")));
            }
            for i in &asked {
                check_input(&cache, &id, i)?;
            }
            let held = cache.queue.get(&id);
            // APP-86: the entry's inputs, then the call's new ones.
            let mut named: Vec<String> = held.map(|h| h.named().into_iter().map(str::to_string).collect()).unwrap_or_default();
            for i in &asked {
                if !named.contains(i) {
                    named.push(i.clone());
                }
            }
            if named.len() > INPUTS_MAX {
                return Err(StoreError::Invalid(format!("an entry names at most {INPUTS_MAX} inputs — this one would name {}", named.len())));
            }
            let asked_at = urgency;
            let urgency = held.map_or(urgency, |h| h.urgency.max(urgency));
            // The one the caller is idle on: named, else a blocking call's
            // only input, else the one the entry had, else its only input.
            let blocking = blocking
                .or_else(|| ask_blocking(asked_at, &asked).then(|| asked[0].clone()))
                .or_else(|| held.and_then(|h| h.blocking_input().map(str::to_string)))
                .or_else(|| (urgency == Urgency::Blocking && named.len() == 1).then(|| named[0].clone()));
            if let Some(b) = &blocking {
                if !named.contains(b) {
                    return Err(StoreError::Invalid(format!(
                        "`blocking` names `{b}`, which is not among the entry's inputs ({}) — name it with `input` too",
                        if named.is_empty() { "none".to_string() } else { named.join(", ") }
                    )));
                }
            }
            if urgency == Urgency::Blocking && blocking.is_none() {
                return Err(StoreError::Invalid(format!(
                    "`blocking` with several inputs ({}) names the one the caller is idle on — `blocking`, one of them",
                    named.join(", ")
                )));
            }
            let action = match held {
                None => QueueAction::Queued,
                Some(h) if urgency > h.urgency => QueueAction::Raised,
                Some(_) => QueueAction::Requeued,
            };
            let entry = QueueEntry {
                ticket: id.clone(),
                urgency,
                reason,
                // For every reader before APP-86: the blocking one, else the first.
                input: None,
                inputs: named,
                blocking,
                by: act.actor.clone(),
                name: act.name.clone(),
                source: act.source,
                queued_unix: held.map_or(now, |h| h.queued_unix),
                asked_unix: now,
                snoozed_until_unix: None,
            }
            .compact();
            let record = record_of(cache.queue.next_id(), &entry, action, &act, now);
            // `queued` and `queued <urgency>` move only when it enters the
            // queue or its urgency does.
            let moves = held.is_none_or(|h| h.urgency != entry.urgency);
            // V2-20: `asked` moves on who queued it and what it names too.
            let under = match moves {
                true => self.queued_before(&cache, &row),
                false => lineage::asked_before_with(&self.unders, &cache, &[(&row.shortcode, row.seq)]),
            };
            (row, entry, record, action, under)
        };
        let change = QueueChange::Queued { entry: entry.clone(), ticket: row.clone(), action };
        self.queue_write(serial, row, Some(entry), record, change, under).await
    }

    /// MENGER-39: takes `id` out of the queue — the owner done with it, or
    /// the caller withdrawing it. `None`: it was not queued, and nothing is
    /// written.
    pub async fn unqueue_by(&self, on: Permit<Editor>, act: Act) -> Result<Option<QueueChange>, StoreError> {
        // ORG-3: the ticket by any short form of its address.
        let id = self.resolve(on.id())?;
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (row, entry, record, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let row = cache.get(&id).cloned().ok_or_else(|| StoreError::NotFound(id.clone()))?;
            let Some(entry) = cache.queue.get(&id).cloned() else { return Ok(None) };
            let record = record_of(cache.queue.next_id(), &entry, QueueAction::Unqueued, &act, now);
            let under = self.queued_before(&cache, &row);
            (row, entry, record, under)
        };
        let change = QueueChange::Unqueued { id: id.clone(), entry, action: QueueAction::Unqueued, by: act.actor, at_unix: now };
        self.queue_write(serial, row, None, record, change, under).await.map(Some)
    }

    /// MENGER-39: `id` off the alerting surfaces for `minutes` (1 to a
    /// week); still queued. Refused when it is not queued.
    pub async fn snooze_by(&self, on: Permit<Editor>, minutes: u32, act: Act) -> Result<QueueChange, StoreError> {
        // ORG-3: the ticket by any short form of its address.
        let id = self.resolve(on.id())?;
        if minutes == 0 || minutes > SNOOZE_MAX_MINUTES {
            return Err(StoreError::Invalid(format!("`minutes` is 1 to {SNOOZE_MAX_MINUTES} (a week) — not {minutes}")));
        }
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (row, entry, record) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let row = cache.get(&id).cloned().ok_or_else(|| StoreError::NotFound(id.clone()))?;
            let mut entry = cache
                .queue
                .get(&id)
                .cloned()
                .ok_or_else(|| StoreError::Invalid(format!("{id} is not queued — nothing to snooze")))?;
            entry.snoozed_until_unix = Some(now + i64::from(minutes) * 60);
            let record = record_of(cache.queue.next_id(), &entry, QueueAction::Snoozed, &act, now);
            (row, entry, record)
        };
        let change = QueueChange::Snoozed { entry: entry.clone(), ticket: row.clone() };
        self.queue_write(serial, row, Some(entry), record, change, None).await
    }

    /// One queue write, through the write path every write takes.
    async fn queue_write(
        &self,
        serial: tokio::sync::OwnedMutexGuard<()>,
        row: Row,
        entry: Option<QueueEntry>,
        record: QueueRecord,
        change: QueueChange,
        under: Option<UnderBefore>,
    ) -> Result<QueueChange, StoreError> {
        let tickets = Arc::clone(&self.tickets);
        let (stored, kept) = ((entry.clone(), record.clone()), (entry, record));
        let (id, shortcode) = (row.id.clone(), row.shortcode.clone());
        let ticket = id.clone();
        self.write(
            serial,
            move |tx| {
                store(tx, &id, stored.0.as_ref(), &stored.1)?;
                Ok(())
            },
            move |cache, _| cache.queue.put(&ticket, kept.0, kept.1),
            move |(), (), tx| {
                tx.send(Change::Queue { shortcode, change: change.clone() });
                if let Some(c) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(c);
                }
                change
            },
        )
        .await
    }

    /// MENGER-39: the open watches whose filter asks `queued`, before a
    /// write that moves `row` in or out of it.
    fn queued_before(&self, cache: &Cache, row: &Row) -> Option<UnderBefore> {
        lineage::queued_before_with(&self.unders, cache, &[(&row.shortcode, row.seq)])
    }

    /// MENGER-39: a page of the queue, most urgent first, then the oldest —
    /// each entry with its ticket's row; V2-20: cut from the kept order, the
    /// rows cloned for the page alone. ACCESS-10: of the tickets `who` sees,
    /// each row redacted. An `after` no page answered is refused. From the
    /// cache.
    pub fn queued_page(&self, who: &(impl super::Reader + ?Sized), ask: QueuedAsk, now: i64) -> Result<QueuedPage, StoreError> {
        let after = ask.after.as_deref().map(|a| place_of(a).ok_or_else(|| StoreError::Invalid(format!("`after` {a} is no place in the queue — pass the `next` a page answered")))).transpose()?;
        let limit = ask.limit.unwrap_or(QUEUED_PAGE).clamp(1, QUEUED_PAGE_MAX);
        let Ok(seen) = super::Seen::of(who) else { return Ok(QueuedPage::default()) };
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        let shows = |e: &QueueEntry| (ask.snoozed || !e.snoozed_at(now)) && sees.id(&e.ticket);
        let mut page = QueuedPage::default();
        // One ticket's: its place alone, nothing walked.
        if let Some(id) = &ask.id {
            let key = sees.resolve(id)?;
            if let Some(entry) = cache.queue.get(&key).filter(|e| shows(e)) {
                if let Some(row) = cache.get(&key).filter(|r| r.archived.is_none()) {
                    let mut row = row.clone();
                    sees.row(&mut row);
                    page.rows.push((entry.clone(), row));
                    page.count = 1;
                    page.snoozed = u64::from(entry.snoozed_at(now));
                }
            }
            return Ok(page);
        }
        let mut last = None;
        // The rows, then the rest of the order — counted, never cloned.
        for (place, entry) in cache.queue.in_order(after.as_ref()).filter(|(_, e)| shows(e)) {
            if page.rows.len() == limit {
                page.remaining += 1;
                continue;
            }
            // ARCHIVE-2: an archived ticket asks nothing — in no view.
            let Some(row) = cache.get(&entry.ticket).filter(|r| r.archived.is_none()) else { continue };
            let mut row = row.clone();
            sees.row(&mut row);
            page.rows.push((entry.clone(), row));
            last = Some(place);
        }
        page.next = last.filter(|_| page.remaining > 0).map(cursor_of);
        page.count = match sees.all() {
            true => cache.queue.entries.len() as u64,
            false => cache.queue.in_order(None).filter(|(_, e)| sees.id(&e.ticket)).count() as u64,
        };
        page.snoozed = match sees.all() {
            true => cache.queue.snoozed(now) as u64,
            false => cache.queue.in_order(None).filter(|(_, e)| e.snoozed_at(now) && sees.id(&e.ticket)).count() as u64,
        };
        Ok(page)
    }

    /// The whole queue at `now`: a test's read.
    #[cfg(test)]
    pub fn queued(&self, who: &(impl super::Reader + ?Sized), snoozed: bool, now: i64) -> Vec<(QueueEntry, TicketSummary)> {
        let mut out = Vec::new();
        let mut after = None;
        loop {
            let page = self.queued_page(who, QueuedAsk { id: None, snoozed, limit: Some(QUEUED_PAGE_MAX), after }, now).expect("a page");
            out.extend(page.rows);
            match page.next {
                Some(next) => after = Some(next),
                None => return out,
            }
        }
    }

    /// MENGER-39: `id`'s queue entry, when it is queued — ACCESS-10: and
    /// `who` sees it. From the cache.
    pub fn queue_entry(&self, who: &(impl super::Reader + ?Sized), id: &str) -> Option<QueueEntry> {
        let seen = super::Seen::of(who).ok()?;
        let cache = self.tickets.read().expect("store lock poisoned");
        seen.sees(&cache).resolve(id).ok().and_then(|key| cache.queue.get(&key).cloned())
    }

    /// MENGER-39: `id`'s queue history, oldest first — ACCESS-10: when `who`
    /// sees it. From the cache.
    pub fn queue_history(&self, who: &(impl super::Reader + ?Sized), id: &str) -> Vec<QueueRecord> {
        let Ok(seen) = super::Seen::of(who) else { return Vec::new() };
        let cache = self.tickets.read().expect("store lock poisoned");
        seen.sees(&cache).resolve(id).map(|key| cache.queue.records(&key).to_vec()).unwrap_or_default()
    }
}
