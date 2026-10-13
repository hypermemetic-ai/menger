//! FILTER-4: what a filter answers — `counts`, a filtered page by cursor or
//! by position (`list { filter, at }`), and `watch { filter }`'s membership.
//!
//! A filter reaches the store as a [`Lens`]: the shortcodes it names and how
//! it judges one (shortcode, cell). The lens is compiled against the cell
//! table under the read lock ([`cells::Admit`]) — once per cell per
//! shortcode it names, and once for every other shortcode — and every
//! answer is then a sum or a merge over the bitmaps it admits
//! ([`cells::View`]): no ticket is tested. A filtered page's rows are looked
//! up by position, as a page of the ready set's are.

use super::cells::{Admit, Key, View, SEVERITIES};
use super::lineage::{Compiled, UNDERS_MAX};
use super::*;
use crate::filter::{Category, Facts, Field, Filter};
use std::collections::BTreeSet;
use std::sync::Arc;

/// A filter as the store judges it: the shortcodes it names (each judged on
/// its own) and, for a shortcode and a cell, whether the filter admits it.
/// Judged once per (shortcode, cell) pair at a compile, never per ticket.
///
/// FILTER-13: and the names its `under` asks for. A judge reads a lineage
/// beside the shortcode and the cell — the shortcodes hosting it, and which
/// of those names a ticket is under — so a lens with `under` is compiled
/// per (shortcode, cell, which `under`s hold) ([`super::lineage::Compiled`]).
#[derive(Clone)]
pub struct Lens {
    /// The filter as text, canonical — what two lenses are compared by.
    text: String,
    named: Vec<String>,
    /// FILTER-13: what `under` names, upper case, each once.
    unders: Vec<String>,
    /// MENGER-13: the history predicates it asks, each once — answered from
    /// the transition log's kept indexes ([`super::history::Log::holds`]),
    /// at the moment and in the zone `clock` says.
    history: Vec<crate::filter::History>,
    clock: super::history::Clock,
    judge: Arc<Judge>,
}

/// A lens's judge: a shortcode, a cell, the lineage's names that hold, and
/// (MENGER-13) the history predicates that hold, by their canonical text.
type Judge = dyn Fn(&str, Key, &[&str], &[&str]) -> bool + Send + Sync;

/// MENGER-13: at most this many distinct history predicates one filter asks.
pub const HISTORY_MAX: usize = 16;

impl Lens {
    /// A lens over `named` shortcodes that admits what `judge` admits. The
    /// rest of the store is judged as the shortcode `""`, which no `in`
    /// names.
    pub(crate) fn new(text: String, named: Vec<String>, judge: impl Fn(&str, Key) -> bool + Send + Sync + 'static) -> Self {
        Self {
            text,
            named,
            unders: Vec::new(),
            history: Vec::new(),
            clock: Default::default(),
            judge: Arc::new(move |e, k, _, _| judge(e, k)),
        }
    }

    /// MENGER-13: this lens reading time by `clock` — its zone, its moment
    /// (a test's, or now), who `me` is.
    pub fn with_clock(mut self, clock: super::history::Clock) -> Self {
        self.clock = clock;
        self
    }

    /// MENGER-13: how it reads time.
    pub fn clock(&self) -> &super::history::Clock {
        &self.clock
    }

    /// MENGER-13: the history predicates it asks.
    pub(super) fn history(&self) -> &[crate::filter::History] {
        &self.history
    }

    /// MENGER-13: whether it asks the transition log.
    pub fn has_history(&self) -> bool {
        !self.history.is_empty()
    }

    /// FILTER-30: whether it asks `touched` — what a write that moves no
    /// cell (a body, a comment, an answer) can move.
    pub fn asks_touched(&self) -> bool {
        self.history.iter().any(|h| h.event == crate::filter::Event::Touched)
    }

    /// MENGER-13: whether what it admits moves as time passes — a history
    /// predicate whose window is relative (`within`, `today`, `yesterday`,
    /// `this week`): a `watch` reads it again on a clock.
    pub fn relative(&self) -> bool {
        self.history.iter().any(crate::filter::History::relative)
    }

