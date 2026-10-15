//! MENGER-39: **the tray** — the owner's queue as tabs along the bottom edge
//! of the app's main column (MENGER-40's variant C, the one chosen).
//!
//! - **What it holds** is the app's copy of `tickets.queued` — fetched on
//!   every connection and after every `lagged` ([`refresh`]), kept in
//!   between by `watch`: `queued` and `snoozed` carry the entry and its row
//!   (an upsert), `unqueued` takes it out, a row event of a queued ticket
//!   moves its title ([`Queue::apply`]). As `inputs`' copy is, the events
//!   heard while a fetch is out are replayed onto its answer.
//! - **A tab** per queued ticket that is not snoozed, most urgent first, then
//!   the oldest: its id, its title (cut short) and an urgency chip — `today`
//!   in the Coding colours, `now` and `blocking` in the contract's urgent
//!   red, taller, nudging every few seconds until the owner opens it. A new
//!   tab slides up into place.
//! - **The sheet**: a click raises it above the tray — the urgency, who
//!   queued it and when, the reason, and the named input's question,
//!   answerable in place (recorded with source `app`, as every answer the
//!   app gives; answering is the owner done with it, so the ticket leaves
//!   the queue — the input itself stays the agent's to close), then *open
//!   the ticket*, *snooze 10m* and *done with it*. A click on the open tab,
//!   Esc, or a press anywhere outside the tray — a frame's included
//!   (APP-26, [`outside_lowers`]) — lowers it.
//! - **Chosen, then submitted** (APP-25): an option clicked is selected
//!   ([`Pick`]), nothing sent; ticks and a reply are held the same way, and
//!   one **submit** sends whichever it is. The sheet comes down once that
//!   write (the answer, then the unqueue) succeeds; a refusal keeps it up
//!   with the problem shown.
//! - **The question on hover** (APP-25): hovering or focusing a tab shows a
//!   card above it, after a short delay — the named input's question, else
//!   the reason, its urgency and who queued it ([`card`]) — gone on leave
//!   or blur and never while that tab's sheet is up. Settings → Queue turns
//!   it off ([`hover_setting`]); on by default.
//! - **Request changes** (APP-23): beside the input's answer (alone when
//!   none is named), a box for a short note — Ctrl+Enter (AGS-28's
//!   `comment.save`) sends it as `tickets.request_changes`: the note goes on
//!   the ticket's conversation as a change request and the ticket leaves the
//!   queue, handed back to whoever queued it; the input stays open. Esc
//!   closes the box. The draft is the tray's, so the clock's tick or an
//!   answer elsewhere does not lose it.
//! - **A hidden tab's title** carries the count: `(2) menger`.
//! - **Motion** is DESIGN-39's setting: `calm` keeps the slide and drops the
//!   nudge, `none` keeps the tray and moves nothing (index.html's rules).
//! - **An arrival** (MENGER-45, MENGER-40's variant C′): a tab that shows up
//!   in front of the reader — a new entry heard on `watch`, one raised to a
//!   higher urgency, one whose snooze ended — flashes in its urgency's
//!   colour (`fresh`), a line sweeps the tray's bottom edge, a label says
//!   "queued · <urgency> · <who>" for ~3s, and a dot (`unseen`) stays on the
//!   tab until it is opened. What a fetch brings (the first load, a
//!   reconnect's refetch) was not arriving in front of anyone: no flash, no
//!   dot ([`step`]). While the page is hidden the flash waits for it to show.
//!
//! What decides — applying an event, the order, what shows at a moment,
//! when a snooze ends, the title, the words — is pure and pinned by native
//! tests; the rest is the session's socket and the DOM.

use menger_tickets_api::vocab::CallerSource;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use leptos::prelude::*;
use menger_tickets_api::queue::order;
use menger_tickets_api::{Input, InputKind, InputState, InputValue, QueueEntry, TicketSummary, TicketsEvent, Urgency};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::{bridge, dom, inputs, session};

/// How long *snooze* puts a ticket away, in minutes.
pub(crate) const SNOOZE_MINUTES: u32 = 10;

/// One queued ticket: its entry and its row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub(crate) entry: QueueEntry,
    pub(crate) ticket: TicketSummary,
}

/// The queue as the app holds it, in the queue's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Queue {
    rows: Vec<Row>,
}

impl Queue {
    /// A `queued {}` answer: one `queue_row` per entry, in the queue's
    /// order; anything else contributes nothing.
    pub(crate) fn from_events(events: impl IntoIterator<Item = TicketsEvent>) -> Self {
        let rows = events
            .into_iter()
            .filter_map(|ev| match ev {
                TicketsEvent::QueueRow { entry, ticket } => Some(Row { entry, ticket }),
                _ => None,
            })
            .collect();
        Self { rows }
    }

    /// Whether `ev` is one the copy may take.
    pub(crate) fn concerns(ev: &TicketsEvent) -> bool {
        matches!(
            ev,
            TicketsEvent::Queued { .. }
                | TicketsEvent::Snoozed { .. }
                | TicketsEvent::Unqueued { .. }
                | TicketsEvent::ChangesRequested { .. }
                | TicketsEvent::Amended { .. }
                | TicketsEvent::StatusChanged { .. }
                | TicketsEvent::KindChanged { .. }
                | TicketsEvent::Deleted { .. }
                | TicketsEvent::Reshortcoded { .. }
        )
    }

    /// Apply a `watch` event; whether the copy moved.
    pub(crate) fn apply(&mut self, ev: &TicketsEvent) -> bool {
        match ev {
            TicketsEvent::Queued { entry, ticket, .. } | TicketsEvent::Snoozed { entry, ticket } => {
                self.upsert(Row { entry: entry.clone(), ticket: ticket.clone() })
            }
            // APP-23: a change request's answer takes it out as `unqueued` does.
            TicketsEvent::Unqueued { id, .. } | TicketsEvent::ChangesRequested { id, .. } => self.remove(id),
            TicketsEvent::Deleted { ids, .. } => {
                let before = self.rows.len();
                self.rows.retain(|r| !ids.contains(&r.entry.ticket));
                self.rows.len() != before
            }
            // ACT-6: a queued ticket moved — its entry and row, renamed.
            TicketsEvent::Reshortcoded { moves, tickets } => {
                let mut moved = false;
                for r in &mut self.rows {
                    if let Some(m) = moves.iter().find(|m| m.from == r.entry.ticket) {
                        r.entry.ticket = m.to.clone();
                        if let Some(t) = tickets.iter().find(|t| t.id == m.to) {
                            r.ticket = TicketSummary::from(t);
                        }
                        moved = true;
                    }
                }
                moved
            }
            TicketsEvent::Amended { ticket, .. } | TicketsEvent::StatusChanged { ticket, .. } | TicketsEvent::KindChanged { ticket, .. } => {
                match self.rows.iter_mut().find(|r| r.ticket.id == ticket.id) {
                    Some(r) => {
                        let row = TicketSummary::from(ticket);
                        let moved = r.ticket != row;
                        r.ticket = row;
                        moved
                    }
                    None => false,
                }
            }
            _ => false,
        }
    }

    fn upsert(&mut self, row: Row) -> bool {
        if let Some(held) = self.rows.iter_mut().find(|r| r.entry.ticket == row.entry.ticket) {
            if *held == row {
                return false;
            }
            *held = row;
        } else {
            self.rows.push(row);
        }
        self.rows.sort_by(|a, b| order(&a.entry, &b.entry));
        true
    }

    fn remove(&mut self, id: &str) -> bool {
        let before = self.rows.len();
        self.rows.retain(|r| r.entry.ticket != id);
        self.rows.len() != before
    }

