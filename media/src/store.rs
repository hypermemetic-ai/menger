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
//! (`spawn_blocking`).
//!
//! **EDITOR-10: any file, of any size, its bytes in a [`BlobStore`].** A
//! row says what a file is — its type, `kind`, `name`, size, owners — and
//! the bytes live behind the blob store (`blob = 1`, its `data` empty):
//! the disk today ([`DiskStore`], `media/blobs/<sha256>`), an object store
//! when one is built. A file uploaded in chunks — tens of GB if the
//! uploader's allowance holds them — streams into the blob store's upload,
//! hashed as it comes, and `finish` commits it under its hash. An upload
//! resumes from the bytes it has after a dropped connection
//! ([`MediaStore::resume`]); one untouched for a day is dropped
//! ([`MediaStore::sweep`]). A row from before EDITOR-10 (`blob = 0`) still
//! holds its image's bytes in `data`, read from there. `unfurls` keeps what
//! a link unfurled to, for a day.

use crate::image::{self, MediaInfo, Problem};
use crate::kinds;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use crate::blob::{BlobError, BlobStore, DiskStore};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The tables. `media.hash` is the file's name, `blob` whether its bytes
/// are in the [`BlobStore`] rather than `data` (a row from before); `media_owners` says
/// whose: `ticket` its id (`''` for a file with no ticket), `principal` the
/// putter's (`''` for a ticket's; `*` for anyone signed in — a link's
/// preview), `put_by` who put the row (whose allowance it counts against).
/// `uploads` holds a chunked upload until it is finished, its bytes in the
/// blob store's upload of the same id; `unfurls` a link's card, by its URL.
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
        CREATE TABLE IF NOT EXISTS uploads (
            id          TEXT PRIMARY KEY,
            ticket      TEXT NOT NULL,
            principal   TEXT NOT NULL,
            put_by      TEXT NOT NULL,
            name        TEXT,
            size        INTEGER NOT NULL,
            received    INTEGER NOT NULL DEFAULT 0,
            at          INTEGER NOT NULL,
            reservation TEXT,
            finishing   INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS unfurls (
            url     TEXT PRIMARY KEY,
            at      INTEGER NOT NULL,
            answer  TEXT NOT NULL
        );
    ";

/// EDITOR-10: the columns a file kept before any file was gains.
const MEDIA_COLUMNS: [(&str, &str); 3] = [("kind", "TEXT NOT NULL DEFAULT 'image'"), ("name", "TEXT"), ("blob", "INTEGER NOT NULL DEFAULT 0")];

/// An upload untouched for a day is dropped ([`MediaStore::sweep`]).
pub const UPLOAD_TTL: i64 = 24 * 3600;

/// The principal a file anyone signed in may read is kept under — a
/// link's preview image, fetched from a public page.
const ANYONE: &str = "*";

/// Whose a file is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Owner {
    /// The ticket it was put for, by its id as the tickets service answered
    /// it then.
    Ticket(String),
    /// Put with no ticket: its putter's principal.
    App(Option<String>),
    /// EDITOR-10: anyone signed in — a public page's preview image.
    Anyone,
    /// V2-29: the document it was put for, by its id — kept in the
    /// `ticket` column as `doc:<id>` (a ticket's id never begins so).
    Doc(String),
}

/// V2-29: how a document's id is kept in `media_owners.ticket`.
pub const DOC_PREFIX: &str = "doc:";

impl Owner {
    /// Its two stored columns.
    fn columns(&self) -> (String, String) {
        match self {
            Owner::Ticket(t) => (t.clone(), String::new()),
            Owner::Doc(d) => (format!("{DOC_PREFIX}{d}"), String::new()),
            Owner::App(p) => (String::new(), p.clone().unwrap_or_default()),
            Owner::Anyone => (String::new(), ANYONE.into()),
        }
    }

