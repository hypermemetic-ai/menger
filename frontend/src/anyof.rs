//! FILTER-37: the query tree — a level's filter as the bar's builder draws
//! and edits it: a root block and blocks within, each one join (`any of` ⇄
//! `all of`) of its items, an item a clause, a saved query by name, or a
//! block. FILTER-36's language holds every such tree (groups nest, `saved
//! "Name"` is a predicate); this is the app's side of it, every edit a pure
//! function here, the DOM (src/filter.rs) thin.
//!
//! - **A tree** ([`Block`], [`Node`]): the root is the filter's top level —
//!   `all of` its top-level groups, or, the filter one `or` group, `any of`
//!   its items ([`from_filter`]). A clause is a [`Member`]: a category's
//!   clause (one field's values under one operator, merged where the
//!   block's join is the clause's own: `status Ready or status Coding` in
//!   an any-of block is one clause, Status is any of Ready, Coding), a time
//!   clause (FILTER-27), or what no category draws (`under X`, `category in
//!   (…)`, a negated group), kept as its text. A saved query is
//!   [`Node::Saved`]; a nested group a [`Node::Block`].
//! - **The tree to the filter** ([`to_groups`]): each block one group of
//!   its items under its join, read back through the language so the text
//!   is canonical (FILTER-36: a block joined as its parent is spliced into
//!   it; one item is that item). An empty block is nothing — a block of one
//!   item or none is a draft the bar keeps while it is worked on
//!   ([`tree_for`]: the tree last edited stands while the filter is still
//!   what it says; otherwise the filter's own tree), so `+ group`, an any-of
//!   root of one item, a block emptied by its last × keep their shape.
//! - **The edits**: [`flip`] a block's join (the root's too); [`add_group`]
//!   (a new block, the other join, so the nesting shows); [`add_saved`];
//!   [`remove`] an item (a block with all it holds); [`inline`] a saved
//!   query — its text parsed in where it stood; a category's clause set in
//!   a block ([`set_cat`]: the category menu's edits, `+ clause`, a pill's
//!   values; [`clause_in`] is what the menu checks); [`flip_member`] a
//!   clause's operator; [`put_time`], [`time_at`] and [`negate`] for a time
//!   clause where it is ([`Slot`]). Nothing is refused for its shape any
//!   more — a group inside a group is the language's (FILTER-36) — only what
//!   the language itself refuses (`text` inside a block), in its words.
//! - **Drawn** ([`sketch`]): the tree as lines — a block's join and its
//!   buttons ([`buttons`]: the joiner, `+ clause`, `+ group`, `+ saved`, ×
//!   but on the root), a clause's words, a saved query's name — what the
//!   bar draws, from one walk (native tests read it as [`sketch`]). And "as
//!   evaluated" ([`evaluated`]): the filter with its names expanded by the
//!   texts the app has fetched.
//!
//! FILTER-31's members of a top-level group ([`members`], [`Member`]) stay
//! for where a filter is shown in words (src/lens.rs's `Kept::pills`).

use menger_tickets_api::filter::{Conj, Field, Filter, Group, Predicate};

use crate::filter::{op_label, Cat, Clause, Op};
use crate::lens::{group_of, token_of};
use crate::when::{self, Timed};

/// One pill of a group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Member {
    /// A category's clause.
    Clause { cat: Cat, clause: Clause },
    /// A time clause (FILTER-27).
    Time(Timed),
    /// Predicates no category draws — one, or one field's values listed —
    /// as their own group: shown as its text.
    Custom(Group),
}

/// A join as the bar says it.
pub(crate) fn conj_words(c: Conj) -> &'static str {
    match c {
        Conj::Any => "any of",
        Conj::All => "all of",
    }
}

/// The word between a group's members.
pub(crate) fn joiner(c: Conj) -> &'static str {
    match c {
        Conj::Any => "or",
        Conj::All => "and",
    }
}

pub(crate) fn other(c: Conj) -> Conj {
    match c {
        Conj::Any => Conj::All,
        Conj::All => Conj::Any,
    }
}

impl Member {
    /// The member alone, as a top-level group — what it is outside a group.
    pub(crate) fn group(&self) -> Option<Group> {
        match self {
            Member::Clause { cat, clause } => group_of(*cat, clause),
            Member::Time(t) => Some(when::group(t)),
            Member::Custom(g) => Some(g.clone()),
        }
    }

    /// Whether it can be one of a group joined by `conj` without a group
    /// of its own: one predicate, or itself joined so.
    #[cfg(test)]
    pub(crate) fn fits(&self, conj: Conj) -> bool {
        self.group().is_some_and(|g| g.preds.len() <= 1 || g.conj == conj)
    }

    /// The category drawing it (When for a time clause; none for custom).
    pub(crate) fn cat(&self) -> Option<Cat> {
        match self {
            Member::Clause { cat, .. } => Some(*cat),
            Member::Time(t) => Some(crate::filter::time_cat(t)),
            Member::Custom(_) => None,
        }
    }

    /// The member in words: `Status is any of Ready, Coding`, "created
    /// today", a custom group's text.
    pub(crate) fn words(&self) -> String {
        match self {
            Member::Clause { cat, clause } => {
                let values = clause.values.iter().map(|v| cat.value_label(v)).collect::<Vec<_>>().join(", ");
                format!("{} {} {values}", cat.label(), op_label(*cat, clause.op, clause.values.len()))
            }
            Member::Time(t) => when::words(t, crate::calendar::reader_h12()),
            Member::Custom(g) => g.format(),
        }
    }
}