    /// Every queued ticket, in the queue's order.
    #[cfg(test)]
    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// `id`'s row, when it is queued.
    #[cfg(test)]
    pub(crate) fn get(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.entry.ticket == id)
    }

    /// What the tray shows at `now`: every row not snoozed then.
    pub(crate) fn showing(&self, now: i64) -> Vec<Row> {
        self.rows.iter().filter(|r| !r.entry.snoozed_at(now)).cloned().collect()
    }

    /// When the next snooze after `now` ends — when the tray must look again.
    pub(crate) fn next_wake(&self, now: i64) -> Option<i64> {
        self.rows.iter().filter_map(|r| r.entry.snoozed_until_unix).filter(|u| *u > now).min()
    }
}

/// MENGER-45: a tab that showed up in front of the reader — what flashes,
/// in which colour, and what the label says.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Arrival {
    pub(crate) ticket: String,
    pub(crate) urgency: Urgency,
    pub(crate) who: String,
}

impl Arrival {
    /// The label beside the tray: `queued · today · Ada`.
    pub(crate) fn label(&self) -> String {
        format!("queued · {} · {}", self.urgency.as_str(), self.who)
    }
}

/// What moved the app's copy of the queue.
pub(crate) enum Cause<'a> {
    /// A `watch` event, heard live.
    Heard(&'a TicketsEvent),
    /// A fetch landed — the first load, or a reconnect's (or a `lagged`'s)
    /// refetch: its answer, then the events heard while it was out.
    Fetched(Vec<TicketsEvent>, &'a [TicketsEvent]),
    /// The tray's clock moved from one moment to the next (a snooze may
    /// have ended between them).
    Tick(i64, i64),
}

/// The tabs in `after` that were not in `before`, or are there at a higher
/// urgency than they were.
pub(crate) fn arrived(before: &[Row], after: &[Row]) -> Vec<Arrival> {
    after
        .iter()
        .filter(|r| before.iter().find(|b| b.entry.ticket == r.entry.ticket).is_none_or(|b| r.entry.urgency > b.entry.urgency))
        .map(|r| Arrival { ticket: r.entry.ticket.clone(), urgency: r.entry.urgency, who: who(&r.entry).to_string() })
        .collect()
}

/// One move of the copy at `now`: whether it moved, and what arrived in
/// front of the reader. Only a live event or a snooze ending arrives; a
/// fetch replaces the copy and announces nothing, and nothing arrives
/// before the first fetch has landed.
pub(crate) fn step(held: &mut Option<Queue>, cause: Cause<'_>, now: i64) -> (bool, Vec<Arrival>) {
    match cause {
        Cause::Heard(ev) => {
            let Some(q) = held.as_mut() else { return (false, Vec::new()) };
            let before = q.showing(now);
            if !q.apply(ev) {
                return (false, Vec::new());
            }
            (true, arrived(&before, &q.showing(now)))
        }
        Cause::Fetched(answer, heard) => {
            *held = Some(settle(answer, heard));
            (true, Vec::new())
        }
        Cause::Tick(from, to) => match held.as_ref() {
            Some(q) if to > from => (false, arrived(&q.showing(from), &q.showing(to))),
            _ => (false, Vec::new()),
        },
    }
}

/// The fetch's rule: `answer`, then every event heard while it was out.
pub(crate) fn settle(answer: Vec<TicketsEvent>, heard: &[TicketsEvent]) -> Queue {
    let mut q = Queue::from_events(answer);
    for ev in heard {
        q.apply(ev);
    }
    q
}

/// The document's title with `n` waiting (and the tab hidden): `(2) menger`.
pub(crate) fn titled(base: &str, n: usize) -> String {
    match n {
        0 => base.to_string(),
        n => format!("({n}) {base}"),
    }
}

/// How long ago `then` was, at `now`, in the app's words.
pub(crate) fn ago(now: i64, then: i64) -> String {
    let s = (now - then).max(0);
    match s {
        0..=59 => "just now".into(),
        60..=3_599 => format!("{}m ago", s / 60),
        3_600..=86_399 => format!("{}h ago", s / 3_600),
        _ => format!("{}d ago", s / 86_400),
    }
}

/// Who queued it: their name, else their principal.
pub(crate) fn who(e: &QueueEntry) -> &str {
    e.name.as_deref().unwrap_or(&e.by)
}

/// How a tab stands: its sheet up, opened since it arrived, flashing its
/// arrival, arrived and not yet opened (the dot).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mark {
    pub(crate) open: bool,
    pub(crate) seen: bool,
    pub(crate) fresh: bool,
    pub(crate) unseen: bool,
}

/// A tab's classes: its urgency, `loud` for now and blocking, `open` while
/// its sheet is up, `seen` once the owner has opened it (no more nudging),
/// `fresh` while it plays its arrival, `unseen` (the dot) from its arrival
/// until it is opened.
pub(crate) fn tab_class(e: &QueueEntry, m: Mark) -> String {
    let mut c = format!("qtab {}", e.urgency.as_str());
    for (on, word) in [(e.urgency.loud(), "loud"), (m.open, "open"), (m.seen, "seen"), (m.fresh, "fresh"), (m.unseen && !m.seen, "unseen")] {
        if on {
            c.push(' ');
            c.push_str(word);
        }
    }
    c
}

/// An answer as the sheet says it.
pub(crate) fn said(input: &Input, v: &InputValue) -> String {
    let label = |o: &str| input.options.iter().find(|x| x.value == o).map_or_else(|| o.to_string(), |x| x.label.clone());
    match v {
        InputValue::Choice { option } => label(option),
        InputValue::Multi { options } => options.iter().map(|o| label(o)).collect::<Vec<_>>().join(", "),
        InputValue::Text { text } => text.clone(),
    }
}

/// Where *open the ticket* goes: the ticket, at its input when it names one.
pub(crate) fn open_href(e: &QueueEntry) -> String {
    match &e.input {
        Some(i) => crate::needs::answer_href(&e.ticket, i),
        None => format!("#/ticket/{}", e.ticket),
    }
}

/// APP-25: what the sheet's controls hold until **submit** — the option
/// chosen, the boxes ticked, the reply written. Choosing sends nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Pick {
    pub(crate) choice: Option<String>,
    pub(crate) ticked: Vec<String>,
    pub(crate) text: String,
}

impl Pick {
    /// `value` chosen (a choice's option); the one chosen before is not.
    pub(crate) fn choose(&mut self, value: &str) {
        self.choice = Some(value.to_string());
    }

    /// `value` ticked or not (a multi's box).
    pub(crate) fn tick(&mut self, value: &str, on: bool) {
        self.ticked.retain(|v| v != value);
        if on {
            self.ticked.push(value.to_string());
        }
    }

    /// Whether `value` is the option chosen.
    pub(crate) fn chosen(&self, value: &str) -> bool {
        self.choice.as_deref() == Some(value)
    }

    /// What submit sends for an input of `kind`, or `None` while there is
    /// nothing to send (submit disabled): no option chosen, no box ticked,
    /// a reply of only spaces.
    pub(crate) fn value(&self, kind: InputKind) -> Option<InputValue> {
        match kind {
            InputKind::Choice => self.choice.clone().map(|option| InputValue::Choice { option }),
            InputKind::Multi => (!self.ticked.is_empty()).then(|| InputValue::Multi { options: self.ticked.clone() }),
            InputKind::Text => (!self.text.trim().is_empty()).then(|| InputValue::Text { text: self.text.clone() }),
        }
    }
}

/// APP-25: what a tab's hover card says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Card {
    /// The full question: the named input's, else the reason.
    pub(crate) question: String,
    /// `blocking · Ada` — the urgency and who queued it.
    pub(crate) line: String,
}