    fn of(ticket: String, principal: String) -> Self {
        if let Some(doc) = ticket.strip_prefix(DOC_PREFIX) {
            return Owner::Doc(doc.to_string());
        }
        match (ticket.is_empty(), principal.as_str()) {
            (false, _) => Owner::Ticket(ticket),
            (true, ANYONE) => Owner::Anyone,
            (true, _) => Owner::App((!principal.is_empty()).then_some(principal)),
        }
    }
}

/// One kept file: what it is, and whose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub info: MediaInfo,
    pub owners: Vec<Owner>,
}

/// What `put` answers: the file as kept, whether its bytes were kept
/// already, and (V2-69) whether the putter keeps it now and did not before
/// — what their allowance is charged for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Put {
    pub info: MediaInfo,
    pub duplicate: bool,
    /// V2-69: the putter's for the first time — charged its bytes, a
    /// duplicate of a file someone else kept too. A file they kept already
    /// (for another ticket, or again) is not charged twice.
    pub charged: bool,
}

/// An upload begun or resumed: its id, its size, and the bytes it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Begun {
    pub upload: String,
    pub size: u64,
    pub received: u64,
    /// Begun now (`false`: resumed).
    pub new: bool,
}

/// What `chunk` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunked {
    pub received: u64,
    pub size: u64,
    pub duplicate: bool,
}

/// An upload dropped by [`MediaStore::sweep`]: whose, and its reservation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub upload: String,
    pub reservation: Option<String>,
}

/// What a finished upload was: the file as kept, and the reservation its
/// bytes were held under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub put: Put,
    pub reservation: Option<String>,
}

