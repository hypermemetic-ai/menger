//! ORG-4: **organizations** — made, renamed and deleted; the scopes a grant
//! names inside them; and where a target lives, org and all, as the gate
//! asks it.
//!
//! An organization is a row of the `orgs` table ORG-3 made (`hypermemetic`
//! is 1, the first, which holds today's tracker). Owner, 2026-10-13 (ORG-1):
//! *"we need to be able to easily create new orgs and scope roles to
//! them"* — so one write makes one ([`Store::create_org`]), empty, its
//! first admin's grant in the same transaction; its name is unique
//! app-wide, among every name an organization has or had.
//!
//! **Scopes** ([`Scope`], ACCESS-37's grammar): a grant is held on `*`, on
//! an organization (`acme:`, every ticket in it) or on a shortcode in one
//! (`acme:MENGER` — its identity, so `acme:MENGER.12` for a namesake —
//! that shortcode and every shortcode hosted under it). A reader writes a
//! shortcode by any short form of its address; [`Store::scope_named`] keeps
//! it by identity. The first open by this build moves every grant from
//! before ORG-4 into the first organization — `IDENTITY` becomes
//! `hypermemetic:IDENTITY`, `*` stays site-wide ([`settle`]).
//!
//! **Where a target lives** ([`Store::gate_chain`], the gate's
//! `Resolve::chain`): ORG-3's chain of identities, each shortcode written
//! `org:ID`, then the organization itself, `org:`. A target naming a ticket
//! is put first as written (folded) — the name a refusal says — so a ticket
//! in an organization the caller holds nothing in is refused in the words
//! a ticket that does not exist is: the gate never names another org's
//! shortcode to a stranger. ACCESS-21: a ticket scope (`under ID`, `ID`)
//! held on a membership is a key, which the chain already holds.
//!
//! **Renamed** ([`Store::rename_org`]): the old name becomes an alias (the
//! `org_aliases` table, ACT-6's way with shortcodes): an address or a scope
//! written with it resolves, every row's full address moves to the new
//! name, and every grant on the org is rewritten in the same transaction.
//! **Deleted** ([`Store::delete_org`]): refused while it holds a shortcode
//! — a ticket, or a deleted one's tombstone, which `undelete` can bring
//! back — and never the first; its grants and aliases go with it.

use super::shortcodes::{Entry, Found, OrgReach, Registry};
use super::*;
use crate::address::{folded, OrgId};
use crate::members::{Grant, Scope, EVERYWHERE};
use crate::orgs::Org;
use rusqlite::params;

impl Registry {
    /// The organization `name` names — its name now, or one it had before a
    /// rename: its row and its name now.
    pub(super) fn org_named(&self, name: &str) -> Option<(i64, &OrgId)> {
        let n = name.trim();
        if let Some((id, org)) = self.orgs.iter().find(|(_, o)| o.as_str() == n) {
            return Some((*id, org));
        }
        let id = self.aliases.get(n)?;
        self.orgs.get(id).map(|org| (*id, org))
    }

    /// `org` as it is called now — `None` when it names no organization.
    pub(super) fn canonical(&self, org: &OrgId) -> Option<&OrgId> {
        self.org_named(org.as_str()).map(|(_, o)| o)
    }

    /// The first organization (the lowest row: `hypermemetic`, 1) — where a
    /// shortcode nobody placed is, and what a reader who names no org means
    /// when login is off.
    pub(super) fn first_org(&self) -> OrgId {
        self.orgs.values().next().cloned().unwrap_or_else(OrgId::hypermemetic)
    }

    /// Every organization, in row order.
    pub(super) fn org_list(&self) -> impl Iterator<Item = (i64, &OrgId)> {
        self.orgs.iter().map(|(id, o)| (*id, o))
    }

    /// The shortcodes of `org`, in the store's order.
    pub(super) fn in_org<'r>(&'r self, org: &'r OrgId) -> impl Iterator<Item = &'r Entry> + 'r {
        self.entries.values().filter(move |e| e.sc.ns.org == *org)
    }

    /// `org`'s root shortcodes — those hosted under none.
    pub(super) fn roots_of(&self, org: &OrgId) -> Vec<ShortcodeId> {
        self.in_org(org).filter(|e| e.sc.ns.host.is_none()).map(|e| e.sc.id.clone()).collect()
    }

    /// The names `row` had before, sorted.
    fn aliases_of(&self, row: i64) -> Vec<String> {
        self.aliases.iter().filter(|(_, r)| **r == row).map(|(n, _)| n.clone()).collect()
    }

    /// Why `name` cannot be a new organization's name, if it cannot: an
    /// organization has it, or had it (it still names that one).
    fn taken_name(&self, name: &OrgId) -> Option<String> {
        if self.orgs.values().any(|o| o == name) {
            return Some(format!("an organization `{name}` is already there: an organization's name is unique (nothing was changed)"));
        }
        self.aliases.get(name.as_str()).and_then(|r| self.orgs.get(r)).map(|now| {
            format!("`{name}` was the name of the organization `{now}` and still names it: pick another (nothing was changed)")
        })
    }
}

