//! **The body's write** — a document's body is a Yjs CRDT (V2-29's
//! `pairing`): every writer's change is an update, merged by the CRDT, so
//! two hands in one paragraph lose nothing. This module is the one that
//! writes a body, replays it and squashes it.
//!
//! - **An update** (`docs.update`, the editor's): the Yjs v1 bytes the
//!   browser's `yjs` made, applied to the document's CRDT, appended to the
//!   log with who and when, said on the channel for every open watch.
//! - **A whole body** (`docs.save`, an agent's over synapse or the API): a
//!   diff of the text now against the body given, made into the CRDT's own
//!   inserts and deletes — an update like any other, so it merges with live
//!   typing instead of overwriting it.
//! - **Sync** (`docs.state`): what a client lacks, by its state vector.
//! - **Replay** (`docs.replay`): the log applied up to a moment, to a fresh
//!   CRDT — the document as it stood then.
//! - **Squash** (`docs.squash`): the log up to a moment collapsed into one
//!   snapshot row, who wrote what kept as its `authors`.
//!
//! The markdown is derived from the CRDT on every write (`docs.text`), so a
//! read never touches the CRDT. A loaded CRDT is kept in memory; a
//! snapshot of it is written every [`SNAPSHOT_EVERY`] changes so a load
//! applies only what came after. A write that fails after the CRDT took it
//! drops the loaded copy, so the next one loads the file's.

use crate::events::{Author, Doc, HistoryRow};
use crate::quota::Reserved;
use crate::store::{live_doc, log, now, now_ms, Change, DocsStore, LogRow, StoreError};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{GetString, OffsetKind, Options, ReadTxn, StateVector, Text, Transact, Update};

/// The CRDT's one text: the body (the browser's `ydoc.getText("body")`).
pub const BODY: &str = "body";

/// A snapshot of a loaded CRDT is kept every this many changes.
pub const SNAPSHOT_EVERY: u32 = 64;

/// A fresh CRDT: byte offsets, a random client id.
fn fresh() -> yrs::Doc {
    yrs::Doc::with_options(Options { offset_kind: OffsetKind::Bytes, ..Options::default() })
}

pub(crate) fn text_of(doc: &yrs::Doc) -> String {
    let body = doc.get_or_insert_text(BODY);
    let txn = doc.transact();
    body.get_string(&txn)
}

fn apply(doc: &yrs::Doc, bytes: &[u8]) -> Result<(), StoreError> {
    let update = Update::decode_v1(bytes).map_err(|e| StoreError::BadUpdate(e.to_string()))?;
    let mut txn = doc.transact_mut();
    txn.apply_update(update).map_err(|e| StoreError::BadUpdate(e.to_string()))
}

pub(crate) fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Base64 to bytes, or why not.
pub fn unb64(text: &str) -> Result<Vec<u8>, StoreError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(text.trim()).map_err(|e| StoreError::BadUpdate(format!("not base64: {e}")))
}

/// The edits that make `old` into `new`, as the CRDT's inserts and removes
/// on `body` in `txn` — a diff by characters, applied last first so every
/// byte offset stands.
fn rewrite(body: &yrs::TextRef, txn: &mut yrs::TransactionMut, old: &str, new: &str) {
    let diff = similar::TextDiff::configure().timeout(std::time::Duration::from_millis(250)).diff_chars(old, new);
    let byte_at: Vec<usize> = old.char_indices().map(|(i, _)| i).chain(std::iter::once(old.len())).collect();
    let new_chars: Vec<char> = new.chars().collect();
    let mut ops: Vec<similar::DiffOp> = diff.ops().to_vec();
    ops.reverse();
    for op in ops {
        match op {
            similar::DiffOp::Equal { .. } => {}
            similar::DiffOp::Delete { old_index, old_len, .. } => {
                body.remove_range(txn, byte_at[old_index] as u32, (byte_at[old_index + old_len] - byte_at[old_index]) as u32);
            }
            similar::DiffOp::Insert { old_index, new_index, new_len } => {
                let s: String = new_chars[new_index..new_index + new_len].iter().collect();
                body.insert(txn, byte_at[old_index] as u32, &s);
            }
            similar::DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                body.remove_range(txn, byte_at[old_index] as u32, (byte_at[old_index + old_len] - byte_at[old_index]) as u32);
                let s: String = new_chars[new_index..new_index + new_len].iter().collect();
                body.insert(txn, byte_at[old_index] as u32, &s);
            }
        }
    }
}

