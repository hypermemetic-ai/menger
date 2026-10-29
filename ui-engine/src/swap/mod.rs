//! UI-6: **the module registry, hot-swap and the sections**.
//!
//! The registry is the live version's `manifest.json`: each component's
//! name → the hashes of its tree and handlers. A mounted component is an
//! instance: its element (named for the component), its props, its state,
//! its folds and the version it was drawn from.
//!
//! - **Hot-swap** ([`App::swap`]): a new manifest; each mounted instance
//!   whose `tree` or `handlers` hash changed is remounted into the same
//!   element with the same props — its state carried over when the declared
//!   state's shape (its keys) is unchanged, else the new initial state;
//!   every other instance is untouched. One that fails to mount (a
//!   malformed tree, handlers refused) keeps its last good version drawn,
//!   and the console says so.
//! - **Sections**: a layout's `slot` node draws as a `div` carrying
//!   `data-layout` and `data-slot`, filled by its assignment — from
//!   `ui.slots.list` ([`App::set_assignments`]), else `manifest.slots`'
//!   default — with the slot's attributes as props. A component of another
//!   version is drawn from that version's files ([`Files::put_version`],
//!   fetched by the engine from `/app/<hash>/components/<name>.json` and
//!   `.rhai`). `slot_changed` ([`App::slot_changed`]) remounts that section
//!   alone, its props kept.
//!
//! What the engine fetches is handed in as [`Files`] (content-addressed by
//! sha256), so everything here is synchronous: the browser fetches first,
//! then swaps.

use crate::live::Clock;
use crate::signals::Runtime;
use crate::transport::Wire;
use crate::tree::Renderer;
use serde_json::{Map, Value};
use sha2::Digest;
use std::collections::BTreeMap;

/// One component of a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentEntry {
    /// The sha256 of `components/<name>.json`.
    pub tree: String,
    /// The sha256 of `components/<name>.rhai`.
    pub handlers: String,
    pub uses: Vec<String>,
}

/// A bundle's `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
    pub abi: String,
    pub contract: String,
    pub commit: String,
    /// A route pattern (`#/ticket/:id`) → the component (or layout) drawn.
    pub routes: BTreeMap<String, String>,
    pub components: BTreeMap<String, ComponentEntry>,
    /// A layout → its slot → the component filling it by default.
    pub slots: BTreeMap<String, BTreeMap<String, String>>,
}

impl Manifest {
    /// From `manifest.json`'s text.
    pub fn parse(json: &str) -> Result<Self, String> {
        let _ = json;
        todo!("UI-6: Manifest::parse")
    }
}

/// What the engine has fetched: files by the sha256 of their text, and
/// another version's component by `(version, name)`.
#[derive(Debug, Clone, Default)]
pub struct Files {
    by_hash: BTreeMap<String, String>,
    by_version: BTreeMap<(String, String), (String, String)>,
}

/// The sha256 of `text`, hex — what a manifest names a file by.
pub fn sha256_hex(text: &str) -> String {
    sha2::Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

impl Files {
    /// Keeps `text`; its hash.
    pub fn put(&mut self, text: &str) -> String {
        let hash = sha256_hex(text);
        self.by_hash.insert(hash.clone(), text.to_string());
        hash
    }

    /// The file named `hash`.
    pub fn get(&self, hash: &str) -> Option<&str> {
        self.by_hash.get(hash).map(String::as_str)
    }

    /// Keeps version `version`'s component `name`: its tree and handlers.
    pub fn put_version(&mut self, version: &str, name: &str, tree: &str, handlers: &str) {
        self.by_version.insert((version.into(), name.into()), (tree.into(), handlers.into()));
    }

    /// Version `version`'s component `name`: `(tree, handlers)`.
    pub fn version(&self, version: &str, name: &str) -> Option<(&str, &str)> {
        self.by_version.get(&(version.to_string(), name.to_string())).map(|(t, h)| (t.as_str(), h.as_str()))
    }
}

/// What fills a section: `(layout, slot) → (component, version)` — kept
/// by the ui service (`ui.slots.*`), said on `ui.watch` as `slot_changed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub layout: String,
    pub slot: String,
    pub component: String,
    /// The version's hash (the live one's, for a component of the live bundle).
    pub version: String,
}

/// A mounted instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MountId(pub u64);

/// What a swap did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SwapReport {
    /// Remounted on their new hashes.
    pub remounted: Vec<MountId>,
    /// Kept on their last good version: each with why.
    pub failed: Vec<(MountId, String)>,
}

/// Why a mount or a load did not happen.
#[derive(Debug, Clone, PartialEq)]
pub enum SwapError {
    /// No component of that name in the version.
    Unknown(String),
    /// A file the manifest names was not fetched: its hash.
    Missing(String),
    /// The tree did not parse or mount, or the handlers were refused: why.
    Failed { component: String, message: String },
}

