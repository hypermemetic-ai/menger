//! FILTER-11: the lens — a filter at every level of the app, each the same
//! generic filter (FILTER-10's language), combined so a surface shows what
//! every filter above it admits (FILTER-9 decisions 1, 2, 7).
//!
//! - **The levels** ([`Level`]): `global` (the header's `Filter` button, the
//!   lens bar under the header while it is non-empty, `F`), `nav`, `board`,
//!   `waves`, `inputs`. A surface's **effective filter** is
//!   `global.and(own)` ([`Lens::effective`]); the palette and the ticket
//!   page read `global` alone.
//! - **The state** of a level is what the reader chose, or — nothing chosen
//!   — its default: the empty filter, except the wave board's, which opens
//!   on its shortcode tree (WAVE-12), and the nav's, which leaves menger's own
//!   documents out — `not menger doc` (FILTER-21, [`nav_default`]; `reset`
//!   returns to it). What a level keeps is [`Kept`]: its
//!   filter, and the surface's own (local) clauses — the wave board's Wave,
//!   Readiness, Off-board blockers and Title, which the language does not
//!   have (FILTER-9 decision 6).
//! - **Kept per viewer and in the URL**: each level's filter is in local
//!   storage ([`storage_key`]) and in the route's query as canonical text,
//!   percent-encoded — `g=` global, `n=` nav, `f=` the route's own surface
//!   (with its local clauses under WAVE-12's keys). On load the URL's value
//!   wins over the stored one, and a malformed value is the level's
//!   default; a change rewrites the query with `replaceState`; a navigation
//!   carries `g` and `n` to the new route ([`carry`]). No value is the
//!   level's default.
//! - **WAVE-12's links** (`#/waves/PLEXUS?sc=is:RETURNS&status=not:Complete`)
//!   still open the board they named; the first change writes it as `f=`.
//! - FILTER-16: **the inputs page** is the `inputs` level: its own
//!   categories (State, Removed) ride beside `f=` under their keys, as the
//!   wave board's do; an INPUT-6 link (`#/inputs?sc=INPUT&state=open&
//!   removed`) opens the filter it named and is written as `f=` at once
//!   ([`crate::inputs_page::read_input_6`]). The components and settings
//!   pages show no ticket: the lens bar says the filter does not apply there
//!   ([`applies`]).
//!
//! - FILTER-39: **the groups** of each level ([`Lens::ors`], src/ors.rs —
//!   groups joined by an explicit `or`, clauses inside a group `and`) — the
//!   groups last edited while the filter still says them (a draft group
//!   `+ or` made), else the filter's own; [`Lens::edit_ors`] (the panel's
//!   ticks) commits them as the level's filter. A filter applied from
//!   outside the panel ([`Lens::apply`], a link's URL, Make global, `reset`)
//!   moves the level's [`Lens::stamp`], and naming saves lets go of the
//!   name.
//!
//! Everything that decides — the query both ways, the storage keys, the
//! carrying, what a route's surface is — is a pure function here, pinned by
//! native tests.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use std::collections::HashSet;

use menger_tickets_api::filter::{Conj, Field, Filter, Group, Predicate};

use crate::ors::Ors;

use crate::filter::{fold_key, Cat, Clash, Clausal, Clause, Clauses, Host, Op, PanelHost, Pill, Pop, ScTree, DEFAULT_KINDS};
use crate::{bindings, dom, keys, nav, Route};

// ---------------------------------------------------------------------
// The levels

/// A level of the lens — where a filter lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Level {
    /// Every ticket surface: the header's filter.
    Global,
    /// The nav (FILTER-14).
    Nav,
    /// The table board, `#/` (FILTER-12).
    Board,
    /// The wave board, `#/waves/<SC>` (its filter is kept per shortcode).
    Waves,
    /// The inputs page, `#/inputs` (FILTER-16).
    Inputs,
}

impl Level {
    pub(crate) const ALL: [Level; 5] = [Level::Global, Level::Nav, Level::Board, Level::Waves, Level::Inputs];

    /// The level's name, as a pill's mark and the DOM's `data-level` say it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Level::Global => "global",
            Level::Nav => "nav",
            Level::Board => "board",
            Level::Waves => "waves",
            Level::Inputs => "inputs",
        }
    }

    /// FILTER-22: the level a name says (`data-tag-level`) — `None` for none.
    pub(crate) fn of_name(name: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.name() == name)
    }

    /// The key its filter rides under in a route's query: `g`, `n`, or `f`
    /// for the route's own surface.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Level::Global => "g",
            Level::Nav => "n",
            Level::Board | Level::Waves | Level::Inputs => "f",
        }
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

/// The route's own surface — the level its `f=` is — and the shortcode a
/// wave board keeps its filter under; `None` for a route with no filter of
/// its own (a ticket page, the boards' index, components, settings).
pub(crate) fn surface_of(route: &Route) -> Option<(Level, Option<String>)> {
    match route {
        Route::Board => Some((Level::Board, None)),
        Route::Waves(sc) if !sc.is_empty() => Some((Level::Waves, Some(sc.to_ascii_uppercase()))),
        Route::Inputs => Some((Level::Inputs, None)),
        _ => None,
    }
}

/// Where a level's filter is kept per viewer: `menger.lens.<level>`, and the
/// wave board's per shortcode (`menger.lens.waves.PLEXUS`) — its default is
/// the shortcode's tree, so one board's filter is not another's.
pub(crate) fn storage_key(level: Level, sc: Option<&str>) -> String {
    match (level, sc) {
        (Level::Waves, Some(sc)) => format!("menger.lens.waves.{}", sc.to_ascii_uppercase()),
        (l, _) => format!("menger.lens.{}", l.name()),
    }
}

// ---------------------------------------------------------------------
// The query

/// One `key=value` of a query as written (the value still percent-encoded);
/// a key with no `=` (`removed`, WAVE-12's `all`) has no value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pair {
    pub(crate) key: String,
    pub(crate) value: Option<String>,
}

impl Pair {
    pub(crate) fn new(key: &str, value: Option<String>) -> Self {
        Pair { key: key.to_string(), value }
    }
}

/// A query's pairs, in order (a leading `?` and empty pieces dropped).
pub(crate) fn pairs(q: &str) -> Vec<Pair> {
    q.trim()
        .trim_start_matches('?')
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => Pair::new(k, Some(v.to_string())),
            None => Pair::new(p, None),
        })
        .collect()
}

