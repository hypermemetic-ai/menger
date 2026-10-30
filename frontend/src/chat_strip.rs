//! CHAT-55: **the chat's ticket strip** — CHAT-35's B, the owner's pick
//! (`direction` = strip; chat 6, 2026-10-19: *"start the work that turns
//! this chat into a ticket surface"*).
//!
//! Under a chat's head, one row of pills, a stage each — *waits on you ·
//! ready · in progress · in review · on test · shipped* — with how many of
//! the chat's tickets stand there; a pill opens its tickets (id, title,
//! status, where it runs) as a drop over the chat: at most half the screen
//! on a phone, closed by a swipe up, the pill again or Escape. No tickets,
//! no strip.
//!
//! **The data** is the server's (`chats.tickets`: every ticket the chat's
//! messages name, and those filed or linked from it, as the reader reads
//! them), asked when the chat opens, again when the chat moves what it
//! names (a post, a link, a delete or a restore — [`heard`]) and on every
//! resync ([`crate::data::again`]). **The stage is live**: each row is held
//! in the row cache, so a status the watch moves moves its pill at once,
//! as do an input answered ([`crate::inputs`]) and a deploy
//! ([`crate::deploys`]) — the same [`stage_of`] the server answers with.

use crate::cache::{Holder, RowSignal};
use crate::deploys::{self, Mark};
use leptos::prelude::*;
use leptos::task::spawn_local;
use menger_tickets_api::chat_tickets::{stage_of, ChatStage, ChatTicket};
use menger_tickets_api::chats::ChatsEvent;
use menger_tickets_api::{InputState, TicketSummary};
use std::collections::BTreeMap;

thread_local! {
    /// Each chat's tickets moved, as a count: a strip asks again when its
    /// chat's moves.
    static MOVED: ArcRwSignal<BTreeMap<u64, u32>> = ArcRwSignal::new(BTreeMap::new());
}

/// The chat whose tickets `ev` may change — a message posted, deleted or
/// restored there, or tickets linked to it.
pub(crate) fn moves(ev: &ChatsEvent) -> Option<u64> {
    match ev {
        ChatsEvent::Posted { message, .. } => Some(message.chat.get()),
        ChatsEvent::Linked { chat, .. } | ChatsEvent::MessagesDeleted { chat, .. } | ChatsEvent::MessagesRestored { chat, .. } => Some(chat.get()),
        _ => None,
    }
}

/// A `chats.watch` event (or a write's answer): the strip of the chat it
/// moves asks again.
pub(crate) fn heard(ev: &ChatsEvent) {
    if let Some(chat) = moves(ev) {
        MOVED.with(|m| m.update(|m| *m.entry(chat).or_default() += 1));
    }
}

/// Whether a ticket is staged somewhere (ready for, deploying or failed:
/// DEPLOY-21) and whether it runs anywhere, from its marks.
pub(crate) fn runs(marks: &[Mark]) -> (bool, bool) {
    (marks.iter().any(|m| m.stage.is_some()), marks.iter().any(|m| m.stage.is_none()))
}

/// The pills: each stage the strip draws that holds a ticket, in order,
/// with how many.
pub(crate) fn pills(stages: &[ChatStage]) -> Vec<(ChatStage, usize)> {
    ChatStage::STRIP.into_iter().map(|s| (s, stages.iter().filter(|x| **x == s).count())).filter(|(_, n)| *n > 0).collect()
}

/// Whether a pointer that went down at `from` and up at `to` (y, px) is a
/// swipe up — what closes the drop on a phone.
pub(crate) fn swiped_up(from: f64, to: f64) -> bool {
    from - to > 40.0
}

/// One of the strip's rows: the ticket, live.
#[derive(Clone)]
struct Row {
    row: RowSignal,
}

impl Row {
    fn id(&self) -> String {
        self.row.with(|t| t.id.to_string())
    }

    /// Its stage now: the row's status (the watch moves it), its open
    /// inputs, its marks.
    fn stage(&self) -> ChatStage {
        stage_now(&self.row)
    }
}

/// A row's stage now: its status (the watch moves it), its open inputs,
/// its marks — the strip's, and CHAT-57's live chips' colour.
pub(crate) fn stage_now(row: &RowSignal) -> ChatStage {
    let (id, kind, status) = row.with(|t| (t.id.to_string(), t.kind, t.status));
    let asking = crate::inputs::signal().with(|i| i.as_ref().is_some_and(|i| i.of_ticket(&id).iter().any(|x| x.state == InputState::Open)));
    let (staged, running) = runs(&deploys::marks_of(&id));
    stage_of(kind, status, asking, staged, running)
}

/// `chats.tickets { chat }`: the rows, or why not.
async fn fetch(chat: u64) -> Result<Vec<ChatTicket>, String> {
    let client = crate::session::chats_client().ok_or_else(|| "not connected".to_string())?;
    let events = client.tickets_all(menger_tickets_api::vocab::ChatId::new(chat)).await.map_err(|e| crate::session::describe(&e))?;
    let mut out = Vec::new();
    for ev in events {
        match ev {
            ChatsEvent::Ticket { ticket } => out.push(ticket),
            ChatsEvent::Error { message } => return Err(message),
            ChatsEvent::NotFound { .. } => return Ok(Vec::new()),
            _ => {}
        }
    }
    Ok(out)
}

