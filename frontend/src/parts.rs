//! FILTER-53: what a saved filter is made of — the card's rules
//! (src/fcard.rs) as a pure model, pinned by native tests.
//!
//! - **A tree to edit** ([`Node`]): one filter's text as groups and leaves —
//!   a leaf a predicate (a clause, or `saved "Name"`), a group its join, its
//!   `not` and its items. [`of_filter`] reads a [`Filter`] into one (the top
//!   an `all of` of its groups, or its one group); [`to_filter`] writes it
//!   back, canonical (the language's own reading of its text). A path is
//!   the items' indices from the top. The edits are [`flip`], [`negate`],
//!   [`remove`] (an emptied inner group goes with its last item), [`add`],
//!   [`replace`], [`move_item`].
//! - **The parts** ([`resolve`]): a saved filter's text as a tree of groups,
//!   rules and the saved filters it names, each name resolved recursively —
//!   safe against a cycle (a refused part, never a hang), a missing name and
//!   a text that does not read; every part says whose it is (its `owner`,
//!   the saved filter whose text holds it) and where (its `path` there), so
//!   an edit lands in the filter it belongs to. A group listing one field's
//!   values (`status in (Pending, Ready)`) is one rule with several values.
//! - **What a name needs** ([`unknown`]): the names not read yet, at any
//!   depth — so every nested name is fetched, which also fills in FILTER-37's
//!   *as evaluated* for a saved filter that names another.
//! - **Drafts** ([`inlined`]): a filter with each name the card holds an edit
//!   of put in its place — what the card counts and its nav asks, since the
//!   server reads only what is saved.
//! - **Would it name itself** ([`names_itself`]): a saved filter offered as a
//!   rule of another is refused when it reaches it.

use menger_tickets_api::filter::{Conj, Display, Event, Field, Filter, Group, History, Predicate, SAVED_DEPTH};

use crate::saved::slug;

/// One filter as a tree to edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    /// A clause, or `saved "Name"`.
    Leaf(Predicate),
    /// Items joined by one word — `not` or not.
    Group { conj: Conj, not: bool, items: Vec<Node> },
}

impl Node {
    /// An empty `all of` — a filter with nothing in it.
    pub(crate) fn empty() -> Node {
        Node::Group { conj: Conj::All, not: false, items: Vec::new() }
    }

    /// The items of a group (none of a leaf).
    pub(crate) fn items(&self) -> &[Node] {
        match self {
            Node::Group { items, .. } => items,
            Node::Leaf(_) => &[],
        }
    }

    /// The saved filters it names, as written, each once, at any depth.
    pub(crate) fn names(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        fn walk(n: &Node, out: &mut Vec<String>) {
            match n {
                Node::Leaf(Predicate { field: Field::Saved(s), .. }) => {
                    if !out.iter().any(|o| slug(o) == slug(s)) {
                        out.push(s.clone());
                    }
                }
                Node::Leaf(_) => {}
                Node::Group { items, .. } => items.iter().for_each(|i| walk(i, out)),
            }
        }
        walk(self, &mut out);
        out
    }
}

fn of_pred(p: &Predicate) -> Node {
    match &p.field {
        Field::Group(g) => match of_group(g) {
            Node::Group { conj, not, items } => Node::Group { conj, not: not != p.not, items },
            Node::Leaf(q) => Node::Leaf(Predicate { not: q.not != p.not, ..q }),
        },
        _ => Node::Leaf(p.clone()),
    }
}

fn of_group(g: &Group) -> Node {
    match g.preds.as_slice() {
        [p] => of_pred(p),
        ps => Node::Group { conj: g.conj, not: false, items: ps.iter().map(of_pred).collect() },
    }
}

/// A filter as a tree: its one group when it has one (`a or b`), else an
/// `all of` of its groups.
pub(crate) fn of_filter(f: &Filter) -> Node {
    match f.groups.as_slice() {
        [g] if g.preds.len() > 1 => of_group(g),
        gs => Node::Group { conj: Conj::All, not: false, items: gs.iter().map(of_group).collect() },
    }
}

/// A filter's text as a tree — `Err` with the language's words when it
/// does not read.
pub(crate) fn of_text(text: &str) -> Result<(Node, Display), String> {
    Filter::parse(text).map(|f| (of_filter(&f), f.display)).map_err(|e| e.to_string())
}

