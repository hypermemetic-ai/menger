//! MFE-9: the app's entire data plane — one WebSocket, typed end to end.
//!
//! What this replaces: `net.rs`, which POSTed to `/rest/tickets/{method}`
//! through the shell's reverse proxy, read `tickets.watch` as SSE-over-POST
//! by hand out of a `ReadableStream`, and handed every view
//! `serde_json::Value`s that `main.rs` re-parsed into a hand-written mirror
//! of the server's `Ticket`. PERF-4 measured where that ends: at 10,000
//! tickets the unary `list` hits the REST gateway's collect cap and answers
//! 413, and the session loop retries it forever under an empty board.
//!
//! What is here instead:
//!
//! ```text
//!   menger-tickets-api            the server's own declarations
//!        │  #[activation(client)]
//!        ▼
//!   MengerTicketsClient<Rc<WsTransport>>       typed calls, typed events
//!        │  plexus_client::decode
//!        ▼
//!   WsTransport (ws-web)          ONE socket, every call multiplexed
//!        │  menger.call / menger.call_unsub
//!        ▼
//!   menger :4455
//! ```
//!
//! # The session, in order
//!
//! 1. **`watch` first.** `Transport::call` writes its subscribe frame when
//!    it is invoked, not on first poll (MACRO-6), so the subscription is
//!    live before the first page is asked for and nothing born during the
//!    walk is missed.
//! 2. **`health`**, for the dock.
//! 3. **The page walk**: `list { summary: true, limit: 500 }`, then again
//!    with `after = next` until `next` is null (MNG-9). Rows upsert as each
//!    page lands, so the board paints while the walk is still running — the
//!    property PERF-4's "list everything, then render everything, then go
//!    live" design could not have.
//! 4. **Live.** The dock reads `live · WS` and the loop pumps `watch`
//!    forever; every mutation event carries the whole affected ticket, so
//!    one upsert keeps board, nav and any open ticket page coherent.
//!
//! Ending the session (any error) drops the transport, which drops the
//! socket; the caller sleeps and starts a new one. Rows are never cleared
//! on the way out — the re-walk upserts over them, so unchanged rows keep
//! their DOM identity across a reconnect (AGS-20's discipline).
//!
//! # Bodies are not rows
//!
//! A row is a [`TicketSummary`] — every field of a ticket except the body.
//! The one body the app shows is the open ticket page's, fetched by `get`
//! and held by that page, never in the row store.

use crate::{dom, upsert, upsert_page, Conn, Dock, Row};
use futures::StreamExt;
use leptos::prelude::*;
use menger_tickets_api::{MengerTicketsClient, Status, Ticket, TicketSummary, TicketsEvent};
use plexus_client::{ClientEvent, WsTransport};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use wasm_bindgen_futures::spawn_local;

/// MNG-9's page size for the board's walk. 500 summary rows is ~150 KB on
/// the wire and one paint; the walk's cost is the number of round trips,
/// the browser's is the size of each paint, and this is the balance MFE-9
/// pinned.
const PAGE_LIMIT: u32 = 500;

/// menger's hub namespace — `DynamicHub::new("menger")` server-side. A
/// property of the server, not of the activation, which is why it is a
/// constructor argument and not part of a `MethodRef`.
const HUB: &str = "menger";

/// The retry gap after a session ends. A server restart costs a few
/// seconds of `reconnecting…`, never a reload.
const RETRY_MS: i32 = 1000;

/// The typed client, over the app's one shared connection.
pub(crate) type Client = MengerTicketsClient<Rc<WsTransport>>;

thread_local! {
    /// The app's single transport, for its lifetime — replaced (never
    /// duplicated) on reconnect. A `thread_local` because wasm is
    /// single-threaded and because the alternative is threading a handle
    /// through every view signature to reach the two views that make a
    /// call.
    static TRANSPORT: RefCell<Option<Rc<WsTransport>>> = const { RefCell::new(None) };
}

/// A client over the live connection, or `None` before the first one is up.
///
/// This is how a *view* reaches the server: it asks for the client, makes
/// one typed call, and lets the answer upsert into the shared row store.
/// A view never touches the transport, and never keeps what it fetched
/// anywhere but the store (or, for a body, its own page state).
pub(crate) fn client() -> Option<Client> {
    TRANSPORT
        .with(|t| t.borrow().clone())
        .map(MengerTicketsClient::new)
}

/// Renders a failed call for the banner. The two layers stay distinct:
/// `transport` means the socket, `remote` means the server refused the
/// call itself — neither is the activation's own in-band `Error` event,
/// which is data and is surfaced by the caller.
fn describe(e: &plexus_client::ClientError) -> String {
    match e {
        plexus_client::ClientError::Transport(t) => format!("socket: {t}"),
        other => other.to_string(),
    }
}

