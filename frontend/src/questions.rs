//! APP-84: **all questions** — a quiet count in the header that opens a
//! panel (APP-51's `all-home`, the owner's pick: *Header count + panel*,
//! Fig 1a).
//!
//! - **The count** ([`HeaderCount`]): `questions N` in the header beside
//!   `live`, in the muted ink with a plain number — it never flashes or
//!   nudges. N is every open input the reader can see: the app's copy of the
//!   inputs (src/inputs.rs — `inputs {}`, kept by `watch`'s
//!   `input_changed`), so it moves the moment a question is asked or
//!   answered anywhere. At a phone's width (≤ 820, APP-50) the header is the
//!   top line, and the count sits there as `? N` ([`PhoneCount`]): the bar
//!   has no fifth place.
//! - **The panel** ([`Panel`]): a click on the count, or `g q`, drops it
//!   under the count (a phone's: from the top line, the whole width, over a
//!   scrim). Its head says `Questions · N open on T tickets` and, when the
//!   tray holds any, `K important ↓`, which raises the tray (the important
//!   tier, unchanged) and lowers the panel. Its list is every open question
//!   grouped by ticket, the ticket asked most recently first ([`groups`]),
//!   each ticket's questions in its body's order; a row says the input's
//!   kind ([`kind_word`]), its question, how long it has waited ([`since`]:
//!   when it was asked — the server's `asked_unix` — or last opened again,
//!   whichever is later) and, when its ticket is queued for it, the
//!   urgency and who queued it ([`queued_for`]). Its foot links `#/inputs`.
//! - **Answer in place**: a row clicked (or Enter on it) opens its answer
//!   under it — the tray's own controls (`queue::controls_of`: pick, then
//!   one submit, APP-25), recorded with source `app`. An answered row stays
//!   in the panel as `✓ <answer>` while it is open (with *change it*, which
//!   clears the answer), so the click is seen to land; the count drops at
//!   once.
//! - **Closing**: Esc, the count again, a press outside the panel (a frame's
//!   included, as the tray's), *open ↗* on a row, or the scrim.
//! - **Titles** are the tickets' rows: on opening, `list { filter: asking }`
//!   (summaries) held in the row cache while the panel is up, so a title
//!   moves live; a ticket that starts asking while it is up is asked again.
//!
//! What decides — the order, a row's age and kind, the words, whether a row
//! is queued — is pure and pinned by native tests; the rest is the DOM.

use std::collections::HashSet;

use leptos::prelude::*;
use menger_tickets_api::vocab::CallerSource;
use menger_tickets_api::{Input, InputKind, InputState, QueueEntry, TicketsEvent, Urgency};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::queue::{self, Pick};
use crate::{bridge, cache, dom, inputs, session};

/// When `input` started waiting: when it was asked (`asked_unix`), or its
/// last history entry — a clear, a reopen, a restore puts it back to open —
/// whichever is later. `None` from a server before APP-84 with no history.
pub(crate) fn since(input: &Input) -> Option<i64> {
    let last = input.history.last().map(|e| e.at_unix);
    match (input.asked_unix, last) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// The kind as a row says it.
pub(crate) fn kind_word(kind: InputKind) -> &'static str {
    match kind {
        InputKind::Choice => "decision",
        InputKind::Multi => "multi",
        InputKind::Text => "text",
    }
}

/// An input's key in the panel: `TICKET#id`.
pub(crate) fn key(input: &Input) -> String {
    format!("{}#{}", input.ticket, input.id)
}

/// One ticket's questions in the panel.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Group {
    pub(crate) ticket: String,
    /// Its rows, in its body's order.
    pub(crate) inputs: Vec<Input>,
    /// How many of them are still open.
    pub(crate) open: usize,
}

/// The panel's list: every open input, and those `kept` (answered while the
/// panel was up), grouped by ticket — the ticket whose newest row waited
/// least first; ties (and inputs from a server that says no time) keep the
/// store's order. Each ticket's rows in its body's order.
pub(crate) fn groups(list: &[Input], kept: &HashSet<String>) -> Vec<Group> {
    let mut out: Vec<(Group, Option<i64>)> = Vec::new();
    for i in list {
        let shown = i.state == InputState::Open || (i.state == InputState::Answered && kept.contains(&key(i)));
        if !shown {
            continue;
        }
        let at = since(i);
        let open = usize::from(i.state == InputState::Open);
        match out.iter_mut().find(|(g, _)| g.ticket == i.ticket.as_str()) {
            Some((g, newest)) => {
                g.inputs.push(i.clone());
                g.open += open;
                *newest = (*newest).max(at);
            }
            None => out.push((Group { ticket: i.ticket.to_string(), inputs: vec![i.clone()], open }, at)),
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out.into_iter().map(|(g, _)| g).collect()
}

/// The head's words: `18 open on 14 tickets`.
pub(crate) fn heading(open: usize, tickets: usize) -> String {
    match (open, tickets) {
        (0, _) => "nothing waiting on you".into(),
        (n, t) => format!("{n} open on {t} ticket{}", if t == 1 { "" } else { "s" }),
    }
}

/// The count's title.
pub(crate) fn count_title(n: Option<usize>) -> String {
    match n {
        None => "every open question (g q)".into(),
        Some(0) => "no question is waiting on you (g q)".into(),
        Some(1) => "1 open question — every one, in a panel (g q)".into(),
        Some(n) => format!("{n} open questions — every one, in a panel (g q)"),
    }
}

/// The queue's entry for `input`, when its ticket is queued for it: the
/// entry names this input, or names none (the ticket as a whole).
pub(crate) fn queued_for<'a>(rows: &'a [queue::Row], input: &Input) -> Option<&'a QueueEntry> {
    rows.iter()
        .map(|r| &r.entry)
        .find(|e| e.ticket == input.ticket.as_str() && e.input.as_deref().is_none_or(|i| i == input.id.as_str()))
}