/// A node as one predicate — `None` for an empty group.
pub(crate) fn to_pred(n: &Node) -> Option<Predicate> {
    match n {
        Node::Leaf(p) => Some(p.clone()),
        Node::Group { conj, not, items } => {
            let preds: Vec<Predicate> = items.iter().filter_map(to_pred).collect();
            match preds.len() {
                0 => None,
                1 => {
                    let p = preds.into_iter().next()?;
                    Some(if *not { Predicate { not: !p.not, ..p } } else { p })
                }
                _ => Some(Group::of(*conj, preds).nested(*not)),
            }
        }
    }
}

/// The tree written back as a filter, `display` kept — canonical: the
/// language's own reading of its text, so it compares with a saved one.
pub(crate) fn to_filter(n: &Node, display: &Display) -> Filter {
    // The top's `and`s are the filter's groups — an `all of` inside it,
    // not negated, spliced in.
    fn chunks(p: Predicate, out: &mut Vec<Group>) {
        match p {
            Predicate { field: Field::Group(g), not: false } if g.conj == Conj::All && !is_list(&g.preds) => g.preds.into_iter().for_each(|q| chunks(q, out)),
            Predicate { field: Field::Group(g), not: false } => out.push(*g),
            p => out.push(Group::one(p)),
        }
    }
    let mut groups: Vec<Group> = Vec::new();
    match n {
        Node::Group { conj: Conj::All, not: false, items } => items.iter().filter_map(to_pred).for_each(|p| chunks(p, &mut groups)),
        n => to_pred(n).into_iter().for_each(|p| chunks(p, &mut groups)),
    }
    let raw = Filter { groups, display: display.clone() };
    Filter::parse(&raw.format()).unwrap_or(raw)
}

/// The node at `path`.
pub(crate) fn get<'a>(n: &'a Node, path: &[usize]) -> Option<&'a Node> {
    match path.split_first() {
        None => Some(n),
        Some((i, rest)) => get(n.items().get(*i)?, rest),
    }
}

fn get_mut<'a>(n: &'a mut Node, path: &[usize]) -> Option<&'a mut Node> {
    match path.split_first() {
        None => Some(n),
        Some((i, rest)) => match n {
            Node::Group { items, .. } => get_mut(items.get_mut(*i)?, rest),
            Node::Leaf(_) => None,
        },
    }
}

/// The group at `path` joined by the other word. `false`: no group there.
pub(crate) fn flip(n: &mut Node, path: &[usize]) -> bool {
    match get_mut(n, path) {
        Some(Node::Group { conj, .. }) => {
            *conj = other(*conj);
            true
        }
        _ => false,
    }
}

/// The other join.
pub(crate) fn other(c: Conj) -> Conj {
    match c {
        Conj::All => Conj::Any,
        Conj::Any => Conj::All,
    }
}

/// The rule at `path` negated: a clause's `is ⇄ is not`; a list's `is any
/// of ⇄ is none of` (each value negated, the join the other).
pub(crate) fn negate(n: &mut Node, path: &[usize]) -> bool {
    match get_mut(n, path) {
        Some(Node::Leaf(p)) => {
            p.not = !p.not;
            true
        }
        Some(g @ Node::Group { .. }) if list_of(g).is_some() => {
            if let Node::Group { conj, items, .. } = g {
                *conj = other(*conj);
                for i in items.iter_mut() {
                    if let Node::Leaf(p) = i {
                        p.not = !p.not;
                    }
                }
            }
            true
        }
        _ => false,
    }
}

/// The item at `path` taken out — and every inner group it leaves empty
/// (never the top).
pub(crate) fn remove(n: &mut Node, path: &[usize]) -> bool {
    let Some((last, parent)) = path.split_last() else { return false };
    let Some(Node::Group { items, .. }) = get_mut(n, parent) else { return false };
    if *last >= items.len() {
        return false;
    }
    items.remove(*last);
    if items.is_empty() && !parent.is_empty() {
        remove(n, parent);
    }
    true
}

/// `item` added at the end of the group at `path` — its path.
pub(crate) fn add(n: &mut Node, path: &[usize], item: Node) -> Option<Vec<usize>> {
    match get_mut(n, path) {
        Some(Node::Group { items, .. }) => {
            items.push(item);
            Some(path.iter().copied().chain([items.len() - 1]).collect())
        }
        _ => None,
    }
}

/// The node at `path` replaced by `with`.
pub(crate) fn replace(n: &mut Node, path: &[usize], with: Node) -> bool {
    match get_mut(n, path) {
        Some(at) => {
            *at = with;
            true
        }
        None => false,
    }
}

