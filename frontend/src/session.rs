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

//!
//! # Membership watches (FILTER-12)
//!
//! A view windowed over a filter's matches — the board under the lens, the
//! header's `n of N`, and next the nav and the inputs page — keeps its
//! counts live with `watch { filter, membership: true }`: only `entered`,
//! `left`, `recount` and `lagged`, since its rows already ride the
//! session's own `watch`. A view holds a [`Membership`] (open, replace,
//! close; [`follow_membership`] for one that follows a signal). Views of one
//! filter share one stream; a replaced filter's stream is dropped only once
//! the new one is subscribed; and every stream is opened again on each new
//! connection, right after the session's `watch` — none while no filter is
//! set, so the session's startup is what it was. A view is told
//! [`Member::Opened`] whenever its watch is open on a connection: when to
//! ask its counts.

use crate::kids::Kids;
use crate::{board, cache, comments, dom, frame, inputs, nav, registry, settings, signin, Conn, Dock};
use futures::StreamExt;
use leptos::prelude::*;
use menger_tickets_api::{CommentsClient, Marked, SearchWord, TicketsClient, Status, Ticket, TicketSummary, TicketsEvent};
use plexus_client::{ClientEvent, WsTransport};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use wasm_bindgen_futures::spawn_local;

/// menger's hub namespace — `DynamicHub::new("menger")` server-side. A
/// property of the server, not of the activation, which is why it is a
/// constructor argument and not part of a `MethodRef`.
pub(crate) const HUB: &str = "menger";

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

// ---------------------------------------------------------------------
// FILTER-12: membership watches

/// What a membership watch — `watch { filter, membership: true }` —
/// tells its consumers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Member {
    /// A write moved the ticket into the filter (`entered`).
    Entered { id: String, epic: String, seq: u32 },
    /// A write moved it out (`left`).
    Left { id: String, epic: String, seq: u32 },
    /// A write moved more than 256: the shortcode's count as the write
    /// left it (`recount`) — ask the window again.
    Recount { epic: String, held: u64 },
    /// The stream fell behind (`lagged`): what was counted may be stale —
    /// ask the counts again, then what is shown.
    Lagged,
    /// The watch is open on this connection — the first time, after a
    /// reconnect (a new socket, a new subscription), or at once for a
    /// consumer joining a stream already open: ask the counts now.
    Opened,
    /// The server did not take the filter (in-band `error`; the stream ends).
    Refused(String),
}

/// One stream per distinct filter: its consumers, and the pump reading it
/// on the current connection.
struct Feed {
    filter: String,
    ears: Vec<(u64, Rc<dyn Fn(&Member)>)>,
    /// The pump on this connection, and the number it was started under —
    /// `None` while there is no connection.
    pump: Option<(u64, futures::future::AbortHandle)>,
}

thread_local! {
    /// FILTER-12: the membership watches the app's views hold, by filter.
    static FEEDS: RefCell<Vec<Feed>> = const { RefCell::new(Vec::new()) };
    static NEXT_MEMBER: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };

    /// FILTER-12: the newest rows `watch` carried — what an `entered` beside
    /// a held row is drawn from without asking (its `status_changed` came on
    /// the session's stream; the cache dropped it, as no view held it).
    static RECENT: RefCell<std::collections::VecDeque<TicketSummary>> = const { RefCell::new(std::collections::VecDeque::new()) };
}

/// How many of the newest rows [`recent`] remembers.
const RECENT_ROWS: usize = 64;

fn next_member() -> u64 {
    NEXT_MEMBER.with(|c| {
        let n = c.get().wrapping_add(1);
        c.set(n);
        n
    })
}

fn remember(t: &TicketSummary) {
    RECENT.with(|r| {
        let mut r = r.borrow_mut();
        r.retain(|x| x.id != t.id);
        r.push_back(t.clone());
        while r.len() > RECENT_ROWS {
            r.pop_front();
        }
    });
}

