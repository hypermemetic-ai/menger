//! AUTH-7: the **roles store** — an admin's edits of a role's grants
//! (`auth.role_set`), kept beside the tickets as records of their own.
//!
//! A role's grants are declared in code first — each activation's
//! `roles(...)` (`viewer`, `editor`) — and an admin's edit wins once made
//! (AUTH-1, Q1: "roles: code, then data"). Only the edits are stored: a role
//! nobody edited is what the code declares, release to release.
//!
//! **Behind the cache, as everything stored is**: read whole when the store
//! opens ([`load`]); every read answers from the cache; an edit takes the
//! write path every write takes — committed to the `roles` table in one
//! transaction, then put into the cache.
//!
//! **Stored** in one table, `roles`, a row per role: its grants, one
//! `<activation>.<method>` per line. Stored form 9 adds it
//! ([`super::stored`]): a file in form 6, 7 or 8 gains it, empty, once, on
//! its first open.

use rusqlite::{params, Connection};
use std::collections::BTreeMap;

pub(super) const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS roles (
            role       TEXT NOT NULL PRIMARY KEY,
            grants     TEXT NOT NULL,
            at         INTEGER NOT NULL
        );
    ";

/// The edited roles: role → its grants, as the admin set them.
pub type RoleEdits = BTreeMap<String, Vec<String>>;

pub(super) fn load(conn: &Connection) -> rusqlite::Result<RoleEdits> {
    let mut stmt = conn.prepare("SELECT role, grants FROM roles ORDER BY role")?;
    let mut rows = stmt.query([])?;
    let mut out = RoleEdits::new();
    while let Some(r) = rows.next()? {
        let role: String = r.get(0)?;
        let grants: String = r.get(1)?;
        out.insert(role, grants.lines().map(str::trim).filter(|g| !g.is_empty()).map(str::to_string).collect());
    }
    Ok(out)
}

pub(super) fn store(conn: &Connection, role: &str, grants: &[String], at: i64) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO roles (role, grants, at) VALUES (?1, ?2, ?3)
             ON CONFLICT (role) DO UPDATE SET grants = excluded.grants, at = excluded.at",
    )?
    .execute(params![role, grants.join("\n"), at])?;
    Ok(())
}
