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
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

pub mod expr;
use expr::Expr;

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
    /// UI-10: `{@html expr}` — the expression's text as HTML, through the
    /// sanitizer ([`crate::html`]).
    Html(String),
}

/// A `live` declaration: the snapshot method and its params, the changes
/// method and its own params (`changes_params`), the key, and the folds by
/// event type (`upsert:ticket`, …). A param is a JSON value or `{ "expr" }`.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveDecl {
    pub snapshot: String,
    pub params: BTreeMap<String, Value>,
    /// UI-10: none — a one-shot read.
    pub changes: Option<String>,
    pub changes_params: BTreeMap<String, Value>,
    pub key: String,
    pub on: BTreeMap<String, String>,
    /// UI-10: how long the params must be quiet before it runs again.
    pub debounce_ms: Option<u64>,
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

fn bad(at: &str, what: &str) -> TreeError {
    TreeError::Malformed(format!("{at}: {what}"))
}

fn attr(v: &Value, at: &str) -> Result<Attr, TreeError> {
    match v {
        Value::String(s) => Ok(Attr::Lit(s.clone())),
        Value::Object(o) => match o.get("expr") {
            Some(Value::String(e)) if o.len() == 1 => Ok(Attr::Expr(e.clone())),
            _ => Err(bad(at, "a value is a string or { \"expr\": … }")),
        },
        Value::Number(_) | Value::Bool(_) => Ok(Attr::Lit(v.to_string())),
        _ => Err(bad(at, "a value is a string or { \"expr\": … }")),
    }
}

fn expr_of(v: Option<&Value>, at: &str) -> Result<String, TreeError> {
    match v {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Object(o)) => match o.get("expr") {
            Some(Value::String(e)) => Ok(e.clone()),
            _ => Err(bad(at, "an { \"expr\": … }")),
        },
        _ => Err(bad(at, "an { \"expr\": … }")),
    }
}

fn attrs(v: Option<&Value>, at: &str) -> Result<BTreeMap<String, Attr>, TreeError> {
    match v {
        None => Ok(BTreeMap::new()),
        Some(Value::Object(o)) => o.iter().map(|(k, v)| Ok((k.clone(), attr(v, &format!("{at}.{k}"))?))).collect(),
        Some(_) => Err(bad(at, "a map")),
    }
}

fn strings(v: Option<&Value>, at: &str) -> Result<BTreeMap<String, String>, TreeError> {
    match v {
        None => Ok(BTreeMap::new()),
        Some(Value::Object(o)) => o
            .iter()
            .map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())).ok_or_else(|| bad(&format!("{at}.{k}"), "a string")))
            .collect(),
        Some(_) => Err(bad(at, "a map of strings")),
    }
}

fn nodes(v: Option<&Value>, at: &str) -> Result<Vec<Node>, TreeError> {
    match v {
        None => Ok(Vec::new()),
        Some(Value::Array(a)) => a.iter().enumerate().map(|(i, n)| node(n, &format!("{at}[{i}]"))).collect(),
        Some(_) => Err(bad(at, "a list of nodes")),
    }
}

