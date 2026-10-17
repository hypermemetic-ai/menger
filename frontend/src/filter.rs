//! FILTER-11: the generic filter bar — WAVE-12's menu, made one component
//! that renders any level's filter (src/lens.rs), configured with the
//! level's categories and given a counts provider.
//!
//! - **The categories** ([`Cat`]): the server-backed ones, offered at every
//!   level — Shortcode (the hosting tree, tri-state), Status (grouped, most
//!   important first, its groups as headings), Kind,
//!   Severity, and the flags Ready, Blocked, Commits, Menger docs (yes · no;
//!   FILTER-21's `menger doc`, which the nav's default excludes) — each a
//!   group of FILTER-10's language; and a surface's own (local) ones, which
//!   the language has not got: the wave board's Wave, Readiness, Off-board
//!   blockers and Title (WAVE-12, FILTER-9 decision 6); the inputs page's
//!   State and Removed (FILTER-16).
//! - **A pill** ([`Pill`]) is a category's clause in words — where a
//!   filter is shown rather than edited (the board's *Nothing matches*, a
//!   ticket outside the global filter); a surface's own clauses are pills
//!   under its bar ([`Clause`]: an operator and canonical values; a flag's
//!   is one value, `is yes` or `is no`).
//! - **The bar** ([`FilterBar`]): one line — the `Filter` button, the
//!   chevron (the panel open ⇄ folded), the query's **name** (a name saves
//!   it, src/naming.rs), the canonical text (on a surface the global
//!   filter's before it, muted), `text`, `reset`, Make global (the nav) and
//!   `Saved ▾`; under it the surface's own clauses and, while there is one,
//!   "Nothing can match" with the clashing groups.
//! - **The panel** ([`Panel`]; FILTER-39, FILTER-38's mockups): the menu,
//!   kept open **in the page's flow** under the bar — the content moves
//!   down under it, no overlay. Its head: the level, a search over every
//!   value (Enter ticks the first it leaves), ×. Then the filter as
//!   **groups joined by an explicit `or`, clauses inside a group joined by
//!   `and`** (src/ors.rs; the owner, 2026-10-17: "ors should be explicit
//!   not any of") — the selected group lit, × on a clause and a group, `+
//!   or`. Then every category a section ([`sections`]: the saved filters,
//!   Shortcode as its tree, Status, Kind, Severity, the flags together,
//!   Label, When, Deployed, Chats, a surface's own), flowing into as many
//!   columns as the width holds: values with their counts (the level's
//!   [`CountsProvider`]) and FILTER-19's `is / is not` in the head. A tick
//!   goes into the selected group — a second value of one category is an
//!   alternative, so a group of its own, and the panel says so; hovering a
//!   value says what ticking it would leave. `f`, the Filter button and the
//!   chevron open and fold it; Escape folds it.

//! Everything that decides — the categories and their values, a clause's
//! edits, the shortcode tree, the panel's sections and rows, a pill's
//! words — is a pure function here, pinned by native tests; the pills ⇄
//! groups of the language are src/lens.rs's, the groups joined by `or`
//! src/ors.rs's.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use leptos::prelude::*;
use menger_tickets_api::{Group, Status};
use wasm_bindgen::JsCast;

use crate::bindings::{self, Context};
use crate::keys::palette_match;
use crate::lens::{self, Lens, Level};
use crate::nav::kind_glyph;
use crate::waves::epic_of;
use crate::{dom, keys, st_class};

// ---------------------------------------------------------------------
// The categories

/// Every category a bar can offer — the server-backed ones (the language's,
/// at every level) first, then the wave board's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Cat {
    Shortcode,
    Status,
    Kind,
    Severity,
    Ready,
    Blocked,
    Commits,
    /// FILTER-21: menger's own documents (`_MENGER/` paths) — `menger doc`.
    MengerDocs,
    /// MENGER-12: the ticket has an open input — `asking`.
    Asking,
    /// MENGER-12: the ticket's labels — `label <name>`; its values are the
    /// labels `facets` counts (and the clause's, and `needs-you`).
    Label,
    /// FILTER-27: when — created, moved into or out of a status, held in it
    /// for a length, changed (MENGER-13's Changed recently folded in): each
    /// clause its own pill ([`Pill::Time`], src/when.rs; uncounted).
    When,
    /// DEPLOY-13: where the work runs — `deployed <env> [window]`, `not
    /// deployed <env>`: time pills too (src/when.rs), built from the
    /// environments the app holds (src/deploys.rs); uncounted.
    Deployed,
    /// CHAT-4: the reader's chats — `chatting` (in an open chat on the
    /// ticket), `unread` (unread there): pills of When's kind
    /// (src/when.rs), built from this category's own pane; uncounted.
    Chats,
    Wave,
    Readiness,
    OffBoard,
    Title,
    /// FILTER-16: the inputs page's — an input's state (open · answered ·
    /// closed), and whether the removed show.
    State,
    Removed,
}

/// The kinds of work, in the menu's order — the default three first. Docs
/// are never cards (WAVE-1 decision 14).
pub(crate) const WORK_KINDS: [&str; 5] = ["build", "spike", "gap", "scope", "execution"];

/// Every kind, in the menu's order.
pub(crate) const KINDS: [&str; 6] = ["build", "spike", "gap", "scope", "execution", "doc"];

/// The kinds a wave board opens on (WAVE-1 decision 14; `waves`' own default).
pub(crate) const DEFAULT_KINDS: [&str; 3] = ["build", "spike", "gap"];

/// Status's values, grouped, the work under way first.
pub(crate) const STATUS_GROUPS: [(&str, &[&str]); 5] = [
    ("in progress", &["Coding", "In Review"]),
    ("ready", &["Ready"]),
    ("not started", &["Pending", "Blocked", "Idea", "Epic"]),
    ("done", &["Complete", "Superseded", "Canceled"]),
    ("docs", &["Doc"]),
];

const STATUSES: [&str; 11] = ["Coding", "In Review", "Ready", "Pending", "Blocked", "Idea", "Epic", "Complete", "Superseded", "Canceled", "Doc"];

/// Severity's values — the language's spelling.
pub(crate) const SEVERITIES: [&str; 5] = ["critical", "high", "medium", "low", "none"];

/// A flag's two values.
pub(crate) const FLAG: [&str; 2] = ["yes", "no"];

/// FILTER-16: State's values — the inputs page's groups but the removed
/// (Removed's), in its order.
pub(crate) const INPUT_STATES: [&str; 3] = ["open", "answered", "closed"];

/// The query an empty filter was written as by WAVE-12 (`?all`).
pub(crate) const ALL: &str = "all";

impl Cat {
    pub(crate) const ALL: [Cat; 19] = [
        Cat::Shortcode,
        Cat::Status,
        Cat::Kind,
        Cat::Severity,
        Cat::Ready,
        Cat::Blocked,
        Cat::Commits,
        Cat::MengerDocs,
        Cat::Asking,
        Cat::Label,
        Cat::When,
        Cat::Deployed,
        Cat::Chats,
        Cat::Wave,
        Cat::Readiness,
        Cat::OffBoard,
        Cat::Title,
        Cat::State,
        Cat::Removed,
    ];

    /// The categories a WAVE-12 link may name (its query names nothing
    /// else).
    pub(crate) const WAVE_12: [Cat; 11] = [
        Cat::Shortcode,
        Cat::Status,
        Cat::Kind,
        Cat::Severity,
        Cat::Ready,
        Cat::Blocked,
        Cat::Commits,
        Cat::Wave,
        Cat::Readiness,
        Cat::OffBoard,
        Cat::Title,
    ];

    /// The server-backed categories, offered at every level, in the menu's
    /// order.
    pub(crate) const SERVER: [Cat; 13] = [
        Cat::Shortcode,
        Cat::Status,
        Cat::Kind,
        Cat::Severity,
        Cat::Ready,
        Cat::Blocked,
        Cat::Commits,
        Cat::MengerDocs,
        Cat::Asking,
        Cat::Label,
        Cat::When,
        Cat::Deployed,
        Cat::Chats,
    ];

    /// The wave board's categories, in its menu's order: WAVE-12's, with the
    /// server's flags after Severity. MENGER-12: Label, which a card's row
    /// carries — not Asking, which the board evaluates on its cards and a
    /// card does not say.
    pub(crate) const WAVES: [Cat; 13] = [
        Cat::Shortcode,
        Cat::Status,
        Cat::Wave,
        Cat::Kind,
        Cat::Severity,
        Cat::Ready,
        Cat::Blocked,
        Cat::Commits,
        Cat::MengerDocs,
        Cat::Label,
        Cat::Readiness,
        Cat::OffBoard,
        Cat::Title,
    ];

    /// FILTER-16: the inputs page's categories, in its menu's order — its
    /// own (State, Removed) first, then the server-backed ones, which filter
    /// the inputs' tickets.
    pub(crate) const INPUTS: [Cat; 15] = [
        Cat::State,
        Cat::Removed,
        Cat::Shortcode,
        Cat::Status,
        Cat::Kind,
        Cat::Severity,
        Cat::Ready,
        Cat::Blocked,
        Cat::Commits,
        Cat::MengerDocs,
        Cat::Asking,
        Cat::Label,
        Cat::When,
        Cat::Deployed,
        Cat::Chats,
    ];

    /// A surface's own category — evaluated by the surface, kept beside the
    /// language (it has no words for it).
    pub(crate) fn is_local(self) -> bool {
        matches!(self, Cat::Wave | Cat::Readiness | Cat::OffBoard | Cat::Title | Cat::State | Cat::Removed)
    }

    /// A yes · no category — the server's flags, and the inputs page's
    /// Removed (`yes`: the removed show).
    pub(crate) fn is_flag(self) -> bool {
        matches!(self, Cat::Ready | Cat::Blocked | Cat::Commits | Cat::MengerDocs | Cat::Asking | Cat::Removed)
    }

    /// The category's key — WAVE-12's in the URL for the wave board's (and
    /// what WAVE-12's links say of the rest), and the DOM's `data-cat`.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Cat::Shortcode => "sc",
            Cat::Status => "status",
            Cat::Kind => "kind",
            Cat::Severity => "sev",
            Cat::Ready => "isready",
            Cat::Blocked => "blocked",
            Cat::Commits => "commits",
            Cat::MengerDocs => "menger",
            Cat::Asking => "asking",
            Cat::Label => "label",
            Cat::When => "when",
            Cat::Deployed => "deployed",
            Cat::Chats => "chats",
            Cat::Wave => "wave",
            Cat::Readiness => "ready",
            Cat::OffBoard => "off",
            Cat::Title => "q",
            Cat::State => "state",
            Cat::Removed => "removed",
        }
    }

    pub(crate) fn of_key(key: &str) -> Option<Cat> {
        Cat::ALL.into_iter().find(|c| c.key() == key)
    }

    /// The category's name on its pill.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Cat::Shortcode => "Shortcode",
            Cat::Status => "Status",
            Cat::Kind => "Kind",
            Cat::Severity => "Severity",
            Cat::Ready => "Ready",
            Cat::Blocked => "Blocked",
            Cat::Commits => "Commits",
            Cat::MengerDocs => "Menger docs",
            Cat::Asking => "Asking",
            Cat::Label => "Label",
            Cat::When => "When",
            Cat::Deployed => "Deployed",
            Cat::Chats => "Chats",
            Cat::Wave => "Wave",
            Cat::Readiness => "Readiness",
            Cat::OffBoard => "Off-board blockers",
            Cat::Title => "Title",
            Cat::State => "State",
            Cat::Removed => "Removed",
        }
    }

    /// FILTER-27: words a menu search finds the category by, beside its
    /// name.
    pub(crate) fn keywords(self) -> &'static str {
        match self {
            Cat::When => "created moved changed recently in status for since before between today",
            Cat::Deployed => "deploy deployment environment running released prod test staging waiting",
            Cat::Chats => "chat chatting unread messages conversation",
            _ => "",
        }
    }

    /// The category's row in the menu.
    pub(crate) fn menu_label(self) -> &'static str {
        match self {
            Cat::Title => "Title contains…",
            c => c.label(),
        }
    }


    /// The values this category always offers, in the menu's order —
    /// Shortcode's and Wave's come from the data, Title's is text.
    pub(crate) fn fixed(self) -> &'static [&'static str] {
        match self {
            Cat::Status => &STATUSES,
            Cat::Kind => &KINDS,
            Cat::Severity => &SEVERITIES,
            Cat::Ready | Cat::Blocked | Cat::Commits | Cat::MengerDocs | Cat::Asking | Cat::Removed => &FLAG,
            Cat::Readiness => &["workable", "waiting", "missing"],
            Cat::State => &INPUT_STATES,
            Cat::OffBoard => &["some", "none"],
            Cat::Shortcode | Cat::Wave | Cat::Title | Cat::Label | Cat::When | Cat::Deployed | Cat::Chats => &[],
        }
    }

    /// `raw` (as a URL or a click names it) as this category's canonical
    /// value, or `None` when it is none of its values. WAVE-12's spelling is
    /// read: a status by any case, `High` for `high`.
    pub(crate) fn token(self, raw: &str) -> Option<String> {
        if self == Cat::Title {
            return (!raw.trim().is_empty()).then(|| raw.to_string());
        }
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        match self {
            Cat::Shortcode => raw
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                .then(|| raw.to_ascii_uppercase()),
            Cat::Status => raw.parse::<Status>().ok().map(|s| s.as_str().to_string()),
            // MENGER-12: a label, lower case — the language's own rule.
            Cat::Label => menger_tickets_api::labels::normalize(raw).ok(),
            Cat::Wave => match raw.to_ascii_lowercase().as_str() {
                "missing" => Some("missing".into()),
                "done" => Some("done".into()),
                n => n.parse::<u32>().ok().filter(|n| *n >= 1).map(|n| n.to_string()),
            },
            _ => self.fixed().iter().find(|t| t.eq_ignore_ascii_case(raw)).map(|t| t.to_string()),
        }
    }

    /// Where a value sorts: the menu's order (Wave: ascending, then missing,
    /// then done; Shortcode: the code's).
    fn rank(self, t: &str) -> (u64, String) {
        match self {
            Cat::Shortcode | Cat::Title | Cat::Label => (0, t.to_string()),
            Cat::Wave => (
                match t {
                    "missing" => u64::MAX - 1,
                    "done" => u64::MAX,
                    n => n.parse().unwrap_or(u64::MAX - 2),
                },
                String::new(),
            ),
            _ => (self.fixed().iter().position(|f| *f == t).unwrap_or(usize::MAX) as u64, t.to_string()),
        }
    }

    /// How a value reads, in the menu and on a pill.
    pub(crate) fn value_label(self, t: &str) -> String {
        match (self, t) {
            (Cat::Wave | Cat::Readiness, "missing") => "missing blocker".into(),
            (Cat::Wave, "done") => "done".into(),
            (Cat::Wave, n) => format!("wave {n}"),
            (Cat::Severity, "none") => "no severity".into(),
            (Cat::Commits, "yes") => "has commits".into(),
            (Cat::Commits, "no") => "none".into(),
            (Cat::Asking, "yes") => "asking".into(),
            (Cat::Asking, "no") => "not asking".into(),
            (Cat::Readiness, "workable") => "workable now".into(),
            (Cat::OffBoard, "some") => "has some".into(),
            (Cat::Removed, "yes") => "shown".into(),
            (Cat::Removed, "no") => "hidden".into(),
            (Cat::Title, t) => format!("\u{201c}{t}\u{201d}"),
            (_, t) => t.into(),
        }
    }
}

/// A clause's operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Is,
    Not,
}

impl Op {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Op::Is => "is",
            Op::Not => "not",
        }
    }

    fn flip(self) -> Self {
        match self {
            Op::Is => Op::Not,
            Op::Not => Op::Is,
        }
    }
}

/// How a clause reads its operator: `is` / `is not`, and for Title
/// `contains` / `does not contain` — FILTER-42: never "is any of" (two
/// values are two alternatives, joined by the word or).
pub(crate) fn op_label(c: Cat, op: Op, n: usize) -> &'static str {
    match (c, op) {
        (Cat::Title, Op::Is) => "contains",
        (Cat::Title, Op::Not) => "does not contain",
        (_, Op::Is) => {
            let _ = n;
            "is"
        }
        (_, Op::Not) => "is not",
    }
}

/// DEPLOY-13: the category a time pill is drawn under — Deployed for a
/// `deployed`, When for the rest.
pub(crate) fn time_cat(t: &crate::when::Timed) -> Cat {
    if crate::when::is_chats(t) {
        Cat::Chats
    } else if crate::when::env_of(t).is_some() {
        Cat::Deployed
    } else {
        Cat::When
    }
}

/// FILTER-19: the words of the operator switch heading `c`'s values in the
/// menu — `is` / `is not`, Title's `contains` / `does not contain` (a
/// pill's, for one value); a flag has none: its `yes` / `no` is already the
/// negation (nor When and Deployed: a time pill's parts are its own).
pub(crate) fn switch_words(c: Cat) -> Option<(&'static str, &'static str)> {
    (!c.is_flag() && !matches!(c, Cat::When | Cat::Deployed | Cat::Chats)).then(|| (op_label(c, Op::Is, 1), op_label(c, Op::Not, 1)))
}

/// One category's clause: its operator and its values — canonical, in the
/// category's order, distinct, never none (a clause left with no value is
/// no clause). Title's one value is its text; a flag's one value is `yes`
/// or `no`, under `is`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Clause {
    pub(crate) op: Op,
    pub(crate) values: Vec<String>,
}

