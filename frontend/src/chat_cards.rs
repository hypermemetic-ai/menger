//! CHAT-56: **ticket cards that act** — CHAT-35's second build, after
//! CHAT-55's strip (owner, chat 6, 2026-10-19: *"start the work that turns
//! this chat into a ticket surface"*).
//!
//! A message that names a ticket on a line of its own ([`lone_codes`]: the
//! builder's "I filed" line, `/ticket CODE`) gets a **card** under it: the
//! id, title and status, where it runs per environment
//! ([`crate::deploys::row_marks`]), and its open questions, answered on the
//! card (`tickets inputs answer`; the input stays the agent's to close). Its
//! actions follow its state ([`actions`]): *Go* (Pending → Ready, what
//! "start it" means), *Drop* (→ Canceled, with 10 s to undo), *Open*, and
//! *Ask again* (`@builder about ID: ` into the composer). A message naming
//! several Pending tickets also gets **Go all** ([`go_all`]).
//!
//! Every card is a row-cache row, so a status the watch moves moves the
//! card, and an answer moves its question ([`crate::inputs`]). A ticket the
//! reader cannot read is no card: its code stays the plain chip in the
//! message. On a phone a card is the turn's width, its actions one row (⋯
//! for the rest) and its questions folded to "N questions" until tapped; at
//! 1440 at most 560 px.

use crate::cache::{Holder, RowSignal};
use leptos::prelude::*;
use leptos::task::spawn_local;
use menger_tickets_api::chat_tickets::{is_code, mentions_in};
use menger_tickets_api::{Input, InputKind, InputState, InputValue, Kind, Status, TicketSummary, TicketsEvent};
use std::collections::{BTreeMap, BTreeSet};

/// How long a drop can be taken back.
pub(crate) const UNDO_MS: i32 = 10_000;

