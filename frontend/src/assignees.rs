//! TICKETS-9: a ticket's assignees in the app — avatars on its rows, on
//! its page, and the person picker that assigns.
//!
//! - **An avatar** ([`avatar`]) is one principal: its initials in a small
//!   square (an agent's round, as the chats draw them), its name beside it
//!   where there is room; its title says the principal.
//! - **On a board row** ([`row_avatars`]): up to three, overlapped, then
//!   `+n` — live, as the row is (`assignees_changed` reaches the row cache).
//! - **On the ticket page** ([`Editor`]): each assignee with × to unassign,
//!   and **+ assign**, which opens the person picker ([`Picker`]): the
//!   reader first ("me"), then everyone `chats.candidates` names — who holds
//!   a grant in the ticket's organization, people and agents — those who
//!   can read the ticket first; one who cannot is drawn but not offered (the
//!   server would refuse them, naming why). For a person who may write; a
//!   reader sees the avatars alone. Each change is one `tickets.assign`; what
//!   the server refuses is shown in its words.
//!
//! The picker is CHAT-14's agent-aware candidates (`chats.candidates {
//! ticket }`): each one's name, and an agent marked (round).

use leptos::prelude::*;
use menger_tickets_api::{TicketSummary, TicketsEvent};
use wasm_bindgen_futures::spawn_local;

use crate::{chats, conversation, session};

/// How many avatars a row draws before `+n`.
pub(crate) const ROW_MAX: usize = 3;

/// A row's avatars and what is left over: the first [`ROW_MAX`], and how
/// many more.
pub(crate) fn shown(all: &[String]) -> (&[String], usize) {
    let n = all.len().min(ROW_MAX);
    (&all[..n], all.len() - n)
}

/// The filter a person's tickets are: `assigned "<principal>"` — quoted, as
/// the language writes a principal.
pub(crate) fn filter_of(principal: &str) -> String {
    menger_tickets_api::filter::History {
        event: menger_tickets_api::filter::Event::Assigned(Some(menger_tickets_api::filter::Who::read(principal))),
        window: None,
        by: None,
    }
    .format()
}

/// The board's link to a person's tickets.
pub(crate) fn board_link(principal: &str) -> String {
    format!("#/?f={}", crate::lens::enc(&filter_of(principal)))
}

/// One principal as an avatar — `named`: its name beside it.
pub(crate) fn avatar(principal: &str, name: &str, named: bool) -> impl IntoView {
    view! {
        <span class="aav-w" data-principal=principal.to_string() title=format!("{name} · {principal}")>
            <span class="aav" class:agent=chats::is_agent(principal) aria-hidden="true">{conversation::initials(name)}</span>
            {named.then(|| view! { <span class="aname">{name.to_string()}</span> })}
        </span>
    }
}

/// A row's assignees, live — up to three avatars, then `+n`; nothing when
/// it has none.
pub(crate) fn row_avatars(assignees: impl Fn() -> Vec<String> + Send + Sync + 'static) -> impl IntoView {
    let name = crate::chat_pane::names();
    move || {
        let all = assignees();
        let (first, more) = shown(&all);
        let title = all.iter().map(|p| name(p)).collect::<Vec<_>>().join(", ");
        (!all.is_empty()).then(|| {
            view! {
                <span class="aavs" title=title aria-label=format!("assigned: {}", all.len())>
                    {first.iter().map(|p| avatar(p, &name(p), false)).collect_view()}
                    {(more > 0).then(|| view! { <span class="amore">{format!("+{more}")}</span> })}
                </span>
            }
        })
    }
}