impl Clause {
    pub(crate) fn new(c: Cat, op: Op, values: impl IntoIterator<Item = String>) -> Option<Self> {
        let mut values: Vec<String> = values.into_iter().collect();
        if c == Cat::Title {
            values.truncate(1);
        } else {
            values.sort_by_key(|v| c.rank(v));
            values.dedup();
        }
        if c.is_flag() {
            // A flag is one value under `is`: `is not yes` is `is no`.
            let v = values.last()?.clone();
            let v = if op == Op::Not { other(&v) } else { v };
            return Some(Clause { op: Op::Is, values: vec![v] });
        }
        (!values.is_empty()).then_some(Clause { op, values })
    }

    /// Whether a value `v` (none: the thing has no value in this category)
    /// passes.
    pub(crate) fn admits(&self, v: Option<&str>) -> bool {
        let has = v.is_some_and(|v| self.values.iter().any(|x| x == v));
        has == (self.op == Op::Is)
    }
}

/// A flag's other value.
fn other(v: &str) -> String {
    if v == "yes" { "no" } else { "yes" }.to_string()
}

/// What the bar edits — a level's clauses, at most one per category — and
/// every edit the menu and the pills make, written once over the two.
pub(crate) trait Clausal {
    fn clause(&self, c: Cat) -> Option<Clause>;
    fn set_clause(&mut self, c: Cat, cl: Option<Clause>);

    /// `c`'s clause set to `op` and `values`; none left, no clause.
    fn set(&mut self, c: Cat, op: Op, values: impl IntoIterator<Item = String>) {
        self.set_clause(c, Clause::new(c, op, values));
    }

    /// `c`'s values replaced, its operator kept (`is` for a new clause).
    fn set_values(&mut self, c: Cat, values: impl IntoIterator<Item = String>) {
        let op = self.clause(c).map_or(Op::Is, |cl| cl.op);
        self.set(c, op, values);
    }

    /// Whether `t` is one of `c`'s values.
    fn has(&self, c: Cat, t: &str) -> bool {
        self.clause(c).is_some_and(|cl| cl.values.iter().any(|v| v == t))
    }

    /// `t` added to `c`'s values, or taken from them; a flag's value is the
    /// one (a click on the other replaces it, on itself removes it).
    fn toggle(&mut self, c: Cat, t: &str) {
        if c.is_flag() {
            let next = (!self.has(c, t)).then(|| t.to_string());
            self.set(c, Op::Is, next);
            return;
        }
        let mut values: Vec<String> = self.clause(c).map(|cl| cl.values).unwrap_or_default();
        match values.iter().position(|v| v == t) {
            Some(i) => {
                values.remove(i);
            }
            None => values.push(t.to_string()),
        }
        self.set_values(c, values);
    }

    /// A click on a shortcode's box: its whole subtree checked, or — all of
    /// it checked — none of it.
    fn toggle_code(&mut self, tree: &ScTree, code: &str) {
        let checked: BTreeSet<String> = self.clause(Cat::Shortcode).map(|cl| cl.values.into_iter().collect()).unwrap_or_default();
        self.set_values(Cat::Shortcode, tree.click(code, &checked));
    }

    /// `c`'s operator turned over (a flag's value).
    fn flip(&mut self, c: Cat) {
        if let Some(cl) = self.clause(c) {
            let op = if c.is_flag() { Op::Not } else { cl.op.flip() };
            self.set(c, op, cl.values);
        }
    }

    /// FILTER-19: the menu's switch — `c`'s operator set to `op`, the same
    /// clause a pill's toggle makes ([`Clausal::flip`]); with no clause, or
    /// on a flag (no switch), nothing.
    fn set_op(&mut self, c: Cat, op: Op) {
        if !c.is_flag() && self.clause(c).is_some_and(|cl| cl.op != op) {
            self.flip(c);
        }
    }

    /// FILTER-19: the edit `f` makes of `c` in the menu, a clause it starts
    /// taking `op` (the switch's) — one already there keeps its own.
    fn edit_as(&mut self, c: Cat, op: Op, f: impl FnOnce(&mut Self))
    where
        Self: Sized,
    {
        let new = self.clause(c).is_none();
        f(self);
        if new {
            self.set_op(c, op);
        }
    }

    fn remove(&mut self, c: Cat) {
        self.set_clause(c, None);
    }
}

/// A set of clauses, one per category — a surface's local clauses, and what
/// a WAVE-12 link says (its keys: `sc, status, wave, kind, sev, ready, off,
/// q`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Clauses {
    clauses: BTreeMap<Cat, Clause>,
}

impl Clausal for Clauses {
    fn clause(&self, c: Cat) -> Option<Clause> {
        self.clauses.get(&c).cloned()
    }

    fn set_clause(&mut self, c: Cat, cl: Option<Clause>) {
        match cl {
            Some(cl) => {
                self.clauses.insert(c, cl);
            }
            None => {
                self.clauses.remove(&c);
            }
        }
    }
}

impl Clauses {
    /// These clauses with `c`'s set (values as canonical tokens).
    #[cfg(test)]
    pub(crate) fn with(mut self, c: Cat, op: Op, values: impl IntoIterator<Item = String>) -> Self {
        self.set(c, op, values);
        self
    }

    pub(crate) fn get(&self, c: Cat) -> Option<&Clause> {
        self.clauses.get(&c)
    }

    /// The clauses, in the categories' order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (Cat, &Clause)> {
        self.clauses.iter().map(|(c, cl)| (*c, cl))
    }

    /// The local clauses alone.
    pub(crate) fn local(&self) -> Clauses {
        Clauses { clauses: self.clauses.iter().filter(|(c, _)| c.is_local()).map(|(c, cl)| (*c, cl.clone())).collect() }
    }

    /// The server-backed clauses alone.
    pub(crate) fn server(&self) -> Clauses {
        Clauses { clauses: self.clauses.iter().filter(|(c, _)| !c.is_local()).map(|(c, cl)| (*c, cl.clone())).collect() }
    }

    /// The clauses as query pairs (values percent-encoded): `key=op:v1,v2`
    /// in the categories' order; Title as `q=text` or `q=not:text`.
    pub(crate) fn to_pairs(&self) -> Vec<lens::Pair> {
        self.clauses
            .iter()
            .map(|(c, cl)| {
                let values = cl.values.iter().map(|v| lens::enc(v)).collect::<Vec<_>>().join(",");
                match (c, cl.op) {
                    (Cat::Title, Op::Is) => lens::Pair::new("q", Some(values)),
                    (c, op) => lens::Pair::new(c.key(), Some(format!("{}:{values}", op.word()))),
                }
            })
            .collect()
    }

    /// The clauses as WAVE-12 wrote a board's query (no `?`); none as
    /// [`ALL`]. (The lens writes them as [`Clauses::to_pairs`].)
    #[cfg(test)]
    pub(crate) fn to_query(&self) -> String {
        if self.clauses.is_empty() {
            return ALL.into();
        }
        lens::join(&self.to_pairs())
    }

    /// WAVE-12's query as clauses — `None` for no query, or one that is
    /// malformed (an unknown key, a key twice, no values, a value that is
    /// none of its category's); `all` is none. `keys`: the categories it
    /// may name (the rest of the query is someone else's).
    pub(crate) fn parse_query(q: &str) -> Option<Self> {
        Clauses::parse(&lens::pairs(q), &Cat::WAVE_12, true)
    }

    /// The pairs naming `cats` as clauses (the rest ignored) — `None` when
    /// none does, or one is malformed.
    pub(crate) fn parse_pairs(ps: &[lens::Pair], cats: &[Cat]) -> Option<Self> {
        Clauses::parse(ps, cats, false)
    }

    /// `ps` as clauses of `cats` — `wave_12`: a WAVE-12 query, which names
    /// nothing else (`all` is none of them); otherwise the pairs naming none
    /// of `cats` are someone else's.
    fn parse(ps: &[lens::Pair], cats: &[Cat], wave_12: bool) -> Option<Self> {
        let mut f = Clauses::default();
        let mut any = false;
        for p in ps {
            if p.key == ALL && p.value.is_none() && wave_12 {
                if ps.len() > 1 {
                    return None;
                }
                return Some(Clauses::default());
            }
            let Some(c) = Cat::of_key(&p.key).filter(|c| cats.contains(c)) else {
                if wave_12 {
                    return None; // a WAVE-12 query names nothing else
                }
                continue;
            };
            any = true;
            let v = p.value.as_deref()?;
            if f.clauses.contains_key(&c) {
                return None;
            }
            let (op, rest) = match (v.strip_prefix("is:"), v.strip_prefix("not:")) {
                (Some(r), _) => (Op::Is, r),
                (_, Some(r)) => (Op::Not, r),
                _ => (Op::Is, v),
            };
            let raw: Vec<String> = if c == Cat::Title { vec![lens::dec(rest)?] } else { rest.split(',').map(lens::dec).collect::<Option<_>>()? };
            let tokens: Vec<String> = raw.iter().map(|r| c.token(r)).collect::<Option<_>>()?;
            f.clauses.insert(c, Clause::new(c, op, tokens)?);
        }
        any.then_some(f)
    }
}

// ---------------------------------------------------------------------
// The shortcode tree

/// The shortcodes as the nav nests them: each under the shortcode of its
/// `host` ticket (when that shortcode is one the groups know), the rest at
/// the top. Children in shortcode order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ScTree {
    kids: BTreeMap<String, Vec<String>>,
    top: Vec<String>,
    known: BTreeSet<String>,
    /// Each shortcode's host ticket, where it has one.
    hosts: BTreeMap<String, String>,
}

/// A shortcode's box in the menu: its whole subtree checked, none of it, or
/// part of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tri {
    On,
    Off,
    Mixed,
}


/// A row of the Shortcode menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScRow {
    Code { code: String, depth: usize },
    /// Between the route's tree and the rest: "other boards".
    Divider,
}

impl ScTree {
    pub(crate) fn of(groups: &BTreeMap<String, Group>) -> Self {
        let known: BTreeSet<String> = groups.keys().cloned().collect();
        let mut kids: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut top = Vec::new();
        let mut hosts = BTreeMap::new();
        for g in groups.values() {
            if let Some(h) = &g.host {
                hosts.insert(g.shortcode.to_string(), h.to_string());
            }
            match g.host.as_deref().and_then(epic_of).filter(|h| *h != g.shortcode.as_str() && known.contains(*h)) {
                Some(h) => kids.entry(h.to_string()).or_default().push(g.shortcode.to_string()),
                None => top.push(g.shortcode.to_string()),
            }
        }
        let mut t = ScTree { kids, top, known, hosts };
        // A host chain that loops reaches no top: its first shortcode is one.
        loop {
            let reached: HashSet<String> = t.top.iter().flat_map(|r| t.subtree(r)).collect();
            match t.known.iter().find(|e| !reached.contains(*e)) {
                Some(lost) => t.top.push(lost.clone()),
                None => break,
            }
        }
        t.top.sort();
        t
    }

    /// Every shortcode the groups know.
    pub(crate) fn known(&self) -> &BTreeSet<String> {
        &self.known
    }

    /// `root` and every shortcode hosted under it, at any depth — depth
    /// first, children in shortcode order — each with its depth. An unknown
    /// `root` is a tree of one.
    pub(crate) fn walk(&self, root: &str) -> Vec<(String, usize)> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![(root.to_string(), 0)];
        while let Some((e, d)) = stack.pop() {
            if !seen.insert(e.clone()) {
                continue;
            }
            if let Some(ks) = self.kids.get(&e) {
                stack.extend(ks.iter().rev().map(|k| (k.clone(), d + 1)));
            }
            out.push((e, d));
        }
        out
    }

    /// The tree of `root`: it and every shortcode hosted under it.
    pub(crate) fn subtree(&self, root: &str) -> Vec<String> {
        self.walk(root).into_iter().map(|(e, _)| e).collect()
    }

    /// What hosts `code`, nearest first: each host ticket and the shortcode
    /// it is in, up the chain — a ticket's lineage beyond its own shortcode.
    pub(crate) fn hosting(&self, code: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut at = code.to_string();
        while let Some(h) = self.hosts.get(&at) {
            let Some(e) = epic_of(h).map(str::to_string) else { break };
            if !seen.insert(e.clone()) || e == at {
                break;
            }
            out.push(h.clone());
            out.push(e.clone());
            at = e;
        }
        out
    }

    /// `code`'s box when `checked` are the checked shortcodes.
    pub(crate) fn tri(&self, code: &str, checked: &BTreeSet<String>) -> Tri {
        let sub = self.subtree(code);
        match sub.iter().filter(|e| checked.contains(*e)).count() {
            0 => Tri::Off,
            n if n == sub.len() => Tri::On,
            _ => Tri::Mixed,
        }
    }

    /// What a click on `code`'s box leaves checked: its whole subtree, or —
    /// when the subtree was all checked — none of it.
    pub(crate) fn click(&self, code: &str, checked: &BTreeSet<String>) -> BTreeSet<String> {
        let sub = self.subtree(code);
        let mut out = checked.clone();
        if self.tri(code, checked) == Tri::On {
            for e in &sub {
                out.remove(e);
            }
        } else {
            out.extend(sub);
        }
        out
    }

    /// The Shortcode menu: the route's tree (`route`, when the level has
    /// one), then — after a divider — every other top-level shortcode's
    /// tree (what the route's tree already listed left out), then any
    /// `extra` (a clause's value no group knows). With no route, every tree,
    /// no divider.
    pub(crate) fn rows<'a>(&self, route: Option<&str>, extra: impl IntoIterator<Item = &'a String>) -> Vec<ScRow> {
        let mut listed = HashSet::new();
        let mut out: Vec<ScRow> = Vec::new();
        if let Some(route) = route {
            for (code, depth) in self.walk(route) {
                listed.insert(code.clone());
                out.push(ScRow::Code { code, depth });
            }
        }
        let mut others = Vec::new();
        for t in &self.top {
            for (code, depth) in self.walk(t) {
                if listed.insert(code.clone()) {
                    others.push(ScRow::Code { code, depth });
                }
            }
        }
        for e in extra {
            if listed.insert(e.clone()) {
                others.push(ScRow::Code { code: e.clone(), depth: 0 });
            }
        }
        if !others.is_empty() && !out.is_empty() {
            out.push(ScRow::Divider);
        }
        out.extend(others);
        out
    }
}

// ---------------------------------------------------------------------
// The pills



// ---------------------------------------------------------------------
// The counts

/// Per category, per value: how many tickets have it. A value absent from
/// its category's map shows no count — nor does a category absent.
pub(crate) type Counts = HashMap<Cat, HashMap<String, usize>>;

/// A level's counts: from a category to its value → count — the wave
/// board's from its held answer; a server-backed level's from `facets`
/// (FILTER-12); none, until a level has one.
#[derive(Clone, Copy)]
pub(crate) struct CountsProvider(Option<Signal<Counts>>);

impl CountsProvider {
    /// No counts show.
    pub(crate) fn none() -> Self {
        CountsProvider(None)
    }

    /// The counts `f` computes, kept as a memo (recomputed when what it
    /// reads changes).
    pub(crate) fn new(f: impl Fn() -> Counts + Send + Sync + 'static) -> Self {
        CountsProvider(Some(Memo::new(move |_| f()).into()))
    }

    /// The counts a signal holds — a server-backed level's, from `facets`
    /// (FILTER-12's `counts::provider`).
    pub(crate) fn of(s: Signal<Counts>) -> Self {
        CountsProvider(Some(s))
    }

    /// `v`'s count in `c`, tracked — `None`: none to show (nor once the
    /// view that gave the counts is gone).
    pub(crate) fn count(self, c: Cat, v: &str) -> Option<usize> {
        self.0.and_then(|s| s.try_with(|k| k.get(&c).and_then(|m| m.get(v)).copied()).flatten())
    }


