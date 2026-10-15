//! ACT-4: the menu's writes — each [`Call`] (src/menu/model.rs) sent as the
//! one call it is, over the app's typed client (src/session.rs).
//!
//! What the server answers lands where `watch` would put it: `update`'s
//! ticket, and every ticket `update_many` answers with (ACT-2: the selection
//! as the write left it) go onto the rows the cache holds at once — so this
//! window is right even when its own `watch` falls behind a bulk write
//! (~6 events a ticket; a `lagged` past 256) and asks again. A refusal goes
//! to the error banner in the server's words, which name each ticket and why.
//!
//! Delete and its undo are here too: `delete { ids, children, unblock }`
//! answers the ids it removed (the selection and, with `children: delete`,
//! every descendant), which is what `undelete` is given back.

use leptos::prelude::*;
use menger_tickets_api::{TicketSummary, TicketsEvent};

use super::model::{Args, Call, Change};
use crate::{cache, session};

/// The first refusal among a write's events, in the server's words.
fn refusal(events: &[TicketsEvent]) -> Option<String> {
    events.iter().find_map(|e| match e {
        TicketsEvent::Error { message } => Some(message.clone()),
        TicketsEvent::NotFound { id } => Some(format!("{id}: not found")),
        TicketsEvent::BodyTooLarge { message, .. } => Some(message.clone()),
        _ => None,
    })
}

/// Every ticket a write answered with, onto the rows the cache holds.
fn put_answered(events: &[TicketsEvent]) {
    for ev in events {
        match ev {
            TicketsEvent::Updated { ticket, .. }
            | TicketsEvent::LabelsChanged { ticket, .. }
            | TicketsEvent::CommitsLinked { ticket, .. }
            | TicketsEvent::StatusChanged { ticket, .. } => {
                cache::put(TicketSummary::from(ticket));
            }
            TicketsEvent::UpdatedMany { tickets, .. } => {
                for t in tickets {
                    cache::put(TicketSummary::from(t));
                }
            }
            _ => {}
        }
    }
}

/// A list the call names, as its type — none when empty (ACCESS-8).
fn some_list<T: std::str::FromStr<Err = String>>(v: Option<Vec<String>>) -> Result<Option<Vec<T>>, String> {
    crate::typed::opt_many(v.filter(|v| !v.is_empty()))
}

/// The menu's `Args` as the parameter types `update` and `update_many`
/// take (ACCESS-8), or the first value that is none.
#[allow(clippy::type_complexity)]
fn typed_args(
    a: Args,
) -> Result<
    (
        Option<menger_tickets_api::vocab::Title>,
        Option<menger_tickets_api::Kind>,
        Option<menger_tickets_api::Status>,
        Option<menger_tickets_api::vocab::TicketRef>,
        Option<menger_tickets_api::vocab::SeverityChange>,
        Option<Vec<menger_tickets_api::vocab::TicketRef>>,
        Option<Vec<menger_tickets_api::vocab::TicketRef>>,
        Option<Vec<menger_tickets_api::vocab::Label>>,
        Option<Vec<menger_tickets_api::vocab::Label>>,
    ),
    String,
> {
    use crate::typed::opt;
    Ok((opt(a.title)?, opt(a.kind)?, opt(a.status)?, opt(a.parent)?, opt(a.severity)?, some_list(a.block)?, some_list(a.unblock)?, some_list(a.label)?, some_list(a.unlabel)?))
}

/// Send `call` — `change` to `ids` — and put what went wrong in the banner.
pub(crate) fn send(call: Call, change: Change, last_error: RwSignal<Option<String>>) {
    if let Call::SetStatus(id, status) = call {
        // MENGER-13's restore prompt rides the board's own path.
        session::request_set_status(id, status, last_error, || {});
        return;
    }
    let what = format!("{} {}", change.words(), match &call {
        Call::Update(id, _) | Call::Label { id, .. } | Call::Commits(id, _) | Call::SetStatus(id, _) => id.clone(),
        Call::Many(ids, _) => format!("on {} tickets", ids.len()),
    });
    wasm_bindgen_futures::spawn_local(async move {
        let Some(client) = session::client() else {
            last_error.set(Some(format!("{what}: not connected yet")));
            return;
        };
        let source = Some(menger_tickets_api::vocab::CallerSource::App);
        // ACCESS-8: the menu's text as the parameter types, at the edge.
        let asked = async {
            use crate::typed::{many, typed};
            Ok::<_, String>(match call {
                Call::Update(id, args) => {
                    let (title, kind, status, parent, severity, block, unblock, _, _) = typed_args(args)?;
                    client.update_all(typed(&id)?, title, None, kind, status, parent, severity, block, unblock, source, None).await
                }
                Call::Many(ids, args) => {
                    let (_, kind, status, parent, severity, block, unblock, label, unlabel) = typed_args(args)?;
                    client
                        .update_many_all(Some(many(ids)?), None, None, None, kind, status, parent, severity, block, unblock, label, unlabel, source, None)
                        .await
                }
                Call::Label { id, add, remove } => client.label_all(Some(typed(&id)?), None, some_list(Some(add))?, some_list(Some(remove))?).await,
                Call::Commits(id, commits) => client.link_commits_all(Some(typed(&id)?), None, many(commits)?).await,
                Call::SetStatus(..) => unreachable!("sent above"),
            })
        };
        let answer = match asked.await {
            Ok(answer) => answer,
            Err(why) => {
                last_error.set(Some(format!("{what}: {why}")));
                return;
            }
        };
        match answer {
            Ok(events) => match refusal(&events) {
                Some(msg) => last_error.set(Some(format!("{what}: {msg}"))),
                None => put_answered(&events),
            },
            Err(e) => last_error.set(Some(format!("{what}: {}", session::describe(&e)))),
        }
    });
}

/// `delete { ids, children, unblock }`: the ids it removed, or the server's
/// words.
pub(crate) async fn delete(ids: Vec<String>, children: Option<String>, unblock: Option<bool>) -> Result<Vec<String>, String> {
    let client = session::client().ok_or_else(|| "not connected".to_string())?;
    let (ids, children) = (crate::typed::many(ids)?, crate::typed::opt(children)?);
    let events = client
        .delete_all(Some(ids), None, None, None, children, unblock, Some(crate::typed::typed("deleted from the app's menu")?))
        .await
        .map_err(|e| session::describe(&e))?;
    if let Some(msg) = refusal(&events) {
        return Err(msg);
    }
    events
        .into_iter()
        .find_map(|e| match e {
            TicketsEvent::Deleted { ids, tombstones } => {
                session::remember_tombstones(tombstones);
                Some(ids)
            }
            _ => None,
        })
        .ok_or_else(|| "no answer".to_string())
}

/// `undelete { ids }`: back as they were, while the tombstones keep them.
pub(crate) async fn undelete(ids: Vec<String>) -> Result<(), String> {
    let client = session::client().ok_or_else(|| "not connected".to_string())?;
    let events = client.undelete_all(crate::typed::many(ids)?).await.map_err(|e| session::describe(&e))?;
    match refusal(&events) {
        Some(msg) => Err(msg),
        None => Ok(()),
    }
}
