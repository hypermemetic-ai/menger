//! FILTER-20: the nav's flat view — every ticket the nav's filter admits, as
//! one list, in id order (owner, FILTER-1 › flat-view: "The nav offers a flat
//! view beside the tree: every ticket the nav's filter admits, as one list in
//! id order; the choice persists").
//!
//! - **The choice** ([`View`]) is the nav header's `Tree | Flat`, kept in this
//!   browser beside the nav's other state — `menger.nav.view` ([`VIEW_KEY`]),
//!   as `menger.nav.expanded` and `.hidden` are: no value is the tree, `flat`
//!   the flat list. Choosing Tree draws the tree exactly as it was;
//!   the flat list is unmounted then, and asks nothing.
//! - **The list** is the nav's effective filter's matches (`global ∧ nav`,
//!   FILTER-14's) — with no group in it, every ticket (the empty filter, `""`)
//!   — in the store's order, which is id order: shortcode, then number
//!   ([`id_key`]). Where a ticket sits is the board's arithmetic under a filter
//!   (FILTER-12), held here for the nav: `counts { filter }` gives each
//!   shortcode's matches ([`Matches`]), position p is the rank-th match of a
//!   shortcode by prefix sums, and the rows held are kept by rank ([`Ranked`]).
//! - **Pages as it scrolls** (SCALE-7: the app loads what is shown). The window
//!   is the board's (`board_layout`, MFE-12: the viewport and a margin either
//!   side, laid out scaled past a browser's height limit) at the nav's row
//!   pitch, and its first stretch not held is one page: `list { filter, after:
//!   <the row before it>, limit, summary }` when that row is held — scrolling
//!   on — and `list { filter, at, limit, summary }` when it is not (a jump);
//!   from the top for position 0 ([`cursor`], [`next_ask`]). Every page says
//!   where it starts (`first`) and how many there are, and is placed there; one
//!   that does not fit the counts asks them again. Rows more than [`KEEP`]
//!   positions from the window are given back. Nothing walks the store.
//! - **Live** by the filter's membership watch (FILTER-12's `watch { filter,
//!   membership: true }`; the empty filter's for every ticket): `entered` — a
//!   ticket born, or moved into the filter — raises its shortcode's count and
//!   lands at its id-order place, held when it lands beside a held row,
//!   otherwise fetched when the window reaches it; `left` drops its row;
//!   `recount`, a `lagged` (on either stream) or a new connection asks again,
//!   rows keeping their DOM. A status change, a title, a commit: the row's
//!   signal in the row cache, in place. A re-parent moves no row — the order is
//!   the id's.
//! - **A row is the tree's leaf**: kind glyph, id, title, status tag; a real
//!   link to `#/ticket/{id}`, selected when it is the open ticket; the ticket
//!   menu's `data-ticket-id`. A menger document (WAVE-10) is a row like any
//!   other, listed when the nav's filter admits it — its default does not
//!   (FILTER-21). Rows are treeitems of the nav's tree, so AGS-18's keys
//!   walk them, and the stretches not yet in hand are gaps the keys scroll
//!   across (SCALE-24).
//! - **A deep link** brings the open ticket's place into view when the list
//!   holds it — where the rows held say, or `list { filter, after: id, limit: 0
//!   }`'s `first` — once per navigation, and again under a new filter.
//!
//! What decides — the stored choice, the order, the cursors, the note, where
//! to scroll — is pure, pinned by native tests.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use menger_tickets_api::{TicketSummary, TicketsEvent};

use crate::board::{ranked_slots, Landed, Matches, Ranked, Slot, Stale};
use crate::session::{self, Client, Member};
use crate::{board_layout, cache, dom, nav, st_class, Geom, Layout, Route};

// ---------------------------------------------------------------------
// The choice

/// The nav's view: the tree (AGS-20, as it was) or the flat list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum View {
    #[default]
    Tree,
    Flat,
}

/// Where the choice is kept — beside the nav's other state.
pub(crate) const VIEW_KEY: &str = "menger.nav.view";

impl View {
    /// The header's two buttons, in order.
    pub(crate) const ALL: [View; 2] = [View::Tree, View::Flat];

    /// The choice as kept: `flat` is the flat list; nothing — or anything
    /// else — the tree.
    pub(crate) fn from_stored(raw: Option<&str>) -> View {
        match raw.map(str::trim) {
            Some("flat") => View::Flat,
            _ => View::Tree,
        }
    }

    /// What is kept for it: the tree, the default, is no value.
    pub(crate) fn stored(self) -> Option<&'static str> {
        match self {
            View::Tree => None,
            View::Flat => Some("flat"),
        }
    }

    /// Its name, as the DOM's `data-view` says it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            View::Tree => "tree",
            View::Flat => "flat",
        }
    }

    /// Its button's word.
    pub(crate) fn label(self) -> &'static str {
        match self {
            View::Tree => "Tree",
            View::Flat => "Flat",
        }
    }
}

/// The choice this browser keeps — the tree where none is kept, or where
/// local storage is unavailable.
pub(crate) fn load_view() -> View {
    View::from_stored(nav::storage().and_then(|s| s.get_item(VIEW_KEY).ok().flatten()).as_deref())
}

pub(crate) fn save_view(v: View) {
    if let Some(s) = nav::storage() {
        let _ = match v.stored() {
            Some(raw) => s.set_item(VIEW_KEY, raw),
            None => s.remove_item(VIEW_KEY),
        };
    }
}

// ---------------------------------------------------------------------
// The order

/// A row's place: (shortcode, number).
pub(crate) type Key = (String, u32);

/// An id's place in the flat list's order — its shortcode, then its number
/// (MNG-9's `(epic, seq)`, the store's own order): `AGS-2` before `AGS-10`,
/// every `MNG` ticket before `MNG2-1`. `None` for what is not an id.
pub(crate) fn id_key(id: &str) -> Option<Key> {
    crate::kids::key(id).map(|(e, s)| (e.to_string(), s))
}

/// Whether `ids` are ids in strictly ascending id order — what every page of
/// the list is; one that is not is not placed.
pub(crate) fn in_id_order<'a>(ids: impl IntoIterator<Item = &'a str>) -> bool {
    let mut last: Option<Key> = None;
    for id in ids {
        let Some(k) = id_key(id) else { return false };
        if last.as_ref().is_some_and(|l| *l >= k) {
            return false;
        }
        last = Some(k);
    }
    true
}

