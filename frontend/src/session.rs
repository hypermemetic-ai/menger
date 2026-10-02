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
//!   TicketsClient<Rc<WsTransport>>       typed calls, typed events
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
//!    SCALE-4: but for a parent's side of a birth, `children_changed`,
//!    which is the parent's id and new count: the count goes on the
//!    parent's row, and the child onto any open page's list of the
//!    parent's children ([`watch_children`]).
//!    MFE-17: and a re-parent (`set_parent`, MNG-12), which the loop used
//!    to drop: `parent_changed` upserts the moved child — its page's
//!    crumb follows — and tells the nav its shape changed;
//!    `child_removed` is the old parent's side, applied as
//!    `children_changed` is.
//!
//! Ending the session (any error) drops the transport, which drops the
//! socket; the caller sleeps and starts a new one. Rows are never cleared
//! on the way out — the re-walk upserts over them, so unchanged rows keep
//! their DOM identity across a reconnect (AGS-20's discipline).
//!
//! SCALE-5: `watch` can also say that *it* fell behind — the server's
//! broadcast is bounded, and a subscriber more than it holds behind loses
//! the oldest changes, which used to vanish while the dock read `live`.
//! Now they arrive as one `lagged { missed }`, and the session answers it
//! the way it answers a reconnect, minus the reconnect: the same walk ([`walk`]),
//! over the same socket, upserting in place, plus the open page's body —
//! while `watch` keeps running and the dock stays `live`. The dock's count
//! follows the walk for the same reason: after any walk it is the rows the
//! walk found, and a `created` adds one only for a ticket the store did not
//! already hold, so neither a re-walk nor the events queued behind it can
//! count a ticket twice.
//!
//! # Bodies are not rows
//!
//! A row is a [`TicketSummary`] — every field of a ticket except the body.
//! The one body the app shows is the open ticket page's, fetched by `get`
//! and held by that page, never in the row store.
//!
//! MNG-16: a body can change after birth now (`tickets.amend`), and the
//! `amended` event carries the whole ticket, body included. The row takes
//! the summary — a new title updates in place, like a status — and the
//! open page, if it is that ticket's, takes the body: the page lends its
//! body signal to the session while it is mounted ([`hold_body`]), so the
//! body still lives in exactly one place and the event needs no `get`.

use crate::kids::Kids;
use crate::{dom, row_index, upsert, upsert_page, Conn, Dock, Row};
use futures::StreamExt;
use leptos::prelude::*;
use menger_tickets_api::{TicketsClient, Status, Ticket, TicketSummary, TicketsEvent};
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
pub(crate) type Client = TicketsClient<Rc<WsTransport>>;

thread_local! {
    /// The app's single transport, for its lifetime — replaced (never
    /// duplicated) on reconnect. A `thread_local` because wasm is
    /// single-threaded and because the alternative is threading a handle
    /// through every view signature to reach the two views that make a
    /// call.
    static TRANSPORT: RefCell<Option<Rc<WsTransport>>> = const { RefCell::new(None) };

    /// MNG-16: the open ticket page's id and its body signal, while it is
    /// mounted — how an `amended` event reaches the one body the app holds.
    /// A `thread_local` for the same reason as `TRANSPORT`.
    static OPEN_BODY: RefCell<Option<(String, RwSignal<Option<String>>)>> = const { RefCell::new(None) };

    /// SCALE-4: the children lists open ticket pages hold, by parent id —
    /// what a parent's side of a birth or a move is applied to. A list,
    /// not a map: there is one open page at a time, and two pages of one
    /// parent would each want the delta.
    static OPEN_KIDS: RefCell<Vec<(String, RwSignal<Kids>)>> = const { RefCell::new(Vec::new()) };
}

/// MNG-16: the ticket page lends the session its body for as long as it is
/// mounted. One page is open at a time; the newest holder wins.
pub(crate) fn hold_body(id: String, body: RwSignal<Option<String>>) {
    OPEN_BODY.with(|b| *b.borrow_mut() = Some((id, body)));
}

/// The page's cleanup. Releases only its own signal: on a move from one
/// ticket to another the next page may already hold the slot.
pub(crate) fn release_body(body: RwSignal<Option<String>>) {
    OPEN_BODY.with(|b| {
        let mut b = b.borrow_mut();
        if b.as_ref().is_some_and(|(_, held)| *held == body) {
            *b = None;
        }
    });
}

/// MNG-16: a ticket's body as an event or a fetch carries it, to the open
/// page if it is that ticket's and the body differs — a title-only amend
/// re-renders nothing below the title (no markdown pass, no mermaid
/// redraw). SCALE-5's re-fetch after a `lagged` lands here too.
fn show_body(ticket: &Ticket) {
    let held = OPEN_BODY.with(|b| {
        b.borrow()
            .as_ref()
            .filter(|(id, _)| *id == ticket.id)
            .map(|(_, body)| *body)
    });
    if let Some(body) = held {
        if body
            .try_with_untracked(|b| b.as_deref() != Some(ticket.body.as_str()))
            .unwrap_or(false)
        {
            body.try_set(Some(ticket.body.clone()));
        }
    }
}

