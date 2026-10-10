//! Plexus RPC surface for tickets — **the one declaration** (MFE-11).
//!
//! This `impl` is both halves of menger's `tickets` activation:
//!
//! | build | what the macro emits from it |
//! |---|---|
//! | `server` (default; the menger binary) | the `Activation` impl, dispatch, the schema, these bodies |
//! | `default-features = false` (the browser app, wasm32) | `TicketsClient<T>` — one typed method per `#[method]`, over the activation's own types |
//!
//! `client` asks for the client; `server_cfg` puts the server half of the
//! expansion — this impl block included — behind the `server` feature, and
//! the author's own server-only items (the struct, its store, the imports)
//! sit behind the same `cfg`. There is no second declaration anywhere: a
//! changed parameter here fails the browser build.
//!
//! `create` names one optional parameter per ticket field (AGS-19 added
//! `parent`) — that is the wire surface talking, here and in the wrapper
//! the activation macro generates, not a call signature asking to become
//! a struct.
#![allow(clippy::too_many_arguments)]

use crate::types::{InputValue, TicketsEvent};
#[cfg(feature = "server")]
use crate::store::{
    BlockerRef, Change, Filtered, InputWrite, Kind, Lens, Membership, NewTicket, Page, PageQuery, Select, Severity,
    Status, Step, Store, StoreError, Walk,
};
#[cfg(feature = "server")]
use crate::store::{TreePage, TreeQuery};
#[cfg(feature = "server")]
use crate::types::{AnswerSource, InputState};
// FILTER-15: the reads that take the lens.
#[cfg(feature = "server")]
use crate::store::{narrowing, narrowing_lens, ready_and, MEMBER_MAX};
// SEARCH-2: search inside tickets.
#[cfg(feature = "server")]
use crate::store::{SearchPage, SEARCH_LIMIT, SEARCH_MAX};
#[cfg(feature = "server")]
use async_stream::stream;
#[cfg(feature = "server")]
use futures::{Stream, StreamExt};
#[cfg(feature = "server")]
use std::sync::Arc;
#[cfg(feature = "server")]
use crate::access::{Access, Caller};
#[cfg(feature = "server")]
use crate::members::Role;
#[cfg(feature = "server")]
use plexus_core::plexus::AuthContext;

/// The activation. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Tickets {
    store: Arc<Store>,
    /// IDENTITY-19: the rule every method asks first ([`crate::access`]).
    access: Access,
}

#[cfg(feature = "server")]
impl Tickets {
    /// No login: every call admitted, as always.
    pub fn new(store: Arc<Store>) -> Self {
        Self::with_access(store, Access::open())
    }

