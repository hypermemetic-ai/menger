//! UI-3: **the ui store** — `ui.db`, this service's own file (beside the
//! tickets file by default, never the same file; opened here alone):
//!
//! - `ui_versions` — one row per bundle, keyed by the sha256 of its bytes:
//!   the same bytes pushed again are the same version;
//! - `ui_pointer` — `name → hash` (`live` is the one `/app/` serves), a
//!   foreign key into `ui_versions`, so it never names a hash not stored;
//! - `ui_preview` — `principal → hash`, a session's override of `live`;
//! - `ui_slots` — UI-9: `(layout, slot) → (component, version_hash)`, what
//!   fills a section in place of the live manifest's default; the hash a
//!   foreign key into `ui_versions` (a stored version is immutable, so an
//!   assignment outlives any pointer move). Its methods are in [`slots`].
//!
//! UI-9: every pointer move and every assignment is said on the store's
//! broadcast ([`UiStore::subscribe`], [`UiChange`]), which `ui.watch` holds.
//!
//! A push is one transaction: the row (when new) and the pointer moved, or
//! neither. A rollback moves the pointer and touches no other row.

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::io::Read;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The file's name in the data directory, beside the tickets file.
pub const UI_DB_FILE: &str = "ui.db";

/// The pointer `/app/` serves.
pub const LIVE: &str = "live";

/// The file's tables. The columns are the ticket's; `pushed_at` with the
/// rowid orders them newest first.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS ui_versions (
  hash TEXT PRIMARY KEY, bundle BLOB NOT NULL, manifest TEXT NOT NULL,
  commit_hash TEXT, contract TEXT NOT NULL, abi TEXT NOT NULL,
  pushed_by TEXT NOT NULL, pushed_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ui_pointer (
  name TEXT PRIMARY KEY, hash TEXT NOT NULL REFERENCES ui_versions(hash)
);
CREATE TABLE IF NOT EXISTS ui_preview (
  principal TEXT PRIMARY KEY, hash TEXT NOT NULL REFERENCES ui_versions(hash)
);
CREATE TABLE IF NOT EXISTS ui_slots (
  layout TEXT NOT NULL, slot TEXT NOT NULL, component TEXT NOT NULL,
  version_hash TEXT NOT NULL REFERENCES ui_versions(hash),
  PRIMARY KEY (layout, slot)
);
";

/// UI-9: how many changes a watch may fall behind by before it is told
/// `lagged`.
pub const CHANGES_CAPACITY: usize = 256;

mod slots;
pub use slots::{Assignment, Section, UiChange};

/// A kept version, as `versions` names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiVersion {
    pub hash: String,
    pub commit: Option<String>,
    pub contract: String,
    pub abi: String,
    pub pushed_by: String,
    pub pushed_at: i64,
}

/// A pointer: a name and the version it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pointer {
    pub name: String,
    pub hash: String,
}

/// What a push keeps: the bundle's bytes, its manifest (its `contract` and
/// `abi` read off it), its commit, who and when.
#[derive(Debug, Clone)]
pub struct NewVersion {
    pub bundle: Vec<u8>,
    pub manifest: String,
    pub commit: Option<String>,
    pub pushed_by: String,
    pub pushed_at: i64,
}

/// What a push answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pushed {
    /// The bundle's sha256, hex.
    pub version: String,
    /// The pointer is on it.
    pub live: bool,
    /// A row was inserted (false: the same bytes were kept already).
    pub new: bool,
}

/// Why the store did not do it.
#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("the ui file: {0}")]
    Db(String),
    #[error("the bundle is not a tar archive with an index.html: {0}")]
    BadBundle(String),
    #[error("the manifest names no {0}")]
    BadManifest(&'static str),
    #[error("no version {0} is kept")]
    UnknownVersion(String),
    /// UI-9: a set naming a component the version's manifest does not hold.
    #[error("version {version} holds no component {component}")]
    UnknownComponent { component: String, version: String },
    /// UI-9: a set naming a slot the live manifest does not declare for the layout.
    #[error("the layout {layout} declares no slot {slot}")]
    UnknownSlot { layout: String, slot: String },
}

impl From<rusqlite::Error> for UiError {
    fn from(e: rusqlite::Error) -> Self {
        UiError::Db(e.to_string())
    }
}

/// The store over `ui.db`. The only handle on the file in the process.
pub struct UiStore {
    path: PathBuf,
    conn: Mutex<Connection>,
    /// UI-9: every pointer move and assignment, for `ui.watch`.
    changes: tokio::sync::broadcast::Sender<UiChange>,
}

