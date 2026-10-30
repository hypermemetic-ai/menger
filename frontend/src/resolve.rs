//! TICKETS-36: **a ticket's route, resolved.** A hash names a ticket — by its
//! uid, `#/t/<uid>[/<ID>-<slug>]`, or by any name a call takes,
//! `#/ticket/<name>` — and the page is what the name resolves to, keyed by
//! the key `get` answered, never by the route's text (ORG-6's survey, row
//! A1): its body, scopes, children, comments and chat pane follow the ticket
//! whatever name opened it.
//!
//! - A uid the app has met is its page at once (`crate::current_route`:
//!   [`crate::Route::Ticket`], by its key).
//! - Any other name is asked once, here ([`Resolver`]), with `get`:
//!   - **one ticket** — a name is replaced in the address bar by the
//!     ticket's own route (`history.replaceState`: no new history entry),
//!     and the page is drawn: the route becomes [`crate::Route::Ticket`], the
//!     answer handed over ([`take_handoff`]) so the page draws at once (a
//!     cold deep link draws from its own `get`, never waiting for the board:
//!     MFE-9);
//!   - **several** — the collision page (src/collision.rs), every choice;
//!   - **none** — the app's not-found;
//!   - **a deleted one** — its tombstone, as before (ACT-5).
//! - The page keeps the address bar on its own route: a stale `<ID>-<slug>`
//!   — a title amended, a ticket moved — is rewritten in place ([`canonical`]).
//!
//! What decides the address bar is pure ([`canonical`], [`with_path`]) and
//! pinned by native tests.

use std::cell::RefCell;

use leptos::prelude::*;
use menger_tickets_api::{Choice, Ticket, TicketSummary, TicketsEvent, Tombstone};
use wasm_bindgen_futures::spawn_local;

use crate::links::{self, Named};
use crate::{cache, dom, session, Route};

thread_local! {
    /// The ticket a resolution answered, for the page it opens.
    static HANDOFF: RefCell<Option<Ticket>> = const { RefCell::new(None) };
}

/// The ticket the resolution of the route just answered, if it is `key`'s —
/// the page's first draw, with its body, before its own `get` returns.
pub(crate) fn take_handoff(key: &str) -> Option<Ticket> {
    HANDOFF.with(|h| h.borrow_mut().take_if(|t| t.id == key))
}

/// `hash` with its path replaced by `path` (a route, `#/t/…`), its query —
/// the page's own (`?chat=41&full`, `?comment=7`) — kept.
pub(crate) fn with_path(hash: &str, path: &str) -> String {
    match hash.split_once('?') {
        Some((_, query)) if !query.is_empty() => format!("{path}?{query}"),
        _ => path.to_string(),
    }
}

/// The hash the address bar should hold for the ticket (`uid`, `key`,
/// `title`) while `hash` is its page — `None` when it holds it already, or
/// when `hash` is no route of this ticket's. A route by its uid with another
/// tail (a stale slug, none, the id it had) is rewritten; with `by_name`, so
/// is a route by a name that resolved to it (`#/ticket/PERF-19`). A ticket
/// with no uid (a server before TICKETS-35) keeps its route.
pub(crate) fn canonical(hash: &str, uid: &str, key: &str, title: &str, by_name: bool) -> Option<String> {
    if uid.is_empty() {
        return None;
    }
    let want = links::uid_href(uid, key, title);
    let path = hash.split_once('?').map_or(hash, |(p, _)| p);
    match links::named(hash)? {
        Named::Uid(u) if u == uid && path != want => Some(with_path(hash, &want)),
        Named::Name(_) if by_name => Some(with_path(hash, &want)),
        _ => None,
    }
}

/// The address bar set to `to`, in place — no history entry, no
/// `hashchange`.
pub(crate) fn replace_hash(to: &str) {
    if let Ok(h) = dom::window().history() {
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(to));
    }
}

/// What the resolver shows while it is the page.
#[derive(Clone, Debug, PartialEq)]
enum State {
    Asking,
    NotFound,
    Deleted(Box<Tombstone>),
    Collision { asked: String, choices: Vec<Choice> },
    Failed(String),
}

/// A name's page until it resolves: asks `get` once per connection (again
/// after a reconnect), then draws the ticket's page, the collision page, the
/// not-found or the tombstone.
#[component]
pub(crate) fn Resolver(
    named: Named,
    route: RwSignal<Route>,
    socket: RwSignal<u32>,
    last_error: RwSignal<Option<String>>,
) -> impl IntoView {
    let state = RwSignal::new(State::Asking);
    let text = match &named {
        Named::Uid(u) => u.clone(),
        Named::Name(n) => n.clone(),
    };
    {
        let (named, text) = (named.clone(), text.clone());
        Effect::new(move |_| {
            if socket.get() == 0 {
                return; // no connection yet: asked when there is one
            }
            ask(named.clone(), text.clone(), route, state, last_error);
        });
    }
    let label = match &named {
        Named::Uid(_) => "the ticket".to_string(),
        Named::Name(n) => n.clone(),
    };
    view! {
        <div id="resolve">
            {move || match state.get() {
                State::Asking => view! { <p class="loading">"loading "{label.clone()}"…"</p> }.into_any(),
                State::NotFound => {
                    let named = text.clone();
                    view! {
                        <div id="notfound">
                            <h2>{named}" — not found"</h2>
                            <p>"No ticket has this name, or none you can read. "<a href="#/">"Back to the board"</a>"."</p>
                        </div>
                    }
                    .into_any()
                }
                State::Deleted(t) => crate::tombstone_view(*t).into_any(),
                State::Collision { asked, choices } => view! { <crate::collision::CollisionPage asked choices /> }.into_any(),
                State::Failed(why) => {
                    let named = text.clone();
                    view! {
                        <div id="notfound">
                            <h2>{named}" — could not be opened"</h2>
                            <p>{why}" "<a href="#/">"Back to the board"</a>"."</p>
                        </div>
                    }
                    .into_any()
                }
            }}
        </div>
    }
}