/// The card for `row`, `input` its named input as the app holds it (if
/// any): the input's question when it has one, else the queue's reason.
pub(crate) fn card(row: &Row, input: Option<&Input>) -> Card {
    let e = &row.entry;
    let question = input
        .filter(|i| e.input.as_deref() == Some(i.id.as_str()))
        .and_then(|i| i.question.clone())
        .filter(|q| !q.trim().is_empty())
        .unwrap_or_else(|| e.reason.clone());
    Card { question, line: format!("{} · queued by {}", e.urgency.as_str(), who(e)) }
}

/// How long a tab is hovered (or focused) before its card shows, in ms.
pub(crate) const CARD_DELAY_MS: i32 = 380;

thread_local! {
    /// APP-25: Settings → Queue → show the question on hover — read from
    /// the record once, set by the settings page, followed by the tray.
    static HOVER: ArcRwSignal<bool> = ArcRwSignal::new(crate::settings::load().queue_hover);
}

/// APP-25: the hover card setting, as a signal (on by default).
pub(crate) fn hover_setting() -> ArcRwSignal<bool> {
    HOVER.with(Clone::clone)
}

thread_local! {
    /// The queue — `None` until the first fetch lands.
    static QUEUE: ArcRwSignal<Option<Queue>> = ArcRwSignal::new(None);
    /// The server's clock less the browser's, in seconds (`queue_end`'s).
    static SKEW: Cell<i64> = const { Cell::new(0) };
    /// The newest fetch; an older one's answer is dropped.
    static FETCH: Cell<u32> = const { Cell::new(0) };
    /// While a fetch is out: the events heard since it went.
    static HEARD: RefCell<Option<Vec<TicketsEvent>>> = const { RefCell::new(None) };
    /// MENGER-45: arrivals not yet played — they wait while the page is hidden.
    static ARRIVING: ArcRwSignal<Vec<Arrival>> = ArcRwSignal::new(Vec::new());
    /// Whether the page is hidden (`visibilitychange`).
    static HIDDEN: ArcRwSignal<bool> = ArcRwSignal::new(false);
}

/// Arrivals to play, once the page is visible.
fn announce(arrivals: Vec<Arrival>) {
    if !arrivals.is_empty() {
        ARRIVING.with(|a| a.update(|q| q.extend(arrivals)));
    }
}

/// The queue, as a signal.
pub(crate) fn signal() -> ArcRwSignal<Option<Queue>> {
    QUEUE.with(Clone::clone)
}

/// Now, by the server's clock, in unix seconds.
pub(crate) fn now() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64 + SKEW.with(Cell::get)
}

/// A new connection, or a `lagged`: ask again.
pub(crate) fn refresh() {
    fetch();
}

/// A `watch` event — called by the session for every event.
pub(crate) fn heard(ev: &TicketsEvent) {
    if !Queue::concerns(ev) {
        return;
    }
    HEARD.with(|h| {
        if let Some(buf) = h.borrow_mut().as_mut() {
            buf.push(ev.clone());
        }
    });
    let mut arrivals = Vec::new();
    QUEUE.with(|s| {
        s.maybe_update(|held| {
            let (moved, a) = step(held, Cause::Heard(ev), now());
            arrivals = a;
            moved
        });
    });
    announce(arrivals);
}

fn fetch() {
    let Some(client) = session::client() else { return };
    let n = FETCH.with(|f| {
        let n = f.get().wrapping_add(1);
        f.set(n);
        n
    });
    HEARD.with(|h| *h.borrow_mut() = Some(Vec::new()));
    spawn_local(async move {
        let answer = client.queued_all(Some(true)).await;
        if FETCH.with(Cell::get) != n {
            return;
        }
        let heard = HEARD.with(|h| h.borrow_mut().take()).unwrap_or_default();
        match answer {
            Ok(events) => {
                for ev in &events {
                    match ev {
                        TicketsEvent::QueueEnd { now_unix, .. } => SKEW.with(|s| s.set(now_unix - (js_sys::Date::now() / 1000.0) as i64)),
                        TicketsEvent::Error { message } => leptos::logging::log!("queue: {message}"),
                        _ => {}
                    }
                }
                let t = now();
                QUEUE.with(|s| s.update(|held| drop(step(held, Cause::Fetched(events, &heard), t))));
            }
            Err(e) => leptos::logging::log!("queue: {}", session::describe(&e)),
        }
    });
}

/// A queue write from the tray, its refusal in `problem`.
async fn write(problem: RwSignal<Option<String>>, ev: Result<Vec<TicketsEvent>, plexus_client::ClientError>) {
    match ev {
        Ok(events) => {
            for ev in events {
                if let TicketsEvent::Error { message } = ev {
                    problem.try_set(Some(message));
                }
            }
        }
        Err(e) => {
            problem.try_set(Some(session::describe(&e)));
        }
    }
}

/// The owner done with `id`: out of the queue.
fn done(id: String, problem: RwSignal<Option<String>>) {
    let Some(client) = session::client() else { return };
    let Ok(id) = crate::typed::typed(&id) else { return };
    spawn_local(async move { write(problem, client.unqueue_all(id, Some(CallerSource::App)).await).await });
}

/// `id` away for [`SNOOZE_MINUTES`].
fn snooze(id: String, problem: RwSignal<Option<String>>) {
    let Some(client) = session::client() else { return };
    let Ok(id) = crate::typed::typed(&id) else { return };
    spawn_local(async move { write(problem, client.snooze_all(id, SNOOZE_MINUTES, Some(CallerSource::App)).await).await });
}