/// A bundle's name: the sha256 of its bytes, hex.
pub fn hash_of(bundle: &[u8]) -> String {
    digest_of(bundle).iter().map(|b| format!("{b:02x}")).collect()
}

/// A bundle's sha256 — what its signature is over.
pub fn digest_of(bundle: &[u8]) -> [u8; 32] {
    Sha256::digest(bundle).into()
}

/// A manifest's `key: value` line, as text (quotes around the value read
/// past); not checked here — that is the compiler's and activation's.
pub fn manifest_field(manifest: &str, key: &str) -> Option<String> {
    manifest.lines().find_map(|line| {
        let (k, v) = line.trim().split_once(':')?;
        let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
        (k.trim() == key && !v.is_empty()).then(|| v.to_string())
    })
}

/// The file `path` in a tar `bundle` (`./` before a name read past); `Err`
/// when the bytes are no tar.
fn file_in(bundle: &[u8], path: &str) -> Result<Option<Vec<u8>>, String> {
    let want = path.trim_start_matches('/');
    let mut archive = tar::Archive::new(bundle);
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry.path().map_err(|e| e.to_string())?.to_string_lossy().trim_start_matches("./").to_string();
        if name == want {
            let mut out = Vec::new();
            entry.read_to_end(&mut out).map_err(|e| e.to_string())?;
            return Ok(Some(out));
        }
    }
    Ok(None)
}

/// A bundle is a tar holding `index.html`.
pub fn check_bundle(bundle: &[u8]) -> Result<(), UiError> {
    match file_in(bundle, "index.html") {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(UiError::BadBundle("no index.html".into())),
        Err(e) => Err(UiError::BadBundle(e)),
    }
}

/// Whether `hash` is a kept version.
fn kept(c: &Connection, hash: &str) -> rusqlite::Result<bool> {
    c.query_row("SELECT 1 FROM ui_versions WHERE hash = ?1", [hash], |_| Ok(())).optional().map(|r| r.is_some())
}

/// Points `name` at `hash`.
fn point(c: &Connection, name: &str, hash: &str) -> rusqlite::Result<()> {
    c.execute("INSERT INTO ui_pointer (name, hash) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET hash = excluded.hash", [name, hash])
        .map(drop)
}