/// The organization `name` names, or [`StoreError::NoOrg`].
fn org_in<'r>(r: &'r Registry, name: &str) -> Result<(i64, &'r OrgId), StoreError> {
    r.org_named(name.trim().trim_end_matches(':')).ok_or_else(|| StoreError::NoOrg(name.trim().to_string()))
}

/// One organization as [`Org`] says it.
fn org_card(cache: &Cache, row: i64, org: &OrgId) -> Org {
    let r = &cache.shortcodes;
    let mut shortcodes = 0u32;
    let mut tickets = 0u64;
    let mut roots = Vec::new();
    for e in r.in_org(org) {
        shortcodes += 1;
        tickets += cache.ids.held(&e.sc.id);
        if e.sc.ns.host.is_none() {
            roots.push(e.address.clone());
        }
    }
    Org { name: org.clone(), aliases: r.aliases_of(row), roots, shortcodes, tickets: tickets.min(u64::from(u32::MAX)) as u32 }
}

/// A grant's scope moved from `old:` to `new:` — `None` for a scope not in
/// that organization.
fn moved_scope(scope: &str, old: &str, new: &OrgId) -> Option<String> {
    let rest = scope.strip_prefix(old)?.strip_prefix(':')?;
    Some(format!("{new}:{rest}"))
}

/// Whether a grant's scope is in the organization named `names` (its name
/// and every alias).
fn scope_in(scope: &str, names: &[String]) -> bool {
    scope.split_once(':').is_some_and(|(o, _)| names.iter().any(|n| n == o))
}

impl Store {
    /// ORG-4: every organization, in row order, from the cache.
    pub fn orgs(&self) -> Vec<Org> {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.shortcodes.org_list().map(|(row, o)| org_card(&cache, row, o)).collect()
    }

