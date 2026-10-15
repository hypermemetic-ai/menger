//! FILTER-11: the generic filter bar — WAVE-12's menu, made one component
//! that renders any level's filter (src/lens.rs), configured with the
//! level's categories and given a counts provider.
//!
//! - **The categories** ([`Cat`]): the server-backed ones, offered at every
//!   level — Shortcode (the hosting tree, tri-state), Status (grouped, most
//!   important first; a group's header checks its statuses), Kind,
//!   Severity, and the flags Ready, Blocked, Commits, Menger docs (yes · no;
//!   FILTER-21's `menger doc`, which the nav's default excludes) — each a
//!   group of FILTER-10's language; and a surface's own (local) ones, which
//!   the language has not got: the wave board's Wave, Readiness, Off-board
//!   blockers and Title (WAVE-12, FILTER-9 decision 6); the inputs page's
//!   State and Removed (FILTER-16).
//! - **A pill** ([`Pill`]) is a category's clause — `[Category] [is any of |
//!   is not] [values +n] [×]`: the operator toggles, the values reopen the
//!   category, × removes it — or a group no category draws (`(blocked or
//!   status Blocked)`, `under X`, a `category` group): a `custom` pill
//!   showing its canonical text, which × removes. A clause is
//!   [`Clause`]: an operator and canonical values; a flag's is one value,
//!   `is yes` or `is no`, and its operator turns the value over.
//! - **The bar** ([`FilterBar`]): a `Filter` button opening the menu, then
//!   — on a surface — the global pills, muted, marked `global`, not
//!   editable there (a click opens the global menu at its category), then
//!   the level's own pills, `reset` when the level differs from its
//!   default, and a `text` toggle: the level's filter as canonical text,
//!   editable; Enter applies it, and a parse error lists every bad chunk
//!   under the box and applies nothing. Under the bar, when the surface's
//!   effective filter admits nothing, "Nothing can match" and the clashing
//!   groups, each marked with its level.
//! - **The menu** (WAVE-12's): a search field over the categories and,
//!   typed into, every category's values ([`search`]); a category opens its
//!   values beside it (in place, with a ‹ back row, on a narrow window): a
//!   checkbox, the label, the count — from the level's [`CountsProvider`]
//!   (none shows while a level has no provider). Placed under its anchor
//!   with the ticket menu's [`menu::clamp`]; Escape or a mousedown outside
//!   closes it.
//! - **The operator switch** (FILTER-19): a category's values open with
//!   `[ is | is not ]` — Title's `[ contains | does not contain ]`; a flag
//!   has none, its `yes` · `no` being the negation already ([`switch_words`]).
//!   It shows the clause's operator, or — no clause yet — the one the pane
//!   holds, `is` until it is set ([`switch_op`]). Set, it turns the clause's
//!   operator exactly as a pill's toggle does ([`Clausal::set_op`]); a value
//!   checked in the pane starts a clause under it ([`Clausal::edit_as`]); a
//!   value checked from the search's results keeps its clause's operator
//!   (`is` for a new one). One button: Tab from the search reaches it, Space
//!   or Enter flips it.
//! - **Any of** (FILTER-31): a group of two pills or more is drawn as a box,
//!   its members joined by `or` (any of) or `and` (all of) — the joiner a
//!   button that flips it; `+ or` adds an alternative. Each own pill has a
//!   `⋯` menu ([`pill_menu_view`]): **Or…** (the menu opens adding to that
//!   pill: what is picked joins it in an any-of group), and on a group
//!   **Make it all of / any of**, **Ungroup**, **Remove**. Shift-click picks
//!   pills; two or more picked, **Group as any of**. While the menu adds to
//!   a group ([`Pop::into`]) its categories are the language's, its checks
//!   are the group's members, and an edit the group cannot hold (a group
//!   inside a group) is refused with why ([`Pop::refusal`]). A category
//!   used inside a group is marked in the menu. The decisions are
//!   src/anyof.rs's.
//!
//! Everything that decides — the categories and their values, a clause's
//! edits, the tree and its tri-state, the menu's rows, a pill's words — is
//! a pure function here, pinned by native tests; the pills ⇄ groups of the
//! language are src/lens.rs's.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use leptos::prelude::*;
use menger_tickets_api::filter::Conj;
use menger_tickets_api::{Group, Status};
use wasm_bindgen::JsCast;