/// Pairs back into a query (no `?`).
pub(crate) fn join(ps: &[Pair]) -> String {
    ps.iter()
        .map(|p| match &p.value {
            Some(v) => format!("{}={v}", p.key),
            None => p.key.clone(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// A value percent-encoded for the query: everything but `A–Z a–z 0–9 - _ . ~`.
pub(crate) fn enc(s: &str) -> String {
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
pub(crate) fn dec(s: &str) -> Option<String> {
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

/// The hash without its query.
pub(crate) fn path_of(hash: &str) -> &str {
    hash.split_once('?').map_or(hash, |(p, _)| p)
}

/// `hash` with its query replaced by `q` (none when `q` is empty).
pub(crate) fn with_query(hash: &str, q: &str) -> String {
    let path = path_of(hash);
    if q.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{q}")
    }
}

/// What a query says of one key: absent; there but not well formed (a
/// value that does not decode, or the key twice); or its decoded text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Said {
    Absent,
    Malformed,
    Text(String),
}

/// What `q` says under `key` — the text a level's filter is written as.
pub(crate) fn said(q: &str, key: &str) -> Said {
    let mut found = Said::Absent;
    for p in pairs(q).into_iter().filter(|p| p.key == key) {
        if found != Said::Absent {
            return Said::Malformed;
        }
        found = match p.value.as_deref().map(dec) {
            Some(Some(t)) => Said::Text(t),
            Some(None) => Said::Malformed,
            None => Said::Text(String::new()),
        };
    }
    found
}

/// The lens's keys on a route: `g`, `n`, `f`, and — on a surface with local
/// categories or WAVE-12's legacy keys — those (`extra`).
fn lens_owns(key: &str, extra: &[&str]) -> bool {
    matches!(key, "g" | "n" | "f") || extra.contains(&key)
}

/// `hash` with the lens's part of its query written: `g`, `n`, `f` as given
/// (`None`: no value — the level's default), then `local` (a surface's own
/// clauses, already as pairs); every key in `owned` that is not rewritten is
/// dropped (WAVE-12's keys, on the first change), and the page's own keys
/// follow in their order.
pub(crate) fn write(hash: &str, g: Option<&str>, n: Option<&str>, f: Option<&str>, local: &[Pair], owned: &[&str]) -> String {
    let mut out: Vec<Pair> = Vec::new();
    for (key, text) in [("g", g), ("n", n), ("f", f)] {
        if let Some(t) = text {
            out.push(Pair::new(key, Some(enc(t))));
        }
    }
    out.extend(local.iter().cloned());
    out.extend(pairs(query_of(hash)).into_iter().filter(|p| !lens_owns(&p.key, owned)));
    with_query(hash, &join(&out))
}

/// One key of the lens's part of a query set (`Some`) or taken out (`None`),
/// everything else as it was.
pub(crate) fn write_key(hash: &str, key: &str, text: Option<&str>) -> String {
    let mut ps = pairs(query_of(hash));
    let at = ps.iter().position(|p| p.key == key);
    match (at, text) {
        (Some(i), Some(t)) => ps[i].value = Some(enc(t)),
        (Some(i), None) => {
            ps.remove(i);
        }
        (None, Some(t)) => {
            // `g`, `n`, `f` first, in that order.
            let order = |k: &str| ["g", "n", "f"].iter().position(|x| *x == k);
            let mine = order(key).unwrap_or(usize::MAX);
            let i = ps.iter().position(|p| order(&p.key).is_none_or(|o| o > mine)).unwrap_or(ps.len());
            ps.insert(i, Pair::new(key, Some(enc(t))));
        }
        (None, None) => {}
    }
    with_query(hash, &join(&ps))
}

/// The hash once the lens has written its part: `g` and `n` as the levels
/// keep them (`None`: the default, no value) — so a navigation to a route
/// that did not say them carries them to it; and, when `surface` is given,
/// the route's surface's `f=` and local pairs, its `owned` keys (WAVE-12's)
/// dropped. With no surface given (none, or a WAVE-12 link not yet changed)
/// the rest of the query is left as it is; a page's own keys are kept.
pub(crate) fn lens_hash(hash: &str, g: Option<&str>, n: Option<&str>, surface: Option<(Option<&str>, &[Pair], &[&str])>) -> String {
    match surface {
        Some((f, local, owned)) => write(hash, g, n, f, local, owned),
        None => write_key(&write_key(hash, "g", g), "n", n),
    }
}

/// FILTER-16: whether the lens applies on `route` — every route but the
/// components and settings pages, which show no ticket, and (COMMENT-3) the
/// comments page, which has filters of its own; (DEPLOY-13) the deployments
/// page, whose waiting list is its own clause.
pub(crate) fn applies(route: &Route) -> bool {
    !matches!(route, Route::Components(_) | Route::Settings | Route::Comments | Route::Deployments(_))
}

// ---------------------------------------------------------------------
// Pills ⇄ the language's groups

/// The language's field for value `t` of the server-backed category `c`
/// (`None`: a local category, or not one of `c`'s values).
pub(crate) fn field_of(c: Cat, t: &str) -> Option<Field> {
    Some(match c {
        Cat::Shortcode => Field::In(t.to_ascii_uppercase()),
        Cat::Status => Field::Status(t.parse().ok()?),
        Cat::Kind => Field::Kind(t.parse().ok()?),
        Cat::Severity if t == "none" => Field::Severity(None),
        Cat::Severity => Field::Severity(Some(t.parse().ok()?)),
        Cat::Ready => Field::Ready,
        Cat::Blocked => Field::Blocked,
        Cat::Commits => Field::HasCommits,
        Cat::MengerDocs => Field::MengerDoc,
        Cat::Asking => Field::Asking,
        Cat::Label => Field::Label(menger_tickets_api::labels::normalize(t).ok()?),
        _ => return None,
    })
}

/// The category drawing a field, and the field's value as that category's
/// token (none for a flag) — `None` for a field no category draws
/// (`category`, `under`, `text`).
pub(crate) fn token_of(f: &Field) -> Option<(Cat, Option<String>)> {
    Some(match f {
        Field::In(e) => (Cat::Shortcode, Some(e.to_ascii_uppercase())),
        Field::Status(s) => (Cat::Status, Some(s.as_str().to_string())),
        Field::Kind(k) => (Cat::Kind, Some(k.as_str().to_string())),
        Field::Severity(s) => (Cat::Severity, Some(s.map_or("none".to_string(), |s| s.as_str().to_ascii_lowercase()))),
        Field::Ready => (Cat::Ready, None),
        Field::Blocked => (Cat::Blocked, None),
        Field::HasCommits => (Cat::Commits, None),
        Field::MengerDoc => (Cat::MengerDocs, None),
        Field::Asking => (Cat::Asking, None),
        Field::Label(l) => (Cat::Label, Some(l.to_ascii_lowercase())),
        // FILTER-27: a time clause is a pill of its own (When's), never a
        // category's value.
        // FILTER-36: a nested group and a saved name are drawn as their text.
        // ARCHIVE-2: `archived` is a pill of its own too (ARCHIVE-3 draws it).
        Field::Category(_) | Field::Under(_) | Field::Ticket(_) | Field::Text(_) | Field::History(_) | Field::Group(_) | Field::Saved(_) | Field::Archived => {
            return None
        }
    })
}

/// A server-backed category's clause as the language's group: `is` one
/// value → `status X`; `is any of` → `status in (…)`; `is not` → `not
/// status in (…)` (Shortcode's: `in X`, `in (A, B)`, `not in (A, B)`); a
/// flag's `yes` → `ready`, `no` → `not ready`.
pub(crate) fn group_of(c: Cat, cl: &Clause) -> Option<Group> {
    if c.is_flag() {
        let field = field_of(c, "yes")?;
        return Some(Group::one(if cl.values.first()? == "yes" { Predicate::is(field) } else { Predicate::is_not(field) }));
    }
    let fields: Vec<Field> = cl.values.iter().map(|v| field_of(c, v)).collect::<Option<_>>()?;
    (!fields.is_empty()).then(|| match cl.op {
        Op::Is => Group::any(fields.into_iter().map(Predicate::is).collect()),
        Op::Not => Group::all(fields.into_iter().map(Predicate::is_not).collect()),
    })
}

/// The group as a category's clause, when a category draws it: one field's
/// values, none negated and any of them (`is`), or all negated and none of
/// them (`is not`); a flag, alone. Anything else — `(blocked or status
/// Blocked)`, `under X`, a `category` group — is a custom pill.
pub(crate) fn clause_of(g: &Group) -> Option<(Cat, Clause)> {
    let first = g.preds.first()?;
    let (cat, _) = token_of(&first.field)?;
    if cat.is_flag() {
        return (g.preds.len() == 1).then(|| (cat, Clause { op: Op::Is, values: vec![if first.not { "no" } else { "yes" }.to_string()] }));
    }
    let not = first.not;
    if g.preds.len() > 1 && g.conj != if not { Conj::All } else { Conj::Any } {
        return None;
    }
    let mut values = Vec::new();
    for p in &g.preds {
        match token_of(&p.field) {
            Some((c, Some(v))) if c == cat && p.not == not => values.push(v),
            _ => return None,
        }
    }
    Some((cat, Clause::new(cat, if not { Op::Not } else { Op::Is }, values)?))
}

// ---------------------------------------------------------------------
// What a level keeps

/// What a level keeps: its filter (FILTER-10's language), and its
/// surface's local clauses — the categories the language has not got.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Kept {
    pub(crate) filter: Filter,
    pub(crate) local: Clauses,
}

impl Kept {
    /// The groups each category's pill is: the first group a category
    /// draws is its pill; a later one of the same category is custom.
    fn drawn(&self) -> Vec<Option<(Cat, Clause)>> {
        let mut taken = HashSet::new();
        self.filter.groups.iter().map(|g| clause_of(g).filter(|(c, _)| taken.insert(*c))).collect()
    }

    /// The pills: a category's clause per group a category draws, a custom
    /// pill per other group — in the filter's order — then the local
    /// clauses in their categories' order.
    pub(crate) fn pills(&self) -> Vec<Pill> {
        let mut out: Vec<Pill> = self
            .drawn()
            .into_iter()
            .zip(&self.filter.groups)
            .enumerate()
            .map(|(at, (d, g))| match d {
                Some((cat, clause)) => Pill::Clause { cat, clause },
                // FILTER-36: a group holding a group or a saved name is its
                // canonical text, edited as text, until FILTER-37's tree.
                None if g.is_nested() => Pill::Custom { text: g.format(), at },
                // FILTER-31: two members or more are a box.
                None if crate::anyof::is_group(g) => Pill::Group { at, conj: g.conj, members: crate::anyof::members(g) },
                None => match crate::when::of_group(g) {
                    Some(t) => Pill::Time { at, t },
                    None => Pill::Custom { text: g.format(), at },
                },
            })
            .collect();
        out.extend(self.local.iter().map(|(cat, cl)| Pill::Clause { cat, clause: cl.clone() }));
        out
    }

    /// Each category's clause (the menu's checked boxes).
    pub(crate) fn clauses(&self) -> Clauses {
        let mut out = self.local.clone();
        for (c, cl) in self.drawn().into_iter().flatten() {
            out.set_clause(c, Some(cl));
        }
        out
    }

    /// How many groups it holds — local clauses counted as groups.
    pub(crate) fn len(&self) -> usize {
        self.filter.groups.len() + self.local.iter().count()
    }

    /// The custom group at `at` taken out.
    #[cfg(test)]
    pub(crate) fn remove_group(&mut self, at: usize) {
        if at < self.filter.groups.len() {
            self.filter.groups.remove(at);
        }
    }
}

impl Clausal for Kept {
    fn clause(&self, c: Cat) -> Option<Clause> {
        if c.is_local() {
            return self.local.clause(c);
        }
        self.drawn().into_iter().flatten().find(|(x, _)| *x == c).map(|(_, cl)| cl)
    }

    /// A server-backed category's clause replaces its group in place (a new
    /// one is added last); none, its group goes.
    fn set_clause(&mut self, c: Cat, cl: Option<Clause>) {
        if c.is_local() {
            self.local.set_clause(c, cl);
            return;
        }
        let at = self.drawn().iter().position(|d| d.as_ref().is_some_and(|(x, _)| *x == c));
        match (at, cl.and_then(|cl| group_of(c, &cl))) {
            (Some(i), Some(g)) => self.filter.groups[i] = g,
            (Some(i), None) => {
                self.filter.groups.remove(i);
            }
            (None, Some(g)) => self.filter.groups.push(g),
            (None, None) => {}
        }
    }
}

/// The local categories a level's surface has (the wave board's; the
/// inputs page's, FILTER-16).
pub(crate) fn locals(level: Level) -> &'static [Cat] {
    match level {
        Level::Waves => &[Cat::Wave, Cat::Readiness, Cat::OffBoard, Cat::Title],
        Level::Inputs => &[Cat::State, Cat::Removed],
        _ => &[],
    }
}

/// WAVE-12's keys a wave board's URL may still carry — read as its board,
/// dropped on the first change.
const WAVE_12_KEYS: [&str; 9] = ["sc", "status", "wave", "kind", "sev", "ready", "off", "q", "all"];

/// FILTER-16: INPUT-6's keys an inputs page's URL may still carry — read
/// as its filter, written as `f=` and its local pairs at once.
pub(crate) const INPUT_6_KEYS: [&str; 3] = ["sc", "state", "removed"];

/// The keys the lens owns on `level`'s route besides `g`, `n`, `f`.
fn owned_keys(level: Option<Level>) -> &'static [&'static str] {
    match level {
        Some(Level::Waves) => &WAVE_12_KEYS,
        Some(Level::Inputs) => &INPUT_6_KEYS,
        _ => &[],
    }
}

/// A level's state from its canonical text and its local pairs — `None`
/// when either is malformed (the text does not parse).
pub(crate) fn kept_of(text: &str, local: &[Pair], level: Level) -> Option<Kept> {
    let filter = Filter::parse(text).ok()?;
    let cats = locals(level);
    let local = if local.iter().any(|p| Cat::of_key(&p.key).is_some_and(|c| cats.contains(&c))) {
        Clauses::parse_pairs(local, cats)?
    } else {
        Clauses::default()
    };
    Some(Kept { filter, local })
}

/// A level's pairs in a query: its key's canonical text, then its local
/// clauses under their keys.
pub(crate) fn pairs_of(level: Level, k: &Kept) -> Vec<Pair> {
    let mut out = vec![Pair::new(level.key(), Some(enc(&k.filter.format())))];
    out.extend(k.local.to_pairs());
    out
}

/// What a query (or a stored value) says of `level`: absent; malformed
/// (`Some(None)`: the default); or the level's state.
pub(crate) fn read(q: &str, level: Level) -> Option<Option<Kept>> {
    match said(q, level.key()) {
        Said::Absent => None,
        Said::Malformed => Some(None),
        Said::Text(t) => Some(kept_of(&t, &pairs(q), level)),
    }
}

/// A WAVE-12 link's board: its keys (the lens's own aside) as the wave
/// board's state — `None` when it names none (`Some(None)`: malformed, the
/// default). Its Shortcode, Status, Kind and Severity are the language's
/// groups, in that order; the rest stay local.
pub(crate) fn read_wave_12(q: &str) -> Option<Option<Kept>> {
    let ps: Vec<Pair> = pairs(q).into_iter().filter(|p| !matches!(p.key.as_str(), "g" | "n" | "f")).collect();
    if !ps.iter().any(|p| WAVE_12_KEYS.contains(&p.key.as_str())) {
        return None;
    }
    Some(Clauses::parse_query(&join(&ps)).map(|cs| {
        let mut k = Kept { filter: Filter::default(), local: cs.local() };
        for (c, cl) in cs.server().iter() {
            k.set_clause(c, Some(cl.clone()));
        }
        k
    }))
}

/// FILTER-21: the nav a viewer who chose nothing sees — menger's own
/// documents (each shortcode's board note, `_MENGER/…`) left out: `not
/// menger doc`, the Menger docs pill at `no`. Choosing every ticket is the
/// empty filter, kept as `n=` like any choice.
pub(crate) fn nav_default() -> Kept {
    let mut k = Kept::default();
    k.set(Cat::MengerDocs, Op::Is, ["no".to_string()]);
    k
}

/// FILTER-26: the nav's own clauses — its groups, less each top-level
/// group its default holds (`not menger doc`, FILTER-21): what the reader
/// added, which is what Make global moves. A group the reader changed
/// (`menger doc`, the pill at `yes`) is the reader's, and moves.
pub(crate) fn nav_added(nav: &Filter, default: &Filter) -> Vec<Group> {
    nav.groups.iter().filter(|g| !default.groups.contains(g)).cloned().collect()
}

/// FILTER-26: whether the nav offers Make global — it holds a clause the
/// reader added. A nav at its default offers none, nor does one whose only
/// change is that the default's clause was taken out (nothing would move).
pub(crate) fn promotable(nav: &Filter, default: &Filter) -> bool {
    !nav_added(nav, default).is_empty()
}

/// FILTER-26: the global filter Make global leaves — `global and nav`, less
/// the nav's default: the global's groups then the nav's added ones, merged
/// as the lens merges a surface's (FILTER-10's `and`: a duplicate dropped,
/// one field's lists merged; a clash kept for [`clash_of`] to name). Each
/// side's or-groups (FILTER-31) carry over whole. The global's local
/// clauses and display stay; the nav's display (its tree or flat list) is
/// the nav's own and does not move.
pub(crate) fn promoted(global: &Kept, nav: &Filter, default: &Filter) -> Kept {
    let added = Filter { groups: nav_added(nav, default), display: Default::default() };
    let mut filter = global.filter.and(&added);
    filter.display = global.filter.display.clone();
    Kept { filter, local: global.local.clone() }
}

/// The wave board a route opens on (WAVE-12): the route's tree, Kind one
/// of build, spike, gap — `under SC and kind in (build, spike, gap)`.
pub(crate) fn waves_default(sc: &str, tree: &ScTree) -> Kept {
    let _ = tree;
    let mut k = Kept::default();
    // FILTER-41: the route's tree is `under` it — one clause, so the
    // board's default reads as three groups joined by or, not a group per
    // shortcode and kind.
    k.filter.groups.push(Group::one(Predicate::is(Field::Under(sc.to_ascii_uppercase()))));
    k.set(Cat::Kind, Op::Is, DEFAULT_KINDS.iter().map(|k| k.to_string()));
    k
}

/// The pills a bar draws, each with the level it is of: a surface's bar
/// shows the global pills first (inherited — muted, not editable there),
/// then its own; the global bar, its own.
pub(crate) fn bar_pills(bar: Level, global: Vec<Pill>, own: Vec<Pill>) -> Vec<(Level, Pill)> {
    let inherited = if bar == Level::Global { Vec::new() } else { global };
    inherited.into_iter().map(|p| (Level::Global, p)).chain(own.into_iter().map(|p| (bar, p))).collect()
}

/// The levels a group of `level`'s effective filter comes from: global,
/// the level, or — merged by `and` from both — each.
pub(crate) fn origins(g: &Group, global: &Filter, own: &Filter, level: Level) -> Vec<Level> {
    let mut out = Vec::new();
    if global.groups.contains(g) {
        out.push(Level::Global);
    }
    if level != Level::Global && own.groups.contains(g) {
        out.push(level);
    }
    if out.is_empty() {
        out = if level == Level::Global { vec![Level::Global] } else { vec![Level::Global, level] };
    }
    out
}

/// The clash `level`'s effective filter is in, if it admits nothing: each
/// clashing group's canonical text with the levels it comes from.
pub(crate) fn clash_of(global: &Filter, own: &Filter, level: Level) -> Option<Vec<(String, Vec<Level>)>> {
    let eff = if level == Level::Global { global.clone() } else { global.and(own) };
    let c = eff.admits_nothing()?;
    Some(c.groups.iter().map(|g| (g.format(), origins(g, global, own, level))).collect())
}

// ---------------------------------------------------------------------
// Local storage — every read and write allowed to fail; unavailable
// storage is "nothing stored".

fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

fn load_stored(key: &str) -> Option<String> {
    storage().and_then(|s| s.get_item(key).ok().flatten())
}

fn store(key: &str, value: Option<&str>) {
    if let Some(s) = storage() {
        let _ = match value {
            Some(v) => s.set_item(key, v),
            None => s.remove_item(key),
        };
    }
}

/// FILTER-26: what the global and nav levels were chosen as before Make
/// global (`None`: the level's default) — what its undo puts back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Promotion {
    pub(crate) global: Option<Kept>,
    pub(crate) nav: Option<Kept>,
}

