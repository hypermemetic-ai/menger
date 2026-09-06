//! The service-wide ticket cache, with the database behind it.
//!
//! This layer is the primary interface through which the whole
//! application touches tickets — activations, DAG queries, and the shell
//! all go through here, never to storage directly. An embedded SQLite
//! database (the system of record) sits behind the cache: every ticket
//! loads into memory at startup, reads are served from the cache only
//! (async-safe: no lock held across an await, no I/O on the read path),
//! and writes go through write-through — durable in the database before
//! the cache is updated, the event emitted, and `watch` subscribers
//! notified. Nothing outside this module holds a database handle.
//! The status vocabulary matches the house ticketing convention; the kind
//! vocabulary is the owner's closed five (AGS-13), and a database written
//! before AGS-13 is migrated to it, durably, the first time it is opened.

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex as StdMutex, RwLock};
use tokio::sync::broadcast;
use tokio::sync::Mutex as AsyncMutex;

/// Lifecycle status. Serialized with frontmatter casing (`Pending`, `Ready`, …)
/// so a ticket round-trips into a plans/ file unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Status {
    Pending,
    Ready,
    Blocked,
    Complete,
    Idea,
    Epic,
    Superseded,
}

impl FromStr for Status {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "blocked" => Ok(Self::Blocked),
            "complete" => Ok(Self::Complete),
            "idea" => Ok(Self::Idea),
            "epic" => Ok(Self::Epic),
            "superseded" => Ok(Self::Superseded),
            other => Err(format!(
                "unknown status '{other}' (expected Pending, Ready, Blocked, Complete, Idea, Epic, or Superseded)"
            )),
        }
    }
}

/// What shape of work a ticket is (AGS-13): the owner's closed five-value
/// set, one vocabulary owning the slot. Serialized lowercase (`kind:
/// build`) on the wire and in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A container that decomposes into children; done only when they
    /// are. Its acceptance is the operator's gate.
    Scope,
    /// Carries out a defined procedure (verify, run, migrate, release)
    /// without producing new capability.
    Execution,
    /// Produces new capability; gated on acceptance criteria.
    Build,
    /// A defect or missing piece discovered in shipped behavior (rim's
    /// "bug").
    Gap,
    /// A binary investigation that answers a question.
    Spike,
}

impl Kind {
    /// The phrase every "unknown kind" error ends with — the five, named.
    pub const EXPECTED: &'static str = "scope, execution, build, gap, or spike";
}

