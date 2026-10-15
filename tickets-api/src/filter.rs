//! FILTER-10: the filter language, one module for the server and the wasm
//! app (FILTER-3 §1–§2).
//!
//! The grammar is manifest's view syntax (`../manifest/app/shared/views.ts`
//! at `b1e84ab`: `parseFilter`, `parseGroup`, `parsePredicate`,
//! `formatFilter`), ported as written, with menger's fields in place of
//! manifest's:
//!
//! - **A filter** is groups joined by `and`, then optionally `·` and a
//!   display part. The empty text is the empty filter, which matches every
//!   ticket.
//! - **A group** is one predicate; `field in (a, b)` (any of them);
//!   `not field in (a, b)` (none of them); or one pair of parentheses holding
//!   items joined all by `or` (any) or all by `and` (all). As in
//!   manifest, a chunk `a or b` without parentheses is an `or` group too.
//!   FILTER-36: an item is a predicate, a list, or a group in parentheses
//!   ([`Field::Group`]), `not` before it or not — groups nest to any depth:
//!   `(a or (b and c)) and d`. One pair of parentheses still holds one join.
//! - **A predicate** is `not` (any number; they toggle) before one of
//!   `status <name>` · `category <open|ready|active|review|done|dropped|static>`
//!   · `kind <name>` · `severity <critical|high|medium|low|none>` ·
//!   `blocked` · `ready` · `has commits` · `menger doc` (FILTER-21) ·
//!   `asking` (MENGER-12: it has an open input) · `label <name>` (and
//!   `label in (a, b)`; MENGER-12) ·
//!   `in <SHORTCODE>` (and `in (A, B)`, `not in (A, B)`) ·
//!   `under <ID or SHORTCODE>` · `text "…"`, or a bare `"…"` · FILTER-36:
//!   `saved "Name"` (or `saved Name`), a saved filter by name
//!   ([`Field::Saved`], expanded by [`Filter::resolve`]). A value
//!   holding spaces, commas or parentheses is quoted with `"`; the language
//!   has no escape, so no value holds a `"`.
//! - **`text`** is tested on rows, so it stands only as a top-level `and`
//!   group of its own: never inside `or`, under `not`, or in parentheses
//!   with other predicates — nor in a nested group, nor through a name.
//! - **The display part** keeps `tree` (the default), `flat`, `columns a, b`
//!   and `order code` (the store's order).
//!
//! What menger changes in manifest's grammar is **strictness** (FILTER-3
//! §2): manifest drops a chunk it cannot read and shows "not understood";
//! here every such chunk — a refused field, a misplaced `text`, an unknown
//! name, anything the grammar cannot read — is named in one [`FilterError`],
//! and nothing is parsed.
//!
//! [`Filter::format`] is canonical: `format(parse(x)) == x` for every
//! canonical text, and `parse(format(f)) == f` for every filter in the
//! language's shape — a one-predicate group is [`Conj::All`] (what
//! [`Group::any`] and [`Group::all`] build), no group is empty, `text`
//! stands alone, no value holds a `"`, and shortcodes and ids are upper
//! case; FILTER-36: and every nested group settled as [`Group::of`] settles
//! it — a group joined as its parent spliced into it, a one-item group its
//! item, a negated list the other list, a group of one group that group.
//! [`Filter::parse`] produces only filters of that shape.
//!
//! On top of the grammar: [`Filter::and`], the lens's aggregation (a level's
//! filter under the global one), [`Filter::admits_nothing`], the
//! contradiction a combined filter can fall into, and [`Filter::matches`],
//! the reference evaluator every compiled form is held equal to — each
//! walking nested groups as it walks a group (FILTER-36). And
//! [`Filter::resolve`]: every `saved "Name"` replaced by the filter the name
//! holds, which the store does before it compiles a filter to cells.
//!
//! Wasm-safe: no feature gate, nothing but `std` and the wire enums.

use std::fmt;
use std::str::FromStr;

use crate::types::{Kind, Severity, Status};

// ---------------------------------------------------------------------
// Categories

/// A status's category — SDLC-12's closed seven (FILTER-3 §1). It never
/// changes meaning; a status maps to one through [`category_of`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Open,
    Ready,
    Active,
    Review,
    Done,
    Dropped,
    Static,
}

impl Category {
    /// The seven, in the lifecycle's order.
    pub const ALL: [Category; 7] = [
        Category::Open,
        Category::Ready,
        Category::Active,
        Category::Review,
        Category::Done,
        Category::Dropped,
        Category::Static,
    ];

    /// The name a filter writes (lowercase).
    pub const fn as_str(self) -> &'static str {
        match self {
            Category::Open => "open",
            Category::Ready => "ready",
            Category::Active => "active",
            Category::Review => "review",
            Category::Done => "done",
            Category::Dropped => "dropped",
            Category::Static => "static",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Case-insensitive; an unknown name is an error that lists the seven.
impl FromStr for Category {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Category::ALL.into_iter().find(|c| c.as_str().eq_ignore_ascii_case(s.trim())).ok_or_else(|| {
            format!(
                "unknown category `{s}` — the categories are {}",
                Category::ALL.map(Category::as_str).join(", ")
            )
        })
    }
}

/// The one table from a status to its category (FILTER-3 §1, open question
/// 3: Blocked, Idea and Epic sit in `open` until SDLC-12 decides).
pub const fn category_of(status: Status) -> Category {
    match status {
        Status::Pending | Status::Blocked | Status::Idea | Status::Epic => Category::Open,
        Status::Ready => Category::Ready,
        Status::Coding => Category::Active,
        Status::InReview => Category::Review,
        Status::Complete => Category::Done,
        Status::Superseded | Status::Canceled => Category::Dropped,
        Status::Doc => Category::Static,
    }
}

// ---------------------------------------------------------------------
// The AST — manifest's `Predicate` / `Group` shapes as Rust types

/// A field with its value — manifest's `field` and `value` in one: what a
/// predicate tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Field {
    /// `status <name>`: the ticket's status is the one named.
    Status(Status),
    /// `category <name>`: its status's category ([`category_of`]).
    Category(Category),
    /// `kind <name>`.
    Kind(Kind),
    /// `severity <name>`; `None` is `severity none` — it has none.
    Severity(Option<Severity>),
    /// `blocked`: it names a blocker that is not done with.
    Blocked,
    /// `ready`: category `ready` and not `blocked` — the ready set's rule.
    Ready,
    /// `has commits`: at least one commit is linked to it.
    HasCommits,
    /// `menger doc` (FILTER-21): one of menger's own documents — its path
    /// starts with `_MENGER/` (WAVE-10), as each shortcode's board note's
    /// does.
    MengerDoc,
    /// `asking` (MENGER-12): it has an input in the state `open` — a
    /// question waiting on the owner (INPUT-1). An answer, a close or the
    /// body dropping it takes the ticket out; a clear or a reopen with no
    /// answer puts it back.
    Asking,
    /// `label <name>` (MENGER-12): the ticket holds the label (lower case,
    /// [`crate::labels::normalize`]). A ticket holds many, so — as `under` —
    /// two inclusions never intersect.
    Label(String),
    /// `in <SHORTCODE>`: its shortcode is the one named (upper case). ORG-3:
    /// by its letters — every shortcode with them, wherever hosted.
    In(String),
    /// `under <ID or SHORTCODE>`: the one named is in its lineage — it is
    /// that ticket or below it, or in that shortcode or one hosted under it.
    /// ORG-3: any address, resolved by the store to one ticket or shortcode
    /// (or refused, naming each it could be).
    Under(String),
    /// `text "…"`: its id or title contains the text — find's rule
    /// (SCALE-6): trimmed, ASCII case folded; a blank text matches all.
    Text(String),
    /// MENGER-13: `entered <status>`, `left <status>`, `changed` — with an
    /// optional window and `by` — over the transition log: some transition
    /// of the ticket's did that, in the window, by that actor ([`History`]).
    History(History),
    /// FILTER-36: a group as one item of another — `(a or (b and c))`, at
    /// any depth. Canonical ([`Group::of`]): it holds two or more items, its
    /// join is not its parent's unless it is negated, and a negated one is
    /// never a list (`not (status A or status B)` is `not status in (A, B)`).
    Group(Box<Group>),
    /// FILTER-36: `saved "Name"` — the saved filter (FILTER-6) of that name,
    /// as written. It means nothing until [`Filter::resolve`] puts the named
    /// filter in its place: the store resolves every filter before it
    /// compiles one, and [`Filter::matches`] passes no ticket through an
    /// unresolved name.
    Saved(String),
}

impl Field {
    /// The field's name as a filter writes it: `status`, `has commits`, …
    pub const fn name(&self) -> &'static str {
        match self {
            Field::Status(_) => "status",
            Field::Category(_) => "category",
            Field::Kind(_) => "kind",
            Field::Severity(_) => "severity",
            Field::Blocked => "blocked",
            Field::Ready => "ready",
            Field::HasCommits => "has commits",
            Field::MengerDoc => "menger doc",
            Field::Asking => "asking",
            Field::Label(_) => "label",
            Field::In(_) => "in",
            Field::Under(_) => "under",
            Field::Text(_) => "text",
            Field::History(h) => h.event.name(),
            Field::Group(_) => "group",
            Field::Saved(_) => "saved",
        }
    }

    /// The value as a filter names it, unquoted (`In Review`, `none`,
    /// `SCALE`); `None` for a flag. MENGER-13: a history predicate's is what
    /// follows its name, as written canonically (`Coding within 12h`).
    pub fn value(&self) -> Option<String> {
        Some(match self {
            Field::Status(s) => s.as_str().into(),
            Field::Category(c) => c.as_str().into(),
            Field::Kind(k) => k.as_str().into(),
            Field::Severity(s) => severity_name(*s).into(),
            Field::Blocked | Field::Ready | Field::HasCommits | Field::MengerDoc | Field::Asking => return None,
            Field::In(x) | Field::Under(x) | Field::Text(x) | Field::Label(x) | Field::Saved(x) => x.clone(),
            // FILTER-36: a nested group's value is its text.
            Field::Group(g) => g.format(),
            Field::History(h) => {
                let full = h.format();
                // FILTER-27: a `status … for …` is its whole text.
                let tail = full.strip_prefix(h.event.name()).unwrap_or(&full).trim_start().to_string();
                if tail.is_empty() {
                    return None;
                }
                tail
            }
        })
    }

    /// Whether a group may list this field's values (`field in (a, b)`).
    const fn listable(&self) -> bool {
        matches!(
            self,
            Field::Status(_)
                | Field::Category(_)
                | Field::Kind(_)
                | Field::Severity(_)
                | Field::In(_)
                | Field::Under(_)
                | Field::Label(_)
        )
    }

    /// Whether a ticket holds exactly one value of this field — what lets
    /// two inclusions intersect and an exclusion subtract. `under` and
    /// `label` (MENGER-12) are the listable fields that do not: a lineage
    /// holds many names, a ticket many labels.
    const fn single_valued(&self) -> bool {
        self.listable() && !matches!(self, Field::Under(_) | Field::Label(_))
    }

    /// The field tested on a cell, `None` for a position or row field.
    fn on_cell(&self, c: &Cell) -> Option<bool> {
        Some(match self {
            Field::Status(s) => c.status == *s,
            Field::Category(x) => category_of(c.status) == *x,
            Field::Kind(k) => c.kind == *k,
            Field::Severity(s) => c.severity == *s,
            Field::Blocked => c.blocked,
            Field::Ready => category_of(c.status) == Category::Ready && !c.blocked,
            Field::HasCommits => c.commits,
            Field::MengerDoc => c.menger,
            Field::Asking => c.asking,
            Field::In(_)
            | Field::Under(_)
            | Field::Text(_)
            | Field::Label(_)
            | Field::History(_)
            | Field::Group(_)
            | Field::Saved(_) => return None,
        })
    }

    fn test(&self, f: &Facts) -> bool {
        if let Some(hit) = self.on_cell(&f.cell()) {
            return hit;
        }
        match self {
            Field::In(e) => f.epic.eq_ignore_ascii_case(e),
            Field::Under(x) => f.lineage.iter().any(|l| l.eq_ignore_ascii_case(x)),
            Field::Text(t) => text_matches(t, &f.id, &f.title),
            Field::Label(l) => f.labels.iter().any(|x| x.eq_ignore_ascii_case(l)),
            Field::History(h) => {
                let key = h.format();
                f.history.contains(&key)
            }
            // FILTER-36: the same walk, deeper.
            Field::Group(g) => g.matches(f),
            // Unresolved: nothing passes ([`Filter::resolve`] first).
            Field::Saved(_) => false,
            _ => false,
        }
    }
}