    /// The counts, tracked (empty with no provider).
    pub(crate) fn get(self) -> Counts {
        self.0.and_then(|s| s.try_get()).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------
// The menu's rows

/// A value a category offers, as [`offered`] lists them — the panel's
/// sections draw them ([`section_rows`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    /// A value to tick. `depth` indents a shortcode under its host.
    Value { cat: Cat, token: String, depth: usize, scoped: bool },
    /// A group of a category's values, by its header (Status's groups):
    /// the panel's heading.
    Group { cat: Cat, label: &'static str, tokens: &'static [&'static str] },
    /// A heading over values — Shortcode's "other boards".
    Heading(&'static str),
}


/// What the menu needs to know to list values.
pub(crate) struct MenuFacts<'a> {
    /// The level's categories, in its panel's order.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) cats: &'a [Cat],
    /// The route's shortcode — its tree first in Shortcode's values.
    pub(crate) route: Option<&'a str>,
    pub(crate) tree: Option<&'a ScTree>,
    /// The level's clauses (what is checked; a value no group knows).
    pub(crate) clauses: &'a Clauses,
    pub(crate) counts: &'a Counts,
}

/// The values category `c` offers, in the menu's order: Shortcode's tree
/// (the route's first, then "other boards"); Status's groups, each under
/// its header; wave 1 … the answer's last (or the clause's, if later),
/// missing blocker, done; the fixed values of the rest. Title offers a text
/// field, no rows.
pub(crate) fn offered(c: Cat, k: &MenuFacts) -> Vec<Row> {
    let value = |token: &str, depth: usize| Row::Value { cat: c, token: token.to_string(), depth, scoped: false };
    match c {
        Cat::Shortcode => {
            let extra: Vec<String> = k.clauses.get(c).map(|cl| cl.values.clone()).unwrap_or_default();
            let rows = match k.tree {
                Some(t) => t.rows(k.route, extra.iter()),
                None => extra.iter().map(|e| ScRow::Code { code: e.clone(), depth: 0 }).collect(),
            };
            rows.into_iter()
                .map(|r| match r {
                    ScRow::Code { code, depth } => value(&code, depth),
                    ScRow::Divider => Row::Heading("other boards"),
                })
                .collect()
        }
        Cat::Status => STATUS_GROUPS
            .iter()
            .flat_map(|(h, vs)| std::iter::once(Row::Group { cat: c, label: h, tokens: vs }).chain(vs.iter().map(|v| value(v, 0))))
            .collect(),
        Cat::Wave => {
            let of_counts =
                k.counts.get(&c).map_or(0, |m| m.iter().filter(|(_, n)| **n > 0).filter_map(|(v, _)| v.parse::<u32>().ok()).max().unwrap_or(0));
            let of_clause = k.clauses.get(c).map_or(0, |cl| cl.values.iter().filter_map(|v| v.parse::<u32>().ok()).max().unwrap_or(0));
            (1..=of_counts.max(of_clause))
                .map(|n| value(&n.to_string(), 0))
                .chain(["missing", "done"].map(|t| value(t, 0)))
                .collect()
        }
        Cat::Title => Vec::new(),
        Cat::Label => label_values(k).iter().map(|t| value(t, 0)).collect(),
        c => c.fixed().iter().map(|t| value(t, 0)).collect(),
    }
}

/// MENGER-12: Label's values, in name order — `needs-you` always (the Needs
/// you filter asks for it), every label the counts name, and the clause's own.
pub(crate) fn label_values(k: &MenuFacts) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::from([menger_tickets_api::labels::NEEDS_YOU.to_string()]);
    out.extend(k.counts.get(&Cat::Label).into_iter().flat_map(|m| m.keys().cloned()));
    out.extend(k.clauses.get(Cat::Label).into_iter().flat_map(|cl| cl.values.iter().cloned()));
    out.into_iter().collect()
}

// ---------------------------------------------------------------------
// The view: the panel's state

/// A level's panel — its state, every handle `Copy`. The lens holds one per
/// level for the app's lifetime (so `F`, a tag's Filter, a chip, can open a
/// panel another view draws); the view drawing it says what it offers
/// ([`Pop::configure`]).
#[derive(Clone, Copy)]
pub(crate) struct Pop {
    pub(crate) level: Level,
    open: RwSignal<bool>,
    query: RwSignal<String>,
    /// FILTER-19: each section's switch, `is` until it is set.
    ops: RwSignal<BTreeMap<Cat, Op>>,
    /// FILTER-39: the group the panel edits (its sections tick its clauses).
    pub(crate) sel: RwSignal<usize>,
    /// FILTER-39: what the last tick said — a second value of one category
    /// made a group of its own.
    note: RwSignal<Option<String>>,
    /// What the level's bar offers: its categories and its counts — set by
    /// the view that draws it; `mounted` while it does.
    cats: RwSignal<Vec<Cat>>,
    counts: RwSignal<CountsProvider>,
    mounted: RwSignal<u32>,
    /// FILTER-27: which time section's pane is where (When, Deployed or
    /// Chats; another section's is at its rows), and whether its window
    /// editor (Custom…) is open.
    pub(crate) when: RwSignal<(Cat, crate::when_view::Step)>,
    pub(crate) custom: RwSignal<bool>,
    /// Why the last edit was refused (what the language refuses).
    pub(crate) refusal: RwSignal<Option<String>>,
    /// The section asked for (a tag's Filter, a chip) — scrolled to, lit.
    focus: RwSignal<Option<Cat>>,
    /// FILTER-39: the value hovered and what ticking it would leave — its
    /// key, and the count once it is in.
    hover: RwSignal<Option<(String, Option<usize>)>>,
    /// FILTER-43: the text drawer is open, and what it holds does not read
    /// — the level keeps the last filter that read, and its chips say so.
    pub(crate) drawer: RwSignal<bool>,
    pub(crate) stale: RwSignal<bool>,
}

impl Pop {
    pub(crate) fn new(level: Level) -> Self {
        Pop {
            level,
            open: RwSignal::new(false),
            query: RwSignal::new(String::new()),
            ops: RwSignal::new(BTreeMap::new()),
            sel: RwSignal::new(0),
            note: RwSignal::new(None),
            cats: RwSignal::new(Cat::SERVER.to_vec()),
            counts: RwSignal::new(CountsProvider::none()),
            mounted: RwSignal::new(0),
            when: RwSignal::new((Cat::When, crate::when_view::Step::Root)),
            custom: RwSignal::new(false),
            refusal: RwSignal::new(None),
            focus: RwSignal::new(None),
            hover: RwSignal::new(None),
            drawer: RwSignal::new(false),
            stale: RwSignal::new(false),
        }
    }

    /// The view drawing this level's bar says what it offers, for as long
    /// as it is mounted.
    pub(crate) fn configure(self, cats: &[Cat], counts: CountsProvider) {
        self.cats.set(cats.to_vec());
        self.counts.set(counts);
        self.mounted.update(|n| *n += 1);
        on_cleanup(move || {
            self.mounted.try_update(|n| *n = n.saturating_sub(1));
            if self.mounted.try_get_untracked() == Some(0) {
                self.close(false);
                self.counts.try_set(CountsProvider::none());
            }
        });
    }

    pub(crate) fn is_mounted(self) -> bool {
        self.mounted.get_untracked() > 0
    }

    /// FILTER-40: whether a view draws this level's line (tracked).
    pub(crate) fn mounted_tracked(self) -> bool {
        self.mounted.get() > 0
    }

    pub(crate) fn is_open(self) -> bool {
        self.open.get_untracked()
    }

    /// FILTER-12: whether it is open (tracked) — when a server-backed
    /// level's counts are asked.
    pub(crate) fn open_tracked(self) -> bool {
        self.open.get()
    }

    /// Opened — on `cat`'s section when given; FILTER-40: `switched`: from another level's tab, the
    /// focus to give back already remembered.
    pub(crate) fn show_from(self, cat: Option<Cat>, switched: bool) {
        if !self.open.get_untracked() {
            if !switched {
                keys::remember_focus();
            }
            if let Some(n) = crate::naming::hook() {
                n.fresh_counts();
            }
            self.query.set(String::new());
            self.note.set(None);
            self.refusal.set(None);
            self.custom.set(false);
        }
        self.focus.set(cat);
        self.open.set(true);
    }

    /// FILTER-40: folded because another level's tab took the panel — the
    /// focus to give back stays remembered.
    pub(crate) fn hand_over(self) {
        if self.open.try_get_untracked().unwrap_or(false) {
            self.open.set(false);
            self.query.set(String::new());
            self.hover.set(None);
        }
    }

    /// Folded — focus handed back where it was (`restore`: a key folded
    /// it), or left where a click put it.
    pub(crate) fn close(self, restore: bool) {
        if !self.open.try_get_untracked().unwrap_or(false) {
            return;
        }
        self.open.set(false);
        self.query.set(String::new());
        self.hover.set(None);
        self.when.set((Cat::When, crate::when_view::Step::Root));
        if restore {
            keys::restore_focus();
        } else {
            keys::forget_focus();
        }
    }

    /// `c`'s switch. Tracked.
    fn op(self, c: Cat) -> Op {
        self.ops.with(|o| o.get(&c).copied().unwrap_or(Op::Is))
    }

    /// FILTER-19: `c`'s switch set — the selected group's clauses of `c`
    /// turned to it (a local category's clause too), and the next tick
    /// under it.
    fn set_op(self, lens: Lens, c: Cat, op: Op) {
        self.ops.update(|o| {
            o.insert(c, op);
        });
        if c.is_local() {
            lens.edit(self.level, |k| k.set_op(c, op));
            return;
        }
        let sel = self.sel.get_untracked();
        let turns = untrack(|| lens.ors(self.level)).and_then(|o| o.groups.get(sel).cloned()).is_some_and(|g| {
            g.iter().any(|p| crate::ors::cat_of(p) == Some(c) && p.not != (op == Op::Not) && !c.is_flag())
        });
        if turns {
            let r = lens.edit_ors(self.level, |o| {
                if let Some(g) = o.groups.get_mut(sel) {
                    for p in g.iter_mut().filter(|p| crate::ors::cat_of(p) == Some(c)) {
                        p.not = op == Op::Not;
                    }
                }
                Ok(())
            });
            self.answer(r);
        }
    }

    fn answer<T>(self, r: Result<T, String>) {
        match r {
            Ok(_) => self.refusal.set(None),
            Err(why) => self.refusal.set(Some(why)),
        }
    }

    /// A value ticked: into the selected group (a second value of one
    /// category, a group of its own — said); a surface's own category, its
    /// clause beside the groups.
    fn check(self, lens: Lens, c: Cat, token: &str) {
        let op = untrack(|| self.op(c));
        self.hover.set(None);
        if c.is_local() {
            lens.edit(self.level, |k| k.edit_as(c, op, |k| k.toggle(c, token)));
            return;
        }
        let tree = lens.sctree().get_untracked();
        let sel = self.sel.get_untracked();
        let r = lens.edit_ors(self.level, |o| o.pick(sel, c, token, op, tree.as_ref()));
        if let Ok(p) = &r {
            self.sel.set(p.sel);
            self.note.set(p.note.clone());
        }
        self.answer(r);
    }

    /// A saved filter ticked: a group by name.
    fn check_saved(self, lens: Lens, name: &str) {
        let sel = self.sel.get_untracked();
        let r = lens.edit_ors(self.level, |o| Ok(o.pick_saved(sel, name)));
        if let Ok(p) = &r {
            self.sel.set(p.sel);
            self.note.set(p.note.clone());
        }
        self.answer(r);
    }

    /// The level's counts provider. Tracked.
    fn counts(self) -> CountsProvider {
        self.counts.get()
    }
}

/// The panel's DOM id for a level.
pub(crate) fn pop_id(level: Level) -> String {
    format!("fmenu-{}", level.name())
}


fn document() -> Option<web_sys::Document> {
    dom::window().document()
}

// ---------------------------------------------------------------------
// The view: the bar

/// FILTER-41: the chips line over a surface (FILTER-38's mockups 1 and 3)
/// — one line: the `Filter` button (its badge the number of clauses), the
/// global filter as a dashed chip, the level's clauses as chips (the
/// field muted, the value inked, ×; a click opens its section) with **and**
/// between them and **or** between the groups, the surface's own clauses
/// likewise, the count (*4 of 37*) and `save as…` (FILTER-37's naming:
/// a name binds the filter, and every edit after amends its doc). Empty,
/// it says what is shown (*all 39 tickets*) and offers the saved filters
/// the reader uses most, each counted. The name field, `text` and `Saved ▾`
/// are gone: text is the panel's foot, choosing is its Saved section or a
/// chip here. Under the line: naming's prompts, "as evaluated", the
/// panel (FILTER-39, in the page's flow), and the contradiction notice.
/// `cats`: the level's categories, server-backed and local, in its panel's
/// order; `counts`: what the panel's values count.
#[component]
pub(crate) fn FilterBar(
    level: Level,
    #[prop(into)] cats: Vec<Cat>,
    counts: CountsProvider,
    /// What the line's Filter button says when hovered.
    #[prop(optional, into)]
    hint: Option<String>,
) -> impl IntoView {
    let lens = lens::use_lens();
    let pop = lens.pop(level);
    pop.configure(&cats, counts);
    let hint = hint.unwrap_or_else(|| "Filter (f)".into());
    let shown = tab_count(lens, level);
    let all = total_count();
    view! {
        <div class="fbar fline" data-fbar=level.name() data-level=level.name() class:open=move || pop.open_tracked() on:keydown=move |ev| fold_key(pop, ev)>
            <div class="fline-row">
                <FilterButton level hint badge=true />
                <span class="fline-chips">{line_chips(lens, level, shown)}</span>
                <span class="fline-end">
                    <span class="fline-n" title="tickets this surface shows, of every ticket" aria-live="polite">
                        <b>{move || shown.get().map_or_else(|| "\u{2026}".to_string(), |n| n.to_string())}</b>
                        <span class="fline-of">{move || all.get().map(|n| format!(" of {n}")).unwrap_or_default()}</span>
                    </span>
                    <SaveAs level />
                    {flash_view(level)}
                </span>
                {move || pop.stale.get().then(|| view! {
                    <span class="fstale" role="status" title="the text being edited does not read yet; the line shows the last filter that did">"\u{26a0} the last filter that read"</span>
                })}
            </div>
            <LineNotes level />
            <PanelHost host=Host::Surface />
        </div>
        <Clash level />
    }
}

/// FILTER-41: every ticket the store holds — counted once.
fn total_count() -> RwSignal<Option<usize>> {
    let n = RwSignal::new(None);
    Effect::new(move |_| {
        crate::session::link().track();
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(got) = crate::counts::count_once(String::new()).await {
                n.try_set(Some(got));
            }
        });
    });
    n
}

/// FILTER-41: what was just done at `level` (a saved filter chosen, `g n`)
/// — for a moment, beside its line.
pub(crate) fn flash_view(level: Level) -> impl IntoView {
    move || crate::saved::hook().and_then(|s| s.flash_for(level)).map(|w| view! { <span class="sflash" role="status">{w}</span> })
}

/// FILTER-41: the line's chips — the global filter's dashed chip; the
/// level's groups, each clause a chip with ×, **and** inside a group and
/// **or** between them (boxed when there are two or more); the surface's
/// own clauses; empty, what is shown and the saved filters most used.
fn line_chips(lens: Lens, level: Level, shown: RwSignal<Option<usize>>) -> impl IntoView {
    move || {
        let global = lens.filter(Level::Global).with(|f| f.format());
        let own = lens.filter(level).with(|f| !f.groups.is_empty());
        let local = lens.kept(level).map(|k| k.local).unwrap_or_default();
        let any = own || local.iter().next().is_some();
        view! {
            {(!global.is_empty()).then(|| view! {
                {global_chip(lens, global.clone())}
                <span class="fw">"and"</span>
            })}
            {if any {
                view! {
                    {own.then(|| edit_chips(lens, level))}
                    {local_chips(lens, level, local, own)}
                }
                .into_any()
            } else {
                empty_line(level, shown).into_any()
            }}
        }
    }
}

/// FILTER-41: the level's groups as chips to edit — × takes a clause out
/// (a group left empty goes); a click on one opens the panel on its
/// section, its group selected (a time clause: its pane, on that clause).
fn edit_chips(lens: Lens, level: Level) -> AnyView {
    let pop = lens.pop(level);
    let Some(o) = lens.ors(level) else {
        let text = lens.filter(level).with(|f| f.format());
        return view! {
            <button type="button" class="fchip fchip-text" title="deeper than groups joined by or \u{2014} edit it as text in the panel" on:click=move |_| lens.open_panel(level, None)>{text}</button>
        }
        .into_any();
    };
    let many = o.live().count() > 1;
    o.groups
        .iter()
        .enumerate()
        .filter(|(_, g)| !g.is_empty())
        .enumerate()
        .map(|(k, (gi, g))| {
            let items = g
                .iter()
                .enumerate()
                .map(|(ii, p)| {
                    let (field, value) = chip_words(p);
                    let words = crate::ors::words(p);
                    let cat = crate::ors::cat_of(p);
                    let slot = crate::ors::Slot { group: gi, item: ii };
                    let saved = matches!(p.field, menger_tickets_api::filter::Field::Saved(_));
                    let open = move |_| {
                        pop.sel.set(gi);
                        if let Some(c) = cat.filter(|c| matches!(c, Cat::When | Cat::Deployed | Cat::Chats)) {
                            pop.when.set((c, crate::when_view::Step::Edit { at: slot, only: None }));
                        }
                        lens.open_panel(level, cat);
                    };
                    let remove = move |_| {
                        let r = lens.edit_ors(level, |o| {
                            o.remove(slot);
                            Ok(())
                        });
                        pop.answer(r);
                        let left = untrack(|| lens.ors(level)).map_or(1, |o| o.groups.len().max(1));
                        pop.sel.update(|s| *s = (*s).min(left - 1));
                    };
                    view! {
                        {(ii > 0).then(|| view! { <span class="fw">"and"</span> })}
                        <span class="fchip" class:saved=saved data-group=gi data-item=ii>
                            <button type="button" class="fchip-b" title=format!("{words} \u{2014} open its section") on:click=open>
                                {(!field.is_empty()).then(|| view! { <span class="fchip-f">{field.clone()}</span> })}
                                <span class="fchip-v">{value.clone()}</span>
                            </button>
                            <button type="button" class="p-x" title=format!("remove {words}") aria-label=format!("remove {words}") on:click=remove>"\u{d7}"</button>
                        </span>
                    }
                })
                .collect_view();
            view! {
                {(k > 0).then(|| view! { <span class="for">"or"</span> })}
                <span class="fchips-g" class:boxed=many>{items}</span>
            }
        })
        .collect_view()
        .into_any()
}

/// FILTER-41: a clause's chip words — its field (muted) and its value
/// (inked): `kind` `build`, `status not` `Ready`, `` `ready`, a time
/// clause whole, a saved filter `⌗` and its name.
pub(crate) fn chip_words(p: &menger_tickets_api::filter::Predicate) -> (String, String) {
    use menger_tickets_api::filter::Field;
    let not = if p.not { " not" } else { "" };
    match &p.field {
        Field::History(_) => (String::new(), crate::ors::words(p)),
        Field::Saved(n) => (format!("\u{2317}{not}"), n.clone()),
        f => match (f.value(), crate::ors::cat_of(p)) {
            (Some(v), Some(c)) if c != Cat::Shortcode => {
                let field = match c {
                    Cat::Label => "label",
                    Cat::Severity => "severity",
                    Cat::Status => "status",
                    Cat::Kind => "kind",
                    _ => f.name(),
                };
                (format!("{field}{not}"), c.value_label(&v))
            }
            (Some(v), _) => (format!("{}{not}", f.name()), v),
            (None, _) => (String::new(), p.format()),
        },
    }
}

