//! MENGER-39: the **queue store** — the tickets put in front of the owner,
//! each with an urgency, a reason and (at `blocking`) the input that answers
//! it; and the queue's own history.
//!
//! **Behind the cache, as everything stored is.** Read whole at open
//! ([`load`]) into a [`Queue`]; every read answers from it — `queued` is its
//! entries in the queue's order ([`crate::queue::order`]), `history` takes a
//! ticket's records, and the filter cell `queued [urgency]`
//! ([`crate::filter::Event::Queued`]) is answered from the kept entries
//! ([`Queue::holds`]), never by testing a ticket. A write — `queue`,
//! `unqueue`, `snooze`, and a `close_input` of the named input — takes the
//! write path every write takes ([`Store::write`]): worked out under the
//! write-serial lock from the cache, committed in one transaction (the
//! entry and its history record), then put into the cache and broadcast.
//!
//! **One entry a ticket.** Queueing a queued ticket again raises its urgency
//! and never lowers it; the reason (and the input, when one is given) are
//! the new call's, and a snooze ends.
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
//! An older build never looks at either. A deleted ticket's entry stays in
//! the file and out of every read (its undo brings it back).

use crate::address::ShortcodeId;
use super::history::Act;
use super::lineage::{self, UnderBefore};
use super::{Cache, Change, OrderKey, Row, Store, StoreError};
use crate::queue::{order, QueueAction, QueueEntry, QueueRecord, Urgency, REASON_MAX, SNOOZE_MAX_MINUTES};
use crate::types::{InputState, TicketSummary, TransitionSource};
use roaring::RoaringBitmap;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// The tables.
pub(super) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS queue (
            ticket        TEXT PRIMARY KEY,
            urgency       TEXT NOT NULL,
            reason        TEXT NOT NULL,
            input         TEXT,
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

/// The queue as the cache holds it: each queued ticket's entry, and each
/// ticket's history records, oldest first.
#[derive(Debug, Clone, Default)]
pub(crate) struct Queue {
    entries: HashMap<String, QueueEntry>,
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
    fn next_id(&self) -> u64 {
        self.next_id.max(1)
    }

    /// The cache's half of a committed write: the entry as it leaves it
    /// (`None`: out of the queue) and its record.
    fn put(&mut self, ticket: &str, entry: Option<QueueEntry>, record: QueueRecord) {
        match entry {
            Some(e) => {
                self.entries.insert(ticket.to_string(), e);
            }
            None => {
                self.entries.remove(ticket);
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
        let mut out: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
        for e in self.entries.values().filter(|e| urgency.is_none_or(|u| e.urgency == u)) {
            if let Some(key) = at.get(&e.ticket) {
                out.entry(key.shortcode.clone()).or_default().insert(key.seq);
            }
        }
        out
    }
}

/// The queue as stored.
pub(super) fn load(conn: &Connection) -> anyhow::Result<Queue> {
    let bad = |at: usize, e: String| rusqlite::Error::FromSqlConversionFailure(at, rusqlite::types::Type::Text, e.into());
    let mut q = Queue::default();
    let mut stmt = conn.prepare(
        "SELECT ticket, urgency, reason, input, actor, name, source, queued_unix, asked_unix, snoozed_until FROM queue",
    )?;
    let entries = stmt.query_map([], |r| {
        Ok(QueueEntry {
            ticket: r.get(0)?,
            urgency: r.get::<_, String>(1)?.parse().map_err(|e| bad(1, e))?,
            reason: r.get(2)?,
            input: r.get(3)?,
            by: r.get(4)?,
            name: r.get(5)?,
            source: r.get::<_, String>(6)?.parse().map_err(|e| bad(6, e))?,
            queued_unix: r.get(7)?,
            asked_unix: r.get(8)?,
            snoozed_until_unix: r.get(9)?,
        })
    })?;
    for e in entries {
        let e = e?;
        q.entries.insert(e.ticket.clone(), e);
    }
    let mut stmt = conn.prepare(
        "SELECT id, ticket, action, urgency, reason, input, at_unix, actor, name, source, until_unix FROM queue_log ORDER BY id",
    )?;
    let records = stmt.query_map([], |r| {
        Ok(QueueRecord {
            id: r.get::<_, i64>(0)? as u64,
            ticket: r.get(1)?,
            action: r.get::<_, String>(2)?.parse().map_err(|e| bad(2, e))?,
            urgency: r.get::<_, String>(3)?.parse().map_err(|e| bad(3, e))?,
            reason: r.get(4)?,
            input: r.get(5)?,
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
        q.log.entry(rec.ticket.clone()).or_default().push(rec);
    }
    Ok(q)
}

/// The entry `ticket` holds after a write (`None`: none), and its record,
/// inside the caller's transaction.
fn store(tx: &Connection, ticket: &str, entry: Option<&QueueEntry>, record: &QueueRecord) -> rusqlite::Result<()> {
    match entry {
        Some(e) => {
            tx.prepare_cached(
                "INSERT OR REPLACE INTO queue (ticket, urgency, reason, input, actor, name, source, queued_unix, asked_unix, snoozed_until)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                e.snoozed_until_unix
            ])?;
        }
        None => {
            tx.prepare_cached("DELETE FROM queue WHERE ticket = ?1")?.execute([ticket])?;
        }
    }
    tx.prepare_cached(
        "INSERT INTO queue_log (id, ticket, action, urgency, reason, input, at_unix, actor, name, source, until_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
        record.until_unix
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
fn record_of(id: u64, entry: &QueueEntry, action: QueueAction, act: &Act, at: i64) -> QueueRecord {
    QueueRecord {
        id,
        ticket: entry.ticket.clone(),
        action,
        urgency: entry.urgency,
        reason: matches!(action, QueueAction::Queued | QueueAction::Raised | QueueAction::Requeued).then(|| entry.reason.clone()),
        input: entry.input.clone(),
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
    let held = cache.inputs.get(ticket, input).ok_or_else(|| StoreError::InputUnknown {
        ticket: t.clone(),
        input: i.clone(),
        known: cache.inputs.of(ticket).iter().map(|x| x.id.to_string()).collect(),
    })?;
    match held.state {
        InputState::Closed => Err(StoreError::InputClosed { ticket: t, input: i }),
        InputState::Removed => Err(StoreError::InputRemoved { ticket: t, input: i }),
        InputState::Withdrawn => Err(StoreError::InputWithdrawn { ticket: t, input: i }),
        InputState::Open | InputState::Answered => Ok(()),
    }
}

/// MENGER-39: what a `close_input` of `input` does to the queue — the entry
/// that names it taken out, worked out before the write commits.
#[derive(Clone)]
pub(super) struct Closing {
    entry: QueueEntry,
    record: QueueRecord,
}

impl Closing {
    /// `ticket`'s entry, when it names `input`, as the close takes it out.
    pub(super) fn of(cache: &Cache, ticket: &str, input: &str, at: i64) -> Option<Self> {
        let entry = cache.queue.get(ticket).filter(|e| e.input.as_deref() == Some(input))?.clone();
        let act = Act { message: None, cause: None, ..Act::default() };
        let mut record = record_of(cache.queue.next_id(), &entry, QueueAction::Closed, &act, at);
        // The store's own act: the input's closing, not a caller's call.
        record.actor = crate::types::SYSTEM_ACTOR.into();
        record.source = TransitionSource::Cascade;
        record.reason = Some(format!("its input `{input}` was closed"));
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

impl Store {
    /// MENGER-39: puts `id` in front of the owner — at `urgency`, for
    /// `reason`, answered by `input` (required at `blocking`; an input the
    /// ticket declares and has not closed). Queued already: the urgency is
    /// raised, never lowered, the reason (and an input given) the new ones,
    /// a snooze ended. One entry, one record, one write; answers the entry
    /// with its row and what the write was.
    pub async fn queue_by(&self, id: &str, urgency: Urgency, reason: &str, input: Option<&str>, act: Act) -> Result<QueueChange, StoreError> {
        // ORG-3: the ticket by any short form of its address.
        let id = self.resolve(id)?;
        let reason = reason.trim().to_string();
        if reason.is_empty() {
            return Err(StoreError::Invalid("a queue entry says why: `reason` is blank".into()));
        }
        if reason.len() > REASON_MAX {
            return Err(StoreError::Invalid(format!("`reason` is {} bytes — at most {REASON_MAX}", reason.len())));
        }
        let input = input.map(str::trim).filter(|i| !i.is_empty()).map(str::to_string);
        if urgency == Urgency::Blocking && input.is_none() {
            return Err(StoreError::Invalid(
                "`blocking` names the input that answers it — `input`, an input on the ticket (the caller is idle until it is answered)".into(),
            ));
        }
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (row, entry, record, action, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let row = cache.get(&id).cloned().ok_or_else(|| StoreError::NotFound(id.clone()))?;
            if let Some(i) = &input {
                check_input(&cache, &id, i)?;
            }
            let held = cache.queue.get(&id);
            let action = match held {
                None => QueueAction::Queued,
                Some(h) if urgency > h.urgency => QueueAction::Raised,
                Some(_) => QueueAction::Requeued,
            };
            let entry = QueueEntry {
                ticket: id.clone(),
                urgency: held.map_or(urgency, |h| h.urgency.max(urgency)),
                reason,
                input: input.or_else(|| held.and_then(|h| h.input.clone())),
                by: act.actor.clone(),
                name: act.name.clone(),
                source: act.source,
                queued_unix: held.map_or(now, |h| h.queued_unix),
                asked_unix: now,
                snoozed_until_unix: None,
            };
            let record = record_of(cache.queue.next_id(), &entry, action, &act, now);
            // `queued` and `queued <urgency>` move only when it enters the
            // queue or its urgency does.
            let moves = held.is_none_or(|h| h.urgency != entry.urgency);
            let under = moves.then(|| self.queued_before(&cache, &row)).flatten();
            (row, entry, record, action, under)
        };
        let change = QueueChange::Queued { entry: entry.clone(), ticket: row.clone(), action };
        self.queue_write(serial, row, Some(entry), record, change, under).await
    }

    /// MENGER-39: takes `id` out of the queue — the owner done with it, or
    /// the caller withdrawing it. `None`: it was not queued, and nothing is
    /// written.
    pub async fn unqueue_by(&self, id: &str, act: Act) -> Result<Option<QueueChange>, StoreError> {
        // ORG-3: the ticket by any short form of its address.
        let id = self.resolve(id)?;
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
    pub async fn snooze_by(&self, id: &str, minutes: u32, act: Act) -> Result<QueueChange, StoreError> {
        // ORG-3: the ticket by any short form of its address.
        let id = self.resolve(id)?;
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

    /// MENGER-39: the queue, most urgent first, then the oldest — each entry
    /// with its ticket's row; `snoozed`: whether the snoozed ones (at `now`)
    /// are in it. A ticket `sees` refuses is left out. From the cache.
    pub fn queued(&self, snoozed: bool, now: i64, sees: impl Fn(&str) -> bool) -> Vec<(QueueEntry, TicketSummary)> {
        let cache = self.tickets.read().expect("store lock poisoned");
        // Within one second, the first queued first: its `queued` record's id.
        let since = |t: &str| cache.queue.records(t).iter().rev().find(|r| r.action == QueueAction::Queued).map_or(0, |r| r.id);
        let mut out: Vec<(QueueEntry, TicketSummary, u64)> = cache
            .queue
            .entries
            .values()
            .filter(|e| snoozed || !e.snoozed_at(now))
            .filter_map(|e| Some((e.clone(), cache.get(&e.ticket)?.clone(), since(&e.ticket))))
            .collect();
        drop(cache);
        out.retain(|(e, _, _)| sees(&e.ticket));
        out.sort_by(|a, b| b.0.urgency.cmp(&a.0.urgency).then(a.0.queued_unix.cmp(&b.0.queued_unix)).then(a.2.cmp(&b.2)).then_with(|| order(&a.0, &b.0)));
        out.into_iter().map(|(e, t, _)| (e, t)).collect()
    }

    /// MENGER-39: `id`'s queue entry, when it is queued. From the cache.
    pub fn queue_entry(&self, id: &str) -> Option<QueueEntry> {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.resolve(id).ok().and_then(|key| cache.queue.get(&key).cloned())
    }

    /// MENGER-39: `id`'s queue history, oldest first. From the cache.
    pub fn queue_history(&self, id: &str) -> Vec<QueueRecord> {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.resolve(id).map(|key| cache.queue.records(&key).to_vec()).unwrap_or_default()
    }
}