    /// IDENTITY-19: behind [`Access`].
    pub fn with_access(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

/// MNG-9: one page of the store, as the events that carry it — a row per
/// ticket and then exactly one `page` event, which is what tells a client
/// whether to ask for more. A failed cursor is the one in-band `error`
/// instead, with no page event: nothing was walked, so there is nowhere to
/// resume from. SCALE-14: the `summary` shape — a page of the cache's rows,
/// as they are; whole tickets come from [`walk_events`].
#[cfg(feature = "server")]
fn page_events(page: Result<Page, StoreError>) -> Box<dyn Iterator<Item = TicketsEvent> + Send> {
    let page = match page {
        Ok(page) => page,
        Err(e) => {
            return Box::new(std::iter::once(TicketsEvent::Error {
                message: e.to_string(),
            }))
        }
    };
    Box::new(
        page.tickets
            .into_iter()
            .map(|summary| TicketsEvent::Summary { summary })
            .chain(std::iter::once(TicketsEvent::Page {
                next: page.next,
                remaining: page.remaining,
                first: page.first,
            })),
    )
}

/// SCALE-14: the same events with whole tickets — a `ticket` row each,
/// then the one `page` event — for `list` and `ready_set` without
/// `summary`. Streamed from a [`Walk`], a page of bodies at a time, so a
/// `list {}` of the whole store holds one page of bodies, not every one. A
/// failed cursor is still the one in-band `error` and nothing else (the
/// walk's first step); a read that fails part way ends the rows with an
/// `error` in place of the page event.
#[cfg(feature = "server")]
fn walk_events(store: Arc<Store>, mut walk: Walk) -> impl Stream<Item = TicketsEvent> + Send {
    stream! {
        loop {
            match walk.next(&store).await {
                Ok(Step::Tickets(tickets)) => {
                    for ticket in tickets {
                        yield TicketsEvent::Ticket { ticket };
                    }
                }
                Ok(Step::End { next, remaining, first }) => {
                    yield TicketsEvent::Page { next, remaining, first };
                    break;
                }
                Err(e) => {
                    yield TicketsEvent::Error { message: e.to_string() };
                    break;
                }
            }
        }
    }
}

/// SCALE-6: how many rows `find` answers when no `limit` is asked for —
/// the 50 the jump palette shows (AGS-18), so a keystroke's answer is a
/// page, not the store.
#[cfg(feature = "server")]
const FIND_LIMIT: u32 = 50;

/// FILTER-15: a `ready_set { filter }` page event as every `ready_set` page
/// is — without `first`, which only `list` gives (FILTER-4).
#[cfg(feature = "server")]
fn without_first(ev: TicketsEvent) -> TicketsEvent {
    match ev {
        TicketsEvent::Page { next, remaining, .. } => TicketsEvent::Page { next, remaining, first: None },
        ev => ev,
    }
}

/// A change as its one wire event — `None` for FILTER-4's cell moves, which
/// are never sent as themselves: `watch { filter }` turns them into
/// `entered`, `left` and `recount` for its filter.
#[cfg(feature = "server")]
fn wire_event(change: Change) -> Option<TicketsEvent> {
    Some(match change {
        Change::Created { ticket } => TicketsEvent::Created { ticket },
        Change::StatusChanged { ticket, previous } => {
            TicketsEvent::StatusChanged { ticket, previous }
        }
        Change::KindChanged { ticket, previous } => TicketsEvent::KindChanged { ticket, previous },
        Change::Amended { ticket, changed } => TicketsEvent::Amended { ticket, changed },
        Change::CommitsLinked { ticket, added } => TicketsEvent::CommitsLinked { ticket, added },
        Change::CommitsUnlinked { ticket, removed } => {
            TicketsEvent::CommitsUnlinked { ticket, removed }
        }
        Change::UnlocksChanged { ticket, added } => TicketsEvent::UnlocksChanged { ticket, added },
        // SCALE-4: the parent's shortcode filtered `watch { epic }` on the
        // way here; the wire carries the id and the count.
        Change::ChildrenChanged {
            id,
            added,
            child_count,
            ..
        } => TicketsEvent::ChildrenChanged {
            id,
            added,
            child_count,
        },
        Change::ParentChanged { ticket, previous } => {
            TicketsEvent::ParentChanged { ticket, previous }
        }
        Change::ChildRemoved {
            id,
            removed,
            child_count,
            ..
        } => TicketsEvent::ChildRemoved {
            id,
            removed,
            child_count,
        },
        Change::GroupChanged { group } => TicketsEvent::GroupChanged { group },
        // WAVE-6: each change's shortcode filtered `watch { epic }` on the
        // way here; the wire carries the id and the placement.
        Change::PlacementsChanged { changes } => TicketsEvent::PlacementsChanged {
            changes: changes.into_iter().map(|(_, change)| change).collect(),
        },
        // COMP-3: past any `epic` filter — a component is in no shortcode.
        Change::ComponentChanged { component } => TicketsEvent::ComponentChanged { component },
        Change::ComponentRemoved { name } => TicketsEvent::ComponentRemoved { name },
        // INPUT-3: its ticket's shortcode filtered `watch { epic }` on the
        // way here; the wire carries the input.
        Change::InputChanged { input, .. } => TicketsEvent::InputChanged { input },
        Change::CellsMoved { .. } => return None,
        // FILTER-13: each `watch { filter }` asking `under` takes its own.
        Change::UnderMoved { .. } => return None,
    })
}

/// FILTER-13: one page of `tree` as its events — a `node` per row, then one
/// `page` (no `first`); a refusal is the one in-band `error`, and nothing
/// else.
#[cfg(feature = "server")]
fn tree_events(page: Result<TreePage, StoreError>) -> Vec<TicketsEvent> {
    match page {
        Err(e) => vec![TicketsEvent::Error { message: e.to_string() }],
        Ok(page) => page
            .nodes
            .into_iter()
            .map(|n| TicketsEvent::Node {
                summary: n.summary,
                matched: n.matched,
                below: n.below,
            })
            .chain(std::iter::once(TicketsEvent::Page {
                next: page.next,
                remaining: page.remaining,
                first: None,
            }))
            .collect(),
    }
}

/// FILTER-4 / FILTER-13: a `watch { filter }`'s membership events as the
/// wire carries them.
#[cfg(feature = "server")]
fn membership_event(m: Membership) -> TicketsEvent {
    match m {
        Membership::Entered { id, epic, seq } => TicketsEvent::Entered { id, epic, seq },
        Membership::Left { id, epic, seq } => TicketsEvent::Left { id, epic, seq },
        Membership::Recount { epic, held } => TicketsEvent::Recount { epic, held },
    }
}

/// A write's own answer: the change it made, which is always one that
/// crosses the wire.
#[cfg(feature = "server")]
fn change_event(change: Change) -> TicketsEvent {
    wire_event(change).expect("a write answers with a change the wire carries")
}

/// INPUT-3: a write to one input, as its one event — `input_changed` for a
/// change, `input` for one that changed nothing, `not_found` for a ticket
/// that is none, an in-band `error` for every refusal.
#[cfg(feature = "server")]
fn input_event(written: Result<InputWrite, StoreError>, ticket: String) -> TicketsEvent {
    match written {
        Ok(InputWrite::Changed(input)) => TicketsEvent::InputChanged { input },
        Ok(InputWrite::Unchanged(input)) => TicketsEvent::Input { input },
        Err(StoreError::NotFound(_)) => TicketsEvent::NotFound { id: ticket },
        Err(e) => TicketsEvent::Error { message: e.to_string() },
    }
}

// The attribute is NOT `#[cfg]`-gated: it has to run in the client-only
// build too, or there would be no client. `server_cfg` tells it to put the
// server half — this impl block included — behind the feature.
#[plexus_macros::activation(
    namespace = "tickets",
    description = "Plans as data — create, query, and watch the work DAG",
    client,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core"
)]
impl Tickets {
    /// Heartbeat. Returns the binary version, the server's unix time, and
    /// how many tickets the store holds.
    #[plexus_macros::method(description = "Heartbeat — version, unix time, ticket count")]
    pub async fn health(&self, auth: Option<&AuthContext>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            yield TicketsEvent::Health {
                version: env!("CARGO_PKG_VERSION").to_string(),
                unix_now: chrono::Utc::now().timestamp(),
                // IDENTITY-21: what this caller can see.
                ticket_count: visible_count(&store, &caller),
            };
        }
    }

    /// Creates a ticket in an epic. The store allocates the next `EPIC-N`
    /// id; status starts at `Pending` (only a human promotes to `Ready`);
    /// `kind` defaults to `build`, and one outside the six yields the
    /// in-band error naming them. SDLC-11: a `doc` starts — and stays — in
    /// `Doc`; it may be a shortcode's first ticket, as a scope may; a doc
    /// with a `blocked_by`, a `blocked_by` naming a doc, and a doc born as
    /// an id some `blocked_by` already names are in-band errors (a doc never
    /// completes, so nothing may wait on one). `parent` (AGS-19) files the ticket as an
    /// output of an existing ticket in any epic; one that names no ticket,
    /// or that would make the newborn its own ancestor, is an in-band error
    /// naming the reason. MNG-14: omitted, `parent` defaults to the
    /// shortcode's scope root; given, it must be inside the same shortcode
    /// — except for the scope root itself, whose parent is the shortcode
    /// it hangs under. Every shortcode starts with a scope (MNG-12) or a
    /// doc (SDLC-11): the first ticket of a prefix must be `kind: scope` or
    /// `kind: doc`, and anything else is an in-band error naming the rule,
    /// with nothing allocated. WAVE-10: `path` gives the ticket a path
    /// address in its shortcode (`<SHORTCODE>/<path>`, which `get` resolves)
    /// — accepted only when it starts with `_MENGER/` and names something
    /// after it, `kind` is `doc`, and `parent` is the shortcode's scope root
    /// (or omitted, which defaults to it); a path another ticket of the
    /// shortcode already holds is an in-band error naming that ticket.
    /// COMP-3: a body's ` ```component <tag> ` blocks register components
    /// (`watch` says so after the rest of the birth); a block whose tag is
    /// not a component name, a name defined twice, and a name another
    /// ticket defines are in-band errors — the last naming that ticket —
    /// with nothing stored or allocated.
    /// Yields exactly one event: `Created` or `Error`.
    #[plexus_macros::method(
        description = "Create a ticket; allocates the next EPIC-N id, status starts Pending (a doc's: Doc, for good); the first ticket of a prefix must be a scope or a doc",
        params(
            epic = "Epic prefix, e.g. MNG (case-insensitive, stored uppercase)",
            title = "Short description",
            kind = "scope | execution | build | gap | spike | doc (default: build; the first ticket of a prefix must be scope or doc; a doc has no lifecycle and takes no blocked_by)",
            severity = "Critical | High | Medium | Low (optional)",
            blocked_by = "Ticket ids that must complete first (optional; never a doc, and not for a doc)",
            body = "Markdown body (optional)",
            parent = "Id of the ticket this one is an output of; must exist and be in the same shortcode (default: the shortcode's scope root). The shortcode's own scope root may name a ticket in another shortcode: the one it hangs under",
            path = "Path address inside the shortcode, e.g. _MENGER/board — then `get` finds it as SHORTCODE/_MENGER/board (optional; only _MENGER/… on a doc directly under the shortcode's scope root, unique in the shortcode)"
        )
    )]
    pub async fn create(
        &self, auth: Option<&AuthContext>,
        epic: String,
        title: String,
        kind: Option<String>,
        severity: Option<String>,
        blocked_by: Option<Vec<String>>,
        body: Option<String>,
        parent: Option<String>,
        path: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            // IDENTITY-21: editor on the parent, or on the shortcode's root;
            // a shortcode with neither needs editor everywhere.
            let anchor = match parent.as_deref() {
                Some(p) => Some(p.to_string()),
                None => store.groups().into_iter().find(|g| g.epic.eq_ignore_ascii_case(&epic)).and_then(|g| g.root),
            };
            let allowed = match &anchor {
                Some(a) => caller.require(Role::Editor, &[a.as_str()], &store),
                None if caller.everywhere(Role::Editor) => Ok(()),
                None => Err(format!("a new shortcode {} needs editor on *", epic.to_ascii_uppercase())),
            };
            if let Err(message) = allowed {
                yield TicketsEvent::Error { message };
                return;
            }
            let kind = match kind.as_deref().map(str::parse::<Kind>) {
                None => Kind::Build,
                Some(Ok(k)) => k,
                Some(Err(e)) => {
                    yield TicketsEvent::Error { message: e };
                    return;
                }
            };
            let severity = match severity.as_deref().map(str::parse::<Severity>) {
                None => None,
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    yield TicketsEvent::Error { message: e };
                    return;
                }
            };
            match store.create(NewTicket {
                epic,
                title,
                kind,
                severity,
                blocked_by: blocked_by.unwrap_or_default(),
                parent,
                path,
                body: body.unwrap_or_default(),
            }).await {
                Ok(ticket) => yield TicketsEvent::Created { ticket },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// Yields exactly one event: `Ticket` if found, `NotFound` otherwise.
    /// WAVE-10: `id` may be a path address, `<SHORTCODE>/<path>` — the
    /// shortcode case-insensitive, the path exact — naming the one ticket of
    /// that shortcode that holds the path; `NotFound` carries the address.
    #[plexus_macros::method(
        description = "Get one ticket by id, or by path address SHORTCODE/path (e.g. PLEXUS/_MENGER/board)",
        params(id = "Ticket id, e.g. MNG-2 — or a path address, e.g. PLEXUS/_MENGER/board")
    )]
    pub async fn get(&self, auth: Option<&AuthContext>, id: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if caller.require(Role::Viewer, &[id.as_str()], &store).is_err() {
                yield TicketsEvent::NotFound { id: id };
                return;
            }
            match store.get(&id).await {
                Ok(ticket) => yield TicketsEvent::Ticket { ticket },
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// Yields one row event per match, ordered by (epic, seq), then
    /// exactly one `page` event saying where the walk stands. The
    /// `parent` filter (AGS-19) yields exactly that ticket's children —
    /// since SCALE-4 the one way to read them (a row carries only their
    /// `child_count`), and a page of them costs the page.
    ///
    /// MNG-9 adds the page: `after` is the exclusive cursor (a ticket id
    /// — the order is total and unique, so an id is a position), `limit`
    /// caps the page, and `summary` swaps the `ticket` rows for `summary`
    /// rows that carry every field but the body. Without them the call is
    /// what it always was — every match, with bodies — followed by
    /// `page { next: null, remaining: 0 }`. An `after` naming no ticket
    /// is one in-band `error` and nothing else. SCALE-14: the rows come
    /// from the cache and the bodies from the database, read a page at a
    /// time as the rows go out — the same events, never every body at
    /// once.
    ///
    /// FILTER-4: `filter` (the language's text, FILTER-10) narrows the
    /// matches further — ANDed with `epic`, `status` and `parent`, which are
    /// sugar beside it — answered from the cells, never by testing a
    /// ticket; and `at` starts the page at a position among the matches
    /// (0-based) instead of after a cursor. Given either, the page event
    /// carries `first`, the page's first row's position among the matches;
    /// `at` past the end is an empty page with `first = at` and `next:
    /// null`; `at` with `after` is an in-band `error`, and so is a filter
    /// the language does not take (every bad chunk named) or one that
    /// asks `under` or `text` (each named, with where it lives). Given
    /// neither, every event is what it was.
    #[plexus_macros::method(
        description = "List tickets, optionally filtered by epic, status, parent and/or a filter; paged with after + limit, or at a position with at",
        params(
            epic = "Epic prefix filter (optional)",
            status = "Status filter: Pending | Ready | Coding | In Review | Blocked | Complete | Idea | Epic | Superseded | Doc (optional)",
            parent = "Only this ticket's children, e.g. PERF-3 (optional)",
            after = "Cursor: start strictly after this ticket id, e.g. MNG-9 (optional)",
            limit = "At most this many tickets; the page event carries the next cursor (optional)",
            summary = "Yield summary rows (every field but the body) instead of full tickets (optional)",
            filter = "A filter, e.g. 'ready and kind build and in SCALE' (FILTER-3's language; optional): ANDed with epic, status and parent; the page event then carries first",
            at = "Start at this position among the matches, 0-based, instead of after a cursor (optional; not with after); the page event carries first"
        )
    )]
    pub async fn list(
        &self, auth: Option<&AuthContext>,
        epic: Option<String>,
        status: Option<String>,
        parent: Option<String>,
        after: Option<String>,
        limit: Option<u32>,
        summary: Option<bool>,
        filter: Option<String>,
        at: Option<u64>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let filter = match caller.scoped(filter.as_deref()) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let status = match status.as_deref().map(str::parse::<Status>) {
                None => None,
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    yield TicketsEvent::Error { message: e };
                    return;
                }
            };
            if filter.is_some() || at.is_some() {
                // FILTER-4: from the cells.
                if after.is_some() && at.is_some() {
                    yield TicketsEvent::Error { message: StoreError::AtAndAfter.to_string() };
                    return;
                }
                let lens = match filter.as_deref().map(Lens::parse).unwrap_or_else(|| Ok(Lens::all())) {
                    Ok(lens) => lens,
                    Err(e) => {
                        yield TicketsEvent::Error { message: e.to_string() };
                        return;
                    }
                };
                let q = Filtered { lens, epic, status, parent };
                if summary.unwrap_or(false) {
                    for ev in page_events(store.filtered_page(&q, after.as_deref(), at, limit)) {
                        yield ev;
                    }
                } else {
                    let walk = store.walk(Select::Filtered(q), after.as_deref(), limit).at(at);
                    let mut events = Box::pin(walk_events(store, walk));
                    while let Some(ev) = events.next().await {
                        yield ev;
                    }
                }
                return;
            }
            if summary.unwrap_or(false) {
                let page = store.page(PageQuery {
                    epic: epic.as_deref(),
                    status,
                    parent: parent.as_deref(),
                    after: after.as_deref(),
                    limit,
                });
                for ev in page_events(page) {
                    yield ev;
                }
            } else {
                let walk = store.walk(Select::List { epic, status, parent }, after.as_deref(), limit);
                let mut events = Box::pin(walk_events(store, walk));
                while let Some(ev) = events.next().await {
                    yield ev;
                }
            }
        }
    }

    /// Yields exactly one event: `StatusChanged`, `NotFound`, or `Error`.
    /// SDLC-7: the vocabulary is nine — the worker states `Coding` and
    /// `In Review` included — and the write is free: any status to any
    /// other, nothing gated (SDLC-4 owns transitions). SDLC-11: a doc has no
    /// lifecycle — `set_status` on one is an in-band `Error`, whatever the
    /// status — and `Doc` is a doc's own: setting it on any other kind is an
    /// in-band `Error` too. Neither writes anything.
    #[plexus_macros::method(
        description = "Set a ticket's status (not a doc's: a doc has no lifecycle)",
        params(
            id = "Ticket id, e.g. MNG-2",
            status = "New status: Pending | Ready | Coding | In Review | Blocked | Complete | Idea | Epic | Superseded (case-insensitive; `active` is Coding; never Doc, a doc's own)"
        )
    )]
    pub async fn set_status(
        &self, auth: Option<&AuthContext>,
        id: String,
        status: String,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.require(Role::Editor, &[id.as_str()], &store) {
                yield TicketsEvent::Error { message };
                return;
            }
            let status = match status.parse::<Status>() {
                Ok(s) => s,
                Err(e) => {
                    yield TicketsEvent::Error { message: e };
                    return;
                }
            };
            match store.set_status(&id, status).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// SDLC-13: changes a ticket's kind — the one write that does — and
    /// everything kept by kind with it, in the one write. Yields exactly one
    /// event: `KindChanged { ticket, previous }`, `NotFound` if `id` names no
    /// ticket, or an in-band `Error` naming what it refuses. The status goes
    /// with the kind across the doc line: to a doc it becomes `Doc` (a
    /// `status` other than `Doc` is an `Error`); from a doc it becomes
    /// `status` when given (never `Doc`), else `Epic` for a scope and
    /// `Pending` for any other kind; between two other kinds it stays unless
    /// `status` is given (`Doc` an `Error`). Refused: a ticket that waits on
    /// a blocker, or that a ticket waits on, becoming a doc (both named); a
    /// shortcode's root becoming a kind that roots nothing (the shortcode
    /// named — doc ↔ scope on a root is allowed); a ticket holding a path
    /// address becoming work; an unknown kind or status. A lower-seq ticket
    /// that becomes a scope or a doc becomes its shortcode's root. The kind
    /// (and status) it already has writes and sends nothing: `KindChanged`
    /// with `previous` equal to its kind. `watch` carries `KindChanged`
    /// first, then the `GroupChanged`, `PlacementsChanged` and (filtered)
    /// `Entered` / `Left` it caused, as for a status change.
    #[plexus_macros::method(
        description = "Set a ticket's kind; everything kept by kind moves with it (to a doc: status Doc; from a doc: the status given, else Epic for a scope and Pending otherwise)",
        params(
            id = "Ticket id, e.g. IDENTITY-1",
            kind = "New kind: scope | execution | build | gap | spike | doc (case-insensitive)",
            status = "Its status with the new kind (optional): Pending | Ready | Coding | In Review | Blocked | Complete | Idea | Epic | Superseded — or Doc, the only one a doc takes; omitted, a doc becomes Doc, a doc becoming work becomes Epic (a scope) or Pending, and work keeps its own"
        )
    )]
    pub async fn set_kind(
        &self, auth: Option<&AuthContext>,
        id: String,
        kind: String,
        status: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.require(Role::Editor, &[id.as_str()], &store) {
                yield TicketsEvent::Error { message };
                return;
            }
            let kind = match kind.parse::<Kind>() {
                Ok(k) => k,
                Err(e) => {
                    yield TicketsEvent::Error { message: e };
                    return;
                }
            };
            let status = match status.as_deref().map(str::parse::<Status>) {
                None => None,
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    yield TicketsEvent::Error { message: e };
                    return;
                }
            };
            match store.set_kind(&id, kind, status).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// MNG-12: files an existing ticket as an output of `parent` — the
    /// verb AGS-19 left out. Adopts a ticket born parentless, or moves
    /// one. Yields exactly one event: `ParentChanged` (the child, with the
    /// parent it had before), `NotFound` if `id` names no ticket, or an
    /// in-band `Error` when the parent does not exist or the chain would
    /// make the ticket its own ancestor. `watch` also carries the parents'
    /// side: `ChildrenChanged` for the new parent and `ChildRemoved` for
    /// the old one — each the parent's id and new `child_count` (SCALE-4).
    #[plexus_macros::method(
        description = "Set a ticket's parent — the ticket it is an output of; adopts a parentless ticket or moves one",
        params(
            id = "Ticket id to file, e.g. AGS-2",
            parent = "Id of the ticket to file it under; any epic, must exist"
        )
    )]
    pub async fn set_parent(
        &self, auth: Option<&AuthContext>,
        id: String,
        parent: String,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.require(Role::Editor, &[id.as_str(), parent.as_str()], &store) {
                yield TicketsEvent::Error { message };
                return;
            }
            match store.set_parent(&id, &parent).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// MNG-16: replaces a ticket's title and/or body — the correction a
    /// DB-first store otherwise has no way to make (a worker's branch, a
    /// divergence note, a contract patched with its finding). Whole values,
    /// last writer wins. Yields exactly one event: `Amended { ticket,
    /// changed }` — `changed` names what moved, and is empty when nothing
    /// did (neither given, or the values already there), which writes and
    /// broadcasts nothing — `NotFound` if `id` names no ticket, or an
    /// in-band `Error` for a title that is empty after trimming. `watch`
    /// carries every `Amended` that changed something. COMP-3: a new body
    /// moves the component registry — its ` ```component ` blocks and `<tag`
    /// references — and `watch` says so after the `Amended`; a malformed
    /// block, or a name another ticket defines, is an in-band `Error` (the
    /// latter naming that ticket) and nothing is written, title included.
    #[plexus_macros::method(
        description = "Replace a ticket's title and/or body (whole values); yields amended { ticket, changed }",
        params(
            id = "Ticket id, e.g. MNG-2",
            title = "New title (optional; must not be blank)",
            body = "New markdown body, the whole of it (optional)"
        )
    )]
    pub async fn amend(
        &self, auth: Option<&AuthContext>,
        id: String,
        title: Option<String>,
        body: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.require(Role::Editor, &[id.as_str()], &store) {
                yield TicketsEvent::Error { message };
                return;
            }
            match store.amend(&id, title, body).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// SDLC-9: records which commits finished a ticket. Each entry is a
    /// commit reference — a hexadecimal object name of 7 to 64 characters
    /// (stored lowercase), optionally prefixed by a repository and `@`
    /// (`77df9a0`, `plexus-macros@4f7114a`) — compared as text: the server
    /// has no repository, so link the form you want kept. Every entry not
    /// already on the ticket is appended, in the order given. Yields
    /// exactly one event: `CommitsLinked { ticket, added }` — `added` is
    /// what was newly linked, empty when nothing was (every entry already
    /// linked, or none given), which writes and broadcasts nothing —
    /// `NotFound` if `id` names no ticket, or an in-band `Error` naming
    /// every entry that is not a reference, or for a link past 256 commits
    /// on the ticket; an error writes nothing. Moves `updated_unix` and
    /// nothing else — not the status. `watch` carries every
    /// `CommitsLinked` that added something.
    #[plexus_macros::method(
        description = "Link commits to a ticket (the commits that finished it); yields commits_linked { ticket, added }",
        params(
            id = "Ticket id, e.g. MNG-10",
            commits = "Commit references, in link order: a hex object name of 7-64 characters, optionally repo@ before it — e.g. [\"77df9a0\", \"plexus-macros@4f7114a\"]"
        )
    )]
    pub async fn link_commits(
        &self, auth: Option<&AuthContext>,
        id: String,
        commits: Vec<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.require(Role::Editor, &[id.as_str()], &store) {
                yield TicketsEvent::Error { message };
                return;
            }
            match store.link_commits(&id, commits).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// SDLC-9: the inverse of `link_commits` — every entry the ticket
    /// carries comes off. The same references, compared the same way, all
    /// or nothing. Yields exactly one event: `CommitsUnlinked { ticket,
    /// removed }` — `removed` empty when none of them was linked, which
    /// writes and broadcasts nothing — `NotFound`, or an in-band `Error`
    /// naming every entry that is not a reference. `watch` carries every
    /// `CommitsUnlinked` that removed something.
    #[plexus_macros::method(
        description = "Unlink commits from a ticket; yields commits_unlinked { ticket, removed }",
        params(
            id = "Ticket id, e.g. MNG-10",
            commits = "Commit references to remove, as linked (the hash is compared lowercase)"
        )
    )]
    pub async fn unlink_commits(
        &self, auth: Option<&AuthContext>,
        id: String,
        commits: Vec<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.require(Role::Editor, &[id.as_str()], &store) {
                yield TicketsEvent::Error { message };
                return;
            }
            match store.unlink_commits(&id, commits).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// The DAG query: which tickets can start now? Yields one row event
    /// per member of the ready set — status `Ready` and every
    /// `blocked_by` entry `Complete` or `Superseded`; never a doc (SDLC-11),
    /// whose status is `Doc` for good — ordered by
    /// (epic, seq), then one `page` event. A dangling blocker keeps its
    /// ticket out (use `blockers` to see which ids dangle). The epic
    /// filter applies to membership; blockers resolve globally.
    /// `after` / `limit` / `summary` page it exactly as `list` (MNG-9).
    ///
    /// FILTER-15: `filter` (the language's text, FILTER-10) narrows it to
    /// the ready set ∧ the filter — a filtered page of `ready and (filter)`
    /// from the cells (`ready` is the ready set's rule), paged and counted
    /// as without it, the page event without `first`. A filter not taken,
    /// or one that asks `under` or `text`, is one in-band `error`; one that
    /// narrows nothing (empty, a display part alone) is no filter.
    #[plexus_macros::method(
        description = "Tickets that can start now: Ready, with every blocker Complete or Superseded; optionally under a filter",
        params(
            epic = "Only tickets in this epic (optional; blockers still resolve globally)",
            after = "Cursor: start strictly after this ticket id, e.g. MNG-9 (optional)",
            limit = "At most this many tickets; the page event carries the next cursor (optional)",
            summary = "Yield summary rows (every field but the body) instead of full tickets (optional)",
            filter = "A filter, e.g. 'kind build and in SCALE' (FILTER-3's language; optional): the ready set and the filter"
        )
    )]
    pub async fn ready_set(
        &self, auth: Option<&AuthContext>,
        epic: Option<String>,
        after: Option<String>,
        limit: Option<u32>,
        summary: Option<bool>,
        filter: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let filter = match caller.scoped(filter.as_deref()) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            // FILTER-15: from the cells.
            match narrowing(filter.as_deref()).and_then(|f| f.map(|f| ready_and(&f, epic.clone())).transpose()) {
                Err(e) => {
                    yield TicketsEvent::Error { message: e.to_string() };
                    return;
                }
                Ok(Some(q)) => {
                    if summary.unwrap_or(false) {
                        for ev in page_events(store.ready_filtered_page(&q, after.as_deref(), limit)) {
                            yield ev;
                        }
                    } else {
                        let walk = store.walk(Select::Filtered(q), after.as_deref(), limit);
                        let mut events = Box::pin(walk_events(store, walk));
                        while let Some(ev) = events.next().await {
                            yield without_first(ev);
                        }
                    }
                    return;
                }
                Ok(None) => {}
            }
            if summary.unwrap_or(false) {
                for ev in page_events(store.ready_page(epic.as_deref(), after.as_deref(), limit)) {
                    yield ev;
                }
            } else {
                let walk = store.walk(Select::Ready { epic }, after.as_deref(), limit);
                let mut events = Box::pin(walk_events(store, walk));
                while let Some(ev) = events.next().await {
                    yield ev;
                }
            }
        }
    }

    /// SCALE-6: the jump palette's question, answered where the rows are
    /// — so a client that does not hold every row (SCALE-7) can still
    /// find one. Yields one `summary` row per ticket whose id or title
    /// contains `query`, case-insensitively with ASCII folding (AGS-18's
    /// rule, the palette's own), in (epic, seq) order — at most `limit`,
    /// default 50, what the palette shows — then exactly one `page`
    /// event: `remaining` plus the rows is the palette's total. `query`
    /// is trimmed; empty or blank matches every ticket, so the palette
    /// opens on the head of the store. `after` pages it exactly as
    /// `list` (MNG-9) — a position, not a match — and one naming no
    /// ticket is one in-band `error`. Ids and titles only: a jump aid,
    /// not search over bodies.
    ///
    /// FILTER-15: `filter` (the language's text, FILTER-10) keeps the text
    /// matches the filter admits, in the same order, paged and counted as
    /// without it — the needle tested only on the candidates the filter's
    /// cells admit, never on the rest of the store. A filter not taken, or
    /// one that asks `under` or `text`, is one in-band `error`; one that
    /// narrows nothing (empty, a display part alone) is no filter.
    #[plexus_macros::method(
        description = "Find tickets whose id or title contains the query (case-insensitive), optionally under a filter; summary rows, paged with after + limit",
        params(
            query = "Substring of the ticket id or title, case-insensitive (ASCII), trimmed; empty matches every ticket",
            after = "Cursor: start strictly after this ticket id, e.g. MNG-9 (optional)",
            limit = "At most this many tickets (default 50); the page event carries the next cursor (optional)",
            filter = "A filter, e.g. 'not category done' (FILTER-3's language; optional): only the matches it admits"
        )
    )]
    pub async fn find(
        &self, auth: Option<&AuthContext>,
        query: String,
        after: Option<String>,
        limit: Option<u32>,
        filter: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let filter = match caller.scoped(filter.as_deref()) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let limit = Some(limit.unwrap_or(FIND_LIMIT));
            let page = match narrowing_lens(filter.as_deref()) {
                Err(e) => Err(e),
                // FILTER-15: the candidates the cells admit.
                Ok(Some(lens)) => store.find_filtered(&query, &lens, after.as_deref(), limit),
                Ok(None) => store.find_page(&query, after.as_deref(), limit),
            };
            for ev in page_events(page) {
                yield ev;
            }
        }
    }

    /// SEARCH-2: search inside tickets — the tickets whose title or body's
    /// text holds every word of `query`, best first, from the search index
    /// in the database (the store's `search`): a word matches itself and as
    /// a prefix; one of four letters or more that the index does not know,
    /// or holds in only a few tickets, also matches the index's words one
    /// edit away (two from eight letters); a word joined by `-` `_` `.` `/`, or a `"quoted phrase"`,
    /// is a phrase. Tickets matched without a correction come first, then
    /// the rest; BM25 within each, a title weighing 4 to the text's 1.
    ///
    /// Yields one `hit { summary, title, snippet, corrected }` per ticket of
    /// the page, then one `searched { matched, next, words }`. `filter` (the
    /// language's text, FILTER-10) keeps the matches it admits (`under`
    /// too); one not taken, or one that asks `text`, is one in-band `error`
    /// and nothing else. `limit` is [`SEARCH_LIMIT`] unless given, at most
    /// [`SEARCH_MAX`]; `offset` skips that many matches.
    #[plexus_macros::method(
        description = "Search inside tickets: the tickets whose title or body holds every word of the query (prefixes and typos too), best first, each with the passage that matched; optionally under a filter, paged with offset + limit",
        params(
            query = "Words to find, e.g. 'plcement wave' — each also as a prefix; a misspelt one also as the words a typo away; FILTER-23 or \"a phrase\" as a phrase",
            filter = "A filter, e.g. 'not category done' (FILTER-3's language; optional): only the matches it admits",
            limit = "At most this many hits (default 20, at most 100) (optional)",
            offset = "Skip this many matches — the searched event's next (optional)"
        )
    )]
    pub async fn search(
        &self, auth: Option<&AuthContext>,
        query: String,
        filter: Option<String>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            // IDENTITY-21: only what this caller can see — its filter ANDed
            // with the shortcodes it holds a role on, as `find`'s is.
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let filter = match caller.scoped(filter.as_deref()) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let lens = match narrowing_lens(filter.as_deref()) {
                Ok(lens) => lens,
                Err(e) => {
                    yield TicketsEvent::Error { message: e.to_string() };
                    return;
                }
            };
            let limit = Some(limit.unwrap_or(SEARCH_LIMIT).min(SEARCH_MAX));
            match store.search(&query, lens, limit, offset).await {
                Ok(SearchPage { hits, matched, next, words }) => {
                    for hit in hits {
                        yield TicketsEvent::Hit {
                            summary: hit.summary,
                            title: hit.title,
                            snippet: hit.snippet,
                            corrected: hit.corrected,
                        };
                    }
                    yield TicketsEvent::Searched { matched, next, words };
                }
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// FILTER-15: which of some tickets a filter admits — what a page that
    /// shows tickets it did not list (a ticket page, its blockers, its
    /// unlocks) asks of the global filter. Yields one `Member { id, matched
    /// }` per id, in the order given — `id` as given, `matched` whether the
    /// filter admits the ticket it names; an id (or `SHORTCODE/path`
    /// address) that names no ticket is `matched: false` — and nothing
    /// after them: the stream's own `done` ends it. Each id is one lookup in
    /// the filter's admitted cells, never a test of the ticket. At most
    /// 1,000 ids: more, a filter not taken, or one that asks `under` or
    /// `text`, is one in-band `error` and nothing else.
    #[plexus_macros::method(
        description = "Which of these tickets a filter admits: one member { id, matched } per id, in the order given (an unknown id: false); at most 1000 ids",
        params(
            ids = "Ticket ids (or SHORTCODE/path addresses), e.g. [\"FILTER-4\", \"FILTER-5\"]; at most 1000",
            filter = "A filter, e.g. 'status Ready' (FILTER-3's language; empty: every ticket)"
        )
    )]
    pub async fn member(&self, auth: Option<&AuthContext>, ids: Vec<String>, filter: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let filter = match caller.scoped_text(&filter) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if ids.len() > MEMBER_MAX {
                yield TicketsEvent::Error {
                    message: format!("member: at most {MEMBER_MAX} ids a call — {} were asked; ask in parts", ids.len()),
                };
                return;
            }
            let lens = match narrowing_lens(Some(&filter)) {
                Ok(lens) => lens.unwrap_or_else(Lens::all),
                Err(e) => {
                    yield TicketsEvent::Error { message: e.to_string() };
                    return;
                }
            };
            let matched = store.member(&ids, &lens);
            for (id, matched) in ids.into_iter().zip(matched) {
                yield TicketsEvent::Member { id, matched };
            }
        }
    }

    /// MNG-10: the nav's tree without its rows. Yields one `Group` per
    /// shortcode row the nav draws, in shortcode order — `done/total` over
    /// the row's whole subtree (SDLC-11: its work; `docs` counts its docs,
    /// which have no completion), its scope (or doc) `root`, and the `host` ticket it
    /// nests under when it is a sub-shortcode — exactly as AGS-20's
    /// `plan()` computes them from every row. AGS-23: and the root's own
    /// row — its title, status and kind (`root_row`) — which the nav draws
    /// as the shortcode's top row, so its top level needs nothing else.
    /// Nothing trails the groups:
    /// unlike `list` there is no cursor to hand back, so the stream's own
    /// `done` is the whole answer, as for `blockers`. `watch` keeps a
    /// client's copy current with `GroupChanged`.
    #[plexus_macros::method(
        description = "One roll-up per shortcode: done/total over its subtree's work, its docs, its root (with the root's title, status and kind), and the host it nests under"
    )]
    pub async fn groups(&self, auth: Option<&AuthContext>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let visible = match visible_epics(&store, &caller) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            for mut group in store.groups() {
                if let Some(v) = &visible {
                    if !v.contains(&group.epic) {
                        continue;
                    }
                    // IDENTITY-21: hosted under a ticket out of reach, a
                    // shortcode stands at the top for this caller.
                    if group.host.as_deref().is_some_and(|h| !v.contains(shortcode_of(h))) {
                        group.host = None;
                    }
                }
                yield TicketsEvent::Group { group };
            }
        }
    }

    /// COMP-3: the component registry. Yields one `Component` per component
    /// a ticket defines or once defined, in name order — its `source` (the
    /// ` ```component <name> ` block's content), its `owner` (`null` for an
    /// orphan: no ticket defines it any more, and something still uses it)
    /// and its `references` (the tickets whose bodies hold `<name` as an
    /// HTML tag, in the store's order, then the components whose sources
    /// do). COMP-8: and its help — `docs`, the source's `<docs>` parsed
    /// (summary, typed attributes, slots, examples, markdown help; `null`
    /// without one) — and `preview`, the attributes on the block's opening
    /// line. Nothing trails them — the stream's own `done` is the whole
    /// answer, as for `groups`. From the cache alone; `watch` keeps a
    /// client's copy current with `ComponentChanged` and `ComponentRemoved`.
    #[plexus_macros::method(
        description = "Every component in the registry, in name order: its source, its owner (null: an orphan), what references it, its docs (summary, typed attributes, slots, examples, help; null without <docs>) and its preview attributes"
    )]
    pub async fn components(&self, auth: Option<&AuthContext>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.reach_filter(Role::Viewer) {
                yield TicketsEvent::Error { message };
                return;
            }
            for component in store.components() {
                yield TicketsEvent::Component { component };
            }
        }
    }

    /// INPUT-3: the inputs store. Yields one `Input` per input of `ticket`
    /// (every ticket's, without one) whose ticket is in the shortcode `epic`
    /// and which is in `state` — its ticket, id, kind (`choice`, `multi`,
    /// `text`), question, options (`{ value, label, recommended }`, the
    /// body's order), recommendation, state (`open`, `answered`, `closed`,
    /// `removed`), current answer (`{ value, rationale, source, at_unix }`,
    /// `null` with none) and whole history, oldest first — in the store's
    /// order of tickets, each ticket's inputs in its body's order and the
    /// removed after. Nothing trails them; the stream's own `done` is the
    /// whole answer. `NotFound` when `ticket` names no ticket; an unknown
    /// `state` is an in-band `Error`. From the cache; `watch` keeps a
    /// client's copy current with `InputChanged`.
    ///
    /// FILTER-15: `filter` (the language's text, FILTER-10) keeps the inputs
    /// whose ticket the filter admits — each such ticket one lookup in the
    /// filter's admitted cells; every other parameter as without it. A
    /// filter not taken, or one that asks `under` or `text`, is one in-band
    /// `error`; one that narrows nothing (empty, a display part alone) is no
    /// filter.
    #[plexus_macros::method(
        description = "Every input (a body's <m-decision id> / <m-input id kind>) with its kind, question, options, recommendation, state, answer and history; filtered by ticket, shortcode, state and a filter on its ticket",
        params(
            ticket = "Only this ticket's inputs, e.g. INPUT-9 (optional)",
            epic = "Only inputs of tickets in this shortcode (optional)",
            state = "Only inputs in this state: open | answered | closed | removed (optional)",
            filter = "A filter on the input's ticket, e.g. 'kind build' (FILTER-3's language; optional): only inputs of tickets it admits"
        )
    )]
    pub async fn inputs(
        &self, auth: Option<&AuthContext>,
        ticket: Option<String>,
        epic: Option<String>,
        state: Option<String>,
        filter: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let filter = match caller.scoped(filter.as_deref()) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Some(t) = ticket.as_deref() {
                if caller.require(Role::Viewer, &[t], &store).is_err() {
                    yield TicketsEvent::NotFound { id: t.to_string() };
                    return;
                }
            }
            let state = match state.as_deref().map(str::parse::<InputState>) {
                None => None,
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    yield TicketsEvent::Error { message: e };
                    return;
                }
            };
            // FILTER-15: of the tickets the filter admits, by the cells.
            let answer = match narrowing_lens(filter.as_deref()) {
                Err(e) => Err(e),
                Ok(Some(lens)) => store.inputs_filtered(ticket.as_deref(), epic.as_deref(), state, &lens),
                Ok(None) => store.inputs(ticket.as_deref(), epic.as_deref(), state),
            };
            match answer {
                Ok(inputs) => {
                    for input in inputs {
                        yield TicketsEvent::Input { input };
                    }
                }
                Err(StoreError::NotFound(id)) => yield TicketsEvent::NotFound { id },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// INPUT-3: answers an input — records `value` against `ticket`'s input
    /// `input`, with its `rationale`, its `source` (`app`: a viewer's click,
    /// which the host says; `api`, the default: anything else) and the
    /// time. The value is typed by the input's kind and checked against the
    /// options its body declares: `{"type": "choice", "option": …}` names
    /// one, `{"type": "multi", "options": […]}` a set (kept in the body's
    /// order, each once; empty: none ticked), `{"type": "text", "text": …}`
    /// a reply, not blank. Every answer is a history entry — a changed one,
    /// and the same one again. Yields exactly one event: `InputChanged` (the
    /// input as it is now, its answer and history included), `NotFound` for
    /// a ticket that is none, or an in-band `Error` — the ticket declares no
    /// such input, the value does not answer it, it is closed, or it was
    /// removed from its body — and nothing is written. Never touches the
    /// body. `watch` carries the `InputChanged`.
    #[plexus_macros::method(
        description = "Answer an input: record a value (checked against its kind and options) with a rationale, a source and the time; yields input_changed",
        params(
            ticket = "Ticket id, e.g. INPUT-9",
            input = "The input's id in that ticket's body, e.g. landing",
            value = "Typed by the input's kind: {\"type\": \"choice\", \"option\": \"archive\"} | {\"type\": \"multi\", \"options\": [\"a\", \"b\"]} | {\"type\": \"text\", \"text\": \"a reply\"}; through synapse, --value.type choice --value.option archive",
            rationale = "Why (optional)",
            source = "app (a viewer's click, said by the host) | api (default)"
        )
    )]
    pub async fn answer(
        &self, auth: Option<&AuthContext>,
        ticket: String,
        input: String,
        value: InputValue,
        rationale: Option<String>,
        source: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.require(Role::Editor, &[ticket.as_str()], &store) {
                yield TicketsEvent::Error { message };
                return;
            }
            let source = match source.as_deref().map(str::parse::<AnswerSource>) {
                None => AnswerSource::Api,
                Some(Ok(s)) => s,
                Some(Err(e)) => {
                    yield TicketsEvent::Error { message: e };
                    return;
                }
            };
            yield input_event(store.answer(&ticket, &input, value, rationale, source).await, ticket);
        }
    }

    /// INPUT-3: clears an input's answer — `open` again, the clear in its
    /// history, with its `source` (INPUT-9: `app`, a viewer's click, which
    /// the host says for a frame's `menger.input.clear`; `api`, the default:
    /// anything else). Yields exactly one event: `InputChanged`; `Input`
    /// when it held no answer (nothing written); `NotFound`; or an in-band
    /// `Error` — no such input, closed, removed, an unknown source.
    #[plexus_macros::method(
        description = "Clear an input's answer (open again; kept in its history, with a source); yields input_changed",
        params(
            ticket = "Ticket id, e.g. INPUT-9",
            input = "The input's id in that ticket's body",
            source = "app (a viewer's click, said by the host) | api (default)"
        )
    )]
    pub async fn clear_answer(
        &self, auth: Option<&AuthContext>,
        ticket: String,
        input: String,
        source: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.require(Role::Editor, &[ticket.as_str()], &store) {
                yield TicketsEvent::Error { message };
                return;
            }
            let source = match source.as_deref().map(str::parse::<AnswerSource>) {
                None => AnswerSource::Api,
                Some(Ok(s)) => s,
                Some(Err(e)) => {
                    yield TicketsEvent::Error { message: e };
                    return;
                }
            };
            yield input_event(store.clear_answer(&ticket, &input, source).await, ticket);
        }
    }

    /// INPUT-3: closes an input — frozen (a decision's DECIDED), with its
    /// answer or without one; an answer or a clear is refused until it is
    /// reopened. Yields exactly one event: `InputChanged`; `Input` when it
    /// was closed already; `NotFound`; or an in-band `Error` — no such
    /// input, removed.
    #[plexus_macros::method(
        description = "Close an input: freeze its answer (or its lack of one); yields input_changed",
        params(ticket = "Ticket id, e.g. INPUT-9", input = "The input's id in that ticket's body")
    )]
    pub async fn close_input(&self, auth: Option<&AuthContext>, ticket: String, input: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.require(Role::Editor, &[ticket.as_str()], &store) {
                yield TicketsEvent::Error { message };
                return;
            }
            yield input_event(store.close_input(&ticket, &input).await, ticket);
        }
    }

    /// INPUT-3: reopens a closed input — `answered` again when it holds an
    /// answer, else `open`. Yields exactly one event: `InputChanged`;
    /// `Input` when it was not closed; `NotFound`; or an in-band `Error` —
    /// no such input, removed.
    #[plexus_macros::method(
        description = "Reopen a closed input; yields input_changed",
        params(ticket = "Ticket id, e.g. INPUT-9", input = "The input's id in that ticket's body")
    )]
    pub async fn reopen_input(&self, auth: Option<&AuthContext>, ticket: String, input: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if let Err(message) = caller.require(Role::Editor, &[ticket.as_str()], &store) {
                yield TicketsEvent::Error { message };
                return;
            }
            yield input_event(store.reopen_input(&ticket, &input).await, ticket);
        }
    }

    /// SCALE-16: where each shortcode ends in the store's order. Yields
    /// one `Extent` per shortcode that holds a ticket, in shortcode order
    /// — its highest sequence, read from the per-shortcode sequence
    /// `create` allocates from (SCALE-3), so the cost is the number of
    /// shortcodes. Sequences are dense, so with these a client turns a
    /// position in the store's order into a cursor without walking to it:
    /// prefix sums find the shortcode E and the row k, and `list { after:
    /// "E-k" }` starts at row k + 1. Nothing trails the extents — there is
    /// no cursor to hand back — so the stream's own `done` is the whole
    /// answer, as for `groups` and `blockers`. No watch event keeps them:
    /// `created` already carries the newborn's `epic` and `seq`.
    #[plexus_macros::method(
        description = "Each shortcode's last sequence, in shortcode order — where each shortcode ends in the store's order"
    )]
    pub async fn extents(&self, auth: Option<&AuthContext>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let visible = match visible_epics(&store, &caller) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            for (epic, last_seq) in store.extents() {
                if visible.as_ref().is_some_and(|v| !v.contains(&epic)) {
                    continue;
                }
                yield TicketsEvent::Extent { epic, last_seq };
            }
        }
    }

    /// FILTER-4: the filtered `extents`. Yields one `Count { epic, held }`
    /// per shortcode that holds a match of `filter`, in shortcode order,
    /// and nothing after them — the stream's own `done` ends it, as for
    /// `extents`. Without a filter, each shortcode's tickets. A sum over
    /// the kept counts of the cells the filter admits: no ticket is tested,
    /// and the cost is the shortcodes and the cells, never the tickets.
    /// With the counts, a client turns a position among the matches into a
    /// (shortcode, rank) by prefix sums, and asks `list { filter, at }`. A
    /// filter not taken, or one that asks `under` or `text`, is one in-band
    /// `error` and nothing else.
    #[plexus_macros::method(
        description = "Each shortcode's number of tickets matching a filter, in shortcode order — the filtered extents (every ticket without one)",
        params(filter = "A filter, e.g. 'ready and kind build' (FILTER-3's language; optional: every ticket)")
    )]
    pub async fn counts(&self, auth: Option<&AuthContext>, filter: Option<String>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let filter = match caller.scoped(filter.as_deref()) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            match filter.as_deref().map(Lens::parse).unwrap_or_else(|| Ok(Lens::all())) {
                Ok(lens) => {
                    for (epic, held) in store.counts(&lens) {
                        yield TicketsEvent::Count { epic, held };
                    }
                }
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// FILTER-4: the counts behind every menu (Linear's facets). For each
    /// field — `status`, `category`, `kind`, `severity`, `blocked`, `ready`,
    /// `has commits`, `menger doc` (FILTER-21), `in`, in that order — the
    /// tickets matching `base` and `filter` with `filter`'s own groups for
    /// that field set aside (a group is a field's own when every predicate
    /// in it names that field; `base`'s are never set aside), counted by
    /// value: one `Facet { field, value, held }` per value with `held` above
    /// 0 — `field` as the language spells it, `value` canonical (`Coding`,
    /// `build`, `none`, `yes`/`no` for the four flags, the shortcode for
    /// `in`), statuses in the store's order of statuses, shortcodes in
    /// shortcode order — then the stream's done. A sum over kept cell
    /// counts, never a ticket. A filter not taken, or one that asks `under`
    /// or `text`, is one in-band `error` and nothing else.
    #[plexus_macros::method(
        description = "Menu counts: per field (status, category, kind, severity, blocked, ready, has commits, menger doc, in), the matches of base and filter without filter's own groups for that field, by value",
        params(
            filter = "The filter whose menus are counted (optional): its own groups for a field are set aside for that field's counts",
            base = "A filter that always applies, e.g. the global one (optional): never set aside"
        )
    )]
    pub async fn facets(&self, auth: Option<&AuthContext>, filter: Option<String>, base: Option<String>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let filter = match caller.scoped(filter.as_deref()) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let base = match caller.scoped(base.as_deref()) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let read = |text: Option<String>| -> Result<crate::filter::Filter, String> {
                text.as_deref().map_or_else(|| Ok(Default::default()), |t| crate::filter::Filter::parse(t).map_err(|e| e.to_string()))
            };
            let answer = read(filter)
                .and_then(|f| Ok((f, read(base)?)))
                .and_then(|(f, b)| store.facets(&f, &b).map_err(|e| e.to_string()));
            match answer {
                Ok(facets) => {
                    for (field, value, held) in facets {
                        yield TicketsEvent::Facet { field, value, held };
                    }
                }
                Err(message) => yield TicketsEvent::Error { message },
            }
        }
    }

    /// FILTER-13: the nav under a filter — the rows the nav would show at an
    /// address unfiltered (`parent`: that ticket's children, as `list {
    /// parent }` gives them; `epic`: the shortcode's top level — its root,
    /// or, for a shortcode with none, its tickets as `list { epic }` gives
    /// them), only those that match `filter` or hold a match below them, in
    /// the store's order. One `Node { summary, matched, below }` per row —
    /// `matched` whether the row matches, `below` how many matches are
    /// strictly under it — then one `Page { next, remaining }`, paged as
    /// `list` pages (`after` a ticket id, at most `limit`). Answered from the
    /// kept tree: a parent's children under a filter are (C(P) ∩ M) ∪ the
    /// holders of every admitted cell, and `below` a sum over the row's
    /// tally — no subtree is walked. Exactly one of `parent` and `epic`; a
    /// filter not taken, or one that asks `text`, is one in-band `error` and
    /// nothing else.
    ///
    /// FILTER-23: `matches_only` — the nav that draws only matches, no
    /// ticket that only holds them. At `parent`, the matches nearest below
    /// it: its children that match and, in place of a child that does not,
    /// that child's own nearest matches, at any depth, in the store's order
    /// — then the hosted shortcodes whose row is drawn there (hosted at the
    /// parent, or at a ticket between that is not drawn), in shortcode
    /// order, each as its root's node, whether or not it holds a match. At
    /// `epic`, the root when it matches, else the top of the shortcode: what
    /// the root's `parent` address answers (a shortcode with no root: its
    /// tickets that match). Paged the same way. Off, `tree` is FILTER-13's,
    /// byte for byte.
    #[plexus_macros::method(
        description = "The nav under a filter: a parent's children (or a shortcode's top level) that match or hold a match, each with matched and below (matches strictly under it), then a page; matches_only: the matches nearest below instead, then the hosted shortcodes drawn there",
        params(
            parent = "Whose children, e.g. FILTER-9 (or give epic)",
            epic = "A shortcode's top level: its root, or its tickets when it has none (or give parent)",
            filter = "A filter, e.g. 'status Ready and under SCALE' (FILTER-3's language; empty: every ticket)",
            after = "Cursor: start strictly after this ticket id (optional)",
            limit = "At most this many rows; the page event carries the next cursor (optional)",
            matches_only = "Only matches (optional, default false): a ticket that does not match is not a row, and the matches below it are answered in its place, at the nearest ancestor that matches or at the top of the shortcode; then the hosted shortcodes drawn there"
        )
    )]
    pub async fn tree(
        &self, auth: Option<&AuthContext>,
        parent: Option<String>,
        epic: Option<String>,
        filter: String,
        after: Option<String>,
        limit: Option<u32>,
        matches_only: Option<bool>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let filter = match caller.scoped_text(&filter) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let lens = match Lens::parse(&filter) {
                Ok(lens) => lens,
                Err(e) => {
                    yield TicketsEvent::Error { message: e.to_string() };
                    return;
                }
            };
            let q = TreeQuery { parent, epic, lens, matches_only: matches_only.unwrap_or(false) };
            for ev in tree_events(store.tree_page(&q, after.as_deref(), limit)) {
                yield ev;
            }
        }
    }

    /// WAVE-4: the order of work, for a board. Yields one `Placed` per
    /// ticket of the shortcodes `epics` (case-insensitive; one with no
    /// tickets contributes nothing) whose kind is in `kinds` — default
    /// `build`, `spike`, `gap`; never a doc — then exactly one
    /// `WaveCounts`. Each ticket's placement is computed over the whole
    /// `blocked_by` graph by MNG-4's rule, following blockers into any
    /// shortcode: `done` when it is `Complete` or `Superseded`;
    /// `missing_blocker` when it, or anything it waits on, names a blocker
    /// that is no ticket; otherwise `wave { n }` — 1 with every blocker
    /// satisfied, else 1 + the highest wave among its unsatisfied ones.
    /// `waiting_on` is its unsatisfied direct blockers, in `blocked_by`
    /// order, distinct. Order: waves ascending, inside a wave Coding, In
    /// Review, Ready, Pending, Blocked, Idea, Epic and then (epic, seq);
    /// then the missing lane; then the done — each in (epic, seq). `epics`
    /// empty, or a kind outside the six, is one in-band `Error` and
    /// nothing else. WAVE-6: each placement is the one the store keeps on the
    /// row (the same rule, kept current by every write that moves one); rows
    /// only.
    #[plexus_macros::method(
        description = "The order of work: each ticket of the shortcodes placed in its wave over the whole blocked_by graph (or missing_blocker, or done) with what it waits on, then the wave counts",
        params(
            epics = "Shortcodes whose tickets are placed, e.g. [\"PLEXUS\", \"MACRO\"] (case-insensitive; at least one); blockers are followed into every shortcode",
            kinds = "Kinds to place: scope | execution | build | gap | spike (optional; default build, spike, gap — a doc is never placed)"
        )
    )]
    pub async fn waves(
        &self, auth: Option<&AuthContext>,
        epics: Vec<String>,
        kinds: Option<Vec<String>>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let visible = match visible_epics(&store, &caller) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if epics.is_empty() {
                yield TicketsEvent::Error {
                    message: "waves: epics is empty — name at least one shortcode, e.g. [\"MNG\"]".into(),
                };
                return;
            }
            if let Some(v) = &visible {
                if let Some(hidden) = epics.iter().find(|e| !v.contains(&e.to_ascii_uppercase())) {
                    yield TicketsEvent::Error { message: format!("waves: no access to {hidden}") };
                    return;
                }
            }
            let kinds: Vec<Kind> = match kinds {
                None => crate::store::waves::DEFAULT_KINDS.to_vec(),
                Some(given) => match given.iter().map(|k| k.parse::<Kind>()).collect() {
                    Ok(kinds) => kinds,
                    Err(e) => {
                        yield TicketsEvent::Error { message: e };
                        return;
                    }
                },
            };
            let answer = store.waves(&epics, &kinds);
            for p in answer.placed {
                yield TicketsEvent::Placed {
                    summary: p.summary,
                    placement: p.placement,
                    waiting_on: p.waiting_on,
                };
            }
            yield TicketsEvent::WaveCounts {
                waves: answer.waves,
                missing: answer.missing,
                done: answer.done,
            };
        }
    }

    /// "Why can't this start?" in one call. Yields, in `blocked_by`
    /// order, one `Blocker` event per blocker that exists (the full
    /// ticket, at its current status) and one `DanglingBlocker` event per
    /// id that names no ticket yet; `NotFound` if the queried ticket
    /// itself doesn't exist. An empty `blocked_by` yields nothing.
    #[plexus_macros::method(
        description = "One ticket's blockers resolved to current statuses; dangling ids marked",
        params(id = "Ticket id whose blockers to resolve, e.g. MNG-2")
    )]
    pub async fn blockers(
        &self, auth: Option<&AuthContext>,
        id: String,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if caller.require(Role::Viewer, &[id.as_str()], &store).is_err() {
                yield TicketsEvent::NotFound { id: id };
                return;
            }
            match store.blockers(&id).await {
                Ok(refs) => {
                    for r in refs {
                        yield match r {
                            BlockerRef::Resolved(ticket) => TicketsEvent::Blocker { ticket },
                            BlockerRef::Dangling(id) => TicketsEvent::DanglingBlocker { id },
                        };
                    }
                }
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// The reactive primitive: an open-ended stream of every mutation, as
    /// `Created` / `StatusChanged` / `UnlocksChanged` / `ChildrenChanged`
    /// events. Runs until the client disconnects. Every event carries the
    /// full affected ticket, so a client that listed once and then only
    /// watches can compute the ready set — and the parent tree — itself.
    ///
    /// SCALE-5: the stream is fed from the store's bounded broadcast, and
    /// a consumer that falls more than it holds behind loses the oldest
    /// changes. That used to be silent — the stream went on as if whole and
    /// the client's copy was stale with nothing to say so. Now the gap is
    /// one `Lagged { missed }` where the lost changes were, then the oldest
    /// change still held, in order, no change twice. Delivered past the
    /// `epic` filter: whether a lost change was in the epic is exactly what
    /// cannot be known. Raising the capacity instead would move the cliff
    /// and keep the silence.
    ///
    /// MNG-10: a mutation that moves a roll-up is followed, after its
    /// ticket events, by one `GroupChanged` per group whose row moved,
    /// innermost first — the whole group, so applying it is one upsert.
    /// The `epic` filter applies to a group by the group's own shortcode.
    /// AGS-23: a group's row includes its root's (`root_row`), so a status
    /// change of a shortcode's root moves its group whether or not it
    /// crosses `Complete`.
    ///
    /// MNG-16: an `amend` that changed something is one `Amended`, and
    /// moves no group — AGS-23: but a new title on a shortcode's root, which
    /// is followed by its group's `GroupChanged`.
    ///
    /// SDLC-9: so is a `link_commits` or `unlink_commits` that changed
    /// something — one `CommitsLinked` or `CommitsUnlinked`, the whole
    /// ticket, no group.
    ///
    /// SDLC-13: a `set_kind` that changed something is one `KindChanged`
    /// — the whole ticket — then what it moved, as a status change's: a
    /// `GroupChanged` per row (a root's whenever its kind or status
    /// changed; every row a re-rooted shortcode moved), the
    /// `PlacementsChanged`, and with a filter the `Entered` / `Left`.
    ///
    /// SCALE-4: a parent's side of a birth or a move is the exception to
    /// "the full ticket": `ChildrenChanged` / `ChildRemoved` carry the
    /// parent's id, the child, and the parent's new `child_count` — the
    /// child's own `Created` / `ParentChanged` already says where it
    /// hangs. The `epic` filter applies to them by the parent's shortcode.
    ///
    /// COMP-3: a `create` or `amend` that moved the component registry ends
    /// with one `ComponentChanged` per component it registered or moved
    /// (source, owner, references — the whole component), in name order,
    /// then one `ComponentRemoved` per component it took out — after every
    /// other event of the write. A component is in no shortcode, so both
    /// pass any `epic` filter.
    ///
    /// INPUT-3: an input a write moved is one `InputChanged` — the whole
    /// input — after every other event of the write, the component events
    /// included: an `answer`, `clear_answer`, `close_input` or
    /// `reopen_input`; or a `create` or `amend` whose body declared,
    /// re-declared or removed inputs (in the body's order, the removed
    /// after). The `epic` filter applies to it by its ticket's shortcode.
    ///
    /// FILTER-4: with a `filter`, the same stream plus, as each write's
    /// last events, one `Entered { id, epic, seq }` or `Left { id, epic,
    /// seq }` per ticket the write moved into or out of the filter — a
    /// birth, a status change, every ticket its blocker crossing moved, a
    /// first commit or a last — in the store's order; a write that moves
    /// more than 256 sends instead one `Recount { epic, held }` per
    /// shortcode it moved, the filter's count there as the write left it.
    /// `epic` filters them by their own shortcode. `membership` sends only
    /// those three and `Lagged` — for a client whose rows already ride an
    /// unfiltered `watch`. Without a filter or `membership`, the stream is
    /// exactly what it was. A filter not taken, or one that asks `under` or
    /// `text`, is one in-band `error` and the stream ends.
    #[plexus_macros::method(
        streaming,
        description = "Subscribe to ticket changes (Created / StatusChanged / KindChanged / Amended / CommitsLinked / CommitsUnlinked / UnlocksChanged / ChildrenChanged / ParentChanged / ChildRemoved / GroupChanged / PlacementsChanged / ComponentChanged / ComponentRemoved / InputChanged), until disconnect; Lagged { missed } where a slow subscriber lost changes; with a filter, Entered / Left / Recount as each write's last events",
        params(
            epic = "Only changes in this epic (optional; a GroupChanged by its group's shortcode; an InputChanged by its ticket's; component changes always)",
            filter = "A filter, e.g. 'category in (active, review)' (optional): adds Entered / Left for each ticket a write moves into or out of it, or Recount past 256",
            membership = "Send only Entered, Left, Recount and Lagged (optional; default false)"
        )
    )]
    pub async fn watch(
        &self, auth: Option<&AuthContext>,
        epic: Option<String>,
        filter: Option<String>,
        membership: Option<bool>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let membership = membership.unwrap_or(false);
        let epic = epic.map(|e| e.to_ascii_uppercase());
        // IDENTITY-19, IDENTITY-21: who is watching, and through what reach.
        let access = self.access.clone();
        let who = auth.cloned();
        let admitted = access.admit(who.as_ref());
        let (filter, refused) = match &admitted {
            Ok(caller) => match caller.scoped(filter.as_deref()) {
                Ok(f) => (f, None),
                Err(e) => (None, Some(e)),
            },
            Err(e) => (None, Some(e.clone())),
        };
        let reach: Option<Lens> = admitted
            .as_ref()
            .ok()
            .and_then(|c| c.reach_filter(Role::Viewer).ok().flatten())
            .and_then(|f| Lens::of(&f).ok());
        // FILTER-4: a filtered watch reads the filtered channel, which
        // carries every change and each write's cell moves after them.
        let filtered = (filter.is_some() || membership).then(|| {
            let lens = filter.as_deref().map(Lens::parse).unwrap_or_else(|| Ok(Lens::all()));
            (self.store.subscribe_filtered(), lens.map(|lens| self.store.watcher_in(lens, epic.as_deref())))
        });
        let store = Arc::clone(&self.store);
        let plain = filtered.is_none().then(|| self.store.subscribe());
        // IDENTITY-19: admitted now, and asked again at every change it
        // relays, so a watch ends when its session does. IDENTITY-21: and a
        // restricted caller hears only of the tickets it can see.
        stream! {
            if let Some(message) = refused {
                yield TicketsEvent::Error { message };
                return;
            }
            if let Some((mut rx, watcher)) = filtered {
                let mut watcher = match watcher {
                    Ok(w) => w,
                    Err(e) => {
                        yield TicketsEvent::Error { message: e.to_string() };
                        return;
                    }
                };
                loop {
                    let next = rx.recv().await;
                    if let Err(message) = access.admit(who.as_ref()) {
                        yield TicketsEvent::Error { message };
                        return;
                    }
                    match next {
                        Ok(Change::CellsMoved { moves }) => {
                            for m in watcher.membership(&store, &moves, epic.as_deref()) {
                                yield membership_event(m);
                            }
                        }
                        // FILTER-13: a filter asking `under` — what the write
                        // worked out for this watch.
                        Ok(Change::UnderMoved { moves }) => {
                            for m in watcher.under_membership(&moves) {
                                yield membership_event(m);
                            }
                        }
                        Ok(change) => {
                            if membership {
                                continue;
                            }
                            if let Some(ev) = change.in_epic(epic.as_deref()).and_then(wire_event) {
                                if let Some(ev) = within(&store, reach.as_ref(), ev) {
                                    yield ev;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                            yield TicketsEvent::Lagged { missed };
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                return;
            }
            let Some(mut rx) = plain else { return };
            loop {
                let next = rx.recv().await;
                if let Err(message) = access.admit(who.as_ref()) {
                    yield TicketsEvent::Error { message };
                    return;
                }
                match next {
                    Ok(change) => {
                        // WAVE-6: of a `placements_changed`, the changes in
                        // the epic.
                        if let Some(ev) = change.in_epic(epic.as_deref()).and_then(wire_event) {
                            if let Some(ev) = within(&store, reach.as_ref(), ev) {
                                yield ev;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                        yield TicketsEvent::Lagged { missed };
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

/// IDENTITY-21: a watched event as a restricted caller may hear it: itself
/// when every ticket it names is within `reach`, the part of it that is
/// (a placements change), or nothing. Events naming no ticket pass.
#[cfg(feature = "server")]
fn within(store: &Store, reach: Option<&Lens>, ev: TicketsEvent) -> Option<TicketsEvent> {
    let Some(reach) = reach else { return Some(ev) };
    let sees = |id: &str| store.member(&[id.to_string()], reach).first().copied().unwrap_or(false);
    let id: Option<String> = match &ev {
        TicketsEvent::Created { ticket }
        | TicketsEvent::Ticket { ticket }
        | TicketsEvent::StatusChanged { ticket, .. }
        | TicketsEvent::KindChanged { ticket, .. }
        | TicketsEvent::Amended { ticket, .. }
        | TicketsEvent::CommitsLinked { ticket, .. }
        | TicketsEvent::CommitsUnlinked { ticket, .. }
        | TicketsEvent::UnlocksChanged { ticket, .. }
        | TicketsEvent::ParentChanged { ticket, .. }
        | TicketsEvent::Blocker { ticket } => Some(ticket.id.clone()),
        TicketsEvent::Summary { summary } | TicketsEvent::Placed { summary, .. } | TicketsEvent::Node { summary, .. } => {
            Some(summary.id.clone())
        }
        TicketsEvent::ChildrenChanged { id, .. }
        | TicketsEvent::ChildRemoved { id, .. }
        | TicketsEvent::Entered { id, .. }
        | TicketsEvent::Left { id, .. } => Some(id.clone()),
        TicketsEvent::Group { group } | TicketsEvent::GroupChanged { group } => {
            if !sees(group.root.as_deref().unwrap_or_default()) {
                return None;
            }
            // As `groups` answers it: a host out of reach is no host.
            let mut group = group.clone();
            if group.host.as_deref().is_some_and(|h| !sees(h)) {
                group.host = None;
            }
            return Some(match ev {
                TicketsEvent::Group { .. } => TicketsEvent::Group { group },
                _ => TicketsEvent::GroupChanged { group },
            });
        }
        TicketsEvent::InputChanged { input } | TicketsEvent::Input { input } => Some(input.ticket.clone()),
        TicketsEvent::PlacementsChanged { changes } => {
            let kept: Vec<_> = changes.iter().filter(|c| sees(&c.id)).cloned().collect();
            return (!kept.is_empty()).then_some(TicketsEvent::PlacementsChanged { changes: kept });
        }
        _ => None,
    };
    match id {
        Some(id) if !sees(&id) => None,
        _ => Some(ev),
    }
}

/// A ticket id's (or path address's) shortcode.
#[cfg(feature = "server")]
fn shortcode_of(id: &str) -> &str {
    id.split(['-', '/']).next().unwrap_or(id)
}

/// IDENTITY-21: the shortcodes a caller can see — `None`: all of them. A
/// grant covers whole shortcodes (`under`), so a shortcode is visible
/// exactly when the caller's reach admits any of its tickets.
#[cfg(feature = "server")]
fn visible_epics(store: &Store, caller: &Caller) -> Result<Option<std::collections::HashSet<String>>, String> {
    let Some(reach) = caller.reach_filter(Role::Viewer)? else { return Ok(None) };
    let lens = Lens::of(&reach).map_err(|e| e.to_string())?;
    Ok(Some(store.counts(&lens).into_iter().filter(|(_, n)| *n > 0).map(|(e, _)| e).collect()))
}

/// IDENTITY-21: how many tickets a caller can see.
#[cfg(feature = "server")]
fn visible_count(store: &Store, caller: &Caller) -> u64 {
    match caller.reach_filter(Role::Viewer) {
        Ok(None) => store.count() as u64,
        Ok(Some(reach)) => Lens::of(&reach).map(|lens| store.counts(&lens).into_iter().map(|(_, n)| n).sum()).unwrap_or(0),
        Err(_) => 0,
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use futures::StreamExt;

    fn hub() -> Tickets {
        Tickets::new(Arc::new(Store::new()))
    }

    async fn one(s: impl Stream<Item = TicketsEvent> + Send + 'static) -> TicketsEvent {
        Box::pin(s).next().await.expect("event")
    }

    #[tokio::test]
    async fn health_reports_count() {
        let hub = hub();
        match one(hub.health(None).await).await {
            TicketsEvent::Health { ticket_count, .. } => assert_eq!(ticket_count, 0),
            other => panic!("expected Health, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_allocates_sequential_ids() {
        let hub = hub();
        let ev = one(hub.create(None, "mng".into(), "first".into(), Some("scope".into()), None, None, None, None, None).await).await;
        match ev {
            TicketsEvent::Created { ticket } => {
                assert_eq!(ticket.id, "MNG-1");
                assert_eq!(ticket.status, Status::Pending);
            }
            other => panic!("expected Created, got {other:?}"),
        }
        let ev = one(hub.create(None, "MNG".into(), "second".into(), None, None, None, None, None, None).await).await;
        match ev {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "MNG-2"),
            other => panic!("expected Created, got {other:?}"),
        }
    }

    /// AGS-13 criterion 2: no kind → `build`; `gap` is accepted; a kind
    /// outside the five (`bug` — rim's name for gap) is the in-band error
    /// that names all five, and nothing is created.
    #[tokio::test]
    async fn create_defaults_kind_to_build_accepts_gap_and_rejects_unknown_naming_the_six() {
        let hub = hub();
        // MNG-12: the prefix's scope root first; the default is measured
        // on the ticket after it.
        one(hub.create(None, "MNG".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
        match one(hub.create(None, "MNG".into(), "no kind".into(), None, None, None, None, None, None).await).await {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.kind, Kind::Build),
            other => panic!("expected Created, got {other:?}"),
        }
        match one(hub.create(None, "MNG".into(), "a gap".into(), Some("gap".into()), None, None, None, None, None).await)
            .await
        {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.kind, Kind::Gap),
            other => panic!("expected Created, got {other:?}"),
        }
        let ev = one(
            hub.create(None, "MNG".into(), "x".into(), Some("bug".into()), None, None, None, None, None)
                .await,
        )
        .await;
        match ev {
            TicketsEvent::Error { message } => {
                assert!(message.contains("'bug'"), "{message}");
                for name in ["scope", "execution", "build", "gap", "spike", "doc"] {
                    assert!(message.contains(name), "names {name}: {message}");
                }
            }
            other => panic!("expected Error, got {other:?}"),
        }
        assert!(matches!(
            one(hub.get(None, "MNG-4".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    /// SDLC-11 over the activation: a doc is created in `Doc` (and may root
    /// a shortcode); `set_status` on it is one in-band `error` saying it has
    /// no lifecycle; `Doc` on work is one in-band `error`; a doc with a
    /// `blocked_by`, and a `blocked_by` naming a doc, are in-band `error`s;
    /// `list { status: Doc }` finds the doc.
    #[tokio::test]
    async fn a_doc_over_the_activation_refuses_every_lifecycle_write_in_band() {
        let hub = hub();
        match one(hub.create(None, "DOCS".into(), "the manifest".into(), Some("doc".into()), None, None, None, None, None).await).await {
            TicketsEvent::Created { ticket } => {
                assert_eq!((ticket.id.as_str(), ticket.kind, ticket.status), ("DOCS-1", Kind::Doc, Status::Doc));
            }
            other => panic!("expected Created, got {other:?}"),
        }
        one(hub.create(None, "DOCS".into(), "work".into(), None, None, None, None, None, None).await).await; // DOCS-2
        for status in ["Ready", "Complete", "Doc", "active"] {
            match one(hub.set_status(None, "DOCS-1".into(), status.into()).await).await {
                TicketsEvent::Error { message } => assert!(message.contains("a doc has no lifecycle"), "{message}"),
                other => panic!("{status}: expected Error, got {other:?}"),
            }
        }
        match one(hub.set_status(None, "DOCS-2".into(), "doc".into()).await).await {
            TicketsEvent::Error { message } => assert!(message.contains("Doc is a doc's own status"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        let blocked = Some(vec!["DOCS-2".to_string()]);
        match one(hub.create(None, "DOCS".into(), "x".into(), Some("doc".into()), None, blocked, None, None, None).await).await {
            TicketsEvent::Error { message } => assert!(message.contains("a doc is never blocked"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        let on_doc = Some(vec!["DOCS-1".to_string()]);
        match one(hub.create(None, "DOCS".into(), "x".into(), None, None, on_doc, None, None, None).await).await {
            TicketsEvent::Error { message } => assert!(message.contains("'DOCS-1' is a doc"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        match one(hub.list(None, None, Some("Doc".into()), None, None, None, Some(true), None, None).await).await {
            TicketsEvent::Summary { summary } => assert_eq!((summary.id.as_str(), summary.status), ("DOCS-1", Status::Doc)),
            other => panic!("expected Summary, got {other:?}"),
        }
        match one(hub.get(None, "DOCS-1".into()).await).await {
            TicketsEvent::Ticket { ticket } => assert_eq!(ticket.status, Status::Doc, "nothing moved it"),
            other => panic!("expected Ticket, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_distinguishes_found_from_missing() {
        let hub = hub();
        one(hub.create(None, "MNG".into(), "x".into(), Some("scope".into()), None, None, None, None, None).await).await;
        assert!(matches!(
            one(hub.get(None, "MNG-1".into()).await).await,
            TicketsEvent::Ticket { .. }
        ));
        assert!(matches!(
            one(hub.get(None, "MNG-99".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn list_filters_by_status() {
        let hub = hub();
        one(hub.create(None, "MNG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
        one(hub.create(None, "MNG".into(), "b".into(), None, None, None, None, None, None).await).await;
        one(hub.set_status(None, "MNG-1".into(), "Ready".into()).await).await;

        let (ids, page) =
            ids_and_page(hub.list(None, None, Some("Ready".into()), None, None, None, None, None, None).await).await;
        assert_eq!(ids, vec!["MNG-1"]);
        assert_eq!(page, Some((None, 0)), "one match, nothing beyond it");
    }

    #[tokio::test]
    async fn set_status_reports_previous() {
        let hub = hub();
        one(hub.create(None, "MNG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
        match one(hub.set_status(None, "MNG-1".into(), "ready".into()).await).await {
            TicketsEvent::StatusChanged { ticket, previous } => {
                assert_eq!(ticket.status, Status::Ready);
                assert_eq!(previous, Status::Pending);
            }
            other => panic!("expected StatusChanged, got {other:?}"),
        }
    }

    /// SDLC-13, over the activation: `set_kind` answers `kind_changed` with
    /// the kind it had, its kind and status as the call and the rules leave
    /// them (case-insensitive, `active` for `Coding`); an unknown id is
    /// `not_found`; an unknown kind or status, and each rule's refusal, one
    /// in-band `error` naming what it refuses.
    #[tokio::test]
    async fn set_kind_reports_previous_and_refuses_in_band() {
        let hub = hub();
        one(hub.create(None, "IDN".into(), "identity".into(), Some("doc".into()), None, None, None, None, None).await).await;
        one(hub.create(None, "IDN".into(), "a".into(), None, None, None, None, None, None).await).await;
        match one(hub.set_kind(None, "IDN-1".into(), "Scope".into(), None).await).await {
            TicketsEvent::KindChanged { ticket, previous } => {
                assert_eq!((ticket.kind, ticket.status, previous), (Kind::Scope, Status::Epic, Kind::Doc));
            }
            other => panic!("expected KindChanged, got {other:?}"),
        }
        match one(hub.set_kind(None, "IDN-2".into(), "gap".into(), Some("active".into())).await).await {
            TicketsEvent::KindChanged { ticket, previous } => {
                assert_eq!((ticket.kind, ticket.status, previous), (Kind::Gap, Status::Coding, Kind::Build));
            }
            other => panic!("expected KindChanged, got {other:?}"),
        }
        assert!(matches!(
            one(hub.set_kind(None, "IDN-9".into(), "gap".into(), None).await).await,
            TicketsEvent::NotFound { id } if id == "IDN-9"
        ));
        for (id, kind, status, says) in [
            ("IDN-2", "bug", None, "unknown kind 'bug'"),
            ("IDN-2", "gap", Some("Shipped"), "unknown status 'shipped'"),
            ("IDN-1", "build", None, "'IDN-1' is the root of IDN"),
            ("IDN-2", "doc", Some("Ready"), "a doc's status is Doc"),
            ("IDN-2", "spike", Some("Doc"), "Doc is a doc's own status"),
        ] {
            match one(hub.set_kind(None, id.into(), kind.into(), status.map(Into::into)).await).await {
                TicketsEvent::Error { message } => assert!(message.contains(says), "{kind} {status:?}: {message}"),
                other => panic!("{kind} {status:?}: expected Error, got {other:?}"),
            }
        }
    }

    /// SDLC-7 criterion 1, over the activation: `set_status` takes both
    /// worker states — the canonical spelling and a hand-typed one —
    /// answers `status_changed`, `get` returns `In Review` byte-exact on
    /// the wire, and `list { status }` filters on each.
    #[tokio::test]
    async fn set_status_takes_the_worker_states_and_list_filters_on_them() {
        let hub = hub();
        one(hub.create(None, "WK".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
        one(hub.create(None, "WK".into(), "a".into(), None, None, None, None, None, None).await).await;
        one(hub.create(None, "WK".into(), "b".into(), None, None, None, None, None, None).await).await;

        for (id, spelling, want, previous) in [
            ("WK-2", "Coding", Status::Coding, Status::Pending),
            ("WK-3", "active", Status::Coding, Status::Pending),
            ("WK-3", "In Review", Status::InReview, Status::Coding),
        ] {
            match one(hub.set_status(None, id.into(), spelling.into()).await).await {
                TicketsEvent::StatusChanged { ticket, previous: was } => {
                    assert_eq!((ticket.id.as_str(), ticket.status, was), (id, want, previous), "{spelling}");
                }
                other => panic!("{spelling}: expected StatusChanged, got {other:?}"),
            }
        }
        let wire = serde_json::to_value(one(hub.get(None, "WK-3".into()).await).await).unwrap();
        assert_eq!(wire["ticket"]["status"], "In Review");

        let (ids, _) = ids_and_page(hub.list(None, None, Some("coding".into()), None, None, None, None, None, None).await).await;
        assert_eq!(ids, ["WK-2"]);
        let (ids, _) = ids_and_page(hub.list(None, None, Some("In Review".into()), None, None, None, None, None, None).await).await;
        assert_eq!(ids, ["WK-3"]);
    }

    /// MNG-16 over the activation: `amend` answers `amended { ticket,
    /// changed }` on the wire (and `changed: []` for a no-op), `not_found`
    /// for a missing id, an in-band `error` for a blank title; `watch`
    /// carries one `amended` per amend that changed something and none for
    /// the no-op or the refusals — so the next event after them is the next
    /// real change.
    #[tokio::test]
    async fn amend_yields_amended_and_watch_carries_only_what_changed() {
        let hub = hub();
        one(hub.create(None, "AM".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
        let mut watch = Box::pin(hub.watch(None, None, None, None).await);

        let ev = one(hub.amend(None, "am-1".into(), None, Some("the body, now".into())).await).await;
        let wire = serde_json::to_value(&ev).unwrap();
        assert_eq!(wire["type"], "amended");
        assert_eq!(wire["changed"], serde_json::json!(["body"]));
        assert_eq!(wire["ticket"]["id"], "AM-1");
        assert_eq!(wire["ticket"]["body"], "the body, now");
        match watch.next().await.expect("change") {
            TicketsEvent::Amended { ticket, changed } => {
                assert_eq!((ticket.id.as_str(), ticket.body.as_str()), ("AM-1", "the body, now"));
                assert_eq!(changed, ["body"]);
            }
            other => panic!("expected Amended, got {other:?}"),
        }

        match one(hub.amend(None, "AM-1".into(), None, Some("the body, now".into())).await).await {
            TicketsEvent::Amended { changed, .. } => assert!(changed.is_empty(), "{changed:?}"),
            other => panic!("expected Amended, got {other:?}"),
        }
        assert!(matches!(
            one(hub.amend(None, "AM-7".into(), Some("t".into()), None).await).await,
            TicketsEvent::NotFound { id } if id == "AM-7"
        ));
        match one(hub.amend(None, "AM-1".into(), Some("  ".into()), None).await).await {
            TicketsEvent::Error { message } => assert!(message.contains("empty"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }

        one(hub.amend(None, "AM-1".into(), Some("a better title".into()), None).await).await;
        match watch.next().await.expect("change") {
            TicketsEvent::Amended { ticket, changed } => {
                assert_eq!(ticket.title, "a better title");
                assert_eq!(changed, ["title"], "the no-op and the refusals said nothing");
            }
            other => panic!("expected Amended, got {other:?}"),
        }
    }

    /// SDLC-9 over the activation (criteria 1, 2, 4): two commits linked,
    /// then one of them again with a third — three in link order, the
    /// second call's `added` the third alone, `get` showing them; a call
    /// with one malformed entry is one in-band `error` naming it and writes
    /// nothing; `not_found` for a missing id; and `watch` carries one event
    /// per call that changed something — `commits_linked`,
    /// `commits_unlinked` — and none for the no-ops or the refusal.
    #[tokio::test]
    async fn link_commits_yields_commits_linked_and_watch_carries_only_what_changed() {
        let hub = hub();
        one(hub.create(None, "LC".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
        let mut watch = Box::pin(hub.watch(None, None, None, None).await);
        let commits = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();

        let ev = one(hub.link_commits(None, "lc-1".into(), commits(&["77df9a0", "plexus-macros@4F7114A"])).await).await;
        let wire = serde_json::to_value(&ev).unwrap();
        assert_eq!(wire["type"], "commits_linked");
        assert_eq!(wire["added"], serde_json::json!(["77df9a0", "plexus-macros@4f7114a"]));
        assert_eq!(wire["ticket"]["id"], "LC-1");
        assert_eq!(wire["ticket"]["commits"], wire["added"]);
        let ev = one(hub.link_commits(None, "LC-1".into(), commits(&["77df9a0", "0bb28b2"])).await).await;
        let wire = serde_json::to_value(&ev).unwrap();
        assert_eq!(wire["added"], serde_json::json!(["0bb28b2"]), "the third alone");
        let three = serde_json::json!(["77df9a0", "plexus-macros@4f7114a", "0bb28b2"]);
        assert_eq!(wire["ticket"]["commits"], three);
        let got = serde_json::to_value(one(hub.get(None, "LC-1".into()).await).await).unwrap();
        assert_eq!(got["ticket"]["commits"], three, "get shows them, in link order");
        let (rows, _) = ids_and_page(hub.list(None, None, None, None, None, None, Some(true), None, None).await).await;
        assert_eq!(rows, ["LC-1"]);
        let evs: Vec<TicketsEvent> = Box::pin(hub.list(None, None, None, None, None, None, Some(true), None, None).await).collect().await;
        assert_eq!(serde_json::to_value(&evs[0]).unwrap()["summary"]["commits"], three, "and the summary row");

        for (i, added) in [&["77df9a0"], &["0bb28b2"]].into_iter().enumerate() {
            match watch.next().await.expect("change") {
                TicketsEvent::CommitsLinked { ticket, added: got } => {
                    assert_eq!((ticket.id.as_str(), got.len()), ("LC-1", if i == 0 { 2 } else { 1 }));
                    assert_eq!(got[0], added[0]);
                }
                other => panic!("expected CommitsLinked, got {other:?}"),
            }
        }

        // The no-op, the refusal, the miss: nothing on `watch`.
        match one(hub.link_commits(None, "LC-1".into(), commits(&["0BB28B2"])).await).await {
            TicketsEvent::CommitsLinked { added, ticket } => {
                assert!(added.is_empty(), "{added:?}");
                assert_eq!(ticket.commits.len(), 3);
            }
            other => panic!("expected CommitsLinked, got {other:?}"),
        }
        match one(hub.link_commits(None, "LC-1".into(), commits(&["c8c5678", "77df9a0;rm", "090cdec"])).await).await {
            TicketsEvent::Error { message } => assert!(message.contains("'77df9a0;rm'"), "names the entry: {message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        assert!(matches!(
            one(hub.link_commits(None, "LC-9".into(), commits(&["c8c5678"])).await).await,
            TicketsEvent::NotFound { id } if id == "LC-9"
        ));
        let got = serde_json::to_value(one(hub.get(None, "LC-1".into()).await).await).unwrap();
        assert_eq!(got["ticket"]["commits"], three, "the refused call wrote nothing");

        let ev = one(hub.unlink_commits(None, "LC-1".into(), commits(&["plexus-macros@4f7114a", "c8c5678"])).await).await;
        let wire = serde_json::to_value(&ev).unwrap();
        assert_eq!(wire["type"], "commits_unlinked");
        assert_eq!(wire["removed"], serde_json::json!(["plexus-macros@4f7114a"]));
        assert_eq!(wire["ticket"]["commits"], serde_json::json!(["77df9a0", "0bb28b2"]), "exactly what it named");
        match one(hub.unlink_commits(None, "LC-1".into(), commits(&["c8c5678"])).await).await {
            TicketsEvent::CommitsUnlinked { removed, .. } => assert!(removed.is_empty()),
            other => panic!("expected CommitsUnlinked, got {other:?}"),
        }
        match watch.next().await.expect("change") {
            TicketsEvent::CommitsUnlinked { ticket, removed } => {
                assert_eq!(removed, ["plexus-macros@4f7114a"], "the no-op, the refusal and the miss said nothing");
                assert_eq!(ticket.commits, ["77df9a0", "0bb28b2"]);
            }
            other => panic!("expected CommitsUnlinked, got {other:?}"),
        }
        assert!(futures::FutureExt::now_or_never(watch.next()).is_none(), "and nothing more");
    }

    #[tokio::test]
    async fn watch_sees_creations_and_status_changes() {
        let hub = hub();
        let mut watch = Box::pin(hub.watch(None, None, None, None).await);

        one(hub.create(None, "MNG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
        match watch.next().await.expect("change") {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "MNG-1"),
            other => panic!("expected Created, got {other:?}"),
        }
        // MNG-10: a new shortcode's row appears after the ticket that made it.
        match watch.next().await.expect("change") {
            TicketsEvent::GroupChanged { group } => assert_eq!((group.epic.as_str(), group.total), ("MNG", 1)),
            other => panic!("expected GroupChanged, got {other:?}"),
        }

        one(hub.set_status(None, "MNG-1".into(), "Ready".into()).await).await;
        match watch.next().await.expect("change") {
            TicketsEvent::StatusChanged { previous, .. } => assert_eq!(previous, Status::Pending),
            other => panic!("expected StatusChanged, got {other:?}"),
        }
    }

    /// MNG-10: `groups` yields one `Group` per shortcode row and nothing
    /// else — no trailing page; the stream's end is the answer's end — and
    /// `watch { epic }` delivers a `GroupChanged` by the group's own
    /// shortcode: a watcher of PLX hears PLX's count move when a ticket of
    /// its sub-shortcode completes, without hearing the ticket.
    #[tokio::test]
    async fn groups_yields_the_rows_and_watch_filters_group_changes_by_their_shortcode() {
        let hub = hub();
        one(hub.create(None, "PLX".into(), "host".into(), Some("scope".into()), None, None, None, None, None).await).await;
        one(hub.create(None, "SUB".into(), "hosted".into(), Some("scope".into()), None, None, None, Some("PLX-1".into()), None).await).await;
        one(hub.create(None, "SUB".into(), "work".into(), None, None, None, None, None, None).await).await;

        let evs: Vec<TicketsEvent> = Box::pin(hub.groups(None).await).collect().await;
        let groups: Vec<crate::types::Group> = evs
            .into_iter()
            .map(|ev| match ev {
                TicketsEvent::Group { group } => group,
                other => panic!("groups yields only `group` events, got {other:?}"),
            })
            .collect();
        assert_eq!(
            groups.iter().map(|g| (g.epic.as_str(), g.total, g.host.as_deref())).collect::<Vec<_>>(),
            [("PLX", 3, None), ("SUB", 2, Some("PLX-1"))]
        );
        assert_eq!(serde_json::to_value(&groups[0]).unwrap()["host"], serde_json::Value::Null, "null, not missing");

        let mut plx = Box::pin(hub.watch(None, Some("plx".into()), None, None).await);
        one(hub.set_status(None, "SUB-2".into(), "Complete".into()).await).await;
        match plx.next().await.expect("change") {
            TicketsEvent::GroupChanged { group } => {
                assert_eq!((group.epic.as_str(), group.total, group.done), ("PLX", 3, 1))
            }
            other => panic!("the SUB ticket's status_changed is filtered out; got {other:?}"),
        }
    }

    /// SCALE-16: `extents` yields one `Extent` per shortcode that holds a
    /// ticket and nothing else — no trailing page — in shortcode order;
    /// a refused create moves nothing, and the `last_seq`s add up to
    /// `health`'s count.
    #[tokio::test]
    async fn extents_yields_each_shortcodes_last_seq_in_shortcode_order() {
        let hub = hub();
        for (epic, kind) in [("ZED", "scope"), ("AB", "scope"), ("ZED", "build"), ("B", "scope"), ("ZED", "gap")] {
            one(hub.create(None, epic.into(), "t".into(), Some(kind.into()), None, None, None, None, None).await).await;
        }
        // Refused: AB-2 would be its own blocker. Nothing is allocated.
        let refused = one(hub.create(None, "AB".into(), "t".into(), None, None, Some(vec!["AB-2".into()]), None, None, None).await).await;
        assert!(matches!(refused, TicketsEvent::Error { .. }), "{refused:?}");

        let evs: Vec<TicketsEvent> = Box::pin(hub.extents(None).await).collect().await;
        let extents: Vec<(String, u32)> = evs
            .into_iter()
            .map(|ev| match ev {
                TicketsEvent::Extent { epic, last_seq } => (epic, last_seq),
                other => panic!("extents yields only `extent` events, got {other:?}"),
            })
            .collect();
        assert_eq!(
            extents,
            [("AB".to_string(), 1), ("B".to_string(), 1), ("ZED".to_string(), 3)],
            "byte order on the epic: AB before B"
        );
        let total: u64 = extents.iter().map(|(_, n)| u64::from(*n)).sum();
        match one(hub.health(None).await).await {
            TicketsEvent::Health { ticket_count, .. } => assert_eq!(ticket_count, total),
            other => panic!("expected Health, got {other:?}"),
        }
        let wire = serde_json::to_value(TicketsEvent::Extent { epic: "AB".into(), last_seq: 1 }).unwrap();
        assert_eq!(wire, serde_json::json!({"type": "extent", "epic": "AB", "last_seq": 1}));
    }

    /// Ids of a `list` / `ready_set` stream, in order — asserting the
    /// MNG-9 shape on the way through: rows, then exactly one closing
    /// `page` event.
    async fn ticket_ids(s: impl Stream<Item = TicketsEvent> + Send + 'static) -> Vec<String> {
        let (ids, page) = ids_and_page(s).await;
        assert!(page.is_some(), "every list/ready_set call ends with a page");
        ids
    }

    /// The same, keeping the page event: `(ids, Some((next, remaining)))`.
    #[allow(clippy::type_complexity)]
    async fn ids_and_page(
        s: impl Stream<Item = TicketsEvent> + Send + 'static,
    ) -> (Vec<String>, Option<(Option<String>, u64)>) {
        let evs: Vec<TicketsEvent> = Box::pin(s).collect().await;
        let mut ids = Vec::new();
        let mut page = None;
        for ev in evs {
            assert!(page.is_none(), "the page event is last: {ev:?}");
            match ev {
                TicketsEvent::Ticket { ticket } => ids.push(ticket.id),
                TicketsEvent::Summary { summary } => ids.push(summary.id),
                TicketsEvent::Page { next, remaining, .. } => page = Some((next, remaining)),
                other => panic!("expected a row or the page, got {other:?}"),
            }
        }
        (ids, page)
    }

    #[tokio::test]
    async fn ready_set_gates_on_blockers() {
        let hub = hub();
        one(hub.create(None, "DAG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
        one(hub
            .create(None, "DAG".into(), "b".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None)
            .await)
        .await;
        one(hub.set_status(None, "DAG-1".into(), "Ready".into()).await).await;
        one(hub.set_status(None, "DAG-2".into(), "Ready".into()).await).await;

        assert_eq!(ticket_ids(hub.ready_set(None, None, None, None, None, None).await).await, vec!["DAG-1"]);

        one(hub.set_status(None, "DAG-1".into(), "Complete".into()).await).await;
        assert_eq!(ticket_ids(hub.ready_set(None, None, None, None, None, None).await).await, vec!["DAG-2"]);
    }

    #[tokio::test]
    async fn ready_set_accepts_superseded_blockers_and_filters_by_epic() {
        let hub = hub();
        one(hub.create(None, "ONE".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
        one(hub
            .create(None, "ONE".into(), "b".into(), None, None, Some(vec!["ONE-1".into()]), None, None, None)
            .await)
        .await;
        one(hub.create(None, "TWO".into(), "c".into(), Some("scope".into()), None, None, None, None, None).await).await;
        one(hub.set_status(None, "ONE-1".into(), "Superseded".into()).await).await;
        one(hub.set_status(None, "ONE-2".into(), "Ready".into()).await).await;
        one(hub.set_status(None, "TWO-1".into(), "Ready".into()).await).await;

        assert_eq!(
            ticket_ids(hub.ready_set(None, None, None, None, None, None).await).await,
            vec!["ONE-2", "TWO-1"]
        );
        assert_eq!(
            ticket_ids(hub.ready_set(None, Some("one".into()), None, None, None, None).await).await,
            vec!["ONE-2"]
        );
    }

    #[tokio::test]
    async fn dangling_blocker_excludes_from_ready_set_and_is_marked() {
        let hub = hub();
        one(hub
            .create(None, "DAG".into(), "c".into(), Some("scope".into()), None, Some(vec!["DAG-99".into()]), None, None, None)
            .await)
        .await;
        one(hub.set_status(None, "DAG-1".into(), "Ready".into()).await).await;

        assert!(ticket_ids(hub.ready_set(None, None, None, None, None, None).await).await.is_empty());

        let evs: Vec<_> = Box::pin(hub.blockers(None, "DAG-1".into()).await).collect().await;
        assert_eq!(evs.len(), 1);
        match &evs[0] {
            TicketsEvent::DanglingBlocker { id } => assert_eq!(id, "DAG-99"),
            other => panic!("expected DanglingBlocker, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn blockers_resolve_to_current_statuses_in_order() {
        let hub = hub();
        one(hub.create(None, "DAG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
        one(hub.create(None, "DAG".into(), "b".into(), None, None, None, None, None, None).await).await;
        one(hub
            .create(None, 
                "DAG".into(),
                "c".into(),
                None,
                None,
                Some(vec!["DAG-1".into(), "DAG-2".into(), "DAG-99".into()]),
                None, None, None,
            )
            .await)
        .await;
        one(hub.set_status(None, "DAG-1".into(), "Complete".into()).await).await;

        let evs: Vec<_> = Box::pin(hub.blockers(None, "DAG-3".into()).await).collect().await;
        assert_eq!(evs.len(), 3);
        match &evs[0] {
            TicketsEvent::Blocker { ticket } => {
                assert_eq!(ticket.id, "DAG-1");
                assert_eq!(ticket.status, Status::Complete);
            }
            other => panic!("expected Blocker, got {other:?}"),
        }
        match &evs[1] {
            TicketsEvent::Blocker { ticket } => {
                assert_eq!(ticket.id, "DAG-2");
                assert_eq!(ticket.status, Status::Pending);
            }
            other => panic!("expected Blocker, got {other:?}"),
        }
        match &evs[2] {
            TicketsEvent::DanglingBlocker { id } => assert_eq!(id, "DAG-99"),
            other => panic!("expected DanglingBlocker, got {other:?}"),
        }

        assert!(matches!(
            one(hub.blockers(None, "DAG-42".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn create_adds_new_id_to_blockers_unlocks_and_watch_carries_it() {
        let hub = hub();
        one(hub.create(None, "DAG".into(), "a".into(), Some("scope".into()), None, None, None, None, None).await).await;
        let mut watch = Box::pin(hub.watch(None, None, None, None).await);

        one(hub
            .create(None, "DAG".into(), "b".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None)
            .await)
        .await;

        match watch.next().await.expect("change") {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "DAG-2"),
            other => panic!("expected Created, got {other:?}"),
        }
        // MNG-14: DAG-2 named no parent, so it hangs under the root — which
        // is also its blocker: one updated ticket, both edges, two events.
        // SCALE-4: the first is the parent's id and count; the second the
        // whole ticket, carrying both edges.
        match watch.next().await.expect("change") {
            TicketsEvent::ChildrenChanged { id, added, child_count } => {
                assert_eq!(id, "DAG-1");
                assert_eq!(child_count, 1);
                assert_eq!(added, "DAG-2");
            }
            other => panic!("expected ChildrenChanged, got {other:?}"),
        }
        match watch.next().await.expect("change") {
            TicketsEvent::UnlocksChanged { ticket, added } => {
                assert_eq!(ticket.id, "DAG-1");
                assert_eq!(ticket.unlocks, vec!["DAG-2"]);
                assert_eq!(ticket.child_count, 1, "both edges on the one ticket");
                assert_eq!(added, "DAG-2");
            }
            other => panic!("expected UnlocksChanged, got {other:?}"),
        }

        match one(hub.get(None, "DAG-1".into()).await).await {
            TicketsEvent::Ticket { ticket } => assert_eq!(ticket.unlocks, vec!["DAG-2"]),
            other => panic!("expected Ticket, got {other:?}"),
        }
    }

    /// AGS-19 acceptance 1: a child's birth is visible from both ends —
    /// `get C` shows the parent, `get P` counts the child and `list
    /// { parent: P }` names it (SCALE-4) — and a watcher sees the parent's
    /// update as `children_changed`: its id, the child and its new count.
    #[tokio::test]
    async fn create_with_parent_updates_children_and_watch_carries_the_parent() {
        let hub = hub();
        one(hub.create(None, "FAM".into(), "P".into(), Some("scope".into()), None, None, None, None, None).await).await;
        let mut watch = Box::pin(hub.watch(None, None, None, None).await);

        match one(
            hub.create(None, 
                "FAM".into(),
                "C".into(),
                Some("execution".into()),
                None,
                None,
                None,
                Some("fam-1".into()), None,
            )
            .await,
        )
        .await
        {
            TicketsEvent::Created { ticket } => {
                assert_eq!(ticket.id, "FAM-2");
                assert_eq!(ticket.parent.as_deref(), Some("FAM-1"), "case-insensitive");
                assert_eq!(ticket.child_count, 0);
            }
            other => panic!("expected Created, got {other:?}"),
        }

        match watch.next().await.expect("change") {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "FAM-2"),
            other => panic!("expected Created, got {other:?}"),
        }
        match watch.next().await.expect("change") {
            TicketsEvent::ChildrenChanged { id, added, child_count } => {
                assert_eq!(id, "FAM-1");
                assert_eq!(added, "FAM-2");
                assert_eq!(child_count, 1, "the count, not the parent");
            }
            other => panic!("expected ChildrenChanged, got {other:?}"),
        }
        let wire = serde_json::to_value(TicketsEvent::ChildrenChanged {
            id: "FAM-1".into(),
            added: "FAM-2".into(),
            child_count: 1,
        })
        .unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"type": "children_changed", "id": "FAM-1", "added": "FAM-2", "child_count": 1})
        );

        match one(hub.get(None, "FAM-1".into()).await).await {
            TicketsEvent::Ticket { ticket } => {
                assert_eq!(ticket.child_count, 1);
                assert_eq!(ticket.parent, None);
            }
            other => panic!("expected Ticket, got {other:?}"),
        }
        assert_eq!(
            ticket_ids(hub.list(None, None, None, Some("FAM-1".into()), None, None, None, None, None).await).await,
            vec!["FAM-2"]
        );
        match one(hub.get(None, "FAM-2".into()).await).await {
            TicketsEvent::Ticket { ticket } => assert_eq!(ticket.parent.as_deref(), Some("FAM-1")),
            other => panic!("expected Ticket, got {other:?}"),
        }
    }

    /// AGS-19 acceptance 2: a parent that names no ticket is an in-band
    /// error naming the reason, and nothing is created.
    #[tokio::test]
    async fn create_rejects_a_nonexistent_parent_in_band() {
        let hub = hub();
        let ev = one(
            hub.create(None, 
                "ORPH".into(),
                "no such parent".into(),
                Some("scope".into()),
                None,
                None,
                None,
                Some("ORPH-9".into()), None,
            )
            .await,
        )
        .await;
        match ev {
            TicketsEvent::Error { message } => {
                assert!(message.contains("ORPH-9"), "names the parent: {message}");
                assert!(message.contains("must exist"), "names the reason: {message}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
        assert!(matches!(
            one(hub.get(None, "ORPH-1".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    /// AGS-19 acceptance 4: `list {parent: P}` yields exactly P's
    /// children, in (epic, seq) order.
    #[tokio::test]
    async fn list_filters_by_parent() {
        let hub = hub();
        one(hub.create(None, "LP".into(), "P".into(), Some("scope".into()), None, None, None, None, None).await).await;
        one(hub.create(None, "LP".into(), "unrelated".into(), None, None, None, None, None, None).await).await;
        // ZZ's first ticket is its scope root (MNG-12), filed under LP-1.
        for (epic, title, kind) in [("LP", "child a", None), ("ZZ", "child b", Some("scope"))] {
            one(hub
                .create(None, 
                    epic.into(),
                    title.into(),
                    kind.map(String::from),
                    None,
                    None,
                    None,
                    Some("LP-1".into()), None,
                )
                .await)
            .await;
        }

        // MNG-14: "unrelated" (LP-2) named no parent and hangs under the root.
        assert_eq!(
            ticket_ids(hub.list(None, None, None, Some("LP-1".into()), None, None, None, None, None).await).await,
            vec!["LP-2", "LP-3", "ZZ-1"]
        );
        assert!(ticket_ids(hub.list(None, None, None, Some("LP-2".into()), None, None, None, None, None).await)
            .await
            .is_empty());
        assert_eq!(
            ticket_ids(hub.list(None, None, None, None, None, None, None, None, None).await).await.len(),
            4,
            "unfiltered list is unchanged"
        );
    }

    /// MNG-9: `summary: true` swaps the row shape — every field of the
    /// ticket except the body, and the serialized row carries no `body`
    /// key at all (what a board renders; the body arrives via `get`).
    #[tokio::test]
    async fn summary_rows_carry_every_field_but_the_body() {
        let hub = hub();
        one(hub
            .create(None, 
                "SUM".into(),
                "with a body".into(),
                Some("scope".into()), // MNG-12: a prefix's first ticket is its scope
                Some("High".into()),
                None,
                Some("## Problem\n\nthe weight of a list".into()),
                None, None,
            )
            .await)
        .await;

        let evs: Vec<_> = Box::pin(hub.list(None, None, None, None, None, None, Some(true), None, None).await)
            .collect()
            .await;
        assert_eq!(evs.len(), 2, "one row, one page: {evs:?}");
        let row = serde_json::to_value(&evs[0]).unwrap();
        assert_eq!(row["type"], "summary");
        let summary = &row["summary"];
        assert!(summary.get("body").is_none(), "no body on a summary: {summary}");
        assert_eq!(summary["id"], "SUM-1");
        assert_eq!(summary["kind"], "scope");
        assert_eq!(summary["severity"], "High");
        assert!(summary["parent"].is_null(), "an absent parent is null, not missing");
        assert_eq!(summary["child_count"], 0);
        assert!(summary.get("children").is_none(), "SCALE-4: no list of children: {summary}");

        // The full ticket still carries it, unchanged.
        let evs: Vec<_> = Box::pin(hub.list(None, None, None, None, None, None, None, None, None).await)
            .collect()
            .await;
        let row = serde_json::to_value(&evs[0]).unwrap();
        assert_eq!(row["type"], "ticket");
        assert!(row["ticket"]["body"].as_str().unwrap().contains("the weight of a list"));
    }

    /// MNG-9: `after` naming no ticket is one in-band error naming it,
    /// and nothing else — no rows, no page (there is nowhere to resume).
    #[tokio::test]
    async fn an_after_that_names_no_ticket_is_one_in_band_error() {
        let hub = hub();
        one(hub.create(None, "CUR".into(), "a".into(), None, None, None, None, None, None).await).await;

        for evs in [
            Box::pin(hub.list(None, None, None, None, Some("ZZZ-9".into()), None, None, None, None).await)
                .collect::<Vec<_>>()
                .await,
            Box::pin(hub.ready_set(None, None, Some("ZZZ-9".into()), None, None, None).await)
                .collect::<Vec<_>>()
                .await,
        ] {
            assert_eq!(evs.len(), 1, "one event, no page: {evs:?}");
            match &evs[0] {
                TicketsEvent::Error { message } => {
                    assert_eq!(message, "after: no ticket ZZZ-9");
                }
                other => panic!("expected Error, got {other:?}"),
            }
        }
    }

    /// MNG-9: `ready_set` takes the same three params, and its page event
    /// counts the ready set — not the store.
    #[tokio::test]
    async fn ready_set_takes_the_same_page_params() {
        let hub = hub();
        for i in 0..3 {
            let kind = (i == 0).then(|| "scope".to_string()); // MNG-12: the root first
            one(hub.create(None, "RS".into(), "r".into(), kind, None, None, None, None, None).await).await;
        }
        one(hub.create(None, "RS".into(), "still pending".into(), None, None, None, None, None, None).await).await;
        for id in ["RS-1", "RS-2", "RS-3"] {
            one(hub.set_status(None, id.into(), "Ready".into()).await).await;
        }

        let (ids, page) = ids_and_page(hub.ready_set(None, None, None, Some(2), Some(true), None).await).await;
        assert_eq!(ids, vec!["RS-1", "RS-2"]);
        assert_eq!(
            page,
            Some((Some("RS-2".into()), 1)),
            "one ready ticket left, not two store rows"
        );

        let (ids, page) =
            ids_and_page(hub.ready_set(None, None, Some("RS-2".into()), Some(2), Some(true), None).await).await;
        assert_eq!(ids, vec!["RS-3"]);
        assert_eq!(page, Some((None, 0)));
    }

    /// SCALE-6: `find` yields `summary` rows — never full tickets — then
    /// one `page`; with no `limit` it answers the palette's 50, and
    /// `remaining` is the rest of the matches. The query is trimmed and
    /// case-folded; `after` is `list`'s cursor (a position: FND-3 does not
    /// match "ticket page"), and one naming no ticket is the one in-band
    /// error.
    #[tokio::test]
    async fn find_yields_summary_rows_then_one_page_fifty_by_default() {
        let hub = hub();
        for i in 1..=60 {
            let kind = (i == 1).then(|| "scope".to_string()); // MNG-12: the root first
            one(hub.create(None, "FND".into(), format!("row {i}"), kind, None, None, Some("a body".into()), None, None).await).await;
        }
        one(hub.create(None, "OTH".into(), "The Ticket Page".into(), Some("scope".into()), None, None, None, None, None).await).await;

        let evs: Vec<TicketsEvent> = Box::pin(hub.find(None, "".into(), None, None, None).await).collect().await;
        assert_eq!(evs.len(), 51, "50 rows and the page");
        let row = serde_json::to_value(&evs[0]).unwrap();
        assert_eq!((row["type"].as_str(), row["summary"]["id"].as_str()), (Some("summary"), Some("FND-1")));
        assert!(row["summary"].get("body").is_none(), "a summary carries no body: {row}");
        let (ids, page) = ids_and_page(hub.find(None, "".into(), None, None, None).await).await;
        assert_eq!(ids.len(), 50, "the palette's 50 when no limit is asked");
        assert_eq!(page, Some((Some("FND-50".into()), 11)));

        let (ids, page) = ids_and_page(hub.find(None, "  fnd-5 ".into(), None, None, None).await).await;
        let expected: Vec<String> =
            std::iter::once(5).chain(50..=59).map(|n| format!("FND-{n}")).collect();
        assert_eq!(ids, expected, "trimmed, case-folded, in (epic, seq) order");
        assert_eq!(page, Some((None, 0)));

        let (ids, page) =
            ids_and_page(hub.find(None, "ticket page".into(), Some("fnd-3".into()), Some(1), None).await).await;
        assert_eq!((ids, page), (vec!["OTH-1".to_string()], Some((None, 0))));

        let evs: Vec<TicketsEvent> = Box::pin(hub.find(None, "row".into(), Some("ZZZ-9".into()), None, None).await)
            .collect()
            .await;
        match evs.as_slice() {
            [TicketsEvent::Error { message }] => assert_eq!(message, "after: no ticket ZZZ-9"),
            other => panic!("one in-band error and nothing else, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dangling_forward_reference_resolves_at_birth() {
        let hub = hub();
        one(hub
            .create(None, "DAG".into(), "d1".into(), Some("scope".into()), None, Some(vec!["DAG-2".into()]), None, None, None)
            .await)
        .await;
        // The ticket that becomes DAG-2 has DAG-1 in unlocks from birth.
        match one(hub.create(None, "DAG".into(), "d2".into(), None, None, None, None, None, None).await).await {
            TicketsEvent::Created { ticket } => {
                assert_eq!(ticket.id, "DAG-2");
                assert_eq!(ticket.unlocks, vec!["DAG-1"]);
            }
            other => panic!("expected Created, got {other:?}"),
        }
        match one(hub.get(None, "DAG-2".into()).await).await {
            TicketsEvent::Ticket { ticket } => assert_eq!(ticket.unlocks, vec!["DAG-1"]),
            other => panic!("expected Ticket, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_rejects_self_reference() {
        let hub = hub();
        // First create in the epic allocates DAG-1; naming it is a self-cycle.
        let ev = one(hub
            .create(None, "DAG".into(), "me".into(), Some("scope".into()), None, Some(vec!["DAG-1".into()]), None, None, None)
            .await)
        .await;
        match ev {
            TicketsEvent::Error { message } => assert!(message.contains("cycle"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        assert!(matches!(
            one(hub.get(None, "DAG-1".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn create_rejects_cycle_through_dangling_forward_reference() {
        let hub = hub();
        // DAG-1 blocked_by the not-yet-existing DAG-2.
        one(hub
            .create(None, "DAG".into(), "d1".into(), Some("scope".into()), None, Some(vec!["DAG-2".into()]), None, None, None)
            .await)
        .await;
        // The create that would become DAG-2, blocked_by DAG-1, closes the loop.
        let ev = one(hub
            .create(None, "DAG".into(), "d2".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None)
            .await)
        .await;
        match ev {
            TicketsEvent::Error { message } => assert!(message.contains("cycle"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        // Nothing was stored.
        assert!(matches!(
            one(hub.get(None, "DAG-2".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn create_rejects_longer_cycles() {
        let hub = hub();
        one(hub
            .create(None, "DAG".into(), "p1".into(), Some("scope".into()), None, Some(vec!["DAG-3".into()]), None, None, None)
            .await)
        .await;
        one(hub
            .create(None, "DAG".into(), "p2".into(), None, None, Some(vec!["DAG-1".into()]), None, None, None)
            .await)
        .await;
        // Would become DAG-3: DAG-3 -> DAG-2 -> DAG-1 -> DAG-3.
        let ev = one(hub
            .create(None, "DAG".into(), "p3".into(), None, None, Some(vec!["DAG-2".into()]), None, None, None)
            .await)
        .await;
        assert!(matches!(ev, TicketsEvent::Error { .. }), "got {ev:?}");
        assert!(matches!(
            one(hub.get(None, "DAG-3".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn watch_epic_filter_drops_other_epics() {
        let hub = hub();
        let mut watch = Box::pin(hub.watch(None, Some("mng".into()), None, None).await);

        one(hub.create(None, "OTHER".into(), "x".into(), Some("scope".into()), None, None, None, None, None).await).await;
        one(hub.create(None, "MNG".into(), "y".into(), Some("scope".into()), None, None, None, None, None).await).await;

        match watch.next().await.expect("change") {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.epic, "MNG"),
            other => panic!("expected Created, got {other:?}"),
        }
    }

    /// One line per event, unique within this test's burst — so "the
    /// newest changes, none twice" is a comparison of lists.
    fn label(ev: &TicketsEvent) -> String {
        match ev {
            TicketsEvent::Created { ticket } => format!("created {}", ticket.id),
            TicketsEvent::ChildrenChanged { id, added, .. } => format!("children {id} +{added}"),
            TicketsEvent::GroupChanged { group } => format!("group {} {}", group.epic, group.total),
            TicketsEvent::Lagged { missed } => format!("lagged {missed}"),
            other => format!("{other:?}"),
        }
    }

    /// SCALE-5 criterion 1, against the store's real channel (capacity 256,
    /// untouched): a `watch` subscribed and then left unread while 200
    /// creates broadcast 600 changes — each create is `created`, the root's
    /// `children_changed`, the shortcode's `group_changed` — reads exactly
    /// one `lagged { missed: 344 }` (600 - 256, the count the channel
    /// reports), then precisely the newest 256 changes in acknowledgment
    /// order, none twice, then later changes as usual. A `watch { epic }`
    /// on another shortcode is told too, and then hears only its own.
    #[tokio::test]
    async fn a_watch_that_falls_behind_reads_one_lagged_then_the_newest_changes() {
        const CREATES: u64 = 200;
        const HELD: u64 = 256; // Store::with_connection's broadcast capacity
        let hub = hub();
        one(hub.create(None, "LAG".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
        let mut watch = Box::pin(hub.watch(None, None, None, None).await);
        let mut other = Box::pin(hub.watch(None, Some("other".into()), None, None).await);

        let mut broadcast: Vec<String> = Vec::new();
        for k in 2..=CREATES + 1 {
            match one(hub.create(None, "LAG".into(), format!("t{k}"), None, None, None, None, None, None).await).await {
                TicketsEvent::Created { ticket } => assert_eq!(ticket.id, format!("LAG-{k}")),
                ev => panic!("expected Created, got {ev:?}"),
            }
            broadcast.push(format!("created LAG-{k}"));
            broadcast.push(format!("children LAG-1 +LAG-{k}"));
            broadcast.push(format!("group LAG {k}"));
        }
        let missed = broadcast.len() as u64 - HELD;
        assert_eq!(missed, 344);

        // The filtered subscriber is told as well; the 256 LAG changes still
        // held are then read and filtered out, and nothing else is ready.
        assert_eq!(label(&other.next().await.expect("an event")), format!("lagged {missed}"), "past the epic filter");
        assert!(futures::FutureExt::now_or_never(other.next()).is_none(), "no LAG change passes the filter");

        let mut read: Vec<String> = Vec::new();
        for _ in 0..=HELD {
            read.push(label(&watch.next().await.expect("an event")));
        }
        assert_eq!(read[0], format!("lagged {missed}"), "one lagged, first, with the channel's count");
        assert_eq!(read[1..], broadcast[missed as usize..], "then the newest {HELD}, in order, none twice");
        assert_eq!(read.iter().filter(|l| l.starts_with("lagged")).count(), 1);
        assert!(futures::FutureExt::now_or_never(watch.next()).is_none(), "and nothing more");

        one(hub.create(None, "LAG".into(), "after".into(), None, None, None, None, None, None).await).await;
        assert_eq!(label(&watch.next().await.expect("an event")), format!("created LAG-{}", CREATES + 2), "and on, as usual");

        one(hub.create(None, "OTHER".into(), "its own".into(), Some("scope".into()), None, None, None, None, None).await).await;
        assert_eq!(label(&other.next().await.expect("an event")), "created OTHER-1", "then only its own epic");

        let wire = serde_json::to_value(TicketsEvent::Lagged { missed: 3 }).unwrap();
        assert_eq!(wire, serde_json::json!({"type": "lagged", "missed": 3}));
    }
    /// MNG-12/14: `set_parent` over the activation — `ParentChanged` on a
    /// move, `NotFound` for a missing child, in-band errors for a missing
    /// parent, a parent outside the shortcode, and ancestry; `watch`
    /// carries both parents' sides.
    #[tokio::test]
    async fn set_parent_yields_parent_changed_and_maps_errors_in_band() {
        let hub = hub();
        one(hub.create(None, "SP".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;
        one(hub.create(None, "SP".into(), "a".into(), None, None, None, None, None, None).await).await; // SP-2 → SP-1
        one(hub.create(None, "SP".into(), "b".into(), None, None, None, None, None, None).await).await; // SP-3 → SP-1
        one(hub.create(None, "OT".into(), "other".into(), Some("scope".into()), None, None, None, None, None).await).await;
        let mut watch = Box::pin(hub.watch(None, None, None, None).await);

        match one(hub.set_parent(None, "sp-3".into(), "sp-2".into()).await).await {
            TicketsEvent::ParentChanged { ticket, previous } => {
                assert_eq!(ticket.id, "SP-3");
                assert_eq!(ticket.parent.as_deref(), Some("SP-2"));
                assert_eq!(previous.as_deref(), Some("SP-1"));
            }
            other => panic!("expected ParentChanged, got {other:?}"),
        }
        assert!(matches!(watch.next().await, Some(TicketsEvent::ParentChanged { .. })));
        match watch.next().await {
            Some(TicketsEvent::ChildrenChanged { id, added, child_count }) => {
                assert_eq!(id, "SP-2");
                assert_eq!(added, "SP-3");
                assert_eq!(child_count, 1);
            }
            other => panic!("expected ChildrenChanged, got {other:?}"),
        }
        match watch.next().await {
            Some(TicketsEvent::ChildRemoved { id, removed, child_count }) => {
                assert_eq!(id, "SP-1");
                assert_eq!(removed, "SP-3");
                assert_eq!(child_count, 1, "SP-2 is left");
            }
            other => panic!("expected ChildRemoved, got {other:?}"),
        }

        assert!(matches!(
            one(hub.set_parent(None, "SP-9".into(), "SP-1".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
        match one(hub.set_parent(None, "SP-2".into(), "SP-9".into()).await).await {
            TicketsEvent::Error { message } => {
                assert!(message.contains("SP-9") && message.contains("must exist"), "{message}")
            }
            other => panic!("expected Error, got {other:?}"),
        }
        match one(hub.set_parent(None, "SP-2".into(), "OT-1".into()).await).await {
            TicketsEvent::Error { message } => {
                assert!(message.contains("OT-1") && message.contains("outside"), "{message}")
            }
            other => panic!("expected Error, got {other:?}"),
        }
        match one(hub.set_parent(None, "SP-1".into(), "SP-3".into()).await).await {
            TicketsEvent::Error { message } => assert!(message.contains("own ancestor"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        // The root may be hosted by another shortcode.
        match one(hub.set_parent(None, "SP-1".into(), "OT-1".into()).await).await {
            TicketsEvent::ParentChanged { ticket, .. } => assert_eq!(ticket.parent.as_deref(), Some("OT-1")),
            other => panic!("expected ParentChanged, got {other:?}"),
        }
    }

    /// MNG-12: the first ticket of a prefix must be a scope — over the
    /// activation the refusal is the in-band error naming the rule.
    #[tokio::test]
    async fn first_ticket_of_a_prefix_must_be_a_scope_in_band() {
        let hub = hub();
        match one(hub.create(None, "FRESH".into(), "a build".into(), None, None, None, None, None, None).await).await {
            TicketsEvent::Error { message } => {
                assert!(message.contains("FRESH") && message.contains("scope"), "{message}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
        assert!(matches!(
            one(hub.get(None, "FRESH-1".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    /// The `waves` stream, split the way the contract reads it: one
    /// `(id, placement, waiting_on)` per `placed`, then the one closing
    /// `wave_counts` — asserted last and alone.
    #[allow(clippy::type_complexity)]
    async fn placed_and_counts(
        s: impl Stream<Item = TicketsEvent> + Send + 'static,
    ) -> (Vec<(String, crate::types::Placement, Vec<String>)>, (Vec<u64>, u64, u64)) {
        let evs: Vec<TicketsEvent> = Box::pin(s).collect().await;
        let mut placed = Vec::new();
        let mut counts = None;
        for ev in evs {
            assert!(counts.is_none(), "wave_counts is last: {ev:?}");
            match ev {
                TicketsEvent::Placed { summary, placement, waiting_on } => {
                    placed.push((summary.id, placement, waiting_on))
                }
                TicketsEvent::WaveCounts { waves, missing, done } => counts = Some((waves, missing, done)),
                other => panic!("expected placed or wave_counts, got {other:?}"),
            }
        }
        (placed, counts.expect("every waves answer ends with wave_counts"))
    }

    /// WAVE-4 acceptance 2: the yield order exactly as specified — waves
    /// ascending; inside a wave Coding, In Review, Ready, Pending, Blocked,
    /// Idea, Epic, then (epic, seq); then the missing lane; then the done,
    /// each in (epic, seq) — with the default kinds (the scope roots are not
    /// work), a blocker followed into a shortcode not asked for, and the
    /// tally after the rows, exactly once.
    #[tokio::test]
    async fn waves_yields_placements_in_the_specified_order_then_one_wave_counts() {
        use crate::types::Placement::{Done, MissingBlocker, Wave};
        let hub = hub();
        let create = |epic: &str, kind: Option<&str>, blocked_by: &[&str]| {
            let blocked_by = (!blocked_by.is_empty()).then(|| blocked_by.iter().map(|b| b.to_string()).collect());
            let (epic, kind) = (epic.to_string(), kind.map(String::from));
            let hub = hub.clone();
            async move { one(hub.create(None, epic, "t".into(), kind, None, blocked_by, None, None, None).await).await }
        };
        let status = |id: &str, s: &str| {
            let (id, s, hub) = (id.to_string(), s.to_string(), hub.clone());
            async move { one(hub.set_status(None, id, s).await).await }
        };
        for epic in ["WA", "WB", "ELSE"] {
            create(epic, Some("scope"), &[]).await; // X-1, each shortcode's root
        }
        create("ELSE", None, &[]).await; // ELSE-2, wave 1 — not asked for
        create("WA", None, &["ELSE-2"]).await; // WA-2: wave 2, Pending
        create("WB", Some("spike"), &[]).await; // WB-2: wave 1, Pending
        create("WA", Some("gap"), &[]).await; // WA-3: wave 1, Ready
        status("WA-3", "Ready").await;
        create("WB", None, &[]).await; // WB-3: wave 1, Coding
        status("WB-3", "active").await;
        create("WA", None, &["WB-3"]).await; // WA-4: wave 2, In Review
        status("WA-4", "in review").await;
        create("WB", None, &["WB-9"]).await; // WB-4: missing (WB-9 is no ticket)
        create("WA", None, &["WB-4"]).await; // WA-5: missing, behind WB-4
        create("WA", None, &[]).await; // WA-6: done
        status("WA-6", "Complete").await;
        create("WA", Some("execution"), &[]).await; // WA-7: not work, not yielded
        create("WB", None, &[]).await; // WB-5: wave 1, Blocked (a marker, not an edge)
        status("WB-5", "Blocked").await;

        let (placed, counts) = placed_and_counts(hub.waves(None, vec!["wa".into(), "WB".into()], None).await).await;
        let want: Vec<(&str, crate::types::Placement, Vec<&str>)> = vec![
            ("WB-3", Wave { n: 1 }, vec![]),
            ("WA-3", Wave { n: 1 }, vec![]),
            ("WB-2", Wave { n: 1 }, vec![]),
            ("WB-5", Wave { n: 1 }, vec![]),
            ("WA-4", Wave { n: 2 }, vec!["WB-3"]),
            ("WA-2", Wave { n: 2 }, vec!["ELSE-2"]),
            ("WA-5", MissingBlocker, vec!["WB-4"]),
            ("WB-4", MissingBlocker, vec!["WB-9"]),
            ("WA-6", Done, vec![]),
        ];
        let got: Vec<(&str, crate::types::Placement, Vec<&str>)> = placed
            .iter()
            .map(|(id, p, w)| (id.as_str(), *p, w.iter().map(String::as_str).collect()))
            .collect();
        assert_eq!(got, want);
        assert_eq!(counts, (vec![4, 2], 2, 1));

        // `kinds` names what is yielded: the scopes and the execution too.
        let (placed, counts) = placed_and_counts(
            hub.waves(None, vec!["WA".into()], Some(vec!["scope".into(), "Execution".into()])).await,
        )
        .await;
        let ids: Vec<&str> = placed.iter().map(|(id, _, _)| id.as_str()).collect();
        assert_eq!(ids, ["WA-1", "WA-7"]);
        assert_eq!(counts, (vec![2], 0, 0));

        // A shortcode with no tickets: nothing placed, the tally all zero.
        let (placed, counts) = placed_and_counts(hub.waves(None, vec!["NONE".into()], None).await).await;
        assert!(placed.is_empty());
        assert_eq!(counts, (vec![], 0, 0));
    }

    /// WAVE-4 acceptance 2: the error cases are in-band — `epics` empty, and
    /// a kind outside the six (the error `create` gives, naming them) — one
    /// `Error` and nothing else, no tally.
    #[tokio::test]
    async fn waves_errors_are_in_band_and_alone() {
        let hub = hub();
        one(hub.create(None, "ER".into(), "root".into(), Some("scope".into()), None, None, None, None, None).await).await;

        let evs: Vec<TicketsEvent> = Box::pin(hub.waves(None, vec![], None).await).collect().await;
        assert_eq!(evs.len(), 1, "{evs:?}");
        match &evs[0] {
            TicketsEvent::Error { message } => assert!(message.contains("epics"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }

        let evs: Vec<TicketsEvent> =
            Box::pin(hub.waves(None, vec!["ER".into()], Some(vec!["build".into(), "bug".into()])).await).collect().await;
        assert_eq!(evs.len(), 1, "{evs:?}");
        match &evs[0] {
            TicketsEvent::Error { message } => {
                assert!(message.contains("'bug'"), "{message}");
                for k in Kind::ALL {
                    assert!(message.contains(k.as_str()), "names {k}: {message}");
                }
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
