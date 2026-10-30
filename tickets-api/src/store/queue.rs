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
//! **Views of mockups** (APP-85): an entry carries the views it was queued
//! with (`mockups`), each checked before the write — a ticket the caller
//! reads, a mockup its body holds ([`Store::mockup_views`]); queueing again
//! adds the call's.
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
use crate::access::{Editor, Permit};
use crate::queue::{order, MockupView, QueueAction, QueueEntry, QueueRecord, Urgency, INPUTS_MAX, REASON_MAX, SNOOZE_MAX_MINUTES, VIEWS_MAX};
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
            inputs        TEXT,
            blocking      TEXT,
            mockups       TEXT,
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
            mockups    TEXT,
            at_unix    INTEGER NOT NULL,
            actor      TEXT NOT NULL,
            name       TEXT,
            source     TEXT NOT NULL,
            until_unix INTEGER,
            comment    INTEGER
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

/// APP-23: a file whose queue log has no `comment` gains it here, in place —
/// the comment a change request was left as; on every open a no-op once it
/// is there. One way: an older build reads `changes_requested` as no action
/// it knows and refuses to open the file (promote's rollback restores the
/// backup it took first).
pub(crate) fn migrate_comment(conn: &Connection) -> anyhow::Result<()> {
    if !super::db::has_column(conn, "queue_log", "comment")? {
        conn.execute("ALTER TABLE queue_log ADD COLUMN comment INTEGER", [])?;
        tracing::warn!("APP-23: the queue log gained its comment column, durably");
    }
    Ok(())
}