/// What the list is asked under: the nav's effective filter's groups as the
/// wire's text (`text`, FILTER-14's) while it has one (`on`) — none at all
/// when they admit nothing — and the empty filter, every ticket, while it has
/// none.
pub(crate) fn list_filter(on: bool, text: Option<String>) -> Option<String> {
    if on {
        text
    } else {
        Some(String::new())
    }
}

// ---------------------------------------------------------------------
// Pages

/// Rows held beyond each edge of the window once it is in hand: a scroll back
/// over them is a re-render, not a fetch. With the window (the viewport and
/// MFE-12's 100-row margins, ~250 nav rows) the list holds about 1,050 rows
/// at most, wherever it has been.
pub(crate) const KEEP: usize = 400;

/// The most one page asks for.
pub(crate) const MAX_PAGE: usize = 500;

/// The nav's row pitch (px) until a row has been measured: 12.5 px type at a
/// 1.55 line height (index.html), as the tree's.
const PITCH_GUESS: f64 = 19.375;

/// Where a page starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Cursor {
    /// Position 0: `list { filter, limit }`.
    Top,
    /// Right after a held row: `list { filter, after: id, limit }`.
    After(String),
    /// A position no held row is beside: `list { filter, at, limit }`.
    At(usize),
}

impl Cursor {
    /// As `list`'s `after` and `at`.
    fn params(&self) -> (Option<String>, Option<u64>) {
        match self {
            Cursor::Top => (None, None),
            Cursor::After(id) => (Some(id.clone()), None),
            Cursor::At(p) => (None, Some(*p as u64)),
        }
    }
}

/// The cursor for the page that starts at position `from`: the top for 0;
/// after the row held at `from - 1` — `after` is a position (MNG-9), so the
/// page starts right behind it; otherwise at `from` itself.
pub(crate) fn cursor<T>(m: &Matches, r: &Ranked<T>, from: usize) -> Cursor {
    if from == 0 {
        return Cursor::Top;
    }
    match r.at(m, from - 1) {
        Some((k, _)) => Cursor::After(format!("{}-{}", k.0, k.1)),
        None => Cursor::At(from),
    }
}

/// One page to ask for: where it should start, from which cursor, how many.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ask {
    pub(crate) from: usize,
    pub(crate) cursor: Cursor,
    pub(crate) limit: usize,
}

/// The page the window `lo..hi` needs next — its stretch not held, from the
/// first position missing to the last (one page, at most [`MAX_PAGE`]) — or
/// `None` when the window is in hand.
pub(crate) fn next_ask<T>(m: &Matches, r: &Ranked<T>, lo: usize, hi: usize) -> Option<Ask> {
    let (from, last) = r.missing(m, lo, hi)?;
    Some(Ask { from, cursor: cursor(m, r, from), limit: (last + 1 - from).min(MAX_PAGE) })
}

/// Where to scroll so row `i` shows — given the row at the viewport's top
/// (`top`, fractional) and how many rows fit (`fits`): `None` when it shows
/// already; else the row to put at the top so it sits mid-viewport (never
/// above the first).
pub(crate) fn scroll_row(top: f64, fits: f64, i: usize) -> Option<f64> {
    let i = i as f64;
    if i >= top && i + 1.0 <= top + fits {
        return None;
    }
    Some((i - (fits - 1.0).max(0.0) / 2.0).max(0.0))
}

/// The line under the list, when it has one to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    /// The counts are not in yet.
    Counting,
    /// Nothing to list.
    Empty,
    /// The server did not take the filter: its words.
    Refused(String),
}

/// The note for a list asked under `filter` (`None`: it admits nothing),
/// counted `n` (`None`: not yet), and refused or not.
pub(crate) fn note_of(filter: Option<&str>, n: Option<usize>, refused: Option<&str>) -> Option<Note> {
    if let Some(why) = refused {
        return Some(Note::Refused(why.to_string()));
    }
    match (filter, n) {
        (None, _) | (_, Some(0)) => Some(Note::Empty),
        (Some(_), None) => Some(Note::Counting),
        (Some(_), Some(_)) => None,
    }
}

/// A note, drawn under the list as the tree's lens notes are.
pub(crate) fn note_view(note: Note, filtered: bool) -> AnyView {
    match note {
        Note::Counting => view! { <div class="nlens-note" data-nflat="counting">"counting…"</div> }.into_any(),
        Note::Empty => {
            let words = if filtered { "no ticket matches the nav's filter" } else { "no tickets" };
            view! { <div class="nlens-note" data-nflat="empty">{words}</div> }.into_any()
        }
        Note::Refused(why) => view! {
            <div class="nlens-note" data-nlens="refused" data-nflat="refused">"The server did not take the filter: "{why}</div>
        }
        .into_any(),
    }
}

// ---------------------------------------------------------------------
// Asking

/// What `counts { filter }` answered.
enum Counted {
    Counts(Vec<(String, u64)>),
    Refused(String),
    Offline,
}

async fn count(client: &Client, filter: &str) -> Counted {
    match client.counts_all(Some(filter.to_string())).await {
        Ok(events) => {
            let mut runs = Vec::new();
            for ev in events {
                match ev {
                    TicketsEvent::Count { epic, held } => runs.push((epic, held)),
                    TicketsEvent::Error { message } => return Counted::Refused(message),
                    _ => {}
                }
            }
            Counted::Counts(runs)
        }
        Err(_) => Counted::Offline,
    }
}

/// One page of the list.
enum Window {
    /// Its rows; `first`, where the first sits; `total`, every match
    /// (`first + rows + remaining`).
    Page { rows: Vec<TicketSummary>, first: usize, total: usize },
    Refused(String),
    Offline,
}