/// APP-87: what a row's pin control is — the owner moving a question from
/// *all* to *important* by queueing its ticket for it, as themself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PinState {
    /// Not in the tray: **↑ important** pins it.
    Pin,
    /// Queued for it by the reader: *★ pinned by you*, **unpin**.
    Pinned,
    /// Queued for it by someone else (an agent): *queued by …*.
    Queued,
    /// Its ticket is queued for another of its questions: a ticket is
    /// queued once, for one input, so pinning this one would take the
    /// other's place — not offered; the row says which.
    Elsewhere(String),
}

/// The pin control for `input`, `rows` the tray's, `me` the reader's
/// principal.
pub(crate) fn pin_state(rows: &[queue::Row], input: &Input, me: Option<&str>) -> PinState {
    if let Some(e) = queued_for(rows, input) {
        return if queue::pinned_by(e, me) { PinState::Pinned } else { PinState::Queued };
    }
    match rows.iter().map(|r| &r.entry).find(|e| e.ticket == input.ticket.as_str()) {
        Some(e) => PinState::Elsewhere(e.input.clone().unwrap_or_default()),
        None => PinState::Pin,
    }
}

/// APP-87: the urgencies a pin offers, quietest first — `blocking` is an
/// agent's word (it is idle), never the owner's.
pub(crate) const PIN_URGENCIES: [Urgency; 3] = [Urgency::Whenever, Urgency::Today, Urgency::Now];

/// The urgency a pin starts at (Fig 4).
pub(crate) const PIN_DEFAULT: Urgency = Urgency::Now;

/// The queue's reason for a pin: the note, or what a pin is.
pub(crate) fn pin_reason(note: &str) -> String {
    match note.trim() {
        "" => "Pinned from all questions.".to_string(),
        n => n.to_string(),
    }
}

/// The words after a pin: `KILN-4 · unit pinned: important, now`.
pub(crate) fn pinned_words(ticket: &str, input: &str, u: Urgency) -> String {
    format!("{ticket} · {input} pinned: important, {}", u.as_str())
}

/// Keys of `list`'s open inputs the tray holds an entry for.
pub(crate) fn queued_keys(list: &[Input], rows: &[queue::Row]) -> HashSet<String> {
    list.iter().filter(|i| i.state == InputState::Open && queued_for(rows, i).is_some()).map(key).collect()
}

/// Those of `now` that `before` did not hold — rows just queued, to flash.
pub(crate) fn arrived(before: &HashSet<String>, now: &HashSet<String>) -> Vec<String> {
    let mut v: Vec<String> = now.difference(before).cloned().collect();
    v.sort();
    v
}

/// How many open questions there are.
fn open_of(list: &[Input]) -> usize {
    list.iter().filter(|i| i.state == InputState::Open).count()
}

thread_local! {
    /// Whether the panel is up.
    static OPEN: ArcRwSignal<bool> = ArcRwSignal::new(false);
}

fn open_signal() -> RwSignal<bool> {
    RwSignal::from(OPEN.with(Clone::clone))
}

/// `g q`, or a click on the count: the panel up, or down.
pub(crate) fn toggle() {
    OPEN.with(|o| o.update(|o| *o = !*o));
}

/// Esc anywhere: the panel down — whether it was up.
pub(crate) fn escape() -> bool {
    OPEN.with(|o| {
        let up = o.get_untracked();
        if up {
            o.set(false);
        }
        up
    })
}

/// The open count, once the app holds the inputs.
fn count() -> Memo<Option<usize>> {
    let held = inputs::signal();
    Memo::new(move |_| held.with(|h| h.as_ref().map(|h| open_of(h.list()))))
}

/// The header's `questions N`, beside `live`.
#[component]
pub(crate) fn HeaderCount() -> impl IntoView {
    inputs::want();
    let open = open_signal();
    let n = count();
    move || n.get().map(|_| view! {
        <button
            type="button"
            class="hq"
            id="questions-link"
            aria-haspopup="dialog"
            aria-controls="qpanel"
            aria-expanded=move || open.get().to_string()
            title=move || count_title(n.get())
            on:click=move |_| toggle()
        >
            "questions"
            <span class="n">{move || n.get().unwrap_or(0)}</span>
        </button>
    })
}