// ---------------------------------------------------------------------
// The lens

/// The lens — every level's filter, its default, its menu — for the app's
/// lifetime; every handle `Copy`. [`use_lens`] finds it.
#[derive(Clone, Copy)]
pub(crate) struct Lens {
    /// What the reader chose, per level — `None`: the level's default.
    chosen: [RwSignal<Option<Kept>>; 5],
    defaults: [Memo<Option<Kept>>; 5],
    /// The route's shortcode, which the wave board's filter is kept under.
    sc: RwSignal<Option<String>>,
    /// The route's own surface.
    surface: RwSignal<Option<Level>>,
    /// FILTER-40: the viewport is the phone's (APP-50's ≤ 820 px): the
    /// panel lives in the phone bar's filter sheet.
    phone: RwSignal<bool>,
    sctree: Memo<Option<ScTree>>,
    pops: [Pop; 5],
    /// The wave board's state is a WAVE-12 link's, not yet changed: its URL
    /// is left as it was until the first change.
    legacy: RwSignal<bool>,
    /// Each level's filter (the empty filter while its default is not
    /// known), its effective filter (`global ∧ own`), and the clash that
    /// admits nothing.
    filters: [Memo<Filter>; 5],
    effective: [Memo<Filter>; 5],
    clashes: [Memo<Option<Vec<(String, Vec<Level>)>>>; 5],
    /// ORG-4: the organization the reader chose (the URL's `o=`, else
    /// storage), and what it means among those held ([`crate::orgs`]):
    /// every effective filter reads within it.
    org_choice: RwSignal<Option<String>>,
    org: Memo<Option<String>>,
    /// FILTER-39: per level, the groups last edited (kept while the filter
    /// is still what they say: a draft group), and the groups the panel
    /// draws — `None` for a filter deeper than or-of-and.
    drafts: [RwSignal<Option<Ors>>; 5],
    drawn: [Memo<Option<Ors>>; 5],
    /// FILTER-37: moves whenever a level's filter is changed from outside
    /// its bar (a link's URL, Make global and its undo, reset) — naming
    /// saves lets go of the name then (src/saved.rs).
    stamps: [RwSignal<u32>; 5],
}

/// The app's lens.
pub(crate) fn use_lens() -> Lens {
    use_context::<Lens>().expect("the lens is provided by the App")
}

