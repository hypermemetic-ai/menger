//! ACCESS-22: the **teams store** — a named set of principals that holds
//! grants as one grantee (`team:<name>`, [`crate::vocab::Grantee::Team`]):
//! each member holds what the team holds, beside their own grants, the
//! higher role winning (a reach is the union of every grant).
//!
//! **Behind the cache, as everything stored is**: read whole when the store
//! opens ([`load`]), kept in [`super::members::Members`] beside the grants
//! (a change to who is in a team is a change to who holds what: the grants
//! generation moves with it, so an open watch verifies its caller again,
//! ACCESS-12); every write commits to the two tables in one transaction,
//! then moves the cache.
//!
//! **Stored** in two tables, stored form 13 ([`super::stored`]): `teams`
//! (a row per team — its name, unique app-wide; its organization's row; who
//! made it, when) and `team_members` (a row per principal in a team — who
//! added them, when). A team's grants are rows of `members` whose
//! principal column holds `team:<name>`, as ACCESS-48 made it hold a
//! grantee. A file in form 10, 11 or 12 gains the two, empty, in place.
//!
//! **The rule** (owner, 2026-10-09): a team is made by an admin of
//! somewhere; who is in it, its name and its end are changed only by one
//! who is admin on **every** scope it holds a grant on (admin of `*`, on
//! any) — [`OnTeam`], checked under the write lock against the grants as
//! they stand; granting to it is admin on the scope, as for a person.

use super::*;
use crate::access::AdminOf;
use crate::address::OrgId;
use crate::members::{Grant, Scope, Team, TeamMember};
use crate::vocab::{GrantedBy, Grantee, Principal, TeamName};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

pub(super) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS teams (
            name       TEXT PRIMARY KEY,
            org        INTEGER NOT NULL,
            created_by TEXT NOT NULL,
            at         INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS team_members (
            team       TEXT NOT NULL,
            principal  TEXT NOT NULL,
            added_by   TEXT NOT NULL,
            at         INTEGER NOT NULL,
            PRIMARY KEY (team, principal)
        );
    ";

/// One team as the cache keeps it.
#[derive(Debug, Clone)]
pub(crate) struct Record {
    /// Its organization's row ([`super::shortcodes::Registry`]): an org's
    /// rename moves nothing here.
    pub(super) org: i64,
    pub(super) created_by: GrantedBy,
    pub(super) at: i64,
    /// Its members, by principal: who added each, when.
    pub(super) members: BTreeMap<String, (GrantedBy, i64)>,
}

/// ACCESS-22: what a change to a team takes — the system's word, or admin
/// on every scope the team holds a grant on, minted by the caller as the
/// grants stood ([`crate::access::VerifiedUser::admin_of`] each). The store
/// checks it under its write lock against the grants as they stand: one
/// granted to the team since is not covered, and the write is refused.
#[derive(Debug)]
pub struct OnTeam(Option<Vec<AdminOf>>);

impl OnTeam {
    /// A user's: admin on each of the scopes the team held when it asked
    /// ([`Store::team_scopes`]) — and at least one (admin somewhere), so a
    /// team that holds nothing is still changed by an admin only.
    pub fn new(admins: Vec<AdminOf>) -> Self {
        Self(Some(admins))
    }

    /// Under the write lock: whether it covers every scope `team` holds.
    fn check(&self, cache: &Cache, team: &TeamName) -> Result<(), StoreError> {
        let Some(admins) = &self.0 else { return Ok(()) };
        if admins.is_empty() {
            return Err(StoreError::Invalid("a team is changed by an admin: nothing written".into()));
        }
        let held = held_scopes(cache, team);
        match held.iter().find(|s| !admins.iter().any(|a| a.scope() == *s || *a.scope() == Scope::Everywhere)) {
            None => Ok(()),
            Some(s) => Err(StoreError::Invalid(format!("changing team {team} needs admin on every scope it holds, and {s} is one (nothing written)"))),
        }
    }
}

impl From<&System> for OnTeam {
    fn from(_: &System) -> Self {
        Self(None)
    }
}

/// The scopes `team` holds a grant on, as the cache keeps them.
fn held_scopes(cache: &Cache, team: &TeamName) -> Vec<Scope> {
    cache.members.of(&Grantee::Team(team.clone()).to_string()).into_iter().map(|g| g.scope).collect()
}

fn refused(why: String) -> StoreError {
    StoreError::Invalid(format!("{why} (nothing written)"))
}

/// The team `name` names, or the refusal.
fn named<'c>(cache: &'c Cache, name: &TeamName) -> Result<&'c Record, StoreError> {
    cache.members.records.get(name).ok_or_else(|| refused(format!("no team {name}")))
}

