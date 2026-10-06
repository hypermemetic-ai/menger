//! WAVE-5: the wave board — `#/waves/<SHORTCODE>`, the order of work as a
//! kanban derived from `blocked_by` (WAVE-1).
//!
//! The board is a shortcode and every shortcode hosted under it, at any
//! depth (WAVE-1 decision 11: until views exist, the nav's `host` chain from
//! `groups` is what a board is). It asks the server where each of their
//! tickets sits — one `tickets.waves { epics, kinds }` per load (WAVE-4),
//! computed over the whole graph, never in the browser — and groups the
//! answer into columns here, in one of two layouts a toggle apart:
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
//! ticket's page. The layout is remembered per shortcode in localStorage —
//! every read and write allowed to fail, the board working without it.
//!
//! Live: the board lends the session an ear ([`session::listen`]). A
//! `created` or `status_changed` for a ticket of one of its shortcodes, or
//! for an id one of its cards waits on, asks `waves` again ([`reasks`]); so
//! does a `lagged`, and a new connection. The asks are coalesced
//! ([`Coalesce`]): one in flight, at most one queued behind it. An answer
//! that is an in-band error, or a call that fails, goes to the app's error
//! line and the last good board stays drawn. The cards' rows are held in the
//! row cache (src/cache.rs) while they are shown, so a status or a title
//! moves on the card the moment `watch` carries it, before the answer that
//! may move the card between columns.
//!
//! Everything that decides — the shortcode set, the columns, the off-board
//! test, the re-ask predicate, the coalescing, the stored layout — is a pure
//! function here, pinned by native tests.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use leptos::prelude::*;
use menger_tickets_api::{Group, Placement, Severity, Status, TicketSummary, TicketsEvent};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::nav::kind_glyph;
use crate::{cache, dom, nav, session, st_class};

// ---------------------------------------------------------------------
// The shortcode set

/// The shortcode a ticket id is in: what comes before its last `-`.
pub(crate) fn epic_of(id: &str) -> Option<&str> {
    id.rsplit_once('-').map(|(epic, _)| epic)
}