/// `not` (or not) before a field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Predicate {
    pub field: Field,
    pub not: bool,
}

impl Predicate {
    /// The field, as is.
    pub fn is(field: Field) -> Self {
        Self { field, not: false }
    }

    /// The field, negated.
    pub fn is_not(field: Field) -> Self {
        Self { field, not: true }
    }

    /// Whether `facts` pass this predicate.
    pub fn matches(&self, facts: &Facts) -> bool {
        self.field.test(facts) != self.not
    }

    /// The predicate as a filter writes it: `not status "In Review"`.
    pub fn format(&self) -> String {
        let head = if self.not { "not " } else { "" };
        match &self.field {
            Field::Blocked | Field::Ready | Field::HasCommits | Field::MengerDoc | Field::Asking => {
                format!("{head}{}", self.field.name())
            }
            Field::Text(t) => format!("{head}text \"{t}\""),
            Field::History(h) => format!("{head}{}", h.format()),
            Field::Group(g) => format!("{head}{}", g.format()),
            Field::Saved(n) => format!("{head}saved {}", quoted(n)),
            f => format!("{head}{} {}", f.name(), quoted(&f.value().unwrap_or_default())),
        }
    }
}

/// How a group joins its predicates: manifest's `conj`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Conj {
    /// Any of them: `( … or … )`, `field in (a, b)`.
    Any,
    /// All of them: `( … and … )`, `not field in (a, b)`, one predicate.
    #[default]
    All,
}

/// Predicates joined by one [`Conj`] — one `and`-joined chunk of a filter.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Group {
    pub conj: Conj,
    pub preds: Vec<Predicate>,
}

impl Group {
    /// One predicate: `status Ready`, `not blocked`.
    pub fn one(pred: Predicate) -> Self {
        Self { conj: Conj::All, preds: vec![pred] }
    }

    /// Any of `preds` — `field in (a, b)` when they are one field's values.
    /// One predicate is [`Group::one`].
    pub fn any(preds: Vec<Predicate>) -> Self {
        Self::of(Conj::Any, preds)
    }

    /// All of `preds` — `not field in (a, b)` when they are one field's
    /// values, each negated.
    pub fn all(preds: Vec<Predicate>) -> Self {
        Self::of(Conj::All, preds)
    }

    /// The canonical shape: one predicate is `All`, whatever joins it.
    /// FILTER-36: and every nested group settled ([`settle`]) — a one-item
    /// group is its item, a group joined as its parent is spliced into it,
    /// a negated list is the other list — and a group of one group, not
    /// negated, is that group.
    pub fn of(conj: Conj, preds: Vec<Predicate>) -> Self {
        let mut out = Vec::with_capacity(preds.len());
        for p in preds {
            settle(p, conj, &mut out);
        }
        if let [Predicate { field: Field::Group(_), not: false }] = out.as_slice() {
            let Some(Predicate { field: Field::Group(g), .. }) = out.pop() else { unreachable!() };
            return *g;
        }
        let conj = if out.len() == 1 { Conj::All } else { conj };
        Self { conj, preds: out }
    }

    /// FILTER-36: this group as one item of another — `not` or not.
    pub fn nested(self, not: bool) -> Predicate {
        Predicate { field: Field::Group(Box::new(self)), not }
    }

    /// FILTER-36: every predicate at its leaves — a nested group's, at any
    /// depth, in the order written; a nested group is not one of them.
    pub fn leaves(&self) -> Vec<&Predicate> {
        let mut out = Vec::new();
        fn walk<'a>(g: &'a Group, out: &mut Vec<&'a Predicate>) {
            for p in &g.preds {
                match &p.field {
                    Field::Group(inner) => walk(inner, out),
                    _ => out.push(p),
                }
            }
        }
        walk(self, &mut out);
        out
    }

    /// FILTER-36: whether it holds a nested group or a `saved` name — what
    /// the app's bar draws as text.
    pub fn is_nested(&self) -> bool {
        self.preds.iter().any(|p| matches!(p.field, Field::Group(_) | Field::Saved(_)))
    }

    /// FILTER-36: a list's other list — `any of a, b` negated is `none of a,
    /// b` (De Morgan, which keeps a negated list a list).
    fn negated_list(self) -> Group {
        let conj = match self.conj {
            Conj::Any => Conj::All,
            Conj::All => Conj::Any,
        };
        Group { conj, preds: self.preds.into_iter().map(|p| Predicate { not: !p.not, ..p }).collect() }
    }

    /// Whether `facts` pass the group. An empty group passes (manifest's
    /// `matchesGroups`).
    pub fn matches(&self, facts: &Facts) -> bool {
        match self.conj {
            _ if self.preds.is_empty() => true,
            Conj::Any => self.preds.iter().any(|p| p.matches(facts)),
            Conj::All => self.preds.iter().all(|p| p.matches(facts)),
        }
    }

    /// The group as a filter writes it (manifest's `formatGroup`): a list
    /// where it lists one field's values, else one predicate, else
    /// parentheses joined by `or` or `and`.
    pub fn format(&self) -> String {
        match self.preds.as_slice() {
            [] => String::new(),
            [p] => p.format(),
            ps if self.listable() => {
                let head = if ps[0].not { "not " } else { "" };
                let values: Vec<String> = ps.iter().map(|p| quoted(&p.field.value().unwrap_or_default())).collect();
                match ps[0].field {
                    Field::In(_) => format!("{head}in ({})", values.join(", ")),
                    ref f => format!("{head}{} in ({})", f.name(), values.join(", ")),
                }
            }
            ps => {
                let join = if self.conj == Conj::Any { " or " } else { " and " };
                format!("({})", ps.iter().map(Predicate::format).collect::<Vec<_>>().join(join))
            }
        }
    }

    /// manifest's `listable`: two or more values of one listable field,
    /// all negated and joined by `All`, or none negated and joined by `Any`.
    fn listable(&self) -> bool {
        let Some(first) = self.preds.first() else { return false };
        self.preds.len() > 1
            && first.field.listable()
            && self.preds.iter().all(|p| p.field.name() == first.field.name() && p.not == first.not)
            && self.conj == if first.not { Conj::All } else { Conj::Any }
    }
}

/// FILTER-36: `p` into `out`, an item of a group joined by `conj`, in the
/// canonical shape: a one-item group is its item (`not (a)` is `not a`); a
/// negated list is the other list; a group joined as its parent, not
/// negated, is spliced into it. Every group `p` holds is settled already.
fn settle(p: Predicate, conj: Conj, out: &mut Vec<Predicate>) {
    let Predicate { field, not } = p;
    let mut g = match field {
        Field::Group(g) => *g,
        field => return out.push(Predicate { field, not }),
    };
    if g.preds.len() == 1 {
        let mut inner = g.preds.pop().expect("one item");
        inner.not ^= not;
        return settle(inner, conj, out);
    }
    let mut not = not;
    if not && g.listable() {
        g = g.negated_list();
        not = false;
    }
    match !not && g.conj == conj && !g.preds.is_empty() {
        true => out.extend(g.preds),
        false => out.push(g.nested(not)),
    }
}

/// The nav's shape under a filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Shape {
    #[default]
    Tree,
    Flat,
}

/// The order served — the store's, the only one (FILTER-3 §6.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Code,
}

/// The display part, after `·` (FILTER-3 §2): the nav's shape, the store's
/// order when named, and the board's columns (lowercase; only the app reads
/// them, and an empty list is the app's default).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Display {
    pub shape: Shape,
    pub order: Option<Order>,
    pub columns: Vec<String>,
}

impl Display {
    /// Each part that is not the default, `·`-joined; empty for the default.
    fn format(&self) -> String {
        let mut parts = Vec::new();
        if self.shape == Shape::Flat {
            parts.push("flat".to_string());
        }
        if self.order == Some(Order::Code) {
            parts.push("order code".to_string());
        }
        if !self.columns.is_empty() {
            parts.push(format!("columns {}", self.columns.join(", ")));
        }
        parts.join(" · ")
    }

    /// `other`'s setting wherever it sets one, else `self`'s.
    fn and(&self, other: &Display) -> Display {
        Display {
            shape: if other.shape == Shape::default() { self.shape } else { other.shape },
            order: other.order.or(self.order),
            columns: if other.columns.is_empty() { self.columns.clone() } else { other.columns.clone() },
        }
    }

    /// One display chunk into `self` (manifest's `parseDisplay`, with
    /// menger's refusals).
    fn read(&mut self, chunk: &str) -> Result<(), String> {
        let lower = chunk.to_ascii_lowercase();
        let words: Vec<&str> = lower.split_ascii_whitespace().collect();
        match words.as_slice() {
            ["tree"] => self.shape = Shape::Tree,
            ["flat"] => self.shape = Shape::Flat,
            ["tree" | "flat", "by", ..] => {
                return Err("grouping by a field is not served: the shape is `tree` or `flat`".into());
            }
            ["board", ..] => return Err("`board by …` (a kanban) is refused (FILTER-3 §6.9)".into()),
            ["order", "code"] => self.order = Some(Order::Code),
            ["order", ..] => {
                return Err("only `order code`, the store's order, is served (FILTER-3 §6.9)".into());
            }
            ["columns", ..] => {
                let mut columns = Vec::new();
                for column in parts(split_top(&lower["columns".len()..], ",")) {
                    if !column.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'_' | b'-')) {
                        return Err(format!(
                            "`{column}` is not a column name: letters, digits, spaces, `-` and `_`"
                        ));
                    }
                    columns.push(column.to_string());
                }
                if columns.is_empty() {
                    return Err("`columns` names at least one column".into());
                }
                self.columns = columns;
            }
            _ => {
                return Err("not a display: after `·` come `tree`, `flat`, `columns a, b` and `order code`".into());
            }
        }
        Ok(())
    }
}

