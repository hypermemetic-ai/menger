//! WAVE-12: the wave board's filter — a menu in Linear's idiom that builds
//! any board, over the board a route opens on.
//!
//! - **The default** (WAVE-1 decision 16): `#/waves/<SC>` with no filter in
//!   its URL is Shortcode is any of the tree of `<SC>` — SC and every
//!   shortcode hosted under it, at any depth ([`ScTree`], from `groups`'
//!   `host`s) — and Kind is any of `build, spike, gap`
//!   ([`BoardFilter::default_for`]). The default follows the groups: a
//!   shortcode newly hosted under SC joins its board.
//! - **The filter** is one clause per category ([`Cat`], most important
//!   first: Shortcode · Status · Wave · Kind · Severity · Readiness ·
//!   Off-board blockers · Title), each `is any of` or `is not` some values
//!   ([`Clause`]); Title is a case-insensitive substring of the title or the
//!   id (the palette's rule, [`keys::palette_match`]).
//! - **Where it applies**: Shortcode's set is the `epics` the board sends
//!   `tickets.waves` (`is not` = the route's tree minus the excluded; no
//!   clause = every shortcode), Kind's set the `kinds` (no clause = the five
//!   kinds of work; docs never, WAVE-1 decision 14) — [`BoardFilter::epics`],
//!   [`BoardFilter::kinds`]. Every clause, those two included, is applied to
//!   the answer's cards in the browser ([`narrow`]): the answer is one board
//!   (tens to hundreds of tickets) and the counts come free ([`counts`]).
//!   An off-board blocker is one the answer holds no card for — what the
//!   cards' chips name.
//! - **The URL holds the board** ([`BoardFilter::to_query`],
//!   [`BoardFilter::parse_query`]): `#/waves/PLEXUS?sc=is:RETURNS,CONTEXT&
//!   status=not:Complete,Superseded&q=envelope` — one `key=op:values` per
//!   pill, values comma-separated and percent-encoded, keys `sc, status,
//!   wave, kind, sev, ready, off, q`; Title's is its text (`q=not:…` for
//!   "does not contain"). No query is the default; an empty filter is
//!   `?all`; a malformed query is the default. The board rewrites the query
//!   with `replaceState` on every change, so a reload or a pasted URL
//!   restores it exactly.
//! - **The menu** ([`filter_bar`]): a `Filter` button at the bar's left (and
//!   `f` while the board is open, unless typing) opens a popover — a search
//!   field over the categories and, typed into, over every category's values
//!   ([`search`]); a category opens its values beside it (in place, with a ‹
//!   back row, on a narrow window): a checkbox, the label, the count of the
//!   answer's cards that have it. Shortcode's values are the hosting tree
//!   (the route's first, then "other boards"), tri-state ([`Tri`]). The
//!   applied clauses are pills — `[Category] [is any of | is not] [values
//!   +n] [×]`; the operator toggles, the values reopen that category, `×`
//!   removes it — and `reset` appears when the filter is not the default.
//!   The popover is placed under its anchor and kept inside the viewport
//!   with the ticket menu's [`menu::clamp`]; it closes as the ticket menu
//!   does — Escape, a mousedown outside it.
//!
//! Everything that decides — the query both ways, the default, the tree and
//! its tri-state, what a card's value is in each category, the narrowing,
//! the counts, the menu's rows — is a pure function here, pinned by native
//! tests.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use leptos::prelude::*;
use menger_tickets_api::{Group, Placement, Severity, Status};
use wasm_bindgen::JsCast;

use crate::keys::palette_match;
use crate::nav::kind_glyph;
use crate::waves::{epic_of, off_board, retally, Answer, Card};
use crate::{dom, keys, menu, st_class};

// ---------------------------------------------------------------------
// The categories

/// A filter's categories, most important first — the menu's order, the
/// pills' order, the query's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Cat {
    Shortcode,
    Status,
    Wave,
    Kind,
    Severity,
    Readiness,
    OffBoard,
    Title,
}

/// The kinds of work, in the menu's order — the default three first. Docs
/// are never cards (WAVE-1 decision 14).
pub(crate) const WORK_KINDS: [&str; 5] = ["build", "spike", "gap", "scope", "execution"];

/// The kinds a board opens on (WAVE-1 decision 14; `waves`' own default).
pub(crate) const DEFAULT_KINDS: [&str; 3] = ["build", "spike", "gap"];

/// Status's values, grouped, the work under way first.
pub(crate) const STATUS_GROUPS: [(&str, &[&str]); 4] = [
    ("in progress", &["Coding", "In Review"]),
    ("ready", &["Ready"]),
    ("not started", &["Pending", "Blocked", "Idea", "Epic"]),
    ("done", &["Complete", "Superseded"]),
];

const STATUSES: [&str; 9] = ["Coding", "In Review", "Ready", "Pending", "Blocked", "Idea", "Epic", "Complete", "Superseded"];

/// The query an empty filter is written as (no query at all is the default).
pub(crate) const ALL: &str = "all";

impl Cat {
    pub(crate) const ALL: [Cat; 8] =
        [Cat::Shortcode, Cat::Status, Cat::Wave, Cat::Kind, Cat::Severity, Cat::Readiness, Cat::OffBoard, Cat::Title];

    /// The category's key in the URL's query.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Cat::Shortcode => "sc",
            Cat::Status => "status",
            Cat::Wave => "wave",
            Cat::Kind => "kind",
            Cat::Severity => "sev",
            Cat::Readiness => "ready",
            Cat::OffBoard => "off",
            Cat::Title => "q",
        }
    }

    fn of_key(key: &str) -> Option<Cat> {
        Cat::ALL.into_iter().find(|c| c.key() == key)
    }

    /// The category's name on its pill.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Cat::Shortcode => "Shortcode",
            Cat::Status => "Status",
            Cat::Wave => "Wave",
            Cat::Kind => "Kind",
            Cat::Severity => "Severity",
            Cat::Readiness => "Readiness",
            Cat::OffBoard => "Off-board blockers",
            Cat::Title => "Title",
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
            Cat::Wave => "≋",
            Cat::Kind => "◇",
            Cat::Severity => "!",
            Cat::Readiness => "▸",
            Cat::OffBoard => "⧖",
            Cat::Title => "T",
        }
    }

    /// The values this category always offers, in the menu's order —
    /// Shortcode's and Wave's come from the data, Title's is text.
    pub(crate) fn fixed(self) -> &'static [&'static str] {
        match self {
            Cat::Status => &STATUSES,
            Cat::Kind => &WORK_KINDS,
            Cat::Severity => &["Critical", "High", "Medium", "Low", "none"],
            Cat::Readiness => &["workable", "waiting", "missing"],
            Cat::OffBoard => &["some", "none"],
            Cat::Shortcode | Cat::Wave | Cat::Title => &[],
        }
    }

    /// `raw` (as a URL or a click names it) as this category's canonical
    /// value, or `None` when it is none of its values.
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
            Cat::Status => raw.parse::<Status>().ok().filter(|s| *s != Status::Doc).map(|s| s.as_str().to_string()),
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
            Cat::Shortcode | Cat::Title => (0, t.to_string()),
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
            (Cat::Readiness, "workable") => "workable now".into(),
            (Cat::OffBoard, "some") => "has some".into(),
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
    fn word(self) -> &'static str {
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

/// One category's clause: its operator and its values — canonical, in the
/// category's order, distinct, never none (a clause left with no value is
/// no clause). Title's one value is its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Clause {
    pub(crate) op: Op,
    pub(crate) values: Vec<String>,
}