/// In the group at `path`, its item `from` moved to `to` (before what was there).
pub(crate) fn move_item(n: &mut Node, path: &[usize], from: usize, to: usize) -> bool {
    match get_mut(n, path) {
        Some(Node::Group { items, .. }) if from < items.len() && to <= items.len() && from != to => {
            let it = items.remove(from);
            items.insert(if to > from { to - 1 } else { to }, it);
            true
        }
        _ => false,
    }
}

/// Whether predicates list one field's values (`not status in (A, B)`).
fn is_list(ps: &[Predicate]) -> bool {
    ps.len() > 1 && listable(&ps[0].field) && ps.iter().all(|p| p.field.name() == ps[0].field.name() && p.not == ps[0].not && !matches!(p.field, Field::Group(_)))
}

/// Whether a field may list values (`status in (A, B)`) — the language's.
fn listable(f: &Field) -> bool {
    matches!(f, Field::Status(_) | Field::Category(_) | Field::Kind(_) | Field::Severity(_) | Field::In(_) | Field::Under(_) | Field::Label(_))
}

/// A group that lists one field's values — two or more leaves of one
/// listable field, all `not` or none, joined as a list joins (`any of`
/// as is, `all of` negated): its field, whether negated, its values.
pub(crate) fn list_of(n: &Node) -> Option<(Field, bool, Vec<String>)> {
    let Node::Group { conj, not: false, items } = n else { return None };
    let leaves: Vec<&Predicate> = items.iter().map(|i| if let Node::Leaf(p) = i { Some(p) } else { None }).collect::<Option<_>>()?;
    let first = leaves.first()?;
    let same = leaves.iter().all(|p| p.field.name() == first.field.name() && p.not == first.not);
    let joined = *conj == if first.not { Conj::All } else { Conj::Any };
    (leaves.len() > 1 && listable(&first.field) && same && joined && !matches!(first.field, Field::Saved(_)))
        .then(|| (first.field.clone(), first.not, leaves.iter().filter_map(|p| p.field.value()).collect()))
}

// ---------------------------------------------------------------------
// The parts

/// What a name's text is, as the card knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Got {
    /// Not read yet.
    Unknown,
    /// No saved filter holds it.
    Missing,
    /// Its tree (an edit the card holds, or its saved text).
    Node(Node),
    /// Its text does not read, or could not be read.
    Bad(String),
}

/// One row of the card's rules, as it says itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rule {
    /// The field (`Status`, `Kind`, `Flag`, `When`…).
    pub(crate) field: String,
    /// The operator (`is`, `is not`, `is any of`, `is none of`).
    pub(crate) op: String,
    pub(crate) not: bool,
    /// Its values, as tokens.
    pub(crate) values: Vec<String>,
    /// A time clause (FILTER-54 makes it a rule's time): drawn as its text,
    /// read only.
    pub(crate) time: bool,
    /// The rule in the language.
    pub(crate) text: String,
}

/// What a name names, resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Inner {
    /// Its parts: a group, owned by the name.
    Parts(Box<Part>),
    /// Its text is being read.
    Loading,
    /// No saved filter holds the name.
    Missing,
    /// It names a filter being expanded — the chain, `"A" names "B" names "A"`.
    Cycle(String),
    /// Names nested past [`SAVED_DEPTH`].
    TooDeep,
    /// Its text does not read.
    Bad(String),
}

/// A part of a saved filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Rule(Rule),
    Group { conj: Conj, not: bool, items: Vec<Part> },
    Named { name: String, not: bool, inner: Inner },
}

/// A part: whose text holds it (its `owner`, a saved filter's name), where
/// (`path` in that text's tree), and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Part {
    pub(crate) owner: String,
    pub(crate) path: Vec<usize>,
    pub(crate) shape: Shape,
}

impl Part {
    /// How many rules it holds — a rule and a named filter one each, a
    /// group its items' (what the chip's `4 rules` says).
    pub(crate) fn rules(&self) -> usize {
        match &self.shape {
            Shape::Rule(_) | Shape::Named { .. } => 1,
            Shape::Group { items, .. } => items.iter().map(Part::rules).sum(),
        }
    }
}