/// A filter: groups, all of which a ticket passes, and a display.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Filter {
    pub groups: Vec<Group>,
    pub display: Display,
}

/// A chunk the language did not take, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadChunk {
    /// The chunk as written: one `and`-joined group, or one display part.
    pub text: String,
    pub why: String,
}

/// Every chunk a filter text holds that the language did not take — a
/// filter with any is not parsed at all (FILTER-3 §2's strictness).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterError {
    pub chunks: Vec<BadChunk>,
    /// FILTER-36: the text was read, and these chunks name saved filters
    /// that do not resolve ([`Filter::resolve`]).
    pub unresolved: bool,
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.unresolved { "not resolved: " } else { "not understood: " })?;
        for (i, c) in self.chunks.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(f, "`{}` — {}", c.text, c.why)?;
        }
        Ok(())
    }
}

impl std::error::Error for FilterError {}

/// The groups of a filter that admit no ticket together: a smallest such
/// set found — one group, else two, else every group that shares a field
/// with them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contradiction {
    /// Their places in the filter's `groups`, ascending.
    pub at: Vec<usize>,
    /// The groups themselves.
    pub groups: Vec<Group>,
}

impl fmt::Display for Contradiction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let named: Vec<String> = self.groups.iter().map(|g| format!("`{}`", g.format())).collect();
        match named.as_slice() {
            [one] => write!(f, "{one} admits no ticket"),
            [rest @ .., last] => write!(f, "{} and {last} admit no ticket together", rest.join(", ")),
            [] => f.write_str("admits no ticket"),
        }
    }
}

/// What the reference evaluator reads of a ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    pub id: String,
    /// Its shortcode.
    pub epic: String,
    pub title: String,
    pub status: Status,
    pub kind: Kind,
    pub severity: Option<Severity>,
    /// It names a blocker that is not done with (a missing one counts).
    pub blocked: bool,
    pub has_commits: bool,
    /// FILTER-21: its path starts with `_MENGER/` — one of menger's own
    /// documents ([`crate::is_menger_doc`]).
    pub menger_doc: bool,
    /// MENGER-12: it has an input in the state `open`.
    pub asking: bool,
    /// MENGER-12: its labels, lower case.
    pub labels: Vec<String>,
    /// What `under` finds it under: its own id, its ancestors' ids, its
    /// shortcode and the shortcodes hosting it.
    pub lineage: Vec<String>,
    /// MENGER-13: the history predicates that hold for it, each as its
    /// canonical text ([`History::format`]) — what the transition log says
    /// of it, at the moment and in the zone the filter is read in.
    pub history: Vec<String>,
}

impl Facts {
    fn cell(&self) -> Cell {
        Cell {
            status: self.status,
            kind: self.kind,
            severity: self.severity,
            blocked: self.blocked,
            commits: self.has_commits,
            menger: self.menger_doc,
            asking: self.asking,
        }
    }
}

impl Filter {
    /// Reads a filter text (manifest's `parseView`, strict): every chunk it
    /// cannot take is named in the error, with why.
    pub fn parse(raw: &str) -> Result<Filter, FilterError> {
        let mut sections = split_top(raw, "·").into_iter();
        let filter_text = sections.next().unwrap_or("");
        let mut bad = Vec::new();
        let mut groups = Vec::new();
        for chunk in parts(split_and(filter_text)) {
            match parse_group(chunk) {
                Ok(g) => groups.push(g),
                Err(why) => bad.push(BadChunk { text: chunk.into(), why }),
            }
        }
        let mut display = Display::default();
        for chunk in sections.map(str::trim).filter(|c| !c.is_empty()) {
            if let Err(why) = display.read(chunk) {
                bad.push(BadChunk { text: chunk.into(), why });
            }
        }
        match bad.is_empty() {
            true => Ok(Filter { groups, display }),
            false => Err(FilterError { chunks: bad, unresolved: false }),
        }
    }

    /// The canonical text (manifest's `formatView`, with the default
    /// display written as nothing): what the wire carries.
    pub fn format(&self) -> String {
        let filter: Vec<String> = self.groups.iter().map(Group::format).filter(|g| !g.is_empty()).collect();
        let (filter, display) = (filter.join(" and "), self.display.format());
        match (filter.is_empty(), display.is_empty()) {
            (_, true) => filter,
            (true, false) => format!("· {display}"),
            (false, false) => format!("{filter} · {display}"),
        }
    }

    /// No group, and the default display: the text is empty.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.display == Display::default()
    }

    /// The reference evaluator: whether a ticket with these facts passes
    /// every group.
    pub fn matches(&self, facts: &Facts) -> bool {
        self.groups.iter().all(|g| g.matches(facts))
    }

    /// The lens's aggregation: the tickets both filters match, as one
    /// filter — `self`'s groups then `other`'s, normalized:
    ///
    /// - a group identical to one already there is dropped;
    /// - two groups that each list values of one field merge: two
    ///   inclusions intersect, two exclusions unite, an inclusion and an
    ///   exclusion subtract — intersecting and subtracting only where a
    ///   ticket holds one value of the field (every listable field but
    ///   `under`, whose lineage holds many);
    /// - a merge that would leave no value is not made: both groups stay as
    ///   written, for [`Filter::admits_nothing`] to name;
    /// - the display is `other`'s wherever it sets one, else `self`'s.
    pub fn and(&self, other: &Filter) -> Filter {
        let mut groups: Vec<Group> = Vec::new();
        for g in self.groups.iter().chain(&other.groups) {
            absorb(&mut groups, g.clone());
        }
        Filter { groups, display: self.display.and(&other.display) }
    }

    /// `Some` exactly when no ticket can match: no world of the closed
    /// domain passes every group. A world is a cell (status × kind ×
    /// severity-or-none × blocked × has commits × menger doc, where a doc
    /// holds `Doc` and only a doc does, and only a doc is menger's own), a
    /// shortcode (one the filter names, or any other), and a free truth for
    /// each `under` name and each text. The
    /// contradiction names the smallest set of groups found that admits
    /// nothing by itself: one group, else two, else every group that
    /// shares a field with them.
    pub fn admits_nothing(&self) -> Option<Contradiction> {
        let n = self.groups.len();
        let all: Vec<usize> = (0..n).collect();
        let whole = clash(&self.groups, &all)?;
        let at = (0..n)
            .map(|i| vec![i])
            .chain((0..n).flat_map(|i| (i + 1..n).map(move |j| vec![i, j])))
            .find(|set| clash(&self.groups, set).is_some())
            .unwrap_or(whole);
        let groups = at.iter().map(|&i| self.groups[i].clone()).collect();
        Some(Contradiction { at, groups })
    }
}

impl Filter {
    /// FILTER-36: every predicate at its leaves, group by group — a nested
    /// group's at any depth; never a nested group itself.
    pub fn leaves(&self) -> Vec<&Predicate> {
        self.groups.iter().flat_map(Group::leaves).collect()
    }

    /// FILTER-36: the saved filters it names, as written, each once.
    pub fn saved_names(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for p in self.leaves() {
            if let Field::Saved(n) = &p.field {
                if !out.contains(&n.as_str()) {
                    out.push(n);
                }
            }
        }
        out
    }

    /// FILTER-36: whether it holds a nested group or a `saved` name — a
    /// shape FILTER-10's two levels do not draw.
    pub fn is_nested(&self) -> bool {
        self.groups.iter().any(Group::is_nested)
    }

    /// FILTER-36: the filter with every `saved "Name"` in it replaced by the
    /// filter that name holds — `lookup` says a name's canonical text, or
    /// `None` when no saved filter has it — recursively, the display kept.
    ///
    /// A name standing alone as a top-level group brings its groups in as
    /// groups of this filter; anywhere else it stands for one group of all
    /// of them, under the join (and the `not`) it stood in: `saved A or
    /// saved B`, each an `and` of groups, is one `or` of two all-of groups.
    ///
    /// Refused, every chunk that names one: a name nothing holds (`no saved
    /// filter named "X"`); a cycle (`"A" names "B" names "A"`); names
    /// nested past [`SAVED_DEPTH`]; a saved text the language does not
    /// read, one that names no predicate, and one holding `text` — which
    /// stands only at a filter's own top, never through a name.
    pub fn resolve(&self, lookup: &mut impl FnMut(&str) -> Option<String>) -> Result<Filter, FilterError> {
        self.resolve_in(lookup, &mut Vec::new())
    }

    fn resolve_in(&self, lookup: &mut impl FnMut(&str) -> Option<String>, stack: &mut Vec<(String, String)>) -> Result<Filter, FilterError> {
        if self.saved_names().is_empty() {
            return Ok(self.clone());
        }
        let (mut groups, mut bad) = (Vec::new(), Vec::new());
        for g in &self.groups {
            let read = match g.preds.as_slice() {
                [Predicate { field: Field::Saved(name), not: false }] => expand(name, lookup, stack).map(|f| f.groups),
                _ => resolve_group(g, lookup, stack).map(|g| vec![g]),
            };
            match read {
                Ok(gs) => groups.extend(gs),
                Err(why) => bad.push(BadChunk { text: g.format(), why }),
            }
        }
        match bad.is_empty() {
            true => Ok(Filter { groups, display: self.display.clone() }),
            false => Err(FilterError { chunks: bad, unresolved: true }),
        }
    }
}

/// FILTER-36: how deep saved filters may name saved filters — a backstop
/// behind the cycle check.
pub const SAVED_DEPTH: usize = 8;

/// FILTER-36: `g` with every name in it resolved.
fn resolve_group(g: &Group, lookup: &mut impl FnMut(&str) -> Option<String>, stack: &mut Vec<(String, String)>) -> Result<Group, String> {
    if !g.leaves().iter().any(|p| matches!(p.field, Field::Saved(_))) {
        return Ok(g.clone());
    }
    let mut preds = Vec::with_capacity(g.preds.len());
    for p in &g.preds {
        preds.push(match &p.field {
            Field::Saved(name) => {
                let named = expand(name, lookup, stack)?;
                Group::of(Conj::All, named.groups.into_iter().map(|g| g.nested(false)).collect()).nested(p.not)
            }
            Field::Group(inner) => resolve_group(inner, lookup, stack)?.nested(p.not),
            _ => p.clone(),
        });
    }
    Ok(Group::of(g.conj, preds))
}

