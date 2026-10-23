//! ORG-13: **epics** — a level of the organization that holds shortcodes,
//! made, renamed, deleted and chosen; and the read a reader is in.
//!
//! Owner, 2026-10-23: *"another mechanism in the organization structure. it
//! should contain scopes and they should be renumbered such that we can
//! start completely fresh in a new epic"* — then *"just do the naive though
//! technically correct implementation. an epic starts fresh, we can move
//! tickets in manually."* Inside, the level is an **era** (the `eras`
//! table, [`Namespace::era`]): `epic` already names a shortcode's letters
//! and a scope's status.
//!
//! **Where it sits.** organization → epic → shortcode → ticket. Every
//! shortcode is in one epic — a root its own, a hosted shortcode its host's
//! ([`Cache::namespaces`] keeps them so) — and ORG-3's namespace gains it:
//! a code is unique within (org, epic, host), so `MENGER` can be a root
//! again in a new epic, its tickets numbered from 1 (its id the store's
//! `MENGER.N`; its address `hypermemetic:fresh:MENGER-1`). The first epic
//! of each organization, `genesis`, holds everything made before ORG-13
//! ([`super::shortcodes::ensure_eras`], additive) and its addresses name no
//! epic, so every address and id written before keeps its meaning.
//!
//! **Which epic a reader is in** ([`Registry::current_era`]): its choice in
//! each organization (`epics switch`, the `era_choices` table), else the
//! newest. A short form resolves there first, then as before (pins, then
//! every match) — read from the call's act ([`Registry::preferred_now`]).
//! A filtered read answers there unless its filter names `epic`
//! ([`Store::era_filter`]); while no organization holds two epics nothing
//! is added, so a file with one epic reads exactly as before.
//!
//! **Moving in by hand**: `reshortcode --to org:epic:CODE` (ACT-6), and a
//! `create --epic org:epic:CODE` — the address names the epic.

use super::shortcodes::{Era, Registry};
use super::*;
use crate::access::{On, Viewer};
use crate::address::{EpicName, OrgId};
use crate::epics::Epic;
use crate::filter::{Filter, EPIC_ANY, EPIC_MARK};
use crate::members::Scope;
use rusqlite::params;

impl Registry {
    /// ORG-13: the shortcodes an epic's `under` name (`~NAME`, `~ORG:NAME`,
    /// `~*`) binds to — every one in each epic it names, in the store's order.
    pub(super) fn in_epic_named(&self, under: &str) -> Vec<ShortcodeId> {
        let u = under.trim();
        if u == EPIC_ANY {
            return self.entries.keys().cloned().collect();
        }
        let rest = u.trim_start_matches(EPIC_MARK).to_ascii_lowercase();
        let (org, name) = match rest.split_once(':') {
            Some((o, n)) => (self.org_named(o).map(|(row, _)| row), n.to_string()),
            None => (None, rest.clone()),
        };
        if rest.contains(':') && org.is_none() {
            return Vec::new();
        }
        let eras: Vec<i64> = self.eras.iter().filter(|(_, e)| e.name.as_str() == name && org.is_none_or(|o| e.org == o)).map(|(id, _)| *id).collect();
        self.entries.iter().filter(|(_, e)| eras.contains(&e.sc.ns.era)).map(|(id, _)| id.clone()).collect()
    }
}

/// `org`'s row and its name now, or [`StoreError::NoOrg`].
fn org_row(r: &Registry, org: &OrgId) -> Result<(i64, OrgId), StoreError> {
    r.org_named(org.as_str()).map(|(row, o)| (row, o.clone())).ok_or_else(|| StoreError::NoOrg(org.to_string()))
}

/// `org`'s epic `name`, or not found by its name.
fn era_in(r: &Registry, org: i64, name: &str) -> Result<i64, StoreError> {
    r.era_named(org, name).ok_or_else(|| StoreError::NotFound(name.trim().to_string()))
}