/// A clause as a rule says it.
pub(crate) fn rule_of(p: &Predicate) -> Rule {
    let is = |many: bool| match (p.not, many) {
        (false, false) => "is",
        (true, false) => "is not",
        (false, true) => "is any of",
        (true, true) => "is none of",
    };
    let (field, values, time): (&str, Vec<String>, bool) = match &p.field {
        Field::Status(_) => ("Status", p.field.value().into_iter().collect(), false),
        Field::Category(_) => ("Category", p.field.value().into_iter().collect(), false),
        Field::Kind(_) => ("Kind", p.field.value().into_iter().collect(), false),
        Field::Severity(_) => ("Severity", p.field.value().into_iter().collect(), false),
        Field::Label(_) => ("Label", p.field.value().into_iter().collect(), false),
        Field::In(_) => ("Shortcode", p.field.value().into_iter().collect(), false),
        Field::Under(_) => ("Under", p.field.value().into_iter().collect(), false),
        Field::Ticket(_) => ("Ticket", p.field.value().into_iter().collect(), false),
        Field::Text(t) => ("Title", vec![t.clone()], false),
        Field::History(h) if timed(h) => ("When", vec![Predicate { not: false, ..p.clone() }.format()], true),
        Field::History(_) => ("Flag", vec![Predicate { not: false, ..p.clone() }.format()], false),
        Field::Saved(n) => ("Saved filter", vec![n.clone()], false),
        Field::Group(g) => ("Group", vec![g.format()], false),
        f => ("Flag", vec![f.name().to_string()], false),
    };
    let op = match &p.field {
        Field::Text(_) => if p.not { "lacks" } else { "contains" }.to_string(),
        Field::History(h) if timed(h) => if p.not { "not" } else { "" }.to_string(),
        _ => is(false).to_string(),
    };
    Rule { field: field.into(), op, not: p.not, values, time, text: p.format() }
}

/// Whether a history clause is a time (FILTER-54 makes it a rule's): a
/// window, or a transition, a birth, a touch or a hold — never `queued`,
/// `asked` or a delivery stage without one, which read as flags.
fn timed(h: &History) -> bool {
    h.window.is_some() || matches!(h.event, Event::Entered(_) | Event::Left(_) | Event::Changed | Event::Created | Event::Touched | Event::Held(_) | Event::Became(_))
}

fn rule_of_list(field: &Field, not: bool, values: Vec<String>, n: &Node) -> Rule {
    let r = rule_of(&Predicate { field: field.clone(), not });
    let op = if not { "is none of" } else { "is any of" };
    Rule { op: op.into(), values, text: to_pred(n).map(|p| p.format()).unwrap_or_default(), ..r }
}

/// A saved filter's parts: `name`'s tree, every name in it resolved by
/// `lookup` — recursively, a cycle, a missing name or a text that does not
/// read a refused part. The answer is the name's own part ([`Shape::Named`],
/// owned by no one).
pub(crate) fn resolve(name: &str, lookup: &dyn Fn(&str) -> Got) -> Part {
    let mut stack = Vec::new();
    Part { owner: String::new(), path: Vec::new(), shape: named(name, false, lookup, &mut stack) }
}

/// A filter that is no saved one (⌂ everywhere's): its parts, owned by
/// `owner`, its names resolved.
pub(crate) fn resolve_node(owner: &str, n: &Node, lookup: &dyn Fn(&str) -> Got) -> Part {
    let mut stack = vec![(slug(owner), owner.to_string())];
    part(owner, Vec::new(), n, lookup, &mut stack)
}

fn named(name: &str, not: bool, lookup: &dyn Fn(&str) -> Got, stack: &mut Vec<(String, String)>) -> Shape {
    let key = slug(name);
    let inner = if let Some(at) = stack.iter().position(|(k, _)| *k == key) {
        let chain: Vec<String> = stack[at..].iter().map(|(_, n)| n.as_str()).chain([name]).map(|n| format!("\u{201c}{n}\u{201d}")).collect();
        Inner::Cycle(chain.join(" names "))
    } else if stack.len() >= SAVED_DEPTH {
        Inner::TooDeep
    } else {
        match lookup(name) {
            Got::Unknown => Inner::Loading,
            Got::Missing => Inner::Missing,
            Got::Bad(why) => Inner::Bad(why),
            Got::Node(n) => {
                stack.push((key, name.to_string()));
                let p = part(name, Vec::new(), &n, lookup, stack);
                stack.pop();
                Inner::Parts(Box::new(p))
            }
        }
    };
    Shape::Named { name: name.to_string(), not, inner }
}

