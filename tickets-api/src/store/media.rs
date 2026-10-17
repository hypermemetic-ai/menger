//! COMMENT-5: the **media store** — the images a comment shows, kept by
//! menger beside the tickets, content-addressed (their SHA-256), and read
//! back through the reach of the ticket each was put for.
//!
//! **Behind the cache, as everything stored is**, with the bytes where the
//! bodies are (SCALE-14): the cache holds what each image is — its type,
//! size, width, height and whose it is ([`Media`]) — read at open, and
//! moved by `put` alone. The bytes stay in the file: `get` checks the
//! caller against the cache under the read lock, lets go of it, and reads
//! the bytes on a read connection of their own ([`super::Readers`]), off the
//! runtime. A kept image never changes (its name is its content), so no
//! write can put a newer one beside its row.
//!
//! **Whose it is**: an image belongs to the ticket it was put for, and is
//! seen by whoever sees that ticket ([`super::seen::Sees::media`]); the same
//! bytes put for a second ticket are one image, and the second's too. One
//! put with no ticket — from a comment on the app, which names none — is
//! the putter's, seen as a comment on the app is (its principal, and the
//! admins of `*`; everyone with login off). A ticket's id is read through
//! the resolver, so a reshortcoded ticket (ACT-6) keeps its images and a
//! deleted one (ACT-5) hides them.
//!
//! **Stored** in two tables of their own, which a file gains, empty, on its
//! first open ([`migrate`]): `media` — a row per image, its bytes a BLOB —
//! and `media_owners` — a row per ticket (or app putter) it is for. Nothing
//! else reads or writes them, and no stored form moves: a release before
//! COMMENT-5 opens such a file and never looks at them.

use super::seen::Seen;
use super::writs::OnMedia;
use super::{Reader, Store, StoreError};
use crate::media::{MediaInfo, Problem};
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// The tables. `media.hash` is the image's name; `media_owners` says whose:
/// `ticket` its key (`''` for an image on the app), `principal` the app
/// putter's (`''` for a ticket's, and with login off).
pub(super) const TABLES: &str = "
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

/// A file without the media tables gains them, empty, here — once; on
/// every later open a no-op. Additive: nothing else in the file moves.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    let had: bool = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'media_owners'",
        [],
        |r| r.get::<_, i64>(0).map(|n| n > 0),
    )?;
    if had {
        return Ok(false);
    }
    conn.execute_batch(TABLES)?;
    tracing::warn!("COMMENT-5: the file gained the media tables, empty, durably; every later open reads them");
    Ok(true)
}

/// Whose an image is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Owner {
    /// The ticket it was put for, by its key as it was then.
    Ticket(String),
    /// Put with no ticket: its putter's principal (`None` with login off).
    App(Option<String>),
}

impl Owner {
    /// Its two stored columns.
    fn columns(&self) -> (&str, &str) {
        match self {
            Owner::Ticket(t) => (t.as_str(), ""),
            Owner::App(p) => ("", p.as_deref().unwrap_or("")),
        }
    }

    fn of(ticket: String, principal: String) -> Self {
        match ticket.is_empty() {
            false => Owner::Ticket(ticket),
            true => Owner::App((!principal.is_empty()).then_some(principal)),
        }
    }
}

/// One kept image, as the cache holds it: what it is, and whose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Kept {
    pub(crate) info: MediaInfo,
    pub(crate) owners: Vec<Owner>,
}

/// Every kept image, by hash — no bytes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Media {
    by_hash: HashMap<String, Kept>,
}

impl Media {
    pub(crate) fn get(&self, hash: &str) -> Option<&Kept> {
        self.by_hash.get(hash)
    }

    /// How many images are kept.
    pub(crate) fn len(&self) -> usize {
        self.by_hash.len()
    }

    /// The cache's half of a committed put: `info` is `owner`'s too.
    fn put(&mut self, info: MediaInfo, owner: Owner) {
        let kept = self.by_hash.entry(info.hash.clone()).or_insert_with(|| Kept { info, owners: Vec::new() });
        if !kept.owners.contains(&owner) {
            kept.owners.push(owner);
        }
    }
}

/// Every image as stored, without its bytes — what a start reads.
pub(super) fn load(conn: &Connection) -> rusqlite::Result<Media> {
    let mut media = Media::default();
    let mut stmt = conn.prepare("SELECT hash, mime, bytes, width, height FROM media")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let info = MediaInfo { hash: r.get(0)?, mime: r.get(1)?, bytes: r.get::<_, i64>(2)? as u64, width: r.get(3)?, height: r.get(4)? };
        media.by_hash.insert(info.hash.clone(), Kept { info, owners: Vec::new() });
    }
    let mut stmt = conn.prepare("SELECT hash, ticket, principal FROM media_owners ORDER BY at, rowid")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let hash: String = r.get(0)?;
        if let Some(kept) = media.by_hash.get_mut(&hash) {
            kept.owners.push(Owner::of(r.get(1)?, r.get(2)?));
        }
    }
    Ok(media)
}