/// The codes a body names on lines of their own — a line that is the code
/// alone (in bold or code, or a list item, too), outside fenced blocks;
/// each once, in order.
pub(crate) fn lone_codes(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut fence: Option<&str> = None;
    for line in body.lines() {
        let t = line.trim();
        if let Some(m) = ["```", "~~~"].into_iter().find(|m| t.starts_with(m)) {
            fence = match fence {
                None => Some(m),
                Some(f) if f == m => None,
                f => f,
            };
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let t = t.trim_start_matches(['-', '*', '+']).trim_start();
        let t = t.trim_matches(|c| matches!(c, '*' | '_' | '`')).trim();
        if is_code(t) && !out.iter().any(|o| o == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// What a card offers, in the order drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    Go,
    Drop,
    Open,
    Ask,
}

impl Act {
    pub(crate) const fn words(self) -> &'static str {
        match self {
            Act::Go => "go",
            Act::Drop => "drop",
            Act::Open => "open",
            Act::Ask => "ask again",
        }
    }
}

/// A card's actions for a ticket of `kind` in `status`: *Go* while it is
/// Pending; *Drop* while it is open work; always *Open* and *Ask again*.
pub(crate) fn actions(kind: Kind, status: Status) -> Vec<Act> {
    let work = !matches!(kind, Kind::Doc | Kind::Scope);
    let open = !matches!(status, Status::Complete | Status::Canceled | Status::Superseded | Status::Doc);
    let mut out = Vec::new();
    if work && status == Status::Pending {
        out.push(Act::Go);
    }
    if work && open {
        out.push(Act::Drop);
    }
    out.extend([Act::Open, Act::Ask]);
    out
}

/// The tickets *Go all* would start: the Pending work among `rows` — when
/// there are two or more of them; none otherwise.
pub(crate) fn go_all(rows: &[(String, Kind, Status)]) -> Vec<String> {
    let pending: Vec<String> = rows.iter().filter(|(_, k, s)| *s == Status::Pending && !matches!(k, Kind::Doc | Kind::Scope)).map(|(id, ..)| id.clone()).collect();
    if pending.len() >= 2 {
        pending
    } else {
        Vec::new()
    }
}

/// The questions a card shows: the ticket's inputs still open or answered
/// (not yet closed), in the body's order.
pub(crate) fn questions(all: &[Input]) -> Vec<Input> {
    all.iter().filter(|i| matches!(i.state, InputState::Open | InputState::Answered)).cloned().collect()
}

/// The text "@builder about ID: " the composer gets from *Ask again*.
pub(crate) fn ask_text(id: &str) -> String {
    format!("@builder about {id}: ")
}

// -- the wire ------------------------------------------------------------------

/// The first in-band refusal among a write's answers.
fn refusal(events: &[TicketsEvent]) -> Option<String> {
    events.iter().find_map(|e| match e {
        TicketsEvent::Error { message } => Some(message.clone()),
        TicketsEvent::NotFound { id } => Some(format!("{id}: not found")),
        _ => None,
    })
}

/// `tickets.set_status` from the app (source `app`); leaving Canceled puts
/// back what that cancel canceled.
async fn set_status(id: &str, status: Status, restore: bool) -> Result<(), String> {
    let client = crate::session::client().ok_or("not connected — nothing was sent")?;
    let t = crate::typed::typed(id)?;
    let events = client.set_status_all(t, status, Some(restore), Some(menger_tickets_api::vocab::CallerSource::App), None).await.map_err(|e| crate::session::describe(&e))?;
    refusal(&events).map_or(Ok(()), Err)
}

/// *Go all*: the tickets to Ready in one write, all or none.
async fn go_many(ids: &[String]) -> Result<(), String> {
    let client = crate::session::client().ok_or("not connected — nothing was sent")?;
    let ids = ids.iter().map(|i| crate::typed::typed(i)).collect::<Result<Vec<_>, String>>()?;
    let events = client.update_many_all(Some(ids), None, None, None, None, Some(Status::Ready), None, None, None, None, None, None, None, None, Some(menger_tickets_api::vocab::CallerSource::App), None).await.map_err(|e| crate::session::describe(&e))?;
    refusal(&events).map_or(Ok(()), Err)
}

/// `tickets.inputs.answer`: the source is the server's, from the caller.
async fn answer(ticket: &str, input: &str, value: InputValue) -> Result<(), String> {
    let client = crate::session::client().ok_or("not connected — nothing was sent")?;
    let (t, i) = (crate::typed::typed(ticket)?, crate::typed::typed(input)?);
    let events = client.inputs().answer_all(t, i, value, None).await.map_err(|e| crate::session::describe(&e))?;
    refusal(&events).map_or(Ok(()), Err)
}

/// A code the reader cannot read, asked once: no card for it again.
fn hidden() -> ArcRwSignal<BTreeSet<String>> {
    thread_local! { static HIDDEN: ArcRwSignal<BTreeSet<String>> = ArcRwSignal::new(BTreeSet::new()); }
    HIDDEN.with(Clone::clone)
}

/// `code`'s row, held by `holder` — the cache's when it has it, else asked
/// with `tickets.get` (a code the reader cannot read is [`hidden`]).
fn resolve(code: String, holder: Holder, rows: RwSignal<BTreeMap<String, RowSignal>>) {
    if crate::cache::get(&code).is_some() && holder.hold_id(&code) {
        if let Some(r) = crate::cache::get(&code) {
            rows.update(|m| {
                m.insert(code, r);
            });
        }
        return;
    }
    if hidden().with_untracked(|h| h.contains(&code)) {
        return;
    }
    spawn_local(async move {
        let Some(client) = crate::session::client() else { return };
        let Ok(t) = crate::typed::typed(&code) else { return };
        let Ok(events) = client.get_all(t).await else { return };
        for ev in events {
            match ev {
                TicketsEvent::Ticket { ticket } => {
                    if let Some(r) = holder.hold(TicketSummary::from(ticket)) {
                        rows.try_update(|m| {
                            m.insert(code.clone(), r);
                        });
                    }
                    return;
                }
                TicketsEvent::NotFound { .. } | TicketsEvent::Error { .. } => {
                    hidden().update(|h| {
                        h.insert(code.clone());
                    });
                    return;
                }
                _ => {}
            }
        }
    });
}

// -- the views -----------------------------------------------------------------

/// The cards under a turn whose body is `body`: one per ticket it names on a
/// line of its own, and *Go all* when it names several Pending tickets.
/// Nothing when it names none.
#[component]
pub(crate) fn Cards(body: String) -> impl IntoView {
    let lone = lone_codes(&body);
    let named = mentions_in(&body);
    let lone_set: BTreeSet<String> = lone.iter().cloned().collect();
    let rows: RwSignal<BTreeMap<String, RowSignal>> = RwSignal::new(BTreeMap::new());
    if !named.is_empty() {
        crate::inputs::want();
        let holder = Holder::new();
        for code in named.iter().filter(|c| lone_set.contains(*c) || named.len() > 1) {
            resolve(code.clone(), holder.clone(), rows);
        }
    }
    let all = Memo::new(move |_| rows.with(|m| m.values().map(|r| r.with(|t| (t.id.to_string(), t.kind, t.status))).collect::<Vec<_>>()));
    let many = Memo::new(move |_| go_all(&all.get()));
    let err = RwSignal::new(None::<String>);
    let going = RwSignal::new(false);
    let go = move |_| {
        let ids = many.get_untracked();
        going.set(true);
        spawn_local(async move {
            let r = go_many(&ids).await;
            going.try_set(false);
            err.try_set(r.err());
        });
    };
    let cards = move || {
        lone.iter()
            .filter_map(|c| rows.with(|m| m.get(c).cloned()))
            .map(|row| view! { <Card row /> })
            .collect_view()
    };
    view! {
        <div class="tkcards">
            {cards}
            {move || {
                let n = many.with(Vec::len);
                (n > 0).then(|| view! {
                    <div class="tkall">
                        <button type="button" class="qa go" disabled=move || going.get() on:click=go>{format!("go all {n}")}</button>
                        <span class="tkallw">{move || many.get().join(" · ")}</span>
                    </div>
                })
            }}
            {move || err.get().map(|e| view! { <p class="tkerr" role="alert">{e}</p> })}
        </div>
    }
}

/// One ticket's card.
#[component]
fn Card(row: RowSignal) -> impl IntoView {
    let row = RwSignal::from(row);
    let id = row.with_untracked(|t| t.id.to_string());
    let href = format!("#/ticket/{id}");
    let status = Memo::new(move |_| row.with(|t| t.status));
    let kind = Memo::new(move |_| row.with(|t| t.kind));
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    // A drop: the status it left, until the undo runs out.
    let undo = RwSignal::new(None::<(Status, u32)>);
    let undos = StoredValue::new(0u32);
    let all = RwSignal::new(false);
    let write = move |id: String, to: Status, restore: bool, after: Option<Status>| {
        busy.set(true);
        spawn_local(async move {
            let r = set_status(&id, to, restore).await;
            busy.try_set(false);
            match (r, after) {
                (Err(e), _) => {
                    err.try_set(Some(e));
                }
                (Ok(()), Some(was)) => {
                    err.try_set(None);
                    undos.update_value(|n| *n += 1);
                    let mine = undos.get_value();
                    undo.try_set(Some((was, mine)));
                    crate::dom::sleep_ms(UNDO_MS).await;
                    if undo.try_get_untracked().flatten().is_some_and(|(_, n)| n == mine) {
                        undo.try_set(None);
                    }
                }
                (Ok(()), None) => {
                    err.try_set(None);
                    undo.try_set(None);
                }
            }
        });
    };
    let acts = {
        let id = id.clone();
        let href = href.clone();
        move || {
            let list = actions(kind.get(), status.get());
            let n = list.len();
            let id = id.clone();
            let href = href.clone();
            let items = list
                .into_iter()
                .enumerate()
                .map(|(i, a)| {
                    let rest = i >= 2;
                    let id = id.clone();
                    match a {
                        Act::Open => view! { <a class="qa" class:rest=rest href=href.clone()>{a.words()}</a> }.into_any(),
                        Act::Ask => view! { <button type="button" class="qa mut" class:rest=rest data-act="ask" data-ticket=id data-kb-keep="">{a.words()}</button> }.into_any(),
                        Act::Go => view! { <button type="button" class="qa go prim" class:rest=rest disabled=move || busy.get() on:click=move |_| write(id.clone(), Status::Ready, false, None)>{a.words()}</button> }.into_any(),
                        Act::Drop => view! {
                            <button type="button" class="qa warn" class:rest=rest disabled=move || busy.get()
                                on:click=move |_| write(id.clone(), Status::Canceled, false, Some(status.get_untracked()))>{a.words()}</button>
                        }
                        .into_any(),
                    }
                })
                .collect_view();
            view! {
                {items}
                {(n > 2).then(|| view! { <button type="button" class="qa mut tkdots" aria-label="more" aria-expanded=move || all.get().to_string() on:click=move |_| all.update(|a| *a = !*a)>"⋯"</button> })}
            }
        }
    };
    let undo_line = {
        let id = id.clone();
        move || {
            undo.get().map(|(was, _)| {
                let id = id.clone();
                view! {
                    <div class="tkundo" role="status">
                        <span>{format!("{id} dropped")}</span>
                        <button type="button" class="qa" disabled=move || busy.get() on:click=move |_| write(id.clone(), was, true, None)>"undo"</button>
                    </div>
                }
            })
        }
    };
    let qs = {
        let id = id.clone();
        Memo::new(move |_| crate::inputs::signal().with(|i| i.as_ref().map(|i| questions(&i.of_ticket(&id))).unwrap_or_default()))
    };
    let wide = crate::dom::window().inner_width().ok().and_then(|w| w.as_f64()).is_some_and(|w| w > 760.0);
    let questions_view = move || {
        let list = qs.get();
        let n = list.len();
        (n > 0).then(|| {
            let open_n = list.iter().filter(|i| i.state == InputState::Open).count();
            let items = list.into_iter().map(|i| view! { <Question input=i /> }).collect_view();
            let label = match (n, open_n) {
                (1, 1) => "1 question".to_string(),
                (1, _) => "1 question, answered".to_string(),
                (n, 0) => format!("{n} questions, answered"),
                (n, o) if o == n => format!("{n} questions"),
                (n, o) => format!("{n} questions, {o} open"),
            };
            view! {
                <details class="tkqs" open=wide>
                    <summary>{label}</summary>
                    {items}
                </details>
            }
        })
    };
    let title = move || row.with(|t| t.title.clone());
    let status_words = move || status.get().to_string();
    view! {
        <div class="tkcard" data-ticket=id.clone() data-status=move || status_words()>
            <div class="tkh">
                <a class="tchip" href=href.clone()>{id.clone()}</a>
                <a class="tkt" href=href.clone()>{title}</a>
                <span class=move || crate::st_class(&status_words())>{status_words}</span>
            </div>
            <div class="tkm">{crate::deploys::row_marks(id.clone())}</div>
            {questions_view}
            <div class="tka" class:all=move || all.get() role="group" aria-label=format!("{id}: actions")>{acts}</div>
            {undo_line}
            {move || err.get().map(|e| view! { <p class="tkerr" role="alert">{e}</p> })}
        </div>
    }
}

/// One question on a card: its options (the recommended marked) — a tap
/// answers; a multi's ticks and a text's reply are sent with *answer*.
#[component]
fn Question(input: Input) -> impl IntoView {
    let ticket = input.ticket.to_string();
    let key = input.id.to_string();
    let answered: Option<InputValue> = input.answer.as_ref().map(|a| a.value.clone());
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let send = {
        let (ticket, key) = (ticket.clone(), key.clone());
        move |v: InputValue| {
            let (ticket, key) = (ticket.clone(), key.clone());
            busy.set(true);
            spawn_local(async move {
                let r = answer(&ticket, &key, v).await;
                busy.try_set(false);
                err.try_set(r.err());
            });
        }
    };
    let chosen = |v: &str| match &answered {
        Some(InputValue::Choice { option }) => option == v,
        Some(InputValue::Multi { options }) => options.iter().any(|o| o == v),
        _ => false,
    };
    let body = match input.kind {
        InputKind::Choice => input
            .options
            .iter()
            .map(|o| {
                let v = o.value.clone();
                let send = send.clone();
                view! {
                    <button type="button" class="tkopt" class:on=chosen(&o.value) aria-pressed=chosen(&o.value).to_string() disabled=move || busy.get()
                        on:click=move |_| send(InputValue::Choice { option: v.clone() })>
                        {o.label.clone()}
                        {o.recommended.then(|| view! { <span class="rec">"recommended"</span> })}
                    </button>
                }
            })
            .collect_view()
            .into_any(),
        InputKind::Multi => {
            let ticks = RwSignal::new(input.options.iter().filter(|o| chosen(&o.value)).map(|o| o.value.clone()).collect::<Vec<_>>());
            let order: Vec<String> = input.options.iter().map(|o| o.value.clone()).collect();
            let boxes = input
                .options
                .iter()
                .map(|o| {
                    let v = o.value.clone();
                    let on = chosen(&o.value);
                    view! {
                        <label class="tkopt">
                            <input type="checkbox" checked=on on:change=move |_| ticks.update(|t| if let Some(i) = t.iter().position(|x| *x == v) { t.remove(i); } else { t.push(v.clone()); }) />
                            {o.label.clone()}
                            {o.recommended.then(|| view! { <span class="rec">"recommended"</span> })}
                        </label>
                    }
                })
                .collect_view();
            let send = send.clone();
            view! {
                {boxes}
                <button type="button" class="qa go" disabled=move || busy.get() on:click=move |_| {
                    let t = ticks.get_untracked();
                    send(InputValue::Multi { options: order.iter().filter(|o| t.contains(o)).cloned().collect() });
                }>"answer"</button>
            }
            .into_any()
        }
        InputKind::Text => {
            let text = RwSignal::new(match &answered {
                Some(InputValue::Text { text }) => text.clone(),
                _ => String::new(),
            });
            let send = send.clone();
            view! {
                <div class="tktext">
                    <input type="text" aria-label="your reply" prop:value=move || text.get() on:input=move |e| text.set(event_target_value(&e)) />
                    <button type="button" class="qa go" disabled=move || busy.get() || text.with(|t| t.trim().is_empty())
                        on:click=move |_| send(InputValue::Text { text: text.get_untracked().trim().to_string() })>"send"</button>
                </div>
            }
            .into_any()
        }
    };
    let state = match (input.state, &answered) {
        (InputState::Answered, Some(v)) => Some(format!("answered: {}", words(v, &input))),
        _ => None,
    };
    view! {
        <div class="tkq" data-input=key.clone() data-state=format!("{:?}", input.state).to_lowercase()>
            <p class="tkqq">{input.question.clone().unwrap_or_else(|| key.clone())}</p>
            <div class="tkopts">{body}</div>
            {input.recommendation.clone().map(|r| view! { <p class="tkrec">{r}</p> })}
            {state.map(|s| view! { <p class="tkans">{s}</p> })}
            {move || err.get().map(|e| view! { <p class="tkerr" role="alert">{e}</p> })}
        </div>
    }
}

/// An answer in words: the options' labels, or the reply.
fn words(v: &InputValue, input: &Input) -> String {
    let label = |x: &str| input.options.iter().find(|o| o.value == x).map_or_else(|| x.to_string(), |o| o.label.clone());
    match v {
        InputValue::Choice { option } => label(option),
        InputValue::Multi { options } if options.is_empty() => "none".to_string(),
        InputValue::Multi { options } => options.iter().map(|o| label(o)).collect::<Vec<_>>().join(", "),
        InputValue::Text { text } => text.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_on_its_own_line_is_a_card() {
        let body = "I filed\n\n**DEPLOY-42**\n\n- KILN-7\n`KILN-8`\nsee KILN-9 too\n```\nKILN-10\n```\nDEPLOY-42";
        assert_eq!(lone_codes(body), ["DEPLOY-42", "KILN-7", "KILN-8"]);
        assert!(lone_codes("Filed KILN-2 and KILN-3.").is_empty(), "named in a sentence: chips only");
    }

    #[test]
    fn the_actions_follow_the_state() {
        use Act::*;
        assert_eq!(actions(Kind::Build, Status::Pending), [Go, Drop, Open, Ask]);
        assert_eq!(actions(Kind::Build, Status::Coding), [Drop, Open, Ask]);
        assert_eq!(actions(Kind::Build, Status::Complete), [Open, Ask]);
        assert_eq!(actions(Kind::Build, Status::Canceled), [Open, Ask], "dropped: nothing to go or drop");
        assert_eq!(actions(Kind::Scope, Status::Pending), [Open, Ask], "a scope is not started or dropped from a card");
    }

    #[test]
    fn go_all_takes_two_or_more_pending() {
        let r = |id: &str, k, s| (id.to_string(), k, s);
        assert_eq!(go_all(&[r("A-1", Kind::Build, Status::Pending), r("A-2", Kind::Gap, Status::Pending), r("A-3", Kind::Build, Status::Ready)]), ["A-1", "A-2"]);
        assert!(go_all(&[r("A-1", Kind::Build, Status::Pending), r("A-2", Kind::Build, Status::Ready)]).is_empty(), "one: its card's go");
        assert!(go_all(&[r("A-1", Kind::Scope, Status::Pending), r("A-2", Kind::Build, Status::Pending)]).is_empty());
    }

    #[test]
    fn ask_again_names_the_ticket() {
        assert_eq!(ask_text("KILN-7"), "@builder about KILN-7: ");
    }
}
