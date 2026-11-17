//! NOTES-5: **marks** — what an agent leaves on a note without writing in
//! it (the owner, 2026-11-15: *"I want to make sure that agents can listen
//! in on notes, and underline sections, add links, and write components
//! into the docuemtn itself."*). A mark is a range of the note's characters
//! carrying a link, one line of state, or both; the page underlines it.
//!
//! **Anchored as a question is** ([`crate::threads`]): two Yjs relative
//! positions in the note's CRDT, resolved now to character offsets of the
//! markdown `notes.get` answers (`from`, `to`), so the mark follows its
//! words through every hand's edit; every character deleted, it is
//! **detached** — kept, listed last, drawn nowhere. **Nothing here writes
//! the note's text**: the CRDT is read, never changed, and the note's
//! revision does not move.
//!
//! **Stored** in the notes file, `note_marks` (a table of its own, made by
//! the store's open on a file from before: such a file opens with no
//! marks): a row per mark — its anchor, the words it held when made, its
//! link and label, who and when, who changed it last. The columns
//! `component`, `version` and `props` are kept for a mark that will carry a
//! piece of the interface under its passage (NOTES-5's third requirement,
//! built after NOTES-4): nullable, never written yet, so that pass reads
//! and writes them with no migration that rewrites a row. Said on the
//! channel for `watch`: `mark_changed` (made or changed, the mark whole) and
//! `mark_removed`.

use crate::crdt::{b64, text_of};
use crate::events::{Anchor, Mark};
use crate::store::{live_doc, now_ms, Change, NotesStore, StoreError};
use crate::threads::{anchored, placed, slice, Anchored, Hand};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Arc;

/// The table.
pub const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS note_marks (
            mark        TEXT PRIMARY KEY,
            id          TEXT NOT NULL,
            from_rel    BLOB NOT NULL,
            to_rel      BLOB NOT NULL,
            quote       TEXT NOT NULL,
            link        TEXT,
            label       TEXT,
            by          TEXT NOT NULL,
            principal   TEXT NOT NULL,
            at          INTEGER NOT NULL,
            changed_at  INTEGER,
            changed_by  TEXT,
            component   TEXT,
            version     TEXT,
            props       TEXT
        );
        CREATE INDEX IF NOT EXISTS note_marks_id ON note_marks (id);
    ";

/// What a mark's change asks: each part `None` stays as it is, `Some("")`
/// takes it off, `Some(text)` sets it.
#[derive(Debug, Clone, Default)]
pub struct Remark {
    pub link: Option<String>,
    pub label: Option<String>,
}

/// A stored row, before its anchor is resolved.
struct Row {
    mark: String,
    id: String,
    from_rel: Vec<u8>,
    to_rel: Vec<u8>,
    quote: String,
    link: Option<String>,
    label: Option<String>,
    by: String,
    principal: String,
    at: i64,
    changed_at: Option<i64>,
    changed_by: Option<String>,
}

const COLUMNS: &str = "mark, id, from_rel, to_rel, quote, link, label, by, principal, at, changed_at, changed_by";

fn row_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        mark: r.get(0)?,
        id: r.get(1)?,
        from_rel: r.get(2)?,
        to_rel: r.get(3)?,
        quote: r.get(4)?,
        link: r.get(5)?,
        label: r.get(6)?,
        by: r.get(7)?,
        principal: r.get(8)?,
        at: r.get(9)?,
        changed_at: r.get(10)?,
        changed_by: r.get(11)?,
    })
}

/// `text` as a column: none when empty.
fn kept(text: Option<String>) -> Option<String> {
    text.filter(|t| !t.is_empty())
}

/// `row` made a [`Mark`]: its anchor resolved in `crdt` now.
fn dressed(crdt: &yrs::Doc, row: Row) -> Mark {
    let text = text_of(crdt);
    let at = placed(crdt, &text, &row.from_rel, &row.to_rel, false);
    Mark {
        mark: row.mark,
        id: row.id,
        anchor: Anchor { from: b64(&row.from_rel), to: b64(&row.to_rel) },
        from: at.map(|(f, _)| f),
        to: at.map(|(_, t)| t),
        detached: at.is_none(),
        words: at.map(|(f, t)| slice(&text, f, t).to_string()).unwrap_or_default(),
        quote: row.quote,
        link: row.link,
        label: row.label,
        by: row.by,
        principal: row.principal,
        at: row.at,
        changed_at: row.changed_at,
        changed_by: row.changed_by,
    }
}

impl NotesStore {
    /// The mark `mark` of the live note `id`, dressed.
    fn mark_now(&self, conn: &Connection, id: &str, mark: &str) -> Result<Mark, StoreError> {
        let row = conn
            .query_row(&format!("SELECT {COLUMNS} FROM note_marks WHERE mark = ?1 AND id = ?2"), params![mark, id], row_of)
            .optional()?
            .ok_or_else(|| StoreError::NoMark(mark.to_string()))?;
        let crdt = self.crdt(conn, id)?;
        let crdt = crdt.lock().unwrap_or_else(|p| p.into_inner());
        Ok(dressed(&crdt, row))
    }