/// What a write of the body did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The document after it.
    pub doc: Doc,
    /// The change's `seq` — `None` when the write changed nothing (no row).
    pub seq: Option<u32>,
    /// Whether the writer's `base` was not the revision it wrote over.
    pub stale: bool,
}

/// What a client lacks, and where the document stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Synced {
    /// The Yjs v1 update holding what the client's state vector lacks (base64).
    pub update: String,
    /// The document's state vector (base64), for the client's own diff back.
    pub vector: String,
    pub revision: u32,
}

/// `id`'s log applied to a fresh CRDT up to change `seq` (or the moment
/// `at`, unix milliseconds): the CRDT, the title then, and the change it
/// stands at, when and by whom. A moment before every change kept, or a
/// `seq` squashed away, is [`StoreError::NoRevision`].
fn replayed(conn: &Connection, id: &str, title_now: &str, seq: Option<u32>, at: Option<i64>) -> Result<(yrs::Doc, String, u32, i64, String), StoreError> {
    let mut stmt = conn.prepare("SELECT seq, kind, update_v1, title, at, by FROM doc_log WHERE id = ?1 ORDER BY seq")?;
    let rows = stmt.query_map([id], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<Vec<u8>>>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, i64>(4)?, r.get::<_, String>(5)?)))?;
    let crdt = fresh();
    let mut last: Option<(u32, i64, String)> = None;
    let mut title = title_now.to_string();
    for row in rows {
        let (n, kind, update, t, when, by) = row?;
        if seq.is_some_and(|s| n > s) || at.is_some_and(|a| when > a) {
            break;
        }
        if kind == "delete" {
            continue;
        }
        if let Some(u) = update {
            apply(&crdt, &u)?;
        }
        if let Some(t) = t {
            title = t;
        }
        last = Some((n, when, by));
    }
    let missing = || StoreError::NoRevision(id.to_string(), seq.unwrap_or_default());
    let (n, when, by) = last.ok_or_else(missing)?;
    if seq.is_some_and(|s| s != n) && at.is_none() {
        // A seq squashed away: the snapshot after it is the first moment kept.
        return Err(missing());
    }
    Ok((crdt, title, n, when, by))
}