/// APP-86: a file whose queue has no `inputs` / `blocking` gains them here,
/// in place, on both tables — and APP-85's `mockups` — on every open a no-op
/// once they are there. An older build reads its columns by name and never
/// these: `input` still holds the one it understands.
pub(crate) fn migrate_inputs(conn: &Connection) -> anyhow::Result<()> {
    for table in ["queue", "queue_log"] {
        for column in ["inputs", "blocking", "mockups"] {
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

/// APP-85: an entry's views as kept — JSON, `NULL` when none.
fn views_text(views: &[MockupView]) -> Option<String> {
    (!views.is_empty()).then(|| serde_json::to_string(views).unwrap_or_default())
}

/// …and read back.
fn views_of(text: Option<String>) -> Vec<MockupView> {
    text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
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
    pub(super) fn next_id(&self) -> u64 {
        self.next_id.max(1)
    }

    /// The cache's half of a committed write: the entry as it leaves it
    /// (`None`: out of the queue) and its record.
    pub(super) fn put(&mut self, ticket: &str, entry: Option<QueueEntry>, record: QueueRecord) {
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
        "SELECT ticket, urgency, reason, input, actor, name, source, queued_unix, asked_unix, snoozed_until, inputs, blocking, mockups FROM queue",
    )?;
    let entries = stmt.query_map([], |r| {
        let input: Option<String> = r.get(3)?;
        Ok(QueueEntry {
            ticket: r.get(0)?,
            urgency: r.get::<_, String>(1)?.parse().map_err(|e| bad(1, e))?,
            reason: r.get(2)?,
            inputs: list_of(r.get(10)?),
            blocking: r.get(11)?,
            mockups: views_of(r.get(12)?),
            input,
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
        "SELECT id, ticket, action, urgency, reason, input, at_unix, actor, name, source, until_unix, comment, inputs, blocking, mockups FROM queue_log ORDER BY id",
    )?;
    let records = stmt.query_map([], |r| {
        let input: Option<String> = r.get(5)?;
        Ok(QueueRecord {
            id: r.get::<_, i64>(0)? as u64,
            ticket: r.get(1)?,
            action: r.get::<_, String>(2)?.parse().map_err(|e| bad(2, e))?,
            urgency: r.get::<_, String>(3)?.parse().map_err(|e| bad(3, e))?,
            reason: r.get(4)?,
            inputs: list_of(r.get(12)?),
            blocking: r.get(13)?,
            mockups: views_of(r.get(14)?),
            input,
            at_unix: r.get(6)?,
            actor: r.get(7)?,
            name: r.get(8)?,
            source: r.get::<_, String>(9)?.parse().map_err(|e| bad(9, e))?,
            until_unix: r.get(10)?,
            comment: r.get::<_, Option<i64>>(11)?.map(|c| c as u64),
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
pub(super) fn store(tx: &Connection, ticket: &str, entry: Option<&QueueEntry>, record: &QueueRecord) -> rusqlite::Result<()> {
    match entry {
        Some(e) => {
            tx.prepare_cached(
                "INSERT OR REPLACE INTO queue (ticket, urgency, reason, input, actor, name, source, queued_unix, asked_unix, snoozed_until, inputs, blocking, mockups)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
                views_text(&e.mockups)
            ])?;
        }
        None => {
            tx.prepare_cached("DELETE FROM queue WHERE ticket = ?1")?.execute([ticket])?;
        }
    }
    tx.prepare_cached(
        "INSERT INTO queue_log (id, ticket, action, urgency, reason, input, at_unix, actor, name, source, until_unix, comment, inputs, blocking, mockups)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
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
        record.comment.map(|c| c as i64),
        list_text(&record.inputs),
        record.blocking,
        views_text(&record.mockups)
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
        mockups: entry.mockups.clone(),
        at_unix: at,
        actor: act.actor.clone(),
        name: act.name.clone(),
        source: act.source,
        until_unix: (action == QueueAction::Snoozed).then_some(entry.snoozed_until_unix).flatten(),
        comment: None,
    }
}

/// Whether `input` of `ticket` may name what answers a queue entry: one it
/// declares (CHAT-20: or `<message>/<input>`, a message's in a chat on it),
/// not closed, removed or withdrawn.
fn check_input(cache: &Cache, ticket: &str, input: &str) -> Result<(), StoreError> {
    let (t, i) = (ticket.to_string(), input.to_string());
    let state = match input.contains('/') {
        true => super::chats::inputs::state_named(cache, ticket, input).ok_or_else(|| StoreError::InputUnknown {
            ticket: t.clone(),
            input: i.clone(),
            known: Vec::new(),
        })?,
        false => cache.inputs.get(ticket, input).map(|x| x.state).ok_or_else(|| StoreError::InputUnknown {
            ticket: t.clone(),
            input: i.clone(),
            known: cache.inputs.of(ticket).iter().map(|x| x.id.to_string()).collect(),
        })?,
    };
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
            let state = match i.contains('/') {
                true => super::chats::inputs::state_named(cache, ticket, i),
                false => cache.inputs.get(ticket, i).map(|x| x.state),
            };
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
    /// The views of mockups it carries (APP-85), checked by
    /// [`Store::mockup_views`].
    pub mockups: Vec<MockupView>,
}

impl Default for QueueAsk {
    fn default() -> Self {
        Self { urgency: Urgency::Whenever, reason: String::new(), inputs: Vec::new(), blocking: None, mockups: Vec::new() }
    }
}

/// A `blocking` call that names one input is idle on it.
fn ask_blocking(urgency: Urgency, asked: &[String]) -> bool {
    urgency == Urgency::Blocking && asked.len() == 1
}

impl Store {
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
        let id = self.keyed(on.id())?;
        let QueueAsk { urgency, reason, inputs, blocking, mockups } = ask;
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
            // APP-85: the entry's views, then the call's new ones.
            let mut views: Vec<MockupView> = held.map(|h| h.mockups.clone()).unwrap_or_default();
            for v in &mockups {
                if !views.contains(v) {
                    views.push(v.clone());
                }
            }
            if views.len() > VIEWS_MAX {
                return Err(StoreError::Invalid(format!("an entry carries at most {VIEWS_MAX} views of mockups — this one would carry {}", views.len())));
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
                mockups: views,
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
            let under = moves.then(|| self.queued_before(&cache, &row)).flatten();
            (row, entry, record, action, under)
        };
        let change = QueueChange::Queued { entry: entry.clone(), ticket: row.clone(), action };
        self.queue_write(serial, row, Some(entry), record, change, under).await
    }

    /// APP-85: the views of mockups `refs` name, as `who` may queue them —
    /// each a ticket `who` reads and a mockup its body holds (`T#mN`: its
    /// body has at least N; `T`: its body is a whole page), the ticket by
    /// its id. Refused, naming the view and why: `who` reading no such
    /// ticket, a number past the body's, a body that is no page.
    pub async fn mockup_views(&self, who: &(impl super::Reader + ?Sized), refs: &[crate::vocab::MockupRef]) -> Result<Vec<MockupView>, StoreError> {
        let mut out = Vec::with_capacity(refs.len());
        for r in refs {
            let mut v = r.view();
            let t = match self.get(who, &v.ticket).await {
                Ok(t) => t,
                Err(StoreError::NotFound(_)) => return Err(StoreError::Invalid(format!("mockup `{r}`: no ticket {} you can read", v.ticket))),
                Err(e) => return Err(e),
            };
            v.ticket = t.id.to_string();
            match v.mock {
                Some(n) => {
                    let have = crate::mockups::count(&t.body);
                    if n as usize > have {
                        let holds = match have {
                            0 => "none".to_string(),
                            1 => "one".to_string(),
                            k => k.to_string(),
                        };
                        return Err(StoreError::Invalid(format!("mockup `{r}`: {} holds {holds} — no mockup {n}", v.ticket)));
                    }
                }
                None if !crate::mockups::whole_page(&t.body) => {
                    return Err(StoreError::Invalid(format!("mockup `{r}`: {} is no whole page — name one of its mockups, `{}#mN`", v.ticket, v.ticket)));
                }
                None => {}
            }
            out.push(v);
        }
        Ok(out)
    }

    /// MENGER-39: takes `id` out of the queue — the owner done with it, or
    /// the caller withdrawing it. `None`: it was not queued, and nothing is
    /// written.
    pub async fn unqueue_by(&self, on: Permit<Editor>, act: Act) -> Result<Option<QueueChange>, StoreError> {
        // ORG-3: the ticket by any short form of its address.
        let id = self.keyed(on.id())?;
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
        let id = self.keyed(on.id())?;
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
    /// are in it. ACCESS-10: of the tickets `who` sees, each row redacted.
    /// From the cache.
    pub fn queued(&self, who: &(impl super::Reader + ?Sized), snoozed: bool, now: i64) -> Vec<(QueueEntry, TicketSummary)> {
        let Ok(seen) = super::Seen::of(who) else { return Vec::new() };
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        // Within one second, the first queued first: its `queued` record's id.
        let since = |t: &str| cache.queue.records(t).iter().rev().find(|r| r.action == QueueAction::Queued).map_or(0, |r| r.id);
        let mut out: Vec<(QueueEntry, TicketSummary, u64)> = cache
            .queue
            .entries
            .values()
            .filter(|e| snoozed || !e.snoozed_at(now))
            .filter(|e| sees.id(&e.ticket))
            .filter_map(|e| Some((e.clone(), cache.get(&e.ticket)?.clone(), since(&e.ticket))))
            // ARCHIVE-2: an archived ticket asks nothing — in no tray.
            .filter(|(_, t, _)| t.archived.is_none())
            .collect();
        out.iter_mut().for_each(|(_, t, _)| sees.row(t));
        drop(sees);
        drop(cache);
        out.sort_by(|a, b| b.0.urgency.cmp(&a.0.urgency).then(a.0.queued_unix.cmp(&b.0.queued_unix)).then(a.2.cmp(&b.2)).then_with(|| order(&a.0, &b.0)));
        out.into_iter().map(|(e, t, _)| (e, t)).collect()
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
