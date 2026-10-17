//! TICKETS-7: **every change keeps a revision.** Owner, 2026-10-13: *"are
//! we saving update history for these tickets? we should be. and I should
//! be able to see the diffs and open the history and restore (or just open
//! to view)."*
//!
//! **Kept by the write itself, whatever the write.** A ticket's row moves
//! through three functions of `db` alone — `upsert` (a birth, an undelete, a
//! reshortcode's new key), `update_row` (every write after birth) and
//! `set_body` — and a delete's `DELETE`. Each tells this module first
//! ([`touch`], [`gone`]), inside the write's transaction, which a write runner
//! ([`Store::write`], [`Store::in_one_write`]) has opened a [`Writing`] for.
//! Before it commits, the write [`flush`]es: each ticket it touched is read
//! back as the write leaves it and compared, field by field
//! ([`crate::revisions::FIELDS`]), with its latest revision; when anything
//! differs, a revision is kept in the same transaction — the fields as they
//! now are, the body by its hash, who, when, from where, why, and which
//! fields moved. A write that changes nothing of a ticket's own (an archive's
//! mark, a blocker's `unlocks`, a parent's count) keeps none; a write of
//! several steps keeps one per ticket; a cascade one per ticket it moved,
//! each naming its cause (the transition the step recorded, [`said`]).
//! Committed, the write's events go out, then one `revised` per revision
//! ([`publish`]). So no write can forget its revision, and none is kept for
//! a write that rolled back.
//!
//! **Who.** The transition a step made for the ticket, when it made one
//! (its actor, source, message and cause — a cascade's are the store's);
//! else the act the write was handed ([`act`]); else the caller's, which the
//! activation sets around every call ([`acting`], [`say_act`]); else nobody
//! signed in, through the API.
//!
//! **A ticket older than revisions.** Its first revised write keeps two:
//! the ticket as it stood before (`source: baseline`, dated by its last
//! update, by nobody known — "the version before"), then the write's. A
//! write that changes nothing keeps neither, and reads no body.
//!
//! **Stored** in `revisions` (a row per revision: the fields, the body's
//! SHA-256, its size and line counts) and `revision_bodies` (a body's text
//! once per content — a label change costs a row, never another copy of a
//! 4 MB mockup). A file without them gains them, empty, on open
//! ([`migrate`]). Not in the cache: reads of old versions go to storage,
//! off the hot path ([`Store::revisions`], [`Store::revision`],
//! [`Store::revision_diff`]). Bodies are not compressed yet (owner,
//! 2026-10-16: "that can be left for later"). `--verify-derived` holds them
//! to the tickets ([`check`]).

use super::history::Act;
use super::*;
use crate::diff;
use crate::revisions::{FieldChange, Revision, RevisionDiff, RevisionSource, CREATED, DELETED, FIELDS, RESTORABLE, UNDELETED};
use crate::types::Transition;
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::cell::RefCell;

/// The tables.
pub(super) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS revisions (
            ticket     TEXT NOT NULL,
            rev        INTEGER NOT NULL,
            at_unix    INTEGER NOT NULL,
            actor      TEXT NOT NULL,
            name       TEXT,
            source     TEXT NOT NULL,
            message    TEXT,
            cause      TEXT,
            fields     TEXT NOT NULL,
            title      TEXT NOT NULL,
            status     TEXT NOT NULL,
            kind       TEXT NOT NULL,
            severity   TEXT,
            parent     TEXT,
            blocked_by TEXT NOT NULL,
            labels     TEXT NOT NULL,
            commits    TEXT NOT NULL,
            body_hash  TEXT NOT NULL,
            body_bytes INTEGER NOT NULL,
            added      INTEGER NOT NULL,
            removed    INTEGER NOT NULL,
            PRIMARY KEY (ticket, rev)
        );
        CREATE TABLE IF NOT EXISTS revision_bodies (
            hash TEXT PRIMARY KEY,
            body TEXT NOT NULL
        );
    ";

/// A file without the tables gains them, empty, here — on every open a
/// no-op once they are there. Returns whether they were made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    if has_table(conn)? {
        return Ok(false);
    }
    conn.execute_batch(TABLES)?;
    tracing::warn!("TICKETS-7: the file gained the revisions tables, empty; each ticket's first change from now on keeps the version before it too");
    Ok(true)
}

fn has_table(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'revisions'", [], |r| r.get::<_, i64>(0).map(|n| n > 0))
}