    /// The empty filter: every ticket.
    pub fn all() -> Self {
        Self::new(String::new(), Vec::new(), |_, _| true)
    }

    /// A filter's text, read by the language (FILTER-10) and judged by its
    /// own evaluator — or the in-band refusal: every chunk the language did
    /// not take, or every predicate that is answered elsewhere.
    pub fn parse(text: &str) -> Result<Self, StoreError> {
        let filter = Filter::parse(text).map_err(|e| StoreError::Filter(e.to_string()))?;
        Self::of(&filter)
    }

    /// `filter`, compiled the way the store compiles it: its `in`
    /// shortcodes named, each (shortcode, cell) judged by the language's own
    /// evaluator ([`Filter::matches`]) on the facts a cell holds — so a
    /// compiled filter admits exactly what the reference evaluator passes.
    /// `text` is refused ([`refuse`]). FILTER-13: `under` is answered — its
    /// names kept, and the evaluator handed the lineage they are tested on.
    pub fn of(filter: &Filter) -> Result<Self, StoreError> {
        refuse(filter)?;
        let preds = || filter.groups.iter().flat_map(|g| &g.preds);
        let named: BTreeSet<String> = preds()
            .filter_map(|p| match &p.field {
                Field::In(e) => Some(e.to_ascii_uppercase()),
                _ => None,
            })
            .collect();
        let unders: BTreeSet<String> = preds()
            .filter_map(|p| match &p.field {
                Field::Under(x) => Some(x.to_ascii_uppercase()),
                _ => None,
            })
            .collect();
        if unders.len() > UNDERS_MAX {
            return Err(StoreError::Filter(format!(
                "a filter asks `under` for at most {UNDERS_MAX} names; this one names {}",
                unders.len()
            )));
        }
        // MENGER-13: its history predicates, each once.
        let mut history: Vec<crate::filter::History> = Vec::new();
        for p in preds() {
            if let Field::History(h) = &p.field {
                if !history.iter().any(|x| x.format() == h.format()) {
                    history.push(h.clone());
                }
            }
        }
        if history.len() > HISTORY_MAX {
            return Err(StoreError::Filter(format!(
                "a filter asks at most {HISTORY_MAX} history predicates; this one asks {}",
                history.len()
            )));
        }
        let judged = filter.clone();
        Ok(Self {
            text: filter.format(),
            named: named.into_iter().collect(),
            unders: unders.into_iter().collect(),
            history,
            clock: Default::default(),
            judge: Arc::new(move |epic, key, lineage, holds| judged.matches(&facts(epic, key, lineage, holds))),
        })
    }

    /// FILTER-13: what `under` names, upper case, each once.
    pub(super) fn unders(&self) -> &[String] {
        &self.unders
    }

    /// The shortcodes `in` names, upper case.
    pub(super) fn named(&self) -> &[String] {
        &self.named
    }

    /// FILTER-13: whether the lens admits a ticket of `epic` in cell `key`
    /// whose lineage holds `lineage` (the shortcodes hosting `epic`, and the
    /// `under` names it is under).
    pub(super) fn judge(&self, epic: &str, key: Key, lineage: &[&str]) -> bool {
        (self.judge)(epic, key, lineage, &[])
    }

    /// MENGER-13: [`Lens::judge`] with the history predicates `holds` names
    /// holding — a ticket's own truths, which no cell carries.
    pub(super) fn judge_holding(&self, epic: &str, key: Key, lineage: &[&str], holds: &[&str]) -> bool {
        (self.judge)(epic, key, lineage, holds)
    }

    /// The filter's text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Compiled against `keys`, `list`'s `status` folded in as the sugar it
    /// is (its `epic` restricts the shortcodes a [`View`] holds: a shortcode
    /// exactly as named, as `list { epic }` always took it).
    ///
    /// FILTER-13: a lens with `under` is compiled with its lineage
    /// ([`super::lineage::Compiled`]); this is the lens without it, where
    /// the lineage holds nothing a judge reads.
    pub(super) fn admit(&self, keys: &[Key], status: Option<Status>) -> Admit {
        debug_assert!(self.unders.is_empty(), "a lens with `under` is compiled with its lineage");
        debug_assert!(self.history.is_empty(), "a lens with history is compiled with the log");
        Admit::compile(keys, self.named.iter().cloned(), |e, k| {
            status.is_none_or(|s| k.status() == s) && (self.judge)(e, k, &[], &[])
        })
    }
}