/// The member a predicate is, alone.
fn member_of(p: &Predicate) -> Member {
    match (&p.field, token_of(&p.field)) {
        (Field::History(h), _) => Member::Time(Timed { not: p.not, h: h.clone() }),
        (_, Some((cat, None))) => {
            Member::Clause { cat, clause: Clause { op: Op::Is, values: vec![if p.not { "no" } else { "yes" }.to_string()] } }
        }
        (_, Some((cat, Some(v)))) => match Clause::new(cat, if p.not { Op::Not } else { Op::Is }, [v]) {
            Some(clause) => Member::Clause { cat, clause },
            None => Member::Custom(Group::one(p.clone())),
        },
        (_, None) => Member::Custom(Group::one(p.clone())),
    }
}

/// The join under which a predicate's values list: `or` for inclusions,
/// `and` for exclusions.
fn listing(not: bool) -> Conj {
    if not {
        Conj::All
    } else {
        Conj::Any
    }
}

/// `p` taken into `m` when they are one field's values under one operator
/// and the group's join is theirs — whether it was.
fn merge(m: &mut Member, p: &Predicate, conj: Conj) -> bool {
    if listing(p.not) != conj {
        return false;
    }
    match (m, member_of(p)) {
        (Member::Clause { cat, clause }, Member::Clause { cat: c, clause: one })
            if *cat == c && !c.is_flag() && clause.op == one.op =>
        {
            match Clause::new(c, clause.op, clause.values.iter().cloned().chain(one.values)) {
                Some(cl) => {
                    *clause = cl;
                    true
                }
                None => false,
            }
        }
        (Member::Custom(g), Member::Custom(_))
            if matches!(p.field, Field::Category(_) | Field::Under(_))
                && g.preds.iter().all(|q| q.field.name() == p.field.name() && q.not == p.not) =>
        {
            let mut preds = g.preds.clone();
            preds.push(p.clone());
            *g = if p.not { Group::all(preds) } else { Group::any(preds) };
            true
        }
        _ => false,
    }
}

/// The group's members, in the order they first appear.
pub(crate) fn members(g: &Group) -> Vec<Member> {
    let conj = if g.preds.len() > 1 { g.conj } else { listing(g.preds.first().is_some_and(|p| p.not)) };
    let mut out: Vec<Member> = Vec::new();
    for p in &g.preds {
        if !out.iter_mut().any(|m| merge(m, p, conj)) {
            out.push(member_of(p));
        }
    }
    out
}

/// The group the members make, joined by `conj` — one member is its own
/// group; none, no group. Refused when a member does not fit the join
/// (FILTER-31's two levels; the pills' round trip holds it).
#[cfg(test)]
pub(crate) fn build(conj: Conj, ms: &[Member]) -> Result<Option<Group>, String> {
    match ms {
        [] => Ok(None),
        [m] => Ok(m.group()),
        ms => {
            if let Some(m) = ms.iter().find(|m| !m.fits(conj)) {
                return Err(misfit(m, conj));
            }
            let preds = ms.iter().filter_map(Member::group).flat_map(|g| g.preds).collect();
            Ok(Some(Group { conj, preds }))
        }
    }
}

/// Why `m` cannot be one of a group joined by `conj`.
#[cfg(test)]
fn misfit(m: &Member, conj: Conj) -> String {
    format!(
        "\u{201c}{}\u{201d} is itself {} its values, so it can't sit in {} group — groups do not nest; keep one value, or leave it outside the group",
        m.words(),
        conj_words(other(conj)),
        if conj == Conj::Any { "an any-of" } else { "an all-of" },
    )
}


/// Whether a group is two members or more — FILTER-31's box, in words.
pub(crate) fn is_group(g: &Group) -> bool {
    members(g).len() > 1
}

// ---------------------------------------------------------------------
// The tree

/// Where a block is: the item indices from the root down — the root is
/// the empty path. `Copy`, at most [`Path::DEPTH`] deep.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Path {
    len: u8,
    at: [u8; Path::DEPTH],
}

impl Path {
    /// How deep a block may sit (the language nests deeper; the bar's
    /// buttons stop here, and the text mode goes on).
    pub(crate) const DEPTH: usize = 8;
    /// The root block.
    pub(crate) const ROOT: Path = Path { len: 0, at: [0; Path::DEPTH] };

    /// The path of item `i` of this block (`None`: too deep).
    pub(crate) fn child(self, i: usize) -> Option<Path> {
        let n = self.len as usize;
        if n >= Self::DEPTH || i > u8::MAX as usize {
            return None;
        }
        let mut p = self;
        p.at[n] = i as u8;
        p.len += 1;
        Some(p)
    }

    /// The block holding this one, and its place there (`None`: the root).
    pub(crate) fn split(self) -> Option<(Path, usize)> {
        let n = self.len as usize;
        (n > 0).then(|| {
            let mut p = self;
            p.len -= 1;
            p.at[n - 1] = 0;
            (p, self.at[n - 1] as usize)
        })
    }

    pub(crate) fn is_root(self) -> bool {
        self.len == 0
    }

    #[cfg(test)]
    pub(crate) fn depth(self) -> usize {
        self.len as usize
    }

    /// The indices, root first.
    pub(crate) fn steps(&self) -> &[u8] {
        &self.at[..self.len as usize]
    }

    /// As the DOM says it: `r`, `r.0.2`.
    pub(crate) fn key(self) -> String {
        std::iter::once("r".to_string()).chain(self.steps().iter().map(u8::to_string)).collect::<Vec<_>>().join(".")
    }
}

