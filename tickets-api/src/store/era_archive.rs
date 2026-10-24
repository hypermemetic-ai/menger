//! ORG-23: **each person stays on their own epic; an admin archives one**.
//!
//! Owner, 2026-10-24: *"every user stays on their own default epic admins
//! though can archive an epic and that would move people to the next
//! epic"*; on `next`: the next newer epic, else the next older.
//!
//! - **The default** ([`Registry::default_era`]): a principal who never
//!   switched is on its organization's default epic — `eras.is_default`,
//!   the first live epic until an admin sets another (`epics set_default`)
//!   — never the newest, so making an epic moves nobody.
//! - **The archive** ([`Store::archive_epic`], admin of the organization,
//!   one write): the epic is marked (`archived_at`, `archived_by`); every
//!   principal on it — its choice, or the default when it was the default —
//!   moves to [`next_era`] (its choice rewritten, a move kept for its
//!   notice); the default moves too when it was this epic. The last live
//!   epic of an organization is refused. `--dry-run` answers the same and
//!   writes nothing (the app's confirm reads it).
//! - **The views**: an archived epic is no one's current one, so a reader's
//!   epic (`Store::era_filter`) never holds it; `epic any` is every live
//!   epic; a filter that names the epic, or `archived`, still answers its
//!   tickets — the mark is the registry's, kept beside the epic, so no view
//!   walks the epic's tickets to drop them. `get` says `epic_archived`.
//! - **Restore** clears the mark and moves nobody back.
//! - **The notice**: each move is kept (`era_moves`) until its principal
//!   dismisses it (`epics dismiss`); `epics current` carries it, and
//!   `epics watch` says it at once to that principal (`moved`).
//! - **Aliases**: a rename keeps the old name (`era_aliases`), so a saved
//!   filter or a link that names it still answers.
//!
//! Additive: three nullable or defaulted columns on `eras`, two tables; no
//! index changes. An older build reads none of them.

use super::eras::{card, era_in, org_admin, org_row};
use super::shortcodes::Registry;
use super::*;
use crate::access::{On, Viewer};
use crate::address::{EpicName, OrgId};
use crate::epics::{Epic, EpicArchived, EpicChange, EpicMove, EpicsEvent};
use crate::members::Scope;
use crate::vocab::Grantee;
use rusqlite::{params, Connection};
use std::collections::BTreeSet;

/// The tables beside `eras`.
const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS era_moves (
            principal TEXT NOT NULL,
            org       INTEGER NOT NULL,
            from_era  INTEGER NOT NULL,
            to_era    INTEGER NOT NULL,
            by        TEXT NOT NULL,
            at        INTEGER NOT NULL,
            reason    TEXT NOT NULL,
            PRIMARY KEY (principal, org)
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS era_aliases (
            org  INTEGER NOT NULL,
            name TEXT NOT NULL,
            era  INTEGER NOT NULL,
            PRIMARY KEY (org, name)
        ) WITHOUT ROWID;
    ";

/// One move an archive made of a principal's current epic, kept until it
/// is dismissed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MoveRec {
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) by: String,
    pub(super) at: i64,
    pub(super) reason: String,
}

/// What `epics.watch` is sent: the organization, the one principal it is
/// for (`None`: every watcher of the organization), and the event.
#[derive(Debug, Clone)]
pub struct EpicSignal {
    pub org: OrgId,
    pub to: Option<String>,
    pub event: EpicsEvent,
}

/// ORG-23: what an archive did (or, dry, would do).
#[derive(Debug, Clone)]
pub struct EpicArchive {
    pub epic: Epic,
    pub moved: Vec<String>,
    pub to: EpicName,
    pub tickets: u32,
}

/// The columns and tables, in the caller's transaction — idempotent.
pub(super) fn ensure(conn: &Connection) -> rusqlite::Result<()> {
    for (col, ty) in [("is_default", "INTEGER NOT NULL DEFAULT 0"), ("archived_at", "INTEGER"), ("archived_by", "TEXT")] {
        let has: i64 = conn.query_row("SELECT count(*) FROM pragma_table_info('eras') WHERE name = ?1", [col], |r| r.get(0))?;
        if has == 0 {
            conn.execute_batch(&format!("ALTER TABLE eras ADD COLUMN {col} {ty}"))?;
        }
    }
    conn.execute_batch(TABLES)
}