/// What the evaluator reads of a cell in a shortcode: everything but a
/// ticket's id and title — which only `text` reads, and it is not compiled.
/// FILTER-13: the lineage `under` reads is what the compile hands it: the
/// shortcode, the shortcodes hosting it, and the `under` names that hold.
/// MENGER-13: and the history predicates that hold, by their text.
fn facts(epic: &str, key: Key, lineage: &[&str], holds: &[&str]) -> Facts {
    Facts {
        id: String::new(),
        epic: epic.to_string(),
        title: String::new(),
        status: key.status(),
        kind: key.kind(),
        severity: key.severity(),
        blocked: key.blocked(),
        has_commits: key.commits(),
        menger_doc: key.menger(),
        asking: key.asking(),
        labels: key.labels().labels().to_vec(),
        lineage: lineage.iter().map(|l| l.to_string()).collect(),
        history: holds.iter().map(|h| h.to_string()).collect(),
    }
}

/// FILTER-4: the predicates every filtered read refuses in-band, naming each
/// chunk that holds one and where it is answered instead: `text` is the
/// palette's (`find`) until SCALE-13's B5 indexes it. FILTER-13: `under` is
/// answered everywhere, from the kept tree.
pub fn refuse(filter: &Filter) -> Result<(), StoreError> {
    let mut bad: Vec<String> = Vec::new();
    for g in &filter.groups {
        let mut why: Vec<&str> = Vec::new();
        if g.preds.iter().any(|p| matches!(p.field, Field::Text(_))) {
            why.push("text is the palette's — ask `find` (a filter's text waits for SCALE-13's B5)");
        }
        if !why.is_empty() {
            bad.push(format!("`{}` — {}", g.format(), why.join("; ")));
        }
    }
    match bad.is_empty() {
        true => Ok(()),
        false => Err(StoreError::Filter(format!("not answered here: {}", bad.join("; ")))),
    }
}

/// FILTER-4: the fields `facets` counts, in the order it answers them — each
/// as the language spells it. FILTER-21: `menger doc` after the other flags.
/// MENGER-12: `asking` after it, then `label` — a count per label held, in
/// name order (a ticket with two labels is counted under each) — before
/// `in`.
pub const FACET_FIELDS: [&str; 11] = [
    "status",
    "category",
    "kind",
    "severity",
    "blocked",
    "ready",
    "has commits",
    "menger doc",
    "asking",
    "label",
    "in",
];

/// One field's tally: a count per value, in the order `facets` answers.
fn tally(field: &str) -> Vec<(String, u64)> {
    let yes_no = || vec![("yes".to_string(), 0), ("no".to_string(), 0)];
    match field {
        "status" => Status::ALL.iter().map(|s| (s.as_str().to_string(), 0)).collect(),
        "category" => Category::ALL.iter().map(|c| (c.as_str().to_string(), 0)).collect(),
        "kind" => Kind::ALL.iter().map(|k| (k.as_str().to_string(), 0)).collect(),
        // The language's order: critical, high, medium, low, none.
        "severity" => SEVERITIES[1..]
            .iter()
            .chain(&SEVERITIES[..1])
            .map(|s| (Field::Severity(*s).value().expect("a severity has a value"), 0))
            .collect(),
        "blocked" | "ready" | "has commits" | "menger doc" | "asking" => yes_no(),
        _ => Vec::new(),
    }
}

