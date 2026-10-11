//! NOTE-2: the **notes store** — every note, kept beside the tickets as a
//! record of its own (NOTE-1, decided 2026-10-11: "I wanted menger to store
//! the notes"), never in a body and never a ticket until `upgrade` makes it
//! one.
//!
//! **Behind the cache, as everything stored is** (AGENTS.md): every note is
//! read into the cache when the store opens ([`load`]) and every read
//! answers from it — `list` walks the cache's notes, newest first, under the
//! read lock, with no I/O. A write — `add`, and `upgrade`'s mark — takes the
//! write path every write takes ([`super::Store::write`], MNG-20): worked out
//! under the write-serial lock from the cache's record, committed in one
//! transaction, then put into the cache.
//!
//! **An upgrade is one write.** The ticket a note becomes and the note's
//! pointer to it commit in the same transaction — the ticket's own birth
//! ([`super::Store::upgrade_note`]) — so a ticket made from a note always
//! has the note pointing at it, and a refused ticket leaves the note open
//! and spends no id.
//!
//! **Stored** in one table, `notes` — a row per note: its id, when it was
//! taken, its author and signer, its body, and the ticket it became and
//! when. A file without the table gains it, empty, on open ([`migrate`]) —
//! the table holds no state derived from the tickets, so nothing is built.

use super::{Cache, Needle, StoreError};
use crate::notes::{Note, NoteState};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

/// The table. `INTEGER PRIMARY KEY`: a note's id is its rowid, allocated by
/// the store (the newest id + 1) and never reused — nothing deletes one.
pub(super) const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS notes (
            id          INTEGER PRIMARY KEY,
            at          INTEGER NOT NULL,
            author      TEXT NOT NULL,
            principal   TEXT,
            body        TEXT NOT NULL,
            ticket      TEXT,
            upgraded_at INTEGER
        );
    ";

/// A file without the notes table gains it, empty, here — on every open, a
/// no-op once it is there. Returns whether it was made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    let had: bool = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'notes'",
        [],
        |r| r.get::<_, i64>(0).map(|n| n > 0),
    )?;
    if had {
        return Ok(false);
    }
    conn.execute_batch(TABLE)?;
    tracing::warn!("NOTE-2: the file gained the notes table, empty, durably; every later open reads it");
    Ok(true)
}

/// What `list` asks: every filter given must hold. Blank is not given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteQuery {
    pub state: Option<NoteState>,
    /// The author is exactly this.
    pub author: Option<String>,
    /// In the body — `find`'s match: a substring, whatever its ASCII case.
    pub text: Option<String>,
}

/// One page of `list`: the notes, newest first, and the cursor to pass as
/// the next page's `after` — `None` when nothing more matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotePage {
    pub notes: Vec<Note>,
    pub next: Option<u64>,
}

/// The store's notes, as the cache holds them: by id, so newest first is
/// the map backwards and a cursor is a range.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Notes {
    by_id: BTreeMap<u64, Note>,
}

impl Notes {
    pub(crate) fn get(&self, id: u64) -> Option<&Note> {
        self.by_id.get(&id)
    }

    /// The id the next note is born with.
    pub(crate) fn next_id(&self) -> u64 {
        self.by_id.keys().next_back().map_or(1, |n| n + 1)
    }

    /// The cache's half of a committed write: `note` as it is now.
    pub(crate) fn put(&mut self, note: Note) {
        self.by_id.insert(note.id, note);
    }
}

/// One page of the notes `q` admits, newest first: strictly older than
/// `after` (a note id) when given, at most `limit` of them. An `after`
/// naming no note is [`StoreError::NoteCursor`]. `next` is the page's last
/// id when more match past it (for `limit: 0`, the cursor it was cut with),
/// else `None`. Under the read lock, from the cache alone.
pub(super) fn page(cache: &Cache, q: &NoteQuery, after: Option<u64>, limit: Option<u32>) -> Result<NotePage, StoreError> {
    let notes = &cache.notes;
    if let Some(a) = after.filter(|a| notes.get(*a).is_none()) {
        return Err(StoreError::NoteCursor(a));
    }
    let author = q.author.as_deref().map(str::trim).filter(|a| !a.is_empty());
    let needle = q.text.as_deref().and_then(Needle::of);
    let admits = |n: &Note| -> bool {
        q.state.is_none_or(|s| n.state() == s)
            && author.is_none_or(|a| n.author == a)
            && needle.as_ref().is_none_or(|needle| needle.within(&n.body))
    };
    let newest_first: Box<dyn Iterator<Item = &Note>> = match after {
        Some(a) => Box::new(notes.by_id.range(..a).rev().map(|(_, n)| n)),
        None => Box::new(notes.by_id.values().rev()),
    };
    let take = limit.map_or(usize::MAX, |n| n as usize);
    let mut matches = newest_first.filter(|n| admits(n));
    let page: Vec<Note> = matches.by_ref().take(take).cloned().collect();
    let more = limit.is_some() && matches.next().is_some();
    let next = more.then(|| page.last().map(|n| n.id).or(after)).flatten();
    Ok(NotePage { notes: page, next })
}

/// Writes `note` whole — a new one, or one just upgraded — inside the
/// caller's transaction. Only the ticket and when move after birth.
pub(super) fn store(conn: &Connection, n: &Note) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO notes (id, at, author, principal, body, ticket, upgraded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (id) DO UPDATE SET ticket = excluded.ticket, upgraded_at = excluded.upgraded_at",
    )?
    .execute(params![n.id as i64, n.at, n.author, n.principal, n.body, n.ticket, n.upgraded_at])?;
    Ok(())
}

/// Every note as stored — what a start reads.
pub(super) fn load(conn: &Connection) -> rusqlite::Result<Notes> {
    let mut stmt = conn.prepare("SELECT id, at, author, principal, body, ticket, upgraded_at FROM notes ORDER BY id")?;
    let mut rows = stmt.query([])?;
    let mut notes = Notes::default();
    while let Some(r) = rows.next()? {
        notes.put(Note {
            id: r.get::<_, i64>(0)? as u64,
            at: r.get(1)?,
            author: r.get(2)?,
            principal: r.get(3)?,
            body: r.get(4)?,
            ticket: r.get(5)?,
            upgraded_at: r.get(6)?,
        });
    }
    Ok(notes)
}

/// The body a note gives the ticket it becomes: its text, then a line
/// saying when it was noted (UTC) and which note it was.
pub(crate) fn ticket_body(n: &Note) -> String {
    format!("{}\n\nNoted {} — note {}.", n.body.trim_end(), stamp(n.at), n.id)
}

/// The title a note gives the ticket it becomes when none is given: its
/// first line with words ([`crate::notes::title_of`]), else when it was
/// taken.
pub(crate) fn ticket_title(n: &Note) -> String {
    crate::notes::title_of(&n.body).unwrap_or_else(|| format!("Note {} of {}", n.id, stamp(n.at)))
}

fn stamp(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| at.to_string())
}
