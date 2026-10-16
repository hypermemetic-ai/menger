//! DEPLOY-20: **the commit-link record** — which `commits link` call brought
//! each of a ticket's commits, and when; what a ticket's *versions* are.
//!
//! Owner, 2026-10-16: *"if we update and add a commit then the deployment
//! should track the older version of the ticket and the new version should
//! say that it is not deployed since it's tied to an earlier commit."* A
//! ticket's commits are kept in the order they were linked (SDLC-9); a
//! **version** is the commits one link call added — a merge commit and its
//! branch commit linked together are one version — and versions are ordered
//! as they were linked, never by the repository's history (menger has
//! none, DEPLOY-11).
//!
//! **Stored** in `commit_links`, a row per commit linked since DEPLOY-20:
//! the ticket, the reference as kept, the version (1, 2, … per ticket) and
//! when — written in the link's own transaction, taken out by an unlink,
//! rekeyed by a reshortcode (ACT-6). A file without it gains it, empty, on
//! open ([`migrate`]).
//!
//! **Commits linked before it** carry no row: they are one version, the
//! first ([`groups`]) — so an old database keeps every answer until a
//! ticket gains a commit (DEPLOY-20's acceptance).

use crate::vocab::CommitRef;
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// The table.
pub(super) const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS commit_links (
            ticket  TEXT NOT NULL,
            sha     TEXT NOT NULL,
            version INTEGER NOT NULL,
            at_unix INTEGER NOT NULL,
            PRIMARY KEY (ticket, sha)
        );
    ";

/// One ticket's record: each commit linked since DEPLOY-20, as kept → its
/// version and when it was linked.
pub(crate) type Linked = HashMap<String, (u32, i64)>;

fn has_table(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'commit_links'", [], |r| {
        r.get::<_, i64>(0).map(|n| n > 0)
    })
}

/// A file without the table gains it, empty, here — on every open, a no-op
/// once it is there. Returns whether it was made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    if has_table(conn)? {
        return Ok(false);
    }
    conn.execute_batch(TABLE)?;
    tracing::warn!("DEPLOY-20: the file gained the commit_links table, empty; every commit linked before it is its ticket's first version");
    Ok(true)
}

/// Every ticket's record, as stored — none for a file without the table.
pub(super) fn load(conn: &Connection) -> rusqlite::Result<HashMap<String, Linked>> {
    let mut out: HashMap<String, Linked> = HashMap::new();
    if !has_table(conn)? {
        return Ok(out);
    }
    let mut stmt = conn.prepare("SELECT ticket, sha, version, at_unix FROM commit_links")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        out.entry(r.get(0)?).or_default().insert(r.get(1)?, (r.get::<_, i64>(2)? as u32, r.get(3)?));
    }
    Ok(out)
}

/// `refs` linked to `ticket` as `version`, at `at` — in the caller's
/// transaction.
pub(super) fn store(conn: &Connection, ticket: &str, refs: &[CommitRef], version: u32, at: i64) -> rusqlite::Result<()> {
    let mut insert = conn.prepare_cached(
        "INSERT INTO commit_links (ticket, sha, version, at_unix) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (ticket, sha) DO UPDATE SET version = excluded.version, at_unix = excluded.at_unix",
    )?;
    for r in refs {
        insert.execute(params![ticket, r.as_str(), i64::from(version), at])?;
    }
    Ok(())
}

/// `refs` unlinked from `ticket` — in the caller's transaction.
pub(super) fn forget(conn: &Connection, ticket: &str, refs: &[CommitRef]) -> rusqlite::Result<()> {
    let mut delete = conn.prepare_cached("DELETE FROM commit_links WHERE ticket = ?1 AND sha = ?2")?;
    for r in refs {
        delete.execute(params![ticket, r.as_str()])?;
    }
    Ok(())
}

/// The version the next link call to a ticket with `linked` makes: one past
/// its newest (the commits before DEPLOY-20 are version 0).
pub(super) fn next(linked: Option<&Linked>) -> u32 {
    linked.into_iter().flat_map(|l| l.values().map(|(v, _)| *v)).max().unwrap_or(0) + 1
}

/// Each of `commits` (in link order) — its version's place among the
/// ticket's versions, 0 first. A commit with no row (linked before
/// DEPLOY-20) is version 0.
pub(crate) fn groups(commits: &[CommitRef], linked: Option<&Linked>) -> Vec<u32> {
    commits.iter().map(|c| linked.and_then(|l| l.get(c.as_str())).map_or(0, |(v, _)| *v)).collect()
}

/// When `c` was linked, when the record says (`None`: before DEPLOY-20).
pub(crate) fn at(c: &str, linked: Option<&Linked>) -> Option<i64> {
    linked.and_then(|l| l.get(c)).map(|(_, at)| *at)
}
