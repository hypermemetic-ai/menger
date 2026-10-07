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
//!    live" design could not have. (SCALE-24: after `groups`, and beside
//!    step 4 rather than before it — see below.)
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
//! the way it answers a reconnect, minus the reconnect: the same walk (`walk`, until SCALE-26),
//! over the same socket, upserting in place, plus the open page's body —
//! while `watch` keeps running and the dock stays `live`. The dock's count
//! follows the walk for the same reason: after any walk it is the rows the
//! walk found, and a `created` adds one only for a ticket the store did not
//! already hold, so neither a re-walk nor the events queued behind it can
//! count a ticket twice.
//!
//! SCALE-24: **`watch` is pumped while the walk runs.** Until now the
//! session walked first and only then read `watch`, applying what had queued
//! behind the walk in order — which made every change wait for the walk: 50 s
//! at 10^6 before a created ticket moved its shortcode's total. The nav no
//! longer needs the walk, so the walk runs beside the live loop (`start_walk`)
//! and every event applies as it arrives. What made the old order safe is
//! kept another way: while a walk runs, the ids `watch` has written are
//! remembered (`touched`), a page of the walk — possibly older than an event
//! already applied — does not overwrite them, and the walk's end does not
//! drop them. The dock still reads `live · WS` only once a walk has finished.
//!
//! SCALE-24: **the nav's rows come first.** Between `health` and the walk
//! the session asks `groups {}` — one roll-up per shortcode row, the whole
//! of what the nav needs to draw its top level — and hands them to the nav's
//! [`nav::Tree`]; `group_changed` keeps them. An open nav list's deltas
//! (`children_changed`, `child_removed`) go to it as they go to an open
//! ticket page. On a `lagged` the groups are asked for again first, and the
//! nav's open lists ask for what they show (the tree's epoch moves).
//!
//! **Rows live in the row cache** (src/cache.rs). Every event goes to it —
//! applied to a row some view holds — and the board's list still takes
//! every ticket it hears of, as before, until SCALE-26 ends the walk. What
//! a ticket page fetches (its ticket, blockers, children) it holds there for
//! as long as it is open, instead of inserting into the board's list.
//!
//! SCALE-26: **the walk is gone.** Every view now asks for what it shows,
//! the board too (src/board.rs), so the session is, in order: `watch`,
//! `health`, `groups`, **`extents`** — each shortcode's last sequence, which
//! is where every ticket sits in the store's order — and **one board
//! window** (`list { after, limit, summary }` for the rows the board's
//! viewport and margins cover); then `live`. Five requests at 100 tickets
//! and at 10^6, where the walk was 2,003 and 43.2 s. `live` means that: the
//! socket is up, `watch` is subscribed, and what the board shows is in hand.
//! The events now: `created` moves the board's extents (the count, the
//! scrollbar) and is held only where the board holds the rows around it
//! ([`board::Board::created`]); every other row event is `cache::put` —
//! applied where some view holds the row, dropped where none does. A
//! `lagged` asks again for what is shown — the groups, the extents, the
//! board's window, the open page's body, children and the rows it names
//! ([`stale`]) — and a reconnect does the same in a new session: no walk,
//! and the rows on screen keep their DOM. The dock's count is the extents'
//! sum (the store's count: sequences are dense), so a `lagged` that lost a
//! thousand births cannot leave it wrong, and nothing counts a ticket twice.
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

//!
//! # The wave board listens (WAVE-5)
//!
//! An open wave board (src/waves.rs) asks `waves` for its shortcodes and
//! asks again when what it shows may have moved. It lends the session an ear
//! for as long as it is mounted ([`listen`]): every `watch` event is offered
//! to it before the loop applies it, and the board decides what it does to
//! its cards. Nothing else about the session changes — `waves` is asked only
//! while a wave board is open, never at startup. WAVE-6: the server keeps
//! every placement and says when one moves (`placements_changed`); the board
//! moves its cards from that, and the session sets each moved placement on
//! the rows views hold.