use crate::anyof::{self, Member, Slot};
use crate::bindings::{self, Context};
use crate::keys::palette_match;
use crate::lens::{self, Lens, Level};
use crate::nav::kind_glyph;
use crate::waves::epic_of;
use crate::{dom, keys, menu, st_class};

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
    pub(crate) const ALL: [Cat; 18] = [
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
    pub(crate) const SERVER: [Cat; 12] = [
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
    pub(crate) const INPUTS: [Cat; 14] = [
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

    fn glyph(self) -> &'static str {
        match self {
            Cat::Shortcode => "#",
            Cat::Status => "◐",
            Cat::Kind => "◇",
            Cat::Severity => "!",
            Cat::Ready => "✓",
            Cat::Blocked => "⊘",
            Cat::Commits => "⑂",
            Cat::MengerDocs => "§",
            Cat::Asking => "⁇",
            Cat::Label => "⌗",
            Cat::When => "◷",
            Cat::Deployed => "⇡",
            Cat::Wave => "≋",
            Cat::Readiness => "▸",
            Cat::OffBoard => "⧖",
            Cat::Title => "T",
            Cat::State => "?",
            Cat::Removed => "⌫",
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
            Cat::Shortcode | Cat::Wave | Cat::Title | Cat::Label | Cat::When | Cat::Deployed => &[],
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

/// How a pill reads its operator: `is` / `is any of` / `is not`, and for
/// Title `contains` / `does not contain`.
pub(crate) fn op_label(c: Cat, op: Op, n: usize) -> &'static str {
    match (c, op) {
        (Cat::Title, Op::Is) => "contains",
        (Cat::Title, Op::Not) => "does not contain",
        (_, Op::Is) if n == 1 => "is",
        (_, Op::Is) => "is any of",
        (_, Op::Not) => "is not",
    }
}

/// DEPLOY-13: the category a time pill is drawn under — Deployed for a
/// `deployed`, When for the rest.
pub(crate) fn time_cat(t: &crate::when::Timed) -> Cat {
    if crate::when::env_of(t).is_some() {
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
    (!c.is_flag() && c != Cat::When && c != Cat::Deployed).then(|| (op_label(c, Op::Is, 1), op_label(c, Op::Not, 1)))
}

/// FILTER-19: the operator the switch shows — the clause's; with no clause,
/// `held`, the one the open pane holds (`is` until the switch is set).
pub(crate) fn switch_op(cl: Option<&Clause>, held: Op) -> Op {
    cl.map_or(held, |cl| cl.op)
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

    /// A group of `c`'s values (Status's group, by its header): every one
    /// added — or, all there already, every one taken.
    fn toggle_all(&mut self, c: Cat, ts: &[&str]) {
        let mut values: Vec<String> = self.clause(c).map(|cl| cl.values).unwrap_or_default();
        if ts.iter().all(|t| values.iter().any(|v| v == t)) {
            values.retain(|v| !ts.contains(&v.as_str()));
        } else {
            values.extend(ts.iter().map(|t| t.to_string()));
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

impl Tri {
    fn as_str(self) -> &'static str {
        match self {
            Tri::On => "on",
            Tri::Off => "off",
            Tri::Mixed => "mixed",
        }
    }

    fn aria(self) -> &'static str {
        match self {
            Tri::On => "true",
            Tri::Off => "false",
            Tri::Mixed => "mixed",
        }
    }
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
                hosts.insert(g.shortcode.to_string(), h.clone());
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

/// A pill of a level's bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pill {
    /// A category's clause.
    Clause { cat: Cat, clause: Clause },
    /// A group no category draws — its canonical text; `at`: which of the
    /// level's groups it is.
    Custom { text: String, at: usize },
    /// FILTER-27: a time clause (When's) — `at`: which of the level's
    /// groups it is; edited part by part.
    Time { at: usize, t: crate::when::Timed },
    /// FILTER-31: a group of two pills or more — `at`: which of the level's
    /// groups it is; its join and its members (src/anyof.rs).
    Group { at: usize, conj: Conj, members: Vec<Member> },
}

/// The order a clause's values show in on its pill: Shortcode's as the
/// menu lists the tree (the route's own first), the rest as stored.
pub(crate) fn display_order(c: Cat, cl: &Clause, route: Option<&str>, tree: Option<&ScTree>) -> Vec<String> {
    match (c, tree) {
        (Cat::Shortcode, Some(t)) => {
            let at: HashMap<String, usize> = t
                .rows(route, cl.values.iter())
                .into_iter()
                .filter_map(|r| if let ScRow::Code { code, .. } = r { Some(code) } else { None })
                .enumerate()
                .map(|(i, c)| (c, i))
                .collect();
            let mut v = cl.values.clone();
            v.sort_by_key(|x| at.get(x).copied().unwrap_or(usize::MAX));
            v
        }
        _ => cl.values.clone(),
    }
}

/// FILTER-12: a pill in words — `Status is any of Ready, Coding`, `Kind is
/// not doc`, a custom group's text — for where it is shown, not edited.
pub(crate) fn pill_words(p: &Pill) -> (String, String) {
    match p {
        Pill::Clause { cat, clause } => {
            let values = clause.values.iter().map(|v| cat.value_label(v)).collect::<Vec<_>>().join(", ");
            (cat.label().to_string(), format!("{} {values}", op_label(*cat, clause.op, clause.values.len())))
        }
        Pill::Custom { text, .. } => ("custom".to_string(), text.clone()),
        Pill::Time { t, .. } => (time_cat(t).label().to_string(), crate::when::words(t, crate::calendar::reader_h12())),
        Pill::Group { conj, members, .. } => {
            let words = members.iter().map(Member::words).collect::<Vec<_>>().join(&format!(" {} ", anyof::joiner(*conj)));
            (anyof::conj_words(*conj).to_string(), words)
        }
    }
}

/// A pill's values: the first, then `+n` for the rest.
pub(crate) fn pill_values(c: Cat, shown: &[String]) -> (String, Option<String>) {
    let first = shown.first().map(|v| c.value_label(v)).unwrap_or_default();
    (first, (shown.len() > 1).then(|| format!("+{}", shown.len() - 1)))
}

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

/// A row of the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    /// A category, to open.
    Cat(Cat),
    /// A value to check or uncheck. `depth` indents a shortcode under its
    /// host; `scoped`: shown among every category's (a search), so it names
    /// its category.
    Value { cat: Cat, token: String, depth: usize, scoped: bool },
    /// A group of a category's values, by its header — checks them all
    /// (Status's groups).
    Group { cat: Cat, label: &'static str, tokens: &'static [&'static str] },
    /// The search's last offer: Title contains what was typed.
    TitleContains(String),
    /// A heading over values — Shortcode's "other boards".
    Heading(&'static str),
}

impl Row {
    fn actionable(&self) -> bool {
        !matches!(self, Row::Heading(_))
    }
}

/// What the menu needs to know to list values.
pub(crate) struct MenuFacts<'a> {
    /// The level's categories, in its menu's order.
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

/// `c`'s values matching what is typed (all of them, as offered, when
/// nothing is): a match is the label or the value, case-insensitive; the
/// headings go, and the tree flattens, while a query narrows.
pub(crate) fn offered_matching(c: Cat, k: &MenuFacts, q: &str) -> Vec<Row> {
    let rows = offered(c, k);
    if q.trim().is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter_map(|r| match r {
            Row::Value { cat, token, .. } if palette_match(&token, &cat.value_label(&token), q) => {
                Some(Row::Value { cat, token, depth: 0, scoped: false })
            }
            _ => None,
        })
        .collect()
}

/// The menu with nothing open: the categories while nothing is typed;
/// typed into, the categories whose name matches, every category's values
/// that match (each naming its category), and last "Title contains" what
/// was typed (when the level offers Title).
pub(crate) fn search(q: &str, k: &MenuFacts) -> Vec<Row> {
    let q = q.trim();
    if q.is_empty() {
        return k.cats.iter().copied().map(Row::Cat).collect();
    }
    let mut out: Vec<Row> = k.cats.iter().copied().filter(|c| palette_match(c.menu_label(), c.keywords(), q)).map(Row::Cat).collect();
    for c in k.cats {
        for r in offered_matching(*c, k, q) {
            if let Row::Value { cat, token, .. } = r {
                out.push(Row::Value { cat, token, depth: 0, scoped: true });
            }
        }
    }
    if k.cats.contains(&Cat::Title) {
        out.push(Row::TitleContains(q.to_string()));
    }
    out
}

/// A Status group's box: all its statuses checked, none, or some.
pub(crate) fn group_tri(cl: Option<&Clause>, tokens: &[&str]) -> Tri {
    let n = tokens.iter().filter(|t| cl.is_some_and(|cl| cl.values.iter().any(|v| v == *t))).count();
    match n {
        0 => Tri::Off,
        n if n == tokens.len() => Tri::On,
        _ => Tri::Mixed,
    }
}

// ---------------------------------------------------------------------
// The view: the menu's state

/// Where a popover opened: under its level's Filter button, or under a
/// pill (the pill's selector) — `direct`: that category's values alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Anchor {
    sel: String,
    direct: bool,
}

/// A level's menu — its state, every handle `Copy`. The lens holds one per
/// level for the app's lifetime (so `F`, and a click on an inherited pill,
/// can open a menu another view draws); the view drawing it says what it
/// offers ([`Pop::configure`]).
#[derive(Clone, Copy)]
pub(crate) struct Pop {
    pub(crate) level: Level,
    open: RwSignal<Option<Anchor>>,
    /// The category whose values are open.
    cat: RwSignal<Option<Cat>>,
    query: RwSignal<String>,
    /// The highlighted row among the active list's actionable rows.
    hi: RwSignal<usize>,
    /// FILTER-19: the operator the open category's switch holds while it
    /// has no clause — `is` as a category opens.
    held: RwSignal<Op>,
    /// What the level's bar offers: its categories and its counts — set by
    /// the view that draws it; `mounted` while it does.
    cats: RwSignal<Vec<Cat>>,
    counts: RwSignal<CountsProvider>,
    mounted: RwSignal<u32>,
    /// FILTER-27: where the When pane is, and whether its window editor
    /// (Custom…) is open.
    pub(crate) when: RwSignal<crate::when_view::Step>,
    pub(crate) custom: RwSignal<bool>,
    /// FILTER-31: the group the menu adds to and edits — Or… on a pill, a
    /// group's `+ or`, a member's values: the level's group at that place;
    /// `None`, the level's own clauses.
    pub(crate) into: RwSignal<Option<usize>>,
    /// FILTER-31: why the last edit was refused (an alternative that would
    /// put a group inside a group) — shown in the menu, or under the bar.
    pub(crate) refusal: RwSignal<Option<String>>,
    /// FILTER-31: the pills Shift-clicked, by their group's place.
    pub(crate) picked: RwSignal<Vec<usize>>,
    /// FILTER-31: whose `⋯` menu is open — a pill's group's place.
    pub(crate) more: RwSignal<Option<usize>>,
}

impl Pop {
    pub(crate) fn new(level: Level) -> Self {
        Pop {
            level,
            open: RwSignal::new(None),
            cat: RwSignal::new(None),
            query: RwSignal::new(String::new()),
            hi: RwSignal::new(0),
            held: RwSignal::new(Op::Is),
            cats: RwSignal::new(Cat::SERVER.to_vec()),
            counts: RwSignal::new(CountsProvider::none()),
            mounted: RwSignal::new(0),
            when: RwSignal::new(crate::when_view::Step::Root),
            custom: RwSignal::new(false),
            into: RwSignal::new(None),
            refusal: RwSignal::new(None),
            picked: RwSignal::new(Vec::new()),
            more: RwSignal::new(None),
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

    /// DEPLOY-13: the category whose pane is open (tracked) — When's pane
    /// draws Deployed's rows under Deployed.
    pub(crate) fn open_cat_tracked(self) -> Option<Cat> {
        self.cat.get()
    }

    pub(crate) fn is_mounted(self) -> bool {
        self.mounted.get_untracked() > 0
    }

    pub(crate) fn is_open(self) -> bool {
        self.open.with_untracked(Option::is_some)
    }

    /// FILTER-12: whether it is open (tracked) — when a server-backed
    /// level's counts are asked.
    pub(crate) fn open_tracked(self) -> bool {
        self.open.with(Option::is_some)
    }

    /// Opened under `sel` — on `cat`'s values when given (alone when
    /// `direct`).
    pub(crate) fn show(self, sel: String, cat: Option<Cat>, direct: bool) {
        if self.open.with_untracked(Option::is_none) {
            keys::remember_focus();
        }
        self.query.set(String::new());
        self.hi.set(0);
        self.held.set(Op::Is);
        self.when.set(crate::when_view::Step::Root);
        self.custom.set(false);
        self.into.set(None);
        self.refusal.set(None);
        self.cat.set(cat);
        self.open.set(Some(Anchor { sel, direct: direct && cat.is_some() }));
    }

    /// Opened under the level's Filter button, on `cat` when given.
    pub(crate) fn show_at_button(self, cat: Option<Cat>) {
        self.show(button_sel(self.level), cat, false);
    }

    /// Closed — focus handed back where it was (`restore`: a key closed it),
    /// or left where a click put it.
    pub(crate) fn close(self, restore: bool) {
        if self.open.try_with_untracked(Option::is_none).unwrap_or(true) {
            return;
        }
        self.open.set(None);
        self.cat.set(None);
        self.into.set(None);
        self.held.set(Op::Is);
        self.query.set(String::new());
        if restore {
            keys::restore_focus();
        } else {
            keys::forget_focus();
        }
    }

    fn direct(self) -> bool {
        self.open.with_untracked(|a| a.as_ref().is_some_and(|a| a.direct))
    }

    /// Placed again under its anchor (a resize can move it).
    pub(crate) fn reposition(self) {
        if let Some(a) = self.open.get_untracked() {
            place(self.level, &a.sel);
        }
    }

    fn open_cat(self, c: Cat) {
        self.held.set(Op::Is);
        self.when.set(crate::when_view::Step::Root);
        self.custom.set(false);
        self.cat.set(Some(c));
        self.query.set(String::new());
        self.hi.set(0);
    }

    /// Back from a category's values to the categories, on it.
    fn back(self) {
        if let Some(c) = self.cat.get_untracked() {
            self.held.set(Op::Is);
            self.cat.set(None);
            self.query.set(String::new());
            self.hi.set(self.cats.with_untracked(|cs| cs.iter().position(|x| *x == c).unwrap_or(0)));
        }
    }

    /// FILTER-31: the menu opened adding to the group at `at` (Or… on a
    /// pill, a group's `+ or`) — under `sel`, on `cat` when given (alone,
    /// `direct`).
    pub(crate) fn show_into(self, lens: Lens, at: usize, sel: String, cat: Option<Cat>, direct: bool) {
        self.close(false);
        lens.open_menu(self.level, cat, Some((sel, direct)));
        self.into.set(Some(at));
    }

    /// FILTER-31: an edit made through the menu — `f` given the group it
    /// adds to; refused, why is kept to be shown; a group the edit took
    /// away is no longer added to.
    fn edit(self, lens: Lens, f: impl FnOnce(Option<usize>) -> Result<(), String>) {
        let into = self.into.get_untracked();
        let count = move || untrack(|| lens.kept(self.level)).map_or(0, |k| k.filter.groups.len());
        let before = count();
        match f(into) {
            Ok(()) => {
                self.refusal.set(None);
                if into.is_some() && count() < before {
                    self.into.set(None);
                }
            }
            Err(why) => self.refusal.set(Some(why)),
        }
    }

    /// The clauses the menu checks: the level's, or the group's it adds to
    /// (FILTER-31). Tracked.
    fn clauses(self, lens: Lens) -> Clauses {
        lens.clauses_in(self.level, self.into.get())
    }

    /// FILTER-19: the operator `c`'s switch shows. Tracked.
    fn op(self, lens: Lens, c: Cat) -> Op {
        let held = self.held.get();
        switch_op(self.clauses(lens).get(c), held)
    }

    /// FILTER-19: the operator a value checked in `c`'s pane is checked
    /// under — what the switch shows, which the pane now holds, so a clause
    /// emptied there leaves the switch as it was.
    fn pane_op(self, lens: Lens, c: Cat) -> Op {
        let op = untrack(|| self.op(lens, c));
        self.held.set(op);
        op
    }

    /// FILTER-19: the switch flipped — the clause's operator turned, as a
    /// pill's toggle turns it; no clause yet, the operator the next one
    /// takes.
    fn flip_switch(self, lens: Lens, c: Cat) {
        let next = untrack(|| self.op(lens, c)).flip();
        self.held.set(next);
        self.edit(lens, |into| lens.edit_in(self.level, into, |k| k.set_op(c, next)));
    }

    /// A value's box clicked (or Enter on its row): in the open category's
    /// pane, under its switch; from the search's results (`scoped`), its
    /// clause's operator kept — `is` for a new one.
    fn check(self, lens: Lens, c: Cat, token: &str, scoped: bool) {
        let op = if scoped { Op::Is } else { self.pane_op(lens, c) };
        self.edit(lens, |into| lens.toggle(self.level, into, c, token, op));
    }

    /// The menu's facts, read (and tracked) for `f`.
    fn facts<T>(self, lens: Lens, f: impl FnOnce(&MenuFacts) -> T) -> T {
        let route = lens.route_sc(self.level);
        let counts = self.counts.get().get();
        let clauses = self.clauses(lens);
        // FILTER-31: adding to a group, the language's categories alone.
        let into = self.into.get().is_some();
        let cats: Vec<Cat> = self.cats.with(|cats| cats.iter().copied().filter(|c| !(into && c.is_local())).collect());
        lens.sctree().with(|tree| f(&MenuFacts { cats: &cats, route: route.as_deref(), tree: tree.as_ref(), clauses: &clauses, counts: &counts }))
    }

    /// The rows the keys move over and Enter acts on.
    fn active(self, lens: Lens) -> Vec<Row> {
        let q = self.query.get_untracked();
        let rows = untrack(|| match self.cat.get_untracked() {
            Some(c) => self.facts(lens, |k| offered_matching(c, k, &q)),
            None => self.facts(lens, |k| search(&q, k)),
        });
        rows.into_iter().filter(Row::actionable).collect()
    }

    fn act(self, lens: Lens, row: &Row) {
        match row {
            Row::Cat(c) => self.open_cat(*c),
            Row::Value { cat, token, scoped, .. } => self.check(lens, *cat, token, *scoped),
            Row::Group { cat, tokens, .. } => {
                let op = self.pane_op(lens, *cat);
                let (cat, tokens) = (*cat, *tokens);
                self.edit(lens, |into| lens.edit_in(self.level, into, |k| k.edit_as(cat, op, |k| k.toggle_all(cat, tokens))));
            }
            Row::TitleContains(text) => {
                let text = text.clone();
                lens.edit(self.level, |k| k.set(Cat::Title, Op::Is, [text]));
                self.close(true);
            }
            Row::Heading(_) => {}
        }
    }

    /// The search field's keys: the arrows move, Enter picks the highlighted
    /// row (the first match), → opens a category, ← or a Backspace on an
    /// empty field goes back, Esc closes; Tab moves on to the open
    /// category's operator switch (FILTER-19), or — none — closes.
    fn key(self, lens: Lens, ev: web_sys::KeyboardEvent) {
        let id = bindings::hit(&ev, Context::Menu);
        let empty = self.query.with_untracked(String::is_empty);
        match id {
            Some("dialog.close") => {
                ev.prevent_default();
                ev.stop_propagation();
                self.close(true);
            }
            Some("list.next" | "list.prev") => {
                let len = self.active(lens).len();
                if let Some(n) = menu::step(Some(self.hi.get_untracked().min(len.saturating_sub(1))), len, id) {
                    self.hi.set(n);
                    ev.prevent_default();
                }
            }
            Some("list.choose") => {
                ev.prevent_default();
                let rows = self.active(lens);
                if let Some(r) = rows.get(self.hi.get_untracked()).or(rows.first()) {
                    self.act(lens, r);
                }
            }
            Some("menu.into") if empty && self.cat.get_untracked().is_none() => {
                if let Some(Row::Cat(c)) = self.active(lens).get(self.hi.get_untracked()) {
                    ev.prevent_default();
                    self.open_cat(*c);
                }
            }
            Some("menu.out") if empty && self.cat.get_untracked().is_some() && !self.direct() => {
                ev.prevent_default();
                self.back();
            }
            // The switch follows the field: Tab's own move reaches it.
            Some("menu.leave") if self.cat.get_untracked().and_then(switch_words).is_some() => {}
            Some("menu.leave" | "menu.leave-back") => self.close(false),
            _ => {}
        }
    }
}

/// The popover's DOM id for a level.
pub(crate) fn pop_id(level: Level) -> String {
    format!("fmenu-{}", level.name())
}

/// The selector of a level's Filter button.
pub(crate) fn button_sel(level: Level) -> String {
    format!("[data-fbtn=\"{}\"]", level.name())
}

/// The selector of a pill's values: `level`'s pill of `c` in the bar of
/// `bar` (a surface's bar draws the global pills too).
fn pill_sel(bar: Level, level: Level, c: Option<Cat>) -> String {
    let cat = c.map_or_else(|| "[data-custom]".to_string(), |c| format!("[data-cat=\"{}\"]", c.key()));
    format!("[data-fbar=\"{}\"] [data-fpill=\"{}\"]{cat} .p-vals", bar.name(), level.name())
}

fn document() -> Option<web_sys::Document> {
    dom::window().document()
}

/// Focus the popover's field — the title's text when Title is open, the
/// search otherwise — once it is drawn.
fn focus_field(level: Level) {
    request_animation_frame(move || {
        let Some(doc) = document() else { return };
        let id = pop_id(level);
        let el = doc
            .query_selector(&format!("#{id} input.ftitle-in"))
            .ok()
            .flatten()
            .or_else(|| doc.query_selector(&format!("#{id} input.fsearch")).ok().flatten());
        if let Some(el) = el.and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = el.focus();
        }
    });
}

/// The width the categories and a category's values need side by side;
/// with less room the values take the categories' place, behind a back row.
const SIDE_BY_SIDE_PX: f64 = 580.0;

/// Place a level's popover under its anchor, kept inside the content
/// column it hangs in (the ticket menu's [`menu::clamp`], over the column
/// rather than the viewport — the column clips what overflows it); it hangs
/// from its positioned parent, so it scrolls with the page. Its height is
/// what the viewport leaves below the anchor; with too little width for two
/// panes it is `narrow`.
pub(crate) fn place(level: Level, sel: &str) {
    let Some(doc) = document() else { return };
    let Some(pop) = doc.get_element_by_id(&pop_id(level)).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) else { return };
    let at = doc.query_selector(sel).ok().flatten().or_else(|| doc.query_selector(&button_sel(level)).ok().flatten());
    let Some(at) = at else { return };
    let Some(bar) = pop.parent_element() else { return };
    let win = dom::window();
    let dim = |v: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>| v.ok().and_then(|v| v.as_f64()).unwrap_or(f64::INFINITY);
    let (vw, vh) = (dim(win.inner_width()), dim(win.inner_height()));
    // The column the bar sits in, within the viewport.
    let (ax, aw) = bar.closest(".content").ok().flatten().map_or((0.0, vw), |c| {
        let r = c.get_bounding_client_rect();
        let left = r.left().max(0.0);
        (left, r.right().min(vw) - left)
    });
    let _ = pop.class_list().toggle_with_force("narrow", aw < SIDE_BY_SIDE_PX);
    let style = pop.style();
    let _ = style.set_property("max-width", &format!("{}px", (aw - 8.0).max(160.0)));
    let (a, b, p) = (at.get_bounding_client_rect(), bar.get_bounding_client_rect(), pop.get_bounding_client_rect());
    let top = a.bottom() + 4.0;
    let (left, _) = menu::clamp(a.left() - ax, top, p.width(), 0.0, aw, vh);
    let _ = style.set_property("left", &format!("{}px", ax + left - b.left()));
    let _ = style.set_property("top", &format!("{}px", top - b.top()));
    let _ = style.set_property("max-height", &format!("{}px", (vh - top - 12.0).max(220.0)));
}

// ---------------------------------------------------------------------
// The view: the bar

/// The generic filter bar — a level's `Filter` button, the global pills
/// (on a surface: muted, marked `global`), the level's own pills, `reset`,
/// the `text` toggle, `Saved ▾` (FILTER-6) and the menu; under it, the
/// contradiction notice.
/// `cats`: the level's categories, server-backed and local, in its menu's
/// order; `counts`: what the menu's values count.
#[component]
pub(crate) fn FilterBar(
    level: Level,
    #[prop(into)] cats: Vec<Cat>,
    counts: CountsProvider,
    /// What the bar's Filter button says when hovered.
    #[prop(optional, into)]
    hint: Option<String>,
) -> impl IntoView {
    let lens = lens::use_lens();
    let pop = lens.pop(level);
    pop.configure(&cats, counts);
    let texting = RwSignal::new(false);
    let hint = hint.unwrap_or_else(|| "Filter (f)".into());
    view! {
        <div class="fbar" data-fbar=level.name() data-level=level.name()>
            <FilterButton level hint />
            {move || (!texting.get()).then(|| view! { <Pills bar=level /> })}
            <TextMode level texting />
            <Reset level />
            // FILTER-26: the nav's filter made global.
            {(level == Level::Nav).then(|| view! { <crate::promote::MakeGlobal /> })}
            // FILTER-6: the level's saved filters, and Save as….
            <crate::saved::SavedButton level />
            <Popover level />
        </div>
        <Clash level />
    }
}

/// A level's `Filter` button: it opens the level's menu, or closes it.
#[component]
pub(crate) fn FilterButton(level: Level, #[prop(into)] hint: String, #[prop(optional)] badge: bool) -> impl IntoView {
    let lens = lens::use_lens();
    let pop = lens.pop(level);
    let count = move || lens.group_count(level);
    view! {
        <button
            class="fbtn"
            data-fbtn=level.name()
            data-fopen=level.name()
            title=hint
            aria-haspopup="dialog"
            aria-expanded=move || pop.open.with(Option::is_some).to_string()
            on:click=move |_| {
                if pop.is_open() {
                    pop.close(true);
                } else {
                    lens.open_menu(level, None, None);
                }
            }
        >
            <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M2 3.5h12l-4.6 5.4v4.1l-2.8-1.3V8.9z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" />
            </svg>
            "Filter"
            {move || (badge && count() > 0).then(|| view! { <span class="fbadge" title="global filter groups">{count}</span> })}
        </button>
    }
}

/// The pills of `bar`'s bar ([`lens::bar_pills`]): on a surface, the
/// global ones first — muted, marked `global`, opening the global menu at
/// their category — then its own, editable. FILTER-31: a group is a box;
/// while the bar holds one, `and` stands between the pills; then what the
/// Shift-click picked offers, and a refusal while the menu is closed.
#[component]
pub(crate) fn Pills(bar: Level) -> impl IntoView {
    let lens = lens::use_lens();
    let pop = lens.pop(bar);
    // A pick, an open ⋯ menu: forgotten once the filter changes under them.
    Effect::new(move |_| {
        let _ = lens.kept(bar);
        pop.picked.set(Vec::new());
        pop.more.set(None);
    });
    let outside = window_event_listener(leptos::ev::mousedown, move |ev: web_sys::MouseEvent| {
        if pop.more.get_untracked().is_none() {
            return;
        }
        let inside = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|el| el.closest(".pmenu, .p-more").ok().flatten().is_some());
        if !inside {
            pop.more.set(None);
        }
    });
    on_cleanup(move || outside.remove());
    move || {
        let route = lens.route_sc(bar);
        let tree = lens.sctree().get();
        let global = if bar == Level::Global { Vec::new() } else { lens.pills(Level::Global) };
        let pills = lens::bar_pills(bar, global, lens.pills(bar));
        let grouped = pills.iter().any(|(_, p)| matches!(p, Pill::Group { .. }));
        let drawn = pills
            .into_iter()
            .enumerate()
            .map(|(i, (level, p))| {
                let fat = pill_at(lens, level, &p);
                view! {
                    {(grouped && i > 0).then(|| view! { <span class="fand" aria-hidden="true">"and"</span> })}
                    {pill_view(lens, bar, level, level != bar, p, fat, route.as_deref(), tree.as_ref())}
                }
            })
            .collect_view();
        view! {
            {drawn}
            {picked_view(lens, bar)}
            {refused_view(pop)}
        }
    }
}

/// FILTER-31: which of its level's groups a pill is — `None` for a
/// surface's own category's (no group holds it).
fn pill_at(lens: Lens, level: Level, p: &Pill) -> Option<usize> {
    match p {
        Pill::Clause { cat, .. } if cat.is_local() => None,
        Pill::Clause { cat, .. } => untrack(|| lens.kept(level)).and_then(|k| k.at_of(*cat)),
        Pill::Time { at, .. } | Pill::Custom { at, .. } | Pill::Group { at, .. } => Some(*at),
    }
}

/// FILTER-31: a Shift-click on an own pill picks it (or lets it go) — taken
/// before the pill's own buttons see the click.
fn pick(pop: Pop, fat: Option<usize>, inherited: bool) -> impl Fn(web_sys::MouseEvent) + 'static {
    move |ev: web_sys::MouseEvent| {
        let shift = bindings::hit_mouse(&ev, Context::FilterBar) == Some("filter.pick");
        let Some(at) = fat.filter(|_| !inherited && shift) else { return };
        ev.prevent_default();
        ev.stop_propagation();
        pop.picked.update(|p| match p.iter().position(|x| *x == at) {
            Some(i) => {
                p.remove(i);
            }
            None => p.push(at),
        });
    }
}

/// FILTER-31: a Shift-press on a pill picks it, never a run of text.
fn no_shift_select(ev: web_sys::MouseEvent) {
    if bindings::hit_mouse(&ev, Context::FilterBar) == Some("filter.pick") {
        ev.prevent_default();
    }
}

/// FILTER-31: what the picked pills offer — one picked, a hint; two or
/// more, Group as any of.
fn picked_view(lens: Lens, bar: Level) -> impl IntoView {
    let pop = lens.pop(bar);
    move || {
        let n = pop.picked.with(Vec::len);
        let group = move |_| {
            let ats = pop.picked.get_untracked();
            match lens.edit_groups(bar, |gs| anyof::group_any(gs, &ats)) {
                Ok(_) => pop.refusal.set(None),
                Err(why) => pop.refusal.set(Some(why)),
            }
            pop.picked.set(Vec::new());
        };
        (n > 0).then(|| {
            view! {
                <span class="fpick" role="group" aria-label="picked filters">
                    {if n > 1 {
                        view! { <button class="fgroup-any" title="make the picked pills one group, any of which matches" on:click=group>"Group as any of"</button> }.into_any()
                    } else {
                        view! { <span class="fpick-h">"Shift-click another pill to group"</span> }.into_any()
                    }}
                    <button class="p-x" title="let the picked pills go" aria-label="let the picked pills go" on:click=move |_| pop.picked.set(Vec::new())>"×"</button>
                </span>
            }
        })
    }
}

/// FILTER-31: an edit refused, under the bar while the menu is closed (the
/// menu shows it while open).
fn refused_view(pop: Pop) -> impl IntoView {
    move || {
        (!pop.open_tracked()).then(|| pop.refusal.get()).flatten().map(|why| {
            view! {
                <span class="frefused" role="alert">
                    {why}
                    <button class="p-x" title="dismiss" aria-label="dismiss" on:click=move |_| pop.refusal.set(None)>"×"</button>
                </span>
            }
        })
    }
}

/// FILTER-31: the `⋯` on an own pill at `at` and, open, its menu.
fn more_view(lens: Lens, bar: Level, at: usize) -> impl IntoView {
    let pop = lens.pop(bar);
    let open = move || pop.more.get() == Some(at);
    view! {
        <span class="pm-wrap">
            <button
                class="p-more"
                title="Or…, negate, group, remove"
                aria-label="more for this filter"
                aria-haspopup="menu"
                aria-expanded=move || open().to_string()
                on:click=move |_| pop.more.set(if pop.more.get_untracked() == Some(at) { None } else { Some(at) })
            >
                "⋯"
            </button>
            {move || open().then(|| pill_menu_view(lens, bar, at))}
        </span>
    }
}

/// FILTER-31: a pill's menu — **Or…** (the filter menu opens adding to the
/// pill: what is picked joins it in an any-of group; off, with why, where
/// it cannot), and on a group **Make it all of / any of**, **Ungroup**;
/// **Remove**. The decisions are src/anyof.rs's; FILTER-30's items go here.
pub(crate) fn pill_menu_view(lens: Lens, bar: Level, at: usize) -> impl IntoView {
    let pop = lens.pop(bar);
    let Some(g) = untrack(|| lens.kept(bar)).and_then(|k| k.filter.groups.get(at).cloned()) else {
        return ().into_any();
    };
    let boxed = anyof::is_group(&g);
    let or_why = anyof::or_refusal(&g);
    let flip_why = anyof::flip_refusal(&g);
    // FILTER-30: Negate — the time pill, or each time member of a box.
    let negatable = anyof::negatable(&g, at);
    let answer = move |r: Result<(), String>| {
        pop.more.set(None);
        match r {
            Ok(()) => pop.refusal.set(None),
            Err(why) => pop.refusal.set(Some(why)),
        }
    };
    let sel = format!("[data-fbar=\"{0}\"] [data-fpill=\"{0}\"][data-fat=\"{at}\"] .p-more", bar.name());
    let or = move |_| {
        pop.more.set(None);
        pop.show_into(lens, at, sel.clone(), None, false);
    };
    let flip_label = if g.conj == Conj::Any { "Make it all of" } else { "Make it any of" };
    let menu = NodeRef::<leptos::html::Span>::new();
    // The first item takes focus, so the keys reach the menu.
    Effect::new(move |_| {
        if let Some(el) = menu.get() {
            if let Ok(Some(first)) = el.query_selector("button:not([disabled])") {
                if let Ok(b) = first.dyn_into::<web_sys::HtmlElement>() {
                    let _ = b.focus();
                }
            }
        }
    });
    view! {
        <span
            class="pmenu"
            role="menu"
            node_ref=menu
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                if bindings::is(&ev, Context::Menu, "dialog.close") {
                    ev.prevent_default();
                    ev.stop_propagation();
                    pop.more.set(None);
                }
            }
        >
            <button class="pm-i" role="menuitem" data-act="or" data-fopen=bar.name() disabled=or_why.is_some() title=or_why.clone().unwrap_or_else(|| "add an alternative: any of them matches".into()) on:click=or>
                "Or…"
            </button>
            {or_why.map(|why| view! { <span class="pm-why">{why}</span> })}
            {boxed.then(|| view! {
                <button class="pm-i" role="menuitem" data-act="flip" disabled=flip_why.is_some() title=flip_why.clone().unwrap_or_default() on:click=move |_| answer(lens.edit_groups(bar, |gs| anyof::flip(gs, at)))>
                    {flip_label}
                </button>
                <button class="pm-i" role="menuitem" data-act="ungroup" title="each pill back on its own, joined by and" on:click=move |_| answer(lens.edit_groups(bar, |gs| { anyof::ungroup(gs, at); Ok(()) }))>
                    "Ungroup"
                </button>
            })}
            {negatable.into_iter().map(|(slot, t)| {
                let label = match slot.member {
                    None => "Negate".to_string(),
                    Some(_) => format!("Negate “{}”", crate::when::words(&t, crate::calendar::reader_h12())),
                };
                let turned = crate::when::words(&crate::when::negate(&t), crate::calendar::reader_h12());
                view! {
                    <button class="pm-i" role="menuitem" data-act="negate" data-member=slot.member title=format!("turn it around: {turned}") on:click=move |_| answer(lens.edit_groups(bar, |gs| anyof::negate(gs, slot)))>
                        {label}
                    </button>
                }
            }).collect_view()}
            <button class="pm-i" role="menuitem" data-act="remove" on:click=move |_| answer(lens.edit_groups(bar, |gs| { if at < gs.len() { gs.remove(at); } Ok(()) }))>
                "Remove"
            </button>
        </span>
    }
    .into_any()
}

#[allow(clippy::too_many_arguments)]
fn pill_view(lens: Lens, bar: Level, level: Level, inherited: bool, p: Pill, fat: Option<usize>, route: Option<&str>, tree: Option<&ScTree>) -> AnyView {
    let pop = lens.pop(level);
    let mark = inherited.then(|| view! { <span class="p-lvl" title=format!("set at the {} level", level.name())>{level.name()}</span> });
    let picked = move || fat.is_some_and(|a| pop.picked.with(|p| p.contains(&a)));
    let more = fat.filter(|_| !inherited).map(|at| more_view(lens, bar, at));
    match p {
        Pill::Clause { cat: c, clause: cl } => {
            let shown = display_order(c, &cl, route, tree);
            let (first, more_n) = pill_values(c, &shown);
            let all = shown.iter().map(|v| c.value_label(v)).collect::<Vec<_>>().join(", ");
            let op = op_label(c, cl.op, cl.values.len());
            let sel = pill_sel(bar, level, Some(c));
            let open = move |_| {
                if pop.open.with_untracked(|a| a.as_ref().is_some_and(|a| a.sel == sel)) {
                    pop.close(true);
                } else {
                    pop.close(false);
                    // An inherited pill opens the global menu at its
                    // category; an own pill, that category's values alone.
                    lens.open_menu(level, Some(c), Some((sel.clone(), !inherited)));
                }
            };
            let op_view = if inherited {
                view! { <span class="p-op">{op}</span> }.into_any()
            } else {
                view! { <button class="p-op" title="toggle the operator" on:click=move |_| lens.edit(level, |k| k.flip(c))>{op}</button> }
                    .into_any()
            };
            let x = (!inherited).then(|| {
                view! {
                    <button
                        class="p-x"
                        title="remove this filter"
                        aria-label=format!("remove the {} filter", c.label())
                        on:click=move |_| lens.edit(level, |k| k.remove(c))
                    >
                        "×"
                    </button>
                }
            });
            view! {
                <span
                    class="pill"
                    class:inherited=inherited
                    class:picked=picked
                    data-fpill=level.name()
                    data-fat=fat
                    data-cat=c.key()
                    data-op=cl.op.word()
                    on:mousedown=no_shift_select on:click:capture=pick(pop, fat, inherited)
                >
                    <span class="p-cat">{c.label()}</span>
                    {op_view}
                    <button class="p-vals" data-fopen=level.name() title=all on:click=open>
                        {first}
                        {more_n.map(|m| view! { <span class="more">" "{m}</span> })}
                    </button>
                    {mark}
                    {more}
                    {x}
                </span>
            }
            .into_any()
        }
        Pill::Time { at, t } => time_pill_view(lens, bar, level, inherited, Slot { at, member: None }, t, more),
        Pill::Group { at, conj, members } => group_view(lens, bar, level, inherited, at, conj, members, more),
        Pill::Custom { text, at } => {
            let sel = pill_sel(bar, level, None);
            let x = (!inherited).then(|| {
                view! {
                    <button class="p-x" title="remove this filter" aria-label="remove this filter" on:click=move |_| lens.remove_custom(level, at)>
                        "×"
                    </button>
                }
            });
            // A custom group is edited as text; a click opens the menu (the
            // global one, from a surface's bar).
            let open = move |_| lens.open_menu(level, None, Some((sel.clone(), false)));
            view! {
                <span
                    class="pill custom"
                    class:inherited=inherited
                    class:picked=picked
                    data-fpill=level.name()
                    data-fat=at
                    data-custom=""
                    on:mousedown=no_shift_select on:click:capture=pick(pop, fat, inherited)
                >
                    <span class="p-cat">"custom"</span>
                    <button class="p-vals" data-fopen=level.name() title=text.clone() on:click=open>{text.clone()}</button>
                    {mark}
                    {more}
                    {x}
                </span>
            }
            .into_any()
        }
    }
}

/// FILTER-31: a group's box — its join's words, its members joined by the
/// joiner (a button: a click flips any of ⇄ all of, or says why it can't),
/// `+ or` (the menu opens adding to it), its `⋯`. A member is drawn as its
/// pill would be, and edited where it is: its operator, its values (the
/// menu, on its category, adding to this group), its ×.
#[allow(clippy::too_many_arguments)]
fn group_view(lens: Lens, bar: Level, level: Level, inherited: bool, at: usize, conj: Conj, members: Vec<Member>, more: Option<impl IntoView + 'static>) -> AnyView {
    let pop = lens.pop(level);
    let base = format!("[data-fbar=\"{}\"] [data-fpill=\"{}\"][data-fat=\"{at}\"]", bar.name(), level.name());
    let words = members.iter().map(Member::words).collect::<Vec<_>>().join(&format!(" {} ", anyof::joiner(conj)));
    let flip_why = untrack(|| lens.kept(level)).and_then(|k| k.filter.groups.get(at).and_then(anyof::flip_refusal));
    let mark = inherited.then(|| view! { <span class="p-lvl" title=format!("set at the {} level", level.name())>{level.name()}</span> });
    let picked = move || pop.picked.with(|p| p.contains(&at));
    let other = if conj == Conj::Any { "all of" } else { "any of" };
    let join_title = flip_why.clone().unwrap_or_else(|| format!("{} — click for {other}", anyof::conj_words(conj)));
    let parts = members
        .into_iter()
        .enumerate()
        .map(|(i, m)| {
            let join = (i > 0).then(|| {
                if inherited {
                    view! { <span class="g-join">{anyof::joiner(conj)}</span> }.into_any()
                } else {
                    view! {
                        <button
                            class="g-join"
                            class:stuck=flip_why.is_some()
                            title=join_title.clone()
                            aria-label=format!("{} — flip to {other}", anyof::conj_words(conj))
                            on:click=move |_| match lens.edit_groups(level, |gs| anyof::flip(gs, at)) {
                                Ok(()) => pop.refusal.set(None),
                                Err(why) => pop.refusal.set(Some(why)),
                            }
                        >
                            {anyof::joiner(conj)}
                        </button>
                    }
                    .into_any()
                }
            });
            view! { {join} {member_view(lens, bar, level, inherited, at, i, m, &base)} }
        })
        .collect_view();
    let add_sel = format!("{base} .g-add");
    let add = (!inherited).then(|| {
        view! {
            <button
                class="g-add"
                data-fopen=level.name()
                title=format!("add an alternative to this {} group", anyof::conj_words(conj))
                on:click=move |_| pop.show_into(lens, at, add_sel.clone(), None, false)
            >
                {format!("+ {}", anyof::joiner(conj))}
            </button>
        }
    });
    let x = (!inherited).then(|| {
        view! {
            <button class="p-x" title="remove this group" aria-label=format!("remove the group {words}") on:click=move |_| lens.remove_custom(level, at)>
                "×"
            </button>
        }
    });
    view! {
        <span
            class="fgrp"
            class:inherited=inherited
            class:picked=picked
            role="group"
            aria-label=format!("{}: {words}", anyof::conj_words(conj))
            data-fpill=level.name()
            data-fat=at
            data-conj=if conj == Conj::Any { "any" } else { "all" }
            title=words.clone()
            on:mousedown=no_shift_select on:click:capture=pick(pop, Some(at), inherited)
        >
            <span class="g-l">{anyof::conj_words(conj)}</span>
            {parts}
            {add}
            {mark}
            {more}
            {x}
        </span>
    }
    .into_any()
}

/// FILTER-31: member `i` of the group at `at`, as its own pill would be.
#[allow(clippy::too_many_arguments)]
fn member_view(lens: Lens, bar: Level, level: Level, inherited: bool, at: usize, i: usize, m: Member, base: &str) -> AnyView {
    let pop = lens.pop(level);
    let answer = move |r: Result<(), String>| match r {
        Ok(()) => pop.refusal.set(None),
        Err(why) => pop.refusal.set(Some(why)),
    };
    let x = move |label: String| {
        (!inherited).then(|| {
            view! {
                <button
                    class="p-x"
                    title="take this alternative out"
                    aria-label=format!("take {label} out of the group")
                    on:click=move |_| answer(lens.edit_groups(level, |gs| { anyof::remove_member(gs, at, i); Ok(()) }))
                >
                    "×"
                </button>
            }
        })
    };
    let words = m.words();
    match m {
        Member::Clause { cat: c, clause: cl } => {
            let (first, more_n) = pill_values(c, &cl.values);
            let all = cl.values.iter().map(|v| c.value_label(v)).collect::<Vec<_>>().join(", ");
            let op = op_label(c, cl.op, cl.values.len());
            let sel = format!("{base} [data-member=\"{i}\"] .p-vals");
            let op_view = if inherited {
                view! { <span class="p-op">{op}</span> }.into_any()
            } else {
                view! {
                    <button class="p-op" title="toggle the operator" on:click=move |_| answer(lens.edit_groups(level, |gs| anyof::flip_member(gs, at, i)))>
                        {op}
                    </button>
                }
                .into_any()
            };
            let open = move |_| pop.show_into(lens, at, sel.clone(), Some(c), !inherited);
            view! {
                <span class="pill gm" data-member=i data-cat=c.key() data-op=cl.op.word()>
                    <span class="p-cat">{c.label()}</span>
                    {op_view}
                    <button class="p-vals" data-fopen=level.name() title=all on:click=open>
                        {first}
                        {more_n.map(|m| view! { <span class="more">" "{m}</span> })}
                    </button>
                    {x(words)}
                </span>
            }
            .into_any()
        }
        Member::Time(t) => time_member_view(lens, bar, level, inherited, Slot { at, member: Some(i) }, t, x(words)),
        Member::Custom(g) => {
            let text = g.format();
            view! {
                <span class="pill custom gm" data-member=i data-custom="">
                    <span class="p-vals" title=text.clone()>{text.clone()}</span>
                    {x(words)}
                </span>
            }
            .into_any()
        }
    }
}

/// FILTER-27: a time pill — `When`, then its segments: the event's words,
/// and each part (status, window, length, who) a button reopening the When
/// pane on that part; × removes it. Inherited (a global pill on a
/// surface's bar), a part opens the global menu there.
fn time_pill_view(lens: Lens, bar: Level, level: Level, inherited: bool, slot: Slot, t: crate::when::Timed, more: Option<impl IntoView + 'static>) -> AnyView {
    let pop = lens.pop(level);
    let at = slot.at;
    let words = crate::when::words(&t, crate::calendar::reader_h12());
    let mark = inherited.then(|| view! { <span class="p-lvl" title=format!("set at the {} level", level.name())>{level.name()}</span> });
    let base = format!("[data-fbar=\"{}\"] [data-fpill=\"{}\"][data-ftime=\"{at}\"]", bar.name(), level.name());
    let segs = time_segments(lens, level, inherited, slot, &t, &base);
    let x = (!inherited).then(|| {
        view! {
            <button class="p-x" title="remove this filter" aria-label=format!("remove the filter {words}") on:click=move |_| lens.remove_custom(level, at)>
                "×"
            </button>
        }
    });
    let picked = move || pop.picked.with(|p| p.contains(&at));
    view! {
        <span
            class="pill when"
            class:inherited=inherited
            class:picked=picked
            data-fpill=level.name()
            data-fat=at
            data-cat=time_cat(&t).key()
            data-ftime=at
            title=words.clone()
            on:mousedown=no_shift_select on:click:capture=pick(pop, Some(at), inherited)
        >
            <span class="p-cat">{time_cat(&t).label()}</span>
            {segs}
            {mark}
            {more}
            {x}
        </span>
    }
    .into_any()
}

/// FILTER-31: a time clause inside a group, at `slot` — its segments as a
/// time pill's, edited where it is.
fn time_member_view(lens: Lens, bar: Level, level: Level, inherited: bool, slot: Slot, t: crate::when::Timed, x: Option<impl IntoView + 'static>) -> AnyView {
    let words = crate::when::words(&t, crate::calendar::reader_h12());
    let base = format!("[data-fbar=\"{}\"] [data-fpill=\"{}\"][data-fat=\"{}\"] [data-member=\"{}\"]", bar.name(), level.name(), slot.at, slot.member.unwrap_or(0));
    let segs = time_segments(lens, level, inherited, slot, &t, &base);
    view! {
        <span class="pill when gm" data-member=slot.member data-cat=time_cat(&t).key() title=words>
            {segs}
            {x}
        </span>
    }
    .into_any()
}

/// A time clause's segments: the event's words, and each part a button
/// reopening the When pane on that part (FILTER-31: inside a group, the
/// menu adds to that group).
fn time_segments(lens: Lens, level: Level, inherited: bool, slot: Slot, t: &crate::when::Timed, base: &str) -> impl IntoView {
    let pop = lens.pop(level);
    let cat = time_cat(t);
    let h12 = crate::calendar::reader_h12();
    crate::when::segments(t, h12)
        .into_iter()
        .map(|(part, w)| match part {
            None => view! { <span class="p-w">{w}</span> }.into_any(),
            Some(part) => {
                let sel = format!("{base} [data-part=\"{}\"]", part.key());
                let open = move |_| {
                    pop.close(false);
                    lens.open_menu(level, Some(cat), Some((sel.clone(), !inherited)));
                    if slot.member.is_some() {
                        pop.into.set(Some(slot.at));
                    }
                    pop.when.set(crate::when_view::Step::Edit { at: slot, only: Some(part) });
                };
                view! {
                    <button class="p-vals p-part" data-fopen=level.name() data-part=part.key() title=format!("change the {}", part.key()) on:click=open>
                        {w}
                    </button>
                }
                .into_any()
            }
        })
        .collect_view()
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
                <button class="reset" title=title on:click=move |_| lens.reset(level)>
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

/// The text mode: a toggle; on, the level's filter as canonical text,
/// editable — Enter applies it; a parse error lists every bad chunk under
/// the box and applies nothing; Escape (or the toggle) goes back to the
/// pills.
#[component]
pub(crate) fn TextMode(level: Level, texting: RwSignal<bool>) -> impl IntoView {
    let lens = lens::use_lens();
    let draft = RwSignal::new(String::new());
    let errors = RwSignal::new(Vec::<(String, String)>::new());
    let field = NodeRef::<leptos::html::Input>::new();
    focus_each_load(field);
    let toggle = move |_| {
        if texting.get_untracked() {
            texting.set(false);
        } else {
            draft.set(lens.text(level));
            errors.set(Vec::new());
            texting.set(true);
        }
    };
    let apply = move || match lens.apply_text(level, &draft.get_untracked()) {
        Ok(()) => {
            errors.set(Vec::new());
            texting.set(false);
        }
        Err(bad) => errors.set(bad),
    };
    view! {
        <button
            class="ftext-t"
            data-ftext=level.name()
            aria-pressed=move || texting.get().to_string()
            title="the filter as text"
            on:click=toggle
        >
            "text"
        </button>
        {move || texting.get().then(|| view! {
            <span class="ftext">
                <input
                    class="ftext-in"
                    node_ref=field
                    type="text"
                    autocomplete="off"
                    spellcheck="false"
                    aria-label=format!("the {} filter as text", level.name())
                    placeholder="status in (Ready, Coding) and in MNG"
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
                            texting.set(false);
                        }
                        _ => {}
                    }
                />
            </span>
        })}
        {move || {
            let bad = errors.get();
            (texting.get() && !bad.is_empty()).then(|| view! {
                <ul class="ferrs" role="alert">
                    {bad.into_iter().map(|(text, why)| view! {
                        <li><code>{text}</code>" — "{why}</li>
                    }).collect_view()}
                </ul>
            })
        }}
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

/// A level's popover, while it is open: the search field, the categories,
/// and the open category's values beside them (alone, opened direct).
#[component]
pub(crate) fn Popover(level: Level) -> impl IntoView {
    let lens = lens::use_lens();
    let pop = lens.pop(level);
    // Placed, and its field focused, when it opens and whenever its shape
    // changes (a category opened or closed).
    Effect::new(move |_| {
        let Some(anchor) = pop.open.get() else { return };
        pop.cat.track();
        request_animation_frame(move || place(level, &anchor.sel));
        focus_field(level);
    });
    let is_open = Memo::new(move |_| pop.open.with(Option::is_some));
    move || is_open.get().then(|| popover(lens, pop))
}

fn popover(lens: Lens, pop: Pop) -> impl IntoView {
    let level = pop.level;
    let direct = move || pop.open.with(|a| a.as_ref().is_some_and(|a| a.direct));
    let main_rows = Memo::new(move |_| {
        let q = pop.query.get();
        let open = pop.cat.get();
        pop.facts(lens, |k| if open.is_some() { search("", k) } else { search(&q, k) })
    });
    let sub_rows = Memo::new(move |_| {
        let q = pop.query.get();
        pop.cat.get().map(|c| pop.facts(lens, |k| offered_matching(c, k, &q))).unwrap_or_default()
    });
    let placeholder = move || match (pop.cat.get(), level) {
        (Some(c), _) => format!("{}…", c.label()),
        (None, Level::Global) => "Filter everything…".to_string(),
        (None, _) => "Filter…".to_string(),
    };
    // The search takes focus the moment it is drawn (as the palette's does).
    let search_ref = NodeRef::<leptos::html::Input>::new();
    focus_each_load(search_ref);
    // Opened on Title's pill, the popover is Title's field alone.
    let no_search = Memo::new(move |_| direct() && pop.cat.get() == Some(Cat::Title));
    let label = if level == Level::Global { "Filter every view".to_string() } else { format!("Filter the {}", level.name()) };

    view! {
        <div
            id=pop_id(level)
            class="fmenu"
            data-level=level.name()
            role="dialog"
            aria-label=label
            class:direct=direct
            class:has-sub=move || pop.cat.with(Option::is_some)
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                // Escape from anywhere inside (the title's field, a row).
                if bindings::is(&ev, Context::Menu, "dialog.close") {
                    ev.prevent_default();
                    ev.stop_propagation();
                    pop.close(true);
                }
            }
        >
            {(level == Level::Global).then(|| view! { <div class="flevel">"global — every view"</div> })}
            {(level == Level::Nav).then(|| view! { <crate::promote::MakeGlobal menu=true /> })}
            {move || pop.into.get().and_then(|at| lens.kept(level).and_then(|k| k.filter.groups.get(at).map(anyof::into_words))).map(|w| view! {
                <div class="finto" role="note">{w}</div>
            })}
            {move || pop.refusal.get().map(|w| view! { <div class="frefused" role="alert">{w}</div> })}
            {move || (!no_search.get()).then(|| view! {
                <input
                    class="fsearch"
                    node_ref=search_ref
                    type="text"
                    autocomplete="off"
                    spellcheck="false"
                    aria-label="filter the categories and their values"
                    placeholder=placeholder
                    prop:value=move || pop.query.get()
                    on:input=move |ev| {
                        pop.query.set(event_target_value(&ev));
                        pop.hi.set(0);
                    }
                    on:keydown=move |ev| pop.key(lens, ev)
                />
            })}
            <div class="panes">
                <div class="fpane fmain" role="menu" aria-label="categories">
                    {move || list_view(lens, pop, main_rows.get(), move || pop.cat.get().is_none())}
                </div>
                {move || pop.cat.get().map(|c| view! {
                    <div class="fpane fsub" role=if matches!(c, Cat::When | Cat::Deployed) { "group" } else { "menu" } aria-label=format!("{} values", c.label()) data-cat=c.key()>
                        <div class="frow back" on:mousedown=|ev| ev.prevent_default() on:click=move |_| pop.back()>
                            <span class="glyph">"‹"</span>
                            <span class="flabel">{c.menu_label()}</span>
                        </div>
                        {switch_view(lens, pop, c)}
                        {if c == Cat::Title {
                            title_view(lens, pop).into_any()
                        } else if matches!(c, Cat::When | Cat::Deployed) {
                            crate::when_view::pane(lens, pop).into_any()
                        } else {
                            view! { {move || list_view(lens, pop, sub_rows.get(), move || true)} }.into_any()
                        }}
                    </div>
                })}
            </div>
        </div>
    }
}

/// FILTER-19: the operator switch heading `c`'s values — one button, its
/// two words as segments, the operator's lit: `[ is | is not ]` (Title's
/// `[ contains | does not contain ]`); none for a flag. Tab from the search
/// reaches it, Space or Enter flips it (a button's own keys), and a click
/// flips it leaving focus in the search; Tab on from it closes the menu as
/// the search's does — but into Title's field, which follows it.
fn switch_view(lens: Lens, pop: Pop, c: Cat) -> Option<impl IntoView> {
    let (is, not) = switch_words(c)?;
    let op = move || pop.op(lens, c);
    Some(view! {
        <div class="fop-row">
            <button
                type="button"
                class="fop"
                role="switch"
                data-cat=c.key()
                data-op=move || op().word()
                aria-checked=move || (op() == Op::Not).to_string()
                aria-label=format!("{} {not}", c.label())
                title=format!("{is} / {not} — Space or Enter flips it")
                on:mousedown=|ev| ev.prevent_default()
                on:click=move |_| pop.flip_switch(lens, c)
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if bindings::is(&ev, Context::Menu, "menu.leave") && c != Cat::Title {
                        pop.close(false);
                    }
                }
            >
                <span class="fop-w" class:on=move || op() == Op::Is>{is}</span>
                <span class="fop-w" class:on=move || op() == Op::Not>{not}</span>
            </button>
        </div>
    })
}

/// Title's pane: its text, matched as you type — under the switch's
/// operator (FILTER-19).
fn title_view(lens: Lens, pop: Pop) -> impl IntoView {
    let level = pop.level;
    let seed = untrack(|| lens.clauses(level).get(Cat::Title).map(|cl| cl.values[0].clone()).unwrap_or_default());
    let op = move || pop.op(lens, Cat::Title);
    let field = NodeRef::<leptos::html::Input>::new();
    field.on_load(|el: web_sys::HtmlInputElement| {
        let _ = el.focus();
    });
    view! {
        <div class="ftitle">
            <label class="fhint">{move || if op() == Op::Is { "title or id contains" } else { "title or id does not contain" }}</label>
            <input
                class="ftitle-in"
                node_ref=field
                type="text"
                autocomplete="off"
                spellcheck="false"
                value=seed
                on:input=move |ev| {
                    let text = event_target_value(&ev);
                    let op = pop.pane_op(lens, Cat::Title);
                    lens.edit(level, |k| k.set(Cat::Title, op, [text].into_iter().filter(|t| !t.trim().is_empty())));
                }
                on:keydown=move |ev: web_sys::KeyboardEvent| match bindings::hit(&ev, Context::Field) {
                    Some("form.submit") => {
                        ev.prevent_default();
                        pop.close(true);
                    }
                    _ if event_target_value(&ev).is_empty() && !pop.direct() && bindings::is(&ev, Context::Menu, "menu.out") => {
                        ev.prevent_default();
                        pop.back();
                        focus_field(level);
                    }
                    _ => {}
                }
            />
            <span class="fhint">"case-insensitive · Enter to apply"</span>
        </div>
    }
}

/// A list of rows; `live`: whether it is the list the keys move over (the
/// highlight shows there only).
fn list_view(lens: Lens, pop: Pop, rows: Vec<Row>, live: impl Fn() -> bool + Copy + Send + Sync + 'static) -> impl IntoView {
    if rows.is_empty() {
        return view! { <div class="fempty">"no match"</div> }.into_any();
    }
    let mut i = 0usize;
    rows.into_iter()
        .map(|row| {
            let at = row.actionable().then(|| {
                i += 1;
                i - 1
            });
            row_view(lens, pop, row, at, live)
        })
        .collect_view()
        .into_any()
}

fn tri_box(state: Memo<Tri>) -> impl IntoView {
    view! {
        <span class="box" data-state=move || state.get().as_str() aria-hidden="true">
            {move || match state.get() {
                Tri::On => "✓",
                Tri::Mixed => "–",
                Tri::Off => "",
            }}
        </span>
    }
}

fn row_view(lens: Lens, pop: Pop, row: Row, at: Option<usize>, live: impl Fn() -> bool + Copy + Send + Sync + 'static) -> impl IntoView {
    let level = pop.level;
    let hi = move || live() && at == Some(pop.hi.get());
    let hover = move |_| {
        if let (true, Some(i)) = (live(), at) {
            pop.hi.set(i);
        }
    };
    match row {
        Row::Heading(h) => view! { <div class="fhead" class:fdiv=h == "other boards">{h}</div> }.into_any(),
        Row::Group { cat: c, label, tokens } => {
            let state = Memo::new(move |_| group_tri(pop.clauses(lens).get(c), tokens));
            view! {
                <div
                    class="frow fgroup"
                    class:hi=hi
                    role="menuitemcheckbox"
                    aria-checked=move || state.get().aria()
                    data-cat=c.key()
                    data-group=label
                    on:mousedown=|ev| ev.prevent_default()
                    on:mouseenter=hover
                    on:click=move |_| pop.act(lens, &Row::Group { cat: c, label, tokens })
                >
                    {tri_box(state)}
                    <span class="flabel fhead-l">{label}</span>
                </div>
            }
            .into_any()
        }
        Row::Cat(c) => {
            let applied = move || match (c, pop.into.get()) {
                (Cat::When | Cat::Deployed, None) => lens.pills(level).iter().any(|p| matches!(p, Pill::Time { t, .. } if time_cat(t) == c)),
                (Cat::When | Cat::Deployed, Some(at)) => lens
                    .kept(level)
                    .and_then(|k| k.filter.groups.get(at).map(|g| anyof::members(g).iter().any(|m| matches!(m, Member::Time(t) if time_cat(t) == c))))
                    .unwrap_or(false),
                _ => pop.clauses(lens).get(c).is_some(),
            };
            // FILTER-31: a category used inside a group says so.
            let grouped = move || pop.into.get().is_none() && lens.in_group(level, c);
            view! {
                <div
                    class="frow cat"
                    class:hi=hi
                    class:open=move || pop.cat.get() == Some(c)
                    role="menuitem"
                    aria-haspopup="menu"
                    data-cat=c.key()
                    on:mousedown=|ev| ev.prevent_default()
                    on:mouseenter=hover
                    on:click=move |_| {
                        pop.open_cat(c);
                        focus_field(level);
                    }
                >
                    <span class="glyph" aria-hidden="true">{c.glyph()}</span>
                    <span class="flabel">{c.menu_label()}</span>
                    {move || applied().then(|| view! { <span class="dot" title="applied"></span> })}
                    {move || grouped().then(|| view! { <span class="ingrp" title="used inside a group of alternatives">"∨"</span> })}
                    <span class="arrow" aria-hidden="true">"▸"</span>
                </div>
            }
            .into_any()
        }
        Row::TitleContains(text) => {
            let row = Row::TitleContains(text.clone());
            view! {
                <div
                    class="frow"
                    class:hi=hi
                    role="menuitem"
                    data-cat="q"
                    on:mousedown=|ev| ev.prevent_default()
                    on:mouseenter=hover
                    on:click=move |_| pop.act(lens, &row)
                >
                    <span class="glyph" aria-hidden="true">"T"</span>
                    <span class="flabel"><span class="fscope">"Title contains "</span>{Cat::Title.value_label(&text)}</span>
                </div>
            }
            .into_any()
        }
        Row::Value { cat: c, token, depth, scoped } => {
            let state = {
                let token = token.clone();
                Memo::new(move |_| {
                    let cl = pop.clauses(lens).get(c).cloned();
                    if c == Cat::Shortcode {
                        let checked: BTreeSet<String> = cl.map(|cl| cl.values.into_iter().collect()).unwrap_or_default();
                        lens.sctree().with(|t| t.as_ref().map_or(if checked.contains(&token) { Tri::On } else { Tri::Off }, |t| t.tri(&token, &checked)))
                    } else if cl.is_some_and(|cl| cl.values.iter().any(|v| *v == token)) {
                        Tri::On
                    } else {
                        Tri::Off
                    }
                })
            };
            let count = {
                let token = token.clone();
                move || pop.counts.get().count(c, &token).map(|n| n.to_string()).unwrap_or_default()
            };
            let click = {
                let token = token.clone();
                move |_| pop.check(lens, c, &token, scoped)
            };
            let only = {
                let token = token.clone();
                move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    let op = pop.pane_op(lens, Cat::Shortcode);
                    pop.edit(lens, |into| lens.only(level, into, &token, op));
                }
            };
            let pad = format!("padding-left: {}px", 8 + depth * 14);
            view! {
                <div
                    class="frow val"
                    class:hi=hi
                    role="menuitemcheckbox"
                    aria-checked=move || state.get().aria()
                    data-cat=c.key()
                    data-value=token.clone()
                    style=pad
                    on:mousedown=|ev| ev.prevent_default()
                    on:mouseenter=hover
                    on:click=click
                >
                    {tri_box(state)}
                    {scoped.then(|| view! { <span class="fscope">{c.label()}" ›"</span> })}
                    <span class="flabel">{value_face(c, &token)}</span>
                    {(c == Cat::Shortcode && !scoped).then(|| view! {
                        <button class="only" title="this shortcode's tree only" on:click=only>"only"</button>
                    })}
                    <span class="fcount">{count}</span>
                </div>
            }
            .into_any()
        }
    }
}

/// A value as the menu draws it: a shortcode as its chip, a status as its
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
                root: Some(format!("{epic}-1")),
                root_row: None,
                host: host.map(Into::into),
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

    /// A Status group's header checks its statuses — or, all checked,
    /// unchecks them; its box is tri-state.
    #[test]
    fn a_status_group_checks_its_statuses() {
        let mut k = Clauses::default().with(Cat::Status, Op::Is, strs(&["Pending"]));
        let not_started: &[&str] = STATUS_GROUPS[2].1;
        assert_eq!(group_tri(k.get(Cat::Status), not_started), Tri::Mixed);
        k.toggle_all(Cat::Status, not_started);
        assert_eq!(k.get(Cat::Status).unwrap().values, strs(&["Pending", "Blocked", "Idea", "Epic"]));
        assert_eq!(group_tri(k.get(Cat::Status), not_started), Tri::On);
        k.toggle_all(Cat::Status, not_started);
        assert!(k.get(Cat::Status).is_none());
        assert_eq!(group_tri(None, STATUS_GROUPS[4].1), Tri::Off);
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
        let k = MenuFacts { cats: &Cat::WAVES, route: Some("PLEXUS"), tree: Some(&t), clauses: &clauses, counts: &counts };
        assert_eq!(search("", &k), Cat::WAVES.map(Row::Cat).to_vec());
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

        let found = search("ret", &k);
        assert!(found.contains(&Row::Value { cat: Cat::Shortcode, token: "RETURNS".into(), depth: 0, scoped: true }), "{found:?}");
        assert_eq!(found.last(), Some(&Row::TitleContains("ret".into())));
        let found = search("sta", &k);
        assert_eq!(found[0], Row::Cat(Cat::Status), "a category by its name first");
        assert!(search("review", &k).contains(&Row::Value { cat: Cat::Status, token: "In Review".into(), depth: 0, scoped: true }));
        assert_eq!(offered_matching(Cat::Shortcode, &k, "gen"), [val(Cat::Shortcode, "CODEGEN"), val(Cat::Shortcode, "GEN")]);
        assert_eq!(offered_matching(Cat::Readiness, &k, "now"), [val(Cat::Readiness, "workable")]);
        assert_eq!(offered_matching(Cat::Commits, &k, "has"), [val(Cat::Commits, "yes")]);
        assert!(offered(Cat::Title, &k).is_empty());

        // The global menu: the server's categories, no Title to offer.
        let g = MenuFacts { cats: &Cat::SERVER, route: None, tree: Some(&t), clauses: &Clauses::default(), counts: &HashMap::new() };
        assert_eq!(search("", &g).len(), 12, "FILTER-27: When among them; DEPLOY-13: Deployed");
        assert!(offered(Cat::When, &g).is_empty(), "When draws its own pane");
        assert!(offered(Cat::Deployed, &g).is_empty(), "DEPLOY-13: and Deployed, When's pane");
        assert_eq!(search("deploy", &g)[0], Row::Cat(Cat::Deployed));
        assert_eq!(search("prod", &g)[0], Row::Cat(Cat::Deployed), "found by what it builds");
        assert_eq!(switch_words(Cat::Deployed), None);
        let dep = crate::when::new_deployed("prod", false, None).unwrap();
        assert_eq!(time_cat(&dep), Cat::Deployed);
        assert_eq!(time_cat(&crate::when::new_clause(crate::when::Kind::Changed, None, None).unwrap()), Cat::When);
        assert_eq!(search("created", &g)[0], Row::Cat(Cat::When), "FILTER-27: found by what it builds");
        assert_eq!(search("moved", &g)[0], Row::Cat(Cat::When));
        // FILTER-21: typing finds Menger docs, by its name.
        assert_eq!(search("menger", &g)[0], Row::Cat(Cat::MengerDocs));
        assert_eq!(offered(Cat::MengerDocs, &g), [val(Cat::MengerDocs, "yes"), val(Cat::MengerDocs, "no")]);
        assert!(!search("ret", &g).iter().any(|r| matches!(r, Row::TitleContains(_))));
        assert!(!search("wave", &g).iter().any(|r| matches!(r, Row::Cat(Cat::Wave))));
        // MENGER-12: Asking is a flag; Label offers `needs-you`, every label
        // the counts name and the clause's own, in name order.
        assert_eq!(search("asking", &g)[0], Row::Cat(Cat::Asking));
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
        let order = display_order(Cat::Shortcode, sc, Some("PLEXUS"), Some(&t));
        assert_eq!(order[0], "PLEXUS");
        assert_eq!(pill_values(Cat::Shortcode, &order), ("PLEXUS".into(), Some("+9".into())));
        assert_eq!(op_label(Cat::Shortcode, Op::Is, 10), "is any of");
        assert_eq!(op_label(Cat::Status, Op::Is, 1), "is");
        assert_eq!(op_label(Cat::Status, Op::Not, 2), "is not");
        assert_eq!(op_label(Cat::Title, Op::Is, 1), "contains");
        assert_eq!(op_label(Cat::Title, Op::Not, 1), "does not contain");
        assert_eq!(pill_values(Cat::Wave, &strs(&["3"])), ("wave 3".into(), None));
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

        let not = Clause { op: Op::Not, values: strs(&["doc"]) };
        let is = Clause { op: Op::Is, values: strs(&["doc"]) };
        assert_eq!(switch_op(None, Op::Is), Op::Is, "a new clause: is");
        assert_eq!(switch_op(None, Op::Not), Op::Not, "set before any value is checked");
        assert_eq!(switch_op(Some(&not), Op::Is), Op::Not, "the clause's operator");
        assert_eq!(switch_op(Some(&is), Op::Not), Op::Is);
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
        assert_eq!(pill_words(&Pill::Clause { cat: Cat::Status, clause: cl }), ("Status".into(), "is not Coding, Ready".into()));
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
        assert_eq!(pill_words(&Pill::Clause { cat: Cat::Kind, clause: kind }), ("Kind".into(), "is not spike, doc".into()));

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

        // A Status group's header, a shortcode's subtree, a surface's own
        // category: under the switch.
        f.edit_as(Cat::Status, Op::Not, |k| k.toggle_all(Cat::Status, STATUS_GROUPS[3].1));
        assert_eq!(f.get(Cat::Status), Some(&Clause { op: Op::Not, values: strs(&["Complete", "Superseded", "Canceled"]) }));
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
}
