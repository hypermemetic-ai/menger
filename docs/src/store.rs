//! **The docs store** — the documents, in a SQLite file of the service's
//! own, `docs.db`: never the tickets file (V2-1: the file is opened by the
//! tickets service alone).
//!
//! A document's body is a **Yjs CRDT** (V2-29's `pairing`, the owner: *"live
//! co editting … we should keep all changes over time too, so we could
//! replay how it got built. with the ability to squash, permission
//! gated"*). **Stored** in two tables:
//!
//! - `docs` — a row per document as it is now: its organization and title,
//!   the markdown derived from its CRDT (`text`, what `get`, `list`, search,
//!   agents and image references read), its revision (the last change's
//!   `seq`), who made it and who wrote it last, and a snapshot of its CRDT
//!   (`state` as of `state_seq`) so a load applies only what came after.
//!   `deleted_at` set once deleted: it reads as none.
//! - `doc_log` — **every change, forever until squashed**: a row per CRDT
//!   update (`update`, the Yjs v1 bytes as the writer sent or the service
//!   made them), per rename, per delete, with who (`by`, `principal`) and
//!   when (`at`, unix milliseconds); `seq` counts them per document. A squash
//!   ([`crate::crdt`]) replaces the rows up to a moment with one `snapshot`
//!   row holding who wrote what as its `authors`.
//!
//! How the body is written, replayed and squashed is [`crate::crdt`]'s; who
//! is on a document is [`crate::presence`]'s. Whether a caller may read or
//! write a document is the gate's word on its organization, asked by the
//! activation ([`crate::seam`]). Every write is said on the channel
//! ([`DocsStore::subscribe`]) for `watch`.
//!
//! One connection behind a mutex; every read and write off the runtime.

use crate::events::{Doc, Present, Thread};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// The tables. `docs.seq` orders the list (newest made first) and is a
/// page's cursor through its id.
pub const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS docs (
            seq         INTEGER PRIMARY KEY AUTOINCREMENT,
            id          TEXT NOT NULL UNIQUE,
            org         TEXT NOT NULL,
            title       TEXT NOT NULL,
            text        TEXT NOT NULL,
            revision    INTEGER NOT NULL,
            created_at  INTEGER NOT NULL,
            created_by  TEXT NOT NULL,
            at          INTEGER NOT NULL,
            by          TEXT NOT NULL,
            state       BLOB NOT NULL,
            state_seq   INTEGER NOT NULL,
            deleted_at  INTEGER
        );
        CREATE TABLE IF NOT EXISTS doc_log (
            id         TEXT NOT NULL,
            seq        INTEGER NOT NULL,
            kind       TEXT NOT NULL,
            update_v1  BLOB,
            title      TEXT,
            at         INTEGER NOT NULL,
            by         TEXT NOT NULL,
            principal  TEXT NOT NULL,
            bytes      INTEGER NOT NULL,
            authors    TEXT,
            PRIMARY KEY (id, seq)
        );
    ";

/// How many writes an open watch may fall behind before it hears `lagged`.
pub const CHANNEL: usize = 4096;

/// A write, as the channel says it to every open watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A document made (body included).
    Created(Doc),
    /// A CRDT update applied: the document after it (body included), the
    /// update's bytes (base64), its `seq`, and the presence session it came
    /// from, when it said.
    Updated { doc: Doc, update: String, seq: u32, session: Option<String> },
    Renamed(Doc),
    Deleted { id: String, org: String, at: i64, by: String },
    /// The changes up to `until` collapsed into one snapshot.
    Squashed { id: String, org: String, until: u32, removed: u32, at: i64, by: String },
    /// Someone is on a document, or moved their caret.
    Present { org: String, present: Present },
    /// Someone left a document.
    Left { id: String, org: String, session: String, client: Option<u64> },
    /// V2-30: a question asked on a range, a reply under it, a thread resolved (or opened again).
    Asked { org: String, thread: Thread },
    Replied { org: String, thread: Thread },
    Resolved { org: String, thread: Thread },
}

impl Change {
    /// The document it is of.
    pub fn id(&self) -> &str {
        match self {
            Change::Created(d) | Change::Renamed(d) | Change::Updated { doc: d, .. } => &d.id,
            Change::Deleted { id, .. } | Change::Squashed { id, .. } | Change::Left { id, .. } => id,
            Change::Present { present, .. } => &present.id,
            Change::Asked { thread, .. } | Change::Replied { thread, .. } | Change::Resolved { thread, .. } => &thread.id,
        }
    }

    /// The organization that document is in.
    pub fn org(&self) -> &str {
        match self {
            Change::Created(d) | Change::Renamed(d) | Change::Updated { doc: d, .. } => &d.org,
            Change::Deleted { org, .. } | Change::Squashed { org, .. } | Change::Present { org, .. } | Change::Left { org, .. } => org,
            Change::Asked { org, .. } | Change::Replied { org, .. } | Change::Resolved { org, .. } => org,
        }
    }
}