fn node(v: &Value, at: &str) -> Result<Node, TreeError> {
    let o = v.as_object().ok_or_else(|| bad(at, "a node is a map"))?;
    if let Some(el) = o.get("el") {
        let el = el.as_str().ok_or_else(|| bad(at, "el is a tag"))?;
        return Ok(Node::El {
            el: el.to_string(),
            attrs: attrs(o.get("attrs"), &format!("{at}.attrs"))?,
            on: strings(o.get("on"), &format!("{at}.on"))?,
            children: nodes(o.get("children"), &format!("{at}.children"))?,
        });
    }
    if let Some(t) = o.get("text") {
        return Ok(Node::Text(attr(t, &format!("{at}.text"))?));
    }
    if o.contains_key("if") {
        return Ok(Node::If {
            cond: expr_of(o.get("if"), &format!("{at}.if"))?,
            then: nodes(o.get("then"), &format!("{at}.then"))?,
            otherwise: nodes(o.get("else"), &format!("{at}.else"))?,
        });
    }
    if let Some(each) = o.get("each") {
        let e = each.as_object().ok_or_else(|| bad(at, "each is a map"))?;
        let s = |k: &str| e.get(k).and_then(Value::as_str).map(str::to_string).ok_or_else(|| bad(&format!("{at}.each"), &format!("each has a {k}")));
        return Ok(Node::Each { expr: s("expr")?, as_: s("as")?, key: s("key")?, body: nodes(o.get("body"), &format!("{at}.body"))? });
    }
    if let Some(c) = o.get("component") {
        let name = c.as_str().ok_or_else(|| bad(at, "component is a name"))?;
        return Ok(Node::Component { name: name.to_string(), props: attrs(o.get("props"), &format!("{at}.props"))? });
    }
    if o.contains_key("html") {
        return Ok(Node::Html(expr_of(o.get("html"), &format!("{at}.html"))?));
    }
    if let Some(sl) = o.get("slot") {
        let name = sl.as_str().ok_or_else(|| bad(at, "slot is a name"))?;
        return Ok(Node::Slot { name: name.to_string(), props: attrs(o.get("props"), &format!("{at}.props"))? });
    }
    Err(bad(at, "a node is el, text, if, each, component or slot"))
}

fn params(v: Option<&Value>, at: &str) -> Result<BTreeMap<String, Value>, TreeError> {
    match v {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(o)) => Ok(o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        Some(_) => Err(bad(at, "params are a map")),
    }
}

impl ComponentTree {
    /// From `components/<name>.json`'s text; refused, naming where, when it
    /// is not the contract's schema or an expression does not compile.
    pub fn parse(json: &str) -> Result<Self, TreeError> {
        let v: Value = serde_json::from_str(json).map_err(|e| TreeError::Malformed(e.to_string()))?;
        let o = v.as_object().ok_or_else(|| bad("the tree", "a map"))?;
        let s = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string).ok_or_else(|| bad("the tree", &format!("{k} is a string")));
        let list = |k: &str| -> Result<Vec<String>, TreeError> {
            match o.get(k) {
                None => Ok(Vec::new()),
                Some(Value::Array(a)) => a.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| bad(k, "a list of strings"))).collect(),
                Some(_) => Err(bad(k, "a list of strings")),
            }
        };
        let mut live = BTreeMap::new();
        if let Some(l) = o.get("live") {
            for (name, d) in l.as_object().ok_or_else(|| bad("live", "a map"))? {
                let at = format!("live.{name}");
                let d = d.as_object().ok_or_else(|| bad(&at, "a map"))?;
                let ds = |k: &str| d.get(k).and_then(Value::as_str).map(str::to_string).ok_or_else(|| bad(&at, &format!("{k} is a string")));
                live.insert(
                    name.clone(),
                    LiveDecl {
                        snapshot: ds("snapshot")?,
                        params: params(d.get("params"), &at)?,
                        changes: d.get("changes").and_then(Value::as_str).map(str::to_string),
                        changes_params: params(d.get("changes_params"), &at)?,
                        key: ds("key")?,
                        on: strings(d.get("on"), &format!("{at}.on"))?,
                        debounce_ms: d.get("debounce_ms").and_then(Value::as_u64),
                    },
                );
            }
        }
        let state = match o.get("state") {
            None => Map::new(),
            Some(Value::Object(m)) => m.clone(),
            Some(_) => return Err(bad("state", "a map")),
        };
        let tree = Self {
            name: s("name")?,
            abi: s("abi")?,
            props: list("props")?,
            state,
            live,
            uses: list("uses")?,
            handlers: o.get("handlers").and_then(Value::as_str).unwrap_or_default().to_string(),
            tree: nodes(Some(o.get("tree").ok_or_else(|| bad("the tree", "tree is a list"))?), "tree")?,
        };
        tree.check()?;
        Ok(tree)
    }

    /// Every expression in it compiles.
    pub fn check(&self) -> Result<(), TreeError> {
        fn walk(nodes: &[Node], out: &mut Vec<String>) {
            for n in nodes {
                match n {
                    Node::El { attrs, children, .. } => {
                        out.extend(attrs.values().filter_map(|a| if let Attr::Expr(e) = a { Some(e.clone()) } else { None }));
                        walk(children, out);
                    }
                    Node::Text(Attr::Expr(e)) => out.push(e.clone()),
                    Node::Text(_) => {}
                    Node::If { cond, then, otherwise } => {
                        out.push(cond.clone());
                        walk(then, out);
                        walk(otherwise, out);
                    }
                    Node::Each { expr, key, body, .. } => {
                        out.push(expr.clone());
                        out.push(key.clone());
                        walk(body, out);
                    }
                    Node::Component { props, .. } | Node::Slot { props, .. } => {
                        out.extend(props.values().filter_map(|a| if let Attr::Expr(e) = a { Some(e.clone()) } else { None }));
                    }
                    Node::Html(e) => out.push(e.clone()),
                }
            }
        }
        let mut all = Vec::new();
        walk(&self.tree, &mut all);
        for e in all {
            Expr::compile(&e).map_err(|message| TreeError::Expr { expr: e.clone(), message })?;
        }
        Ok(())
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
    /// Calls `handler` with the event's payload whenever `event` fires on
    /// `node` (`window-<event>`: on the window, while `node` is mounted).
    /// UI-10: the answer takes the listener away again — the tree calls it
    /// when the node's block goes, so no closure outlives its component.
    fn listen(&self, node: &Self::Node, event: &str, handler: Rc<dyn Fn(Value)>) -> Unlisten;
}

