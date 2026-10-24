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
//! - **The bar** ([`FilterBar`]; FILTER-41, FILTER-55): one line — the
//!   `Filter` button (its badge the rules), ⌂'s chip, and the filter's
//!   name when it has one (a saved filter, or the level's default); its
//!   rules are spelled out only in the builder (src/fcard.rs), which the
//!   button, the line and `f` open. Under it naming's prompts and, while
//!   there is one, "Nothing can match" with the clashing groups.
//! - **The menu is gone** (FILTER-55, the owner, comment 42: "we don't
//!   need to show the full break down other than inside the actual filter
//!   builder"): what stays here is what the builder reads — the
//!   categories, their values ([`offered`]) and counts.

//! Everything that decides — the categories and their values, a clause's
//! edits, the shortcode tree, a pill's words — is a pure function here, pinned by native tests; the pills ⇄
//! groups of the language are src/lens.rs's, the groups joined by `or`
//! src/ors.rs's.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use leptos::prelude::*;
use menger_tickets_api::{Group, Status};
use crate::lens::{self, Lens, Level};
use crate::waves::epic_of;

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
    /// for a length, changed (MENGER-13's Changed recently closed in): each
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
    /// TICKETS-9: who holds it — `assigned me`, `assigned`, `unassigned`,
    /// `assigned "<principal>"`: pills of When's kind (src/when.rs), built
    /// from this category's own pane; uncounted.
    People,
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
    ("not started", &["Pending", "Blocked", "Draft", "Epic"]),
    ("done", &["Complete", "Superseded", "Canceled"]),
    ("docs", &["Doc"]),
];

const STATUSES: [&str; 11] = ["Coding", "In Review", "Ready", "Pending", "Blocked", "Draft", "Epic", "Complete", "Superseded", "Canceled", "Doc"];

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
    pub(crate) const ALL: [Cat; 20] = [
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
        Cat::People,
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
        Cat::Deployed,
        Cat::Chats,
        Cat::People,
    ];

    #[cfg(test)]
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
        Cat::Deployed,
        Cat::Chats,
        Cat::People,
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
            Cat::People => "people",
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
            Cat::People => "People",
            Cat::Wave => "Wave",
            Cat::Readiness => "Readiness",
            Cat::OffBoard => "Off-board blockers",
            Cat::Title => "Title",
            Cat::State => "State",
            Cat::Removed => "Removed",
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
            Cat::Shortcode | Cat::Wave | Cat::Title | Cat::Label | Cat::When | Cat::Deployed | Cat::Chats | Cat::People => &[],
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
    } else if crate::when::is_people(t) {
        Cat::People
    } else if crate::when::env_of(t).is_some() {
        Cat::Deployed
    } else {
        Cat::When
    }
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

