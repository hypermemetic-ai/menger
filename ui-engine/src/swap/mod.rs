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

use crate::folds::{FoldError, Folds, Host};
use crate::live::{Clock, LiveCache, LiveSpec, Reader};
use crate::signals::{Runtime, Signal};
use crate::transport::Wire;
use crate::tree::{Children, ComponentTree, Mounted, Renderer, Scope, State};
use serde_json::{Map, Value};
use sha2::Digest;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

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
        let v: Value = serde_json::from_str(json).map_err(|e| format!("manifest.json: {e}"))?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let map = |k: &str| -> BTreeMap<String, String> {
            v.get(k).and_then(Value::as_object).map(|o| o.iter().filter_map(|(a, b)| Some((a.clone(), b.as_str()?.to_string()))).collect()).unwrap_or_default()
        };
        let components = v
            .get("components")
            .and_then(Value::as_object)
            .ok_or("manifest.json names no components")?
            .iter()
            .map(|(name, e)| {
                let f = |k: &str| e.get(k).and_then(Value::as_str).map(str::to_string).ok_or(format!("manifest.json: {name} has no {k}"));
                let uses = e.get("uses").and_then(Value::as_array).map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_string)).collect()).unwrap_or_default();
                Ok((name.clone(), ComponentEntry { tree: f("tree")?, handlers: f("handlers")?, uses }))
            })
            .collect::<Result<_, String>>()?;
        let slots = v
            .get("slots")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(layout, m)| {
                        let m = m.as_object().map(|m| m.iter().filter_map(|(a, b)| Some((a.clone(), b.as_str()?.to_string()))).collect()).unwrap_or_default();
                        (layout.clone(), m)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self { abi: s("abi"), contract: s("contract"), commit: s("commit"), routes: map("routes"), components, slots })
    }
}

/// What the engine has fetched: each version's component by
/// `(version, name)` — `/app/<version>/components/<name>.json` and `.rhai`.
#[derive(Debug, Clone, Default)]
pub struct Files {
    by_version: BTreeMap<(String, String), (String, String)>,
}

/// The sha256 of `text`, hex — what a manifest names a file by.
pub fn sha256_hex(text: &str) -> String {
    sha2::Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

impl Files {
    /// Keeps version `version`'s component `name`: its tree and handlers.
    pub fn put_version(&mut self, version: &str, name: &str, tree: &str, handlers: &str) {
        self.by_version.insert((version.into(), name.into()), (tree.into(), handlers.into()));
    }

    /// Version `version`'s component `name`: `(tree, handlers)`.
    pub fn version(&self, version: &str, name: &str) -> Option<(&str, &str)> {
        self.by_version.get(&(version.to_string(), name.to_string())).map(|(t, h)| (t.as_str(), h.as_str()))
    }

    /// Everything `other` holds, kept too.
    pub fn extend(&mut self, other: &Files) {
        self.by_version.extend(other.by_version.iter().map(|(k, v)| (k.clone(), v.clone())));
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
    /// A component's files were not fetched yet: `(version, name)`
    /// ([`App::needs`] lists it).
    Missing(String, String),
    /// The tree did not parse or mount, or the handlers were refused: why.
    Failed { component: String, message: String },
}

impl std::fmt::Display for SwapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SwapError::Unknown(c) => write!(f, "no component {c} in this version"),
            SwapError::Missing(v, n) => write!(f, "{n} of {v} is not fetched yet"),
            SwapError::Failed { component, message } => write!(f, "{component} failed to mount: {message}"),
        }
    }
}

/// The section menu (a right-click on a section): the live bundle's
/// components by name, and the section's component's other stored
/// versions — those whose hash for it differs from the live one's —
/// newest first, the live one excluded.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Menu {
    /// The section's component, as drawn.
    pub component: String,
    pub components: Vec<String>,
    pub versions: Vec<String>,
}

/// A component ready to mount: its tree and folds, checked.
struct Prepared {
    version: String,
    tree: Rc<ComponentTree>,
    folds: Rc<Folds>,
    tree_hash: String,
    handlers_hash: String,
}