/// FILTER-41: the surface's own clauses as chips — beside the groups,
/// and-ed with every one: its values joined by **or** (`is`) or **and not**
/// (`is not`), × taking it out; a click opens its section.
fn local_chips(lens: Lens, level: Level, local: Clauses, after: bool) -> impl IntoView {
    local
        .iter()
        .map(|(c, cl)| (c, cl.clone()))
        .collect::<Vec<_>>()
        .into_iter()
        .enumerate()
        .map(|(i, (c, cl))| {
            let joiner = if cl.op == Op::Is { " or " } else { " and not " };
            let values = cl.values.iter().map(|v| c.value_label(v)).collect::<Vec<_>>().join(joiner);
            let field = format!("{}{}", c.label().to_ascii_lowercase(), if cl.op == Op::Not && !c.is_flag() { " not" } else { "" });
            view! {
                {(after || i > 0).then(|| view! { <span class="fw">"and"</span> })}
                <span class="fchip fchip-local" data-cat=c.key() title=format!("{} \u{2014} beside the groups, and-ed with every one", crate::saved::surface_words(level))>
                    <button type="button" class="fchip-b" on:click=move |_| lens.open_panel(level, Some(c))>
                        <span class="fchip-f">{field}</span>
                        <span class="fchip-v">{values.clone()}</span>
                    </button>
                    <button type="button" class="p-x" title=format!("remove the {} filter", c.label()) aria-label=format!("remove the {} filter", c.label()) on:click=move |_| lens.edit(level, |k| k.remove(c))>"\u{d7}"</button>
                </span>
            }
        })
        .collect_view()
}

/// FILTER-41: an empty line — what is shown (*all 39 tickets*), then the
/// saved filters the reader uses most (the recently used first), each
/// counted; a chip applies it (bound by its name), as the panel's Saved
/// section would choose it. With none saved yet, FILTER-27's suggestions.
fn empty_line(level: Level, shown: RwSignal<Option<usize>>) -> impl IntoView {
    let naming = crate::naming::use_naming();
    let saved = crate::saved::hook();
    let recent = naming.recent();
    let mut entries: Vec<crate::saved::Entry> = saved.map(|s| s.listed()).unwrap_or_default();
    entries.sort_by_key(|e| recent.iter().position(|r| crate::saved::slug(r) == crate::saved::slug(&e.name)).unwrap_or(usize::MAX));
    entries.truncate(3);
    let words = move || shown.get().map_or_else(|| "every ticket".to_string(), |n| format!("all {n} ticket{}", if n == 1 { "" } else { "s" }));
    let chips = if entries.is_empty() {
        crate::saved::SUGGESTED
            .iter()
            .take(3)
            .map(|(name, text)| {
                let (name, text) = (*name, *text);
                view! {
                    <button type="button" class="fchip fchip-sug" title=format!("apply \u{201c}{name}\u{201d}: {text}") on:click=move |_| {
                        if let Some(s) = saved {
                            s.suggest(level, name, text);
                        }
                    }>
                        <span class="fchip-f">"\u{2317}"</span>
                        <span class="fchip-v">{name}</span>
                    </button>
                }
            })
            .collect_view()
            .into_any()
    } else {
        entries
            .into_iter()
            .map(|e| {
                untrack(|| naming.ask_count(&e.name));
                let name = e.name.clone();
                let count = move || naming.count(&name).flatten().map(|n| n.to_string()).unwrap_or_default();
                let title = format!("apply \u{201c}{}\u{201d}", e.name);
                let label = e.name.clone();
                view! {
                    <button type="button" class="fchip fchip-sug" title=title on:click=move |_| {
                        if let Some(s) = saved {
                            naming.touch(&e.name);
                            s.choose(level, e.clone());
                        }
                    }>
                        <span class="fchip-f">"\u{2317}"</span>
                        <span class="fchip-v">{label}</span>
                        <span class="fchip-n">{count}</span>
                    </button>
                }
            })
            .collect_view()
            .into_any()
    };
    view! {
        <span class="fchips-empty">{words}</span>
        <span class="fw fline-saved">"saved:"</span>
        {chips}
    }
}

/// FILTER-41: `save as…` — FILTER-37's naming behind a button: a name
/// saves the level's filter and binds it (every edit after amends its
/// doc); bound, the name shows (`⌗ open builds`), and a click renames it
/// (emptied, it is unlisted, its doc kept).
#[component]
pub(crate) fn SaveAs(level: Level) -> impl IntoView {
    let lens = lens::use_lens();
    let naming = crate::naming::use_naming();
    let editing = RwSignal::new(false);
    let field = NodeRef::<leptos::html::Input>::new();
    focus_each_load(field);
    let bound = Memo::new(move |_| naming.bound(level).map(|b| b.name));
    let pending = Memo::new(move |_| naming.pending(level));
    let empty = Memo::new(move |_| lens.filter(level).with(|f| f.groups.is_empty()));
    let done = move |v: Option<String>| {
        editing.set(false);
        if let Some(v) = v {
            naming.name(level, &v);
        }
    };
    move || {
        if editing.get() {
            let seed = untrack(|| bound.get().or_else(|| pending.get())).unwrap_or_default();
            return view! {
                <span class="fsave">
                    <input
                        class="fsave-in"
                        node_ref=field
                        type="text"
                        autocomplete="off"
                        spellcheck="false"
                        maxlength=crate::saved::NAME_MAX.to_string()
                        placeholder="a name"
                        aria-label=format!("save the {} filter as", level.name())
                        value=seed
                        on:keydown=move |ev: web_sys::KeyboardEvent| match bindings::hit(&ev, Context::Field) {
                            Some("form.submit") => {
                                ev.prevent_default();
                                done(Some(event_target_value(&ev)));
                            }
                            Some("form.cancel") => {
                                ev.prevent_default();
                                ev.stop_propagation();
                                done(None);
                            }
                            _ => {}
                        }
                    />
                    <span class="fhint">"Enter saves \u{00b7} Esc"</span>
                </span>
            }
            .into_any();
        }
        match bound.get() {
            Some(name) => view! {
                <button
                    type="button"
                    class="fsaved"
                    data-saved-as=level.name()
                    title=format!("saved as \u{201c}{name}\u{201d} \u{2014} every edit keeps it saved; click to rename it, or empty the name to unlist it")
                    on:click=move |_| editing.set(true)
                >
                    "\u{2317} "{name.clone()}
                </button>
            }
            .into_any(),
            None => (!empty.get() || pending.get().is_some())
                .then(|| view! {
                    <button type="button" class="fsave-b" data-save-as=level.name() title="save this filter under a name" on:click=move |_| editing.set(true)>
                        {move || pending.get().map_or_else(|| "save as\u{2026}".to_string(), |p| format!("saved with its first clause: \u{201c}{p}\u{201d}"))}
                    </button>
                })
                .into_any(),
        }
    }
}

/// FILTER-40: a level's filter as chips — its groups joined by **or**, the
/// clauses in each joined by **and** (src/ors.rs's words); one deeper than
/// that as its text; nothing, `empty`.
pub(crate) fn chips_view(lens: Lens, level: Level, empty: &'static str) -> impl IntoView {
    move || {
        let f = lens.filter(level).get();
        if f.groups.is_empty() {
            return view! { <span class="fchips-empty">{empty}</span> }.into_any();
        }
        match lens.ors(level) {
            None => view! { <code class="fchip fchip-text">{f.format()}</code> }.into_any(),
            Some(o) => {
                let live: Vec<Vec<menger_tickets_api::filter::Predicate>> = o.live().cloned().collect();
                let many = live.len() > 1;
                live.into_iter()
                    .enumerate()
                    .map(|(gi, g)| {
                        let items = g
                            .iter()
                            .enumerate()
                            .map(|(ii, p)| {
                                view! {
                                    {(ii > 0).then(|| view! { <span class="fw">"and"</span> })}
                                    <span class="fchip" class:saved=matches!(p.field, menger_tickets_api::filter::Field::Saved(_))>{crate::ors::words(p)}</span>
                                }
                            })
                            .collect_view();
                        view! {
                            {(gi > 0).then(|| view! { <span class="for">"or"</span> })}
                            <span class="fchips-g" class:boxed=many>{items}</span>
                        }
                    })
                    .collect_view()
                    .into_any()
            }
        }
    }
}

/// FILTER-40: the nav's head — its filter as chips: the global filter's
/// dashed chip, then the nav's own groups (or "every ticket"); its
/// `Filter` button opens the one panel on the nav's tab. The nav's bar is
/// gone: its name, text and `Saved ▾` are the panel's.
#[component]
pub(crate) fn NavChips(#[prop(into)] cats: Vec<Cat>, counts: CountsProvider) -> impl IntoView {
    let lens = lens::use_lens();
    let pop = lens.pop(Level::Nav);
    pop.configure(&cats, counts);
    let global = Memo::new(move |_| lens.filter(Level::Global).with(|f| f.format()));
    view! {
        <div class="fbar nchips" data-fbar="nav" data-level="nav">
            <FilterButton level=Level::Nav hint="Filter the nav (f — on a ticket page, f filters it to that ticket and its children)" />
            {move || {
                let g = global.get();
                (!g.is_empty()).then(|| view! { {global_chip(lens, g)}<span class="fw">"and"</span> })
            }}
            {chips_view(lens, Level::Nav, "every ticket")}
            <Reset level=Level::Nav />
            {flash_view(Level::Nav)}
        </div>
        <Clash level=Level::Nav />
    }
}

/// FILTER-39: Escape anywhere in the bar while its panel is open — but a
/// field that keeps its own — folds the panel (`app.escape`'s row).
pub(crate) fn fold_key(pop: Pop, ev: web_sys::KeyboardEvent) {
    if ev.default_prevented() || !pop.is_open() || !bindings::is(&ev, Context::Global, "app.escape") {
        return;
    }
    ev.prevent_default();
    ev.stop_propagation();
    pop.close(true);
}

/// A level's `Filter` button: it opens the level's panel, or folds it.
#[component]
pub(crate) fn FilterButton(level: Level, #[prop(into)] hint: String, #[prop(optional)] badge: bool) -> impl IntoView {
    let lens = lens::use_lens();
    let pop = lens.pop(level);
    let count = move || lens.clause_count(level);
    view! {
        <button
            class="fbtn"
            data-fbtn=level.name()
            data-fopen=level.name()
            title=hint
            aria-controls=pop_id(level)
            aria-expanded=move || pop.open_tracked().to_string()
            on:click=move |_| {
                if pop.is_open() {
                    lens.close_panel(true);
                } else {
                    lens.open_panel(level, None);
                }
            }
        >
            <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M2 3.5h12l-4.6 5.4v4.1l-2.8-1.3V8.9z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" />
            </svg>
            "Filter"
            {move || (badge && count() > 0).then(|| view! { <span class="fbadge" title="clauses in this filter">{count}</span> })}
        </button>
    }
}

/// Under the line — naming's prompt (a name already saved: replace it,
/// or another) and its note; with saved names, "as evaluated"; an edit
/// refused while the panel is folded.
#[component]
pub(crate) fn LineNotes(level: Level) -> impl IntoView {
    let lens = lens::use_lens();
    let pop = lens.pop(level);
    let naming = crate::naming::use_naming();
    view! {
        {move || naming.taken(level).map(|(typed, e)| view! {
            <div class="qtaken" role="alert" data-level=level.name()>
                <span>{crate::saved::taken_words(&typed, &e)}</span>
                <button class="primary" data-act="replace" disabled=move || naming.busy(level) on:click=move |_| naming.replace(level)>"Replace it"</button>
                <button data-act="another" on:click=move |_| {
                    naming.another(level);
                    if let Some(el) = document().and_then(|d| d.query_selector(&format!("[data-save-as=\"{}\"], [data-saved-as=\"{}\"]", level.name(), level.name())).ok().flatten()).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.click();
                    }
                }>"Choose another name"</button>
            </div>
        })}
        {move || naming.note(level).map(|(bad, words)| view! {
            <div class="qnote" class:sproblem=bad role="status" data-level=level.name()>
                {words}
                <button class="p-x" title="dismiss" aria-label="dismiss" on:click=move |_| naming.dismiss(level)>"\u{d7}"</button>
            </div>
        })}
        {evaluated_view(lens, level)}
        {refused_view(pop)}
    }
}

/// FILTER-37: "as evaluated" — the level's filter with its saved names
/// expanded ([`crate::naming::evaluated`]), "…" until every name's text is in.
fn evaluated_view(lens: Lens, level: Level) -> impl IntoView {
    let naming = crate::naming::use_naming();
    // FILTER-42: the names are fetched as the filter names them — an
    // effect of the filter alone, so a text arriving never asks again (the
    // view below only reads; drawing it fetched, it re-drew itself without
    // end once the line drew it unfolded).
    Effect::new(move |_| {
        let names: Vec<String> = lens.filter(level).with(|f| f.saved_names().into_iter().map(str::to_string).collect());
        untrack(|| {
            for n in &names {
                naming.fetch(n);
            }
        });
    });
    move || {
        let f = lens.filter(level).get();
        if f.saved_names().is_empty() {
            return None;
        }
        let said = match crate::naming::evaluated(&f, &|n| naming.lookup(n)) {
            Err(_) => "\u{2026}".to_string(),
            Ok(Ok(text)) => text,
            Ok(Err(why)) => why,
        };
        Some(view! {
            <div class="qeval" data-qeval=level.name()>
                <span class="qeval-l">"as evaluated: "</span>
                <code>{said}</code>
            </div>
        })
    }
}

/// An edit refused, under the bar while the panel is folded (the panel
/// shows it while open).
fn refused_view(pop: Pop) -> impl IntoView {
    move || {
        (!pop.open_tracked()).then(|| pop.refusal.get()).flatten().map(|why| {
            view! {
                <span class="frefused" role="alert">
                    {why}
                    <button class="p-x" title="dismiss" aria-label="dismiss" on:click=move |_| pop.refusal.set(None)>"\u{d7}"</button>
                </span>
            }
        })
    }
}

/// `reset` (a surface) or `clear` (global): back to the level's default,
/// shown while the level differs from it.
#[component]
pub(crate) fn Reset(level: Level) -> impl IntoView {
    let lens = lens::use_lens();
    let (word, title) = if level == Level::Global {
        ("clear", "clear the global filter")
    } else {
        ("reset", "back to this view's default")
    };
    move || {
        lens.differs(level).then(|| {
            view! {
                <button class="reset" title=title on:click=move |_| {
                    lens.reset(level);
                    lens.pop(level).sel.set(0);
                }>
                    {word}
                </button>
            }
        })
    }
}

/// Focus `field` each time it is drawn. FILTER-6: `NodeRef::on_load` runs
/// its callback once and panics (`Option::unwrap` on `None`) when the same
/// ref is drawn again — the text mode opened a second time on one bar, the
/// search drawn again after Title's field.
fn focus_each_load(field: NodeRef<leptos::html::Input>) {
    Effect::new(move |_| {
        if let Some(el) = field.get() {
            let _ = el.focus();
        }
    });
}

/// FILTER-43: what a fix says on its button — `use build`, `close it`,
/// `remove it`.
pub(crate) fn fix_words(fix: &str) -> String {
    match fix {
        "" => "remove it".into(),
        f if f.chars().all(|c| c == ')') => "close it".into(),
        f => format!("use {f}"),
    }
}

/// FILTER-43: the text with each mistake marked — the runs between them
/// plain, each mistake's span (a caret where something is missing) marked;
/// overlapping marks after the first dropped.
pub(crate) fn marked(text: &str, mistakes: &[menger_tickets_api::filter::Mistake]) -> Vec<(bool, String)> {
    let mut ms: Vec<&menger_tickets_api::filter::Mistake> = mistakes.iter().filter(|m| m.at + m.len <= text.len()).collect();
    ms.sort_by_key(|m| m.at);
    let (mut out, mut at) = (Vec::new(), 0usize);
    for m in ms {
        if m.at < at || !text.is_char_boundary(m.at) || !text.is_char_boundary(m.at + m.len) {
            continue;
        }
        if m.at > at {
            out.push((false, text[at..m.at].to_string()));
        }
        out.push((true, if m.len == 0 { "\u{2038}".to_string() } else { text[m.at..m.at + m.len].to_string() }));
        at = m.at + m.len;
    }
    if at < text.len() {
        out.push((false, text[at..].to_string()));
    }
    out
}