/// A value a category offers, as [`offered`] lists them — the menu's
/// sections draw them ([`section_rows`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    /// A value to tick. `depth` indents a shortcode under its host.
    Value { cat: Cat, token: String, depth: usize, scoped: bool },
    /// A group of a category's values, by its header (Status's groups):
    /// the menu's heading.
    Group { cat: Cat, label: &'static str, tokens: &'static [&'static str] },
    /// A heading over values — Shortcode's "other boards".
    Heading(&'static str),
}


/// What the menu needs to know to list values.
pub(crate) struct MenuFacts<'a> {
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
// A level's bar

/// What a level's bar offers — its categories and its counts — while a view
/// draws it, every handle `Copy`. The lens holds one per level for the app's
/// lifetime; the view drawing the bar says what it offers
/// ([`Pop::configure`]). FILTER-55: the builder (src/fcard.rs) reads a
/// surface's own categories' values and counts from here.
#[derive(Clone, Copy)]
pub(crate) struct Pop {
    counts: RwSignal<CountsProvider>,
    mounted: RwSignal<u32>,
}

impl Pop {
    pub(crate) fn new(_: Level) -> Self {
        Pop { counts: RwSignal::new(CountsProvider::none()), mounted: RwSignal::new(0) }
    }

    /// The view drawing this level's bar says what it offers, for as long
    /// as it is mounted.
    pub(crate) fn configure(self, counts: CountsProvider) {
        self.counts.set(counts);
        self.mounted.update(|n| *n += 1);
        on_cleanup(move || {
            self.mounted.try_update(|n| *n = n.saturating_sub(1));
            if self.mounted.try_get_untracked() == Some(0) {
                self.counts.try_set(CountsProvider::none());
            }
        });
    }

    pub(crate) fn is_mounted(self) -> bool {
        self.mounted.get_untracked() > 0
    }

    /// The level's counts provider. Tracked.
    pub(crate) fn counts(self) -> CountsProvider {
        self.counts.get()
    }
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
/// are gone: text is the menu's foot, choosing is its Saved section or a
/// chip here. Under the line: naming's prompts, "as evaluated", the
/// menu (FILTER-49, over the page), and the contradiction notice.
/// `cats`: the level's categories, server-backed and local, in its menu's
/// order; `counts`: what the menu's values count.
/// FILTER-55: the board's line (a level the builder edits, src/fcard.rs's
/// `builds`) draws no chips — the global chip, the filter's name when it has
/// one ([`line_summary`]), the count; its Filter button opens the builder.
#[component]
pub(crate) fn FilterBar(
    level: Level,
    counts: CountsProvider,
    /// What the line's Filter button says when hovered.
    #[prop(optional, into)]
    hint: Option<String>,
) -> impl IntoView {
    let lens = lens::use_lens();
    lens.pop(level).configure(counts);
    let hint = hint.unwrap_or_else(|| "Filter (f)".into());
    let shown = tab_count(lens, level);
    let all = total_count();
    view! {
        <div class="fbar fline" data-fbar=level.name() data-level=level.name()>
            <div class="fline-row">
                <FilterButton level hint badge=true />
                <span class="fline-chips">{line_summary(lens, level, Some(shown))}</span>
                <span class="fline-end">
                    <span class="fline-n" title="tickets this surface shows, of every ticket" aria-live="polite">
                        <b>{move || shown.get().map_or_else(|| "\u{2026}".to_string(), |n| n.to_string())}</b>
                        <span class="fline-of">{move || all.get().map(|n| format!(" of {n}")).unwrap_or_default()}</span>
                    </span>
                    {flash_view(level)}
                </span>
            </div>
            <LineNotes level />
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

/// FILTER-55: a line the builder edits says only what its filter is (the
/// owner, 2026-10-24: *"just show the filter name if available, and the
/// filter button should open to it"*) — the global filter's dashed chip, then
/// the filter's name when it has one (a press opens the builder on it), and
/// empty, what is shown and the saved filters most used. The rules are the
/// builder's; the Filter button's badge counts them.
fn line_summary(lens: Lens, level: Level, shown: Option<RwSignal<Option<usize>>>) -> impl IntoView {
    let naming = crate::naming::use_naming();
    move || {
        let global = lens.filter(Level::Global).with(|f| f.format());
        let f = lens.filter(level).get();
        let bound = naming.bound(level).map(|b| b.name);
        // A surface's own clauses (the wave board's Wave, the inputs' State)
        // are rules too: with any, the line names nothing.
        let local = lens.kept(level).is_some_and(|k| k.local.iter().next().is_some());
        let name = (!local).then(|| crate::fcard::line_name(&f, bound.as_deref())).flatten();
        let single = bound.is_none() && name.is_some();
        let empty = f.groups.is_empty() && !local;
        let more = empty || name.is_some();
        view! {
            {(!global.is_empty()).then(|| view! {
                {global_chip(lens, global.clone())}
                {more.then(|| view! { <span class="fw">"and"</span> })}
            })}
            {match (empty, name) {
                (true, _) => match shown {
                    Some(shown) => empty_line(level, shown).into_any(),
                    None => view! { <span class="fchips-empty">"every ticket"</span> }.into_any(),
                },
                (false, Some(n)) => {
                    let n2 = n.clone();
                    let open = move || crate::fcard::hook().is_some_and(|c| c.is_on(&crate::fcard::Subject::Line(level)));
                    view! {
                        <button type="button" class="fchip saved fsum" data-fline-name=level.name() data-fcard=format!("line:{}", level.name()) title=format!("{} \u{2014} open the filter builder", crate::fcard::line_words(level)) aria-expanded=move || open().to_string() on:click=move |ev| {
                            if let Some(c) = crate::fcard::hook() {
                                ev.stop_propagation();
                                c.press_line(level, ev.as_ref(), single.then(|| n2.clone()));
                            }
                        }>
                            <span class="fchip-f">"\u{2317}"</span>
                            <span class="fchip-v">{n.clone()}</span>
                            {move || crate::fcard::is_edited(&n).then(|| view! { <i class="fchip-ed">"edited"</i> })}
                        </button>
                    }
                    .into_any()
                }
                (false, None) => ().into_any(),
            }}
        }
    }
}

/// FILTER-53: a saved chip's face — `⌗ Name · 4 rules ⌄`, *edited* while
/// the card holds a change of it; a hover opens its card, a press pins it.
fn saved_face(name: &str, level: Level, field: String, value: String) -> impl IntoView {
    let (enter, leave, press) = crate::fcard::chip_on(name, level);
    let (n1, n2, n3) = (name.to_string(), name.to_string(), name.to_string());
    let open = move || crate::fcard::hook().is_some_and(|c| c.is_on(&crate::fcard::Subject::Saved(n3.clone())));
    view! {
        <button
            type="button"
            class="fchip-b"
            data-fcard=name.to_string()
            title=format!("what {name} is made of")
            aria-expanded=move || open().to_string()
            on:pointerenter=enter
            on:pointerleave=leave
            on:click=press
        >
            <span class="fchip-f">{field}</span>
            <span class="fchip-v">{value}</span>
            {move || crate::fcard::is_edited(&n1).then(|| view! { <i class="fchip-ed">"edited"</i> })}
            <span class="fchip-info">{move || crate::fcard::rules_words(&n2).map(|w| format!("{w} \u{2304}")).unwrap_or_else(|| "\u{2304}".into())}</span>
        </button>
    }
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

/// FILTER-41: an empty line — what is shown (*all 39 tickets*), then the
/// saved filters the reader uses most (the recently used first), each
/// counted; a chip applies it (bound by its name), as the menu's Saved
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
                // FILTER-53: an offer opens its card, read only — its switch applies it.
                let _ = (title, saved);
                let (enter, leave, press) = crate::fcard::chip_on(&e.name, level);
                view! {
                    <button type="button" class="fchip fchip-sug" data-fcard=e.name.clone() title=format!("what {} is made of \u{2014} its card applies it", e.name) on:pointerenter=enter on:pointerleave=leave on:click=press>
                        <span class="fchip-f">"\u{2317}"</span>
                        <span class="fchip-v">{label}</span>
                        <span class="fchip-n">{count}</span>
                        <span class="fchip-info">"\u{2304}"</span>
                    </button>
                }
            })
            .collect_view()
            .into_any()
    };
    view! {
        <span class="fchips-empty">{words}</span>
        <button type="button" class="fline-first" data-first=level.name() data-fcard=format!("line:{}", level.name()) title="add a rule" on:click=move |ev| {
            if let Some(c) = crate::fcard::hook() {
                ev.stop_propagation();
                c.open_line(level, true);
            }
        }>"+ add a filter"</button>
        <span class="fw fline-saved">"saved:"</span>
        {chips}
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
                                let chip = match &p.field {
                                    // FILTER-53: the nav's head opens the card too.
                                    menger_tickets_api::filter::Field::Saved(n) => {
                                        let (f, v) = chip_words(p);
                                        view! { <span class="fchip saved">{saved_face(n, level, f, v)}</span> }.into_any()
                                    }
                                    _ => view! { <span class="fchip">{crate::ors::words(p)}</span> }.into_any(),
                                };
                                view! {
                                    {(ii > 0).then(|| view! { <span class="fw">"and"</span> })}
                                    {chip}
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
/// `Filter` button opens the one menu on the nav's tab. The nav's bar is
/// gone: its name, text and `Saved ▾` are the menu's.
#[component]
pub(crate) fn NavChips(counts: CountsProvider) -> impl IntoView {
    let lens = lens::use_lens();
    lens.pop(Level::Nav).configure(counts);
    view! {
        <div class="fbar nchips" data-fbar="nav" data-level="nav">
            <FilterButton level=Level::Nav hint="Filter the nav (f — on a ticket page, f filters it to that ticket and its children)" badge=true />
            {line_summary(lens, Level::Nav, None)}
            {flash_view(Level::Nav)}
        </div>
        <Clash level=Level::Nav />
    }
}

/// A level's `Filter` button: it opens the level's menu, or closes it.
#[component]
pub(crate) fn FilterButton(level: Level, #[prop(into)] hint: String, #[prop(optional)] badge: bool) -> impl IntoView {
    let lens = lens::use_lens();
    // FILTER-55: it counts the filter's rules, as the builder draws them.
    let count = move || crate::fcard::line_rules(lens, level);
    let line = crate::fcard::Subject::Line(level);
    view! {
        <button
            class="fbtn"
            data-fbtn=level.name()
            data-fcard=format!("line:{}", level.name())
            title=hint
            aria-haspopup="dialog"
            aria-expanded=move || crate::fcard::hook().is_some_and(|c| c.is_on(&line)).to_string()
            on:click=move |ev| {
                if let Some(c) = crate::fcard::hook() {
                    ev.stop_propagation();
                    c.press_line(level, ev.as_ref(), None);
                }
            }
        >
            <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M2 3.5h12l-4.6 5.4v4.1l-2.8-1.3V8.9z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" />
            </svg>
            "Filter"
            {move || (badge && count() > 0).then(|| view! { <span class="fbadge" title="rules in this filter">{count}</span> })}
        </button>
    }
}

/// Under the line — naming's prompt (a name already saved: replace it,
/// or another — the builder's name field) and its note. FILTER-55: the line
/// spells out nothing ("as evaluated" is gone).
#[component]
pub(crate) fn LineNotes(level: Level) -> impl IntoView {
    let naming = crate::naming::use_naming();
    view! {
        {move || naming.taken(level).map(|(typed, e)| view! {
            <div class="qtaken" role="alert" data-level=level.name()>
                <span>{crate::saved::taken_words(&typed, &e)}</span>
                <button class="primary" data-act="replace" disabled=move || naming.busy(level) on:click=move |_| naming.replace(level)>"Replace it"</button>
                <button data-act="another" on:click=move |_| {
                    naming.another(level);
                    if let Some(c) = crate::fcard::hook() {
                        c.name_line(level);
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

/// FILTER-40: the global filter as a dashed chip — on every surface's line
/// and in the nav's head. FILTER-55: it says the filter's name (else how
/// many rules), never its rules; a press opens the builder on it.
pub(crate) fn global_chip(lens: Lens, text: String) -> impl IntoView {
    let naming = crate::naming::hook();
    let face = move || {
        let bound = naming.and_then(|n| n.bound(Level::Global)).map(|b| b.name);
        lens.filter(Level::Global).with(|f| {
            crate::fcard::line_name(f, bound.as_deref()).map(|n| format!("\u{2317} {n}")).unwrap_or_else(|| {
                let n = crate::fcard::rules_of(f);
                format!("{n} rule{}", if n == 1 { "" } else { "s" })
            })
        })
    };
    let (enter, leave, press) = crate::fcard::chip_on(crate::fcard::HOME, Level::Global);
    view! {
        <button
            type="button"
            class="fchip-g"
            data-fcard=crate::fcard::key_of(&crate::fcard::Subject::Line(Level::Global))
            title=format!("the filter everywhere \u{2014} every view shows what it admits: {text}")
            on:pointerenter=enter
            on:pointerleave=leave
            on:click=press
        >
            <span class="fchip-g-h" aria-hidden="true">"\u{2302}"</span>
            <span class="fchip-g-w">{face}</span>
        </button>
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

    /// Status's groups (the menu's headings) hold every status, once.
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
        let t = platform();
        let clauses = Clauses::default().with(Cat::Shortcode, Op::Is, t.subtree("PLEXUS"));
        let mut counts: Counts = HashMap::new();
        counts.insert(Cat::Wave, [("1", 3), ("2", 1), ("4", 1), ("9", 0)].into_iter().map(|(k, v)| (k.to_string(), v)).collect());
        let k = MenuFacts { route: Some("PLEXUS"), tree: Some(&t), clauses: &clauses, counts: &counts };
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

        assert!(offered(Cat::Title, &k).is_empty());

        // The global menu: the server's categories, no Title to offer.
        let g = MenuFacts { route: None, tree: Some(&t), clauses: &Clauses::default(), counts: &HashMap::new() };
        assert!(!Cat::SERVER.contains(&Cat::When) && !Cat::INPUTS.contains(&Cat::When), "FILTER-54: a time is a rule's, set in the card");
        // TICKETS-9: People — its own pane of three cells, found by them.
        assert!(offered(Cat::People, &g).is_empty());
        assert_eq!(time_cat(&crate::when::new_people(None, true)), Cat::People);
        assert_eq!(crate::when::group(&crate::when::new_people(Some(menger_tickets_api::filter::Who::Me), false)).format(), "assigned me");
        assert_eq!(crate::when::group(&crate::when::new_people(None, true)).format(), "unassigned");
        // CHAT-4: Chats — its own pane of two cells, found by them, and the
        // pills it builds drawn under it.
        assert!(offered(Cat::Chats, &g).is_empty());
        assert_eq!(time_cat(&crate::when::new_chats(false)), Cat::Chats);
        assert_eq!(crate::when::group(&crate::when::new_chats(true)).format(), "unread");
        assert_eq!(crate::when::group(&crate::when::new_chats(false)).format(), "chatting");
        assert!(offered(Cat::When, &g).is_empty(), "When draws its own pane");
        assert!(offered(Cat::Deployed, &g).is_empty(), "DEPLOY-13: and Deployed, When's pane");
        let dep = crate::when::new_deployed("prod", false, None).unwrap();
        assert_eq!(time_cat(&dep), Cat::Deployed);
        assert_eq!(time_cat(&crate::when::new_clause(crate::when::Kind::Changed, None, None).unwrap()), Cat::When);
        // FILTER-21: typing finds Menger docs, by its name.
        assert_eq!(offered(Cat::MengerDocs, &g), [val(Cat::MengerDocs, "yes"), val(Cat::MengerDocs, "no")]);
        // MENGER-12: Asking is a flag; Label offers `needs-you`, every label
        // the counts name and the clause's own, in name order.
        assert_eq!(offered(Cat::Asking, &g), [val(Cat::Asking, "yes"), val(Cat::Asking, "no")]);
        assert_eq!(offered(Cat::Label, &g), [val(Cat::Label, "needs-you")]);
        let mut counts = HashMap::new();
        counts.insert(Cat::Label, [("blocked-on-ci", 2), ("needs-you", 3)].into_iter().map(|(k, v)| (k.to_string(), v)).collect());
        let clauses = Clauses::default().with(Cat::Label, Op::Is, strs(&["zz-mine"]));
        let l = MenuFacts { route: None, tree: Some(&t), clauses: &clauses, counts: &counts };
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
        assert_eq!(op_label(Cat::Title, title.op, 1), "does not contain");
        assert_eq!(q.to_pairs(), [lens::Pair::new("q", Some("not:envelope".into()))]);

        // A flag: no switch — nothing to set; its value is the negation.
        let mut g = Clauses::default().with(Cat::Ready, Op::Is, strs(&["yes"]));
        g.set_op(Cat::Ready, Op::Not);
        assert_eq!(g.get(Cat::Ready), Some(&Clause { op: Op::Is, values: strs(&["yes"]) }));
        g.edit_as(Cat::Blocked, Op::Not, |k| k.toggle(Cat::Blocked, "yes"));
        assert_eq!(g.get(Cat::Blocked), Some(&Clause { op: Op::Is, values: strs(&["yes"]) }), "not turned over");
    }
}