/// A `watch` event that carries a ticket whose row it replaces whole —
/// `status_changed`, `kind_changed` (SDLC-13), `unlocks_changed` — applied:
/// remembered as the newest row for its id, and put on the row the cache
/// holds (dropped where no view holds it). True when a view holds it.
pub(crate) fn upsert(ev: &TicketsEvent) -> bool {
    let ticket = match ev {
        TicketsEvent::StatusChanged { ticket, .. }
        | TicketsEvent::KindChanged { ticket, .. }
        | TicketsEvent::UnlocksChanged { ticket, .. } => ticket,
        _ => return false,
    };
    let summary = TicketSummary::from(ticket);
    remember(&summary);
    cache::put(summary)
}

/// FILTER-12: the row `watch` last carried for `id`, if it was recent.
pub(crate) fn recent(id: &str) -> Option<TicketSummary> {
    RECENT.with(|r| r.borrow().iter().rev().find(|t| t.id == id).cloned())
}

/// FILTER-12: a consumer's membership watch — `watch { filter, membership:
/// true }` for its filter (canonical text), kept across reconnects: opened
/// on each new connection right after the session's own `watch`. Consumers
/// of one filter share one stream. A handle; nothing closes it but
/// [`Membership::close`] ([`follow_membership`] does that on cleanup).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Membership(u64);

impl Membership {
    /// Hear `filter`'s membership: `ear` is told every [`Member`] event,
    /// and [`Member::Opened`] whenever the watch is open on a connection —
    /// when to ask the counts.
    pub(crate) fn open(filter: String, ear: impl Fn(&Member) + 'static) -> Self {
        let me = Membership(next_member());
        attach(me.0, filter, Rc::new(ear), None);
        me
    }

    /// The filter replaced: the new filter's watch is opened before the old
    /// one is dropped (when no one else hears it). The same filter changes
    /// nothing.
    pub(crate) fn replace(self, filter: String) {
        let found = FEEDS.with(|f| {
            f.borrow().iter().find_map(|feed| {
                feed.ears.iter().find(|(id, _)| *id == self.0).map(|(_, ear)| (feed.filter.clone(), ear.clone()))
            })
        });
        let Some((old, ear)) = found else { return };
        if old == filter {
            return;
        }
        // Out of the old feed first (it hears nothing more), but its stream
        // is dropped only once the new one is subscribed.
        let then = detach(self.0, &old);
        attach(self.0, filter, ear, then);
    }

    /// Dropped: its stream goes once no consumer hears it.
    pub(crate) fn close(self) {
        let old = FEEDS.with(|f| f.borrow().iter().find(|feed| feed.ears.iter().any(|(id, _)| *id == self.0)).map(|feed| feed.filter.clone()));
        if let Some(old) = old {
            if let Some(abort) = detach(self.0, &old) {
                abort.abort(); // drops the stream: `call_unsub`
            }
        }
    }

    /// The filter it hears.
    #[allow(dead_code)] // FILTER-14, FILTER-16
    pub(crate) fn filter(self) -> Option<String> {
        FEEDS.with(|f| f.borrow().iter().find(|feed| feed.ears.iter().any(|(id, _)| *id == self.0)).map(|feed| feed.filter.clone()))
    }
}

/// FILTER-12: a membership watch that follows `filter` for the current
/// owner's lifetime — opened when it says a filter, replaced (new first)
/// when it says another, dropped when it says none and on cleanup.
#[allow(dead_code)] // FILTER-14, FILTER-16
pub(crate) fn follow_membership(filter: impl Fn() -> Option<String> + 'static, ear: impl Fn(&Member) + 'static) {
    let ear: Rc<dyn Fn(&Member)> = Rc::new(ear);
    let held = StoredValue::new(None::<Membership>);
    Effect::new(move |_| {
        let next = filter();
        match (held.get_value(), next) {
            (Some(m), Some(f)) => m.replace(f),
            (Some(m), None) => {
                m.close();
                held.set_value(None);
            }
            (None, Some(f)) => {
                let ear = ear.clone();
                held.set_value(Some(Membership::open(f, move |e| ear(e))));
            }
            (None, None) => {}
        }
    });
    on_cleanup(move || {
        if let Some(Some(m)) = held.try_get_value() {
            m.close();
        }
    });
}