/// FILTER-36: the filter `name` holds, its own names resolved — `stack`
/// the names being expanded, outermost first (by slug, as names match).
fn expand(name: &str, lookup: &mut impl FnMut(&str) -> Option<String>, stack: &mut Vec<(String, String)>) -> Result<Filter, String> {
    let key = crate::saved_filters::slug(name);
    if let Some(at) = stack.iter().position(|(k, _)| *k == key) {
        let chain: Vec<String> = stack[at..].iter().map(|(_, n)| n.as_str()).chain([name]).map(|n| format!("\"{n}\"")).collect();
        return Err(format!("a cycle: {}", chain.join(" names ")));
    }
    if stack.len() >= SAVED_DEPTH {
        return Err(format!("saved filters name saved filters at most {SAVED_DEPTH} deep; \"{name}\" is deeper"));
    }
    let text = lookup(name).ok_or_else(|| format!("no saved filter named \"{name}\""))?;
    let parsed = Filter::parse(&text).map_err(|e| format!("the saved filter \"{name}\" does not read — {e}"))?;
    if parsed.groups.is_empty() {
        return Err(format!("the saved filter \"{name}\" names no predicate"));
    }
    if parsed.leaves().iter().any(|p| matches!(p.field, Field::Text(_))) {
        return Err(format!(
            "the saved filter \"{name}\" holds `text`, which stands only at a filter's own top — never through a name"
        ));
    }
    stack.push((key, name.to_string()));
    let out = parsed.resolve_in(lookup, stack);
    stack.pop();
    out.map_err(|e| e.chunks.into_iter().map(|c| c.why).collect::<Vec<_>>().join("; "))
}

impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}

impl FromStr for Filter {
    type Err = FilterError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Filter::parse(s)
    }
}

// ---------------------------------------------------------------------
// Reading — manifest's `splitTop`, `wrapped`, `stripNot`, `parsePredicate`,
// `listGroup`, `parseGroup`

/// The fields a filter may name — menger's (FILTER-3 §1) and manifest's
/// that menger refuses — longest first, as manifest's `BY_LENGTH`.
const NAMES: [&str; 33] = [
    "has commits",
    "menger doc",
    "discussion",
    "unattested",
    // DEPLOY-11: where a ticket's work runs.
    "deployed",
    "category",
    "severity",
    "past cap",
    "archived",
    // MENGER-13: the history predicates.
    "entered",
    "changed",
    "blocked",
    "session",
    "asking",
    // MENGER-39: the owner's queue.
    "queued",
    "created",
    // FILTER-30: the ticket's last touch.
    "touched",
    "written",
    "status",
    "lowest",
    // FILTER-36: a saved filter by name.
    "saved",
    "under",
    "label",
    "ready",
    "depth",
    "kind",
    "text",
    "left",
    "tag",
    // FILTER-27: `for over 2w`.
    "for",
    "age",
    "in",
    "by",
];

const TEXT_ALONE: &str = "`text` is tested on rows, so it stands alone as a top-level `and` group — never \
                          inside `or`, under `not`, or in parentheses with other predicates";
const MIXED: &str = "`or` and `and` do not mix inside one pair of parentheses: a group is all `or` or all `and`";
const UNBALANCED: &str = "a group's parentheses close where they open: `(a or (b and c))`";
const NOT_UNDERSTOOD: &str = "not understood: a predicate is `status`, `category`, `kind`, `severity`, `blocked`, \
                              `ready`, `has commits`, `menger doc`, `asking`, `label`, `in`, `under`, `text`, \
                              `entered`, `left`, `changed`, `by`, `created`, `for`, `deployed`, `queued` or \
                              `saved`, a group in parentheses, or a quoted text";
const QUOTES: &str = "a quoted value is quoted whole — one `\"` before it and one after, none inside";

/// `raw` cut at `sep` (ASCII case-insensitive) where it stands outside
/// quotes and parentheses — every piece, untrimmed, empty ones included.
fn split_top<'a>(raw: &'a str, sep: &str) -> Vec<&'a str> {
    let (b, s) = (raw.as_bytes(), sep.as_bytes());
    let (mut out, mut depth, mut quoted, mut start, mut i) = (Vec::new(), 0usize, false, 0, 0);
    while i < b.len() {
        let c = b[i];
        if quoted {
            quoted = c != b'"';
            i += 1;
            continue;
        }
        match c {
            b'"' => {
                quoted = true;
                i += 1;
                continue;
            }
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && b.len() - i >= s.len() && b[i..i + s.len()].eq_ignore_ascii_case(s) {
            out.push(&raw[start..i]);
            i += s.len();
            start = i;
            continue;
        }
        i += 1;
    }
    out.push(&raw[start..]);
    out
}

/// MENGER-13: `raw` cut at ` and ` as [`split_top`] cuts it — except the
/// ` and ` of a history window's `between <moment> and <moment>`, which
/// joins its two moments rather than two groups: a piece that ends in
/// `between <moment>` keeps the next piece.
fn split_and(raw: &str) -> Vec<&str> {
    let pieces = split_top(raw, " and ");
    let mut out: Vec<&str> = Vec::with_capacity(pieces.len());
    let offset = |p: &str| p.as_ptr() as usize - raw.as_ptr() as usize;
    let mut i = 0;
    while i < pieces.len() {
        let start = offset(pieces[i]);
        let mut end = start + pieces[i].len();
        while i + 1 < pieces.len() && ends_in_between(&raw[start..end]) {
            i += 1;
            end = offset(pieces[i]) + pieces[i].len();
        }
        out.push(&raw[start..end]);
        i += 1;
    }
    out
}

/// Whether `s` ends `between <moment>` — a window waiting for its second
/// moment (a date, a date and a time written apart, or — FILTER-27 — a
/// length `ago`).
fn ends_in_between(s: &str) -> bool {
    let all: Vec<&str> = s.split_ascii_whitespace().collect();
    let Some(at) = all.iter().rposition(|w| w.eq_ignore_ascii_case("between")) else { return false };
    let after: Vec<String> = all[at + 1..].iter().map(|w| w.to_string()).collect();
    let mut i = 0;
    !after.is_empty() && read_when(&after, &mut i, "between").is_ok() && i == after.len()
}

/// The pieces trimmed, the empty ones dropped (manifest's `trimAll`).
fn parts(pieces: Vec<&str>) -> Vec<&str> {
    pieces.into_iter().map(str::trim).filter(|p| !p.is_empty()).collect()
}