/// What [`Renderer::listen`] answers: called once, it removes the listener.
pub type Unlisten = Box<dyn FnOnce()>;

/// The engine's side of a mount: fills a `component` or `slot` node's
/// element, and empties it when the node goes.
pub trait Children<R: Renderer> {
    /// `name`'s instance into `at` (its element), with `props`.
    fn mount_component(&self, at: &R::Node, name: &str, props: Map<String, Value>);
    /// The section `slot` of `layout` into `at` (its `div`), with `props`.
    fn mount_slot(&self, at: &R::Node, layout: &str, slot: &str, props: Map<String, Value>);
    /// What was mounted into `at` goes (its element is being removed).
    fn unmount(&self, at: &R::Node);
}

/// A component's state as signals, one per top-level key: replacing it
/// sets only the keys whose value changed.
#[derive(Clone, Debug)]
pub struct State {
    rt: Runtime,
    keys: Rc<RefCell<BTreeMap<String, Signal<Value>>>>,
}

impl State {
    /// The state `initial` in `rt`.
    pub fn new(rt: &Runtime, initial: &Map<String, Value>) -> Self {
        let keys = initial.iter().map(|(k, v)| (k.clone(), rt.signal(v.clone()))).collect();
        Self { rt: rt.clone(), keys: Rc::new(RefCell::new(keys)) }
    }

    /// The whole state, untracked.
    pub fn snapshot(&self) -> Map<String, Value> {
        self.keys.borrow().iter().map(|(k, s)| (k.clone(), s.peek())).collect()
    }

    /// Its keys, in order: the state's shape.
    pub fn shape(&self) -> Vec<String> {
        self.keys.borrow().keys().cloned().collect()
    }

    /// A fold's return: each key set (only a changed value moves its signal),
    /// as one batch.
    pub fn replace(&self, next: &Map<String, Value>) {
        self.rt.batch(|| {
            for (k, v) in next {
                let existing = self.keys.borrow().get(k).cloned();
                match existing {
                    Some(sig) => sig.set(v.clone()),
                    None => {
                        let sig = self.rt.signal(v.clone());
                        self.keys.borrow_mut().insert(k.clone(), sig);
                    }
                }
            }
        });
    }

