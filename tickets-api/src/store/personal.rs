//! V2-41: **a person's own organization**. Owner, 2026-11-04: *"automatically
//! create an organization that belongs to the user. users can join other orgs
//! but they always have their personal org, and they are always editor inside
//! it. this happens after being accepted onto the platform"*.
//!
//! **Accepted** is the first grant a person holds: one to them by name
//! (`members grant`, `auth grant`, `orgs create --admin`), one to a team they
//! are in, or being added to a team that holds one. The write that keeps it
//! also makes the organization, in its own transaction: the row in `orgs`,
//! its owner in `personal_orgs`, its first epic, and the owner's `editor` on
//! it (signed [`GrantedBy::Personal`]). A person is a principal of `idp`,
//! `oidc` or `nostr`; an agent's key (`key`, `apikey`) gets none.
//!
//! **Its name** is the owner's principal, hashed: `u-` and the first 10 hex
//! of its SHA-256 (`u-3f9a02c1d4`) — it names no one, and is the same at
//! every start; longer when that is taken. An admin may rename it.
//!
//! **Always editor**: a grant to the owner on it below editor is refused; a
//! revoke of their editor is refused, and of their admin puts editor back;
//! the organization is never deleted. Everything else is any organization's.
//!
//! **Backfilled** ([`Store::ensure_personal_orgs`]): at every start, each
//! person holding a grant and no organization of their own is given one —
//! the first start of this build makes everyone's; a later one finds none.

use super::shortcodes::{Era, Registry};
use super::*;
use crate::address::{EpicName, OrgId};
use crate::members::{Grant, Role, Scope};
use crate::vocab::{GrantedBy, Grantee, Principal};
use rusqlite::params;
use sha2::{Digest, Sha256};

/// Each personal organization's owner. Additive: an older build never reads
/// it, and a file gains it at its first personal organization.
const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS personal_orgs (
            org       INTEGER PRIMARY KEY REFERENCES orgs (id),
            principal TEXT NOT NULL UNIQUE
        );
    ";

/// Whether `principal` is a person's (`idp`, `oidc`, `nostr`) — not an
/// agent's key.
pub fn is_person(principal: &str) -> bool {
    matches!(principal.split_once(':').map(|(issuer, _)| issuer), Some("idp" | "oidc" | "nostr"))
}