/// A mounted instance.
struct Inst<R: Renderer> {
    name: String,
    version: String,
    tree_hash: String,
    handlers_hash: String,
    /// Drawn from an assignment's version: a swap leaves it.
    pinned: bool,
    element: R::Node,
    props: Map<String, Value>,
    state: State,
    live: BTreeMap<String, Signal<Value>>,
    readers: Vec<Reader>,
    folds: Rc<Folds>,
    tree: Rc<ComponentTree>,
    mounted: Option<Mounted<R>>,
    /// Its last swap failed: it carries `data-failed`.
    failed: bool,
}

/// A section drawn: its `div`, its props and what fills it.
struct Section<R: Renderer> {
    sid: u64,
    layout: String,
    slot: String,
    div: R::Node,
    props: Map<String, Value>,
    instance: Option<MountId>,
    /// What it waits on, when its files are not fetched yet.
    waiting: Option<(String, String, bool)>,
}

struct St<R: Renderer> {
    version: String,
    manifest: Manifest,
    files: Files,
    needs: Vec<(String, String)>,
    instances: BTreeMap<MountId, Inst<R>>,
    next: u64,
    assignments: Vec<Assignment>,
    sections: Vec<Section<R>>,
    console: Vec<String>,
}

/// The engine: the registry, the instances, the sections — over a
/// renderer, the wire (for `call` and `live`) and a clock.
pub struct App<R: Renderer, W: Wire, C: Clock> {
    rt: Runtime,
    renderer: R,
    wire: W,
    cache: Rc<LiveCache<W, C>>,
    st: RefCell<St<R>>,
    navigate: RefCell<Option<Rc<dyn Fn(&str)>>>,
    me: Weak<Self>,
}

/// The engine's side of every mount ([`Children`]).
struct Kids<R: Renderer, W: Wire, C: Clock>(Weak<App<R, W, C>>);

impl<R: Renderer, W: Wire + Clone + 'static, C: Clock + Clone + 'static> Children<R> for Kids<R, W, C> {
    fn mount_component(&self, at: &R::Node, name: &str, props: Map<String, Value>) {
        if let Some(app) = self.0.upgrade() {
            let version = app.st.borrow().version.clone();
            if let Err(e) = app.instantiate(at, name, &version, false, props) {
                app.say(&format!("{name} ({version}): {e}"));
            }
        }
    }

    fn mount_slot(&self, at: &R::Node, layout: &str, slot: &str, props: Map<String, Value>) {
        if let Some(app) = self.0.upgrade() {
            let sid = app.next_id().0;
            let s = Section { sid, layout: layout.into(), slot: slot.into(), div: at.clone(), props, instance: None, waiting: None };
            app.st.borrow_mut().sections.push(s);
            app.fill(sid);
        }
    }

    fn unmount(&self, at: &R::Node) {
        if let Some(app) = self.0.upgrade() {
            app.unmount_at(at);
        }
    }
}

/// The folds' host: `call` through the wire, `navigate` to the router.
struct AppHost<R: Renderer, W: Wire, C: Clock>(Weak<App<R, W, C>>);

impl<R: Renderer, W: Wire + Clone + 'static, C: Clock + Clone + 'static> Host for AppHost<R, W, C> {
    fn call(&self, method: &str, params: Value) {
        if let Some(app) = self.0.upgrade() {
            app.wire.open(method, params, Box::new(|_| {}));
        }
    }
    fn navigate(&self, route: &str) {
        let f = self.0.upgrade().and_then(|app| app.navigate.borrow().clone());
        if let Some(f) = f {
            f(route);
        }
    }
}