/// `list { filter, after | at, limit, summary }` — `from`, where the page
/// was meant to start, only if the server does not say (it always does when
/// a filter is given).
async fn ask_page(client: &Client, filter: &str, cursor: &Cursor, from: usize, limit: usize) -> Window {
    let (after, at) = cursor.params();
    match client
        .list_all(None, None, None, after, Some(limit as u32), Some(true), Some(filter.to_string()), at)
        .await
    {
        Ok(events) => {
            let mut rows = Vec::new();
            for ev in events {
                match ev {
                    TicketsEvent::Summary { summary } => rows.push(summary),
                    TicketsEvent::Page { remaining, first, .. } => {
                        let first = first.map_or(from, |f| f as usize);
                        let total = first + rows.len() + remaining as usize;
                        return Window::Page { rows, first, total };
                    }
                    TicketsEvent::Error { message } => return Window::Refused(message),
                    _ => {}
                }
            }
            Window::Refused("list: no page".into())
        }
        Err(_) => Window::Offline,
    }
}

/// Where `id` sits in the list by the server's word — `None` when the filter
/// does not admit it (`member`), or no answer came. `list { filter, after: id,
/// limit: 0 }`'s `first` is how many matches there are up to it, so a match
/// sits at `first - 1`.
async fn locate(client: &Client, filter: &str, id: &str) -> Option<usize> {
    let matched = client.member_all(vec![id.to_string()], filter.to_string()).await.ok()?.into_iter().any(|ev| {
        matches!(ev, TicketsEvent::Member { matched: true, .. })
    });
    if !matched {
        return None;
    }
    let events = client
        .list_all(None, None, None, Some(id.to_string()), Some(0), Some(true), Some(filter.to_string()), None)
        .await
        .ok()?;
    let first = events.into_iter().find_map(|ev| match ev {
        TicketsEvent::Page { first, .. } => first,
        _ => None,
    })?;
    (first as usize).checked_sub(1)
}

// ---------------------------------------------------------------------
// The list, live

/// A row the list holds: its id and its signal.
type Row = (String, cache::RowSignal);

/// The flat list's model: the filter it shows, its counts, the rows it holds
/// by rank, and the geometry its view last measured — FILTER-12's board under
/// a filter, in the nav. Every handle `Copy`; made by the view, for its
/// lifetime.
#[derive(Clone, Copy)]
struct Flat {
    /// The filter shown — `""` every ticket; `None` a filter that admits
    /// nothing (nothing is asked).
    text: StoredValue<Option<String>>,
    /// `counts { filter }`, once answered.
    matches: RwSignal<Option<Matches>>,
    ranked: RwSignal<Ranked<Row>>,
    holder: StoredValue<cache::Holder>,
    geom: RwSignal<Geom>,
    /// Moves with every new filter and every asking again: an answer to an
    /// older question is not applied.
    gen: StoredValue<u32>,
    /// The generation of the request in flight — one at a time.
    flight: StoredValue<Option<u32>>,
    /// Moves with every membership event — a page asked before one is not
    /// placed (its positions may have moved).
    moved: StoredValue<u64>,
    /// An answer that changed nothing re-runs nothing — except when it came
    /// back to a newer generation: then the list asks again.
    retry: Trigger,
    /// Asks in a row that told the list nothing, for one window.
    strikes: StoredValue<((usize, usize), u8)>,
    /// Counts asked again because a page did not fit them, since the last
    /// page that did.
    resyncs: StoredValue<u8>,
    /// What the server said of a filter it did not take.
    refused: RwSignal<Option<String>>,
}

impl Flat {
    fn new() -> Self {
        Flat {
            text: StoredValue::new(None),
            matches: RwSignal::new(None),
            ranked: RwSignal::new(Ranked::default()),
            holder: StoredValue::new(cache::Holder::new()),
            geom: RwSignal::new(Geom { above: 0.0, viewport: 900.0, pitch: PITCH_GUESS }),
            gen: StoredValue::new(0),
            flight: StoredValue::new(None),
            moved: StoredValue::new(0),
            retry: Trigger::new(),
            strikes: StoredValue::new(((0, 0), 0)),
            resyncs: StoredValue::new(0),
            refused: RwSignal::new(None),
        }
    }

    /// Every row of the list (tracked) — 0 until counted.
    fn n(self) -> usize {
        self.matches.with(|m| m.as_ref().map_or(0, Matches::n))
    }

    /// The window at the geometry last measured.
    fn window(self) -> (usize, usize) {
        let n = self.matches.with_untracked(|m| m.as_ref().map_or(0, Matches::n));
        let l = board_layout(n, self.geom.get_untracked());
        (l.start.min(n), l.end.min(n))
    }

    fn release(self, ids: impl IntoIterator<Item = String>) {
        self.holder.with_value(|h| ids.into_iter().for_each(|id| h.release(&id)));
    }

    /// A new filter (or the first): what was held for the old one given back;
    /// nothing counted until its membership watch says it is open.
    fn reset(self, text: Option<String>) {
        self.gen.update_value(|g| *g = g.wrapping_add(1));
        self.flight.set_value(None);
        self.strikes.set_value(((0, 0), 0));
        self.resyncs.set_value(0);
        let gone = self.ranked.try_update(Ranked::clear).unwrap_or_default();
        self.release(gone.into_iter().map(|(id, _)| id));
        self.refused.set(None);
        self.matches.set(text.is_none().then(Matches::default));
        self.text.set_value(text);
    }

    /// A membership event: `entered` and `left` move the counts and the rows
    /// held (a row entering beside a held one is held — from the row cache, or
    /// the row `watch` just carried; elsewhere nothing is fetched until the
    /// window reaches it); `recount` sets the counts and asks the window again;
    /// `lagged`, and the watch opened on a connection, ask the counts, then the
    /// window.
    fn heard(self, ev: &Member) {
        if self.text.try_with_value(Option::is_none).unwrap_or(true) {
            return;
        }
        match ev {
            Member::Entered { id, epic, seq } => {
                self.moved.update_value(|n| *n = n.wrapping_add(1));
                let Some(mut m) = self.matches.get_untracked() else { return };
                let landed = self.ranked.try_update(|r| r.entered(&mut m, epic, *seq));
                self.matches.set(Some(m));
                if let Some(Landed::At { rank: Some(rank), beside: true, .. }) = landed {
                    let sig = if self.holder.with_value(|h| h.hold_id(id)) {
                        cache::get(id)
                    } else {
                        session::recent(id).and_then(|t| self.holder.with_value(|h| h.hold(t)))
                    };
                    if let Some(sig) = sig {
                        self.ranked.update(|r| r.insert((epic.clone(), *seq), (id.clone(), sig), rank));
                    }
                }
            }
            Member::Left { epic, seq, .. } => {
                self.moved.update_value(|n| *n = n.wrapping_add(1));
                let Some(mut m) = self.matches.get_untracked() else { return };
                let gone = self.ranked.try_update(|r| r.left(&mut m, epic, *seq)).flatten();
                self.matches.set(Some(m));
                self.release(gone.map(|(id, _)| id));
            }
            Member::Recount { epic, held } => {
                self.moved.update_value(|n| *n = n.wrapping_add(1));
                let Some(mut m) = self.matches.get_untracked() else { return };
                self.ranked.update(|r| r.recount(&mut m, epic, *held));
                self.matches.set(Some(m));
                self.again(false);
            }
            Member::Lagged | Member::Opened => self.again(true),
            Member::Refused(message) => {
                self.matches.set(Some(Matches::default()));
                self.refused.set(Some(message.clone()));
            }
        }
    }