/// APP-23: `id` handed back with `body` — a change request (source `app`);
/// on its answer the tab leaves at once and the box closes.
fn request_changes(id: String, body: String, problem: RwSignal<Option<String>>, busy: RwSignal<bool>, draft: Draft) {
    let Some(client) = session::client() else { return };
    busy.set(true);
    spawn_local(async move {
        let asked = match (crate::typed::typed(&id), crate::typed::typed(&body)) {
            (Ok(id), Ok(body)) => client.request_changes_all(id, body, Some(CallerSource::App)).await,
            (Err(why), _) | (_, Err(why)) => {
                problem.try_set(Some(why));
                busy.try_set(false);
                return;
            }
        };
        match asked {
            Ok(events) => {
                for ev in events {
                    match ev {
                        TicketsEvent::Error { message } => {
                            problem.try_set(Some(message));
                        }
                        ev @ TicketsEvent::ChangesRequested { .. } => {
                            draft.clear();
                            heard(&ev);
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                problem.try_set(Some(session::describe(&e)));
            }
        }
        busy.try_set(false);
    });
}

/// APP-23: the Request changes box — open or not, and what is written in
/// it — held by the tray, not the sheet, so a re-drawn sheet keeps it.
#[derive(Clone, Copy)]
struct Draft {
    open: RwSignal<bool>,
    text: RwSignal<String>,
}

impl Draft {
    fn new() -> Self {
        Draft { open: RwSignal::new(false), text: RwSignal::new(String::new()) }
    }

    fn clear(self) {
        self.open.try_set(false);
        self.text.try_set(String::new());
    }
}

/// What the Request changes box says while it is empty.
pub(crate) const CHANGES_PLACEHOLDER: &str = "What should change? It goes on the ticket's conversation, and the ticket goes back to whoever queued it — Ctrl+Enter sends, Esc closes";

/// APP-25: **submit** — answer `input` of `id` with `value` (source `app`),
/// then — the owner has answered what was asked — take the ticket out of
/// the queue. Only once both writes succeed does the sheet come down (and
/// the pick go); a refusal of either keeps it up with the problem shown.
#[allow(clippy::too_many_arguments)]
fn submit(id: String, input: String, value: InputValue, problem: RwSignal<Option<String>>, busy: RwSignal<bool>, open: RwSignal<Option<String>>, pick: RwSignal<Pick>) {
    let Some(client) = session::client() else { return };
    busy.set(true);
    problem.set(None);
    spawn_local(async move {
        let sent = bridge::submit(&client, id.clone(), bridge::Submit { input, value, rationale: None }).await;
        let unqueued = match sent {
            Ok(_) => match crate::typed::typed(&id) {
                Ok(tid) => client.unqueue_all(tid, Some(CallerSource::App)).await.map_err(|e| session::describe(&e)),
                Err(why) => Err(why),
            },
            Err(why) => Err(why),
        };
        let refused = match unqueued {
            Ok(events) => {
                let refused = events.iter().find_map(|ev| match ev {
                    TicketsEvent::Error { message } => Some(message.clone()),
                    _ => None,
                });
                if refused.is_none() {
                    // The write's own answer takes the tab out now; the
                    // watch's copy of it moves nothing more.
                    for ev in &events {
                        heard(ev);
                    }
                }
                refused
            }
            Err(why) => Some(why),
        };
        match refused {
            Some(why) => {
                problem.try_set(Some(why));
            }
            None => {
                pick.try_set(Pick::default());
                if open.try_get_untracked().flatten().as_deref() == Some(id.as_str()) {
                    open.try_set(None);
                }
            }
        }
        busy.try_set(false);
    });
}

/// MENGER-39: the document's title follows the count while the tab is
/// hidden — installed once by the App.
pub(crate) fn install_title() {
    let Some(doc) = dom::window().document() else { return };
    let base = doc.title();
    let hidden = HIDDEN.with(Clone::clone);
    hidden.set(doc.hidden());
    let on = Closure::<dyn Fn()>::new({
        let hidden = hidden.clone();
        move || {
            if let Some(d) = dom::window().document() {
                hidden.try_set(d.hidden());
            }
        }
    });
    let _ = doc.add_event_listener_with_callback("visibilitychange", on.as_ref().unchecked_ref());
    on.forget();
    let held = signal();
    let clock = clock();
    Effect::new(move |_| {
        let n = held.with(|q| q.as_ref().map_or(0, |q| q.showing(clock.get()).len()));
        let title = if hidden.get() { titled(&base, n) } else { base.clone() };
        if let Some(d) = dom::window().document() {
            if d.title() != title {
                d.set_title(&title);
            }
        }
    });
}

thread_local! {
    static CLOCK: RefCell<Option<RwSignal<i64>>> = const { RefCell::new(None) };
}

/// The tray's clock: now, moved when a snooze ends (and every half minute,
/// for the sheet's "ago"). One for the app.
fn clock() -> RwSignal<i64> {
    if let Some(c) = CLOCK.with(|c| *c.borrow()) {
        return c;
    }
    let c = RwSignal::new(now());
    CLOCK.with(|x| *x.borrow_mut() = Some(c));
    let held = signal();
    spawn_local(async move {
        loop {
            let t = now();
            let wake = held.with_untracked(|q| q.as_ref().and_then(|q| q.next_wake(t)));
            let wait = wake.map_or(30, |w| (w - t).clamp(1, 30));
            dom::sleep_ms((wait * 1000) as i32).await;
            let (from, to) = (c.try_get_untracked().unwrap_or(t), now());
            if c.try_set(to).is_some() {
                return;
            }
            // A snooze that ended is an arrival.
            let arrivals = held.try_update_untracked(|h| step(h, Cause::Tick(from, to), to).1).unwrap_or_default();
            announce(arrivals);
        }
    });
    c
}

/// The tray: a tab per queued ticket along the main column's bottom edge,
/// and the sheet the open one raises.
#[component]
pub(crate) fn Tray() -> impl IntoView {
    let held = signal();
    let clock = clock();
    inputs::want();
    let inputs_held = inputs::signal();
    let card_inputs = inputs_held.clone();
    let open = RwSignal::new(None::<String>);
    let seen = RwSignal::new(HashSet::<String>::new());
    // MENGER-45: the tabs playing an arrival, the tabs with a dot, and the
    // edge's sweep and the label (the arrival they say, a generation).
    let fresh = RwSignal::new(HashSet::<String>::new());
    let unseen = RwSignal::new(HashSet::<String>::new());
    let flash = RwSignal::new(None::<(Arrival, u32)>);
    let problem = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let draft = Draft::new();
    // APP-25: the open sheet's choice, held until submit; the tab hovered
    // (its card's place, once the delay is out) and the hover's generation.
    let pick = RwSignal::new(Pick::default());
    let hovered = RwSignal::new(None::<(String, f64, f64)>);
    let hover_gen = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let hover_on = hover_setting();
    let tray_ref = NodeRef::<leptos::html::Div>::new();
    // Arrivals play once the page is visible: each tab loses `fresh` for a
    // frame and takes it again (a raise replays it), loses `seen` (a raised
    // tab nudges again) and takes the dot; the most urgent sweeps the edge
    // and is the label.
    {
        let arriving = ARRIVING.with(Clone::clone);
        let hidden = HIDDEN.with(Clone::clone);
        let generation = std::rc::Rc::new(Cell::new(0u32));
        Effect::new(move |_| {
            if hidden.get() || arriving.with(Vec::is_empty) {
                return;
            }
            let got = arriving.try_update_untracked(std::mem::take).unwrap_or_default();
            let ids: Vec<String> = got.iter().map(|a| a.ticket.clone()).collect();
            seen.update(|s| s.retain(|id| !ids.contains(id)));
            unseen.update(|s| s.extend(ids.iter().cloned()));
            fresh.update(|s| s.retain(|id| !ids.contains(id)));
            let Some(top) = got.into_iter().max_by_key(|a| a.urgency) else { return };
            let n = generation.get().wrapping_add(1);
            generation.set(n);
            let generation = generation.clone();
            spawn_local(async move {
                dom::sleep_ms(40).await;
                fresh.try_update(|s| s.extend(ids));
                flash.try_set(Some((top, n)));
                dom::sleep_ms(3_600).await;
                if generation.get() == n {
                    flash.try_set(None);
                }
            });
        });
    }
    let rows = {
        let held = held.clone();
        Memo::new(move |_| held.with(|q| q.as_ref().map(|q| q.showing(clock.get())).unwrap_or_default()))
    };
    // The open ticket left the tray (unqueued, snoozed, answered elsewhere):
    // the sheet comes down.
    Effect::new(move |_| {
        let gone = open.with(|o| o.as_ref().is_some_and(|id| rows.with(|r| !r.iter().any(|x| &x.entry.ticket == id))));
        if gone {
            open.set(None);
            pick.set(Pick::default());
        }
    });
    // APP-26: a press anywhere outside the tray lowers the sheet (it slides
    // down as every close does; Motion none, at once); not while a submit
    // is out. A tab, the sheet and the card are inside, so the press that
    // opens or switches a tab is the tab's own. A press in a frame (a
    // ticket's body) never reaches this window: focus going into the frame
    // blurs it instead, and that is heard as the press.
    let lower_outside = move |target: Option<web_sys::Element>| {
        let inside = target.is_some_and(|el| el.closest("#qtray").ok().flatten().is_some());
        let (Some(is_open), Some(is_busy)) = (open.try_with_untracked(Option::is_some), busy.try_get_untracked()) else { return };
        if outside_lowers(is_open, is_busy, inside) {
            open.set(None);
        }
    };
    let press = window_event_listener(leptos::ev::pointerdown, move |ev| {
        lower_outside(ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()));
    });
    let into_frame = window_event_listener(leptos::ev::blur, move |_| {
        spawn_local(async move {
            dom::sleep_ms(0).await;
            let active = dom::window().document().and_then(|d| d.active_element());
            if let Some(frame) = active.filter(|el| el.tag_name().eq_ignore_ascii_case("iframe")) {
                lower_outside(Some(frame));
            }
        });
    });
    // The tray goes with the session's view (a sign-out): so do they.
    on_cleanup(move || {
        press.remove();
        into_frame.remove();
    });
    let toggle = move |id: String| {
        problem.set(None);
        draft.clear();
        pick.set(Pick::default());
        seen.update(|s| {
            s.insert(id.clone());
        });
        unseen.update(|s| {
            s.remove(&id);
        });
        open.update(|o| *o = if o.as_deref() == Some(id.as_str()) { None } else { Some(id) });
    };
    // APP-25: the sheet slides down rather than vanishing — the last ticket
    // drawn stays in it while it closes (on submit its row has already
    // left), and is replaced when another opens.
    let last = RwSignal::new(None::<Row>);
    let sheet = move || {
        let row = match open.get() {
            Some(id) => rows.with(|r| r.iter().find(|x| x.entry.ticket == id).cloned()).or_else(|| last.get_untracked().filter(|r| r.entry.ticket == id))?,
            None => last.get_untracked()?,
        };
        last.set(Some(row.clone()));
        let id = row.entry.ticket.clone();
        let e = row.entry.clone();
        let input = named_input(&inputs_held, &e);
        // The time moves on its own: the sheet is not drawn again for it.
        let hd = {
            let e = e.clone();
            move || format!("{} · {} · {}", e.urgency.as_str(), who(&e), ago(clock.get(), e.asked_unix))
        };
        let (snooze_id, done_id) = (id.clone(), id.clone());
        Some(view! {
            <div class="qsheet-in" data-ticket-id=id.clone()>
                <div class=format!("qhd {}", e.urgency.as_str())>{hd}</div>
                <h4><a class="code" href=format!("#/ticket/{id}")>{id.clone()}</a>" · "{row.ticket.title.clone()}</h4>
                <p class="qwhy">{e.reason.clone()}</p>
                {input_view(id.clone(), e.input.clone(), input, problem, busy, open, pick)}
                {changes_view(id.clone(), problem, busy, draft)}
                <div class="qacts">
                    <a class="qbtn fill" href=open_href(&e) on:click=move |_| open.set(None)>"open the ticket"</a>
                    <button type="button" class="qbtn" on:click=move |_| snooze(snooze_id.clone(), problem)>{format!("snooze {SNOOZE_MINUTES}m")}</button>
                    <button type="button" class="qbtn" on:click=move |_| done(done_id.clone(), problem)>"done with it"</button>
                </div>
                {move || problem.get().map(|p| view! { <p class="qproblem" role="alert">{p}</p> })}
            </div>
        })
    };
    // APP-25: a tab hovered or focused shows its card after the delay, at
    // the tab (relative to the tray); leaving, blurring or opening it ends it.
    use std::sync::atomic::Ordering::Relaxed;
    let hover_end = {
        let gen = hover_gen.clone();
        move || {
            gen.fetch_add(1, Relaxed);
            hovered.set(None);
        }
    };
    let hover_begin = {
        let gen = hover_gen.clone();
        let on = hover_on.clone();
        move |id: String, ev: web_sys::Event| {
            let n = gen.fetch_add(1, Relaxed).wrapping_add(1);
            if !on.get_untracked() {
                return;
            }
            let (Some(tab), Some(tray)) = (ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()), tray_ref.get_untracked()) else { return };
            let (t, r) = (tab.get_bounding_client_rect(), tray.get_bounding_client_rect());
            let place = (id, t.left() - r.left(), r.bottom() - t.top() + 6.0);
            let gen = gen.clone();
            spawn_local(async move {
                dom::sleep_ms(CARD_DELAY_MS).await;
                if gen.load(Relaxed) == n {
                    hovered.try_set(Some(place));
                }
            });
        }
    };
    let card_view = {
        let on = hover_on.clone();
        let inputs_held = card_inputs;
        move || {
            if !on.get() {
                return None;
            }
            let (id, left, bottom) = hovered.get()?;
            if open.with(|o| o.as_deref() == Some(id.as_str())) {
                return None;
            }
            let row = rows.with(|r| r.iter().find(|x| x.entry.ticket == id).cloned())?;
            let c = card(&row, named_input(&inputs_held, &row.entry).as_ref());
            Some(view! {
                <div
                    id="qcard"
                    class=format!("qcard {}", row.entry.urgency.as_str())
                    role="tooltip"
                    data-ticket-id=id.clone()
                    style=format!("--x: {left:.0}px; bottom: {bottom:.0}px")
                >
                    <div class=format!("qhd {}", row.entry.urgency.as_str())>{format!("{id} · {}", c.line)}</div>
                    <p class="qcq">{c.question}</p>
                </div>
            })
        }
    };
    view! {
        <div
            id="qtray"
            node_ref=tray_ref
            class="qtray"
            class:empty=move || rows.with(Vec::is_empty)
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                if crate::bindings::hit(&ev, crate::bindings::Context::Dialog) == Some("dialog.close") && open.get_untracked().is_some() {
                    ev.stop_propagation();
                    open.set(None);
                }
            }
        >
            <div
                class="qsheet"
                class:open=move || open.with(Option::is_some)
                inert=move || open.with(Option::is_none)
                class:loud=move || open.with(|o| o.as_ref().is_some_and(|id| rows.with(|r| r.iter().any(|x| &x.entry.ticket == id && x.entry.urgency.loud()))))
                aria-live="polite"
            >
                <div class="qsheet-clip">{sheet}</div>
            </div>
            {move || flash.get().map(|(a, n)| view! {
                <i class=format!("qedge {}", a.urgency.as_str()) data-gen=n.to_string() aria-hidden="true"></i>
                <div class="qarrived" role="status">{a.label()}</div>
            })}
            {card_view}
            <div class="qtabs" role="toolbar" aria-label="queued for you">
                <For each=move || rows.get() key=|r| r.entry.ticket.clone() let:row>
                    {
                        let id = row.entry.ticket.clone();
                        let now_row = {
                            let id = id.clone();
                            Memo::new(move |_| rows.with(|r| r.iter().find(|x| x.entry.ticket == id).cloned()))
                        };
                        let (cid, tid, did) = (id.clone(), id.clone(), id.clone());
                        let (enter_id, focus_id) = (id.clone(), id.clone());
                        let (begin_enter, begin_focus) = (hover_begin.clone(), hover_begin.clone());
                        let (end_leave, end_blur, end_click) = (hover_end.clone(), hover_end.clone(), hover_end.clone());
                        let titled_on = hover_on.clone();
                        view! {
                            <button
                                type="button"
                                class=move || now_row.with(|r| r.as_ref().map(|r| tab_class(&r.entry, Mark {
                                    open: open.with(|o| o.as_deref() == Some(cid.as_str())),
                                    seen: seen.with(|s| s.contains(&cid)),
                                    fresh: fresh.with(|s| s.contains(&cid)),
                                    unseen: unseen.with(|s| s.contains(&cid)),
                                })).unwrap_or_default())
                                data-ticket-id=id.clone()
                                aria-expanded=move || open.with(|o| o.as_deref() == Some(tid.as_str())).to_string()
                                // APP-25: the card replaces the browser's title while it is on.
                                title=move || (!titled_on.get()).then(|| now_row.with(|r| r.as_ref().map(|r| format!("{} — {} ({})", r.entry.ticket, r.entry.reason, r.entry.urgency.as_str())).unwrap_or_default()))
                                aria-describedby=move || hovered.with(|h| h.as_ref().is_some_and(|h| h.0 == did)).then_some("qcard")
                                on:mouseenter=move |ev| begin_enter(enter_id.clone(), ev.into())
                                on:focus=move |ev| begin_focus(focus_id.clone(), ev.into())
                                on:mouseleave=move |_| end_leave()
                                on:blur=move |_| end_blur()
                                on:click=move |_| {
                                    end_click();
                                    toggle(id.clone());
                                }
                            >
                                <span class="id">{row.entry.ticket.clone()}</span>
                                <span class="t">{move || now_row.with(|r| r.as_ref().map(|r| r.ticket.title.clone()).unwrap_or_default())}</span>
                                <span class="u">{move || now_row.with(|r| r.as_ref().map(|r| r.entry.urgency.as_str()).unwrap_or_default())}</span>
                            </button>
                        }
                    }
                </For>
            </div>
        </div>
    }
}