    /// The keys read (all of them for `None`), tracked, as a map.
    fn read(&self, keys: Option<&std::collections::BTreeSet<String>>) -> Map<String, Value> {
        let all = self.keys.borrow();
        match keys {
            None => all.iter().map(|(k, s)| (k.clone(), s.get())).collect(),
            Some(keys) => keys.iter().filter_map(|k| all.get(k).map(|s| (k.clone(), s.get()))).collect(),
        }
    }
}

/// UI-10: a `live`'s params evaluated over the scope — a `{ "expr" }`
/// value a Rhai expression over the props and `state` (its reads of state
/// tracked, so a binding around this re-runs when they change), any other
/// value as written.
pub fn eval_params(params: &BTreeMap<String, Value>, props: &Map<String, Value>, state: &State) -> Value {
    let mut out = Map::new();
    for (k, v) in params {
        let value = match v.as_object().and_then(|o| o.get("expr")).and_then(Value::as_str) {
            None => v.clone(),
            Some(text) => match Expr::compile(text) {
                Err(m) => {
                    warn(&format!("the param {k}: `{text}`: {m}"));
                    Value::Null
                }
                Ok(e) => {
                    let mut scope = rhai::Scope::new();
                    for (pk, pv) in props {
                        if e.reads.names.contains(pk) {
                            scope.push_constant_dynamic(pk.clone(), expr::to_dynamic(pv));
                        }
                    }
                    if e.reads.names.contains("state") {
                        scope.push_dynamic("state", expr::to_dynamic(&Value::Object(state.read(e.reads.state_keys.as_ref()))));
                    }
                    e.eval(&mut scope).map(|d| expr::to_value(&d)).unwrap_or_else(|m| {
                        warn(&format!("the param {k}: `{text}`: {m}"));
                        Value::Null
                    })
                }
            },
        };
        out.insert(k.clone(), value);
    }
    Value::Object(out)
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

/// Everything one mount draws with.
struct Ctx<R: Renderer> {
    rt: Runtime,
    renderer: R,
    layout: String,
    scope: Scope,
    children: Rc<dyn Children<R>>,
    exprs: RefCell<BTreeMap<String, Expr>>,
}

/// The `{#each}` variables in scope, innermost last.
type Env = Rc<Vec<(String, Signal<Value>)>>;

/// What one stretch of a tree drew: the nodes it put into its parent, the
/// bindings it made, the elements it handed to the engine, and the
/// `if` / `each` blocks inside it.
struct Block<R: Renderer> {
    top: Vec<R::Node>,
    bindings: Vec<BindingId>,
    /// The listeners its elements hold, taken away with it.
    unlisten: Vec<Unlisten>,
    hosts: Vec<R::Node>,
    dyns: Vec<Rc<RefCell<Dyn<R>>>>,
}

impl<R: Renderer> Default for Block<R> {
    fn default() -> Self {
        Self { top: Vec::new(), bindings: Vec::new(), unlisten: Vec::new(), hosts: Vec::new(), dyns: Vec::new() }
    }
}

/// An `if`'s or `each`'s current blocks, by key, in order.
struct Dyn<R: Renderer> {
    parent: R::Node,
    /// Whether its nodes sit in its block's own parent (then removing the
    /// block removes them one by one).
    top: bool,
    rows: Vec<(String, Block<R>, Option<Signal<Value>>)>,
}

impl<R: Renderer> Ctx<R> {
    fn expr(&self, text: &str) -> Option<Expr> {
        if let Some(e) = self.exprs.borrow().get(text) {
            return Some(e.clone());
        }
        match Expr::compile(text) {
            Ok(e) => {
                self.exprs.borrow_mut().insert(text.to_string(), e.clone());
                Some(e)
            }
            Err(m) => {
                warn(&format!("{}: the expression `{text}`: {m}", self.layout));
                None
            }
        }
    }

    /// Evaluates `text` in the scope with `env`, reading (tracked) only
    /// what it names.
    fn eval(&self, env: &Env, text: &str) -> rhai::Dynamic {
        self.eval_with(env, None, text)
    }

    /// [`Self::eval`] with one more constant in scope, innermost.
    fn eval_with(&self, env: &Env, extra: Option<(&str, &Value)>, text: &str) -> rhai::Dynamic {
        let Some(e) = self.expr(text) else { return rhai::Dynamic::UNIT };
        let mut scope = rhai::Scope::new();
        for (k, v) in &self.scope.props {
            if e.reads.names.contains(k) {
                scope.push_constant_dynamic(k.clone(), expr::to_dynamic(v));
            }
        }
        if e.reads.names.contains("state") {
            let state = self.scope.state.read(e.reads.state_keys.as_ref());
            scope.push_dynamic("state", expr::to_dynamic(&Value::Object(state)));
        }
        for (k, sig) in &self.scope.live {
            if e.reads.names.contains(k) {
                scope.push_dynamic(k.clone(), expr::to_dynamic(&sig.get()));
            }
        }
        for (k, sig) in env.iter() {
            if e.reads.names.contains(k) {
                scope.push_dynamic(k.clone(), expr::to_dynamic(&sig.get()));
            }
        }
        if let Some((k, v)) = extra {
            scope.push_constant_dynamic(k.to_string(), expr::to_dynamic(v));
        }
        match e.eval(&mut scope) {
            Ok(d) => d,
            Err(m) => {
                warn(&format!("{}: `{text}`: {m}", self.layout));
                rhai::Dynamic::UNIT
            }
        }
    }

    fn props(&self, env: &Env, props: &BTreeMap<String, Attr>) -> Map<String, Value> {
        self.rt.untracked(|| {
            props
                .iter()
                .map(|(k, a)| {
                    let v = match a {
                        Attr::Lit(s) => Value::String(s.clone()),
                        Attr::Expr(e) => expr::to_value(&self.eval(env, e)),
                    };
                    (k.clone(), v)
                })
                .collect()
        })
    }
}

fn warn(msg: &str) {
    #[cfg(feature = "dom")]
    web_sys::console::warn_1(&msg.into());
    #[cfg(not(feature = "dom"))]
    let _ = msg;
}

/// A binding over `compute`: its first value returned, `changed` called
/// (untracked) with each later value that differs.
fn watch<T: PartialEq + Clone + 'static>(rt: &Runtime, mut compute: impl FnMut() -> T + 'static, mut changed: impl FnMut(T) + 'static) -> (BindingId, T) {
    let last: Rc<RefCell<Option<T>>> = Rc::default();
    let (l, r) = (last.clone(), rt.clone());
    let id = rt.bind(move || {
        let v = compute();
        let mut held = l.borrow_mut();
        match held.as_ref() {
            None => *held = Some(v),
            Some(old) if *old == v => {}
            Some(_) => {
                *held = Some(v.clone());
                drop(held);
                r.untracked(|| changed(v));
            }
        }
    });
    let first = last.borrow().clone().expect("a binding runs when made");
    (id, first)
}

