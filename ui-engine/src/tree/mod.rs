//! UI-6: **the view tree** — UI-4's compiled `components/<name>.json`, and
//! its interpreter over a [`Renderer`].
//!
//! Node kinds (the contract's): `el` (attrs each a string or `{ "expr" }`,
//! `on` event → handler, children), `text` (a string or `{ "expr" }`),
//! `if` / `then` / `else`, `each` (`as`, `key`, body), `component` (props),
//! and `slot` (a section: its name and the props of what fills it).
//!
//! Mounting draws the tree once and makes one binding per dynamic place
//! (an attribute, a text, an `if`, an `each`): a fold that changes a state
//! key re-runs only the bindings that read it. An `if` and an `each` hold
//! their place with an empty text node (their anchor) and insert before
//! it; an `each` is keyed — a row whose key stays keeps its nodes.
//!
//! A `component` node draws as an element named for the component
//! (`<ticket-row>`), filled by the engine through [`Children`]; a `slot`
//! as a `div` carrying `data-slot` (and `data-layout`), filled the same way.

use crate::signals::{BindingId, Runtime, Signal};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::rc::Rc;

pub mod recording;
pub use recording::{Call, Recording};

#[cfg(feature = "dom")]
pub mod dom;

/// A value in the tree: a literal string, or a Rhai expression over the
/// scope.
#[derive(Debug, Clone, PartialEq)]
pub enum Attr {
    Lit(String),
    Expr(String),
}

/// One node of a compiled tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    El { el: String, attrs: BTreeMap<String, Attr>, on: BTreeMap<String, String>, children: Vec<Node> },
    Text(Attr),
    If { cond: String, then: Vec<Node>, otherwise: Vec<Node> },
    Each { expr: String, as_: String, key: String, body: Vec<Node> },
    Component { name: String, props: BTreeMap<String, Attr> },
    Slot { name: String, props: BTreeMap<String, Attr> },
}

/// A `live` declaration: the snapshot method and its params, the changes
/// method, the key, and the folds by event type (`upsert:ticket`, …).
#[derive(Debug, Clone, PartialEq)]
pub struct LiveDecl {
    pub snapshot: String,
    pub params: BTreeMap<String, Value>,
    pub changes: String,
    pub key: String,
    pub on: BTreeMap<String, String>,
}

/// A component's compiled tree — `components/<name>.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentTree {
    pub name: String,
    pub abi: String,
    pub props: Vec<String>,
    /// The declared state: its initial value (a map).
    pub state: Map<String, Value>,
    pub live: BTreeMap<String, LiveDecl>,
    pub uses: Vec<String>,
    /// The sha256 of `components/<name>.rhai`.
    pub handlers: String,
    pub tree: Vec<Node>,
}

/// Why a tree did not parse or mount.
#[derive(Debug, Clone, PartialEq)]
pub enum TreeError {
    /// Not the contract's schema: where, and what.
    Malformed(String),
    /// An expression did not compile or evaluate: the expression and why.
    Expr { expr: String, message: String },
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeError::Malformed(m) => write!(f, "a malformed tree: {m}"),
            TreeError::Expr { expr, message } => write!(f, "the expression `{expr}`: {message}"),
        }
    }
}

impl ComponentTree {
    /// From `components/<name>.json`'s text; refused, naming where, when it
    /// is not the contract's schema.
    pub fn parse(json: &str) -> Result<Self, TreeError> {
        let _ = json;
        todo!("UI-6: ComponentTree::parse")
    }
}

/// What draws: the DOM in the browser, [`Recording`] in the tests. Every
/// method takes `&self` (a handle), as `web_sys`'s do.
pub trait Renderer: Clone + 'static {
    type Node: Clone + PartialEq + std::fmt::Debug + 'static;
    fn create_element(&self, tag: &str) -> Self::Node;
    /// A text node holding `text` (an `if`'s or `each`'s anchor is an empty one).
    fn create_text(&self, text: &str) -> Self::Node;
    fn set_attr(&self, node: &Self::Node, name: &str, value: &str);
    fn set_text(&self, node: &Self::Node, text: &str);
    /// `child` into `parent`, before `before` (none: last).
    fn insert(&self, parent: &Self::Node, child: &Self::Node, before: Option<&Self::Node>);
    fn remove(&self, parent: &Self::Node, child: &Self::Node);
    /// Calls `handler` with the event's payload (`{ "value": … }` for an
    /// input) whenever `event` fires on `node`.
    fn listen(&self, node: &Self::Node, event: &str, handler: Rc<dyn Fn(Value)>);
}

/// The engine's side of a mount: fills a `component` or `slot` node's
/// element, and empties it when the node goes.
pub trait Children<R: Renderer> {
    /// `name`'s instance into `at` (its element), with `props`.
    fn mount_component(&self, at: &R::Node, name: &str, props: Map<String, Value>);
    /// The section `slot` into `at`, with `props`.
    fn mount_slot(&self, at: &R::Node, slot: &str, props: Map<String, Value>);
    /// What was mounted into `at` goes (its element is being removed).
    fn unmount(&self, at: &R::Node);
}

/// A component's state as signals, one per top-level key: replacing it
/// sets only the keys whose value changed.
#[derive(Clone, Debug)]
pub struct State {
    keys: BTreeMap<String, Signal<Value>>,
}

impl State {
    /// The state `initial` in `rt`.
    pub fn new(rt: &Runtime, initial: &Map<String, Value>) -> Self {
        let _ = (rt, initial);
        todo!("UI-6: State::new")
    }

    /// The whole state, untracked.
    pub fn snapshot(&self) -> Map<String, Value> {
        let _ = &self.keys;
        todo!("UI-6: State::snapshot")
    }

    /// A fold's return: each key set (only a changed value moves its signal).
    pub fn replace(&self, next: &Map<String, Value>) {
        let _ = next;
        todo!("UI-6: State::replace")
    }
}

/// What a tree is mounted with: the props by name, the state, the `live`
/// collections by name (each `{ rows, status }`), and where its events go.
pub struct Scope {
    pub props: Map<String, Value>,
    pub state: State,
    pub live: BTreeMap<String, Signal<Value>>,
    /// `(handler, event payload)` for each `on:` that fires.
    pub on_event: Rc<dyn Fn(&str, Value)>,
}

/// A mounted tree: its nodes under the parent and its bindings.
pub struct Mounted<R: Renderer> {
    pub parent: R::Node,
    pub roots: Vec<R::Node>,
    pub bindings: Vec<BindingId>,
}

impl<R: Renderer> Mounted<R> {
    /// Takes every node it inserted out of the parent and stops its
    /// bindings; [`Children::unmount`] for each component and slot in it.
    pub fn unmount(self, rt: &Runtime, renderer: &R, children: &dyn Children<R>) {
        let _ = (rt, renderer, children);
        todo!("UI-6: Mounted::unmount")
    }
}

/// Draws `tree` into `parent` (appended) with `scope`, through `renderer`;
/// `children` fills its components and slots.
pub fn mount<R: Renderer>(
    rt: &Runtime,
    renderer: &R,
    tree: &ComponentTree,
    scope: Scope,
    parent: &R::Node,
    children: Rc<dyn Children<R>>,
) -> Result<Mounted<R>, TreeError> {
    let _ = (rt, renderer, tree, scope, parent, children);
    todo!("UI-6: tree::mount")
}

#[cfg(test)]
mod tests;
