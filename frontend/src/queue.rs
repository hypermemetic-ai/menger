//! MENGER-39: **the tray** — the owner's queue on the bottom edge of the
//! app's main column: one control, and the sheet it slides in (APP-38).
//!
//! - **What it holds** is the app's copy of `tickets.queued` — fetched on
//!   every connection and after every `lagged` ([`refresh`]), kept in
//!   between by `watch`: `queued` and `snoozed` carry the entry and its row
//!   (an upsert), `unqueued` takes it out, a row event of a queued ticket
//!   moves its title ([`Queue::apply`]). As `inputs`' copy is, the events
//!   heard while a fetch is out are replayed onto its answer.
//! - **One control** (APP-38, APP-36's direction A, the one chosen) on the
//!   main column's bottom edge, at the right. One ticket: its tab, as the
//!   tray always drew it — its id, its title, an urgency chip. More: the
//!   count, `queued`, the loudest urgency's chip with how many share it
//!   (`3 blocking`) and `▴`. Its ground and border are the loudest
//!   urgency's ([`Control`]); `now` / `blocking` are taller, red, and nudge
//!   while one of them is unopened. At a phone's width it is a 46 px pill
//!   at the bottom right.
//! - **The sheet**: the control slides it in from the bottom of the main
//!   column (a phone's: the whole width, 88% tall, over a scrim, with a grab
//!   handle). Its head says `queue N`, a chip per urgency (each jumps to its
//!   group), `j` / `k`, close (Esc). Its list is the queue most urgent first,
//!   then oldest, grouped `blocking · now · today · whenever` under sticky
//!   heads, `whenever` folded past six ([`groups`]); a row picked is
//!   answered beside the list (a phone's: under its row) — who and when,
//!   the reason, and the named input's question, answerable in place
//!   (recorded with source `app`, as every answer the app gives; answering
//!   is the owner done with it, so the ticket leaves the queue — the input
//!   itself stays the agent's to close), then *open ↗*, *snooze 10m* and
//!   *done with it*. Submit, snooze or done picks the next row
//!   ([`next_pick`]); the last one leaving takes the sheet down, and the
//!   control with it. With one ticket the sheet holds its answer alone.
//!   Esc, close, the scrim, the control again, or a press anywhere outside
//!   the tray — a frame's included (APP-26, [`outside_lowers`]) — lowers it.
//! - **Chosen, then submitted** (APP-25): an option clicked is selected
//!   ([`Pick`]), nothing sent; ticks and a reply are held the same way, and
//!   one **submit** sends whichever it is. The sheet comes down once that
//!   write (the answer, then the unqueue) succeeds; a refusal keeps it up
//!   with the problem shown.
//! - **The question on hover** (APP-25): hovering or focusing the one
//!   ticket's tab shows a card above it, after a short delay — the named
//!   input's question, else the reason, its urgency and who queued it
//!   ([`card`]) — gone on leave or blur and never while the sheet is up.
//!   Settings → Queue turns it off ([`hover_setting`]); on by default.
//! - **Request changes** (APP-23): beside the input's answer (alone when
//!   none is named), a box for a short note — Ctrl+Enter (AGS-28's
//!   `comment.save`) sends it as `tickets.request_changes`: the note goes on
//!   the ticket's conversation as a change request and the ticket leaves the
//!   queue, handed back to whoever queued it; the input stays open. Esc
//!   closes the box. The draft is the tray's, so the clock's tick or an
//!   answer elsewhere does not lose it.
//! - **A hidden tab's title** carries the count: `(2) menger`.
//! - **Motion** is DESIGN-39's setting (LOOK-8): the sheet in in .32 s
//!   eased out, down in .2 s; `calm` keeps the slide and the blinks and
//!   drops the nudge and the sweep, `none` moves nothing (index.html's
//!   rules).
//! - **An arrival** (MENGER-45, MENGER-40's variant C′) — a new entry heard
//!   on `watch`, one raised to a higher urgency, one whose snooze ended —
//!   while the sheet is down blinks the control in its urgency's colour,
//!   ticks the count, sweeps the edge, says "queued · <urgency> · <who>" for
//!   ~3s, and leaves `N new` (a dot) until the sheet is opened; its row
//!   keeps a dot until it is picked. What a fetch brings (the first load, a
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
                self.rows.retain(|r| !ids.iter().any(|id| *id == r.entry.ticket));
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

/// APP-38: the urgencies, loudest first — the sheet's groups, in order, and
/// its head's chips.
pub(crate) const LOUDEST_FIRST: [Urgency; 4] = [Urgency::Blocking, Urgency::Now, Urgency::Today, Urgency::Whenever];

/// APP-38: past this many, `whenever` folds under one line until asked for.
pub(crate) const FOLD_OVER: usize = 6;

/// APP-38: what the one control on the edge says — one ticket's tab, or the
/// count and the loudest urgency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Control {
    /// How many are queued (and not snoozed).
    pub(crate) count: usize,
    /// The loudest urgency among them: the control's colours.
    pub(crate) loudest: Urgency,
    /// How many share it — `3 blocking`.
    pub(crate) sharing: usize,
    /// What arrived since the sheet was last opened — `1 new`, a dot.
    pub(crate) new: usize,
    /// A now or blocking ticket not yet opened: the control nudges.
    pub(crate) nudge: bool,
}