/// `id` hears `filter`: joins its stream, or opens one (at once when there
/// is a connection; else the next session does). `then`: a stream to drop
/// once this one is subscribed (the filter it replaces).
fn attach(id: u64, filter: String, ear: Rc<dyn Fn(&Member)>, then: Option<futures::future::AbortHandle>) {
    let live = FEEDS.with(|f| {
        let mut feeds = f.borrow_mut();
        match feeds.iter_mut().find(|feed| feed.filter == filter) {
            Some(feed) => {
                feed.ears.push((id, ear.clone()));
                Some(feed.pump.is_some())
            }
            None => {
                feeds.push(Feed { filter: filter.clone(), ears: vec![(id, ear.clone())], pump: None });
                None
            }
        }
    });
    let opened = match live {
        // Joined a stream open on this connection: it is open for this ear
        // too — and what it replaces can go now.
        Some(true) => {
            spawn_local(async move { ear(&Member::Opened) });
            then
        }
        // Joined one waiting for a connection: the next session opens it.
        Some(false) => then,
        None => match client() {
            Some(client) => {
                pump(client, &filter, then);
                None
            }
            None => then,
        },
    };
    if let Some(abort) = opened {
        abort.abort();
    }
}

/// `id` no longer hears `filter`. The feed goes with its last ear: its
/// pump's handle is handed back, to be aborted (which drops the stream —
/// `call_unsub`).
fn detach(id: u64, filter: &str) -> Option<futures::future::AbortHandle> {
    let gone = FEEDS.with(|f| {
        let mut feeds = f.borrow_mut();
        let at = feeds.iter().position(|feed| feed.filter == filter)?;
        feeds[at].ears.retain(|(e, _)| *e != id);
        feeds[at].ears.is_empty().then(|| feeds.remove(at))
    });
    gone.and_then(|feed| feed.pump).map(|(_, abort)| abort)
}

/// Every consumer of `filter`, at this moment.
fn ears(filter: &str) -> Vec<Rc<dyn Fn(&Member)>> {
    FEEDS.with(|f| {
        f.borrow().iter().find(|feed| feed.filter == filter).map(|feed| feed.ears.iter().map(|(_, e)| e.clone()).collect()).unwrap_or_default()
    })
}

/// Open `filter`'s membership watch on `client` and pump it into its
/// consumers until the stream ends or the feed goes: the subscribe frame is
/// written as the pump starts (MACRO-6), then `then` (the stream it
/// replaces) is dropped, then each consumer is told it is
/// [`Member::Opened`].
fn pump(client: Client, filter: &str, then: Option<futures::future::AbortHandle>) {
    let n = next_member();
    let (abort, reg) = futures::future::AbortHandle::new_pair();
    let old = FEEDS.with(|f| {
        f.borrow_mut().iter_mut().find(|feed| feed.filter == filter).and_then(|feed| feed.pump.replace((n, abort)))
    });
    if let Some((_, old)) = old {
        old.abort();
    }
    let filter = filter.to_string();
    let run = async move {
        let mut stream = client.watch(None, Some(filter.clone()), Some(true));
        if let Some(then) = then {
            then.abort();
        }
        for ear in ears(&filter) {
            ear(&Member::Opened);
        }
        while let Some(ev) = stream.next().await {
            let member = match ev {
                Ok(ClientEvent::Data(TicketsEvent::Entered { id, epic, seq })) => Member::Entered { id, epic, seq },
                Ok(ClientEvent::Data(TicketsEvent::Left { id, epic, seq })) => Member::Left { id, epic, seq },
                Ok(ClientEvent::Data(TicketsEvent::Recount { epic, held })) => Member::Recount { epic, held },
                Ok(ClientEvent::Data(TicketsEvent::Lagged { missed })) => {
                    leptos::logging::log!("membership watch lagged: {missed} change(s) missed; counting again");
                    Member::Lagged
                }
                Ok(ClientEvent::Data(TicketsEvent::Error { message })) => Member::Refused(message),
                Ok(_) => continue,
                Err(_) => break, // the socket: the next session opens it again
            };
            for ear in ears(&filter) {
                ear(&member);
            }
        }
        FEEDS.with(|f| {
            if let Some(feed) = f.borrow_mut().iter_mut().find(|feed| feed.filter == filter) {
                if feed.pump.as_ref().is_some_and(|(m, _)| *m == n) {
                    feed.pump = None;
                }
            }
        });
    };
    spawn_local(async move {
        let _ = futures::future::Abortable::new(run, reg).await;
    });
}