impl<R: Renderer, W: Wire + Clone + 'static, C: Clock + Clone + 'static> App<R, W, C> {
    pub fn new(rt: &Runtime, renderer: R, wire: W, clock: C) -> Rc<Self> {
        let cache = LiveCache::new(rt, wire.clone(), clock);
        Rc::new_cyclic(|me| Self {
            rt: rt.clone(),
            renderer,
            wire,
            cache,
            st: RefCell::new(St {
                version: String::new(),
                manifest: Manifest::default(),
                files: Files::default(),
                needs: Vec::new(),
                instances: BTreeMap::new(),
                next: 0,
                assignments: Vec::new(),
                sections: Vec::new(),
                console: Vec::new(),
            }),
            navigate: RefCell::new(None),
            me: me.clone(),
        })
    }

    /// The `live` subscriptions (the page sweeps and reconnects them).
    pub fn cache(&self) -> &Rc<LiveCache<W, C>> {
        &self.cache
    }

    /// Where a handler's `navigate` goes.
    pub fn set_navigate(&self, f: Rc<dyn Fn(&str)>) {
        *self.navigate.borrow_mut() = Some(f);
    }

    /// The live version: its hash and manifest. Mounting needs its
    /// components' files ([`Self::provide`]).
    pub fn load(&self, version: &str, manifest: Manifest) {
        let mut st = self.st.borrow_mut();
        st.version = version.into();
        st.manifest = manifest;
    }

    /// The live version's hash.
    pub fn version(&self) -> String {
        self.st.borrow().version.clone()
    }

    /// The live version's manifest.
    pub fn manifest(&self) -> Manifest {
        self.st.borrow().manifest.clone()
    }

    /// Files fetched: kept; every section waiting on them is filled.
    pub fn provide(&self, files: &Files) {
        let waiting: Vec<u64> = {
            let mut st = self.st.borrow_mut();
            st.files.extend(files);
            let have = st.files.clone();
            st.needs.retain(|(v, n)| have.version(v, n).is_none());
            st.sections.iter().filter(|s| s.waiting.as_ref().is_some_and(|(v, n, _)| have.version(v, n).is_some())).map(|s| s.sid).collect()
        };
        for sid in waiting {
            self.fill(sid);
        }
    }

    /// What the engine asks to be fetched: `(version, component)`, each once.
    pub fn needs(&self) -> Vec<(String, String)> {
        self.st.borrow().needs.clone()
    }

    fn say(&self, line: &str) {
        #[cfg(feature = "dom")]
        web_sys::console::warn_1(&line.into());
        self.st.borrow_mut().console.push(line.to_string());
    }

    /// `name` of `version`, checked against what the live manifest names
    /// (for the live version), its handlers compiled.
    fn prepare(&self, version: &str, name: &str) -> Result<Prepared, SwapError> {
        let (files, entry) = {
            let mut st = self.st.borrow_mut();
            let entry = (version == st.version).then(|| st.manifest.components.get(name).cloned());
            if let Some(None) = entry {
                return Err(SwapError::Unknown(name.into()));
            }
            match st.files.version(version, name) {
                Some((t, h)) => ((t.to_string(), h.to_string()), entry.flatten()),
                None => {
                    let need = (version.to_string(), name.to_string());
                    if !st.needs.contains(&need) {
                        st.needs.push(need);
                    }
                    return Err(SwapError::Missing(version.into(), name.into()));
                }
            }
        };
        let failed = |message: String| SwapError::Failed { component: name.into(), message };
        let (tree_hash, handlers_hash) = (sha256_hex(&files.0), sha256_hex(&files.1));
        if let Some(e) = &entry {
            if e.tree != tree_hash || e.handlers != handlers_hash {
                return Err(failed("its files are not the ones the manifest names (their hashes differ)".into()));
            }
        }
        let tree = ComponentTree::parse(&files.0).map_err(|e| failed(e.to_string()))?;
        let host: Rc<dyn Host> = Rc::new(AppHost(self.me.clone()));
        let folds = Folds::compile(&files.1, &tree.uses, host).map_err(|e| failed(e.to_string()))?;
        Ok(Prepared { version: version.into(), tree: Rc::new(tree), folds: Rc::new(folds), tree_hash, handlers_hash })
    }