/// The phone's top line: `? N`.
#[component]
pub(crate) fn PhoneCount() -> impl IntoView {
    inputs::want();
    let open = open_signal();
    let n = count();
    move || n.get().map(|_| view! {
        <button
            type="button"
            class="hq"
            id="pquestions"
            aria-haspopup="dialog"
            aria-controls="qpanel"
            aria-label="every open question"
            aria-expanded=move || open.get().to_string()
            title=move || count_title(n.get())
            on:click=move |_| toggle()
        >
            "?"
            <span class="n">{move || n.get().unwrap_or(0)}</span>
        </button>
    })
}

/// Where the panel hangs: under the count that is showing — its top, and
/// (on a desk) its right edge from the window's.
fn anchor() -> Option<(f64, f64)> {
    let doc = dom::window().document()?;
    // A phone's: from the top line's lower edge.
    for (sel, line) in [("#questions-link", None), ("#pquestions", Some("#phead"))] {
        let Some(el) = doc.query_selector(sel).ok().flatten() else { continue };
        let r = el.get_bounding_client_rect();
        if r.width() > 0.0 {
            let w = dom::window().inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
            let top = match line.and_then(|l| doc.query_selector(l).ok().flatten()) {
                Some(head) => head.get_bounding_client_rect().bottom(),
                None => r.bottom() + 6.0,
            };
            // Under the count's right edge, kept inside the window.
            let width = 560f64.min(w - 32.0);
            return Some((top, (w - r.right()).min(w - width - 8.0).max(8.0)));
        }
    }
    None
}

/// The tickets' rows, for their titles: `list { filter: asking }`, held.
fn fetch_titles(holder: cache::Holder, asked: RwSignal<u32>) {
    let Some(client) = session::client() else { return };
    asked.update(|n| *n = n.wrapping_add(1));
    spawn_local(async move {
        let Ok(filter) = crate::typed::typed("asking") else { return };
        let answer = client
            .list_all(None, None, None, None, Some(1000), Some(true), Some(filter), None, None, session::zone())
            .await;
        match answer {
            Ok(events) => {
                for ev in events {
                    if let TicketsEvent::Summary { summary } = ev {
                        holder.hold(summary);
                    }
                }
            }
            Err(e) => leptos::logging::log!("questions: {}", session::describe(&e)),
        }
    });
}

/// What one open row's answer reads and writes.
#[derive(Clone, Copy)]
struct Answering {
    sel: RwSignal<Option<String>>,
    pick: RwSignal<Pick>,
    busy: RwSignal<bool>,
    problem: RwSignal<Option<String>>,
    kept: RwSignal<HashSet<String>>,
    open: RwSignal<bool>,
}

/// Submit: the input answered (source `app`); kept in the panel as answered.
fn submit(a: Answering, ticket: String, input: String, value: menger_tickets_api::InputValue) {
    let Some(client) = session::client() else { return };
    a.busy.set(true);
    a.problem.set(None);
    let k = format!("{ticket}#{input}");
    spawn_local(async move {
        let sent = bridge::submit(&client, ticket, bridge::Submit { input, value, rationale: None }).await;
        match sent {
            Ok(_) => {
                a.kept.try_update(|s| s.insert(k));
                a.sel.try_set(None);
                a.pick.try_set(Pick::default());
            }
            Err(why) => {
                a.problem.try_set(Some(why));
            }
        }
        a.busy.try_set(false);
    });
}

/// *change it*: the answer cleared — the row open again.
fn change(a: Answering, ticket: String, input: String) {
    let Some(client) = session::client() else { return };
    let k = format!("{ticket}#{input}");
    a.problem.set(None);
    spawn_local(async move {
        match bridge::clear(&client, ticket, bridge::Clear { input }).await {
            Ok(_) => {
                a.kept.try_update(|s| s.remove(&k));
                a.sel.try_set(Some(k));
            }
            Err(why) => {
                a.problem.try_set(Some(why));
            }
        }
    });
}

/// APP-87: a pin being asked — which question, where its popover hangs.
#[derive(Clone, Debug, PartialEq)]
struct PinAsk {
    ticket: String,
    input: String,
    top: f64,
    left: f64,
    /// The list's scroll when it opened: a scroll that moves it away closes it.
    scrolled: f64,
}

/// What pinning reads and writes.
#[derive(Clone, Copy)]
struct Pinning {
    ask: RwSignal<Option<PinAsk>>,
    urgency: RwSignal<Urgency>,
    note: RwSignal<String>,
    /// The last pin, for *undo*: its ticket and the words.
    undo: RwSignal<Option<(String, String)>>,
    flash: RwSignal<HashSet<String>>,
    me: Memo<Option<String>>,
    busy: RwSignal<bool>,
    problem: RwSignal<Option<String>>,
}

/// The popover's place: under `el`, its right edge at the button's (a
/// phone's: CSS sets the sides), above it when there is no room below.
fn pin_place(el: &web_sys::Element) -> (f64, f64) {
    let r = el.get_bounding_client_rect();
    let w = dom::window().inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
    let h = dom::window().inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
    let top = if r.bottom() + 6.0 + 230.0 > h { (r.top() - 236.0).max(8.0) } else { r.bottom() + 6.0 };
    (top, (r.right() - 290.0).min(w - 300.0).max(8.0))
}