impl Clause {
    fn new(c: Cat, op: Op, values: impl IntoIterator<Item = String>) -> Option<Self> {
        let mut values: Vec<String> = values.into_iter().collect();
        if c == Cat::Title {
            values.truncate(1);
        } else {
            values.sort_by_key(|v| c.rank(v));
            values.dedup();
        }
        (!values.is_empty()).then_some(Clause { op, values })
    }

    /// Whether a card whose value in this category is `v` passes.
    fn admits(&self, v: Option<&str>) -> bool {
        let has = v.is_some_and(|v| self.values.iter().any(|x| x == v));
        has == (self.op == Op::Is)
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
        for g in groups.values() {
            match g.host.as_deref().and_then(epic_of).filter(|h| *h != g.epic && known.contains(*h)) {
                Some(h) => kids.entry(h.to_string()).or_default().push(g.epic.clone()),
                None => top.push(g.epic.clone()),
            }
        }
        let mut t = ScTree { kids, top, known };
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

    /// The Shortcode menu: the route's tree, then — after a divider — every
    /// other top-level shortcode's tree (what the route's tree already listed
    /// left out), then any `extra` (a clause's value no group knows).
    pub(crate) fn rows<'a>(&self, route: &str, extra: impl IntoIterator<Item = &'a String>) -> Vec<ScRow> {
        let mut listed = HashSet::new();
        let mut out: Vec<ScRow> = Vec::new();
        for (code, depth) in self.walk(route) {
            listed.insert(code.clone());
            out.push(ScRow::Code { code, depth });
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
        if !others.is_empty() {
            out.push(ScRow::Divider);
            out.extend(others);
        }
        out
    }
}

// ---------------------------------------------------------------------
// The filter

/// The board's filter: at most one clause per category.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BoardFilter {
    clauses: BTreeMap<Cat, Clause>,
}

impl BoardFilter {
    /// The board `#/waves/<route>` opens on: Shortcode is any of the
    /// route's tree, Kind is any of build, spike, gap.
    pub(crate) fn default_for(route: &str, tree: &ScTree) -> Self {
        BoardFilter::default()
            .with(Cat::Shortcode, Op::Is, tree.subtree(&route.to_ascii_uppercase()))
            .with(Cat::Kind, Op::Is, DEFAULT_KINDS.iter().map(|k| k.to_string()))
    }

    /// This filter with `c`'s clause set (values as canonical tokens).
    pub(crate) fn with(mut self, c: Cat, op: Op, values: impl IntoIterator<Item = String>) -> Self {
        self.set(c, op, values);
        self
    }

    pub(crate) fn clause(&self, c: Cat) -> Option<&Clause> {
        self.clauses.get(&c)
    }

    /// The clauses, in the categories' order.
    pub(crate) fn clauses(&self) -> impl Iterator<Item = (Cat, &Clause)> {
        self.clauses.iter().map(|(c, cl)| (*c, cl))
    }

    /// `c`'s clause set to `op` and `values`; none left, no clause.
    pub(crate) fn set(&mut self, c: Cat, op: Op, values: impl IntoIterator<Item = String>) {
        match Clause::new(c, op, values) {
            Some(cl) => {
                self.clauses.insert(c, cl);
            }
            None => {
                self.clauses.remove(&c);
            }
        }
    }

    /// `c`'s values replaced, its operator kept (`is` for a new clause).
    pub(crate) fn set_values(&mut self, c: Cat, values: impl IntoIterator<Item = String>) {
        let op = self.clause(c).map_or(Op::Is, |cl| cl.op);
        self.set(c, op, values);
    }

    pub(crate) fn remove(&mut self, c: Cat) {
        self.clauses.remove(&c);
    }

    /// Whether `t` is one of `c`'s values.
    pub(crate) fn has(&self, c: Cat, t: &str) -> bool {
        self.clause(c).is_some_and(|cl| cl.values.iter().any(|v| v == t))
    }

    /// `t` added to `c`'s values, or taken from them.
    pub(crate) fn toggle(&mut self, c: Cat, t: &str) {
        let mut values: Vec<String> = self.clause(c).map(|cl| cl.values.clone()).unwrap_or_default();
        match values.iter().position(|v| v == t) {
            Some(i) => {
                values.remove(i);
            }
            None => values.push(t.to_string()),
        }
        self.set_values(c, values);
    }

    /// `c`'s operator turned over.
    pub(crate) fn flip(&mut self, c: Cat) {
        if let Some(cl) = self.clauses.get_mut(&c) {
            cl.op = cl.op.flip();
        }
    }

    /// The shortcodes `waves` is asked for: Shortcode's values (`is`), the
    /// route's tree without them (`is not`), or every shortcode (no clause).
    pub(crate) fn epics(&self, route: &str, tree: &ScTree) -> BTreeSet<String> {
        match self.clause(Cat::Shortcode) {
            None => tree.known().clone(),
            Some(cl) if cl.op == Op::Is => cl.values.iter().cloned().collect(),
            Some(cl) => tree.subtree(&route.to_ascii_uppercase()).into_iter().filter(|e| !cl.values.contains(e)).collect(),
        }
    }

    /// The kinds `waves` is asked for: Kind's values (`is`), the kinds of
    /// work without them (`is not`), or every kind of work (no clause).
    pub(crate) fn kinds(&self) -> Vec<String> {
        match self.clause(Cat::Kind) {
            Some(cl) if cl.op == Op::Is => cl.values.clone(),
            cl => WORK_KINDS.iter().filter(|k| !cl.is_some_and(|cl| cl.values.iter().any(|v| v == *k))).map(|k| k.to_string()).collect(),
        }
    }

    /// Whether `card` passes every clause; `off`: whether it waits on a
    /// ticket the answer holds no card for.
    pub(crate) fn admits(&self, card: &Card, off: bool) -> bool {
        self.clauses.iter().all(|(c, cl)| match c {
            Cat::Title => palette_match(&card.summary.id, &card.summary.title, &cl.values[0]) == (cl.op == Op::Is),
            c => cl.admits(value_of(*c, card, off).as_deref()),
        })
    }

    /// The filter as the route's query (no `?`): `key=op:v1,v2&…` in the
    /// categories' order; Title as `q=text` or `q=not:text`; an empty
    /// filter as [`ALL`].
    pub(crate) fn to_query(&self) -> String {
        if self.clauses.is_empty() {
            return ALL.into();
        }
        self.clauses
            .iter()
            .map(|(c, cl)| {
                let values = cl.values.iter().map(|v| enc(v)).collect::<Vec<_>>().join(",");
                match (c, cl.op) {
                    (Cat::Title, Op::Is) => format!("q={values}"),
                    (c, op) => format!("{}={}:{values}", c.key(), op.word()),
                }
            })
            .collect::<Vec<_>>()
            .join("&")
    }

    /// The route's query as a filter — `None` for the default: no query, or
    /// one that is malformed (an unknown key, a key twice, no values, a
    /// value that is none of its category's).
    pub(crate) fn parse_query(q: &str) -> Option<Self> {
        let q = q.trim().trim_start_matches('?');
        if q.is_empty() {
            return None;
        }
        if q == ALL {
            return Some(BoardFilter::default());
        }
        let mut f = BoardFilter::default();
        for pair in q.split('&').filter(|p| !p.is_empty()) {
            let (key, v) = pair.split_once('=')?;
            let c = Cat::of_key(key)?;
            if f.clauses.contains_key(&c) {
                return None;
            }
            let (op, rest) = match (v.strip_prefix("is:"), v.strip_prefix("not:")) {
                (Some(r), _) => (Op::Is, r),
                (_, Some(r)) => (Op::Not, r),
                _ => (Op::Is, v),
            };
            let raw: Vec<String> = if c == Cat::Title { vec![dec(rest)?] } else { rest.split(',').map(dec).collect::<Option<_>>()? };
            let tokens: Vec<String> = raw.iter().map(|r| c.token(r)).collect::<Option<_>>()?;
            f.clauses.insert(c, Clause::new(c, op, tokens)?);
        }
        (!f.clauses.is_empty()).then_some(f)
    }
}