fn build<R: Renderer>(ctx: &Rc<Ctx<R>>, env: &Env, nodes: &[Node], parent: &R::Node, before: Option<&R::Node>, block: &mut Block<R>, top: bool) {
    for n in nodes {
        build_node(ctx, env, n, parent, before, block, top);
    }
}

fn place<R: Renderer>(ctx: &Ctx<R>, parent: &R::Node, node: &R::Node, before: Option<&R::Node>, block: &mut Block<R>, top: bool) {
    ctx.renderer.insert(parent, node, before);
    if top {
        block.top.push(node.clone());
    }
}

fn build_node<R: Renderer>(ctx: &Rc<Ctx<R>>, env: &Env, node: &Node, parent: &R::Node, before: Option<&R::Node>, block: &mut Block<R>, top: bool) {
    let r = &ctx.renderer;
    match node {
        Node::El { el, attrs, on, children } => {
            let e = r.create_element(el);
            for (name, a) in attrs {
                match a {
                    Attr::Lit(v) => r.set_attr(&e, name, v),
                    Attr::Expr(x) => {
                        let (c, env2, x2) = (ctx.clone(), env.clone(), x.clone());
                        let (c2, e2, name2) = (ctx.clone(), e.clone(), name.clone());
                        let (id, first) = watch(&ctx.rt, move || expr::as_text(&c.eval(&env2, &x2)), move |v: String| c2.renderer.set_attr(&e2, &name2, &v));
                        r.set_attr(&e, name, &first);
                        block.bindings.push(id);
                    }
                }
            }
            for (event, handler) in on {
                let (on_event, handler) = (ctx.scope.on_event.clone(), handler.clone());
                block.unlisten.push(r.listen(&e, event, Rc::new(move |payload: Value| on_event(&handler, payload))));
            }
            build(ctx, env, children, &e, None, block, false);
            place(ctx, parent, &e, before, block, top);
        }
        Node::Text(Attr::Lit(t)) => {
            let t = r.create_text(t);
            place(ctx, parent, &t, before, block, top);
        }
        Node::Text(Attr::Expr(x)) => {
            let cell: Rc<RefCell<Option<R::Node>>> = Rc::default();
            let (c, env2, x2) = (ctx.clone(), env.clone(), x.clone());
            let (c2, cell2) = (ctx.clone(), cell.clone());
            let (id, first) = watch(&ctx.rt, move || expr::as_text(&c.eval(&env2, &x2)), move |v: String| {
                if let Some(n) = cell2.borrow().as_ref() {
                    c2.renderer.set_text(n, &v);
                }
            });
            let t = r.create_text(&first);
            *cell.borrow_mut() = Some(t.clone());
            block.bindings.push(id);
            place(ctx, parent, &t, before, block, top);
        }
        Node::If { cond, then, otherwise } => {
            let anchor = r.create_text("");
            place(ctx, parent, &anchor, before, block, top);
            let d = Rc::new(RefCell::new(Dyn { parent: parent.clone(), top, rows: Vec::new() }));
            let branches = Rc::new((then.clone(), otherwise.clone()));
            let (c, env2, cond2) = (ctx.clone(), env.clone(), cond.clone());
            let (c2, env3, d2, a2, b2) = (ctx.clone(), env.clone(), d.clone(), anchor.clone(), branches.clone());
            let (id, first) = watch(&ctx.rt, move || expr::truthy(&c.eval(&env2, &cond2)), move |v: bool| {
                fill_if(&c2, &env3, &d2, &a2, &b2, v);
            });
            block.bindings.push(id);
            ctx.rt.untracked(|| fill_if(ctx, env, &d, &anchor, &branches, first));
            block.dyns.push(d);
        }
        Node::Each { expr: list, as_, key, body } => {
            let anchor = r.create_text("");
            place(ctx, parent, &anchor, before, block, top);
            let d = Rc::new(RefCell::new(Dyn { parent: parent.clone(), top, rows: Vec::new() }));
            let spec = Rc::new((as_.clone(), body.clone()));
            let (c, env2, list2, as2, key2) = (ctx.clone(), env.clone(), list.clone(), as_.clone(), key.clone());
            let (c2, env3, d2, a2, s2) = (ctx.clone(), env.clone(), d.clone(), anchor.clone(), spec.clone());
            let (id, first) = watch(
                &ctx.rt,
                move || keyed(&c, &env2, &list2, &as2, &key2),
                move |rows: Vec<(String, Value)>| reconcile(&c2, &env3, &d2, &a2, &s2, rows),
            );
            block.bindings.push(id);
            ctx.rt.untracked(|| reconcile(ctx, env, &d, &anchor, &spec, first));
            block.dyns.push(d);
        }
        Node::Component { name, props } => {
            let e = r.create_element(name);
            let props = ctx.props(env, props);
            ctx.children.mount_component(&e, name, props);
            block.hosts.push(e.clone());
            place(ctx, parent, &e, before, block, top);
        }
        Node::Html(x) => {
            let anchor = r.create_text("");
            place(ctx, parent, &anchor, before, block, top);
            let d = Rc::new(RefCell::new(Dyn { parent: parent.clone(), top, rows: Vec::new() }));
            let (c, env2, x2) = (ctx.clone(), env.clone(), x.clone());
            let (c2, d2, a2) = (ctx.clone(), d.clone(), anchor.clone());
            let (id, first) = watch(&ctx.rt, move || expr::as_text(&c.eval(&env2, &x2)), move |html: String| fill_html(&c2, &d2, &a2, &html));
            block.bindings.push(id);
            ctx.rt.untracked(|| fill_html(ctx, &d, &anchor, &first));
            block.dyns.push(d);
        }
        Node::Slot { name, props } => {
            let e = r.create_element("div");
            r.set_attr(&e, "data-layout", &ctx.layout);
            r.set_attr(&e, "data-slot", name);
            let props = ctx.props(env, props);
            ctx.children.mount_slot(&e, &ctx.layout, name, props);
            block.hosts.push(e.clone());
            place(ctx, parent, &e, before, block, top);
        }
    }
}