/// Where in [`tally`]'s order a cell's value of `field` sits.
fn slot(field: &str, k: Key) -> usize {
    let flag = |b: bool| usize::from(!b);
    match field {
        "status" => Status::ALL.iter().position(|s| *s == k.status()).expect("a status"),
        "category" => {
            let c = crate::filter::category_of(k.status());
            Category::ALL.iter().position(|x| *x == c).expect("a category")
        }
        "kind" => Kind::ALL.iter().position(|x| *x == k.kind()).expect("a kind"),
        "severity" => match SEVERITIES.iter().position(|s| *s == k.severity()).expect("a severity") {
            0 => 4,
            i => i - 1,
        },
        "blocked" => flag(k.blocked()),
        "ready" => flag(crate::filter::category_of(k.status()) == Category::Ready && !k.blocked()),
        "has commits" => flag(k.commits()),
        "menger doc" => flag(k.menger()),
        "asking" => flag(k.asking()),
        other => unreachable!("not a cell field: {other}"),
    }
}

impl PartialEq for Lens {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl std::fmt::Debug for Lens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lens({:?})", self.text)
    }
}

/// FILTER-4: a filtered `list` — the filter, and `list`'s three old
/// parameters, which are sugar beside it: `epic` for `in`, `status` for a
/// status, `parent` for a parent's page. FILTER-24: and `keep`, a ticket
/// held as a match whatever the filter says of it (the nav's open ticket) —
/// when it is inside the sugar.
#[derive(Debug, Clone, PartialEq)]
pub struct Filtered {
    pub lens: Lens,
    pub epic: Option<String>,
    pub status: Option<Status>,
    pub parent: Option<String>,
    pub keep: Option<String>,
}

/// FILTER-4: what `watch { filter }` adds to a write's events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Membership {
    /// The write moved the ticket into the filter.
    Entered { id: String, epic: String, seq: u32 },
    /// …or out of it.
    Left { id: String, epic: String, seq: u32 },
    /// The write moved more than [`RECOUNT_OVER`] memberships: the filter's
    /// count of each shortcode it moved, as the write left it.
    Recount { epic: String, held: u64 },
}

/// FILTER-4: one `watch { filter }`'s filter, compiled — and compiled again
/// when a write names a cell interned since.
pub struct Watcher {
    lens: Lens,
    admit: Admit,
    /// FILTER-13: a filter that asks `under` — each write works out this
    /// watch's membership itself ([`super::lineage::UnderMoves`]), and its
    /// `admit` is not read. MENGER-13: and one that asks the log.
    under: Option<Arc<super::lineage::UnderWatch>>,
    /// MENGER-13: the moment its relative windows were last read at.
    read_at: i64,
}

/// The matches of `q` under the read lock: the admitted bitmaps, and — for
/// a parent's page — inside its children. FILTER-13: compiled with the
/// lineage `under` reads — so every read through here (FILTER-15's too)
/// answers `under`.
pub(super) fn view<'c>(cache: &'c Cache, q: &Filtered) -> View<'c> {
    let view = Compiled::new(cache, &q.lens, q.status).view(q.epic.as_deref());
    let view = match q.parent.as_deref() {
        None => view,
        Some(parent) => view.within(|epic| cache.children.in_shortcode(parent, epic).map(|seqs| seqs.bitmap().into_owned())),
    };
    match kept(cache, q) {
        Some(t) => view.keeping(&t.epic, t.seq),
        None => view,
    }
}

/// FILTER-24: the ticket a `keep` names — as given, else upper-cased (as
/// `member` takes an id).
pub(super) fn keep_row<'c>(cache: &'c Cache, id: &str) -> Option<&'c Row> {
    let id = id.trim();
    cache.get(id).or_else(|| cache.get(&id.to_ascii_uppercase()))
}

/// FILTER-24: the ticket `q.keep` names, when it is inside `q`'s sugar —
/// its shortcode, its status, its parent — and so a match of `q` whatever
/// the filter says of it.
fn kept<'c>(cache: &'c Cache, q: &Filtered) -> Option<&'c Row> {
    let t = keep_row(cache, q.keep.as_deref()?)?;
    let inside = q.epic.as_deref().is_none_or(|e| e == t.epic)
        && q.status.is_none_or(|s| s == t.status)
        && q.parent.as_deref().is_none_or(|p| cache.children.in_shortcode(p, &t.epic).is_some_and(|seqs| seqs.bitmap().contains(t.seq)));
    inside.then_some(t)
}