/// FILTER-43: the text drawer at the panel's foot — the level's filter as
/// canonical text, edited in place. As you type, each mistake is
/// underlined where it is and listed once, with its fix (`use build`,
/// `close it`; a click puts it in) — the language's placed mistakes, never
/// its grammar. Enter (or apply) applies the text once it reads; until
/// then the level keeps the last filter that read, and its chips say so.
/// Escape leaves the drawer.
#[component]
pub(crate) fn TextDrawer(level: Level) -> impl IntoView {
    let lens = lens::use_lens();
    let pop = lens.pop(level);
    let draft = RwSignal::new(untrack(|| lens.text(level)));
    let field = NodeRef::<leptos::html::Textarea>::new();
    Effect::new(move |_| {
        if let Some(el) = field.get() {
            let _ = el.focus();
        }
    });
    let read = Memo::new(move |_| draft.with(|t| menger_tickets_api::filter::Filter::parse(t).err()));
    // The chips line follows: stale while the draft does not read.
    Effect::new(move |_| pop.stale.set(read.with(Option::is_some)));
    on_cleanup(move || {
        pop.stale.try_set(false);
    });
    let apply = move || {
        if read.with_untracked(Option::is_some) {
            return;
        }
        if lens.apply_text(level, &draft.get_untracked()).is_ok() {
            pop.sel.set(0);
            draft.set(untrack(|| lens.text(level)));
        }
    };
    let fix = move |at: usize, len: usize, with: String| {
        draft.update(|t| {
            if at + len <= t.len() && t.is_char_boundary(at) && t.is_char_boundary(at + len) {
                t.replace_range(at..at + len, &with);
            }
        });
        if let Some(el) = field.get_untracked() {
            let _ = el.focus();
        }
    };
    view! {
        <div class="fp-drawer" data-level=level.name()>
            <textarea
                class="fdraft"
                class:bad=move || read.with(Option::is_some)
                node_ref=field
                rows="2"
                spellcheck="false"
                aria-label=format!("the {} filter as text", level.name())
                placeholder="(kind build and status Ready) or kind gap"
                prop:value=move || draft.get()
                on:input=move |ev| draft.set(event_target_value(&ev))
                on:keydown=move |ev: web_sys::KeyboardEvent| match bindings::hit(&ev, Context::Field) {
                    Some("form.submit") => {
                        ev.prevent_default();
                        apply();
                    }
                    Some("form.cancel") => {
                        ev.prevent_default();
                        ev.stop_propagation();
                        pop.drawer.set(false);
                    }
                    _ => {}
                }
            ></textarea>
            {move || read.get().map(|e| {
                let text = draft.get();
                let all: Vec<menger_tickets_api::filter::Mistake> = e.chunks.iter().flat_map(|c| c.mistakes.iter().cloned()).collect();
                let runs = marked(&text, &all);
                let lines = e
                    .chunks
                    .iter()
                    .flat_map(|c| {
                        if c.mistakes.is_empty() {
                            // A chunk read no closer: its own words, shortened.
                            let why = if c.why.starts_with("not understood: a predicate is") { "not understood".to_string() } else { c.why.clone() };
                            vec![(format!("`{}` \u{2014} {why}", c.text), None)]
                        } else {
                            c.mistakes.iter().map(|m| (m.words.clone(), m.fix.clone().map(|f| (m.at, m.len, f)))).collect()
                        }
                    })
                    .map(|(words, f)| {
                        let parts: Vec<(bool, String)> = words.split('`').enumerate().map(|(i, p)| (i % 2 == 1, p.to_string())).collect();
                        view! {
                            <li>
                                {parts.into_iter().map(|(code, p)| if code { view! { <b>{p}</b> }.into_any() } else { view! { <span>{p}</span> }.into_any() }).collect_view()}
                                {f.map(|(at, len, with)| {
                                    let label = fix_words(&with);
                                    view! { <button type="button" class="ffix" on:click=move |_| fix(at, len, with.clone())>{label}</button> }
                                })}
                            </li>
                        }
                    })
                    .collect_view();
                view! {
                    <div class="fmarked" aria-hidden="true">
                        {runs.into_iter().map(|(bad, t)| if bad { view! { <span class="fbad">{t}</span> }.into_any() } else { view! { <span>{t}</span> }.into_any() }).collect_view()}
                    </div>
                    <ul class="fmistakes" role="alert">{lines}</ul>
                }
            })}
            <div class="fp-drawer-foot">
                <button type="button" class="fapply" disabled=move || read.with(Option::is_some) on:click=move |_| apply()>"apply"</button>
                <span class="fhint">"Enter applies \u{00b7} the chips keep the last filter that read"</span>
            </div>
        </div>
    }
}

/// Under a surface's bar: "Nothing can match" and the clashing groups, each
/// marked with its level, while the surface's effective filter admits
/// nothing.
#[component]
pub(crate) fn Clash(level: Level) -> impl IntoView {
    let lens = lens::use_lens();
    move || {
        lens.clash(level).map(|groups| {
            view! {
                <div class="fclash" role="status" data-level=level.name()>
                    <strong>"Nothing can match"</strong>
                    " — these exclude each other: "
                    {groups.into_iter().enumerate().map(|(i, (text, levels))| view! {
                        {(i > 0).then_some(" · ")}
                        <span class="fclash-g">
                            {levels.iter().map(|l| view! { <span class="p-lvl">{l.name()}</span> }).collect_view()}
                            <code>{text}</code>
                        </span>
                    }).collect_view()}
                </div>
            }
        })
    }
}

// ---------------------------------------------------------------------
// The view: the panel (FILTER-39)

/// A section of the panel: a category with values, the flags together,
/// a time category's pane, or Title's field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    /// The saved filters, by name — each a group (FILTER-38's mockups).
    Saved,
    Values(Cat),
    Flags,
    Time(Cat),
    Title,
}

impl Section {
    /// Its key, as the DOM's `data-sec` says it.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Section::Values(c) | Section::Time(c) => c.key(),
            Section::Flags => "flags",
            Section::Title => "q",
            Section::Saved => "saved",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Section::Values(c) | Section::Time(c) => c.label(),
            Section::Flags => "Flags",
            Section::Title => "Title contains",
            Section::Saved => "Saved",
        }
    }

    /// Whether `c`'s values are in it.
    pub(crate) fn holds(self, c: Cat) -> bool {
        match self {
            Section::Values(x) | Section::Time(x) => x == c,
            Section::Flags => c.is_flag() && !c.is_local(),
            Section::Title => c == Cat::Title,
            Section::Saved => false,
        }
    }
}

/// The panel's sections for a level's categories, in their order — the
/// saved filters first where the server reads them (`saved` resolves
/// there): each category with values its own, the server's flags gathered into one
/// (where the first of them stands), When, Deployed and Chats their panes,
/// Title its field.
pub(crate) fn sections(level: Level, cats: &[Cat]) -> Vec<Section> {
    let mut out = Vec::new();
    if matches!(level, Level::Global | Level::Nav | Level::Board) {
        out.push(Section::Saved);
    }
    for &c in cats {
        let s = match c {
            Cat::When | Cat::Deployed | Cat::Chats => Section::Time(c),
            Cat::Title => Section::Title,
            c if c.is_flag() && !c.is_local() => Section::Flags,
            c => Section::Values(c),
        };
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// A saved filter's row: ticked, it is a group by name (it fills the
/// selected group while that is empty).
fn saved_row(lens: Lens, pop: Pop, name: String) -> impl IntoView {
    let level = pop.level;
    let on = {
        let name = name.clone();
        Memo::new(move |_| {
            let sel = pop.sel.get();
            lens.ors(level).and_then(|o| o.groups.get(sel).cloned()).is_some_and(|g| {
                g.iter().any(|p| matches!(&p.field, menger_tickets_api::filter::Field::Saved(n) if crate::saved::slug(n) == crate::saved::slug(&name)))
            })
        })
    };
    let elsewhere = {
        let name = name.clone();
        move || {
            let sel = pop.sel.get();
            let gs: Vec<String> = lens
                .ors(level)
                .map(|o| {
                    o.groups
                        .iter()
                        .enumerate()
                        .filter(|(i, g)| *i != sel && g.iter().any(|p| matches!(&p.field, menger_tickets_api::filter::Field::Saved(n) if crate::saved::slug(n) == crate::saved::slug(&name))))
                        .map(|(i, _)| (i + 1).to_string())
                        .collect()
                })
                .unwrap_or_default();
            (!gs.is_empty()).then(|| view! { <span class="fv-in">{format!("in group {}", gs.join(", "))}</span> })
        }
    };
    let naming = crate::naming::use_naming();
    untrack(|| naming.ask_count(&name));
    let count = {
        let name = name.clone();
        move || naming.count(&name).flatten().map(|n| n.to_string()).unwrap_or_default()
    };
    let click = {
        let name = name.clone();
        move |_| {
            naming.touch(&name);
            pop.check_saved(lens, &name)
        }
    };
    view! {
        <button
            type="button"
            class="fv fv-saved"
            data-cat="saved"
            data-value=name.clone()
            data-mark=move || if on.get() { "on" } else { "off" }
            aria-pressed=move || on.get().to_string()
            on:click=click
        >
            <span class="box round" data-state=move || if on.get() { "on" } else { "off" } aria-hidden="true"></span>
            <span class="flabel"><span>{name.clone()}</span>{elsewhere}</span>
            <span class="fcount">{count}</span>
        </button>
    }
}

/// A flag's row in the Flags section: what it says when it holds.
pub(crate) fn flag_words(c: Cat) -> &'static str {
    match c {
        Cat::Ready => "ready",
        Cat::Blocked => "blocked",
        Cat::Commits => "has commits",
        Cat::MengerDocs => "menger doc",
        Cat::Asking => "asking",
        Cat::Removed => "removed shown",
        _ => c.label(),
    }
}

/// A row of a section, as the panel draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PanelRow {
    /// A value to tick — `depth` indents a shortcode under its host.
    Value { cat: Cat, token: String, depth: usize },
    /// A heading over values (Status's groups, Shortcode's "other boards").
    Heading(&'static str),
}

/// The rows a value section offers, narrowed by what is typed (the
/// headings go while it narrows): the menu's own values ([`offered`]),
/// Status's groups as headings.
pub(crate) fn section_rows(c: Cat, k: &MenuFacts, q: &str) -> Vec<PanelRow> {
    let q = q.trim();
    let by_name = !q.is_empty() && palette_match(c.menu_label(), c.keywords(), q);
    offered(c, k)
        .into_iter()
        .filter_map(|r| match r {
            Row::Value { cat, token, depth, .. } if q.is_empty() || by_name || palette_match(&token, &cat.value_label(&token), q) => {
                Some(PanelRow::Value { cat, token, depth: if q.is_empty() { depth } else { 0 } })
            }
            Row::Group { label, .. } if q.is_empty() => Some(PanelRow::Heading(label)),
            Row::Heading(h) if q.is_empty() => Some(PanelRow::Heading(h)),
            _ => None,
        })
        .collect()
}

/// The flags a level offers, narrowed by what is typed.
pub(crate) fn flag_rows(cats: &[Cat], q: &str) -> Vec<Cat> {
    let q = q.trim();
    cats.iter()
        .copied()
        .filter(|c| c.is_flag() && !c.is_local())
        .filter(|c| q.is_empty() || palette_match(flag_words(*c), c.label(), q) || palette_match("flags", "", q))
        .collect()
}

/// FILTER-40: where the one panel is drawn — under the route's surface
/// line (the board's, a wave board's, the inputs page's), or under the
/// header (a page with no line; at phone widths always: the filter sheet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Host {
    Top,
    Surface,
}

/// FILTER-40: the panel's host: the phone's sheet, else the surface's line
/// where the route draws one, else under the header.
pub(crate) fn host_of(phone: bool, surface_line: bool) -> Host {
    match (phone, surface_line) {
        (false, true) => Host::Surface,
        _ => Host::Top,
    }
}

/// FILTER-40: the global filter as a dashed chip, read-only — on every
/// surface's line and in the nav's head; a click opens the panel on its ⌂
/// everywhere tab.
pub(crate) fn global_chip(lens: Lens, text: String) -> impl IntoView {
    view! {
        <button
            type="button"
            class="fchip-g"
            data-fopen="global"
            title=format!("the filter everywhere \u{2014} every view shows what it admits: {text}")
            on:click=move |_| lens.open_panel(Level::Global, None)
        >
            <span class="fchip-g-h" aria-hidden="true">"\u{2302}"</span>
            <span class="fchip-g-w">{text.clone()}</span>
        </button>
    }
}

/// FILTER-40: the one panel, drawn here while `host` is where it goes —
/// in the page's flow (FILTER-39), editing the level whose tab is lit.
#[component]
pub(crate) fn PanelHost(host: Host) -> impl IntoView {
    let lens = lens::use_lens();
    let here = Memo::new(move |_| (lens.host() == host).then(|| lens.open_level()).flatten());
    // The search takes focus as the panel opens or changes tab; a section
    // asked for is scrolled to and lit.
    Effect::new(move |_| {
        let Some(level) = here.get() else { return };
        let pop = lens.pop(level);
        let focus = pop.focus.get();
        request_animation_frame(move || {
            let Some(doc) = document() else { return };
            let id = pop_id(level);
            if let Some(c) = focus {
                let s = sections(level, &pop.cats.get_untracked()).into_iter().find(|s| s.holds(c));
                if let Some(el) = s.and_then(|s| doc.query_selector(&format!("#{id} [data-sec=\"{}\"]", s.key())).ok().flatten()) {
                    let o = web_sys::ScrollIntoViewOptions::new();
                    o.set_block(web_sys::ScrollLogicalPosition::Nearest);
                    el.scroll_into_view_with_scroll_into_view_options(&o);
                    crate::motion::replay(&el, "lit", 1200);
                }
            }
            if let Some(el) = doc.query_selector(&format!("#{id} input.fsearch")).ok().flatten().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
                let _ = el.focus();
            }
        });
    });
    move || here.get().map(|level| panel(lens, lens.pop(level)))
}

/// FILTER-40: a level as its tab says it.
pub(crate) fn tab_words(level: Level) -> &'static str {
    match level {
        Level::Global => "everywhere",
        Level::Nav => "nav",
        Level::Board => "this board",
        Level::Waves => "this wave board",
        Level::Inputs => "the inputs",
    }
}

/// FILTER-40: the panel's tabs — each level it edits, with what that level
/// shows (`global ∧ own`), counted; the open one lit.
fn tabs_view(lens: Lens, open: Level) -> impl IntoView {
    view! {
        <span class="fp-tabs" role="tablist" aria-label="which filter the panel edits">
            {move || lens.tabs().into_iter().map(|l| {
                let n = tab_count(lens, l);
                view! {
                    <button
                        type="button"
                        class="fp-tab"
                        class:on=l == open
                        role="tab"
                        data-tab=l.name()
                        aria-selected=(l == open).to_string()
                        title=format!("edit the filter {}", if l == Level::Global { "every view shows".to_string() } else { format!("of {}", tab_words(l)) })
                        on:click=move |_| if l != open { lens.open_panel(l, None) }
                    >
                        {(l == Level::Global).then(|| view! { <span class="fp-home" aria-hidden="true">"\u{2302}"</span> })}
                        {tab_words(l)}
                        <b>{move || n.get().map(|n| n.to_string()).unwrap_or_default()}</b>
                    </button>
                }
            }).collect_view()}
        </span>
    }
}

/// FILTER-40: what `level` shows — its effective filter counted, asked
/// again whenever it changes (an empty one: every ticket).
pub(crate) fn tab_count(lens: Lens, level: Level) -> RwSignal<Option<usize>> {
    let n = RwSignal::new(None);
    let text = Memo::new(move |_| lens.effective(level).with(|f| (f.admits_nothing().is_none()).then(|| crate::counts::wire_text(f))));
    let asked = StoredValue::new(0u32);
    Effect::new(move |_| {
        let t = text.get();
        crate::session::link().track();
        let k = asked.get_value().wrapping_add(1);
        asked.set_value(k);
        match t {
            None => n.set(Some(0)),
            Some(t) => wasm_bindgen_futures::spawn_local(async move {
                let got = crate::counts::count_once(t).await;
                if asked.try_get_value() == Some(k) {
                    n.try_set(got);
                }
            }),
        }
    });
    n
}

fn panel(lens: Lens, pop: Pop) -> impl IntoView {
    let level = pop.level;
    let label = if level == Level::Global { "Filter every view".to_string() } else { format!("Filter the {}", level.name()) };
    let secs = Memo::new(move |_| sections(level, &pop.cats.get()));
    view! {
        <section
            id=pop_id(level)
            class="fpanel"
            data-level=level.name()
            role="region"
            aria-label=label
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                if bindings::is(&ev, Context::Menu, "dialog.close") && !ev.default_prevented() {
                    ev.prevent_default();
                    ev.stop_propagation();
                    lens.close_panel(true);
                }
            }
        >
            <div class="fp-head">
                <span class="fp-title">"Filter"</span>
                {tabs_view(lens, level)}
                <input
                    class="fsearch"
                    type="text"
                    autocomplete="off"
                    spellcheck="false"
                    aria-label="search every value"
                    placeholder="search every value\u{2026}"
                    prop:value=move || pop.query.get()
                    on:input=move |ev| pop.query.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        // Enter ticks the first value the search leaves.
                        if bindings::is(&ev, Context::Field, "form.submit") {
                            ev.prevent_default();
                            let first = document().and_then(|d| d.query_selector(&format!("#{} .fv", pop_id(level))).ok().flatten());
                            if let Some(el) = first.and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
                                el.click();
                            }
                        }
                    }
                />
                <button class="fp-x" title="fold the panel (Escape)" aria-label="fold the panel" on:click=move |_| lens.close_panel(true)>"\u{d7}"</button>
            </div>
            {(level == Level::Nav).then(|| view! { <crate::promote::MakeGlobal menu=true /> })}
            {groups_view(lens, pop)}
            {move || pop.note.get().map(|w| view! { <div class="fp-note" role="status">{w}</div> })}
            {move || pop.refusal.get().map(|w| view! { <div class="frefused" role="alert">{w}</div> })}
            <div class="fp-flow">
                {move || secs.get().into_iter().map(|s| section_view(lens, pop, s)).collect_view()}
            </div>
            <div class="fp-foot">
                <button
                    type="button"
                    class="fp-text"
                    data-ftext=level.name()
                    aria-expanded=move || pop.drawer.get().to_string()
                    title="the filter as text, in FILTER-3's language"
                    on:click=move |_| pop.drawer.update(|d| *d = !*d)
                >
                    {move || if pop.drawer.get() { "\u{25be} edit as text" } else { "\u{25b8} edit as text" }}
                </button>
                <Reset level />
                <SaveAs level />
                <span class="fp-count">{move || foot_words(lens, level)}</span>
            </div>
            {move || pop.drawer.get().then(|| view! { <TextDrawer level /> })}
        </section>
    }
}