/// A value percent-encoded for the query: everything but `A–Z a–z 0–9 - _ . ~`.
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A query value decoded (`%XX`, and `+` as a space); `None` if it is not
/// well formed.
fn dec(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = b.get(i + 1..i + 3)?;
                if !hex.iter().all(u8::is_ascii_hexdigit) {
                    return None;
                }
                out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// The route's query, as the hash carries it (`#/waves/PLEXUS?…`).
pub(crate) fn query_of(hash: &str) -> &str {
    hash.split_once('?').map_or("", |(_, q)| q)
}

/// `hash` with its query replaced by `q` (none when `q` is empty).
pub(crate) fn with_query(hash: &str, q: &str) -> String {
    let path = hash.split_once('?').map_or(hash, |(p, _)| p);
    if q.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{q}")
    }
}

// ---------------------------------------------------------------------
// The cards

/// `card`'s value in category `c` (none for Title, which matches text; and
/// none for a done card's readiness). `off`: whether it waits on a ticket
/// the answer holds no card for.
pub(crate) fn value_of(c: Cat, card: &Card, off: bool) -> Option<String> {
    let s = &card.summary;
    match c {
        Cat::Shortcode => Some(s.epic.clone()),
        Cat::Status => Some(s.status.as_str().into()),
        Cat::Wave => Some(match card.placement {
            Placement::Wave { n } => n.to_string(),
            Placement::MissingBlocker => "missing".into(),
            Placement::Done => "done".into(),
        }),
        Cat::Kind => Some(s.kind.as_str().into()),
        Cat::Severity => Some(s.severity.map_or("none", Severity::as_str).into()),
        Cat::Readiness => match card.placement {
            Placement::Wave { n: 1 } => Some("workable".into()),
            Placement::Wave { .. } => Some("waiting".into()),
            Placement::MissingBlocker => Some("missing".into()),
            Placement::Done => None,
        },
        Cat::OffBoard => Some(if off { "some" } else { "none" }.into()),
        Cat::Title => None,
    }
}

fn ids(a: &Answer) -> HashSet<String> {
    a.cards.iter().map(|c| c.summary.id.clone()).collect()
}

/// The answer narrowed to the cards `f` admits, the tally recounted — what
/// the columns, the tally and the Done count show.
pub(crate) fn narrow(a: &Answer, f: &BoardFilter) -> Answer {
    let on = ids(a);
    let mut out = Answer {
        cards: a.cards.iter().filter(|c| f.admits(c, !off_board(c, &on).is_empty())).cloned().collect(),
        ..Answer::default()
    };
    retally(&mut out);
    out
}

/// Per category, per value: how many of the answer's cards have it.
pub(crate) type Counts = HashMap<Cat, HashMap<String, usize>>;

pub(crate) fn counts(a: &Answer) -> Counts {
    let on = ids(a);
    let mut out: Counts = HashMap::new();
    for card in &a.cards {
        let off = !off_board(card, &on).is_empty();
        for c in Cat::ALL {
            if let Some(v) = value_of(c, card, off) {
                *out.entry(c).or_default().entry(v).or_default() += 1;
            }
        }
    }
    out
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
    /// The search's last offer: Title contains what was typed.
    TitleContains(String),
    /// A heading over a group of values — Status's groups, Shortcode's
    /// "other boards".
    Heading(&'static str),
}

impl Row {
    fn actionable(&self) -> bool {
        !matches!(self, Row::Heading(_))
    }
}

/// What the menu needs to know to list values.
pub(crate) struct Facts<'a> {
    pub(crate) route: &'a str,
    pub(crate) tree: &'a ScTree,
    pub(crate) filter: &'a BoardFilter,
    pub(crate) answer: Option<&'a Answer>,
}

/// The values category `c` offers, in the menu's order: Shortcode's tree
/// (the route's first, then "other boards"); Status's groups; wave 1 … the
/// answer's last (or the clause's, if later), missing blocker, done; the
/// fixed values of the rest. Title offers a text field, no rows.
pub(crate) fn offered(c: Cat, k: &Facts) -> Vec<Row> {
    let value = |token: &str, depth: usize| Row::Value { cat: c, token: token.to_string(), depth, scoped: false };
    match c {
        Cat::Shortcode => {
            let extra: Vec<String> = k.filter.clause(c).map(|cl| cl.values.clone()).unwrap_or_default();
            k.tree
                .rows(k.route, extra.iter())
                .into_iter()
                .map(|r| match r {
                    ScRow::Code { code, depth } => value(&code, depth),
                    ScRow::Divider => Row::Heading("other boards"),
                })
                .collect()
        }
        Cat::Status => STATUS_GROUPS
            .iter()
            .flat_map(|(h, vs)| std::iter::once(Row::Heading(h)).chain(vs.iter().map(|v| value(v, 0))))
            .collect(),
        Cat::Wave => {
            let of_answer = k.answer.map_or(0, |a| {
                a.cards.iter().filter_map(|c| if let Placement::Wave { n } = c.placement { Some(n) } else { None }).max().unwrap_or(0)
            });
            let of_clause = k.filter.clause(c).map_or(0, |cl| cl.values.iter().filter_map(|v| v.parse::<u32>().ok()).max().unwrap_or(0));
            (1..=of_answer.max(of_clause))
                .map(|n| value(&n.to_string(), 0))
                .chain(["missing", "done"].map(|t| value(t, 0)))
                .collect()
        }
        Cat::Title => Vec::new(),
        c => c.fixed().iter().map(|t| value(t, 0)).collect(),
    }
}

