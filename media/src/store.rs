//! **The media store** — the images a body shows, kept content-addressed
//! (their SHA-256) in a SQLite file of the service's own, `media.db`: never
//! the tickets file (V2-1: the file is opened by the tickets service alone).
//!
//! **Stored** in two tables, as v1 kept them (COMMENT-5): `media` — a row
//! per image, its bytes a BLOB — and `media_owners` — a row per ticket (or
//! putter) it is for. A kept image never changes (its name is its
//! content), so no write puts a newer one beside its row; the same bytes
//! put again are no second row (`duplicate`), for a new owner one row more.
//!
//! **Whose it is** is kept here and decided elsewhere: an [`Owner`] is a
//! ticket's id (as the tickets service answered it when the image was put)
//! or the putter's principal; whether a caller sees one is the tickets
//! service's word, asked over the seam by the activation
//! ([`crate::streams`]) — this module reads and writes rows, nothing more.
//!
//! One connection behind a mutex; every read and write off the runtime
//! (`spawn_blocking`): an image is up to 7 MiB.

use crate::image::{self, MediaInfo, Problem};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The tables. `media.hash` is the image's name; `media_owners` says whose:
/// `ticket` its id (`''` for an image with no ticket), `principal` the
/// putter's (`''` for a ticket's), `put_by` who put the row.
pub const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS media (
            hash    TEXT PRIMARY KEY,
            mime    TEXT NOT NULL,
            bytes   INTEGER NOT NULL,
            width   INTEGER NOT NULL,
            height  INTEGER NOT NULL,
            at      INTEGER NOT NULL,
            data    BLOB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS media_owners (
            hash      TEXT NOT NULL,
            ticket    TEXT NOT NULL,
            principal TEXT NOT NULL,
            at        INTEGER NOT NULL,
            put_by    TEXT,
            PRIMARY KEY (hash, ticket, principal)
        );
    ";

/// Whose an image is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Owner {
    /// The ticket it was put for, by its id as the tickets service answered
    /// it then.
    Ticket(String),
    /// Put with no ticket: its putter's principal.
    App(Option<String>),
}

impl Owner {
    /// Its two stored columns.
    fn columns(&self) -> (String, String) {
        match self {
            Owner::Ticket(t) => (t.clone(), String::new()),
            Owner::App(p) => (String::new(), p.clone().unwrap_or_default()),
        }
    }

    fn of(ticket: String, principal: String) -> Self {
        match ticket.is_empty() {
            false => Owner::Ticket(ticket),
            true => Owner::App((!principal.is_empty()).then_some(principal)),
        }
    }
}

/// One kept image: what it is, and whose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub info: MediaInfo,
    pub owners: Vec<Owner>,
}

/// What `put` answers: the image as kept, and whether its bytes were kept
/// already.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Put {
    pub info: MediaInfo,
    pub duplicate: bool,
}

/// Why the store did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// `put` refused the image — too large, not a kept type by its bytes,
    /// not the type declared ([`Problem`]). Nothing was kept.
    Refused(Problem),
    /// No image of that hash.
    NotFound(String),
    /// The file.
    Db(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Refused(p) => write!(f, "{p} (nothing was kept)"),
            StoreError::NotFound(h) => write!(f, "no image {h}"),
            StoreError::Db(e) => write!(f, "the media file: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Db(e.to_string())
    }
}

/// The file, open.
pub struct MediaStore {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl std::fmt::Debug for MediaStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaStore").field("path", &self.path).finish()
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl MediaStore {
    /// The store on `path`, made if missing; a file without the tables
    /// gains them, empty — once; on every later open a no-op.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| StoreError::Db(format!("{}: {e}", dir.display())))?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        let had: bool = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'media_owners'", [], |r| r.get::<_, i64>(0).map(|n| n > 0))?;
        conn.execute_batch(TABLES)?;
        if !had {
            tracing::info!("media:     {} (new: the tables made, empty)", path.display());
        }
        Ok(Self { conn: Mutex::new(conn), path: path.to_path_buf() })
    }