/// What one pair of parentheses around the whole of `s` holds, if one does.
fn wrapped(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    if b.first() != Some(&b'(') {
        return None;
    }
    let (mut depth, mut quoted) = (0usize, false);
    for (i, &c) in b.iter().enumerate() {
        if quoted {
            quoted = c != b'"';
            continue;
        }
        match c {
            b'"' => quoted = true,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return (i == b.len() - 1).then(|| &s[1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Leading `not`s off `raw`, toggling.
fn strip_not(raw: &str) -> (bool, &str) {
    let (mut not, mut rest) = (false, raw.trim());
    loop {
        let b = rest.as_bytes();
        if b.len() > 3 && b[..3].eq_ignore_ascii_case(b"not") && b[3].is_ascii_whitespace() {
            not = !not;
            rest = rest[3..].trim_start();
        } else {
            return (not, rest);
        }
    }
}

/// The field `rest` begins with, and what follows it, trimmed.
fn split_field(rest: &str) -> Option<(&'static str, &str)> {
    NAMES.into_iter().find_map(|name| {
        let b = rest.as_bytes();
        if b.len() < name.len() || !b[..name.len()].eq_ignore_ascii_case(name.as_bytes()) {
            return None;
        }
        let tail = &rest[name.len()..];
        match tail.bytes().next() {
            None => Some((name, "")),
            Some(c) if c.is_ascii_whitespace() => Some((name, tail.trim())),
            Some(_) => None,
        }
    })
}

/// A value as written: quoted whole, or holding no quote at all.
fn unquote(s: &str) -> Result<&str, String> {
    match s.strip_prefix('"') {
        Some(inner) => match inner.strip_suffix('"') {
            Some(v) if !v.contains('"') => Ok(v),
            _ => Err(QUOTES.into()),
        },
        None if s.contains('"') => Err(QUOTES.into()),
        None => Ok(s),
    }
}

/// A value as a filter writes it: quoted when it holds a space, a comma, a
/// parenthesis or a `·`, or is empty.
fn quoted(value: &str) -> String {
    let bare = !value.is_empty() && !value.chars().any(|c| c.is_whitespace() || matches!(c, ',' | '(' | ')' | '·'));
    if bare {
        value.to_string()
    } else {
        format!("\"{value}\"")
    }
}

const fn severity_name(s: Option<Severity>) -> &'static str {
    match s {
        None => "none",
        Some(Severity::Critical) => "critical",
        Some(Severity::High) => "high",
        Some(Severity::Medium) => "medium",
        Some(Severity::Low) => "low",
    }
}

fn no_data(name: &str) -> String {
    format!("menger holds no data for `{name}` (FILTER-3 §1)")
}

/// One predicate (manifest's `parsePredicate`, strict).
fn parse_predicate(raw: &str) -> Result<Predicate, String> {
    let (not, rest) = strip_not(raw);
    Ok(Predicate { field: read_field(rest)?, not })
}

fn read_field(rest: &str) -> Result<Field, String> {
    if rest.is_empty() {
        return Err("`not` stands before a predicate, and none follows".into());
    }
    if rest.starts_with('#') {
        return Err(no_data("#tag"));
    }
    if rest.starts_with('"') {
        return text(unquote(rest)?);
    }
    // FILTER-36: a group inside a group.
    if rest.starts_with('(') {
        return match wrapped(rest) {
            Some(inner) => inner_group(inner).map(|g| Field::Group(Box::new(g))),
            None => Err(UNBALANCED.into()),
        };
    }
    // FILTER-27: `… for over|under <length>`, one predicate.
    if let Some(at) = held_at(rest).filter(|at| *at > 0) {
        return read_held(&rest[..at], &rest[at + 3..]).map(Field::History);
    }
    let Some((name, tail)) = split_field(rest) else { return Err(NOT_UNDERSTOOD.into()) };
    let flag = |field: Field| match tail.is_empty() {
        true => Ok(field),
        false => Err(format!("`{name}` takes no value")),
    };
    let value = || match tail.is_empty() {
        true => Err(format!("`{name}` needs a value")),
        false => unquote(tail),
    };
    match name {
        "blocked" => flag(Field::Blocked),
        "ready" => flag(Field::Ready),
        "has commits" => flag(Field::HasCommits),
        "menger doc" => flag(Field::MengerDoc),
        "asking" => flag(Field::Asking),
        "label" => crate::labels::normalize(value()?).map(Field::Label),
        "text" => text(value()?),
        "status" => {
            let v = value()?;
            v.parse().map(Field::Status).map_err(|_| {
                format!(
                    "unknown status `{v}` — the statuses are {} (`active` is Coding, `in review` is In Review)",
                    Status::ALL.map(Status::as_str).join(", ")
                )
            })
        }
        "category" => value()?.parse().map(Field::Category),
        "kind" => {
            let v = value()?;
            v.parse().map(Field::Kind).map_err(|_| {
                format!("unknown kind `{v}` — the kinds are {}", Kind::ALL.map(Kind::as_str).join(", "))
            })
        }
        "severity" => {
            let v = value()?;
            [None, Some(Severity::Critical), Some(Severity::High), Some(Severity::Medium), Some(Severity::Low)]
                .into_iter()
                .find(|s| severity_name(*s).eq_ignore_ascii_case(v))
                .map(Field::Severity)
                .ok_or_else(|| format!("unknown severity `{v}` — the severities are critical, high, medium, low, none"))
        }
        "in" => {
            let v = value()?;
            // ACCESS-8: the one shortcode rule, `Code`'s.
            match crate::address::Code::parse(v) {
                Ok(code) => Ok(Field::In(code.as_str().to_string())),
                Err(_) => Err(format!("`in` takes a shortcode, as `in SCALE` — not `{v}`")),
            }
        }
        // ORG-3: an id, a shortcode or any address — `org:ROOT/…/SC-N`,
        // `MENGER/THEME`, a path address — spelled as the store spells it
        // ([`crate::address::folded`]: its codes upper case, its org and
        // path as written); the store resolves it to one.
        "under" => {
            let v = value()?;
            let ok = v.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
                && v.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':' | b'/'));
            match ok {
                true => Ok(Field::Under(crate::address::folded(v))),
                false => Err(format!("`under` takes an id, a shortcode or an address, as `under SCALE-13` — not `{v}`")),
            }
        }
        // MENGER-13: the transition log.
        "entered" | "left" | "changed" | "by" => read_history(name, tail).map(Field::History),
        // FILTER-27: births, over their own index.
        "created" => read_history(name, tail).map(Field::History),
        // FILTER-30: the last touch, over its own index.
        "touched" => read_history(name, tail).map(Field::History),
        // DEPLOY-11: over the deployments' kept cells.
        "deployed" => read_history(name, tail).map(Field::History),
        // MENGER-39: over the queue's kept entries.
        "queued" => read_history(name, tail).map(Field::History),
        "for" => read_held("", &rest[name.len()..]).map(Field::History),
        // FILTER-36: a saved filter, by its name.
        "saved" => {
            let v = value()?;
            let name = v.split_whitespace().collect::<Vec<_>>().join(" ");
            match crate::saved_filters::slug(&name).is_empty() {
                true => Err(format!("`saved` takes a saved filter's name, which holds a letter or a digit — not `{v}`")),
                false => Ok(Field::Saved(name)),
            }
        }
        "written" | "age" => Err(format!(
            "`{name}` is refused: it needs an index of its own (FILTER-3 §6.9, open question 6)"
        )),
        _ => Err(no_data(name)),
    }
}

fn text(value: &str) -> Result<Field, String> {
    match value.is_empty() {
        true => Err("`text` needs a text, as `text \"watch\"`".into()),
        false => Ok(Field::Text(value.into())),
    }
}

/// `items` — each a label and its reading — as one group, or why not: every
/// predicate's fault, labelled when there are several, and `text` out of
/// place.
fn group(conj: Conj, items: Vec<(&str, Result<Predicate, String>)>) -> Result<Group, String> {
    let labelled = items.len() > 1;
    let (mut preds, mut whys) = (Vec::new(), Vec::<String>::new());
    for (label, read) in items {
        match read {
            Ok(p) => preds.push(p),
            Err(why) => {
                let why = if labelled { format!("`{label}`: {why}") } else { why };
                if !whys.contains(&why) {
                    whys.push(why);
                }
            }
        }
    }
    if whys.is_empty() && preds.is_empty() {
        whys.push("nothing inside the parentheses".into());
    }
    // FILTER-36: judged settled — a `text` a nested group brings up is in
    // the group it lands in.
    let settled = Group::of(conj, preds);
    if !text_alone(&settled) {
        whys.push(TEXT_ALONE.into());
    }
    match whys.is_empty() {
        true => Ok(settled),
        false => Err(whys.join("; ")),
    }
}

/// Whether every `text` in `g` stands alone, not negated, at the top: a
/// group of it alone.
fn text_alone(g: &Group) -> bool {
    let alone = g.preds.len() == 1;
    g.preds.iter().all(|p| match &p.field {
        Field::Text(_) => alone && !p.not,
        Field::Group(inner) => !inner.leaves().iter().any(|l| matches!(l.field, Field::Text(_))),
        _ => true,
    })
}

/// `field in (a, b)` or `not field in (a, b)` — `in (A, B)` for the `in`
/// field — when `s` is one (manifest's `listGroup`).
fn list_group(s: &str) -> Option<Result<Group, String>> {
    let (not, rest) = strip_not(s);
    let (name, tail) = split_field(rest)?;
    let list = match tail.as_bytes() {
        [b'(', ..] if name == "in" => tail,
        [i, n, ..] if [*i, *n].eq_ignore_ascii_case(b"in") && tail[2..].trim_start().starts_with('(') => {
            tail[2..].trim_start()
        }
        _ => return None,
    };
    let inner = wrapped(list)?;
    let items = parts(split_top(inner, ","))
        .into_iter()
        .map(|v| (v, parse_predicate(&format!("{name} {v}")).map(|p| Predicate { not, ..p })))
        .collect();
    Some(group(if not { Conj::All } else { Conj::Any }, items))
}

/// The predicates of an `or` or an `and` inside a group. FILTER-36: a list
/// among them is a group inside it, as a parenthesized one is.
fn members<'a>(raws: &[&'a str]) -> Vec<(&'a str, Result<Predicate, String>)> {
    raws.iter()
        .map(|&r| match list_group(r) {
            Some(list) => (r, list.map(|g| g.nested(false))),
            None => (r, parse_predicate(r)),
        })
        .collect()
}

/// What one pair of parentheses holds: items joined all by `or` or all by
/// `and` — FILTER-36: each a predicate, a list, or a group in parentheses.
fn inner_group(inner: &str) -> Result<Group, String> {
    let ors = parts(split_top(inner, " or "));
    if ors.len() > 1 {
        if ors.iter().any(|o| parts(split_and(o)).len() > 1) {
            return Err(MIXED.into());
        }
        return group(Conj::Any, members(&ors));
    }
    group(Conj::All, members(&parts(split_and(inner))))
}

/// One `and`-joined chunk (manifest's `parseGroup`, strict).
fn parse_group(s: &str) -> Result<Group, String> {
    if let Some(inner) = wrapped(s) {
        return inner_group(inner);
    }
    if let Some(list) = list_group(s) {
        return list;
    }
    let ors = parts(split_top(s, " or "));
    if ors.len() > 1 {
        return group(Conj::Any, members(&ors));
    }
    group(Conj::All, vec![(s, parse_predicate(s))])
}

// ---------------------------------------------------------------------
// MENGER-13: history — predicates over the transition log
//
// `entered <status> [window] [by who]`, `left <status> [window] [by who]`,
// `changed [window] [by who]`, and `by who [window]` (which is `changed …
// by who`). A window is `within <n>{m,h,d,w,mo}` (minutes, hours, days,
// weeks, months of 30 days), `today`, `yesterday`, `this week` (from
// Monday), `since <moment>`, `before <moment>` or `between <moment> and
// <moment>`; a moment is a date, `2026-10-10`, or a date and a time,
// `2026-10-10T08:00` — both in the zone the filter is read in (the viewer's
// in the app; UTC unless a call says `tz`) — or (FILTER-27) a moment
// relative to now, `<n>{m,h,d,w,mo} ago`. No window: ever, since the log
// began. Who: `me` (the caller), `system` (the store's own cascades), or a
// principal as the log names it.
//
// FILTER-27, over time itself rather than the log: `created <window>` — the
// ticket's birth in the window; and `for over <length>` / `for under
// <length>` — how long the ticket has held its current status (since its
// last transition into it, else since its birth; a ticket from before the
// log, since its last update at the latest). `status S for over D` and
// `category C for over D` (a list too) are sugar for `status S and for over
// D`, and the canonical form prints the sugar: one predicate.
//
// FILTER-30: `touched <window>` — the ticket's last touch in the window: a
// write to its own row, a comment on it, an answer to one of its inputs.
// `not touched within 2w` is "hasn't changed in 2 weeks".
//
// The language only reads and writes them; whether one holds for a ticket
// is the store's to say ([`Facts::history`]), from the log and its times.

/// What a history predicate asks happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A transition into this status.
    Entered(Status),
    /// A transition out of this status.
    Left(Status),
    /// Any transition — of the status or of the kind (SDLC-13), a birth's
    /// included.
    Changed,
    /// FILTER-27: the ticket's birth (`created_unix`).
    Created,
    /// FILTER-27: how long it has held its current status.
    Held(Held),
    /// FILTER-30: the ticket's last touch — any write to its own row (title,
    /// body, status, kind, severity, parent, labels, blockers, commits), a
    /// comment left on it, or an answer to (or a clear of) one of its
    /// inputs. A roll-up or a count moving does not touch it.
    Touched,
    /// DEPLOY-11: `deployed <env>` — the ticket's work runs in an
    /// environment of that name (`prod`, of any project; `menger/prod`, of
    /// one), since a moment in the window when one is given. Answered from
    /// the deployments' kept cells, never from the transition log.
    Deployed(String),
    /// MENGER-39: `queued` — the ticket is in the owner's queue; `queued
    /// <urgency>`, at that urgency (a snooze does not take it out). Answered
    /// from the queue's kept entries; no window, no `by`.
    Queued(Option<crate::queue::Urgency>),
}

impl Event {
    /// The predicate's name: `entered`, `left`, `changed`, `created`,
    /// `for` (a `status … for …` is still `for`'s).
    pub const fn name(&self) -> &'static str {
        match self {
            Event::Entered(_) => "entered",
            Event::Left(_) => "left",
            Event::Changed => "changed",
            Event::Created => "created",
            Event::Touched => "touched",
            Event::Held(_) => "for",
            Event::Deployed(_) => "deployed",
            Event::Queued(_) => "queued",
        }
    }
}

/// FILTER-27: which current statuses a `for` asks of — any, or (the sugar)
/// those named, or those of the categories named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeldOf {
    Any,
    Statuses(Vec<Status>),
    Categories(Vec<Category>),
}

impl HeldOf {
    /// Whether a ticket in `s` is one it asks of.
    pub fn admits(&self, s: Status) -> bool {
        match self {
            HeldOf::Any => true,
            HeldOf::Statuses(v) => v.contains(&s),
            HeldOf::Categories(v) => v.contains(&category_of(s)),
        }
    }
}

/// FILTER-27: `[status S | category C] for over|under <length>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub of: HeldOf,
    /// `over` (held longer than the length) or `under` (not yet that long).
    pub over: bool,
    pub n: u32,
    pub unit: Unit,
}

impl Held {
    /// `over 2w`, `under 1h`.
    pub fn length(&self) -> String {
        format!("{} {}{}", if self.over { "over" } else { "under" }, self.n, self.unit.suffix())
    }

    /// The length in seconds.
    pub fn seconds(&self) -> i64 {
        i64::from(self.n) * self.unit.seconds()
    }
}

/// A unit of a length — `within`'s, `for`'s, an `ago`'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Minute,
    Hour,
    Day,
    Week,
    /// FILTER-27: 30 days.
    Month,
}