/// SCALE-4's page size for a ticket's children — the board walk's
/// reasoning at a smaller scale: a page is one paint, and a parent of
/// 163,530 children is shown a page at a time, with the rest one click
/// (and one `list { parent, after }`) away.
const KIDS_LIMIT: u32 = 200;

/// SCALE-4: keep `kids` — an open page's list of `parent`'s children —
/// live from `watch` until the page goes. Registered here, applied by the
/// session loop, unregistered when the page's owner is cleaned up.
pub(crate) fn watch_children(parent: String, kids: RwSignal<Kids>) {
    OPEN_KIDS.with(|open| open.borrow_mut().push((parent, kids)));
    on_cleanup(move || {
        OPEN_KIDS.with(|open| open.borrow_mut().retain(|(_, k)| *k != kids));
    });
}

/// Applies `change` to every open list of `parent`'s children.
fn kids_of(parent: &str, change: impl Fn(&mut Kids) -> bool) {
    let lists: Vec<RwSignal<Kids>> = OPEN_KIDS.with(|open| {
        open.borrow()
            .iter()
            .filter(|(p, _)| p == parent)
            .map(|(_, k)| *k)
            .collect()
    });
    for kids in lists {
        // `try_`: a page can be leaving while the event lands.
        let moved = kids.try_with_untracked(|k| {
            let mut k = k.clone();
            change(&mut k).then_some(k)
        });
        if let Some(Some(next)) = moved {
            kids.try_set(next);
        }
    }
}

/// SCALE-4: a parent's count moved — set it on its row, if the store holds
/// it, in place (the list signal never fires).
fn set_child_count(rows: RwSignal<Vec<Row>>, id: &str, count: u64) {
    let sig = rows.with_untracked(|v| row_index(v, id).map(|i| v[i].ticket));
    if let Some(sig) = sig {
        if sig.with_untracked(|t| t.child_count != count) {
            sig.update(|t| t.child_count = count);
        }
    }
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
        .map(TicketsClient::new)
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
    let client = TicketsClient::new(transport);

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
    walk(&client, rows, dock, last_error).await?;
    conn.set(Conn::Live);

    // 4. Live. `watch` runs until the socket dies.
    while let Some(ev) = live.next().await {
        match ev.map_err(|e| describe(&e))? {
            // Every mutation event carries the full updated ticket —
            // AGS-19's `children_changed` carries the full *parent*, which
            // is how a page and a nav that never re-fetch learn that a
            // child was born (AGS-20).
            ClientEvent::Data(TicketsEvent::Created { ticket }) => {
                // SCALE-5: news to the store, or one a walk already found
                // (born while it ran, or queued behind a `lagged`)?
                let fresh = rows.with_untracked(|v| row_index(v, &ticket.id).is_none());
                upsert(rows, TicketSummary::from(&ticket));
                if fresh {
                    dock.update(|d| {
                        if let Some(d) = d {
                            d.ticket_count += 1;
                        }
                    });
                }
            }
            ClientEvent::Data(
                TicketsEvent::StatusChanged { ticket, .. }
                | TicketsEvent::UnlocksChanged { ticket, .. },
            ) => upsert(rows, TicketSummary::from(&ticket)),
            // SCALE-4: the parent's id, the child and the new count — the
            // child's own row arrived with its `created`.
            ClientEvent::Data(TicketsEvent::ChildrenChanged { id, added, child_count }) => {
                set_child_count(rows, &id, child_count);
                kids_of(&id, |k| k.added(&added));
            }
            // MFE-17: a ticket was filed under another parent. Its row
            // takes the new `parent`; and since the nav reads the forest's
            // shape off the row list, untracked (a status change must not
            // re-plan it), the list is told — the one change of shape that
            // is not an insert.
            ClientEvent::Data(TicketsEvent::ParentChanged { ticket, .. }) => {
                upsert(rows, TicketSummary::from(&ticket));
                rows.notify();
            }
            // MFE-17: the old parent's side of that move — its count, and
            // any open page's list of its children.
            ClientEvent::Data(TicketsEvent::ChildRemoved { id, removed, child_count }) => {
                set_child_count(rows, &id, child_count);
                kids_of(&id, |k| k.removed(&removed));
            }
            // MNG-16: the row's title in place; the open page's body too.
            ClientEvent::Data(TicketsEvent::Amended { ticket, .. }) => {
                show_body(&ticket);
                upsert(rows, TicketSummary::from(&ticket));
            }
            // SDLC-9: commits linked or unlinked. They are on the row, so
            // the row takes them — and the open ticket page, which renders
            // through it, shows them; the body did not change.
            ClientEvent::Data(
                TicketsEvent::CommitsLinked { ticket, .. }
                | TicketsEvent::CommitsUnlinked { ticket, .. },
            ) => upsert(rows, TicketSummary::from(&ticket)),
            // SCALE-5: the server dropped `missed` changes this stream never
            // saw. What the rows hold may be stale: fetch again what is
            // shown — every row, by the walk a reconnect makes, and the open
            // page's body. `watch` is whole again from here (what queued
            // behind the gap applies after the walk, in order, as during the
            // first walk), so the dock never leaves `live`.
            ClientEvent::Data(TicketsEvent::Lagged { missed }) => {
                leptos::logging::log!("watch lagged: {missed} change(s) missed; re-walking");
                walk(&client, rows, dock, last_error).await?;
                refetch_open_body(&client, rows, last_error).await;
                refetch_open_children(rows, last_error);
            }
            // Future event kinds: ignore, stay live.
            _ => {}
        }
    }
    Err("watch stream ended".into())
}