/// UI-10: an `{@html}`'s nodes: the old ones out, the sanitized tree drawn
/// before the anchor through the renderer (no HTML string reaches it).
fn fill_html<R: Renderer>(ctx: &Rc<Ctx<R>>, d: &Rc<RefCell<Dyn<R>>>, anchor: &R::Node, html: &str) {
    let (old, parent) = {
        let mut d = d.borrow_mut();
        (std::mem::take(&mut d.rows), d.parent.clone())
    };
    for (_, b, _) in old {
        dispose(ctx, b, Some(&parent));
    }
    fn draw<R: Renderer>(r: &R, nodes: &[crate::html::HNode], parent: &R::Node, before: Option<&R::Node>, top: &mut Vec<R::Node>) {
        for n in nodes {
            let node = match n {
                crate::html::HNode::Text(t) => r.create_text(t),
                crate::html::HNode::Element { tag, attrs, children } => {
                    let e = r.create_element(tag);
                    for (k, v) in attrs {
                        r.set_attr(&e, k, v);
                    }
                    draw(r, children, &e, None, &mut Vec::new());
                    e
                }
            };
            r.insert(parent, &node, before);
            top.push(node);
        }
    }
    let mut b = Block::default();
    draw(&ctx.renderer, &crate::html::sanitize(html), &parent, Some(anchor), &mut b.top);
    d.borrow_mut().rows.push((String::new(), b, None));
}

