//! NOTES-4: **a note's slot** — the component drawn on the note's right
//! (the owner, 2026-11-15: *"every note associated with a plexus-ui slot on
//! it's right"*): a component of a version the ui service keeps, its props,
//! and every fill before it; and what an agent says while it builds the
//! next one (*"we instead want to show that the agent is building
//! components as it builds"*).
//!
//! **Stored** in the notes file, by the store's own migration (`CREATE …
//! IF NOT EXISTS`, so a `notes.db` from before opens with every slot empty):
//! `note_slots` — a row a note: the fill drawn now (`fill`, a fill's `n`;
//! null: empty), the props (a JSON object), who and when last; and
//! `note_fills` — a row for every fill: its `n`, the component, the
//! version's whole hash, its line, who and when. A delete of the note
//! leaves both, so a note brought back has its slot as it was.
//!
//! **A build under way** is kept in memory alone ([`NotesStore::builds`]):
//! a fill or an empty ends it, so does [`BUILD_IDLE_MS`] after its last
//! word ([`NotesStore::sweep_builds`]), and a restart. Every change is said
//! on the channel for `watch`, the slot whole: `SlotChanged`,
//! `BuildingChanged`, `PropsChanged`.

use crate::events::{Building, Fill, Slot};
use crate::store::{live_doc, now_ms, Change, NotesStore, StoreError};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};
use std::sync::Arc;