/// `tickets.assign { id, add, remove }`, with what went wrong.
pub(crate) async fn assign(id: String, add: Vec<String>, remove: Vec<String>) -> Result<(), String> {
    let client = session::client().ok_or_else(|| "not connected".to_string())?;
    let add = (!add.is_empty()).then_some(crate::typed::many(add)?);
    let remove = (!remove.is_empty()).then_some(crate::typed::many(remove)?);
    let source = Some(crate::typed::typed("app")?);
    let events = client.assign_all(Some(crate::typed::typed(&id)?), None, add, remove, source, None).await.map_err(|e| session::describe(&e))?;
    for ev in events {
        match ev {
            TicketsEvent::AssigneesChanged { ticket, .. } => {
                crate::cache::put(TicketSummary::from(&ticket));
                return Ok(());
            }
            TicketsEvent::NotFound { id } => return Err(format!("{id}: not found")),
            TicketsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Err("no answer".into())
}

/// The picker's rows: the reader first (when not assigned already), then
/// the candidates [`crate::chat_pane::rows`] keeps — those who read the
/// ticket first, then by name.
pub(crate) fn picker_rows(cands: &[(String, bool)], me: &str, held: &[String], query: &str, name: impl Fn(&str) -> String) -> Vec<(String, bool)> {
    let q = query.trim().to_lowercase();
    let mine = !held.iter().any(|p| p == me) && me != "anonymous" && (q.is_empty() || "me".contains(&q) || me.to_lowercase().contains(&q) || name(me).to_lowercase().contains(&q));
    let mut out: Vec<(String, bool)> = mine.then(|| (me.to_string(), true)).into_iter().collect();
    out.extend(crate::chat_pane::rows(cands, me, held, query, name));
    out
}

/// The person picker: who may be assigned `ticket` — the reader, then the
/// candidates — filtered as typed; a pick is `on_pick(principal)`.
#[component]
pub(crate) fn Picker(ticket: String, held: Vec<String>, on_pick: Callback<String>, on_close: Callback<()>) -> impl IntoView {
    let cands = RwSignal::new(None::<Result<Vec<(String, bool)>, String>>);
    {
        let t = ticket.clone();
        spawn_local(async move {
            let r = chats::candidates(Some(&t)).await;
            cands.try_set(Some(r));
        });
    }
    let query = RwSignal::new(String::new());
    let me = chats::me();
    let name = crate::chat_pane::names();
    let list = move || match cands.get() {
        None => view! { <p class="pempty">"asking who can read it…"</p> }.into_any(),
        Some(Err(e)) => view! { <p class="perr" role="alert">{e}</p> }.into_any(),
        Some(Ok(all)) => {
            let rows = picker_rows(&all, &me, &held, &query.get(), &name);
            if rows.is_empty() {
                return view! { <p class="pempty">"Everyone who holds a grant here is assigned."</p> }.into_any();
            }
            let items = rows
                .into_iter()
                .map(|(p, reads)| {
                    let who = if p == me { format!("{} (me)", name(&p)) } else { name(&p) };
                    let what = if reads { format!("can read {ticket}") } else { format!("can't read {ticket}, so can't be assigned") };
                    let pc = p.clone();
                    view! {
                        <li
                            class="acand"
                            class:no=!reads
                            role="option"
                            aria-disabled=(!reads).to_string()
                            data-principal=p.clone()
                            on:click=move |_| {
                                if reads {
                                    on_pick.run(pc.clone());
                                }
                            }
                        >
                            <span class="aav" class:agent=chats::is_agent(&p) aria-hidden="true">{conversation::initials(&name(&p))}</span>
                            <div>{who}<small class:refuse=!reads>{what}</small></div>
                        </li>
                    }
                })
                .collect_view();
            view! { <ul class="acands" role="listbox" aria-label="people and agents">{items}</ul> }.into_any()
        }
    };
    view! {
        <div class="apicker" id="assign-picker" on:keydown=move |ev| {
            if crate::bindings::is(&ev, crate::bindings::Context::Field, "form.cancel") {
                on_close.run(());
            }
        }>
            <input
                class="aq"
                type="text"
                placeholder="assign to…"
                aria-label="find a person or agent"
                autofocus=true
                prop:value=move || query.get()
                on:input=move |ev| query.set(event_target_value(&ev))
            />
            {list}
        </div>
    }
}

/// The ticket page's assignees: each one, and — for a person who may write
/// — × on each and **+ assign**.
#[component]
pub(crate) fn Editor(ticket: RwSignal<TicketSummary>) -> impl IntoView {
    let held = Memo::new(move |_| ticket.with(|t| t.assignees.clone()));
    let problem = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let picking = RwSignal::new(false);
    let writes = Memo::new(move |_| crate::labels::may_write());
    let name = crate::chat_pane::names();
    let send = move |add: Vec<String>, remove: Vec<String>| {
        let id = ticket.with_untracked(|t| t.id.to_string());
        busy.set(true);
        problem.set(None);
        spawn_local(async move {
            let done = assign(id, add, remove).await;
            busy.try_set(false);
            if let Err(why) = done {
                problem.try_set(Some(why));
            }
        });
    };
    let pick = Callback::new(move |p: String| {
        picking.set(false);
        send(vec![p], Vec::new());
    });
    let close = Callback::new(move |()| picking.set(false));
    view! {
        <span class="achips" id="t-assignees-chips">
            {move || {
                let w = writes.get();
                let all = held.get();
                let none = all.is_empty();
                let chips = all
                    .into_iter()
                    .map(|p| {
                        let gone = p.clone();
                        let who = name(&p);
                        let x = w.then(|| {
                            view! {
                                <button
                                    class="lx"
                                    title=format!("unassign {who}")
                                    aria-label=format!("unassign {who}")
                                    disabled=move || busy.get()
                                    on:click=move |_| send(Vec::new(), vec![gone.clone()])
                                >
                                    "×"
                                </button>
                            }
                        });
                        view! { <span class="awrap">{avatar(&p, &name(&p), true)}{x}</span> }
                    })
                    .collect_view();
                view! { {chips}{(none && !w).then_some("—")} }
            }}
            {move || writes.get().then(|| view! {
                <button
                    class="hbtn aadd"
                    id="t-assign"
                    aria-haspopup="listbox"
                    aria-expanded=move || picking.get().to_string()
                    disabled=move || busy.get()
                    on:click=move |_| picking.update(|p| *p = !*p)
                >
                    "+ assign"
                </button>
            })}
        </span>
        {move || picking.get().then(|| {
            let (t, h) = ticket.with_untracked(|t| (t.id.to_string(), t.assignees.clone()));
            view! { <Picker ticket=t held=h on_pick=pick on_close=close /> }
        })}
        {move || problem.get().map(|p| view! { <p class="lproblem" role="alert" id="t-assign-problem">{p}</p> })}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// A row draws three, then says how many more.
    #[test]
    fn a_row_draws_three_then_more() {
        let all = strs(&["a", "b", "c", "d", "e"]);
        assert_eq!(shown(&all), (&all[..3], 2));
        assert_eq!(shown(&all[..2]), (&all[..2], 0));
    }

    /// A person's tickets are the cell `assigned "<principal>"`, as the
    /// language reads it back.
    #[test]
    fn a_persons_filter_is_the_assigned_cell() {
        let f = filter_of(ADA);
        assert_eq!(f, format!("assigned {ADA}"));
        assert_eq!(menger_tickets_api::filter::Filter::parse(&f).unwrap().format(), f);
        assert!(board_link(ADA).starts_with("#/?f=assigned"));
    }

    /// The reader comes first unless assigned already; who is assigned is
    /// never offered again; who can't read comes last.
    #[test]
    fn the_picker_offers_me_first_then_who_reads() {
        let cands = vec![("key:b".to_string(), false), ("key:c".to_string(), true), ("key:me".to_string(), true)];
        let name = |p: &str| p.to_string();
        let rows = picker_rows(&cands, "key:me", &[], "", name);
        assert_eq!(rows, vec![("key:me".to_string(), true), ("key:c".to_string(), true), ("key:b".to_string(), false)]);
        let rows = picker_rows(&cands, "key:me", &strs(&["key:me", "key:c"]), "", name);
        assert_eq!(rows, vec![("key:b".to_string(), false)]);
        let rows = picker_rows(&cands, "key:me", &[], "c", name);
        assert_eq!(rows, vec![("key:c".to_string(), true)]);
    }
}