/// The name a person's organization takes: `u-` and the first `hex` hex
/// digits of the SHA-256 of their principal.
pub fn personal_name(principal: &str, hex: usize) -> OrgId {
    let digest: String = Sha256::digest(principal.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    OrgId::new(&format!("u-{}", &digest[..hex.min(digest.len())])).expect("u- and hex is an organization's name")
}

/// The owners, as the file holds them (none before the first).
pub(super) fn load(conn: &rusqlite::Connection, r: &mut Registry) -> rusqlite::Result<()> {
    let present: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'personal_orgs'", [], |row| row.get(0))?;
    if present == 0 {
        return Ok(());
    }
    let mut stmt = conn.prepare("SELECT org, principal FROM personal_orgs")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    for row in rows {
        let (org, principal) = row?;
        r.owners.insert(org, principal);
    }
    Ok(())
}

impl Registry {
    /// The owner of the organization `org` names, when it is someone's own.
    pub(super) fn owner_of(&self, org: &OrgId) -> Option<&str> {
        let (row, _) = self.org_named(org.as_str())?;
        self.owners.get(&row).map(String::as_str)
    }

    fn owns_one(&self, principal: &str) -> bool {
        self.owners.values().any(|p| p == principal)
    }
}

/// One personal organization to make.
#[derive(Debug, Clone)]
pub(super) struct Making {
    row: i64,
    org: OrgId,
    owner: String,
    grant: Grant,
}

/// What made each: its epic's row and when.
pub(super) type Made = Vec<(i64, i64)>;

/// The organizations to make for `principals`: each person among them with
/// none, once, in rows from `first_row` on, named apart from every
/// organization there is and from `taken`.
pub(super) fn plan(cache: &Cache, principals: impl IntoIterator<Item = String>, first_row: i64, taken: &[OrgId]) -> Vec<Making> {
    let r = &cache.shortcodes;
    let mut out: Vec<Making> = Vec::new();
    for owner in principals {
        if !is_person(&owner) || r.owns_one(&owner) || out.iter().any(|m| m.owner == owner) {
            continue;
        }
        let Ok(principal) = owner.parse::<Principal>() else { continue };
        let free = |o: &OrgId| r.taken_name(o).is_none() && !taken.contains(o) && !out.iter().any(|m| m.org == *o);
        let Some(org) = (10..=64).step_by(2).map(|n| personal_name(&owner, n)).find(free) else { continue };
        let grant = Grant {
            principal: Grantee::Principal(principal),
            scope: Scope::Org(org.clone()),
            role: Role::Editor,
            granted_by: GrantedBy::Personal,
            at: chrono::Utc::now().timestamp(),
        };
        out.push(Making { row: first_row + out.len() as i64, org, owner, grant });
    }
    out
}

/// The next organization's row.
pub(super) fn next_row(cache: &Cache) -> i64 {
    cache.shortcodes.orgs.keys().next_back().map_or(1, |last| last + 1)
}

/// Keeps `making` in the caller's transaction: each organization, its
/// owner, its first epic and the owner's editor grant.
pub(super) fn keep(tx: &rusqlite::Transaction<'_>, making: &[Making]) -> rusqlite::Result<Made> {
    if making.is_empty() {
        return Ok(Vec::new());
    }
    tx.execute_batch(TABLE)?;
    for m in making {
        tx.execute("INSERT INTO orgs (id, name) VALUES (?1, ?2)", params![m.row, m.org.as_str()])?;
        tx.execute("INSERT INTO personal_orgs (org, principal) VALUES (?1, ?2)", params![m.row, m.owner])?;
        members::store(tx, &m.grant)?;
    }
    super::shortcodes::ensure_eras(tx)?;
    making
        .iter()
        .map(|m| tx.query_row("SELECT id, created_unix FROM eras WHERE org = ?1 ORDER BY id LIMIT 1", [m.row], |r| Ok((r.get(0)?, r.get(1)?))))
        .collect()
}

/// The cache moved as [`keep`] moved the file.
pub(super) fn put(cache: &mut Cache, making: Vec<Making>, made: &Made) {
    for (m, (era, created)) in making.into_iter().zip(made) {
        cache.shortcodes.orgs.insert(m.row, m.org);
        cache.shortcodes.owners.insert(m.row, m.owner);
        cache.shortcodes.eras.insert(*era, Era::new(m.row, EpicName::genesis(), *created));
        cache.members.put(m.grant);
    }
}

/// Who a grant to `to` reaches: the principal, or a team's members.
pub(super) fn reached(cache: &Cache, to: &Grantee) -> Vec<String> {
    match to {
        Grantee::Principal(p) => vec![p.as_str().to_string()],
        Grantee::Team(t) => cache.members.records.get(t).map(|r| r.members.keys().cloned().collect()).unwrap_or_default(),
        Grantee::Everyone => Vec::new(),
    }
}

/// Why `g` may not be kept, when it would leave an owner below editor in
/// their own organization.
pub(super) fn grant_refused(cache: &Cache, g: &Grant) -> Option<String> {
    let (Grantee::Principal(p), Scope::Org(org)) = (&g.principal, &g.scope) else { return None };
    let owner = cache.shortcodes.owner_of(org)?;
    (owner == p.as_str() && !matches!(g.role, Role::Editor | Role::Admin)).then(|| {
        format!("{org} is {p}'s own organization: they are always editor in it — grant editor or admin, or nothing (nothing written)")
    })
}

/// What a revoke on `scope` of `principal` does to an owner in their own
/// organization: `None` when it is no such revoke; else editor put back
/// (`Ok`, the grant) for an admin, or the refusal for editor.
pub(super) fn revoke_held(cache: &Cache, principal: &Grantee, scope: &Scope) -> Option<Result<Grant, String>> {
    let (Grantee::Principal(p), Scope::Org(org)) = (principal, scope) else { return None };
    if cache.shortcodes.owner_of(org)? != p.as_str() {
        return None;
    }
    let held = cache.members.get(p.as_str(), scope)?;
    Some(match held.role {
        Role::Admin => Ok(Grant { role: Role::Editor, granted_by: GrantedBy::Personal, at: chrono::Utc::now().timestamp(), ..held.clone() }),
        _ => Err(format!("{org} is {p}'s own organization: they are always editor in it, and that is not revoked (nothing written)")),
    })
}

impl Store {
    /// V2-41: whether `org` is a person's own — named, never defaulted to:
    /// a call that leaves the organization out goes to the one shared
    /// organization the caller holds a role in.
    pub fn is_personal(&self, org: &OrgId) -> bool {
        self.tickets.read().expect("store lock poisoned").shortcodes.owner_of(org).is_some()
    }

    /// V2-41: each person who holds a grant — by name or through a team —
    /// or is one of `admins` (this start's `--admin`), and has no
    /// organization of their own, given one, in one transaction (the module
    /// docs). The organizations made; none when everyone has theirs. As the
    /// store's [`System`]: the server's own upkeep, at its open.
    pub async fn ensure_personal_orgs(&self, _sys: &System, admins: &[String]) -> Result<Vec<OrgId>, StoreError> {
        let serial = self.serial().await;
        let making = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut who: Vec<String> = admins.to_vec();
            for g in cache.members.all() {
                who.extend(reached(&cache, &g.principal));
            }
            plan(&cache, who, next_row(&cache), &[])
        };
        if making.is_empty() {
            return Ok(Vec::new());
        }
        let names: Vec<OrgId> = making.iter().map(|m| m.org.clone()).collect();
        let kept = making.clone();
        self.write(serial, move |tx| Ok(keep(tx, &kept)?), move |cache, made| put(cache, making, made), move |_, (), _| names).await
    }
}