fn fill_if<R: Renderer>(ctx: &Rc<Ctx<R>>, env: &Env, d: &Rc<RefCell<Dyn<R>>>, anchor: &R::Node, branches: &(Vec<Node>, Vec<Node>), on: bool) {
    let (old, parent) = {
        let mut d = d.borrow_mut();
        (std::mem::take(&mut d.rows), d.parent.clone())
    };
    for (_, b, _) in old {
        dispose(ctx, b, Some(&parent));
    }
    let nodes = if on { &branches.0 } else { &branches.1 };
    if nodes.is_empty() {
        return;
    }
    let mut b = Block::default();
    build(ctx, env, nodes, &parent, Some(anchor), &mut b, true);
    d.borrow_mut().rows.push((String::new(), b, None));
}

/// The list `list` names, each item with its key (a key seen twice gets
/// its index after it).
fn keyed<R: Renderer>(ctx: &Ctx<R>, env: &Env, list: &str, as_: &str, key: &str) -> Vec<(String, Value)> {
    let items = match expr::to_value(&ctx.eval(env, list)) {
        Value::Array(a) => a,
        _ => Vec::new(),
    };
    ctx.rt.untracked(|| {
        let mut seen = std::collections::BTreeSet::new();
        items
            .into_iter()
            .enumerate()
            .map(|(i, item)| {
                let mut k = expr::as_text(&ctx.eval_with(env, Some((as_, &item)), key));
                if !seen.insert(k.clone()) {
                    k = format!("{k}#{i}");
                }
                (k, item)
            })
            .collect()
    })
}