/// How far the panel's list is scrolled.
fn list_scroll() -> f64 {
    dom::window()
        .document()
        .and_then(|d| d.get_element_by_id("HPL"))
        .map_or(0.0, |el| f64::from(el.scroll_top()))
}

/// **↑ important** pressed: the popover asks an urgency and a note.
fn ask_pin(p: Pinning, ticket: String, input: String, el: Option<web_sys::Element>) {
    let (top, left) = el.as_ref().map_or((80.0, 16.0), pin_place);
    p.urgency.set(PIN_DEFAULT);
    p.note.set(String::new());
    p.problem.set(None);
    p.ask.set(Some(PinAsk { ticket, input, top, left, scrolled: list_scroll() }));
}

/// The queue's write's answer: its refusal, if any; otherwise each event
/// to the tray's copy at once (the watch's copy moves nothing more).
async fn queue_write(ev: Result<Vec<TicketsEvent>, plexus_client::ClientError>) -> Option<String> {
    match ev {
        Ok(events) => {
            let refused = events.iter().find_map(|e| match e {
                TicketsEvent::Error { message } => Some(message.clone()),
                _ => None,
            });
            if refused.is_none() {
                events.iter().for_each(queue::heard);
            }
            refused
        }
        Err(e) => Some(session::describe(&e)),
    }
}

/// **pin**: the ticket queued for the input, as the reader (`tickets.queue`,
/// source `app`) — the urgency asked, the note its reason.
fn pin(p: Pinning) {
    let Some(ask) = p.ask.get_untracked() else { return };
    let Some(client) = session::client() else { return };
    let (u, reason) = (p.urgency.get_untracked(), pin_reason(&p.note.get_untracked()));
    let (Ok(id), Ok(input), Ok(reason)) =
        (crate::typed::typed(&ask.ticket), crate::typed::typed(&ask.input), crate::typed::typed(&reason))
    else {
        p.problem.set(Some("the pin could not be read".into()));
        return;
    };
    p.busy.set(true);
    spawn_local(async move {
        let refused = queue_write(client.queue_all(id, u, reason, Some(input), Some(CallerSource::App)).await).await;
        match refused {
            Some(why) => {
                p.problem.try_set(Some(why));
            }
            None => {
                p.ask.try_set(None);
                let words = Some((ask.ticket.clone(), pinned_words(&ask.ticket, &ask.input, u)));
                p.undo.try_set(words.clone());
                // The undo bar goes after a while, unless another pin replaced it.
                spawn_local(async move {
                    dom::sleep_ms(6000).await;
                    if p.undo.try_get_untracked().flatten() == words {
                        p.undo.try_set(None);
                    }
                });
            }
        }
        p.busy.try_set(false);
    });
}

/// **unpin** / **undo**: the ticket out of the queue.
fn unpin(p: Pinning, ticket: String) {
    let Some(client) = session::client() else { return };
    let Ok(id) = crate::typed::typed(&ticket) else { return };
    p.undo.set(None);
    spawn_local(async move {
        if let Some(why) = queue_write(client.unqueue_all(id, Some(CallerSource::App)).await).await {
            p.problem.try_set(Some(why));
        }
    });
}

/// The pin control beside a row (or in its answer): **↑ important**,
/// **unpin**, or nothing.
fn pin_button(p: Pinning, state: &PinState, ticket: String, input: String, class: &'static str) -> Option<AnyView> {
    match state {
        PinState::Pin => Some(
            view! {
                <button
                    type="button"
                    class=class
                    data-act="pin"
                    title="bring it into the tray — important"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        let el = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
                        ask_pin(p, ticket.clone(), input.clone(), el);
                    }
                >
                    "↑ important"
                </button>
            }
            .into_any(),
        ),
        PinState::Pinned => Some(
            view! {
                <button
                    type="button"
                    class=format!("{class} on")
                    data-act="unpin"
                    title="take it out of the tray — it stays under all"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        unpin(p, ticket.clone());
                    }
                >
                    "unpin"
                </button>
            }
            .into_any(),
        ),
        PinState::Queued | PinState::Elsewhere(_) => None,
    }
}