/// APP-26: whether a press lowers the sheet — one is up, no submit is out,
/// and the press landed outside the tray (a tab, the sheet and the card are
/// inside).
fn outside_lowers(open: bool, busy: bool, inside: bool) -> bool {
    open && !busy && !inside
}

/// The named input of `e`, as the app's copy of the inputs holds it.
fn named_input(inputs_held: &ArcRwSignal<Option<inputs::Inputs>>, e: &QueueEntry) -> Option<Input> {
    let i = e.input.as_ref()?;
    inputs_held.with(|h| h.as_ref().and_then(|h| h.list().iter().find(|x| x.ticket == e.ticket && &x.id == i).cloned()))
}

/// APP-23: Request changes — a button, or, once pressed, the box.
fn changes_view(id: String, problem: RwSignal<Option<String>>, busy: RwSignal<bool>, draft: Draft) -> impl IntoView {
    let send = {
        let id = id.clone();
        move || {
            let body = draft.text.get_untracked();
            if !body.trim().is_empty() && !busy.get_untracked() {
                problem.set(None);
                request_changes(id.clone(), body, problem, busy, draft);
            }
        }
    };
    let send_key = send.clone();
    move || {
        if !draft.open.get() {
            return view! {
                <div class="qchange">
                    <button type="button" class="qbtn" data-act="request-changes" on:click=move |_| draft.open.set(true)>"request changes"</button>
                </div>
            }
            .into_any();
        }
        let (send, send_key) = (send.clone(), send_key.clone());
        // A box drawn is focused (one ref per drawing: a ref loads once).
        let area = NodeRef::<leptos::html::Textarea>::new();
        area.on_load(|el: web_sys::HtmlTextAreaElement| {
            let _ = el.focus();
        });
        view! {
            <div class="qchange open">
                <textarea
                    node_ref=area
                    rows="3"
                    aria-label="request changes"
                    placeholder=CHANGES_PLACEHOLDER
                    prop:value=move || draft.text.get()
                    on:input=move |ev| draft.text.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| match crate::bindings::hit(&ev, crate::bindings::Context::Field) {
                        Some("comment.save") => {
                            ev.prevent_default();
                            send_key();
                        }
                        Some("form.cancel") => {
                            // The box closes; the sheet stays.
                            ev.prevent_default();
                            ev.stop_propagation();
                            draft.open.set(false);
                        }
                        _ => {}
                    }
                ></textarea>
                <div class="qchange-acts">
                    <button
                        type="button"
                        class="qbtn fill"
                        disabled=move || busy.get() || draft.text.with(|t| t.trim().is_empty())
                        on:click=move |_| send()
                    >
                        "send — request changes"
                    </button>
                    <button type="button" class="qbtn" on:click=move |_| draft.open.set(false)>"cancel"</button>
                </div>
            </div>
        }
        .into_any()
    }
}