/// The foot's words: how many clauses, in how many groups.
fn foot_words(lens: Lens, level: Level) -> String {
    match lens.ors(level) {
        None => "deeper than groups joined by or \u{2014} edit it as text".into(),
        Some(o) => {
            let groups = o.live().count();
            match (o.clauses(), groups) {
                (0, _) => "no clause \u{2014} every ticket".into(),
                (n, 1) => format!("{n} clause{}", if n == 1 { "" } else { "s" }),
                (n, g) => format!("{n} clauses in {g} groups joined by or"),
            }
        }
    }
}

/// The groups, joined by **or**, each a row of its clauses joined by
/// **and** — the selected one lit; × on a clause, on a group; `+ or`.
fn groups_view(lens: Lens, pop: Pop) -> impl IntoView {
    let level = pop.level;
    move || {
        let Some(o) = lens.ors(level) else {
            // FILTER-42: past what the panel draws, the filter is its text —
            // read here, edited in the foot's text, never rewritten.
            let text = lens.filter(level).with(|f| f.format());
            return view! {
                <div class="fp-groups deep" role="note">
                    <span class="fp-hint">"This filter spells out into more groups than the panel draws: it is shown as its text, and edited as text in the foot."</span>
                    <code class="fp-deep">{text}</code>
                </div>
            }
            .into_any();
        };
        let mut groups = o.groups.clone();
        if groups.is_empty() {
            groups.push(Vec::new());
        }
        let n = groups.len();
        let sel = pop.sel.get().min(n - 1);
        let rows = groups
            .into_iter()
            .enumerate()
            .map(|(gi, g)| {
                let empty = g.is_empty();
                let items = g
                    .into_iter()
                    .enumerate()
                    .map(|(ii, p)| {
                        let words = crate::ors::words(&p);
                        let time = crate::ors::cat_of(&p).filter(|c| matches!(c, Cat::When | Cat::Deployed | Cat::Chats));
                        let slot = crate::ors::Slot { group: gi, item: ii };
                        let saved = matches!(p.field, menger_tickets_api::filter::Field::Saved(_));
                        let remove = move |ev: web_sys::MouseEvent| {
                            ev.stop_propagation();
                            pop.note.set(None);
                            let r = lens.edit_ors(level, |o| {
                                o.remove(slot);
                                Ok(())
                            });
                            pop.answer(r);
                            let left = untrack(|| lens.ors(level)).map_or(1, |o| o.groups.len().max(1));
                            pop.sel.update(|s| *s = (*s).min(left - 1));
                        };
                        let open_time = move |ev: web_sys::MouseEvent| {
                            if let Some(c) = time {
                                ev.stop_propagation();
                                pop.sel.set(gi);
                                pop.when.set((c, crate::when_view::Step::Edit { at: slot, only: None }));
                                lens.open_panel(level, Some(c));
                            }
                        };
                        // FILTER-42: a saved filter in a group shows what it holds.
                        let peek = match &p.field {
                            menger_tickets_api::filter::Field::Saved(n) => Some(saved_peek(n.clone())),
                            _ => None,
                        };
                        view! {
                            {(ii > 0).then(|| view! { <span class="fw">"and"</span> })}
                            <span class="fchip" class:saved=saved data-group=gi data-item=ii>
                                <span class="fchip-w" class:part=time.is_some() on:click=open_time>{words.clone()}</span>
                                <button class="p-x" title=format!("remove {words}") aria-label=format!("remove {words}") on:click=remove>"\u{d7}"</button>
                            </span>
                            {peek}
                        }
                    })
                    .collect_view();
                let pick = move |_| {
                    pop.sel.set(gi);
                    pop.note.set(None);
                };
                let drop_group = move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    pop.note.set(None);
                    let r = lens.edit_ors(level, |o| {
                        o.remove_group(gi);
                        Ok(())
                    });
                    pop.answer(r);
                    pop.sel.update(|s| *s = s.saturating_sub(usize::from(*s >= gi && *s > 0)));
                };
                view! {
                    {(gi > 0).then(|| view! { <span class="for">"or"</span> })}
                    <div class="fgrp" class:sel=gi == sel data-group=gi on:click=pick>
                        <button
                            type="button"
                            class="fgrp-n"
                            aria-pressed=(gi == sel).to_string()
                            title=format!("tick values into group {}", gi + 1)
                        >
                            {format!("group {}", gi + 1)}
                        </button>
                        {if empty { view! { <span class="fgrp-empty">"tick values below"</span> }.into_any() } else { items.into_any() }}
                        {(n > 1 || !empty).then(|| view! {
                            <button class="p-x fgrp-x" title=format!("remove group {}", gi + 1) aria-label=format!("remove group {}", gi + 1) on:click=drop_group>"\u{d7}"</button>
                        })}
                    </div>
                }
            })
            .collect_view();
        let add = move |_| {
            pop.note.set(None);
            let r = lens.edit_ors(level, |o| Ok(o.add_group()));
            if let Ok(at) = &r {
                pop.sel.set(*at);
            }
            pop.answer(r);
        };
        view! {
            <div class="fp-groups" role="group" aria-label="groups, joined by or">
                <span class="fp-hint">"Clauses in a group are joined by "<b>"and"</b>"; groups are joined by "<b class="for">"or"</b>". Values go into the selected group."</span>
                {rows}
                <button class="fp-addor" title="start another group, joined by or" on:click=add>"+ or"</button>
            </div>
        }
        .into_any()
    }
}

/// FILTER-42: what a saved filter in a group holds — its text, read the
/// first time it is shown (`…` until then; said when no filter has the
/// name).
fn saved_peek(name: String) -> impl IntoView {
    let naming = crate::naming::use_naming();
    untrack(|| naming.fetch(&name));
    move || {
        let words = match naming.text(&name) {
            Some(crate::naming::Fetch::Text(t)) => format!("= {t}"),
            Some(crate::naming::Fetch::Missing) => "no saved filter has this name".to_string(),
            Some(crate::naming::Fetch::Failed(why)) => format!("not read: {why}"),
            _ => "= \u{2026}".to_string(),
        };
        view! { <span class="fp-peek" title="what the saved filter holds">{words}</span> }
    }
}

/// A section: its head (its name; `is / is not` where it has values to
/// negate) and its rows — hidden while the search leaves it none.
fn section_view(lens: Lens, pop: Pop, s: Section) -> impl IntoView {
    let level = pop.level;
    move || {
        let q = pop.query.get();
        let body = match s {
            Section::Saved => {
                let naming = crate::naming::use_naming();
                let recent = naming.recent();
                let mut names: Vec<String> = crate::saved::hook().map(|h| h.listed().into_iter().map(|e| e.name).collect()).unwrap_or_default();
                // The ones used lately first, then the rest in the list's order.
                names.sort_by_key(|n| recent.iter().position(|r| crate::saved::slug(r) == crate::saved::slug(n)).unwrap_or(usize::MAX));
                let names: Vec<String> = names.into_iter().filter(|n| q.trim().is_empty() || palette_match(n, "saved", &q)).collect();
                if names.is_empty() {
                    return None;
                }
                names.into_iter().map(|n| saved_row(lens, pop, n)).collect_view().into_any()
            }
            Section::Values(c) => {
                let rows = pop.facts(lens, |k| section_rows(c, k, &q));
                if rows.iter().all(|r| matches!(r, PanelRow::Heading(_))) {
                    return None;
                }
                rows.into_iter().map(|r| row_view(lens, pop, r)).collect_view().into_any()
            }
            Section::Flags => {
                let flags = pop.cats.with(|cs| flag_rows(cs, &q));
                if flags.is_empty() {
                    return None;
                }
                flags.into_iter().map(|c| flag_view(lens, pop, c)).collect_view().into_any()
            }
            Section::Time(c) => {
                if !q.trim().is_empty() && !palette_match(c.menu_label(), c.keywords(), &q) {
                    return None;
                }
                crate::when_view::pane(lens, pop, c).into_any()
            }
            Section::Title => {
                if !q.trim().is_empty() && !palette_match("title", "contains text", &q) {
                    return None;
                }
                title_view(lens, pop).into_any()
            }
        };
        let switch = match s {
            Section::Values(c) => switch_words(c).map(|w| (c, w)),
            Section::Flags => Some((Cat::Ready, ("is", "is not"))),
            Section::Title => Some((Cat::Title, (op_label(Cat::Title, Op::Is, 1), op_label(Cat::Title, Op::Not, 1)))),
            Section::Time(_) | Section::Saved => None,
        };
        let flags = s == Section::Flags;
        Some(view! {
            <div class="fsec" data-sec=s.key() data-level=level.name()>
                <h5 class="fsec-h">
                    <span>{s.label()}</span>
                    {switch.map(|(c, (is, not))| {
                        // The Flags section's switch is one for every flag.
                        let op = move || pop.op(c);
                        let set = move |o: Op| {
                            if flags {
                                pop.ops.update(|m| {
                                    m.insert(Cat::Ready, o);
                                });
                            } else {
                                pop.set_op(lens, c, o);
                            }
                        };
                        view! {
                            <span class="fop" role="group" aria-label=format!("{}: {is} or {not}", s.label()) data-cat=c.key()>
                                <button type="button" class:on=move || op() == Op::Is aria-pressed=move || (op() == Op::Is).to_string() on:click=move |_| set(Op::Is)>{is}</button>
                                <button type="button" class:on=move || op() == Op::Not aria-pressed=move || (op() == Op::Not).to_string() on:click=move |_| set(Op::Not)>{not}</button>
                            </span>
                        }
                    })}
                </h5>
                {body}
            </div>
        })
    }
}

impl Pop {
    /// The panel's facts, read (and tracked) for `f`: the level's own
    /// categories, the route's shortcode, the counts, and — what is
    /// checked, for a value no group knows — every clause the groups hold.
    fn facts<T>(self, lens: Lens, f: impl FnOnce(&MenuFacts) -> T) -> T {
        let route = lens.route_sc(self.level);
        let counts = self.counts().get();
        let clauses = lens.clauses(self.level);
        let cats = self.cats.get();
        lens.sctree().with(|tree| f(&MenuFacts { cats: &cats, route: route.as_deref(), tree: tree.as_ref(), clauses: &clauses, counts: &counts }))
    }

    /// FILTER-39: what ticking `c`'s `token` would leave on the surface —
    /// asked once per value hovered (a server-backed level's).
    fn hovered(self, lens: Lens, c: Cat, token: &str) {
        if !matches!(self.level, Level::Global | Level::Nav | Level::Board) || c.is_local() {
            return;
        }
        let key = format!("{}:{token}", c.key());
        if self.hover.with_untracked(|h| h.as_ref().is_some_and(|(k, _)| *k == key)) {
            return;
        }
        let tree = lens.sctree().get_untracked();
        let sel = self.sel.get_untracked();
        let op = self.op(c);
        let text = lens.would_be(self.level, |o| o.pick(sel, c, token, op, tree.as_ref()).map(|_| ()));
        self.hover.set(Some((key.clone(), None)));
        let Some(text) = text else { return };
        wasm_bindgen_futures::spawn_local(async move {
            let n = crate::counts::count_once(text).await;
            if self.hover.try_with_untracked(|h| h.as_ref().is_some_and(|(k, _)| *k == key)) == Some(true) {
                self.hover.try_set(Some((key, n)));
            }
        });
    }

    /// The count a value's row shows: what ticking it would leave while it
    /// is hovered (once in), else its count. Tracked.
    fn shown_count(self, c: Cat, token: &str) -> (String, bool) {
        let key = format!("{}:{token}", c.key());
        if let Some(Some(n)) = self.hover.with(|h| h.as_ref().filter(|(k, _)| *k == key).map(|(_, n)| *n)) {
            return (format!("\u{2192} {n}"), true);
        }
        (self.counts().count(c, token).map(|n| n.to_string()).unwrap_or_default(), false)
    }

    /// `c`'s `token`'s box in the selected group (a surface's own: its
    /// clause's). Tracked.
    fn mark(self, lens: Lens, c: Cat, token: &str) -> crate::ors::Mark {
        use crate::ors::Mark;
        if c.is_local() {
            return match lens.clauses(self.level).get(c) {
                Some(cl) if cl.values.iter().any(|v| v == token) => {
                    if cl.op == Op::Is || c.is_flag() {
                        Mark::On
                    } else {
                        Mark::Not
                    }
                }
                _ => Mark::Off,
            };
        }
        let sel = self.sel.get();
        lens.sctree().with(|t| lens.ors(self.level).map_or(Mark::Off, |o| o.state(sel, c, token, t.as_ref())))
    }

    /// The other groups holding `c`'s `token`. Tracked.
    fn elsewhere(self, lens: Lens, c: Cat, token: &str) -> Vec<usize> {
        if c.is_local() {
            return Vec::new();
        }
        let sel = self.sel.get();
        lens.sctree().with(|t| lens.ors(self.level).map(|o| o.elsewhere(sel, c, token, t.as_ref())).unwrap_or_default())
    }
}

/// A value's row: its box, its face, the other groups that hold it, its
/// count (what ticking it would leave, while hovered).
fn row_view(lens: Lens, pop: Pop, row: PanelRow) -> AnyView {
    match row {
        PanelRow::Heading(h) => view! { <div class="fsec-sub">{h}</div> }.into_any(),
        PanelRow::Value { cat: c, token, depth } => {
            let mark = {
                let token = token.clone();
                Memo::new(move |_| pop.mark(lens, c, &token))
            };
            let elsewhere = {
                let token = token.clone();
                move || {
                    let gs = pop.elsewhere(lens, c, &token);
                    (!gs.is_empty()).then(|| {
                        let n = gs.iter().map(usize::to_string).collect::<Vec<_>>().join(", ");
                        view! { <span class="fv-in">{format!("in group {n}")}</span> }
                    })
                }
            };
            let count = {
                let token = token.clone();
                Memo::new(move |_| pop.shown_count(c, &token))
            };
            let click = {
                let token = token.clone();
                move |_| pop.check(lens, c, &token)
            };
            let enter = {
                let token = token.clone();
                move |_| pop.hovered(lens, c, &token)
            };
            let pad = format!("padding-left: {}px", 4 + depth * 14);
            view! {
                <button
                    type="button"
                    class="fv"
                    data-cat=c.key()
                    data-value=token.clone()
                    data-mark=move || mark.get().as_str()
                    aria-pressed=move || (mark.get() != crate::ors::Mark::Off).to_string()
                    style=pad
                    on:click=click
                    on:mouseenter=enter
                    on:mouseleave=move |_| pop.hover.set(None)
                >
                    <span class="box" data-state=move || mark.get().as_str() aria-hidden="true"></span>
                    <span class="flabel">{value_face(c, &token)}{elsewhere}</span>
                    <span class="fcount" class:delta=move || count.get().1>{move || count.get().0}</span>
                </button>
            }
            .into_any()
        }
    }
}

/// A flag's row in the Flags section: ticked, it holds (`is`) or does not
/// (`is not`, the section's switch).
fn flag_view(lens: Lens, pop: Pop, c: Cat) -> impl IntoView {
    use crate::ors::Mark;
    let mark = Memo::new(move |_| match (pop.mark(lens, c, "yes"), pop.mark(lens, c, "no")) {
        (Mark::On, _) => Mark::On,
        (_, Mark::On) => Mark::Not,
        _ => Mark::Off,
    });
    let token = move || if pop.op(Cat::Ready) == Op::Not { "no" } else { "yes" };
    let count = Memo::new(move |_| pop.shown_count(c, token()));
    view! {
        <button
            type="button"
            class="fv"
            data-cat=c.key()
            data-value=move || token()
            data-mark=move || mark.get().as_str()
            aria-pressed=move || (mark.get() != Mark::Off).to_string()
            on:click=move |_| pop.check(lens, c, untrack(token))
            on:mouseenter=move |_| pop.hovered(lens, c, untrack(token))
            on:mouseleave=move |_| pop.hover.set(None)
        >
            <span class="box" data-state=move || mark.get().as_str() aria-hidden="true"></span>
            <span class="flabel">{flag_words(c)}</span>
            <span class="fcount" class:delta=move || count.get().1>{move || count.get().0}</span>
        </button>
    }
}

/// Title's section (the wave board's): its text, matched as you type —
/// under the section's switch (FILTER-19).
fn title_view(lens: Lens, pop: Pop) -> impl IntoView {
    let level = pop.level;
    let seed = untrack(|| lens.clauses(level).get(Cat::Title).map(|cl| cl.values[0].clone()).unwrap_or_default());
    view! {
        <div class="ftitle">
            <input
                class="ftitle-in"
                type="text"
                autocomplete="off"
                spellcheck="false"
                aria-label="title or id contains"
                placeholder="title or id\u{2026}"
                value=seed
                on:input=move |ev| {
                    let text = event_target_value(&ev);
                    let op = untrack(|| pop.op(Cat::Title));
                    lens.edit(level, |k| k.set(Cat::Title, op, [text].into_iter().filter(|t| !t.trim().is_empty())));
                }
            />
            <span class="fhint">"case-insensitive"</span>
        </div>
    }
}