fn part(owner: &str, path: Vec<usize>, n: &Node, lookup: &dyn Fn(&str) -> Got, stack: &mut Vec<(String, String)>) -> Part {
    let shape = match n {
        Node::Leaf(Predicate { field: Field::Saved(name), not }) => named(name, *not, lookup, stack),
        Node::Leaf(p) => Shape::Rule(rule_of(p)),
        g => match list_of(g) {
            // A list is drawn as one rule — but the top is always a group.
            Some((field, not, values)) if !path.is_empty() => Shape::Rule(rule_of_list(&field, not, values, g)),
            _ => {
                let Node::Group { conj, not, items } = g else { unreachable!() };
                let items = items
                    .iter()
                    .enumerate()
                    .map(|(i, it)| part(owner, path.iter().copied().chain([i]).collect(), it, lookup, stack))
                    .collect();
                Shape::Group { conj: *conj, not: *not, items }
            }
        },
    };
    Part { owner: owner.to_string(), path, shape }
}

/// Every name the parts wait on (not read yet), each once.
pub(crate) fn unknown(p: &Part) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    fn walk(p: &Part, out: &mut Vec<String>) {
        match &p.shape {
            Shape::Rule(_) => {}
            Shape::Group { items, .. } => items.iter().for_each(|i| walk(i, out)),
            Shape::Named { name, inner, .. } => match inner {
                Inner::Loading => {
                    if !out.iter().any(|o| slug(o) == slug(name)) {
                        out.push(name.clone());
                    }
                }
                Inner::Parts(p) => walk(p, out),
                _ => {}
            },
        }
    }
    walk(p, &mut out);
    out
}

/// `n` with every name `drafted` holds an edit of put in its place, as a
/// group of its edit (itself inlined) — a cycle left as written.
pub(crate) fn inlined(n: &Node, drafted: &dyn Fn(&str) -> Option<Node>) -> Node {
    fn go(n: &Node, drafted: &dyn Fn(&str) -> Option<Node>, stack: &mut Vec<String>) -> Node {
        match n {
            Node::Leaf(Predicate { field: Field::Saved(name), not }) if !stack.contains(&slug(name)) && stack.len() < SAVED_DEPTH => match drafted(name) {
                Some(d) => {
                    stack.push(slug(name));
                    let inner = go(&d, drafted, stack);
                    stack.pop();
                    match inner {
                        Node::Group { conj, not: inner_not, items } => Node::Group { conj, not: inner_not != *not, items },
                        Node::Leaf(p) => Node::Leaf(Predicate { not: p.not != *not, ..p }),
                    }
                }
                None => n.clone(),
            },
            Node::Leaf(_) => n.clone(),
            Node::Group { conj, not, items } => Node::Group { conj: *conj, not: *not, items: items.iter().map(|i| go(i, drafted, stack)).collect() },
        }
    }
    go(n, drafted, &mut Vec::new())
}