/// A team as [`Team`] says it — its organization by its name now.
fn card(cache: &Cache, name: &TeamName, r: &Record) -> Team {
    let org = cache.shortcodes.orgs.get(&r.org).cloned().unwrap_or_else(|| cache.shortcodes.first_org());
    Team { name: name.clone(), org, created_by: r.created_by.clone(), at: r.at, members: r.members.len() as u32 }
}

fn member_card(team: &TeamName, principal: &str, (by, at): &(GrantedBy, i64)) -> Option<TeamMember> {
    Some(TeamMember { team: team.clone(), principal: principal.parse().ok()?, added_by: by.clone(), at: *at })
}

/// Every team and its members, as stored, into `members` — at the open,
/// after ORG-4 has settled the grants. A row naming no team, or a principal
/// or grantor in no form one holds, is held nowhere and said once.
pub(super) fn load(conn: &Connection, members: &mut super::members::Members) -> rusqlite::Result<()> {
    let mut refused = Vec::new();
    let mut stmt = conn.prepare("SELECT name, org, created_by, at FROM teams ORDER BY name")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let (name, org, by, at): (String, i64, String, i64) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
        match (name.parse::<TeamName>(), GrantedBy::try_from(by.as_str())) {
            (Ok(name), Ok(created_by)) => {
                members.records.insert(name, Record { org, created_by, at, members: BTreeMap::new() });
            }
            _ => refused.push(format!("team {name}")),
        }
    }
    let mut stmt = conn.prepare("SELECT team, principal, added_by, at FROM team_members ORDER BY team, principal")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let (team, principal, by, at): (String, String, String, i64) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
        let parsed = (team.parse::<TeamName>(), principal.parse::<Principal>(), GrantedBy::try_from(by.as_str()));
        match parsed {
            (Ok(t), Ok(p), Ok(by)) if members.records.contains_key(&t) => {
                members.records.get_mut(&t).expect("checked").members.insert(p.as_str().to_string(), (by, at));
                members.set_team(p.as_str(), t, true);
            }
            _ => refused.push(format!("{principal} in {team}")),
        }
    }
    if !refused.is_empty() {
        tracing::warn!("ACCESS-22: {} stored team row(s) in no form a team holds, held nowhere — {}", refused.len(), refused.join("; "));
    }
    Ok(())
}

impl Store {
    /// ACCESS-22: every team, by name — in `orgs` when given (a reader's
    /// organizations), from the cache. Read as the store's [`System`]: the
    /// access layer's ([`crate::access::Access::teams`]).
    pub fn teams(&self, _: &System, orgs: Option<&[OrgId]>) -> Vec<Team> {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.members.records.iter().map(|(n, r)| card(&cache, n, r)).filter(|t| orgs.is_none_or(|o| o.contains(&t.org))).collect()
    }