/// Start the data plane. Runs for the app's lifetime, regardless of route
/// — the ticket page's liveness is this same stream.
pub(crate) fn drive(
    rows: RwSignal<Vec<Row>>,
    dock: RwSignal<Option<Dock>>,
    conn: RwSignal<Conn>,
    socket: RwSignal<u32>,
    last_error: RwSignal<Option<String>>,
) {
    spawn_local(async move {
        loop {
            let err = session(rows, dock, conn, socket, last_error)
                .await
                .expect_err("a session only returns on error");
            TRANSPORT.with(|t| *t.borrow_mut() = None);
            leptos::logging::log!("session ended: {err}; reconnecting");
            conn.set(Conn::Reconnecting);
            dom::sleep_ms(RETRY_MS).await;
        }
    });
}

/// One connected session. `Err` ends it; [`drive`] reconnects.
async fn session(
    rows: RwSignal<Vec<Row>>,
    dock: RwSignal<Option<Dock>>,
    conn: RwSignal<Conn>,
    socket: RwSignal<u32>,
    last_error: RwSignal<Option<String>>,
) -> Result<(), String> {
    let transport = Rc::new(
        WsTransport::connect(&dom::ws_url(), HUB)
            .await
            .map_err(|e| format!("connect {}: {e}", dom::ws_url()))?,
    );
    TRANSPORT.with(|t| *t.borrow_mut() = Some(transport.clone()));
    // A connection exists — announced here, before the walk, because an
    // open ticket page needs only two point queries and must not wait for
    // the board.
    socket.update(|g| *g += 1);
    let client = MengerTicketsClient::new(transport);

    // 1. `watch` first — subscribed eagerly, so a ticket born during the
    //    walk below arrives even if the walk has already passed its
    //    position (MNG-9 acceptance 5).
    let mut live = client.watch(None);

    // 2. The dock.
    for ev in client.health_all().await.map_err(|e| describe(&e))? {
        match ev {
            TicketsEvent::Health {
                version,
                ticket_count,
                ..
            } => dock.set(Some(Dock {
                version,
                ticket_count,
            })),
            TicketsEvent::Error { message } => last_error.set(Some(format!("health: {message}"))),
            _ => {}
        }
    }

    // 3. The page walk. Every page paints as it lands.
    let mut seen: HashSet<String> = HashSet::new();
    let mut after: Option<String> = None;
    let mut pending: Vec<TicketSummary> = Vec::with_capacity(PAGE_LIMIT as usize);
    let mut painted = 0usize;
    loop {
        let mut page = client.list(None, None, None, after.take(), Some(PAGE_LIMIT), Some(true));
        // Each summary is its own WebSocket notification, so the rows are
        // gathered rather than merged one at a time: a per-row merge
        // notifies the row list 10,000 times, and each notification
        // re-diffs the board and rebuilds the nav's forest over every row
        // held so far. See `upsert_page`.
        while let Some(ev) = page.next().await {
            match ev.map_err(|e| describe(&e))? {
                ClientEvent::Data(TicketsEvent::Summary { summary }) => {
                    seen.insert(summary.id.clone());
                    pending.push(summary);
                }
                ClientEvent::Data(TicketsEvent::Page { next, .. }) => after = next,
                ClientEvent::Data(TicketsEvent::Error { message }) => {
                    last_error.set(Some(format!("list: {message}")))
                }
                _ => {}
            }
        }
        // Paint when the pending rows would at least double what is on
        // screen — and always at the end of the walk. One page still
        // paints one page while the board is small (the first is on screen
        // in under 300 ms at 10^4), and the walk stays O(n) overall
        // instead of O(pages x n): painting every page costs a full
        // re-diff of everything already painted, which at 10^4 is 8.2 s of
        // work against 2.3 s for the same rows painted once. Measured, in
        // plans/MFE/artifacts/MFE-9-numbers.js.
        if pending.len() >= painted.max(PAGE_LIMIT as usize) || after.is_none() {
            painted += pending.len();
            upsert_page(rows, std::mem::take(&mut pending));
        }
        if after.is_none() {
            break;
        }
    }
    // The walk is the server's truth: drop rows it no longer names. Nothing
    // deletes tickets today, but a reconnect must converge regardless — and
    // this is a retain, not a rebuild, so every surviving row keeps its DOM.
    drop_missing(rows, &seen);
    conn.set(Conn::Live);

    // 4. Live. `watch` runs until the socket dies.
    while let Some(ev) = live.next().await {
        match ev.map_err(|e| describe(&e))? {
            // Every mutation event carries the full updated ticket —
            // AGS-19's `children_changed` carries the full *parent*, which
            // is how a page and a nav that never re-fetch learn that a
            // child was born (AGS-20).
            ClientEvent::Data(TicketsEvent::Created { ticket }) => {
                upsert(rows, TicketSummary::from(&ticket));
                dock.update(|d| {
                    if let Some(d) = d {
                        d.ticket_count += 1;
                    }
                });
            }
            ClientEvent::Data(
                TicketsEvent::StatusChanged { ticket, .. }
                | TicketsEvent::UnlocksChanged { ticket, .. }
                | TicketsEvent::ChildrenChanged { ticket, .. },
            ) => upsert(rows, TicketSummary::from(&ticket)),
            // Future event kinds: ignore, stay live.
            _ => {}
        }
    }
    Err("watch stream ended".into())
}