    /// Draws `p` into `element` as instance `id`: its state (`state`, else
    /// its initial), its `live` readers, its tree.
    fn attach(&self, id: MountId, element: &R::Node, p: Prepared, props: Map<String, Value>, state: Option<Map<String, Value>>, pinned: bool) -> Result<(), SwapError> {
        let state = State::new(&self.rt, state.as_ref().unwrap_or(&p.tree.state));
        let mut readers = Vec::new();
        let mut live = BTreeMap::new();
        for (name, d) in &p.tree.live {
            let on: Vec<(&str, &str)> = d.on.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let params = crate::live::resolve(&d.params, &props);
            let changes_params = crate::live::resolve(&d.changes_params, &props);
            let spec = LiveSpec::new(&d.snapshot, params, d.changes.as_deref(), changes_params, &d.key, &on)
                .map_err(|message| SwapError::Failed { component: p.tree.name.clone(), message })?;
            let r = self.cache.acquire(&spec);
            live.insert(name.clone(), r.signal());
            readers.push(r);
        }
        let me = self.me.clone();
        let on_event: Rc<dyn Fn(&str, Value)> = Rc::new(move |handler: &str, ev: Value| {
            if let Some(app) = me.upgrade() {
                if let Err(e) = app.dispatch(id, handler, ev) {
                    app.say(&e.to_string());
                }
            }
        });
        let scope = Scope { props: props.clone(), state: state.clone(), live: live.clone(), on_event };
        let kids: Rc<dyn Children<R>> = Rc::new(Kids(self.me.clone()));
        let mounted = crate::tree::mount(&self.rt, &self.renderer, &p.tree, scope, element, kids)
            .map_err(|e| SwapError::Failed { component: p.tree.name.clone(), message: e.to_string() })?;
        let inst = Inst {
            name: p.tree.name.clone(),
            version: p.version,
            tree_hash: p.tree_hash,
            handlers_hash: p.handlers_hash,
            pinned,
            element: element.clone(),
            props,
            state,
            live,
            readers,
            folds: p.folds,
            tree: p.tree,
            mounted: Some(mounted),
            failed: false,
        };
        self.st.borrow_mut().instances.insert(id, inst);
        Ok(())
    }

    fn next_id(&self) -> MountId {
        let mut st = self.st.borrow_mut();
        st.next += 1;
        MountId(st.next)
    }

    /// An instance of `name` of `version` into `element`; on a failure the
    /// element carries `data-failed`.
    fn instantiate(&self, element: &R::Node, name: &str, version: &str, pinned: bool, props: Map<String, Value>) -> Result<MountId, SwapError> {
        let p = match self.prepare(version, name) {
            Ok(p) => p,
            Err(e) => {
                if !matches!(e, SwapError::Missing(..)) {
                    self.renderer.set_attr(element, "data-failed", version);
                }
                return Err(e);
            }
        };
        let id = self.next_id();
        self.attach(id, element, p, props, None, pinned).inspect_err(|_| self.renderer.set_attr(element, "data-failed", version))?;
        Ok(id)
    }

    /// Mounts `name` of the live version into a new element (named for it)
    /// appended to `parent`. A component that fails is drawn as failed (its
    /// element carries `data-failed`, the version) and the console says why.
    pub fn mount(&self, parent: &R::Node, name: &str, props: Map<String, Value>) -> Result<MountId, SwapError> {
        let el = self.renderer.create_element(name);
        let version = self.version();
        let r = self.instantiate(&el, name, &version, false, props);
        self.renderer.insert(parent, &el, None);
        r.inspect_err(|e| self.say(&format!("{name} ({version}): {e}")))
    }

    /// Takes instance `id` out: its tree, its readers, the instances in it.
    pub fn unmount(&self, id: MountId) {
        let inst = self.st.borrow_mut().instances.remove(&id);
        if let Some(mut inst) = inst {
            if let Some(m) = inst.mounted.take() {
                m.unmount();
            }
        }
    }