/// `c`'s values matching what is typed (all of them, as offered, when
/// nothing is): a match is the label or the value, case-insensitive; the
/// headings go, and the tree flattens, while a query narrows.
pub(crate) fn offered_matching(c: Cat, k: &Facts, q: &str) -> Vec<Row> {
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
/// was typed.
pub(crate) fn search(q: &str, k: &Facts) -> Vec<Row> {
    let q = q.trim();
    if q.is_empty() {
        return Cat::ALL.into_iter().map(Row::Cat).collect();
    }
    let mut out: Vec<Row> = Cat::ALL.into_iter().filter(|c| palette_match(c.menu_label(), "", q)).map(Row::Cat).collect();
    for c in Cat::ALL {
        for r in offered_matching(c, k, q) {
            if let Row::Value { cat, token, .. } = r {
                out.push(Row::Value { cat, token, depth: 0, scoped: true });
            }
        }
    }
    out.push(Row::TitleContains(q.to_string()));
    out
}

/// The order a clause's values show in on its pill: Shortcode's as the
/// menu lists the tree (the route's own first), the rest as stored.
pub(crate) fn display_order(c: Cat, cl: &Clause, route: &str, tree: Option<&ScTree>) -> Vec<String> {
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

/// A pill's values: the first, then `+n` for the rest.
pub(crate) fn pill_values(c: Cat, shown: &[String]) -> (String, Option<String>) {
    let first = shown.first().map(|v| c.value_label(v)).unwrap_or_default();
    (first, (shown.len() > 1).then(|| format!("+{}", shown.len() - 1)))
}

// ---------------------------------------------------------------------
// The view

/// What the bar's filter shares with the board — every handle `Copy`.
#[derive(Clone, Copy)]
pub(crate) struct FilterCtx {
    /// The route's shortcode.
    pub(crate) route: StoredValue<String>,
    pub(crate) tree: Memo<Option<ScTree>>,
    /// What the user built — `None` is the default.
    pub(crate) chosen: RwSignal<Option<BoardFilter>>,
    pub(crate) default: Memo<Option<BoardFilter>>,
    /// What applies: the chosen filter, or the default (`None` until the
    /// groups are in).
    pub(crate) filter: Memo<Option<BoardFilter>>,
    /// The shortcodes asked for.
    pub(crate) epics: Memo<Option<BTreeSet<String>>>,
    pub(crate) answer: RwSignal<Option<Answer>>,
    pub(crate) counts: Memo<Counts>,
}

impl FilterCtx {
    /// The filter changed by `f`; one equal to the default is the default.
    fn update(self, f: impl FnOnce(&mut BoardFilter)) {
        let Some(mut next) = self.filter.get_untracked() else { return };
        f(&mut next);
        let default = self.default.get_untracked();
        self.chosen.set(if default.as_ref() == Some(&next) { None } else { Some(next) });
    }

    /// A click on a value: a shortcode takes its subtree with it.
    fn toggle(self, c: Cat, token: &str) {
        let tree = self.tree.get_untracked();
        self.update(|f| match (c, &tree) {
            (Cat::Shortcode, Some(t)) => {
                let checked: BTreeSet<String> = f.clause(c).map(|cl| cl.values.iter().cloned().collect()).unwrap_or_default();
                f.set_values(c, t.click(token, &checked));
            }
            _ => f.toggle(c, token),
        });
    }

    /// Shortcode is (any of) `code`'s tree only.
    fn only(self, code: &str) {
        let Some(t) = self.tree.get_untracked() else { return };
        self.update(|f| f.set_values(Cat::Shortcode, t.subtree(code)));
    }

    fn facts<T>(self, f: impl FnOnce(&Facts) -> T) -> Option<T> {
        let route = self.route.get_value();
        self.tree.with(|t| {
            self.filter.with(|fl| {
                self.answer.with(|a| match (t, fl) {
                    (Some(tree), Some(filter)) => Some(f(&Facts { route: &route, tree, filter, answer: a.as_ref() })),
                    _ => None,
                })
            })
        })
    }
}

/// Where the popover opened: under the Filter button, or under a pill's
/// values (that category's values alone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    Button,
    Pill(Cat),
}

/// The popover's state — every handle `Copy`.
#[derive(Clone, Copy)]
struct Pop {
    fx: FilterCtx,
    open: RwSignal<Option<Anchor>>,
    /// The category whose values are open.
    cat: RwSignal<Option<Cat>>,
    query: RwSignal<String>,
    /// The highlighted row among the active list's actionable rows.
    hi: RwSignal<usize>,
}

const POP_ID: &str = "fmenu";

fn document() -> Option<web_sys::Document> {
    dom::window().document()
}

impl Pop {
    fn show(self, anchor: Anchor, cat: Option<Cat>) {
        if self.open.get_untracked().is_none() {
            keys::remember_focus();
        }
        self.query.set(String::new());
        self.hi.set(0);
        self.cat.set(cat);
        self.open.set(Some(anchor));
    }

    /// Closed — focus handed back where it was (`restore`: a key closed it),
    /// or left where a click put it.
    fn close(self, restore: bool) {
        if self.open.get_untracked().is_none() {
            return;
        }
        self.open.set(None);
        self.cat.set(None);
        self.query.set(String::new());
        if restore {
            keys::restore_focus();
        } else {
            keys::forget_focus();
        }
    }

    fn direct(self) -> bool {
        matches!(self.open.get_untracked(), Some(Anchor::Pill(_)))
    }

    fn open_cat(self, c: Cat) {
        self.cat.set(Some(c));
        self.query.set(String::new());
        self.hi.set(0);
    }

    /// Back from a category's values to the categories, on it.
    fn back(self) {
        if let Some(c) = self.cat.get_untracked() {
            self.cat.set(None);
            self.query.set(String::new());
            self.hi.set(Cat::ALL.iter().position(|x| *x == c).unwrap_or(0));
        }
    }

    /// The rows the keys move over and Enter acts on.
    fn active(self) -> Vec<Row> {
        let q = self.query.get_untracked();
        let rows = untrack(|| match self.cat.get_untracked() {
            Some(c) => self.fx.facts(|k| offered_matching(c, k, &q)),
            None => self.fx.facts(|k| search(&q, k)),
        });
        rows.unwrap_or_default().into_iter().filter(Row::actionable).collect()
    }

    fn act(self, row: &Row) {
        match row {
            Row::Cat(c) => self.open_cat(*c),
            Row::Value { cat, token, .. } => self.fx.toggle(*cat, token),
            Row::TitleContains(text) => {
                let text = text.clone();
                self.fx.update(|f| f.set(Cat::Title, Op::Is, [text]));
                self.close(true);
            }
            Row::Heading(_) => {}
        }
    }

    /// The search field's keys: the arrows move, Enter picks the highlighted
    /// row (the first match), → opens a category, ← or a Backspace on an
    /// empty field goes back, Esc closes.
    fn key(self, ev: web_sys::KeyboardEvent) {
        let key = ev.key();
        let empty = self.query.with_untracked(String::is_empty);
        match key.as_str() {
            "Escape" => {
                ev.prevent_default();
                ev.stop_propagation();
                self.close(true);
            }
            "ArrowDown" | "ArrowUp" => {
                let len = self.active().len();
                if let Some(n) = menu::step(Some(self.hi.get_untracked().min(len.saturating_sub(1))), len, &key) {
                    self.hi.set(n);
                    ev.prevent_default();
                }
            }
            "Enter" => {
                ev.prevent_default();
                let rows = self.active();
                if let Some(r) = rows.get(self.hi.get_untracked()).or(rows.first()) {
                    self.act(r);
                }
            }
            "ArrowRight" if empty && self.cat.get_untracked().is_none() => {
                if let Some(Row::Cat(c)) = self.active().get(self.hi.get_untracked()) {
                    ev.prevent_default();
                    self.open_cat(*c);
                }
            }
            "ArrowLeft" | "Backspace" if empty && self.cat.get_untracked().is_some() && !self.direct() => {
                ev.prevent_default();
                self.back();
            }
            "Tab" => self.close(false),
            _ => {}
        }
    }
}

/// Focus the popover's field — the title's text when Title is open, the
/// search otherwise — once it is drawn.
fn focus_field() {
    request_animation_frame(|| {
        let Some(doc) = document() else { return };
        let el = doc
            .query_selector(&format!("#{POP_ID} input.ftitle-in"))
            .ok()
            .flatten()
            .or_else(|| doc.query_selector(&format!("#{POP_ID} input.fsearch")).ok().flatten());
        if let Some(el) = el.and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = el.focus();
        }
    });
}

/// The width the categories and a category's values need side by side;
/// with less room the values take the categories' place, behind a back row.
const SIDE_BY_SIDE_PX: f64 = 580.0;

/// Place the popover under its anchor, kept inside the content column it
/// hangs from (the ticket menu's [`menu::clamp`], over the column rather
/// than the viewport — the column clips what overflows it); it hangs from
/// the bar, so it scrolls with the page. Its height is what the viewport
/// leaves below the anchor; with too little width for two panes it is
/// `narrow`.
fn place(anchor: Anchor) {
    let Some(doc) = document() else { return };
    let Some(pop) = doc.get_element_by_id(POP_ID).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) else { return };
    let sel = match anchor {
        Anchor::Button => "#waves .fbtn".to_string(),
        Anchor::Pill(c) => format!("#waves .pill[data-cat=\"{}\"] .p-vals", c.key()),
    };
    let Some(at) = doc.query_selector(&sel).ok().flatten() else { return };
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