    /// ORG-4: the organization `name` names (its name now, or one it had),
    /// from the cache.
    pub fn org(&self, name: &str) -> Option<Org> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let (row, o) = cache.shortcodes.org_named(name.trim().trim_end_matches(':'))?;
        Some(org_card(&cache, row, o))
    }

    /// ORG-4: the organization a ticket or a shortcode is in — any address,
    /// as the resolver reads it; `None` when it names none (or several).
    pub fn org_of(&self, text: &str) -> Option<OrgId> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let sc = match cache.find_ticket(text, OrgReach::All) {
            Found::One(key) => cache.get(&key).map(|r| r.shortcode.clone())?,
            _ => match cache.shortcodes.find_shortcode(text, OrgReach::All) {
                Found::One(sc) => sc,
                _ => return None,
            },
        };
        cache.shortcodes.org(&sc).cloned()
    }

    /// ORG-4: the full address of the one ticket `text` names in `orgs` —
    /// any short form, resolved as a reader of those organizations reads it
    /// (a pin to a ticket elsewhere is no answer there). `None`: it names
    /// none there, or several.
    pub fn address_in(&self, text: &str, orgs: &[OrgId]) -> Option<String> {
        let cache = self.tickets.read().expect("store lock poisoned");
        match cache.find_ticket(text, OrgReach::Only(orgs)) {
            Found::One(key) => cache.get(&key).map(|r| r.address.clone()),
            _ => None,
        }
    }

    /// ORG-4: makes the organization `name` — empty, unique app-wide (among
    /// every name an organization has or had) — and, when `admin` names one,
    /// that principal's admin grant on it (`name:`), in one transaction.
    /// The organization and the grant as kept.
    pub async fn create_org(&self, name: &str, admin: Option<(String, String)>) -> Result<(Org, Option<Grant>), StoreError> {
        let org = OrgId::new(name.trim()).map_err(StoreError::Invalid)?;
        let serial = self.serial().await;
        let row = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = &cache.shortcodes;
            if let Some(why) = r.taken_name(&org) {
                return Err(StoreError::Invalid(why));
            }
            r.orgs.keys().next_back().map_or(1, |last| last + 1)
        };
        let grant = admin.map(|(principal, by)| Grant {
            principal,
            scope: Scope::Org(org.clone()).text(),
            role: crate::members::Role::Admin,
            granted_by: by,
            at: chrono::Utc::now().timestamp(),
        });
        let (name, stored, kept) = (org.clone(), grant.clone(), grant.clone());
        self.write(
            serial,
            move |tx| {
                tx.execute("INSERT INTO orgs (id, name) VALUES (?1, ?2)", params![row, name.as_str()])?;
                if let Some(g) = &stored {
                    members::store(tx, g)?;
                }
                Ok(())
            },
            move |cache, ()| {
                cache.shortcodes.orgs.insert(row, org.clone());
                if let Some(g) = kept {
                    cache.members.put(g);
                }
                org_card(cache, row, &org)
            },
            move |(), org, _| (org, grant),
        )
        .await
    }

    /// ORG-4: renames the organization `from` (its name now or an old one)
    /// to `to` — unique as a new one's is. The old name becomes an alias,
    /// so every address and scope written with it keeps resolving; every
    /// grant on the organization is rewritten to the new name, every row's
    /// full address moves, and each of its groups is said again
    /// (`group_changed`). Renaming to an alias it had takes the name back.
    pub async fn rename_org(&self, from: &str, to: &str) -> Result<Org, StoreError> {
        let to = OrgId::new(to.trim()).map_err(StoreError::Invalid)?;
        let serial = self.serial().await;
        let (row, old, moved) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = &cache.shortcodes;
            let (row, old) = org_in(r, from)?;
            let old = old.clone();
            if old == to {
                return Ok(org_card(&cache, row, &old));
            }
            let own_alias = r.aliases.get(to.as_str()) == Some(&row);
            if let Some(why) = r.taken_name(&to).filter(|_| !own_alias) {
                return Err(StoreError::Invalid(why));
            }
            let moved: Vec<(Grant, String)> = cache
                .members
                .all()
                .into_iter()
                .filter_map(|g| moved_scope(&g.scope, old.as_str(), &to).map(|s| (g, s)))
                .collect();
            (row, old, moved)
        };
        let (new, stored, kept) = (to.clone(), old.clone(), moved.clone());
        let tx_moved = moved;
        self.write(
            serial,
            move |tx| {
                tx.execute("UPDATE orgs SET name = ?2 WHERE id = ?1", params![row, new.as_str()])?;
                tx.execute("DELETE FROM org_aliases WHERE name = ?1", [new.as_str()])?;
                tx.execute("INSERT OR REPLACE INTO org_aliases (name, org) VALUES (?1, ?2)", params![stored.as_str(), row])?;
                for (g, scope) in &tx_moved {
                    members::delete(tx, &g.principal, &g.scope)?;
                    members::store(tx, &Grant { scope: scope.clone(), ..g.clone() })?;
                }
                Ok(())
            },
            move |cache, ()| {
                let r = &mut cache.shortcodes;
                r.orgs.insert(row, to.clone());
                r.aliases.remove(to.as_str());
                r.aliases.insert(old.as_str().to_string(), row);
                let ids: Vec<ShortcodeId> = r.entries.values().filter(|e| e.org == row).map(|e| e.sc.id.clone()).collect();
                for id in &ids {
                    if let Some(e) = r.entries.get_mut(id) {
                        e.sc.ns.org = to.clone();
                    }
                }
                r.readdress();
                for id in &ids {
                    cache.readdress_rows(id);
                }
                for (g, scope) in kept {
                    cache.members.remove(&g.principal, &g.scope);
                    cache.members.put(Grant { scope, ..g });
                }
                let groups: Vec<Group> = ids.iter().filter_map(|id| cache.rollups.group(id)).map(|g| cache.dress(g)).collect();
                (org_card(cache, row, &to), groups)
            },
            move |(), (org, groups), tx| {
                broadcast_groups(tx, groups);
                org
            },
        )
        .await
    }

    /// ORG-4: deletes the organization `name` — refused while it holds a
    /// shortcode (a ticket, or a deleted ticket's tombstone, which
    /// `undelete` brings back), and for the first organization, which every
    /// shortcode nobody placed is in. Its grants and its old names go with
    /// it. The organization's name, and how many grants went.
    pub async fn delete_org(&self, name: &str) -> Result<(OrgId, u32), StoreError> {
        let serial = self.serial().await;
        let (row, org, grants) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = &cache.shortcodes;
            let (row, org) = org_in(r, name)?;
            let org = org.clone();
            if r.orgs.keys().next() == Some(&row) {
                return Err(StoreError::Invalid(format!(
                    "{org} is the first organization — where every shortcode nobody placed is — and is not deleted (nothing was changed)"
                )));
            }
            let held: Vec<&Entry> = r.in_org(&org).collect();
            if !held.is_empty() {
                let tickets: u64 = held.iter().map(|e| cache.ids.held(&e.sc.id)).sum();
                return Err(StoreError::Invalid(format!(
                    "{org} holds {} shortcode(s) and {tickets} ticket(s) — delete or move them first: an organization is deleted \
                     only empty (nothing was changed)",
                    held.len()
                )));
            }
            let mut names = r.aliases_of(row);
            names.push(org.as_str().to_string());
            let grants: Vec<Grant> = cache.members.all().into_iter().filter(|g| scope_in(&g.scope, &names)).collect();
            (row, org, grants)
        };
        let n = grants.len() as u32;
        let gone = grants.clone();
        self.write(
            serial,
            move |tx| {
                for g in &grants {
                    members::delete(tx, &g.principal, &g.scope)?;
                }
                tx.execute("DELETE FROM org_aliases WHERE org = ?1", [row])?;
                tx.execute("DELETE FROM orgs WHERE id = ?1", [row])?;
                Ok(())
            },
            move |cache, ()| {
                cache.shortcodes.orgs.remove(&row);
                cache.shortcodes.aliases.retain(|_, r| *r != row);
                for g in gone {
                    cache.members.remove(&g.principal, &g.scope);
                }
            },
            move |(), (), _| (org, n),
        )
        .await
    }

    /// ORG-4: a scope as a reader writes it — `*`, an organization (`acme:`,
    /// or a name it had), or a shortcode by any short form of its address —
    /// as the store keeps it ([`Scope::text`]): the organization's name now,
    /// the shortcode's identity. A shortcode no ticket is in yet, written as
    /// a bare code, is kept as that code in the organization it names (the
    /// first, unnamed) — the id a root of that code takes when it is born
    /// there. Refused: a name that is no scope, an organization there is
    /// not, a shortcode naming several.
    pub fn scope_named(&self, text: &str) -> Result<String, StoreError> {
        let t = crate::members::scope_of(text).map_err(StoreError::Invalid)?;
        if t == EVERYWHERE {
            return Ok(t);
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let r = &cache.shortcodes;
        if let Some(org) = t.strip_suffix(':') {
            return org_in(r, org).map(|(_, o)| Scope::Org(o.clone()).text());
        }
        if let Some(Scope::Shortcode { org, id }) = Scope::stored(&t) {
            let now = org_in(r, org.as_str())?.1.clone();
            if r.org(&id) == Some(&now) {
                return Ok(Scope::Shortcode { org: now, id }.text());
            }
        }
        let a = Address::parse(&t).map_err(StoreError::Invalid)?;
        let org = match &a.org {
            Some(o) => Some(org_in(r, o.as_str())?.1.clone()),
            None => None,
        };
        let asked = match &org {
            Some(o) => Address { org: Some(o.clone()), ..a.clone() }.to_string(),
            None => t.clone(),
        };
        match r.find_shortcode(&asked, OrgReach::All) {
            Found::One(id) => Ok(Scope::Shortcode { org: r.org(&id).cloned().unwrap_or_else(|| r.first_org()), id }.text()),
            Found::Many(candidates) => Err(StoreError::Ambiguous { text: t, candidates }),
            Found::None if a.hosts.is_empty() => {
                Ok(Scope::Shortcode { org: org.unwrap_or_else(|| r.first_org()), id: ShortcodeId::new(a.code.as_str()) }.text())
            }
            Found::None => Err(StoreError::NoShortcode(t)),
        }
    }

    /// ORG-4: the scopes `target` lives in, innermost first, as the gate
    /// asks them (`Resolve::chain`, without the `*` the caller adds) —
    /// [`Store::scope_chain`]'s identities with each shortcode written
    /// `org:ID` (and its bare identity after it), then its organization, `org:`. A target naming a ticket is
    /// first as written (folded): what a refusal names, the same for a ticket
    /// out of reach as for none. An organization (`acme:`, or a name it had)
    /// is itself; a stored shortcode scope (`acme:MENGER.12`) is its
    /// shortcode's chain. `None`: it names nothing a scope could hold.
    pub fn gate_chain(&self, target: &str) -> Option<Vec<String>> {
        let t = target.trim();
        if t.is_empty() {
            return None;
        }
        let parsed = Address::parse(t).ok();
        let (raw, label) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = &cache.shortcodes;
            if let Some(org) = t.strip_suffix(':') {
                return r.org_named(org).map(|(_, o)| vec![Scope::Org(o.clone()).text()]);
            }
            let stored = match Scope::stored(t) {
                Some(Scope::Shortcode { org, id }) if r.canonical(&org).is_some() && r.canonical(&org) == r.org(&id) => Some(id),
                _ => None,
            };
            // A stored scope's shortcode: it and the shortcodes hosting it.
            let hosted = stored.map(|id| {
                std::iter::once(id.as_str().to_string()).chain(lineage::host_chain(&cache, &id).into_iter().map(|h| h.as_str().to_string())).collect()
            });
            drop(cache);
            match hosted {
                Some(chain) => (chain, None),
                None => {
                    let names_ticket = parsed.as_ref().is_none_or(Address::names_ticket);
                    (self.scope_chain(t)?, names_ticket.then(|| folded(t)))
                }
            }
        };
        let cache = self.tickets.read().expect("store lock poisoned");
        let r = &cache.shortcodes;
        // A shortcode the registry does not hold (no ticket is in it yet)
        // is in the organization its address names, else the first.
        let named_org = parsed.as_ref().and_then(|a| a.org.as_ref()).and_then(|o| r.canonical(o)).cloned();
        let fallback = named_org.unwrap_or_else(|| r.first_org());
        let mut org: Option<OrgId> = None;
        let mut out: Vec<String> = Vec::with_capacity(raw.len() + 2);
        if let Some(l) = label {
            out.push(l);
        }
        for id in raw {
            // A key holds a `-`; a shortcode's identity never does. A
            // shortcode is its scope, `org:ID`, then its bare identity — unique
            // app-wide, what a session's ceiling minted before ORG-4 names.
            let items = match id.contains('-') {
                true => vec![id],
                false => {
                    let sc = ShortcodeId::new(&id);
                    let o = r.org(&sc).cloned().unwrap_or_else(|| fallback.clone());
                    org.get_or_insert_with(|| o.clone());
                    vec![Scope::Shortcode { org: o, id: sc }.text(), id]
                }
            };
            for item in items {
                if !out.contains(&item) {
                    out.push(item);
                }
            }
        }
        out.push(Scope::Org(org.unwrap_or(fallback)).text());
        Some(out)
    }
}