    fn unmount_at(&self, at: &R::Node) {
        let ids: Vec<MountId> = self.st.borrow().instances.iter().filter(|(_, i)| &i.element == at).map(|(id, _)| *id).collect();
        for id in ids {
            self.unmount(id);
        }
        let gone: Vec<Option<MountId>> = {
            let mut st = self.st.borrow_mut();
            let (out, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut st.sections).into_iter().partition(|s| &s.div == at);
            st.sections = keep;
            out.into_iter().map(|s| s.instance).collect()
        };
        for id in gone.into_iter().flatten() {
            self.unmount(id);
        }
    }

    /// Runs `handler` of instance `id` on `event`: its state replaced.
    pub fn dispatch(&self, id: MountId, handler: &str, event: Value) -> Result<(), FoldError> {
        let (folds, state, props, live) = {
            let st = self.st.borrow();
            let i = st.instances.get(&id).ok_or_else(|| FoldError::NoHandler(handler.into()))?;
            (i.folds.clone(), i.state.clone(), i.props.clone(), i.live.clone())
        };
        let rows: Map<String, Value> = live.iter().map(|(k, s)| (k.clone(), s.peek().get("rows").cloned().unwrap_or(Value::Null))).collect();
        let next = folds.run(handler, &state.snapshot(), &event, &props, &rows)?;
        state.replace(&next);
        Ok(())
    }

    /// A new live version: remounts exactly the instances drawn from the
    /// live version whose hashes changed; the files of the changed
    /// components must be provided first.
    pub fn swap(&self, version: &str, manifest: Manifest) -> SwapReport {
        let old = {
            let mut st = self.st.borrow_mut();
            let old = std::mem::replace(&mut st.version, version.into());
            st.manifest = manifest.clone();
            old
        };
        let candidates: Vec<(MountId, String, String, String)> = self
            .st
            .borrow()
            .instances
            .iter()
            .filter(|(_, i)| !i.pinned)
            .map(|(id, i)| (*id, i.name.clone(), i.tree_hash.clone(), i.handlers_hash.clone()))
            .collect();
        let mut report = SwapReport::default();
        for (id, name, tree_hash, handlers_hash) in candidates {
            if !self.st.borrow().instances.contains_key(&id) {
                continue;
            }
            let Some(entry) = manifest.components.get(&name) else {
                self.fail(id, version, &old, &name, "not in the new version".into(), &mut report);
                continue;
            };
            if entry.tree == tree_hash && entry.handlers == handlers_hash {
                continue;
            }
            match self.prepare(version, &name) {
                Ok(p) => match self.remount(id, p) {
                    Ok(()) => report.remounted.push(id),
                    Err(e) => self.fail(id, version, &old, &name, e.to_string(), &mut report),
                },
                Err(e) => self.fail(id, version, &old, &name, e.to_string(), &mut report),
            }
        }
        // A section whose default moved to another component follows it.
        let refill: Vec<u64> = {
            let st = self.st.borrow();
            st.sections
                .iter()
                .filter(|s| !st.assignments.iter().any(|a| a.layout == s.layout && a.slot == s.slot))
                .filter(|s| {
                    let default = st.manifest.slots.get(&s.layout).and_then(|m| m.get(&s.slot));
                    let drawn = s.instance.and_then(|id| st.instances.get(&id)).map(|i| &i.name);
                    default != drawn
                })
                .map(|s| s.sid)
                .collect()
        };
        for sid in refill {
            self.fill(sid);
        }
        report
    }

    fn fail(&self, id: MountId, version: &str, old: &str, name: &str, why: String, report: &mut SwapReport) {
        let (element, drawn) = {
            let mut st = self.st.borrow_mut();
            let i = st.instances.get_mut(&id).expect("an instance");
            i.failed = true;
            (i.element.clone(), i.version.clone())
        };
        self.renderer.set_attr(&element, "data-failed", version);
        self.say(&format!("{name}: {version} failed to mount ({why}); keeping its last good version {drawn}"));
        let _ = old;
        report.failed.push((id, why));
    }