    /// ACCESS-22: the team `name`, its members (by principal) and its
    /// grants (by scope); `None` when there is none.
    pub fn team(&self, _: &System, name: &TeamName) -> Option<(Team, Vec<TeamMember>, Vec<Grant>)> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let r = cache.members.records.get(name)?;
        let members = r.members.iter().filter_map(|(p, m)| member_card(name, p, m)).collect();
        Some((card(&cache, name, r), members, cache.members.of(&Grantee::Team(name.clone()).to_string())))
    }

    /// ACCESS-22: the scopes `team` holds a grant on now — what a change to
    /// it needs admin on ([`OnTeam`]); `None` when there is no such team.
    pub fn team_scopes(&self, team: &TeamName) -> Option<Vec<Scope>> {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.members.records.contains_key(team).then(|| held_scopes(&cache, team))
    }

    /// ACCESS-22: makes the team `name`, empty, in `org` — its name unique
    /// app-wide, its org one there is. One row committed, then the cache.
    ///
    /// By `by`: the system, or admin on some scope (an [`AdminOf`], minted
    /// while login is on): a team is made by an admin of somewhere.
    pub async fn create_team(&self, by: impl Into<Authority>, name: TeamName, org: &OrgId, created_by: GrantedBy) -> Result<Team, StoreError> {
        by.into().administers_somewhere("a team is made by an admin of somewhere")?;
        let serial = self.serial().await;
        let row = {
            let cache = self.tickets.read().expect("store lock poisoned");
            if cache.members.records.contains_key(&name) {
                return Err(refused(format!("there is a team {name} already: a team's name is unique")));
            }
            cache.shortcodes.org_named(org.as_str()).map(|(row, _)| row).ok_or_else(|| StoreError::NoOrg(org.to_string()))?
        };
        let at = chrono::Utc::now().timestamp();
        let (stored, by_text) = (name.clone(), created_by.to_string());
        self.write(
            serial,
            move |tx| Ok(tx.execute("INSERT INTO teams (name, org, created_by, at) VALUES (?1, ?2, ?3, ?4)", params![stored.as_str(), row, by_text, at])?),
            move |cache, _| {
                let r = Record { org: row, created_by, at, members: BTreeMap::new() };
                // An empty team moves no one's grants: the generation stays.
                let team = card(cache, &name, &r);
                cache.members.records.insert(name, r);
                team
            },
            move |_, team, _| team,
        )
        .await
    }

    /// ACCESS-22: `principal` into `team` (`added_by` signing it), or the
    /// row as it was when they are in it already. One row, then the cache;
    /// the grants generation moves (ACCESS-12: their open watches follow).
    pub async fn add_to_team(&self, on: impl Into<OnTeam>, team: &TeamName, principal: &Principal, added_by: GrantedBy) -> Result<TeamMember, StoreError> {
        let on = on.into();
        let serial = self.serial().await;
        {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = named(&cache, team)?;
            on.check(&cache, team)?;
            if let Some(m) = r.members.get(principal.as_str()).and_then(|m| member_card(team, principal.as_str(), m)) {
                return Ok(m);
            }
        }
        let at = chrono::Utc::now().timestamp();
        let (t, p, by) = (team.clone(), principal.clone(), added_by.to_string());
        let member = TeamMember { team: team.clone(), principal: principal.clone(), added_by: added_by.clone(), at };
        self.write(
            serial,
            move |tx| Ok(tx.execute("INSERT INTO team_members (team, principal, added_by, at) VALUES (?1, ?2, ?3, ?4)", params![t.as_str(), p.as_str(), by, at])?),
            move |cache, _| {
                if let Some(r) = cache.members.records.get_mut(&member.team) {
                    r.members.insert(member.principal.as_str().to_string(), (added_by, at));
                }
                cache.members.set_team(member.principal.as_str(), member.team.clone(), true);
                member
            },
            move |_, member, _| member,
        )
        .await
    }

    /// ACCESS-22: `principal` out of `team`; whether they were in it. The
    /// generation moves: their open watches stop hearing the team's scopes.
    pub async fn remove_from_team(&self, on: impl Into<OnTeam>, team: &TeamName, principal: &Principal) -> Result<bool, StoreError> {
        let on = on.into();
        let serial = self.serial().await;
        {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = named(&cache, team)?;
            on.check(&cache, team)?;
            if !r.members.contains_key(principal.as_str()) {
                return Ok(false);
            }
        }
        let (t, p) = (team.clone(), principal.clone());
        let (team, principal) = (team.clone(), principal.clone());
        self.write(
            serial,
            move |tx| Ok(tx.execute("DELETE FROM team_members WHERE team = ?1 AND principal = ?2", params![t.as_str(), p.as_str()])? > 0),
            move |cache, _| {
                if let Some(r) = cache.members.records.get_mut(&team) {
                    r.members.remove(principal.as_str());
                }
                cache.members.set_team(principal.as_str(), team, false);
            },
            move |existed, (), _| existed,
        )
        .await
    }

    /// ACCESS-22: `team` renamed `to` — unique as a new one's is: the team,
    /// its members and every grant it holds (`team:<old>` → `team:<new>`),
    /// in one transaction. The team as it stands.
    pub async fn rename_team(&self, on: impl Into<OnTeam>, team: &TeamName, to: TeamName) -> Result<Team, StoreError> {
        let on = on.into();
        let serial = self.serial().await;
        let grants = {
            let cache = self.tickets.read().expect("store lock poisoned");
            named(&cache, team)?;
            on.check(&cache, team)?;
            if cache.members.records.contains_key(&to) {
                return Err(refused(format!("there is a team {to} already: a team's name is unique")));
            }
            cache.members.of(&Grantee::Team(team.clone()).to_string())
        };
        let (from, new) = (team.clone(), to.clone());
        let moved: Vec<Grant> = grants.iter().map(|g| Grant { principal: Grantee::Team(to.clone()), ..g.clone() }).collect();
        let (old_grants, new_grants) = (grants.clone(), moved.clone());
        let (from_c, to_c) = (team.clone(), to.clone());
        self.write(
            serial,
            move |tx| {
                tx.execute("UPDATE teams SET name = ?2 WHERE name = ?1", params![from.as_str(), new.as_str()])?;
                tx.execute("UPDATE team_members SET team = ?2 WHERE team = ?1", params![from.as_str(), new.as_str()])?;
                for g in &old_grants {
                    members::delete(tx, &g.principal, &g.scope)?;
                }
                for g in &new_grants {
                    members::store(tx, g)?;
                }
                Ok(())
            },
            move |cache, ()| {
                for g in &grants {
                    cache.members.remove(&g.principal, &g.scope);
                }
                for g in moved {
                    cache.members.put(g);
                }
                let r = cache.members.records.remove(&from_c).expect("checked under the lock");
                for p in r.members.keys() {
                    cache.members.set_team(p, from_c.clone(), false);
                    cache.members.set_team(p, to_c.clone(), true);
                }
                let team = card(cache, &to_c, &r);
                cache.members.records.insert(to_c, r);
                team
            },
            move |(), team, _| team,
        )
        .await
    }

    /// ACCESS-22: `team` deleted, and with it every grant it holds and
    /// every member's place in it, in one transaction: `(grants, members)`.
    pub async fn delete_team(&self, on: impl Into<OnTeam>, team: &TeamName) -> Result<(u32, u32), StoreError> {
        let on = on.into();
        let serial = self.serial().await;
        let (grants, held) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = named(&cache, team)?;
            on.check(&cache, team)?;
            (cache.members.of(&Grantee::Team(team.clone()).to_string()), r.members.len() as u32)
        };
        let n = grants.len() as u32;
        let (t, gone) = (team.clone(), grants.clone());
        let team = team.clone();
        self.write(
            serial,
            move |tx| {
                for g in &gone {
                    members::delete(tx, &g.principal, &g.scope)?;
                }
                tx.execute("DELETE FROM team_members WHERE team = ?1", params![t.as_str()])?;
                tx.execute("DELETE FROM teams WHERE name = ?1", params![t.as_str()])?;
                Ok(())
            },
            move |cache, ()| {
                for g in &grants {
                    cache.members.remove(&g.principal, &g.scope);
                }
                // Each grant and each member gone moved the generation.
                if let Some(r) = cache.members.records.remove(&team) {
                    for p in r.members.keys() {
                        cache.members.set_team(p, team.clone(), false);
                    }
                }
            },
            move |(), (), _| (n, held),
        )
        .await
    }

    /// ACCESS-22: whether `team` is one there is.
    pub fn has_team(&self, team: &TeamName) -> bool {
        self.tickets.read().expect("store lock poisoned").members.records.contains_key(team)
    }
}