/// Why the store did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// No document of that id (or a deleted one).
    NotFound(String),
    /// The document has no change at that `seq` (or it was squashed).
    NoRevision(String, u32),
    /// V2-30: no thread of that id on the document.
    NoThread(String),
    /// V2-30: a range that is not one of the text.
    BadRange(String),
    /// The update or state is not one the CRDT reads.
    BadUpdate(String),
    /// The quota refused the write (its words).
    Quota(String),
    /// V2-39: the quota refused it, with its reason (`over_allowance`, …).
    Allowance(crate::quota::Refused),
    /// The file.
    Db(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::NotFound(id) => write!(f, "no document {id}"),
            StoreError::NoRevision(id, r) => write!(f, "document {id} has no change {r}"),
            StoreError::NoThread(t) => write!(f, "no thread {t}"),
            StoreError::BadRange(e) => write!(f, "not a range of the text: {e}"),
            StoreError::BadUpdate(e) => write!(f, "not a Yjs v1 update: {e}"),
            StoreError::Quota(e) => write!(f, "{e}"),
            StoreError::Allowance(r) => write!(f, "{}", r.message),
            StoreError::Db(e) => write!(f, "the docs file: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Db(e.to_string())
    }
}

/// What a page of `list` asks.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    /// The organizations whose documents may be listed (`None`: every one).
    pub orgs: Option<Vec<String>>,
    /// Words the title holds, case-blind.
    pub query: Option<String>,
    /// The page's size.
    pub limit: usize,
    /// The id of the last row of the page before.
    pub after: Option<String>,
}

/// A page of `list`: its rows, the cursor past it, how many more match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub rows: Vec<Doc>,
    pub next: Option<String>,
    pub remaining: u64,
}

/// The file, open, with the CRDTs it has loaded and who is on them.
pub struct DocsStore {
    conn: Mutex<Connection>,
    path: PathBuf,
    changes: broadcast::Sender<Change>,
    /// Each loaded document's CRDT — loaded on its first write or sync, kept.
    pub(crate) loaded: Mutex<HashMap<String, Arc<Mutex<yrs::Doc>>>>,
    /// Who is on which document ([`crate::presence`]).
    pub(crate) presence: crate::presence::Presence,
    /// The meter every byte written is reserved against (V2-31's).
    pub(crate) quota: std::sync::RwLock<Arc<dyn crate::quota::Quota>>,
}

impl std::fmt::Debug for DocsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocsStore").field("path", &self.path).finish()
    }
}

/// Now, in unix seconds.
pub(crate) fn now() -> i64 {
    now_ms() / 1000
}

/// Now, in unix milliseconds — a log row's `at`.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The columns a [`Doc`] is read from, in [`doc_of`]'s order.
pub(crate) const DOC_COLUMNS: &str = "id, org, title, text, revision, created_at, created_by, at, by";

pub(crate) fn doc_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<Doc> {
    let body: String = r.get(3)?;
    Ok(Doc {
        id: r.get(0)?,
        org: r.get(1)?,
        title: r.get(2)?,
        bytes: body.len() as u64,
        body: Some(body),
        revision: r.get(4)?,
        created_at: r.get(5)?,
        created_by: r.get(6)?,
        at: r.get(7)?,
        by: r.get(8)?,
    })
}

/// The live document `id`, body and all, in `conn`.
pub(crate) fn live_doc(conn: &Connection, id: &str) -> Result<Doc, StoreError> {
    conn.query_row(&format!("SELECT {DOC_COLUMNS} FROM docs WHERE id = ?1 AND deleted_at IS NULL"), [id], doc_of)
        .optional()?
        .ok_or_else(|| StoreError::NotFound(id.to_string()))
}

/// One row of the log, as it is written.
pub(crate) struct LogRow<'a> {
    pub id: &'a str,
    pub seq: u32,
    pub kind: &'a str,
    pub update: Option<&'a [u8]>,
    pub title: Option<&'a str>,
    pub at: i64,
    pub by: &'a str,
    pub principal: &'a str,
    pub authors: Option<&'a str>,
}

/// Appends `row` to the log.
pub(crate) fn log(tx: &Connection, row: LogRow<'_>) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO doc_log (id, seq, kind, update_v1, title, at, by, principal, bytes, authors) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![row.id, row.seq, row.kind, row.update, row.title, row.at, row.by, row.principal, row.update.map_or(0, |u| u.len() as i64), row.authors],
    )?;
    Ok(())
}

