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
//! Status/type vocabulary matches the house ticketing convention.

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

/// Unit of work. Frontmatter uses lowercase (`type: implementation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TicketType {
    Implementation,
    Analysis,
    Spike,
    Epic,
    Checkpoint,
}

impl FromStr for TicketType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "implementation" => Ok(Self::Implementation),
            "analysis" => Ok(Self::Analysis),
            "spike" => Ok(Self::Spike),
            "epic" => Ok(Self::Epic),
            "checkpoint" => Ok(Self::Checkpoint),
            other => Err(format!(
                "unknown ticket type '{other}' (expected implementation, analysis, spike, epic, or checkpoint)"
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
    pub ticket_type: TicketType,
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

/// A mutation, broadcast to every `watch` subscriber.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    Created { ticket: Ticket },
    StatusChanged { ticket: Ticket, previous: Status },
}

impl Change {
    pub fn ticket(&self) -> &Ticket {
        match self {
            Self::Created { ticket } | Self::StatusChanged { ticket, .. } => ticket,
        }
    }
}

pub struct NewTicket {
    pub epic: String,
    pub title: String,
    pub ticket_type: TicketType,
    pub severity: Option<Severity>,
    pub blocked_by: Vec<String>,
    pub body: String,
}

#[derive(Debug)]
pub enum StoreError {
    NotFound(String),
    InvalidEpic(String),
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
            Self::Db(msg) => write!(f, "database error: {msg}"),
        }
    }
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

    fn with_connection(conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch(db::SCHEMA)?;
        let tickets = db::load_all(&conn)?;
        let (tx, _) = broadcast::channel(256);
        Ok(Self {
            tickets: RwLock::new(tickets),
            tx,
            db: Arc::new(StdMutex::new(conn)),
            write_serial: AsyncMutex::new(()),
        })
    }

    /// Writes one ticket durably to the database, off the async runtime.
    /// On `Err` nothing has been acknowledged: the cache is untouched and
    /// no event is emitted.
    async fn persist(&self, ticket: Ticket) -> Result<(), StoreError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().expect("db lock poisoned");
            db::upsert(&conn, &ticket)
        })
        .await
        .map_err(|e| StoreError::Db(format!("write task failed: {e}")))?
        .map_err(|e| StoreError::Db(e.to_string()))
    }

    /// Allocates the next `EPIC-N` id, writes through to the database, and
    /// only then updates the cache and notifies `watch` subscribers.
    pub async fn create(&self, new: NewTicket) -> Result<Ticket, StoreError> {
        let epic = new.epic.trim().to_ascii_uppercase();
        if epic.is_empty() || !epic.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(StoreError::InvalidEpic(new.epic));
        }

        let _serial = self.write_serial.lock().await;
        // Under the write lock the cache mirrors the database exactly, so
        // max-existing-seq + 1 is the next id — monotonic across restarts
        // (the cache was loaded from the database) and race-free (writers
        // are serialized).
        let seq = self
            .tickets
            .read()
            .expect("store lock poisoned")
            .values()
            .filter(|t| t.epic == epic)
            .map(|t| t.seq)
            .max()
            .unwrap_or(0)
            + 1;
        let now = chrono::Utc::now().timestamp();
        let ticket = Ticket {
            id: format!("{epic}-{seq}"),
            epic,
            seq,
            title: new.title,
            status: Status::Pending,
            ticket_type: new.ticket_type,
            severity: new.severity,
            blocked_by: new.blocked_by,
            unlocks: Vec::new(),
            body: new.body,
            created_unix: now,
            updated_unix: now,
        };

        self.persist(ticket.clone()).await?;
        self.tickets
            .write()
            .expect("store lock poisoned")
            .insert(ticket.id.clone(), ticket.clone());
        let _ = self.tx.send(Change::Created {
            ticket: ticket.clone(),
        });
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

        self.persist(ticket.clone()).await?;
        self.tickets
            .write()
            .expect("store lock poisoned")
            .insert(ticket.id.clone(), ticket.clone());
        let change = Change::StatusChanged { ticket, previous };
        let _ = self.tx.send(change.clone());
        Ok(change)
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

    /// Serializes a unit enum to its serde string form (`Pending`,
    /// `implementation`, `Critical`) — the same casing the wire uses, and
    /// what the `FromStr` impls parse back.
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
                 (id, epic, seq, title, status, ticket_type, severity,
                  blocked_by, unlocks, body, created_unix, updated_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT (id) DO UPDATE SET
                 epic = excluded.epic,
                 seq = excluded.seq,
                 title = excluded.title,
                 status = excluded.status,
                 ticket_type = excluded.ticket_type,
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
                enum_str(&t.ticket_type),
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
            "SELECT id, epic, seq, title, status, ticket_type, severity,
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
                ticket_type: parse_col(5, &row.get::<_, String>(5)?)?,
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
            ticket_type: TicketType::Implementation,
            severity: None,
            blocked_by: Vec::new(),
            body: String::new(),
        }
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
                    ticket_type: TicketType::Analysis,
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