/// A body's hash, as kept.
pub(crate) fn hash(body: &str) -> String {
    let d = Sha256::digest(body.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

// ── Who: the caller's act, carried with the call ─────────────────────────

tokio::task_local! {
    static ACTING: RefCell<Act>;
}

/// Runs `f` as `act` — what a revision kept by a write inside it says of
/// who, unless the write says more. The activation wraps every call.
pub async fn acting<F: std::future::Future>(act: Act, f: F) -> F::Output {
    ACTING.scope(RefCell::new(act), f).await
}

/// The call's act from here on — a method that reads its caller's source
/// and message says them (`activation/sight.rs`'s `act_of`). Outside
/// [`acting`], nothing.
pub fn say_act(act: &Act) {
    let _ = ACTING.try_with(|c| *c.borrow_mut() = act.clone());
}

/// The caller's act, when the write runs inside [`acting`].
pub(super) fn caller_act() -> Option<Act> {
    ACTING.try_with(|c| c.borrow().clone()).ok()
}

// ── The write's recording ────────────────────────────────────────────────

/// A ticket's own fields but its body, as a revision keeps them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Fields {
    title: String,
    status: String,
    kind: String,
    severity: Option<String>,
    parent: Option<String>,
    blocked_by: Vec<String>,
    labels: Vec<String>,
    commits: Vec<String>,
}

impl Fields {
    fn of(row: &Row) -> Self {
        Self {
            title: row.title.clone(),
            status: db::enum_str(&row.status),
            kind: db::enum_str(&row.kind),
            severity: row.severity.as_ref().map(db::enum_str),
            parent: row.parent.as_ref().map(|p| p.to_string()),
            blocked_by: row.blocked_by.iter().map(|b| b.to_string()).collect(),
            labels: row.labels.iter().map(|l| l.as_str().to_string()).collect(),
            commits: row.commits.iter().map(|c| c.as_str().to_string()).collect(),
        }
    }

    /// The fields that differ from `before`, in [`FIELDS`]' order (the body
    /// is the caller's).
    fn differ(&self, before: &Fields, body: bool) -> Vec<String> {
        let mut out = Vec::new();
        let mut push = |f: &str, d: bool| {
            if d {
                out.push(f.to_string());
            }
        };
        push("title", self.title != before.title);
        push("body", body);
        push("status", self.status != before.status);
        push("kind", self.kind != before.kind);
        push("severity", self.severity != before.severity);
        push("parent", self.parent != before.parent);
        push("blocked_by", self.blocked_by != before.blocked_by);
        push("labels", self.labels != before.labels);
        push("commits", self.commits != before.commits);
        out
    }

    /// One field's value as text (a list's items joined by `, `).
    fn text(&self, field: &str) -> Option<String> {
        let list = |l: &Vec<String>| (!l.is_empty()).then(|| l.join(", "));
        match field {
            "title" => Some(self.title.clone()),
            "status" => Some(self.status.clone()),
            "kind" => Some(self.kind.clone()),
            "severity" => self.severity.clone(),
            "parent" => self.parent.clone(),
            "blocked_by" => list(&self.blocked_by),
            "labels" => list(&self.labels),
            "commits" => list(&self.commits),
            _ => None,
        }
    }
}

/// What a ticket was before the write: its latest revision (`rev` > 0), or
/// — a ticket older than revisions — its row as stored (`rev` 0; its body
/// read only once the write is about to replace it).
struct Prior {
    rev: u32,
    fields: Fields,
    hash: Option<String>,
    body: Option<String>,
    deleted: bool,
    at: i64,
}

/// A ticket the write touched.
struct Touch {
    id: String,
    shortcode: Option<ShortcodeId>,
    /// `None`: born in this write.
    prior: Option<Prior>,
    body: Option<String>,
    gone: bool,
}

/// Who a step's transition says moved a ticket.
struct Said {
    actor: String,
    name: Option<String>,
    source: RevisionSource,
    message: Option<String>,
    cause: Option<String>,
}

/// A revision a write kept — what [`publish`] says once it has committed.
pub(super) struct Kept {
    shortcode: ShortcodeId,
    ticket: TicketId,
    rev: u32,
    fields: Vec<String>,
}

struct Recording {
    act: Act,
    touched: Vec<Touch>,
    said: HashMap<String, Said>,
    kept: Vec<Kept>,
}

thread_local! {
    static RECORDING: RefCell<Option<Recording>> = const { RefCell::new(None) };
}

/// A write's recording, open on its thread from before its transaction to
/// after its events — dropped, it is gone (a write that failed keeps
/// nothing: its transaction rolled back what [`touch`] wrote).
pub(super) struct Writing(());

impl Writing {
    /// Opens the write's recording, as `act` (the caller's, when known).
    pub(super) fn begin(act: Option<Act>) -> Self {
        let act = act.unwrap_or_default();
        RECORDING.with(|r| *r.borrow_mut() = Some(Recording { act, touched: Vec::new(), said: HashMap::new(), kept: Vec::new() }));
        Writing(())
    }
}

impl Drop for Writing {
    fn drop(&mut self) {
        RECORDING.with(|r| *r.borrow_mut() = None);
    }
}

fn with<T>(f: impl FnOnce(&mut Recording) -> T) -> Option<T> {
    RECORDING.with(|r| r.borrow_mut().as_mut().map(f))
}

/// The write acts as `act` — a write handed one (`update_by`, `delete_by`, …).
pub(super) fn act(act: &Act) {
    with(|r| r.act = act.clone());
}

/// A step's transition — who moved its ticket, and what caused it.
pub(super) fn said(t: &Transition) {
    with(|r| {
        r.said.insert(
            t.ticket.to_string(),
            Said { actor: t.actor.clone(), name: t.name.clone(), source: t.source.into(), message: t.message.clone(), cause: t.cause_ticket.clone() },
        )
    });
}

/// The latest revision of `id`, as a prior.
fn latest(conn: &Connection, id: &str) -> rusqlite::Result<Option<Prior>> {
    conn.prepare_cached(
        "SELECT rev, title, status, kind, severity, parent, blocked_by, labels, commits, body_hash, fields, at_unix
           FROM revisions WHERE ticket = ?1 ORDER BY rev DESC LIMIT 1",
    )?
    .query_row([id], |r| {
        let fields: String = r.get(10)?;
        Ok(Prior { rev: r.get(0)?, fields: fields_of(r, 1)?, hash: Some(r.get(9)?), body: None, deleted: list(&fields).iter().any(|f| f == DELETED), at: r.get(11)? })
    })
    .optional()
}

fn list(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// [`Fields`] from a statement's columns from `at`: title, status, kind,
/// severity, parent, blocked_by, labels, commits.
fn fields_of(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<Fields> {
    Ok(Fields {
        title: r.get(at)?,
        status: r.get(at + 1)?,
        kind: r.get(at + 2)?,
        severity: r.get(at + 3)?,
        parent: r.get(at + 4)?,
        blocked_by: list(&r.get::<_, String>(at + 5)?),
        labels: list(&r.get::<_, String>(at + 6)?),
        commits: list(&r.get::<_, String>(at + 7)?),
    })
}

/// A write is about to move `id`'s row (`body`: and its body, to this).
/// Outside a recording (a migration, a seed, a test's own SQL), nothing.
pub(super) fn touch(conn: &Connection, id: &str, body: Option<&str>) -> rusqlite::Result<()> {
    let open = with(|_| ()).is_some();
    if !open {
        return Ok(());
    }
    let first = with(|r| !r.touched.iter().any(|t| t.id == id)).unwrap_or(false);
    if first {
        let (prior, shortcode) = match latest(conn, id)? {
            Some(p) => (Some(p), None),
            None => match db::row(conn, id)? {
                Some(row) => (Some(Prior { rev: 0, fields: Fields::of(&row), hash: None, body: None, deleted: false, at: row.updated_unix }), Some(row.shortcode)),
                None => (None, None),
            },
        };
        with(|r| r.touched.push(Touch { id: id.to_string(), shortcode, prior, body: None, gone: false }));
    }
    if let Some(new) = body {
        // A ticket older than revisions keeps the body it had: read it before
        // it is replaced.
        let needs = with(|r| r.touched.iter().find(|t| t.id == id).and_then(|t| t.prior.as_ref()).is_some_and(|p| p.rev == 0 && p.body.is_none())).unwrap_or(false);
        let old = if needs { db::body(conn, id)? } else { None };
        with(|r| {
            if let Some(t) = r.touched.iter_mut().find(|t| t.id == id) {
                if let (Some(p), Some(old)) = (t.prior.as_mut(), old) {
                    p.body = Some(old);
                }
                t.body = Some(new.to_string());
            }
        });
    }
    Ok(())
}

/// A write is about to delete `id`'s row: its revisions stay, and it gains
/// one saying so.
pub(super) fn gone(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    touch(conn, id, None)?;
    let row = db::row(conn, id)?;
    let needs = with(|r| r.touched.iter().find(|t| t.id == id).and_then(|t| t.prior.as_ref()).is_some_and(|p| p.rev == 0 && p.body.is_none())).unwrap_or(false);
    let old = if needs { db::body(conn, id)? } else { None };
    with(|r| {
        if let Some(t) = r.touched.iter_mut().find(|t| t.id == id) {
            t.gone = true;
            t.shortcode = t.shortcode.take().or(row.map(|r| r.shortcode));
            if let (Some(p), Some(old)) = (t.prior.as_mut(), old) {
                p.body = Some(old);
            }
        }
    });
    Ok(())
}

/// ACT-6: tickets moved to new keys — their revisions with them (before
/// the write touches them under the new ones).
pub(super) fn rekey(conn: &Connection, map: &HashMap<String, String>) -> rusqlite::Result<()> {
    let mut pairs: Vec<(&String, &String)> = map.iter().collect();
    pairs.sort();
    for (old, new) in pairs {
        conn.prepare_cached("UPDATE revisions SET ticket = ?2 WHERE ticket = ?1")?.execute([old, new])?;
    }
    Ok(())
}

/// One revision's row.
struct Row_<'a> {
    ticket: &'a str,
    rev: u32,
    at: i64,
    actor: &'a str,
    name: Option<&'a str>,
    source: RevisionSource,
    message: Option<&'a str>,
    cause: Option<&'a str>,
    fields: &'a [String],
    snap: &'a Fields,
    hash: &'a str,
    bytes: u64,
    added: u32,
    removed: u32,
}

fn insert(conn: &Connection, r: Row_<'_>) -> rusqlite::Result<()> {
    let json = |v: &Vec<String>| serde_json::to_string(v).expect("a list");
    conn.prepare_cached(
        "INSERT INTO revisions (ticket, rev, at_unix, actor, name, source, message, cause, fields, title, status, kind, severity, parent,
                                blocked_by, labels, commits, body_hash, body_bytes, added, removed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
    )?
    .execute(params![
        r.ticket,
        r.rev,
        r.at,
        r.actor,
        r.name,
        r.source.as_str(),
        r.message,
        r.cause,
        serde_json::to_string(r.fields).expect("a list"),
        r.snap.title,
        r.snap.status,
        r.snap.kind,
        r.snap.severity,
        r.snap.parent,
        json(&r.snap.blocked_by),
        json(&r.snap.labels),
        json(&r.snap.commits),
        r.hash,
        r.bytes as i64,
        r.added,
        r.removed,
    ])?;
    Ok(())
}

/// Keeps `body` once, by its hash.
fn keep_body(conn: &Connection, hash: &str, body: &str) -> rusqlite::Result<()> {
    conn.prepare_cached("INSERT OR IGNORE INTO revision_bodies (hash, body) VALUES (?1, ?2)")?.execute([hash, body])?;
    Ok(())
}

/// A kept body's text.
pub(super) fn body_of(conn: &Connection, hash: &str) -> rusqlite::Result<Option<String>> {
    conn.prepare_cached("SELECT body FROM revision_bodies WHERE hash = ?1")?.query_row([hash], |r| r.get(0)).optional()
}

/// Lines of a whole body, as a birth's `added`.
fn line_count(body: &str) -> u32 {
    if body.is_empty() {
        0
    } else {
        body.strip_suffix('\n').unwrap_or(body).split('\n').count() as u32
    }
}

/// Keeps the revisions of everything the write touched — in its
/// transaction, before it commits. Outside a recording, nothing.
pub(super) fn flush(conn: &Connection) -> rusqlite::Result<()> {
    let Some((touched, act, mut said)) = with(|r| (std::mem::take(&mut r.touched), r.act.clone(), std::mem::take(&mut r.said))) else {
        return Ok(());
    };
    let now = chrono::Utc::now().timestamp();
    let mut kept: Vec<Kept> = Vec::new();
    for t in touched {
        let row = db::row(conn, &t.id)?;
        let (now_fields, shortcode, at) = match (&row, &t.prior) {
            (Some(r), _) => (Fields::of(r), r.shortcode.clone(), r.updated_unix.max(now)),
            // Deleted: the last version, said gone.
            (None, Some(p)) if t.gone && !p.deleted => (p.fields.clone(), t.shortcode.clone().expect("a deleted ticket's shortcode"), now),
            (None, _) => continue,
        };
        let prior_hash = |p: &Prior| -> rusqlite::Result<Option<String>> {
            Ok(match (&p.hash, &p.body) {
                (Some(h), _) => Some(h.clone()),
                (None, Some(b)) => Some(hash(b)),
                (None, None) => None,
            })
        };
        let new_hash = t.body.as_deref().map(hash);
        let (mut fields, body_moved) = match &t.prior {
            None => (vec![CREATED.to_string()], true),
            Some(p) => {
                let before = prior_hash(p)?;
                let moved = match (&new_hash, &before) {
                    (Some(n), Some(b)) => n != b,
                    // A new body against a row whose body was not read: it was
                    // read when the body came (`touch`), so this is a body the
                    // ticket already had.
                    (Some(_), None) => false,
                    (None, _) => false,
                };
                let mut f = now_fields.differ(&p.fields, moved);
                if row.is_none() {
                    f = vec![DELETED.to_string()];
                } else if p.deleted {
                    f.insert(0, UNDELETED.to_string());
                }
                (f, moved)
            }
        };
        if fields.is_empty() {
            continue;
        }
        let first = kept.first().map(|k| k.ticket.to_string());
        let who = said.remove(&t.id);
        let (actor, name, source, message) = match &who {
            Some(s) => (s.actor.clone(), s.name.clone(), s.source, s.message.clone()),
            None => (act.actor.clone(), act.name.clone(), act.source.into(), act.message.clone()),
        };
        let cause = who.as_ref().and_then(|s| s.cause.clone()).or(first.filter(|f| *f != t.id));
        // The version before, for a ticket older than revisions.
        let mut rev = t.prior.as_ref().map_or(0, |p| p.rev);
        let mut prior_body: Option<String> = t.prior.as_ref().and_then(|p| p.body.clone());
        let mut prior_hash_kept = t.prior.as_ref().and_then(|p| p.hash.clone());
        if let Some(p) = t.prior.as_ref().filter(|p| p.rev == 0) {
            let body = match &p.body {
                Some(b) => b.clone(),
                None => db::body(conn, &t.id)?.unwrap_or_default(),
            };
            let h = hash(&body);
            keep_body(conn, &h, &body)?;
            insert(
                conn,
                Row_ {
                    ticket: &t.id,
                    rev: 1,
                    at: p.at,
                    actor: crate::types::SYSTEM_ACTOR,
                    name: None,
                    source: RevisionSource::Baseline,
                    message: Some("the ticket as it stood before revisions were kept"),
                    cause: None,
                    fields: &[],
                    snap: &p.fields,
                    hash: &h,
                    bytes: body.len() as u64,
                    added: 0,
                    removed: 0,
                },
            )?;
            rev = 1;
            prior_hash_kept = Some(h);
            prior_body = Some(body);
        }
        // This write's body: the new one, or the one before.
        let (h, bytes, added, removed) = if body_moved || t.prior.is_none() {
            let body = t.body.clone().unwrap_or_default();
            let h = new_hash.clone().unwrap_or_else(|| hash(&body));
            keep_body(conn, &h, &body)?;
            let (added, removed) = match &t.prior {
                None => (line_count(&body), 0),
                Some(_) => {
                    let old = match prior_body {
                        Some(b) => b,
                        None => prior_hash_kept.as_deref().map(|ph| body_of(conn, ph)).transpose()?.flatten().unwrap_or_default(),
                    };
                    diff::counts(&old, &body)
                }
            };
            (h, body.len() as u64, added, removed)
        } else {
            let h = prior_hash_kept.clone().expect("a prior's body hash");
            let bytes: i64 = conn.query_row("SELECT length(CAST(body AS BLOB)) FROM revision_bodies WHERE hash = ?1", [&h], |r| r.get(0)).optional()?.unwrap_or(0);
            (h, bytes as u64, 0, 0)
        };
        let next = rev + 1;
        insert(
            conn,
            Row_ {
                ticket: &t.id,
                rev: next,
                at,
                actor: &actor,
                name: name.as_deref(),
                source,
                message: message.as_deref(),
                cause: cause.as_deref(),
                fields: &fields,
                snap: &now_fields,
                hash: &h,
                bytes,
                added,
                removed,
            },
        )?;
        fields.shrink_to_fit();
        kept.push(Kept { shortcode, ticket: TicketId::new(t.id.clone()), rev: next, fields });
    }
    with(|r| r.kept.extend(kept));
    Ok(())
}

/// [`flush`], then the commit — a write runner's closing step.
pub(super) fn commit(txn: rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    flush(&txn)?;
    txn.commit()
}

/// Once the write has committed and sent its events: one `revised` per
/// revision it kept.
pub(super) fn publish(tx: &Broadcast) {
    let kept = with(|r| {
        debug_assert!(r.touched.is_empty(), "TICKETS-7: a write committed without keeping its revisions (revisions::commit)");
        std::mem::take(&mut r.kept)
    })
    .unwrap_or_default();
    for k in kept {
        let _ = tx.revisions.send(Change::Revised { shortcode: k.shortcode, ticket: k.ticket, rev: k.rev, fields: k.fields });
    }
}

// ── Reads ────────────────────────────────────────────────────────────────

/// A kept revision whole: its line, its fields and its body's hash.
pub(super) struct Kept_ {
    pub(super) revision: Revision,
    fields: Fields,
}

const SELECT: &str = "SELECT ticket, rev, at_unix, actor, name, source, message, cause, fields, body_hash, body_bytes, added, removed,
                             title, status, kind, severity, parent, blocked_by, labels, commits FROM revisions";

fn read(r: &rusqlite::Row<'_>) -> rusqlite::Result<Kept_> {
    Ok(Kept_ {
        revision: Revision {
            ticket: TicketId::new(r.get::<_, String>(0)?),
            rev: r.get(1)?,
            at_unix: r.get(2)?,
            actor: r.get(3)?,
            name: r.get(4)?,
            source: RevisionSource::parse(&r.get::<_, String>(5)?),
            message: r.get(6)?,
            cause: r.get(7)?,
            fields: list(&r.get::<_, String>(8)?),
            body_hash: r.get(9)?,
            body_bytes: r.get::<_, i64>(10)? as u64,
            added: r.get(11)?,
            removed: r.get(12)?,
        },
        fields: fields_of(r, 13)?,
    })
}

/// A page of `id`'s revisions, newest first, below `before`; and its count
/// and latest.
fn page(conn: &Connection, id: &str, before: Option<u32>, limit: u32) -> rusqlite::Result<(Vec<Revision>, u32, u32)> {
    let (count, latest): (u32, Option<u32>) = conn.query_row("SELECT count(*), max(rev) FROM revisions WHERE ticket = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut stmt = conn.prepare_cached(&format!("{SELECT} WHERE ticket = ?1 AND rev < ?2 ORDER BY rev DESC LIMIT ?3"))?;
    let rows = stmt.query_map(params![id, before.unwrap_or(u32::MAX), limit], read)?;
    let out = rows.map(|r| r.map(|k| k.revision)).collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((out, count, latest.unwrap_or(0)))
}

fn one(conn: &Connection, id: &str, rev: u32) -> rusqlite::Result<Option<Kept_>> {
    conn.prepare_cached(&format!("{SELECT} WHERE ticket = ?1 AND rev = ?2"))?.query_row(params![id, rev], read).optional()
}

/// The most revisions one `list` page answers, and its default.
pub const PAGE_MAX: u32 = 500;
pub const PAGE_DEFAULT: u32 = 100;

/// A page of revisions, as [`Store::revisions`] answers it.
#[derive(Debug, Clone)]
pub struct RevisionPage {
    pub revisions: Vec<Revision>,
    pub count: u32,
    pub latest: u32,
    pub next: Option<u32>,
}

/// How `revisions diff` reads a body: its source, or its visible text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffView {
    #[default]
    Source,
    Text,
}

/// What a restore asks of the store: the update that puts the chosen fields
/// of the revision back, and the fields it puts back (empty: nothing differs).
pub struct Restoring {
    pub update: Update,
    pub fields: Vec<String>,
}

impl Store {
    /// TICKETS-7: the revisions every write keeps, as `Revised` changes — a
    /// channel apart, which `watch { revisions: true }` reads beside its own.
    pub fn subscribe_revisions(&self) -> broadcast::Receiver<Change> {
        self.tx.revisions.subscribe()
    }

    /// The key `id` names for `who` — a ticket in its reach, or a deleted one
    /// whose shortcode is — as `history` resolves it.
    fn revisions_key(&self, who: &(impl Reader + ?Sized), id: &str) -> Result<String, StoreError> {
        let seen = Seen::of(who)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let key = match cache.find_ticket(id, shortcodes::OrgReach::All) {
            shortcodes::Found::One(key) => key,
            shortcodes::Found::Many(candidates) => return Err(StoreError::Ambiguous { text: id.trim().to_string(), candidates }),
            shortcodes::Found::None => crate::address::folded(id),
        };
        let there = cache.get(&key).is_some() || cache.tombstones.place_of(&key).is_some();
        if !there {
            return Err(StoreError::NotFound(key));
        }
        if !seen.all() {
            let sees = seen.sees(&cache);
            let seen_it = match cache.get(&key) {
                Some(_) => sees.id(&key),
                None => cache.tombstones.place_of(&key).is_some_and(|(sc, _)| sees.shortcodes().unwrap_or_default().contains(sc)),
            };
            if !seen_it {
                return Err(StoreError::NotFound(key));
            }
        }
        Ok(key)
    }

    /// TICKETS-7: a page of the revisions of `id`, newest first — below
    /// `before` (a revision number; none: from the latest), at most `limit`
    /// ([`PAGE_DEFAULT`], at most [`PAGE_MAX`]). [`StoreError::NotFound`]
    /// for a ticket out of `who`'s reach (or none). From storage.
    pub async fn revisions(&self, who: &(impl Reader + ?Sized), id: &str, before: Option<u32>, limit: Option<u32>) -> Result<RevisionPage, StoreError> {
        let key = self.revisions_key(who, id)?;
        let limit = limit.unwrap_or(PAGE_DEFAULT).clamp(1, PAGE_MAX);
        let (revisions, count, latest) = self.readers.read(move |conn| Ok(page(conn, &key, before, limit)?)).await?;
        let next = match revisions.last() {
            Some(last) if last.rev > 1 && revisions.len() as u32 == limit => Some(last.rev),
            _ => None,
        };
        Ok(RevisionPage { revisions, count, latest, next })
    }

    /// TICKETS-7: one revision of `id` with its body — the cheap read of an
    /// old body by number (TICKETS-8 reads a mockup's versions with it).
    pub async fn revision_body(&self, who: &(impl Reader + ?Sized), id: &str, rev: u32) -> Result<(Revision, String), StoreError> {
        let key = self.revisions_key(who, id)?;
        let (kept, body) = self.readers.read(move |conn| fetch(conn, &key, rev)).await?;
        Ok((kept.revision, body))
    }

    /// TICKETS-7: the ticket `id` exactly as it was at revision `rev` —
    /// every field a revision keeps from it, the rest (its id, address,
    /// children, placement) as it is now; its names out of `who`'s reach cut
    /// as a read cuts them.
    pub async fn revision(&self, who: &(impl Reader + ?Sized), id: &str, rev: u32) -> Result<(Revision, Ticket), StoreError> {
        let key = self.revisions_key(who, id)?;
        let k = key.clone();
        let (kept, body) = self.readers.read(move |conn| fetch(conn, &k, rev)).await?;
        let seen = Seen::of(who)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let mut ticket = as_it_was(&cache, &key, &kept, body);
        if !seen.all() {
            seen.sees(&cache).ticket(&mut ticket);
        }
        Ok((kept.revision, ticket))
    }

    /// TICKETS-7: what changed from revision `from` to `to` of `id` — `to`
    /// none: the latest; `from` none: the one before `to`. Field by field,
    /// the frontmatter key by key, and the body as a line diff (with words
    /// within a changed line) of its source or of its visible text.
    pub async fn revision_diff(&self, who: &(impl Reader + ?Sized), id: &str, from: Option<u32>, to: Option<u32>, view: DiffView) -> Result<RevisionDiff, StoreError> {
        let key = self.revisions_key(who, id)?;
        self.readers
            .read(move |conn| {
                let (_, _, latest) = page(conn, &key, None, 1)?;
                let to = to.unwrap_or(latest);
                let from = from.unwrap_or(to.saturating_sub(1).max(1));
                let (a, ab) = fetch(conn, &key, from)?;
                let (b, bb) = fetch(conn, &key, to)?;
                let mut fields: Vec<FieldChange> =
                    FIELDS.iter().filter(|f| **f != "body").map(|f| (f, a.fields.text(f), b.fields.text(f))).filter(|(_, x, y)| x != y).map(|(f, x, y)| FieldChange { field: f.to_string(), from: x, to: y }).collect();
                let body = (a.revision.body_hash != b.revision.body_hash).then(|| match view {
                    DiffView::Source => diff::text(&ab, &bb),
                    DiffView::Text => diff::text(&diff::visible(&ab), &diff::visible(&bb)),
                });
                if let Some(d) = &body {
                    fields.insert(1.min(fields.len()), FieldChange { field: "body".into(), from: Some(format!("{} bytes", a.revision.body_bytes)), to: Some(format!("{} bytes (+{} −{})", b.revision.body_bytes, d.added, d.removed)) });
                }
                let (fa, fb) = (diff::frontmatter(&ab), diff::frontmatter(&bb));
                let mut keys: Vec<&String> = fa.iter().map(|(k, _)| k).chain(fb.iter().map(|(k, _)| k)).collect();
                keys.dedup();
                let mut seen_keys = HashSet::new();
                let value = |m: &[(String, String)], k: &str| m.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
                let frontmatter = keys
                    .into_iter()
                    .filter(|k| seen_keys.insert(k.as_str()))
                    .map(|k| FieldChange { field: k.clone(), from: value(&fa, k), to: value(&fb, k) })
                    .filter(|c| c.from != c.to)
                    .collect();
                Ok(RevisionDiff { ticket: TicketId::new(key), from, to, fields, frontmatter, body })
            })
            .await
    }

    /// TICKETS-7: what `restore` writes — the update that puts `fields` of
    /// revision `rev` back (none named: every one of
    /// [`crate::revisions::RESTORABLE`]), each that differs from the ticket
    /// now. Refused: a field that is not restorable (the commits are linked,
    /// never restored), a deleted ticket (undelete it first), a parent the
    /// revision did not have (a parent is moved, never cleared).
    pub async fn restoring(&self, who: &(impl Reader + ?Sized), id: &str, rev: u32, fields: Option<Vec<String>>) -> Result<Restoring, StoreError> {
        let key = self.revisions_key(who, id)?;
        let asked: Vec<String> = match fields {
            Some(f) if !f.is_empty() => f.into_iter().map(|f| f.trim().to_ascii_lowercase()).collect(),
            _ => RESTORABLE.iter().map(|f| f.to_string()).collect(),
        };
        if let Some(bad) = asked.iter().find(|f| !RESTORABLE.contains(&f.as_str())) {
            return Err(StoreError::Invalid(format!("{bad} is not a field restore puts back (one of {}; commits are linked, never restored)", RESTORABLE.join(", "))));
        }
        let k = key.clone();
        let (kept, body) = self.readers.read(move |conn| fetch(conn, &k, rev)).await?;
        let current = self.get_unscoped(&key).await.map_err(|_| StoreError::Invalid(format!("{key} is deleted: undelete it before restoring a revision")))?;
        let now = Fields::of(&TicketSummary::from(current.clone()));
        let was = &kept.fields;
        let mut u = Update::default();
        let mut put = Vec::new();
        for f in &asked {
            let differs = match f.as_str() {
                "body" => hash(&current.body) != kept.revision.body_hash,
                f => now.text(f) != was.text(f),
            };
            if !differs {
                continue;
            }
            match f.as_str() {
                "title" => u.title = Some(was.title.clone()),
                "body" => u.body = Some(body.clone()),
                "status" => u.status = Some(was.status.parse().map_err(StoreError::Invalid)?),
                "kind" => u.kind = Some(was.kind.parse().map_err(StoreError::Invalid)?),
                "severity" => u.severity = Some(was.severity.as_deref().map(str::parse).transpose().map_err(StoreError::Invalid)?),
                "parent" => match &was.parent {
                    Some(p) => u.parent = Some(p.clone()),
                    None => return Err(StoreError::Invalid(format!("revision {rev} of {key} had no parent: a parent is moved, never cleared"))),
                },
                "blocked_by" => {
                    u.block = was.blocked_by.iter().filter(|b| !now.blocked_by.contains(b)).map(|b| crate::vocab::TicketRef::stored(b.clone())).collect();
                    u.unblock = now.blocked_by.iter().filter(|b| !was.blocked_by.contains(b)).map(|b| crate::vocab::TicketRef::stored(b.clone())).collect();
                }
                "labels" => {
                    u.label = was.labels.iter().filter(|l| !now.labels.contains(l)).map(|l| crate::vocab::Label::stored(l.clone())).collect();
                    u.unlabel = now.labels.iter().filter(|l| !was.labels.contains(l)).map(|l| crate::vocab::Label::stored(l.clone())).collect();
                }
                _ => {}
            }
            put.push(f.clone());
        }
        // A kind with its status, as `set_kind` takes them.
        if u.kind.is_some() && u.status.is_none() && was.status != now.status {
            u.status = Some(was.status.parse().map_err(StoreError::Invalid)?);
        }
        Ok(Restoring { update: u, fields: put })
    }
}

/// Revision `rev` of `key` and its body; [`StoreError::Invalid`] naming it
/// when there is none.
fn fetch(conn: &Connection, key: &str, rev: u32) -> Result<(Kept_, String), StoreError> {
    let kept = one(conn, key, rev)?.ok_or_else(|| StoreError::Invalid(format!("{key} has no revision {rev}")))?;
    let body = body_of(conn, &kept.revision.body_hash)?.ok_or_else(|| StoreError::Db(format!("{key} revision {rev}: its body ({}) is not kept", kept.revision.body_hash)))?;
    Ok((kept, body))
}

/// The ticket as revision `kept` holds it, over what it is now (a deleted
/// one's: what its tombstone keeps).
fn as_it_was(cache: &Cache, key: &str, kept: &Kept_, body: String) -> Ticket {
    let f = &kept.fields;
    let mut t = match cache.get(key) {
        Some(row) => whole(row.clone(), String::new()),
        None => {
            let (sc, seq) = cache.tombstones.place_of(key).map(|(s, n)| (s.clone(), n)).unwrap_or_else(|| (ShortcodeId::new(""), 0));
            Ticket {
                id: TicketId::new(key.to_string()),
                epic: crate::address::Code::stored(sc.as_str()),
                shortcode: sc,
                address: String::new(),
                seq,
                title: String::new(),
                status: Status::Pending,
                kind: Kind::Build,
                severity: None,
                blocked_by: Vec::new(),
                unlocks: Vec::new(),
                hidden_blocked_by: 0,
                hidden_unlocks: 0,
                parent_hidden: false,
                archived: None,
                parent: None,
                child_count: 0,
                commits: Vec::new(),
                path: None,
                placement: None,
                labels: Vec::new(),
                body: String::new(),
                created_unix: kept.revision.at_unix,
                updated_unix: kept.revision.at_unix,
                moved_from: Vec::new(),
            }
        }
    };
    t.title = f.title.clone();
    if let Ok(s) = f.status.parse() {
        t.status = s;
    }
    if let Ok(k) = f.kind.parse() {
        t.kind = k;
    }
    t.severity = f.severity.as_deref().and_then(|s| s.parse().ok());
    t.parent = f.parent.clone().map(TicketId::new);
    t.blocked_by = f.blocked_by.iter().cloned().map(TicketName::new).collect();
    t.labels = f.labels.iter().cloned().map(Label::stored).collect();
    t.commits = f.commits.iter().cloned().map(CommitRef::stored).collect();
    t.updated_unix = kept.revision.at_unix;
    t.body = body;
    t
}

/// `--verify-derived`: each ticket's latest revision is the ticket as it is
/// (its fields and its body's hash), every ticket's revisions are numbered
/// 1, 2, … with none missing, and every body a revision names is kept with
/// that hash. `None` for a file without the tables; else how many
/// revisions, and what differs.
pub(super) fn check(conn: &Connection) -> rusqlite::Result<Option<(usize, Vec<String>)>> {
    if !has_table(conn)? {
        return Ok(None);
    }
    let mut out = Vec::new();
    let total: i64 = conn.query_row("SELECT count(*) FROM revisions", [], |r| r.get(0))?;
    let mut gaps = conn.prepare("SELECT ticket, count(*), max(rev), min(rev) FROM revisions GROUP BY ticket ORDER BY ticket")?;
    let rows = gaps.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, u32>(2)?, r.get::<_, u32>(3)?)))?;
    let mut tickets = Vec::new();
    for row in rows {
        let (t, n, max, min) = row?;
        if n != max || min != 1 {
            out.push(format!("revisions of {t}: {n} kept, numbered {min} to {max}"));
        }
        tickets.push(t);
    }
    for t in tickets {
        let Some(k) = one(conn, &t, latest(conn, &t)?.map_or(0, |p| p.rev))? else { continue };
        if k.revision.fields.iter().any(|f| f == DELETED) {
            continue;
        }
        let Some(row) = db::row_unmigrated(conn, &t)? else {
            out.push(format!("revisions of {t}: the ticket is not there and its latest revision does not say deleted"));
            continue;
        };
        if Fields::of(&row) != k.fields {
            out.push(format!("revisions of {t}: the latest ({}) is not the ticket as it is", k.revision.rev));
        }
        let body = db::body(conn, &t)?.unwrap_or_default();
        if hash(&body) != k.revision.body_hash {
            out.push(format!("revisions of {t}: the latest ({})'s body is not the ticket's", k.revision.rev));
        }
    }
    let mut bodies = conn.prepare("SELECT DISTINCT r.body_hash, b.body FROM revisions r LEFT JOIN revision_bodies b ON b.hash = r.body_hash")?;
    let rows = bodies.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
    for row in rows {
        match row? {
            (h, None) => out.push(format!("revision body {h}: named, not kept")),
            (h, Some(b)) if hash(&b) != h => out.push(format!("revision body {h}: kept under another hash")),
            _ => {}
        }
    }
    Ok(Some((total as usize, out)))
}
