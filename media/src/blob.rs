//! EDITOR-10: **where a file's bytes live — one trait, swappable.** The
//! owner, 2026-11-02: "make sure we support swapping out the media
//! activation for one that uses object storage when we need it. for now …
//! hosting data on our own machines". Every byte the media service keeps
//! goes through [`BlobStore`]; what a file is and whose (hash, type, name,
//! size, owners, uploads) stays in `media.db`. Nothing here assumes a
//! filesystem: a key is a hash or an upload's id, never a path, and an
//! upload is written, sealed, read back and committed by those ids alone —
//! what an S3-compatible store (R2, B2, MinIO, Garage) does with a
//! multipart upload, a temporary object and a copy.
//!
//! | step | call |
//! |---|---|
//! | a file in one message | [`BlobStore::put`] |
//! | a resumable upload | [`BlobStore::begin_upload`], [`BlobStore::append`] at the offset it has ([`BlobStore::uploaded`]), [`BlobStore::seal`], then [`BlobStore::commit`] under its SHA-256 — or [`BlobStore::abort`] |
//! | serving | [`BlobStore::presign`] (an address the browser is sent to, after the checks) else [`BlobStore::get_range`] streamed by the shell |
//! | sweeping | [`BlobStore::list_orphans`]: uploads nothing touched in a while |
//!
//! One implementation ships: [`DiskStore`], `<root>/<sha256>` with uploads
//! in `<root>/tmp/` (`--media-blob-dir`, `MENGER_MEDIA_BLOB_DIR`; by default
//! `media/blobs` beside the media file). The root may be another disk or a
//! network mount. `--media-store s3` is refused until V2-32 builds it.

use async_trait::async_trait;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Why the blob store did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobError {
    /// No blob (or upload) of that key.
    NotFound(String),
    /// A write at an offset that is not the upload's length.
    Offset { expected: u64, got: u64 },
    /// The store itself failed, in words.
    Io(String),
}

impl std::fmt::Display for BlobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlobError::NotFound(k) => write!(f, "no blob {k}"),
            BlobError::Offset { expected, got } => write!(f, "a write at {got}; the upload has {expected} bytes"),
            BlobError::Io(e) => write!(f, "the blob store: {e}"),
        }
    }
}

impl std::error::Error for BlobError {}

impl From<std::io::Error> for BlobError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => BlobError::NotFound(e.to_string()),
            _ => BlobError::Io(e.to_string()),
        }
    }
}

/// Where a file's bytes live. Keys are a file's SHA-256 (64 lower-case
/// hex digits) or an upload's id (32 hex digits) — never a path.
#[async_trait]
pub trait BlobStore: Send + Sync + std::fmt::Debug {
    /// Keeps `bytes` under `hash` (a no-op when it is kept already).
    async fn put(&self, hash: &str, bytes: Vec<u8>) -> Result<(), BlobError>;
    /// Starts an upload, empty.
    async fn begin_upload(&self, upload: &str) -> Result<(), BlobError>;
    /// Appends `bytes` at `offset`, which must be what the upload has (a
    /// write cut short before is overwritten); its length after.
    async fn append(&self, upload: &str, offset: u64, bytes: Vec<u8>) -> Result<u64, BlobError>;
    /// What the upload has, in bytes.
    async fn uploaded(&self, upload: &str) -> Result<u64, BlobError>;
    /// No more bytes come: the upload is readable whole ([`Self::read_sealed`]).
    async fn seal(&self, upload: &str) -> Result<(), BlobError>;
    /// At most `len` bytes of a sealed upload from `offset`.
    async fn read_sealed(&self, upload: &str, offset: u64, len: usize) -> Result<Vec<u8>, BlobError>;
    /// The sealed upload kept under `hash` — or dropped, when `hash` is kept already.
    async fn commit(&self, upload: &str, hash: &str) -> Result<(), BlobError>;
    /// The upload dropped, whatever it had.
    async fn abort(&self, upload: &str) -> Result<(), BlobError>;
    /// At most `len` bytes of `hash` from `offset`; nothing past its end.
    async fn get_range(&self, hash: &str, offset: u64, len: usize) -> Result<Vec<u8>, BlobError>;
    /// Its size, or `None`.
    async fn stat(&self, hash: &str) -> Result<Option<u64>, BlobError>;
    /// `hash` dropped.
    async fn delete(&self, hash: &str) -> Result<(), BlobError>;
    /// The uploads nothing has touched for `older_than`.
    async fn list_orphans(&self, older_than: Duration) -> Result<Vec<String>, BlobError>;
    /// An address the browser may fetch `hash` from for `ttl`, signed — an
    /// object store's; `None` (the default, and the disk's): the shell
    /// streams the bytes itself.
    async fn presign(&self, _hash: &str, _ttl: Duration) -> Option<String> {
        None
    }
    /// Where it keeps them, for a log line.
    fn describe(&self) -> String;
}

