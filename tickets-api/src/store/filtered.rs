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

use crate::address::ShortcodeId;
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
    /// ACCESS-21: the tickets `ticket <ID>` names, each once, as written
    /// (folded) — a free truth per ticket, handed to the judge as a history
    /// predicate's is ([`crate::filter::ticket_truth`]).
    tickets: Vec<String>,
    clock: super::history::Clock,
    judge: Arc<Judge>,
    /// ACCESS-10: the filter it was compiled from ([`Lens::of`]) — what a
    /// read ANDs the reader's reach into ([`Lens::and`]), as the language
    /// ANDs two filters. `None` for a lens made of a judge alone (a test's).
    filter: Option<Arc<Filter>>,
    /// ARCHIVE-2: judged as written — without the archive's default, which
    /// admits no archived ticket unless the filter names `archived`
    /// ([`Filter::matches`]). A reader's reach ([`Lens::reach`]) and
    /// [`Lens::all`] are: what may be read, never what a view shows.
    written: bool,
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
            tickets: Vec::new(),
            clock: Default::default(),
            judge: Arc::new(move |e, k, _, _| judge(e, k)),
            filter: None,
            written: true,
        }
    }

    /// ACCESS-10: this lens AND `reach` — what a read answers a reader
    /// under: the two filters ANDed and compiled as one ([`Filter::and`],
    /// exactly the lens IDENTITY-21's `scoped` compiled), its clock this
    /// lens's. A lens made of a judge alone is ANDed judge by judge.
    pub(super) fn and(&self, reach: &Lens) -> Result<Lens, StoreError> {
        if let (Some(a), Some(b)) = (&self.filter, &reach.filter) {
            // ARCHIVE-2: judged as this lens is — a view's keeps the
            // archive's default, a reach's or `all`'s has none.
            return Ok(Lens::compiled(&a.and(b), self.written)?.with_clock(self.clock.clone()));
        }
        let union = |a: &[String], b: &[String]| -> Vec<String> {
            let mut v: Vec<String> = a.iter().chain(b).cloned().collect();
            v.sort();
            v.dedup();
            v
        };
        let mut history = self.history.clone();
        for h in &reach.history {
            if !history.iter().any(|x| x.format() == h.format()) {
                history.push(h.clone());
            }
        }
        let (a, b) = (Arc::clone(&self.judge), Arc::clone(&reach.judge));
        Ok(Lens {
            text: format!("({}) and ({})", self.text, reach.text),
            named: union(&self.named, &reach.named),
            unders: union(&self.unders, &reach.unders),
            history,
            tickets: union(&self.tickets, &reach.tickets),
            clock: self.clock.clone(),
            judge: Arc::new(move |e, k, l, h| a(e, k, l, h) && b(e, k, l, h)),
            filter: None,
            written: self.written,
        })
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
    ///
    /// ACCESS-21: or names a ticket alone (`ticket <ID>`) — a truth of one
    /// ticket's, worked out as a history predicate's is.
    pub fn has_history(&self) -> bool {
        !self.history.is_empty() || !self.tickets.is_empty()
    }

    /// ACCESS-21: the tickets `ticket <ID>` names, as written.
    pub(super) fn tickets(&self) -> &[String] {
        &self.tickets
    }

    /// FILTER-30: whether it asks `touched` — what a write that moves no
    /// cell (a body, a comment, an answer) can move.
    pub fn asks_touched(&self) -> bool {
        self.history.iter().any(|h| h.event == crate::filter::Event::Touched)
    }

    /// DEPLOY-11: whether it asks `deployed` (DEPLOY-20: or `behind`) —
    /// what a recorded deployment can move.
    pub fn asks_deployed(&self) -> bool {
        self.history.iter().any(|h| matches!(h.event, crate::filter::Event::Deployed(_) | crate::filter::Event::Behind(_)))
    }

    /// MENGER-39: whether it asks `queued` — what a queue write can move.
    pub fn asks_queued(&self) -> bool {
        self.history.iter().any(|h| matches!(h.event, crate::filter::Event::Queued(_)))
    }

    /// MENGER-13: whether what it admits moves as time passes — a history
    /// predicate whose window is relative (`within`, `today`, `yesterday`,
    /// `this week`): a `watch` reads it again on a clock.
    pub fn relative(&self) -> bool {
        self.history.iter().any(crate::filter::History::relative)
    }

    /// The empty filter as written: every ticket, the archived included —
    /// what an access check asks a ticket of (ARCHIVE-2: a write to an
    /// archived ticket is allowed), never what a view shows ([`Lens::live`]).
    pub fn all() -> Self {
        // ACCESS-10: the empty filter's, so a reach ANDs into it as text.
        Self { filter: Some(Arc::new(Filter::default())), ..Self::new(String::new(), Vec::new(), |_, _| true) }
    }

    /// ARCHIVE-2: the empty filter as a view reads it — every live ticket,
    /// the archive left out ([`Filter::matches`]'s default).
    pub fn live() -> Self {
        Self::of(&Filter::default()).expect("the empty filter compiles")
    }

    /// ARCHIVE-2: a reader's reach — `filter` judged as written, so an
    /// archived ticket in reach is still reached by name.
    pub(super) fn reach(filter: &Filter) -> Result<Self, StoreError> {
        Self::compiled(filter, true)
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
        Self::compiled(filter, false)
    }

    /// [`Lens::of`], or (`written`) [`Lens::reach`].
    fn compiled(filter: &Filter, written: bool) -> Result<Self, StoreError> {
        refuse(filter)?;
        // FILTER-36: every leaf, a nested group's at any depth.
        let leaves = filter.leaves();
        let preds = || leaves.iter().copied();
        let named: BTreeSet<String> = preds()
            .filter_map(|p| match &p.field {
                Field::In(e) => Some(e.to_ascii_uppercase()),
                _ => None,
            })
            .collect();
        let unders: BTreeSet<String> = preds()
            .filter_map(|p| match &p.field {
                Field::Under(x) => Some(crate::address::folded(x)),
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
        // ACCESS-21: the tickets it names alone, each once.
        let tickets: BTreeSet<String> = preds()
            .filter_map(|p| match &p.field {
                Field::Ticket(x) => Some(x.trim().to_ascii_uppercase()),
                _ => None,
            })
            .collect();
        if tickets.len() > UNDERS_MAX {
            return Err(StoreError::Filter(format!(
                "a filter names at most {UNDERS_MAX} tickets with `ticket`; this one names {}",
                tickets.len()
            )));
        }
        let judged = filter.clone();
        Ok(Self {
            text: filter.format(),
            named: named.into_iter().collect(),
            unders: unders.into_iter().collect(),
            history,
            tickets: tickets.into_iter().collect(),
            clock: Default::default(),
            judge: Arc::new(move |epic, key, lineage, holds| {
                let f = facts(epic, key, lineage, holds);
                if written {
                    judged.matches_as_written(&f)
                } else {
                    judged.matches(&f)
                }
            }),
            filter: Some(Arc::new(filter.clone())),
            written,
        })
    }

    /// FILTER-13: what `under` names, upper case, each once.
    pub(super) fn unders(&self) -> &[String] {
        &self.unders
    }

    /// The codes `in` names, upper case — ORG-3: letters, each naming every
    /// shortcode that has them.
    pub(super) fn named(&self) -> &[String] {
        &self.named
    }

    /// FILTER-13: whether the lens admits a ticket of `epic` in cell `key`
    /// whose lineage holds `lineage` (the shortcodes hosting `epic`, and the
    /// `under` names it is under).
    ///
    /// ORG-3: `letters`, the shortcode's code — what `in` matches; the
    /// lineage holds identities and the `under` names bound to them.
    pub(super) fn judge(&self, letters: &str, key: Key, lineage: &[&str]) -> bool {
        (self.judge)(letters, key, lineage, &[])
    }

    /// MENGER-13: [`Lens::judge`] with the history predicates `holds` names
    /// holding — a ticket's own truths, which no cell carries.
    pub(super) fn judge_holding(&self, letters: &str, key: Key, lineage: &[&str], holds: &[&str]) -> bool {
        (self.judge)(letters, key, lineage, holds)
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
    ///
    /// ORG-3: `in` names letters — every shortcode of `r` with them is judged
    /// on its own, by its letters.
    pub(super) fn admit(&self, r: &super::shortcodes::Registry, keys: &[Key], status: Option<Status>) -> Admit {
        debug_assert!(self.unders.is_empty(), "a lens with `under` is compiled with its lineage");
        debug_assert!(self.history.is_empty(), "a lens with history is compiled with the log");
        let named: BTreeSet<ShortcodeId> = self.named.iter().flat_map(|code| r.in_set(code)).collect();
        Admit::compile(keys, named, |e, k| {
            status.is_none_or(|s| k.status() == s) && (self.judge)(r.code(e).as_str(), k, &[], &[])
        })
    }
}

/// What the evaluator reads of a cell in a shortcode: everything but a
/// ticket's id and title — which only `text` reads, and it is not compiled.
/// FILTER-13: the lineage `under` reads is what the compile hands it: the
/// shortcode, the shortcodes hosting it, and the `under` names that hold.
/// MENGER-13: and the history predicates that hold, by their text. ORG-3:
/// `letters`, the shortcode's code — what `in` matches.
fn facts(letters: &str, key: Key, lineage: &[&str], holds: &[&str]) -> Facts {
    Facts {
        id: String::new(),
        epic: letters.to_string(),
        title: String::new(),
        status: key.status(),
        kind: key.kind(),
        severity: key.severity(),
        blocked: key.blocked(),
        has_commits: key.commits(),
        menger_doc: key.menger(),
        asking: key.asking(),
        labels: key.labels().labels().iter().map(|l| l.to_string()).collect(),
        archived: key.archived(),
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
        // FILTER-36: a name is the store's to resolve ([`Store::lens`]);
        // one that reaches a compile unresolved is not answered blind.
        if g.leaves().iter().any(|p| matches!(p.field, Field::Saved(_))) {
            why.push("a saved filter's name is resolved by the store first, and this read did not");
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

/// FILTER-4: the fields `facets` counts, in order — the vocabulary's list
/// (ACCESS-8), which [`crate::vocab::FacetField`] is checked against.
pub use crate::vocab::FACET_FIELDS;

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
    /// ARCHIVE-2: and how it is judged — the empty filter as a view reads
    /// it ([`Lens::live`]) is not the empty filter as written ([`Lens::all`]).
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text && self.written == other.written
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
/// when it is inside the sugar. ORG-3: `epic`, `parent` and `keep` as a
/// reader names them — resolved by the store when it cuts the page.
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
    Entered { id: String, shortcode: ShortcodeId, seq: u32 },
    /// …or out of it.
    Left { id: String, shortcode: ShortcodeId, seq: u32 },
    /// The write moved more than [`RECOUNT_OVER`] memberships: the filter's
    /// count of each shortcode it moved, as the write left it.
    Recount { shortcode: ShortcodeId, held: u64 },
}

/// FILTER-4: one `watch { filter }`'s filter, compiled — and compiled again
/// when a write names a cell interned since.
pub struct Watcher {
    pub(super) lens: Lens,
    /// FILTER-36: the text it was asked, names unresolved — read again when
    /// a saved filter it names changes ([`Watcher::resaved`]); `None` when
    /// it names none.
    pub(super) source: Option<String>,
    admit: Admit,
    /// FILTER-13: a filter that asks `under` — each write works out this
    /// watch's membership itself ([`super::lineage::UnderMoves`]), and its
    /// `admit` is not read. MENGER-13: and one that asks the log.
    under: Option<Arc<super::lineage::UnderWatch>>,
    /// MENGER-13: the moment its relative windows were last read at.
    read_at: i64,
    /// ORG-3: how many shortcodes the registry held when `admit` was
    /// compiled — a shortcode minted since may be one an `in` names.
    shortcodes: usize,
}

/// The matches of `q` under the read lock: the admitted bitmaps, and — for
/// a parent's page — inside its children. FILTER-13: compiled with the
/// lineage `under` reads — so every read through here (FILTER-15's too)
/// answers `under`.
///
/// ORG-3: `q`'s `epic` and `parent` resolved first ([`Sugar::of`]) — a
/// name that is ambiguous is refused, naming each.
pub(super) fn view<'c>(cache: &'c Cache, q: &Filtered) -> Result<View<'c>, StoreError> {
    view_of(cache, q, Sugar::of(cache, q)?, Compiled::read(cache, &q.lens, q.status)?)
}

/// ORG-3: [`view`] for a reach's own question (a grant's lens, `member`
/// asked by the gate) — a name that has come to name several holds nowhere,
/// never refusing the read it narrows.
pub(super) fn view_lenient<'c>(cache: &'c Cache, q: &Filtered) -> View<'c> {
    let sugar = Sugar::of(cache, q).unwrap_or(Sugar { epic: Some(ShortcodeId::new("")), parent: None });
    view_of(cache, q, sugar, Compiled::new(cache, &q.lens, q.status)).expect("a lenient view refuses nothing")
}

fn view_of<'c>(cache: &'c Cache, q: &Filtered, sugar: Sugar, c: Compiled<'c>) -> Result<View<'c>, StoreError> {
    let view = c.view(sugar.epic.as_ref());
    let view = match sugar.parent.as_deref() {
        None => view,
        Some(parent) => view.within(|epic| cache.children.in_shortcode(parent, epic).map(|seqs| seqs.bitmap().into_owned())),
    };
    Ok(match kept(cache, q, &sugar) {
        Some(t) => view.keeping(&t.shortcode, t.seq),
        None => view,
    })
}

/// ORG-3: a filtered read's sugar, resolved — the shortcode `epic` names
/// (one that names none is a shortcode with no tickets: nothing matches),
/// the ticket `parent` names (one that names none stays as written: a
/// parent only a hand edit names has children all the same).
pub(super) struct Sugar {
    pub(super) epic: Option<ShortcodeId>,
    pub(super) parent: Option<String>,
}

impl Sugar {
    pub(super) fn of(cache: &Cache, q: &Filtered) -> Result<Self, StoreError> {
        let epic = match q.epic.as_deref() {
            None => None,
            Some(e) => Some(cache.shortcode_named(e)?.unwrap_or_else(|| ShortcodeId::new(&e.trim().to_ascii_uppercase()))),
        };
        let parent = match q.parent.as_deref() {
            None => None,
            Some(p) => Some(match cache.find_ticket(p, super::shortcodes::OrgReach::All) {
                super::shortcodes::Found::One(key) => key,
                super::shortcodes::Found::Many(candidates) => {
                    return Err(StoreError::Ambiguous { text: p.trim().to_string(), candidates })
                }
                super::shortcodes::Found::None => p.trim().to_ascii_uppercase(),
            }),
        };
        Ok(Self { epic, parent })
    }
}

/// FILTER-24: the ticket a `keep` names — ORG-3: by any short form, or as
/// given, else upper-cased (as `member` takes an id).
pub(super) fn keep_row<'c>(cache: &'c Cache, id: &str) -> Option<&'c Row> {
    let id = id.trim();
    match cache.find_ticket(id, super::shortcodes::OrgReach::All) {
        super::shortcodes::Found::One(key) => cache.get(&key),
        _ => cache.get(id).or_else(|| cache.get(&id.to_ascii_uppercase())),
    }
}