/// The control for `rows` (what shows), `seen` the tickets opened since they
/// arrived, `new` the arrivals since the sheet was last opened; none for an
/// empty queue.
pub(crate) fn control(rows: &[Row], seen: &HashSet<String>, new: &HashSet<String>) -> Option<Control> {
    let loudest = rows.iter().map(|r| r.entry.urgency).max()?;
    Some(Control {
        count: rows.len(),
        loudest,
        sharing: rows.iter().filter(|r| r.entry.urgency == loudest).count(),
        new: rows.iter().filter(|r| new.contains(&r.entry.ticket)).count(),
        nudge: rows.iter().any(|r| r.entry.urgency.loud() && !seen.contains(&r.entry.ticket)),
    })
}

impl Control {
    /// One ticket: the control is its tab.
    pub(crate) fn one(&self) -> bool {
        self.count == 1
    }

    /// The loudest urgency's chip: `3 blocking` (one ticket: `blocking`).
    pub(crate) fn chip(&self) -> String {
        if self.one() {
            self.loudest.as_str().to_string()
        } else {
            format!("{} {}", self.sharing, self.loudest.as_str())
        }
    }

    /// The control's classes: its loudest urgency (`u-…`, the colours),
    /// `one` (and `qtab`, as the tab it is) or `many`, `loud` for now and
    /// blocking, `nudge` while one of those is unopened, `open` while the
    /// sheet is up, `fresh f-<urgency>` while it plays an arrival, `has-new`
    /// while arrivals wait for the sheet.
    pub(crate) fn class(&self, open: bool, fresh: Option<Urgency>) -> String {
        let mut c = format!("qc u-{}", self.loudest.as_str());
        c.push_str(if self.one() { " one qtab" } else { " many" });
        for (on, word) in [(self.loudest.loud(), "loud"), (self.nudge, "nudge"), (open, "open"), (self.new > 0, "has-new")] {
            if on {
                c.push(' ');
                c.push_str(word);
            }
        }
        if let Some(u) = fresh {
            c.push_str(" fresh f-");
            c.push_str(u.as_str());
        }
        c
    }

    /// What a screen reader hears: `9 queued, 1 blocking, 1 new`.
    pub(crate) fn words(&self) -> String {
        let mut w = format!("{} queued, {} {}", self.count, self.sharing, self.loudest.as_str());
        if self.new > 0 {
            w.push_str(&format!(", {} new", self.new));
        }
        w
    }
}

/// APP-38: one urgency's group in the sheet — its rows, or, folded, how
/// many it holds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Group {
    pub(crate) urgency: Urgency,
    pub(crate) rows: Vec<Row>,
    /// Folded: the rows it holds, none drawn (`18 whenever · show`).
    pub(crate) folded: usize,
}

impl Group {
    /// How many it holds, drawn or folded.
    pub(crate) fn len(&self) -> usize {
        self.rows.len() + self.folded
    }
}

/// The sheet's groups for `rows` (in the queue's order): `blocking · now ·
/// today · whenever`, the empty ones left out; `whenever` folded past
/// [`FOLD_OVER`] unless `unfold` or the ticket `picked` is in it.
pub(crate) fn groups(rows: &[Row], unfold: bool, picked: Option<&str>) -> Vec<Group> {
    LOUDEST_FIRST
        .iter()
        .filter_map(|&urgency| {
            let rows: Vec<Row> = rows.iter().filter(|r| r.entry.urgency == urgency).cloned().collect();
            if rows.is_empty() {
                return None;
            }
            let holds_pick = picked.is_some_and(|p| rows.iter().any(|r| r.entry.ticket == p));
            let fold = urgency == Urgency::Whenever && rows.len() > FOLD_OVER && !unfold && !holds_pick;
            Some(if fold { Group { urgency, folded: rows.len(), rows: Vec::new() } } else { Group { urgency, rows, folded: 0 } })
        })
        .collect()
}

/// APP-38: after the queue moved from `before` to `after` (ids, in order),
/// the row to have picked: `picked` while it is still there; when it left
/// (submitted, snoozed, done with, or answered elsewhere), the next row that
/// stayed, else the one before it, else the first; none when nothing is
/// picked or nothing is left.
pub(crate) fn next_pick(before: &[String], after: &[String], picked: Option<&str>) -> Option<String> {
    let p = picked?;
    if after.iter().any(|a| a == p) {
        return Some(p.to_string());
    }
    let stays = |b: &&String| after.contains(b);
    let at = before.iter().position(|b| b == p);
    at.and_then(|i| before[i + 1..].iter().find(stays).or_else(|| before[..i].iter().rev().find(stays)))
        .or_else(|| after.first())
        .cloned()
}

/// APP-38: `j` / `k` — the row `by` places from `picked` in `order`, held at
/// either end; the first when nothing is picked.
pub(crate) fn step_pick(order: &[String], picked: Option<&str>, by: i32) -> Option<String> {
    let at = picked.and_then(|p| order.iter().position(|o| o == p));
    let to = match at {
        Some(i) => (i as i64 + i64::from(by)).clamp(0, order.len() as i64 - 1) as usize,
        None => 0,
    };
    order.get(to).cloned()
}

/// APP-38: where `picked` stands in `order` — `(k, n)`, one-based, for
/// `k of N · submit moves to the next`.
pub(crate) fn position(order: &[String], picked: Option<&str>) -> Option<(usize, usize)> {
    let p = picked?;
    order.iter().position(|o| o == p).map(|i| (i + 1, order.len()))
}