/// The tables.
pub const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS note_slots (
            id        TEXT PRIMARY KEY,
            fill      INTEGER,
            props     TEXT NOT NULL DEFAULT '{}',
            at        INTEGER NOT NULL,
            by        TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS note_fills (
            id         TEXT NOT NULL,
            n          INTEGER NOT NULL,
            component  TEXT NOT NULL,
            version    TEXT NOT NULL,
            say        TEXT,
            by         TEXT NOT NULL,
            principal  TEXT NOT NULL,
            at         INTEGER NOT NULL,
            PRIMARY KEY (id, n)
        );
    ";

/// A build is over ten minutes after its last word.
pub const BUILD_IDLE_MS: i64 = 10 * 60 * 1000;

/// What a fill sets.
#[derive(Debug, Clone)]
pub struct FillOf {
    pub component: String,
    /// The version's whole hash.
    pub version: String,
    /// The props it starts from; none: the props the slot holds stay.
    pub props: Option<Map<String, Value>>,
    pub say: Option<String>,
}

/// Who writes.
#[derive(Debug, Clone)]
pub struct Hand {
    pub name: String,
    pub principal: String,
}

fn fill_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<Fill> {
    Ok(Fill { n: r.get(0)?, component: r.get(1)?, version: r.get(2)?, say: r.get(3)?, by: r.get(4)?, principal: r.get(5)?, at: r.get(6)? })
}

/// `props` merged with `patch`: each key set, a `null` taken away.
pub fn merged(mut props: Map<String, Value>, patch: Map<String, Value>) -> Map<String, Value> {
    for (k, v) in patch {
        match v {
            Value::Null => {
                props.remove(&k);
            }
            v => {
                props.insert(k, v);
            }
        }
    }
    props
}

impl NotesStore {
    /// The slot of `id` as `conn` holds it, with `building` beside it.
    fn slot_in(&self, conn: &Connection, id: &str) -> Result<Slot, StoreError> {
        let row: Option<(Option<u32>, String)> = conn.query_row("SELECT fill, props FROM note_slots WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        let (now, props) = row.unwrap_or((None, "{}".into()));
        let mut stmt = conn.prepare("SELECT n, component, version, say, by, principal, at FROM note_fills WHERE id = ?1 ORDER BY n DESC")?;
        let fills = stmt.query_map([id], fill_of)?.collect::<Result<Vec<_>, _>>()?;
        Ok(Slot {
            id: id.to_string(),
            fill: now.and_then(|n| fills.iter().find(|f| f.n == n).cloned()),
            props: serde_json::from_str(&props).unwrap_or_default(),
            fills,
            building: self.builds().get(id).cloned(),
        })
    }

    /// The builds under way, by note.
    fn builds(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, Building>> {
        self.builds.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The slot of the live note `id`, with every fill and the build under way.
    pub async fn slot(self: &Arc<Self>, id: &str) -> Result<Slot, StoreError> {
        let id = id.to_string();
        self.blocking(move |s| {
            let conn = s.lock();
            live_doc(&conn, &id)?;
            s.slot_in(&conn, &id)
        })
        .await
    }

    /// Fills the slot of `id` with `fill` — a fill more, the build under way
    /// ended — and says it.
    pub async fn fill(self: &Arc<Self>, id: &str, fill: FillOf, hand: Hand) -> Result<Slot, StoreError> {
        let id = id.to_string();
        let (org, slot) = self
            .blocking(move |s| {
                let conn = s.lock();
                let tx = conn.unchecked_transaction()?;
                let org = live_doc(&tx, &id)?.org;
                let n: u32 = tx.query_row("SELECT COALESCE(MAX(n), 0) + 1 FROM note_fills WHERE id = ?1", [&id], |r| r.get(0))?;
                let at = now_ms();
                tx.execute(
                    "INSERT INTO note_fills (id, n, component, version, say, by, principal, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![id, n, fill.component, fill.version, fill.say, hand.name, hand.principal, at],
                )?;
                let props = fill.props.map(|p| Value::Object(p).to_string());
                tx.execute(
                    "INSERT INTO note_slots (id, fill, props, at, by) VALUES (?1, ?2, COALESCE(?3, '{}'), ?4, ?5)
                     ON CONFLICT(id) DO UPDATE SET fill = excluded.fill, props = COALESCE(?3, props), at = excluded.at, by = excluded.by",
                    params![id, n, props, at, hand.name],
                )?;
                tx.commit()?;
                s.builds().remove(&id);
                Ok((org, s.slot_in(&conn, &id)?))
            })
            .await?;
        self.said(Change::SlotChanged { org, slot: slot.clone() });
        Ok(slot)
    }

    /// Empties the slot of `id` — nothing drawn, its props cleared, its
    /// fills kept, the build under way ended — and says it.
    pub async fn empty(self: &Arc<Self>, id: &str, hand: Hand) -> Result<Slot, StoreError> {
        let id = id.to_string();
        let (org, slot) = self
            .blocking(move |s| {
                let conn = s.lock();
                let org = live_doc(&conn, &id)?.org;
                conn.execute(
                    "INSERT INTO note_slots (id, fill, props, at, by) VALUES (?1, NULL, '{}', ?2, ?3)
                     ON CONFLICT(id) DO UPDATE SET fill = NULL, props = '{}', at = excluded.at, by = excluded.by",
                    params![id, now_ms(), hand.name],
                )?;
                s.builds().remove(&id);
                Ok((org, s.slot_in(&conn, &id)?))
            })
            .await?;
        self.said(Change::SlotChanged { org, slot: slot.clone() });
        Ok(slot)
    }

    /// Merges `patch` into the props of `id`'s slot (a `null` takes its key
    /// away), held to [`crate::vocab::PROPS_MAX`], and says it.
    pub async fn props(self: &Arc<Self>, id: &str, patch: Map<String, Value>, hand: Hand) -> Result<Slot, StoreError> {
        let id = id.to_string();
        let by = hand.name.clone();
        let (org, slot) = self
            .blocking(move |s| {
                let conn = s.lock();
                let tx = conn.unchecked_transaction()?;
                let org = live_doc(&tx, &id)?.org;
                let was: Option<String> = tx.query_row("SELECT props FROM note_slots WHERE id = ?1", [&id], |r| r.get(0)).optional()?;
                let props = merged(was.and_then(|p| serde_json::from_str(&p).ok()).unwrap_or_default(), patch);
                let text = Value::Object(props).to_string();
                if text.len() > crate::vocab::PROPS_MAX {
                    return Err(StoreError::TooLarge(format!("the slot's props would be {} bytes written as JSON; they are held to {} (nothing was written)", text.len(), crate::vocab::PROPS_MAX)));
                }
                tx.execute(
                    "INSERT INTO note_slots (id, fill, props, at, by) VALUES (?1, NULL, ?2, ?3, ?4)
                     ON CONFLICT(id) DO UPDATE SET props = excluded.props, at = excluded.at, by = excluded.by",
                    params![id, text, now_ms(), hand.name],
                )?;
                tx.commit()?;
                Ok((org, s.slot_in(&conn, &id)?))
            })
            .await?;
        self.said(Change::PropsChanged { org, slot: slot.clone(), by });
        Ok(slot)
    }

    /// Marks `id`'s slot as being built — `building` its word now (kept from
    /// the build before when it is of the same component and has not
    /// failed) — and says it.
    pub async fn building(self: &Arc<Self>, id: &str, mut building: Building) -> Result<Slot, StoreError> {
        let id = id.to_string();
        let (org, slot) = self
            .blocking(move |s| {
                let conn = s.lock();
                let org = live_doc(&conn, &id)?.org;
                {
                    let mut builds = s.builds();
                    if let Some(was) = builds.get(&id).filter(|b| b.component == building.component && !b.failed) {
                        building.started = was.started;
                    }
                    builds.insert(id.clone(), building);
                }
                Ok((org, s.slot_in(&conn, &id)?))
            })
            .await?;
        self.said(Change::BuildingChanged { org, slot: slot.clone() });
        Ok(slot)
    }

    /// Ends every build silent for [`BUILD_IDLE_MS`], saying each.
    pub fn sweep_builds(&self) {
        let now = now_ms();
        let gone: Vec<String> = {
            let mut builds = self.builds();
            let idle: Vec<String> = builds.iter().filter(|(_, b)| now - b.at > BUILD_IDLE_MS).map(|(id, _)| id.clone()).collect();
            idle.into_iter().filter(|id| builds.remove(id).is_some()).collect()
        };
        for id in gone {
            let conn = self.lock();
            let (Ok(doc), Ok(slot)) = (live_doc(&conn, &id), self.slot_in(&conn, &id)) else { continue };
            drop(conn);
            self.said(Change::BuildingChanged { org: doc.org, slot });
        }
    }

    /// Test seam: a build's last word moved back by `ms`.
    #[cfg(test)]
    pub(crate) fn age_build(&self, id: &str, ms: i64) {
        if let Some(b) = self.builds().get_mut(id) {
            b.at -= ms;
        }
    }
}