/// A new connection: every membership watch the views hold, opened on it
/// — right after the session's own `watch`, before anything is counted.
/// True when there was one: its pump subscribes once the session yields.
fn reopen_memberships() -> bool {
    let filters: Vec<String> = FEEDS.with(|f| f.borrow().iter().map(|feed| feed.filter.clone()).collect());
    for filter in &filters {
        if let Some(client) = client() {
            pump(client, filter, None);
        }
    }
    !filters.is_empty()
}

/// Lets every task already queued run first — once.
struct YieldNow(bool);

impl std::future::Future for YieldNow {
    type Output = ();
    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        if self.0 {
            return std::task::Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
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

/// COMMENT-3: the `comments` activation's typed client, over the same
/// connection — `CommentsClient`, emitted from the activation's own `impl`
/// (COMMENT-2) as `TicketsClient` is.
pub(crate) fn comments_client() -> Option<CommentsClient<Rc<WsTransport>>> {
    TRANSPORT.with(|t| t.borrow().clone()).map(CommentsClient::new)
}

/// IDENTITY-22: the `members` activation's typed client, over the same
/// connection.
pub(crate) fn members_client() -> Option<menger_tickets_api::MembersClient<Rc<WsTransport>>> {
    TRANSPORT.with(|t| t.borrow().clone()).map(menger_tickets_api::MembersClient::new)
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
            // IDENTITY-31: what the Checking screen says, while it checks.
            if !signin::account().with_untracked(|a| a.known()) {
                signin::problem().set(Some(err.clone()));
            }
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
    // MFE-21: the override from #/settings when one is set, else this
    // page's server (`dom::ws_url()`) as before — fixed for the page's life.
    let url = settings::effective_ws_url();
    // IDENTITY-22: who this browser is, first. Signed out, the browser is
    // on its way to the login page; the session goes no further.
    signin::gate().await?;
    let transport = Rc::new(
        WsTransport::connect(&url, HUB)
            .await
            .map_err(|e| format!("connect {url}: {e}"))?,
    );
    TRANSPORT.with(|t| *t.borrow_mut() = Some(transport.clone()));
    LINK.with(|l| l.set(true));
    // IDENTITY-22: the person's grants, for the header and the views. With
    // none, the session asks nothing more: the app shows their id, and the
    // socket stays open until they reload or sign out.
    let allowed = signin::after_connect(transport.clone()).await?;
    signin::problem().set(None);
    if !allowed {
        LINK.with(|l| l.set(true));
        conn.set(Conn::Live);
        loop {
            dom::sleep_ms(60_000).await;
        }
    }
    // A connection exists — announced here, before the board's window,
    // because an open ticket page needs only two point queries and must not
    // wait for the board.
    socket.update(|g| *g += 1);
    let client = TicketsClient::new(transport);

    // 1. `watch` first — subscribed eagerly, so a ticket born while the
    //    calls below are out arrives behind them (MNG-9 acceptance 5).
    let mut live = client.watch(None, None, None);
    // FILTER-12: the views' membership watches, on this connection — after
    // `watch`, before anything they count is asked (none while no filter is
    // set: nothing new at startup).
    if reopen_memberships() {
        // Their subscribe frames next, behind `watch`'s, before `health`.
        YieldNow(false).await;
    }
    // COMP-5: the component registry, if a view wants it — after `watch`,
    // so nothing it misses is lost (src/registry.rs).
    registry::refresh();
    // INPUT-6: the inputs, likewise — only once #/inputs has asked.
    inputs::refresh();
    // COMMENT-3: the comments — `comments.watch`, then the list — only once
    // a view has asked (annotation mode, #/comments, a ticket page's count).
    comments::refresh();

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
            // INPUT-6: an `input_changed` upserts the app's inputs.
            inputs::heard(data);
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
                remember(&summary);
                // SCALE-24: the `children_changed` behind it names it.
                nav::arrived(&summary, true);
                wanted(&summary);
                board.created(&summary);
            }
            // SCALE-26: applied to the row where a view holds it, dropped
            // where none does. SDLC-13: a `kind_changed` as a
            // `status_changed` — the row upserts, so every view that draws
            // it (the nav's glyph, the board's kind and status, the ticket
            // page's kind) moves with it.
            ClientEvent::Data(
                ev @ (TicketsEvent::StatusChanged { .. }
                | TicketsEvent::KindChanged { .. }
                | TicketsEvent::UnlocksChanged { .. }),
            ) => {
                upsert(&ev);
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
                remember(&summary);
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
                let summary = TicketSummary::from(&ticket);
                remember(&summary);
                cache::put(summary);
            }
            // SDLC-9: commits linked or unlinked. They are on the row, so
            // the row takes them — and the open ticket page, which renders
            // through it, shows them; the body did not change.
            ClientEvent::Data(
                TicketsEvent::CommitsLinked { ticket, .. }
                | TicketsEvent::CommitsUnlinked { ticket, .. },
            ) => {
                let summary = TicketSummary::from(&ticket);
                remember(&summary);
                cache::put(summary);
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
                inputs::refresh();
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
///
/// FILTER-16: under the global filter, the list's filter (`Kids::
/// list_filter`) rides the same call — `list { parent, filter }` — and a
/// page asked under another filter than the list's when it lands is
/// dropped ([`Kids::page_under`]).
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
        let Some(filter) = kids.try_with_untracked(|k| k.list_filter().map(str::to_string)) else {
            return; // the page is gone
        };
        match client
            .list_all(None, None, Some(id.clone()), after, Some(KIDS_LIMIT), Some(true), filter.clone(), None, None)
            .await
        {
            Ok(events) => {
                let mut page: Vec<TicketSummary> = Vec::new();
                let mut next = None;
                let mut remaining = 0;
                for ev in events {
                    match ev {
                        TicketsEvent::Summary { summary } => page.push(summary),
                        TicketsEvent::Page { next: n, remaining: r, .. } => {
                            next = n;
                            remaining = r;
                        }
                        // FILTER-16: a filter the server did not take is the
                        // list's to say, not the banner's.
                        TicketsEvent::Error { message } if filter.is_some() => {
                            kids.try_update(|k| k.refuse(filter.as_deref(), message));
                            return;
                        }
                        TicketsEvent::Error { message } => {
                            last_error.set(Some(format!("children of {id}: {message}")))
                        }
                        _ => {}
                    }
                }
                let ids: Vec<String> = page.iter().map(|t| t.id.clone()).collect();
                holder.hold_page(page);
                kids.try_update(|k| k.page_under(filter.as_deref(), first, ids, next, remaining));
            }
            Err(e) => last_error.set(Some(format!("children of {id}: {}", describe(&e)))),
        }
    });
}