/// The named input in the sheet: its question and its controls while it is
/// open, what was answered once it is, or nothing when there is none.
fn input_view(
    id: String,
    named: Option<String>,
    input: Option<Input>,
    problem: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
    open: RwSignal<Option<String>>,
    pick: RwSignal<Pick>,
) -> Option<AnyView> {
    let named = named?;
    let Some(input) = input else {
        return Some(view! { <p class="qq muted">"asks "<code>{format!("#{named}")}</code>" — on the ticket"</p> }.into_any());
    };
    let question = input.question.clone().unwrap_or_else(|| format!("#{named}"));
    if input.state != InputState::Open {
        let words = match (&input.state, &input.answer) {
            (InputState::Answered, Some(a)) => format!("answered: {} — the agent acts on it and closes it", said(&input, &a.value)),
            (state, _) => format!("the input is {}", state.as_str()),
        };
        return Some(view! { <div class="qinput"><p class="qq"><b>{question}</b></p><p class="qdone">{words}</p></div> }.into_any());
    }
    // APP-25: every control only picks; the one submit sends.
    let controls = match input.kind {
        InputKind::Choice => {
            let opts = input
                .options
                .iter()
                .map(|o| {
                    let (value, chose) = (o.value.clone(), o.value.clone());
                    let (sel, pressed) = (o.value.clone(), o.value.clone());
                    view! {
                        <button
                            type="button"
                            class="qopt"
                            class:rec=o.recommended
                            class:sel=move || pick.with(|p| p.chosen(&sel))
                            aria-pressed=move || pick.with(|p| p.chosen(&pressed)).to_string()
                            data-value=value
                            disabled=move || busy.get()
                            on:click=move |_| pick.update(|p| p.choose(&chose))
                        >
                            <span class="r" aria-hidden="true"></span>
                            {o.label.clone()}
                            {o.recommended.then(|| view! { <span class="rl">"recommended"</span> })}
                        </button>
                    }
                })
                .collect_view();
            view! { <div class="qopts">{opts}</div> }.into_any()
        }
        InputKind::Multi => {
            let boxes = input
                .options
                .iter()
                .map(|o| {
                    let (v, c) = (o.value.clone(), o.value.clone());
                    view! {
                        <label class="qtick">
                            <input
                                type="checkbox"
                                prop:checked=move || pick.with(|p| p.ticked.contains(&c))
                                disabled=move || busy.get()
                                on:change=move |ev| {
                                    let on = event_target_checked(&ev);
                                    pick.update(|p| p.tick(&v, on));
                                }
                            />
                            {o.label.clone()}
                        </label>
                    }
                })
                .collect_view();
            view! { <div class="qopts multi">{boxes}</div> }.into_any()
        }
        InputKind::Text => view! {
            <div class="qopts text">
                <textarea
                    rows="2"
                    aria-label="your reply"
                    prop:value=move || pick.with(|p| p.text.clone())
                    on:input=move |ev| {
                        let t = event_target_value(&ev);
                        pick.update(|p| p.text = t);
                    }
                ></textarea>
            </div>
        }
        .into_any(),
    };
    let kind = input.kind;
    let send = move |_| {
        if busy.get_untracked() {
            return;
        }
        if let Some(value) = pick.with_untracked(|p| p.value(kind)) {
            submit(id.clone(), named.clone(), value, problem, busy, open, pick);
        }
    };
    let controls = view! {
        {controls}
        <div class="qsubmit">
            <button
                type="button"
                class="qbtn fill"
                data-act="submit"
                disabled=move || busy.get() || pick.with(|p| p.value(kind).is_none())
                on:click=send
            >
                {move || if busy.get() { "submitting…" } else { "submit" }}
            </button>
        </div>
    };
    Some(view! { <div class="qinput"><p class="qq"><b>{question}</b></p>{controls}</div> }.into_any())
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::{Kind, QueueAction, Status, TransitionSource, Urgency};

    fn summary(id: &str, title: &str) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').unwrap();
        TicketSummary {
            hidden_blocked_by: 0,
            hidden_unlocks: 0,
            parent_hidden: false,
            id: menger_tickets_api::TicketId::new(id),
            epic: menger_tickets_api::Code::stored(epic), shortcode: menger_tickets_api::ShortcodeId::new(epic), address: String::new(),
            seq: seq.parse().unwrap(),
            title: title.into(),
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
            labels: Vec::new(),
            created_unix: 0,
            updated_unix: 0,
        }
    }

    fn entry(id: &str, u: Urgency, at: i64) -> QueueEntry {
        QueueEntry {
            ticket: id.into(),
            urgency: u,
            reason: "why".into(),
            input: None,
            by: "idp:agent".into(),
            name: None,
            source: TransitionSource::Api,
            queued_unix: at,
            asked_unix: at,
            snoozed_until_unix: None,
        }
    }

    fn queued(id: &str, u: Urgency, at: i64) -> TicketsEvent {
        TicketsEvent::Queued { entry: entry(id, u, at), ticket: summary(id, "t"), action: QueueAction::Queued }
    }

    fn ids(q: &Queue) -> Vec<&str> {
        q.rows().iter().map(|r| r.entry.ticket.as_str()).collect()
    }

    #[test]
    fn events_keep_the_queue_in_its_order() {
        let mut q = Queue::default();
        assert!(q.apply(&queued("A-1", Urgency::Whenever, 1)));
        assert!(q.apply(&queued("A-2", Urgency::Today, 2)));
        assert!(q.apply(&queued("A-3", Urgency::Blocking, 3)));
        assert_eq!(ids(&q), ["A-3", "A-2", "A-1"]);
        assert!(q.apply(&queued("A-1", Urgency::Now, 1)), "raised: it moves up");
        assert_eq!(ids(&q), ["A-3", "A-1", "A-2"]);
        assert!(!q.apply(&queued("A-1", Urgency::Now, 1)), "the same again moves nothing");
        let gone = TicketsEvent::Unqueued { id: "A-3".into(), entry: entry("A-3", Urgency::Blocking, 3), action: QueueAction::Closed, by: "system".into(), at_unix: 9 };
        assert!(q.apply(&gone));
        assert_eq!(ids(&q), ["A-1", "A-2"]);
        assert!(q.apply(&TicketsEvent::Deleted { ids: vec!["A-2".into()], tombstones: Vec::new() }));
        assert_eq!(ids(&q), ["A-1"]);
        // APP-23: handed back — the watch's `unqueued` (changes_requested) or
        // the write's own answer — the tab leaves.
        q.apply(&queued("A-4", Urgency::Now, 4));
        let handed = TicketsEvent::Unqueued { id: "A-4".into(), entry: entry("A-4", Urgency::Now, 4), action: QueueAction::ChangesRequested, by: "owner".into(), at_unix: 9 };
        assert!(Queue::concerns(&handed) && q.apply(&handed));
        assert_eq!(ids(&q), ["A-1"]);
        q.apply(&queued("A-4", Urgency::Now, 4));
        let mut c = crate::comments::tests::comment(7, "#/ticket/A-4", "conversation", Some("A-4"));
        c.intent = menger_tickets_api::CommentIntent::Changes;
        let answered = TicketsEvent::ChangesRequested { id: "A-4".into(), comment: c, entry: entry("A-4", Urgency::Now, 4), by: "owner".into(), at_unix: 9 };
        assert!(Queue::concerns(&answered) && q.apply(&answered));
        assert_eq!(ids(&q), ["A-1"]);
        // Queued again after acting: back, and an arrival (APP-20).
        let before = q.rows().to_vec();
        q.apply(&queued("A-4", Urgency::Now, 10));
        assert_eq!(arrived(&before, q.rows()).iter().map(|a| a.ticket.as_str()).collect::<Vec<_>>(), ["A-4"]);
    }

    #[test]
    fn a_queued_ticket_that_moves_keeps_its_tab_under_its_new_id() {
        let mut q = Queue::default();
        q.apply(&queued("A-1", Urgency::Now, 1));
        let moves = vec![menger_tickets_api::ShortcodeMove { from: "A-1".into(), to: "B-4".into() }];
        assert!(q.apply(&TicketsEvent::Reshortcoded { moves, tickets: Vec::new() }));
        assert_eq!(ids(&q), ["B-4"]);
    }

    #[test]
    fn a_snoozed_tab_leaves_and_comes_back_on_time() {
        let mut q = Queue::default();
        q.apply(&queued("A-1", Urgency::Now, 1));
        q.apply(&queued("A-2", Urgency::Today, 2));
        let mut e = entry("A-1", Urgency::Now, 1);
        e.snoozed_until_unix = Some(700);
        assert!(q.apply(&TicketsEvent::Snoozed { entry: e, ticket: summary("A-1", "t") }));
        let at = |now| q.showing(now).into_iter().map(|r| r.entry.ticket).collect::<Vec<_>>();
        assert_eq!(at(100), ["A-2"]);
        assert_eq!(q.next_wake(100), Some(700));
        assert_eq!(at(700), ["A-1", "A-2"]);
        assert_eq!(q.next_wake(700), None);
    }

    #[test]
    fn a_fetch_replays_what_was_heard_while_it_was_out() {
        let answer = vec![
            TicketsEvent::QueueRow { entry: entry("A-1", Urgency::Today, 1), ticket: summary("A-1", "old") },
            TicketsEvent::QueueEnd { count: 1, snoozed: 0, now_unix: 5 },
        ];
        let s = summary("A-1", "new");
        let renamed = menger_tickets_api::Ticket {
            hidden_blocked_by: 0,
            hidden_unlocks: 0,
            parent_hidden: false,
            labels: s.labels,
            id: s.id,
            epic: s.epic,
            shortcode: s.shortcode,
            address: s.address,
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
            moved_from: Vec::new(),
        };
        let heard = vec![queued("A-2", Urgency::Now, 3), TicketsEvent::Amended { ticket: renamed, changed: vec![menger_tickets_api::vocab::AmendedField::Title] }];
        let q = settle(answer, &heard);
        assert_eq!(ids(&q), ["A-2", "A-1"]);
        assert_eq!(q.get("A-1").unwrap().ticket.title, "new", "a row event moves the title");
    }

    fn landed(rows: Vec<TicketsEvent>) -> Option<Queue> {
        let mut held = None;
        let answer = rows
            .into_iter()
            .filter_map(|ev| match ev {
                TicketsEvent::Queued { entry, ticket, .. } => Some(TicketsEvent::QueueRow { entry, ticket }),
                _ => None,
            })
            .collect();
        let (_, a) = step(&mut held, Cause::Fetched(answer, &[]), 10);
        assert!(a.is_empty());
        held
    }

    fn arrivals(a: &[Arrival]) -> Vec<(&str, Urgency)> {
        a.iter().map(|a| (a.ticket.as_str(), a.urgency)).collect()
    }

    #[test]
    fn a_new_entry_heard_live_arrives() {
        let mut held = landed(vec![queued("A-1", Urgency::Whenever, 1)]);
        let (moved, a) = step(&mut held, Cause::Heard(&queued("A-2", Urgency::Today, 20)), 20);
        assert!(moved);
        assert_eq!(arrivals(&a), [("A-2", Urgency::Today)]);
        assert_eq!(a[0].label(), "queued · today · idp:agent");
    }

    #[test]
    fn a_raise_arrives_again_and_a_requeue_does_not() {
        let mut held = landed(vec![queued("A-1", Urgency::Today, 1)]);
        let (_, a) = step(&mut held, Cause::Heard(&queued("A-1", Urgency::Now, 1)), 20);
        assert_eq!(arrivals(&a), [("A-1", Urgency::Now)], "raised: it flashes at the new urgency");
        let mut same = entry("A-1", Urgency::Now, 1);
        same.reason = "a new reason".into();
        let ev = TicketsEvent::Queued { entry: same, ticket: summary("A-1", "t"), action: QueueAction::Requeued };
        let (moved, a) = step(&mut held, Cause::Heard(&ev), 21);
        assert!(moved, "the reason moved");
        assert!(a.is_empty(), "the same urgency is no arrival");
        let (_, a) = step(&mut held, Cause::Heard(&queued("A-1", Urgency::Whenever, 1)), 22);
        assert!(a.is_empty(), "nor a lower one");
    }

    #[test]
    fn a_snooze_ending_arrives() {
        let mut held = landed(vec![queued("A-1", Urgency::Now, 1)]);
        let mut e = entry("A-1", Urgency::Now, 1);
        e.snoozed_until_unix = Some(700);
        let (moved, a) = step(&mut held, Cause::Heard(&TicketsEvent::Snoozed { entry: e, ticket: summary("A-1", "t") }), 100);
        assert!(moved && a.is_empty(), "snoozing takes it out: nothing arrives");
        assert!(step(&mut held, Cause::Tick(100, 650), 650).1.is_empty(), "still snoozed");
        let (_, a) = step(&mut held, Cause::Tick(650, 700), 700);
        assert_eq!(arrivals(&a), [("A-1", Urgency::Now)]);
        assert!(step(&mut held, Cause::Tick(700, 730), 730).1.is_empty(), "once");
    }

    #[test]
    fn load_and_refetch_do_not_arrive() {
        let mut held = None;
        assert_eq!(step(&mut held, Cause::Heard(&queued("A-9", Urgency::Now, 1)), 5), (false, Vec::new()), "before the first fetch: nothing");
        let first = vec![TicketsEvent::QueueRow { entry: entry("A-1", Urgency::Now, 1), ticket: summary("A-1", "t") }];
        let (moved, a) = step(&mut held, Cause::Fetched(first, &[]), 10);
        assert!(moved && a.is_empty(), "the first load");
        let refetch = vec![
            TicketsEvent::QueueRow { entry: entry("A-1", Urgency::Blocking, 1), ticket: summary("A-1", "t") },
            TicketsEvent::QueueRow { entry: entry("A-2", Urgency::Today, 2), ticket: summary("A-2", "t") },
        ];
        let heard = [queued("A-3", Urgency::Now, 3)];
        let (_, a) = step(&mut held, Cause::Fetched(refetch, &heard), 20);
        assert!(a.is_empty(), "a reconnect's refetch, new rows, a raise and replayed events included");
        assert_eq!(ids(held.as_ref().unwrap()), ["A-1", "A-3", "A-2"]);
        assert!(step(&mut held, Cause::Tick(20, 50), 50).1.is_empty(), "the clock alone moves nothing");
    }

    /// APP-25: choosing picks, nothing more; submit is enabled only once
    /// there is something to send, and sends what was picked.
    #[test]
    fn a_choice_is_picked_then_submitted() {
        let mut p = Pick::default();
        assert_eq!(p.value(InputKind::Choice), None, "nothing chosen: submit disabled");
        p.choose("archive");
        assert!(p.chosen("archive") && !p.chosen("delete"));
        p.choose("delete");
        assert!(p.chosen("delete") && !p.chosen("archive"), "one option at a time");
        assert_eq!(p.value(InputKind::Choice), Some(InputValue::Choice { option: "delete".into() }));
        // A multi's ticks and a text reply go through the same submit.
        assert_eq!(p.value(InputKind::Multi), None, "no box ticked");
        p.tick("unit", true);
        p.tick("wire", true);
        p.tick("unit", false);
        assert_eq!(p.value(InputKind::Multi), Some(InputValue::Multi { options: vec!["wire".into()] }));
        assert_eq!(p.value(InputKind::Text), None, "no reply");
        p.text = "   ".into();
        assert_eq!(p.value(InputKind::Text), None, "only spaces");
        p.text = "ship it".into();
        assert_eq!(p.value(InputKind::Text), Some(InputValue::Text { text: "ship it".into() }));
        assert_eq!(Pick::default().value(InputKind::Choice), None, "a fresh sheet holds nothing");
    }

    /// APP-25: the card says the named input's question, else the reason,
    /// and the urgency and who queued it; the setting is on by default.
    #[test]
    fn the_hover_card() {
        let mut e = entry("A-1", Urgency::Blocking, 1);
        e.reason = "the landing waits on you".into();
        e.name = Some("Ada".into());
        let row = Row { entry: e.clone(), ticket: summary("A-1", "t") };
        let none = card(&row, None);
        assert_eq!(none, Card { question: "the landing waits on you".into(), line: "blocking · queued by Ada".into() });
        let mut asked = Row { entry: e, ticket: summary("A-1", "t") };
        asked.entry.input = Some("landing".into());
        let mut input = crate::inputs::tests::input("A-1", "landing");
        input.question = Some("Expired events: archive or delete them, keeping the holds?".into());
        assert_eq!(card(&asked, Some(&input)).question, "Expired events: archive or delete them, keeping the holds?");
        assert_eq!(card(&asked, None).question, "the landing waits on you", "the input not held yet: the reason");
        input.question = None;
        assert_eq!(card(&asked, Some(&input)).question, "the landing waits on you", "no question: the reason");
        let other = crate::inputs::tests::input("A-1", "other");
        assert_eq!(card(&asked, Some(&other)).question, "the landing waits on you", "another input is not the named one");
        assert!(crate::settings::Settings::default().queue_hover, "on by default");
    }

    #[test]
    fn the_words() {
        assert_eq!(titled("menger", 0), "menger");
        assert_eq!(titled("menger", 2), "(2) menger");
        assert_eq!(ago(100, 100), "just now");
        assert_eq!(ago(400, 100), "5m ago");
        assert_eq!(ago(7_300, 100), "2h ago");
        assert_eq!(ago(300_000, 100), "3d ago");
        let mut e = entry("A-1", Urgency::Blocking, 0);
        assert_eq!(tab_class(&e, Mark::default()), "qtab blocking loud");
        assert_eq!(tab_class(&e, Mark { open: true, seen: true, ..Mark::default() }), "qtab blocking loud open seen");
        assert_eq!(tab_class(&e, Mark { fresh: true, unseen: true, ..Mark::default() }), "qtab blocking loud fresh unseen");
        assert_eq!(tab_class(&e, Mark { seen: true, fresh: true, unseen: true, ..Mark::default() }), "qtab blocking loud seen fresh", "opened: no dot");
        e.urgency = Urgency::Today;
        assert_eq!(tab_class(&e, Mark::default()), "qtab today");
        assert_eq!(who(&e), "idp:agent");
        e.name = Some("Ada".into());
        assert_eq!(who(&e), "Ada");
        assert_eq!(open_href(&e), "#/ticket/A-1");
        e.input = Some("landing".into());
        assert_eq!(open_href(&e), "#/ticket/A-1?anchor=landing");
    }

    /// APP-26: a press outside the tray lowers an open sheet; one inside it
    /// (the tab that opens it, the sheet, the card) or during a submit does
    /// not, and nothing is lowered when nothing is up.
    #[test]
    fn a_press_outside_lowers_the_sheet() {
        assert!(outside_lowers(true, false, false), "outside, open, idle");
        assert!(!outside_lowers(true, false, true), "inside the tray");
        assert!(!outside_lowers(true, true, false), "a submit is out");
        assert!(!outside_lowers(false, false, false), "nothing is up");
    }
}