/// The board for `root` (case-insensitive): `root` and every shortcode whose
/// `host` ticket's shortcode is already in the set, transitively — a
/// sub-shortcode hosted under a sub-shortcode joins as surely as one hosted
/// under the root. In shortcode order.
pub(crate) fn shortcode_set(root: &str, groups: &BTreeMap<String, Group>) -> BTreeSet<String> {
    let mut set = BTreeSet::from([root.to_ascii_uppercase()]);
    loop {
        let joining: Vec<String> = groups
            .values()
            .filter(|g| !set.contains(&g.epic))
            .filter(|g| g.host.as_deref().and_then(epic_of).is_some_and(|h| set.contains(h)))
            .map(|g| g.epic.clone())
            .collect();
        if joining.is_empty() {
            return set;
        }
        set.extend(joining);
    }
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

/// Whether `ev` may move what the board shows, so `waves` is asked again: a
/// ticket born into, or changing status in, one of the board's shortcodes
/// (`epics`); one any card waits on (`waiting` — a blocker on another board,
/// or an id that is no ticket yet being born); or a `lagged`, after which
/// anything may have.
pub(crate) fn reasks(ev: &TicketsEvent, epics: &BTreeSet<String>, waiting: &HashSet<String>) -> bool {
    match ev {
        TicketsEvent::Created { ticket } | TicketsEvent::StatusChanged { ticket, .. } => {
            epics.contains(&ticket.epic) || waiting.contains(&ticket.id)
        }
        TicketsEvent::Lagged { .. } => true,
        _ => false,
    }
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

/// The kinds asked for with the checkbox ticked: the work, and the scopes
/// and executions beside it (WAVE-1 decision 9: docs never).
const WIDE_KINDS: [&str; 5] = ["build", "spike", "gap", "scope", "execution"];

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
// The view

/// What the board's asks share — every handle `Copy`, so the ear, the
/// effects and the request each take their own.
#[derive(Clone, Copy)]
struct Ctx {
    set: Memo<Option<BTreeSet<String>>>,
    wide: RwSignal<bool>,
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
        let (Some(Some(epics)), Some(wide)) = (ctx.set.try_get_untracked(), ctx.wide.try_get_untracked()) else {
            return;
        };
        let Some(client) = session::client() else {
            // Not connected: the next connection asks (the socket ticks).
            ctx.coalesce.try_update_value(|c| *c = Coalesce::default());
            return;
        };
        let kinds = wide.then(|| WIDE_KINDS.iter().map(|k| k.to_string()).collect());
        match client.waves_all(epics.into_iter().collect(), kinds).await {
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
/// shortcodes, each a link to its board.
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
    let set = {
        let sc = sc.clone();
        Memo::new(move |_| tree.with_groups(|g| g.map(|g| shortcode_set(&sc, g))))
    };
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
    let wide = RwSignal::new(false);
    let done_open = RwSignal::new(false);
    let ctx = Ctx {
        set,
        wide,
        answer: RwSignal::new(None),
        coalesce: StoredValue::new(Coalesce::default()),
        holder: StoredValue::new(cache::Holder::new()),
        last_error,
    };

    // A load: on entry once the groups say what the set is, on a new
    // connection, and when the set or the kinds change.
    Effect::new(move |_| {
        if socket.get() == 0 || set.with(Option::is_none) {
            return; // no connection, or no groups yet: this re-runs when there are
        }
        wide.track();
        ask(ctx);
    });

    // Live: ask again when an event may move what is shown.
    let waiting = Memo::new(move |_| {
        ctx.answer.with(|a| {
            a.iter().flat_map(|a| a.cards.iter()).flat_map(|c| c.waiting_on.iter().cloned()).collect::<HashSet<String>>()
        })
    });
    session::listen(move |ev| {
        let (Some(Some(epics)), Some(waiting)) = (set.try_get_untracked(), waiting.try_get_untracked()) else {
            return;
        };
        if reasks(ev, &epics, &waiting) {
            ask(ctx);
        }
    });

    let on_board = Memo::new(move |_| {
        ctx.answer.with(|a| a.iter().flat_map(|a| a.cards.iter()).map(|c| c.summary.id.clone()).collect::<HashSet<String>>())
    });
    let cols = Memo::new(move |_| {
        let l = layout.get();
        ctx.answer.with(|a| a.as_ref().map(|a| columns(&a.cards, l)).unwrap_or_default())
    });
    let lanes = Memo::new(move |_| cols.with(|cs| cs.iter().map(|c| c.lane.clone()).collect::<Vec<_>>()));
    let hosted = {
        let sc = sc.clone();
        move || set.with(|s| s.iter().flatten().filter(|e| **e != sc).cloned().collect::<Vec<_>>().join(" · "))
    };
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
        <section id="waves" data-shortcode=sc.clone() data-layout=move || layout.get().as_str()>
            <div class="wbar">
                <span class="code">{sc.clone()}</span>
                <span class="hosted" title="the shortcodes hosted under it">{hosted}</span>
                <span class="layout" role="group" aria-label="layout">"[ "{seg(Layout::Waves)}" | "{seg(Layout::Stages)}" ]"</span>
                <label class="wide">
                    <input
                        type="checkbox"
                        prop:checked=move || wide.get()
                        on:change=move |ev| wide.set(event_target_checked(&ev))
                    />
                    " scopes & executions"
                </label>
                <span class="tally">{move || ctx.answer.with(|a| a.as_ref().map(tally))}</span>
            </div>
            {move || {
                ctx.answer.with(Option::is_none).then(|| view! { <p class="loading">"placing the work…"</p> })
            }}
            <div class="lanes">
                <For
                    each=move || lanes.get()
                    key=Lane::slug
                    children=move |lane: Lane| lane_view(lane, cols, on_board, done_open)
                />
            </div>
        </section>
    }
    .into_any()
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
            <div class="top">
                <a class="code" href=href.clone()>{id.clone()}</a>
                <span class="kind" title=kind>{kind_glyph(kind)}" "{kind}</span>
                {move || row.with(|t| t.severity).map(|s| view! {
                    <span class="sev" data-sev=s.as_str() title=format!("severity: {}", s.as_str())>{severity_mark(s)}</span>
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
                    view! { <span class=format!("tag {}", st_class(st.as_str()))>{st.as_str()}</span> }
                }}
                {off.into_iter().map(|b| {
                    let href = format!("#/ticket/{b}");
                    view! { <a class="off" href=href title=format!("waits on {b}, not on this board")>"⧖ "{b.clone()}" ↗"</a> }
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
            <p class="none">"A board is a shortcode and every shortcode hosted under it."</p>
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
    fn platform() -> BTreeMap<String, Group> {
        [
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
        .collect()
    }

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Acceptance 1: the shortcode set for PLEXUS — every shortcode hosted
    /// under one of its tickets, and those hosted under them, transitively;
    /// nothing hosted elsewhere. Case-insensitive.
    #[test]
    fn the_shortcode_set_follows_the_host_chain() {
        let g = platform();
        let want = set(&[
            "CODEGEN", "CONTEXT", "GEN", "HYPERFORGE", "MACRO", "PLEXUS", "RETURNS", "STARTER", "SYNAPSE", "TRANSPORT",
        ]);
        assert_eq!(shortcode_set("PLEXUS", &g), want);
        assert_eq!(shortcode_set("plexus", &g), want, "case-insensitive");
        assert_eq!(shortcode_set("MACRO", &g), set(&["CODEGEN", "GEN", "MACRO"]), "a sub-shortcode's board is its subtree");
        assert_eq!(shortcode_set("MNG", &g), set(&["LOOSE", "MNG"]));
        assert_eq!(shortcode_set("WAVE", &g), set(&["WAVE"]));
        assert_eq!(shortcode_set("NOPE", &g), set(&["NOPE"]), "an unknown shortcode is a board of one");
        assert_eq!(epic_of("PLEXUS-12"), Some("PLEXUS"));
        assert_eq!(epic_of("PLEXUS"), None);
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
            created_unix: 0,
            updated_unix: 0,
        }
    }

    fn card(id: &str, status: Status, placement: Placement, waiting_on: &[&str]) -> Card {
        Card { summary: summary(id, status), placement, waiting_on: waiting_on.iter().map(|s| s.to_string()).collect() }
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
            body: String::new(),
            created_unix: 0,
            updated_unix: 0,
        }
    }

    /// Acceptance 1: the re-ask predicate — a birth or a status change in
    /// the board's shortcodes, or of an id a card waits on; a `lagged`;
    /// nothing else.
    #[test]
    fn the_board_asks_again_for_its_shortcodes_its_waiting_on_and_a_lag() {
        let epics = set(&["PLEXUS", "MACRO"]);
        let waiting: HashSet<String> = ["MNG-4".to_string(), "GONE-1".to_string()].into();
        let created = |id: &str| TicketsEvent::Created { ticket: ticket(id) };
        let moved = |id: &str| TicketsEvent::StatusChanged { ticket: ticket(id), previous: Status::Ready };
        assert!(reasks(&created("MACRO-12"), &epics, &waiting), "born on the board");
        assert!(reasks(&moved("PLEXUS-3"), &epics, &waiting), "moved on the board");
        assert!(reasks(&moved("MNG-4"), &epics, &waiting), "a card waits on it, off the board");
        assert!(reasks(&created("GONE-1"), &epics, &waiting), "a missing blocker is born");
        assert!(!reasks(&moved("MNG-5"), &epics, &waiting), "elsewhere, and nothing waits on it");
        assert!(!reasks(&created("WAVE-9"), &epics, &waiting));
        assert!(reasks(&TicketsEvent::Lagged { missed: 3 }, &epics, &waiting));
        let amended = TicketsEvent::Amended { ticket: ticket("PLEXUS-3"), changed: vec!["title".into()] };
        assert!(!reasks(&amended, &epics, &waiting), "a new title moves no card (its row repaints in place)");
        let unlocks = TicketsEvent::UnlocksChanged { ticket: ticket("PLEXUS-3"), added: "PLEXUS-9".into() };
        assert!(!reasks(&unlocks, &epics, &waiting), "the birth that caused it already asked");
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
}