impl DocsStore {
    /// The loaded CRDT of `id`, loading it from the file when it is not:
    /// its snapshot, then every change after it.
    pub(crate) fn crdt(&self, conn: &Connection, id: &str) -> Result<Arc<Mutex<yrs::Doc>>, StoreError> {
        if let Some(d) = self.loaded.lock().unwrap_or_else(|p| p.into_inner()).get(id) {
            return Ok(Arc::clone(d));
        }
        let (state, state_seq): (Vec<u8>, u32) = conn
            .query_row("SELECT state, state_seq FROM docs WHERE id = ?1 AND deleted_at IS NULL", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        let doc = fresh();
        apply(&doc, &state)?;
        let mut stmt = conn.prepare("SELECT update_v1 FROM doc_log WHERE id = ?1 AND seq > ?2 AND update_v1 IS NOT NULL ORDER BY seq")?;
        for u in stmt.query_map(params![id, state_seq], |r| r.get::<_, Vec<u8>>(0))? {
            apply(&doc, &u?)?;
        }
        let doc = Arc::new(Mutex::new(doc));
        self.loaded.lock().unwrap_or_else(|p| p.into_inner()).insert(id.to_string(), Arc::clone(&doc));
        Ok(doc)
    }

    fn forget(&self, id: &str) {
        self.loaded.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
    }

    /// Makes a document in `org`, titled `title`, its body `body`, by `by`
    /// (`principal`): change 1, its id fresh ([`crate::vocab::mint`]).
    pub async fn create(self: &Arc<Self>, org: &str, title: &str, body: &str, by: &str, principal: &str) -> Result<Doc, StoreError> {
        let (org, title, body, by, principal) = (org.to_string(), title.to_string(), body.to_string(), by.to_string(), principal.to_string());
        let doc = self
            .blocking(move |s| {
                let quota = s.quota();
                let born = quota.reserve_count(&principal).map_err(StoreError::Quota)?;
                let crdt = fresh();
                let update = {
                    let text = crdt.get_or_insert_text(BODY);
                    let mut txn = crdt.transact_mut();
                    text.insert(&mut txn, 0, &body);
                    txn.encode_update_v1()
                };
                // V2-31: the bytes the change keeps (its update), as every later change counts.
                let reserved = quota.reserve(&principal, update.len() as u64).map_err(StoreError::Quota)?;
                let state = crdt.transact().encode_state_as_update_v1(&StateVector::default());
                let conn = s.lock();
                let tx = conn.unchecked_transaction()?;
                let id = crate::vocab::mint(|id| tx.query_row("SELECT 1 FROM docs WHERE id = ?1", [id], |_| Ok(())).optional().ok().flatten().is_some());
                let at = now();
                let doc = Doc { id, org, title, bytes: body.len() as u64, body: Some(body), revision: 1, created_at: at, created_by: by.clone(), at, by };
                tx.execute(
                    "INSERT INTO docs (id, org, title, text, revision, created_at, created_by, at, by, state, state_seq) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?5, ?6, ?7, 1)",
                    params![doc.id, doc.org, doc.title, doc.body.as_deref().unwrap_or_default(), doc.at, doc.by, state],
                )?;
                log(&tx, LogRow { id: &doc.id, seq: 1, kind: "create", update: Some(&update), title: Some(&doc.title), at: now_ms(), by: &doc.by, principal: &principal, authors: None })?;
                tx.commit()?;
                quota.commit(born);
                quota.commit(reserved);
                s.loaded.lock().unwrap_or_else(|p| p.into_inner()).insert(doc.id.clone(), Arc::new(Mutex::new(crdt)));
                Ok(doc)
            })
            .await?;
        self.said(Change::Created(doc.clone()));
        Ok(doc)
    }

    /// Appends one change made to the loaded CRDT of `id` — its update
    /// `bytes`, applied already — with who and when; writes the derived
    /// text, and a snapshot every [`SNAPSHOT_EVERY`]. The quota's one call
    /// site: the update's size reserved for `principal` before, committed
    /// after.
    fn append(&self, conn: &Connection, id: &str, crdt: &yrs::Doc, bytes: &[u8], kind: &str, by: &str, principal: &str) -> Result<(Doc, u32), StoreError> {
        let quota = self.quota();
        let reserved: Reserved = quota.reserve(principal, bytes.len() as u64).map_err(StoreError::Quota)?;
        let tx = conn.unchecked_transaction()?;
        let was = live_doc(&tx, id)?;
        let text = text_of(crdt);
        let seq = was.revision + 1;
        let doc = Doc { bytes: text.len() as u64, body: Some(text), revision: seq, at: now(), by: by.to_string(), ..was };
        tx.execute("UPDATE docs SET text = ?2, revision = ?3, at = ?4, by = ?5 WHERE id = ?1", params![doc.id, doc.body.as_deref().unwrap_or_default(), seq, doc.at, doc.by])?;
        log(&tx, LogRow { id, seq, kind, update: Some(bytes), title: None, at: now_ms(), by, principal, authors: None })?;
        let state_seq: u32 = tx.query_row("SELECT state_seq FROM docs WHERE id = ?1", [id], |r| r.get(0))?;
        if seq - state_seq >= SNAPSHOT_EVERY {
            let state = crdt.transact().encode_state_as_update_v1(&StateVector::default());
            tx.execute("UPDATE docs SET state = ?2, state_seq = ?3 WHERE id = ?1", params![id, state, seq])?;
        }
        tx.commit()?;
        quota.commit(reserved);
        Ok((doc, seq))
    }

    /// `docs.update`: a writer's Yjs v1 update (`update`, base64) applied to
    /// `id`'s CRDT and kept, by `by` (`principal`), from the presence
    /// `session` when it says.
    pub async fn update(self: &Arc<Self>, id: &str, update: &str, by: &str, principal: &str, session: Option<String>) -> Result<Written, StoreError> {
        let bytes = unb64(update)?;
        let (id, by, principal) = (id.to_string(), by.to_string(), principal.to_string());
        let said_update = update.trim().to_string();
        let w = self
            .blocking(move |s| {
                let conn = s.lock();
                let crdt = s.crdt(&conn, &id)?;
                let crdt = crdt.lock().unwrap_or_else(|p| p.into_inner());
                let written = apply(&crdt, &bytes).and_then(|()| s.append(&conn, &id, &crdt, &bytes, "update", &by, &principal));
                match written {
                    Ok((doc, seq)) => Ok(Written { doc, seq: Some(seq), stale: false }),
                    Err(e) => {
                        drop(crdt);
                        s.forget(&id);
                        Err(e)
                    }
                }
            })
            .await?;
        self.said(Change::Updated { doc: w.doc.clone(), update: said_update, seq: w.seq.unwrap_or_default(), session });
        Ok(w)
    }

    /// `docs.save`: `body` whole, by `by` — made into the CRDT's own edits by
    /// a diff of the text its writer read (`base`, the revision it read)
    /// against `body`, made on the CRDT as it stood then and merged into it
    /// now: what others wrote since stays. With no `base` (or one squashed
    /// away) the diff is against the text now. A body equal to the text
    /// writes nothing.
    pub async fn save(self: &Arc<Self>, id: &str, body: &str, base: Option<u32>, by: &str, principal: &str) -> Result<Written, StoreError> {
        let (id, body, by, principal) = (id.to_string(), body.to_string(), by.to_string(), principal.to_string());
        let (w, update) = self
            .blocking(move |s| {
                let conn = s.lock();
                let crdt = s.crdt(&conn, &id)?;
                let crdt = crdt.lock().unwrap_or_else(|p| p.into_inner());
                let was = live_doc(&conn, &id)?;
                let stale = base.is_some_and(|b| b != was.revision);
                if text_of(&crdt) == body {
                    return Ok((Written { doc: was, seq: None, stale }, None));
                }
                // The edits are made where the writer read: on the CRDT as it
                // stood at `base` when it is an earlier one still kept (so what
                // others wrote since is merged, not undone), else on it now.
                let at_base = base.filter(|_| stale).and_then(|b| replayed(&conn, &id, &was.title, Some(b), None).ok()).map(|(d, ..)| d);
                let update = {
                    let on = at_base.as_ref().unwrap_or(&*crdt);
                    let old = text_of(on);
                    let text = on.get_or_insert_text(BODY);
                    let mut txn = on.transact_mut();
                    rewrite(&text, &mut txn, &old, &body);
                    txn.encode_update_v1()
                };
                if at_base.is_some() {
                    if let Err(e) = apply(&crdt, &update) {
                        drop(crdt);
                        s.forget(&id);
                        return Err(e);
                    }
                }
                match s.append(&conn, &id, &crdt, &update, "save", &by, &principal) {
                    Ok((doc, seq)) => Ok((Written { doc, seq: Some(seq), stale }, Some(update))),
                    Err(e) => {
                        drop(crdt);
                        s.forget(&id);
                        Err(e)
                    }
                }
            })
            .await?;
        if let (Some(update), Some(seq)) = (update, w.seq) {
            self.said(Change::Updated { doc: w.doc.clone(), update: b64(&update), seq, session: None });
        }
        Ok(w)
    }

    /// `docs.state`: what a client whose state vector is `vector` (base64;
    /// none: it holds nothing) lacks of `id`, and the document's own vector.
    pub async fn state(self: &Arc<Self>, id: &str, vector: Option<String>) -> Result<Synced, StoreError> {
        let id = id.to_string();
        self.blocking(move |s| {
            let sv = match vector.as_deref().map(unb64).transpose()? {
                Some(b) => StateVector::decode_v1(&b).map_err(|e| StoreError::BadUpdate(e.to_string()))?,
                None => StateVector::default(),
            };
            let conn = s.lock();
            let revision = live_doc(&conn, &id)?.revision;
            let crdt = s.crdt(&conn, &id)?;
            let crdt = crdt.lock().unwrap_or_else(|p| p.into_inner());
            let txn = crdt.transact();
            Ok(Synced { update: b64(&txn.encode_state_as_update_v1(&sv)), vector: b64(&txn.state_vector().encode_v1()), revision })
        })
        .await
    }

    /// `docs.replay`: `id` as it stood after change `seq` — or, with `at`
    /// (unix milliseconds), after the last change made by then: the log to
    /// there applied to a fresh CRDT, the title as it was. A moment before
    /// the first change kept (or before a squash's snapshot) is
    /// [`StoreError::NoRevision`].
    pub async fn replay(self: &Arc<Self>, id: &str, seq: Option<u32>, at: Option<i64>) -> Result<Doc, StoreError> {
        let id = id.to_string();
        self.blocking(move |s| {
            let conn = s.lock();
            let now = live_doc(&conn, &id)?;
            let (crdt, title, n, when, by) = replayed(&conn, &id, &now.title, seq, at)?;
            let text = text_of(&crdt);
            Ok(Doc { title, bytes: text.len() as u64, body: Some(text), revision: n, at: when / 1000, by, ..now })
        })
        .await
    }

    /// `docs.restore`: `id`'s body put back as it stood after change `seq`,
    /// by `by` — a whole-body write over the CRDT ([`DocsStore::save`]), so a
    /// restore is a change like any other, merged with live typing.
    pub async fn restore(self: &Arc<Self>, id: &str, seq: u32, by: &str, principal: &str) -> Result<Written, StoreError> {
        let then = self.replay(id, Some(seq), None).await?;
        self.save(id, then.body.as_deref().unwrap_or_default(), None, by, principal).await
    }

    /// `docs.history`: `id`'s changes, newest first, at most `limit`, those
    /// before `before` (a `seq`) when it says.
    pub async fn history(self: &Arc<Self>, id: &str, limit: usize, before: Option<u32>) -> Result<Vec<HistoryRow>, StoreError> {
        let id = id.to_string();
        self.blocking(move |s| {
            let conn = s.lock();
            live_doc(&conn, &id)?;
            let mut stmt = conn.prepare("SELECT seq, kind, at, by, principal, bytes, title, authors FROM doc_log WHERE id = ?1 AND seq < ?2 ORDER BY seq DESC LIMIT ?3")?;
            let rows = stmt
                .query_map(params![id, before.map_or(i64::MAX, i64::from), limit as i64], |r| {
                    let authors: Option<String> = r.get(7)?;
                    Ok(HistoryRow {
                        id: id.clone(),
                        seq: r.get(0)?,
                        kind: r.get(1)?,
                        at: r.get(2)?,
                        by: r.get(3)?,
                        principal: r.get(4)?,
                        bytes: r.get::<_, i64>(5)? as u64,
                        title: r.get(6)?,
                        authors: authors.and_then(|a| serde_json::from_str(&a).ok()),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }

    /// `docs.squash`: `id`'s changes up to `until` (a `seq`) collapsed into
    /// one `snapshot` row at `until` — the CRDT as it stood then — its
    /// `authors` saying who wrote what in them (an earlier snapshot's authors
    /// folded in). Answers how many rows went. Nothing after `until` moves.
    pub async fn squash(self: &Arc<Self>, id: &str, until: u32, by: &str, principal: &str) -> Result<u32, StoreError> {
        let (id, by, principal) = (id.to_string(), by.to_string(), principal.to_string());
        let squasher = by.clone();
        let then = self.replay(&id, Some(until), None).await?;
        let (org, removed) = self
            .blocking(move |s| {
                let conn = s.lock();
                let tx = conn.unchecked_transaction()?;
                let doc = live_doc(&tx, &id)?;
                let rows: Vec<(u32, String, Option<Vec<u8>>, i64, String, String, i64, Option<String>)> = {
                    let mut stmt = tx.prepare("SELECT seq, kind, update_v1, at, by, principal, bytes, authors FROM doc_log WHERE id = ?1 AND seq <= ?2 ORDER BY seq")?;
                    let rows = stmt
                        .query_map(params![id, until], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))?
                        .collect::<Result<Vec<_>, _>>()?;
                    rows
                };
                if rows.len() < 2 {
                    return Ok((doc.org, 0));
                }
                let crdt = fresh();
                let mut authors: Vec<Author> = Vec::new();
                let mut add = |a: Author| match authors.iter_mut().find(|x| x.principal == a.principal) {
                    Some(x) => {
                        x.changes += a.changes;
                        x.bytes += a.bytes;
                        x.first = x.first.min(a.first);
                        x.last = x.last.max(a.last);
                    }
                    None => authors.push(a),
                };
                let mut last_at = 0;
                for (_, kind, update, at, who, p, bytes, folded) in &rows {
                    if let Some(u) = update {
                        apply(&crdt, u)?;
                    }
                    last_at = *at;
                    match (kind.as_str(), folded) {
                        ("snapshot", Some(f)) => {
                            for a in serde_json::from_str::<Vec<Author>>(f).unwrap_or_default() {
                                add(a);
                            }
                        }
                        _ => add(Author { by: who.clone(), principal: p.clone(), changes: 1, bytes: *bytes as u64, first: *at, last: *at }),
                    }
                }
                let state = crdt.transact().encode_state_as_update_v1(&StateVector::default());
                let authors = serde_json::to_string(&authors).map_err(|e| StoreError::Db(e.to_string()))?;
                tx.execute("DELETE FROM doc_log WHERE id = ?1 AND seq <= ?2", params![id, until])?;
                log(&tx, LogRow { id: &id, seq: until, kind: "snapshot", update: Some(&state), title: Some(&then.title), at: last_at, by: &by, principal: &principal, authors: Some(&authors) })?;
                tx.commit()?;
                Ok((doc.org, rows.len() as u32 - 1))
            })
            .await?;
        if removed > 0 {
            self.said(Change::Squashed { id: then.id.clone(), org, until, removed, at: now(), by: squasher });
        }
        Ok(removed)
    }
}