/// `get` the name, and act on the answer.
fn ask(named: Named, text: String, route: RwSignal<Route>, state: RwSignal<State>, last_error: RwSignal<Option<String>>) {
    spawn_local(async move {
        let Some(client) = session::client() else { return };
        // A route by uid names a uid, nothing else (`#/t/PERF-2` is not found).
        let uid_ok = !matches!(named, Named::Uid(_)) || text.parse::<menger_tickets_api::Uid>().is_ok();
        let Some(id) = crate::typed::typed::<menger_tickets_api::vocab::TicketRef>(&text).ok().filter(|_| uid_ok) else {
            state.try_set(State::NotFound);
            return;
        };
        let events = match client.get_all(id).await {
            Ok(events) => events,
            Err(e) => {
                let why = session::describe(&e);
                last_error.set(Some(format!("get {text}: {why}")));
                state.try_set(State::Failed(why));
                return;
            }
        };
        for ev in events {
            match ev {
                TicketsEvent::Ticket { ticket } => {
                    opened(ticket, &named, route);
                    return;
                }
                TicketsEvent::Ambiguous { id, candidates, choices } => {
                    // A server before TICKETS-35 lists addresses alone.
                    let choices = match choices.is_empty() {
                        true => candidates.into_iter().map(|address| Choice { address, ..Choice::default() }).collect(),
                        false => choices,
                    };
                    for c in &choices {
                        crate::collision::learn(c);
                    }
                    state.try_set(State::Collision { asked: id.to_string(), choices });
                }
                TicketsEvent::NotFound { .. } => {
                    state.try_set(State::NotFound);
                }
                TicketsEvent::Deleted { tombstones, .. } => {
                    session::remember_tombstones(tombstones.clone());
                    match tombstones.into_iter().next() {
                        Some(t) => state.try_set(State::Deleted(Box::new(t))),
                        None => state.try_set(State::NotFound),
                    };
                }
                TicketsEvent::Error { message } => {
                    last_error.set(Some(format!("get {text}: {message}")));
                    state.try_set(State::Failed(message));
                }
                _ => {}
            }
        }
    });
}

/// One ticket answered: the address bar on its own route (a name replaced,
/// a stale tail rewritten), the answer handed to its page, and the route
/// set to that page.
fn opened(ticket: Ticket, named: &Named, route: RwSignal<Route>) {
    cache::learn(&TicketSummary::from(&ticket));
    let hash = dom::window().location().hash().unwrap_or_default();
    if let Some(to) = canonical(&hash, &ticket.uid, &ticket.id, &ticket.title, matches!(named, Named::Name(_))) {
        replace_hash(&to);
    }
    let key = ticket.id.to_string();
    HANDOFF.with(|h| *h.borrow_mut() = Some(ticket));
    route.try_set(Route::Ticket(key));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The address bar on the ticket's own route: a stale tail rewritten, a
    /// name replaced (only by the resolution), the page's query kept; another
    /// ticket's route, another page, or a ticket with no uid left alone.
    #[test]
    fn the_address_bar_is_kept_on_the_ticket_s_own_route() {
        let c = |h: &str, by_name| canonical(h, "7k3mq2xa", "PERF-19", "Compression on the WebSocket", by_name);
        let want = "#/t/7k3mq2xa/PERF-19-compression-on-the-websocket";
        assert_eq!(c(want, false), None, "already there");
        assert_eq!(c(&format!("{want}?chat=41&full"), false), None);
        assert_eq!(c("#/t/7k3mq2xa/PERF-19-an-old-title", false).as_deref(), Some(want), "a stale slug");
        assert_eq!(c("#/t/7k3mq2xa", false).as_deref(), Some(want), "no tail");
        assert_eq!(c("#/t/7K3MQ2XA/x?comment=7", false).as_deref(), Some(&*format!("{want}?comment=7")), "the query kept");
        assert_eq!(c("#/t/7k3mq2xa/STORE-3-moved?chat=2", false).as_deref(), Some(&*format!("{want}?chat=2")), "the id it had");
        assert_eq!(c("#/ticket/PERF-19?chat=41", true).as_deref(), Some(&*format!("{want}?chat=41")), "a name, resolved");
        assert_eq!(c("#/ticket/hypermemetic:MENGER/STORE/PERF-19", true).as_deref(), Some(want));
        assert_eq!(c("#/ticket/PERF-19", false), None, "only the resolution replaces a name");
        assert_eq!(c("#/t/q2r9wd4c/PERF-19-x", false), None, "another ticket's");
        assert_eq!(c("#/t/q2r9wd4c/PERF-19-x", true), None);
        assert_eq!(c("#/waves/PERF", true), None, "another page");
        assert_eq!(canonical("#/ticket/PERF-19", "", "PERF-19", "x", true), None, "no uid: as before");
    }

    #[test]
    fn a_route_s_query_survives_a_new_path() {
        assert_eq!(with_path("#/ticket/A-1?chat=3&full", "#/t/u/A-1"), "#/t/u/A-1?chat=3&full");
        assert_eq!(with_path("#/ticket/A-1", "#/t/u/A-1"), "#/t/u/A-1");
        assert_eq!(with_path("#/ticket/A-1?", "#/t/u/A-1"), "#/t/u/A-1");
    }
}
