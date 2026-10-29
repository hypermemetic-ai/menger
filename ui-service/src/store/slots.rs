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

use super::{UiError, UiStore};
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

impl UiStore {
    /// Every assignment (of `layout`, when named), each with the default it
    /// overrides, ordered by layout then slot.
    pub fn slots_list(&self, layout: Option<&str>) -> Result<Vec<Assignment>, UiError> {
        let _ = layout;
        todo!("UI-9: slots_list")
    }

    /// Assigns `component` of `version` (none: the live version) to `slot` of
    /// `layout` — checked, written, and said as `slot_changed`.
    pub fn slots_set(&self, layout: &str, slot: &str, component: &str, version: Option<&str>) -> Result<Assignment, UiError> {
        let _ = (layout, slot, component, version);
        todo!("UI-9: slots_set")
    }

    /// Removes `slot` of `layout`'s assignment; what the section shows now
    /// (its default), said as `slot_changed`.
    pub fn slots_clear(&self, layout: &str, slot: &str) -> Result<Section, UiError> {
        let _ = (layout, slot);
        todo!("UI-9: slots_clear")
    }
}
