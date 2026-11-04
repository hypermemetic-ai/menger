//! V2-30: **asking on a selection** — a question anchored to a range of a
//! document, answered in a thread beside it by a person or an agent (the
//! owner, 2026-11-02: *"I want to have the ability to highlight a line in a
//! doc and ask a question … these docs are really important and will be key
//! locations for paired work."*).
//!
//! **The anchor follows the text.** A thread's range is two Yjs relative
//! positions in the document's CRDT ([`yrs::StickyIndex`], the browser's
//! `Y.RelativePosition` byte for byte): the start sticks to the character
//! after it, the end to the character before it, so every hand's edit —
//! around the range, inside it — moves them with the text. Resolved now,
//! they are the range's offsets in characters (`from`, `to`); a range whose
//! text was deleted whole resolves to nothing and the thread is
//! **detached** (kept, listed at the end). The browser resolves the same
//! positions in its own copy (`anchor`, base64), so the margin is exact.
//!
//! **Stored** in the docs file: `doc_threads` (a row per question: its
//! anchor, the text it quoted, the question, to whom, who and when, and
//! when it was resolved) and `doc_replies` (a row per answer). Said on the
//! channel for `watch`: `asked`, `replied`, `resolved`, each with the
//! thread whole.

use crate::crdt::{b64, text_of, unb64, BODY};
use crate::events::{Anchor, Reply, Resolution, Thread};
use crate::store::{live_doc, now_ms, Change, DocsStore, StoreError};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Arc;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Assoc, IndexedSequence, StickyIndex, Transact};

/// The tables.
pub const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS doc_threads (
            thread       TEXT PRIMARY KEY,
            id           TEXT NOT NULL,
            from_rel     BLOB NOT NULL,
            to_rel       BLOB NOT NULL,
            quote        TEXT NOT NULL,
            question     TEXT NOT NULL,
            to_whom      TEXT,
            by           TEXT NOT NULL,
            principal    TEXT NOT NULL,
            at           INTEGER NOT NULL,
            resolved_at  INTEGER,
            resolved_by  TEXT
        );
        CREATE INDEX IF NOT EXISTS doc_threads_id ON doc_threads (id);
        CREATE TABLE IF NOT EXISTS doc_replies (
            thread     TEXT NOT NULL,
            n          INTEGER NOT NULL,
            body       TEXT NOT NULL,
            by         TEXT NOT NULL,
            principal  TEXT NOT NULL,
            at         INTEGER NOT NULL,
            PRIMARY KEY (thread, n)
        );
    ";

/// Who asked or answered.
#[derive(Debug, Clone)]
pub struct Hand {
    pub name: String,
    pub principal: String,
}

/// The byte offset of character `chars` in `text` (its end past the last).
fn byte_at(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(i, _)| i)
}

/// The character offset of byte `bytes` in `text`.
fn chars_at(text: &str, bytes: usize) -> usize {
    text[..bytes.min(text.len())].chars().count()
}

/// What the stored row of a thread holds before its anchor is resolved.
struct Row {
    thread: String,
    id: String,
    from_rel: Vec<u8>,
    to_rel: Vec<u8>,
    quote: String,
    question: String,
    to_whom: Option<String>,
    by: String,
    principal: String,
    at: i64,
    resolved: Option<Resolution>,
}

const COLUMNS: &str = "thread, id, from_rel, to_rel, quote, question, to_whom, by, principal, at, resolved_at, resolved_by";

fn row_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    let resolved_at: Option<i64> = r.get(10)?;
    let resolved_by: Option<String> = r.get(11)?;
    Ok(Row {
        thread: r.get(0)?,
        id: r.get(1)?,
        from_rel: r.get(2)?,
        to_rel: r.get(3)?,
        quote: r.get(4)?,
        question: r.get(5)?,
        to_whom: r.get(6)?,
        by: r.get(7)?,
        principal: r.get(8)?,
        at: r.get(9)?,
        resolved: resolved_at.map(|at| Resolution { at, by: resolved_by.unwrap_or_default() }),
    })
}