/// The strip for chat `chat`, under its head.
#[component]
pub(crate) fn Strip(chat: u64) -> impl IntoView {
    crate::inputs::want();
    deploys::want();
    let holder = Holder::new();
    let rows: RwSignal<Vec<Row>> = RwSignal::new(Vec::new());
    let open: RwSignal<Option<ChatStage>> = RwSignal::new(None);
    let ask = {
        let holder = holder.clone();
        move || {
            let holder = holder.clone();
            spawn_local(async move {
                match fetch(chat).await {
                    Ok(ts) => {
                        let held: Vec<Row> = ts.into_iter().filter_map(|t| holder.hold(t.summary).map(|row| Row { row })).collect();
                        rows.try_set(held);
                    }
                    Err(e) => leptos::logging::log!("chats.tickets: {e}"),
                }
            });
        }
    };
    let moved = MOVED.with(Clone::clone);
    let moved = Memo::new(move |_| moved.with(|m| m.get(&chat).copied().unwrap_or(0)));
    let first = ask.clone();
    Effect::new(move |_| {
        moved.track();
        first();
    });
    crate::data::again(ask);
    // Every row's stage, live.
    let staged = Memo::new(move |_| rows.with(|rs| rs.iter().map(|r| (r.stage(), r.clone())).map(|(s, r)| (s, r.id())).collect::<Vec<_>>()));
    let counts = Memo::new(move |_| staged.with(|s| pills(&s.iter().map(|(st, _)| *st).collect::<Vec<_>>())));
    // A pill emptied by a move closes its drop.
    Effect::new(move |_| {
        if let Some(o) = open.get() {
            if !counts.with(|c| c.iter().any(|(s, _)| *s == o)) {
                open.set(None);
            }
        }
    });
    let down = StoredValue::new(None::<f64>);
    let on_key = move |e: web_sys::KeyboardEvent| {
        // The open list is a menu: Esc closes it (dialog.close).
        if crate::bindings::hit(&e, crate::bindings::Context::Menu) == Some("dialog.close") && open.get_untracked().is_some() {
            e.stop_propagation();
            open.set(None);
        }
    };
    move || {
        let cs = counts.get();
        (!cs.is_empty()).then(|| {
            let pills = cs
                .into_iter()
                .map(|(s, n)| {
                    let label = format!("{}: {n} ticket{}", s.words(), if n == 1 { "" } else { "s" });
                    view! {
                        <button type="button" class:w=s == ChatStage::WaitsOnYou data-stage=s.as_str() aria-label=label
                            aria-expanded=move || (open.get() == Some(s)).to_string()
                            on:click=move |_| open.update(|o| *o = if *o == Some(s) { None } else { Some(s) })>
                            {s.words()}" "<b>{n}</b>
                        </button>
                    }
                })
                .collect_view();
            let drop = move || {
                open.get().map(|s| {
                    let list = rows.with(|rs| rs.iter().filter(|r| r.stage() == s).map(|r| r.row.clone()).collect::<Vec<_>>());
                    let items = list.into_iter().map(row_view).collect_view();
                    view! {
                        <div class="cdrop" role="region" aria-label=format!("{} — this chat's tickets", s.words())
                            on:pointerdown=move |e: web_sys::PointerEvent| down.set_value(Some(e.client_y() as f64))
                            on:pointerup=move |e: web_sys::PointerEvent| {
                                if down.get_value().is_some_and(|y| swiped_up(y, e.client_y() as f64)) { open.set(None); }
                                down.set_value(None);
                            }
                            on:pointercancel=move |_| down.set_value(None)>
                            {items}
                            <button type="button" class="cdclose" aria-label="close" on:click=move |_| open.set(None)>"close"</button>
                        </div>
                    }
                })
            };
            view! {
                <div class="cstrip-wrap" on:keydown=on_key>
                    <div class="cstrip" role="toolbar" aria-label="this chat's tickets">
                        <span class="sl">"tickets"</span>
                        {pills}
                    </div>
                    {drop}
                </div>
            }
        })
    }
}

/// A row of the drop: the id, the title, the status, where it runs.
fn row_view(row: RowSignal) -> impl IntoView {
    let t: TicketSummary = row.get_untracked();
    let id = t.id.to_string();
    let href = crate::links::row_href(&t);
    let row = RwSignal::from(row);
    let status = move || row.with(|t| t.status.to_string());
    let title = move || row.with(|t| t.title.clone());
    view! {
        <a class="cdrow" href=href>
            <span class="code">{id.clone()}</span>
            <span class="cdt">{title}</span>
            <span class=move || crate::st_class(&status())>{status}</span>
            <span class="cdm">{deploys::row_marks(id.clone())}</span>
        </a>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::deployments::DeliveryStage;

    fn mark(stage: Option<DeliveryStage>) -> Mark {
        Mark { project: "menger".into(), env: "test".into(), since: 0, deployment: 1, behind: Vec::new(), version: None, stage }
    }

    #[test]
    fn the_pills_are_the_stages_held_in_order() {
        use ChatStage::*;
        assert_eq!(pills(&[Shipped, WaitsOnYou, InProgress, WaitsOnYou, Closed, Other]), [(WaitsOnYou, 2), (InProgress, 1), (Shipped, 1)]);
        assert!(pills(&[Closed, Other]).is_empty(), "no strip");
    }

    #[test]
    fn a_ticket_runs_or_is_staged_by_its_marks() {
        assert_eq!(runs(&[]), (false, false));
        assert_eq!(runs(&[mark(None)]), (false, true));
        assert_eq!(runs(&[mark(None), mark(Some(DeliveryStage::ReadyFor))]), (true, true));
    }

    #[test]
    fn a_swipe_up_closes_and_a_tap_does_not() {
        assert!(swiped_up(300.0, 200.0));
        assert!(!swiped_up(300.0, 290.0));
        assert!(!swiped_up(200.0, 300.0), "down is a scroll");
    }
}