    /// Ask again, on the connection there is — the counts first when
    /// `counts`.
    fn again(self, counts: bool) {
        spawn_local(async move { self.refresh(counts).await });
    }

    /// The list asked again: its counts (when `counts`, or when it has none),
    /// then the window's rows in one page, placed over the rows held (they
    /// keep their DOM); every row outside the window given back, since it may
    /// be stale. An answer to an older question is not applied.
    async fn refresh(self, counts: bool) {
        let Some(Some(text)) = self.text.try_get_value() else { return };
        let Some(client) = session::client() else { return };
        let Some(gen) = self.gen.try_get_value().map(|g| g.wrapping_add(1)) else { return };
        self.gen.set_value(gen);
        self.flight.set_value(Some(gen));
        self.strikes.set_value(((0, 0), 0));
        let mut fit = self.ask(&client, &text, gen, counts).await;
        // A page the counts did not place: they are stale — once more, the
        // counts first.
        if fit == Some(false) && !counts && self.gen.try_get_value() == Some(gen) {
            fit = self.ask(&client, &text, gen, true).await;
        }
        let _ = fit;
        if self.gen.try_get_value() != Some(gen) {
            return; // asked again meanwhile, or gone
        }
        let (lo, hi) = self.window();
        self.keep(lo, hi);
        if self.flight.get_value() == Some(gen) {
            self.flight.set_value(None);
        }
        self.retry.notify();
    }

    /// The asking behind [`Flat::refresh`]: `Some(false)` when the window's
    /// page did not fit the counts, `None` when no answer came.
    async fn ask(self, client: &Client, text: &str, gen: u32, counts: bool) -> Option<bool> {
        if counts || self.matches.try_with_untracked(Option::is_none)? {
            match count(client, text).await {
                Counted::Counts(runs) => {
                    if self.gen.try_get_value()? != gen {
                        return Some(true);
                    }
                    self.matches.set(Some(Matches::new(runs)));
                    self.refused.set(None);
                }
                Counted::Refused(message) => {
                    if self.gen.try_get_value()? == gen {
                        self.matches.set(Some(Matches::default()));
                        self.refused.set(Some(message));
                    }
                    return Some(true);
                }
                Counted::Offline => return None,
            }
        }
        let (lo, hi) = self.window();
        if hi <= lo {
            return Some(true);
        }
        let moved = self.moved.get_value();
        let from = if lo == 0 { Cursor::Top } else { Cursor::At(lo) };
        match ask_page(client, text, &from, lo, (hi - lo).min(MAX_PAGE)).await {
            Window::Page { rows, first, total } => {
                if self.gen.try_get_value()? != gen || self.moved.try_get_value()? != moved {
                    return Some(true); // the list's own asking fills what moved
                }
                Some(self.place(rows, first, total).is_ok())
            }
            Window::Refused(message) => {
                self.refused.try_set(Some(message));
                Some(true)
            }
            Window::Offline => None,
        }
    }

    /// A page came back: held, ranked where the counts put it, and the rows it
    /// displaced given back — `Ok(true)` when it told the list anything;
    /// [`Stale`] (nothing held) when it does not fit.
    fn place(self, rows: Vec<TicketSummary>, first: usize, total: usize) -> Result<bool, Stale> {
        if !in_id_order(rows.iter().map(|t| t.id.as_str())) {
            return Err(Stale);
        }
        let keys: Vec<Key> = rows.iter().map(|t| (t.epic.clone(), t.seq)).collect();
        self.matches.with_untracked(|m| Ranked::<Row>::fits(m.as_ref().ok_or(Stale)?, first, Some(total), &keys))?;
        self.resyncs.set_value(0);
        if !self.holder.with_value(|h| h.hold_page(rows.clone())) {
            return Ok(false);
        }
        let page: Vec<(Key, Row)> = rows.into_iter().filter_map(|t| cache::get(&t.id).map(|sig| ((t.epic, t.seq), (t.id, sig)))).collect();
        let placed = self.matches.with_untracked(|m| {
            let m = m.as_ref()?;
            self.ranked.try_update(|r| r.place(m, first, Some(total), page))
        });
        match placed {
            Some(Ok(p)) => {
                let told = p.fresh || !p.gone.is_empty();
                self.release(p.gone.into_iter().map(|(id, _)| id));
                Ok(told)
            }
            Some(Err(s)) => Err(s),
            None => Ok(false),
        }
    }

    /// Give back every row held outside `lo..hi`. Touches the rows' signal
    /// only when something goes.
    fn keep(self, lo: usize, hi: usize) {
        let any = self.matches.with_untracked(|m| m.as_ref().is_some_and(|m| self.ranked.with_untracked(|r| r.outside(m, lo, hi))));
        if !any {
            return;
        }
        let gone = self.matches.with_untracked(|m| {
            let m = m.as_ref()?;
            self.ranked.try_update(|r| r.keep(m, lo, hi))
        });
        self.release(gone.into_iter().flatten().map(|(id, _)| id));
    }