    /// The file it keeps.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Off the runtime, with the connection.
    async fn blocking<T: Send + 'static>(self: &Arc<Self>, work: impl FnOnce(&Connection) -> Result<T, StoreError> + Send + 'static) -> Result<T, StoreError> {
        let store = Arc::clone(self);
        tokio::task::spawn_blocking(move || work(&store.lock())).await.map_err(|e| StoreError::Db(format!("media task failed: {e}")))?
    }

    /// Keeps an image — `data` (base64) checked against `mime` by its own
    /// bytes ([`image::check`]: a kept type, at most [`image::MAX_BYTES`],
    /// the type declared), named by its SHA-256 — as `owner`'s, put by
    /// `put_by`. A refusal is [`StoreError::Refused`], naming why, and
    /// nothing is kept. The same bytes kept already are no second row
    /// (`duplicate`); for a new owner the one row more says it is theirs
    /// too; for the same one nothing is written at all.
    pub async fn put(self: &Arc<Self>, owner: Owner, mime: &str, data: String, put_by: Option<String>) -> Result<Put, StoreError> {
        let declared = mime.to_string();
        // The decode, the check and the hash, off the runtime: up to 7 MiB.
        let (bytes, info) = tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, MediaInfo), Problem> {
            let bytes = image::decode(&data)?;
            let found = image::check(&declared, &bytes)?;
            let info = MediaInfo { hash: image::hash(&bytes), mime: found.mime.to_string(), bytes: bytes.len() as u64, width: found.width, height: found.height };
            Ok((bytes, info))
        })
        .await
        .map_err(|e| StoreError::Db(format!("media task failed: {e}")))?
        .map_err(StoreError::Refused)?;
        let at = now();
        self.blocking(move |conn| {
            let had: Option<i64> = conn.query_row("SELECT 1 FROM media WHERE hash = ?1", [&info.hash], |r| r.get(0)).optional()?;
            let (ticket, principal) = owner.columns();
            let tx = conn.unchecked_transaction()?;
            if had.is_none() {
                tx.execute(
                    "INSERT OR IGNORE INTO media (hash, mime, bytes, width, height, at, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![info.hash, info.mime, info.bytes as i64, info.width, info.height, at, bytes],
                )?;
            }
            tx.execute("INSERT OR IGNORE INTO media_owners (hash, ticket, principal, at, put_by) VALUES (?1, ?2, ?3, ?4, ?5)", params![info.hash, ticket, principal, at, put_by])?;
            tx.commit()?;
            Ok(Put { info, duplicate: had.is_some() })
        })
        .await
    }

    /// What the image named `hash` is, and whose — without its bytes;
    /// [`StoreError::NotFound`] for none.
    pub async fn kept(self: &Arc<Self>, hash: &str) -> Result<Kept, StoreError> {
        let h = hash.to_string();
        self.blocking(move |conn| {
            let info = conn
                .query_row("SELECT hash, mime, bytes, width, height FROM media WHERE hash = ?1", [&h], |r| {
                    Ok(MediaInfo { hash: r.get(0)?, mime: r.get(1)?, bytes: r.get::<_, i64>(2)? as u64, width: r.get(3)?, height: r.get(4)? })
                })
                .optional()?
                .ok_or(StoreError::NotFound(h.clone()))?;
            let mut stmt = conn.prepare("SELECT ticket, principal FROM media_owners WHERE hash = ?1 ORDER BY at, rowid")?;
            let owners = stmt.query_map([&h], |r| Ok(Owner::of(r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
            Ok(Kept { info, owners })
        })
        .await
    }

    /// The image named `hash` and its bytes; [`StoreError::NotFound`] for
    /// none. Whether the caller may see it is decided before this is asked.
    pub async fn bytes(self: &Arc<Self>, hash: &str) -> Result<(MediaInfo, Vec<u8>), StoreError> {
        let h = hash.to_string();
        self.blocking(move |conn| {
            conn.query_row("SELECT hash, mime, bytes, width, height, data FROM media WHERE hash = ?1", [&h], |r| {
                Ok((MediaInfo { hash: r.get(0)?, mime: r.get(1)?, bytes: r.get::<_, i64>(2)? as u64, width: r.get(3)?, height: r.get(4)? }, r.get::<_, Vec<u8>>(5)?))
            })
            .optional()?
            .ok_or(StoreError::NotFound(h))
        })
        .await
    }

    /// How many images the store keeps — for a log line.
    pub fn count(&self) -> usize {
        self.lock().query_row("SELECT count(*) FROM media", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    }
}

#[cfg(test)]
mod tests;