/// Where a clause is: its block, and its place among the block's items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slot {
    pub(crate) block: Path,
    pub(crate) item: usize,
}

/// An item of a block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    /// A clause: a category's, a time clause, or one no category draws.
    Leaf(Member),
    /// FILTER-36: a saved query, by name — `not saved "Name"` when `not`.
    Saved { name: String, not: bool },
    /// A group within: a block of its own.
    Block(Block),
}

/// One join of items — the root, or a group within.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) conj: Conj,
    pub(crate) items: Vec<Node>,
}

impl Default for Block {
    /// The empty filter: an all-of root with nothing in it.
    fn default() -> Self {
        Block { conj: Conj::All, items: Vec::new() }
    }
}

/// The node a group is as one item: its one item, or a block of them.
fn item_of(g: &Group) -> Node {
    let mut ns = nodes(g);
    match ns.len() {
        1 => ns.pop().expect("one"),
        _ => Node::Block(Block { conj: g.conj, items: ns }),
    }
}

/// A group's items, in the order they first appear: a category's values
/// under the group's join merged into one clause, a nested group (not
/// negated) a block, a name a saved node; a negated group is a clause of
/// its own, shown as its text.
fn nodes(g: &Group) -> Vec<Node> {
    let conj = if g.preds.len() > 1 { g.conj } else { listing(g.preds.first().is_some_and(|p| p.not)) };
    let mut out: Vec<Node> = Vec::new();
    for p in &g.preds {
        match &p.field {
            Field::Group(inner) if !p.not => out.push(item_of(inner)),
            Field::Saved(name) => out.push(Node::Saved { name: name.clone(), not: p.not }),
            _ => {
                let merged = out.iter_mut().any(|n| match n {
                    Node::Leaf(m) => merge(m, p, conj),
                    _ => false,
                });
                if !merged {
                    out.push(Node::Leaf(member_of(p)));
                }
            }
        }
    }
    out
}

/// The filter's tree: the filter one `or` group of two items or more is
/// an any-of root of them; else an all-of root of its top-level groups.
pub(crate) fn from_filter(f: &Filter) -> Block {
    if let [g] = f.groups.as_slice() {
        if g.conj == Conj::Any && g.preds.len() > 1 {
            let items = nodes(g);
            if items.len() > 1 {
                return Block { conj: Conj::Any, items };
            }
        }
    }
    Block { conj: Conj::All, items: f.groups.iter().map(item_of).collect() }
}

/// An item as one predicate — `None` for an empty block.
fn pred_of(n: &Node) -> Option<Predicate> {
    let one = |g: Group| match <[Predicate; 1]>::try_from(g.preds) {
        Ok([p]) => p,
        Err(preds) => Group { conj: g.conj, preds }.nested(false),
    };
    match n {
        Node::Leaf(m) => m.group().map(one),
        Node::Saved { name, not } => Some(Predicate { field: Field::Saved(name.clone()), not: *not }),
        Node::Block(b) => block_group(b).map(one),
    }
}

/// A block as one group — `None` while it holds nothing.
fn block_group(b: &Block) -> Option<Group> {
    let preds: Vec<Predicate> = b.items.iter().filter_map(pred_of).collect();
    (!preds.is_empty()).then(|| Group::of(b.conj, preds))
}

/// The tree's groups, canonical — read back through the language, so what
/// it refuses (`text` inside a block) is refused here, in its words.
pub(crate) fn to_groups(root: &Block) -> Result<Vec<Group>, String> {
    let filled: Vec<&Node> = root.items.iter().filter(|n| pred_of(n).is_some()).collect();
    let groups: Vec<Group> = match (root.conj, filled.len()) {
        (Conj::Any, n) if n > 1 => block_group(root).into_iter().collect(),
        _ => filled
            .into_iter()
            .filter_map(|n| match n {
                Node::Leaf(m) => m.group(),
                Node::Saved { .. } => pred_of(n).map(Group::one),
                Node::Block(b) => block_group(b),
            })
            .collect(),
    };
    let text = Filter { groups, ..Filter::default() }.format();
    Filter::parse(&text).map(|f| f.groups).map_err(|e| e.to_string())
}

/// The tree the bar draws for `f`: `kept` — the tree last edited — while
/// it still says `f`, else `f`'s own.
pub(crate) fn tree_for(kept: Option<&Block>, f: &Filter) -> Block {
    match kept {
        Some(t) if to_groups(t).is_ok_and(|g| g == f.groups) => t.clone(),
        _ => from_filter(f),
    }
}

/// The block at `p`.
pub(crate) fn block_at(root: &Block, p: Path) -> Option<&Block> {
    let mut b = root;
    for &i in p.steps() {
        match b.items.get(i as usize)? {
            Node::Block(inner) => b = inner,
            _ => return None,
        }
    }
    Some(b)
}

fn block_mut(root: &mut Block, p: Path) -> Result<&mut Block, String> {
    let mut b = root;
    for &i in p.steps() {
        match b.items.get_mut(i as usize) {
            Some(Node::Block(inner)) => b = inner,
            _ => return Err("that group is gone".into()),
        }
    }
    Ok(b)
}

// ---------------------------------------------------------------------
// The edits

/// The block's join flipped: any of ⇄ all of — the root's too.
pub(crate) fn flip(root: &mut Block, p: Path) -> Result<(), String> {
    let b = block_mut(root, p)?;
    b.conj = other(b.conj);
    Ok(())
}