/// The popover: an urgency and a note, then **pin**.
fn pin_popover(p: Pinning) -> impl IntoView {
    move || {
        let ask = p.ask.get()?;
        let urgs = PIN_URGENCIES
            .iter()
            .map(|&u| {
                view! {
                    <button
                        type="button"
                        class=format!("u-{}", u.as_str())
                        class:on=move || p.urgency.get() == u
                        aria-pressed=move || (p.urgency.get() == u).to_string()
                        data-u=u.as_str()
                        on:click=move |_| p.urgency.set(u)
                    >
                        {u.as_str()}
                    </button>
                }
            })
            .collect_view();
        Some(view! {
            <div
                id="qpin"
                class="pop"
                role="dialog"
                aria-label="bring it into the tray"
                style=format!("top: {}px; left: {}px", ask.top, ask.left)
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if crate::bindings::is(&ev, crate::bindings::Context::Dialog, "dialog.close") {
                        ev.stop_propagation();
                        p.ask.set(None);
                    }
                }
            >
                <h5>"Bring it into the tray"</h5>
                <p>{ask.ticket.clone()}" · "<code>{ask.input.clone()}</code>" joins the important tier; it stays under all too."</p>
                <div class="urgs">{urgs}</div>
                <textarea
                    rows="2"
                    aria-label="a note to yourself"
                    placeholder="a note to yourself (optional)"
                    prop:value=move || p.note.get()
                    on:input=move |ev| p.note.set(event_target_value(&ev))
                ></textarea>
                {move || p.problem.get().map(|why| view! { <p class="qproblem" role="alert">{why}</p> })}
                <div class="row">
                    <button type="button" class="qbtn" data-act="pin-cancel" on:click=move |_| p.ask.set(None)>"cancel"</button>
                    <button type="button" class="qbtn fill" data-act="pin-go" disabled=move || p.busy.get() on:click=move |_| pin(p)>
                        {move || if p.busy.get() { "pinning…" } else { "pin" }}
                    </button>
                </div>
            </div>
        })
    }
}

/// One row: its kind, its question, its age, whether it is queued — and,
/// picked, its answer in place.
fn row_view(input: Input, a: Answering, p: Pinning, now: i64, queued: Option<QueueEntry>, state: PinState) -> impl IntoView {
    let k = key(&input);
    let (ticket, id) = (input.ticket.to_string(), input.id.to_string());
    let done = input.state == InputState::Answered;
    let picked = {
        let k = k.clone();
        Memo::new(move |_| a.sel.with(|s| s.as_deref() == Some(k.as_str())))
    };
    let flip = {
        let k = k.clone();
        move || {
            a.problem.set(None);
            a.pick.set(Pick::default());
            a.sel.update(|s| *s = if s.as_deref() == Some(k.as_str()) { None } else { Some(k.clone()) });
        }
    };
    let age = since(&input).map(|t| queue::age(now, t)).unwrap_or_default();
    let said = input.answer.as_ref().map(|ans| queue::said(&input, &ans.value));
    let mine = state == PinState::Pinned;
    let st = queued.as_ref().map(|e| {
        let u = e.urgency.as_str();
        let by = if mine {
            view! { <span class="pinned">"★ pinned by you"</span> }.into_any()
        } else {
            view! { <span>{format!("queued by {}", queue::who(e))}</span> }.into_any()
        };
        view! { <span class=format!("chip-u u-{u}")>{u}</span>{by} }
    });
    let elsewhere = match &state {
        PinState::Elsewhere(other) => Some(view! { <span class="also">{format!("its ticket is queued for #{other}")}</span> }),
        _ => None,
    };
    let side_pin = (!done).then(|| pin_button(p, &state, ticket.clone(), id.clone(), "pin")).flatten();
    let pane_state = state.clone();
    let ok = said.clone().filter(|_| done).map(|s| view! { <span class="ok">{format!("✓ {s}")}</span> });
    let question = input.question.clone().unwrap_or_else(|| format!("#{id}"));
    let href = crate::needs::answer_href(&ticket, &id);
    let pane = {
        let input = input.clone();
        let (ticket, id, href) = (ticket.clone(), id.clone(), href.clone());
        move || {
            if !picked.get() {
                return None;
            }
            let open_link = view! { <a class="qbtn qopen" href=href.clone() on:click=move |_| a.open.set(false)>"open ↗"</a> };
            let problem = move || a.problem.get().map(|p| view! { <p class="qproblem" role="alert">{p}</p> });
            if done {
                let (t, i) = (ticket.clone(), id.clone());
                let words = format!("✓ {} — recorded; the ticket’s agent acts on it and closes it.", said.clone().unwrap_or_default());
                return Some(
                    view! {
                        <div class="inrow">
                            <div class="qd">
                                <p class="qdone">{words}</p>
                                <div class="qacts">
                                    <button type="button" class="qbtn" data-act="change" on:click=move |_| change(a, t.clone(), i.clone())>"change it"</button>
                                    {open_link}
                                </div>
                                {problem}
                            </div>
                        </div>
                    }
                    .into_any(),
                );
            }
            let kind = input.kind;
            let pane_pin = pin_button(p, &pane_state, ticket.clone(), id.clone(), "qbtn");
            let (t, i) = (ticket.clone(), id.clone());
            let send = move |_| {
                if a.busy.get_untracked() {
                    return;
                }
                if let Some(value) = a.pick.with_untracked(|p| p.value(kind)) {
                    submit(a, t.clone(), i.clone(), value);
                }
            };
            Some(
                view! {
                    <div class="inrow">
                        <div class="qd">
                            {(kind == InputKind::Multi).then(|| view! { <p class="qnext">"tick any"</p> })}
                            {queue::controls_of(&input, a.busy, a.pick)}
                            <div class="qacts">
                                <button
                                    type="button"
                                    class="qbtn fill"
                                    data-act="submit"
                                    disabled=move || a.busy.get() || a.pick.with(|p| p.value(kind).is_none())
                                    on:click=send
                                >
                                    {move || if a.busy.get() { "submitting…" } else { "submit" }}
                                </button>
                                {pane_pin}
                                {open_link}
                            </div>
                            {problem}
                        </div>
                    </div>
                }
                .into_any(),
            )
        }
    };
    let has_pin = side_pin.is_some();
    let fk = k.clone();
    let flashing = Memo::new(move |_| p.flash.with(|f| f.contains(&fk)));
    // A button cannot hold a button: the pin control sits over the row's
    // right edge, beside its age.
    view! {
        <div class="arw" class:pinnable=has_pin class:sel=move || picked.get()>
            <button
                type="button"
                class="ar"
                class:sel=move || picked.get()
                class:done=done
                class:flash=move || flashing.get()
                data-key=k.clone()
                aria-expanded=move || picked.get().to_string()
                on:click=move |_| flip()
            >
                <span class="kind">{kind_word(input.kind)}</span>
                <span class="q">{question}</span>
                <span class="side">{age}</span>
                <span class="st">{st}{ok}{elsewhere}</span>
            </button>
            {side_pin}
        </div>
        {pane}
    }
}