/// The bar's filter: the Filter button, the pills, `reset`, and the
/// popover. Mounted inside the board's `.wbar`, which the popover hangs
/// from.
pub(crate) fn filter_bar(fx: FilterCtx) -> impl IntoView {
    let pop = Pop {
        fx,
        open: RwSignal::new(None),
        cat: RwSignal::new(None),
        query: RwSignal::new(String::new()),
        hi: RwSignal::new(0),
    };

    // `f` opens the menu while the board is open — unless typing, or in a
    // dialog or a menu (keys.rs's rule), or an overlay is up.
    let fkey = window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
        if ev.default_prevented() || ev.is_composing() || ev.ctrl_key() || ev.meta_key() || ev.alt_key() || ev.key() != "f" {
            return;
        }
        let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
        if target.as_ref().is_some_and(keys::defer_to_focus) {
            return;
        }
        if document().and_then(|d| d.query_selector(".overlay").ok().flatten()).is_some() {
            return;
        }
        ev.prevent_default();
        if pop.open.get_untracked().is_none() {
            pop.show(Anchor::Button, None);
        }
    });
    // A mousedown anywhere but the popover (or the button, which toggles it)
    // closes it, as it does the ticket menu.
    let down = window_event_listener(leptos::ev::mousedown, move |ev: web_sys::MouseEvent| {
        if pop.open.get_untracked().is_none() {
            return;
        }
        let inside = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|el| el.closest(&format!("#{POP_ID}, #waves .fbtn, #waves .pill .p-vals")).ok().flatten().is_some());
        if !inside {
            pop.close(false);
        }
    });
    // A resize can move the anchor and change the room.
    let resize = window_event_listener(leptos::ev::resize, move |_| {
        if let Some(anchor) = pop.open.get_untracked() {
            place(anchor);
        }
    });
    on_cleanup(move || {
        fkey.remove();
        down.remove();
        resize.remove();
    });

    // Placed, and its field focused, when it opens and whenever its shape
    // changes (a category opened or closed).
    Effect::new(move |_| {
        let Some(anchor) = pop.open.get() else { return };
        pop.cat.track();
        request_animation_frame(move || place(anchor));
        focus_field();
    });

    let button = view! {
        <button
            class="fbtn"
            title="Filter (f)"
            aria-haspopup="dialog"
            aria-expanded=move || pop.open.with(Option::is_some).to_string()
            on:click=move |_| {
                if pop.open.get_untracked().is_some() {
                    pop.close(true);
                } else {
                    pop.show(Anchor::Button, None);
                }
            }
        >
            <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M2 3.5h12l-4.6 5.4v4.1l-2.8-1.3V8.9z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" />
            </svg>
            "Filter"
        </button>
    };

    let pills = move || {
        let route = fx.route.get_value();
        let tree = fx.tree.get();
        fx.filter.with(|f| {
            f.iter()
                .flat_map(|f| f.clauses())
                .map(|(c, cl)| pill_view(pop, c, cl, &route, tree.as_ref()))
                .collect_view()
        })
    };

    let reset = move || {
        let differs = fx.filter.with(|f| fx.default.with(|d| f.is_some() && d.is_some() && f != d));
        differs.then(|| {
            view! {
                <button class="reset" title="back to this board's default" on:click=move |_| fx.chosen.set(None)>
                    "reset"
                </button>
            }
        })
    };

    let is_open = Memo::new(move |_| pop.open.with(Option::is_some));
    view! {
        <span class="filters">{button}{pills}{reset}</span>
        {move || is_open.get().then(|| popover(pop))}
    }
}

fn pill_view(pop: Pop, c: Cat, cl: &Clause, route: &str, tree: Option<&ScTree>) -> impl IntoView {
    let fx = pop.fx;
    let shown = display_order(c, cl, route, tree);
    let (first, more) = pill_values(c, &shown);
    let all = shown.iter().map(|v| c.value_label(v)).collect::<Vec<_>>().join(", ");
    let op = op_label(c, cl.op, cl.values.len());
    view! {
        <span class="pill" data-cat=c.key() data-op=cl.op.word()>
            <span class="p-cat">{c.label()}</span>
            <button class="p-op" title="toggle the operator" on:click=move |_| fx.update(|f| f.flip(c))>{op}</button>
            <button
                class="p-vals"
                title=all
                on:click=move |_| {
                    if pop.open.get_untracked() == Some(Anchor::Pill(c)) {
                        pop.close(true);
                    } else {
                        pop.close(false);
                        pop.show(Anchor::Pill(c), Some(c));
                    }
                }
            >
                {first}
                {more.map(|m| view! { <span class="more">" "{m}</span> })}
            </button>
            <button class="p-x" title="remove this filter" aria-label=format!("remove the {} filter", c.label()) on:click=move |_| fx.update(|f| f.remove(c))>
                "×"
            </button>
        </span>
    }
}

/// The popover: the search field, the categories, and the open category's
/// values beside them (alone, opened from a pill).
fn popover(pop: Pop) -> impl IntoView {
    let fx = pop.fx;
    let direct = move || matches!(pop.open.get(), Some(Anchor::Pill(_)));
    let main_rows = Memo::new(move |_| {
        let q = pop.query.get();
        let open = pop.cat.get();
        fx.facts(|k| if open.is_some() { search("", k) } else { search(&q, k) }).unwrap_or_default()
    });
    let sub_rows = Memo::new(move |_| {
        let q = pop.query.get();
        pop.cat.get().and_then(|c| fx.facts(|k| offered_matching(c, k, &q))).unwrap_or_default()
    });
    let placeholder = move || match pop.cat.get() {
        Some(c) => format!("{}…", c.label()),
        None => "Filter…".to_string(),
    };
    // The search takes focus the moment it is drawn (as the palette's does).
    let search_ref = NodeRef::<leptos::html::Input>::new();
    search_ref.on_load(|el: web_sys::HtmlInputElement| {
        let _ = el.focus();
    });
    // Opened on Title's pill, the popover is Title's field alone.
    let no_search = Memo::new(move |_| matches!(pop.open.get(), Some(Anchor::Pill(_))) && pop.cat.get() == Some(Cat::Title));

    view! {
        <div
            id=POP_ID
            role="dialog"
            aria-label="Filter the board"
            class:direct=direct
            class:has-sub=move || pop.cat.with(Option::is_some)
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                // Escape from anywhere inside (the title's field, a row).
                if ev.key() == "Escape" {
                    ev.prevent_default();
                    ev.stop_propagation();
                    pop.close(true);
                }
            }
        >
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
                    on:keydown=move |ev| pop.key(ev)
                />
            })}
            <div class="panes">
                <div class="fpane fmain" role="menu" aria-label="categories">
                    {move || list_view(pop, main_rows.get(), move || pop.cat.get().is_none())}
                </div>
                {move || pop.cat.get().map(|c| view! {
                    <div class="fpane fsub" role="menu" aria-label=format!("{} values", c.label()) data-cat=c.key()>
                        <div class="frow back" on:mousedown=|ev| ev.prevent_default() on:click=move |_| pop.back()>
                            <span class="glyph">"‹"</span>
                            <span class="flabel">{c.menu_label()}</span>
                        </div>
                        {if c == Cat::Title {
                            title_view(pop).into_any()
                        } else {
                            view! { {move || list_view(pop, sub_rows.get(), move || true)} }.into_any()
                        }}
                    </div>
                })}
            </div>
        </div>
    }
}