/// Kept honest by the compiler: `Ticket` is the only shape that carries a
/// body, and it is menger's own type. If the server renames the field this
/// stops compiling — which is the whole point of the crate split.
const _: fn(&Ticket) -> &String = |t| &t.body;

// ---------------------------------------------------------------------
// FILTER-16: the reads the palette, the inputs page and the ticket page
// make under the global filter (FILTER-4's `list { parent, filter }`,
// FILTER-15's `member`) — one block, beside the session's own.

/// FILTER-15: the most ids one `member` call takes.
const MEMBER_MAX: usize = 1000;

/// FILTER-16: `member { ids, filter }` — whether `filter` admits each id,
/// in the order asked, asked in parts of [`MEMBER_MAX`]. `Err`: no
/// connection, or the server's words (a filter it did not take).
pub(crate) async fn member(ids: Vec<String>, filter: String) -> Result<Vec<(String, bool)>, String> {
    let client = client().ok_or_else(|| "not connected".to_string())?;
    let mut out = Vec::with_capacity(ids.len());
    for part in ids.chunks(MEMBER_MAX) {
        for ev in client.member_all(part.to_vec(), filter.clone()).await.map_err(|e| describe(&e))? {
            match ev {
                TicketsEvent::Member { id, matched } => out.push((id, matched)),
                TicketsEvent::Error { message } => return Err(message),
                _ => {}
            }
        }
    }
    Ok(out)
}

