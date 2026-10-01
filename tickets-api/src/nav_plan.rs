//! Test-only: AGS-20's nav `plan()` — the oracle MNG-10's roll-ups are
//! held equal to.
//!
//! Transcribed from `frontend/src/nav.rs` (the pure half: `Facts`,
//! `resolved_parent`, `place`, `nests_inside`, `plan`) with only the
//! parts a roll-up needs kept — `top`, `kids`, `subtree`, `sub_of` — plus
//! the group row's count (`group_row`: `done` is `Status::Complete`, over
//! `subtree`). The frontend is its own cargo unit (leptos, wasm), so the
//! server's tests cannot call the nav's function; this is the same
//! function, line for line, so they can call *it*. A change to the nav's
//! rule is a change here, and then the roll-ups' tests say whether the
//! server still agrees.
//!
//! Compiled twice: as `menger_tickets_api::nav_plan` under `cfg(test)` (the
//! store's tests), and by path from `tests/wire.rs` (the wire test).

use std::collections::{BTreeMap, BTreeSet};

/// The facts one ticket contributes to the forest's shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    pub id: String,
    pub epic: String,
    pub seq: u32,
    pub kind: String,
    pub parent: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Ticket(String),
    Group(String),
}

impl Node {
    pub fn key(&self) -> &str {
        match self {
            Node::Ticket(k) | Node::Group(k) => k,
        }
    }
    fn is_group(&self) -> bool {
        matches!(self, Node::Group(_))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub top: Vec<String>,
    pub kids: BTreeMap<String, Vec<Node>>,
    pub subtree: BTreeMap<String, Vec<String>>,
    pub sub_of: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place<'a> {
    Under(&'a str),
    Root(&'a str),
}

fn resolved_parent<'a>(index: &BTreeMap<&'a str, &'a Facts>, f: &'a Facts) -> Option<&'a str> {
    let p = f.parent.as_deref()?;
    let mut cur = *index.get(p)?;
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    loop {
        if cur.id == f.id {
            return None;
        }
        if !seen.insert(cur.id.as_str()) {
            return Some(p);
        }
        match cur.parent.as_deref().and_then(|n| index.get(n)) {
            Some(next) => cur = *next,
            None => return Some(p),
        }
    }
}

fn place<'a>(
    f: &'a Facts,
    resolved: &BTreeMap<&'a str, Option<&'a str>>,
    sub_of: &BTreeMap<String, String>,
) -> Place<'a> {
    let parent = resolved.get(f.id.as_str()).copied().flatten();
    match sub_of.get(&f.epic) {
        Some(host) => match parent {
            None => Place::Root(&f.epic),
            Some(p) if p == host => Place::Root(&f.epic),
            Some(p) => Place::Under(p),
        },
        None => match parent {
            Some(p) => Place::Under(p),
            None => Place::Root(&f.epic),
        },
    }
}

fn nests_inside<'a>(
    index: &BTreeMap<&'a str, &'a Facts>,
    resolved: &BTreeMap<&'a str, Option<&'a str>>,
    sub_of: &BTreeMap<String, String>,
    host: &str,
    epic: &str,
) -> bool {
    let mut cur = host.to_string();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    loop {
        if !seen.insert(cur.clone()) {
            return true;
        }
        let Some(f) = index.get(cur.as_str()) else {
            return false;
        };
        match place(f, resolved, sub_of) {
            Place::Under(p) => cur = p.to_string(),
            Place::Root(g) => {
                if g == epic {
                    return true;
                }
                match sub_of.get(g) {
                    Some(h) => cur = h.clone(),
                    None => return false,
                }
            }
        }
    }
}

/// The forest, from the facts in (epic, seq) order — `nav.rs::plan`.
pub fn plan(facts: &[Facts]) -> Plan {
    let index: BTreeMap<&str, &Facts> = facts.iter().map(|f| (f.id.as_str(), f)).collect();
    let resolved: BTreeMap<&str, Option<&str>> = facts
        .iter()
        .map(|f| (f.id.as_str(), resolved_parent(&index, f)))
        .collect();

    let mut scope_root: BTreeMap<&str, &Facts> = BTreeMap::new();
    for f in facts.iter().filter(|f| f.kind == "scope") {
        scope_root
            .entry(f.epic.as_str())
            .and_modify(|cur| {
                if f.seq < cur.seq {
                    *cur = f;
                }
            })
            .or_insert(f);
    }
    let mut sub_of: BTreeMap<String, String> = BTreeMap::new();
    for (epic, root) in &scope_root {
        let Some(host) = resolved.get(root.id.as_str()).copied().flatten() else {
            continue;
        };
        let Some(hf) = index.get(host) else { continue };
        if hf.epic == **epic {
            continue;
        }
        sub_of.insert((*epic).to_string(), host.to_string());
        if nests_inside(&index, &resolved, &sub_of, host, epic) {
            sub_of.remove(*epic);
        }
    }

    let mut kids: BTreeMap<String, Vec<Node>> = BTreeMap::new();
    for f in facts {
        let (key, node) = match place(f, &resolved, &sub_of) {
            Place::Under(p) => (p.to_string(), Node::Ticket(f.id.clone())),
            Place::Root(g) => (g.to_string(), Node::Ticket(f.id.clone())),
        };
        kids.entry(key).or_default().push(node);
    }
    for (epic, host) in &sub_of {
        kids.entry(host.clone())
            .or_default()
            .push(Node::Group(epic.clone()));
    }

    let mut top: Vec<String> = Vec::new();
    for f in facts {
        if sub_of.contains_key(&f.epic) || top.iter().any(|t| t == &f.epic) {
            continue;
        }
        if kids.contains_key(&f.epic) {
            top.push(f.epic.clone());
        }
    }

    let mut subtree: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for g in top.iter().chain(sub_of.keys()) {
        let mut out: Vec<String> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<String> = vec![g.clone()];
        while let Some(k) = stack.pop() {
            for n in kids.get(&k).into_iter().flatten() {
                if !seen.insert(n.key().to_string()) {
                    continue;
                }
                if !n.is_group() {
                    out.push(n.key().to_string());
                }
                stack.push(n.key().to_string());
            }
        }
        out.sort();
        subtree.insert(g.clone(), out);
    }

    Plan {
        top,
        kids,
        subtree,
        sub_of,
    }
}

/// One group row as the nav draws it: `done/total` over the row's subtree
/// (`group_row`'s rollup), the shortcode's lowest-seq scope, and the host
/// its row nests under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub epic: String,
    pub total: u64,
    pub done: u64,
    pub root: Option<String>,
    pub host: Option<String>,
}

/// Every group row the nav draws for `facts` (in (epic, seq) order — the
/// store's), in shortcode order. `complete` holds the ids whose status is
/// `Complete`.
pub fn rows(facts: &[Facts], complete: &BTreeSet<String>) -> Vec<Row> {
    let p = plan(facts);
    let mut root: BTreeMap<&str, &Facts> = BTreeMap::new();
    for f in facts.iter().filter(|f| f.kind == "scope") {
        root.entry(f.epic.as_str()).or_insert(f);
    }
    let mut out: Vec<Row> = p
        .top
        .iter()
        .chain(p.sub_of.keys())
        .map(|g| {
            let ids = &p.subtree[g];
            Row {
                epic: g.clone(),
                total: ids.len() as u64,
                done: ids.iter().filter(|id| complete.contains(*id)).count() as u64,
                root: root.get(g.as_str()).map(|f| f.id.clone()),
                host: p.sub_of.get(g).cloned(),
            }
        })
        .collect();
    out.sort_by(|a, b| a.epic.cmp(&b.epic));
    out
}