    /// Instance `id` again from `p`, into its element, its props kept and
    /// its state too when the shape is the same.
    fn remount(&self, id: MountId, p: Prepared) -> Result<(), SwapError> {
        let mut inst = self.st.borrow_mut().instances.remove(&id).expect("an instance");
        let shape: Vec<String> = p.tree.state.keys().cloned().collect();
        let state = (inst.state.shape() == shape).then(|| inst.state.snapshot());
        if let Some(m) = inst.mounted.take() {
            m.unmount();
        }
        drop(std::mem::take(&mut inst.readers));
        let (element, props, pinned, was_failed) = (inst.element.clone(), inst.props.clone(), inst.pinned, inst.failed);
        let r = self.attach(id, &element, p, props, state, pinned);
        if r.is_ok() && was_failed {
            self.renderer.set_attr(&element, "data-failed", "");
        }
        r
    }

    /// The sections' assignments (`ui.slots.list`); a section with none
    /// takes `manifest.slots`' default.
    pub fn set_assignments(&self, assignments: Vec<Assignment>) {
        let changed: Vec<u64> = {
            let mut st = self.st.borrow_mut();
            let before = std::mem::replace(&mut st.assignments, assignments);
            let find = |list: &[Assignment], l: &str, s: &str| list.iter().find(|a| a.layout == l && a.slot == s).cloned();
            st.sections.iter().filter(|s| find(&before, &s.layout, &s.slot) != find(&st.assignments, &s.layout, &s.slot)).map(|s| s.sid).collect()
        };
        for sid in changed {
            self.fill(sid);
        }
    }

    /// `slot_changed` on `ui.watch`: that section remounts with its props,
    /// nothing else.
    pub fn slot_changed(&self, change: Assignment) {
        let idx = {
            let mut st = self.st.borrow_mut();
            st.assignments.retain(|a| !(a.layout == change.layout && a.slot == change.slot));
            st.assignments.push(change.clone());
            st.sections.iter().filter(|s| s.layout == change.layout && s.slot == change.slot).map(|s| s.sid).collect::<Vec<_>>()
        };
        for sid in idx {
            self.fill(sid);
        }
    }

    /// `slot_changed` back to the default (`ui.slots.clear`).
    pub fn slot_reset(&self, layout: &str, slot: &str) {
        let idx = {
            let mut st = self.st.borrow_mut();
            st.assignments.retain(|a| !(a.layout == layout && a.slot == slot));
            st.sections.iter().filter(|s| s.layout == layout && s.slot == slot).map(|s| s.sid).collect::<Vec<_>>()
        };
        for sid in idx {
            self.fill(sid);
        }
    }

    /// Section `i` drawn again: its assignment, else its default; what it
    /// had goes. Its files not fetched: it waits ([`Self::needs`]).
    fn fill(&self, sid: u64) {
        let (div, props, old, want) = {
            let st = self.st.borrow();
            let Some(s) = st.sections.iter().find(|s| s.sid == sid) else { return };
            let want = match st.assignments.iter().find(|a| a.layout == s.layout && a.slot == s.slot) {
                Some(a) => Some((a.component.clone(), a.version.clone(), true)),
                None => st.manifest.slots.get(&s.layout).and_then(|m| m.get(&s.slot)).map(|c| (c.clone(), st.version.clone(), false)),
            };
            let old = s.instance.and_then(|id| st.instances.get(&id).map(|inst| (id, inst.element.clone())));
            (s.div.clone(), s.props.clone(), old, want)
        };
        let Some((component, version, pinned)) = want else {
            if let Some((id, el)) = old {
                self.unmount(id);
                self.renderer.remove(&div, &el);
            }
            self.with_section(sid, |s| s.instance = None);
            return;
        };
        let p = match self.prepare(&version, &component) {
            Ok(p) => p,
            Err(SwapError::Missing(..)) => {
                self.with_section(sid, |s| s.waiting = Some((version, component, pinned)));
                return;
            }
            Err(e) => {
                self.say(&format!("{component} ({version}): {e}; the section keeps what it shows"));
                return;
            }
        };
        if let Some((id, el)) = old {
            self.unmount(id);
            self.renderer.remove(&div, &el);
        }
        let el = self.renderer.create_element(&component);
        let id = self.next_id();
        let r = self.attach(id, &el, p, props, None, pinned);
        self.renderer.insert(&div, &el, None);
        let ok = r.is_ok();
        self.with_section(sid, |s| {
            s.waiting = None;
            s.instance = ok.then_some(id);
        });
        if let Err(e) = r {
            self.renderer.set_attr(&el, "data-failed", &version);
            self.say(&format!("{component} ({version}): {e}"));
        }
    }