/// Whether `candidate`, put in `owner`'s text, would name `owner` — it is
/// `owner`, or reaches it through the names it holds (`lookup`'s trees).
pub(crate) fn names_itself(owner: &str, candidate: &str, lookup: &dyn Fn(&str) -> Got) -> bool {
    fn reaches(at: &str, target: &str, lookup: &dyn Fn(&str) -> Got, depth: usize) -> bool {
        if slug(at) == slug(target) {
            return true;
        }
        if depth >= SAVED_DEPTH {
            return false;
        }
        match lookup(at) {
            Got::Node(n) => n.names().iter().any(|m| reaches(m, target, lookup, depth + 1)),
            _ => false,
        }
    }
    reaches(candidate, owner, lookup, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The saved filters on app.menger.sh (2026-10-23), with FILTER-50's
    /// synthetic Plexus-Attention, which names two of them in a nested group.
    fn seven() -> HashMap<String, String> {
        [
            ("Created-Active-Weekly", "not menger doc and created this week and not status in (Pending, Complete) and not kind doc"),
            ("Deployment", "in (ACCESS, DEPLOY, FILTER, VIEW) and not status in (Coding, Complete, Superseded, Canceled) and not kind doc"),
            ("Needs you", "queued or asking or asked or label needs-you"),
            ("Plexus-Coding", "in (CONTEXT, HYPERFORGE, IDENTITY, MACRO, MANIFEST, PLEXUS, RETURNS, STARTER, SYNAPSE, TRANSPORT) and not blocked and not status Coding"),
            ("Plexus-Ready", "in (CONTEXT, HYPERFORGE, IDENTITY, MACRO, MANIFEST, PLEXUS, RETURNS, STARTER, SYNAPSE, TRANSPORT) and not status Complete and not blocked"),
            ("Shipped today", "deployed app-prod today"),
            ("Plexus-Attention", "saved \"Plexus-Ready\" and (saved \"Needs you\" or (kind gap and severity in (High, Critical)))"),
        ]
        .into_iter()
        .map(|(n, t)| (slug(n), t.to_string()))
        .collect()
    }

    fn lookup_in(m: &HashMap<String, String>) -> impl Fn(&str) -> Got + '_ {
        move |name| match m.get(&slug(name)) {
            None => Got::Missing,
            Some(t) => match of_text(t) {
                Ok((n, _)) => Got::Node(n),
                Err(e) => Got::Bad(e),
            },
        }
    }

    fn inner(p: &Part) -> &Part {
        match &p.shape {
            Shape::Named { inner: Inner::Parts(p), .. } => p,
            s => panic!("not resolved: {s:?}"),
        }
    }

    fn items(p: &Part) -> &[Part] {
        match &p.shape {
            Shape::Group { items, .. } => items,
            s => panic!("not a group: {s:?}"),
        }
    }

    #[test]
    fn a_rule_with_its_time_is_a_read_only_time_row() {
        // FILTER-52's `status S <time>`: the time is FILTER-54's to edit.
        for text in ["status Ready within 12h", "status in (Ready, Coding) today", "kind gap this week"] {
            let (n, _) = of_text(text).expect(text);
            let Node::Group { items, .. } = &n else { panic!("{text}: {n:?}") };
            let [Node::Leaf(p)] = items.as_slice() else { panic!("{text}: one clause, {n:?}") };
            let r = rule_of(p);
            assert!(r.time && r.field == "When", "{text}: {r:?}");
        }
    }

    fn rule(p: &Part) -> &Rule {
        match &p.shape {
            Shape::Rule(r) => r,
            s => panic!("not a rule: {s:?}"),
        }
    }

    #[test]
    fn the_seven_saved_filters_read_as_parts() {
        let m = seven();
        let look = lookup_in(&m);
        for name in ["Created-Active-Weekly", "Deployment", "Needs you", "Plexus-Coding", "Plexus-Ready", "Shipped today", "Plexus-Attention"] {
            let p = resolve(name, &look);
            assert!(unknown(&p).is_empty(), "{name}: nothing waits");
            let top = inner(&p);
            assert_eq!(top.owner, name, "{name}: its top is its own");
            // Written back, each is its saved text.
            let (n, d) = of_text(&m[&slug(name)]).unwrap();
            assert_eq!(to_filter(&n, &d).format(), Filter::parse(&m[&slug(name)]).unwrap().format(), "{name} round-trips");
        }
        // Deployment: a list of shortcodes, a list of statuses negated, a kind.
        let d = resolve("Deployment", &look);
        let rows = items(inner(&d));
        assert_eq!(rows.len(), 3);
        assert_eq!(rule(&rows[0]).field, "Shortcode");
        assert_eq!(rule(&rows[0]).op, "is any of");
        assert_eq!(rule(&rows[0]).values, ["ACCESS", "DEPLOY", "FILTER", "VIEW"]);
        assert_eq!(rule(&rows[1]).op, "is none of");
        assert!(rule(&rows[1]).not);
        assert_eq!(rule(&rows[1]).values, ["Coding", "Complete", "Superseded", "Canceled"]);
        assert_eq!((rule(&rows[2]).field.as_str(), rule(&rows[2]).op.as_str()), ("Kind", "is not"));
        // Needs you: one group of four, joined by or — its top.
        let ny = resolve("Needs you", &look);
        match &inner(&ny).shape {
            Shape::Group { conj: Conj::Any, items, .. } => assert_eq!(items.len(), 4),
            s => panic!("{s:?}"),
        }
        // Needs you's clauses are flags, not times.
        assert!(items(inner(&ny)).iter().map(rule).all(|r| !r.time && r.op == "is"), "queued, asking, asked, a label");
        assert_eq!(rule(&items(inner(&ny))[0]).values, ["queued"]);
        // Created-Active-Weekly's time clause: read only, its text.
        let caw = resolve("Created-Active-Weekly", &look);
        let t = items(inner(&caw)).iter().map(rule).find(|r| r.time).expect("a time rule");
        assert_eq!(t.values, ["created this week"]);
        // Plexus-Attention: a name, then an or-group holding a name and an and-group.
        let pa = resolve("Plexus-Attention", &look);
        let top = inner(&pa);
        assert_eq!(top.rules(), 4);
        let rows = items(top);
        match &rows[0].shape {
            Shape::Named { name, inner: Inner::Parts(p), .. } => {
                assert_eq!(name, "Plexus-Ready");
                assert_eq!(p.owner, "Plexus-Ready", "a part inside a name is that name's");
                assert_eq!(p.rules(), 3);
            }
            s => panic!("{s:?}"),
        }
        assert_eq!(rows[1].owner, "Plexus-Attention");
        assert_eq!(rows[1].path, [1]);
        let or = items(&rows[1]);
        assert!(matches!(&or[0].shape, Shape::Named { name, inner: Inner::Parts(_), .. } if name == "Needs you"));
        let gap = items(&or[1]);
        assert_eq!(gap[1].path, [1, 1, 1]);
        assert_eq!(rule(&gap[1]).values, ["high", "critical"]);
    }

    #[test]
    fn a_cycle_is_a_refused_part() {
        let mut m = seven();
        m.insert(slug("A"), "saved B and ready".into());
        m.insert(slug("B"), "kind gap or saved A".into());
        m.insert(slug("Me"), "saved Me".into());
        let look = lookup_in(&m);
        let a = resolve("A", &look);
        let b = &items(inner(&a))[0];
        let back = &items(inner(b))[1];
        match &back.shape {
            Shape::Named { name, inner: Inner::Cycle(chain), .. } => {
                assert_eq!(name, "A");
                assert_eq!(chain, "\u{201c}A\u{201d} names \u{201c}B\u{201d} names \u{201c}A\u{201d}");
            }
            s => panic!("{s:?}"),
        }
        // A name that names itself.
        assert!(matches!(&items(inner(&resolve("Me", &look)))[0].shape, Shape::Named { inner: Inner::Cycle(_), .. }));
        // Offered as a rule: B in A reaches A.
        assert!(names_itself("A", "B", &look));
        assert!(names_itself("A", "a", &look), "itself, by slug");
        assert!(!names_itself("Plexus-Attention", "Deployment", &look));
        assert!(names_itself("Plexus-Ready", "Plexus-Attention", &look), "Plexus-Attention names Plexus-Ready");
    }

    #[test]
    fn a_missing_name_and_an_unread_one() {
        let m = seven();
        let look = lookup_in(&m);
        let mut f = of_text("saved Nope or saved \"Needs you\"").unwrap().0;
        let p = resolve_node("\u{2302}", &f, &look);
        assert!(matches!(&items(&p)[0].shape, Shape::Named { inner: Inner::Missing, .. }));
        // Not read yet: waited on, and asked for.
        let none = |_: &str| Got::Unknown;
        let p = resolve("Plexus-Attention", &none);
        assert_eq!(unknown(&p), ["Plexus-Attention"]);
        let half = |n: &str| if slug(n) == "plexus-attention" { look(n) } else { Got::Unknown };
        assert_eq!(unknown(&resolve("Plexus-Attention", &half)), ["Plexus-Ready", "Needs you"]);
        // A text that does not read.
        let bad = |_: &str| Got::Bad("`frobnicate` is not a field".into());
        assert!(matches!(&resolve("X", &bad).shape, Shape::Named { inner: Inner::Bad(_), .. }));
        assert!(remove(&mut f, &[0]));
        assert_eq!(to_filter(&f, &Display::default()).format(), "saved \"Needs you\"");
    }

    #[test]
    fn names_resolve_three_deep() {
        let mut m = seven();
        m.insert(slug("Top"), "saved Mid and not blocked".into());
        m.insert(slug("Mid"), "saved Low or kind spike".into());
        m.insert(slug("Low"), "saved Plexus-Attention and severity High".into());
        let look = lookup_in(&m);
        let p = resolve("Top", &look);
        assert!(unknown(&p).is_empty());
        let mid = &items(inner(&p))[0];
        let low = &items(inner(mid))[0];
        let pa = &items(inner(low))[0];
        let ready = &items(inner(pa))[0];
        assert!(matches!(&ready.shape, Shape::Named { name, inner: Inner::Parts(_), .. } if name == "Plexus-Ready"));
        assert_eq!(inner(ready).owner, "Plexus-Ready");
        // Past SAVED_DEPTH: refused, never a hang.
        let deep: HashMap<String, String> = (0..12).map(|i| (slug(&format!("D{i}")), format!("saved D{} and ready", i + 1))).collect();
        let look = lookup_in(&deep);
        let mut at = resolve("D0", &look);
        let mut depth = 0;
        loop {
            match &at.shape {
                Shape::Named { inner: Inner::Parts(p), .. } => {
                    at = items(p)[0].clone();
                    depth += 1;
                }
                Shape::Named { inner: Inner::TooDeep, .. } => break,
                s => panic!("{s:?}"),
            }
        }
        assert_eq!(depth, SAVED_DEPTH);
    }

    fn canon(t: &str) -> String {
        Filter::parse(t).unwrap().format()
    }

    #[test]
    fn the_edits() {
        let t = "saved \"Plexus-Ready\" and (saved \"Needs you\" or (kind gap and severity in (High, Critical)))";
        let (mut n, d) = of_text(t).unwrap();
        // Flip the or-group.
        assert!(flip(&mut n, &[1]));
        assert_eq!(to_filter(&n, &d).format(), "saved Plexus-Ready and saved \"Needs you\" and kind gap and severity in (high, critical)");
        assert!(flip(&mut n, &[1]));
        // Negate a list: any of ⇄ none of.
        assert!(negate(&mut n, &[1, 1, 1]));
        assert_eq!(to_filter(&n, &d).format(), canon("saved Plexus-Ready and (saved \"Needs you\" or (kind gap and not severity in (high, critical)))"));
        assert!(negate(&mut n, &[1, 1, 1]));
        // Negate a clause.
        assert!(negate(&mut n, &[1, 1, 0]));
        assert!(to_filter(&n, &d).format().contains("not kind gap"));
        assert!(negate(&mut n, &[1, 1, 0]));
        assert_eq!(to_filter(&n, &d).format(), Filter::parse(t).unwrap().format());
        // Add a rule to the or-group, then take it out.
        let at = add(&mut n, &[1], Node::Leaf(Predicate::is(Field::Blocked))).unwrap();
        assert_eq!(at, [1, 2]);
        assert!(to_filter(&n, &d).format().contains("or blocked"));
        assert!(remove(&mut n, &at));
        // A new inner group: a group of one draws as a group, writes as its rule.
        let g = add(&mut n, &[], Node::Group { conj: Conj::Any, not: false, items: vec![Node::Leaf(Predicate::is(Field::Asking))] }).unwrap();
        assert_eq!(get(&n, &g).map(|x| x.items().len()), Some(1));
        assert!(to_filter(&n, &d).format().ends_with("and asking"));
        // An emptied inner group goes with its last rule.
        assert!(remove(&mut n, &[g[0], 0]));
        assert_eq!(n.items().len(), 2);
        // Removing the gap group's two rules empties it, and it goes.
        assert!(remove(&mut n, &[1, 1, 0]));
        assert!(remove(&mut n, &[1, 1, 0]));
        assert_eq!(to_filter(&n, &d).format(), "saved Plexus-Ready and saved \"Needs you\"");
        // Replace and move.
        assert!(replace(&mut n, &[1], Node::Leaf(Predicate::is(Field::Ready))));
        assert!(move_item(&mut n, &[], 1, 0));
        assert_eq!(to_filter(&n, &d).format(), "ready and saved Plexus-Ready");
        // A group named: its place becomes the name.
        let (mut n, d) = of_text(t).unwrap();
        let g = get(&n, &[1, 1]).cloned().unwrap();
        assert_eq!(to_filter(&g, &d).format(), "kind gap and severity in (high, critical)");
        assert!(replace(&mut n, &[1, 1], Node::Leaf(Predicate::is(Field::Saved("Urgent gaps".into())))));
        assert_eq!(to_filter(&n, &d).format(), "saved Plexus-Ready and (saved \"Needs you\" or saved \"Urgent gaps\")");
    }

    #[test]
    fn drafts_are_inlined_for_counting() {
        let (n, d) = of_text("saved A and not saved B").unwrap();
        let drafted = |name: &str| match slug(name).as_str() {
            "b" => Some(of_text("kind gap or saved A").unwrap().0),
            "a" => None,
            _ => None,
        };
        assert_eq!(to_filter(&inlined(&n, &drafted), &d).format(), "saved A and not (kind gap or saved A)");
        // A draft that names itself stays a name.
        let me = |name: &str| (slug(name) == "b").then(|| of_text("saved B or ready").unwrap().0);
        assert_eq!(to_filter(&inlined(&n, &me), &d).format(), "saved A and not (saved B or ready)");
    }
}
