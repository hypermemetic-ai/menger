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
use menger_tickets_api::{Input, InputKind, InputState, QueueEntry, TicketsEvent};
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

/// One row: its kind, its question, its age, whether it is queued — and,
/// picked, its answer in place.
fn row_view(input: Input, a: Answering, now: i64, queued: Option<QueueEntry>) -> impl IntoView {
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
    let st = queued.as_ref().map(|e| {
        let u = e.urgency.as_str();
        view! { <span class=format!("chip-u u-{u}")>{u}</span><span>{format!("queued by {}", queue::who(e))}</span> }
    });
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
    view! {
        <button
            type="button"
            class="ar"
            class:sel=move || picked.get()
            class:done=done
            data-key=k.clone()
            aria-expanded=move || picked.get().to_string()
            on:click=move |_| flip()
        >
            <span class="kind">{kind_word(input.kind)}</span>
            <span class="q">{question}</span>
            <span class="side">{age}</span>
            <span class="st">{st}{ok}</span>
        </button>
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
        }
    });
    let all = Memo::new(move |_| {
        let kept = a.kept.get();
        held.with(|h| h.as_ref().map(|h| groups(h.list(), &kept)).unwrap_or_default())
    });
    let n = count();
    let important = Memo::new(move |_| qheld.with(|q| q.as_ref().map_or(0, |q| q.showing(queue::now()).len())));
    // A press outside the panel and its counts lowers it (APP-26's rule, a
    // frame's press included).
    let lower_outside = move |target: Option<web_sys::Element>| {
        let inside = target.is_some_and(|el| el.closest("#qpanel, .hq").ok().flatten().is_some());
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
            let rows = qheld.with(|q| q.as_ref().map(|q| q.showing(queue::now())).unwrap_or_default());
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
                            row_view(i, a, now, q)
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
                                {format!("{k} important ↓")}
                            </button>
                        })
                    }}
                </div>
                <div class="scroll" id="HPL">{list}</div>
                <div class="foot">
                    <span>"newest first · answer in place"</span>
                    <span class="sp"></span>
                    <a href="#/inputs" on:click=move |_| open.set(false)>"every input →"</a>
                    <kbd>"esc"</kbd>
                </div>
            </section>
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
    use menger_tickets_api::{InputAction, InputHistoryEntry, AnswerSource, Urgency};

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
}