/// A file's hash or an upload's id: hex, of the length its kind has.
fn key(k: &str, len: usize) -> Result<&str, BlobError> {
    match k.len() == len && k.bytes().all(|b| b.is_ascii_hexdigit()) {
        true => Ok(k),
        false => Err(BlobError::NotFound(k.to_string())),
    }
}

/// The bytes on this machine's disks: `<root>/<sha256>`, uploads in
/// `<root>/tmp/<id>.part`, a commit a rename.
#[derive(Debug, Clone)]
pub struct DiskStore {
    root: PathBuf,
}

impl DiskStore {
    /// Over `root`, made if missing (`tmp/` within it).
    pub fn open(root: &Path) -> Result<Self, BlobError> {
        std::fs::create_dir_all(root.join("tmp")).map_err(|e| BlobError::Io(format!("{}: {e}", root.display())))?;
        Ok(Self { root: root.to_path_buf() })
    }

    /// Its root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn blob(&self, hash: &str) -> Result<PathBuf, BlobError> {
        Ok(self.root.join(key(hash, 64)?))
    }

    fn part(&self, upload: &str) -> Result<PathBuf, BlobError> {
        Ok(self.root.join("tmp").join(format!("{}.part", key(upload, 32)?)))
    }

    async fn run<T: Send + 'static>(work: impl FnOnce() -> Result<T, BlobError> + Send + 'static) -> Result<T, BlobError> {
        tokio::task::spawn_blocking(work).await.map_err(|e| BlobError::Io(format!("blob task failed: {e}")))?
    }
}

fn read_range(path: PathBuf, offset: u64, len: usize) -> Result<Vec<u8>, BlobError> {
    let mut file = std::fs::File::open(&path)?;
    let size = file.metadata()?.len();
    if offset >= size {
        return Ok(Vec::new());
    }
    let n = len.min((size - offset) as usize);
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; n];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

#[async_trait]
impl BlobStore for DiskStore {
    async fn put(&self, hash: &str, bytes: Vec<u8>) -> Result<(), BlobError> {
        let (path, tmp) = (self.blob(hash)?, self.root.join("tmp").join(format!("{hash}.put.{}", uuid::Uuid::new_v4().simple())));
        Self::run(move || {
            if path.exists() {
                return Ok(());
            }
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            std::fs::rename(&tmp, &path)?;
            Ok(())
        })
        .await
    }

    async fn begin_upload(&self, upload: &str) -> Result<(), BlobError> {
        let part = self.part(upload)?;
        Self::run(move || Ok(std::fs::File::create(part).map(|_| ())?)).await
    }

    async fn append(&self, upload: &str, offset: u64, bytes: Vec<u8>) -> Result<u64, BlobError> {
        let part = self.part(upload)?;
        Self::run(move || {
            let mut f = std::fs::OpenOptions::new().write(true).open(&part)?;
            let has = f.metadata()?.len();
            if offset > has {
                return Err(BlobError::Offset { expected: has, got: offset });
            }
            f.set_len(offset)?;
            f.seek(SeekFrom::Start(offset))?;
            f.write_all(&bytes)?;
            Ok(offset + bytes.len() as u64)
        })
        .await
    }

    async fn uploaded(&self, upload: &str) -> Result<u64, BlobError> {
        let part = self.part(upload)?;
        Self::run(move || Ok(std::fs::metadata(part)?.len())).await
    }

    async fn seal(&self, upload: &str) -> Result<(), BlobError> {
        let part = self.part(upload)?;
        Self::run(move || Ok(std::fs::File::open(part)?.sync_all()?)).await
    }

    async fn read_sealed(&self, upload: &str, offset: u64, len: usize) -> Result<Vec<u8>, BlobError> {
        let part = self.part(upload)?;
        Self::run(move || read_range(part, offset, len)).await
    }

