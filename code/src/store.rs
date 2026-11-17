//! **What is kept** — the service's own file, `code.db` (never the tickets
//! file), and its files' bytes on disk by SHA-256 (`<root>/<aa>/<sha256>`,
//! `--code-blob-dir`; a fetch stages in `<root>/tmp/`):
//!
//! | table | what |
//! |---|---|
//! | `blobs` | a file's bytes, once: its hash, size, whom they were reserved for, when |
//! | `commits` | a kept commit of a repository: its files, bytes, who fetched it, when it was last read |
//! | `entries` | a kept commit's tree: path, kind, size, blob, a link's target |
//!
//! A commit is kept in one transaction ([`CodeStore::keep`]): its rows, the
//! blobs new to the store, and — past the cap — the commits read longest
//! ago dropped (never one read in the last hour), a blob going only when no
//! kept commit names it; the staged blobs moved into place before it
//! commits. A commit that cannot fit is refused whole, nothing dropped.

use crate::events::TreeEntry;
use crate::unpack::{Entry, Kind, Staged};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Never dropped for room: a commit read within this many seconds.
pub const RECENT_SECS: i64 = 3600;

const TABLES: &str = "
CREATE TABLE IF NOT EXISTS blobs (
    hash TEXT PRIMARY KEY,
    bytes INTEGER NOT NULL,
    charged_to TEXT,
    at INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS commits (
    repo TEXT NOT NULL,
    hash TEXT NOT NULL,
    committed_unix INTEGER,
    files INTEGER NOT NULL,
    bytes INTEGER NOT NULL,
    fetched_by TEXT,
    fetched_unix INTEGER NOT NULL,
    read_unix INTEGER NOT NULL,
    PRIMARY KEY (repo, hash)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS commits_read ON commits (read_unix);
CREATE TABLE IF NOT EXISTS entries (
    repo TEXT NOT NULL,
    hash TEXT NOT NULL,
    path TEXT NOT NULL,
    kind TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    blob TEXT,
    target TEXT,
    PRIMARY KEY (repo, hash, path)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS entries_blob ON entries (blob);
";

/// Why the store did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The commit does not fit under the cap, even with every commit not
    /// read in the last hour dropped: what it would hold, and the cap.
    Full { needed: u64, max: u64 },
    /// A short hash that begins more than one kept commit.
    Ambiguous(String),
    /// The file or the disk failed, in words.
    Db(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Full { needed, max } => write!(
                f,
                "the code store would hold {} with this commit, past its cap of {}, and every other commit was read within the hour",
                crate::github::size(*needed),
                crate::github::size(*max)
            ),
            StoreError::Ambiguous(c) => write!(f, "{c} begins more than one kept commit: give more of its hash"),
            StoreError::Db(e) => write!(f, "the code store: {e}"),
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Db(e.to_string())
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Db(e.to_string())
    }
}

/// A kept commit's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRow {
    pub repo: String,
    pub hash: String,
    pub committed_unix: Option<i64>,
    pub files: u64,
    pub bytes: u64,
    pub fetched_by: Option<String>,
    pub fetched_unix: i64,
    pub read_unix: i64,
}

/// A kept entry, as `read` needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRow {
    pub kind: String,
    pub bytes: u64,
    pub blob: Option<String>,
    pub target: Option<String>,
}

/// What [`CodeStore::keep`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kept {
    pub files: u64,
    pub bytes: u64,
    /// Bytes new to the store, each blob once.
    pub new_bytes: u64,
    /// The commits dropped for room.
    pub dropped: Vec<(String, String)>,
    /// The bytes that went, by whom they were reserved for.
    pub credits: BTreeMap<String, u64>,
}

/// The store: the file and the blobs' root.
#[derive(Debug)]
pub struct CodeStore {
    conn: Mutex<Connection>,
    path: PathBuf,
    root: PathBuf,
}

/// Where the blobs go when `--code-blob-dir` names none: `code/blobs`
/// beside the file.
pub fn default_blob_dir(db: &Path) -> PathBuf {
    db.parent().map_or_else(|| PathBuf::from("code/blobs"), |d| d.join("code").join("blobs"))
}

/// A blob's place under `root`: `<aa>/<sha256>`.
fn blob_at(root: &Path, hash: &str) -> PathBuf {
    root.join(&hash[..2.min(hash.len())]).join(hash)
}