use crate::kids::Kids;
use crate::{board, cache, dom, frame, nav, registry, Conn, Dock};
use futures::StreamExt;
use leptos::prelude::*;
use menger_tickets_api::{TicketsClient, Status, Ticket, TicketSummary, TicketsEvent};
use plexus_client::{ClientEvent, WsTransport};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use wasm_bindgen_futures::spawn_local;

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

    /// SCALE-25: whether there is a connection — true from the moment a
    /// session's socket is up, false from the moment it ends. What a view
    /// that asks the server for what it shows (the jump palette) follows, so
    /// it can say it is offline rather than show its last answer as current.
    static LINK: ArcRwSignal<bool> = ArcRwSignal::new(false);

    /// MNG-16: the open ticket page's id and its body signal, while it is
    /// mounted — how an `amended` event reaches the one body the app holds.
    /// A `thread_local` for the same reason as `TRANSPORT`.
    static OPEN_BODY: RefCell<Option<(String, RwSignal<Option<String>>)>> = const { RefCell::new(None) };

    /// SCALE-4: the children lists open ticket pages hold, by parent id —
    /// what a parent's side of a birth or a move is applied to. A list,
    /// not a map: there is one open page at a time, and two pages of one
    /// parent would each want the delta. SCALE-24: with the page's holder,
    /// which a re-fetch after a `lagged` holds its rows with.
    static OPEN_KIDS: RefCell<Vec<(String, RwSignal<Kids>, cache::Holder)>> = const { RefCell::new(Vec::new()) };

    /// SCALE-26: moves on every `lagged` — what an open ticket page shows by
    /// id beyond its own ticket (its ancestors, blockers, unlocks) is asked
    /// for again, as a reconnect's new socket asks for it.
    static STALE: ArcRwSignal<u32> = ArcRwSignal::new(0);

    /// SCALE-26: the ids open ticket pages show and have asked the server
    /// for, with the page's holder — a blocker that dangled, an unlock not
    /// yet fetched. A `created` for one of them is held by that page at once:
    /// the walk used to put every newborn in the store, which is how a
    /// dangling blocker resolved live (MFE-7).
    static WANTED: RefCell<Vec<(StoredValue<HashSet<String>>, cache::Holder)>> = const { RefCell::new(Vec::new()) };

    /// WAVE-5: the open wave boards' ears on `watch`, each with the number
    /// it was lent under — offered every event before the loop applies it.
    static EARS: RefCell<Vec<(u64, Rc<dyn Fn(&TicketsEvent)>)>> = const { RefCell::new(Vec::new()) };
    static NEXT_EAR: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// WAVE-5: lend the session an ear on `watch` until the current owner (a
/// wave board) is cleaned up — `ear` is offered every event the stream
/// carries, before it is applied.
pub(crate) fn listen(ear: impl Fn(&TicketsEvent) + 'static) {
    let n = NEXT_EAR.with(|c| {
        let n = c.get();
        c.set(n.wrapping_add(1));
        n
    });
    EARS.with(|e| e.borrow_mut().push((n, Rc::new(ear))));
    on_cleanup(move || EARS.with(|e| e.borrow_mut().retain(|(m, _)| *m != n)));
}

/// Every ear, offered `ev` — taken out of the list first, so an ear that
/// lends or returns one (a view mounting or leaving) never meets a borrow.
fn hear(ev: &TicketsEvent) {
    let ears: Vec<Rc<dyn Fn(&TicketsEvent)>> = EARS.with(|e| e.borrow().iter().map(|(_, f)| f.clone()).collect());
    for ear in ears {
        ear(ev);
    }
}

/// SCALE-26: the `lagged` count, as a signal — see `STALE`.
pub(crate) fn stale() -> ArcRwSignal<u32> {
    STALE.with(Clone::clone)
}

/// SCALE-26: an open page registers the ids it asked for (see `WANTED`)
/// until it goes.
pub(crate) fn watch_wanted(asked: StoredValue<HashSet<String>>, holder: cache::Holder) {
    WANTED.with(|w| w.borrow_mut().push((asked, holder)));
    on_cleanup(move || WANTED.with(|w| w.borrow_mut().retain(|(a, _)| *a != asked)));
}