/// The bytes a `data` parameter carries: base64, standard alphabet, a
/// `data:…;base64,` before it and whitespace inside it read past.
pub(crate) fn decode(data: &str) -> Result<Vec<u8>, Problem> {
    use base64::Engine;
    let t = data.trim();
    let t = match t.strip_prefix("data:").and_then(|r| r.split_once(',')) {
        Some((head, rest)) if head.ends_with(";base64") => rest,
        _ => t,
    };
    let clean: String = t.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if clean.is_empty() {
        return Err(Problem::Empty);
    }
    base64::engine::general_purpose::STANDARD.decode(clean.as_bytes()).map_err(|e| Problem::NotBase64(e.to_string()))
}

/// An image's name: its SHA-256, in lower-case hex.
pub(crate) fn hash(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// What `put` answers: the image as kept, and whether its bytes were kept
/// already.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Put {
    pub info: MediaInfo,
    pub duplicate: bool,
}

impl Store {
    /// COMMENT-5: keeps an image — `data` (base64) checked against `mime`
    /// by its own bytes ([`crate::media::check`]: a kept type, at most
    /// [`crate::media::MAX_BYTES`], the type declared), named by its
    /// SHA-256 — for the ticket `on` is for (or, with none, its putter,
    /// `principal`). A refusal is [`StoreError::MediaRefused`], naming why,
    /// and nothing is kept. The same bytes kept already are no second row
    /// (`duplicate`); for a new ticket the one row more says it is its too;
    /// for the same one nothing is written at all.
    ///
    /// ACCESS-11: as `on` allows ([`OnMedia`]) — an editor's permit on the
    /// ticket, checked under the write-serial lock.
    pub async fn put_media(&self, on: impl Into<OnMedia>, mime: &str, data: String, principal: Option<String>) -> Result<Put, StoreError> {
        let on = on.into();
        let declared = mime.to_string();
        // The decode and the hash, off the runtime: up to 7 MiB each.
        let (bytes, info) = tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, MediaInfo), Problem> {
            let bytes = decode(&data)?;
            let found = crate::media::check(&declared, &bytes)?;
            let info = MediaInfo { hash: hash(&bytes), mime: found.mime.to_string(), bytes: bytes.len() as u64, width: found.width, height: found.height };
            Ok((bytes, info))
        })
        .await
        .map_err(|e| StoreError::Db(format!("media task failed: {e}")))?
        .map_err(StoreError::MediaRefused)?;
        let serial = self.serial().await;
        let (owner, had) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let owner = on.owner(&cache, principal.clone())?;
            let had = cache.media.get(&info.hash).map(|k| k.owners.contains(&owner));
            (owner, had)
        };
        match had {
            // Kept, and this ticket's already: nothing to write.
            Some(true) => return Ok(Put { info, duplicate: true }),
            Some(false) | None => {}
        }
        let at = chrono::Utc::now().timestamp();
        let new = had.is_none();
        let (stored, kept) = ((info.clone(), owner.clone(), principal), info.clone());
        self.write(
            serial,
            move |tx| {
                let (i, o, by) = &stored;
                if new {
                    tx.prepare_cached("INSERT OR IGNORE INTO media (hash, mime, bytes, width, height, at, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?
                        .execute(params![i.hash, i.mime, i.bytes as i64, i.width, i.height, at, bytes])?;
                }
                let (ticket, principal) = o.columns();
                tx.prepare_cached("INSERT OR IGNORE INTO media_owners (hash, ticket, principal, at, put_by) VALUES (?1, ?2, ?3, ?4, ?5)")?
                    .execute(params![i.hash, ticket, principal, at, by])?;
                Ok(())
            },
            move |cache, _| cache.media.put(kept, owner),
            move |(), (), _| Put { info, duplicate: !new },
        )
        .await
    }

    /// COMMENT-5: the image named `hash` and its bytes — when `who` sees a
    /// ticket it was put for (ACCESS-10); else, as for a hash that names
    /// none, [`StoreError::MediaNotFound`]. What it is from the cache; the
    /// bytes from the file, on a read connection, the lock let go first.
    pub async fn media(&self, who: &(impl Reader + ?Sized), hash: &str) -> Result<(MediaInfo, Vec<u8>), StoreError> {
        let info = self.media_info(who, hash)?;
        let h = info.hash.clone();
        let data = self
            .readers
            .read(move |conn| Ok(conn.query_row("SELECT data FROM media WHERE hash = ?1", [&h], |r| r.get::<_, Vec<u8>>(0))?))
            .await?;
        Ok((info, data))
    }

    /// COMMENT-5: what the image named `hash` is, without its bytes — when
    /// `who` sees it ([`Store::media`]'s rule). From the cache alone.
    pub fn media_info(&self, who: &(impl Reader + ?Sized), hash: &str) -> Result<MediaInfo, StoreError> {
        let seen = Seen::of(who)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        match cache.media.get(hash) {
            Some(k) if sees.media(&k.owners) => Ok(k.info.clone()),
            _ => Err(StoreError::MediaNotFound(hash.to_string())),
        }
    }

    /// COMMENT-5: how many images the store keeps — for a log line.
    pub fn media_count(&self, _: &super::System) -> usize {
        self.tickets.read().expect("store lock poisoned").media.len()
    }
}