impl Unit {
    pub const ALL: [Unit; 5] = [Unit::Minute, Unit::Hour, Unit::Day, Unit::Week, Unit::Month];

    /// Its length in seconds — a month is 30 days.
    pub const fn seconds(self) -> i64 {
        match self {
            Unit::Minute => 60,
            Unit::Hour => 3_600,
            Unit::Day => 86_400,
            Unit::Week => 604_800,
            Unit::Month => 2_592_000,
        }
    }

    /// What a filter writes it with: `m`, `h`, `d`, `w`, `mo`.
    pub const fn suffix(self) -> &'static str {
        match self {
            Unit::Minute => "m",
            Unit::Hour => "h",
            Unit::Day => "d",
            Unit::Week => "w",
            Unit::Month => "mo",
        }
    }

    /// Its name in words, one or many: `day`, `days`.
    pub const fn word(self, many: bool) -> &'static str {
        match (self, many) {
            (Unit::Minute, false) => "minute",
            (Unit::Minute, true) => "minutes",
            (Unit::Hour, false) => "hour",
            (Unit::Hour, true) => "hours",
            (Unit::Day, false) => "day",
            (Unit::Day, true) => "days",
            (Unit::Week, false) => "week",
            (Unit::Week, true) => "weeks",
            (Unit::Month, false) => "month",
            (Unit::Month, true) => "months",
        }
    }

    pub fn read(s: &str) -> Option<Unit> {
        Some(match s.to_ascii_lowercase().as_str() {
            "m" | "min" | "mins" | "minute" | "minutes" => Unit::Minute,
            "h" | "hr" | "hrs" | "hour" | "hours" => Unit::Hour,
            "d" | "day" | "days" => Unit::Day,
            "w" | "wk" | "wks" | "week" | "weeks" => Unit::Week,
            "mo" | "mon" | "mos" | "month" | "months" => Unit::Month,
            _ => return None,
        })
    }
}

/// A moment a window names: a calendar date, and a time of day to the
/// minute when one is given — read in the filter's zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moment {
    pub year: i16,
    pub month: u8,
    pub day: u8,
    /// `(hour, minute)`; `None`: the date alone (its start, as `since` and
    /// `before` read it; its whole day, as the end of a `between`).
    pub time: Option<(u8, u8)>,
}

impl Moment {
    /// `2026-10-10`, or `2026-10-10T08:00`.
    pub fn format(&self) -> String {
        let date = format!("{:04}-{:02}-{:02}", self.year, self.month, self.day);
        match self.time {
            None => date,
            Some((h, m)) => format!("{date}T{h:02}:{m:02}"),
        }
    }

    /// A date (`YYYY-MM-DD`), or a date and a time joined by `T`.
    pub fn read(s: &str) -> Option<Moment> {
        let (date, time) = match s.split_once(['T', 't']) {
            Some((d, t)) => (d, Some(clock_time(t)?)),
            None => (s, None),
        };
        let mut it = date.split('-');
        let (y, m, d) = (it.next()?, it.next()?, it.next()?);
        if it.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
            return None;
        }
        let (year, month, day): (i16, u8, u8) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
        (year >= 1970 && (1..=days_in_month(year, month)?).contains(&day)).then_some(Moment { year, month, day, time })
    }
}

/// How many days `month` of `year` has — `None` for no month.
pub fn days_in_month(year: i16, month: u8) -> Option<u8> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => return None,
    })
}

/// FILTER-27: a moment as a window names it — on the calendar, or a length
/// back from now (`2w ago`), which moves with the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    At(Moment),
    Ago(u32, Unit),
}

impl When {
    /// `2026-10-10`, `2026-10-10T08:00`, `2w ago`.
    pub fn format(&self) -> String {
        match self {
            When::At(m) => m.format(),
            When::Ago(n, u) => format!("{n}{} ago", u.suffix()),
        }
    }

    /// Whether it moves with the clock.
    pub const fn relative(&self) -> bool {
        matches!(self, When::Ago(..))
    }
}

/// `HH:MM`, a time of day.
fn clock_time(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let (h, m): (u8, u8) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

/// When a history predicate looks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Window {
    /// The last `n` units, up to now.
    Within(u32, Unit),
    /// Since this day's start, in the filter's zone.
    Today,
    /// The day before today, whole.
    Yesterday,
    /// Since this week's Monday's start.
    ThisWeek,
    /// From this moment on (a date: its start).
    Since(When),
    /// Before this moment (a date: before its start).
    Before(When),
    /// From the first moment (a date: its start) to the second (a date: its
    /// end; a time or an `ago`: that instant, excluded).
    Between(When, When),
}

impl Window {
    /// The window as a filter writes it.
    pub fn format(&self) -> String {
        match self {
            Window::Within(n, u) => format!("within {n}{}", u.suffix()),
            Window::Today => "today".into(),
            Window::Yesterday => "yesterday".into(),
            Window::ThisWeek => "this week".into(),
            Window::Since(m) => format!("since {}", m.format()),
            Window::Before(m) => format!("before {}", m.format()),
            Window::Between(a, b) => format!("between {} and {}", a.format(), b.format()),
        }
    }

    /// Whether the window moves with the clock — what a `watch` reads it
    /// again for as time passes. `since`, `before` and `between` name fixed
    /// moments, unless one is an `ago` (FILTER-27).
    pub const fn relative(&self) -> bool {
        match self {
            Window::Within(..) | Window::Today | Window::Yesterday | Window::ThisWeek => true,
            Window::Since(w) | Window::Before(w) => w.relative(),
            Window::Between(a, b) => a.relative() || b.relative(),
        }
    }

    /// Whether the window names a calendar day or a moment — read in a zone
    /// (`within` and an `ago` are the same everywhere).
    pub const fn zoned(&self) -> bool {
        match self {
            Window::Within(..) => false,
            Window::Since(w) | Window::Before(w) => !w.relative(),
            Window::Between(a, b) => !a.relative() || !b.relative(),
            _ => true,
        }
    }
}

/// Who made a transition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Who {
    /// The caller — the principal the call is made as (`anonymous` with
    /// login off).
    Me,
    /// The store itself: a cascade, or its undoing.
    System,
    /// A principal, as the log names it (`idp:…`, `key:…`, `anonymous`).
    Principal(String),
}

impl Who {
    /// As a filter writes it: `me`, `system`, a principal (quoted when it
    /// must be).
    pub fn format(&self) -> String {
        match self {
            Who::Me => "me".into(),
            Who::System => "system".into(),
            Who::Principal(p) => quoted(p),
        }
    }

    /// `me`, `system` (any case), else a principal as written.
    pub fn read(s: &str) -> Who {
        match s.to_ascii_lowercase().as_str() {
            "me" => Who::Me,
            "system" => Who::System,
            _ => Who::Principal(s.to_string()),
        }
    }
}

/// MENGER-13: one history predicate — an event, the window it looks in, and
/// who made it. It holds for a ticket when some transition of the ticket's
/// is that event, in the window, by that actor. FILTER-27: `created` (its
/// birth in the window; no `by`) and `for` (no window, no `by`) too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    pub event: Event,
    pub window: Option<Window>,
    pub by: Option<Who>,
}

impl History {
    /// The predicate as a filter writes it — canonical, and the key the
    /// store's truths are named by ([`Facts::history`]): `entered "In
    /// Review" within 12h`, `changed today by system`, `created today`,
    /// `status Pending for over 2w`.
    pub fn format(&self) -> String {
        let mut out = match &self.event {
            Event::Entered(s) => format!("entered {}", quoted(s.as_str())),
            Event::Left(s) => format!("left {}", quoted(s.as_str())),
            Event::Changed => "changed".to_string(),
            Event::Created => "created".to_string(),
            Event::Touched => "touched".to_string(),
            Event::Deployed(env) => format!("deployed {}", quoted(env)),
            Event::Queued(None) => "queued".to_string(),
            Event::Queued(Some(u)) => format!("queued {u}"),
            Event::Held(h) => {
                let of = |name: &str, vs: Vec<String>| match vs.as_slice() {
                    [one] => format!("{name} {one} "),
                    _ => format!("{name} in ({}) ", vs.join(", ")),
                };
                let head = match &h.of {
                    HeldOf::Any => String::new(),
                    HeldOf::Statuses(v) => of("status", v.iter().map(|s| quoted(s.as_str())).collect()),
                    HeldOf::Categories(v) => of("category", v.iter().map(|c| c.as_str().to_string()).collect()),
                };
                format!("{head}for {}", h.length())
            }
        };
        if let Some(w) = &self.window {
            out.push(' ');
            out.push_str(&w.format());
        }
        if let Some(b) = &self.by {
            out.push_str(" by ");
            out.push_str(&b.format());
        }
        out
    }

    /// Whether what it holds for moves with the clock — a relative window,
    /// or a `for`, which every passing moment lengthens.
    pub fn relative(&self) -> bool {
        matches!(self.event, Event::Held(_)) || self.window.as_ref().is_some_and(Window::relative)
    }
}

/// `s`'s words, a `"…"` one word whatever it holds (its quotes off).
fn words(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        if let Some(inner) = rest.strip_prefix('"') {
            let end = inner.find('"').ok_or_else(|| QUOTES.to_string())?;
            out.push(inner[..end].to_string());
            rest = inner[end + 1..].trim_start();
            continue;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..end];
        if word.contains('"') {
            return Err(QUOTES.into());
        }
        out.push(word.to_string());
        rest = rest[end..].trim_start();
    }
    Ok(out)
}

const HISTORY_HELP: &str = "a history predicate is `entered <status>`, `left <status>`, `changed`, `by <who>`, \
                            `created`, `touched` or `deployed <environment>`, then a window — `within 12h` (m, h, d, w, mo), `today`, `yesterday`, `this \
                            week`, `since 2026-10-10`, `before 2w ago`, `between … and …` — and (not for `created` or `touched`) \
                            `by me`, `by system` or `by <principal>`; or `[status S | category C] for over|under 2w`";