/// A ticket was born: held by every open page that asked for it.
fn wanted(t: &TicketSummary) {
    let pages: Vec<cache::Holder> = WANTED.with(|w| {
        w.borrow()
            .iter()
            .filter(|(asked, _)| asked.try_with_value(|a| a.contains(&t.id)).unwrap_or(false))
            .map(|(_, h)| h.clone())
            .collect()
    });
    for holder in pages {
        holder.hold(t.clone());
    }
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
pub(crate) fn watch_children(parent: String, kids: RwSignal<Kids>, holder: cache::Holder) {
    OPEN_KIDS.with(|open| open.borrow_mut().push((parent, kids, holder)));
    on_cleanup(move || {
        OPEN_KIDS.with(|open| open.borrow_mut().retain(|(_, k, _)| *k != kids));
    });
}

/// Applies `change` to every open list of `parent`'s children.
fn kids_of(parent: &str, change: impl Fn(&mut Kids) -> bool) {
    let lists: Vec<RwSignal<Kids>> = OPEN_KIDS.with(|open| {
        open.borrow()
            .iter()
            .filter(|(p, _, _)| p == parent)
            .map(|(_, k, _)| *k)
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

/// SCALE-25: the connection as a signal — see `LINK`.
pub(crate) fn link() -> ArcRwSignal<bool> {
    LINK.with(Clone::clone)
}

/// A client over the live connection, or `None` before the first one is up.
///
/// This is how a *view* reaches the server: it asks for the client, makes
/// one typed call, and lets the answer upsert into the shared row store.
/// A view never touches the transport, and never keeps what it fetched
/// anywhere but the store (or, for a body, its own page state). SCALE-24:
/// the store is the row cache, where a view holds what it shows.
pub(crate) fn client() -> Option<Client> {
    TRANSPORT
        .with(|t| t.borrow().clone())
        .map(TicketsClient::new)
}

/// Renders a failed call for the banner. The two layers stay distinct:
/// `transport` means the socket, `remote` means the server refused the
/// call itself — neither is the activation's own in-band `Error` event,
/// which is data and is surfaced by the caller.
pub(crate) fn describe(e: &plexus_client::ClientError) -> String {
    match e {
        plexus_client::ClientError::Transport(t) => format!("socket: {t}"),
        other => other.to_string(),
    }
}

/// Start the data plane. Runs for the app's lifetime, regardless of route
/// — the ticket page's liveness is this same stream.
pub(crate) fn drive(
    board: board::Board,
    dock: RwSignal<Option<Dock>>,
    conn: RwSignal<Conn>,
    socket: RwSignal<u32>,
    last_error: RwSignal<Option<String>>,
    tree: nav::Tree,
) {
    spawn_local(async move {
        loop {
            let err = session(board, dock, conn, socket, last_error, tree)
                .await
                .expect_err("a session only returns on error");
            TRANSPORT.with(|t| *t.borrow_mut() = None);
            LINK.with(|l| l.set(false));
            leptos::logging::log!("session ended: {err}; reconnecting");
            conn.set(Conn::Reconnecting);
            dom::sleep_ms(RETRY_MS).await;
        }
    });
}

/// One connected session. `Err` ends it; [`drive`] reconnects.
async fn session(
    board: board::Board,
    dock: RwSignal<Option<Dock>>,
    conn: RwSignal<Conn>,
    socket: RwSignal<u32>,
    last_error: RwSignal<Option<String>>,
    tree: nav::Tree,
) -> Result<(), String> {
    let transport = Rc::new(
        WsTransport::connect(&dom::ws_url(), HUB)
            .await
            .map_err(|e| format!("connect {}: {e}", dom::ws_url()))?,
    );
    TRANSPORT.with(|t| *t.borrow_mut() = Some(transport.clone()));
    LINK.with(|l| l.set(true));
    // A connection exists — announced here, before the board's window,
    // because an open ticket page needs only two point queries and must not
    // wait for the board.
    socket.update(|g| *g += 1);
    let client = TicketsClient::new(transport);

    // 1. `watch` first — subscribed eagerly, so a ticket born while the
    //    calls below are out arrives behind them (MNG-9 acceptance 5).
    let mut live = client.watch(None);
    // COMP-5: the component registry, if a view wants it — after `watch`,
    // so nothing it misses is lost (src/registry.rs).
    registry::refresh();

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

    // 3. SCALE-24: the nav's shortcode rows — one roll-up per shortcode,
    //    whatever N is; the nav draws its top level from these alone.
    groups(&client, tree, last_error).await?;

    // 4. SCALE-26: where every ticket sits — each shortcode's last
    //    sequence, one item per shortcode (252 at 10^6, 1.9 ms) — and the
    //    board's window there: one page, whatever N is. The walk that stood
    //    here (every summary, 2,000 pages at 10^6) is gone.
    extents(&client, board, last_error).await?;
    board.refresh(&client).await?;

    // 5. Live. Events queued behind the calls above apply now, in order —
    //    a birth the extents or the window already counted moves nothing.
    conn.set(Conn::Live);
    while let Some(ev) = live.next().await {
        let ev = ev.map_err(|e| describe(&e))?;
        // WAVE-5: an open wave board hears it first (it only decides
        // whether to ask `waves` again).
        if let ClientEvent::Data(data) = &ev {
            hear(data);
            // COMP-5: the app's registry takes a component event, then
            // every open frame hears the event as a change notice.
            registry::heard(data);
            frame::broadcast_change(data);
        }
        match ev {
            // Every mutation event carries the full updated ticket —
            // AGS-19's `children_changed` carries the full *parent*, which
            // is how a page and a nav that never re-fetch learn that a
            // child was born (AGS-20).
            // SCALE-26: the board's extents move (the dock's count with
            // them); the row is held where the board holds its neighbours,
            // and by an open page that was waiting for it.
            ClientEvent::Data(TicketsEvent::Created { ticket }) => {
                let summary = TicketSummary::from(&ticket);
                // SCALE-24: the `children_changed` behind it names it.
                nav::arrived(&summary, true);
                wanted(&summary);
                board.created(&summary);
            }
            // SCALE-26: applied to the row where a view holds it, dropped
            // where none does.
            ClientEvent::Data(
                TicketsEvent::StatusChanged { ticket, .. }
                | TicketsEvent::UnlocksChanged { ticket, .. },
            ) => {
                cache::put(TicketSummary::from(&ticket));
            }
            // SCALE-4: the parent's id, the child and the new count — the
            // child's own row arrived with its `created`.
            // SCALE-24: and onto the nav's open list of the parent's children.
            ClientEvent::Data(TicketsEvent::ChildrenChanged { id, added, child_count }) => {
                cache::set_child_count(&id, child_count);
                kids_of(&id, |k| k.added(&added));
                nav::children_changed(&id, &added, child_count);
            }
            // MFE-17: a ticket was filed under another parent. Its row
            // takes the new `parent`. SCALE-24: the nav no longer reads the
            // board's list, so the list is not told; the two parents' sides
            // (`child_removed`, `children_changed`) move the nav's lists, and
            // a re-hosted shortcode moves with its `group_changed`.
            ClientEvent::Data(TicketsEvent::ParentChanged { ticket, .. }) => {
                let summary = TicketSummary::from(&ticket);
                nav::arrived(&summary, false);
                cache::put(summary);
            }
            // MFE-17: the old parent's side of that move — its count, and
            // any open page's list of its children.
            ClientEvent::Data(TicketsEvent::ChildRemoved { id, removed, child_count }) => {
                cache::set_child_count(&id, child_count);
                kids_of(&id, |k| k.removed(&removed));
                nav::child_removed(&id, &removed, child_count);
            }
            // SCALE-24 (MNG-10): a shortcode row's roll-up moved.
            ClientEvent::Data(TicketsEvent::GroupChanged { group }) => tree.group_changed(group),
            // WAVE-6: kept placements a write moved — on each row a view
            // holds (an open wave board heard it first, and moved its cards).
            ClientEvent::Data(TicketsEvent::PlacementsChanged { changes }) => {
                for c in changes {
                    cache::set_placement(&c.id, c.placement);
                }
            }
            // MNG-16: the row's title in place; the open page's body too.
            ClientEvent::Data(TicketsEvent::Amended { ticket, .. }) => {
                show_body(&ticket);
                cache::put(TicketSummary::from(&ticket));
            }
            // SDLC-9: commits linked or unlinked. They are on the row, so
            // the row takes them — and the open ticket page, which renders
            // through it, shows them; the body did not change.
            ClientEvent::Data(
                TicketsEvent::CommitsLinked { ticket, .. }
                | TicketsEvent::CommitsUnlinked { ticket, .. },
            ) => {
                cache::put(TicketSummary::from(&ticket));
            }
            // SCALE-5: the server dropped `missed` changes this stream never
            // saw. What the rows hold may be stale: fetch again what is
            // shown — every row, by the walk a reconnect makes, and the open
            // page's body. `watch` is whole again from here, so the dock
            // never leaves `live`.
            // SCALE-24: the nav's rows first — the groups again, and the
            // epoch they move makes every open nav list ask for its window.
            // SCALE-26: then the extents and the board's window — no walk;
            // what comes in behind the gap applies after them, and a birth
            // they already counted moves nothing — and the rows the open page
            // names (`stale`).
            ClientEvent::Data(TicketsEvent::Lagged { missed }) => {
                leptos::logging::log!("watch lagged: {missed} change(s) missed; asking again for what is shown");
                groups(&client, tree, last_error).await?;
                extents(&client, board, last_error).await?;
                board.refresh(&client).await?;
                refetch_open_body(&client, last_error).await;
                refetch_open_children(last_error);
                registry::refresh();
                STALE.with(|s| s.update(|n| *n = n.wrapping_add(1)));
            }
            // Future event kinds: ignore, stay live.
            _ => {}
        }
    }
    Err("watch stream ended".into())
}

/// SCALE-24: `groups {}` — every shortcode row the nav draws, with its
/// roll-up, root and host — handed to the nav. Its size is the number of
/// shortcodes (252 at 10^6), not tickets: 2.1 ms at 10^6 (MNG-17).
async fn groups(client: &Client, tree: nav::Tree, last_error: RwSignal<Option<String>>) -> Result<(), String> {
    let mut all = Vec::new();
    for ev in client.groups_all().await.map_err(|e| describe(&e))? {
        match ev {
            TicketsEvent::Group { group } => all.push(group),
            TicketsEvent::Error { message } => last_error.set(Some(format!("groups: {message}"))),
            _ => {}
        }
    }
    tree.set_groups(all);
    Ok(())
}

/// SCALE-26: `extents {}` — each shortcode's last sequence, in the store's
/// order: where every ticket sits, without a walk. Its size is the number of
/// shortcodes (252 at 10^6), not tickets: 1.9 ms at 10^6 (SCALE-18).
async fn extents(client: &Client, board: board::Board, last_error: RwSignal<Option<String>>) -> Result<(), String> {
    let mut runs = Vec::new();
    for ev in client.extents_all().await.map_err(|e| describe(&e))? {
        match ev {
            TicketsEvent::Extent { epic, last_seq } => runs.push((epic, last_seq)),
            TicketsEvent::Error { message } => last_error.set(Some(format!("extents: {message}"))),
            _ => {}
        }
    }
    board.set_extents(runs);
    Ok(())
}

/// SCALE-5: after a `lagged`, the open page's body — the one thing the
/// walk does not carry — fetched again, as a reconnect's `get` would. An
/// `amended` for it may have been among the changes lost. (SCALE-26: there is
/// no walk; the page's own row comes back with it.)
async fn refetch_open_body(client: &Client, last_error: RwSignal<Option<String>>) {
    let Some(id) = OPEN_BODY.with(|b| b.borrow().as_ref().map(|(id, _)| id.clone())) else {
        return;
    };
    match client.get_all(id.clone()).await {
        Ok(events) => {
            for ev in events {
                if let TicketsEvent::Ticket { ticket } = ev {
                    show_body(&ticket);
                    cache::put(TicketSummary::from(&ticket));
                }
            }
        }
        Err(e) => last_error.set(Some(format!("get {id}: {}", describe(&e)))),
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
/// dangling forward reference once `blockers_loaded` is set. SCALE-24: the
/// store is the row cache, and the page holds them there (`holder`) for as
/// long as it is open — at 10^6 a cold deep link's ticket is one the board's
/// walk will not reach for most of a minute. (SCALE-26: one no other view
/// holds at all.)
pub(crate) fn load_ticket(
    id: String,
    holder: cache::Holder,
    body: RwSignal<Option<String>>,
    not_found: RwSignal<bool>,
    blockers_loaded: RwSignal<bool>,
    last_error: RwSignal<Option<String>>,
) {
    spawn_local(async move {
        let Some(client) = client() else {
            // Not connected yet: the page's effect asks again when the
            // socket is up (`socket` ticks). Leave the body unfetched rather
            // than claiming the ticket does not exist.
            return;
        };
        match client.get_all(id.clone()).await {
            Ok(events) => {
                for ev in events {
                    match ev {
                        // `try_`: the page can have been left while `get`
                        // was out (SCALE-25's palette opens one and moves on
                        // in a breath), and its signals with it.
                        TicketsEvent::Ticket { ticket } => {
                            body.try_set(Some(ticket.body.clone()));
                            holder.hold(TicketSummary::from(&ticket));
                        }
                        TicketsEvent::NotFound { .. } => {
                            not_found.try_set(true);
                        }
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
                        holder.hold(TicketSummary::from(&ticket));
                    }
                }
            }
            Err(e) => last_error.set(Some(format!("blockers {id}: {}", describe(&e)))),
        }
        blockers_loaded.try_set(true);
    });
}

/// SCALE-26: rows a view names by id and nothing else holds — an open
/// page's ancestors, blockers and unlocks, the ticket a menu is open on — one
/// `get` each, all at once, held by the view. A `get` that finds nothing
/// leaves the id dangling until it is born (`watch_wanted`).
pub(crate) fn load_rows(ids: Vec<String>, holder: cache::Holder) {
    spawn_local(async move {
        let Some(client) = client() else {
            return; // not connected yet: the page asks again when it is
        };
        let gets = ids.iter().map(|id| client.get_all(id.clone()));
        for events in futures::future::join_all(gets).await.into_iter().flatten() {
            for ev in events {
                if let TicketsEvent::Ticket { ticket } = ev {
                    holder.hold(TicketSummary::from(&ticket));
                }
            }
        }
    });
}

/// SCALE-4 × SCALE-5: a `children_changed` or `child_removed` may be among
/// the changes a `lagged` lost, and nothing else carries a parent's list —
/// so every open page's children are asked for again from their first page,
/// which is what the page itself does on a reconnect.
fn refetch_open_children(last_error: RwSignal<Option<String>>) {
    let open: Vec<(String, RwSignal<Kids>, cache::Holder)> = OPEN_KIDS.with(|open| open.borrow().clone());
    for (parent, kids, holder) in open {
        load_children(parent, None, holder, kids, last_error);
    }
}

/// SCALE-4: one page of `id`'s children — the first (`after: None`, which
/// replaces what the page held: it is asked on entry and again on every
/// reconnect) or the next one after the cursor the last page handed back.
/// The rows upsert into the shared store, so each child renders live
/// through its own signal as before; the ids, in the store's order, go to
/// the page's list. SCALE-24: the store is the row cache; the page holds
/// the rows (`holder`). `list { parent }` costs the page at any size of parent
/// (the server keeps an index of children), so this is one round trip
/// whether the ticket has two children or 163,530.
pub(crate) fn load_children(
    id: String,
    after: Option<String>,
    holder: cache::Holder,
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
                holder.hold_page(page);
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
