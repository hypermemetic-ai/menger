//! UI-6: **the recording renderer** — the native tests' [`Renderer`]: every
//! call kept in order ([`Call`]), and the tree it built kept too, so a test
//! asserts both what was called and what is drawn ([`Recording::html`]).
//! A node is a number; `0` is the root, there from the start.

use super::Renderer;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// One renderer call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Create { node: u32, tag: String },
    CreateText { node: u32, text: String },
    SetAttr { node: u32, name: String, value: String },
    SetText { node: u32, text: String },
    Insert { parent: u32, child: u32, before: Option<u32> },
    Remove { parent: u32, child: u32 },
    Listen { node: u32, event: String },
}

impl Call {
    /// The node the call made or changed (an insert's or remove's child).
    pub fn node(&self) -> u32 {
        match self {
            Call::Create { node, .. } | Call::CreateText { node, .. } | Call::SetAttr { node, .. } | Call::SetText { node, .. } | Call::Listen { node, .. } => *node,
            Call::Insert { child, .. } | Call::Remove { child, .. } => *child,
        }
    }
}

#[derive(Default)]
struct Drawn {
    /// An element's tag, or `None` for a text node.
    tag: Option<String>,
    text: String,
    attrs: BTreeMap<String, String>,
    children: Vec<u32>,
    parent: Option<u32>,
}

#[derive(Default)]
struct Log {
    calls: Vec<Call>,
    nodes: Vec<Drawn>,
    listeners: Vec<(u32, String, Rc<dyn Fn(Value)>)>,
}

/// The recording renderer: a handle; clones share one log.
#[derive(Clone)]
pub struct Recording(Rc<RefCell<Log>>);

impl std::fmt::Debug for Recording {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Recording({} calls)", self.0.borrow().calls.len())
    }
}

impl Default for Recording {
    fn default() -> Self {
        Self::new()
    }
}

impl Recording {
    /// A log holding only the root, `0` (`<root>`).
    pub fn new() -> Self {
        let log = Log { nodes: vec![Drawn { tag: Some("root".into()), ..Drawn::default() }], ..Log::default() };
        Self(Rc::new(RefCell::new(log)))
    }

    /// The root node.
    pub fn root(&self) -> u32 {
        0
    }

    /// Every call so far.
    pub fn calls(&self) -> Vec<Call> {
        self.0.borrow().calls.clone()
    }

    /// Where the log stands: [`Self::calls_since`] takes it.
    pub fn mark(&self) -> usize {
        self.0.borrow().calls.len()
    }

    /// The calls after `mark`.
    pub fn calls_since(&self, mark: usize) -> Vec<Call> {
        self.0.borrow().calls[mark..].to_vec()
    }

    /// What is drawn under `node`, as HTML: attributes in name order, a
    /// text node's text as it is (an anchor draws nothing).
    pub fn html(&self, node: u32) -> String {
        let log = self.0.borrow();
        let mut out = String::new();
        for c in &log.nodes[node as usize].children {
            draw(&log.nodes, *c, &mut out);
        }
        out
    }

    /// The nodes now in the tree under `node` (itself included), depth first.
    pub fn under(&self, node: u32) -> Vec<u32> {
        let log = self.0.borrow();
        let mut out = vec![node];
        let mut i = 0;
        while i < out.len() {
            out.extend(log.nodes[out[i] as usize].children.iter().copied());
            i += 1;
        }
        out
    }

    /// The elements now in the tree with attribute `name` = `value`.
    pub fn find_attr(&self, name: &str, value: &str) -> Vec<u32> {
        self.under(0).into_iter().filter(|n| self.0.borrow().nodes[*n as usize].attrs.get(name).is_some_and(|v| v == value)).collect()
    }

    /// The elements now in the tree named `tag`.
    pub fn find_tag(&self, tag: &str) -> Vec<u32> {
        self.under(0).into_iter().filter(|n| self.0.borrow().nodes[*n as usize].tag.as_deref() == Some(tag)).collect()
    }

    /// Fires `event` on `node` with `payload`: each listener on it runs.
    pub fn fire(&self, node: u32, event: &str, payload: Value) {
        let hit: Vec<Rc<dyn Fn(Value)>> =
            self.0.borrow().listeners.iter().filter(|(n, e, _)| *n == node && e == event).map(|(_, _, h)| h.clone()).collect();
        assert!(!hit.is_empty(), "nothing listens for {event} on node {node}");
        for h in hit {
            h(payload.clone());
        }
    }

    fn push(&self, call: Call) {
        self.0.borrow_mut().calls.push(call);
    }

    fn make(&self, drawn: Drawn) -> u32 {
        let mut log = self.0.borrow_mut();
        log.nodes.push(drawn);
        (log.nodes.len() - 1) as u32
    }
}

fn draw(nodes: &[Drawn], n: u32, out: &mut String) {
    let d = &nodes[n as usize];
    match &d.tag {
        None => out.push_str(&d.text),
        Some(tag) => {
            out.push('<');
            out.push_str(tag);
            for (k, v) in &d.attrs {
                out.push_str(&format!(" {k}=\"{v}\""));
            }
            out.push('>');
            for c in &d.children {
                draw(nodes, *c, out);
            }
            out.push_str(&format!("</{tag}>"));
        }
    }
}

impl Renderer for Recording {
    type Node = u32;

    fn create_element(&self, tag: &str) -> u32 {
        let node = self.make(Drawn { tag: Some(tag.into()), ..Drawn::default() });
        self.push(Call::Create { node, tag: tag.into() });
        node
    }

    fn create_text(&self, text: &str) -> u32 {
        let node = self.make(Drawn { text: text.into(), ..Drawn::default() });
        self.push(Call::CreateText { node, text: text.into() });
        node
    }

    fn set_attr(&self, node: &u32, name: &str, value: &str) {
        self.0.borrow_mut().nodes[*node as usize].attrs.insert(name.into(), value.into());
        self.push(Call::SetAttr { node: *node, name: name.into(), value: value.into() });
    }

    fn set_text(&self, node: &u32, text: &str) {
        self.0.borrow_mut().nodes[*node as usize].text = text.into();
        self.push(Call::SetText { node: *node, text: text.into() });
    }

    fn insert(&self, parent: &u32, child: &u32, before: Option<&u32>) {
        {
            let mut log = self.0.borrow_mut();
            if let Some(old) = log.nodes[*child as usize].parent.take() {
                log.nodes[old as usize].children.retain(|c| c != child);
            }
            let kids = &mut log.nodes[*parent as usize].children;
            let at = before.and_then(|b| kids.iter().position(|c| c == b)).unwrap_or(kids.len());
            kids.insert(at, *child);
            log.nodes[*child as usize].parent = Some(*parent);
        }
        self.push(Call::Insert { parent: *parent, child: *child, before: before.copied() });
    }

    fn remove(&self, parent: &u32, child: &u32) {
        {
            let mut log = self.0.borrow_mut();
            log.nodes[*parent as usize].children.retain(|c| c != child);
            log.nodes[*child as usize].parent = None;
        }
        self.push(Call::Remove { parent: *parent, child: *child });
    }

    fn listen(&self, node: &u32, event: &str, handler: Rc<dyn Fn(Value)>) {
        self.0.borrow_mut().listeners.push((*node, event.into(), handler));
        self.push(Call::Listen { node: *node, event: event.into() });
    }
}