/// FILTER-16: the palette's `find { query, limit, filter }` — its rows and
/// `remaining`, or (`Ok(Err)`) the server's words for a filter it did not
/// take. `Err`: the call failed (the socket).
pub(crate) async fn find_under(query: String, limit: u32, filter: Option<String>) -> Result<Result<(Vec<TicketSummary>, u64), String>, String> {
    let client = client().ok_or_else(|| "not connected".to_string())?;
    let events = client.find_all(query, None, Some(limit), filter).await.map_err(|e| describe(&e))?;
    let mut rows = Vec::new();
    let mut remaining = 0;
    for ev in events {
        match ev {
            TicketsEvent::Summary { summary } => rows.push(summary),
            TicketsEvent::Page { remaining: r, .. } => remaining = r,
            TicketsEvent::Error { message } => return Ok(Err(message)),
            _ => {}
        }
    }
    Ok(Ok((rows, remaining)))
}

/// SEARCH-3: one hit of the palette's `search` — its row, its title and
/// passage as marked stretches, and whether it matched only through a
/// word's correction.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hit {
    pub(crate) summary: TicketSummary,
    pub(crate) title: Vec<Marked>,
    pub(crate) snippet: Vec<Marked>,
    pub(crate) corrected: bool,
}

/// SEARCH-3: the palette's `search { query, limit, filter }` — its hits, how
/// many tickets match, and the words as the server read them; or
/// (`Ok(Err)`) the server's words for a filter it did not take. `Err`: the
/// call failed (the socket).
pub(crate) async fn search_under(
    query: String,
    limit: u32,
    filter: Option<String>,
) -> Result<Result<(Vec<Hit>, u64, Vec<SearchWord>), String>, String> {
    let client = client().ok_or_else(|| "not connected".to_string())?;
    let events = client.search_all(query, filter, Some(limit), None).await.map_err(|e| describe(&e))?;
    let (mut hits, mut matched, mut words) = (Vec::new(), 0, Vec::new());
    for ev in events {
        match ev {
            TicketsEvent::Hit { summary, title, snippet, corrected } => hits.push(Hit { summary, title, snippet, corrected }),
            TicketsEvent::Searched { matched: m, words: w, .. } => (matched, words) = (m, w),
            TicketsEvent::Error { message } => return Ok(Err(message)),
            _ => {}
        }
    }
    Ok(Ok((hits, matched, words)))
}