    /// `notes.mark`: a mark on the characters `from..to` of `id`'s text now,
    /// anchored in its CRDT (which it does not change), with its `link` and
    /// `label` (empty: none), by `hand`.
    pub async fn mark(self: &Arc<Self>, id: &str, from: u32, to: u32, link: Option<String>, label: Option<String>, hand: Hand) -> Result<Mark, StoreError> {
        let id = id.to_string();
        let (mark, org) = self
            .blocking(move |s| {
                let conn = s.lock();
                let doc = live_doc(&conn, &id)?;
                let crdt = s.crdt(&conn, &id)?;
                let crdt = crdt.lock().unwrap_or_else(|p| p.into_inner());
                if from >= to {
                    return Err(StoreError::BadRange(format!("a mark is on some characters: from {from} is not before to {to}")));
                }
                let Anchored { from_rel, to_rel, quote } = anchored(&crdt, from, to)?;
                if quote.is_empty() {
                    return Err(StoreError::BadRange(format!("{from}..{to} is past the end of the text")));
                }
                let mark = crate::vocab::mint(|m| conn.query_row("SELECT 1 FROM note_marks WHERE mark = ?1", [m], |_| Ok(())).optional().ok().flatten().is_some());
                conn.execute(
                    &format!("INSERT INTO note_marks ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, NULL)"),
                    params![mark, doc.id, from_rel, to_rel, quote, kept(link), kept(label), hand.name, hand.principal, now_ms()],
                )?;
                let row = conn.query_row(&format!("SELECT {COLUMNS} FROM note_marks WHERE mark = ?1"), [&mark], row_of)?;
                Ok((dressed(&crdt, row), doc.org))
            })
            .await?;
        self.said(Change::Marked { org, mark: mark.clone() });
        Ok(mark)
    }

    /// `notes.mark { mark }`: the mark `mark` of `id` changed as `to` says, by `hand`.
    pub async fn remark(self: &Arc<Self>, id: &str, mark: &str, to: Remark, hand: Hand) -> Result<Mark, StoreError> {
        let (id, mark) = (id.to_string(), mark.to_string());
        let (m, org) = self
            .blocking(move |s| {
                let conn = s.lock();
                let doc = live_doc(&conn, &id)?;
                s.mark_now(&conn, &id, &mark)?;
                let set = |column: &str, value: Option<String>| -> Result<(), StoreError> {
                    if let Some(v) = value {
                        conn.execute(&format!("UPDATE note_marks SET {column} = ?2 WHERE mark = ?1"), params![mark, kept(Some(v))])?;
                    }
                    Ok(())
                };
                set("link", to.link)?;
                set("label", to.label)?;
                conn.execute("UPDATE note_marks SET changed_at = ?2, changed_by = ?3 WHERE mark = ?1", params![mark, now_ms(), hand.name])?;
                Ok((s.mark_now(&conn, &id, &mark)?, doc.org))
            })
            .await?;
        self.said(Change::Marked { org, mark: m.clone() });
        Ok(m)
    }

    /// `notes.unmark`: the mark `mark` taken off `id`.
    pub async fn unmark(self: &Arc<Self>, id: &str, mark: &str) -> Result<(), StoreError> {
        let (id, mark) = (id.to_string(), mark.to_string());
        let note = id.clone();
        let (gone, org) = self
            .blocking(move |s| {
                let conn = s.lock();
                let doc = live_doc(&conn, &id)?;
                match conn.execute("DELETE FROM note_marks WHERE mark = ?1 AND id = ?2", params![mark, id])? {
                    0 => Err(StoreError::NoMark(mark)),
                    _ => Ok((mark, doc.org)),
                }
            })
            .await?;
        self.said(Change::Unmarked { id: note, org, mark: gone });
        Ok(())
    }

    /// `notes.marks`: `id`'s marks, by where they stand in the text — the
    /// detached ones at the end, each by when it was made.
    pub async fn marks(self: &Arc<Self>, id: &str) -> Result<Vec<Mark>, StoreError> {
        let id = id.to_string();
        self.blocking(move |s| {
            let conn = s.lock();
            live_doc(&conn, &id)?;
            let rows = {
                let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM note_marks WHERE id = ?1 ORDER BY at"))?;
                let rows = stmt.query_map([&id], row_of)?.collect::<Result<Vec<_>, _>>()?;
                rows
            };
            let crdt = s.crdt(&conn, &id)?;
            let crdt = crdt.lock().unwrap_or_else(|p| p.into_inner());
            let mut out: Vec<Mark> = rows.into_iter().map(|r| dressed(&crdt, r)).collect();
            out.sort_by_key(|m| (m.detached, m.from.unwrap_or(u32::MAX), m.at));
            Ok(out)
        })
        .await
    }
}