/// Why the store did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Refused — too large, a truncated image, a chunk out of order
    /// ([`Problem`]). Nothing was kept.
    Refused(Problem),
    /// No file of that hash (or no upload of that id, or another's).
    NotFound(String),
    /// The file.
    Db(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Refused(p) => write!(f, "{p} (nothing was kept)"),
            StoreError::NotFound(h) => write!(f, "no file {h}"),
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

impl From<BlobError> for StoreError {
    fn from(e: BlobError) -> Self {
        match e {
            BlobError::NotFound(k) => StoreError::NotFound(k),
            e => StoreError::Db(e.to_string()),
        }
    }
}

/// An upload's running hash: the SHA-256 of its first `upto` bytes.
type Hashing = (sha2::Sha256, u64);

/// An upload's row as `finish` reads it: ticket, principal, name, size,
/// received, reservation.
type UploadRow = (String, String, Option<String>, i64, i64, Option<String>);

/// The file, open, and where its bytes are.
pub struct MediaStore {
    conn: Mutex<Connection>,
    path: PathBuf,
    blobs: Arc<dyn BlobStore>,
    /// Each upload's hash so far, as its chunks came (lost on a restart:
    /// `finish` then reads the sealed upload back and hashes it whole).
    hashing: Mutex<HashMap<String, Hashing>>,
    /// One chunk of an upload at a time.
    writing: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl std::fmt::Debug for MediaStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaStore").field("path", &self.path).field("blobs", &self.blobs).finish()
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

const INFO_COLUMNS: &str = "hash, mime, bytes, width, height, kind, name";

fn info_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<MediaInfo> {
    Ok(MediaInfo { hash: r.get(0)?, mime: r.get(1)?, bytes: r.get::<_, i64>(2)? as u64, width: r.get(3)?, height: r.get(4)?, kind: r.get(5)?, name: r.get(6)? })
}

/// How much of a sealed upload is hashed at a time.
const STEP: usize = 4 * 1024 * 1024;

/// The blob store's root when none is named: `media/blobs` beside the
/// media file `path`.
pub fn default_blob_dir(path: &Path) -> PathBuf {
    path.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")).join("media").join("blobs")
}

impl MediaStore {
    /// The store on `path` with its bytes on the disk beside it
    /// ([`default_blob_dir`]).
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let disk = DiskStore::open(&default_blob_dir(path))?;
        Self::open_with(path, Arc::new(disk))
    }

    /// The store on `path`, made if missing, its bytes in `blobs`; a file
    /// without the tables gains them, empty — once; on every later open a
    /// no-op. A file from before EDITOR-10 gains `kind`, `name` and `blob`
    /// (its rows images, unnamed, their bytes in the row).
    pub fn open_with(path: &Path, blobs: Arc<dyn BlobStore>) -> Result<Self, StoreError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| StoreError::Db(format!("{}: {e}", dir.display())))?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        let had: bool = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'media_owners'", [], |r| r.get::<_, i64>(0).map(|n| n > 0))?;
        conn.execute_batch(TABLES)?;
        let columns: Vec<String> = conn.prepare("SELECT name FROM pragma_table_info('media')")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        for (column, decl) in MEDIA_COLUMNS {
            if !columns.iter().any(|c| c == column) {
                conn.execute_batch(&format!("ALTER TABLE media ADD COLUMN {column} {decl}"))?;
            }
        }
        // A finish the process died in the middle of is unfinished again.
        conn.execute("UPDATE uploads SET finishing = 0", [])?;
        if !had {
            tracing::info!("media:     {} (new: the tables made, empty)", path.display());
        }
        Ok(Self { conn: Mutex::new(conn), path: path.to_path_buf(), blobs, hashing: Mutex::new(HashMap::new()), writing: Mutex::new(HashMap::new()) })
    }

    /// The file it keeps.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where the bytes are.
    pub fn blobs(&self) -> &Arc<dyn BlobStore> {
        &self.blobs
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn hashing(&self) -> std::sync::MutexGuard<'_, HashMap<String, Hashing>> {
        self.hashing.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn writer(&self, upload: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.writing.lock().unwrap_or_else(|p| p.into_inner()).entry(upload.to_string()).or_default().clone()
    }

    fn done_writing(&self, upload: &str) {
        self.writing.lock().unwrap_or_else(|p| p.into_inner()).remove(upload);
        self.hashing().remove(upload);
    }

    /// Off the runtime, with the connection.
    async fn blocking<T: Send + 'static>(self: &Arc<Self>, work: impl FnOnce(&Connection) -> Result<T, StoreError> + Send + 'static) -> Result<T, StoreError> {
        let store = Arc::clone(self);
        tokio::task::spawn_blocking(move || work(&store.lock())).await.map_err(|e| StoreError::Db(format!("media task failed: {e}")))?
    }

    /// Keeps a file — `data` in base64 — as [`Self::put_bytes`] does, at
    /// most [`kinds::PUT_MAX`]. A refusal is [`StoreError::Refused`],
    /// naming why, and nothing is kept.
    pub async fn put(self: &Arc<Self>, owner: Owner, data: String, name: Option<String>, put_by: Option<String>) -> Result<Put, StoreError> {
        let bytes = tokio::task::spawn_blocking(move || image::decode(&data)).await.map_err(|e| StoreError::Db(format!("media task failed: {e}")))?.map_err(StoreError::Refused)?;
        self.put_bytes(owner, bytes, name, put_by, kinds::PUT_MAX).await
    }

    /// Keeps `bytes` (at most `max`) as `owner`'s, put by `put_by`: named
    /// by their SHA-256, their type read from their own magic bytes
    /// ([`kinds::sniff`]) — any file is kept, a truncated image is refused
    /// — the bytes in the blob store, the row here. The same bytes kept
    /// already are no second row (`duplicate`); for a new owner the one
    /// row more says it is theirs too.
    pub async fn put_bytes(self: &Arc<Self>, owner: Owner, bytes: Vec<u8>, name: Option<String>, put_by: Option<String>, max: usize) -> Result<Put, StoreError> {
        if bytes.is_empty() {
            return Err(StoreError::Refused(Problem::Empty));
        }
        if bytes.len() > max {
            return Err(StoreError::Refused(Problem::TooLarge { bytes: bytes.len() as u64, max: max as u64 }));
        }
        let name = name.as_deref().and_then(kinds::clean_name);
        let (bytes, info) = tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, MediaInfo), Problem> {
            let found = kinds::sniff(&bytes)?;
            let info = MediaInfo { hash: image::hash(&bytes), mime: found.mime.to_string(), bytes: bytes.len() as u64, width: found.width, height: found.height, kind: found.kind.as_str().into(), name };
            Ok((bytes, info))
        })
        .await
        .map_err(|e| StoreError::Db(format!("media task failed: {e}")))?
        .map_err(StoreError::Refused)?;
        let h = info.hash.clone();
        let had = self.blocking(move |conn| Ok(conn.query_row("SELECT 1 FROM media WHERE hash = ?1", [&h], |r| r.get::<_, i64>(0)).optional()?.is_some())).await?;
        if !had {
            // The bytes first: a row never names bytes that are not there.
            self.blobs.put(&info.hash, bytes).await?;
        }
        self.row(info, owner, put_by).await
    }

    /// The row for `info`, kept in the blob store, and `owner`'s.
    async fn row(self: &Arc<Self>, info: MediaInfo, owner: Owner, put_by: Option<String>) -> Result<Put, StoreError> {
        let at = now();
        self.blocking(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let had: Option<i64> = tx.query_row("SELECT 1 FROM media WHERE hash = ?1", [&info.hash], |r| r.get(0)).optional()?;
            if had.is_none() {
                tx.execute(
                    "INSERT INTO media (hash, mime, bytes, width, height, at, data, kind, name, blob) VALUES (?1, ?2, ?3, ?4, ?5, ?6, x'', ?7, ?8, 1)",
                    params![info.hash, info.mime, info.bytes as i64, info.width, info.height, at, info.kind, info.name],
                )?;
            }
            let before = keeps(&tx, &info.hash, put_by.as_deref())?;
            let info = own(&tx, info, &owner, at, put_by.as_deref())?;
            let charged = !before && keeps(&tx, &info.hash, put_by.as_deref())?;
            tx.commit()?;
            Ok(Put { info, duplicate: had.is_some(), charged })
        })
        .await
    }

    /// EDITOR-10: starts a chunked upload of `size` bytes for `owner`, by
    /// `put_by`, its bytes held under `reservation` (the allowance's). `size`
    /// is checked against `max` only when the server names one.
    pub async fn begin(self: &Arc<Self>, owner: Owner, name: Option<String>, size: u64, max: Option<u64>, put_by: String, reservation: Option<String>) -> Result<Begun, StoreError> {
        if size == 0 {
            return Err(StoreError::Refused(Problem::Empty));
        }
        if let Some(max) = max.filter(|m| size > *m) {
            return Err(StoreError::Refused(Problem::TooLarge { bytes: size, max }));
        }
        let name = name.as_deref().and_then(kinds::clean_name);
        let id = uuid::Uuid::new_v4().simple().to_string();
        self.blobs.begin_upload(&id).await?;
        let upload = id.clone();
        let made = self
            .blocking(move |conn| {
                let (ticket, principal) = owner.columns();
                conn.execute(
                    "INSERT INTO uploads (id, ticket, principal, put_by, name, size, at, reservation) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![upload, ticket, principal, put_by, name, size as i64, now(), reservation],
                )?;
                Ok(())
            })
            .await;
        if let Err(e) = made {
            let _ = self.blobs.abort(&id).await;
            return Err(e);
        }
        self.hashing().insert(id.clone(), (<sha2::Sha256 as sha2::Digest>::new(), 0));
        Ok(Begun { upload: id, size, received: 0, new: true })
    }

    /// EDITOR-10: the caller's upload `upload` again, after a dropped
    /// connection: its size and the bytes it has, from which the next chunk
    /// goes. Another's, or none, is [`StoreError::NotFound`].
    pub async fn resume(self: &Arc<Self>, upload: &str, by: &str) -> Result<Begun, StoreError> {
        let (upload, by) = (upload.to_string(), by.to_string());
        self.blocking(move |conn| {
            let row: Option<(i64, i64)> = conn.query_row("SELECT size, received FROM uploads WHERE id = ?1 AND put_by = ?2", params![upload, by], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
            let Some((size, received)) = row else { return Err(StoreError::NotFound(upload)) };
            conn.execute("UPDATE uploads SET at = ?2 WHERE id = ?1", params![upload, now()])?;
            Ok(Begun { upload, size: size as u64, received: received as u64, new: false })
        })
        .await
    }

    /// EDITOR-10: a chunk of `upload` (whose putter is `by`) starting at
    /// byte `offset`: the next one is appended and hashed; one sent again
    /// (wholly before what the upload has) is `duplicate` and written no
    /// second time; one that leaves a gap is refused naming the offset
    /// expected. Another's upload, or none, is [`StoreError::NotFound`].
    pub async fn chunk(self: &Arc<Self>, upload: &str, by: &str, offset: u64, bytes: Vec<u8>) -> Result<Chunked, StoreError> {
        if bytes.is_empty() {
            return Err(StoreError::Refused(Problem::Empty));
        }
        let writer = self.writer(upload);
        let _one = writer.lock().await;
        let (id, who) = (upload.to_string(), by.to_string());
        let row = self
            .blocking(move |conn| Ok(conn.query_row("SELECT size, received, finishing FROM uploads WHERE id = ?1 AND put_by = ?2", params![id, who], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))).optional()?))
            .await?;
        let Some((size, received, finishing)) = row else { return Err(StoreError::NotFound(upload.to_string())) };
        let (size, received) = (size as u64, received as u64);
        if offset + bytes.len() as u64 <= received {
            return Ok(Chunked { received, size, duplicate: true });
        }
        if offset != received || finishing != 0 {
            return Err(StoreError::Refused(Problem::OutOfOrder { expected: received, got: offset }));
        }
        if received + bytes.len() as u64 > size {
            return Err(StoreError::Refused(Problem::PastSize { size }));
        }
        let len = bytes.len() as u64;
        let hashed = {
            let mut hashing = self.hashing();
            match hashing.get_mut(upload) {
                Some((h, upto)) if *upto == received => Some((h.clone(), *upto)),
                _ => None,
            }
        };
        let hashed = hashed.map(|(mut h, upto)| {
            sha2::Digest::update(&mut h, &bytes);
            (h, upto + len)
        });
        let now_has = self.blobs.append(upload, received, bytes).await?;
        if let Some(h) = hashed {
            self.hashing().insert(upload.to_string(), h);
        }
        let id = upload.to_string();
        self.blocking(move |conn| Ok(conn.execute("UPDATE uploads SET received = ?1, at = ?2 WHERE id = ?3", params![now_has as i64, now(), id])?)).await?;
        Ok(Chunked { received: now_has, size, duplicate: false })
    }

    /// EDITOR-10: `upload` finished: every byte came, so it is sealed, its
    /// hash the running one (or the sealed upload's, read back after a
    /// restart), its kind read from its first bytes, and it is committed
    /// under that hash — dropped, when those bytes are kept already. The
    /// row is written and the upload gone.
    pub async fn finish(self: &Arc<Self>, upload: &str, by: &str) -> Result<Finished, StoreError> {
        let (id, who) = (upload.to_string(), by.to_string());
        let writer = self.writer(upload);
        let _one = writer.lock().await;
        let row = self
            .blocking(move |conn| {
                let row: Option<UploadRow> = conn
                    .query_row("SELECT ticket, principal, name, size, received, reservation FROM uploads WHERE id = ?1 AND put_by = ?2 AND finishing = 0", params![id, who], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
                    })
                    .optional()?;
                let Some(row) = row else { return Err(StoreError::NotFound(id)) };
                if row.4 < row.3 {
                    return Err(StoreError::Refused(Problem::Incomplete { received: row.4 as u64, size: row.3 as u64 }));
                }
                conn.execute("UPDATE uploads SET finishing = 1 WHERE id = ?1", [&id])?;
                Ok(row)
            })
            .await?;
        let (ticket, principal, name, size, _, reservation) = row;
        let size = size as u64;
        let made = self.seal_and_commit(upload, size).await;
        let id = upload.to_string();
        let found = match made {
            Ok(f) => f,
            Err(e) => {
                let _ = self.blobs.abort(upload).await;
                self.done_writing(upload);
                let _ = self.blocking(move |conn| Ok(conn.execute("DELETE FROM uploads WHERE id = ?1", [&id])?)).await;
                return Err(e);
            }
        };
        self.done_writing(upload);
        let (hash, found) = found;
        let info = MediaInfo { hash, mime: found.mime.into(), bytes: size, width: found.width, height: found.height, kind: found.kind.as_str().into(), name };
        let put = self.row(info, Owner::of(ticket, principal), Some(by.to_string())).await?;
        self.blocking(move |conn| Ok(conn.execute("DELETE FROM uploads WHERE id = ?1", [&id])?)).await?;
        Ok(Finished { put, reservation })
    }

    async fn seal_and_commit(self: &Arc<Self>, upload: &str, size: u64) -> Result<(String, kinds::Sniffed), StoreError> {
        use sha2::Digest;
        self.blobs.seal(upload).await?;
        let head = self.blobs.read_sealed(upload, 0, 64 * 1024).await?;
        let found = kinds::sniff(&head).map_err(StoreError::Refused)?;
        let running = self.hashing().remove(upload);
        let digest = match running {
            Some((h, upto)) if upto == size => h.finalize(),
            _ => {
                let mut h = sha2::Sha256::new();
                let mut at = 0u64;
                while at < size {
                    let piece = self.blobs.read_sealed(upload, at, STEP).await?;
                    if piece.is_empty() {
                        break;
                    }
                    at += piece.len() as u64;
                    h.update(&piece);
                }
                h.finalize()
            }
        };
        let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        self.blobs.commit(upload, &hash).await?;
        Ok((hash, found))
    }

    /// EDITOR-10: drops every upload untouched for `older_than` seconds —
    /// its row and its bytes — and any upload the blob store holds that no
    /// row names: what was dropped, so the reservations they held are
    /// released.
    pub async fn sweep(self: &Arc<Self>, older_than: i64) -> Result<Vec<Dropped>, StoreError> {
        let (gone, live) = self
            .blocking(move |conn| {
                let mut stmt = conn.prepare("SELECT id, reservation FROM uploads WHERE at < ?1 AND finishing = 0")?;
                let gone = stmt.query_map([now() - older_than], |r| Ok(Dropped { upload: r.get(0)?, reservation: r.get(1)? }))?.collect::<Result<Vec<_>, _>>()?;
                for d in &gone {
                    conn.execute("DELETE FROM uploads WHERE id = ?1", [&d.upload])?;
                }
                let live: Vec<String> = conn.prepare("SELECT id FROM uploads")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
                Ok((gone, live))
            })
            .await?;
        for d in &gone {
            self.done_writing(&d.upload);
            let _ = self.blobs.abort(&d.upload).await;
        }
        let quiet = std::time::Duration::from_secs(older_than.max(0) as u64);
        for orphan in self.blobs.list_orphans(quiet).await? {
            if !live.contains(&orphan) && !gone.iter().any(|d| d.upload == orphan) {
                let _ = self.blobs.abort(&orphan).await;
            }
        }
        Ok(gone)
    }

    /// What the file named `hash` is, and whose — without its bytes;
    /// [`StoreError::NotFound`] for none.
    pub async fn kept(self: &Arc<Self>, hash: &str) -> Result<Kept, StoreError> {
        let h = hash.to_string();
        self.blocking(move |conn| {
            let info = conn.query_row(&format!("SELECT {INFO_COLUMNS} FROM media WHERE hash = ?1"), [&h], info_row).optional()?.ok_or(StoreError::NotFound(h.clone()))?;
            let mut stmt = conn.prepare("SELECT ticket, principal FROM media_owners WHERE hash = ?1 ORDER BY at, rowid")?;
            let owners = stmt.query_map([&h], |r| Ok(Owner::of(r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
            Ok(Kept { info, owners })
        })
        .await
    }

    /// The file named `hash` and all its bytes; [`StoreError::NotFound`]
    /// for none. Whether the caller may see it is decided before this is
    /// asked; a file larger than one message is read by range instead.
    pub async fn bytes(self: &Arc<Self>, hash: &str) -> Result<(MediaInfo, Vec<u8>), StoreError> {
        let kept = self.kept(hash).await?;
        let bytes = self.read_at(hash, 0, kept.info.bytes as usize).await?;
        Ok((kept.info, bytes))
    }

    /// EDITOR-10: at most `len` bytes of the file named `hash` from
    /// `offset` — from the blob store, or from its row for one kept before
    /// EDITOR-10; what a range of `/media/<hash>` reads. Past its end:
    /// nothing.
    pub async fn read_at(self: &Arc<Self>, hash: &str, offset: u64, len: usize) -> Result<Vec<u8>, StoreError> {
        let h = hash.to_string();
        let (rowid, size, blob) = {
            let h = h.clone();
            self.blocking(move |conn| {
                conn.query_row("SELECT rowid, bytes, blob FROM media WHERE hash = ?1", [&h], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? != 0)))
                    .optional()?
                    .ok_or(StoreError::NotFound(h))
            })
            .await?
        };
        if offset >= size {
            return Ok(Vec::new());
        }
        let n = len.min((size - offset) as usize);
        if blob {
            return Ok(self.blobs.get_range(&h, offset, n).await?);
        }
        self.blocking(move |conn| {
            let blob = conn.blob_open(rusqlite::MAIN_DB, "media", "data", rowid, true)?;
            let mut buf = vec![0u8; n];
            blob.read_at_exact(&mut buf, offset as usize)?;
            Ok(buf)
        })
        .await
    }

    /// EDITOR-10: an address the browser may fetch `hash` from for `ttl`,
    /// when the blob store signs one (an object store); `None` for a row
    /// from before EDITOR-10 and for the disk.
    pub async fn presign(self: &Arc<Self>, hash: &str, ttl: std::time::Duration) -> Option<String> {
        let h = hash.to_string();
        let blob = self.blocking(move |conn| Ok(conn.query_row("SELECT blob FROM media WHERE hash = ?1", [&h], |r| r.get::<_, i64>(0)).optional()?)).await.ok().flatten();
        match blob {
            Some(1) => self.blobs.presign(hash, ttl).await,
            _ => None,
        }
    }

    /// EDITOR-10: what `principal` has put — the files (each once, however
    /// many tickets it was put for) and their bytes: what counts against
    /// their allowance. V2-69: by [`KEEPER`], as the quota service charges it.
    pub async fn usage(self: &Arc<Self>, principal: &str) -> Result<(u64, u64), StoreError> {
        let p = principal.to_string();
        self.blocking(move |conn| {
            Ok(conn.query_row(
                &format!("SELECT count(*), coalesce(sum(bytes), 0) FROM media WHERE hash IN (SELECT hash FROM media_owners WHERE {KEEPER} = ?1)"),
                [&p],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
            )?)
        })
        .await
    }

    /// V2-31, V2-69: every keeper's charge — each file once per principal
    /// who keeps it ([`KEEPER`]): a duplicate is charged to each who put
    /// it, as the quota service commits it (`Put::charged`): (principal,
    /// files, bytes), by principal.
    pub async fn usage_charged(self: &Arc<Self>) -> Result<Vec<(String, u64, u64)>, StoreError> {
        self.blocking(move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT k, count(*), coalesce(sum(bytes), 0) FROM
                   (SELECT DISTINCT {KEEPER} AS k, m.hash, m.bytes FROM media m JOIN media_owners o ON o.hash = m.hash)
                  WHERE k IS NOT NULL GROUP BY k ORDER BY k"
            ))?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
    }

    /// V2-69: what no one is charged for — files with no keeper
    /// ([`KEEPER`]): a link's preview (kept as anyone's: the server's
    /// cache of a public page, never a person's), or a file kept for a
    /// ticket before its uploader was recorded. (files, bytes).
    pub async fn unattributed(self: &Arc<Self>) -> Result<(u64, u64), StoreError> {
        self.blocking(move |conn| {
            Ok(conn.query_row(
                &format!("SELECT count(*), coalesce(sum(bytes), 0) FROM media WHERE hash NOT IN (SELECT hash FROM media_owners o WHERE {KEEPER} IS NOT NULL)"),
                [],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
            )?)
        })
        .await
    }

    /// V2-70: each keeper's charge by the UTC day they first kept each file
    /// (days since 1970-01-01): (principal, day, files, bytes) — what the
    /// quota service's history is backfilled from; summed, [`Self::usage_charged`].
    pub async fn usage_daily(self: &Arc<Self>) -> Result<Vec<(String, i64, u64, u64)>, StoreError> {
        self.blocking(move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT k, d, count(*), coalesce(sum(bytes), 0) FROM
                   (SELECT {KEEPER} AS k, m.hash, m.bytes, min(o.at) / 86400 AS d FROM media m JOIN media_owners o ON o.hash = m.hash
                     WHERE {KEEPER} IS NOT NULL GROUP BY k, m.hash)
                  GROUP BY k, d ORDER BY k, d"
            ))?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)? as u64, r.get::<_, i64>(3)? as u64)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
    }

    /// EDITOR-10: what `url` unfurled to, kept within `max_age` seconds.
    pub async fn unfurl_cached(self: &Arc<Self>, url: &str, max_age: i64) -> Result<Option<String>, StoreError> {
        let url = url.to_string();
        self.blocking(move |conn| Ok(conn.query_row("SELECT answer FROM unfurls WHERE url = ?1 AND at >= ?2", params![url, now() - max_age], |r| r.get(0)).optional()?)).await
    }

    /// EDITOR-10: keeps what `url` unfurled to (its answer, as JSON).
    pub async fn unfurl_keep(self: &Arc<Self>, url: &str, answer: String) -> Result<(), StoreError> {
        let url = url.to_string();
        self.blocking(move |conn| {
            conn.execute("INSERT OR REPLACE INTO unfurls (url, at, answer) VALUES (?1, ?2, ?3)", params![url, now(), answer])?;
            Ok(())
        })
        .await
    }

    /// How many files the store keeps — for a log line.
    pub fn count(&self) -> usize {
        self.lock().query_row("SELECT count(*) FROM media", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    }
}