impl DocsStore {
    /// The store on `path`, made if missing; a file without the tables
    /// gains them, empty — once; on every later open a no-op. Every write
    /// allowed ([`crate::quota::AllowAll`]) until a meter is set.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| StoreError::Db(format!("{}: {e}", dir.display())))?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        conn.execute_batch(TABLES)?;
        conn.execute_batch(crate::threads::TABLES)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
            changes: broadcast::channel(CHANNEL).0,
            loaded: Mutex::new(HashMap::new()),
            presence: crate::presence::Presence::default(),
            quota: std::sync::RwLock::new(Arc::new(crate::quota::AllowAll)),
        })
    }

    /// The same store, its writes reserved against `quota` (V2-31's meter).
    pub fn metered(self, quota: Arc<dyn crate::quota::Quota>) -> Self {
        self.set_quota(quota);
        self
    }

    /// V2-31: its writes reserved against `quota` from now on — the builder
    /// sets the quota service's meter on a store already shared.
    pub fn set_quota(&self, quota: Arc<dyn crate::quota::Quota>) {
        *self.quota.write().unwrap_or_else(|p| p.into_inner()) = quota;
    }

    /// The meter now.
    pub(crate) fn quota(&self) -> Arc<dyn crate::quota::Quota> {
        self.quota.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// V2-31: what each principal keeps, as the quota service counts it —
    /// **the update log as stored**: every change's bytes its writer's, and
    /// a squash's snapshot shared among its authors by what each wrote
    /// ([`crate::events::shares`]) — so a squash shrinks it; `count` the
    /// documents they made (a `create` change, or, folded into a snapshot,
    /// its earliest author's). (principal, count, bytes), by principal.
    pub async fn usage_by_principal(self: &Arc<Self>) -> Result<Vec<(String, u64, u64)>, StoreError> {
        let rows = self.usage_rows(false).await?;
        Ok(rows.into_iter().map(|((p, _), (n, b))| (p, n, b)).collect())
    }

    /// V2-70: [`Self::usage_by_principal`] by the UTC day each change was
    /// kept (days since 1970-01-01; a snapshot's shares on its own day):
    /// (principal, day, count, bytes) — what the quota service's history is
    /// backfilled from; summed per principal, the same.
    pub async fn usage_daily(self: &Arc<Self>) -> Result<Vec<(String, i64, u64, u64)>, StoreError> {
        let rows = self.usage_rows(true).await?;
        Ok(rows.into_iter().map(|((p, d), (n, b))| (p, d, n, b)).collect())
    }

    /// What each principal keeps, by (principal, day — 0 unless `by_day`).
    async fn usage_rows(self: &Arc<Self>, by_day: bool) -> Result<std::collections::BTreeMap<(String, i64), (u64, u64)>, StoreError> {
        self.blocking(move |s| {
            let conn = s.lock();
            let mut stmt = conn.prepare("SELECT kind, principal, bytes, authors, at FROM doc_log")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, i64>(4)?)))?;
            let mut out: std::collections::BTreeMap<(String, i64), (u64, u64)> = std::collections::BTreeMap::new();
            for row in rows {
                let (kind, principal, bytes, authors, at) = row?;
                let day = if by_day { at.div_euclid(86_400_000) } else { 0 };
                match (kind.as_str(), authors) {
                    ("snapshot", Some(a)) => {
                        let authors: Vec<crate::events::Author> = serde_json::from_str(&a).unwrap_or_default();
                        if let Some(first) = authors.iter().min_by_key(|a| a.first) {
                            out.entry((first.principal.clone(), day)).or_default().0 += 1;
                        }
                        for (who, n) in crate::events::shares(bytes.max(0) as u64, &authors) {
                            out.entry((who, day)).or_default().1 += n;
                        }
                    }
                    (k, _) => {
                        let e = out.entry((principal, day)).or_default();
                        e.0 += u64::from(k == "create");
                        e.1 += bytes.max(0) as u64;
                    }
                }
            }
            Ok(out)
        })
        .await
    }

    /// The file it keeps.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Off the runtime, with the store.
    pub(crate) async fn blocking<T: Send + 'static>(self: &Arc<Self>, work: impl FnOnce(&Arc<DocsStore>) -> Result<T, StoreError> + Send + 'static) -> Result<T, StoreError> {
        let store = Arc::clone(self);
        tokio::task::spawn_blocking(move || work(&store)).await.map_err(|e| StoreError::Db(format!("docs task failed: {e}")))?
    }

    /// Every write from now on, as it is committed.
    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.changes.subscribe()
    }

    /// Say `change` to every open watch.
    pub(crate) fn said(&self, change: Change) {
        let _ = self.changes.send(change);
    }

    /// The document `id` as it is now, body and all; [`StoreError::NotFound`]
    /// for none (or a deleted one).
    pub async fn get(self: &Arc<Self>, id: &str) -> Result<Doc, StoreError> {
        let id = id.to_string();
        self.blocking(move |s| live_doc(&s.lock(), &id)).await
    }

    /// A page of the documents `ask` admits, newest made first, without
    /// their bodies.
    pub async fn list(self: &Arc<Self>, ask: Listing) -> Result<Page, StoreError> {
        self.blocking(move |s| {
            let conn = s.lock();
            let mut sql = format!("SELECT {DOC_COLUMNS}, seq FROM docs WHERE deleted_at IS NULL");
            let mut args: Vec<rusqlite::types::Value> = Vec::new();
            if let Some(orgs) = &ask.orgs {
                if orgs.is_empty() {
                    return Ok(Page { rows: Vec::new(), next: None, remaining: 0 });
                }
                sql.push_str(&format!(" AND org IN ({})", vec!["?"; orgs.len()].join(", ")));
                args.extend(orgs.iter().cloned().map(rusqlite::types::Value::Text));
            }
            for word in ask.query.as_deref().unwrap_or_default().split_whitespace() {
                sql.push_str(" AND instr(lower(title), ?) > 0");
                args.push(rusqlite::types::Value::Text(word.to_lowercase()));
            }
            if let Some(after) = &ask.after {
                let seq: Option<i64> = conn.query_row("SELECT seq FROM docs WHERE id = ?1", [after], |r| r.get(0)).optional()?;
                sql.push_str(" AND seq < ?");
                args.push(rusqlite::types::Value::Integer(seq.unwrap_or(i64::MAX)));
            }
            sql.push_str(" ORDER BY seq DESC");
            let mut stmt = conn.prepare(&sql)?;
            let all = stmt.query_map(rusqlite::params_from_iter(args.iter()), doc_of)?.collect::<Result<Vec<_>, _>>()?;
            let limit = ask.limit.max(1);
            let remaining = all.len().saturating_sub(limit) as u64;
            let rows: Vec<Doc> = all.into_iter().take(limit).map(|d| d.summary()).collect();
            let next = (remaining > 0).then(|| rows.last().map(|d| d.id.clone())).flatten();
            Ok(Page { rows, next, remaining })
        })
        .await
    }

    /// Retitles `id` to `title`, by `by` (`principal`): a change more in the log.
    pub async fn rename(self: &Arc<Self>, id: &str, title: &str, by: &str, principal: &str) -> Result<Doc, StoreError> {
        let (id, title, by, principal) = (id.to_string(), title.to_string(), by.to_string(), principal.to_string());
        let doc = self
            .blocking(move |s| {
                let conn = s.lock();
                let tx = conn.unchecked_transaction()?;
                let was = live_doc(&tx, &id)?;
                let doc = Doc { title, revision: was.revision + 1, at: now(), by, ..was };
                tx.execute("UPDATE docs SET title = ?2, revision = ?3, at = ?4, by = ?5 WHERE id = ?1", params![doc.id, doc.title, doc.revision, doc.at, doc.by])?;
                log(&tx, LogRow { id: &doc.id, seq: doc.revision, kind: "rename", update: None, title: Some(&doc.title), at: now_ms(), by: &doc.by, principal: &principal, authors: None })?;
                tx.commit()?;
                Ok(doc)
            })
            .await?;
        self.said(Change::Renamed(doc.clone()));
        Ok(doc)
    }

    /// Deletes `id`, by `by`: it reads as none from now on; its log, and one
    /// row more saying the delete, stay in the file.
    pub async fn delete(self: &Arc<Self>, id: &str, by: &str, principal: &str) -> Result<(String, i64), StoreError> {
        let (id, by, principal) = (id.to_string(), by.to_string(), principal.to_string());
        let gone = id.clone();
        let (org, at, by) = self
            .blocking(move |s| {
                let conn = s.lock();
                let tx = conn.unchecked_transaction()?;
                let was = live_doc(&tx, &id)?;
                let doc = Doc { revision: was.revision + 1, at: now(), by, ..was };
                tx.execute("UPDATE docs SET deleted_at = ?2, revision = ?3, at = ?2, by = ?4 WHERE id = ?1", params![doc.id, doc.at, doc.revision, doc.by])?;
                log(&tx, LogRow { id: &doc.id, seq: doc.revision, kind: "delete", update: None, title: None, at: now_ms(), by: &doc.by, principal: &principal, authors: None })?;
                tx.commit()?;
                s.loaded.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
                Ok((doc.org, doc.at, doc.by))
            })
            .await?;
        self.said(Change::Deleted { id: gone, org: org.clone(), at, by });
        Ok((org, at))
    }

    /// The organization the live document `id` is in.
    pub async fn org_of(self: &Arc<Self>, id: &str) -> Result<String, StoreError> {
        Ok(self.get(id).await?.org)
    }

    /// How many documents the store keeps — for a log line.
    pub fn count(&self) -> usize {
        self.lock().query_row("SELECT count(*) FROM docs WHERE deleted_at IS NULL", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    }
}

#[cfg(test)]
mod tests;