/// Retain only the rows the walk named. Skipped entirely when nothing is
/// missing, so the common case never touches the list signal.
fn drop_missing(rows: RwSignal<Vec<Row>>, seen: &HashSet<String>) {
    let stale = rows.with_untracked(|v| v.iter().any(|r| !seen.contains(&r.id)));
    if stale {
        rows.update(|v| v.retain(|r| seen.contains(&r.id)));
    }
}

/// The app's one mutation: `set_status`, fired from a row's select or the
/// ticket menu (AGS-16) — one call path. The caller's control is left
/// showing the user's choice while the call is in flight; the authoritative
/// update arrives back through `watch`. Any failure — a dead socket, a
/// refused call, or the activation's in-band `error` / `not_found` —
/// surfaces in the banner and runs `on_failure` (the board snaps its select
/// back to the last server-confirmed status; the menu has nothing to
/// revert — its chips follow the server).
pub(crate) fn request_set_status(
    id: String,
    chosen: Status,
    last_error: RwSignal<Option<String>>,
    on_failure: impl FnOnce() + 'static,
) {
    spawn_local(async move {
        let Some(client) = client() else {
            last_error.set(Some(format!(
                "set_status {id} -> {chosen}: not connected yet"
            )));
            on_failure();
            return;
        };
        let failure = match client
            .set_status_all(id.clone(), chosen.as_str().to_string())
            .await
        {
            Ok(events) => first_refusal(&events),
            Err(e) => Some(describe(&e)),
        };
        if let Some(msg) = failure {
            last_error.set(Some(format!("set_status {id} -> {chosen}: {msg}")));
            on_failure();
        }
    });
}

/// The first in-band refusal in a call's events. `error` and `not_found`
/// both mean "the call did not do what was asked" — the server is the
/// validator and this is how it says no.
fn first_refusal(events: &[TicketsEvent]) -> Option<String> {
    events.iter().find_map(|e| match e {
        TicketsEvent::Error { message } => Some(message.clone()),
        TicketsEvent::NotFound { id } => Some(format!("{id}: not found")),
        _ => None,
    })
}

/// What the ticket page asks for on entry: the one body the app holds, and
/// the ticket's blockers resolved to their current statuses.
///
/// `get` is the only call that carries a body. The ticket itself upserts
/// into the shared row store (so the page renders through the same signals
/// as the board and stays live on `watch`); the *body* goes to the page's
/// own signal and nowhere else. Resolved blockers upsert too — their
/// statuses then render live — and an id the call did not resolve is a
/// dangling forward reference once `blockers_loaded` is set.
pub(crate) fn load_ticket(
    id: String,
    rows: RwSignal<Vec<Row>>,
    body: RwSignal<Option<String>>,
    not_found: RwSignal<bool>,
    blockers_loaded: RwSignal<bool>,
    last_error: RwSignal<Option<String>>,
) {
    spawn_local(async move {
        let Some(client) = client() else {
            // Not connected yet: the session's walk will fill the row in,
            // and the page re-renders through the store when it does. Only
            // the body needs this call, so leave it unfetched rather than
            // claiming the ticket does not exist.
            return;
        };
        match client.get_all(id.clone()).await {
            Ok(events) => {
                for ev in events {
                    match ev {
                        TicketsEvent::Ticket { ticket } => {
                            body.set(Some(ticket.body.clone()));
                            upsert(rows, TicketSummary::from(&ticket));
                        }
                        TicketsEvent::NotFound { .. } => not_found.set(true),
                        TicketsEvent::Error { message } => {
                            last_error.set(Some(format!("get {id}: {message}")))
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => last_error.set(Some(format!("get {id}: {}", describe(&e)))),
        }
        match client.blockers_all(id.clone()).await {
            Ok(events) => {
                for ev in events {
                    if let TicketsEvent::Blocker { ticket } = ev {
                        upsert(rows, TicketSummary::from(&ticket));
                    }
                }
            }
            Err(e) => last_error.set(Some(format!("blockers {id}: {}", describe(&e)))),
        }
        blockers_loaded.set(true);
    });
}

/// Kept honest by the compiler: `Ticket` is the only shape that carries a
/// body, and it is menger's own type. If the server renames the field this
/// stops compiling — which is the whole point of the crate split.
const _: fn(&Ticket) -> &String = |t| &t.body;
