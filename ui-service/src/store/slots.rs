//! UI-9: **sections** — what fills each slot of each layout. An assignment
//! is data about the layout, kept apart from the bundle: `(layout, slot) →
//! (component, version)` in `ui_slots`, so a version can be tried in one
//! section without becoming the live version (UI-4 "Sections").
//!
//! - a **set** names a stored version (none: the live one, its hash kept),
//!   a component that version's manifest holds, and a slot the live
//!   manifest's `slots` declares for the layout — each checked, refused
//!   naming which, nothing written;
//! - a **clear** removes the row; the section shows its default again;
//! - a **list** says every assignment with the default it overrides.
//!
//! A manifest is read out of the version's bundle (`manifest.json`, UI-4's
//! JSON: `components`, `slots`). Every set and clear is said on the store's
//! broadcast as [`UiChange::SlotChanged`].

use super::{ManifestDoc, UiError, UiStore, LIVE};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// One assignment, as `ui.slots.list` answers it: what fills `slot` of
/// `layout`, and the default it overrides (the live manifest's
/// `slots[layout][slot]`; `None` when it names none, or none is live).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub layout: String,
    pub slot: String,
    pub component: String,
    /// The stored version's hash.
    pub version: String,
    pub default: Option<String>,
}

/// What a section shows now — an assignment's component and version, or,
/// after a clear, its default in the live version (`None`s: no default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub layout: String,
    pub slot: String,
    pub component: Option<String>,
    pub version: Option<String>,
}

/// What the store says on its broadcast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiChange {
    /// The live pointer moved (a push, a rollback): the version it names.
    Live { version: String },
    /// A section's assignment set or cleared: what it shows now.
    SlotChanged(Section),
}

/// The live pointer's hash, when any version is live.
fn live_hash(c: &Connection) -> rusqlite::Result<Option<String>> {
    c.query_row("SELECT hash FROM ui_pointer WHERE name = ?1", [LIVE], |r| r.get(0)).optional()
}

/// Version `hash`'s manifest, read; `None` when it is not kept.
fn manifest_of(c: &Connection, hash: &str) -> Result<Option<ManifestDoc>, UiError> {
    let text: Option<String> = c.query_row("SELECT manifest FROM ui_versions WHERE hash = ?1", [hash], |r| r.get(0)).optional()?;
    text.map(|t| ManifestDoc::parse(&t)).transpose()
}

/// The live version's hash and manifest, when one is live.
fn live_manifest(c: &Connection) -> Result<Option<(String, ManifestDoc)>, UiError> {
    let Some(hash) = live_hash(c)? else { return Ok(None) };
    Ok(manifest_of(c, &hash)?.map(|doc| (hash, doc)))
}

/// What `slot` of `layout` shows by default: the live manifest's component
/// for it, in the live version — `None`s when it names none.
fn default_section(live: Option<&(String, ManifestDoc)>, layout: &str, slot: &str) -> Section {
    let named = live.and_then(|(hash, doc)| doc.default_for(layout, slot).map(|c| (c.to_string(), hash.clone())));
    let (component, version) = named.unzip();
    Section { layout: layout.into(), slot: slot.into(), component, version }
}

impl UiStore {
    /// Every assignment (of `layout`, when named), each with the default it
    /// overrides, ordered by layout then slot.
    pub fn slots_list(&self, layout: Option<&str>) -> Result<Vec<Assignment>, UiError> {
        let c = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let live = live_manifest(&c)?;
        let mut q = c.prepare(
            "SELECT layout, slot, component, version_hash FROM ui_slots WHERE ?1 IS NULL OR layout = ?1 ORDER BY layout, slot",
        )?;
        let rows = q.query_map([layout], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (layout, slot, component, version) = row?;
            let default = live.as_ref().and_then(|(_, doc)| doc.default_for(&layout, &slot)).map(str::to_string);
            out.push(Assignment { layout, slot, component, version, default });
        }
        Ok(out)
    }

    /// Assigns `component` of `version` (none: the live version, its hash
    /// pinned) to `slot` of `layout` — the version kept, the component in
    /// its manifest, the slot declared by the live manifest, or refused
    /// naming which with nothing written — and says it as `slot_changed`.
    pub fn slots_set(&self, layout: &str, slot: &str, component: &str, version: Option<&str>) -> Result<Assignment, UiError> {
        let mut c = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let tx = c.transaction()?;
        let live = live_manifest(&tx)?;
        let hash = match version {
            Some(h) => h.to_string(),
            None => live.as_ref().map(|(h, _)| h.clone()).ok_or_else(|| UiError::UnknownVersion("(none named, and none is live)".into()))?,
        };
        let doc = manifest_of(&tx, &hash)?.ok_or_else(|| UiError::UnknownVersion(hash.clone()))?;
        if !doc.components.contains_key(component) {
            return Err(UiError::UnknownComponent { component: component.into(), version: hash });
        }
        let declared = live.as_ref().is_some_and(|(_, d)| d.slots.get(layout).is_some_and(|s| s.contains_key(slot)));
        if !declared {
            return Err(UiError::UnknownSlot { layout: layout.into(), slot: slot.into() });
        }
        tx.execute(
            "INSERT INTO ui_slots (layout, slot, component, version_hash) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(layout, slot) DO UPDATE SET component = excluded.component, version_hash = excluded.version_hash",
            [layout, slot, component, hash.as_str()],
        )?;
        tx.commit()?;
        let default = live.as_ref().and_then(|(_, d)| d.default_for(layout, slot)).map(str::to_string);
        let said = Section { layout: layout.into(), slot: slot.into(), component: Some(component.into()), version: Some(hash.clone()) };
        let _ = self.changes.send(UiChange::SlotChanged(said));
        Ok(Assignment { layout: layout.into(), slot: slot.into(), component: component.into(), version: hash, default })
    }

    /// Removes `slot` of `layout`'s assignment; what the section shows now
    /// (its default), said as `slot_changed` — when there was a row: a
    /// section with none writes and says nothing.
    pub fn slots_clear(&self, layout: &str, slot: &str) -> Result<Section, UiError> {
        let mut c = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let tx = c.transaction()?;
        let removed = tx.execute("DELETE FROM ui_slots WHERE layout = ?1 AND slot = ?2", [layout, slot])?;
        let shows = default_section(live_manifest(&tx)?.as_ref(), layout, slot);
        tx.commit()?;
        if removed > 0 {
            let _ = self.changes.send(UiChange::SlotChanged(shows.clone()));
        }
        Ok(shows)
    }
}