/// FILTER-16: `inputs { filter }` — every input (every state) of the
/// tickets `filter` admits, in the store's order (FILTER-15). `Err`: no
/// connection, or the server's words.
pub(crate) async fn inputs_under(filter: String) -> Result<Vec<TicketsEvent>, String> {
    let client = client().ok_or_else(|| "not connected".to_string())?;
    let events = client.inputs_all(None, None, None, Some(filter)).await.map_err(|e| describe(&e))?;
    if let Some(message) = events.iter().find_map(|ev| match ev {
        TicketsEvent::Error { message } => Some(message.clone()),
        _ => None,
    }) {
        return Err(message);
    }
    Ok(events)
}

/// FILTER-16: how many of `parent`'s children `filter` admits — `list {
/// parent, filter, limit: 0 }`'s `remaining`, from the cells (FILTER-4).
pub(crate) async fn children_matching(parent: String, filter: String) -> Result<u64, String> {
    let client = client().ok_or_else(|| "not connected".to_string())?;
    let mut remaining = None;
    for ev in client
        .list_all(None, None, Some(parent), None, Some(0), Some(true), Some(filter), None, None)
        .await
        .map_err(|e| describe(&e))?
    {
        match ev {
            TicketsEvent::Page { remaining: r, .. } => remaining = Some(r),
            TicketsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    remaining.ok_or_else(|| "no page".to_string())
}

#[cfg(test)]
mod tests {
    use super::{recent, upsert};
    use crate::cache;
    use leptos::prelude::*;
    use menger_tickets_api::{Kind, Placement, Status, Ticket, TicketSummary, TicketsEvent};

    fn ticket(id: &str, kind: Kind, status: Status) -> Ticket {
        let (epic, seq) = id.rsplit_once('-').expect("EPIC-N");
        Ticket {
            id: id.to_string(),
            epic: epic.to_string(),
            seq: seq.parse().expect("seq"),
            title: format!("the title of {id}"),
            status,
            kind,
            severity: None,
            blocked_by: vec![],
            unlocks: vec![],
            parent: None,
            child_count: 0,
            commits: vec![],
            path: None,
            placement: kind.has_lifecycle().then_some(Placement::Wave { n: 1 }),
            body: String::new(),
            created_unix: 0,
            updated_unix: 0,
        }
    }

    /// SDLC-13: a `kind_changed` is applied as a `status_changed` is — the
    /// row a view holds takes the ticket's new kind, status and placement in
    /// place (the one signal the nav's glyph, the board's row and the ticket
    /// page all draw it through), and it is the newest row `watch` carried
    /// for its id; a row no view holds is remembered, not inserted.
    #[test]
    fn a_kind_changed_upserts_the_row() {
        let held = cache::hold(TicketSummary::from(&ticket("KC-1", Kind::Doc, Status::Doc)));
        let scope = ticket("KC-1", Kind::Scope, Status::Epic);
        assert!(upsert(&TicketsEvent::KindChanged { ticket: scope, previous: Kind::Doc }));
        assert_eq!(
            held.with_untracked(|t| (t.kind, t.status, t.placement)),
            (Kind::Scope, Status::Epic, Some(Placement::Wave { n: 1 }))
        );
        assert_eq!(recent("KC-1").map(|t| t.kind), Some(Kind::Scope));

        // Back to a doc: placed nowhere again.
        assert!(upsert(&TicketsEvent::KindChanged { ticket: ticket("KC-1", Kind::Doc, Status::Doc), previous: Kind::Scope }));
        assert_eq!(held.with_untracked(|t| (t.kind, t.status, t.placement)), (Kind::Doc, Status::Doc, None));

        assert!(!upsert(&TicketsEvent::KindChanged { ticket: ticket("KC-2", Kind::Gap, Status::Pending), previous: Kind::Build }));
        assert!(cache::get("KC-2").is_none(), "no view holds it");
        assert_eq!(recent("KC-2").map(|t| t.kind), Some(Kind::Gap));
        cache::release("KC-1");
    }
}