impl Store {
    /// FILTER-4: `counts { filter }` — each shortcode that holds a match,
    /// with how many, in shortcode order: the sum of the counts of the cells
    /// the filter admits there. The empty lens is each shortcode's tickets.
    pub fn counts(&self, lens: &Lens) -> Vec<(String, u64)> {
        self.counts_keeping(lens, None)
    }

    /// FILTER-24: [`Store::counts`] with `keep` — a ticket counted as a
    /// match whatever the filter says of it (once: a match is counted as it
    /// was).
    pub fn counts_keeping(&self, lens: &Lens, keep: Option<&str>) -> Vec<(String, u64)> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let view = Compiled::new(&cache, lens, None).view(None);
        let view = match keep.and_then(|id| keep_row(&cache, id)) {
            Some(t) => view.keeping(&t.epic, t.seq),
            None => view,
        };
        let out = view.counts().map(|(e, n)| (e.to_string(), n)).collect();
        out
    }

    /// FILTER-4: `facets { filter, base }` — the counts behind every menu
    /// (Linear's facets). For each field of [`FACET_FIELDS`], in that order:
    /// the tickets matching `base` and `filter` with `filter`'s own groups
    /// for that field set aside (a group is a field's own when every
    /// predicate in it names that field; `base`'s are never set aside),
    /// counted by their value of the field — `(field, value, held)` for
    /// each value held, statuses in [`Status::ALL`]'s order, shortcodes in
    /// the store's. A sum over the kept cell counts: each distinct filter
    /// the fields leave is compiled once, and its admitted bitmaps' lengths
    /// are tallied for every field that left it.
    pub fn facets(&self, filter: &Filter, base: &Filter) -> Result<Vec<(String, String, u64)>, StoreError> {
        self.facets_at(filter, base, &Default::default())
    }

    /// MENGER-13: [`Store::facets`] with a history filter's time read by
    /// `clock`.
    pub fn facets_at(
        &self,
        filter: &Filter,
        base: &Filter,
        clock: &super::history::Clock,
    ) -> Result<Vec<(String, String, u64)>, StoreError> {
        refuse(filter)?;
        refuse(base)?;
        // Each field's filter, and the fields that share one.
        let mut distinct: Vec<(Filter, Vec<&str>)> = Vec::new();
        for field in FACET_FIELDS {
            let own = |g: &crate::filter::Group| g.preds.iter().all(|p| p.field.name() == field);
            let rest = Filter {
                groups: filter.groups.iter().filter(|g| !own(g)).cloned().collect(),
                display: Default::default(),
            };
            match distinct.iter_mut().find(|(f, _)| *f == rest) {
                Some((_, fields)) => fields.push(field),
                None => distinct.push((rest, vec![field])),
            }
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let mut tallies: Vec<Vec<(String, u64)>> = FACET_FIELDS.iter().map(|f| tally(f)).collect();
        let mut shortcodes: Vec<(String, u64)> = Vec::new();
        // MENGER-12: a count per label, in name order — the values are the
        // labels the admitted cells hold, not a closed list.
        let mut labels: BTreeMap<String, u64> = BTreeMap::new();
        for (rest, fields) in &distinct {
            let both = Lens::of(&base.and(rest))?.with_clock(clock.clone());
            let compiled = Compiled::new(&cache, &both, None);
            let mut by_epic: Vec<(&str, u64)> = Vec::new();
            for (epic, _) in cache.cells.shortcodes() {
                let mut in_epic = 0;
                for (cell, n) in compiled.cell_counts(epic) {
                    let key = cache.cells.keys()[cell as usize];
                    in_epic += n;
                    for field in fields.iter().filter(|f| **f != "in") {
                        if *field == "label" {
                            for l in key.labels().labels().iter() {
                                *labels.entry(l.clone()).or_default() += n;
                            }
                            continue;
                        }
                        let i = FACET_FIELDS.iter().position(|f| f == field).expect("a facet field");
                        tallies[i][slot(field, key)].1 += n;
                    }
                }
                if in_epic > 0 {
                    by_epic.push((epic, in_epic));
                }
            }
            if fields.contains(&"in") {
                shortcodes = by_epic.into_iter().map(|(e, n)| (e.to_string(), n)).collect();
            }
        }
        *tallies.last_mut().expect("`in` is the last field") = shortcodes;
        let at = FACET_FIELDS.iter().position(|f| *f == "label").expect("`label` is a facet field");
        tallies[at] = labels.into_iter().collect();
        Ok(FACET_FIELDS
            .iter()
            .zip(tallies)
            .flat_map(|(field, values)| {
                values.into_iter().filter(|(_, n)| *n > 0).map(move |(v, n)| (field.to_string(), v, n))
            })
            .collect())
    }

    /// FILTER-4: one page of `q`'s matches, in the store's order — starting
    /// strictly after `after` (a ticket id, any ticket: a position), or at
    /// the `at`-th match (0-based), or at the first; at most `limit`. Its
    /// `first` is the position of its first row among the matches (for an
    /// empty page, where it would start); `remaining` the matches after it,
    /// exactly. `at` past the end is an empty page at `at`. Both `at` and
    /// `after` is [`StoreError::AtAndAfter`].
    pub fn filtered_page(
        &self,
        q: &Filtered,
        after: Option<&str>,
        at: Option<u64>,
        limit: Option<u32>,
    ) -> Result<Page, StoreError> {
        let q = Filtered {
            epic: q.epic.as_deref().map(str::to_ascii_uppercase),
            parent: q.parent.as_deref().map(str::to_ascii_uppercase),
            ..q.clone()
        };
        let after = after.map(str::to_ascii_uppercase);
        let cache = self.tickets.read().expect("store lock poisoned");
        Self::cut_filtered(&cache, &q, after.as_deref(), at, limit, Rest::Count)
    }

    /// What a filtered page — and each step of a filtered [`Walk`] — is cut
    /// from, under the read lock the caller holds.
    pub(super) fn cut_filtered(
        cache: &Cache,
        q: &Filtered,
        after: Option<&str>,
        at: Option<u64>,
        limit: Option<u32>,
        rest: Rest,
    ) -> Result<Page, StoreError> {
        let cursor = match (after, at) {
            (Some(_), Some(_)) => return Err(StoreError::AtAndAfter),
            (Some(id), None) => Some(cache.at.get(id).cloned().ok_or_else(|| StoreError::CursorNotFound(id.to_string()))?),
            _ => None,
        };
        let view = view(cache, q);
        let take = limit.map_or(usize::MAX, |n| n as usize);
        let (first, keys) = match (&cursor, at) {
            (Some(key), _) => (view.rank(key), view.page_after(key, take)),
            (None, Some(p)) => match view.select(p) {
                Some(start) => (p, view.page(Some(start), take)),
                None => (p, Vec::new()),
            },
            (None, None) => (0, view.page(None, take)),
        };
        let tickets: Vec<Row> = keys
            .into_iter()
            .filter_map(|(epic, seq)| cache.order.get(&OrderKey { epic: epic.to_string(), seq }).cloned())
            .collect();
        let remaining = match rest {
            Rest::Count => view.total().saturating_sub(first).saturating_sub(tickets.len() as u64),
            Rest::Skip => 0,
        };
        let next = if remaining == 0 {
            None
        } else {
            // The page's last id; for an empty page, the cursor it was cut
            // with — or, cut at a position, the match before it, which is
            // where `after` resumes at that position (none before the first).
            tickets.last().map(|t| t.id.clone()).or_else(|| match (after, first) {
                (Some(id), _) => Some(id.to_string()),
                (None, 0) => None,
                (None, p) => view
                    .select(p - 1)
                    .and_then(|(epic, seq)| cache.order.get(&OrderKey { epic: epic.to_string(), seq }))
                    .map(|t| t.id.clone()),
            })
        };
        Ok(Page {
            tickets,
            next,
            remaining,
            first: Some(first),
        })
    }

    /// FILTER-4: a `watch { filter }`'s filter, compiled against the cells
    /// as they stand.
    pub fn watcher(&self, lens: Lens) -> Watcher {
        self.watcher_in(lens, None)
    }

    /// FILTER-13: [`Store::watcher`] for a watch in `epic` alone (when one
    /// is asked) — which a filter asking `under` registers with the store, so
    /// each write works out its membership.
    pub fn watcher_in(&self, lens: Lens, epic: Option<&str>) -> Watcher {
        // MENGER-13: a filter asking the log is a ticket's own truth as well
        // as its cell's — each write works it out, as it does `under`'s.
        if !lens.unders().is_empty() || lens.has_history() {
            let read_at = lens.clock().now();
            let under = self.unders.open(lens.clone(), epic.map(str::to_string));
            return Watcher {
                lens,
                admit: Admit::compile(&[], std::iter::empty(), |_, _| false),
                under: Some(under),
                read_at,
            };
        }
        let admit = {
            let cache = self.tickets.read().expect("store lock poisoned");
            lens.admit(cache.cells.keys(), None)
        };
        Watcher { lens, admit, under: None, read_at: 0 }
    }

    /// FILTER-27: the next moment after the lens's now at which time alone
    /// moves what it admits — the earliest of its history predicates' next
    /// boundaries ([`super::history::Log::next_boundary`]); `None` when
    /// none will move.
    pub fn next_boundary(&self, lens: &Lens) -> Option<i64> {
        let cache = self.tickets.read().expect("store lock poisoned");
        lens.history().iter().filter_map(|h| cache.log.next_boundary(h, lens.clock())).min()
    }

    /// MENGER-13: what time did to a `watch { filter }` whose filter has a
    /// relative window, since `then` — each ticket whose history truths
    /// differ between `then` and `now`, judged both ways against the store
    /// as it stands: `Entered` / `Left` for those it moved, in the store's
    /// order (in `epic` alone when one is asked); past [`RECOUNT_OVER`], one
    /// `Recount` per shortcode they are in. A write between the two moments
    /// was said by the write itself; a ticket it moved is only said again
    /// here when time moved it too.
    pub fn aged(&self, lens: &Lens, then: i64, now: i64, epic: Option<&str>) -> Vec<Membership> {
        if !lens.relative() || now <= then {
            return Vec::new();
        }
        let at = |t: i64| {
            let mut clock = lens.clock().clone();
            clock.fixed = Some(t);
            lens.clone().with_clock(clock)
        };
        let (before, after) = (at(then), at(now));
        let cache = self.tickets.read().expect("store lock poisoned");
        let (b, a) = (Compiled::new(&cache, &before, None), Compiled::new(&cache, &after, None));
        let mut candidates: BTreeMap<String, roaring::RoaringBitmap> = b.history_candidates();
        for (e, bm) in a.history_candidates() {
            *candidates.entry(e).or_default() |= bm;
        }
        let mut moved: Vec<(String, u32, bool)> = Vec::new();
        for (e, cand) in candidates {
            if epic.is_some_and(|x| x != e) {
                continue;
            }
            for seq in cand {
                let Some(t) = cache.order.get(&OrderKey { epic: e.clone(), seq }) else { continue };
                let (was, is) = (b.matches_row(t), a.matches_row(t));
                if was != is {
                    moved.push((e.clone(), seq, is));
                }
            }
        }
        if moved.len() > RECOUNT_OVER {
            let mut epics: Vec<&String> = moved.iter().map(|(e, _, _)| e).collect();
            epics.dedup();
            return epics.into_iter().map(|e| Membership::Recount { epic: e.clone(), held: a.matches_in(e).len() }).collect();
        }
        moved
            .into_iter()
            .map(|(epic, seq, now)| {
                let id = cache.order.get(&OrderKey { epic: epic.clone(), seq }).map_or_else(|| format!("{epic}-{seq}"), |t| t.id.clone());
                if now {
                    Membership::Entered { id, epic, seq }
                } else {
                    Membership::Left { id, epic, seq }
                }
            })
            .collect()
    }
}