/// One history predicate's words after its name ([`History`]), strictly:
/// every word read, or why not.
fn read_history(name: &str, tail: &str) -> Result<History, String> {
    let w = words(tail)?;
    let mut i = 0;
    let status = |w: &[String], i: &mut usize| -> Result<Status, String> {
        let one = w.get(*i).ok_or_else(|| format!("`{name}` names a status, as `{name} Coding`"))?;
        if let Ok(s) = one.parse::<Status>() {
            *i += 1;
            return Ok(s);
        }
        if let Some(two) = w.get(*i + 1).map(|t| format!("{one} {t}")) {
            if let Ok(s) = two.parse::<Status>() {
                *i += 2;
                return Ok(s);
            }
        }
        Err(format!(
            "unknown status `{one}` — the statuses are {}",
            Status::ALL.map(Status::as_str).join(", ")
        ))
    };
    let (event, mut by) = match name {
        "entered" => (Event::Entered(status(&w, &mut i)?), None),
        "left" => (Event::Left(status(&w, &mut i)?), None),
        "changed" => (Event::Changed, None),
        "created" => (Event::Created, None),
        "touched" => (Event::Touched, None),
        "deployed" => {
            let env = w.first().ok_or("`deployed` names an environment, as `deployed prod` or `deployed menger/prod`")?;
            i = 1;
            (Event::Deployed(deployed_env(env)?), None)
        }
        // MENGER-39: `queued [urgency]`, and nothing after it.
        "queued" => {
            let u = w.first().map(|u| u.parse::<crate::queue::Urgency>()).transpose()?;
            if let Some(extra) = w.get(usize::from(u.is_some())) {
                return Err(format!("`{extra}` is not understood here — `queued` takes an urgency (whenever, today, now, blocking) and nothing else: no window, no `by`"));
            }
            return Ok(History { event: Event::Queued(u), window: None, by: None });
        }
        _ => {
            let who = w.first().ok_or("`by` names who: `me`, `system` or a principal")?;
            i = 1;
            (Event::Changed, Some(Who::read(who)))
        }
    };
    let window = read_window(&w, &mut i)?;
    if matches!(event, Event::Created | Event::Touched) && window.is_none() {
        return Err(format!("`{name}` takes a window, as `{name} today` or `{name} within 3d`"));
    }
    if by.is_none() && w.get(i).is_some_and(|t| t.eq_ignore_ascii_case("by")) {
        if event == Event::Created {
            return Err("`created` takes no `by`: a birth before the log has no author on record".into());
        }
        if event == Event::Touched {
            return Err("`touched` takes no `by`: a ticket's last touch is kept without who made it".into());
        }
        if matches!(event, Event::Deployed(_)) {
            return Err("`deployed` takes no `by`: a deployment is a CI's, not a person's".into());
        }
        let who = w.get(i + 1).ok_or("`by` names who: `me`, `system` or a principal")?;
        by = Some(Who::read(who));
        i += 2;
    }
    if let Some(extra) = w.get(i) {
        return Err(format!("`{extra}` is not understood here — {HISTORY_HELP}"));
    }
    Ok(History { event, window, by })
}

/// DEPLOY-11: `deployed`'s environment — a name (`prod`, `review/pr-412`),
/// or a project's (`menger/prod`) — lower case, or why not.
fn deployed_env(raw: &str) -> Result<String, String> {
    crate::deployments::environment_of(raw)
}

/// A length at `w[*i]` — `2w`, `2 w`, `2 weeks`, `1mo` — as `(n, unit)`.
fn read_length(w: &[String], i: &mut usize, what: &str) -> Result<(u32, Unit), String> {
    let raw = w.get(*i).ok_or_else(|| format!("`{what}` takes a length, as `{what} 12h`"))?;
    let digits = raw.bytes().take_while(u8::is_ascii_digit).count();
    let (n, unit, used) = match &raw[digits..] {
        "" => (&raw[..digits], w.get(*i + 1).and_then(|u| Unit::read(u)), 2),
        u => (&raw[..digits], Unit::read(u), 1),
    };
    let n: u32 = n
        .parse()
        .ok()
        .filter(|n| (1..=100_000).contains(n))
        .ok_or_else(|| format!("`{what}` takes a length of 1 or more, as `{what} 12h` — not `{raw}`"))?;
    let unit = unit.ok_or_else(|| format!("`{what} {raw}` needs a unit: m, h, d, w or mo"))?;
    *i += used;
    Ok((n, unit))
}

/// Whether `w[i..]` starts with a length then `ago`, and how many words
/// that is.
fn ago_at(w: &[String], i: usize) -> Option<usize> {
    let raw = w.get(i)?;
    let digits = raw.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let used = match &raw[digits..] {
        "" => {
            Unit::read(w.get(i + 1)?)?;
            2
        }
        u => {
            Unit::read(u)?;
            1
        }
    };
    w.get(i + used).is_some_and(|a| a.eq_ignore_ascii_case("ago")).then_some(used + 1)
}

/// A moment at `w[*i]`: a date, a date and a time (joined by `T` or
/// written apart), or a length `ago`.
fn read_when(w: &[String], i: &mut usize, what: &str) -> Result<When, String> {
    let raw = w
        .get(*i)
        .ok_or_else(|| format!("`{what}` names a moment, as `{what} 2026-10-10` or `{what} 2w ago`"))?;
    if ago_at(w, *i).is_some() {
        let (n, unit) = read_length(w, i, "ago")?;
        *i += 1; // `ago`
        return Ok(When::Ago(n, unit));
    }
    // A date and a time written apart: `2026-10-10 08:00`.
    if let Some(t) = w.get(*i + 1).filter(|t| clock_time(t).is_some() && !raw.contains(['T', 't'])) {
        if let Some(m) = Moment::read(&format!("{raw}T{t}")) {
            *i += 2;
            return Ok(When::At(m));
        }
    }
    let m = Moment::read(raw).ok_or_else(|| {
        format!("`{raw}` is not a date — a date is `2026-10-10`, a moment `2026-10-10T08:00` or `2w ago`")
    })?;
    *i += 1;
    Ok(When::At(m))
}

/// A window at `w[*i]`, if one starts there.
fn read_window(w: &[String], i: &mut usize) -> Result<Option<Window>, String> {
    let Some(head) = w.get(*i) else { return Ok(None) };
    let window = match head.to_ascii_lowercase().as_str() {
        "within" => {
            *i += 1;
            let (n, unit) = read_length(w, i, "within")?;
            Window::Within(n, unit)
        }
        "today" => {
            *i += 1;
            Window::Today
        }
        "yesterday" => {
            *i += 1;
            Window::Yesterday
        }
        "this" => {
            if !w.get(*i + 1).is_some_and(|t| t.eq_ignore_ascii_case("week")) {
                return Err("`this` is `this week`".into());
            }
            *i += 2;
            Window::ThisWeek
        }
        "since" => {
            *i += 1;
            Window::Since(read_when(w, i, "since")?)
        }
        "before" => {
            *i += 1;
            Window::Before(read_when(w, i, "before")?)
        }
        "between" => {
            *i += 1;
            let a = read_when(w, i, "between")?;
            if !w.get(*i).is_some_and(|t| t.eq_ignore_ascii_case("and")) {
                return Err("`between` is `between <date> and <date>`".into());
            }
            *i += 1;
            let b = read_when(w, i, "between … and")?;
            Window::Between(a, b)
        }
        _ => return Ok(None),
    };
    Ok(Some(window))
}