impl UiStore {
    /// Opens (creating, when missing) the file at `path` and its tables,
    /// foreign keys on.
    pub fn open(path: &Path) -> Result<Self, UiError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.execute_batch(SCHEMA)?;
        let (changes, _) = tokio::sync::broadcast::channel(CHANGES_CAPACITY);
        Ok(Self { path: path.to_path_buf(), conn: Mutex::new(conn), changes })
    }

    /// UI-9: the store's changes from now on — a `live` on every pointer
    /// move, a `slot_changed` on every set and clear.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<UiChange> {
        self.changes.subscribe()
    }

    /// Where the file is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Keeps `v` (once per hash) and moves the live pointer onto it — one
    /// transaction.
    pub fn push(&self, v: NewVersion) -> Result<Pushed, UiError> {
        self.push_with(v, |_| Ok(()))
    }

    /// [`Self::push`], running `after_insert` inside the transaction after
    /// the row is written and before the pointer moves — the tests' way to
    /// abort a push midway.
    #[cfg(test)]
    pub(crate) fn push_then(
        &self,
        v: NewVersion,
        after_insert: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<()>,
    ) -> Result<Pushed, UiError> {
        self.push_with(v, after_insert)
    }

    /// The one push both [`Self::push`] and the tests' seam run.
    fn push_with(
        &self,
        v: NewVersion,
        after_insert: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<()>,
    ) -> Result<Pushed, UiError> {
        let hash = hash_of(&v.bundle);
        check_bundle(&v.bundle)?;
        let contract = manifest_field(&v.manifest, "contract").ok_or(UiError::BadManifest("contract"))?;
        let abi = manifest_field(&v.manifest, "abi").ok_or(UiError::BadManifest("abi"))?;
        let mut c = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let tx = c.transaction()?;
        let new = !kept(&tx, &hash)?;
        if new {
            tx.execute(
                "INSERT INTO ui_versions (hash, bundle, manifest, commit_hash, contract, abi, pushed_by, pushed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![hash, v.bundle, v.manifest, v.commit, contract, abi, v.pushed_by, v.pushed_at],
            )?;
        }
        after_insert(&tx)?;
        point(&tx, LIVE, &hash)?;
        tx.commit()?;
        Ok(Pushed { version: hash, live: true, new })
    }

    /// Moves the live pointer to `to`, a kept version; no other row changes.
    pub fn rollback(&self, to: &str) -> Result<String, UiError> {
        let mut c = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let tx = c.transaction()?;
        if !kept(&tx, to)? {
            return Err(UiError::UnknownVersion(to.to_string()));
        }
        point(&tx, LIVE, to)?;
        tx.commit()?;
        Ok(to.to_string())
    }

    /// Every kept version, newest first, each with whether it is live.
    pub fn versions(&self) -> Result<Vec<(UiVersion, bool)>, UiError> {
        let c = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut q = c.prepare(
            "SELECT v.hash, v.commit_hash, v.contract, v.abi, v.pushed_by, v.pushed_at, p.hash IS NOT NULL \
             FROM ui_versions v LEFT JOIN ui_pointer p ON p.name = ?1 AND p.hash = v.hash \
             ORDER BY v.pushed_at DESC, v.rowid DESC",
        )?;
        let rows = q.query_map([LIVE], |r| {
            Ok((
                UiVersion { hash: r.get(0)?, commit: r.get(1)?, contract: r.get(2)?, abi: r.get(3)?, pushed_by: r.get(4)?, pushed_at: r.get(5)? },
                r.get::<_, bool>(6)?,
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The live pointer, when any version is live.
    pub fn pointer(&self) -> Result<Option<Pointer>, UiError> {
        let c = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let hash: Option<String> = c.query_row("SELECT hash FROM ui_pointer WHERE name = ?1", [LIVE], |r| r.get(0)).optional()?;
        Ok(hash.map(|hash| Pointer { name: LIVE.into(), hash }))
    }

    /// Sets (or, `None`, clears) `principal`'s preview; the version it
    /// names now.
    pub fn set_preview(&self, principal: &str, version: Option<&str>) -> Result<Option<String>, UiError> {
        let mut c = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let tx = c.transaction()?;
        match version {
            None => {
                tx.execute("DELETE FROM ui_preview WHERE principal = ?1", [principal])?;
            }
            Some(h) if !kept(&tx, h)? => return Err(UiError::UnknownVersion(h.to_string())),
            Some(h) => {
                tx.execute(
                    "INSERT INTO ui_preview (principal, hash) VALUES (?1, ?2) ON CONFLICT(principal) DO UPDATE SET hash = excluded.hash",
                    [principal, h],
                )?;
            }
        }
        tx.commit()?;
        Ok(version.map(str::to_string))
    }

    /// `principal`'s preview, when it holds one.
    pub fn preview_of(&self, principal: &str) -> Result<Option<String>, UiError> {
        let c = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        Ok(c.query_row("SELECT hash FROM ui_preview WHERE principal = ?1", [principal], |r| r.get(0)).optional()?)
    }

    /// The file at `path` inside version `hash`'s bundle.
    pub fn file(&self, hash: &str, path: &str) -> Result<Option<Vec<u8>>, UiError> {
        let bundle: Option<Vec<u8>> = {
            let c = self.conn.lock().unwrap_or_else(|p| p.into_inner());
            c.query_row("SELECT bundle FROM ui_versions WHERE hash = ?1", [hash], |r| r.get(0)).optional()?
        };
        match bundle {
            None => Ok(None),
            Some(b) => file_in(&b, path).map_err(UiError::BadBundle),
        }
    }

    /// The entrypoint `/app/` serves to `principal` (none: anonymous) — its
    /// preview's `index.html`, else the live version's — with the version's
    /// hash; `None` when neither names one.
    pub fn entry_for(&self, principal: Option<&str>) -> Result<Option<(String, Vec<u8>)>, UiError> {
        let preview = match principal {
            Some(p) => self.preview_of(p)?,
            None => None,
        };
        let Some(hash) = preview.or(self.pointer()?.map(|p| p.hash)) else { return Ok(None) };
        Ok(self.file(&hash, "index.html")?.map(|html| (hash, html)))
    }
}

// Named `ui_tests` so `cargo test -p menger-ui-service ui_` selects them
// (the ticket's acceptance 7); the file is `store/tests.rs`.
#[cfg(test)]
#[path = "tests.rs"]
mod ui_tests;

// UI-9: the sections' store tests (`cargo test -p menger-ui-service slots_`).
#[cfg(test)]
#[path = "slots_tests.rs"]
mod slots_tests;