/// Title's pane: its text, matched as you type.
fn title_view(pop: Pop) -> impl IntoView {
    let fx = pop.fx;
    let seed = fx.filter.with_untracked(|f| f.as_ref().and_then(|f| f.clause(Cat::Title)).map(|cl| cl.values[0].clone()).unwrap_or_default());
    let op = move || fx.filter.with(|f| f.as_ref().and_then(|f| f.clause(Cat::Title)).map_or(Op::Is, |cl| cl.op));
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
                    fx.update(|f| {
                        let op = f.clause(Cat::Title).map_or(Op::Is, |cl| cl.op);
                        f.set(Cat::Title, op, [text].into_iter().filter(|t| !t.trim().is_empty()));
                    });
                }
                on:keydown=move |ev: web_sys::KeyboardEvent| match ev.key().as_str() {
                    "Enter" => {
                        ev.prevent_default();
                        pop.close(true);
                    }
                    "Backspace" if event_target_value(&ev).is_empty() && !pop.direct() => {
                        ev.prevent_default();
                        pop.back();
                        focus_field();
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
fn list_view(pop: Pop, rows: Vec<Row>, live: impl Fn() -> bool + Copy + Send + Sync + 'static) -> impl IntoView {
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
            row_view(pop, row, at, live)
        })
        .collect_view()
        .into_any()
}