    /// The list's asking, for its lifetime: whenever the window, the counts
    /// or what is held move, what is far from the window is given back and
    /// the window's next missing stretch is asked for — one page, one request
    /// in flight ([`next_ask`]). A page asked before a membership event moved
    /// the positions is asked again; one the counts do not place asks the
    /// counts again (twice at most, until a page fits).
    fn drive(self) {
        Effect::new(move |_| {
            self.retry.track();
            let Some(n) = self.matches.with(|m| m.as_ref().map(Matches::n)) else { return };
            let l = board_layout(n, self.geom.get());
            let (lo, hi) = (l.start.min(n), l.end.min(n));
            self.ranked.track();
            self.keep(lo.saturating_sub(KEEP), hi + KEEP);
            if self.flight.get_value().is_some() {
                return;
            }
            if self.strikes.with_value(|(w, k)| *w == (lo, hi) && *k >= 3) {
                return; // until the window, or the connection, moves
            }
            let Some(Some(text)) = self.text.try_get_value() else { return };
            let ask = self.matches.with_untracked(|m| self.ranked.with_untracked(|r| next_ask(m.as_ref()?, r, lo, hi)));
            let Some(ask) = ask else { return };
            let gen = self.gen.get_value();
            let moved = self.moved.get_value();
            self.flight.set_value(Some(gen));
            spawn_local(async move {
                let answer = match session::client() {
                    Some(client) => ask_page(&client, &text, &ask.cursor, ask.from, ask.limit).await,
                    None => Window::Offline,
                };
                if self.flight.try_get_value() == Some(Some(gen)) {
                    self.flight.set_value(None);
                }
                if self.gen.try_get_value() != Some(gen) || self.moved.try_get_value() != Some(moved) {
                    self.retry.notify(); // asked again; a no-op once the list is gone
                    return;
                }
                let told = match answer {
                    Window::Page { rows, first, total } => match self.place(rows, first, total) {
                        Ok(told) => told,
                        Err(Stale) => {
                            if self.resyncs.get_value() < 2 {
                                self.resyncs.update_value(|n| *n += 1);
                                self.again(true);
                                return;
                            }
                            false
                        }
                    },
                    Window::Refused(message) => {
                        self.refused.set(Some(message));
                        false
                    }
                    // The membership watch sees the socket go, and asks again
                    // when it is back.
                    Window::Offline => return,
                };
                self.strikes.update_value(|(w, k)| {
                    if *w != (lo, hi) || told {
                        *w = (lo, hi);
                        *k = 0;
                    }
                    if !told {
                        *k += 1;
                    }
                });
                if !told {
                    self.retry.notify();
                }
            });
        });
    }

    /// Where `id` sits — where the list holds it, else the server's word.
    async fn locate(self, id: &str) -> Option<usize> {
        let k = id_key(id)?;
        let held = self.matches.try_with_untracked(|m| self.ranked.with_untracked(|r| r.pos_of(m.as_ref()?, &k)))?;
        if held.is_some() {
            return held;
        }
        let text = self.text.try_get_value()??;
        let client = session::client()?;
        locate(&client, &text, id).await
    }
}

/// The list's geometry off the DOM: how far its top has scrolled above the
/// nav's viewport, the viewport's height, and the row pitch (a mounted row's
/// computed height; `pitch` until one is mounted).
fn measure(list: &web_sys::Element, pitch: f64) -> Option<Geom> {
    let sc = nav::scroller()?;
    let (s, k) = (sc.get_bounding_client_rect(), list.get_bounding_client_rect());
    let measured = list
        .query_selector(".row")
        .ok()
        .flatten()
        .and_then(|row| dom::window().get_computed_style(&row).ok().flatten())
        .and_then(|st| st.get_property_value("height").ok())
        .map(|h| js_sys::parse_float(&h))
        .filter(|p| *p > 1.0);
    Some(Geom { above: s.top() - k.top(), viewport: f64::from(sc.client_height()), pitch: measured.unwrap_or(pitch) })
}

/// The nav's flat list — the rows its window shows, by page, between two
/// spacers that stand for the rest (MFE-12's board, in the nav). `filter` is
/// what it lists ([`list_filter`]); `viewport` moves when the nav scrolls or
/// resizes; `note` is the line the nav draws under it.
pub(crate) fn flat_list(
    filter: Memo<Option<String>>,
    route: RwSignal<Route>,
    viewport: Trigger,
    note: RwSignal<Option<Note>>,
) -> AnyView {
    let f = Flat::new();
    let list_ref = NodeRef::<leptos::html::Div>::new();
    // A deep link's ticket, once per navigation (and again under a new
    // filter).
    let revealed = StoredValue::new(None::<String>);

    // The filter shown: a new one is a new list, from its top — a position
    // among one filter's matches is nothing among another's.
    Effect::new(move |prev: Option<Option<String>>| {
        let now = filter.get();
        if prev.as_ref() != Some(&now) {
            f.reset(now.clone());
            revealed.set_value(None);
            if prev.is_some() {
                if let Some(sc) = nav::scroller() {
                    sc.set_scroll_top(0);
                }
            }
        }
        now
    });
    // Its membership watch — opened, replaced and dropped with the filter;
    // what opens it asks the counts, then the window.
    session::follow_membership(move || filter.get(), move |ev| f.heard(ev));
    // A `lagged` on the session's own `watch`: what the rows say may be stale.
    let stale = session::stale();
    Effect::new(move |prev: Option<u32>| {
        let now = stale.get();
        if prev.is_some_and(|p| p != now) {
            f.again(true);
        }
        now
    });
    f.drive();

    // The geometry, off the DOM — on every scroll and resize of the nav, and
    // after every render of the window.
    let measure_now = move || {
        let Some(Some(el)) = list_ref.try_get_untracked() else { return };
        let Some(old) = f.geom.try_get_untracked() else { return };
        let Some(next) = measure(&el, old.pitch) else { return };
        if next != old {
            f.geom.try_set(next);
        }
    };
    Effect::new(move |_| {
        viewport.track();
        measure_now();
    });
    let n = Memo::new(move |_| f.n());
    let layout = Memo::new(move |_| board_layout(n.get(), f.geom.get()));
    let range = Memo::new(move |_| layout.with(|l| (l.start.min(n.get()), l.end.min(n.get()))));
    let slots = Memo::new(move |_| {
        let (lo, hi) = range.get();
        f.matches.with(|m| match m {
            Some(m) => f.ranked.with(|r| ranked_slots(m, r, lo, hi)),
            None => Vec::new(),
        })
    });
    Effect::new(move |_| {
        slots.track();
        spawn_local(async move {
            dom::sleep_ms(0).await;
            measure_now();
            nav::ensure_tree_tab_stop();
        });
    });

    // The line under the list.
    Effect::new(move |_| {
        let counted = f.matches.with(|m| m.as_ref().map(Matches::n));
        let next = filter.with(|t| f.refused.with(|r| note_of(t.as_deref(), counted, r.as_deref())));
        if note.with_untracked(|n| *n != next) {
            note.set(next);
        }
    });
    on_cleanup(move || {
        note.try_set(None);
    });

    // A deep link: the open ticket's place brought into view, once the list
    // is counted.
    Effect::new(move |_| {
        let Route::Ticket(id) = route.get() else { return };
        if f.matches.with(Option::is_none) {
            return; // not counted yet: this re-runs when it is
        }
        if revealed.get_value().as_deref() == Some(id.as_str()) {
            return;
        }
        revealed.set_value(Some(id.clone()));
        spawn_local(async move {
            let Some(p) = f.locate(&id).await else { return };
            if revealed.try_get_value() != Some(Some(id)) {
                return; // a newer navigation, or the list is gone
            }
            dom::sleep_ms(0).await;
            show(list_ref, f, p);
        });
    });

    let spacer = move |px: fn(&Layout) -> f64| {
        let present = Memo::new(move |_| layout.with(|l| px(l) > 0.0));
        move || {
            present.get().then(|| {
                view! { <div class="spacer gap" role="none" aria-hidden="true" style:height=move || format!("{}px", layout.with(px))></div> }
            })
        }
    };
    let geom = f.geom;
    view! {
        <div class="flat" role="none" node_ref=list_ref data-flat-n=move || n.get()>
            {spacer(|l| l.top_px)}
            <For
                each=move || slots.get()
                key=Slot::key
                children=move |slot: Slot| match slot {
                    Slot::Row(id, sig) => flat_row(id, sig, f, n, route),
                    Slot::Gap(at, len) => view! {
                        <div
                            class="pending gap"
                            role="none"
                            aria-hidden="true"
                            data-index=at
                            style:height=move || format!("{}px", len as f64 * geom.with(|g| g.pitch()))
                        >
                            "…"
                        </div>
                    }
                    .into_any(),
                }
            />
            {spacer(|l| l.bottom_px)}
        </div>
    }
    .into_any()
}

