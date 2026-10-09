//! WAVE-5: the wave board — `#/waves/<SHORTCODE>`, the order of work as a
//! kanban derived from `blocked_by` (WAVE-1).
//!
//! A board opens on its shortcode tree — the shortcode and every shortcode
//! hosted under it, at any depth (the nav's `host` chain from `groups`), kinds
//! build, spike and gap — and its filter builds any other (WAVE-12, WAVE-1
//! decision 16; src/filter.rs): a Linear-style menu whose Shortcode and Kind
//! say what `waves` is asked for, whose other categories narrow the answer
//! in the browser, and which lives in the route's query. (`f528177` made a
//! board one shortcode with the ones under it as links; the owner rejected
//! it.) The board asks the server where each of its tickets sits — one
//! `tickets.waves { epics, kinds }` per load (WAVE-4), computed over the
//! whole graph, never in the browser — and groups the answer into columns
//! here, in one of two layouts a toggle apart:
//!
//! - **waves**: a column per non-empty wave, ascending, then `missing
//!   blocker` when anything waits on an id that is no ticket, then `Done`,
//!   collapsed to its count until clicked;
//! - **stages**: `Blocked` (every ticket in wave 2 or later, every missing
//!   one, and a wave-1 ticket whose status is `Blocked`, each with its wave
//!   as a badge), then the wave-1 tickets by status — `Pending`, `Ready`,
//!   `Coding`, `In Review` — then `Done`, collapsed.
//!
//! A column keeps the order the server yields (Coding, In Review, Ready,
//! Pending, Blocked, Idea, Epic, then the store's order). A card is its id,
//! title, kind, status, severity when set, a marker when its status is
//! `Blocked`, and a chip for each ticket it waits on that this board does
//! not show (an **off-board blocker**); the card and every chip link to the
//! ticket's page. The columns, the tally and the Done count are the cards
//! the filter admits; an off-board blocker is one the answer holds no card
//! for. The layout is remembered per shortcode in localStorage — every read
//! and write allowed to fail, the board working without it.
//!
//! Live: the board lends the session an ear ([`session::listen`]). WAVE-6:
//! the server keeps every ticket's placement and says, once per write, which
//! moved (`placements_changed`) — however far downstream of the write, so a
//! card moves when a ticket two blockers back completes. The board applies
//! that to the cards it shows ([`apply`]): each moves to its new column, the
//! columns keep the server's order, and the tally (the `Done` count among
//! it) follows. A `status_changed` is applied too — the card's own row (its
//! stage, in the stages layout) and every card's `waiting_on`. `waves` is
//! asked again only for a ticket born into one of the board's shortcodes (a
//! card the board has not got), a `lagged`, or a new connection
//! ([`heard`]) — and once more behind an answer in flight when a change
//! lands while it is out, since that answer may predate it. The asks are
//! coalesced ([`Coalesce`]): one in flight, at most one queued behind it.
//! An answer that is an in-band error, or a call that fails, goes to the
//! app's error line and the last good board stays drawn. The cards' rows are
//! held in the row cache (src/cache.rs) while they are shown, so a title
//! moves on the card the moment `watch` carries it.
//!
//! FILTER-11: the board's filter is the lens's (src/lens.rs) — its bar the
//! generic one (src/filter.rs) with WAVE-12's categories (the server-backed
//! ones, and its local Wave, Readiness, Off-board blockers and Title) and
//! defaults, its counts from the held answer ([`counts`]). What it shows is
//! its effective filter, global ∧ waves: the effective Shortcode and Kind
//! groups decide the `epics` and `kinds` asked ([`epics`], [`kinds`]), and
//! every group is evaluated on the held cards with FILTER-10's
//! `Filter::matches` over each card's facts ([`facts_of`], [`narrow`]) —
//! the local categories as WAVE-12 evaluates them. A filter that admits
//! nothing is the empty board, the notice under the bar naming the clash.
//!
//! Everything that decides — the columns, the off-board test, what an event
//! does, how a change moves the cards, the coalescing, the stored layout,
//! what is asked and what is shown — is a pure function here, pinned by
//! native tests.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use leptos::prelude::*;
use menger_tickets_api::{Placement, Severity, Status, TicketSummary, TicketsEvent};
use std::cmp::Ordering;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use menger_tickets_api::filter::{Facts, Filter};

use crate::filter::{Cat, Clauses, Counts, CountsProvider, FilterBar, Op, ScTree, WORK_KINDS};
use crate::keys::palette_match;
use crate::lens::{self, Level};
use crate::nav::kind_glyph;
use crate::notes::BoardNotes;
use crate::tags::Tag;
use crate::{cache, dom, nav, session, st_class};

// ---------------------------------------------------------------------
// The shortcodes

/// The shortcode a ticket id is in: what comes before its last `-`. (A
/// board's shortcodes are its filter's — src/filter.rs.)
pub(crate) fn epic_of(id: &str) -> Option<&str> {
    id.rsplit_once('-').map(|(epic, _)| epic)
}

// ---------------------------------------------------------------------
// The layouts

/// The board's two layouts (WAVE-1 decision 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layout {
    Waves,
    Stages,
}

impl Layout {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Layout::Waves => "waves",
            Layout::Stages => "stages",
        }
    }

    /// What was stored, or the default — waves — for anything else (nothing
    /// stored, storage refused, a stale value).
    pub(crate) fn parse(stored: Option<&str>) -> Self {
        match stored {
            Some("stages") => Layout::Stages,
            _ => Layout::Waves,
        }
    }
}

/// Where a shortcode's layout is remembered.
pub(crate) fn layout_key(shortcode: &str) -> String {
    format!("menger.waves.{}", shortcode.to_ascii_uppercase())
}

fn storage() -> Option<web_sys::Storage> {
    // `Err`: the browser refused storage (a sandboxed frame, a privacy
    // mode) — the board works without it.
    dom::window().local_storage().ok().flatten()
}

fn load_layout(shortcode: &str) -> Layout {
    Layout::parse(storage().and_then(|s| s.get_item(&layout_key(shortcode)).ok().flatten()).as_deref())
}

fn save_layout(shortcode: &str, layout: Layout) {
    if let Some(s) = storage() {
        let _ = s.set_item(&layout_key(shortcode), layout.as_str());
    }
}

/// One placed ticket, as `waves` answered it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Card {
    pub(crate) summary: TicketSummary,
    pub(crate) placement: Placement,
    pub(crate) waiting_on: Vec<String>,
}

/// What a column stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Lane {
    Wave(u32),
    Missing,
    /// The stages layout's derived column.
    Blocked,
    /// A stage column: `Pending`, `Ready`, `Coding` or `In Review`.
    Stage(Status),
    Done,
}

impl Lane {
    /// The column's heading (a `Done` column adds its count).
    pub(crate) fn title(&self) -> String {
        match self {
            Lane::Wave(n) => format!("wave {n}"),
            Lane::Missing => "missing blocker".into(),
            Lane::Blocked => "Blocked".into(),
            Lane::Stage(s) => s.as_str().into(),
            Lane::Done => "Done".into(),
        }
    }

    /// The column's `data-lane` — what a harness finds it by.
    pub(crate) fn slug(&self) -> String {
        match self {
            Lane::Wave(n) => format!("wave-{n}"),
            Lane::Missing => "missing".into(),
            Lane::Blocked => "blocked".into(),
            Lane::Stage(s) => s.as_str().to_ascii_lowercase().replace(' ', "-"),
            Lane::Done => "done".into(),
        }
    }
}

/// A card as a column draws it: the card, and its badge — its wave, on a
/// card in the stages layout's `Blocked` column.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placed {
    pub(crate) card: Card,
    pub(crate) badge: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Column {
    pub(crate) lane: Lane,
    pub(crate) cards: Vec<Placed>,
}

/// The stage columns of the wave-1 tickets, in board order.
const STAGES: [Status; 4] = [Status::Pending, Status::Ready, Status::Coding, Status::InReview];

