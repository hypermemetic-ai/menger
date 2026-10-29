//! UI-3: **the ui store** — `ui.db`, this service's own file (beside the
//! tickets file by default, never the same file; opened here alone):
//!
//! - `ui_versions` — one row per bundle, keyed by the sha256 of its bytes:
//!   the same bytes pushed again are the same version;
//! - `ui_pointer` — `name → hash` (`live` is the one `/app/` serves), a
//!   foreign key into `ui_versions`, so it never names a hash not stored;
//! - `ui_preview` — `principal → hash`, a session's override of `live`.
//!
//! A push is one transaction: the row (when new) and the pointer moved, or
//! neither. A rollback moves the pointer and touches no other row.

use rusqlite::Connection;
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
";

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
}

/// A bundle's name: the sha256 of its bytes, hex.
pub fn hash_of(bundle: &[u8]) -> String {
    let _ = bundle;
    todo!("UI-3 phase 2: a bundle's sha256")
}

impl UiStore {
    /// Opens (creating, when missing) the file at `path` and its tables,
    /// foreign keys on.
    pub fn open(path: &Path) -> Result<Self, UiError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { path: path.to_path_buf(), conn: Mutex::new(conn) })
    }

    /// Where the file is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Keeps `v` (once per hash) and moves the live pointer onto it — one
    /// transaction.
    pub fn push(&self, v: NewVersion) -> Result<Pushed, UiError> {
        self.push_then(v, |_| Ok(()))
    }

    /// [`Self::push`], running `after_insert` inside the transaction after
    /// the row is written and before the pointer moves — the tests' way to
    /// abort a push midway.
    pub(crate) fn push_then(
        &self,
        v: NewVersion,
        after_insert: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<()>,
    ) -> Result<Pushed, UiError> {
        let _ = (v, after_insert, &self.conn);
        todo!("UI-3 phase 2: push as one transaction")
    }

    /// Moves the live pointer to `to`, a kept version; no other row changes.
    pub fn rollback(&self, to: &str) -> Result<String, UiError> {
        let _ = to;
        todo!("UI-3 phase 2: rollback")
    }

    /// Every kept version, newest first, each with whether it is live.
    pub fn versions(&self) -> Result<Vec<(UiVersion, bool)>, UiError> {
        todo!("UI-3 phase 2: versions")
    }

    /// The live pointer, when any version is live.
    pub fn pointer(&self) -> Result<Option<Pointer>, UiError> {
        todo!("UI-3 phase 2: the live pointer")
    }

    /// Sets (or, `None`, clears) `principal`'s preview; the version it
    /// names now.
    pub fn set_preview(&self, principal: &str, version: Option<&str>) -> Result<Option<String>, UiError> {
        let _ = (principal, version);
        todo!("UI-3 phase 2: set a preview")
    }

    /// `principal`'s preview, when it holds one.
    pub fn preview_of(&self, principal: &str) -> Result<Option<String>, UiError> {
        let _ = principal;
        todo!("UI-3 phase 2: a principal's preview")
    }

    /// The file at `path` inside version `hash`'s bundle.
    pub fn file(&self, hash: &str, path: &str) -> Result<Option<Vec<u8>>, UiError> {
        let _ = (hash, path);
        todo!("UI-3 phase 2: a version's file")
    }

    /// The entrypoint `/app/` serves to `principal` (none: anonymous) — its
    /// preview's `index.html`, else the live version's — with the version's
    /// hash; `None` when neither names one.
    pub fn entry_for(&self, principal: Option<&str>) -> Result<Option<(String, Vec<u8>)>, UiError> {
        let _ = principal;
        todo!("UI-3 phase 2: the entrypoint")
    }
}

// Named `ui_tests` so `cargo test -p menger-ui-service ui_` selects them
// (the ticket's acceptance 7); the file is `store/tests.rs`.
#[cfg(test)]
#[path = "tests.rs"]
mod ui_tests;