/// One epic as [`Epic`] says it, for `principal`.
fn card(cache: &Cache, era: i64, principal: &str) -> Option<Epic> {
    let r = &cache.shortcodes;
    let e = r.eras.get(&era)?;
    let org = r.orgs.get(&e.org)?.clone();
    let (mut shortcodes, mut tickets) = (0u32, 0u64);
    for entry in r.entries.values().filter(|x| x.sc.ns.era == era) {
        shortcodes += 1;
        tickets += cache.ids.held(&entry.sc.id);
    }
    Some(Epic {
        org,
        name: e.name.clone(),
        first: r.first_era(e.org) == Some(era),
        current: r.current_era(principal, e.org) == Some(era),
        shortcodes,
        tickets: tickets.min(u64::from(u32::MAX)) as u32,
        created_unix: e.created,
    })
}

/// ACCESS-11: whether `by` administers `org`: admin on it, or on `*`.
fn org_admin(by: &Authority, org: &OrgId) -> Result<(), StoreError> {
    let what = "an epic's making, name and end need admin on its organization (or on *)";
    by.admin_on(&Scope::Org(org.clone()), what).or_else(|_| by.admin_on(&Scope::Everywhere, what))
}

impl Store {
    /// ORG-13: `org`'s epics, oldest first, as `principal` sees them.
    pub fn epics(&self, org: &OrgId, principal: &str) -> Result<Vec<Epic>, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let (row, _) = org_row(&cache.shortcodes, org)?;
        Ok(cache.shortcodes.eras.iter().filter(|(_, e)| e.org == row).filter_map(|(id, _)| card(&cache, *id, principal)).collect())
    }

    /// ORG-13: `org`'s epic `name` — not found by its name when there is none.
    pub fn epic(&self, org: &OrgId, name: &str, principal: &str) -> Result<Epic, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let (row, _) = org_row(&cache.shortcodes, org)?;
        let era = era_in(&cache.shortcodes, row, name)?;
        card(&cache, era, principal).ok_or_else(|| StoreError::NotFound(name.to_string()))
    }

    /// ORG-13: `principal`'s current epic in `org` — its choice, else the newest.
    pub fn current_epic(&self, org: &OrgId, principal: &str) -> Result<Epic, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let (row, _) = org_row(&cache.shortcodes, org)?;
        let era = cache.shortcodes.current_era(principal, row).ok_or_else(|| StoreError::NotFound(org.to_string()))?;
        card(&cache, era, principal).ok_or_else(|| StoreError::NotFound(org.to_string()))
    }

    /// ORG-13: what a filtered read by `principal` is held to — the epic it
    /// is in, in each organization `orgs` names (`None`: every one): `epic
    /// org:name or …`. `None` while no organization holds two epics, or for
    /// a filter that names `epic` itself.
    pub fn era_filter(&self, principal: &str, orgs: Option<&[OrgId]>, asked: &Filter) -> Option<Filter> {
        if asked.names_epic() {
            return None;
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let r = &cache.shortcodes;
        if !r.several_eras() {
            return None;
        }
        let names: Vec<String> = r
            .orgs
            .iter()
            .filter(|(_, o)| orgs.is_none_or(|held| held.contains(o)))
            .filter_map(|(row, o)| r.current_era(principal, *row).and_then(|e| r.eras.get(&e)).map(|e| format!("epic {o}:{}", e.name)))
            .collect();
        match names.is_empty() {
            true => None,
            false => Filter::parse(&names.join(" or ")).ok(),
        }
    }

    /// ORG-13: makes the epic `name` in `org`, empty — unique in it. Its
    /// shortcodes will be numbered from 1. Needs admin on `org` (or `*`).
    pub async fn create_epic(&self, by: impl Into<Authority>, org: &OrgId, name: &EpicName, principal: &str) -> Result<Epic, StoreError> {
        let by = by.into();
        let serial = self.serial().await;
        let (row, id) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = &cache.shortcodes;
            let (row, o) = org_row(r, org)?;
            org_admin(&by, &o)?;
            if r.era_named(row, name).is_some() {
                return Err(StoreError::Invalid(format!("{o} has an epic `{name}` already: an epic's name is unique in its organization (nothing was changed)")));
            }
            (row, r.eras.keys().next_back().map_or(1, |l| l + 1))
        };
        let now = chrono::Utc::now().timestamp();
        let (stored, kept, who) = (name.clone(), name.clone(), principal.to_string());
        self.write(
            serial,
            move |tx| {
                tx.execute("INSERT INTO eras (id, org, name, created_unix) VALUES (?1, ?2, ?3, ?4)", params![id, row, stored.as_str(), now])?;
                Ok(())
            },
            move |cache, ()| {
                cache.shortcodes.eras.insert(id, Era { org: row, name: kept, created: now });
                card(cache, id, &who)
            },
            move |(), epic, _| epic.ok_or_else(|| StoreError::Invalid("the epic was not kept".into())),
        )
        .await?
    }

    /// ORG-13: renames `org`'s epic `name` to `to`. Every address naming
    /// it moves to the new name (the first's name no address holds); each
    /// of its groups is said again. Needs admin on `org` (or `*`).
    pub async fn rename_epic(&self, by: impl Into<Authority>, org: &OrgId, name: &str, to: &EpicName, principal: &str) -> Result<Epic, StoreError> {
        let by = by.into();
        let serial = self.serial().await;
        let era = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = &cache.shortcodes;
            let (row, o) = org_row(r, org)?;
            org_admin(&by, &o)?;
            let era = era_in(r, row, name)?;
            if r.era_named(row, to).is_some_and(|e| e != era) {
                return Err(StoreError::Invalid(format!("{o} has an epic `{to}` already (nothing was changed)")));
            }
            era
        };
        let (stored, kept, who) = (to.clone(), to.clone(), principal.to_string());
        self.write(
            serial,
            move |tx| {
                tx.execute("UPDATE eras SET name = ?2 WHERE id = ?1", params![era, stored.as_str()])?;
                Ok(())
            },
            move |cache, ()| {
                if let Some(e) = cache.shortcodes.eras.get_mut(&era) {
                    e.name = kept;
                }
                let moved = cache.shortcodes.readdress();
                for id in &moved {
                    cache.readdress_rows(id);
                }
                let groups: Vec<Group> = moved.iter().filter_map(|id| cache.rollups.group(id)).map(|g| cache.dress(g)).collect();
                (card(cache, era, &who), groups)
            },
            move |(), (epic, groups), tx| {
                broadcast_groups(tx, groups);
                epic.ok_or_else(|| StoreError::Invalid("the epic was not kept".into()))
            },
        )
        .await?
    }

    /// ORG-13: deletes `org`'s epic `name` — refused for its organization's
    /// first and while any shortcode is in it (a ticket, or one moved out,
    /// whose numbers it keeps). Every choice of it goes with it: those
    /// readers are in the newest again. Needs admin on `org` (or `*`).
    pub async fn delete_epic(&self, by: impl Into<Authority>, org: &OrgId, name: &str) -> Result<(OrgId, EpicName), StoreError> {
        let by = by.into();
        let serial = self.serial().await;
        let (era, o, gone) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = &cache.shortcodes;
            let (row, o) = org_row(r, org)?;
            org_admin(&by, &o)?;
            let era = era_in(r, row, name)?;
            if r.first_era(row) == Some(era) {
                return Err(StoreError::Invalid(format!("`{name}` is {o}'s first epic — everything before epics is in it — and is not deleted (nothing was changed)")));
            }
            let held = r.entries.values().filter(|e| e.sc.ns.era == era).count();
            if held > 0 {
                return Err(StoreError::Invalid(format!("`{name}` holds {held} shortcode(s): an epic is deleted only empty (nothing was changed)")));
            }
            (era, o, r.eras[&era].name.clone())
        };
        self.write(
            serial,
            move |tx| {
                tx.execute("DELETE FROM era_choices WHERE era = ?1", [era])?;
                tx.execute("DELETE FROM eras WHERE id = ?1", [era])?;
                Ok(())
            },
            move |cache, ()| {
                cache.shortcodes.eras.remove(&era);
                cache.shortcodes.choices.retain(|_, e| *e != era);
            },
            move |(), (), _| (o, gone),
        )
        .await
    }

    /// ORG-13: `principal`'s current epic in `org` set to `name` — a
    /// reader's own setting, so a viewer's (`on`, held on the organization).
    pub async fn switch_epic(&self, on: On<Viewer>, org: &OrgId, name: &str, principal: &str) -> Result<Epic, StoreError> {
        let serial = self.serial().await;
        let (row, era) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = &cache.shortcodes;
            let (row, o) = org_row(r, org)?;
            if on.scope() != &Scope::Org(o.clone()) {
                return Err(StoreError::Invalid(format!("the proof is for {}, not {o}: (nothing was changed)", on.scope())));
            }
            (row, era_in(r, row, name)?)
        };
        let (who, kept) = (principal.to_string(), principal.to_string());
        self.write(
            serial,
            move |tx| {
                tx.execute(
                    "INSERT INTO era_choices (principal, org, era) VALUES (?1, ?2, ?3)
                         ON CONFLICT (principal, org) DO UPDATE SET era = excluded.era",
                    params![who, row, era],
                )?;
                Ok(())
            },
            move |cache, ()| {
                cache.shortcodes.choices.insert((kept.clone(), row), era);
                card(cache, era, &kept)
            },
            move |(), epic, _| epic.ok_or_else(|| StoreError::Invalid("the epic was not kept".into())),
        )
        .await?
    }

    /// ORG-13: whether shortcode `sc` is in an epic `principal` is in — the
    /// nav's `groups` and `extents` hold a reader to those (every one while
    /// no organization holds two).
    pub fn in_current_era(&self, principal: &str, sc: &ShortcodeId) -> bool {
        let cache = self.tickets.read().expect("store lock poisoned");
        let r = &cache.shortcodes;
        if !r.several_eras() {
            return true;
        }
        let era = r.era_of(sc);
        let Some(org) = r.eras.get(&era).map(|e| e.org) else { return true };
        r.current_era(principal, org) == Some(era)
    }
}