fn reconcile<R: Renderer>(ctx: &Rc<Ctx<R>>, env: &Env, d: &Rc<RefCell<Dyn<R>>>, anchor: &R::Node, spec: &(String, Vec<Node>), rows: Vec<(String, Value)>) {
    let (mut old, parent) = {
        let mut d = d.borrow_mut();
        (std::mem::take(&mut d.rows), d.parent.clone())
    };
    // The rows that go.
    let wanted: std::collections::BTreeSet<&String> = rows.iter().map(|(k, _)| k).collect();
    let mut kept = Vec::new();
    for (k, b, s) in old.drain(..) {
        if wanted.contains(&k) {
            kept.push((k, b, s));
        } else {
            dispose(ctx, b, Some(&parent));
        }
    }
    // Whether the rows that stay keep their order.
    let kept_order: Vec<&String> = kept.iter().map(|(k, _, _)| k).collect();
    let new_order: Vec<&String> = rows.iter().map(|(k, _)| k).filter(|k| kept_order.contains(k)).collect();
    let moved = kept_order != new_order;
    let mut by_key: BTreeMap<String, (Block<R>, Option<Signal<Value>>)> = kept.into_iter().map(|(k, b, s)| (k, (b, s))).collect();
    let mut next: Vec<(String, Block<R>, Option<Signal<Value>>)> = Vec::with_capacity(rows.len());
    let mut before = anchor.clone();
    for (k, item) in rows.into_iter().rev() {
        match by_key.remove(&k) {
            Some((b, s)) => {
                if let Some(s) = &s {
                    s.set(item);
                }
                if moved {
                    for n in &b.top {
                        ctx.renderer.insert(&parent, n, Some(&before));
                    }
                }
                if let Some(first) = b.top.first() {
                    before = first.clone();
                }
                next.push((k, b, s));
            }
            None => {
                let sig = ctx.rt.signal(item);
                let mut inner = (**env).clone();
                inner.push((spec.0.clone(), sig.clone()));
                let mut b = Block::default();
                build(ctx, &Rc::new(inner), &spec.1, &parent, Some(&before), &mut b, true);
                if let Some(first) = b.top.first() {
                    before = first.clone();
                }
                next.push((k, b, Some(sig)));
            }
        }
    }
    next.reverse();
    d.borrow_mut().rows = next;
}

/// Stops `block`'s bindings, empties the engine's elements in it, and —
/// given its parent — takes its nodes out.
fn dispose<R: Renderer>(ctx: &Ctx<R>, block: Block<R>, parent: Option<&R::Node>) {
    for id in block.bindings {
        ctx.rt.dispose(id);
    }
    for off in block.unlisten {
        off();
    }
    for d in block.dyns {
        let (rows, dparent, top) = {
            let mut d = d.borrow_mut();
            (std::mem::take(&mut d.rows), d.parent.clone(), d.top)
        };
        for (_, b, _) in rows {
            dispose(ctx, b, if top && parent.is_some() { Some(&dparent) } else { None });
        }
    }
    for h in &block.hosts {
        ctx.children.unmount(h);
    }
    if let Some(p) = parent {
        for n in &block.top {
            ctx.renderer.remove(p, n);
        }
    }
}

/// A mounted tree: its nodes under the parent and its bindings.
pub struct Mounted<R: Renderer> {
    pub parent: R::Node,
    ctx: Rc<Ctx<R>>,
    block: Block<R>,
}

impl<R: Renderer> Mounted<R> {
    /// Takes every node it inserted out of the parent and stops its
    /// bindings; [`Children::unmount`] for each component and slot in it.
    pub fn unmount(self) {
        dispose(&self.ctx, self.block, Some(&self.parent));
    }

    /// The nodes it put into the parent, in order.
    pub fn top(&self) -> Vec<R::Node> {
        self.block.top.clone()
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
    tree.check()?;
    let ctx = Rc::new(Ctx { rt: rt.clone(), renderer: renderer.clone(), layout: tree.name.clone(), scope, children, exprs: RefCell::default() });
    let mut block = Block::default();
    rt.untracked(|| build(&ctx, &Rc::new(Vec::new()), &tree.tree, parent, None, &mut block, true));
    Ok(Mounted { parent: parent.clone(), ctx, block })
}

#[cfg(test)]
mod tests;
