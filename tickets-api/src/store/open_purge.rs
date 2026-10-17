//! ACCESS-24 (ACCESS-4 C2, ACCESS-3 #11): **what was planted while open is
//! purged** — once per database, at the first open of a build that has no
//! open mode.
//!
//! While menger could run with login off, every caller acted for no one, and
//! a grant one wrote was kept signed `granted_by = --admin` — the operator's
//! mark, so once login was on it read as if the operator had made it (C2:
//! an anonymous REST `members.grant` gave a stranger admin on `*`; ACCESS-51:
//! `orgs create --admin P` while open). Under login every write is signed by
//! a principal, and the operator's own `--admin` grants are never stored (the
//! flag makes them, [`crate::access::Book::grants_of`]). So a stored row
//! signed `--admin` is one written while open, and nothing else is.
//!
//! [`Store::purge_open_grants`] removes each such grant — and each team
//! membership added by `--admin` — unless an `--admin` principal of this
//! start accounts for it: its principal is one the operator names, which
//! holds admin on `*` by the flag whatever the row says. The purge, its
//! report and its mark (`open_purge`, one row: when, what was removed, what
//! was kept) are one transaction; the cache moves after it, as every write's
//! does. A later open finds the mark and does nothing. Teams themselves are
//! never made while open (ACCESS-22 refused it from the first), and the
//! tickets written while open are the tracker itself — only grants are
//! someone's word about who may do what.

use super::*;
use crate::members::Scope;
use crate::vocab::{Grantee, TeamName, OPERATOR};
use rusqlite::{params, OptionalExtension};

/// The mark that the purge ran: one row, made in the purge's own
/// transaction (a file gains the table then, empty until that commit).
const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS open_purge (
            one     INTEGER PRIMARY KEY CHECK (one = 1),
            at      INTEGER NOT NULL,
            removed TEXT NOT NULL,
            kept    TEXT NOT NULL
        );
    ";

/// What [`Store::purge_open_grants`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenPurge {
    /// It ran at an earlier open, at `at` (unix seconds): nothing done now.
    Done { at: i64 },
    /// It ran now. `removed`: each grant (`principal role on scope`) and
    /// team membership (`principal in team:<name>`) signed `--admin` that no
    /// `--admin` principal accounts for, removed; `kept`: each `--admin` grant
    /// one does, kept.
    Ran { removed: Vec<String>, kept: Vec<String> },
}

/// One stored grant row signed `--admin`, as text.
struct Planted {
    principal: String,
    scope: String,
    role: String,
}

impl Planted {
    fn said(&self) -> String {
        format!("{} {} on {}", self.principal, self.role, self.scope)
    }
}

/// What the transaction found and did.
struct Purged {
    done: Option<i64>,
    grants: Vec<Planted>,
    kept: Vec<Planted>,
    teams: Vec<(String, String)>,
}

impl Store {
    /// ACCESS-24: the purge of what open mode planted (the module docs):
    /// every stored grant and team membership signed `--admin` that none of
    /// `admins` (this start's `--admin` principals) accounts for, removed —
    /// once per database, durably marked in the same transaction. As the
    /// store's [`System`]: the server's own upkeep, at its open.
    pub async fn purge_open_grants(&self, _sys: &System, admins: &[String], now: i64) -> Result<OpenPurge, StoreError> {
        let serial = self.serial().await;
        let admins: Vec<String> = admins.to_vec();
        self.write(
            serial,
            move |tx| {
                tx.execute_batch(TABLE)?;
                let done: Option<i64> = tx.query_row("SELECT at FROM open_purge WHERE one = 1", [], |r| r.get(0)).optional()?;
                if done.is_some() {
                    return Ok(Purged { done, grants: Vec::new(), kept: Vec::new(), teams: Vec::new() });
                }
                let planted: Vec<Planted> = {
                    let mut stmt = tx.prepare("SELECT principal, scope, role FROM members WHERE granted_by = ?1 ORDER BY principal, scope")?;
                    let rows = stmt.query_map([OPERATOR], |r| Ok(Planted { principal: r.get(0)?, scope: r.get(1)?, role: r.get(2)? }))?;
                    rows.collect::<rusqlite::Result<_>>()?
                };
                let (kept, grants): (Vec<Planted>, Vec<Planted>) = planted.into_iter().partition(|p| admins.contains(&p.principal));
                for g in &grants {
                    members::delete_row(tx, &g.principal, &g.scope)?;
                }
                let teams: Vec<(String, String)> = {
                    let mut stmt = tx.prepare("SELECT team, principal FROM team_members WHERE added_by = ?1 ORDER BY team, principal")?;
                    let rows = stmt.query_map([OPERATOR], |r| Ok((r.get(0)?, r.get(1)?)))?;
                    rows.collect::<rusqlite::Result<_>>()?
                };
                tx.execute("DELETE FROM team_members WHERE added_by = ?1", [OPERATOR])?;
                let removed = said(&grants, &teams).join("\n");
                let kept_said = kept.iter().map(Planted::said).collect::<Vec<_>>().join("\n");
                tx.execute("INSERT INTO open_purge (one, at, removed, kept) VALUES (1, ?1, ?2, ?3)", params![now, removed, kept_said])?;
                Ok(Purged { done: None, grants, kept, teams })
            },
            |cache, p| {
                for g in &p.grants {
                    // A row the cache could not read (ACCESS-48) was never held.
                    if let (Ok(who), Ok(scope)) = (Grantee::try_from(g.principal.as_str()), Scope::try_from(g.scope.as_str())) {
                        cache.members.remove(&who, &scope);
                    }
                }
                for (team, principal) in &p.teams {
                    let team = TeamName::stored(team);
                    if let Some(r) = cache.members.records.get_mut(&team) {
                        r.members.remove(principal.as_str());
                    }
                    cache.members.set_team(principal, team, false);
                }
            },
            |p, (), _| match p.done {
                Some(at) => OpenPurge::Done { at },
                None => OpenPurge::Ran { removed: said(&p.grants, &p.teams), kept: p.kept.iter().map(Planted::said).collect() },
            },
        )
        .await
    }
}

/// Each removal as the report says it.
fn said(grants: &[Planted], teams: &[(String, String)]) -> Vec<String> {
    grants.iter().map(Planted::said).chain(teams.iter().map(|(t, p)| format!("{p} in team:{t}"))).collect()
}