impl FromStr for Kind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "scope" => Ok(Self::Scope),
            "execution" => Ok(Self::Execution),
            "build" => Ok(Self::Build),
            "gap" => Ok(Self::Gap),
            "spike" => Ok(Self::Spike),
            other => Err(format!(
                "unknown kind '{other}' (expected {})",
                Kind::EXPECTED
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

impl FromStr for Severity {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "critical" => Ok(Self::Critical),
            "high" => Ok(Self::High),
            "medium" => Ok(Self::Medium),
            "low" => Ok(Self::Low),
            other => Err(format!(
                "unknown severity '{other}' (expected Critical, High, Medium, or Low)"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Ticket {
    /// `EPIC-N`, allocated by the store.
    pub id: String,
    /// Epic prefix, uppercase (e.g. `MNG`).
    pub epic: String,
    /// Sequence number within the epic (the `N` in `EPIC-N`).
    pub seq: u32,
    pub title: String,
    pub status: Status,
    pub kind: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// Ticket ids that must complete before this one starts.
    pub blocked_by: Vec<String>,
    /// Ticket ids that can start once this one completes.
    pub unlocks: Vec<String>,
    /// Markdown body (Problem / Required behavior / Acceptance criteria …).
    pub body: String,
    pub created_unix: i64,
    pub updated_unix: i64,
}

/// A mutation, broadcast to every `watch` subscriber. (Internal — the
/// wire form is `TicketsEvent`, same `type` tag; `kind` is a ticket's.)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Change {
    Created { ticket: Ticket },
    StatusChanged { ticket: Ticket, previous: Status },
    /// An existing ticket's `unlocks` gained `added` because a newly
    /// created ticket named it in `blocked_by`. Carries the full updated
    /// ticket so watch-only clients stay coherent.
    UnlocksChanged { ticket: Ticket, added: String },
}

impl Change {
    pub fn ticket(&self) -> &Ticket {
        match self {
            Self::Created { ticket }
            | Self::StatusChanged { ticket, .. }
            | Self::UnlocksChanged { ticket, .. } => ticket,
        }
    }
}

/// One entry of a ticket's `blocked_by`, resolved against current state.
#[derive(Debug, Clone)]
pub enum BlockerRef {
    /// The blocker exists; carries it at its current status.
    Resolved(Ticket),
    /// The id names no existing ticket (a dangling forward reference).
    Dangling(String),
}

pub struct NewTicket {
    pub epic: String,
    pub title: String,
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub blocked_by: Vec<String>,
    pub body: String,
}

#[derive(Debug)]
pub enum StoreError {
    NotFound(String),
    InvalidEpic(String),
    /// Creating `id` with the requested `blocked_by` would close a cycle;
    /// `path` is the blocked_by chain from `id` back to `id`.
    Cycle { id: String, path: Vec<String> },
    /// The write could not be made durable; nothing was acknowledged.
    Db(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "no ticket with id '{id}'"),
            Self::InvalidEpic(e) => write!(
                f,
                "invalid epic '{e}' (expected a short alphanumeric prefix like MNG)"
            ),
            Self::Cycle { id, path } => write!(
                f,
                "creating '{id}' would close a blocked_by cycle: {id} -> {}",
                path.join(" -> ")
            ),
            Self::Db(msg) => write!(f, "database error: {msg}"),
        }
    }
}

/// Walks `blocked_by` edges from the proposed blockers through existing
/// tickets; if the allocated `new_id` is reachable (self-reference
/// included), returns the blocked_by chain that closes the cycle.
/// Dangling ids other than `new_id` terminate their branch.
fn find_cycle(
    tickets: &HashMap<String, Ticket>,
    new_id: &str,
    blocked_by: &[String],
) -> Option<Vec<String>> {
    let mut stack: Vec<Vec<String>> = blocked_by.iter().map(|b| vec![b.clone()]).collect();
    let mut visited: HashSet<String> = HashSet::new();
    while let Some(path) = stack.pop() {
        let last = path.last().expect("paths start non-empty");
        if last == new_id {
            return Some(path);
        }
        if !visited.insert(last.clone()) {
            continue;
        }
        if let Some(t) = tickets.get(last) {
            for b in &t.blocked_by {
                let mut next = path.clone();
                next.push(b.clone());
                stack.push(next);
            }
        }
    }
    None
}

/// A blocker satisfies the ready-set rule when it is done with.
fn satisfied(status: Status) -> bool {
    matches!(status, Status::Complete | Status::Superseded)
}

pub struct Store {
    /// The cache. Every read in the process is answered from here; the
    /// lock is synchronous and never held across an await.
    tickets: RwLock<HashMap<String, Ticket>>,
    tx: broadcast::Sender<Change>,
    /// The system of record. Private to this module — nothing else in the
    /// application holds a database handle. Only touched off-runtime, via
    /// `spawn_blocking`.
    db: Arc<StdMutex<Connection>>,
    /// Serializes mutations end to end: id allocation, the durable write,
    /// the cache update, and the broadcast happen atomically with respect
    /// to other writers, so ids are unique under concurrent creates and
    /// `watch` subscribers see changes in acknowledgment order. Readers
    /// never take this lock.
    write_serial: AsyncMutex<()>,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    /// An in-memory store (SQLite `:memory:`): the same write-through path
    /// with no file behind it. For unit tests; the server always opens a
    /// real location via [`Store::open`].
    pub fn new() -> Self {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        Self::with_connection(conn).expect("init in-memory schema")
    }

    /// Opens (creating if absent) the database at `path` and loads every
    /// ticket into the cache. A fresh location is an empty store, not an
    /// error. Does blocking I/O — call from a blocking context (the
    /// builder wraps this in `spawn_blocking`).
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        // WAL + synchronous=FULL: an acknowledged commit survives SIGKILL.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch("PRAGMA synchronous = FULL;")?;
        Self::with_connection(conn)
    }

    fn with_connection(mut conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch(db::SCHEMA)?;
        // A database written before AGS-13 is rewritten to kinds here, once,
        // durably — before anything is loaded. An unmappable row refuses
        // the open (loudly, nothing changed) rather than loading a guess.
        db::migrate_legacy_ticket_type(&mut conn)?;
        let tickets = db::load_all(&conn)?;
        let (tx, _) = broadcast::channel(256);
        Ok(Self {
            tickets: RwLock::new(tickets),
            tx,
            db: Arc::new(StdMutex::new(conn)),
            write_serial: AsyncMutex::new(()),
        })
    }

    /// Writes a batch of tickets durably in one transaction, off the
    /// async runtime. On `Err` nothing has been acknowledged: the cache
    /// is untouched and no event is emitted.
    async fn persist(&self, batch: Vec<Ticket>) -> Result<(), StoreError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
            let mut conn = db.lock().expect("db lock poisoned");
            let tx = conn.transaction()?;
            for t in &batch {
                db::upsert(&tx, t)?;
            }
            tx.commit()
        })
        .await
        .map_err(|e| StoreError::Db(format!("write task failed: {e}")))?
        .map_err(|e| StoreError::Db(e.to_string()))
    }

    /// Allocates the next `EPIC-N` id, maintains the DAG, writes through
    /// to the database, and only then updates the cache and notifies
    /// `watch` subscribers.
    ///
    /// DAG maintenance: a create whose allocated id would close a
    /// `blocked_by` cycle (self-reference included) is rejected before
    /// anything persists — nothing stored, the id stays unallocated.
    /// Otherwise the new id is appended to each existing blocker's
    /// `unlocks`, and any existing tickets whose `blocked_by` already
    /// named the allocated id (dangling forward references) become the
    /// newborn's `unlocks` from birth. The newborn and every updated
    /// blocker commit in one transaction; broadcasts (`Created`, then one
    /// `UnlocksChanged` per blocker) follow the commit, in
    /// acknowledgment order.
    pub async fn create(&self, new: NewTicket) -> Result<Ticket, StoreError> {
        let epic = new.epic.trim().to_ascii_uppercase();
        if epic.is_empty() || !epic.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(StoreError::InvalidEpic(new.epic));
        }

        let _serial = self.write_serial.lock().await;
        // Under the write-serial lock the cache mirrors the database
        // exactly, so max-existing-seq + 1 is the next id — monotonic
        // across restarts (the cache was loaded from the database) and
        // race-free (writers are serialized). This read phase computes
        // everything and mutates nothing: a rejection leaves no trace.
        let (ticket, updated_blockers) = {
            let tickets = self.tickets.read().expect("store lock poisoned");
            let seq = tickets
                .values()
                .filter(|t| t.epic == epic)
                .map(|t| t.seq)
                .max()
                .unwrap_or(0)
                + 1;
            let id = format!("{epic}-{seq}");

            if let Some(path) = find_cycle(&tickets, &id, &new.blocked_by) {
                return Err(StoreError::Cycle { id, path });
            }

            // Dangling forward references to this id resolve now: their
            // owners are the newborn's `unlocks` from birth.
            let mut referrers: Vec<(String, u32, String)> = tickets
                .values()
                .filter(|t| t.blocked_by.iter().any(|b| b == &id))
                .map(|t| (t.epic.clone(), t.seq, t.id.clone()))
                .collect();
            referrers.sort();
            let unlocks: Vec<String> = referrers.into_iter().map(|(_, _, rid)| rid).collect();

            let now = chrono::Utc::now().timestamp();
            let ticket = Ticket {
                id: id.clone(),
                epic,
                seq,
                title: new.title,
                status: Status::Pending,
                kind: new.kind,
                severity: new.severity,
                blocked_by: new.blocked_by,
                unlocks,
                body: new.body,
                created_unix: now,
                updated_unix: now,
            };

            // Symmetry: each existing blocker gains the new id in
            // `unlocks`. Updated clones only — the cache changes after
            // the database commit.
            let mut updated_blockers = Vec::new();
            for blocker_id in &ticket.blocked_by {
                if let Some(blocker) = tickets.get(blocker_id) {
                    if !blocker.unlocks.contains(&id) {
                        let mut updated = blocker.clone();
                        updated.unlocks.push(id.clone());
                        updated.updated_unix = now;
                        updated_blockers.push(updated);
                    }
                }
            }
            (ticket, updated_blockers)
        };

        let mut batch = updated_blockers.clone();
        batch.push(ticket.clone());
        self.persist(batch).await?;

        {
            let mut tickets = self.tickets.write().expect("store lock poisoned");
            for blocker in &updated_blockers {
                tickets.insert(blocker.id.clone(), blocker.clone());
            }
            tickets.insert(ticket.id.clone(), ticket.clone());
        }
        let _ = self.tx.send(Change::Created {
            ticket: ticket.clone(),
        });
        for blocker in updated_blockers {
            let _ = self.tx.send(Change::UnlocksChanged {
                ticket: blocker,
                added: ticket.id.clone(),
            });
        }
        Ok(ticket)
    }

    pub fn get(&self, id: &str) -> Option<Ticket> {
        self.tickets
            .read()
            .expect("store lock poisoned")
            .get(id)
            .cloned()
    }

    /// Tickets ordered by (epic, seq), optionally filtered.
    pub fn list(&self, epic: Option<&str>, status: Option<Status>) -> Vec<Ticket> {
        let epic = epic.map(str::to_ascii_uppercase);
        let mut out: Vec<Ticket> = self
            .tickets
            .read()
            .expect("store lock poisoned")
            .values()
            .filter(|t| epic.as_deref().is_none_or(|e| t.epic == e))
            .filter(|t| status.is_none_or(|s| t.status == s))
            .cloned()
            .collect();
        out.sort_by(|a, b| a.epic.cmp(&b.epic).then(a.seq.cmp(&b.seq)));
        out
    }

    /// Changes the status, write-through: durable in the database before
    /// the cache is updated and subscribers are notified.
    pub async fn set_status(&self, id: &str, status: Status) -> Result<Change, StoreError> {
        let _serial = self.write_serial.lock().await;
        let mut ticket = self
            .get(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        let previous = ticket.status;
        ticket.status = status;
        ticket.updated_unix = chrono::Utc::now().timestamp();

        self.persist(vec![ticket.clone()]).await?;
        self.tickets
            .write()
            .expect("store lock poisoned")
            .insert(ticket.id.clone(), ticket.clone());
        let change = Change::StatusChanged { ticket, previous };
        let _ = self.tx.send(change.clone());
        Ok(change)
    }

    /// Tickets that can start now: status `Ready` and every `blocked_by`
    /// entry resolves to a ticket whose status is `Complete` or
    /// `Superseded`. A dangling blocker (id naming no existing ticket)
    /// keeps its ticket out. The epic filter applies to membership;
    /// blockers resolve globally. Ordered by (epic, seq).
    pub fn ready_set(&self, epic: Option<&str>) -> Vec<Ticket> {
        let epic = epic.map(str::to_ascii_uppercase);
        let tickets = self.tickets.read().expect("store lock poisoned");
        let mut out: Vec<Ticket> = tickets
            .values()
            .filter(|t| epic.as_deref().is_none_or(|e| t.epic == e))
            .filter(|t| t.status == Status::Ready)
            .filter(|t| {
                t.blocked_by
                    .iter()
                    .all(|b| tickets.get(b).is_some_and(|bt| satisfied(bt.status)))
            })
            .cloned()
            .collect();
        drop(tickets);
        out.sort_by(|a, b| a.epic.cmp(&b.epic).then(a.seq.cmp(&b.seq)));
        out
    }

    /// One ticket's `blocked_by`, each entry resolved to the current
    /// ticket or marked dangling, in `blocked_by` order.
    pub fn blockers(&self, id: &str) -> Result<Vec<BlockerRef>, StoreError> {
        let tickets = self.tickets.read().expect("store lock poisoned");
        let ticket = tickets
            .get(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        Ok(ticket
            .blocked_by
            .iter()
            .map(|b| match tickets.get(b) {
                Some(t) => BlockerRef::Resolved(t.clone()),
                None => BlockerRef::Dangling(b.clone()),
            })
            .collect())
    }

    pub fn count(&self) -> usize {
        self.tickets.read().expect("store lock poisoned").len()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.tx.subscribe()
    }
}

/// SQL for the system of record. Private: the rest of the application
/// only ever sees the cache above.
mod db {
    use super::Ticket;
    use rusqlite::types::Type;
    use rusqlite::{params, Connection};
    use std::collections::HashMap;
    use std::str::FromStr;

    pub(super) const SCHEMA: &str = "
        CREATE TABLE IF NOT EXISTS tickets (
            id           TEXT PRIMARY KEY,
            epic         TEXT NOT NULL,
            seq          INTEGER NOT NULL,
            title        TEXT NOT NULL,
            status       TEXT NOT NULL,
            kind         TEXT NOT NULL,
            severity     TEXT,
            blocked_by   TEXT NOT NULL,
            unlocks      TEXT NOT NULL,
            body         TEXT NOT NULL,
            created_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL,
            UNIQUE (epic, seq)
        );
    ";

    /// The pre-AGS-13 column, and its vocabulary mapped onto kinds (the
    /// pinned migration). This is the only place the legacy vocabulary
    /// survives: it exists to be read once and rewritten, never written.
    const LEGACY_COLUMN: &str = "ticket_type";

    fn legacy_kind(ticket_type: &str) -> Option<&'static str> {
        match ticket_type {
            "epic" => Some("scope"),
            "implementation" => Some("build"),
            "analysis" => Some("spike"),
            "checkpoint" => Some("execution"),
            "spike" => Some("spike"),
            _ => None,
        }
    }

    pub(super) fn has_column(
        conn: &Connection,
        table: &str,
        column: &str,
    ) -> rusqlite::Result<bool> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let names = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for name in names {
            if name? == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// One-shot, durable migration of a pre-AGS-13 database. If the
    /// `tickets` table still carries `ticket_type`, every row is checked
    /// against the pinned mapping first — one unmappable value refuses
    /// the open, naming the rows, with nothing changed — then, in a single
    /// transaction, the column is renamed to `kind` and each row's value
    /// is rewritten. Committed before the cache loads, so a restart (or a
    /// SIGKILL right after) finds kinds in the file, never the old
    /// values, and the check above is a no-op from then on. Returns how
    /// many rows were rewritten (0: nothing to migrate).
    pub(super) fn migrate_legacy_ticket_type(conn: &mut Connection) -> anyhow::Result<usize> {
        if !has_column(conn, "tickets", LEGACY_COLUMN)? {
            return Ok(0);
        }
        let tx = conn.transaction()?;
        let rows: Vec<(String, String)> = {
            let mut stmt = tx.prepare(&format!("SELECT id, {LEGACY_COLUMN} FROM tickets"))?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let unmappable: Vec<String> = rows
            .iter()
            .filter(|(_, tt)| legacy_kind(tt).is_none())
            .map(|(id, tt)| format!("{id} ({LEGACY_COLUMN} '{tt}')"))
            .collect();
        if !unmappable.is_empty() {
            anyhow::bail!(
                "refusing to open: {} ticket(s) carry a legacy {LEGACY_COLUMN} outside the pinned \
                 kind mapping (epic->scope, implementation->build, analysis->spike, \
                 checkpoint->execution, spike->spike): {}. Nothing was changed; fix those rows \
                 and reopen.",
                unmappable.len(),
                unmappable.join(", ")
            );
        }
        tx.execute(
            &format!("ALTER TABLE tickets RENAME COLUMN {LEGACY_COLUMN} TO kind"),
            [],
        )?;
        for (id, tt) in &rows {
            let kind = legacy_kind(tt).expect("checked above");
            tx.execute("UPDATE tickets SET kind = ?1 WHERE id = ?2", params![kind, id])?;
        }
        tx.commit()?;
        tracing::info!(
            "migrated {} tickets: {LEGACY_COLUMN} -> kind (AGS-13), durably",
            rows.len()
        );
        Ok(rows.len())
    }

    /// Serializes a unit enum to its serde string form (`Pending`,
    /// `build`, `Critical`) — the same casing the wire uses, and what the
    /// `FromStr` impls parse back.
    fn enum_str<T: serde::Serialize>(v: &T) -> String {
        match serde_json::to_value(v) {
            Ok(serde_json::Value::String(s)) => s,
            other => unreachable!("unit enum must serialize to a string, got {other:?}"),
        }
    }

    fn parse_col<T: FromStr>(idx: usize, s: &str) -> rusqlite::Result<T>
    where
        T::Err: std::fmt::Display,
    {
        s.parse().map_err(|e: T::Err| {
            rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, format!("{e}").into())
        })
    }

    fn parse_ids(idx: usize, s: &str) -> rusqlite::Result<Vec<String>> {
        serde_json::from_str(s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, format!("{e}").into())
        })
    }

    /// Inserts or replaces one ticket. A single implicit transaction:
    /// when this returns Ok, the row is durable.
    pub(super) fn upsert(conn: &Connection, t: &Ticket) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO tickets
                 (id, epic, seq, title, status, kind, severity,
                  blocked_by, unlocks, body, created_unix, updated_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT (id) DO UPDATE SET
                 epic = excluded.epic,
                 seq = excluded.seq,
                 title = excluded.title,
                 status = excluded.status,
                 kind = excluded.kind,
                 severity = excluded.severity,
                 blocked_by = excluded.blocked_by,
                 unlocks = excluded.unlocks,
                 body = excluded.body,
                 created_unix = excluded.created_unix,
                 updated_unix = excluded.updated_unix",
            params![
                t.id,
                t.epic,
                t.seq,
                t.title,
                enum_str(&t.status),
                enum_str(&t.kind),
                t.severity.as_ref().map(enum_str),
                serde_json::to_string(&t.blocked_by).expect("serialize ids"),
                serde_json::to_string(&t.unlocks).expect("serialize ids"),
                t.body,
                t.created_unix,
                t.updated_unix,
            ],
        )?;
        Ok(())
    }

    pub(super) fn load_all(conn: &Connection) -> anyhow::Result<HashMap<String, Ticket>> {
        let mut stmt = conn.prepare(
            "SELECT id, epic, seq, title, status, kind, severity,
                    blocked_by, unlocks, body, created_unix, updated_unix
             FROM tickets",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Ticket {
                id: row.get(0)?,
                epic: row.get(1)?,
                seq: row.get(2)?,
                title: row.get(3)?,
                status: parse_col(4, &row.get::<_, String>(4)?)?,
                kind: parse_col(5, &row.get::<_, String>(5)?)?,
                severity: row
                    .get::<_, Option<String>>(6)?
                    .as_deref()
                    .map(|s| parse_col(6, s))
                    .transpose()?,
                blocked_by: parse_ids(7, &row.get::<_, String>(7)?)?,
                unlocks: parse_ids(8, &row.get::<_, String>(8)?)?,
                body: row.get(9)?,
                created_unix: row.get(10)?,
                updated_unix: row.get(11)?,
            })
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let ticket = row?;
            out.insert(ticket.id.clone(), ticket);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_ticket(epic: &str, title: &str) -> NewTicket {
        NewTicket {
            epic: epic.into(),
            title: title.into(),
            kind: Kind::Build,
            severity: None,
            blocked_by: Vec::new(),
            body: String::new(),
        }
    }

    /// The store's schema as it was before AGS-13 (`ticket_type`), so a
    /// test can build "a database created before this ticket" by hand.
    const LEGACY_SCHEMA: &str = "
        CREATE TABLE tickets (
            id           TEXT PRIMARY KEY,
            epic         TEXT NOT NULL,
            seq          INTEGER NOT NULL,
            title        TEXT NOT NULL,
            status       TEXT NOT NULL,
            ticket_type  TEXT NOT NULL,
            severity     TEXT,
            blocked_by   TEXT NOT NULL,
            unlocks      TEXT NOT NULL,
            body         TEXT NOT NULL,
            created_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL,
            UNIQUE (epic, seq)
        );
    ";

    /// Writes a pre-AGS-13 database at `path` holding one row per
    /// `(seq, ticket_type)` in `rows`, epic `LEG`, bodies with frontmatter.
    fn write_legacy_db(path: &Path, rows: &[(u32, &str)]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(LEGACY_SCHEMA).unwrap();
        for (seq, tt) in rows {
            conn.execute(
                "INSERT INTO tickets VALUES (?1, 'LEG', ?2, ?3, 'Pending', ?4, NULL, '[]', '[]', ?5, 1, 1)",
                rusqlite::params![
                    format!("LEG-{seq}"),
                    seq,
                    format!("legacy {tt}"),
                    tt,
                    format!("---\nid: LEG-{seq}\ntype: {tt}\n---\n\nbody"),
                ],
            )
            .unwrap();
        }
    }

    fn columns(path: &Path) -> Vec<String> {
        let conn = Connection::open(path).unwrap();
        let mut stmt = conn.prepare("PRAGMA table_info(tickets)").unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// AGS-13 criterion 1 at the store level: every legacy value maps per
    /// the pinned table, the rows are rewritten in the file (not remapped
    /// per load), and the body's frontmatter is left exactly as it was.
    #[tokio::test]
    async fn legacy_ticket_type_migrates_durably_on_first_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        write_legacy_db(
            &path,
            &[
                (1, "epic"),
                (2, "implementation"),
                (3, "analysis"),
                (4, "checkpoint"),
                (5, "spike"),
            ],
        );
        assert!(columns(&path).contains(&"ticket_type".to_string()));

        let expected = [
            ("LEG-1", Kind::Scope),
            ("LEG-2", Kind::Build),
            ("LEG-3", Kind::Spike),
            ("LEG-4", Kind::Execution),
            ("LEG-5", Kind::Spike),
        ];
        {
            let store = Store::open(&path).unwrap();
            assert_eq!(store.count(), 5);
            for (id, kind) in expected {
                let t = store.get(id).unwrap();
                assert_eq!(t.kind, kind, "{id}");
                assert!(t.body.contains("type: "), "{id}: frontmatter untouched");
            }
        }

        // The file itself now speaks kinds: column renamed, values rewritten.
        let cols = columns(&path);
        assert!(cols.contains(&"kind".to_string()), "{cols:?}");
        assert!(!cols.contains(&"ticket_type".to_string()), "{cols:?}");
        {
            let conn = Connection::open(&path).unwrap();
            let raw: Vec<(String, String)> = conn
                .prepare("SELECT id, kind FROM tickets ORDER BY seq")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(
                raw,
                vec![
                    ("LEG-1".into(), "scope".into()),
                    ("LEG-2".into(), "build".into()),
                    ("LEG-3".into(), "spike".into()),
                    ("LEG-4".into(), "execution".into()),
                    ("LEG-5".into(), "spike".into()),
                ]
            );
            assert!(!db::has_column(&conn, "tickets", "ticket_type").unwrap());
        }

        // Second open: nothing to migrate, same kinds, still writable.
        let store = Store::open(&path).unwrap();
        for (id, kind) in expected {
            assert_eq!(store.get(id).unwrap().kind, kind, "{id} after reopen");
        }
        let next = store.create(new_ticket("LEG", "born after the migration")).await.unwrap();
        assert_eq!(next.id, "LEG-6");
        assert_eq!(next.kind, Kind::Build);
    }

    /// A legacy value outside the pinned mapping refuses the open,
    /// naming the row, and leaves the database exactly as it was.
    #[test]
    fn legacy_value_outside_the_mapping_fails_the_open_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("odd.db");
        write_legacy_db(&path, &[(1, "implementation"), (2, "wibble")]);

        let err = match Store::open(&path) {
            Ok(_) => panic!("an unmappable legacy value must refuse the open"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("LEG-2"), "names the row: {err}");
        assert!(err.contains("'wibble'"), "names the value: {err}");
        assert!(err.contains("implementation->build"), "shows the mapping: {err}");

        // Untouched: the legacy column and both values are still there.
        let cols = columns(&path);
        assert!(cols.contains(&"ticket_type".to_string()), "{cols:?}");
        assert!(!cols.contains(&"kind".to_string()), "{cols:?}");
        let conn = Connection::open(&path).unwrap();
        let tt: String = conn
            .query_row("SELECT ticket_type FROM tickets WHERE id = 'LEG-1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tt, "implementation", "the good row was not rewritten either");
    }

    #[test]
    fn unknown_kind_error_names_the_five() {
        let err = "bug".parse::<Kind>().unwrap_err();
        for name in ["scope", "execution", "build", "gap", "spike"] {
            assert!(err.contains(name), "{err}");
        }
        assert!(err.contains("'bug'"), "{err}");
        assert_eq!("GAP".parse::<Kind>(), Ok(Kind::Gap), "case-insensitive");
    }

    #[tokio::test]
    async fn reopen_round_trips_every_field_and_continues_seq() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tickets.db");

        let after_flip = {
            let store = Store::open(&path).unwrap();
            store
                .create(NewTicket {
                    epic: "rt".into(),
                    title: "first — ünïcode ✓".into(),
                    kind: Kind::Gap,
                    severity: Some(Severity::Critical),
                    blocked_by: vec!["RT-0".into(), "X-9".into()],
                    body: "## Problem\n\nlines\nand more".into(),
                })
                .await
                .unwrap();
            match store.set_status("RT-1", Status::Ready).await.unwrap() {
                Change::StatusChanged { ticket, previous } => {
                    assert_eq!(previous, Status::Pending);
                    ticket
                }
                other => panic!("expected StatusChanged, got {other:?}"),
            }
        };

        // Restart: same location, everything identical.
        let store = Store::open(&path).unwrap();
        assert_eq!(store.count(), 1);
        assert_eq!(store.get("RT-1").unwrap(), after_flip);

        // Id allocation continues from the persisted max, not from 1.
        let next = store.create(new_ticket("RT", "second")).await.unwrap();
        assert_eq!(next.id, "RT-2");
        assert_eq!(next.seq, 2);
    }

    #[tokio::test]
    async fn fresh_location_is_an_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("brand-new.db")).unwrap();
        assert_eq!(store.count(), 0);
        assert!(store.create(new_ticket("NEW", "works")).await.is_ok());
    }

    #[tokio::test]
    async fn concurrent_creates_never_collide_on_ids() {
        let store = Arc::new(Store::new());
        let mut handles = Vec::new();
        for i in 0..16 {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                store
                    .create(new_ticket("RACE", &format!("t{i}")))
                    .await
                    .unwrap()
                    .id
            }));
        }
        let mut ids = Vec::new();
        for h in handles {
            ids.push(h.await.unwrap());
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 16, "every concurrent create got a unique id");
    }
}