impl Lens {
    /// The lens, provided to every view under the caller; the groups give
    /// the Shortcode tree. Call [`Lens::follow`] with every hash the app
    /// routes to — before the route moves.
    pub(crate) fn new(tree: nav::Tree) -> Self {
        let sctree = Memo::new(move |_| tree.with_groups(|g| g.map(ScTree::of)));
        let sc = RwSignal::new(None::<String>);
        let empty = || Memo::new(|_| Some(Kept::default()));
        let waves = Memo::new(move |_| sc.with(|s| sctree.with(|t| match (s, t) {
            (Some(s), Some(t)) => Some(waves_default(s, t)),
            _ => None,
        })));
        // The global and nav filters as this viewer left them; the URL's,
        // when it says one, wins ([`Lens::follow`]).
        let stored = |level: Level| load_stored(&storage_key(level, None)).and_then(|s| read(&s, level)).flatten();
        let chosen: [RwSignal<Option<Kept>>; 5] = std::array::from_fn(|i| {
            RwSignal::new(match Level::ALL[i] {
                l @ (Level::Global | Level::Nav) => stored(l),
                _ => None,
            })
        });
        let nav = Memo::new(|_| Some(nav_default()));
        let defaults = [empty(), nav, empty(), waves, empty()];
        let filters: [Memo<Filter>; 5] = std::array::from_fn(|i| {
            Memo::new(move |_| {
                chosen[i].with(|c| match c {
                    Some(k) => k.filter.clone(),
                    None => defaults[i].with(|d| d.as_ref().map(|d| d.filter.clone()).unwrap_or_default()),
                })
            })
        });
        let g = Level::Global.index();
        // ORG-4: the organization every page reads within.
        let org_choice = RwSignal::new(load_stored(crate::orgs::STORAGE_KEY));
        let held = crate::orgs::held();
        let org = Memo::new(move |_| held.with(|h| crate::orgs::chosen_of(org_choice.get().as_deref(), h)));
        let effective: [Memo<Filter>; 5] = std::array::from_fn(|i| {
            Memo::new(move |_| {
                let f = if i == g { filters[g].get() } else { filters[g].with(|gf| filters[i].with(|own| gf.and(own))) };
                org.with(|o| crate::orgs::within(o.as_deref(), &f))
            })
        });
        let clashes = Level::ALL.map(|level| {
            Memo::new(move |_| filters[g].with(|gf| filters[level.index()].with(|own| clash_of(gf, own, level))))
        });
        let drafts: [RwSignal<Option<Ors>>; 5] = std::array::from_fn(|_| RwSignal::new(None));
        let drawn: [Memo<Option<Ors>>; 5] = std::array::from_fn(|i| {
            Memo::new(move |_| {
                filters[i].with(|f| {
                    drafts[i].with(|d| match d {
                        Some(o) if o.to_groups().is_ok_and(|g| g == f.groups) => Some(o.clone()),
                        _ => Ors::of_filter(f),
                    })
                })
            })
        });
        let lens = Lens {
            chosen,
            defaults,
            sc,
            surface: RwSignal::new(None),
            phone: RwSignal::new(crate::phonebar::is_phone(dom::window().inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1440.0))),
            sctree,
            pops: Level::ALL.map(Pop::new),
            legacy: RwSignal::new(false),
            filters,
            effective,
            clashes,
            org_choice,
            org,
            drafts,
            drawn,
            stamps: std::array::from_fn(|_| RwSignal::new(0)),
        };
        // A default that moves (the groups came in) onto what was chosen
        // makes the choice the default.
        for level in Level::ALL {
            Effect::new(move |_| {
                let Some(d) = lens.defaults[level.index()].get() else { return };
                if lens.chosen[level.index()].with_untracked(|c| c.as_ref() == Some(&d)) {
                    lens.commit(level, None);
                }
            });
        }
        provide_context(lens);
        lens
    }

    /// The app routes to `hash`: the global and nav filters the URL says
    /// win (malformed, the default); one it does not say is carried into it.
    /// The route's surface takes its filter from `f=` (or a WAVE-12 link),
    /// else from storage, else its default.
    pub(crate) fn follow(self, hash: &str) {
        let q = query_of(hash).to_string();
        for level in [Level::Global, Level::Nav] {
            if let Some(k) = read(&q, level) {
                self.set_kept_from_outside(level, k);
            }
        }
        // ORG-4: the URL's organization, when it says one, wins; one it
        // does not say is carried into it.
        if let Said::Text(t) = said(&q, crate::orgs::KEY) {
            let t = Some(t).filter(|t| !t.is_empty());
            store(crate::orgs::STORAGE_KEY, t.as_deref());
            self.org_choice.set(t);
        }
        let route = crate::parse_route(hash);
        let surface = surface_of(&route);
        self.surface.set(surface.as_ref().map(|(l, _)| *l));
        let mut legacy = false;
        if let Some((level, sc)) = surface {
            if level == Level::Waves {
                self.sc.set(sc.clone());
            }
            let k = match read(&q, level) {
                Some(k) => k,
                None => match (level == Level::Waves).then(|| read_wave_12(&q)).flatten() {
                    Some(k) => {
                        legacy = k.is_some();
                        k
                    }
                    // FILTER-16: an INPUT-6 link's filter, written as `f=`
                    // at once (below).
                    None => match (level == Level::Inputs).then(|| crate::inputs_page::read_input_6(&q)).flatten() {
                        Some(k) => k,
                        None => load_stored(&storage_key(level, sc.as_deref())).and_then(|s| read(&s, level)).flatten(),
                    },
                },
            };
            self.set_kept_from_outside(level, k);
        }
        self.legacy.set(legacy);
        self.write_url();
    }

    /// `level`'s state set to `k` (`None`: its default) — normalized (the
    /// default is `None`), kept in storage; the URL rewritten when `url`.
    fn set_kept(self, level: Level, k: Option<Kept>, url: bool) {
        let d = self.defaults[level.index()].get_untracked();
        let k = k.filter(|k| d.as_ref() != Some(k));
        let key = storage_key(level, self.sc.get_untracked().as_deref());
        store(&key, k.as_ref().map(|k| join(&pairs_of(level, k))).as_deref());
        self.chosen[level.index()].set(k);
        if url {
            self.write_url();
        }
    }

    /// FILTER-37: `level`'s state set from outside its bar — the stamp moves
    /// when that changed what it keeps.
    fn set_kept_from_outside(self, level: Level, k: Option<Kept>) {
        let before = untrack(|| self.kept(level));
        self.set_kept(level, k, false);
        if untrack(|| self.kept(level)) != before {
            self.stamps[level.index()].update(|n| *n = n.wrapping_add(1));
        }
    }

    /// FILTER-37: the stamp of `level`'s changes from outside its bar.
    /// Tracked.
    pub(crate) fn stamp(self, level: Level) -> u32 {
        self.stamps[level.index()].get()
    }

    fn bump(self, level: Level) {
        self.stamps[level.index()].update(|n| *n = n.wrapping_add(1));
    }

    /// FILTER-37: a whole filter applied to `level` from outside its bar (a
    /// saved filter chosen, `g n`, the palette's filter) — committed, and
    /// the stamp moved, so a name the bar wore lets go.
    pub(crate) fn apply(self, level: Level, k: Option<Kept>) {
        self.commit(level, k);
        self.bump(level);
    }

    /// `level`'s state committed: stored, and written into the URL.
    pub(crate) fn commit(self, level: Level, k: Option<Kept>) {
        if level == Level::Waves {
            self.legacy.set(false);
        }
        self.set_kept(level, k, true);
    }

    /// The URL's query rewritten to the lens's state — `replaceState`, so no
    /// history entry per change and no `hashchange`.
    fn write_url(self) {
        let win = dom::window();
        let hash = win.location().hash().unwrap_or_default();
        let text = |l: Level| self.chosen[l.index()].with_untracked(|c| c.as_ref().map(|k| k.filter.format()));
        let surface = self.surface.get_untracked().filter(|l| !(*l == Level::Waves && self.legacy.get_untracked()));
        let (f, local) = match surface.and_then(|l| self.chosen[l.index()].get_untracked()) {
            Some(k) => (Some(k.filter.format()), k.local.to_pairs()),
            None => (None, Vec::new()),
        };
        let part = surface.map(|l| (f.as_deref(), local.as_slice(), owned_keys(Some(l))));
        let next = lens_hash(&hash, text(Level::Global).as_deref(), text(Level::Nav).as_deref(), part);
        let next = write_key(&next, crate::orgs::KEY, self.org_choice.get_untracked().as_deref());
        replace(&win, &hash, &next);
    }

    /// ORG-4: the organization every page reads within — `None`: every one
    /// the reader holds. Tracked.
    pub(crate) fn org(self) -> Memo<Option<String>> {
        self.org
    }

    /// ORG-4: the reader chose `org` (`None`: every one) — kept, and in the
    /// URL.
    pub(crate) fn set_org(self, org: Option<String>) {
        store(crate::orgs::STORAGE_KEY, org.as_deref());
        self.org_choice.set(org);
        self.write_url();
    }

    /// What `level` keeps — what was chosen, else its default (`None` until
    /// it is known: the wave board's, before the groups). Tracked.
    pub(crate) fn kept(self, level: Level) -> Option<Kept> {
        self.chosen[level.index()].get().or_else(|| self.defaults[level.index()].get())
    }

    /// `level`'s state changed by `f` (nothing, while it is not known).
    pub(crate) fn edit(self, level: Level, f: impl FnOnce(&mut Kept)) {
        let Some(mut next) = untrack(|| self.kept(level)) else { return };
        f(&mut next);
        self.commit(level, Some(next));
    }

    /// FILTER-39: `level`'s filter as the panel draws it — groups joined by
    /// `or` — `None` while it is deeper than that (or not known). Tracked.
    pub(crate) fn ors(self, level: Level) -> Option<Ors> {
        self.drawn[level.index()].get()
    }

    /// FILTER-39: `level`'s groups changed by one of src/ors.rs's edits and
    /// committed as its filter (its local clauses and display kept), the
    /// groups kept as the draft the panel goes on drawing; what the
    /// language refuses is refused, nothing committed.
    pub(crate) fn edit_ors<T>(self, level: Level, f: impl FnOnce(&mut Ors) -> Result<T, String>) -> Result<T, String> {
        let Some(mut next) = untrack(|| self.kept(level)) else { return Err("the filter is not known yet".into()) };
        let Some(mut o) = untrack(|| self.ors(level)) else {
            return Err("this filter nests deeper than groups joined by or \u{2014} edit it as text".into());
        };
        let out = f(&mut o)?;
        next.filter.groups = o.to_groups()?;
        self.drafts[level.index()].set(Some(o));
        self.commit(level, Some(next));
        Ok(out)
    }

    /// FILTER-39: what `level`'s surface would show were its groups changed
    /// by `f` — the effective filter's wire text (`global ∧ own`, in the
    /// chosen organization); `None` when it cannot be said.
    pub(crate) fn would_be(self, level: Level, f: impl FnOnce(&mut Ors) -> Result<(), String>) -> Option<String> {
        let mut o = untrack(|| self.ors(level))?;
        f(&mut o).ok()?;
        let own = Filter { groups: o.to_groups().ok()?, ..Filter::default() };
        let eff = match level {
            Level::Global => own,
            _ => untrack(|| self.filters[Level::Global.index()].with(|g| g.and(&own))),
        };
        let eff = untrack(|| self.org.with(|org| crate::orgs::within(org.as_deref(), &eff)));
        Some(crate::counts::wire_text(&eff))
    }

    /// FILTER-26: Make global — the global filter becomes `global and nav`
    /// less the nav's default ([`promoted`]) and the nav goes back to its
    /// default. Answers what both were chosen as before, for the one undo
    /// ([`Lens::restore`]); `None` (nothing done) while the nav has nothing
    /// to move.
    pub(crate) fn promote(self) -> Option<Promotion> {
        let nav = untrack(|| self.kept(Level::Nav))?;
        let default = nav_default().filter;
        if !promotable(&nav.filter, &default) {
            return None;
        }
        let before = Promotion { global: self.chosen[Level::Global.index()].get_untracked(), nav: self.chosen[Level::Nav.index()].get_untracked() };
        let global = untrack(|| self.kept(Level::Global)).unwrap_or_default();
        let next = promoted(&global, &nav.filter, &default);
        self.set_kept(Level::Global, Some(next), false);
        self.set_kept(Level::Nav, None, true);
        self.bump(Level::Global);
        self.bump(Level::Nav);
        Some(before)
    }

    /// FILTER-26: the global and nav filters put back as `p` says — Make
    /// global's undo.
    pub(crate) fn restore(self, p: Promotion) {
        self.set_kept(Level::Global, p.global, false);
        self.set_kept(Level::Nav, p.nav, true);
        self.bump(Level::Global);
        self.bump(Level::Nav);
    }

    /// FILTER-26: whether the nav offers Make global ([`promotable`]).
    /// Tracked.
    pub(crate) fn can_promote(self) -> bool {
        self.filters[Level::Nav.index()].with(|f| promotable(f, &nav_default().filter))
    }

    /// Back to the level's default.
    pub(crate) fn reset(self, level: Level) {
        self.commit(level, None);
        self.bump(level);
    }

    /// Whether `level` differs from its default. Tracked.
    pub(crate) fn differs(self, level: Level) -> bool {
        self.chosen[level.index()].with(Option::is_some)
    }


    /// `level`'s clauses, one per category (the menu's checked boxes).
    /// Tracked.
    pub(crate) fn clauses(self, level: Level) -> Clauses {
        self.kept(level).map(|k| k.clauses()).unwrap_or_default()
    }

    /// FILTER-41: how many clauses `level` holds — its line's Filter badge:
    /// every clause of its groups (a filter deeper than or-of-and, its
    /// top-level groups), and its surface's own. Tracked.
    pub(crate) fn clause_count(self, level: Level) -> usize {
        let own = self.ors(level).map(|o| o.clauses()).unwrap_or_else(|| self.filter(level).with(|f| f.groups.len()));
        own + self.kept(level).map_or(0, |k| k.local.iter().count())
    }

    /// How many groups `level` holds — the header button's badge. Tracked.
    pub(crate) fn group_count(self, level: Level) -> usize {
        self.kept(level).map_or(0, |k| k.len())
    }

    /// `level`'s filter as canonical text.
    pub(crate) fn text(self, level: Level) -> String {
        untrack(|| self.kept(level)).map(|k| k.filter.format()).unwrap_or_default()
    }

    /// `level`'s filter from text (its local clauses kept) — or every bad
    /// chunk, each with why, and nothing applied.
    pub(crate) fn apply_text(self, level: Level, text: &str) -> Result<(), Vec<(String, String)>> {
        match Filter::parse(text) {
            Ok(filter) => {
                let local = untrack(|| self.kept(level)).map(|k| k.local).unwrap_or_default();
                self.commit(level, Some(Kept { filter, local }));
                Ok(())
            }
            Err(e) => Err(e.chunks.into_iter().map(|c| (c.text, c.why)).collect()),
        }
    }

    /// `level`'s filter (the empty filter while its default is not known).
    #[allow(dead_code)] // FILTER-12, FILTER-14, FILTER-16
    pub(crate) fn filter(self, level: Level) -> Signal<Filter> {
        self.filters[level.index()].into()
    }

    /// `level`'s filter set — its local clauses kept.
    #[allow(dead_code)] // FILTER-12, FILTER-14, FILTER-16
    pub(crate) fn set(self, level: Level, filter: Filter) {
        let local = untrack(|| self.kept(level)).map(|k| k.local).unwrap_or_default();
        self.commit(level, Some(Kept { filter, local }));
    }

    /// What a surface at `level` shows: `global.and(own)`, merged (FILTER-10);
    /// the global level's own is the global filter.
    pub(crate) fn effective(self, level: Level) -> Signal<Filter> {
        self.effective[level.index()].into()
    }

    /// The groups that clash when `level`'s effective filter admits nothing,
    /// each with the levels it comes from. Tracked.
    pub(crate) fn clash(self, level: Level) -> Option<Vec<(String, Vec<Level>)>> {
        self.clashes[level.index()].get()
    }

    /// The Shortcode tree, from the groups.
    pub(crate) fn sctree(self) -> Memo<Option<ScTree>> {
        self.sctree
    }

    /// The route's shortcode, for `level`'s Shortcode menu (its tree first):
    /// the wave board's.
    pub(crate) fn route_sc(self, level: Level) -> Option<String> {
        (level == Level::Waves).then(|| self.sc.get()).flatten()
    }

    /// The route's own surface.
    pub(crate) fn surface(self) -> Option<Level> {
        self.surface.get_untracked()
    }

    /// FILTER-40: the level whose panel is open — one at a time. Tracked.
    pub(crate) fn open_level(self) -> Option<Level> {
        Level::ALL.into_iter().find(|l| self.pop(*l).open_tracked())
    }

    /// FILTER-40: where the one panel is drawn ([`crate::filter::host_of`]).
    /// Tracked.
    pub(crate) fn host(self) -> crate::filter::Host {
        let line = self.surface.get().is_some_and(|l| self.pop(l).mounted_tracked());
        crate::filter::host_of(self.phone.get(), line)
    }

    /// FILTER-40: the levels the panel's tabs offer — everywhere; the nav,
    /// where it is drawn; the route's surface, where its line is. Tracked.
    pub(crate) fn tabs(self) -> Vec<Level> {
        let mut out = vec![Level::Global];
        if self.pop(Level::Nav).mounted_tracked() {
            out.push(Level::Nav);
        }
        if let Some(l) = self.surface.get().filter(|l| self.pop(*l).mounted_tracked()) {
            out.push(l);
        }
        out
    }

    pub(crate) fn pop(self, level: Level) -> Pop {
        self.pops[level.index()]
    }

    /// FILTER-39: `level`'s panel opened in its page's flow — on `c`'s
    /// section when given (scrolled to and lit) — every other level's
    /// folded.
    pub(crate) fn open_panel(self, level: Level, c: Option<Cat>) {
        // FILTER-40: a tab changes which level the one panel edits; the
        // focus it was opened from stays where it was.
        let was = Level::ALL.into_iter().find(|l| self.pop(*l).is_open());
        for l in Level::ALL.into_iter().filter(|l| *l != level) {
            self.pop(l).hand_over();
        }
        self.pop(level).show_from(c, was.is_some());
        // On a phone the panel is the filter sheet's: raise it.
        if self.phone.get_untracked() {
            crate::phonebar::open(Some(crate::phonebar::Place::Filter));
        }
    }

    /// FILTER-40: the open panel folded, whichever level it edits.
    pub(crate) fn close_panel(self, restore: bool) {
        for l in Level::ALL {
            self.pop(l).close(restore);
        }
    }

    /// The keys, for the app's lifetime: `F` opens or folds the global
    /// panel anywhere unless typing (or in a dialog, a menu, an overlay);
    /// `f` the route's surface's, where its bar is drawn (FILTER-39: the
    /// panel is in the page's flow — a click elsewhere leaves it open).
    pub(crate) fn install(self) {
        window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
            // AGS-28: `f` and `F` are the table's `filter.view` and
            // `filter.global`.
            let global = bindings::is(&ev, bindings::Context::Global, "filter.global");
            if ev.default_prevented() || !(global || bindings::is(&ev, bindings::Context::Global, "filter.view")) {
                return;
            }
            let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
            if target.as_ref().is_some_and(keys::defer_to_focus) {
                return;
            }
            if dom::window().document().and_then(|d| d.query_selector(".overlay").ok().flatten()).is_some() {
                return;
            }
            let level = if global {
                Level::Global
            } else {
                match self.surface().filter(|l| self.pop(*l).is_mounted()) {
                    Some(l) => l,
                    None => return,
                }
            };
            ev.prevent_default();
            if self.pop(level).is_open() {
                self.close_panel(true);
            } else {
                self.open_panel(level, None);
            }
        });
        // FILTER-40: the panel moves into the phone bar's sheet at phone
        // widths, and back.
        window_event_listener(leptos::ev::resize, move |_| {
            let phone = crate::phonebar::is_phone(dom::window().inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1440.0));
            if self.phone.get_untracked() != phone {
                self.phone.set(phone);
            }
        });
    }
}