    fn with_section(&self, sid: u64, f: impl FnOnce(&mut Section<R>)) {
        if let Some(s) = self.st.borrow_mut().sections.iter_mut().find(|s| s.sid == sid) {
            f(s);
        }
    }

    /// The instance filling `layout`'s section `slot`.
    pub fn section(&self, layout: &str, slot: &str) -> Option<MountId> {
        self.st.borrow().sections.iter().find(|s| s.layout == layout && s.slot == slot).and_then(|s| s.instance)
    }

    /// The sections drawn: `(layout, slot)` each.
    pub fn sections(&self) -> Vec<(String, String)> {
        self.st.borrow().sections.iter().map(|s| (s.layout.clone(), s.slot.clone())).collect()
    }

    /// The section menu for `layout`'s `slot`, given every stored version
    /// (newest first) with its manifest's entry for the section's component.
    pub fn menu(&self, layout: &str, slot: &str, versions: &[(String, Option<ComponentEntry>)]) -> Menu {
        let st = self.st.borrow();
        let drawn = st.sections.iter().find(|s| s.layout == layout && s.slot == slot).and_then(|s| s.instance).and_then(|id| st.instances.get(&id));
        let component = drawn
            .map(|i| i.name.clone())
            .or_else(|| st.manifest.slots.get(layout).and_then(|m| m.get(slot)).cloned())
            .unwrap_or_default();
        let live = st.manifest.components.get(&component).map(|e| (e.tree.clone(), e.handlers.clone()));
        let versions = versions
            .iter()
            .filter(|(v, _)| *v != st.version)
            .filter_map(|(v, e)| {
                let e = e.as_ref()?;
                (Some((e.tree.clone(), e.handlers.clone())) != live).then(|| v.clone())
            })
            .collect();
        Menu { component, components: st.manifest.components.keys().cloned().collect(), versions }
    }

    /// Every instance mounted: `(id, name)`.
    pub fn instances(&self) -> Vec<(MountId, String)> {
        self.st.borrow().instances.iter().map(|(id, i)| (*id, i.name.clone())).collect()
    }

    /// Instance `id`'s element.
    pub fn element_of(&self, id: MountId) -> R::Node {
        self.st.borrow().instances[&id].element.clone()
    }

    /// Instance `id`'s props.
    pub fn props_of(&self, id: MountId) -> Map<String, Value> {
        self.st.borrow().instances[&id].props.clone()
    }

    /// Instance `id`'s state now.
    pub fn state_of(&self, id: MountId) -> Map<String, Value> {
        self.st.borrow().instances[&id].state.snapshot()
    }

    /// Instance `id`'s drawn version: `(version, tree hash, handlers hash)`.
    pub fn drawn_from(&self, id: MountId) -> (String, String, String) {
        let st = self.st.borrow();
        let i = &st.instances[&id];
        let _ = (&i.tree, &i.live);
        (i.version.clone(), i.tree_hash.clone(), i.handlers_hash.clone())
    }

    /// What the engine said on the console, in order.
    pub fn console(&self) -> Vec<String> {
        self.st.borrow().console.clone()
    }
}

#[cfg(test)]
mod tests;