impl CodeStore {
    /// The store on `path`, its blobs under `root` — each made if missing;
    /// a fetch's staging left by a process that died is cleared.
    pub fn open(path: &Path, root: &Path) -> Result<Self, StoreError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| StoreError::Db(format!("{}: {e}", dir.display())))?;
        }
        let tmp = root.join("tmp");
        if tmp.is_dir() {
            let _ = std::fs::remove_dir_all(&tmp);
        }
        std::fs::create_dir_all(&tmp).map_err(|e| StoreError::Db(format!("{}: {e}", tmp.display())))?;
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        conn.execute_batch(TABLES)?;
        Ok(Self { conn: Mutex::new(conn), path: path.to_path_buf(), root: root.to_path_buf() })
    }

    /// The file it keeps.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where the blobs are.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a fetch stages its archive and files.
    pub fn staging(&self) -> PathBuf {
        self.root.join("tmp")
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The kept commit of `repo` that `prefix` (7 to 40 hex digits) begins.
    pub fn resolve(&self, repo: &str, prefix: &str) -> Result<Option<String>, StoreError> {
        let conn = self.conn();
        let mut st = conn.prepare_cached("SELECT hash FROM commits WHERE repo = ?1 AND substr(hash, 1, length(?2)) = ?2 LIMIT 2")?;
        let found: Vec<String> = st.query_map(params![repo, prefix], |r| r.get(0))?.collect::<Result<_, _>>()?;
        match found.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(one.clone())),
            _ => Err(StoreError::Ambiguous(prefix.to_string())),
        }
    }

    /// A kept commit's row.
    pub fn commit(&self, repo: &str, hash: &str) -> Result<Option<CommitRow>, StoreError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT repo, hash, committed_unix, files, bytes, fetched_by, fetched_unix, read_unix FROM commits WHERE repo = ?1 AND hash = ?2",
                params![repo, hash],
                commit_row,
            )
            .optional()?;
        Ok(row)
    }

    /// Every kept commit, the one read longest ago first.
    pub fn kept(&self) -> Result<Vec<CommitRow>, StoreError> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT repo, hash, committed_unix, files, bytes, fetched_by, fetched_unix, read_unix FROM commits ORDER BY read_unix, repo, hash")?;
        let rows = st.query_map([], commit_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// The bytes on disk, each blob once.
    pub fn total(&self) -> Result<u64, StoreError> {
        Ok(self.conn().query_row("SELECT COALESCE(SUM(bytes), 0) FROM blobs", [], |r| r.get::<_, i64>(0))? as u64)
    }

    /// The commit was read `now`.
    pub fn touch(&self, repo: &str, hash: &str, now: i64) -> Result<(), StoreError> {
        self.conn().execute("UPDATE commits SET read_unix = MAX(read_unix, ?3) WHERE repo = ?1 AND hash = ?2", params![repo, hash, now])?;
        Ok(())
    }

    /// Every entry under `path` (all when empty), ordered by path, at most
    /// `limit` (and whether there were more). `None`: `path` is no
    /// directory of the tree.
    pub fn tree(&self, repo: &str, hash: &str, path: &str, limit: usize) -> Result<Option<(Vec<TreeEntry>, bool)>, StoreError> {
        let conn = self.conn();
        if !path.is_empty() {
            let kind: Option<String> = conn.query_row("SELECT kind FROM entries WHERE repo = ?1 AND hash = ?2 AND path = ?3", params![repo, hash, path], |r| r.get(0)).optional()?;
            if !matches!(kind.as_deref(), Some("dir" | "module")) {
                return Ok(None);
            }
        }
        // Under `p/`: from `p/` up to `p0` ('0' follows '/').
        let (lo, hi) = match path {
            "" => (String::new(), String::from("\u{10FFFF}")),
            p => (format!("{p}/"), format!("{p}0")),
        };
        let mut st = conn.prepare_cached("SELECT path, kind, bytes, target FROM entries WHERE repo = ?1 AND hash = ?2 AND path >= ?3 AND path < ?4 ORDER BY path LIMIT ?5")?;
        let mut entries: Vec<TreeEntry> = st
            .query_map(params![repo, hash, lo, hi, limit as i64 + 1], |r| Ok(TreeEntry { path: r.get(0)?, kind: r.get(1)?, bytes: r.get::<_, i64>(2)? as u64, target: r.get(3)? }))?
            .collect::<Result<_, _>>()?;
        let truncated = entries.len() > limit;
        entries.truncate(limit);
        Ok(Some((entries, truncated)))
    }

    /// The entry at `path`.
    pub fn entry(&self, repo: &str, hash: &str, path: &str) -> Result<Option<EntryRow>, StoreError> {
        let conn = self.conn();
        let row = conn
            .query_row("SELECT kind, bytes, blob, target FROM entries WHERE repo = ?1 AND hash = ?2 AND path = ?3", params![repo, hash, path], |r| {
                Ok(EntryRow { kind: r.get(0)?, bytes: r.get::<_, i64>(1)? as u64, blob: r.get(2)?, target: r.get(3)? })
            })
            .optional()?;
        Ok(row)
    }

    /// A blob's bytes.
    pub fn read_blob(&self, hash: &str) -> Result<Vec<u8>, StoreError> {
        Ok(std::fs::read(blob_at(&self.root, hash))?)
    }

    /// The bytes `staged` would add to the store: its files not kept yet,
    /// each once.
    pub fn new_bytes(&self, staged: &Staged) -> Result<u64, StoreError> {
        let conn = self.conn();
        let mut st = conn.prepare_cached("SELECT 1 FROM blobs WHERE hash = ?1")?;
        let mut seen = std::collections::BTreeSet::new();
        let mut new = 0;
        for e in staged.entries.iter().filter(|e| e.kind == Kind::File) {
            let Some(h) = &e.hash else { continue };
            if seen.insert(h.clone()) && !st.exists([h])? {
                new += e.bytes;
            }
        }
        Ok(new)
    }

    /// Keeps `staged` as `hash` of `repo`, fetched by `by` at `now`, its new
    /// blobs moved from `stage` — the store held under `max` by dropping
    /// the commits read longest ago, or [`StoreError::Full`] with nothing
    /// changed.
    pub fn keep(&self, repo: &str, hash: &str, staged: &Staged, stage: &Path, by: &str, now: i64, max: u64) -> Result<Kept, StoreError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let (files, bytes) = staged.totals();
        let mut out = Kept { files, bytes, ..Kept::default() };
        let mut moved = Vec::new();
        {
            let mut has = tx.prepare_cached("SELECT 1 FROM blobs WHERE hash = ?1")?;
            let mut add = tx.prepare_cached("INSERT INTO blobs (hash, bytes, charged_to, at) VALUES (?1, ?2, ?3, ?4)")?;
            for e in staged.entries.iter().filter(|e| e.kind == Kind::File) {
                let Some(h) = &e.hash else { continue };
                if !has.exists([h])? {
                    add.execute(params![h, e.bytes as i64, by, now])?;
                    out.new_bytes += e.bytes;
                    moved.push(h.clone());
                }
            }
            tx.execute(
                "INSERT OR REPLACE INTO commits (repo, hash, committed_unix, files, bytes, fetched_by, fetched_unix, read_unix) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![repo, hash, staged.committed_unix, files as i64, bytes as i64, by, now],
            )?;
            let mut row = tx.prepare_cached("INSERT OR REPLACE INTO entries (repo, hash, path, kind, bytes, blob, target) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?;
            for Entry { path, kind, bytes, hash: blob, target } in &staged.entries {
                row.execute(params![repo, hash, path, kind.as_str(), *bytes as i64, blob, target])?;
            }
        }
        let mut total: u64 = tx.query_row("SELECT COALESCE(SUM(bytes), 0) FROM blobs", [], |r| r.get::<_, i64>(0))? as u64;
        let mut gone = Vec::new();
        while total > max {
            let victim: Option<(String, String)> = tx
                .query_row(
                    "SELECT repo, hash FROM commits WHERE read_unix <= ?1 AND NOT (repo = ?2 AND hash = ?3) ORDER BY read_unix, fetched_unix LIMIT 1",
                    params![now - RECENT_SECS, repo, hash],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((vr, vh)) = victim else {
                return Err(StoreError::Full { needed: total, max });
            };
            for (blob, size, whose) in drop_commit(&tx, &vr, &vh)? {
                total = total.saturating_sub(size);
                if let Some(w) = whose {
                    *out.credits.entry(w).or_default() += size;
                }
                gone.push(blob);
            }
            out.dropped.push((vr, vh));
        }
        place(&self.root, stage, &moved)?;
        tx.commit()?;
        for blob in gone {
            let _ = std::fs::remove_file(blob_at(&self.root, &blob));
        }
        Ok(out)
    }

    /// Drops `hash` of `repo`: its rows, and its blobs no other kept commit
    /// names — the bytes freed, by whom they were reserved for. `None`: not kept.
    pub fn forget(&self, repo: &str, hash: &str) -> Result<Option<(u64, BTreeMap<String, u64>)>, StoreError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let there = tx.query_row("SELECT 1 FROM commits WHERE repo = ?1 AND hash = ?2", params![repo, hash], |_| Ok(())).optional()?;
        if there.is_none() {
            return Ok(None);
        }
        let (mut freed, mut credits, mut gone) = (0, BTreeMap::new(), Vec::new());
        for (blob, size, whose) in drop_commit(&tx, repo, hash)? {
            freed += size;
            if let Some(w) = whose {
                *credits.entry(w).or_default() += size;
            }
            gone.push(blob);
        }
        tx.commit()?;
        for blob in gone {
            let _ = std::fs::remove_file(blob_at(&self.root, &blob));
        }
        Ok(Some((freed, credits)))
    }

    /// The blobs on disk (a test's look).
    pub fn blobs_on_disk(&self) -> usize {
        let Ok(dirs) = std::fs::read_dir(&self.root) else { return 0 };
        dirs.flatten().filter(|d| d.file_name() != "tmp").filter_map(|d| std::fs::read_dir(d.path()).ok()).map(|f| f.count()).sum()
    }
}