/// `+ group`: a new block at the end of the block at `p`, under the other
/// join — so the nesting shows — and empty until a clause is added to it;
/// its path.
pub(crate) fn add_group(root: &mut Block, p: Path) -> Result<Path, String> {
    let n = block_at(root, p).ok_or("that group is gone")?.items.len();
    let at = p.child(n).ok_or_else(|| format!("the tree's buttons nest groups {} deep — deeper is the text mode's", Path::DEPTH))?;
    let b = block_mut(root, p)?;
    b.items.push(Node::Block(Block { conj: other(b.conj), items: Vec::new() }));
    Ok(at)
}

/// `+ saved`: the saved query `name` added to the block at `p`.
pub(crate) fn add_saved(root: &mut Block, p: Path, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a saved query needs a name".into());
    }
    block_mut(root, p)?.items.push(Node::Saved { name: name.to_string(), not: false });
    Ok(())
}

/// ×: item `i` of the block at `p` taken out — a block with all it holds.
/// The block at `p` stays, empty or not, while the bar works on it.
pub(crate) fn remove(root: &mut Block, p: Path, i: usize) -> Result<(), String> {
    let b = block_mut(root, p)?;
    if i < b.items.len() {
        b.items.remove(i);
    }
    Ok(())
}

/// `inline`: the saved query at item `i` of the block at `p` replaced by
/// `text`, what it holds, parsed in — spliced into the block when it is
/// one item or joined as the block is, else a block of its own. Its own
/// names stay names (one level is inlined). A negated name is refused (no
/// block is negated in the tree; `not (…)` is the text mode's).
pub(crate) fn inline(root: &mut Block, p: Path, i: usize, text: &str) -> Result<(), String> {
    let b = block_mut(root, p)?;
    let Some(Node::Saved { name, not }) = b.items.get(i) else { return Err("that saved query is gone".into()) };
    if *not {
        return Err(format!("\u{201c}not saved {name}\u{201d} is inlined in the text mode — the tree draws no negated group"));
    }
    let f = Filter::parse(text).map_err(|e| format!("\u{201c}{name}\u{201d} does not read — {e}"))?;
    if f.groups.is_empty() {
        return Err(format!("\u{201c}{name}\u{201d} holds no clause"));
    }
    let sub = from_filter(&f);
    if sub.items.len() <= 1 || sub.conj == b.conj {
        b.items.splice(i..=i, sub.items);
    } else {
        b.items[i] = Node::Block(sub);
    }
    Ok(())
}

/// The item a category's edit takes in the block: `item` when it is that
/// category's clause, else the block's first clause of it.
fn cat_item(b: &Block, item: Option<usize>, c: Cat) -> Option<usize> {
    let is = |n: &Node| matches!(n, Node::Leaf(Member::Clause { cat, .. }) if *cat == c);
    item.filter(|i| b.items.get(*i).is_some_and(is)).or_else(|| b.items.iter().position(is))
}

/// `c`'s clause in the block at `p` (`item`'s, when it is one of `c`) —
/// what the menu checks while it adds there.
pub(crate) fn clause_in(root: &Block, p: Path, item: Option<usize>, c: Cat) -> Option<Clause> {
    let b = block_at(root, p)?;
    match b.items.get(cat_item(b, item, c)?)? {
        Node::Leaf(Member::Clause { clause, .. }) => Some(clause.clone()),
        _ => None,
    }
}

/// `c`'s clause in the block at `p` set: its clause replaced, added as one
/// more item (last), or — none — taken out.
pub(crate) fn set_cat(root: &mut Block, p: Path, item: Option<usize>, c: Cat, cl: Option<Clause>) -> Result<(), String> {
    let b = block_mut(root, p)?;
    match (cat_item(b, item, c), cl) {
        (Some(i), Some(clause)) => b.items[i] = Node::Leaf(Member::Clause { cat: c, clause }),
        (Some(i), None) => {
            b.items.remove(i);
        }
        (None, Some(clause)) => b.items.push(Node::Leaf(Member::Clause { cat: c, clause })),
        (None, None) => {}
    }
    Ok(())
}

/// A clause's operator turned (a flag's value), as its pill's toggle turns
/// it.
pub(crate) fn flip_member(root: &mut Block, s: Slot) -> Result<(), String> {
    let b = block_mut(root, s.block)?;
    let Some(Node::Leaf(Member::Clause { cat, clause })) = b.items.get(s.item).cloned() else { return Ok(()) };
    let op = if cat.is_flag() || clause.op == Op::Is { Op::Not } else { Op::Is };
    if let Some(clause) = Clause::new(cat, op, clause.values) {
        b.items[s.item] = Node::Leaf(Member::Clause { cat, clause });
    }
    Ok(())
}

/// The time clause at `s`.
pub(crate) fn time_at(root: &Block, s: Slot) -> Option<Timed> {
    match block_at(root, s.block)?.items.get(s.item)? {
        Node::Leaf(Member::Time(t)) => Some(t.clone()),
        _ => None,
    }
}

/// `t` written at `slot`, or — none — added last to the block at `into`;
/// where it now is.
pub(crate) fn put_time(root: &mut Block, into: Path, slot: Option<Slot>, t: &Timed) -> Result<Slot, String> {
    let at = slot.map_or(into, |s| s.block);
    let b = block_mut(root, at)?;
    let item = match slot {
        Some(s) if s.item < b.items.len() => {
            b.items[s.item] = Node::Leaf(Member::Time(t.clone()));
            s.item
        }
        _ => {
            b.items.push(Node::Leaf(Member::Time(t.clone())));
            b.items.len() - 1
        }
    };
    Ok(Slot { block: at, item })
}