fn replace(win: &web_sys::Window, hash: &str, next: &str) {
    if next != hash {
        if let Ok(history) = win.history() {
            let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(next));
        }
    }
}

// ---------------------------------------------------------------------
// Under the header: the panel where no surface's line holds it

/// FILTER-40: under the header — the one panel while it is drawn here (a
/// page with no surface line of its own, a ticket's; at phone widths
/// always: this is the filter sheet's), the global filter's contradiction,
/// and — FILTER-16 — on a route the filter does not reach (`applies`
/// false: the components and settings pages), while it holds anything,
/// that it does not apply there. The GLOBAL row is gone: the global filter
/// is a dashed chip on every surface and the panel's ⌂ everywhere tab.
#[component]
pub(crate) fn LensBar(#[prop(into)] applies: Signal<bool>) -> impl IntoView {
    let lens = use_lens();
    // FILTER-12: the global panel's counts are `facets { filter: global }`.
    lens.pop(Level::Global).configure(&Cat::SERVER, crate::counts::provider(Level::Global));
    let na = Memo::new(move |_| !applies.get() && lens.differs(Level::Global));
    view! {
        {move || na.get().then(|| view! {
            <div class="lens-na" role="note" title="this page shows no ticket">"The global filter doesn't apply here"</div>
        })}
        <div class="ftop" on:keydown=move |ev| {
            if let Some(l) = lens.open_level() {
                fold_key(lens.pop(l), ev)
            }
        }>
            <PanelHost host=Host::Top />
        </div>
        <Clash level=Level::Global />
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The levels: their names, their keys, which are a route's surface.
    #[test]
    fn the_levels_and_their_keys() {
        assert_eq!(Level::ALL.map(Level::name), ["global", "nav", "board", "waves", "inputs"]);
        assert_eq!(Level::ALL.map(Level::key), ["g", "n", "f", "f", "f"]);
        for l in Level::ALL {
            assert_eq!(Level::of_name(l.name()), Some(l), "FILTER-22: a surface names its level");
        }
        assert_eq!(surface_of(&Route::Board), Some((Level::Board, None)));
        assert_eq!(surface_of(&Route::Waves("plexus".into())), Some((Level::Waves, Some("PLEXUS".into()))));
        assert_eq!(surface_of(&Route::Waves(String::new())), None, "the boards' index has no filter");
        assert_eq!(surface_of(&Route::Inputs), Some((Level::Inputs, None)));
        assert_eq!(surface_of(&Route::Ticket("MNG-4".into())), None);
        assert_eq!(surface_of(&Route::Settings), None);
        assert_eq!(storage_key(Level::Global, None), "menger.lens.global");
        assert_eq!(storage_key(Level::Nav, None), "menger.lens.nav");
        assert_eq!(storage_key(Level::Waves, Some("plexus")), "menger.lens.waves.PLEXUS");
        assert_ne!(storage_key(Level::Waves, Some("PLEXUS")), storage_key(Level::Waves, Some("MNG")));
    }

    /// A level's text in the query: percent-encoded both ways; absent, there
    /// and empty, or malformed (a bad escape, the key twice).
    #[test]
    fn a_level_rides_the_query_percent_encoded() {
        let text = "status in (Ready, \"In Review\") and not in MNG";
        let hash = write_key("#/waves/PLEXUS", "g", Some(text));
        assert_eq!(hash, "#/waves/PLEXUS?g=status%20in%20%28Ready%2C%20%22In%20Review%22%29%20and%20not%20in%20MNG");
        assert_eq!(said(query_of(&hash), "g"), Said::Text(text.into()));
        assert_eq!(said(query_of(&hash), "n"), Said::Absent);
        assert_eq!(said("g=", "g"), Said::Text(String::new()), "there, and empty");
        assert_eq!(said("g", "g"), Said::Text(String::new()));
        assert_eq!(said("g=status+Ready", "g"), Said::Text("status Ready".into()), "+ is a space");
        assert_eq!(said("g=%zz", "g"), Said::Malformed);
        assert_eq!(said("g=%E2%82", "g"), Said::Malformed);
        assert_eq!(said("g=a&g=b", "g"), Said::Malformed);
        for s in ["", "a b", "x & y = z?", "\"quoted\" · tree", "ünïcode"] {
            assert_eq!(dec(&enc(s)).as_deref(), Some(s));
        }
    }

    /// Setting a key keeps the rest of the query in its order, puts `g`,
    /// `n`, `f` first, and replaces a key in place; `None` takes it out.
    #[test]
    fn one_key_is_written_and_the_rest_kept() {
        let h = "#/inputs?sc=INPUT&state=open&removed";
        let h = write_key(h, "f", Some("ready"));
        assert_eq!(h, "#/inputs?f=ready&sc=INPUT&state=open&removed");
        let h = write_key(&h, "g", Some("kind build"));
        assert_eq!(h, "#/inputs?g=kind%20build&f=ready&sc=INPUT&state=open&removed");
        let h = write_key(&h, "n", Some("in MNG"));
        assert_eq!(h, "#/inputs?g=kind%20build&n=in%20MNG&f=ready&sc=INPUT&state=open&removed");
        let h = write_key(&h, "g", Some("blocked"));
        assert_eq!(h, "#/inputs?g=blocked&n=in%20MNG&f=ready&sc=INPUT&state=open&removed");
        let h = write_key(&h, "f", None);
        assert_eq!(h, "#/inputs?g=blocked&n=in%20MNG&sc=INPUT&state=open&removed");
        let h = write_key(&write_key(&h, "g", None), "n", None);
        assert_eq!(h, "#/inputs?sc=INPUT&state=open&removed");
        assert_eq!(write_key("#/", "g", None), "#/");
        assert_eq!(write_key("#/?g=x", "g", None), "#/");
    }

    /// The lens's whole part written at once: its keys, the surface's local
    /// pairs, then the page's keys — the owned (legacy) keys dropped.
    #[test]
    fn the_lens_part_is_written_whole() {
        let local = [Pair::new("wave", Some("is:1,2".into())), Pair::new("q", Some("not:x".into()))];
        let legacy = ["sc", "status", "kind", "sev", "wave", "ready", "off", "q", "all"];
        let h = write("#/waves/PLEXUS?sc=is:RETURNS&status=not:Complete&q=old", Some("ready"), None, Some("status Ready"), &local, &legacy);
        assert_eq!(h, "#/waves/PLEXUS?g=ready&f=status%20Ready&wave=is:1,2&q=not:x");
        assert_eq!(write("#/waves/PLEXUS?all", None, None, None, &[], &legacy), "#/waves/PLEXUS");
        assert_eq!(write("#/waves/PLEXUS", None, None, Some(""), &[], &legacy), "#/waves/PLEXUS?f=", "the empty filter is f=");
        assert_eq!(write("#/inputs?sc=A&g=old", Some("new"), None, None, &[], &[]), "#/inputs?g=new&sc=A", "the page's keys stay");
    }

    /// A navigation carries the global and nav filters: the new route's
    /// hash gets the levels' `g` and `n` (the URL's own value was read into
    /// them first — it wins); the defaults add nothing; a surface's part is
    /// written, or left as it was.
    #[test]
    fn a_navigation_carries_g_and_n() {
        let carried = |h: &str, g: Option<&str>, n: Option<&str>| lens_hash(h, g, n, None);
        assert_eq!(carried("#/ticket/MNG-4", Some("status Ready"), None), "#/ticket/MNG-4?g=status%20Ready");
        assert_eq!(carried("#/board", Some("ready"), Some("in MNG")), "#/board?g=ready&n=in%20MNG");
        assert_eq!(carried("#/waves/PLEXUS?f=kind%20gap", Some("ready"), None), "#/waves/PLEXUS?g=ready&f=kind%20gap");
        assert_eq!(carried("#/?g=blocked", Some("blocked"), None), "#/?g=blocked", "the URL's own, read in, kept");
        assert_eq!(carried("#/?g=blocked", None, None), "#/", "cleared: no value");
        assert_eq!(carried("#/inputs?sc=A", None, None), "#/inputs?sc=A", "the defaults add nothing");
        assert_eq!(carried("#/inputs?sc=A", Some("ready"), None), "#/inputs?g=ready&sc=A", "the page's keys kept");
        // A WAVE-12 link not yet changed is left as it is, g carried into it.
        assert_eq!(carried("#/waves/PLEXUS?sc=is:RETURNS", Some("ready"), None), "#/waves/PLEXUS?g=ready&sc=is:RETURNS");
        // A surface's part written: its f= and local pairs, WAVE-12's keys gone.
        let local = [Pair::new("wave", Some("is:1".into()))];
        assert_eq!(
            lens_hash("#/waves/PLEXUS?sc=is:RETURNS", Some("ready"), None, Some((Some("in RETURNS"), &local, &["sc", "wave"]))),
            "#/waves/PLEXUS?g=ready&f=in%20RETURNS&wave=is:1"
        );
        assert_eq!(lens_hash("#/waves/PLEXUS?f=x&wave=is:1", None, None, Some((None, &[], &["wave"]))), "#/waves/PLEXUS");
    }

    /// FILTER-16: the lens applies on every route but the components and
    /// settings pages.
    #[test]
    fn the_filter_applies_where_tickets_are_shown() {
        for r in [Route::Board, Route::Ticket("MNG-4".into()), Route::Waves("PLEXUS".into()), Route::Waves(String::new()), Route::Inputs] {
            assert!(applies(&r), "{r:?}");
        }
        for r in [Route::Components(None), Route::Components(Some("m-stat".into())), Route::Settings, Route::Comments, Route::Deployments(None)] {
            assert!(!applies(&r), "{r:?}");
        }
    }

    /// FILTER-16: the inputs page's level — its local categories ride
    /// beside `f=` under their keys, both ways; INPUT-6's keys are dropped
    /// when the lens writes its part.
    #[test]
    fn the_inputs_level_rides_the_query_with_its_own_categories() {
        assert_eq!(locals(Level::Inputs), [Cat::State, Cat::Removed]);
        let mut k = Kept::default();
        k.set(Cat::Shortcode, Op::Is, ["INPUT".to_string(), "MNG".to_string()]);
        k.set(Cat::Kind, Op::Is, ["build".to_string()]);
        k.set(Cat::State, Op::Is, ["answered".to_string(), "open".to_string()]);
        k.set(Cat::Removed, Op::Is, ["yes".to_string()]);
        assert_eq!(k.filter.format(), "in (INPUT, MNG) and kind build");
        let q = join(&pairs_of(Level::Inputs, &k));
        assert_eq!(q, "f=in%20%28INPUT%2C%20MNG%29%20and%20kind%20build&state=is:open,answered&removed=is:yes");
        assert_eq!(read(&q, Level::Inputs), Some(Some(k.clone())), "round trip");
        assert_eq!(read("f=&state=is:closed", Level::Inputs).flatten().map(|k| k.local.get(Cat::State).cloned()).flatten().map(|c| c.values), Some(vec!["closed".to_string()]));
        assert_eq!(read("f=kind%20bogus", Level::Inputs), Some(None), "malformed: the default");
        assert_eq!(read("f=&removed", Level::Inputs), Some(None), "a local key with no value is malformed");
        // The wave board's local keys are not the inputs page's.
        assert_eq!(read("f=&wave=is:1", Level::Inputs), Some(Some(Kept::default())));
        // The lens's part written: INPUT-6's keys go, f= and the local pairs come.
        let local = k.local.to_pairs();
        let f = k.filter.format();
        assert_eq!(
            lens_hash("#/inputs?g=ready&sc=INPUT,MNG&state=open,answered&removed", Some("ready"), None, Some((Some(&f), &local, &INPUT_6_KEYS))),
            "#/inputs?g=ready&f=in%20%28INPUT%2C%20MNG%29%20and%20kind%20build&state=is:open,answered&removed=is:yes"
        );
        assert_eq!(lens_hash("#/inputs?sc=A&removed", None, None, Some((None, &[], &INPUT_6_KEYS))), "#/inputs", "the default: no query");
    }

    /// The hash's query, read and replaced.
    #[test]
    fn the_hash_carries_the_query() {
        assert_eq!(query_of("#/waves/PLEXUS?sc=is:A&q=x"), "sc=is:A&q=x");
        assert_eq!(query_of("#/waves/PLEXUS"), "");
        assert_eq!(path_of("#/waves/PLEXUS?x"), "#/waves/PLEXUS");
        assert_eq!(with_query("#/waves/PLEXUS?sc=is:A", "q=x"), "#/waves/PLEXUS?q=x");
        assert_eq!(with_query("#/waves/PLEXUS?sc=is:A", ""), "#/waves/PLEXUS");
        assert_eq!(with_query("#/waves/plexus", "all"), "#/waves/plexus?all");
        assert_eq!(pairs("?a=1&&b&c="), vec![Pair::new("a", Some("1".into())), Pair::new("b", None), Pair::new("c", Some(String::new()))]);
        assert_eq!(join(&pairs("a=1&b&c=")), "a=1&b&c=");
    }

    use crate::filter::tests::{platform, strs, PLEXUS_TREE};

    fn parsed(text: &str) -> Filter {
        Filter::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    fn clause(op: Op, v: &[&str]) -> Clause {
        Clause { op, values: strs(v) }
    }

    /// Acceptance 1: each category's pill is its group, both ways — both
    /// operators, one value and many; a flag's yes and no.
    #[test]
    fn each_categorys_pill_is_its_group() {
        use Op::{Is, Not};
        let cases: [(Cat, Clause, &str); 27] = [
            (Cat::Shortcode, clause(Is, &["MNG"]), "in MNG"),
            (Cat::Shortcode, clause(Is, &["CONTEXT", "RETURNS"]), "in (CONTEXT, RETURNS)"),
            (Cat::Shortcode, clause(Not, &["CONTEXT", "RETURNS"]), "not in (CONTEXT, RETURNS)"),
            (Cat::Shortcode, clause(Not, &["MNG"]), "not in MNG"),
            (Cat::Status, clause(Is, &["Ready"]), "status Ready"),
            (Cat::Status, clause(Is, &["Coding", "In Review"]), "status in (Coding, In Review)"),
            (Cat::Status, clause(Not, &["Complete", "Superseded"]), "not status in (Complete, Superseded)"),
            (Cat::Status, clause(Not, &["Doc"]), "not status Doc"),
            (Cat::Kind, clause(Is, &["build", "spike", "gap"]), "kind in (build, spike, gap)"),
            (Cat::Kind, clause(Not, &["doc"]), "not kind doc"),
            (Cat::Severity, clause(Is, &["high", "none"]), "severity in (high, none)"),
            (Cat::Severity, clause(Not, &["none"]), "not severity none"),
            (Cat::Severity, clause(Is, &["critical"]), "severity critical"),
            (Cat::Ready, clause(Is, &["yes"]), "ready"),
            (Cat::Ready, clause(Is, &["no"]), "not ready"),
            (Cat::Blocked, clause(Is, &["yes"]), "blocked"),
            (Cat::Blocked, clause(Is, &["no"]), "not blocked"),
            (Cat::Commits, clause(Is, &["yes"]), "has commits"),
            (Cat::Commits, clause(Is, &["no"]), "not has commits"),
            (Cat::MengerDocs, clause(Is, &["yes"]), "menger doc"),
            (Cat::MengerDocs, clause(Is, &["no"]), "not menger doc"),
            (Cat::Asking, clause(Is, &["yes"]), "asking"),
            (Cat::Asking, clause(Is, &["no"]), "not asking"),
            (Cat::Label, clause(Is, &["needs-you"]), "label needs-you"),
            (Cat::Label, clause(Not, &["a", "b"]), "not label in (a, b)"),
            (Cat::Status, clause(Is, &["In Review"]), "status \"In Review\""),
            (Cat::Kind, clause(Is, &["scope", "execution"]), "kind in (scope, execution)"),
        ];
        for (cat, cl, text) in cases {
            let g = group_of(cat, &cl).unwrap_or_else(|| panic!("{cat:?} {cl:?}"));
            let f = Filter { groups: vec![g.clone()], ..Filter::default() };
            // The canonical text is the language's; it parses back to the group.
            assert_eq!(parsed(&f.format()).groups, vec![g.clone()], "{text}");
            assert!(f.format() == text || parsed(text).groups == vec![g.clone()], "{} ≠ {text}", f.format());
            assert_eq!(clause_of(&g), Some((cat, cl.clone())), "{text}: the group draws its pill");
            assert_eq!(clause_of(&parsed(text).groups[0]), Some((cat, cl)), "{text}: typed, it is the pill too");
        }
        // A flag's operator turned over is its other value.
        let mut k = Kept::default();
        k.set(Cat::Ready, Op::Is, strs(&["yes"]));
        k.flip(Cat::Ready);
        assert_eq!(k.filter.format(), "not ready");
        // Typed in another order, a list is still its category's pill.
        assert_eq!(clause_of(&parsed("status in (Ready, Coding)").groups[0]), Some((Cat::Status, clause(Is, &["Coding", "Ready"]))));
        assert_eq!(field_of(Cat::Status, "Bogus"), None);
        assert_eq!(field_of(Cat::Wave, "1"), None, "a local category is no field");
    }

    /// FILTER-19: the menu's switch on a level's filter — `is not` turns a
    /// pill's group exactly as the pill's toggle does, and the pill and the
    /// canonical text follow; values checked under it are a new excluding
    /// group; a flag's is untouched.
    #[test]
    fn the_menus_switch_writes_the_not() {
        let mut k = Kept { filter: parsed("status in (Ready, Coding) and in MNG"), ..Kept::default() };
        let mut by_pill = k.clone();
        by_pill.flip(Cat::Status);
        k.set_op(Cat::Status, Op::Not);
        assert_eq!(k, by_pill, "the same clause a pill's toggle makes");
        assert_eq!(k.filter.format(), "not status in (Coding, Ready) and in MNG");
        assert_eq!(k.pills()[0], Pill::Clause { cat: Cat::Status, clause: clause(Op::Not, &["Coding", "Ready"]) });
        k.set_op(Cat::Status, Op::Is);
        assert_eq!(k.filter.format(), "status in (Coding, Ready) and in MNG");

        // Filter ▸ Kind, `is not`, doc and spike checked.
        k.edit_as(Cat::Kind, Op::Not, |k| k.toggle(Cat::Kind, "doc"));
        assert_eq!(k.filter.format(), "status in (Coding, Ready) and in MNG and not kind doc");
        k.edit_as(Cat::Kind, Op::Not, |k| k.toggle(Cat::Kind, "spike"));
        assert_eq!(k.filter.format(), "status in (Coding, Ready) and in MNG and not kind in (spike, doc)");
        assert_eq!(k.pills()[2], Pill::Clause { cat: Cat::Kind, clause: clause(Op::Not, &["spike", "doc"]) });
        assert_eq!(parsed(&k.filter.format()), k.filter, "the text parses back to the filter");

        // A shortcode's tree, excluded.
        let t = platform();
        let mut n = Kept::default();
        n.edit_as(Cat::Shortcode, Op::Not, |k| k.toggle_code(&t, "MACRO"));
        assert_eq!(n.filter.format(), "not in (CODEGEN, GEN, MACRO)");

        // A flag has no switch: its value says it.
        let mut r = Kept { filter: parsed("ready"), ..Kept::default() };
        r.set_op(Cat::Ready, Op::Not);
        assert_eq!(r.filter.format(), "ready");
    }

    /// FILTER-27 acceptance 3: every time clause of the language block, typed
    /// in the bar's text mode, comes back as a time pill — beside the
    /// categories' pills, in the filter's order — and the pills print the
    /// canonical text again: text ⇄ pills round-trips; × takes one out.
    #[test]
    fn time_clauses_are_pills_and_round_trip() {
        let forms = [
            "created today",
            "created within 3d",
            "created this week",
            "created before 2w ago",
            "created between 2026-10-01 and 2026-10-07",
            "entered Pending today",
            "left Pending this week",
            "entered Ready within 2d by owner",
            "status Pending for over 2w",
            "status \"In Review\" for over 3d",
            "category active for under 1h",
            "for over 2w",
            "changed before 1w ago",
            "category in (ready, active, review) for over 1w",
            "created between 2026-10-01T08:30 and 2026-10-03T17:00",
            "changed today",
        ];
        for form in forms {
            let text = format!("kind build and {form} and in MNG");
            let k = Kept { filter: parsed(&text), ..Kept::default() };
            let pills = k.pills();
            assert_eq!(pills.len(), 3, "{form}");
            assert!(matches!(&pills[0], Pill::Clause { cat: Cat::Kind, .. }), "{form}");
            let Pill::Time { at, t } = &pills[1] else { panic!("{form}: {:?}", pills[1]) };
            assert_eq!(*at, 1);
            // The pill back to its group, and the whole back to text.
            let mut back = k.clone();
            back.filter.groups[1] = crate::when::group(t);
            assert_eq!(back.filter.format(), parsed(&text).format(), "{form}");
            assert_eq!(k.clauses().get(Cat::When), None, "a time clause is no category's value");
        }
        let mut k = Kept { filter: parsed("created today and status Ready"), ..Kept::default() };
        k.remove_group(0);
        assert_eq!(k.filter.format(), "status Ready");
        // Two time clauses are two pills.
        let k = Kept { filter: parsed("created today and left Pending this week"), ..Kept::default() };
        assert!(matches!(k.pills().as_slice(), [Pill::Time { at: 0, .. }, Pill::Time { at: 1, .. }]));
        // FILTER-31: an `or` of time clauses is a group of time pills.
        let k = Kept { filter: parsed("(changed within 1h or changed yesterday)"), ..Kept::default() };
        let pills_of_k = k.pills();
        let [Pill::Group { at: 0, conj: Conj::Any, members }] = pills_of_k.as_slice() else { panic!("{:?}", k.pills()) };
        assert!(matches!(members.as_slice(), [crate::anyof::Member::Time(_), crate::anyof::Member::Time(_)]));
    }

    /// FILTER-36: a group holding a group, and a saved name, are custom
    /// pills of their canonical text — never a box nor a category's clause —
    /// and × takes them out; the unnested pills beside them are as before.
    #[test]
    fn a_nested_group_or_a_name_is_a_custom_pill() {
        let mut k = Kept {
            filter: parsed("status Ready and (blocked or (kind build and asking)) and saved \"Needs you\" and (ready or saved Plexus-Ready)"),
            ..Kept::default()
        };
        assert_eq!(
            k.pills(),
            vec![
                Pill::Clause { cat: Cat::Status, clause: clause(Op::Is, &["Ready"]) },
                Pill::Custom { text: "(blocked or (kind build and asking))".into(), at: 1 },
                Pill::Custom { text: "saved \"Needs you\"".into(), at: 2 },
                Pill::Custom { text: "(ready or saved Plexus-Ready)".into(), at: 3 },
            ]
        );
        k.remove_group(2);
        assert_eq!(k.filter.format(), "status Ready and (blocked or (kind build and asking)) and (ready or saved Plexus-Ready)");
        assert_eq!(Kept { filter: parsed("not saved Deployment"), ..Kept::default() }.pills(), vec![Pill::Custom { text: "not saved Deployment".into(), at: 0 }]);
    }

    /// Acceptance 1: a group no category draws is a custom pill showing its
    /// canonical text; × takes it out. A second group of one category is
    /// custom too: a category has one pill. FILTER-31: a group of pills the
    /// categories draw is a group pill, not custom.
    #[test]
    fn a_group_no_category_draws_is_a_custom_pill() {
        for text in [
            "(blocked or status Blocked)",
            "under PLEXUS-1",
            "category open",
            "text \"envelope\"",
            "(status Ready and kind build)",
            "(not status Ready or not status Coding)",
        ] {
            let g = &parsed(text).groups[0];
            assert_eq!(clause_of(g), None, "{text}");
        }
        let mut k = Kept { filter: parsed("status Ready and (blocked or status Blocked) and not status Coding and in MNG"), ..Kept::default() };
        assert_eq!(
            k.pills(),
            vec![
                Pill::Clause { cat: Cat::Status, clause: clause(Op::Is, &["Ready"]) },
                Pill::Group {
                    at: 1,
                    conj: Conj::Any,
                    members: vec![
                        crate::anyof::Member::Clause { cat: Cat::Blocked, clause: clause(Op::Is, &["yes"]) },
                        crate::anyof::Member::Clause { cat: Cat::Status, clause: clause(Op::Is, &["Blocked"]) },
                    ],
                },
                Pill::Custom { text: "not status Coding".into(), at: 2 },
                Pill::Clause { cat: Cat::Shortcode, clause: clause(Op::Is, &["MNG"]) },
            ]
        );
        assert_eq!(k.len(), 4);
        k.remove_group(1);
        assert_eq!(k.filter.format(), "status Ready and not status Coding and in MNG");
        // A category's pill edits its own group in place; a new one goes last;
        // none, its group goes.
        k.toggle(Cat::Status, "Coding");
        assert_eq!(k.filter.format(), "status in (Coding, Ready) and not status Coding and in MNG");
        k.set(Cat::Kind, Op::Is, strs(&["gap"]));
        assert_eq!(k.filter.format(), "status in (Coding, Ready) and not status Coding and in MNG and kind gap");
        k.remove(Cat::Status);
        assert_eq!(k.filter.format(), "not status Coding and in MNG and kind gap");
        assert_eq!(
            k.pills()[0],
            Pill::Clause { cat: Cat::Status, clause: clause(Op::Not, &["Coding"]) },
            "the custom group, alone of its category now, is its pill"
        );
        // Local clauses ride beside the language, after it.
        k.set(Cat::Wave, Op::Is, strs(&["1"]));
        assert_eq!(k.filter.format(), "not status Coding and in MNG and kind gap");
        assert_eq!(k.pills().last(), Some(&Pill::Clause { cat: Cat::Wave, clause: clause(Op::Is, &["1"]) }));
        assert_eq!(k.clauses().get(Cat::Wave), Some(&clause(Op::Is, &["1"])));
        assert_eq!(k.clauses().get(Cat::Shortcode), Some(&clause(Op::Is, &["MNG"])));
    }

    /// FILTER-31 acceptance 1: every shape the language holds — `and` of
    /// groups, each `and` or `or`, with time pills, flags, labels and
    /// negated clauses inside — is drawn as pills and boxes (never one raw
    /// pill), and comes back the same through the pills, the text, the URL
    /// and a saved filter.
    #[test]
    fn groups_round_trip_through_pills_text_url_and_saved() {
        use crate::anyof::{self, Member};
        let forms = [
            "(changed today or created today)",
            "(changed today or created today) and kind build",
            "(status Ready or kind gap) and in MNG",
            "(status Ready and kind gap)",
            "(blocked or not ready or has commits)",
            "(label needs-you or asking) and not menger doc",
            "(not status Ready or not kind gap) and severity in (high, none)",
            "(status Pending for over 2w or entered Ready today by owner)",
            "(not created today or status Coding or status \"In Review\")",
            "(not kind doc and not kind spike and not blocked)",
            "kind build and (in MNG or under PLEXUS-1)",
        ];
        for form in forms {
            let k = Kept { filter: parsed(form), ..Kept::default() };
            let pills = k.pills();
            assert!(pills.iter().any(|p| matches!(p, Pill::Group { .. })), "{form}: {pills:?}");
            assert!(!pills.iter().any(|p| matches!(p, Pill::Custom { .. })), "{form}: never one raw pill");
            // Pills → groups: each group pill's members build its group.
            let mut back = k.clone();
            for p in &pills {
                if let Pill::Group { at, conj, members } = p {
                    back.filter.groups[*at] = anyof::build(*conj, members).unwrap().unwrap();
                }
            }
            // (Values in their category's order: the same pills read back.)
            assert_eq!(back.pills(), pills, "{form}");
            assert_eq!(Kept { filter: parsed(&back.filter.format()), ..Kept::default() }, back, "{form}");
            // Text, the URL and a saved filter.
            let text = k.filter.format();
            assert_eq!(Kept { filter: parsed(&text), ..Kept::default() }.pills(), pills, "{form}: text");
            for level in [Level::Global, Level::Nav, Level::Board] {
                let q = join(&pairs_of(level, &k));
                assert_eq!(read(&q, level), Some(Some(k.clone())), "{form}: the URL at {level:?}");
            }
            let saved = crate::saved::filter_of(&crate::saved::saved_text(&k.filter)).unwrap();
            assert_eq!(crate::saved::chosen(saved).pills(), pills, "{form}: saved");
        }
        // A member keeps its own pill, operator and values; Status inside a
        // group is not the level's Status clause, and the menu says so.
        let k = Kept { filter: parsed("(status Ready or kind gap) and status Coding"), ..Kept::default() };
        let pills = k.pills();
        assert_eq!(
            pills[0],
            Pill::Group {
                at: 0,
                conj: Conj::Any,
                members: vec![
                    Member::Clause { cat: Cat::Status, clause: clause(Op::Is, &["Ready"]) },
                    Member::Clause { cat: Cat::Kind, clause: clause(Op::Is, &["gap"]) },
                ],
            }
        );
        assert_eq!(k.clauses().get(Cat::Status), Some(&clause(Op::Is, &["Coding"])));
        // FILTER-36: groups nest now — a nested shape is drawn as its text.
        for (text, canonical) in [
            ("(changed today and kind build) or created today", "((changed today and kind build) or created today)"),
            ("((changed today or created today) and kind build)", "((changed today or created today) and kind build)"),
        ] {
            let k = Kept { filter: parsed(text), ..Kept::default() };
            assert_eq!(k.pills(), vec![Pill::Custom { text: canonical.into(), at: 0 }], "{text}");
        }
        // A shape the language still refuses draws nothing: `or` and `and` in one pair.
        let e = Filter::parse("(changed today and kind build or created today)").unwrap_err();
        assert!(e.chunks.iter().any(|c| c.why.contains("do not mix")), "{e}");
    }

    /// Acceptance 1: each level's query and storage round trip — the same
    /// pairs both ways; the empty filter is `f=` (not the default); absent
    /// is nothing said; malformed (a bad escape, a text that does not
    /// parse, a key twice, a bad local clause) is the level's default.
    #[test]
    fn each_level_round_trips_through_its_query_and_storage() {
        let mut waves = Kept { filter: parsed("in (CONTEXT, RETURNS) and not status Complete"), ..Kept::default() };
        waves.set(Cat::Wave, Op::Is, strs(&["2", "missing"]));
        waves.set(Cat::Title, Op::Not, strs(&["a, b & c"]));
        let cases = [
            (Level::Global, Kept { filter: parsed("status Ready and not blocked"), ..Kept::default() }),
            (Level::Nav, Kept { filter: parsed("in MNG"), ..Kept::default() }),
            (Level::Board, Kept { filter: parsed("(blocked or status Blocked) and severity in (high, none)"), ..Kept::default() }),
            (Level::Waves, waves),
            (Level::Inputs, Kept { filter: parsed("under PLEXUS-1"), ..Kept::default() }),
        ];
        for (level, k) in cases {
            let q = join(&pairs_of(level, &k));
            assert!(q.starts_with(&format!("{}=", level.key())), "{q}");
            assert_eq!(read(&q, level), Some(Some(k.clone())), "{level:?}: {q}");
            // The URL is the same pairs, beside the page's own.
            let (text, local) = (k.filter.format(), k.local.to_pairs());
            let h = match level {
                Level::Global => lens_hash("#/x?sc=A", Some(&text), None, None),
                Level::Nav => lens_hash("#/x?sc=A", None, Some(&text), None),
                _ => lens_hash("#/x?sc=A", None, None, Some((Some(&text), &local, &[]))),
            };
            assert_eq!(read(query_of(&h), level), Some(Some(k.clone())), "{level:?}: {h}");
            // Empty: there, the empty filter; absent, nothing said.
            assert_eq!(read(&format!("{}=", level.key()), level), Some(Some(Kept::default())));
            assert_eq!(read("", level), None);
            // Malformed: the default.
            for bad in ["%zz", "status%20Bogus", "tag%20x", "text%20%22x%22%20or%20ready"] {
                assert_eq!(read(&format!("{}={bad}", level.key()), level), Some(None), "{level:?}: {bad}");
            }
            assert_eq!(read(&format!("{0}=ready&{0}=blocked", level.key()), level), Some(None), "a key twice");
        }
        assert_eq!(read("f=ready&wave=is:0", Level::Waves), Some(None), "a bad local clause");
        assert_eq!(read("f=ready&wave=is:1", Level::Board).unwrap().unwrap().local, Clauses::default(), "the board has no local Wave");
        assert_eq!(storage_key(Level::Waves, Some("plexus")), "menger.lens.waves.PLEXUS");
    }

    /// Acceptance 1: WAVE-12's keys are read — its Shortcode, Status, Kind
    /// and Severity as the language's groups, the rest local; `all` the
    /// empty filter; a malformed link the default; the lens's own keys
    /// beside it ignored.
    #[test]
    fn wave_12s_links_are_read() {
        let k = read_wave_12("sc=is:RETURNS,CONTEXT&status=not:Complete,Superseded&kind=is:build&sev=is:High&wave=is:1&q=envelope&g=ready")
            .unwrap()
            .unwrap();
        assert_eq!(
            k.filter.format(),
            "in (CONTEXT, RETURNS) and not status in (Complete, Superseded) and kind build and severity high"
        );
        assert_eq!(k.local.get(Cat::Wave), Some(&clause(Op::Is, &["1"])));
        assert_eq!(k.local.get(Cat::Title), Some(&clause(Op::Is, &["envelope"])));
        assert_eq!(read_wave_12("all"), Some(Some(Kept::default())));
        assert_eq!(read_wave_12("sc=is:A,,B"), Some(None), "malformed: the default");
        assert_eq!(read_wave_12("g=ready&f=x"), None, "no WAVE-12 key");
        assert_eq!(read_wave_12(""), None);
    }

    /// The wave board opens on its shortcode tree: the default is
    /// `in (the tree) and kind in (build, spike, gap)`.
    #[test]
    fn the_wave_board_opens_on_its_tree() {
        let t = platform();
        let d = waves_default("plexus", &t);
        assert_eq!(d.filter.format(), "under PLEXUS and kind in (build, spike, gap)");
        assert_eq!(d.local, Clauses::default());
        assert_eq!(waves_default("WAVE", &t).filter.format(), "under WAVE and kind in (build, spike, gap)");
        // FILTER-41: drawn as three groups joined by or.
        assert_eq!(crate::ors::Ors::of_filter(&d.filter).map(|o| o.groups.len()), Some(3));
    }

    /// FILTER-21: the nav opens on `not menger doc` — one pill, Menger docs
    /// at `no` — which leaves a board note out and keeps every other ticket,
    /// an ordinary doc included. A stored or linked `n=` that says the same
    /// reads as the default; the empty filter chosen (every ticket, menger's
    /// own included) is a choice of its own, kept as `n=`; and a global
    /// `menger doc` against it is a clash the nav's bar names.
    #[test]
    fn the_nav_opens_without_menger_docs() {
        let d = nav_default();
        assert_eq!(d.filter.format(), "not menger doc");
        assert_eq!(d.pills(), vec![Pill::Clause { cat: Cat::MengerDocs, clause: clause(Op::Is, &["no"]) }]);
        assert_eq!(d.clauses().get(Cat::MengerDocs), Some(&clause(Op::Is, &["no"])));
        assert_eq!(d.local, Clauses::default());
        let ticket = |id: &str, kind: menger_tickets_api::Kind, path: Option<&str>| menger_tickets_api::filter::Facts {
            asking: false,
            labels: Vec::new(),
            archived: false,
            id: id.into(),
            epic: "PLEXUS".into(),
            title: String::new(),
            status: if kind == menger_tickets_api::Kind::Doc { menger_tickets_api::Status::Doc } else { menger_tickets_api::Status::Ready },
            kind,
            severity: None,
            blocked: false,
            has_commits: false,
            menger_doc: menger_tickets_api::is_menger_doc(path),
            lineage: vec![id.into(), "PLEXUS".into()],
            history: Vec::new(),
        };
        use menger_tickets_api::Kind::{Build, Doc};
        assert!(!d.filter.matches(&ticket("PLEXUS-2", Doc, Some("_MENGER/board"))), "the board note: out");
        assert!(d.filter.matches(&ticket("PLEXUS-3", Doc, None)), "an ordinary doc: in");
        assert!(d.filter.matches(&ticket("PLEXUS-4", Build, None)));
        // The pill turned over is menger's own documents alone.
        let mut only = d.clone();
        only.flip(Cat::MengerDocs);
        assert_eq!(only.filter.format(), "menger doc");
        // Said by a link or by storage, the default reads as itself; the
        // empty filter is a choice, kept as `n=`.
        assert_eq!(read("n=not%20menger%20doc", Level::Nav), Some(Some(d.clone())));
        assert_eq!(read("n=", Level::Nav), Some(Some(Kept::default())));
        assert_ne!(Kept::default(), d, "every ticket is not the nav's default");
        assert_eq!(join(&pairs_of(Level::Nav, &Kept::default())), "n=");
        // Under the global filter the nav shows both; a global `menger doc`
        // meets the default and nothing can match — named in the nav's bar.
        let global = parsed("menger doc");
        assert_eq!(
            clash_of(&global, &d.filter, Level::Nav),
            Some(vec![("menger doc".to_string(), vec![Level::Global]), ("not menger doc".to_string(), vec![Level::Nav])])
        );
        assert_eq!(parsed("kind doc").and(&d.filter).format(), "kind doc and not menger doc");
    }

    /// Acceptance 1: a surface's bar shows the global pills first, each
    /// marked global, then its own; the global bar, its own.
    #[test]
    fn a_surface_bar_inherits_the_global_pills() {
        let g = Kept { filter: parsed("status Ready and under PLEXUS-1"), ..Kept::default() };
        let own = Kept { filter: parsed("kind gap"), ..Kept::default() };
        let shown = bar_pills(Level::Waves, g.pills(), own.pills());
        assert_eq!(
            shown,
            vec![
                (Level::Global, Pill::Clause { cat: Cat::Status, clause: clause(Op::Is, &["Ready"]) }),
                (Level::Global, Pill::Custom { text: "under PLEXUS-1".into(), at: 1 }),
                (Level::Waves, Pill::Clause { cat: Cat::Kind, clause: clause(Op::Is, &["gap"]) }),
            ]
        );
        assert_eq!(bar_pills(Level::Global, Vec::new(), g.pills()).iter().filter(|(l, _)| *l == Level::Global).count(), 2);
        assert!(bar_pills(Level::Board, Vec::new(), Vec::new()).is_empty());
    }

    /// Acceptance 1: the contradiction notice names each clashing group's
    /// level — global, the surface's, or (merged by `and`) both; none when
    /// something can match.
    #[test]
    fn the_contradiction_names_each_groups_level() {
        let g = parsed("status Ready");
        let w = parsed("in MNG and status Coding");
        assert_eq!(
            clash_of(&g, &w, Level::Waves),
            Some(vec![("status Ready".into(), vec![Level::Global]), ("status Coding".into(), vec![Level::Waves])])
        );
        assert_eq!(clash_of(&g, &parsed("status in (Ready, Coding)"), Level::Waves), None, "they merge to status Ready");
        // A merged group comes from both levels.
        let g = parsed("status in (Ready, Coding)");
        let w = parsed("status in (Coding, Pending) and not status Coding");
        let c = clash_of(&g, &w, Level::Board).expect("nothing can match");
        assert!(c.contains(&("status Coding".into(), vec![Level::Global, Level::Board])), "{c:?}");
        assert!(c.contains(&("not status Coding".into(), vec![Level::Board])), "{c:?}");
        // The global filter alone.
        assert_eq!(
            clash_of(&parsed("ready and blocked"), &Filter::default(), Level::Global),
            Some(vec![("ready".into(), vec![Level::Global]), ("blocked".into(), vec![Level::Global])])
        );
        assert_eq!(clash_of(&Filter::default(), &Filter::default(), Level::Nav), None);
    }

    /// FILTER-26: Make global — the global filter becomes `global and nav`
    /// less the nav's default; only what the reader added moves; an empty
    /// global becomes the added clauses; or-groups carry over whole; a
    /// duplicate is dropped and one field's lists merge as the lens merges;
    /// a clash is kept for the bar to name. A nav at its default (or with
    /// only the default taken out) offers nothing.
    #[test]
    fn make_global_moves_what_the_reader_added() {
        let d = nav_default().filter;
        let up = |g: &str, n: &str| promoted(&Kept { filter: parsed(g), ..Kept::default() }, &parsed(n), &d).filter.format();
        // The ticket's diagram.
        assert_eq!(up("status Ready", "in THEME and kind build and not menger doc"), "status Ready and in THEME and kind build");
        // The default is not copied up, wherever it sits.
        assert_eq!(up("status Ready", "not menger doc and kind build"), "status Ready and kind build");
        // An empty global becomes the added clauses.
        assert_eq!(up("", "kind gap and not menger doc"), "kind gap");
        // The reader removed the default and added a clause: the clause moves.
        assert_eq!(up("", "kind gap"), "kind gap");
        // The reader turned the default over: `menger doc` is theirs.
        assert_eq!(up("kind doc", "menger doc"), "kind doc and menger doc");
        // Or-groups carry over whole, from either side.
        assert_eq!(
            up("(changed today or created today)", "(status Ready or kind gap) and not menger doc"),
            "(changed today or created today) and (status Ready or kind gap)"
        );
        // FILTER-30's clauses move intact: a negated time clause, `touched`.
        assert_eq!(
            up("status Ready", "not touched within 2w and not menger doc and not changed today"),
            "status Ready and not touched within 2w and not changed today"
        );
        assert_eq!(up("not changed today", "touched today"), "not changed today and touched today");
        // A duplicate is dropped; one field's lists merge.
        assert_eq!(up("kind build", "kind build and not menger doc"), "kind build");
        assert_eq!(up("status in (Ready, Coding)", "status in (Coding, Pending)"), "status Coding");
        // A clash is kept as written, for the global bar to name.
        let clash = promoted(&Kept { filter: parsed("status Ready"), ..Kept::default() }, &parsed("status Coding"), &d);
        assert_eq!(clash.filter.format(), "status Ready and status Coding");
        assert!(clash.filter.admits_nothing().is_some());
        // The global's display stays; the nav's (its flat list) does not move.
        let flat = promoted(&Kept::default(), &parsed("kind gap · flat"), &d);
        assert_eq!(flat.filter.format(), "kind gap");
        // What is offered.
        assert!(!promotable(&d, &d), "the nav at its default");
        assert!(!promotable(&Filter::default(), &d), "only the default taken out: nothing to move");
        assert!(!promotable(&parsed("not menger doc · flat"), &d), "a display alone");
        assert!(promotable(&parsed("kind build and not menger doc"), &d));
        assert!(promotable(&parsed("menger doc"), &d));
        assert_eq!(nav_added(&parsed("not menger doc and in THEME"), &d), parsed("in THEME").groups);
        // Through the URL: the new global rides as `g=`, the nav's `n=` drops.
        let g = promoted(&Kept::default(), &parsed("in THEME and not menger doc"), &d);
        assert_eq!(lens_hash("#/?n=in%20THEME%20and%20not%20menger%20doc", Some(&g.filter.format()), None, None), "#/?g=in%20THEME");
    }
}