impl DocsStore {
    /// `row` made a [`Thread`]: its anchor resolved in `crdt` now, its replies read.
    fn dressed(&self, conn: &Connection, crdt: &yrs::Doc, row: Row) -> Result<Thread, StoreError> {
        let text = text_of(crdt);
        let resolve = |bytes: &[u8]| -> Option<usize> {
            let sticky = StickyIndex::decode_v1(bytes).ok()?;
            let txn = crdt.transact();
            sticky.get_offset(&txn).map(|o| chars_at(&text, o.index as usize))
        };
        let (from, to) = (resolve(&row.from_rel), resolve(&row.to_rel));
        let (from, to, detached) = match (from, to) {
            (Some(f), Some(t)) if t > f || row.quote.is_empty() => (Some(f as u32), Some(t as u32), false),
            _ => (None, None, true),
        };
        let mut stmt = conn.prepare("SELECT n, body, by, principal, at FROM doc_replies WHERE thread = ?1 ORDER BY n")?;
        let replies = stmt
            .query_map([&row.thread], |r| Ok(Reply { n: r.get(0)?, body: r.get(1)?, by: r.get(2)?, principal: r.get(3)?, at: r.get(4)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Thread {
            thread: row.thread,
            id: row.id,
            anchor: Anchor { from: b64(&row.from_rel), to: b64(&row.to_rel) },
            from,
            to,
            detached,
            quote: row.quote,
            question: row.question,
            to_whom: row.to_whom,
            by: row.by,
            principal: row.principal,
            at: row.at,
            resolved: row.resolved,
            replies,
        })
    }

    /// The thread `thread` of the live document `id`, dressed.
    fn thread_now(&self, conn: &Connection, id: &str, thread: &str) -> Result<Thread, StoreError> {
        let row = conn
            .query_row(&format!("SELECT {COLUMNS} FROM doc_threads WHERE thread = ?1 AND id = ?2"), params![thread, id], row_of)
            .optional()?
            .ok_or_else(|| StoreError::NoThread(thread.to_string()))?;
        let crdt = self.crdt(conn, id)?;
        let crdt = crdt.lock().unwrap_or_else(|p| p.into_inner());
        self.dressed(conn, &crdt, row)
    }

    /// `docs.ask`: a question on the characters `from..to` of `id`'s text
    /// now, anchored in its CRDT, to `to_whom` when it says, by `hand`.
    pub async fn ask(self: &Arc<Self>, id: &str, from: u32, to: u32, question: &str, to_whom: Option<String>, hand: Hand) -> Result<Thread, StoreError> {
        let (id, question) = (id.to_string(), question.to_string());
        let thread = self
            .blocking(move |s| {
                // V2-39: one question held for the asker before it is kept,
                // committed once it is, released if it is not.
                let quota = s.quota();
                let held = quota.reserve_question(&hand.principal).map_err(StoreError::Allowance)?;
                let kept = (|| {
                    let conn = s.lock();
                    let doc = live_doc(&conn, &id)?;
                    let crdt = s.crdt(&conn, &id)?;
                    let crdt = crdt.lock().unwrap_or_else(|p| p.into_inner());
                    let text = text_of(&crdt);
                    let n = text.chars().count() as u32;
                    let (from, to) = (from.min(n), to.min(n));
                    if from > to {
                        return Err(StoreError::BadRange(format!("from {from} is past to {to}")));
                    }
                    let (fb, tb) = (byte_at(&text, from as usize), byte_at(&text, to as usize));
                    let body = crdt.get_or_insert_text(BODY);
                    let (from_rel, to_rel) = {
                        let txn = crdt.transact();
                        let at = |b: usize, assoc| body.sticky_index(&txn, b as u32, assoc).map(|s| s.encode_v1());
                        (at(fb, Assoc::After), at(tb, Assoc::Before))
                    };
                    let (from_rel, to_rel) = from_rel.zip(to_rel).ok_or_else(|| StoreError::BadRange(format!("{from}..{to} is not in the text")))?;
                    let thread = crate::vocab::mint(|t| conn.query_row("SELECT 1 FROM doc_threads WHERE thread = ?1", [t], |_| Ok(())).optional().ok().flatten().is_some());
                    conn.execute(
                        &format!("INSERT INTO doc_threads ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, NULL)"),
                        params![thread, doc.id, from_rel, to_rel, text[fb..tb].to_string(), question, to_whom, hand.name, hand.principal, now_ms()],
                    )?;
                    let row = conn.query_row(&format!("SELECT {COLUMNS} FROM doc_threads WHERE thread = ?1"), [&thread], row_of)?;
                    Ok((s.dressed(&conn, &crdt, row)?, doc.org))
                })();
                match kept {
                    Ok(k) => {
                        quota.commit(held);
                        Ok(k)
                    }
                    Err(e) => {
                        quota.release(held);
                        Err(e)
                    }
                }
            })
            .await?;
        self.said(Change::Asked { org: thread.1, thread: thread.0.clone() });
        Ok(thread.0)
    }

    /// `docs.reply`: `body` under the thread, by `hand`.
    pub async fn reply(self: &Arc<Self>, id: &str, thread: &str, body: &str, hand: Hand) -> Result<Thread, StoreError> {
        let (id, thread, body) = (id.to_string(), thread.to_string(), body.to_string());
        let (t, org) = self
            .blocking(move |s| {
                let conn = s.lock();
                let doc = live_doc(&conn, &id)?;
                s.thread_now(&conn, &id, &thread)?;
                let n: u32 = conn.query_row("SELECT coalesce(max(n), 0) + 1 FROM doc_replies WHERE thread = ?1", [&thread], |r| r.get(0))?;
                conn.execute("INSERT INTO doc_replies (thread, n, body, by, principal, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![thread, n, body, hand.name, hand.principal, now_ms()])?;
                Ok((s.thread_now(&conn, &id, &thread)?, doc.org))
            })
            .await?;
        self.said(Change::Replied { org, thread: t.clone() });
        Ok(t)
    }

    /// `docs.resolve`: the thread folded, by `hand` (again: it opens).
    pub async fn resolve(self: &Arc<Self>, id: &str, thread: &str, open: bool, hand: Hand) -> Result<Thread, StoreError> {
        let (id, thread) = (id.to_string(), thread.to_string());
        let (t, org) = self
            .blocking(move |s| {
                let conn = s.lock();
                let doc = live_doc(&conn, &id)?;
                s.thread_now(&conn, &id, &thread)?;
                match open {
                    false => conn.execute("UPDATE doc_threads SET resolved_at = ?2, resolved_by = ?3 WHERE thread = ?1", params![thread, now_ms(), hand.name])?,
                    true => conn.execute("UPDATE doc_threads SET resolved_at = NULL, resolved_by = NULL WHERE thread = ?1", [&thread])?,
                };
                Ok((s.thread_now(&conn, &id, &thread)?, doc.org))
            })
            .await?;
        self.said(Change::Resolved { org, thread: t.clone() });
        Ok(t)
    }

    /// `docs.threads`: `id`'s threads, by where they stand in the text —
    /// the detached ones at the end, each by when it was asked.
    pub async fn threads(self: &Arc<Self>, id: &str) -> Result<Vec<Thread>, StoreError> {
        let id = id.to_string();
        self.blocking(move |s| {
            let conn = s.lock();
            live_doc(&conn, &id)?;
            let rows = {
                let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM doc_threads WHERE id = ?1 ORDER BY at"))?;
                let rows = stmt.query_map([&id], row_of)?.collect::<Result<Vec<_>, _>>()?;
                rows
            };
            let crdt = s.crdt(&conn, &id)?;
            let crdt = crdt.lock().unwrap_or_else(|p| p.into_inner());
            let mut out = rows.into_iter().map(|r| s.dressed(&conn, &crdt, r)).collect::<Result<Vec<_>, _>>()?;
            out.sort_by_key(|t| (t.detached, t.from.unwrap_or(u32::MAX), t.at));
            Ok(out)
        })
        .await
    }

    /// `docs.waiting`: the open threads asked of someone (`to_whom`) in the
    /// documents of `orgs`, oldest first — `to` keeping those asked of it.
    pub async fn waiting(self: &Arc<Self>, orgs: Vec<String>, to: Option<String>) -> Result<Vec<Thread>, StoreError> {
        self.blocking(move |s| {
            let conn = s.lock();
            if orgs.is_empty() {
                return Ok(Vec::new());
            }
            let sql = format!(
                "SELECT t.{} FROM doc_threads t JOIN docs d ON d.id = t.id WHERE d.deleted_at IS NULL AND t.resolved_at IS NULL AND t.to_whom IS NOT NULL AND d.org IN ({}) ORDER BY t.at",
                COLUMNS.replace(", ", ", t."),
                vec!["?"; orgs.len()].join(", ")
            );
            let rows = {
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(rusqlite::params_from_iter(orgs.iter()), row_of)?.collect::<Result<Vec<_>, _>>()?;
                rows
            };
            let mut out = Vec::new();
            for r in rows.into_iter().filter(|r| to.as_deref().is_none_or(|w| r.to_whom.as_deref() == Some(w))) {
                let crdt = s.crdt(&conn, &r.id)?;
                let crdt = crdt.lock().unwrap_or_else(|p| p.into_inner());
                out.push(s.dressed(&conn, &crdt, r)?);
            }
            Ok(out)
        })
        .await
    }
}

/// A thread's anchor from base64 — for a client resolving it itself.
pub fn anchor_bytes(text: &str) -> Result<Vec<u8>, StoreError> {
    unb64(text)
}
