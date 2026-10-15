//! IDENTITY-21: the **members store** — every grant of a role on a scope,
//! kept beside the tickets as records of their own.
//!
//! **Behind the cache, as everything stored is**: read whole when the store
//! opens ([`load`]); every read answers from the cache; a grant or a revoke
//! takes the write path every write takes — committed to the `members` table
//! in one transaction, then put into the cache.
//!
//! **Stored** in one table, `members`, a row per (principal, scope). Stored
//! form 8 adds it ([`super::stored`]): a file in form 6 or 7 gains it, empty,
//! once, on its first open.

use crate::members::{Grant, Role};
use crate::vocab::TeamName;
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS members (
            principal  TEXT NOT NULL,
            scope      TEXT NOT NULL,
            role       TEXT NOT NULL,
            granted_by TEXT NOT NULL,
            at         INTEGER NOT NULL,
            PRIMARY KEY (principal, scope)
        );
    ";

/// The grants, by principal then scope.
///
/// ACCESS-9: and the **grants generation** — a count every change to who
/// holds what moves (a grant, a revoke, a team joined or left), which a
/// [`crate::access::Reach`] records when it is resolved and a watch
/// compares (ACCESS-12) — and the teams each principal is in (ACCESS-22
/// keeps them; none today, so a team's grants reach no one yet).
#[derive(Debug, Clone, Default)]
pub struct Members {
    by_principal: BTreeMap<String, BTreeMap<String, Grant>>,
    teams: BTreeMap<String, BTreeSet<TeamName>>,
    generation: u64,
}

impl Members {
    pub(super) fn put(&mut self, g: Grant) {
        self.by_principal.entry(g.principal.clone()).or_default().insert(g.scope.clone(), g);
        self.generation += 1;
    }

    pub(super) fn remove(&mut self, principal: &str, scope: &str) -> Option<Grant> {
        let scopes = self.by_principal.get_mut(principal)?;
        let gone = scopes.remove(scope);
        if scopes.is_empty() {
            self.by_principal.remove(principal);
        }
        if gone.is_some() {
            self.generation += 1;
        }
        gone
    }

    /// ACCESS-9: the grants generation ([`Members`]).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// ACCESS-9: the teams `principal` is in, by name — the input a reach
    /// adds each team's grants from. Empty until ACCESS-22 keeps teams.
    pub fn teams_of(&self, principal: &str) -> Vec<TeamName> {
        self.teams.get(principal).map(|t| t.iter().cloned().collect()).unwrap_or_default()
    }

    /// ACCESS-9: `principal` in `team` (`true`) or out of it — the
    /// generation moved when that changed anything. ACCESS-22 stores it;
    /// until then, tests alone call it.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn set_team(&mut self, principal: &str, team: TeamName, member: bool) {
        let teams = self.teams.entry(principal.to_string()).or_default();
        let moved = if member { teams.insert(team) } else { teams.remove(&team) };
        if teams.is_empty() {
            self.teams.remove(principal);
        }
        if moved {
            self.generation += 1;
        }
    }

    /// One principal's grants, by scope.
    pub fn of(&self, principal: &str) -> Vec<Grant> {
        self.by_principal.get(principal).map(|m| m.values().cloned().collect()).unwrap_or_default()
    }

    /// Every grant, by principal then scope.
    pub fn all(&self) -> Vec<Grant> {
        self.by_principal.values().flat_map(|m| m.values().cloned()).collect()
    }

    pub fn get(&self, principal: &str, scope: &str) -> Option<&Grant> {
        self.by_principal.get(principal)?.get(scope)
    }
}

pub(super) fn load(conn: &Connection) -> rusqlite::Result<Members> {
    let mut stmt = conn.prepare("SELECT principal, scope, role, granted_by, at FROM members ORDER BY principal, scope")?;
    let mut rows = stmt.query([])?;
    let mut members = Members::default();
    while let Some(r) = rows.next()? {
        let role = r.get_ref(2)?.as_str()?.parse::<Role>().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into())
        })?;
        members.put(Grant { principal: r.get(0)?, scope: r.get(1)?, role, granted_by: r.get(3)?, at: r.get(4)? });
    }
    Ok(members)
}

pub(super) fn store(conn: &Connection, g: &Grant) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO members (principal, scope, role, granted_by, at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (principal, scope) DO UPDATE SET role = excluded.role,
             granted_by = excluded.granted_by, at = excluded.at",
    )?
    .execute(params![g.principal, g.scope, g.role.as_str(), g.granted_by, g.at])?;
    Ok(())
}

pub(super) fn delete(conn: &Connection, principal: &str, scope: &str) -> rusqlite::Result<usize> {
    conn.prepare_cached("DELETE FROM members WHERE principal = ?1 AND scope = ?2")?.execute(params![principal, scope])
}