/// FILTER-27: where `rest` holds `for over|under <length>` at its top level
/// (outside quotes and parentheses), the byte offset of that `for` — the
/// sugar's split between its head (`status S`, `category C`, a list, or
/// nothing) and its length.
fn held_at(rest: &str) -> Option<usize> {
    let b = rest.as_bytes();
    let (mut depth, mut quoted) = (0usize, false);
    for (i, &c) in b.iter().enumerate() {
        if quoted {
            quoted = c != b'"';
            continue;
        }
        match c {
            b'"' => quoted = true,
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        let starts = i == 0 || b[i - 1].is_ascii_whitespace();
        if depth == 0 && starts && b.len() >= i + 4 && b[i..i + 3].eq_ignore_ascii_case(b"for") && b[i + 3].is_ascii_whitespace() {
            let after = rest[i + 3..].trim_start();
            let word = after.split_ascii_whitespace().next().unwrap_or("");
            if word.eq_ignore_ascii_case("over") || word.eq_ignore_ascii_case("under") {
                return Some(i);
            }
        }
    }
    None
}

/// FILTER-27: `[status S | status in (…) | category C | category in (…)]
/// for over|under <length>` — one predicate.
fn read_held(head: &str, tail: &str) -> Result<History, String> {
    let w = words(tail)?;
    let over = match w.first().map(|s| s.to_ascii_lowercase()) {
        Some(o) if o == "over" => true,
        Some(u) if u == "under" => false,
        _ => return Err("`for` is `for over <length>` or `for under <length>`".into()),
    };
    let mut i = 1;
    let (n, unit) = read_length(&w, &mut i, if over { "for over" } else { "for under" })?;
    if let Some(extra) = w.get(i) {
        return Err(format!("`{extra}` is not understood here — `for over 2w` takes no window and no `by`"));
    }
    let of = held_of(head.trim())?;
    Ok(History { event: Event::Held(Held { of, over, n, unit }), window: None, by: None })
}

/// The head of a `for`: nothing, or the statuses or categories it asks of.
fn held_of(head: &str) -> Result<HeldOf, String> {
    if head.is_empty() {
        return Ok(HeldOf::Any);
    }
    const HEAD: &str = "before `for` stands `status S`, `status in (…)`, `category C` or `category in (…)` — or nothing";
    let (name, tail) = split_field(head).filter(|(n, _)| matches!(*n, "status" | "category")).ok_or(HEAD)?;
    // `in (a, b)`, or the bare list `(a, b)`.
    let list = match tail.as_bytes() {
        [b'(', ..] => Some(tail),
        [i, n, ..] if [*i, *n].eq_ignore_ascii_case(b"in") && tail[2..].trim_start().starts_with('(') => {
            Some(tail[2..].trim_start())
        }
        _ => None,
    };
    let values: Vec<&str> = match list {
        Some(l) => parts(split_top(wrapped(l).ok_or(HEAD)?, ",")),
        None => vec![tail],
    };
    if values.is_empty() || values.iter().any(|v| v.is_empty()) {
        return Err(HEAD.into());
    }
    let read = |v: &str| read_field(&format!("{name} {v}"));
    let mut statuses = Vec::new();
    let mut categories = Vec::new();
    for v in values {
        match read(v)? {
            Field::Status(s) if !statuses.contains(&s) => statuses.push(s),
            Field::Category(c) if !categories.contains(&c) => categories.push(c),
            _ => {}
        }
    }
    Ok(match name {
        "status" => HeldOf::Statuses(statuses),
        _ => HeldOf::Categories(categories),
    })
}

// ---------------------------------------------------------------------
// The evaluator's rules

/// find's rule (SCALE-6, the palette's `palette_match`): the text, trimmed,
/// is a substring of the id or of the title, ASCII case folded; a blank
/// text matches everything.
fn text_matches(query: &str, id: &str, title: &str) -> bool {
    let q = query.trim();
    q.is_empty() || contains_folded(id, q) || contains_folded(title, q)
}

fn contains_folded(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

// ---------------------------------------------------------------------
// `and`: merging lists

/// A group that lists values of one field: an inclusion (any of them) or an
/// exclusion (none of them). One predicate is a list of one.
struct List {
    incl: bool,
    values: Vec<Field>,
}

impl List {
    fn of(g: &Group) -> Option<List> {
        let first = g.preds.first()?;
        let same = g.preds.iter().all(|p| p.field.name() == first.field.name() && p.not == first.not);
        let joined = g.preds.len() == 1 || g.conj == if first.not { Conj::All } else { Conj::Any };
        if !(first.field.listable() && same && joined) {
            return None;
        }
        let mut values: Vec<Field> = Vec::new();
        for p in &g.preds {
            if !values.contains(&p.field) {
                values.push(p.field.clone());
            }
        }
        Some(List { incl: !first.not, values })
    }

    /// The one list both say, if it holds a value and the field allows it.
    fn merge(&self, other: &List) -> Option<List> {
        let (a, b) = (&self.values[0], &other.values[0]);
        if a.name() != b.name() {
            return None;
        }
        let single = a.single_valued();
        let (incl, values): (bool, Vec<Field>) = match (self.incl, other.incl) {
            (true, true) if single => (true, self.values.iter().filter(|v| other.values.contains(v)).cloned().collect()),
            (false, false) => {
                let more = other.values.iter().filter(|v| !self.values.contains(v));
                (false, self.values.iter().chain(more).cloned().collect())
            }
            (true, false) if single => (true, self.values.iter().filter(|v| !other.values.contains(v)).cloned().collect()),
            (false, true) if single => (true, other.values.iter().filter(|v| !self.values.contains(v)).cloned().collect()),
            _ => return None,
        };
        (!values.is_empty()).then_some(List { incl, values })
    }

    fn group(self) -> Group {
        let preds = self.values.into_iter().map(|field| Predicate { field, not: !self.incl }).collect();
        match self.incl {
            true => Group::any(preds),
            false => Group::all(preds),
        }
    }
}

/// `g` into `out`: dropped when it is there already, merged into the first
/// list of its field it merges with, else added.
fn absorb(out: &mut Vec<Group>, g: Group) {
    if out.contains(&g) {
        return;
    }
    if let Some(list) = List::of(&g) {
        for i in 0..out.len() {
            let Some(merged) = List::of(&out[i]).and_then(|held| held.merge(&list)) else { continue };
            let merged = merged.group();
            if out.iter().enumerate().any(|(j, h)| j != i && *h == merged) {
                out.remove(i);
            } else {
                out[i] = merged;
            }
            return;
        }
    }
    out.push(g);
}

// ---------------------------------------------------------------------
// `admits_nothing`: the closed domain

/// One cell of the closed domain (FILTER-3 §6.1). FILTER-21: and whether it
/// is one of menger's own documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    status: Status,
    kind: Kind,
    severity: Option<Severity>,
    blocked: bool,
    commits: bool,
    menger: bool,
    /// MENGER-12: it has an open input.
    asking: bool,
}

const SEVERITIES: [Option<Severity>; 5] =
    [None, Some(Severity::Critical), Some(Severity::High), Some(Severity::Medium), Some(Severity::Low)];

/// Every cell a ticket can be in: status × kind × severity-or-none ×
/// blocked × has commits × menger doc, where a doc holds `Doc` and only a
/// doc does (SDLC-11), and only a doc carries a path (WAVE-10), so only a
/// doc is one of menger's own — 940 of them; MENGER-12: × asking, any
/// ticket may ask (a doc's body may declare an input) — 1,880.
fn cells() -> Vec<Cell> {
    let mut out = Vec::with_capacity(1880);
    for status in Status::ALL {
        for kind in Kind::ALL {
            if (status == Status::Doc) != (kind == Kind::Doc) {
                continue;
            }
            let mengers: &[bool] = if kind == Kind::Doc { &[false, true] } else { &[false] };
            for severity in SEVERITIES {
                for blocked in [false, true] {
                    for commits in [false, true] {
                        for &menger in mengers {
                            for asking in [false, true] {
                                out.push(Cell { status, kind, severity, blocked, commits, menger, asking });
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// What a predicate reads, for telling independent groups apart: a cell's
/// coordinates — its state (status and kind, which a doc ties together;
/// FILTER-21: and `menger doc`, which only a doc is), blocked, severity,
/// commits — the shortcode, or a free truth (an `under` name, a text). A
/// blank text reads nothing: it is always true.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Dim {
    State,
    Blocked,
    Severity,
    Commits,
    /// MENGER-12: whether it has an open input.
    Asking,
    Epic,
    Under(String),
    Text(String),
    /// MENGER-12: whether it holds this label — a free truth, as an
    /// `under` name: a ticket holds any set of labels.
    Label(String),
    /// MENGER-13: a history predicate's truth — free, as the log is.
    History(String),
    /// FILTER-36: an unresolved `saved` name — free, as nothing says what
    /// it holds (its slug).
    Saved(String),
}

fn dims(field: &Field) -> Vec<Dim> {
    match field {
        Field::Status(_) | Field::Category(_) | Field::Kind(_) | Field::MengerDoc => vec![Dim::State],
        Field::Ready => vec![Dim::State, Dim::Blocked],
        Field::Blocked => vec![Dim::Blocked],
        Field::Severity(_) => vec![Dim::Severity],
        Field::HasCommits => vec![Dim::Commits],
        Field::Asking => vec![Dim::Asking],
        Field::In(_) => vec![Dim::Epic],
        Field::Under(_) | Field::Text(_) | Field::Label(_) | Field::History(_) | Field::Saved(_) => {
            var(field).into_iter().collect()
        }
        // FILTER-36: what its leaves read.
        Field::Group(g) => g.leaves().into_iter().flat_map(|p| dims(&p.field)).collect(),
    }
}

/// The free truth an `under`, a `text` or (MENGER-13) a history predicate
/// reads — `under`'s name upper-cased, a text trimmed and folded as find
/// folds it, a history predicate's canonical text; `None` for a blank
/// text, which is always true.
fn var(field: &Field) -> Option<Dim> {
    match field {
        Field::Under(x) => Some(Dim::Under(x.to_ascii_uppercase())),
        Field::Label(l) => Some(Dim::Label(l.to_ascii_lowercase())),
        Field::Text(t) => {
            let key = t.trim().to_ascii_lowercase();
            (!key.is_empty()).then_some(Dim::Text(key))
        }
        Field::History(h) => Some(Dim::History(h.format())),
        Field::Saved(n) => Some(Dim::Saved(crate::saved_filters::slug(n))),
        _ => None,
    }
}

/// Past this many worlds a set of groups is not enumerated, and is taken
/// to admit a ticket — never a false contradiction.
const WORLD_LIMIT: usize = 1 << 20;

/// Among `groups[subset]`, a set that reads something in common and admits
/// nothing — `None` when they admit some ticket. Groups that read nothing
/// in common are independent, so each such component is decided alone.
fn clash(groups: &[Group], subset: &[usize]) -> Option<Vec<usize>> {
    let read: Vec<Vec<Dim>> =
        subset.iter().map(|&i| groups[i].leaves().into_iter().flat_map(|p| dims(&p.field)).collect()).collect();
    let mut root: Vec<usize> = (0..subset.len()).collect();
    fn find(root: &mut [usize], mut x: usize) -> usize {
        while root[x] != x {
            root[x] = root[root[x]];
            x = root[x];
        }
        x
    }
    for a in 0..subset.len() {
        for b in a + 1..subset.len() {
            if read[a].iter().any(|d| read[b].contains(d)) {
                let (ra, rb) = (find(&mut root, a), find(&mut root, b));
                root[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    let mut components: Vec<(usize, Vec<usize>)> = Vec::new();
    for (k, &i) in subset.iter().enumerate() {
        let r = find(&mut root, k);
        match components.iter_mut().find(|(cr, _)| *cr == r) {
            Some((_, members)) => members.push(i),
            None => components.push((r, vec![i])),
        }
    }
    components
        .into_iter()
        .map(|(_, members)| members)
        .find(|members| !satisfiable(&members.iter().map(|&i| &groups[i]).collect::<Vec<_>>()))
}

/// A predicate compiled against one component's worlds.
enum Lit<'a> {
    Cell(&'a Field),
    /// The world's shortcode is the `n`th the groups name.
    Epic(usize),
    /// The `n`th free truth.
    Var(usize),
    Always,
    /// FILTER-36: a nested group, its items compiled.
    Tree(Conj, Vec<(Lit<'a>, bool)>),
}

/// Whether some world of the closed domain passes every one of `groups`.
/// FILTER-36: a nested group is judged as its parent is, deeper.
fn satisfiable(groups: &[&Group]) -> bool {
    let (mut cell, mut epics, mut vars) = (false, Vec::<String>::new(), Vec::<Dim>::new());
    for p in groups.iter().flat_map(|g| g.leaves()) {
        match &p.field {
            Field::In(e) => {
                let e = e.to_ascii_uppercase();
                if !epics.contains(&e) {
                    epics.push(e);
                }
            }
            f @ (Field::Under(_) | Field::Text(_) | Field::Label(_) | Field::History(_) | Field::Saved(_)) => {
                if let Some(v) = var(f).filter(|v| !vars.contains(v)) {
                    vars.push(v);
                }
            }
            _ => cell = true,
        }
    }
    let cells = match cell {
        true => cells(),
        // Nothing reads the cell: any one stands for all.
        false => vec![Cell {
            status: Status::Pending,
            kind: Kind::Build,
            severity: None,
            blocked: false,
            commits: false,
            menger: false,
            asking: false,
        }],
    };
    if vars.len() > 20 || cells.len().saturating_mul(epics.len() + 1).saturating_mul(1 << vars.len()) > WORLD_LIMIT {
        return true;
    }
    fn compile<'a>(g: &'a Group, epics: &[String], vars: &[Dim]) -> Vec<(Lit<'a>, bool)> {
        g.preds
            .iter()
            .map(|p| {
                let lit = match &p.field {
                    Field::In(e) => Lit::Epic(epics.iter().position(|x| x.eq_ignore_ascii_case(e)).unwrap_or(usize::MAX)),
                    f @ (Field::Under(_) | Field::Text(_) | Field::Label(_) | Field::History(_) | Field::Saved(_)) => {
                        match var(f) {
                            Some(v) => Lit::Var(vars.iter().position(|x| *x == v).unwrap_or(usize::MAX)),
                            None => Lit::Always,
                        }
                    }
                    Field::Group(inner) => Lit::Tree(inner.conj, compile(inner, epics, vars)),
                    f => Lit::Cell(f),
                };
                (lit, p.not)
            })
            .collect()
    }
    let compiled: Vec<(Conj, Vec<(Lit, bool)>)> = groups.iter().map(|g| (g.conj, compile(g, &epics, &vars))).collect();
    fn passes(conj: Conj, lits: &[(Lit, bool)], c: &Cell, epic: usize, truths: usize) -> bool {
        let test = |(lit, not): &(Lit, bool)| {
            let hit = match lit {
                Lit::Cell(f) => f.on_cell(c).unwrap_or(false),
                Lit::Epic(n) => epic == *n,
                Lit::Var(n) => truths >> n & 1 == 1,
                Lit::Always => true,
                Lit::Tree(conj, inner) => passes(*conj, inner, c, epic, truths),
            };
            hit != *not
        };
        match conj {
            _ if lits.is_empty() => true,
            Conj::Any => lits.iter().any(test),
            Conj::All => lits.iter().all(test),
        }
    }
    let holds = |c: &Cell, epic: usize, truths: usize| compiled.iter().all(|(conj, lits)| passes(*conj, lits, c, epic, truths));
    cells.iter().any(|c| (0..=epics.len()).any(|e| (0..1usize << vars.len()).any(|t| holds(c, e, t))))
}

#[cfg(test)]
#[path = "filter_tests.rs"]
mod tests;