// ── Who reads: the caller a read's short forms are resolved for ─────────

tokio::task_local! {
    static READER: String;
}

/// ORG-13: `s`, every poll of it read as `principal` — a short form it
/// resolves is read in that principal's current epic first
/// ([`Registry::preferred_now`]). A write's call says its caller through
/// its act already ([`super::revisions::acting`]).
pub fn reading_as<S: futures::Stream + Send + 'static>(principal: String, s: S) -> impl futures::Stream<Item = S::Item> + Send + 'static {
    Reading { who: principal, inner: Box::pin(s) }
}

/// ORG-13: `f` read as `principal`, at once — [`reading_as`] for a read
/// that answers in one go.
pub fn reading_now<T>(principal: String, f: impl FnOnce() -> T) -> T {
    READER.sync_scope(principal, f)
}

/// ORG-13: the principal a read or a write runs for, when one does.
pub(super) fn reader_now() -> Option<String> {
    READER.try_with(Clone::clone).ok().or_else(|| super::revisions::caller_act().map(|a| a.actor))
}

struct Reading<S> {
    who: String,
    inner: std::pin::Pin<Box<S>>,
}

impl<S: futures::Stream> futures::Stream for Reading<S> {
    type Item = S::Item;
    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<S::Item>> {
        let this = self.get_mut();
        let inner = this.inner.as_mut();
        READER.sync_scope(this.who.clone(), || inner.poll_next(cx))
    }
}