/// FILTER-30: the time clause at `s` turned around (`created today` ⇄ `not
/// created today`), where it is.
pub(crate) fn negate(root: &mut Block, s: Slot) -> Result<(), String> {
    let t = time_at(root, s).ok_or("only a time clause is negated here — a category's pill turns with its operator")?;
    put_time(root, s.block, Some(s), &when::negate(&t)).map(|_| ())
}

/// Every saved name the tree holds, at any depth, each once (by slug).
pub(crate) fn saved_names(root: &Block) -> Vec<String> {
    fn walk(b: &Block, out: &mut Vec<String>) {
        for n in &b.items {
            match n {
                Node::Saved { name, .. } => {
                    let s = crate::saved::slug(name);
                    if !out.iter().any(|o| crate::saved::slug(o) == s) {
                        out.push(name.clone());
                    }
                }
                Node::Block(inner) => walk(inner, out),
                Node::Leaf(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

/// Whether `c` draws a clause in a block below the root — the menu marks
/// it.
pub(crate) fn nested_cat(root: &Block, c: Cat) -> bool {
    fn walk(b: &Block, c: Cat, deep: bool) -> bool {
        b.items.iter().any(|n| match n {
            Node::Leaf(m) => deep && m.cat() == Some(c),
            Node::Block(inner) => walk(inner, c, true),
            Node::Saved { .. } => false,
        })
    }
    walk(root, c, false)
}

// ---------------------------------------------------------------------
// In words, and drawn

/// An item in words: a clause's, `saved “Name”`, a block's in parentheses.
pub(crate) fn node_words(n: &Node) -> String {
    match n {
        Node::Leaf(m) => m.words(),
        Node::Saved { name, not } => format!("{}saved \u{201c}{name}\u{201d}", if *not { "not " } else { "" }),
        Node::Block(b) if b.items.is_empty() => format!("(an empty {} group)", conj_words(b.conj)),
        Node::Block(b) => format!("({})", block_words(b)),
    }
}

/// A block's items in words, joined by its joiner.
pub(crate) fn block_words(b: &Block) -> String {
    b.items.iter().map(node_words).collect::<Vec<_>>().join(&format!(" {} ", joiner(b.conj)))
}

/// What the menu says while it adds to the block at `p` below the root.
pub(crate) fn into_words(root: &Block, p: Path) -> Option<String> {
    let b = block_at(root, p).filter(|_| !p.is_root())?;
    Some(match b.items.is_empty() {
        true => format!("Adding to a new {} group", conj_words(b.conj)),
        false => format!("Adding to {}: {}", conj_words(b.conj), block_words(b)),
    })
}

/// A block's buttons, in the order drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Button {
    /// The join, flipping it.
    Joiner,
    Clause,
    Group,
    Saved,
    /// The block taken out — never the root's.
    Remove,
}

/// The buttons a block draws: every one, × but on the root; none at all
/// read-only (the global root on a surface's bar).
pub(crate) fn buttons(p: Path, read_only: bool) -> Vec<Button> {
    match read_only {
        true => Vec::new(),
        false => [Button::Joiner, Button::Clause, Button::Group, Button::Saved]
            .into_iter()
            .chain((!p.is_root()).then_some(Button::Remove))
            .collect(),
    }
}

/// A line of the drawn tree, `depth` its indent.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Line {
    Block { depth: usize, conj: Conj, buttons: Vec<Button> },
    Clause { depth: usize, words: String },
    Saved { depth: usize, name: String, not: bool },
}

/// The tree as the bar draws it, line by line — the native tests' view of
/// what src/filter.rs's `block_view` draws, walked the same way, its
/// buttons from the same [`buttons`].
#[cfg(test)]
pub(crate) fn sketch(root: &Block, read_only: bool) -> Vec<Line> {
    fn walk(b: &Block, p: Path, read_only: bool, out: &mut Vec<Line>) {
        out.push(Line::Block { depth: p.depth(), conj: b.conj, buttons: buttons(p, read_only) });
        for (i, n) in b.items.iter().enumerate() {
            let depth = p.depth() + 1;
            match n {
                Node::Leaf(m) => out.push(Line::Clause { depth, words: m.words() }),
                Node::Saved { name, not } => out.push(Line::Saved { depth, name: name.clone(), not: *not }),
                Node::Block(inner) => match p.child(i) {
                    Some(q) => walk(inner, q, read_only, out),
                    None => out.push(Line::Clause { depth, words: node_words(n) }),
                },
            }
        }
    }
    let mut out = Vec::new();
    walk(root, Path::ROOT, read_only, &mut out);
    out
}

/// FILTER-37's "as evaluated": the filter with every name expanded by the
/// texts known (`texts`: `Some(text)` a name's saved text, `Some(None)` no
/// saved filter holds it, `None` not fetched yet) — or the names still to
/// fetch, each once, while one is unknown. What the language refuses (a
/// cycle, a name nothing holds) is the refusal, in its words.
pub(crate) fn evaluated(f: &Filter, texts: &dyn Fn(&str) -> Option<Option<String>>) -> Result<Result<String, String>, Vec<String>> {
    let mut missing: Vec<String> = Vec::new();
    let mut lookup = |name: &str| match texts(name) {
        Some(t) => t,
        None => {
            if !missing.iter().any(|m| crate::saved::slug(m) == crate::saved::slug(name)) {
                missing.push(name.to_string());
            }
            // Something that reads, so the walk finds the rest.
            Some("ready".to_string())
        }
    };
    let out = f.resolve(&mut lookup);
    if !missing.is_empty() {
        return Err(missing);
    }
    Ok(out.map(|r| crate::counts::wire_text(&r)).map_err(|e| e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Filter {
        Filter::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    fn text(t: &Block) -> String {
        Filter { groups: to_groups(t).unwrap(), ..Filter::default() }.format()
    }

    fn tree(s: &str) -> Block {
        from_filter(&parsed(s))
    }

    fn cl(op: Op, v: &[&str]) -> Clause {
        Clause { op, values: v.iter().map(|s| s.to_string()).collect() }
    }

    fn leaf(c: Cat, op: Op, v: &[&str]) -> Node {
        Node::Leaf(Member::Clause { cat: c, clause: cl(op, v) })
    }

    const R: Path = Path::ROOT;

    #[test]
    fn a_path_is_a_block_from_the_root() {
        let p = R.child(2).unwrap().child(0).unwrap();
        assert_eq!(p.key(), "r.2.0");
        assert_eq!(p.split(), Some((R.child(2).unwrap(), 0)));
        assert!(R.split().is_none() && R.is_root() && R.key() == "r");
        let mut deep = R;
        for _ in 0..Path::DEPTH {
            deep = deep.child(1).unwrap();
        }
        assert!(deep.child(0).is_none(), "no deeper than DEPTH");
    }

    /// Acceptance 1: the tree drawn — empty, one clause, and-only, an
    /// or-group, nested groups, names — from one function, every block's
    /// buttons.
    #[test]
    fn the_tree_is_drawn_from_one_function() {
        use Button::*;
        let block = |depth: usize, conj| {
            let mut b = vec![Joiner, Clause, Group, Saved];
            if depth > 0 {
                b.push(Remove);
            }
            Line::Block { depth, conj, buttons: b }
        };
        let all = |d| block(d, Conj::All);
        let any = |d| block(d, Conj::Any);
        let c = |depth, w: &str| Line::Clause { depth, words: w.into() };
        let s = |depth, n: &str| Line::Saved { depth, name: n.into(), not: false };
        // Empty: the root alone, its buttons.
        assert_eq!(sketch(&tree(""), false), vec![all(0)]);
        // One clause.
        assert_eq!(sketch(&tree("status Ready"), false), vec![all(0), c(1, "Status is Ready")]);
        // And-only.
        assert_eq!(sketch(&tree("status Ready and kind build"), false), vec![all(0), c(1, "Status is Ready"), c(1, "Kind is build")]);
        // An or-group alone is an any-of root; among others, a block.
        assert_eq!(sketch(&tree("status Ready or kind gap"), false), vec![any(0), c(1, "Status is Ready"), c(1, "Kind is gap")]);
        assert_eq!(
            sketch(&tree("(status Ready or kind gap) and in MNG"), false),
            vec![all(0), any(1), c(2, "Status is Ready"), c(2, "Kind is gap"), c(1, "Shortcode is MNG")]
        );
        // Nested groups, and names.
        assert_eq!(
            sketch(&tree("(saved \"Needs you\" or (kind build and blocked)) and saved Deployment"), false),
            vec![all(0), any(1), s(2, "Needs you"), all(2), c(3, "Kind is build"), c(3, "Blocked is yes"), s(1, "Deployment")]
        );
        assert_eq!(sketch(&tree("saved \"Needs you\" or saved Plexus-Ready"), false), vec![any(0), s(1, "Needs you"), s(1, "Plexus-Ready")]);
        // Read only (the global root on a surface): no button anywhere.
        let ro = sketch(&tree("(ready or blocked) and asking"), true);
        assert_eq!(ro.len(), 5);
        assert!(ro.iter().all(|l| !matches!(l, Line::Block { buttons, .. } if !buttons.is_empty())));
    }

    #[test]
    fn a_clause_merges_its_values_where_the_join_is_theirs() {
        assert_eq!(tree("status in (Ready, Coding)").items, vec![leaf(Cat::Status, Op::Is, &["Coding", "Ready"])]);
        assert_eq!(tree("(status Ready or status Coding or kind gap)").items[0], leaf(Cat::Status, Op::Is, &["Coding", "Ready"]));
        assert_eq!(tree("not kind in (doc, spike)").items, vec![leaf(Cat::Kind, Op::Not, &["spike", "doc"])]);
        // In an all-of block, `status in (A, B)` is still one clause.
        let t = tree("(status in (Ready, Coding) and kind gap) or blocked");
        let Node::Block(b) = &t.items[0] else { panic!("{t:?}") };
        assert_eq!(b.items[0], leaf(Cat::Status, Op::Is, &["Coding", "Ready"]));
        // A negated group is a clause shown as its text.
        let t = tree("not (kind build and blocked) and ready");
        assert!(matches!(&t.items[0], Node::Leaf(Member::Custom(_))), "{t:?}");
    }

    /// Every tree the language holds comes back as its text.
    #[test]
    fn every_shape_round_trips_through_the_tree() {
        for form in [
            "",
            "status Ready",
            "status Ready and kind build",
            "(status Ready or kind gap)",
            "(status Ready or kind gap) and in MNG",
            "(status Ready and kind gap)",
            "(changed today or created today) and kind build",
            "(not kind spike and not kind doc and not blocked)",
            "(label needs-you or asking)",
            "(status Pending for over 2w or entered Ready today by owner)",
            "saved \"Needs you\"",
            "(saved \"Needs you\" or saved Plexus-Ready)",
            "saved Deployment and saved Plexus-Coding",
            "(saved Deployment or (kind build and blocked)) and not saved Stale",
            "((changed today and kind build) or created today)",
            "(blocked or (kind build and (asking or ready)))",
            "not (kind build and blocked) and ready",
            "text \"menger\" and kind build",
            "in (MNG, WAVE) and under PLEXUS-1",
        ] {
            let f = parsed(form);
            assert_eq!(f.format(), form, "canonical");
            let t = from_filter(&f);
            assert_eq!(to_groups(&t).unwrap(), f.groups, "{form}: {t:?}");
            assert_eq!(text(&t), form);
        }
    }

    #[test]
    fn the_root_flips_and_holds_names_joined_by_or() {
        // From the empty bar, buttons only: flip the root, + saved twice.
        let mut t = tree("");
        flip(&mut t, R).unwrap();
        add_saved(&mut t, R, "Needs you").unwrap();
        // One item: the filter is the name; the root keeps its join.
        assert_eq!(text(&t), "saved \"Needs you\"");
        assert_eq!(tree_for(Some(&t), &parsed("saved \"Needs you\"")).conj, Conj::Any);
        add_saved(&mut t, R, "Plexus-Ready").unwrap();
        assert_eq!(text(&t), "(saved \"Needs you\" or saved Plexus-Ready)");
        assert_eq!(from_filter(&parsed(&text(&t))), t, "and the text draws the same tree");
        // A filter changed elsewhere draws its own tree.
        assert_eq!(tree_for(Some(&t), &parsed("kind build")), tree("kind build"));
        // The intersection: an all-of root.
        let mut t = tree("");
        add_saved(&mut t, R, "Deployment").unwrap();
        add_saved(&mut t, R, "Plexus-Coding").unwrap();
        assert_eq!(text(&t), "saved Deployment and saved Plexus-Coding");
        assert!(add_saved(&mut t, R, "  ").is_err());
        // Flipping the root of an and-filter makes it one or.
        let mut t = tree("status Ready and kind build");
        flip(&mut t, R).unwrap();
        assert_eq!(text(&t), "(status Ready or kind build)");
        flip(&mut t, R).unwrap();
        assert_eq!(text(&t), "status Ready and kind build");
    }

    #[test]
    fn a_new_group_takes_the_other_join_and_waits_for_a_clause() {
        let mut t = tree("kind build");
        let g = add_group(&mut t, R).unwrap();
        assert_eq!(g, R.child(1).unwrap());
        assert_eq!(block_at(&t, g).unwrap().conj, Conj::Any);
        // Empty, it is nothing: the filter is unchanged, and the tree stays.
        assert_eq!(text(&t), "kind build");
        assert_eq!(tree_for(Some(&t), &parsed("kind build")), t);
        assert_eq!(into_words(&t, g).as_deref(), Some("Adding to a new any of group"));
        assert_eq!(into_words(&t, R), None);
        // + clause in it (the menu's edits): Status Ready, then Kind gap.
        set_cat(&mut t, g, None, Cat::Status, Some(cl(Op::Is, &["Ready"]))).unwrap();
        assert_eq!(text(&t), "kind build and status Ready", "one item: that item");
        set_cat(&mut t, g, None, Cat::Kind, Some(cl(Op::Is, &["gap"]))).unwrap();
        assert_eq!(text(&t), "kind build and (status Ready or kind gap)");
        assert_eq!(clause_in(&t, g, None, Cat::Kind), Some(cl(Op::Is, &["gap"])));
        assert_eq!(clause_in(&t, R, None, Cat::Kind), Some(cl(Op::Is, &["build"])), "the root's own Kind");
        assert!(nested_cat(&t, Cat::Kind) && nested_cat(&t, Cat::Status) && !nested_cat(&t, Cat::Severity));
        // A group within that group: the other join again.
        let h = add_group(&mut t, g).unwrap();
        assert_eq!(block_at(&t, h).unwrap().conj, Conj::All);
        set_cat(&mut t, h, None, Cat::Blocked, Some(cl(Op::Is, &["yes"]))).unwrap();
        set_cat(&mut t, h, None, Cat::Severity, Some(cl(Op::Is, &["high"]))).unwrap();
        assert_eq!(text(&t), "kind build and (status Ready or kind gap or (blocked and severity high))");
        // × the inner block, then the clauses: an emptied block stays a draft.
        let (parent, i) = h.split().unwrap();
        assert_eq!(parent, g);
        remove(&mut t, parent, i).unwrap();
        assert_eq!(text(&t), "kind build and (status Ready or kind gap)");
        remove(&mut t, g, 0).unwrap();
        remove(&mut t, g, 0).unwrap();
        assert_eq!(text(&t), "kind build");
        assert!(block_at(&t, g).unwrap().items.is_empty());
    }

    #[test]
    fn a_clause_is_set_and_turned_where_it_is() {
        let mut t = tree("(status Ready or kind gap) and status Coding");
        let g = R.child(0).unwrap();
        // The menu's toggle on Status inside the block: its own clause.
        set_cat(&mut t, g, None, Cat::Status, Clause::new(Cat::Status, Op::Is, ["Ready".into(), "Pending".into()])).unwrap();
        assert_eq!(text(&t), "(status Ready or status Pending or kind gap) and status Coding");
        // Opened from the second Status pill at the root: that one.
        let mut t = tree("status Ready and not status Coding");
        set_cat(&mut t, R, Some(1), Cat::Status, Some(cl(Op::Not, &["Coding", "Ready"]))).unwrap();
        assert_eq!(text(&t), "status Ready and not status in (Coding, Ready)");
        // The operator toggle — inside an any-of block, `is not` of two
        // values is a group within: no refusal any more.
        let mut t = tree("(status Ready or status Coding or kind gap)");
        flip_member(&mut t, Slot { block: R, item: 0 }).unwrap();
        assert_eq!(text(&t), "(not status in (Coding, Ready) or kind gap)");
        // `text` stays at the top: inside a block the language refuses it.
        let mut t = tree("text \"x\"");
        assert!(to_groups(&t).is_ok());
        flip(&mut t, R).unwrap();
        set_cat(&mut t, R, None, Cat::Kind, Some(cl(Op::Is, &["gap"]))).unwrap();
        assert!(to_groups(&t).unwrap_err().contains("text"), "{t:?}");
    }

    #[test]
    fn a_saved_query_inlines_its_text() {
        // Into an any-of root: an and-filter is a block of its own.
        let mut t = tree("saved \"Needs you\" or saved Plexus-Ready");
        inline(&mut t, R, 1, "in (MACRO, PLEXUS) and not status Complete and not blocked").unwrap();
        assert_eq!(text(&t), "(saved \"Needs you\" or (in (MACRO, PLEXUS) and not status Complete and not blocked))");
        // An or-filter joins the any-of root's items.
        inline(&mut t, R, 0, "queued or asking or label needs-you").unwrap();
        assert_eq!(text(&t), "(queued or asking or label needs-you or (in (MACRO, PLEXUS) and not status Complete and not blocked))");
        // Into an all-of root, an and-filter's clauses are spliced in; its names stay names.
        let mut t = tree("saved Deployment and kind build");
        inline(&mut t, R, 0, "in (DEPLOY, FILTER) and saved Plexus-Coding").unwrap();
        assert_eq!(text(&t), "in (DEPLOY, FILTER) and saved Plexus-Coding and kind build");
        // Refused: a negated name, a text that does not read; nothing changed.
        let mut t = tree("not saved Deployment");
        let before = t.clone();
        assert!(inline(&mut t, R, 0, "kind build").is_err());
        assert_eq!(t, before);
        assert!(inline(&mut tree("saved X"), R, 0, "status Nope").is_err());
        assert_eq!(saved_names(&tree("(saved A or saved B) and saved a")), vec!["A".to_string(), "B".to_string()]);
    }

    #[test]
    fn a_time_clause_is_put_and_negated_where_it_is() {
        let mut t = tree("kind build");
        let g = add_group(&mut t, R).unwrap();
        let today = when::of_group(&parsed("created today").groups[0]).unwrap();
        let s = put_time(&mut t, g, None, &today).unwrap();
        assert_eq!(s, Slot { block: g, item: 0 });
        set_cat(&mut t, g, None, Cat::Status, Some(cl(Op::Is, &["Ready"]))).unwrap();
        assert_eq!(text(&t), "kind build and (created today or status Ready)");
        assert_eq!(time_at(&t, s), Some(today.clone()));
        negate(&mut t, s).unwrap();
        assert_eq!(text(&t), "kind build and (not created today or status Ready)");
        let y = when::of_group(&parsed("created yesterday").groups[0]).unwrap();
        put_time(&mut t, R, Some(s), &y).unwrap();
        assert_eq!(text(&t), "kind build and (created yesterday or status Ready)");
        // A category's clause is not negated here.
        assert!(negate(&mut t, Slot { block: g, item: 1 }).is_err());
        assert!(negate(&mut t, Slot { block: R, item: 0 }).is_err());
    }

    #[test]
    fn as_evaluated_expands_the_names_known() {
        let f = parsed("saved \"Needs you\" or saved Plexus-Ready");
        let none = |_: &str| None;
        assert_eq!(evaluated(&f, &none), Err(vec!["Needs you".to_string(), "Plexus-Ready".to_string()]));
        let known = |n: &str| match crate::saved::slug(n).as_str() {
            "needs-you" => Some(Some("queued or asking".to_string())),
            "plexus-ready" => Some(Some("in PLEXUS and not blocked".to_string())),
            _ => Some(None),
        };
        assert_eq!(evaluated(&f, &known), Ok(Ok("(queued or asking or (in PLEXUS and not blocked))".to_string())));
        let gone = parsed("saved Nope and ready");
        assert!(evaluated(&gone, &known).unwrap().unwrap_err().contains("no saved filter named"));
    }

    // FILTER-31's members, still what a top-level group is in words.
    #[test]
    fn a_group_is_its_members() {
        let g = &parsed("(changed today or created today)").groups[0];
        let ms = members(g);
        assert_eq!(ms.len(), 2);
        assert!(matches!(&ms[0], Member::Time(_)) && matches!(&ms[1], Member::Time(_)));
        let ms = members(&parsed("(status Ready or status Coding or kind gap)").groups[0]);
        assert_eq!(
            ms,
            vec![
                Member::Clause { cat: Cat::Status, clause: cl(Op::Is, &["Coding", "Ready"]) },
                Member::Clause { cat: Cat::Kind, clause: cl(Op::Is, &["gap"]) },
            ]
        );
        let ms = members(&parsed("(blocked or not ready or label needs-you or under PLEXUS-1 or category ready or category active)").groups[0]);
        assert_eq!(ms.len(), 5, "{ms:?}");
        assert_eq!(ms[4], Member::Custom(parsed("category in (ready, active)").groups[0].clone()));
        assert!(is_group(&parsed("(status Ready or kind gap)").groups[0]));
        assert!(!is_group(&parsed("status in (Ready, Coding)").groups[0]));
    }
}