/// The columns `cards` (in the order `waves` yields them) fall into under
/// `layout`. Each column keeps that order.
///
/// Waves: a column per wave that holds a card, ascending (an empty wave —
/// one whose tickets are all on other boards — has none); `missing blocker`
/// only when a card is; `Done` always, collapsed by the view.
///
/// Stages: `Blocked`, `Pending`, `Ready`, `Coding`, `In Review`, `Done`,
/// always. `Blocked` holds every card in wave 2 or later, every missing one
/// and every wave-1 card whose status is `Blocked`, each badged with its
/// wave (`w3`, or `missing`); the other wave-1 cards go to their status's
/// column — and an `Idea` or an `Epic`, which has no stage of its own, to
/// `Pending`, so no card is ever left off.
pub(crate) fn columns(cards: &[Card], layout: Layout) -> Vec<Column> {
    let plain = |c: &Card| Placed { card: c.clone(), badge: None };
    match layout {
        Layout::Waves => {
            let mut waves: BTreeMap<u32, Vec<Placed>> = BTreeMap::new();
            let (mut missing, mut done) = (Vec::new(), Vec::new());
            for c in cards {
                match c.placement {
                    Placement::Wave { n } => waves.entry(n).or_default().push(plain(c)),
                    Placement::MissingBlocker => missing.push(plain(c)),
                    Placement::Done => done.push(plain(c)),
                }
            }
            let mut out: Vec<Column> = waves.into_iter().map(|(n, cards)| Column { lane: Lane::Wave(n), cards }).collect();
            if !missing.is_empty() {
                out.push(Column { lane: Lane::Missing, cards: missing });
            }
            out.push(Column { lane: Lane::Done, cards: done });
            out
        }
        Layout::Stages => {
            let mut blocked = Vec::new();
            let mut stages: Vec<Vec<Placed>> = vec![Vec::new(); STAGES.len()];
            let mut done = Vec::new();
            for c in cards {
                let status = c.summary.status;
                match c.placement {
                    Placement::Done => done.push(plain(c)),
                    Placement::MissingBlocker => blocked.push(Placed { card: c.clone(), badge: Some("missing".into()) }),
                    Placement::Wave { n } if n >= 2 || status == Status::Blocked => {
                        blocked.push(Placed { card: c.clone(), badge: Some(format!("w{n}")) })
                    }
                    Placement::Wave { .. } => {
                        let at = STAGES.iter().position(|s| *s == status).unwrap_or(0);
                        stages[at].push(plain(c));
                    }
                }
            }
            let mut out = vec![Column { lane: Lane::Blocked, cards: blocked }];
            out.extend(STAGES.iter().zip(stages).map(|(s, cards)| Column { lane: Lane::Stage(*s), cards }));
            out.push(Column { lane: Lane::Done, cards: done });
            out
        }
    }
}

/// A card's off-board blockers: each id it waits on that is not a card of
/// this board, in its `waiting_on` order.
pub(crate) fn off_board(card: &Card, on_board: &HashSet<String>) -> Vec<String> {
    card.waiting_on.iter().filter(|id| !on_board.contains(*id)).cloned().collect()
}

/// What a `watch` event does to an open board (WAVE-6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    /// Ask `waves` again: a ticket born into one of the board's shortcodes
    /// (`epics`) — a card it has not got — or a `lagged`, after which
    /// anything may have moved; SDLC-13: or a kind changed there.
    Ask,
    /// Apply it to the cards ([`apply`]): a `placements_changed`, a
    /// `status_changed`.
    Apply,
    /// Nothing the cards show.
    Nothing,
}

/// What `ev` does to a board of the shortcodes `epics`. A birth elsewhere —
/// even of an id a card waits on — asks nothing: the cards it moves out of
/// the missing lane arrive in the `placements_changed` behind it.
pub(crate) fn heard(ev: &TicketsEvent, epics: &BTreeSet<String>) -> Heard {
    match ev {
        TicketsEvent::Created { ticket } if epics.contains(&ticket.epic) => Heard::Ask,
        // SDLC-13: a kind changed in one of its shortcodes — a doc made work
        // is a card it has not got, work made a doc a card it must not show,
        // and a kind the board filters on may let it in or out. Elsewhere, it
        // is a status change to the cards that wait on it.
        TicketsEvent::KindChanged { ticket, .. } if epics.contains(&ticket.epic) => Heard::Ask,
        TicketsEvent::Lagged { .. } => Heard::Ask,
        TicketsEvent::PlacementsChanged { .. } | TicketsEvent::StatusChanged { .. } | TicketsEvent::KindChanged { .. } => {
            Heard::Apply
        }
        _ => Heard::Nothing,
    }
}

/// Where a status sorts inside a wave — the server's order (WAVE-4): the
/// work under way, then what can start, then what waits.
fn rank(status: Status) -> u8 {
    match status {
        Status::Coding => 0,
        Status::InReview => 1,
        Status::Ready => 2,
        Status::Pending => 3,
        Status::Blocked => 4,
        Status::Idea => 5,
        Status::Epic => 6,
        Status::Complete | Status::Superseded | Status::Doc => 7,
    }
}

/// The order `waves` yields cards in: waves ascending, inside a wave by
/// [`rank`]; then the missing lane; then the done — each then in the store's
/// order, `(epic, seq)`.
fn yield_order(a: &Card, b: &Card) -> Ordering {
    let key = |c: &Card| match c.placement {
        Placement::Wave { n } => (0u8, n, rank(c.summary.status)),
        Placement::MissingBlocker => (1, 0, 0),
        Placement::Done => (2, 0, 0),
    };
    key(a)
        .cmp(&key(b))
        .then_with(|| a.summary.epic.cmp(&b.summary.epic))
        .then_with(|| a.summary.seq.cmp(&b.summary.seq))
}

/// The tally of `cards`, as `wave_counts` gives it.
pub(crate) fn retally(a: &mut Answer) {
    let (mut waves, mut missing, mut done) = (Vec::<u64>::new(), 0, 0);
    for c in &a.cards {
        match c.placement {
            Placement::Wave { n } => {
                let i = n as usize - 1;
                if waves.len() <= i {
                    waves.resize(i + 1, 0);
                }
                waves[i] += 1;
            }
            Placement::MissingBlocker => missing += 1,
            Placement::Done => done += 1,
        }
    }
    (a.waves, a.missing, a.done) = (waves, missing, done);
}

/// Whether a status is done with — MNG-4's line: `Complete` or `Superseded`.
fn satisfied(s: Status) -> bool {
    matches!(s, Status::Complete | Status::Superseded)
}

/// WAVE-6: `ev` applied to the board's answer — each `placements_changed`
/// card moved to where the server now keeps it; a `status_changed` card's
/// row (its stage) taken, and the changed ticket added to or taken from
/// every card's `waiting_on` as it crossed the line; then the cards back in
/// the server's order and the tally recounted. True when a card moved.
pub(crate) fn apply(a: &mut Answer, ev: &TicketsEvent) -> bool {
    let mut moved = false;
    match ev {
        TicketsEvent::PlacementsChanged { changes } => {
            for change in changes {
                if let Some(card) = a.cards.iter_mut().find(|c| c.summary.id == change.id) {
                    if card.placement != change.placement {
                        card.placement = change.placement;
                        card.summary.placement = Some(change.placement);
                        moved = true;
                    }
                }
            }
        }
        TicketsEvent::StatusChanged { ticket, .. } | TicketsEvent::KindChanged { ticket, .. } => {
            let done = satisfied(ticket.status);
            for card in &mut a.cards {
                if card.summary.id == ticket.id {
                    let placement = card.placement;
                    card.summary = TicketSummary::from(ticket);
                    // Its placement is the board's to move, from the
                    // `placements_changed` behind this event.
                    card.summary.placement = Some(placement);
                    moved = true;
                }
                if card.summary.blocked_by.iter().any(|b| *b == ticket.id) {
                    let waits = card.waiting_on.iter().any(|w| *w == ticket.id);
                    if done && waits {
                        card.waiting_on.retain(|w| *w != ticket.id);
                        moved = true;
                    } else if !done && !waits {
                        // Back in `blocked_by` order, distinct.
                        let mut next: Vec<String> = Vec::new();
                        for b in &card.summary.blocked_by {
                            if (*b == ticket.id || card.waiting_on.contains(b)) && !next.contains(b) {
                                next.push(b.clone());
                            }
                        }
                        card.waiting_on = next;
                        moved = true;
                    }
                }
            }
        }
        _ => {}
    }
    if moved {
        a.cards.sort_by(yield_order);
        retally(a);
    }
    moved
}

/// The asks, coalesced: at most one request in flight and one queued behind
/// it — however many events ask while one is out, one more request follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Coalesce {
    inflight: bool,
    queued: bool,
}

impl Coalesce {
    /// Something wants the board asked again: true when the request is to go
    /// now; false when one is out already (this one is queued behind it).
    pub(crate) fn ask(&mut self) -> bool {
        if self.inflight {
            self.queued = true;
            false
        } else {
            self.inflight = true;
            true
        }
    }

    /// WAVE-6: a request is out — an answer may land that predates a change
    /// applied meanwhile, so the change asks once more behind it.
    pub(crate) fn busy(&self) -> bool {
        self.inflight
    }

    /// The request in flight was answered: true when the queued one is to go
    /// now (it is then the one in flight).
    pub(crate) fn answered(&mut self) -> bool {
        if self.queued {
            self.queued = false;
            true
        } else {
            self.inflight = false;
            false
        }
    }
}

/// One answer of `waves`: the cards in yield order, and the tally.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct Answer {
    pub(crate) cards: Vec<Card>,
    pub(crate) waves: Vec<u64>,
    pub(crate) missing: u64,
    pub(crate) done: u64,
}

