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
//!   predicates joined all by `or` (any) or all by `and` (all). As in
//!   manifest, a chunk `a or b` without parentheses is an `or` group too.
//! - **A predicate** is `not` (any number; they toggle) before one of
//!   `status <name>` · `category <open|ready|active|review|done|dropped|static>`
//!   · `kind <name>` · `severity <critical|high|medium|low|none>` ·
//!   `blocked` · `ready` · `has commits` · `menger doc` (FILTER-21) ·
//!   `in <SHORTCODE>` (and `in (A, B)`, `not in (A, B)`) ·
//!   `under <ID or SHORTCODE>` · `text "…"`, or a bare `"…"`. A value
//!   holding spaces, commas or parentheses is quoted with `"`; the language
//!   has no escape, so no value holds a `"`.
//! - **`text`** is tested on rows, so it stands only as a top-level `and`
//!   group of its own: never inside `or`, under `not`, or in parentheses
//!   with other predicates.
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
//! case. [`Filter::parse`] produces only filters of that shape.
//!
//! On top of the grammar: [`Filter::and`], the lens's aggregation (a level's
//! filter under the global one), [`Filter::admits_nothing`], the
//! contradiction a combined filter can fall into, and [`Filter::matches`],
//! the reference evaluator every compiled form is held equal to.
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
    /// `in <SHORTCODE>`: its shortcode is the one named (upper case).
    In(String),
    /// `under <ID or SHORTCODE>`: the one named is in its lineage — it is
    /// that ticket or below it, or in that shortcode or one hosted under it.
    Under(String),
    /// `text "…"`: its id or title contains the text — find's rule
    /// (SCALE-6): trimmed, ASCII case folded; a blank text matches all.
    Text(String),
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
            Field::In(_) => "in",
            Field::Under(_) => "under",
            Field::Text(_) => "text",
        }
    }

    /// The value as a filter names it, unquoted (`In Review`, `none`,
    /// `SCALE`); `None` for a flag.
    pub fn value(&self) -> Option<String> {
        Some(match self {
            Field::Status(s) => s.as_str().into(),
            Field::Category(c) => c.as_str().into(),
            Field::Kind(k) => k.as_str().into(),
            Field::Severity(s) => severity_name(*s).into(),
            Field::Blocked | Field::Ready | Field::HasCommits | Field::MengerDoc => return None,
            Field::In(x) | Field::Under(x) | Field::Text(x) => x.clone(),
        })
    }

    /// Whether a group may list this field's values (`field in (a, b)`).
    const fn listable(&self) -> bool {
        matches!(
            self,
            Field::Status(_) | Field::Category(_) | Field::Kind(_) | Field::Severity(_) | Field::In(_) | Field::Under(_)
        )
    }

    /// Whether a ticket holds exactly one value of this field — what lets
    /// two inclusions intersect and an exclusion subtract. `under` is the
    /// one listable field that does not: a lineage holds many names.
    const fn single_valued(&self) -> bool {
        self.listable() && !matches!(self, Field::Under(_))
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
            Field::In(_) | Field::Under(_) | Field::Text(_) => return None,
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
            Field::Blocked | Field::Ready | Field::HasCommits | Field::MengerDoc => format!("{head}{}", self.field.name()),
            Field::Text(t) => format!("{head}text \"{t}\""),
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
    fn of(conj: Conj, preds: Vec<Predicate>) -> Self {
        let conj = if preds.len() == 1 { Conj::All } else { conj };
        Self { conj, preds }
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
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not understood: ")?;
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
    /// What `under` finds it under: its own id, its ancestors' ids, its
    /// shortcode and the shortcodes hosting it.
    pub lineage: Vec<String>,
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
        for chunk in parts(split_top(filter_text, " and ")) {
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
            false => Err(FilterError { chunks: bad }),
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
const NAMES: [&str; 22] = [
    "has commits",
    "menger doc",
    "discussion",
    "unattested",
    "category",
    "severity",
    "past cap",
    "archived",
    "blocked",
    "session",
    "created",
    "written",
    "status",
    "lowest",
    "under",
    "ready",
    "depth",
    "kind",
    "text",
    "tag",
    "age",
    "in",
];

const TEXT_ALONE: &str = "`text` is tested on rows, so it stands alone as a top-level `and` group — never \
                          inside `or`, under `not`, or in parentheses with other predicates";
const MIXED: &str = "`or` and `and` do not mix inside one pair of parentheses: a group is all `or` or all `and`";
const NESTED: &str = "groups do not nest: parentheses hold predicates, not groups";
const LIST_NESTED: &str = "a list `field in (…)` is a group of its own, never inside parentheses or `or`";
const NOT_UNDERSTOOD: &str = "not understood: a predicate is `status`, `category`, `kind`, `severity`, `blocked`, \
                              `ready`, `has commits`, `menger doc`, `in`, `under` or `text`, or a quoted text";
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
    if rest.starts_with('(') {
        return Err(NESTED.into());
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
            match !v.is_empty() && v.bytes().all(|b| b.is_ascii_alphanumeric()) {
                true => Ok(Field::In(v.to_ascii_uppercase())),
                false => Err(format!("`in` takes a shortcode, as `in SCALE` — not `{v}`")),
            }
        }
        "under" => {
            let v = value()?;
            let ok = v.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
                && v.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
            match ok {
                true => Ok(Field::Under(v.to_ascii_uppercase())),
                false => Err(format!("`under` takes an id or a shortcode, as `under SCALE-13` — not `{v}`")),
            }
        }
        "created" | "written" | "age" => Err(format!(
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
    let alone = preds.len() == 1;
    if preds.iter().any(|p| matches!(p.field, Field::Text(_)) && (p.not || !alone)) {
        whys.push(TEXT_ALONE.into());
    }
    match whys.is_empty() {
        true => Ok(Group::of(conj, preds)),
        false => Err(whys.join("; ")),
    }
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

/// The predicates of an `or` or an `and` inside a group.
fn members<'a>(raws: &[&'a str]) -> Vec<(&'a str, Result<Predicate, String>)> {
    raws.iter()
        .map(|&r| match list_group(r) {
            Some(_) => (r, Err(LIST_NESTED.to_string())),
            None => (r, parse_predicate(r)),
        })
        .collect()
}

/// One `and`-joined chunk (manifest's `parseGroup`, strict).
fn parse_group(s: &str) -> Result<Group, String> {
    if let Some(inner) = wrapped(s) {
        let ors = parts(split_top(inner, " or "));
        if ors.len() > 1 {
            if ors.iter().any(|o| parts(split_top(o, " and ")).len() > 1) {
                return Err(MIXED.into());
            }
            return group(Conj::Any, members(&ors));
        }
        return group(Conj::All, members(&parts(split_top(inner, " and "))));
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
}

const SEVERITIES: [Option<Severity>; 5] =
    [None, Some(Severity::Critical), Some(Severity::High), Some(Severity::Medium), Some(Severity::Low)];

/// Every cell a ticket can be in: status × kind × severity-or-none ×
/// blocked × has commits × menger doc, where a doc holds `Doc` and only a
/// doc does (SDLC-11), and only a doc carries a path (WAVE-10), so only a
/// doc is one of menger's own — 940 of them.
fn cells() -> Vec<Cell> {
    let mut out = Vec::with_capacity(940);
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
                            out.push(Cell { status, kind, severity, blocked, commits, menger });
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
    Epic,
    Under(String),
    Text(String),
}

fn dims(field: &Field) -> Vec<Dim> {
    match field {
        Field::Status(_) | Field::Category(_) | Field::Kind(_) | Field::MengerDoc => vec![Dim::State],
        Field::Ready => vec![Dim::State, Dim::Blocked],
        Field::Blocked => vec![Dim::Blocked],
        Field::Severity(_) => vec![Dim::Severity],
        Field::HasCommits => vec![Dim::Commits],
        Field::In(_) => vec![Dim::Epic],
        Field::Under(_) | Field::Text(_) => var(field).into_iter().collect(),
    }
}

/// The free truth an `under` or a `text` predicate reads — `under`'s name
/// upper-cased, a text trimmed and folded as find folds it; `None` for a
/// blank text, which is always true.
fn var(field: &Field) -> Option<Dim> {
    match field {
        Field::Under(x) => Some(Dim::Under(x.to_ascii_uppercase())),
        Field::Text(t) => {
            let key = t.trim().to_ascii_lowercase();
            (!key.is_empty()).then_some(Dim::Text(key))
        }
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
        subset.iter().map(|&i| groups[i].preds.iter().flat_map(|p| dims(&p.field)).collect()).collect();
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
}

/// Whether some world of the closed domain passes every one of `groups`.
fn satisfiable(groups: &[&Group]) -> bool {
    let (mut cell, mut epics, mut vars) = (false, Vec::<String>::new(), Vec::<Dim>::new());
    for p in groups.iter().flat_map(|g| &g.preds) {
        match &p.field {
            Field::In(e) => {
                let e = e.to_ascii_uppercase();
                if !epics.contains(&e) {
                    epics.push(e);
                }
            }
            f @ (Field::Under(_) | Field::Text(_)) => {
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
        false => vec![Cell { status: Status::Pending, kind: Kind::Build, severity: None, blocked: false, commits: false, menger: false }],
    };
    if vars.len() > 20 || cells.len().saturating_mul(epics.len() + 1).saturating_mul(1 << vars.len()) > WORLD_LIMIT {
        return true;
    }
    let compiled: Vec<(Conj, Vec<(Lit, bool)>)> = groups
        .iter()
        .map(|g| {
            let lits = g
                .preds
                .iter()
                .map(|p| {
                    let lit = match &p.field {
                        Field::In(e) => {
                            Lit::Epic(epics.iter().position(|x| x.eq_ignore_ascii_case(e)).unwrap_or(usize::MAX))
                        }
                        f @ (Field::Under(_) | Field::Text(_)) => match var(f) {
                            Some(v) => Lit::Var(vars.iter().position(|x| *x == v).unwrap_or(usize::MAX)),
                            None => Lit::Always,
                        },
                        f => Lit::Cell(f),
                    };
                    (lit, p.not)
                })
                .collect();
            (g.conj, lits)
        })
        .collect();
    let holds = |c: &Cell, epic: usize, truths: usize| {
        compiled.iter().all(|(conj, lits)| {
            let test = |(lit, not): &(Lit, bool)| {
                let hit = match lit {
                    Lit::Cell(f) => f.on_cell(c).unwrap_or(false),
                    Lit::Epic(n) => epic == *n,
                    Lit::Var(n) => truths >> n & 1 == 1,
                    Lit::Always => true,
                };
                hit != *not
            };
            match conj {
                _ if lits.is_empty() => true,
                Conj::Any => lits.iter().any(test),
                Conj::All => lits.iter().all(test),
            }
        })
    };
    cells.iter().any(|c| (0..=epics.len()).any(|e| (0..1usize << vars.len()).any(|t| holds(c, e, t))))
}

#[cfg(test)]
#[path = "filter_tests.rs"]
mod tests;