/// The panel: every open question, grouped by ticket, answered in place.
#[component]
pub(crate) fn Panel() -> impl IntoView {
    inputs::want();
    let open = open_signal();
    let held = RwSignal::from(inputs::signal());
    let qheld = RwSignal::from(queue::signal());
    let a = Answering {
        sel: RwSignal::new(None),
        pick: RwSignal::new(Pick::default()),
        busy: RwSignal::new(false),
        problem: RwSignal::new(None),
        kept: RwSignal::new(HashSet::new()),
        open,
    };
    let me = {
        let account = crate::signin::account();
        Memo::new(move |_| account.with(|acc| acc.me.as_ref().map(|m| m.principal.clone())))
    };
    let p = Pinning {
        ask: RwSignal::new(None),
        urgency: RwSignal::new(PIN_DEFAULT),
        note: RwSignal::new(String::new()),
        undo: RwSignal::new(None),
        flash: RwSignal::new(HashSet::new()),
        me,
        busy: RwSignal::new(false),
        problem: RwSignal::new(None),
    };
    let at = RwSignal::new(None::<(f64, f64)>);
    // Opening: hung under the count; closing forgets what was picked and
    // what was answered here.
    Effect::new(move |_| {
        if open.get() {
            at.set(anchor());
        } else {
            a.sel.set(None);
            a.kept.set(HashSet::new());
            a.problem.set(None);
            p.ask.set(None);
            p.undo.set(None);
        }
    });
    let all = Memo::new(move |_| {
        let kept = a.kept.get();
        held.with(|h| h.as_ref().map(|h| groups(h.list(), &kept)).unwrap_or_default())
    });
    let n = count();
    let important = Memo::new(move |_| qheld.with(|q| q.as_ref().map_or(0, |q| q.showing(queue::now()).len())));
    // APP-87: the important count ticks when it moves (an agent's queue, a pin).
    crate::motion::tick_on_change("#qpanel .qimp", move || Some(important.get()));
    // A question queued while the panel is up (an agent's call, heard live)
    // flashes its row once.
    let tray = Memo::new(move |_| qheld.with(|q| q.as_ref().map(|q| q.showing(queue::now())).unwrap_or_default()));
    Effect::new(move |prev: Option<HashSet<String>>| {
        let now = held.with(|h| h.as_ref().map(|h| queued_keys(h.list(), &tray.get())).unwrap_or_default());
        if let Some(before) = prev.filter(|_| open.get_untracked()) {
            let fresh = arrived(&before, &now);
            if !fresh.is_empty() {
                p.flash.update(|f| f.extend(fresh.iter().cloned()));
                spawn_local(async move {
                    dom::sleep_ms(1700).await;
                    p.flash.try_update(|f| fresh.iter().for_each(|k| { f.remove(k); }));
                });
            }
        }
        now
    });
    // A press outside the panel and its counts lowers it (APP-26's rule, a
    // frame's press included).
    let lower_outside = move |target: Option<web_sys::Element>| {
        let inside = target.is_some_and(|el| el.closest("#qpanel, #qpin, .hq").ok().flatten().is_some());
        if !inside && open.try_get_untracked() == Some(true) && a.busy.try_get_untracked() == Some(false) {
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
    let body = move || {
        if !open.get() {
            return None;
        }
        // The titles, held while the panel is up.
        let holder = cache::Holder::new();
        let asked = RwSignal::new(0u32);
        fetch_titles(holder.clone(), asked);
        let h2 = holder.clone();
        Effect::new(move |_| {
            let missing = all.with(|gs| gs.iter().any(|g| !h2.holds(&g.ticket)));
            if missing && asked.get_untracked() < 8 {
                fetch_titles(h2.clone(), asked);
            }
        });
        let now = queue::now();
        let list = move || {
            let gs = all.get();
            if gs.is_empty() {
                return view! { <p class="empty">"Nothing is waiting on you. A question asked in any ticket shows here."</p> }.into_any();
            }
            let rows = tray.get();
            let me = p.me.get();
            gs.into_iter()
                .map(|g| {
                    let id = g.ticket.clone();
                    let title = {
                        let id = id.clone();
                        move || cache::find(&id).map(|r| r.with(|r| r.title.clone())).unwrap_or_default()
                    };
                    let more = if g.open == 0 { "answered".to_string() } else { format!("{} open", g.open) };
                    let lines = g
                        .inputs
                        .into_iter()
                        .map(|i| {
                            let q = queued_for(&rows, &i).cloned();
                            let state = pin_state(&rows, &i, me.as_deref());
                            row_view(i, a, p, now, q, state)
                        })
                        .collect_view();
                    view! {
                        <div class="ag" data-ticket-id=id.clone()>
                            <div class="agh">
                                <a class="id" href=format!("#/ticket/{id}") on:click=move |_| open.set(false)>{id.clone()}</a>
                                <span class="t">{title}</span>
                                <span class="n">{more}</span>
                            </div>
                            {lines}
                        </div>
                    }
                })
                .collect_view()
                .into_any()
        };
        let tickets = move || all.with(|gs| gs.iter().filter(|g| g.open > 0).count());
        Some(view! {
            <div class="hscrim" on:click=move |_| open.set(false)></div>
            <section
                id="qpanel"
                class="hp"
                role="dialog"
                aria-label="every open question"
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if crate::bindings::is(&ev, crate::bindings::Context::Dialog, "dialog.close") {
                        ev.stop_propagation();
                        open.set(false);
                    }
                }
            >
                <div class="hph">
                    <b>"Questions"</b>
                    <span class="m" id="hpn">{move || heading(n.get().unwrap_or(0), tickets())}</span>
                    <span class="sp"></span>
                    {move || {
                        let k = important.get();
                        (k > 0).then(|| view! {
                            <button
                                type="button"
                                class="qlink"
                                title="the important ones, in the tray"
                                on:click=move |_| {
                                    open.set(false);
                                    queue::want(true);
                                }
                            >
                                <span class="qimp">{k}</span>" important ↓"
                            </button>
                        })
                    }}
                </div>
                <div class="scroll" id="HPL" on:scroll=move |_| {
                    if p.ask.with_untracked(|a| a.as_ref().is_some_and(|a| (list_scroll() - a.scrolled).abs() > 24.0)) {
                        p.ask.set(None);
                    }
                }>{list}</div>
                {move || p.undo.get().map(|(ticket, words)| view! {
                    <div class="qundo" role="status">
                        <span>{words}</span>
                        <span class="sp"></span>
                        <button type="button" data-act="undo" on:click=move |_| unpin(p, ticket.clone())>"undo"</button>
                    </div>
                })}
                <div class="foot">
                    <span>"newest first · answer in place"</span>
                    <span class="sp"></span>
                    <a href="#/inputs" on:click=move |_| open.set(false)>"every input →"</a>
                    <kbd>"esc"</kbd>
                </div>
            </section>
            {pin_popover(p)}
        })
    };
    let place = move || at.get().map(|(top, right)| format!("--hp-top: {top}px; --hp-right: {right}px"));
    view! { <div id="questions" class:up=move || open.get() style=place>{body}</div> }
}

/// Titles by ticket, for the tests' fixtures.
#[cfg(test)]
fn titled(gs: &[Group]) -> Vec<(String, Vec<String>)> {
    gs.iter().map(|g| (g.ticket.clone(), g.inputs.iter().map(|i| i.id.to_string()).collect())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inputs::tests::{answered, input};
    use menger_tickets_api::{InputAction, InputHistoryEntry, AnswerSource};

    fn at(mut i: Input, t: i64) -> Input {
        i.asked_unix = Some(t);
        i
    }

    /// The age is the later of when it was asked and its last entry — an
    /// answer cleared, an input reopened, waits from then.
    #[test]
    fn a_question_waits_from_when_it_was_asked_or_opened_again() {
        let i = at(input("A-1", "x"), 100);
        assert_eq!(since(&i), Some(100));
        let mut cleared = answered(i.clone(), "archive", 150);
        cleared.history.push(InputHistoryEntry { action: InputAction::Cleared, value: None, rationale: None, source: AnswerSource::App, at_unix: 200 });
        cleared.state = InputState::Open;
        assert_eq!(since(&cleared), Some(200));
        let mut old = input("A-1", "y");
        old.asked_unix = None;
        assert_eq!(since(&old), None, "a server before APP-84, no history");
        assert_eq!(since(&answered(old, "archive", 50)), Some(50));
    }

    /// Grouped by ticket, the ticket asked most recently first; each
    /// ticket's in its body's order; only the open, and those answered here.
    #[test]
    fn open_questions_group_by_ticket_newest_first() {
        let list = vec![
            at(input("A-1", "a"), 100),
            at(input("A-1", "b"), 400),
            at(input("B-2", "c"), 300),
            answered(at(input("C-3", "d"), 900), "archive", 950),
            at(input("D-4", "e"), 200),
        ];
        let gs = groups(&list, &HashSet::new());
        assert_eq!(
            titled(&gs),
            [("A-1".into(), vec!["a".to_string(), "b".into()]), ("B-2".into(), vec!["c".into()]), ("D-4".into(), vec!["e".into()])]
        );
        assert_eq!(gs[0].open, 2);
        let kept = HashSet::from(["C-3#d".to_string()]);
        let gs = groups(&list, &kept);
        assert_eq!(gs[0].ticket, "C-3", "answered here: kept, still the newest");
        assert_eq!(gs[0].open, 0);
        let mut closed = list[1].clone();
        closed.state = InputState::Closed;
        assert!(groups(&[closed], &kept).is_empty(), "a closed input is no question");
    }

    #[test]
    fn the_words() {
        assert_eq!(heading(18, 14), "18 open on 14 tickets");
        assert_eq!(heading(1, 1), "1 open on 1 ticket");
        assert_eq!(heading(0, 0), "nothing waiting on you");
        assert_eq!(kind_word(InputKind::Choice), "decision");
        assert_eq!(kind_word(InputKind::Multi), "multi");
        assert_eq!(kind_word(InputKind::Text), "text");
        assert_eq!(count_title(Some(3)), "3 open questions — every one, in a panel (g q)");
        assert_eq!(count_title(Some(1)), "1 open question — every one, in a panel (g q)");
        assert_eq!(key(&input("A-1", "x")), "A-1#x");
    }

    fn entry(ticket: &str, input: Option<&str>) -> queue::Row {
        let mut entry = queue::tests::entry(ticket, Urgency::Now, 0);
        entry.input = input.map(str::to_string);
        queue::Row { entry, ticket: queue::tests::summary(ticket, "t") }
    }

    /// A row is queued when its ticket's entry names it, or names none.
    #[test]
    fn a_row_is_queued_when_its_ticket_is_queued_for_it() {
        let rows = [entry("A-1", Some("x")), entry("B-2", None)];
        assert_eq!(queued_for(&rows, &input("A-1", "x")).map(|e| e.urgency), Some(Urgency::Now));
        assert!(queued_for(&rows, &input("A-1", "y")).is_none(), "another input of a queued ticket");
        assert!(queued_for(&rows, &input("B-2", "z")).is_some(), "the ticket as a whole");
        assert!(queued_for(&rows, &input("C-3", "x")).is_none());
    }

    /// APP-87: a row's pin control — pin it; pinned by the reader (unpin);
    /// queued by an agent (its mark, no pin); its ticket queued for another
    /// question (no pin: a ticket is queued for one input).
    #[test]
    fn a_rows_pin_follows_who_queued_it() {
        let mut mine = entry("A-1", Some("x"));
        mine.entry.by = "key:me".into();
        let agent = entry("B-2", Some("y"));
        let rows = [mine, agent];
        assert_eq!(pin_state(&rows, &input("A-1", "x"), Some("key:me")), PinState::Pinned);
        assert_eq!(pin_state(&rows, &input("A-1", "x"), Some("key:other")), PinState::Queued, "someone else's pin is a queue");
        assert_eq!(pin_state(&rows, &input("A-1", "x"), None), PinState::Queued, "not known yet: never claimed");
        assert_eq!(pin_state(&rows, &input("B-2", "y"), Some("key:me")), PinState::Queued);
        assert_eq!(pin_state(&rows, &input("B-2", "z"), Some("key:me")), PinState::Elsewhere("y".into()));
        assert_eq!(pin_state(&rows, &input("C-3", "q"), Some("key:me")), PinState::Pin);
    }

    #[test]
    fn a_pin_asks_an_urgency_and_a_note() {
        assert_eq!(PIN_URGENCIES, [Urgency::Whenever, Urgency::Today, Urgency::Now], "never blocking");
        assert_eq!(PIN_DEFAULT, Urgency::Now);
        assert_eq!(pin_reason("  "), "Pinned from all questions.");
        assert_eq!(pin_reason(" look before the firing "), "look before the firing");
        assert_eq!(pinned_words("KILN-4", "unit", Urgency::Today), "KILN-4 · unit pinned: important, today");
    }

    /// An agent's queue heard while the panel is up: its row is new among
    /// the queued, and flashes; one that was queued already does not.
    #[test]
    fn a_question_queued_live_is_the_one_that_flashes() {
        let list = [input("A-1", "x"), input("B-2", "y"), answered(input("C-3", "z"), "archive", 1)];
        let before = queued_keys(&list, &[entry("A-1", Some("x"))]);
        let after = queued_keys(&list, &[entry("A-1", Some("x")), entry("B-2", None), entry("C-3", None)]);
        assert_eq!(before, HashSet::from(["A-1#x".to_string()]));
        assert_eq!(arrived(&before, &after), ["B-2#y"], "an answered one is no question");
        assert!(arrived(&after, &after).is_empty());
    }
}