/// Scroll the nav so position `p` shows — mid-viewport, unless it shows
/// already. The window follows the scroll and fetches what is there.
fn show(list_ref: NodeRef<leptos::html::Div>, f: Flat, p: usize) {
    let Some(Some(el)) = list_ref.try_get_untracked() else { return };
    let Some(sc) = nav::scroller() else { return };
    let Some(old) = f.geom.try_get_untracked() else { return };
    let Some(g) = measure(&el, old.pitch) else { return };
    let Some(n) = f.matches.try_with_untracked(|m| m.as_ref().map_or(0, Matches::n)) else { return };
    let Some(want) = scroll_row(g.row_at(n, g.above), g.viewport / g.pitch(), p) else { return };
    let delta = g.offset_of(n, want) - g.above;
    sc.set_scroll_top(sc.scroll_top() + delta.round() as i32);
}

/// One row of the flat list: the tree's leaf (kind glyph, id, title, status
/// tag), a real link, selected when the route is its ticket's page, and its
/// place among every row of the list for assistive technology. Its signal
/// handle is made in its own owner and goes with it.
fn flat_row(
    id: String,
    sig: cache::RowSignal,
    f: Flat,
    n: Memo<usize>,
    route: RwSignal<Route>,
) -> AnyView {
    let ticket = RwSignal::from(sig);
    let sel = {
        let id = id.clone();
        Memo::new(move |_| route.with(|rt| matches!(rt, Route::Ticket(x) if *x == id)))
    };
    // A ticket's place in the order is fixed at birth; SDLC-13: its kind is
    // not (`set_kind`), so the row follows it.
    let kind = Memo::new(move |_| ticket.with(|t| t.kind.as_str()));
    let key: Key = ticket.with_untracked(|t| (t.epic.clone(), t.seq));
    let posinset = Memo::new(move |_| f.matches.with(|m| f.ranked.with(|r| r.pos_of(m.as_ref()?, &key))).map(|p| p + 1));
    let href = format!("#/ticket/{id}");
    view! {
        <a
            class="row leaf"
            class:sel=move || sel.get()
            role="treeitem"
            tabindex="-1"
            aria-selected=move || sel.get().to_string()
            aria-setsize=move || n.get()
            aria-posinset=move || posinset.get()
            data-node-key=id.clone()
            data-nav-id=id.clone()
            data-ticket-id=id.clone()
            data-kind=move || kind.get()
            href=href
        >
            // FILTER-22: the kind, the id and the status are tags, as in
            // the tree.
            <span class="toggle kind" title=move || kind.get() data-tag=move || crate::tags::Tag::kind(ticket.with(|t| t.kind)).attr()>
                {move || nav::kind_glyph(kind.get())}
            </span>
            <span class="code" data-tag=crate::tags::Tag::ticket(&id).attr()>{id.clone()}</span>
            <span class="label">{move || ticket.with(|t| t.title.clone())}</span>
            // The status tag, its paint and its word one effect: they move
            // together.
            {move || {
                let st = ticket.with(|t| t.status);
                view! { <span class=format!("tag {}", st_class(st.as_str())) data-tag=crate::tags::Tag::status(st).attr()>{st.as_str()}</span> }
            }}
        </a>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::{cursor, id_key, in_id_order, list_filter, next_ask, note_of, scroll_row, Ask, Cursor, Key, Note, View, MAX_PAGE, VIEW_KEY};
    use crate::board::{ranked_slots, Landed, Matches, Ranked, Slot};
    use leptos::prelude::*;
    use menger_tickets_api::{Kind, Status, TicketSummary};

    fn k(id: &str) -> Key {
        id_key(id).expect("an id")
    }

    /// A page of `ids`, placed at `first` of `total`, as the list places one.
    fn placed(m: &Matches, r: &mut Ranked<String>, first: usize, ids: &[&str]) {
        let page = ids.iter().map(|id| (k(id), id.to_string())).collect();
        r.place(m, first, Some(m.n()), page).expect("fits the counts");
    }

    fn ids_at(m: &Matches, r: &Ranked<String>, lo: usize, hi: usize) -> Vec<(usize, String)> {
        r.window(m, lo, hi).into_iter().map(|(p, _, t)| (p, t.clone())).collect()
    }

    /// FILTER-20, the stored choice: kept beside the nav's other state under
    /// `menger.nav.view`; `flat` is the flat list, and nothing — the default,
    /// what a browser that never chose has — is the tree, as is anything
    /// else; choosing the tree removes the key rather than writing it.
    #[test]
    fn the_choice_is_kept_beside_the_navs_other_state() {
        assert_eq!(VIEW_KEY, "menger.nav.view");
        assert!(VIEW_KEY.starts_with("menger.nav."), "with expanded and hidden");
        assert_eq!(View::default(), View::Tree);
        assert_eq!(View::from_stored(None), View::Tree, "never chosen: the tree");
        assert_eq!(View::from_stored(Some("flat")), View::Flat);
        assert_eq!(View::from_stored(Some(" flat\n")), View::Flat);
        assert_eq!(View::from_stored(Some("tree")), View::Tree);
        assert_eq!(View::from_stored(Some("Flat")), View::Tree, "only the word written");
        assert_eq!(View::from_stored(Some("")), View::Tree);
        assert_eq!(View::stored(View::Flat), Some("flat"));
        assert_eq!(View::stored(View::Tree), None, "the default is no value");
        for v in View::ALL {
            assert_eq!(View::from_stored(v.stored()), v, "{v:?} round-trips");
        }
        assert_eq!(View::ALL.map(View::name), ["tree", "flat"]);
        assert_eq!(View::ALL.map(View::label), ["Tree", "Flat"]);
    }

    /// FILTER-20, the order: shortcode, then number — numerically within a
    /// shortcode (`AGS-2` before `AGS-10`), by the shortcode's own order across
    /// them (every `MNG` before `MNG2`, which is before `MNGX`); a page out of
    /// that order, or holding what is no id, is not the list's.
    #[test]
    fn the_list_is_in_id_order_shortcode_then_number() {
        let mut ids = vec!["MNG2-1", "AGS-10", "MNGX-3", "MNG-12", "AGS-2", "MNG-3", "B-1", "AGS-1"];
        ids.sort_by_key(|id| id_key(id));
        assert_eq!(ids, ["AGS-1", "AGS-2", "AGS-10", "B-1", "MNG-3", "MNG-12", "MNG2-1", "MNGX-3"]);
        assert_eq!(id_key("FILTER-20"), Some(("FILTER".to_string(), 20)));
        assert_eq!(id_key("nope"), None);
        assert!(in_id_order(["AGS-2", "AGS-10", "MNG-1", "MNG2-1"]));
        assert!(in_id_order([]));
        assert!(!in_id_order(["AGS-10", "AGS-2"]), "lexical is not the order");
        assert!(!in_id_order(["AGS-2", "AGS-2"]), "strictly ascending");
        assert!(!in_id_order(["MNG2-1", "MNG-9"]));
        assert!(!in_id_order(["AGS-1", "x"]));
    }

    /// FILTER-20, the order on screen: pages placed where the counts put them
    /// — in any order, a jump before the top — draw the window in id order
    /// across shortcodes, gaps where nothing is held; a ticket that enters
    /// (born, or moved into the filter) lands at its id-order place and moves
    /// every row after it one place on; one that leaves takes its row with it.
    #[test]
    fn rows_are_drawn_in_id_order_and_an_arrival_takes_its_place() {
        // AGS: 2, 3, 10, 12 match; MNG: 1, 4; MNG2: 1 — seven matches.
        let mut m = Matches::new(vec![("MNG".into(), 2), ("AGS".into(), 4), ("MNG2".into(), 1)]);
        let mut r: Ranked<String> = Ranked::default();
        placed(&m, &mut r, 4, &["MNG-1", "MNG-4", "MNG2-1"]);
        placed(&m, &mut r, 0, &["AGS-2", "AGS-3", "AGS-10"]);
        assert_eq!(
            ids_at(&m, &r, 0, m.n()),
            [(0, "AGS-2"), (1, "AGS-3"), (2, "AGS-10"), (4, "MNG-1"), (5, "MNG-4"), (6, "MNG2-1")].map(|(p, s)| (p, s.to_string()))
        );
        // AGS-12 is not held: a gap at 3.
        let sigs = |r: &Ranked<String>| {
            let mut out: Ranked<(String, ArcRwSignal<TicketSummary>)> = Ranked::default();
            for (p, key, id) in r.window(&m, 0, m.n()) {
                let rank = m.locate(p).unwrap().1;
                out.insert(key.clone(), (id.clone(), ArcRwSignal::new(row(id))), rank);
            }
            out
        };
        let keys: Vec<String> = ranked_slots(&m, &sigs(&r), 0, m.n()).iter().map(Slot::key).collect();
        assert_eq!(keys, ["AGS-2", "AGS-3", "AGS-10", "?3+1", "MNG-1", "MNG-4", "MNG2-1"]);
        // Where AGS-11 would land is not known while AGS-12 is not held (it
        // could be either side of it): the counts move, and the window asks.
        let mut unsure = (m.clone(), r.clone());
        assert_eq!(unsure.1.entered(&mut unsure.0, "AGS", 11), Landed::At { pos: None, rank: None, beside: false });
        assert_eq!(unsure.1.pos_of(&unsure.0, &k("MNG2-1")), Some(7));
        // The window's page brings AGS-12; then AGS-11 is born between AGS-10
        // and AGS-12 — beside a held row, so it is drawn at once.
        placed(&m, &mut r, 3, &["AGS-12"]);
        assert_eq!(r.entered(&mut m, "AGS", 11), Landed::At { pos: Some(3), rank: Some(3), beside: true });
        r.insert(k("AGS-11"), "AGS-11".into(), 3);
        assert_eq!(m.n(), 8);
        assert_eq!(
            ids_at(&m, &r, 0, m.n()).into_iter().map(|(_, id)| id).collect::<Vec<_>>(),
            ["AGS-2", "AGS-3", "AGS-10", "AGS-11", "AGS-12", "MNG-1", "MNG-4", "MNG2-1"]
        );
        assert_eq!(r.pos_of(&m, &k("MNG2-1")), Some(7), "every row after it one place on");
        // A new shortcode between AGS and MNG: its first, at its place.
        assert_eq!(r.entered(&mut m, "B", 1), Landed::At { pos: Some(5), rank: Some(0), beside: true });
        assert_eq!(r.pos_of(&m, &k("MNG-1")), Some(6));
        // MNG-1 leaves: its row goes, MNG-4 moves up.
        assert_eq!(r.left(&mut m, "MNG", 1), Some("MNG-1".to_string()));
        assert_eq!(r.pos_of(&m, &k("MNG-4")), Some(6));
        assert_eq!(m.n(), 8);
    }

    fn row(id: &str) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').expect("EPIC-N");
        TicketSummary {
            id: id.to_string(),
            epic: epic.to_string(),
            seq: seq.parse().expect("seq"),
            title: String::new(),
            status: Status::Pending,
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

    /// FILTER-20, the paging cursors: position 0 is the top; a page right
    /// behind a held row is `after` that row (scrolling on: `list { filter,
    /// after, limit }`); one no held row is beside is `at` its position (a
    /// jump). A window's next page is its first stretch not held, from the
    /// first position missing to the last, one page at most.
    #[test]
    fn a_page_is_asked_after_the_row_before_it_or_at_its_position() {
        // 10^6 matches over three shortcodes.
        let m = Matches::new(vec![("AGS".into(), 400_000), ("MNG".into(), 350_000), ("ZZ".into(), 250_000)]);
        let mut r: Ranked<String> = Ranked::default();
        assert_eq!(cursor(&m, &r, 0), Cursor::Top);
        assert_eq!(next_ask(&m, &r, 0, 250), Some(Ask { from: 0, cursor: Cursor::Top, limit: 250 }), "the first window");
        // The first page is in: AGS-1 … AGS-250.
        let first: Vec<String> = (1..=250).map(|s| format!("AGS-{s}")).collect();
        let refs: Vec<&str> = first.iter().map(String::as_str).collect();
        placed(&m, &mut r, 0, &refs);
        assert_eq!(next_ask(&m, &r, 0, 250), None, "in hand");
        // Scrolling on: the next stretch starts right behind AGS-250.
        assert_eq!(cursor(&m, &r, 250), Cursor::After("AGS-250".into()));
        assert_eq!(next_ask(&m, &r, 125, 375), Some(Ask { from: 250, cursor: Cursor::After("AGS-250".into()), limit: 125 }));
        // A jump to the middle of MNG: nothing held near — at the position.
        assert_eq!(next_ask(&m, &r, 600_000, 600_250), Some(Ask { from: 600_000, cursor: Cursor::At(600_000), limit: 250 }));
        // The last rows of the list, from the scrollbar's end.
        assert_eq!(next_ask(&m, &r, 999_800, 1_000_000), Some(Ask { from: 999_800, cursor: Cursor::At(999_800), limit: 200 }));
        // Across a shortcode's end: the row before is the last of AGS.
        placed(&m, &mut r, 399_990, &["AGS-399991", "AGS-399992", "AGS-399993", "AGS-399994", "AGS-399995", "AGS-399996", "AGS-399997", "AGS-399998", "AGS-399999", "AGS-400000"]);
        assert_eq!(cursor(&m, &r, 400_000), Cursor::After("AGS-400000".into()), "the next page is MNG's first");
        // A held stretch with a hole in it: the page spans the hole only.
        let mut h: Ranked<String> = Ranked::default();
        placed(&m, &mut h, 0, &["AGS-1", "AGS-2", "AGS-3"]);
        placed(&m, &mut h, 6, &["AGS-7", "AGS-8"]);
        assert_eq!(next_ask(&m, &h, 0, 8), Some(Ask { from: 3, cursor: Cursor::After("AGS-3".into()), limit: 3 }));
        // A window wider than a page asks one page.
        assert_eq!(next_ask(&m, &Ranked::<String>::default(), 0, 2_000).map(|a| a.limit), Some(MAX_PAGE));
        // No matches: nothing to ask.
        assert_eq!(next_ask(&Matches::default(), &Ranked::<String>::default(), 0, 250), None);
    }

    /// FILTER-20: the list is asked under the nav's effective filter while it
    /// has a group — none at all when it admits nothing — and under the
    /// empty filter (every ticket) while it has none.
    #[test]
    fn the_list_follows_the_navs_filter() {
        assert_eq!(list_filter(false, None), Some(String::new()), "no filter: every ticket");
        assert_eq!(list_filter(true, Some("kind build".into())), Some("kind build".into()));
        assert_eq!(list_filter(true, None), None, "a filter that admits nothing asks nothing");
    }

    /// FILTER-20: the line under the list — counting until the counts are
    /// in; nothing to list; the server's refusal before either.
    #[test]
    fn the_note_says_counting_empty_or_refused() {
        assert_eq!(note_of(Some(""), None, None), Some(Note::Counting));
        assert_eq!(note_of(Some("kind build"), Some(0), None), Some(Note::Empty));
        assert_eq!(note_of(None, None, None), Some(Note::Empty), "admits nothing");
        assert_eq!(note_of(Some("kind build"), Some(12), None), None);
        assert_eq!(note_of(Some("kind build"), Some(12), Some("no")), Some(Note::Refused("no".into())));
    }

    /// FILTER-20, a deep link: a row that shows is left where it is; one
    /// above or below the viewport is brought to its middle, never above the
    /// first row.
    #[test]
    fn the_open_ticket_is_scrolled_to_mid_viewport_unless_it_shows() {
        assert_eq!(scroll_row(100.0, 40.0, 100), None, "the top row");
        assert_eq!(scroll_row(100.0, 40.0, 139), None, "the bottom row");
        assert_eq!(scroll_row(100.0, 40.0, 140), Some(140.0 - 19.5));
        assert_eq!(scroll_row(100.0, 40.0, 99), Some(99.0 - 19.5));
        assert_eq!(scroll_row(500.0, 40.0, 3), Some(0.0), "never above the first row");
        assert_eq!(scroll_row(0.0, 40.0, 600_000), Some(600_000.0 - 19.5));
    }
}