/// The staged blobs `moved` put in place under `root`; on a failure, the
/// ones placed taken back.
fn place(root: &Path, stage: &Path, moved: &[String]) -> Result<(), StoreError> {
    let mut placed: Vec<PathBuf> = Vec::new();
    for h in moved {
        let to = blob_at(root, h);
        if to.exists() {
            continue;
        }
        if let Err(e) = to.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::rename(stage.join(h), &to)) {
            for p in &placed {
                let _ = std::fs::remove_file(p);
            }
            return Err(StoreError::Db(format!("{}: {e}", to.display())));
        }
        placed.push(to);
    }
    Ok(())
}

/// Deletes a commit's rows within `tx`: the blobs no kept commit names any
/// more, each (hash, size, whom it was reserved for), gone from `blobs` too.
fn drop_commit(tx: &rusqlite::Transaction<'_>, repo: &str, hash: &str) -> Result<Vec<(String, u64, Option<String>)>, StoreError> {
    let blobs: Vec<String> = tx
        .prepare_cached("SELECT DISTINCT blob FROM entries WHERE repo = ?1 AND hash = ?2 AND blob IS NOT NULL")?
        .query_map(params![repo, hash], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    tx.execute("DELETE FROM entries WHERE repo = ?1 AND hash = ?2", params![repo, hash])?;
    tx.execute("DELETE FROM commits WHERE repo = ?1 AND hash = ?2", params![repo, hash])?;
    let mut out = Vec::new();
    for b in blobs {
        if tx.prepare_cached("SELECT 1 FROM entries WHERE blob = ?1")?.exists([&b])? {
            continue;
        }
        let row: Option<(i64, Option<String>)> = tx.query_row("SELECT bytes, charged_to FROM blobs WHERE hash = ?1", [&b], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        tx.execute("DELETE FROM blobs WHERE hash = ?1", [&b])?;
        if let Some((size, whose)) = row {
            out.push((b, size as u64, whose));
        }
    }
    Ok(out)
}

fn commit_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CommitRow> {
    Ok(CommitRow {
        repo: r.get(0)?,
        hash: r.get(1)?,
        committed_unix: r.get(2)?,
        files: r.get::<_, i64>(3)? as u64,
        bytes: r.get::<_, i64>(4)? as u64,
        fetched_by: r.get(5)?,
        fetched_unix: r.get(6)?,
        read_unix: r.get(7)?,
    })
}