/// FILTER-24: the ticket `q.keep` names, when it is inside `q`'s sugar —
/// its shortcode, its status, its parent — and so a match of `q` whatever
/// the filter says of it.
fn kept<'c>(cache: &'c Cache, q: &Filtered, sugar: &Sugar) -> Option<&'c Row> {
    let t = keep_row(cache, q.keep.as_deref()?)?;
    let inside = sugar.epic.as_ref().is_none_or(|e| *e == t.shortcode)
        && q.status.is_none_or(|s| s == t.status)
        && sugar.parent.as_deref().is_none_or(|p| cache.children.in_shortcode(p, &t.shortcode).is_some_and(|seqs| seqs.bitmap().contains(t.seq)));
    inside.then_some(t)
}

impl Store {
    /// FILTER-4: `counts { filter }` — each shortcode that holds a match,
    /// with how many, in shortcode order: the sum of the counts of the cells
    /// the filter admits there. The empty lens is each shortcode's tickets.
    ///
    /// ORG-3: by the shortcode's identity — two THEMEs are two counts.
    pub(super) fn counts_unscoped(&self, lens: &Lens) -> Vec<(ShortcodeId, u64)> {
        self.counts_keeping_unscoped(lens, None)
    }

    /// FILTER-24: [`Store::counts`] with `keep` — a ticket counted as a
    /// match whatever the filter says of it (once: a match is counted as it
    /// was).
    pub(super) fn counts_keeping_unscoped(&self, lens: &Lens, keep: Option<&str>) -> Vec<(ShortcodeId, u64)> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let view = Compiled::new(&cache, lens, None).view(None);
        let view = match keep.and_then(|id| keep_row(&cache, id)) {
            Some(t) => view.keeping(&t.shortcode, t.seq),
            None => view,
        };
        let out = view.counts().map(|(e, n)| (e.clone(), n)).collect();
        out
    }

    /// ORG-3: [`Store::counts_keeping`] for a read — an `under` name that
    /// names more than one in reach is refused, naming each.
    pub(super) fn counts_read_unscoped(&self, lens: &Lens, keep: Option<&str>) -> Result<Vec<(ShortcodeId, u64)>, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let view = Compiled::read(&cache, lens, None)?.view(None);
        let view = match keep.and_then(|id| keep_row(&cache, id)) {
            Some(t) => view.keeping(&t.shortcode, t.seq),
            None => view,
        };
        let out = view.counts().map(|(e, n)| (e.clone(), n)).collect();
        Ok(out)
    }

    /// ORG-3: a shortcode's letters and full address, as the registry says
    /// — what an event that names a shortcode by its identity carries beside
    /// it.
    pub fn shortcode_named_by(&self, sc: &ShortcodeId) -> (crate::address::Code, String) {
        let cache = self.tickets.read().expect("store lock poisoned");
        (cache.shortcodes.code(sc), cache.shortcodes.address(sc))
    }

    /// MENGER-13: [`Store::facets`] with a history filter's time read by
    /// `clock`.
    pub(super) fn facets_at_unscoped(
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
            let compiled = Compiled::read(&cache, &both, None)?;
            let mut by_epic: Vec<(&ShortcodeId, u64)> = Vec::new();
            for (epic, _) in cache.cells.shortcodes() {
                let mut in_epic = 0;
                for (cell, n) in compiled.cell_counts(epic) {
                    let key = cache.cells.keys()[cell as usize];
                    in_epic += n;
                    for field in fields.iter().filter(|f| **f != "in") {
                        if *field == "label" {
                            for l in key.labels().labels().iter() {
                                *labels.entry(l.to_string()).or_default() += n;
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
                // ORG-3: `in` names letters, so its menu counts each code —
                // namesakes summed, in the order the first of each comes.
                shortcodes = Vec::new();
                for (e, n) in by_epic {
                    let code = cache.shortcodes.code(e).as_str().to_string();
                    match shortcodes.iter_mut().find(|(c, _)| *c == code) {
                        Some((_, held)) => *held += n,
                        None => shortcodes.push((code, n)),
                    }
                }
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
    pub(super) fn filtered_page_unscoped(
        &self,
        q: &Filtered,
        after: Option<&str>,
        at: Option<u64>,
        limit: Option<u32>,
    ) -> Result<Page, StoreError> {
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
        let view = view(cache, q)?;
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
            .filter_map(|(epic, seq)| cache.order.get(&OrderKey { shortcode: epic.clone(), seq }).cloned())
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
            tickets.last().map(|t| t.id.to_string()).or_else(|| match (after, first) {
                (Some(id), _) => Some(id.to_string()),
                (None, 0) => None,
                (None, p) => view
                    .select(p - 1)
                    .and_then(|(epic, seq)| cache.order.get(&OrderKey { shortcode: epic.clone(), seq }))
                    .map(|t| t.id.to_string()),
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
    pub fn watcher_in(&self, lens: Lens, epic: Option<&ShortcodeId>) -> Watcher {
        // MENGER-13: a filter asking the log is a ticket's own truth as well
        // as its cell's — each write works it out, as it does `under`'s.
        if !lens.unders().is_empty() || lens.has_history() {
            let read_at = lens.clock().now();
            let under = self.unders.open(lens.clone(), epic.cloned());
            return Watcher {
                lens,
                source: None,
                admit: Admit::compile(&[], std::iter::empty(), |_, _| false),
                under: Some(under),
                read_at,
                shortcodes: 0,
            };
        }
        let (admit, shortcodes) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            (lens.admit(&cache.shortcodes, cache.cells.keys(), None), cache.shortcodes.len())
        };
        Watcher { lens, source: None, admit, under: None, read_at: 0, shortcodes }
    }

    /// ORG-3: how many shortcodes the registry holds — what a watcher's
    /// compile is current with.
    fn shortcodes_held(&self) -> usize {
        self.tickets.read().expect("store lock poisoned").shortcodes.len()
    }

    /// FILTER-27: the next moment after the lens's now at which time alone
    /// moves what it admits — the earliest of its history predicates' next
    /// boundaries ([`super::history::Log::next_boundary`]); `None` when
    /// none will move.
    pub fn next_boundary(&self, lens: &Lens) -> Option<i64> {
        let cache = self.tickets.read().expect("store lock poisoned");
        lens.history().iter().filter_map(|h| cache.next_boundary(h, lens.clock())).min()
    }

    /// MENGER-13: what time did to a `watch { filter }` whose filter has a
    /// relative window, since `then` — each ticket whose history truths
    /// differ between `then` and `now`, judged both ways against the store
    /// as it stands: `Entered` / `Left` for those it moved, in the store's
    /// order (in `epic` alone when one is asked); past [`RECOUNT_OVER`], one
    /// `Recount` per shortcode they are in. A write between the two moments
    /// was said by the write itself; a ticket it moved is only said again
    /// here when time moved it too.
    pub fn aged(&self, lens: &Lens, then: i64, now: i64, epic: Option<&ShortcodeId>) -> Vec<Membership> {
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
        let mut candidates: BTreeMap<ShortcodeId, roaring::RoaringBitmap> = b.history_candidates();
        for (e, bm) in a.history_candidates() {
            *candidates.entry(e).or_default() |= bm;
        }
        let mut moved: Vec<(ShortcodeId, u32, bool)> = Vec::new();
        for (e, cand) in candidates {
            if epic.is_some_and(|x| *x != e) {
                continue;
            }
            for seq in cand {
                let Some(t) = cache.order.get(&OrderKey { shortcode: e.clone(), seq }) else { continue };
                let (was, is) = (b.matches_row(t), a.matches_row(t));
                if was != is {
                    moved.push((e.clone(), seq, is));
                }
            }
        }
        if moved.len() > RECOUNT_OVER {
            let mut epics: Vec<&ShortcodeId> = moved.iter().map(|(e, _, _)| e).collect();
            epics.dedup();
            return epics.into_iter().map(|e| Membership::Recount { shortcode: e.clone(), held: a.matches_in(e).len() }).collect();
        }
        moved
            .into_iter()
            .map(|(shortcode, seq, now)| {
                let id = cache.order.get(&OrderKey { shortcode: shortcode.clone(), seq }).map_or_else(|| shortcode.key(seq), |t| t.id.to_string());
                if now {
                    Membership::Entered { id, shortcode, seq }
                } else {
                    Membership::Left { id, shortcode, seq }
                }
            })
            .collect()
    }
}

impl Watcher {
    /// ACCESS-12: the lens it admits by — its filter within its reach.
    pub fn lens(&self) -> &Lens {
        &self.lens
    }

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
    pub fn tick(&mut self, store: &Store, epic: Option<&ShortcodeId>) -> Vec<Membership> {
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
    pub fn membership(&mut self, store: &Store, moves: &Moves, epic: Option<&ShortcodeId>) -> Vec<Membership> {
        if self.under.is_some() {
            return Vec::new(); // the write's own `UnderMoved` says it
        }
        // ACT-5: a deleted ticket's `to` is no cell (`GONE`), never admitted.
        let highest = moves.moves.iter().flat_map(|m| m.from.into_iter().chain([m.to])).filter(|c| *c != super::cells::GONE).max();
        if highest.is_some_and(|c| c as usize >= self.admit.len()) || store.shortcodes_held() != self.shortcodes {
            // A cell interned after the compile: compile again — the table
            // only grows, and every id it held means what it meant. ORG-3:
            // or a shortcode minted since, which an `in` may name.
            let again = store.watcher(self.lens.clone());
            (self.admit, self.shortcodes) = (again.admit, again.shortcodes);
        }
        let admits = |e: &ShortcodeId, c: u32| self.admit.admits(e, c).unwrap_or(false);
        let moved: Vec<(&CellMove, bool)> = moves
            .moves
            .iter()
            .filter(|m| epic.is_none_or(|e| *e == m.shortcode))
            .filter_map(|m| {
                let was = m.from.is_some_and(|c| admits(&m.shortcode, c));
                let now = admits(&m.shortcode, m.to);
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
                    let (shortcode, seq) = (m.shortcode.clone(), m.seq);
                    let key = OrderKey { shortcode, seq };
                    let id = cache.order.get(&key).map_or_else(|| key.shortcode.key(seq), |t| t.id.to_string());
                    let shortcode = key.shortcode;
                    if now {
                        Membership::Entered { id, shortcode, seq }
                    } else {
                        Membership::Left { id, shortcode, seq }
                    }
                })
                .collect();
        }
        let counts = moves.counts.as_ref().expect("a write past the threshold carries its counts");
        let mut epics: Vec<&ShortcodeId> = moved.iter().map(|(m, _)| &m.shortcode).collect();
        epics.dedup();
        epics
            .into_iter()
            .map(|e| Membership::Recount {
                shortcode: e.clone(),
                held: counts
                    .get(e)
                    .map_or(0, |held| held.iter().filter(|(c, _)| admits(e, *c)).map(|(_, n)| n).sum()),
            })
            .collect()
    }
}