impl std::fmt::Display for SwapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SwapError::Unknown(c) => write!(f, "no component {c} in this version"),
            SwapError::Missing(h) => write!(f, "the file {h} was not fetched"),
            SwapError::Failed { component, message } => write!(f, "{component} failed to mount: {message}"),
        }
    }
}

/// The section menu (a right-click on a section): the live bundle's
/// components by name, and the section's component's other stored
/// versions — those in which its hash differs — newest first.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Menu {
    pub components: Vec<String>,
    pub versions: Vec<String>,
}

/// The engine: the registry, the instances, the sections — over a
/// renderer, the wire (for `call` and `live`) and a clock.
pub struct App<R: Renderer, W: Wire, C: Clock> {
    rt: Runtime,
    renderer: R,
    wire: W,
    clock: C,
}

impl<R: Renderer, W: Wire + Clone + 'static, C: Clock + Clone + 'static> App<R, W, C> {
    pub fn new(rt: &Runtime, renderer: R, wire: W, clock: C) -> std::rc::Rc<Self> {
        std::rc::Rc::new(Self { rt: rt.clone(), renderer, wire, clock })
    }

    /// The first version: `version`'s manifest, its files fetched.
    pub fn load(&self, version: &str, manifest: Manifest, files: &Files) -> Result<(), SwapError> {
        let _ = (version, manifest, files, &self.rt, &self.renderer, &self.wire, &self.clock);
        todo!("UI-6: App::load")
    }

    /// The live version's hash.
    pub fn version(&self) -> String {
        todo!("UI-6: App::version")
    }

    /// Mounts `name` of the live version into a new element (named for it)
    /// appended to `parent`. A component that fails is drawn as failed (its
    /// element carries `data-failed`, the reason) and the console says why.
    pub fn mount(&self, parent: &R::Node, name: &str, props: Map<String, Value>) -> Result<MountId, SwapError> {
        let _ = (parent, name, props);
        todo!("UI-6: App::mount")
    }

    /// Runs `handler` of instance `id` on `event`: its state replaced.
    pub fn dispatch(&self, id: MountId, handler: &str, event: Value) -> Result<(), crate::folds::FoldError> {
        let _ = (id, handler, event);
        todo!("UI-6: App::dispatch")
    }

    /// A new live version: remounts exactly the instances whose hashes changed.
    pub fn swap(&self, version: &str, manifest: Manifest, files: &Files) -> SwapReport {
        let _ = (version, manifest, files);
        todo!("UI-6: App::swap")
    }

    /// The sections' assignments (`ui.slots.list`); a section with none
    /// takes `manifest.slots`' default.
    pub fn set_assignments(&self, assignments: Vec<Assignment>) {
        let _ = assignments;
        todo!("UI-6: App::set_assignments")
    }

    /// `slot_changed` on `ui.watch`: that section remounts with its props,
    /// nothing else.
    pub fn slot_changed(&self, change: Assignment, files: &Files) -> Result<(), SwapError> {
        let _ = (change, files);
        todo!("UI-6: App::slot_changed")
    }

    /// The instance filling `layout`'s section `slot`.
    pub fn section(&self, layout: &str, slot: &str) -> Option<MountId> {
        let _ = (layout, slot);
        todo!("UI-6: App::section")
    }

    /// The section menu for `layout`'s `slot`, given `ui.versions` (newest
    /// first) and each version's hash of the section's component.
    pub fn menu(&self, layout: &str, slot: &str, versions: &[(String, Option<String>)]) -> Menu {
        let _ = (layout, slot, versions);
        todo!("UI-6: App::menu")
    }

    /// Instance `id`'s element.
    pub fn element_of(&self, id: MountId) -> R::Node {
        let _ = id;
        todo!("UI-6: App::element_of")
    }

    /// Instance `id`'s props.
    pub fn props_of(&self, id: MountId) -> Map<String, Value> {
        let _ = id;
        todo!("UI-6: App::props_of")
    }

    /// Instance `id`'s state now.
    pub fn state_of(&self, id: MountId) -> Map<String, Value> {
        let _ = id;
        todo!("UI-6: App::state_of")
    }

    /// Instance `id`'s drawn version: `(version, tree hash, handlers hash)`.
    pub fn drawn_from(&self, id: MountId) -> (String, String, String) {
        let _ = id;
        todo!("UI-6: App::drawn_from")
    }

    /// What the engine said on the console, in order.
    pub fn console(&self) -> Vec<String> {
        todo!("UI-6: App::console")
    }
}

#[cfg(test)]
mod tests;
