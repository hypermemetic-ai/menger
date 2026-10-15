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
//!   or Esc, lowers it.
//! - **A hidden tab's title** carries the count: `(2) menger`.
//! - **Motion** is DESIGN-39's setting: `calm` keeps the slide and drops the
//!   nudge, `none` keeps the tray and moves nothing (index.html's rules).
//!
//! What decides — applying an event, the order, what shows at a moment,
//! when a snooze ends, the title, the words — is pure and pinned by native
//! tests; the rest is the session's socket and the DOM.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use leptos::prelude::*;
use menger_tickets_api::queue::order;
use menger_tickets_api::{Input, InputKind, InputState, InputValue, QueueEntry, TicketSummary, TicketsEvent};
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
            TicketsEvent::Unqueued { id, .. } => self.remove(id),
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

/// A tab's classes: its urgency, `loud` for now and blocking, `open` while
/// its sheet is up, `seen` once the owner has opened it (no more nudging).
pub(crate) fn tab_class(e: &QueueEntry, open: bool, seen: bool) -> String {
    let mut c = format!("qtab {}", e.urgency.as_str());
    if e.urgency.loud() {
        c.push_str(" loud");
    }
    if open {
        c.push_str(" open");
    }
    if seen {
        c.push_str(" seen");
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

thread_local! {
    /// The queue — `None` until the first fetch lands.
    static QUEUE: ArcRwSignal<Option<Queue>> = ArcRwSignal::new(None);
    /// The server's clock less the browser's, in seconds (`queue_end`'s).
    static SKEW: Cell<i64> = const { Cell::new(0) };
    /// The newest fetch; an older one's answer is dropped.
    static FETCH: Cell<u32> = const { Cell::new(0) };
    /// While a fetch is out: the events heard since it went.
    static HEARD: RefCell<Option<Vec<TicketsEvent>>> = const { RefCell::new(None) };
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
    QUEUE.with(|s| {
        s.maybe_update(|held| held.as_mut().is_some_and(|q| q.apply(ev)));
    });
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
                QUEUE.with(|s| s.set(Some(settle(events, &heard))));
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
    spawn_local(async move { write(problem, client.unqueue_all(id, Some("app".into())).await).await });
}

/// `id` away for [`SNOOZE_MINUTES`].
fn snooze(id: String, problem: RwSignal<Option<String>>) {
    let Some(client) = session::client() else { return };
    spawn_local(async move { write(problem, client.snooze_all(id, SNOOZE_MINUTES, Some("app".into())).await).await });
}

/// Answer `input` of `id` with `value` (source `app`), then — the owner
/// has answered what was asked — take the ticket out of the queue.
fn answer(id: String, input: String, value: InputValue, problem: RwSignal<Option<String>>, busy: RwSignal<bool>) {
    let Some(client) = session::client() else { return };
    busy.set(true);
    spawn_local(async move {
        let sent = bridge::submit(&client, id.clone(), bridge::Submit { input, value, rationale: None }).await;
        match sent {
            Ok(_) => write(problem, client.unqueue_all(id, Some("app".into())).await).await,
            Err(why) => {
                problem.try_set(Some(why));
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
    let hidden = RwSignal::new(doc.hidden());
    let on = Closure::<dyn Fn()>::new(move || {
        if let Some(d) = dom::window().document() {
            hidden.try_set(d.hidden());
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
            if c.try_set(now()).is_some() {
                return;
            }
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
    let open = RwSignal::new(None::<String>);
    let seen = RwSignal::new(HashSet::<String>::new());
    let problem = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
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
        }
    });
    let toggle = move |id: String| {
        problem.set(None);
        seen.update(|s| {
            s.insert(id.clone());
        });
        open.update(|o| *o = if o.as_deref() == Some(id.as_str()) { None } else { Some(id) });
    };
    let sheet = move || {
        let id = open.get()?;
        let row = rows.with(|r| r.iter().find(|x| x.entry.ticket == id).cloned())?;
        let e = row.entry.clone();
        let input = e.input.clone().and_then(|i| inputs_held.with(|h| h.as_ref().and_then(|h| h.list().iter().find(|x| x.ticket == e.ticket && x.id == i).cloned())));
        let hd = format!("{} · {} · {}", e.urgency.as_str(), who(&e), ago(clock.get(), e.asked_unix));
        let (snooze_id, done_id) = (id.clone(), id.clone());
        Some(view! {
            <div class="qsheet-in" data-ticket-id=id.clone()>
                <div class=format!("qhd {}", e.urgency.as_str())>{hd}</div>
                <h4><a class="code" href=format!("#/ticket/{id}")>{id.clone()}</a>" · "{row.ticket.title.clone()}</h4>
                <p class="qwhy">{e.reason.clone()}</p>
                {input_view(id.clone(), e.input.clone(), input, problem, busy)}
                <div class="qacts">
                    <a class="qbtn fill" href=open_href(&e) on:click=move |_| open.set(None)>"open the ticket"</a>
                    <button type="button" class="qbtn" on:click=move |_| snooze(snooze_id.clone(), problem)>{format!("snooze {SNOOZE_MINUTES}m")}</button>
                    <button type="button" class="qbtn" on:click=move |_| done(done_id.clone(), problem)>"done with it"</button>
                </div>
                {move || problem.get().map(|p| view! { <p class="qproblem" role="alert">{p}</p> })}
            </div>
        })
    };
    view! {
        <div
            id="qtray"
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
                class:loud=move || open.with(|o| o.as_ref().is_some_and(|id| rows.with(|r| r.iter().any(|x| &x.entry.ticket == id && x.entry.urgency.loud()))))
                aria-live="polite"
            >
                <div class="qsheet-clip">{sheet}</div>
            </div>
            <div class="qtabs" role="toolbar" aria-label="queued for you">
                <For each=move || rows.get() key=|r| r.entry.ticket.clone() let:row>
                    {
                        let id = row.entry.ticket.clone();
                        let now_row = {
                            let id = id.clone();
                            Memo::new(move |_| rows.with(|r| r.iter().find(|x| x.entry.ticket == id).cloned()))
                        };
                        let (cid, tid) = (id.clone(), id.clone());
                        view! {
                            <button
                                type="button"
                                class=move || now_row.with(|r| r.as_ref().map(|r| tab_class(&r.entry, open.with(|o| o.as_deref() == Some(cid.as_str())), seen.with(|s| s.contains(&cid)))).unwrap_or_default())
                                data-ticket-id=id.clone()
                                aria-expanded=move || open.with(|o| o.as_deref() == Some(tid.as_str())).to_string()
                                title=move || now_row.with(|r| r.as_ref().map(|r| format!("{} — {} ({})", r.entry.ticket, r.entry.reason, r.entry.urgency.as_str())).unwrap_or_default())
                                on:click=move |_| toggle(id.clone())
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

/// The named input in the sheet: its question and its controls while it is
/// open, what was answered once it is, or nothing when there is none.
fn input_view(
    id: String,
    named: Option<String>,
    input: Option<Input>,
    problem: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
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
    let controls = match input.kind {
        InputKind::Choice => {
            let opts = input
                .options
                .iter()
                .map(|o| {
                    let (id, named, value) = (id.clone(), named.clone(), o.value.clone());
                    view! {
                        <button
                            type="button"
                            class="qopt"
                            class:rec=o.recommended
                            disabled=move || busy.get()
                            on:click=move |_| answer(id.clone(), named.clone(), InputValue::Choice { option: value.clone() }, problem, busy)
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
            let ticked = RwSignal::new(Vec::<String>::new());
            let boxes = input
                .options
                .iter()
                .map(|o| {
                    let v = o.value.clone();
                    view! {
                        <label class="qtick">
                            <input
                                type="checkbox"
                                on:change=move |ev| {
                                    let on = event_target_checked(&ev);
                                    let v = v.clone();
                                    ticked.update(|t| {
                                        t.retain(|x| *x != v);
                                        if on {
                                            t.push(v);
                                        }
                                    });
                                }
                            />
                            {o.label.clone()}
                        </label>
                    }
                })
                .collect_view();
            let send = move |_| answer(id.clone(), named.clone(), InputValue::Multi { options: ticked.get_untracked() }, problem, busy);
            view! { <div class="qopts multi">{boxes}<button type="button" class="qbtn fill" disabled=move || busy.get() on:click=send>"answer"</button></div> }.into_any()
        }
        InputKind::Text => {
            let text = RwSignal::new(String::new());
            let send = move |_| answer(id.clone(), named.clone(), InputValue::Text { text: text.get_untracked() }, problem, busy);
            view! {
                <div class="qopts text">
                    <textarea rows="2" aria-label="your reply" prop:value=move || text.get() on:input=move |ev| text.set(event_target_value(&ev))></textarea>
                    <button type="button" class="qbtn fill" disabled=move || busy.get() || text.with(|t| t.trim().is_empty()) on:click=send>"answer"</button>
                </div>
            }
            .into_any()
        }
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
            id: id.into(),
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
        let heard = vec![queued("A-2", Urgency::Now, 3), TicketsEvent::Amended { ticket: renamed, changed: vec!["title".into()] }];
        let q = settle(answer, &heard);
        assert_eq!(ids(&q), ["A-2", "A-1"]);
        assert_eq!(q.get("A-1").unwrap().ticket.title, "new", "a row event moves the title");
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
        assert_eq!(tab_class(&e, false, false), "qtab blocking loud");
        assert_eq!(tab_class(&e, true, true), "qtab blocking loud open seen");
        e.urgency = Urgency::Today;
        assert_eq!(tab_class(&e, false, false), "qtab today");
        assert_eq!(who(&e), "idp:agent");
        e.name = Some("Ada".into());
        assert_eq!(who(&e), "Ada");
        assert_eq!(open_href(&e), "#/ticket/A-1");
        e.input = Some("landing".into());
        assert_eq!(open_href(&e), "#/ticket/A-1?anchor=landing");
    }
}