/// MNG-9's walk of the whole store: `list { summary, limit }` page by page
/// with `after = next`, painting as pages land, then the rows the walk did
/// not name dropped. The session's step 3 — and, since SCALE-5, its answer
/// to `lagged`: a re-walk over rows already held upserts them in place, so
/// every row that did not change keeps its DOM. Leaves the dock's count at
/// the number of tickets the walk found.
async fn walk(
    client: &Client,
    rows: RwSignal<Vec<Row>>,
    dock: RwSignal<Option<Dock>>,
    last_error: RwSignal<Option<String>>,
) -> Result<(), String> {
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
    dock.update(|d| {
        if let Some(d) = d {
            d.ticket_count = seen.len() as u64;
        }
    });
    Ok(())
}

/// SCALE-5: after a `lagged`, the open page's body — the one thing the
/// walk does not carry — fetched again, as a reconnect's `get` would. An
/// `amended` for it may have been among the changes lost.
async fn refetch_open_body(
    client: &Client,
    rows: RwSignal<Vec<Row>>,
    last_error: RwSignal<Option<String>>,
) {
    let Some(id) = OPEN_BODY.with(|b| b.borrow().as_ref().map(|(id, _)| id.clone())) else {
        return;
    };
    match client.get_all(id.clone()).await {
        Ok(events) => {
            for ev in events {
                if let TicketsEvent::Ticket { ticket } = ev {
                    show_body(&ticket);
                    upsert(rows, TicketSummary::from(&ticket));
                }
            }
        }
        Err(e) => last_error.set(Some(format!("get {id}: {}", describe(&e)))),
    }
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

/// SCALE-4 × SCALE-5: a `children_changed` or `child_removed` may be among
/// the changes a `lagged` lost, and the walk does not carry a parent's
/// list — so every open page's children are asked for again from their
/// first page, which is what the page itself does on a reconnect.
fn refetch_open_children(rows: RwSignal<Vec<Row>>, last_error: RwSignal<Option<String>>) {
    let open: Vec<(String, RwSignal<Kids>)> = OPEN_KIDS.with(|open| open.borrow().clone());
    for (parent, kids) in open {
        load_children(parent, None, rows, kids, last_error);
    }
}

/// SCALE-4: one page of `id`'s children — the first (`after: None`, which
/// replaces what the page held: it is asked on entry and again on every
/// reconnect) or the next one after the cursor the last page handed back.
/// The rows upsert into the shared store, so each child renders live
/// through its own signal as before; the ids, in the store's order, go to
/// the page's list. `list { parent }` costs the page at any size of parent
/// (the server keeps an index of children), so this is one round trip
/// whether the ticket has two children or 163,530.
pub(crate) fn load_children(
    id: String,
    after: Option<String>,
    rows: RwSignal<Vec<Row>>,
    kids: RwSignal<Kids>,
    last_error: RwSignal<Option<String>>,
) {
    spawn_local(async move {
        let Some(client) = client() else {
            return; // not connected yet: the page asks again when it is
        };
        let first = after.is_none();
        match client
            .list_all(None, None, Some(id.clone()), after, Some(KIDS_LIMIT), Some(true))
            .await
        {
            Ok(events) => {
                let mut page: Vec<TicketSummary> = Vec::new();
                let mut next = None;
                for ev in events {
                    match ev {
                        TicketsEvent::Summary { summary } => page.push(summary),
                        TicketsEvent::Page { next: n, .. } => next = n,
                        TicketsEvent::Error { message } => {
                            last_error.set(Some(format!("children of {id}: {message}")))
                        }
                        _ => {}
                    }
                }
                let ids: Vec<String> = page.iter().map(|t| t.id.clone()).collect();
                upsert_page(rows, page);
                kids.try_update(|k| k.page(first, ids, next));
            }
            Err(e) => last_error.set(Some(format!("children of {id}: {}", describe(&e)))),
        }
    });
}

/// Kept honest by the compiler: `Ticket` is the only shape that carries a
/// body, and it is menger's own type. If the server renames the field this
/// stops compiling — which is the whole point of the crate split.
const _: fn(&Ticket) -> &String = |t| &t.body;