impl Watcher {
    /// MENGER-13: whether this watch's filter moves with the clock — the
    /// stream reads it again ([`Watcher::tick`]) on a timer.
    pub fn relative(&self) -> bool {
        self.lens.relative()
    }

    /// FILTER-27: how long to sleep before time next moves what this
    /// watch's filter admits ([`Store::next_boundary`]) — at least a second,
    /// at most `cap`.
    pub fn wait(&self, store: &Store, cap: std::time::Duration) -> std::time::Duration {
        let now = self.lens.clock().now();
        match store.next_boundary(&self.lens) {
            Some(at) => std::time::Duration::from_secs(at.saturating_sub(now).max(1) as u64).min(cap),
            None => cap,
        }
    }

    /// MENGER-13: what time did to this watch's filter since it was last
    /// read — [`Store::aged`] from then to now; the moment read is now.
    pub fn tick(&mut self, store: &Store, epic: Option<&str>) -> Vec<Membership> {
        let now = self.lens.clock().now();
        let out = store.aged(&self.lens, self.read_at, now, epic);
        self.read_at = now;
        out
    }
    /// FILTER-13: this watch's events in a write's [`super::UnderMoves`] —
    /// none for a filter without `under`, whose membership its cell moves say.
    pub fn under_membership(&self, moves: &super::UnderMoves) -> Vec<Membership> {
        self.under.as_ref().and_then(|w| moves.by.get(&w.id())).cloned().unwrap_or_default()
    }