/// APP-38: a row's classes — its urgency (`u-…`, the left rule), `sel` while
/// picked, `unseen` (the dot) from its arrival until it is picked.
pub(crate) fn row_class(e: &QueueEntry, sel: bool, unseen: bool) -> String {
    let mut c = format!("qr u-{}", e.urgency.as_str());
    for (on, word) in [(sel, "sel"), (unseen && !sel, "unseen")] {
        if on {
            c.push(' ');
            c.push_str(word);
        }
    }
    c
}

/// APP-38: a row's age, short: `now`, `5m`, `2h`, `3d`.
pub(crate) fn age(now: i64, then: i64) -> String {
    let s = (now - then).max(0);
    match s {
        0..=59 => "now".into(),
        60..=3_599 => format!("{}m", s / 60),
        3_600..=86_399 => format!("{}h", s / 3_600),
        _ => format!("{}d", s / 86_400),
    }
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
/// the queue. Once both writes succeed the pick goes and the row leaves, and
/// the tray picks the next (APP-38; the last one takes the sheet down); a
/// refusal of either keeps the row with the problem shown.
fn submit(id: String, input: String, value: InputValue, problem: RwSignal<Option<String>>, busy: RwSignal<bool>, pick: RwSignal<Pick>) {
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

/// APP-38: the phone's width — the control a pill, the sheet the whole
/// width over a scrim, a row's answer opened under it.
const PHONE: &str = "(max-width: 640px)";

/// Whether the window is a phone's width, followed.
fn phone_width() -> RwSignal<bool> {
    let s = RwSignal::new(false);
    let Some(m) = dom::window().match_media(PHONE).ok().flatten() else { return s };
    s.set(m.matches());
    let held = m.clone();
    let on = Closure::<dyn Fn()>::new(move || {
        s.try_set(held.matches());
    });
    let _ = m.add_event_listener_with_callback("change", on.as_ref().unchecked_ref());
    on.forget();
    s
}

/// How long the sheet takes to slide down (index.html's `.2s`), so the
/// tray outlives its last row by as much.
const SLIDE_OUT_MS: i32 = 220;

/// What the answer in the sheet reads and writes — the tray's.
#[derive(Clone, Copy)]
struct Sheet {
    open: RwSignal<bool>,
    picked: RwSignal<Option<String>>,
    order: Memo<Vec<String>>,
    clock: RwSignal<i64>,
    problem: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
    draft: Draft,
    pick: RwSignal<Pick>,
}

/// One ticket's answer — today's sheet: who and when, the reason, the
/// named input (pick, then submit), Request changes, open, snooze, done;
/// with many, where it stands (`2 of 9 · submit moves to the next`).
fn answer_view(row: Row, s: Sheet, many: bool) -> impl IntoView {
    let id = row.entry.ticket.clone();
    let e = row.entry.clone();
    let input = named_input(&inputs::signal(), &e);
    // The time moves on its own: the answer is not drawn again for it.
    let hd = {
        let e = e.clone();
        let clock = s.clock;
        move || format!("{} · queued by {} · {}", e.urgency.as_str(), who(&e), ago(clock.get(), e.asked_unix))
    };
    let (snooze_id, done_id) = (id.clone(), id.clone());
    let (problem, open) = (s.problem, s.open);
    let place = move || {
        if !many {
            return None;
        }
        let (k, n) = position(&s.order.get(), s.picked.get().as_deref())?;
        Some(view! { <span class="qnext">{format!("{k} of {n} · submit moves to the next")}</span> })
    };
    view! {
        <div class=format!("qd u-{}", e.urgency.as_str()) data-ticket-id=id.clone()>
            <div class="qhd">{hd}</div>
            <h4><a class="code" href=format!("#/ticket/{id}")>{id.clone()}</a>" · "{row.ticket.title.clone()}</h4>
            <p class="qwhy">{e.reason.clone()}</p>
            {input_view(id.clone(), e.input.clone(), input, s.problem, s.busy, s.pick)}
            {changes_view(id.clone(), s.problem, s.busy, s.draft)}
            <div class="qacts">
                <a class="qbtn" href=open_href(&e) on:click=move |_| open.set(false)>"open ↗"</a>
                <button type="button" class="qbtn" on:click=move |_| snooze(snooze_id.clone(), problem)>{format!("snooze {SNOOZE_MINUTES}m")}</button>
                <button type="button" class="qbtn" on:click=move |_| done(done_id.clone(), problem)>"done with it"</button>
                {place}
            </div>
            {move || problem.get().map(|p| view! { <p class="qproblem" role="alert">{p}</p> })}
        </div>
    }
}

/// `#qtray`'s element for `selector`, brought into view in its list
/// (`nearest`: a row on screen never jumps), at once.
fn reveal(selector: &str, block: web_sys::ScrollLogicalPosition) {
    let Some(el) = dom::window().document().and_then(|d| d.query_selector(selector).ok().flatten()) else { return };
    let opts = web_sys::ScrollIntoViewOptions::new();
    opts.set_block(block);
    el.scroll_into_view_with_scroll_into_view_options(&opts);
}

/// The tray (APP-38, APP-36's direction A): one control on the main
/// column's bottom edge — one ticket's tab, or the count and the loudest
/// urgency — and the sheet it slides in from the bottom: the queue grouped
/// by urgency, answered row by row.
#[component]
pub(crate) fn Tray() -> impl IntoView {
    let held = signal();
    let clock = clock();
    inputs::want();
    let card_inputs = inputs::signal();
    let open = RwSignal::new(false);
    let picked = RwSignal::new(None::<String>);
    // Opened since it arrived (no nudge); arrived and not picked yet (the
    // row's dot); arrived since the sheet was last opened (`N new`).
    let seen = RwSignal::new(HashSet::<String>::new());
    let unseen = RwSignal::new(HashSet::<String>::new());
    let new = RwSignal::new(HashSet::<String>::new());
    // MENGER-45 on the one control: the arrival it blinks for, the rows
    // arriving in the open sheet, and the edge's sweep and the label.
    let fresh = RwSignal::new(None::<Urgency>);
    let fresh_rows = RwSignal::new(HashSet::<String>::new());
    let flash = RwSignal::new(None::<(Arrival, u32)>);
    let problem = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let draft = Draft::new();
    let pick = RwSignal::new(Pick::default());
    let unfold = RwSignal::new(false);
    let closing = RwSignal::new(false);
    let phone = phone_width();
    // APP-25: the one ticket's tab hovered (its card's place, once the delay
    // is out) and the hover's generation.
    let hovered = RwSignal::new(None::<(String, f64, f64)>);
    let hover_gen = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let hover_on = hover_setting();
    let tray_ref = NodeRef::<leptos::html::Div>::new();
    let sheet_ref = NodeRef::<leptos::html::Div>::new();
    let control_ref = NodeRef::<leptos::html::Button>::new();
    // Arrivals play once the page is visible: each loses `seen` (a raised
    // one nudges again) and takes the dot; while the sheet is down they
    // count as new and the control blinks for the most urgent, which sweeps
    // the edge and is the label; while it is up their rows slide in.
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
            let up = open.get_untracked();
            seen.update(|s| s.retain(|id| !ids.contains(id)));
            unseen.update(|s| s.extend(ids.iter().cloned()));
            if !up {
                new.update(|s| s.extend(ids.iter().cloned()));
            }
            fresh_rows.update(|s| s.retain(|id| !ids.contains(id)));
            let Some(top) = got.into_iter().max_by_key(|a| a.urgency) else { return };
            fresh.set(None);
            let n = generation.get().wrapping_add(1);
            generation.set(n);
            let generation = generation.clone();
            spawn_local(async move {
                dom::sleep_ms(40).await;
                fresh_rows.try_update(|s| s.extend(ids));
                if !up {
                    fresh.try_set(Some(top.urgency));
                    flash.try_set(Some((top, n)));
                }
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
    let order = Memo::new(move |_| rows.with(|r| r.iter().map(|x| x.entry.ticket.clone()).collect::<Vec<_>>()));
    let one = Memo::new(move |_| rows.with(|r| r.len() == 1));
    let ctl = Memo::new(move |_| rows.with(|r| seen.with(|s| new.with(|n| control(r, s, n)))));
    let picked_row = Memo::new(move |_| picked.with(|p| p.as_ref().and_then(|id| rows.with(|r| r.iter().find(|x| &x.entry.ticket == id).cloned()))));
    let sheet = Sheet { open, picked, order, clock, problem, busy, draft, pick };
    // A row picked: its answer starts clean, it is opened (no nudge, no dot).
    let pick_row = move |id: String| {
        if picked.with_untracked(|p| p.as_deref() != Some(id.as_str())) {
            problem.set(None);
            draft.clear();
            pick.set(Pick::default());
            picked.set(Some(id.clone()));
        }
        if seen.with_untracked(|s| !s.contains(&id)) {
            seen.update(|s| {
                s.insert(id.clone());
            });
        }
        if unseen.with_untracked(|s| s.contains(&id)) {
            unseen.update(|s| {
                s.remove(&id);
            });
        }
    };
    let focus_sheet = move || {
        spawn_local(async move {
            dom::sleep_ms(0).await;
            if let Some(el) = sheet_ref.get_untracked() {
                let opts = web_sys::FocusOptions::new();
                opts.set_prevent_scroll(true);
                let _ = el.focus_with_options(&opts);
            }
        });
    };
    // The sheet up: the row picked before (if still queued), else the
    // first — the most urgent, the oldest; the arrivals are no longer new.
    let open_sheet = move || {
        let keep = picked.get_untracked().filter(|p| order.with_untracked(|o| o.contains(p)));
        if let Some(id) = keep.or_else(|| order.with_untracked(|o| o.first().cloned())) {
            pick_row(id);
        }
        new.set(HashSet::new());
        fresh.set(None);
        flash.set(None);
        open.set(true);
        focus_sheet();
    };
    let close_sheet = move |refocus: bool| {
        open.set(false);
        if refocus {
            if let Some(b) = control_ref.get_untracked() {
                let opts = web_sys::FocusOptions::new();
                opts.set_prevent_scroll(true);
                let _ = b.focus_with_options(&opts);
            }
        }
    };
    // The queue moved: a picked row that left (submitted, snoozed, done
    // with, answered elsewhere) hands the pick to the next; the last one
    // leaving takes the sheet down, and the tray stays as long as it slides.
    {
        let before = StoredValue::new(Vec::<String>::new());
        Effect::new(move |_| {
            let after = order.get();
            let prev = before.get_value();
            before.set_value(after.clone());
            if after.is_empty() {
                if open.get_untracked() {
                    open.set(false);
                    closing.set(true);
                    spawn_local(async move {
                        dom::sleep_ms(SLIDE_OUT_MS).await;
                        closing.try_set(false);
                    });
                }
                return;
            }
            let p = picked.get_untracked();
            if !open.get_untracked() {
                if p.as_ref().is_some_and(|p| !after.contains(p)) {
                    picked.set(None);
                }
                return;
            }
            if let Some(next) = next_pick(&prev, &after, p.as_deref()) {
                if p.as_deref() != Some(next.as_str()) {
                    pick_row(next);
                }
            }
        });
    }
    // What the sheet draws while it slides down after its last row left.
    let last = RwSignal::new(None::<Row>);
    Effect::new(move |_| {
        if let Some(r) = picked_row.get() {
            last.set(Some(r));
        }
    });
    // APP-26: a press anywhere outside the tray lowers the sheet; not while
    // a submit is out. The control, the sheet, its scrim and the card are
    // inside. A press in a frame never reaches this window: focus going
    // into the frame blurs it instead, and that is heard as the press.
    let lower_outside = move |target: Option<web_sys::Element>| {
        let inside = target.is_some_and(|el| el.closest("#qtray").ok().flatten().is_some());
        let (Some(is_open), Some(is_busy)) = (open.try_get_untracked(), busy.try_get_untracked()) else { return };
        if outside_lowers(is_open, is_busy, inside) {
            open.set(false);
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
    on_cleanup(move || {
        press.remove();
        into_frame.remove();
    });
    // j / k: the next or previous row, picked and brought into view.
    let step_by = move |by: i32| {
        let Some(id) = order.with_untracked(|o| step_pick(o, picked.get_untracked().as_deref(), by)) else { return };
        pick_row(id.clone());
        spawn_local(async move {
            dom::sleep_ms(0).await;
            reveal(&format!("#qtray .qr[data-ticket-id=\"{id}\"]"), web_sys::ScrollLogicalPosition::Nearest);
        });
    };
    // A head chip: its group's first row picked, the group brought to the
    // top of the list (whenever unfolded).
    let jump = move |u: Urgency| {
        if u == Urgency::Whenever {
            unfold.set(true);
        }
        let first = rows.with_untracked(|r| r.iter().find(|x| x.entry.urgency == u).map(|x| x.entry.ticket.clone()));
        if let Some(id) = first {
            pick_row(id);
        }
        spawn_local(async move {
            dom::sleep_ms(0).await;
            reveal(&format!("#qtray .grp[data-urgency=\"{}\"]", u.as_str()), web_sys::ScrollLogicalPosition::Start);
        });
    };
    // APP-25: the one ticket's tab hovered or focused shows its card after
    // the delay, above the control; leaving, blurring or opening ends it.
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
        move |ev: web_sys::Event| {
            let n = gen.fetch_add(1, Relaxed).wrapping_add(1);
            if !on.get_untracked() || !one.get_untracked() || open.get_untracked() {
                return;
            }
            let Some(id) = order.with_untracked(|o| o.first().cloned()) else { return };
            let (Some(tab), Some(tray)) = (ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()), tray_ref.get_untracked()) else { return };
            let (t, r) = (tab.get_bounding_client_rect(), tray.get_bounding_client_rect());
            let place = (id, t.right() - r.left(), r.bottom() - t.top() + 6.0);
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
            if !on.get() || open.get() || !one.get() {
                return None;
            }
            let (id, right, bottom) = hovered.get()?;
            let row = rows.with(|r| r.iter().find(|x| x.entry.ticket == id).cloned())?;
            let c = card(&row, named_input(&inputs_held, &row.entry).as_ref());
            Some(view! {
                <div
                    id="qcard"
                    class=format!("qcard u-{}", row.entry.urgency.as_str())
                    role="tooltip"
                    data-ticket-id=id.clone()
                    style=format!("--x: {right:.0}px; bottom: {bottom:.0}px")
                >
                    <div class="qhd">{format!("{id} · {}", c.line)}</div>
                    <p class="qcq">{c.question}</p>
                </div>
            })
        }
    };
    // The control's face: one ticket's id, title and urgency; or the count,
    // `queued`, the loudest's chip, what is new, and the caret.
    let face = move || {
        if one.get() {
            let title = move || rows.with(|r| r.first().map(|r| r.ticket.title.clone()).unwrap_or_default());
            let tid = move || order.with(|o| o.first().cloned().unwrap_or_default());
            view! {
                <span class="id">{tid}</span>
                <span class="t">{title}</span>
                <span class="chip-u">{move || ctl.with(|c| c.as_ref().map(Control::chip).unwrap_or_default())}</span>
                <span class="car" aria-hidden="true">"▴"</span>
            }
            .into_any()
        } else {
            view! {
                <span class="cnt">{move || ctl.with(|c| c.as_ref().map_or(0, |c| c.count))}</span>
                <span class="lbl">"queued"</span>
                <span class="chip-u">{move || ctl.with(|c| c.as_ref().map(Control::chip).unwrap_or_default())}</span>
                {move || ctl.with(|c| c.as_ref().filter(|c| c.new > 0).map(|c| c.new)).map(|n| view! { <span class="new">{format!("{n} new")}</span> })}
                <span class="car" aria-hidden="true">"▴"</span>
            }
            .into_any()
        }
    };
    // The sheet's head: `queue N`, a chip per urgency held (each jumps to
    // its group), j / k, close.
    let chips = move || {
        rows.with(|r| {
            LOUDEST_FIRST
                .iter()
                .filter_map(|&u| {
                    let n = r.iter().filter(|x| x.entry.urgency == u).count();
                    (n > 0).then(|| {
                        view! {
                            <button type="button" class=format!("qj u-{}", u.as_str()) on:click=move |_| jump(u)>{format!("{n} {}", u.as_str())}</button>
                        }
                    })
                })
                .collect_view()
        })
    };
    // One row of the list; on a phone its answer opens under it.
    let row_view = move |row: Row| {
        let id = row.entry.ticket.clone();
        let now_row = {
            let id = id.clone();
            Memo::new(move |_| rows.with(|r| r.iter().find(|x| x.entry.ticket == id).cloned()))
        };
        let (cid, pid, hid) = (id.clone(), id.clone(), id.clone());
        let class = move || {
            let mut c = now_row.with(|r| {
                r.as_ref()
                    .map(|r| row_class(&r.entry, picked.with(|p| p.as_deref() == Some(cid.as_str())), unseen.with(|s| s.contains(&cid))))
                    .unwrap_or_default()
            });
            if fresh_rows.with(|s| s.contains(&cid)) {
                c.push_str(" fresh");
            }
            c
        };
        let here = move || phone.get() && picked.with(|p| p.as_deref() == Some(hid.as_str()));
        view! {
            <button
                type="button"
                class=class
                data-ticket-id=id.clone()
                aria-pressed=move || picked.with(|p| p.as_deref() == Some(pid.as_str())).to_string()
                on:click=move |_| pick_row(id.clone())
            >
                <span class="id">{row.entry.ticket.clone()}</span>
                <span class="t">{move || now_row.with(|r| r.as_ref().map(|r| r.ticket.title.clone()).unwrap_or_default())}</span>
                <span class="m">
                    {move || now_row.with(|r| r.as_ref().map(|r| age(clock.get(), r.entry.asked_unix)).unwrap_or_default())}
                    {move || now_row.with(|r| r.as_ref().is_some_and(|r| r.entry.input.is_some())).then(|| view! { " · "<span class="ask">"asks"</span> })}
                </span>
            </button>
            {move || here().then(|| picked_row.get().map(|r| view! { <div class="qd-ph">{answer_view(r, sheet, true)}</div> }))}
        }
    };
    let grouped = Memo::new(move |_| rows.with(|r| groups(r, unfold.get(), picked.get().as_deref())));
    let list = move || {
        view! {
            <div class="ql">
                <For each=move || grouped.with(|g| g.iter().map(|g| g.urgency).collect::<Vec<_>>()) key=|u| *u let:u>
                    {
                        let g = Memo::new(move |_| grouped.with(|gs| gs.iter().find(|g| g.urgency == u).cloned()));
                        view! {
                            <div class="grp" data-urgency=u.as_str()>
                                <div class=format!("qg u-{}", u.as_str())>
                                    {u.as_str()}" "<b>{move || g.with(|g| g.as_ref().map_or(0, Group::len))}</b>
                                    <span class="ln"></span>
                                </div>
                                <For each=move || g.with(|g| g.as_ref().map(|g| g.rows.clone()).unwrap_or_default()) key=|r| r.entry.ticket.clone() let:row>
                                    {row_view(row)}
                                </For>
                                {move || {
                                    let n = g.with(|g| g.as_ref().map_or(0, |g| g.folded));
                                    (n > 0).then(|| view! {
                                        <button type="button" class="qmore" on:click=move |_| unfold.set(true)>{format!("{n} whenever · show")}</button>
                                    })
                                }}
                            </div>
                        }
                    }
                </For>
            </div>
        }
    };
    // The answer beside the list (and the one ticket's, alone): the picked
    // row's, else the last drawn while the sheet slides down.
    let pane = move || {
        let many = !one.get();
        if many && phone.get() {
            return None;
        }
        let row = picked_row.get().or_else(|| last.get_untracked())?;
        Some(view! { <div class="qpane">{answer_view(row, sheet, many)}</div> })
    };
    view! {
        <div
            id="qtray"
            node_ref=tray_ref
            class="qtray"
            class:empty=move || rows.with(Vec::is_empty) && !closing.get()
            class:open=move || open.get()
            class:one=move || one.get()
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                if !open.get_untracked() {
                    return;
                }
                let typing = ev
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .is_some_and(|el| matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT"));
                match crate::bindings::hit(&ev, crate::bindings::Context::Dialog) {
                    Some("dialog.close") => {
                        ev.stop_propagation();
                        close_sheet(true);
                    }
                    Some("queue.next") if !typing && !one.get_untracked() => {
                        ev.prevent_default();
                        step_by(1);
                    }
                    Some("queue.prev") if !typing && !one.get_untracked() => {
                        ev.prevent_default();
                        step_by(-1);
                    }
                    _ => {}
                }
            }
        >
            <div class="qscrim" aria-hidden="true" on:click=move |_| close_sheet(false)></div>
            <div class="qclip">
                <div
                    id="qsheet"
                    node_ref=sheet_ref
                    class="qs"
                    class:one=move || one.get()
                    class:loud=move || ctl.with(|c| c.as_ref().is_some_and(|c| c.loudest.loud()))
                    role="dialog"
                    aria-label="the queue"
                    tabindex="-1"
                    inert=move || !open.get()
                    aria-live="polite"
                >
                    <div class="qgrab" aria-hidden="true"></div>
                    <div class="qh">
                        <span class="qt">"queue"</span>
                        <b class="qn">{move || rows.with(Vec::len)}</b>
                        {move || (!one.get()).then(|| view! { <span class="qjs">{chips}</span> })}
                        <span class="sp"></span>
                        {move || (!one.get()).then(|| view! { <span class="kb" aria-hidden="true"><kbd>"j"</kbd>" "<kbd>"k"</kbd></span> })}
                        <button type="button" class="qx" on:click=move |_| close_sheet(true)>"close ▾ "<kbd>"esc"</kbd></button>
                    </div>
                    <div class="qb">
                        {move || (!one.get()).then(list)}
                        {pane}
                    </div>
                </div>
            </div>
            {move || flash.get().map(|(a, n)| view! {
                <i class=format!("qedge u-{}", a.urgency.as_str()) data-gen=n.to_string() aria-hidden="true"></i>
                <div class=format!("qarrived u-{}", a.urgency.as_str()) role="status" aria-label=a.label()>"queued · "<b>{a.urgency.as_str()}</b>" · "{a.who.clone()}</div>
            })}
            {card_view}
            <button
                type="button"
                id="qcontrol"
                node_ref=control_ref
                class=move || ctl.with(|c| c.as_ref().map(|c| c.class(open.get(), fresh.get())).unwrap_or_else(|| "qc".into()))
                aria-expanded=move || open.get().to_string()
                aria-controls="qsheet"
                aria-label=move || ctl.with(|c| c.as_ref().map(Control::words).unwrap_or_default())
                aria-describedby=move || hovered.with(Option::is_some).then_some("qcard")
                data-ticket-id=move || one.get().then(|| order.with(|o| o.first().cloned())).flatten()
                on:mouseenter={
                    let begin = hover_begin.clone();
                    move |ev| begin(ev.into())
                }
                on:focus={
                    let begin = hover_begin.clone();
                    move |ev| begin(ev.into())
                }
                on:mouseleave={
                    let end = hover_end.clone();
                    move |_| end()
                }
                on:blur={
                    let end = hover_end.clone();
                    move |_| end()
                }
                on:click=move |_| {
                    hover_end();
                    if open.get_untracked() {
                        close_sheet(false);
                    } else {
                        open_sheet();
                    }
                }
            >
                {face}
            </button>
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
        crate::grow::follow(area, move || draft.text.track());
        view! {
            <div class="qchange open">
                <textarea
                    node_ref=area
                    class="grow"
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
        InputKind::Text => {
            let area = NodeRef::<leptos::html::Textarea>::new();
            crate::grow::follow(area, move || pick.with(|p| p.text.clone()));
            view! {
                <div class="qopts text">
                    <textarea
                        node_ref=area
                        class="grow"
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
            .into_any()
        }
    };
    let kind = input.kind;
    let send = move |_| {
        if busy.get_untracked() {
            return;
        }
        if let Some(value) = pick.with_untracked(|p| p.value(kind)) {
            submit(id.clone(), named.clone(), value, problem, busy, pick);
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
            archived: None,
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
        assert!(q.apply(&TicketsEvent::Deleted { ids: vec![menger_tickets_api::TicketId::new("A-2")], tombstones: Vec::new() }));
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
            archived: None,
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
        assert_eq!(age(100, 100), "now");
        assert_eq!(age(400, 100), "5m");
        assert_eq!(age(7_300, 100), "2h");
        assert_eq!(age(300_000, 100), "3d");
        let mut e = entry("A-1", Urgency::Blocking, 0);
        assert_eq!(row_class(&e, false, false), "qr u-blocking");
        assert_eq!(row_class(&e, false, true), "qr u-blocking unseen", "arrived: the dot");
        assert_eq!(row_class(&e, true, true), "qr u-blocking sel", "picked: no dot");
        e.urgency = Urgency::Today;
        assert_eq!(who(&e), "idp:agent");
        e.name = Some("Ada".into());
        assert_eq!(who(&e), "Ada");
        assert_eq!(open_href(&e), "#/ticket/A-1");
        e.input = Some("landing".into());
        assert_eq!(open_href(&e), "#/ticket/A-1?anchor=landing");
    }

    fn rows_of(spec: &[(&str, Urgency)]) -> Vec<Row> {
        let mut q = Queue::default();
        for (i, (id, u)) in spec.iter().enumerate() {
            q.apply(&queued(id, *u, i as i64));
        }
        q.rows().to_vec()
    }

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn strings(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// APP-38: the control's model — the count, the loudest urgency and how
    /// many share it, what is new, and whether it nudges.
    #[test]
    fn the_control_says_the_count_and_the_loudest() {
        assert_eq!(control(&[], &set(&[]), &set(&[])), None, "nothing queued: no control");
        let rows = rows_of(&[("A-1", Urgency::Whenever), ("A-2", Urgency::Now), ("A-3", Urgency::Blocking), ("A-4", Urgency::Blocking), ("A-5", Urgency::Today)]);
        let c = control(&rows, &set(&[]), &set(&["A-5", "Z-9"])).unwrap();
        assert_eq!(c, Control { count: 5, loudest: Urgency::Blocking, sharing: 2, new: 1, nudge: true });
        assert_eq!(c.chip(), "2 blocking");
        assert_eq!(c.words(), "5 queued, 2 blocking, 1 new");
        assert_eq!(c.class(false, None), "qc u-blocking many loud nudge has-new");
        assert_eq!(c.class(true, Some(Urgency::Today)), "qc u-blocking many loud nudge open has-new fresh f-today", "the arrival's colour, not the loudest's");
        // Every loud one opened: no nudge; a quiet one unopened does not nudge.
        let c = control(&rows, &set(&["A-2", "A-3", "A-4"]), &set(&[])).unwrap();
        assert!(!c.nudge);
        assert_eq!(c.class(false, None), "qc u-blocking many loud");
        // The loudest is today: Coding's pair, no loud, no nudge.
        let quiet = rows_of(&[("B-1", Urgency::Whenever), ("B-2", Urgency::Today), ("B-3", Urgency::Today)]);
        let c = control(&quiet, &set(&[]), &set(&[])).unwrap();
        assert_eq!((c.loudest, c.sharing, c.nudge), (Urgency::Today, 2, false));
        assert_eq!(c.class(false, None), "qc u-today many");
        // One ticket: the control is its tab.
        let one = rows_of(&[("C-1", Urgency::Now)]);
        let c = control(&one, &set(&[]), &set(&[])).unwrap();
        assert!(c.one());
        assert_eq!(c.chip(), "now");
        assert_eq!(c.class(false, None), "qc u-now one qtab loud nudge");
    }

    /// APP-38: grouped blocking · now · today · whenever, the empty ones
    /// left out, in the queue's order within each; whenever folded past six
    /// unless unfolded or holding the pick.
    #[test]
    fn the_sheet_groups_by_urgency_and_folds_whenever() {
        let mut spec = vec![("T-1", Urgency::Today), ("B-1", Urgency::Blocking), ("T-2", Urgency::Today)];
        let ws: Vec<String> = (1..=7).map(|i| format!("W-{i}")).collect();
        for w in &ws {
            spec.push((w.as_str(), Urgency::Whenever));
        }
        let rows = rows_of(&spec);
        let g = groups(&rows, false, None);
        let shape: Vec<(Urgency, Vec<&str>, usize)> = g.iter().map(|g| (g.urgency, g.rows.iter().map(|r| r.entry.ticket.as_str()).collect(), g.folded)).collect();
        assert_eq!(
            shape,
            [(Urgency::Blocking, vec!["B-1"], 0), (Urgency::Today, vec!["T-1", "T-2"], 0), (Urgency::Whenever, vec![], 7)],
            "no now group; whenever's seven folded"
        );
        assert_eq!(g[2].len(), 7);
        assert_eq!(groups(&rows, true, None)[2].rows.len(), 7, "shown when asked");
        assert_eq!(groups(&rows, false, Some("W-3"))[2].rows.len(), 7, "or when the pick is in it");
        let six = rows_of(&[("W-1", Urgency::Whenever), ("W-2", Urgency::Whenever), ("W-3", Urgency::Whenever), ("W-4", Urgency::Whenever), ("W-5", Urgency::Whenever), ("W-6", Urgency::Whenever)]);
        assert_eq!(groups(&six, false, None)[0].folded, 0, "six are not folded");
        assert!(groups(&[], false, None).is_empty());
    }

    /// APP-38: submit, snooze or done moves the pick to the next row; the
    /// last row's leaving goes back one; a pick still queued stays.
    #[test]
    fn the_next_row_is_picked_after_one_leaves() {
        let before = strings(&["A", "B", "C", "D"]);
        assert_eq!(next_pick(&before, &strings(&["A", "C", "D"]), Some("B")).as_deref(), Some("C"), "the next");
        assert_eq!(next_pick(&before, &strings(&["A", "D"]), Some("B")).as_deref(), Some("D"), "the next that stayed");
        assert_eq!(next_pick(&before, &strings(&["A", "B", "C"]), Some("D")).as_deref(), Some("C"), "the last left: the one before");
        assert_eq!(next_pick(&before, &strings(&["A", "B", "C", "D"]), Some("B")).as_deref(), Some("B"), "still there: kept");
        assert_eq!(next_pick(&before, &strings(&["E"]), Some("B")).as_deref(), Some("E"), "everything else left: the first there is");
        assert_eq!(next_pick(&before, &[], Some("B")), None, "nothing left");
        assert_eq!(next_pick(&before, &strings(&["A"]), None), None, "nothing picked");
        // An arrival ahead of it does not take the pick.
        assert_eq!(next_pick(&before, &strings(&["N", "A", "C", "D"]), Some("B")).as_deref(), Some("C"));
        // j / k, held at either end; where the pick stands.
        assert_eq!(step_pick(&before, Some("B"), 1).as_deref(), Some("C"));
        assert_eq!(step_pick(&before, Some("B"), -1).as_deref(), Some("A"));
        assert_eq!(step_pick(&before, Some("D"), 1).as_deref(), Some("D"));
        assert_eq!(step_pick(&before, Some("A"), -1).as_deref(), Some("A"));
        assert_eq!(step_pick(&before, None, 1).as_deref(), Some("A"));
        assert_eq!(step_pick(&[], None, 1), None);
        assert_eq!(position(&before, Some("C")), Some((3, 4)));
        assert_eq!(position(&before, Some("Z")), None);
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