/// V2-69: **whose a kept file is**, per `media_owners` row (aliased `o`, or
/// bare): whoever put it (`put_by`), else — a row from before the uploader
/// was recorded — the principal it was kept as (a file put with no ticket),
/// else no one (a ticket's file from then, or a link's preview: anyone's).
pub const KEEPER: &str = "coalesce(nullif(put_by, ''), CASE WHEN ticket = '' AND principal NOT IN ('', '*') THEN principal END)";

/// Whether `put_by` keeps `hash` already, by [`KEEPER`].
fn keeps(tx: &rusqlite::Transaction<'_>, hash: &str, put_by: Option<&str>) -> Result<bool, StoreError> {
    let Some(p) = put_by.filter(|p| !p.is_empty()) else { return Ok(false) };
    Ok(tx.query_row(&format!("SELECT 1 FROM media_owners WHERE hash = ?1 AND {KEEPER} = ?2 LIMIT 1"), params![hash, p], |_| Ok(())).optional()?.is_some())
}

/// The owner's row for a kept file, and the file as kept: a name given now
/// names a file kept before with none.
fn own(tx: &rusqlite::Transaction<'_>, info: MediaInfo, owner: &Owner, at: i64, put_by: Option<&str>) -> Result<MediaInfo, StoreError> {
    let (ticket, principal) = owner.columns();
    tx.execute("INSERT OR IGNORE INTO media_owners (hash, ticket, principal, at, put_by) VALUES (?1, ?2, ?3, ?4, ?5)", params![info.hash, ticket, principal, at, put_by])?;
    if info.name.is_some() {
        tx.execute("UPDATE media SET name = ?2 WHERE hash = ?1 AND name IS NULL", params![info.hash, info.name])?;
    }
    Ok(tx.query_row(&format!("SELECT {INFO_COLUMNS} FROM media WHERE hash = ?1"), [&info.hash], info_row)?)
}

#[cfg(test)]
mod tests;