/// A value as the panel draws it: a shortcode as its chip, a status as its
/// chip, a kind with its glyph.
fn value_face(c: Cat, token: &str) -> AnyView {
    let label = c.value_label(token);
    match c {
        Cat::Shortcode => view! { <span class="code">{label}</span> }.into_any(),
        Cat::Status => view! { <span class=format!("tag {}", st_class(token))>{label}</span> }.into_any(),
        Cat::Kind => {
            let kind = token.to_string();
            view! { <span class="kind" data-kind=kind.clone()><span class="kglyph">{kind_glyph(&kind)}</span>" "{label}</span> }.into_any()
        }
        _ => view! { <span>{label}</span> }.into_any(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn group(epic: &str, host: Option<&str>) -> (String, Group) {
        (
            epic.to_string(),
            Group {
                epic: menger_tickets_api::Code::stored(epic), shortcode: menger_tickets_api::ShortcodeId::new(epic), address: String::new(),
                total: 3,
                done: 0,
                docs: 0,
                archived: 0,
                root: Some(menger_tickets_api::TicketId::new(format!("{epic}-1"))),
                root_row: None,
                host: host.map(menger_tickets_api::TicketId::new),
            },
        )
    }

    /// The platform as the tracker holds it: PLEXUS hosts seven shortcodes
    /// under its root and its tickets; CODEGEN is hosted under a MACRO
    /// ticket (two levels down) and GEN under CODEGEN (three); MNG and WAVE
    /// are boards of their own, and LOOSE hangs under an MNG ticket.
    pub(crate) fn platform() -> ScTree {
        let g: BTreeMap<String, Group> = [
            group("PLEXUS", None),
            group("MACRO", Some("PLEXUS-1")),
            group("SYNAPSE", Some("PLEXUS-1")),
            group("TRANSPORT", Some("PLEXUS-1")),
            group("STARTER", Some("PLEXUS-1")),
            group("HYPERFORGE", Some("PLEXUS-1")),
            group("RETURNS", Some("PLEXUS-4")),
            group("CONTEXT", Some("PLEXUS-1")),
            group("CODEGEN", Some("MACRO-7")),
            group("GEN", Some("CODEGEN-2")),
            group("MNG", None),
            group("WAVE", None),
            group("LOOSE", Some("MNG-3")),
        ]
        .into_iter()
        .collect();
        ScTree::of(&g)
    }

    /// FILTER-38's mockups' shortcodes: KILN hosts GLAZE; PANTRY and
    /// ATLAS are boards of their own.
    pub(crate) fn kiln_tree() -> ScTree {
        let g: BTreeMap<String, Group> =
            [group("KILN", None), group("GLAZE", Some("KILN-1")), group("PANTRY", None), group("ATLAS", None)].into_iter().collect();
        ScTree::of(&g)
    }

    pub(crate) fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    pub(crate) fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    pub(crate) const PLEXUS_TREE: [&str; 10] =
        ["CODEGEN", "CONTEXT", "GEN", "HYPERFORGE", "MACRO", "PLEXUS", "RETURNS", "STARTER", "SYNAPSE", "TRANSPORT"];

    /// The shortcode tree: a shortcode's tree is it and every shortcode
    /// hosted under it, transitively (the board before `f528177`), depth
    /// first; an unknown shortcode is a tree of one.
    #[test]
    fn the_tree_of_a_shortcode_follows_the_host_chain() {
        let t = platform();
        let plexus: BTreeSet<String> = t.subtree("PLEXUS").into_iter().collect();
        assert_eq!(plexus, set(&PLEXUS_TREE));
        assert_eq!(
            t.walk("PLEXUS"),
            vec![
                ("PLEXUS".into(), 0),
                ("CONTEXT".into(), 1),
                ("HYPERFORGE".into(), 1),
                ("MACRO".into(), 1),
                ("CODEGEN".into(), 2),
                ("GEN".into(), 3),
                ("RETURNS".into(), 1),
                ("STARTER".into(), 1),
                ("SYNAPSE".into(), 1),
                ("TRANSPORT".into(), 1),
            ]
        );
        assert_eq!(t.subtree("MACRO"), strs(&["MACRO", "CODEGEN", "GEN"]));
        assert_eq!(t.subtree("MNG"), strs(&["MNG", "LOOSE"]));
        assert_eq!(t.subtree("WAVE"), strs(&["WAVE"]));
        assert_eq!(t.subtree("NOPE"), strs(&["NOPE"]), "an unknown shortcode is a tree of one");
        assert_eq!(epic_of("PLEXUS-12"), Some("PLEXUS"));
        // What hosts a shortcode, up the chain: a ticket's lineage.
        assert_eq!(t.hosting("GEN"), strs(&["CODEGEN-2", "CODEGEN", "MACRO-7", "MACRO", "PLEXUS-1", "PLEXUS"]));
        assert_eq!(t.hosting("PLEXUS"), Vec::<String>::new());
    }

    /// A host chain that loops still lists every shortcode, once.
    #[test]
    fn a_looping_host_chain_is_still_listed() {
        let g: BTreeMap<String, Group> = [group("A", Some("B-2")), group("B", Some("A-3")), group("C", None)].into_iter().collect();
        let t = ScTree::of(&g);
        let rows = t.rows(Some("C"), std::iter::empty());
        let codes: Vec<String> = rows.iter().filter_map(|r| if let ScRow::Code { code, .. } = r { Some(code.clone()) } else { None }).collect();
        assert_eq!(codes, strs(&["C", "A", "B"]));
        assert!(t.hosting("A").len() <= 4, "a loop ends");
    }

    /// Acceptance 1: the tree's tri-state — a parent with its whole subtree
    /// checked is checked, with none unchecked, with part of it (itself
    /// alone, or some children) indeterminate; a click checks the subtree,
    /// or — all checked — unchecks it.
    #[test]
    fn the_shortcode_tree_is_tri_state() {
        let t = platform();
        let all = set(&PLEXUS_TREE);
        assert_eq!(t.tri("PLEXUS", &all), Tri::On);
        assert_eq!(t.tri("MACRO", &all), Tri::On);
        assert_eq!(t.tri("PLEXUS", &set(&[])), Tri::Off);
        let mut partly = all.clone();
        partly.remove("GEN");
        assert_eq!(t.tri("PLEXUS", &partly), Tri::Mixed, "a grandchild unchecked");
        assert_eq!(t.tri("MACRO", &partly), Tri::Mixed);
        assert_eq!(t.tri("CODEGEN", &partly), Tri::Mixed);
        assert_eq!(t.tri("GEN", &partly), Tri::Off, "a leaf is on or off");
        assert_eq!(t.tri("RETURNS", &partly), Tri::On);
        assert_eq!(t.tri("PLEXUS", &set(&["PLEXUS"])), Tri::Mixed, "the parent alone is part of its tree");
        assert_eq!(t.tri("PLEXUS", &set(&["RETURNS"])), Tri::Mixed);

        // A click on a partly-checked parent checks its subtree; on a
        // checked one, unchecks it; a leaf toggles alone.
        assert_eq!(t.click("PLEXUS", &partly), all);
        assert_eq!(t.click("PLEXUS", &all), set(&[]));
        assert_eq!(t.click("MACRO", &all), set(&["CONTEXT", "HYPERFORGE", "PLEXUS", "RETURNS", "STARTER", "SYNAPSE", "TRANSPORT"]));
        assert_eq!(t.click("RETURNS", &set(&["MNG"])), set(&["MNG", "RETURNS"]));

        // Through the clauses: a click on a parent checks its subtree.
        let mut k = Clauses::default();
        k.toggle_code(&t, "MACRO");
        assert_eq!(k.get(Cat::Shortcode).unwrap().values, strs(&["CODEGEN", "GEN", "MACRO"]));
        k.toggle_code(&t, "MACRO");
        assert!(k.get(Cat::Shortcode).is_none());
    }

    /// The Shortcode menu lists the route's tree first, then "other boards"
    /// — every other top-level tree, without what was listed — then a value
    /// no group knows; with no route (the global menu), every tree.
    #[test]
    fn the_shortcode_menu_lists_the_route_tree_first() {
        let t = platform();
        let code = |c: &str, d: usize| ScRow::Code { code: c.into(), depth: d };
        let rows = t.rows(Some("PLEXUS"), std::iter::empty());
        assert_eq!(rows[0], code("PLEXUS", 0));
        assert_eq!(rows[4], code("CODEGEN", 2));
        assert_eq!(rows[10], ScRow::Divider);
        assert_eq!(rows[11..], [code("MNG", 0), code("LOOSE", 1), code("WAVE", 0)]);

        // A sub-shortcode's board: its tree first; PLEXUS's, without it, after.
        let extra = strs(&["NOPE"]);
        let rows = t.rows(Some("MACRO"), extra.iter());
        assert_eq!(rows[..3], [code("MACRO", 0), code("CODEGEN", 1), code("GEN", 2)]);
        assert_eq!(rows[3], ScRow::Divider);
        assert!(rows.iter().filter(|r| **r == code("MACRO", 0) || **r == code("MACRO", 1)).count() == 1, "listed once");
        assert_eq!(rows.last(), Some(&code("NOPE", 0)));

        // No route: every tree in order, no divider.
        let rows = t.rows(None, std::iter::empty());
        assert_eq!(rows.len(), 13);
        assert!(!rows.contains(&ScRow::Divider));
        assert_eq!(rows[..2], [code("MNG", 0), code("LOOSE", 1)]);
    }

    /// The clauses and WAVE-12's query, both ways — every key, both
    /// operators; the empty filter is `all`; no query, or a malformed one,
    /// is `None` (the board's default).
    #[test]
    fn the_clauses_round_trip_through_wave_12s_query() {
        let f = Clauses::default()
            .with(Cat::Shortcode, Op::Is, strs(&["RETURNS", "CONTEXT"]))
            .with(Cat::Status, Op::Not, strs(&["Superseded", "Complete"]))
            .with(Cat::Wave, Op::Is, strs(&["done", "10", "2", "missing"]))
            .with(Cat::Kind, Op::Is, strs(&["gap", "build", "spike"]))
            .with(Cat::Severity, Op::Not, strs(&["none", "high"]))
            .with(Cat::Readiness, Op::Is, strs(&["workable"]))
            .with(Cat::OffBoard, Op::Not, strs(&["some"]))
            .with(Cat::Title, Op::Is, strs(&["envelope"]));
        let q = f.to_query();
        assert_eq!(
            q,
            "sc=is:CONTEXT,RETURNS&status=not:Complete,Superseded&kind=is:build,spike,gap&sev=not:high,none\
             &wave=is:2,10,missing,done&ready=is:workable&off=not:some&q=envelope"
        );
        assert_eq!(Clauses::parse_query(&q), Some(f.clone()));
        assert_eq!(Clauses::parse_query(&format!("?{q}")), Some(f.clone()), "a leading ? is the query's");

        // Status with its space, a title that says "not:", commas and an
        // ampersand: encoded, and back.
        let g = Clauses::default().with(Cat::Status, Op::Is, strs(&["In Review"])).with(Cat::Title, Op::Not, strs(&["not: a, b & c"]));
        let q = g.to_query();
        assert_eq!(q, "status=is:In%20Review&q=not:not%3A%20a%2C%20b%20%26%20c");
        assert_eq!(Clauses::parse_query(&q), Some(g));
        let h = Clauses::default().with(Cat::Title, Op::Is, strs(&["is: it"]));
        assert_eq!(Clauses::parse_query(&h.to_query()), Some(h));

        // WAVE-12's spelling, read: any case, `+` for a space, the op
        // optional, `High` for `high`.
        let lenient = Clauses::parse_query("sc=returns&status=is:in+review,PENDING&kind=not:Scope&sev=is:High").unwrap();
        assert_eq!(lenient.get(Cat::Shortcode).unwrap().values, strs(&["RETURNS"]));
        assert_eq!(lenient.get(Cat::Status).unwrap().values, strs(&["In Review", "Pending"]));
        assert_eq!(lenient.get(Cat::Kind), Some(&Clause { op: Op::Not, values: strs(&["scope"]) }));
        assert_eq!(lenient.get(Cat::Severity).unwrap().values, strs(&["high"]));

        // Empty: the empty filter is `all`; no query is the default.
        assert_eq!(Clauses::default().to_query(), "all");
        assert_eq!(Clauses::parse_query("all"), Some(Clauses::default()));
        assert_eq!(Clauses::parse_query(""), None);
        assert_eq!(Clauses::parse_query("?"), None);
        assert_eq!(Clauses::parse_query("&&"), None);

        // Malformed: the default.
        for bad in [
            "bogus=is:x",
            "status=is:Nope",
            "status=",
            "status=is:",
            "sc=is:A,,B",
            "sc=is:A:B",
            "sc=is:RETURNS&sc=is:CONTEXT",
            "wave=is:0",
            "wave=is:two",
            "sev=is:urgent",
            "q=",
            "q=%20",
            "q=%zz",
            "q=%E2%82",
            "status",
            "all&sc=is:A",
        ] {
            assert_eq!(Clauses::parse_query(bad), None, "{bad}");
        }

        // A surface reads its local categories alone, beside others' keys.
        let ps = lens::pairs("f=ready&wave=is:1&q=x&sc=MNG");
        let local = Clauses::parse_pairs(&ps, &[Cat::Wave, Cat::Readiness, Cat::OffBoard, Cat::Title]).unwrap();
        assert_eq!(local, Clauses::default().with(Cat::Wave, Op::Is, strs(&["1"])).with(Cat::Title, Op::Is, strs(&["x"])));
        assert_eq!(Clauses::parse_pairs(&lens::pairs("f=ready"), &[Cat::Wave]), None, "none named");
    }

    /// A flag is one value under `is`: `is not yes` is `is no`; its operator
    /// turns the value over; a click on its value removes it, on the other
    /// replaces it.
    #[test]
    fn a_flag_is_one_value() {
        assert_eq!(Clause::new(Cat::Ready, Op::Is, strs(&["yes"])), Some(Clause { op: Op::Is, values: strs(&["yes"]) }));
        assert_eq!(Clause::new(Cat::Ready, Op::Not, strs(&["yes"])), Some(Clause { op: Op::Is, values: strs(&["no"]) }));
        assert_eq!(Clause::new(Cat::Ready, Op::Is, strs(&[])), None);
        let mut k = Clauses::default();
        k.toggle(Cat::Blocked, "yes");
        assert_eq!(k.get(Cat::Blocked).unwrap().values, strs(&["yes"]));
        k.toggle(Cat::Blocked, "no");
        assert_eq!(k.get(Cat::Blocked).unwrap().values, strs(&["no"]), "the other replaces it");
        k.flip(Cat::Blocked);
        assert_eq!(k.get(Cat::Blocked), Some(&Clause { op: Op::Is, values: strs(&["yes"]) }), "the operator turns it over");
        k.toggle(Cat::Blocked, "yes");
        assert!(k.get(Cat::Blocked).is_none(), "itself removes it");
        assert_eq!(op_label(Cat::Commits, Op::Is, 1), "is");
        assert_eq!(Cat::Commits.value_label("yes"), "has commits");
    }

    /// Status's groups (the panel's headings) hold every status, once.
    #[test]
    fn status_groups_hold_every_status() {
        assert_eq!(STATUS_GROUPS.iter().map(|g| g.1.len()).sum::<usize>(), Status::ALL.len(), "every status, once");
    }

    /// The menu: a level's categories in its order; Status in its groups,
    /// each header a row; Wave up to the counts' last; typing searches every
    /// category's values — "ret" offers RETURNS — and ends with Title
    /// contains where the level offers Title; inside a category, typing
    /// narrows its values.
    #[test]
    fn the_menu_lists_categories_values_and_search() {
        // FILTER-42: what the panel's search leaves — the categories whose
        // section still shows (a time section by its name and keywords,
        // a value section by a value or its name, Title by its name).
        fn found(q: &str, k: &MenuFacts) -> Vec<Cat> {
            k.cats
                .iter()
                .copied()
                .filter(|c| match c {
                    Cat::When | Cat::Deployed | Cat::Chats => q.trim().is_empty() || palette_match(c.menu_label(), c.keywords(), q),
                    Cat::Title => q.trim().is_empty() || palette_match("title", "contains text", q),
                    c if c.is_flag() && !c.is_local() => flag_rows(&[*c], q).contains(c) || palette_match(c.label(), "", q),
                    c => section_rows(*c, k, q).iter().any(|r| matches!(r, PanelRow::Value { .. })),
                })
                .collect()
        }
        let t = platform();
        let clauses = Clauses::default().with(Cat::Shortcode, Op::Is, t.subtree("PLEXUS"));
        let mut counts: Counts = HashMap::new();
        counts.insert(Cat::Wave, [("1", 3), ("2", 1), ("4", 1), ("9", 0)].into_iter().map(|(k, v)| (k.to_string(), v)).collect());
        let k = MenuFacts { cats: &Cat::WAVES, route: Some("PLEXUS"), tree: Some(&t), clauses: &clauses, counts: &counts };
        assert_eq!(found("", &k), Cat::WAVES.to_vec(), "FILTER-42: the panel draws every category");
        assert_eq!(
            Cat::WAVES.map(Cat::menu_label),
            [
                "Shortcode",
                "Status",
                "Wave",
                "Kind",
                "Severity",
                "Ready",
                "Blocked",
                "Commits",
                "Menger docs",
                "Label",
                "Readiness",
                "Off-board blockers",
                "Title contains…"
            ]
        );
        let val = |c: Cat, t: &str| Row::Value { cat: c, token: t.into(), depth: 0, scoped: false };
        let status = offered(Cat::Status, &k);
        assert_eq!(status[0], Row::Group { cat: Cat::Status, label: "in progress", tokens: &["Coding", "In Review"] });
        assert_eq!(status[1..3], [val(Cat::Status, "Coding"), val(Cat::Status, "In Review")]);
        assert!(matches!(status[3], Row::Group { label: "ready", .. }));
        assert_eq!(status.last(), Some(&val(Cat::Status, "Doc")));
        let wave: Vec<String> = offered(Cat::Wave, &k).into_iter().filter_map(|r| if let Row::Value { token, .. } = r { Some(token) } else { None }).collect();
        assert_eq!(wave, strs(&["1", "2", "3", "4", "missing", "done"]), "a wave counted 0 is not the answer's");
        assert_eq!(offered(Cat::Kind, &k).len(), 6);
        assert_eq!(offered(Cat::Ready, &k), [val(Cat::Ready, "yes"), val(Cat::Ready, "no")]);

        let pv = |c: Cat, t: &str| PanelRow::Value { cat: c, token: t.into(), depth: 0 };
        assert!(section_rows(Cat::Shortcode, &k, "ret").contains(&pv(Cat::Shortcode, "RETURNS")));
        assert!(found("sta", &k).contains(&Cat::Status), "a category by its name");
        assert!(section_rows(Cat::Status, &k, "review").contains(&pv(Cat::Status, "In Review")));
        assert_eq!(section_rows(Cat::Shortcode, &k, "gen"), [pv(Cat::Shortcode, "CODEGEN"), pv(Cat::Shortcode, "GEN")]);
        assert_eq!(section_rows(Cat::Readiness, &k, "now"), [pv(Cat::Readiness, "workable")]);
        assert_eq!(flag_rows(&Cat::WAVES, "has"), [Cat::Commits]);
        assert!(offered(Cat::Title, &k).is_empty());

        // The global menu: the server's categories, no Title to offer.
        let g = MenuFacts { cats: &Cat::SERVER, route: None, tree: Some(&t), clauses: &Clauses::default(), counts: &HashMap::new() };
        assert_eq!(found("", &g).len(), 13, "FILTER-27: When among them; DEPLOY-13: Deployed; CHAT-4: Chats");
        // CHAT-4: Chats — its own pane of two cells, found by them, and the
        // pills it builds drawn under it.
        assert!(offered(Cat::Chats, &g).is_empty());
        assert!(found("unread", &g).contains(&Cat::Chats));
        assert!(found("chat", &g).contains(&Cat::Chats));
        assert_eq!(switch_words(Cat::Chats), None);
        assert_eq!(time_cat(&crate::when::new_chats(false)), Cat::Chats);
        assert_eq!(crate::when::group(&crate::when::new_chats(true)).format(), "unread");
        assert_eq!(crate::when::group(&crate::when::new_chats(false)).format(), "chatting");
        assert!(offered(Cat::When, &g).is_empty(), "When draws its own pane");
        assert!(offered(Cat::Deployed, &g).is_empty(), "DEPLOY-13: and Deployed, When's pane");
        assert!(found("deploy", &g).contains(&Cat::Deployed));
        assert!(found("prod", &g).contains(&Cat::Deployed), "found by what it builds");
        assert_eq!(switch_words(Cat::Deployed), None);
        let dep = crate::when::new_deployed("prod", false, None).unwrap();
        assert_eq!(time_cat(&dep), Cat::Deployed);
        assert_eq!(time_cat(&crate::when::new_clause(crate::when::Kind::Changed, None, None).unwrap()), Cat::When);
        assert!(found("created", &g).contains(&Cat::When), "FILTER-27: found by what it builds");
        assert!(found("moved", &g).contains(&Cat::When));
        // FILTER-21: typing finds Menger docs, by its name.
        assert!(found("menger", &g).contains(&Cat::MengerDocs));
        assert_eq!(offered(Cat::MengerDocs, &g), [val(Cat::MengerDocs, "yes"), val(Cat::MengerDocs, "no")]);
        assert!(!found("ret", &g).contains(&Cat::Title));
        assert!(!found("wave", &g).contains(&Cat::Wave));
        // MENGER-12: Asking is a flag; Label offers `needs-you`, every label
        // the counts name and the clause's own, in name order.
        assert!(found("asking", &g).contains(&Cat::Asking));
        assert_eq!(offered(Cat::Asking, &g), [val(Cat::Asking, "yes"), val(Cat::Asking, "no")]);
        assert_eq!(offered(Cat::Label, &g), [val(Cat::Label, "needs-you")]);
        let mut counts = HashMap::new();
        counts.insert(Cat::Label, [("blocked-on-ci", 2), ("needs-you", 3)].into_iter().map(|(k, v)| (k.to_string(), v)).collect());
        let clauses = Clauses::default().with(Cat::Label, Op::Is, strs(&["zz-mine"]));
        let l = MenuFacts { cats: &Cat::SERVER, route: None, tree: Some(&t), clauses: &clauses, counts: &counts };
        assert_eq!(
            offered(Cat::Label, &l),
            [val(Cat::Label, "blocked-on-ci"), val(Cat::Label, "needs-you"), val(Cat::Label, "zz-mine")]
        );
        assert_eq!(Cat::Label.token("Needs-You").as_deref(), Some("needs-you"));
        assert_eq!(Cat::Label.token("two words"), None, "not a label");
    }

    /// A pill: its operator in words, its values first-then-`+n` — the
    /// route's own shortcode first.
    #[test]
    fn a_pill_reads_like_linear() {
        let t = platform();
        let d = Clauses::default().with(Cat::Shortcode, Op::Is, t.subtree("PLEXUS")).with(Cat::Kind, Op::Is, strs(&DEFAULT_KINDS));
        let sc = d.get(Cat::Shortcode).unwrap();
        assert_eq!(sc.values.len(), 10);
        assert_eq!(op_label(Cat::Shortcode, Op::Is, 10), "is", "FILTER-42: no any of anywhere");
        assert_eq!(op_label(Cat::Status, Op::Is, 1), "is");
        assert_eq!(op_label(Cat::Status, Op::Not, 2), "is not");
        assert_eq!(op_label(Cat::Title, Op::Is, 1), "contains");
        assert_eq!(op_label(Cat::Title, Op::Not, 1), "does not contain");
        assert_eq!(Cat::Wave.value_label("3"), "wave 3");
        assert_eq!(Cat::Readiness.value_label("workable"), "workable now");

        // Toggling, flipping, removing; a clause left empty is gone.
        let mut f = d.clone();
        f.toggle(Cat::Status, "Pending");
        assert!(f.has(Cat::Status, "Pending"));
        f.flip(Cat::Status);
        assert_eq!(f.get(Cat::Status).unwrap().op, Op::Not);
        f.toggle(Cat::Status, "Complete");
        assert_eq!(f.get(Cat::Status).unwrap().values, strs(&["Pending", "Complete"]), "the menu's order");
        f.toggle(Cat::Status, "Pending");
        f.toggle(Cat::Status, "Complete");
        assert!(f.get(Cat::Status).is_none());
        f.remove(Cat::Kind);
        assert_ne!(f, d);
        assert_eq!(d.local(), Clauses::default());
        assert_eq!(d.server(), d);
    }

    /// FILTER-19: a category's values open with an operator switch — `is` /
    /// `is not` on every category with values, the surface's own included;
    /// Title's `contains` / `does not contain`; none on a flag. It shows the
    /// clause's operator, or — no clause — the pane's (`is` for a new one).
    #[test]
    fn a_categorys_values_open_with_an_operator_switch() {
        for c in [Cat::Shortcode, Cat::Status, Cat::Kind, Cat::Severity, Cat::Wave, Cat::Readiness, Cat::OffBoard, Cat::State] {
            assert_eq!(switch_words(c), Some(("is", "is not")), "{c:?}");
        }
        assert_eq!(switch_words(Cat::Title), Some(("contains", "does not contain")));
        for c in [Cat::Ready, Cat::Blocked, Cat::Commits, Cat::MengerDocs, Cat::Asking, Cat::Removed] {
            assert_eq!(switch_words(c), None, "{c:?}: its yes / no is the negation");
        }
        assert_eq!(switch_words(Cat::Label), Some(("is", "is not")));
        assert_eq!(Cat::ALL.into_iter().filter(|c| switch_words(*c).is_some()).count(), 10, "every category but the flags and When");
        assert_eq!(switch_words(Cat::When), None, "FILTER-27: a time pill's parts are its own");

    }

    /// FILTER-19: the switch set to `is not` is a pill's toggle — the same
    /// clause; values checked under it join as excluded values; one checked
    /// from the search's results keeps its clause's operator (`is` for a new
    /// one); a flag has no switch to set.
    #[test]
    fn the_switch_builds_a_not() {
        // A clause there: the switch turns it as the pill's toggle does.
        let k = Clauses::default().with(Cat::Status, Op::Is, strs(&["Ready", "Coding"]));
        let mut by_pill = k.clone();
        by_pill.flip(Cat::Status);
        let mut by_switch = k.clone();
        by_switch.set_op(Cat::Status, Op::Not);
        assert_eq!(by_switch, by_pill);
        let cl = by_switch.get(Cat::Status).unwrap().clone();
        assert_eq!(cl, Clause { op: Op::Not, values: strs(&["Coding", "Ready"]) });
        by_switch.set_op(Cat::Status, Op::Not);
        assert_eq!(by_switch, by_pill, "set again: as it was");
        by_switch.set_op(Cat::Status, Op::Is);
        assert_eq!(by_switch, k, "and back");

        // No clause yet: the switch alone writes nothing (a clause is never
        // empty); the values checked under it are excluded.
        let mut f = Clauses::default();
        f.set_op(Cat::Kind, Op::Not);
        assert_eq!(f, Clauses::default());
        f.edit_as(Cat::Kind, Op::Not, |k| k.toggle(Cat::Kind, "doc"));
        f.edit_as(Cat::Kind, Op::Not, |k| k.toggle(Cat::Kind, "spike"));
        let kind = f.get(Cat::Kind).unwrap().clone();
        assert_eq!(kind, Clause { op: Op::Not, values: strs(&["spike", "doc"]) });
        assert!(!kind.admits(Some("doc")) && !kind.admits(Some("spike")) && kind.admits(Some("build")));

        // From the search's results (checked under `is`): a clause keeps its
        // operator; a new one is `is`.
        f.edit_as(Cat::Kind, Op::Is, |k| k.toggle(Cat::Kind, "gap"));
        assert_eq!(f.get(Cat::Kind), Some(&Clause { op: Op::Not, values: strs(&["spike", "gap", "doc"]) }));
        f.edit_as(Cat::Severity, Op::Is, |k| k.toggle(Cat::Severity, "high"));
        assert_eq!(f.get(Cat::Severity), Some(&Clause { op: Op::Is, values: strs(&["high"]) }));

        // Unchecked under the switch, the last value takes the clause.
        for v in ["spike", "gap", "doc"] {
            f.edit_as(Cat::Kind, Op::Not, |k| k.toggle(Cat::Kind, v));
        }
        assert!(f.get(Cat::Kind).is_none());

        // A shortcode's subtree, a surface's own category: under the switch.
        let t = platform();
        f.edit_as(Cat::Shortcode, Op::Not, |k| k.toggle_code(&t, "MACRO"));
        assert_eq!(f.get(Cat::Shortcode), Some(&Clause { op: Op::Not, values: strs(&["CODEGEN", "GEN", "MACRO"]) }));
        f.edit_as(Cat::Wave, Op::Not, |k| k.toggle(Cat::Wave, "missing"));
        assert_eq!(f.to_pairs().iter().find(|p| p.key == "wave"), Some(&lens::Pair::new("wave", Some("not:missing".into()))));

        // Title: `does not contain`, as its pill says it.
        let mut q = Clauses::default().with(Cat::Title, Op::Is, strs(&["envelope"]));
        q.set_op(Cat::Title, Op::Not);
        let title = q.get(Cat::Title).unwrap();
        assert_eq!(op_label(Cat::Title, title.op, 1), switch_words(Cat::Title).unwrap().1);
        assert_eq!(q.to_pairs(), [lens::Pair::new("q", Some("not:envelope".into()))]);

        // A flag: no switch — nothing to set; its value is the negation.
        let mut g = Clauses::default().with(Cat::Ready, Op::Is, strs(&["yes"]));
        g.set_op(Cat::Ready, Op::Not);
        assert_eq!(g.get(Cat::Ready), Some(&Clause { op: Op::Is, values: strs(&["yes"]) }));
        g.edit_as(Cat::Blocked, Op::Not, |k| k.toggle(Cat::Blocked, "yes"));
        assert_eq!(g.get(Cat::Blocked), Some(&Clause { op: Op::Is, values: strs(&["yes"]) }), "not turned over");
    }

    /// FILTER-39: the panel's sections — the saved filters first where the
    /// server reads them, each category with values its own, the flags
    /// gathered where the first stands, the time categories their panes,
    /// Title its field; every category and value the menu offered is in one.
    #[test]
    fn the_panel_has_a_section_for_every_category() {
        let board = sections(Level::Board, &Cat::SERVER);
        assert_eq!(
            board,
            [
                Section::Saved,
                Section::Values(Cat::Shortcode),
                Section::Values(Cat::Status),
                Section::Values(Cat::Kind),
                Section::Values(Cat::Severity),
                Section::Flags,
                Section::Values(Cat::Label),
                Section::Time(Cat::When),
                Section::Time(Cat::Deployed),
                Section::Time(Cat::Chats),
            ]
        );
        for c in Cat::SERVER {
            assert!(board.iter().any(|s| s.holds(c)), "{c:?} has a section");
        }
        let waves = sections(Level::Waves, &Cat::WAVES);
        assert!(!waves.contains(&Section::Saved), "a wave board reads its filter on its cards — no saved names");
        assert!(waves.contains(&Section::Values(Cat::Wave)) && waves.contains(&Section::Title));
        for c in Cat::WAVES {
            assert!(waves.iter().any(|s| s.holds(c)), "{c:?} has a section");
        }
        let inputs = sections(Level::Inputs, &Cat::INPUTS);
        assert_eq!(inputs[..3], [Section::Values(Cat::State), Section::Values(Cat::Removed), Section::Values(Cat::Shortcode)]);
        assert_eq!(flag_words(Cat::Commits), "has commits");
    }

    /// FILTER-39: a section's rows are the menu's values, Status's groups
    /// as headings, the tree indented; typing narrows every section at once
    /// (a section found by its name keeps all its values), headings gone.
    #[test]
    fn a_sections_rows_narrow_as_you_type() {
        let t = platform();
        let k = MenuFacts { cats: &Cat::SERVER, route: None, tree: Some(&t), clauses: &Clauses::default(), counts: &HashMap::new() };
        let status = section_rows(Cat::Status, &k, "");
        assert_eq!(status[0], PanelRow::Heading("in progress"));
        assert_eq!(status.iter().filter(|r| matches!(r, PanelRow::Value { .. })).count(), 11, "every status");
        assert!(section_rows(Cat::Shortcode, &k, "").contains(&PanelRow::Value { cat: Cat::Shortcode, token: "GEN".into(), depth: 3 }));
        assert_eq!(section_rows(Cat::Status, &k, "review"), [PanelRow::Value { cat: Cat::Status, token: "In Review".into(), depth: 0 }]);
        assert!(section_rows(Cat::Kind, &k, "review").is_empty());
        assert_eq!(section_rows(Cat::Kind, &k, "kind").len(), KINDS.len(), "found by its name: all of it");
        assert_eq!(section_rows(Cat::Shortcode, &k, "gen"), [
            PanelRow::Value { cat: Cat::Shortcode, token: "CODEGEN".into(), depth: 0 },
            PanelRow::Value { cat: Cat::Shortcode, token: "GEN".into(), depth: 0 },
        ]);
        assert_eq!(flag_rows(&Cat::SERVER, ""), [Cat::Ready, Cat::Blocked, Cat::Commits, Cat::MengerDocs, Cat::Asking]);
        assert_eq!(flag_rows(&Cat::SERVER, "commits"), [Cat::Commits]);
        assert_eq!(flag_rows(&Cat::SERVER, "flags").len(), 5);
        assert_eq!(flag_rows(&Cat::INPUTS, ""), [Cat::Ready, Cat::Blocked, Cat::Commits, Cat::MengerDocs, Cat::Asking], "Removed is the page's own");
    }

    /// FILTER-40: one panel — under the route's surface line on a wide
    /// window, under the header where there is none, and in the phone
    /// bar's filter sheet at phone widths; each level its tab.
    #[test]
    fn the_one_panel_is_drawn_in_one_place() {
        assert_eq!(host_of(false, true), Host::Surface);
        assert_eq!(host_of(false, false), Host::Top);
        assert_eq!(host_of(true, true), Host::Top, "the phone's sheet");
        assert_eq!(host_of(true, false), Host::Top);
        assert_eq!(Level::ALL.map(tab_words), ["everywhere", "nav", "this board", "this wave board", "the inputs"]);
    }

    /// FILTER-43: the drawer marks each mistake where it is (a caret where
    /// something is missing) and says each fix in two words.
    #[test]
    fn the_drawer_marks_each_mistake_and_says_its_fix() {
        let text = "(kind build and status Ready) or (kind bild and severity hgh";
        let e = menger_tickets_api::filter::Filter::parse(text).unwrap_err();
        let ms: Vec<_> = e.chunks.iter().flat_map(|c| c.mistakes.iter().cloned()).collect();
        let runs = marked(text, &ms);
        assert_eq!(runs.iter().filter(|(bad, _)| *bad).map(|(_, t)| t.as_str()).collect::<Vec<_>>(), ["bild", "hgh", "\u{2038}"]);
        assert_eq!(runs.iter().map(|(_, t)| t.as_str()).filter(|t| *t != "\u{2038}").collect::<String>(), text, "every run as written");
        assert_eq!(ms.iter().map(|m| fix_words(m.fix.as_deref().unwrap())).collect::<Vec<_>>(), ["use build", "use high", "close it"]);
        assert_eq!(fix_words(""), "remove it");
        // Nothing marked past the text's end.
        assert_eq!(marked("ready", &[]), [(false, "ready".to_string())]);
    }
}