/// The marks, the default, the moves and the aliases into `r` — each only
/// where the file has it (a file `--verify-derived` reads before this
/// build's first open has none).
pub(super) fn load(conn: &Connection, r: &mut Registry) -> rusqlite::Result<()> {
    let has: i64 = conn.query_row("SELECT count(*) FROM pragma_table_info('eras') WHERE name = 'archived_at'", [], |x| x.get(0))?;
    if has > 0 {
        let mut stmt = conn.prepare("SELECT id, is_default, archived_at, archived_by FROM eras")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let Some(e) = r.eras.get_mut(&row.get::<_, i64>(0)?) else { continue };
            e.default = row.get::<_, i64>(1)? != 0;
            e.archived = row.get::<_, Option<i64>>(2)?.map(|at| (at, row.get::<_, Option<String>>(3).ok().flatten().unwrap_or_default()));
        }
    }
    let tables: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('era_moves', 'era_aliases')", [], |x| x.get(0))?;
    if tables == 2 {
        let mut stmt = conn.prepare("SELECT principal, org, from_era, to_era, by, at, reason FROM era_moves")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let rec = MoveRec { from: row.get(2)?, to: row.get(3)?, by: row.get(4)?, at: row.get(5)?, reason: row.get(6)? };
            r.moves.insert((row.get(0)?, row.get(1)?), rec);
        }
        let mut stmt = conn.prepare("SELECT org, name, era FROM era_aliases")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            r.era_aliases.insert((row.get(0)?, row.get(1)?), row.get(2)?);
        }
    }
    Ok(())
}

/// ORG-23 `next`: where the people on `era` go when it is archived — the
/// next newer live epic of its organization, else the next older one
/// (epics are ordered by creation: their rows). `None`: it is the last.
pub(super) fn next_era(r: &Registry, era: i64) -> Option<i64> {
    let org = r.eras.get(&era)?.org;
    let live: Vec<i64> = r.eras.iter().filter(|(id, e)| e.org == org && e.archived.is_none() && **id != era).map(|(id, _)| *id).collect();
    live.iter().copied().find(|id| *id > era).or_else(|| live.iter().rev().copied().find(|id| *id < era))
}

/// The principals with a grant in the organization `org` (its row) — on
/// it, on `*`, on one of its shortcodes or tickets; a team's members for
/// its grants — and every one that chose an epic there.
pub(super) fn principals_in(cache: &Cache, org: i64) -> BTreeSet<String> {
    let r = &cache.shortcodes;
    let Some(name) = r.orgs.get(&org) else { return BTreeSet::new() };
    let mut out = BTreeSet::new();
    for g in cache.members.all() {
        let here = match &g.scope {
            Scope::Everywhere => true,
            Scope::Org(o) | Scope::Shortcode { org: o, .. } => o == name,
            Scope::Under(k) | Scope::Ticket(k) => cache.get(k.as_str()).and_then(|row| r.org(&row.shortcode)).is_some_and(|o| o == name),
        };
        match (&g.principal, here) {
            (_, false) | (Grantee::Everyone, _) => {}
            (Grantee::Principal(p), true) => {
                out.insert(p.to_string());
            }
            (Grantee::Team(t), true) => out.extend(cache.members.records.get(t).into_iter().flat_map(|rec| rec.members.keys().cloned())),
        }
    }
    out.extend(r.choices.keys().filter(|(_, o)| *o == org).map(|(p, _)| p.clone()));
    out
}