    async fn commit(&self, upload: &str, hash: &str) -> Result<(), BlobError> {
        let (part, blob) = (self.part(upload)?, self.blob(hash)?);
        Self::run(move || {
            if blob.exists() {
                std::fs::remove_file(&part)?;
            } else {
                std::fs::rename(&part, &blob)?;
            }
            Ok(())
        })
        .await
    }

    async fn abort(&self, upload: &str) -> Result<(), BlobError> {
        let part = self.part(upload)?;
        Self::run(move || match std::fs::remove_file(part) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        })
        .await
    }

    async fn get_range(&self, hash: &str, offset: u64, len: usize) -> Result<Vec<u8>, BlobError> {
        let path = self.blob(hash)?;
        Self::run(move || read_range(path, offset, len)).await
    }

    async fn stat(&self, hash: &str) -> Result<Option<u64>, BlobError> {
        let path = self.blob(hash)?;
        Self::run(move || match std::fs::metadata(path) {
            Ok(m) => Ok(Some(m.len())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        })
        .await
    }

    async fn delete(&self, hash: &str) -> Result<(), BlobError> {
        let path = self.blob(hash)?;
        Self::run(move || match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        })
        .await
    }

    async fn list_orphans(&self, older_than: Duration) -> Result<Vec<String>, BlobError> {
        let tmp = self.root.join("tmp");
        Self::run(move || {
            let mut out = Vec::new();
            for e in std::fs::read_dir(tmp)? {
                let e = e?;
                let name = e.file_name().to_string_lossy().to_string();
                let Some(id) = name.strip_suffix(".part") else { continue };
                let age = e.metadata()?.modified().ok().and_then(|m| m.elapsed().ok()).unwrap_or_default();
                if age >= older_than && key(id, 32).is_ok() {
                    out.push(id.to_string());
                }
            }
            Ok(out)
        })
        .await
    }

    fn describe(&self) -> String {
        format!("disk {}", self.root.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_disk_keeps_puts_and_resumable_uploads_by_key_never_by_path() {
        let dir = tempfile::tempdir().unwrap();
        let d = DiskStore::open(dir.path()).unwrap();
        let h = "ab".repeat(32);
        d.put(&h, b"hello".to_vec()).await.unwrap();
        assert_eq!(d.get_range(&h, 1, 3).await.unwrap(), b"ell");
        assert_eq!(d.get_range(&h, 9, 3).await.unwrap(), b"");
        assert_eq!(d.stat(&h).await.unwrap(), Some(5));
        let u = "0f".repeat(16);
        d.begin_upload(&u).await.unwrap();
        assert_eq!(d.append(&u, 0, b"abc".to_vec()).await.unwrap(), 3);
        assert_eq!(d.append(&u, 9, b"x".to_vec()).await, Err(BlobError::Offset { expected: 3, got: 9 }));
        assert_eq!(d.append(&u, 3, b"def".to_vec()).await.unwrap(), 6);
        assert_eq!(d.uploaded(&u).await.unwrap(), 6);
        d.seal(&u).await.unwrap();
        assert_eq!(d.read_sealed(&u, 2, 10).await.unwrap(), b"cdef");
        assert_eq!(d.list_orphans(Duration::ZERO).await.unwrap(), vec![u.clone()]);
        assert!(d.list_orphans(Duration::from_secs(3600)).await.unwrap().is_empty());
        let h2 = "cd".repeat(32);
        d.commit(&u, &h2).await.unwrap();
        assert_eq!(d.get_range(&h2, 0, 100).await.unwrap(), b"abcdef");
        assert!(d.list_orphans(Duration::ZERO).await.unwrap().is_empty());
        d.delete(&h2).await.unwrap();
        assert_eq!(d.stat(&h2).await.unwrap(), None);
        assert!(d.presign(&h, Duration::from_secs(60)).await.is_none(), "the disk streams; it signs nothing");
        // A key that is a path is none.
        for bad in ["../../etc/passwd", "tmp", &"g".repeat(64)] {
            assert!(matches!(d.get_range(bad, 0, 1).await, Err(BlobError::NotFound(_))), "{bad}");
        }
        assert!(matches!(d.append("../x", 0, vec![1]).await, Err(BlobError::NotFound(_))));
    }
}