    /// What one write's cell moves are to this filter — in `epic` alone,
    /// when one is asked for: each ticket moved into it (`Entered`) or out
    /// of it (`Left`), in the store's order; or, when there are more than
    /// [`RECOUNT_OVER`] of those, one `Recount` per shortcode they are in,
    /// with the filter's count there as the write left it.
    pub fn membership(&mut self, store: &Store, moves: &Moves, epic: Option<&str>) -> Vec<Membership> {
        if self.under.is_some() {
            return Vec::new(); // the write's own `UnderMoved` says it
        }
        // ACT-5: a deleted ticket's `to` is no cell (`GONE`), never admitted.
        let highest = moves.moves.iter().flat_map(|m| m.from.into_iter().chain([m.to])).filter(|c| *c != super::cells::GONE).max();
        if highest.is_some_and(|c| c as usize >= self.admit.len()) {
            // A cell interned after the compile: compile again — the table
            // only grows, and every id it held means what it meant.
            self.admit = store.watcher(self.lens.clone()).admit;
        }
        let admits = |e: &str, c: u32| self.admit.admits(e, c).unwrap_or(false);
        let moved: Vec<(&CellMove, bool)> = moves
            .moves
            .iter()
            .filter(|m| epic.is_none_or(|e| e == &*m.epic))
            .filter_map(|m| {
                let was = m.from.is_some_and(|c| admits(&m.epic, c));
                let now = admits(&m.epic, m.to);
                (was != now).then_some((m, now))
            })
            .collect();
        if moved.is_empty() {
            return Vec::new();
        }
        if moved.len() <= RECOUNT_OVER {
            // Each ticket's id is its row's — read here, for the few said.
            let cache = store.tickets.read().expect("store lock poisoned");
            return moved
                .into_iter()
                .map(|(m, now)| {
                    let (epic, seq) = (m.epic.to_string(), m.seq);
                    let key = OrderKey { epic, seq };
                    let id = cache.order.get(&key).map_or_else(|| format!("{}-{}", key.epic, seq), |t| t.id.clone());
                    let epic = key.epic;
                    if now {
                        Membership::Entered { id, epic, seq }
                    } else {
                        Membership::Left { id, epic, seq }
                    }
                })
                .collect();
        }
        let counts = moves.counts.as_ref().expect("a write past the threshold carries its counts");
        let mut epics: Vec<&str> = moved.iter().map(|(m, _)| &*m.epic).collect();
        epics.dedup();
        epics
            .into_iter()
            .map(|e| Membership::Recount {
                epic: e.to_string(),
                held: counts
                    .get(e)
                    .map_or(0, |held| held.iter().filter(|(c, _)| admits(e, *c)).map(|(_, n)| n).sum()),
            })
            .collect()
    }
}