/// `waves`' events as an answer — or the first in-band error.
pub(crate) fn answer_of(events: Vec<TicketsEvent>) -> Result<Answer, String> {
    let mut a = Answer::default();
    for ev in events {
        match ev {
            TicketsEvent::Placed { summary, placement, waiting_on } => a.cards.push(Card { summary, placement, waiting_on }),
            TicketsEvent::WaveCounts { waves, missing, done } => {
                a.waves = waves;
                a.missing = missing;
                a.done = done;
            }
            TicketsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Ok(a)
}

/// The board's tally, in words: "12 in 4 waves · 1 missing · 17 done".
pub(crate) fn tally(a: &Answer) -> String {
    let open: u64 = a.waves.iter().sum();
    let waves = a.waves.iter().filter(|n| **n > 0).count();
    let mut out = format!("{open} in {waves} wave{}", if waves == 1 { "" } else { "s" });
    if a.missing > 0 {
        out.push_str(&format!(" · {} missing", a.missing));
    }
    out.push_str(&format!(" · {} done", a.done));
    out
}

// ---------------------------------------------------------------------
// The board's filter (WAVE-12, on the lens since FILTER-11)

/// What the board's filter is: its effective filter (global ∧ waves, the
/// language's) and its local clauses (WAVE-12's Wave, Readiness, Off-board
/// blockers, Title).
pub(crate) type Effective = (Filter, Clauses);

/// The shortcodes `waves` is asked for: every group Shortcode draws in the
/// effective filter (`is`: its values; `is not`: the route's tree without
/// them), all of them; with none, every shortcode. A group no category
/// draws asks for nothing narrower — it is evaluated on the cards.
pub(crate) fn epics(eff: &Filter, route: &str, tree: &ScTree) -> BTreeSet<String> {
    let mut out: Option<BTreeSet<String>> = None;
    for (_, cl) in eff.groups.iter().filter_map(lens::clause_of).filter(|(c, _)| *c == Cat::Shortcode) {
        let these: BTreeSet<String> = match cl.op {
            Op::Is => cl.values.iter().cloned().collect(),
            Op::Not => tree.subtree(&route.to_ascii_uppercase()).into_iter().filter(|e| !cl.values.contains(e)).collect(),
        };
        out = Some(match out {
            Some(o) => o.intersection(&these).cloned().collect(),
            None => these,
        });
    }
    out.unwrap_or_else(|| tree.known().clone())
}

/// The kinds `waves` is asked for: the kinds of work every group Kind draws
/// admits (`is`: its values; `is not`: the rest); with none, every kind of
/// work. Docs are never cards.
pub(crate) fn kinds(eff: &Filter) -> Vec<String> {
    let mut out: Vec<String> = WORK_KINDS.iter().map(|k| k.to_string()).collect();
    for (_, cl) in eff.groups.iter().filter_map(lens::clause_of).filter(|(c, _)| *c == Cat::Kind) {
        out.retain(|k| cl.values.contains(k) == (cl.op == Op::Is));
    }
    out
}

/// What the language reads of a card (FILTER-10's `Facts`): its summary;
/// `blocked` — it waits on something (`waiting_on`, its unsatisfied
/// blockers); `lineage` — its id, its ancestors as far as `parent_of`
/// knows them, its shortcode, and what hosts that shortcode (`tree`).
pub(crate) fn facts_of(card: &Card, tree: Option<&ScTree>, parent_of: &dyn Fn(&str) -> Option<String>) -> Facts {
    let s = &card.summary;
    let mut lineage = vec![s.id.clone()];
    let mut at = s.parent.clone();
    while let Some(p) = at.take() {
        if lineage.contains(&p) {
            break;
        }
        at = parent_of(&p);
        lineage.push(p);
    }
    lineage.push(s.epic.clone());
    if let Some(t) = tree {
        lineage.extend(t.hosting(&s.epic));
    }
    Facts {
        id: s.id.clone(),
        epic: s.epic.clone(),
        title: s.title.clone(),
        status: s.status,
        kind: s.kind,
        severity: s.severity,
        blocked: !card.waiting_on.is_empty(),
        has_commits: !s.commits.is_empty(),
        menger_doc: menger_tickets_api::is_menger_doc(s.path.as_deref()),
        lineage,
    }
}

/// `card`'s value in category `c` (none for Title, which matches text; and
/// none for a done card's readiness). `off`: whether it waits on a ticket
/// the answer holds no card for.
pub(crate) fn value_of(c: Cat, card: &Card, off: bool) -> Option<String> {
    let s = &card.summary;
    let flag = |b: bool| Some(if b { "yes" } else { "no" }.to_string());
    match c {
        Cat::Shortcode => Some(s.epic.clone()),
        Cat::Status => Some(s.status.as_str().into()),
        Cat::Kind => Some(s.kind.as_str().into()),
        Cat::Severity => Some(s.severity.map_or("none".to_string(), |v| v.as_str().to_ascii_lowercase())),
        Cat::Ready => flag(s.status == Status::Ready && card.waiting_on.is_empty()),
        Cat::Blocked => flag(!card.waiting_on.is_empty()),
        Cat::Commits => flag(!s.commits.is_empty()),
        Cat::MengerDocs => flag(menger_tickets_api::is_menger_doc(s.path.as_deref())),
        Cat::Wave => Some(match card.placement {
            Placement::Wave { n } => n.to_string(),
            Placement::MissingBlocker => "missing".into(),
            Placement::Done => "done".into(),
        }),
        Cat::Readiness => match card.placement {
            Placement::Wave { n: 1 } => Some("workable".into()),
            Placement::Wave { .. } => Some("waiting".into()),
            Placement::MissingBlocker => Some("missing".into()),
            Placement::Done => None,
        },
        Cat::OffBoard => Some(if off { "some" } else { "none" }.into()),
        Cat::Title | Cat::State | Cat::Removed => None,
    }
}

/// Whether `card` passes the local clauses `k` — WAVE-12's evaluation.
pub(crate) fn admits_local(k: &Clauses, card: &Card, off: bool) -> bool {
    k.iter().all(|(c, cl)| match c {
        Cat::Title => palette_match(&card.summary.id, &card.summary.title, &cl.values[0]) == (cl.op == Op::Is),
        c => cl.admits(value_of(c, card, off).as_deref()),
    })
}

fn ids(a: &Answer) -> HashSet<String> {
    a.cards.iter().map(|c| c.summary.id.clone()).collect()
}

/// The answer narrowed to the cards the effective filter admits — every
/// group evaluated with `Filter::matches` on the card's facts, the local
/// clauses as WAVE-12 evaluates them — the tally recounted: what the
/// columns, the tally and the Done count show.
pub(crate) fn narrow(a: &Answer, eff: &Effective, tree: Option<&ScTree>) -> Answer {
    let on = ids(a);
    let parents: HashMap<&str, &str> = a.cards.iter().filter_map(|c| Some((c.summary.id.as_str(), c.summary.parent.as_deref()?))).collect();
    let parent_of = |id: &str| -> Option<String> {
        parents.get(id).map(|p| p.to_string()).or_else(|| cache::get(id).and_then(|r| r.try_with_untracked(|t| t.parent.clone()).flatten()))
    };
    let (filter, local) = eff;
    let mut out = Answer {
        cards: a
            .cards
            .iter()
            .filter(|c| {
                let off = !off_board(c, &on).is_empty();
                admits_local(local, c, off) && filter.matches(&facts_of(c, tree, &parent_of))
            })
            .cloned()
            .collect(),
        ..Answer::default()
    };
    retally(&mut out);
    out
}

/// Per category, per value: how many of the answer's cards have it — a
/// value no card has counts 0 (Shortcode: among the shortcodes asked; one
/// not asked shows no count).
pub(crate) fn counts(a: &Answer, asked: &BTreeSet<String>) -> Counts {
    let on = ids(a);
    let mut out: Counts = HashMap::new();
    for c in Cat::ALL {
        let m = out.entry(c).or_default();
        for v in c.fixed() {
            m.insert(v.to_string(), 0);
        }
    }
    out.entry(Cat::Shortcode).or_default().extend(asked.iter().map(|e| (e.clone(), 0)));
    for card in &a.cards {
        let off = !off_board(card, &on).is_empty();
        for c in Cat::ALL {
            if let Some(v) = value_of(c, card, off) {
                *out.entry(c).or_default().entry(v).or_default() += 1;
            }
        }
    }
    out.remove(&Cat::Title);
    out
}

// ---------------------------------------------------------------------
// The view

/// What the board's asks share — every handle `Copy`, so the ear, the
/// effects and the request each take their own.
#[derive(Clone, Copy)]
struct Ctx {
    /// The shortcodes and the kinds asked for — the filter's (WAVE-12);
    /// `None` until the groups are in.
    set: Memo<Option<BTreeSet<String>>>,
    kinds: Memo<Option<Vec<String>>>,
    answer: RwSignal<Option<Answer>>,
    coalesce: StoredValue<Coalesce>,
    holder: StoredValue<cache::Holder>,
    last_error: RwSignal<Option<String>>,
}

/// Ask `waves` again — now, or behind the request in flight.
fn ask(ctx: Ctx) {
    if ctx.coalesce.try_update_value(Coalesce::ask) == Some(true) {
        spawn_local(fetch(ctx));
    }
}

/// The request, and every one queued behind it while it was out.
async fn fetch(ctx: Ctx) {
    loop {
        // `try_`: the board can have been left while a request was out.
        let (Some(Some(epics)), Some(Some(kinds))) = (ctx.set.try_get_untracked(), ctx.kinds.try_get_untracked()) else {
            ctx.coalesce.try_update_value(|c| *c = Coalesce::default());
            return;
        };
        // WAVE-12: a filter that leaves no shortcode or no kind is an empty
        // board — nothing to ask (`waves` refuses an empty `epics`).
        if epics.is_empty() || kinds.is_empty() {
            take(ctx, Answer::default());
            if ctx.coalesce.try_update_value(Coalesce::answered) != Some(true) {
                return;
            }
            continue;
        }
        let Some(client) = session::client() else {
            // Not connected: the next connection asks (the socket ticks).
            ctx.coalesce.try_update_value(|c| *c = Coalesce::default());
            return;
        };
        match client.waves_all(epics.into_iter().collect(), Some(kinds)).await {
            Ok(events) => match answer_of(events) {
                Ok(a) => take(ctx, a),
                Err(message) => {
                    ctx.last_error.try_set(Some(format!("waves: {message}")));
                }
            },
            Err(e) => {
                ctx.last_error.try_set(Some(format!("waves: {}", session::describe(&e))));
            }
        }
        if ctx.coalesce.try_update_value(Coalesce::answered) != Some(true) {
            return;
        }
    }
}

/// A good answer: its rows held in the row cache (and only its rows), then
/// drawn.
fn take(ctx: Ctx, a: Answer) {
    let Some(holder) = ctx.holder.try_get_value() else { return };
    let ids: HashSet<String> = a.cards.iter().map(|c| c.summary.id.clone()).collect();
    holder.hold_page(a.cards.iter().map(|c| c.summary.clone()).collect());
    holder.retain(|id| ids.contains(id));
    ctx.answer.try_set(Some(a));
}

/// `#/waves/<SHORTCODE>` — or, with no shortcode, `#/waves`: the top-level
/// shortcodes, each a link to its board. FILTER-11: the board's filter is
/// the lens's waves level (`f=` in the route's query, kept per shortcode),
/// under the global one.
#[component]
pub(crate) fn WaveBoard(
    shortcode: String,
    tree: nav::Tree,
    /// The socket's generation (main.rs): it ticks on every new connection,
    /// which asks again.
    socket: RwSignal<u32>,
    last_error: RwSignal<Option<String>>,
) -> impl IntoView {
    if shortcode.is_empty() {
        return view! { <WaveIndex tree /> }.into_any();
    }
    let sc = shortcode.to_ascii_uppercase();
    // FILTER-11: the board's filter is the lens's — global ∧ waves; the
    // lens reads it from the URL (or storage, or the board's default) and
    // writes it back. Its Shortcode and Kind say what `waves` is asked for.
    let lens = lens::use_lens();
    let sctree = lens.sctree();
    let effective = lens.effective(Level::Waves);
    // Only while the lens's waves level is this board's: a move to another
    // board changes it before this one is gone, and this one asks nothing
    // for it.
    let eff = {
        let sc = sc.clone();
        Memo::new(move |_| {
            (lens.route_sc(Level::Waves).as_deref() == Some(sc.as_str())).then(|| lens.kept(Level::Waves).map(|w| (effective.get(), w.local))).flatten()
        })
    };
    let set = {
        let sc = sc.clone();
        Memo::new(move |_| {
            eff.with(|f| sctree.with(|t| match (f, t) {
                (Some((f, _)), Some(t)) => Some(epics(f, &sc, t)),
                _ => None,
            }))
        })
    };
    let kinds = Memo::new(move |_| eff.with(|f| f.as_ref().map(|(f, _)| kinds(f))));
    let layout = RwSignal::new(load_layout(&sc));
    {
        let sc = sc.clone();
        Effect::new(move |prev: Option<Layout>| {
            let now = layout.get();
            if prev.is_some_and(|p| p != now) {
                save_layout(&sc, now);
            }
            now
        });
    }
    let done_open = RwSignal::new(false);
    let ctx = Ctx {
        set,
        kinds,
        answer: RwSignal::new(None),
        coalesce: StoredValue::new(Coalesce::default()),
        holder: StoredValue::new(cache::Holder::new()),
        last_error,
    };

    // A load: on entry once the groups say what the set is, on a new
    // connection, and when the set or the kinds change (a filter's Shortcode
    // or Kind — its other categories ask nothing).
    Effect::new(move |_| {
        if socket.get() == 0 || set.with(Option::is_none) {
            return; // no connection, or no groups yet: this re-runs when there are
        }
        kinds.track();
        ask(ctx);
    });

    // Live (WAVE-6): the cards move as the server says they do; a card the
    // board has not got, or a lag, asks again.
    session::listen(move |ev| {
        let Some(Some(epics)) = set.try_get_untracked() else {
            return;
        };
        match heard(ev, &epics) {
            Heard::Ask => ask(ctx),
            Heard::Apply => {
                let moved = ctx
                    .answer
                    .try_maybe_update(|a| {
                        let moved = a.as_mut().is_some_and(|a| apply(a, ev));
                        (moved, moved)
                    })
                    .unwrap_or(false);
                // An answer out now may predate this change: one more behind it.
                if moved && ctx.coalesce.try_with_value(Coalesce::busy) == Some(true) {
                    ask(ctx);
                }
            }
            Heard::Nothing => {}
        }
    });

    let on_board = Memo::new(move |_| {
        ctx.answer.with(|a| a.iter().flat_map(|a| a.cards.iter()).map(|c| c.summary.id.clone()).collect::<HashSet<String>>())
    });
    // WAVE-11: what the annotation's `{wave: ID}`s read for a card.
    let placements = Memo::new(move |_| {
        ctx.answer.with(|a| {
            a.iter().flat_map(|a| a.cards.iter()).map(|c| (c.summary.id.clone(), c.placement)).collect::<HashMap<String, Placement>>()
        })
    });
    // WAVE-12: what the filter admits — the columns, the tally, the Done
    // count; live changes to the answer re-filter.
    let shown = Memo::new(move |_| {
        ctx.answer.with(|a| {
            a.as_ref().map(|a| eff.with(|f| sctree.with(|t| f.as_ref().map_or_else(|| a.clone(), |f| narrow(a, f, t.as_ref())))))
        })
    });
    let cols = Memo::new(move |_| {
        let l = layout.get();
        shown.with(|a| a.as_ref().map(|a| columns(&a.cards, l)).unwrap_or_default())
    });
    let lanes = Memo::new(move |_| cols.with(|cs| cs.iter().map(|c| c.lane.clone()).collect::<Vec<_>>()));
    let counts = CountsProvider::new(move || {
        ctx.answer.with(|a| set.with(|e| a.as_ref().map(|a| counts(a, e.as_ref().unwrap_or(&BTreeSet::new()))).unwrap_or_default()))
    });
    // Owner, 2026-10-06: the columns are the page's height, all of them, and
    // scroll inside. The lanes row takes what the viewport has left below
    // the header and the bar (which wraps on a narrow window, so it is
    // measured, not assumed); the annotation stays below, one scroll away.
    let lanes_ref = NodeRef::<leptos::html::Div>::new();
    let lanes_h = RwSignal::new(None::<f64>);
    let measure = move || {
        let Some(el) = lanes_ref.get_untracked() else { return };
        let el: &web_sys::Element = el.as_ref();
        // The content pane is the scroller and starts at the viewport's top,
        // so the lanes sit `top + scrolled` into the page.
        let scrolled = el.closest(".content").ok().flatten().map_or(0.0, |c| f64::from(c.scroll_top()));
        let top = el.get_bounding_client_rect().top() + scrolled;
        let vh = dom::window().inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
        lanes_h.set(Some(lane_height(vh, top)));
    };
    Effect::new(move |_| {
        // Whatever moves the lanes' top: the answer (the "placing…" line
        // above them comes and goes), the layout, and the filter (its pills
        // can wrap the bar).
        ctx.answer.track();
        layout.track();
        eff.track();
        request_animation_frame(measure);
    });
    let resize = window_event_listener(leptos::ev::resize, move |_| measure());
    on_cleanup(move || resize.remove());

    let seg = move |l: Layout| {
        view! {
            <button
                class="seg"
                class:on=move || layout.get() == l
                aria-pressed=move || (layout.get() == l).to_string()
                on:click=move |_| layout.set(l)
            >
                {l.as_str()}
            </button>
        }
    };

    view! {
        // FILTER-22: a tag on a card builds the wave board's filter.
        <section id="waves" data-shortcode=sc.clone() data-tag-level="waves" data-layout=move || layout.get().as_str()>
            <div class="wbar">
                <FilterBar level=Level::Waves cats=Cat::WAVES.to_vec() counts />
                <span class="layout" role="group" aria-label="layout">"[ "{seg(Layout::Waves)}" | "{seg(Layout::Stages)}" ]"</span>
                <span class="tally">{move || shown.with(|a| a.as_ref().map(tally))}</span>
            </div>
            {move || {
                ctx.answer.with(Option::is_none).then(|| view! { <p class="loading">"placing the work…"</p> })
            }}
            {move || {
                shown.with(|a| a.as_ref().is_some_and(|a| a.cards.is_empty())).then(|| view! {
                    <p class="none">"no card matches this filter"</p>
                })
            }}
            <div
                class="lanes"
                node_ref=lanes_ref
                style=move || lanes_h.get().map(|h| format!("height: {h:.0}px")).unwrap_or_default()
            >
                <For
                    each=move || lanes.get()
                    key=Lane::slug
                    children=move |lane: Lane| lane_view(lane, cols, on_board, done_open)
                />
            </div>
            // WAVE-11: the board's annotation, below it in either layout.
            <BoardNotes root=sc.clone() cards=placements socket last_error />
        </section>
    }
    .into_any()
}

/// Air left under the lanes row, and the shortest it may be — a viewport too
/// small for the bar still shows a few cards per column.
const LANE_AIR: f64 = 16.0;
const LANE_MIN: f64 = 240.0;

/// The lanes row's height: what the viewport has left below the lanes'
/// top, less a little air — every column is this tall and scrolls inside.
fn lane_height(viewport: f64, top: f64) -> f64 {
    (viewport - top - LANE_AIR).max(LANE_MIN)
}

/// One column: its heading and its cards. `Done` is collapsed to its count
/// until its heading is clicked.
fn lane_view(lane: Lane, cols: Memo<Vec<Column>>, on_board: Memo<HashSet<String>>, done_open: RwSignal<bool>) -> impl IntoView {
    let is_done = lane == Lane::Done;
    let cards = {
        let lane = lane.clone();
        Memo::new(move |_| cols.with(|cs| cs.iter().find(|c| c.lane == lane).map(|c| c.cards.clone()).unwrap_or_default()))
    };
    let count = move || cards.with(Vec::len);
    let open = move || !is_done || done_open.get();
    let heading = {
        let title = lane.title();
        move || {
            if is_done {
                view! {
                    <button class="lane-h done" aria-expanded=move || done_open.get().to_string() on:click=move |_| done_open.update(|o| *o = !*o)>
                        {format!("{title} ({})", count())}
                        <span class="arrow" aria-hidden="true">{move || if done_open.get() { " ▾" } else { " ▸" }}</span>
                    </button>
                }
                .into_any()
            } else {
                view! { <h3 class="lane-h">{title.clone()}<span class="n">{count}</span></h3> }.into_any()
            }
        }
    };
    view! {
        <section class="lane" class:collapsed=move || !open() data-lane=lane.slug()>
            {heading}
            {move || {
                open().then(|| view! {
                    <ul class="cards">
                        <For
                            each=move || cards.get()
                            // What a card shows beyond its row's signal —
                            // its badge and its off-board chips — is in the
                            // key, so a card that keeps its column but gains a
                            // chip is drawn again; one that changes nothing
                            // keeps its DOM.
                            key=move |p: &Placed| {
                                let off = on_board.with_untracked(|b| off_board(&p.card, b));
                                format!("{}|{:?}|{}", p.card.summary.id, p.badge, off.join(","))
                            }
                            children=move |p: Placed| card_view(p, on_board)
                        />
                    </ul>
                })
            }}
        </section>
    }
}

fn severity_mark(s: Severity) -> &'static str {
    match s {
        Severity::Critical => "!!!",
        Severity::High => "!!",
        Severity::Medium => "!",
        Severity::Low => "·",
    }
}

/// One card: its row from the row cache (held by the board while it is
/// shown), so a status or a title moves in place.
fn card_view(p: Placed, on_board: Memo<HashSet<String>>) -> impl IntoView {
    let Placed { card, badge } = p;
    let id = card.summary.id.clone();
    let row: Signal<TicketSummary> = match cache::get(&id) {
        Some(sig) => Signal::from(sig),
        None => {
            let s = card.summary.clone();
            Signal::derive(move || s.clone())
        }
    };
    let off = on_board.with_untracked(|b| off_board(&card, b));
    let href = format!("#/ticket/{id}");
    let kind = card.summary.kind.as_str();
    let placement = match card.placement {
        Placement::Wave { n } => format!("wave-{n}"),
        Placement::MissingBlocker => "missing".into(),
        Placement::Done => "done".into(),
    };
    // The card links to its ticket: a click anywhere on it but a link (its
    // id, its title, a chip) opens the ticket's page, as a board row does.
    let open = {
        let id = id.clone();
        move |ev: web_sys::MouseEvent| {
            let on_link = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .is_some_and(|el| el.closest("a").ok().flatten().is_some());
            if !on_link {
                crate::navigate_to_ticket(&id);
            }
        }
    };
    view! {
        <li class="card" data-ticket-id=id.clone() data-placement=placement data-kind=kind on:click=open>
            // FILTER-22: the id, the kind, the severity, the status and each
            // off-board blocker are tags; the rest of the card keeps the
            // ticket menu.
            <div class="top">
                <a class="code" href=href.clone() data-tag=Tag::ticket(&id).attr()>{id.clone()}</a>
                <span class="kind" title=kind data-tag=Tag::kind(card.summary.kind).attr()>{kind_glyph(kind)}" "{kind}</span>
                {move || row.with(|t| t.severity).map(|s| view! {
                    <span class="sev" data-sev=s.as_str() title=format!("severity: {}", s.as_str()) data-tag=Tag::severity(s).attr()>{severity_mark(s)}</span>
                })}
                {move || (row.with(|t| t.status) == Status::Blocked).then(|| view! {
                    <span class="blocked-mark" title="status: Blocked">"⊘"</span>
                })}
                {badge.map(|b| view! { <span class="badge">{b}</span> })}
            </div>
            <a class="title" href=href>{move || row.with(|t| t.title.clone())}</a>
            <div class="foot">
                {move || {
                    let st = row.with(|t| t.status);
                    view! { <span class=format!("tag {}", st_class(st.as_str())) data-tag=Tag::status(st).attr()>{st.as_str()}</span> }
                }}
                {off.into_iter().map(|b| {
                    let href = format!("#/ticket/{b}");
                    view! { <a class="off" href=href title=format!("waits on {b}, not on this board") data-tag=Tag::ticket(&b).attr()>"⧖ "{b.clone()}" ↗"</a> }
                }).collect_view()}
            </div>
        </li>
    }
}

/// `#/waves`: every top-level shortcode, a link to its board.
#[component]
fn WaveIndex(tree: nav::Tree) -> impl IntoView {
    let top = tree.top();
    view! {
        <section id="waves-index">
            <h2>"Wave boards"</h2>
            <p class="none">"A board opens on a shortcode and every shortcode hosted under it; its filter builds any other."</p>
            <ul>
                <For
                    each=move || top.get()
                    key=|sc| sc.clone()
                    children=move |sc: String| {
                        let href = format!("#/waves/{sc}");
                        view! { <li><a class="code" href=href>{sc}</a></li> }
                    }
                />
            </ul>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::{Kind, Ticket};

    /// Owner, 2026-10-06: every column is the page's height and scrolls
    /// inside — the row takes what the viewport leaves below its top.
    #[test]
    fn the_lanes_take_what_the_viewport_leaves_below_them() {
        assert_eq!(lane_height(1000.0, 200.0), 1000.0 - 200.0 - LANE_AIR);
        assert_eq!(lane_height(1100.0, 96.5), 1100.0 - 96.5 - LANE_AIR);
        // A viewport the bar nearly fills still leaves a usable column.
        assert_eq!(lane_height(300.0, 200.0), LANE_MIN);
    }

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn summary(id: &str, status: Status) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').unwrap();
        TicketSummary {
            id: id.into(),
            epic: epic.into(),
            seq: seq.parse().unwrap(),
            title: id.into(),
            status,
            kind: Kind::Build,
            severity: None,
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

    /// A card as `waves` answers it — what it waits on among what it names.
    fn card(id: &str, status: Status, placement: Placement, waiting_on: &[&str]) -> Card {
        let waiting_on: Vec<String> = waiting_on.iter().map(|s| s.to_string()).collect();
        let summary = TicketSummary { blocked_by: waiting_on.clone(), placement: Some(placement), ..summary(id, status) };
        Card { summary, placement, waiting_on }
    }

    /// As `waves` yields them: wave 1 (Coding, Ready, Pending, Blocked,
    /// Idea), wave 3 — wave 2 holds nothing of this board's — wave 4, the
    /// missing lane, then the done.
    fn placed() -> Vec<Card> {
        use Placement::{Done, MissingBlocker, Wave};
        vec![
            card("A-2", Status::Coding, Wave { n: 1 }, &[]),
            card("A-3", Status::Ready, Wave { n: 1 }, &[]),
            card("A-4", Status::Pending, Wave { n: 1 }, &[]),
            card("A-5", Status::Blocked, Wave { n: 1 }, &[]),
            card("A-6", Status::Idea, Wave { n: 1 }, &[]),
            card("A-7", Status::InReview, Wave { n: 3 }, &["X-9"]),
            card("A-8", Status::Pending, Wave { n: 3 }, &["X-9", "A-2"]),
            card("A-9", Status::Pending, Wave { n: 4 }, &["A-8"]),
            card("A-10", Status::Pending, MissingBlocker, &["A-99"]),
            card("A-11", Status::Complete, Done, &[]),
            card("A-12", Status::Superseded, Done, &[]),
        ]
    }

    fn shape(cols: &[Column]) -> Vec<(Lane, Vec<(&str, Option<&str>)>)> {
        cols.iter()
            .map(|c| (c.lane.clone(), c.cards.iter().map(|p| (p.card.summary.id.as_str(), p.badge.as_deref())).collect()))
            .collect()
    }

    /// Acceptance 1, waves: a column per non-empty wave, ascending (wave 2,
    /// empty here, is skipped), the missing lane, Done with its cards (the
    /// view collapses it to the count); each column in yield order.
    #[test]
    fn the_waves_layout_skips_empty_waves_then_missing_then_done() {
        let cards = placed();
        let cols = columns(&cards, Layout::Waves);
        assert_eq!(
            shape(&cols),
            vec![
                (Lane::Wave(1), vec![("A-2", None), ("A-3", None), ("A-4", None), ("A-5", None), ("A-6", None)]),
                (Lane::Wave(3), vec![("A-7", None), ("A-8", None)]),
                (Lane::Wave(4), vec![("A-9", None)]),
                (Lane::Missing, vec![("A-10", None)]),
                (Lane::Done, vec![("A-11", None), ("A-12", None)]),
            ]
        );
        assert_eq!(Lane::Done.title(), "Done");
        assert_eq!(Lane::Wave(3).title(), "wave 3");

        // Nothing missing: no missing lane; nothing at all: Done alone, empty.
        let none_missing: Vec<Card> = cards.iter().filter(|c| c.placement != Placement::MissingBlocker).cloned().collect();
        assert!(!columns(&none_missing, Layout::Waves).iter().any(|c| c.lane == Lane::Missing));
        assert_eq!(shape(&columns(&[], Layout::Waves)), vec![(Lane::Done, vec![])]);
    }

    /// Acceptance 1, stages: Blocked holds every card in wave 2 or later and
    /// every missing one, badged with its wave, after the wave-1 card whose
    /// status is Blocked; the wave-1 cards by status (an Idea with Pending);
    /// Done last; every column present, empty or not.
    #[test]
    fn the_stages_layout_puts_waves_two_and_up_in_blocked_with_badges() {
        let cols = columns(&placed(), Layout::Stages);
        assert_eq!(
            shape(&cols),
            vec![
                (
                    Lane::Blocked,
                    vec![("A-5", Some("w1")), ("A-7", Some("w3")), ("A-8", Some("w3")), ("A-9", Some("w4")), ("A-10", Some("missing"))]
                ),
                (Lane::Stage(Status::Pending), vec![("A-4", None), ("A-6", None)]),
                (Lane::Stage(Status::Ready), vec![("A-3", None)]),
                (Lane::Stage(Status::Coding), vec![("A-2", None)]),
                (Lane::Stage(Status::InReview), vec![]),
                (Lane::Done, vec![("A-11", None), ("A-12", None)]),
            ]
        );
        assert_eq!(Lane::Stage(Status::InReview).slug(), "in-review");
        let total: usize = cols.iter().map(|c| c.cards.len()).sum();
        assert_eq!(total, placed().len(), "no card is left off");
    }

    /// Acceptance 1: the off-board test — a waiting-on id that is no card of
    /// this board is a chip; one that is a card is not.
    #[test]
    fn a_blocker_not_on_the_board_is_off_board() {
        let cards = placed();
        let on: HashSet<String> = cards.iter().map(|c| c.summary.id.clone()).collect();
        let of = |id: &str| off_board(cards.iter().find(|c| c.summary.id == id).unwrap(), &on);
        assert_eq!(of("A-7"), ["X-9"]);
        assert_eq!(of("A-8"), ["X-9"], "A-2 is on the board");
        assert_eq!(of("A-9"), Vec::<String>::new());
        assert_eq!(of("A-10"), ["A-99"], "an id that is no ticket is on no board");
        assert!(of("A-2").is_empty());
    }

    fn ticket(id: &str) -> Ticket {
        let s = summary(id, Status::Pending);
        Ticket {
            id: s.id,
            epic: s.epic,
            seq: s.seq,
            title: s.title,
            status: s.status,
            kind: s.kind,
            severity: None,
            blocked_by: vec![],
            unlocks: vec![],
            parent: None,
            child_count: 0,
            commits: vec![],
            path: None,
            placement: None,
            body: String::new(),
            created_unix: 0,
            updated_unix: 0,
        }
    }

    /// WAVE-6: what an event does to a board — a birth in its shortcodes or
    /// a lag asks `waves` again; a `placements_changed` or a
    /// `status_changed`, anywhere, is applied to the cards; a birth
    /// elsewhere (even of an id a card waits on — its moves follow it) and
    /// everything else, nothing.
    #[test]
    fn the_board_asks_only_for_a_card_it_has_not_got_and_applies_the_rest() {
        let epics = set(&["PLEXUS", "MACRO"]);
        let created = |id: &str| TicketsEvent::Created { ticket: ticket(id) };
        let moved = |id: &str| TicketsEvent::StatusChanged { ticket: ticket(id), previous: Status::Ready };
        assert_eq!(heard(&created("MACRO-12"), &epics), Heard::Ask, "born on the board");
        assert_eq!(heard(&TicketsEvent::Lagged { missed: 3 }, &epics), Heard::Ask);
        assert_eq!(heard(&created("GONE-1"), &epics), Heard::Nothing, "a missing blocker is born: its moves follow");
        assert_eq!(heard(&created("WAVE-9"), &epics), Heard::Nothing);
        assert_eq!(heard(&moved("PLEXUS-3"), &epics), Heard::Apply, "moved on the board");
        assert_eq!(heard(&moved("MNG-4"), &epics), Heard::Apply, "moved elsewhere: a card may wait on it");
        let placed = TicketsEvent::PlacementsChanged { changes: vec![] };
        assert_eq!(heard(&placed, &epics), Heard::Apply);
        let amended = TicketsEvent::Amended { ticket: ticket("PLEXUS-3"), changed: vec!["title".into()] };
        assert_eq!(heard(&amended, &epics), Heard::Nothing, "a new title moves no card (its row repaints in place)");
        let unlocks = TicketsEvent::UnlocksChanged { ticket: ticket("PLEXUS-3"), added: "PLEXUS-9".into() };
        assert_eq!(heard(&unlocks, &epics), Heard::Nothing);
        // SDLC-13: a kind changed on the board may add or take a card; one
        // elsewhere is a status change to the cards that wait on it.
        let rekinded = |id: &str| TicketsEvent::KindChanged { ticket: ticket(id), previous: Kind::Doc };
        assert_eq!(heard(&rekinded("PLEXUS-3"), &epics), Heard::Ask, "a card it may not have, or must not show");
        assert_eq!(heard(&rekinded("MNG-4"), &epics), Heard::Apply, "a card may wait on it");
    }

    fn change(id: &str, placement: Placement) -> menger_tickets_api::PlacementChange {
        menger_tickets_api::PlacementChange { id: id.into(), placement }
    }

    fn answer(cards: Vec<Card>) -> Answer {
        let mut a = Answer { cards, ..Answer::default() };
        retally(&mut a);
        a
    }

    fn lanes(a: &Answer) -> Vec<(Lane, Vec<String>)> {
        columns(&a.cards, Layout::Waves)
            .into_iter()
            .map(|c| (c.lane, c.cards.into_iter().map(|p| p.card.summary.id).collect()))
            .collect()
    }

    fn lane(l: Lane, ids: &[&str]) -> (Lane, Vec<String>) {
        (l, ids.iter().map(|s| s.to_string()).collect())
    }

    /// WAVE-6 acceptance 2, on the board: C waits on X, X — off the board —
    /// waits on Y. Y completes; the one `placements_changed` moves C a
    /// column left (and X and Y, which the board does not show, change
    /// nothing); the columns keep the server's order and the tally follows.
    #[test]
    fn a_placements_changed_moves_a_card_two_blockers_back() {
        use Placement::{Done, Wave};
        let mut a = answer(vec![
            card("A-2", Status::Ready, Wave { n: 1 }, &[]),
            card("A-4", Status::Pending, Wave { n: 2 }, &["A-2"]),
            card("A-3", Status::Pending, Wave { n: 3 }, &["X-9"]),
            card("A-5", Status::Complete, Done, &[]),
        ]);
        assert_eq!(tally(&a), "3 in 3 waves · 1 done");
        let ev = TicketsEvent::PlacementsChanged {
            changes: vec![change("Y-1", Done), change("X-9", Wave { n: 1 }), change("A-3", Wave { n: 2 })],
        };
        assert!(apply(&mut a, &ev));
        assert_eq!(
            lanes(&a),
            vec![lane(Lane::Wave(1), &["A-2"]), lane(Lane::Wave(2), &["A-3", "A-4"]), lane(Lane::Done, &["A-5"])],
            "A-3 joins wave 2 in the store's order"
        );
        assert_eq!(tally(&a), "3 in 2 waves · 1 done");
        assert!(!apply(&mut a, &ev), "applied twice, nothing moves");

        // A-2 completes: its own card goes to Done, A-4 loses it from what
        // it waits on, and the move behind it takes A-4 to wave 1.
        let mut done = ticket("A-2");
        done.status = Status::Complete;
        assert!(apply(&mut a, &TicketsEvent::StatusChanged { ticket: done, previous: Status::Ready }));
        assert!(apply(&mut a, &TicketsEvent::PlacementsChanged { changes: vec![change("A-2", Done), change("A-4", Wave { n: 1 })] }));
        assert_eq!(
            lanes(&a),
            vec![lane(Lane::Wave(1), &["A-4"]), lane(Lane::Wave(2), &["A-3"]), lane(Lane::Done, &["A-2", "A-5"])]
        );
        let a4 = a.cards.iter().find(|c| c.summary.id == "A-4").unwrap();
        assert!(a4.waiting_on.is_empty(), "A-2 is done with");
        assert_eq!(tally(&a), "2 in 2 waves · 2 done");
    }

    /// WAVE-6: a `status_changed` keeps the stages layout and the chips
    /// right without asking — the card's stage moves, a wave keeps the
    /// server's rank order, and a reopened blocker is waited on again in
    /// `blocked_by` order.
    #[test]
    fn a_status_change_moves_the_stage_and_what_a_card_waits_on() {
        use Placement::Wave;
        let mut c = card("B-3", Status::Pending, Wave { n: 2 }, &["B-9"]);
        c.summary.blocked_by = vec!["B-1".into(), "B-9".into()];
        let mut a = answer(vec![
            card("B-2", Status::Pending, Wave { n: 1 }, &[]),
            card("B-4", Status::Ready, Wave { n: 1 }, &[]),
            c,
        ]);
        assert_eq!(a.cards.iter().map(|c| c.summary.id.as_str()).collect::<Vec<_>>(), ["B-2", "B-4", "B-3"]);
        let mut coding = ticket("B-2");
        coding.status = Status::Coding;
        assert!(apply(&mut a, &TicketsEvent::StatusChanged { ticket: coding, previous: Status::Pending }));
        let stages = columns(&a.cards, Layout::Stages);
        let coding_col = stages.iter().find(|c| c.lane == Lane::Stage(Status::Coding)).unwrap();
        assert_eq!(coding_col.cards[0].card.summary.id, "B-2", "its stage moved");
        assert_eq!(a.cards[0].summary.id, "B-2", "Coding first in its wave");

        // B-1, which B-3 names first, is reopened: waited on again, first.
        let mut reopened = ticket("B-1");
        reopened.status = Status::Pending;
        assert!(apply(&mut a, &TicketsEvent::StatusChanged { ticket: reopened, previous: Status::Complete }));
        let b3 = a.cards.iter().find(|c| c.summary.id == "B-3").unwrap();
        assert_eq!(b3.waiting_on, ["B-1", "B-9"]);
        // A ticket nothing names, off the board: nothing moves.
        assert!(!apply(&mut a, &TicketsEvent::StatusChanged { ticket: ticket("Q-1"), previous: Status::Ready }));
    }

    /// The asks coalesce: one in flight, one queued however many ask, and
    /// the queued one goes when the first is answered.
    #[test]
    fn asks_coalesce_to_one_in_flight_and_one_queued() {
        let mut c = Coalesce::default();
        assert!(c.ask(), "idle: go");
        assert!(!c.ask(), "in flight: queued");
        assert!(!c.ask(), "still one queued");
        assert!(!c.ask());
        assert!(c.answered(), "answered with one queued: it goes");
        assert!(!c.answered(), "answered with none queued: idle");
        assert_eq!(c, Coalesce::default());
        assert!(c.ask(), "idle again: go");
    }

    /// The layout is remembered per shortcode, and anything unreadable is
    /// the default, waves.
    #[test]
    fn the_layout_is_kept_per_shortcode_and_defaults_to_waves() {
        assert_eq!(layout_key("plexus"), "menger.waves.PLEXUS");
        assert_ne!(layout_key("PLEXUS"), layout_key("MNG"));
        for l in [Layout::Waves, Layout::Stages] {
            assert_eq!(Layout::parse(Some(l.as_str())), l);
        }
        assert_eq!(Layout::parse(None), Layout::Waves);
        assert_eq!(Layout::parse(Some("table")), Layout::Waves);
    }

    /// `waves`' events into an answer; an in-band error is the answer's
    /// failure (the board keeps its last good one).
    #[test]
    fn an_answer_is_its_cards_and_tally_or_its_error() {
        let ev = |id: &str, p: Placement| TicketsEvent::Placed {
            summary: summary(id, Status::Pending),
            placement: p,
            waiting_on: vec![],
        };
        let a = answer_of(vec![
            ev("A-2", Placement::Wave { n: 1 }),
            ev("A-3", Placement::Wave { n: 3 }),
            ev("A-4", Placement::Done),
            TicketsEvent::WaveCounts { waves: vec![1, 0, 1], missing: 0, done: 1 },
        ])
        .unwrap();
        assert_eq!(a.cards.len(), 3);
        assert_eq!(tally(&a), "2 in 2 waves · 1 done");
        let err = answer_of(vec![TicketsEvent::Error { message: "waves: epics is empty".into() }]);
        assert_eq!(err, Err("waves: epics is empty".into()));
        let one = Answer { waves: vec![2], missing: 1, done: 0, cards: vec![] };
        assert_eq!(tally(&one), "2 in 1 wave · 1 missing · 0 done");
    }

    // -----------------------------------------------------------------
    // The board's filter (WAVE-12's tests, on the lens since FILTER-11)

    use crate::filter::tests::{platform, strs, PLEXUS_TREE};
    use crate::filter::Clausal;
    use crate::lens::{waves_default, Kept};

    fn rich(id: &str, status: Status, kind: Kind, sev: Option<Severity>, placement: Placement, waiting_on: &[&str], title: &str) -> Card {
        let mut c = card(id, status, placement, waiting_on);
        c.summary.kind = kind;
        c.summary.severity = sev;
        c.summary.title = title.into();
        c
    }

    /// A board's answer: RETURNS and CONTEXT, in four waves, a missing
    /// blocker, two done; one card waits on a ticket off the board (MNG-4);
    /// RETURNS-4 has a commit, and CONTEXT-2 is RETURNS-4's child.
    fn fixture() -> Answer {
        use Placement::{Done, MissingBlocker, Wave};
        let mut cards = vec![
            rich("RETURNS-4", Status::Ready, Kind::Build, Some(Severity::High), Wave { n: 1 }, &[], "envelope 0.7"),
            rich("RETURNS-5", Status::Coding, Kind::Spike, None, Wave { n: 1 }, &[], "hub-codegen streams"),
            rich("CONTEXT-2", Status::Pending, Kind::Gap, None, Wave { n: 1 }, &[], "context loss"),
            rich("RETURNS-6", Status::Pending, Kind::Build, Some(Severity::Low), Wave { n: 2 }, &["RETURNS-4"], "Envelope conformance"),
            rich("CONTEXT-4", Status::Blocked, Kind::Build, None, Wave { n: 3 }, &["RETURNS-6", "MNG-4"], "carry the context"),
            rich("RETURNS-7", Status::Pending, Kind::Build, None, Wave { n: 4 }, &["CONTEXT-4"], "release"),
            rich("RETURNS-9", Status::Pending, Kind::Build, None, MissingBlocker, &["RETURNS-99"], "orphan"),
            rich("RETURNS-2", Status::Complete, Kind::Build, None, Done, &[], "old envelope"),
            rich("CONTEXT-1", Status::Superseded, Kind::Spike, None, Done, &[], "first try"),
        ];
        cards[0].summary.commits = vec!["abc1234".into()];
        cards[2].summary.parent = Some("RETURNS-4".into());
        let mut a = Answer { cards, ..Answer::default() };
        retally(&mut a);
        a
    }

    fn shown_by(k: &Kept) -> Vec<String> {
        narrow(&fixture(), &(k.filter.clone(), k.local.clone()), Some(&platform())).cards.into_iter().map(|c| c.summary.id).collect()
    }

    fn one(c: Cat, op: Op, v: &[&str]) -> Vec<String> {
        let mut k = Kept::default();
        k.set(c, op, strs(v));
        shown_by(&k)
    }

    /// Acceptance 1 (WAVE-12): each category applied to the answer's cards
    /// — the server-backed ones through the language's `matches`, the local
    /// ones as WAVE-12 evaluates them — both operators; and the tally
    /// follows what is shown.
    #[test]
    fn each_category_narrows_the_cards() {
        assert_eq!(shown_by(&Kept::default()).len(), 9, "no clause: every card");
        assert_eq!(one(Cat::Shortcode, Op::Is, &["CONTEXT"]), strs(&["CONTEXT-2", "CONTEXT-4", "CONTEXT-1"]));
        assert_eq!(one(Cat::Shortcode, Op::Not, &["RETURNS"]), strs(&["CONTEXT-2", "CONTEXT-4", "CONTEXT-1"]));
        assert_eq!(one(Cat::Status, Op::Not, &["Pending"]), strs(&["RETURNS-4", "RETURNS-5", "CONTEXT-4", "RETURNS-2", "CONTEXT-1"]));
        assert_eq!(one(Cat::Status, Op::Is, &["Coding", "In Review"]), strs(&["RETURNS-5"]));
        assert_eq!(one(Cat::Wave, Op::Is, &["2", "missing"]), strs(&["RETURNS-6", "RETURNS-9"]));
        assert_eq!(one(Cat::Wave, Op::Not, &["1", "done"]), strs(&["RETURNS-6", "CONTEXT-4", "RETURNS-7", "RETURNS-9"]));
        assert_eq!(one(Cat::Kind, Op::Is, &["spike"]), strs(&["RETURNS-5", "CONTEXT-1"]));
        assert_eq!(one(Cat::Kind, Op::Not, &["build"]), strs(&["RETURNS-5", "CONTEXT-2", "CONTEXT-1"]));
        assert_eq!(one(Cat::Severity, Op::Is, &["high", "low"]), strs(&["RETURNS-4", "RETURNS-6"]));
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
        // FILTER-11's flags: ready is Ready and waiting on nothing; blocked
        // waits on something; has commits.
        assert_eq!(one(Cat::Ready, Op::Is, &["yes"]), strs(&["RETURNS-4"]));
        assert_eq!(one(Cat::Blocked, Op::Is, &["yes"]), strs(&["RETURNS-6", "CONTEXT-4", "RETURNS-7", "RETURNS-9"]));
        assert_eq!(one(Cat::Blocked, Op::Is, &["no"]).len(), 5);
        assert_eq!(one(Cat::Commits, Op::Is, &["yes"]), strs(&["RETURNS-4"]));

        // Clauses combine; the tally is the shown cards'.
        let mut k = Kept::default();
        k.set(Cat::Shortcode, Op::Is, strs(&["RETURNS"]));
        k.set(Cat::Status, Op::Not, strs(&["Pending"]));
        let a = narrow(&fixture(), &(k.filter.clone(), k.local.clone()), None);
        assert_eq!(a.cards.iter().map(|c| c.summary.id.as_str()).collect::<Vec<_>>(), ["RETURNS-4", "RETURNS-5", "RETURNS-2"]);
        assert_eq!(tally(&a), "2 in 1 wave · 1 done");
        assert_eq!(tally(&fixture()), "6 in 4 waves · 1 missing · 2 done");
    }

    fn eff(text: &str) -> Filter {
        Filter::parse(text).unwrap()
    }

    /// FILTER-11: every group of the effective filter is evaluated on the
    /// held cards with `Filter::matches` — a custom group too (`under`
    /// through the card's lineage: its parent, its shortcode, what hosts
    /// it); global ∧ waves narrows to what both admit, and a clash admits
    /// nothing.
    #[test]
    fn the_effective_filter_is_evaluated_on_the_cards() {
        let show = |f: &str| -> Vec<String> {
            narrow(&fixture(), &(eff(f), Clauses::default()), Some(&platform())).cards.into_iter().map(|c| c.summary.id).collect()
        };
        assert_eq!(show("status Ready"), strs(&["RETURNS-4"]));
        assert_eq!(show("(blocked or status Ready)"), strs(&["RETURNS-4", "RETURNS-6", "CONTEXT-4", "RETURNS-7", "RETURNS-9"]));
        assert_eq!(show("under RETURNS-4"), strs(&["RETURNS-4", "CONTEXT-2"]), "the ticket and its child");
        assert_eq!(show("under PLEXUS-4").len(), 6, "RETURNS is hosted under PLEXUS-4");
        assert_eq!(show("under PLEXUS"), show(""), "both are hosted under PLEXUS");
        assert_eq!(show("category done"), strs(&["RETURNS-2"]));
        assert_eq!(show("text \"envelope\""), strs(&["RETURNS-4", "RETURNS-6", "RETURNS-2"]));
        // Global ∧ waves: the global Ready over the board's default.
        let t = platform();
        let mut d = waves_default("PLEXUS", &t);
        d.filter.groups.retain(|g| crate::lens::clause_of(g).is_some_and(|(c, _)| c == Cat::Shortcode));
        let both = eff("status Ready").and(&d.filter);
        assert_eq!(narrow(&fixture(), &(both, Clauses::default()), Some(&t)).cards.len(), 1);
        let clash = eff("status Ready").and(&eff("status Coding"));
        assert!(clash.admits_nothing().is_some());
        assert!(narrow(&fixture(), &(clash, Clauses::default()), Some(&t)).cards.is_empty(), "nothing can match: the empty board");
    }

    /// A card's facts: blocked is waiting on something; its lineage its id,
    /// its ancestors as far as known, its shortcode, what hosts it.
    #[test]
    fn a_cards_facts_are_its_summary_and_lineage() {
        let a = fixture();
        let t = platform();
        let c2 = a.cards.iter().find(|c| c.summary.id == "CONTEXT-2").unwrap();
        let parent_of = |id: &str| (id == "RETURNS-4").then(|| "RETURNS-1".to_string());
        let f = facts_of(c2, Some(&t), &parent_of);
        assert_eq!(f.lineage, strs(&["CONTEXT-2", "RETURNS-4", "RETURNS-1", "CONTEXT", "PLEXUS-1", "PLEXUS"]));
        assert!(!f.blocked);
        let c4 = a.cards.iter().find(|c| c.summary.id == "CONTEXT-4").unwrap();
        assert!(facts_of(c4, None, &|_| None).blocked);
        assert!(facts_of(&a.cards[0], None, &|_| None).has_commits);
        // FILTER-21: a card is never one of menger's own documents (docs are
        // never cards); a summary with a `_MENGER/` path would be.
        assert!(a.cards.iter().all(|c| !facts_of(c, None, &|_| None).menger_doc));
        let mut note = a.cards[0].clone();
        note.summary.path = Some("_MENGER/board".into());
        assert!(facts_of(&note, None, &|_| None).menger_doc);
        assert_eq!(value_of(Cat::MengerDocs, &note, false).as_deref(), Some("yes"));
    }

    /// Acceptance 1 (WAVE-12): the default for a route — what it asks
    /// `waves` for: Shortcode's set the `epics` (`is not`: the route's tree
    /// without them; none: every shortcode), Kind's the `kinds`; every group
    /// of Shortcode or Kind in the effective filter narrows them.
    #[test]
    fn a_board_opens_on_its_shortcode_tree() {
        let t = platform();
        let d = waves_default("plexus", &t);
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<BTreeSet<String>>();
        assert_eq!(epics(&d.filter, "PLEXUS", &t), set(&PLEXUS_TREE));
        assert_eq!(kinds(&d.filter), strs(&["build", "spike", "gap"]));
        assert_eq!(epics(&waves_default("WAVE", &t).filter, "WAVE", &t), set(&["WAVE"]));
        assert_eq!(epics(&waves_default("NOPE", &t).filter, "NOPE", &t), set(&["NOPE"]));

        assert_eq!(epics(&eff("not in (MACRO, CODEGEN, GEN, MNG)"), "PLEXUS", &t), set(&["CONTEXT", "HYPERFORGE", "PLEXUS", "RETURNS", "STARTER", "SYNAPSE", "TRANSPORT"]));
        assert_eq!(epics(&Filter::default(), "PLEXUS", &t).len(), 13);
        assert_eq!(kinds(&Filter::default()), strs(&WORK_KINDS));
        assert_eq!(kinds(&eff("not kind gap")), strs(&["build", "spike", "scope", "execution"]));
        // The global filter's groups narrow what is asked too.
        assert_eq!(epics(&eff("in (MNG, RETURNS)").and(&d.filter), "PLEXUS", &t), set(&["RETURNS"]));
        assert_eq!(kinds(&eff("kind in (build, doc)").and(&d.filter)), strs(&["build"]), "docs are never cards");
        assert_eq!(epics(&eff("(in MNG or ready)"), "PLEXUS", &t).len(), 13, "a custom group asks nothing narrower");
    }

    /// Acceptance 1 (WAVE-12): the count per value — how many of the
    /// answer's cards have it, whatever else is applied; a value no card
    /// has is 0, a shortcode not asked for shows none.
    #[test]
    fn counts_are_per_value_over_the_answer() {
        let asked: BTreeSet<String> = ["RETURNS", "CONTEXT", "MNG"].into_iter().map(String::from).collect();
        let k = counts(&fixture(), &asked);
        let n = |c: Cat, v: &str| k.get(&c).and_then(|m| m.get(v)).copied();
        assert_eq!((n(Cat::Shortcode, "RETURNS"), n(Cat::Shortcode, "CONTEXT"), n(Cat::Shortcode, "MNG")), (Some(6), Some(3), Some(0)));
        assert_eq!(n(Cat::Shortcode, "WAVE"), None, "not asked for: no count");
        assert_eq!((n(Cat::Status, "Pending"), n(Cat::Status, "Coding"), n(Cat::Status, "In Review")), (Some(4), Some(1), Some(0)));
        assert_eq!((n(Cat::Wave, "1"), n(Cat::Wave, "2"), n(Cat::Wave, "missing"), n(Cat::Wave, "done")), (Some(3), Some(1), Some(1), Some(2)));
        assert_eq!((n(Cat::Kind, "build"), n(Cat::Kind, "spike"), n(Cat::Kind, "gap"), n(Cat::Kind, "scope")), (Some(6), Some(2), Some(1), Some(0)));
        assert_eq!((n(Cat::Severity, "high"), n(Cat::Severity, "low"), n(Cat::Severity, "none")), (Some(1), Some(1), Some(7)));
        assert_eq!((n(Cat::Readiness, "workable"), n(Cat::Readiness, "waiting"), n(Cat::Readiness, "missing")), (Some(3), Some(3), Some(1)));
        assert_eq!((n(Cat::OffBoard, "some"), n(Cat::OffBoard, "none")), (Some(2), Some(7)));
        assert_eq!((n(Cat::Ready, "yes"), n(Cat::Blocked, "yes"), n(Cat::Commits, "yes")), (Some(1), Some(4), Some(1)));
        assert_eq!((n(Cat::MengerDocs, "yes"), n(Cat::MengerDocs, "no")), (Some(0), Some(9)), "no card is a menger doc");
        assert!(!k.contains_key(&Cat::Title));
    }
}