fn row_view(pop: Pop, row: Row, at: Option<usize>, live: impl Fn() -> bool + Copy + Send + Sync + 'static) -> impl IntoView {
    let fx = pop.fx;
    let hi = move || live() && at == Some(pop.hi.get());
    let hover = move |_| {
        if let (true, Some(i)) = (live(), at) {
            pop.hi.set(i);
        }
    };
    match row {
        Row::Heading(h) => view! { <div class="fhead" class:fdiv=h == "other boards">{h}</div> }.into_any(),
        Row::Cat(c) => {
            let applied = move || fx.filter.with(|f| f.as_ref().is_some_and(|f| f.clause(c).is_some()));
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
                        focus_field();
                    }
                >
                    <span class="glyph" aria-hidden="true">{c.glyph()}</span>
                    <span class="flabel">{c.menu_label()}</span>
                    {move || applied().then(|| view! { <span class="dot" title="applied"></span> })}
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
                    on:click=move |_| pop.act(&row)
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
                    if c == Cat::Shortcode {
                        fx.filter.with(|f| {
                            fx.tree.with(|t| {
                                let checked: BTreeSet<String> =
                                    f.as_ref().and_then(|f| f.clause(c)).map(|cl| cl.values.iter().cloned().collect()).unwrap_or_default();
                                t.as_ref().map_or(Tri::Off, |t| t.tri(&token, &checked))
                            })
                        })
                    } else if fx.filter.with(|f| f.as_ref().is_some_and(|f| f.has(c, &token))) {
                        Tri::On
                    } else {
                        Tri::Off
                    }
                })
            };
            let count = {
                let token = token.clone();
                move || {
                    // A shortcode not asked for has no count on this answer.
                    if c == Cat::Shortcode && !fx.epics.with(|e| e.as_ref().is_some_and(|e| e.contains(&token))) {
                        return String::new();
                    }
                    fx.counts.with(|k| k.get(&c).and_then(|m| m.get(&token)).copied().unwrap_or(0)).to_string()
                }
            };
            let click = {
                let token = token.clone();
                move |_| fx.toggle(c, &token)
            };
            let only = {
                let token = token.clone();
                move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    fx.only(&token);
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
                    <span class="box" data-state=move || state.get().as_str() aria-hidden="true">
                        {move || match state.get() {
                            Tri::On => "✓",
                            Tri::Mixed => "–",
                            Tri::Off => "",
                        }}
                    </span>
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
mod tests {
    use super::*;
    use menger_tickets_api::{Kind, TicketSummary};

    fn group(epic: &str, host: Option<&str>) -> (String, Group) {
        (
            epic.to_string(),
            Group {
                epic: epic.into(),
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
    fn platform() -> ScTree {
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

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    const PLEXUS_TREE: [&str; 10] =
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
    }

    /// A host chain that loops still lists every shortcode, once.
    #[test]
    fn a_looping_host_chain_is_still_listed() {
        let g: BTreeMap<String, Group> = [group("A", Some("B-2")), group("B", Some("A-3")), group("C", None)].into_iter().collect();
        let t = ScTree::of(&g);
        let rows = t.rows("C", std::iter::empty());
        let codes: Vec<String> = rows.iter().filter_map(|r| if let ScRow::Code { code, .. } = r { Some(code.clone()) } else { None }).collect();
        assert_eq!(codes, strs(&["C", "A", "B"]));
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
    }

    /// The Shortcode menu lists the route's tree first, then "other boards"
    /// — every other top-level tree, without what was listed — then a value
    /// no group knows.
    #[test]
    fn the_shortcode_menu_lists_the_route_tree_first() {
        let t = platform();
        let code = |c: &str, d: usize| ScRow::Code { code: c.into(), depth: d };
        let rows = t.rows("PLEXUS", std::iter::empty());
        assert_eq!(rows[0], code("PLEXUS", 0));
        assert_eq!(rows[4], code("CODEGEN", 2));
        assert_eq!(rows[10], ScRow::Divider);
        assert_eq!(rows[11..], [code("MNG", 0), code("LOOSE", 1), code("WAVE", 0)]);

        // A sub-shortcode's board: its tree first; PLEXUS's, without it, after.
        let extra = strs(&["NOPE"]);
        let rows = t.rows("MACRO", extra.iter());
        assert_eq!(rows[..3], [code("MACRO", 0), code("CODEGEN", 1), code("GEN", 2)]);
        assert_eq!(rows[3], ScRow::Divider);
        assert!(rows.iter().filter(|r| **r == code("MACRO", 0) || **r == code("MACRO", 1)).count() == 1, "listed once");
        assert_eq!(rows.last(), Some(&code("NOPE", 0)));
    }

    /// Acceptance 1: the default filter for a route — Shortcode is any of
    /// its tree, Kind is any of build, spike, gap; what it asks `waves` for.
    #[test]
    fn a_board_opens_on_its_shortcode_tree() {
        let t = platform();
        let d = BoardFilter::default_for("plexus", &t);
        assert_eq!(d.clause(Cat::Shortcode), Some(&Clause { op: Op::Is, values: strs(&PLEXUS_TREE) }));
        assert_eq!(d.clause(Cat::Kind), Some(&Clause { op: Op::Is, values: strs(&["build", "spike", "gap"]) }));
        assert_eq!(d.clauses().count(), 2);
        assert_eq!(d.epics("PLEXUS", &t), set(&PLEXUS_TREE));
        assert_eq!(d.kinds(), strs(&["build", "spike", "gap"]));
        assert_eq!(BoardFilter::default_for("WAVE", &t).epics("WAVE", &t), set(&["WAVE"]));
        assert_eq!(BoardFilter::default_for("NOPE", &t).epics("NOPE", &t), set(&["NOPE"]));

        // `is not` is the route's tree without the excluded; no clause,
        // every shortcode — and every kind of work.
        let not = BoardFilter::default().with(Cat::Shortcode, Op::Not, strs(&["MACRO", "CODEGEN", "GEN", "MNG"]));
        assert_eq!(not.epics("PLEXUS", &t), set(&["CONTEXT", "HYPERFORGE", "PLEXUS", "RETURNS", "STARTER", "SYNAPSE", "TRANSPORT"]));
        assert_eq!(BoardFilter::default().epics("PLEXUS", &t).len(), 13);
        assert_eq!(BoardFilter::default().kinds(), strs(&WORK_KINDS));
        let no_gaps = BoardFilter::default().with(Cat::Kind, Op::Not, strs(&["gap"]));
        assert_eq!(no_gaps.kinds(), strs(&["build", "spike", "scope", "execution"]));
    }

    /// Acceptance 1: the filter and the query, both ways — every key, both
    /// operators; the empty filter is `all`; no query, or a malformed one,
    /// is the default (`None`).
    #[test]
    fn the_filter_round_trips_through_the_query() {
        let f = BoardFilter::default()
            .with(Cat::Shortcode, Op::Is, strs(&["RETURNS", "CONTEXT"]))
            .with(Cat::Status, Op::Not, strs(&["Superseded", "Complete"]))
            .with(Cat::Wave, Op::Is, strs(&["done", "10", "2", "missing"]))
            .with(Cat::Kind, Op::Is, strs(&["gap", "build", "spike"]))
            .with(Cat::Severity, Op::Not, strs(&["none", "High"]))
            .with(Cat::Readiness, Op::Is, strs(&["workable"]))
            .with(Cat::OffBoard, Op::Not, strs(&["some"]))
            .with(Cat::Title, Op::Is, strs(&["envelope"]));
        let q = f.to_query();
        assert_eq!(
            q,
            "sc=is:CONTEXT,RETURNS&status=not:Complete,Superseded&wave=is:2,10,missing,done&kind=is:build,spike,gap\
             &sev=not:High,none&ready=is:workable&off=not:some&q=envelope"
        );
        assert_eq!(BoardFilter::parse_query(&q), Some(f.clone()));
        assert_eq!(BoardFilter::parse_query(&format!("?{q}")), Some(f.clone()), "a leading ? is the query's");

        // Status with its space, a title that says "not:", commas and an
        // ampersand: encoded, and back.
        let g = BoardFilter::default()
            .with(Cat::Status, Op::Is, strs(&["In Review"]))
            .with(Cat::Title, Op::Not, strs(&["not: a, b & c"]));
        let q = g.to_query();
        assert_eq!(q, "status=is:In%20Review&q=not:not%3A%20a%2C%20b%20%26%20c");
        assert_eq!(BoardFilter::parse_query(&q), Some(g));
        let h = BoardFilter::default().with(Cat::Title, Op::Is, strs(&["is: it"]));
        assert_eq!(BoardFilter::parse_query(&h.to_query()), Some(h));

        // Lenient spelling: any case, `+` for a space, the op optional.
        let lenient = BoardFilter::parse_query("sc=returns&status=is:in+review,PENDING&kind=not:Scope").unwrap();
        assert_eq!(lenient.clause(Cat::Shortcode).unwrap().values, strs(&["RETURNS"]));
        assert_eq!(lenient.clause(Cat::Status).unwrap().values, strs(&["In Review", "Pending"]));
        assert_eq!(lenient.clause(Cat::Kind), Some(&Clause { op: Op::Not, values: strs(&["scope"]) }));

        // Empty: the empty filter is `all`; no query is the default.
        assert_eq!(BoardFilter::default().to_query(), "all");
        assert_eq!(BoardFilter::parse_query("all"), Some(BoardFilter::default()));
        assert_eq!(BoardFilter::parse_query(""), None);
        assert_eq!(BoardFilter::parse_query("?"), None);
        assert_eq!(BoardFilter::parse_query("&&"), None);

        // Malformed: the default.
        for bad in [
            "bogus=is:x",
            "status=is:Nope",
            "status=is:Doc",
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
            assert_eq!(BoardFilter::parse_query(bad), None, "{bad}");
        }
    }

    /// The hash's query, read and replaced.
    #[test]
    fn the_hash_carries_the_query() {
        assert_eq!(query_of("#/waves/PLEXUS?sc=is:A&q=x"), "sc=is:A&q=x");
        assert_eq!(query_of("#/waves/PLEXUS"), "");
        assert_eq!(with_query("#/waves/PLEXUS?sc=is:A", "q=x"), "#/waves/PLEXUS?q=x");
        assert_eq!(with_query("#/waves/PLEXUS?sc=is:A", ""), "#/waves/PLEXUS");
        assert_eq!(with_query("#/waves/plexus", "all"), "#/waves/plexus?all");
    }

    fn summary(id: &str, status: Status, kind: Kind, severity: Option<Severity>, title: &str) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').unwrap();
        TicketSummary {
            id: id.into(),
            epic: epic.into(),
            seq: seq.parse().unwrap(),
            title: title.into(),
            status,
            kind,
            severity,
            parent: None,
            child_count: 0,
            blocked_by: Vec::new(),
            unlocks: Vec::new(),
            commits: Vec::new(),
            path: None,
            placement: None,
            created_unix: 0,
            updated_unix: 0,
        }
    }

    fn card(id: &str, status: Status, kind: Kind, sev: Option<Severity>, placement: Placement, waiting_on: &[&str], title: &str) -> Card {
        let waiting_on = strs(waiting_on);
        let summary = TicketSummary { blocked_by: waiting_on.clone(), placement: Some(placement), ..summary(id, status, kind, sev, title) };
        Card { summary, placement, waiting_on }
    }

    /// A board's answer: RETURNS and CONTEXT, in four waves, a missing
    /// blocker, two done; one card waits on a ticket off the board (MNG-4).
    fn fixture() -> Answer {
        use Placement::{Done, MissingBlocker, Wave};
        let mut a = Answer {
            cards: vec![
                card("RETURNS-4", Status::Ready, Kind::Build, Some(Severity::High), Wave { n: 1 }, &[], "envelope 0.7"),
                card("RETURNS-5", Status::Coding, Kind::Spike, None, Wave { n: 1 }, &[], "hub-codegen streams"),
                card("CONTEXT-2", Status::Pending, Kind::Gap, None, Wave { n: 1 }, &[], "context loss"),
                card("RETURNS-6", Status::Pending, Kind::Build, Some(Severity::Low), Wave { n: 2 }, &["RETURNS-4"], "Envelope conformance"),
                card("CONTEXT-4", Status::Blocked, Kind::Build, None, Wave { n: 3 }, &["RETURNS-6", "MNG-4"], "carry the context"),
                card("RETURNS-7", Status::Pending, Kind::Build, None, Wave { n: 4 }, &["CONTEXT-4"], "release"),
                card("RETURNS-9", Status::Pending, Kind::Build, None, MissingBlocker, &["RETURNS-99"], "orphan"),
                card("RETURNS-2", Status::Complete, Kind::Build, None, Done, &[], "old envelope"),
                card("CONTEXT-1", Status::Superseded, Kind::Spike, None, Done, &[], "first try"),
            ],
            ..Answer::default()
        };
        retally(&mut a);
        a
    }

    fn shown(f: &BoardFilter) -> Vec<String> {
        narrow(&fixture(), f).cards.into_iter().map(|c| c.summary.id).collect()
    }

    /// Acceptance 1: each category applied to the answer's cards — both
    /// operators; and the tally follows what is shown.
    #[test]
    fn each_category_narrows_the_cards() {
        let one = |c: Cat, op: Op, v: &[&str]| shown(&BoardFilter::default().with(c, op, strs(v)));
        assert_eq!(shown(&BoardFilter::default()).len(), 9, "no clause: every card");
        assert_eq!(one(Cat::Shortcode, Op::Is, &["CONTEXT"]), strs(&["CONTEXT-2", "CONTEXT-4", "CONTEXT-1"]));
        assert_eq!(one(Cat::Shortcode, Op::Not, &["RETURNS"]), strs(&["CONTEXT-2", "CONTEXT-4", "CONTEXT-1"]));
        assert_eq!(
            one(Cat::Status, Op::Not, &["Pending"]),
            strs(&["RETURNS-4", "RETURNS-5", "CONTEXT-4", "RETURNS-2", "CONTEXT-1"])
        );
        assert_eq!(one(Cat::Status, Op::Is, &["Coding", "In Review"]), strs(&["RETURNS-5"]));
        assert_eq!(one(Cat::Wave, Op::Is, &["2", "missing"]), strs(&["RETURNS-6", "RETURNS-9"]));
        assert_eq!(one(Cat::Wave, Op::Not, &["1", "done"]), strs(&["RETURNS-6", "CONTEXT-4", "RETURNS-7", "RETURNS-9"]));
        assert_eq!(one(Cat::Kind, Op::Is, &["spike"]), strs(&["RETURNS-5", "CONTEXT-1"]));
        assert_eq!(one(Cat::Kind, Op::Not, &["build"]), strs(&["RETURNS-5", "CONTEXT-2", "CONTEXT-1"]));
        assert_eq!(one(Cat::Severity, Op::Is, &["High", "Low"]), strs(&["RETURNS-4", "RETURNS-6"]));
        assert_eq!(one(Cat::Severity, Op::Not, &["none"]), strs(&["RETURNS-4", "RETURNS-6"]));
        assert_eq!(one(Cat::Readiness, Op::Is, &["workable"]), strs(&["RETURNS-4", "RETURNS-5", "CONTEXT-2"]));
        assert_eq!(one(Cat::Readiness, Op::Is, &["waiting"]), strs(&["RETURNS-6", "CONTEXT-4", "RETURNS-7"]));
        assert_eq!(one(Cat::Readiness, Op::Is, &["missing"]), strs(&["RETURNS-9"]));
        assert_eq!(
            one(Cat::Readiness, Op::Not, &["workable", "waiting", "missing"]),
            strs(&["RETURNS-2", "CONTEXT-1"]),
            "a done card has no readiness"
        );
        // Off the board: MNG-4 and RETURNS-99; RETURNS-4 and -6 are cards.
        assert_eq!(one(Cat::OffBoard, Op::Is, &["some"]), strs(&["CONTEXT-4", "RETURNS-9"]));
        assert_eq!(one(Cat::OffBoard, Op::Not, &["none"]), strs(&["CONTEXT-4", "RETURNS-9"]));
        assert_eq!(one(Cat::OffBoard, Op::Is, &["none"]).len(), 7);
        // Title: a substring of the title or the id, any case.
        assert_eq!(one(Cat::Title, Op::Is, &["ENVELOPE"]), strs(&["RETURNS-4", "RETURNS-6", "RETURNS-2"]));
        assert_eq!(one(Cat::Title, Op::Is, &["context-"]), strs(&["CONTEXT-2", "CONTEXT-4", "CONTEXT-1"]));
        assert_eq!(one(Cat::Title, Op::Not, &["envelope"]).len(), 6);

        // Clauses combine; the tally is the shown cards'.
        let f = BoardFilter::default()
            .with(Cat::Shortcode, Op::Is, strs(&["RETURNS"]))
            .with(Cat::Status, Op::Not, strs(&["Pending"]));
        let a = narrow(&fixture(), &f);
        assert_eq!(a.cards.iter().map(|c| c.summary.id.as_str()).collect::<Vec<_>>(), ["RETURNS-4", "RETURNS-5", "RETURNS-2"]);
        assert_eq!(crate::waves::tally(&a), "2 in 1 wave · 1 done");
        assert_eq!(crate::waves::tally(&fixture()), "6 in 4 waves · 1 missing · 2 done");
    }

    /// Acceptance 1: the count per value — how many of the answer's cards
    /// have it, whatever else is applied.
    #[test]
    fn counts_are_per_value_over_the_answer() {
        let k = counts(&fixture());
        let n = |c: Cat, v: &str| k.get(&c).and_then(|m| m.get(v)).copied().unwrap_or(0);
        assert_eq!((n(Cat::Shortcode, "RETURNS"), n(Cat::Shortcode, "CONTEXT"), n(Cat::Shortcode, "MNG")), (6, 3, 0));
        assert_eq!((n(Cat::Status, "Pending"), n(Cat::Status, "Coding"), n(Cat::Status, "In Review")), (4, 1, 0));
        assert_eq!((n(Cat::Wave, "1"), n(Cat::Wave, "2"), n(Cat::Wave, "missing"), n(Cat::Wave, "done")), (3, 1, 1, 2));
        assert_eq!((n(Cat::Kind, "build"), n(Cat::Kind, "spike"), n(Cat::Kind, "gap"), n(Cat::Kind, "scope")), (6, 2, 1, 0));
        assert_eq!((n(Cat::Severity, "High"), n(Cat::Severity, "Low"), n(Cat::Severity, "none")), (1, 1, 7));
        assert_eq!((n(Cat::Readiness, "workable"), n(Cat::Readiness, "waiting"), n(Cat::Readiness, "missing")), (3, 3, 1));
        assert_eq!((n(Cat::OffBoard, "some"), n(Cat::OffBoard, "none")), (2, 7));
        assert!(!k.contains_key(&Cat::Title));
    }

    /// The menu: the categories in order; Status in its groups; Wave up to
    /// the answer's last; typing searches every category's values — "ret"
    /// offers RETURNS — and ends with Title contains; inside a category,
    /// typing narrows its values.
    #[test]
    fn the_menu_lists_categories_values_and_search() {
        let t = platform();
        let f = BoardFilter::default_for("PLEXUS", &t);
        let a = fixture();
        let k = Facts { route: "PLEXUS", tree: &t, filter: &f, answer: Some(&a) };
        assert_eq!(search("", &k), Cat::ALL.map(Row::Cat).to_vec());
        assert_eq!(
            Cat::ALL.map(Cat::menu_label),
            ["Shortcode", "Status", "Wave", "Kind", "Severity", "Readiness", "Off-board blockers", "Title contains…"]
        );
        let val = |c: Cat, t: &str| Row::Value { cat: c, token: t.into(), depth: 0, scoped: false };
        let status = offered(Cat::Status, &k);
        assert_eq!(status[0], Row::Heading("in progress"));
        assert_eq!(status[1..3], [val(Cat::Status, "Coding"), val(Cat::Status, "In Review")]);
        assert_eq!(status[3], Row::Heading("ready"));
        assert_eq!(status.last(), Some(&val(Cat::Status, "Superseded")));
        let wave: Vec<String> = offered(Cat::Wave, &k).into_iter().filter_map(|r| if let Row::Value { token, .. } = r { Some(token) } else { None }).collect();
        assert_eq!(wave, strs(&["1", "2", "3", "4", "missing", "done"]));

        let found = search("ret", &k);
        assert!(found.contains(&Row::Value { cat: Cat::Shortcode, token: "RETURNS".into(), depth: 0, scoped: true }), "{found:?}");
        assert_eq!(found.last(), Some(&Row::TitleContains("ret".into())));
        let found = search("sta", &k);
        assert_eq!(found[0], Row::Cat(Cat::Status), "a category by its name first");
        assert!(search("review", &k).contains(&Row::Value { cat: Cat::Status, token: "In Review".into(), depth: 0, scoped: true }));
        assert_eq!(offered_matching(Cat::Shortcode, &k, "gen"), [val(Cat::Shortcode, "CODEGEN"), val(Cat::Shortcode, "GEN")]);
        assert_eq!(offered_matching(Cat::Readiness, &k, "now"), [val(Cat::Readiness, "workable")]);
        assert!(offered(Cat::Title, &k).is_empty());
    }

    /// A pill: its operator in words, its values first-then-`+n` — the
    /// route's own shortcode first.
    #[test]
    fn a_pill_reads_like_linear() {
        let t = platform();
        let d = BoardFilter::default_for("PLEXUS", &t);
        let sc = d.clause(Cat::Shortcode).unwrap();
        let order = display_order(Cat::Shortcode, sc, "PLEXUS", Some(&t));
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
        assert_eq!(f.clause(Cat::Status).unwrap().op, Op::Not);
        f.toggle(Cat::Status, "Complete");
        assert_eq!(f.clause(Cat::Status).unwrap().values, strs(&["Pending", "Complete"]), "the menu's order");
        f.toggle(Cat::Status, "Pending");
        f.toggle(Cat::Status, "Complete");
        assert!(f.clause(Cat::Status).is_none());
        f.remove(Cat::Kind);
        assert_ne!(f, d);
    }
}