/// How many principals of the organization are on `era` now.
pub(super) fn members_on(cache: &Cache, era: i64) -> u32 {
    let Some(org) = cache.shortcodes.eras.get(&era).map(|e| e.org) else { return 0 };
    let n = principals_in(cache, org).iter().filter(|p| cache.shortcodes.current_era(p, org) == Some(era)).count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl Registry {
    /// ORG-23: the archive's mark of the epic shortcode `sc` is in — what
    /// `get` says of its tickets (`epic_archived`).
    pub(super) fn epic_mark(&self, sc: &ShortcodeId) -> Option<EpicArchived> {
        let e = self.eras.get(&self.era_of(sc))?;
        let (at, by) = e.archived.clone()?;
        Some(EpicArchived { name: e.name.clone(), at_unix: at, by })
    }

    /// ORG-23: refused when the epic `era` is archived — nothing switches
    /// to, is made in, or moves into one.
    pub(super) fn live_era(&self, era: i64, what: &str) -> Result<(), StoreError> {
        match self.eras.get(&era).filter(|e| e.archived.is_some()) {
            Some(e) => Err(StoreError::Invalid(format!("epic `{}` is archived: restore it first ({what}; nothing was changed)", e.name))),
            None => Ok(()),
        }
    }

    /// ORG-23: the move `principal` has not dismissed in `org`, as said.
    fn move_of(&self, principal: &str, org: i64) -> Option<EpicMove> {
        let m = self.moves.get(&(principal.to_string(), org))?;
        let name = |e: i64| self.eras.get(&e).map(|x| x.name.clone());
        Some(EpicMove { from: name(m.from)?, to: name(m.to)?, by: m.by.clone(), at_unix: m.at, reason: m.reason.clone() })
    }
}

/// `org`'s change of its epic `name`, to every watcher of it.
pub(super) fn said(tx: &Broadcast, org: &OrgId, name: &EpicName, what: EpicChange) {
    let event = EpicsEvent::EpicChanged { org: org.clone(), name: name.clone(), what };
    let _ = tx.epics.send(EpicSignal { org: org.clone(), to: None, event });
}

impl Store {
    /// ORG-23: `epics.watch`'s channel.
    pub fn subscribe_epics(&self) -> tokio::sync::broadcast::Receiver<EpicSignal> {
        self.tx.epics.subscribe()
    }

    /// ORG-23: the move an archive made of `principal`'s epic in `org`,
    /// until it is dismissed.
    pub fn epic_move(&self, org: &OrgId, principal: &str) -> Option<EpicMove> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let (row, _) = org_row(&cache.shortcodes, org).ok()?;
        cache.shortcodes.move_of(principal, row)
    }

    /// ORG-23: archives `org`'s epic `name` (the module doc) — or, `dry`,
    /// answers what it would do and writes nothing. Needs admin on `org`.
    pub async fn archive_epic(&self, by: impl Into<Authority>, org: &OrgId, name: &str, dry: bool, principal: &str) -> Result<EpicArchive, StoreError> {
        let by = by.into();
        let serial = self.serial().await;
        let (row, o, era, to, was_default, answer) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = &cache.shortcodes;
            let (row, o) = org_row(r, org)?;
            org_admin(&by, &o)?;
            let era = era_in(r, row, name)?;
            let e = &r.eras[&era];
            if e.archived.is_some() {
                return Err(StoreError::Invalid(format!("`{}` is archived already (nothing was changed)", e.name)));
            }
            let to = next_era(r, era).ok_or_else(|| StoreError::Invalid(format!("{} is {o}'s last epic: create another first (nothing was changed)", e.name)))?;
            let moved: Vec<String> = principals_in(&cache, row).into_iter().filter(|p| r.current_era(p, row) == Some(era)).collect();
            let epic = card(&cache, era, principal).ok_or_else(|| StoreError::NotFound(name.to_string()))?;
            let answer = EpicArchive { tickets: epic.tickets, epic, moved, to: r.eras[&to].name.clone() };
            (row, o, era, to, r.default_era(row) == Some(era), answer)
        };
        if dry {
            return Ok(answer);
        }
        let now = chrono::Utc::now().timestamp();
        let (who, moved) = (principal.to_string(), answer.moved.clone());
        let (w, m) = (who.clone(), moved.clone());
        self.write(
            serial,
            move |tx| {
                tx.execute("UPDATE eras SET archived_at = ?2, archived_by = ?3, is_default = 0 WHERE id = ?1", params![era, now, w])?;
                if was_default {
                    tx.execute("UPDATE eras SET is_default = (id = ?2) WHERE org = ?1", params![row, to])?;
                }
                tx.execute("UPDATE era_choices SET era = ?2 WHERE era = ?1", params![era, to])?;
                for p in &m {
                    tx.execute(
                        "INSERT INTO era_moves (principal, org, from_era, to_era, by, at, reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'archived')
                             ON CONFLICT (principal, org) DO UPDATE SET from_era = excluded.from_era, to_era = excluded.to_era, by = excluded.by, at = excluded.at, reason = excluded.reason",
                        params![p, row, era, to, w, now],
                    )?;
                }
                Ok(())
            },
            move |cache, ()| {
                let r = &mut cache.shortcodes;
                for (id, e) in r.eras.iter_mut().filter(|(_, e)| e.org == row) {
                    if *id == era {
                        (e.archived, e.default) = (Some((now, who.clone())), false);
                    } else if was_default {
                        e.default = *id == to;
                    }
                }
                r.choices.values_mut().filter(|e| **e == era).for_each(|e| *e = to);
                for p in &moved {
                    r.moves.insert((p.clone(), row), MoveRec { from: era, to, by: who.clone(), at: now, reason: "archived".into() });
                }
                let names = (r.eras[&era].name.clone(), r.eras[&to].name.clone());
                let moves: Vec<(String, Option<EpicMove>)> = moved.iter().map(|p| (p.clone(), r.move_of(p, row))).collect();
                (names, moves, card(cache, era, &who))
            },
            move |(), ((gone, next), moves, epic), tx| {
                said(tx, &o, &gone, EpicChange::Archived);
                if was_default {
                    said(tx, &o, &next, EpicChange::DefaultMoved);
                }
                for (p, mv) in moves {
                    if let Some(moved) = mv {
                        let _ = tx.epics.send(EpicSignal { org: o.clone(), to: Some(p), event: EpicsEvent::Moved { org: o.clone(), moved } });
                    }
                }
                EpicArchive { epic: epic.unwrap_or(answer.epic), ..answer }
            },
        )
        .await
    }

    /// ORG-23: restores `org`'s archived epic `name` — live again, in the
    /// picker and the views; nobody moves. Needs admin on `org`.
    pub async fn restore_epic(&self, by: impl Into<Authority>, org: &OrgId, name: &str, principal: &str) -> Result<Epic, StoreError> {
        let by = by.into();
        let serial = self.serial().await;
        let (era, o) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = &cache.shortcodes;
            let (row, o) = org_row(r, org)?;
            org_admin(&by, &o)?;
            let era = era_in(r, row, name)?;
            if !r.era_archived(era) {
                return Err(StoreError::Invalid(format!("`{}` is not archived (nothing was changed)", r.eras[&era].name)));
            }
            (era, o)
        };
        let who = principal.to_string();
        self.write(
            serial,
            move |tx| {
                tx.execute("UPDATE eras SET archived_at = NULL, archived_by = NULL WHERE id = ?1", [era])?;
                Ok(())
            },
            move |cache, ()| {
                if let Some(e) = cache.shortcodes.eras.get_mut(&era) {
                    e.archived = None;
                }
                card(cache, era, &who)
            },
            move |(), epic, tx| {
                let epic = epic.ok_or_else(|| StoreError::Invalid("the epic was not kept".into()))?;
                said(tx, &o, &epic.name, EpicChange::Restored);
                Ok(epic)
            },
        )
        .await?
    }

    /// ORG-23: `org`'s default epic set to `name` — where a principal who
    /// never switched is. Refused for an archived epic. Needs admin on `org`.
    pub async fn set_default_epic(&self, by: impl Into<Authority>, org: &OrgId, name: &str, principal: &str) -> Result<Epic, StoreError> {
        let by = by.into();
        let serial = self.serial().await;
        let (row, era, o) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let r = &cache.shortcodes;
            let (row, o) = org_row(r, org)?;
            org_admin(&by, &o)?;
            let era = era_in(r, row, name)?;
            r.live_era(era, "an archived epic is no one's default")?;
            (row, era, o)
        };
        let who = principal.to_string();
        self.write(
            serial,
            move |tx| {
                tx.execute("UPDATE eras SET is_default = (id = ?2) WHERE org = ?1", params![row, era])?;
                Ok(())
            },
            move |cache, ()| {
                for (id, e) in cache.shortcodes.eras.iter_mut().filter(|(_, e)| e.org == row) {
                    e.default = *id == era;
                }
                card(cache, era, &who)
            },
            move |(), epic, tx| {
                let epic = epic.ok_or_else(|| StoreError::Invalid("the epic was not kept".into()))?;
                said(tx, &o, &epic.name, EpicChange::DefaultMoved);
                Ok(epic)
            },
        )
        .await?
    }

    /// ORG-23: `principal`'s move notice in `org` dismissed — its own
    /// setting, so a viewer's (`on`, held on the organization).
    pub async fn dismiss_epic_move(&self, on: On<Viewer>, org: &OrgId, principal: &str) -> Result<OrgId, StoreError> {
        let serial = self.serial().await;
        let (row, o) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let (row, o) = org_row(&cache.shortcodes, org)?;
            if on.scope() != &Scope::Org(o.clone()) {
                return Err(StoreError::Invalid(format!("the proof is for {}, not {o}: (nothing was changed)", on.scope())));
            }
            (row, o)
        };
        let (who, kept) = (principal.to_string(), principal.to_string());
        self.write(
            serial,
            move |tx| {
                tx.execute("DELETE FROM era_moves WHERE principal = ?1 AND org = ?2", params![who, row])?;
                Ok(())
            },
            move |cache, ()| {
                cache.shortcodes.moves.remove(&(kept, row));
            },
            move |(), (), _| o,
        )
        .await
    }
}