/// ORG-4: a grant's scope from before ORG-4 — a bare shortcode, `IDENTITY`
/// — as this build keeps it: the shortcode of that identity in the
/// organization it is in, else (no ticket is in it) that code in the first
/// organization. `None` for a scope already kept in a form of [`Scope`].
fn legacy_scope(r: &Registry, scope: &str) -> Option<String> {
    let t = scope.trim();
    if t == EVERYWHERE || Scope::stored(t).is_some() {
        return None;
    }
    let id = ShortcodeId::new(&t.to_ascii_uppercase());
    let org = r.org(&id).cloned().unwrap_or_else(|| r.first_org());
    Some(format!("{org}:{}", id.as_str()))
}

/// ORG-4: the file's grants made to name organizations — at every open,
/// after the shortcodes are settled: every grant whose scope is a bare
/// shortcode (a grant from before ORG-4) becomes that shortcode in its
/// organization (`IDENTITY` → `hypermemetic:IDENTITY`), durably, in one
/// transaction; `*` stays site-wide. A file from after ORG-4 has none, and
/// nothing is written. Returns how many it moved.
pub(super) fn settle(conn: &mut rusqlite::Connection, cache: &mut Cache) -> anyhow::Result<usize> {
    let moved: Vec<(Grant, String)> =
        cache.members.all().into_iter().filter_map(|g| legacy_scope(&cache.shortcodes, &g.scope).map(|s| (g, s))).collect();
    if moved.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    for (g, scope) in &moved {
        members::delete(&tx, &g.principal, &g.scope)?;
        members::store(&tx, &Grant { scope: scope.clone(), ..g.clone() })?;
    }
    tx.commit()?;
    let org = cache.shortcodes.first_org();
    let unplaced = moved.iter().filter(|(_, s)| Scope::stored(s).is_none()).count();
    for (g, scope) in &moved {
        cache.members.remove(&g.principal, &g.scope);
        cache.members.put(Grant { scope: scope.clone(), ..g.clone() });
    }
    tracing::warn!(
        "ORG-4: moved {} grant(s) from before organizations into their organization ({org} for every shortcode in it; \
         `*` stays site-wide){}, durably; every later open reads them",
        moved.len(),
        match unplaced {
            0 => String::new(),
            n => format!(" — {n} named no shortcode this build can place and hold nowhere"),
        }
    );
    Ok(moved.len())
}
