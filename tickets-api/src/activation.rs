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

use crate::types::{InputValue, Kind, Severity, Status, TicketsEvent};
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::types::InputState;
use crate::queue::Urgency;
use crate::vocab::{
    Body, CallerSource, CommitRef, Cursor, DeleteChildren, FilterText, InputId, InputSource, Label, MengerPath, Message,
    Query, Reason, SeverityChange, ShortcodeRef, TicketRef, Title, Zone,
};
use crate::address::OrgId;
#[cfg(feature = "server")]
use crate::store::{
    BlockerRef, Change, Filtered, InputWrite, Lens, Membership, NewTicket, Page, PageQuery, Select, Step, Store, StoreError, Walk,
};
#[cfg(feature = "server")]
use crate::store::{TreePage, TreeQuery};
// MENGER-13: the transition log.
#[cfg(feature = "server")]
use crate::store::{Act, Clock};
#[cfg(feature = "server")]
use crate::types::TransitionSource;
#[cfg(feature = "server")]
use crate::types::AnswerSource;
// FILTER-15: the reads that take the lens.
#[cfg(feature = "server")]
use crate::store::{ready_and, MEMBER_MAX};
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
use crate::access::{Access, Caller, ADMINISTER};
#[cfg(feature = "server")]
use plexus_core::plexus::{AuthContext, Grant, Reach};

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

// MENGER-29: everything but the methods lives beside this file —
// `events` (store answers and changes as wire events), `sight` (the
// caller's reach, clock and act) and the tests — so this file stays the
// activation's `impl` and inside its budget (tickets-api/tests/sizes.rs).
#[cfg(feature = "server")]
mod events;
#[cfg(feature = "server")]
mod sight;
#[cfg(feature = "server")]
mod update;
#[cfg(feature = "server")]
use events::*;
#[cfg(feature = "server")]
use sight::*;
#[cfg(feature = "server")]
use update::*;
#[cfg(feature = "server")]
mod waves;
#[cfg(feature = "server")]
use waves::*;
#[cfg(feature = "server")]
mod overview;
#[cfg(feature = "server")]
use overview::*;
#[cfg(feature = "server")]
mod delete;
#[cfg(feature = "server")]
use delete::*;
#[cfg(feature = "server")]
mod await_reply;
#[cfg(feature = "server")]
use await_reply::await_reply_stream;
#[cfg(feature = "server")]
mod queue;
#[cfg(feature = "server")]
use queue::*;
#[cfg(feature = "server")]
mod reads;
#[cfg(feature = "server")]
use reads::*;
#[cfg(feature = "server")]
mod orgs;
#[cfg(feature = "server")]
use orgs::*;
#[cfg(all(test, feature = "server"))]
mod tests;

// The attribute is NOT `#[cfg]`-gated: it has to run in the client-only
// build too, or there would be no client. `server_cfg` tells it to put the
// server half — this impl block included — behind the feature.
// AUTH-7: the default roles, explicit lists of grants (AUTH-1) — what
// IDENTITY-21's three roles meant, method by method: `viewer` reads,
// `editor` reads and writes; `admin`, built in, holds every grant. An
// admin's `auth.role_set` edit wins over these (kept in the store).
#[plexus_macros::activation(
    namespace = "tickets",
    description = "Plans as data — create, query, and watch the work DAG",
    client,
    typed_params,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core",
    roles(
        viewer = [
            "health", "get", "list", "ready_set", "find", "search", "member", "groups", "components",
            "inputs", "extents", "counts", "facets", "tree", "waves", "waves_under", "waves_to", "blockers", "watch", "history",
            "queued", "await_reply"
        ],
        editor = [
            viewer, "create", "set_status", "set_kind", "set_parent", "amend", "link_commits",
            "unlink_commits", "label", "update", "update_many", "answer", "clear_answer", "close_input", "reopen_input",
            "queue", "unqueue", "snooze", "request_changes", "reshortcode"
        ],
    )
)]
impl Tickets {
    /// Heartbeat. Returns the binary version, the server's unix time, and
    /// how many tickets the store holds.
    #[plexus_macros::method(group = "read", description = "Heartbeat — version, unix time, ticket count")]
    pub async fn health(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
        group = "write",
        description = "Create a ticket; allocates the next EPIC-N id, status starts Pending (a doc's: Doc, for good); the first ticket of a prefix must be a scope or a doc",
        params(
            epic = "Epic prefix, e.g. MNG (case-insensitive, stored uppercase)",
            title = "Short description",
            kind = "scope | execution | build | gap | spike | doc (default: build; the first ticket of a prefix must be scope or doc; a doc has no lifecycle and takes no blocked_by)",
            severity = "Critical | High | Medium | Low (optional)",
            blocked_by = "Ticket ids that must complete first (optional; never a doc, and not for a doc)",
            body = "Markdown body (optional)",
            parent = "Id of the ticket this one is an output of; must exist and be in the same shortcode (default: the shortcode's scope root). The shortcode's own scope root may name a ticket in another shortcode: the one it hangs under",
            path = "Path address inside the shortcode, e.g. _MENGER/board — then `get` finds it as SHORTCODE/_MENGER/board (optional; only _MENGER/… on a doc directly under the shortcode's scope root, unique in the shortcode)",
            org = "ORG-4: the organization, e.g. acme (optional: your only one; refused, naming them, when you hold roles in several and the epic is new)"
        ),
        values(kind = ["scope", "execution", "build", "gap", "spike", "doc"], severity = ["Critical", "High", "Medium", "Low"]),
        example(note = "a build ticket under MENGER-2", params = r#"{"epic": "MENGER", "title": "Wrap long titles on the board", "kind": "build", "parent": "MENGER-2"}"#),
        example(note = "a gap, blocked on another ticket", params = r#"{"epic": "MENGER", "title": "Board loses focus on reload", "kind": "gap", "severity": "High", "blocked_by": ["MENGER-40"]}"#)
    )]
    pub async fn create(
        &self, auth: Option<&AuthContext>, reach: Reach,
        epic: ShortcodeRef,
        title: Title,
        kind: Option<Kind>,
        severity: Option<Severity>,
        blocked_by: Option<Vec<TicketRef>>,
        body: Option<Body>,
        parent: Option<TicketRef>,
        path: Option<MengerPath>,
        org: Option<OrgId>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let blocked_by = blocked_by.map(texts);
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            // ORG-4: the organization first — `--org`, the caller's only
            // one, or refused naming them (activation/orgs.rs) — then
            // IDENTITY-21's `tickets.create` where it lands.
            let epic = match epic_in(&store, &caller, epic.into_string(), org, parent.as_deref()) {
                Ok(e) => e,
                Err(ev) => {
                    yield ev;
                    return;
                }
            };
            let reach = match create_admits(&store, &caller, &epic, parent.as_deref(), blocked_by.as_deref()) {
                Ok(r) => r,
                Err(ev) => {
                    yield ev;
                    return;
                }
            };
            let kind = kind.unwrap_or(Kind::Build);
            // MENGER-13: its first status is a transition, the caller's.
            let act = match act_of(&caller, None, None) {
                Ok(a) => a,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            match store.create_by(NewTicket {
                epic,
                title: title.into_string(),
                kind,
                severity,
                blocked_by: blocked_by.unwrap_or_default(),
                parent: parent.map(String::from),
                path: path.map(String::from),
                body: body.map(String::from).unwrap_or_default(),
            }, act).await {
                Ok(ticket) => yield TicketsEvent::Created { ticket },
                Err(e @ StoreError::BodyTooLarge { .. }) => yield too_large(e),
                Err(e) => yield TicketsEvent::Error { message: redacted(e, |id| sees_id(&store, reach.as_ref(), id)) },
            }
        }
    }

    /// Yields exactly one event: `Ticket` if found, `NotFound` otherwise.
    /// WAVE-10: `id` may be a path address, `<SHORTCODE>/<path>` — the
    /// shortcode case-insensitive, the path exact — naming the one ticket of
    /// that shortcode that holds the path; `NotFound` carries the address.
    #[plexus_macros::method(
        group = "read",
        description = "Get one ticket by id, or by path address SHORTCODE/path (e.g. PLEXUS/_MENGER/board)",
        params(id = "Ticket id, e.g. MNG-2 — or a path address, e.g. PLEXUS/_MENGER/board"),
        example(note = "one ticket", params = r#"{"id": "MENGER-40"}"#),
        example(note = "a doc by its path address", params = r#"{"id": "PLEXUS/_MENGER/board"}"#)
    )]
    pub async fn get(&self, auth: Option<&AuthContext>, reach: Reach, id: TicketRef) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        get_stream(self.store.clone(), self.access.admit(auth, &reach), id.into_string())
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
    /// rows that carry every field but the body. ACCESS-29: a page is 1,000
    /// rows unless `limit` says, and never past 10,000 ([`crate::limits`]).
    /// An `after` naming no ticket
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
    ///
    /// FILTER-24: with a filter, `keep` (a ticket id: the nav's open ticket)
    /// is listed and counted as a match whatever the filter says of it — at
    /// its place, in `first` and `remaining` — when it is inside `epic`,
    /// `status` and `parent` and inside the caller's reach; otherwise, and
    /// without a filter, it changes nothing.
    #[plexus_macros::method(
        group = "read",
        description = "List tickets, optionally filtered by epic, status, parent and/or a filter; paged with after + limit, or at a position with at",
        params(
            epic = "Epic prefix filter (optional)",
            status = "Status filter: Pending | Ready | Coding | In Review | Blocked | Complete | Idea | Epic | Superseded | Canceled | Doc (optional)",
            parent = "Only this ticket's children, e.g. PERF-3 (optional)",
            after = "Cursor: start strictly after this ticket id, e.g. MNG-9 (optional)",
            limit = "At most this many tickets (default 1000, at most 10000); the page event carries the next cursor (optional)",
            summary = "Yield summary rows (every field but the body) instead of full tickets (optional)",
            filter = "A filter, e.g. 'ready and kind build and in SCALE' (FILTER-3's language; optional): ANDed with epic, status and parent; the page event then carries first",
            at = "Start at this position among the matches, 0-based, instead of after a cursor (optional; not with after); the page event carries first",
            keep = "With a filter: this ticket id is listed and counted as a match whatever the filter says of it, e.g. the open ticket (optional; one inside epic, status and parent)",
            tz = "The time zone a history filter's days are read in (today, yesterday, this week, a date): an IANA name, e.g. Europe/Berlin, UTC, or an offset, e.g. +02:00 (optional; default UTC)"
        ),
        values(status = ["Pending", "Ready", "Coding", "In Review", "Blocked", "Complete", "Idea", "Epic", "Superseded", "Canceled", "Doc"]),
        example(note = "a shortcode's tickets, summary rows, 20 at a time", params = r#"{"epic": "MENGER", "summary": true, "limit": 20}"#),
        example(note = "the ready build work in SCALE", params = r#"{"filter": "ready and kind build and in SCALE", "summary": true}"#),
        example(note = "in review, and got there in the last 12 hours", params = r#"{"filter": "status \"In Review\" and entered \"In Review\" within 12h", "summary": true}"#)
    )]
    pub async fn list(
        &self, auth: Option<&AuthContext>, reach: Reach,
        epic: Option<ShortcodeRef>,
        status: Option<Status>,
        parent: Option<TicketRef>,
        after: Option<Cursor>,
        limit: Option<u32>,
        summary: Option<bool>,
        filter: Option<FilterText>,
        at: Option<u64>,
        keep: Option<TicketRef>,
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let (epic, parent) = (epic.map(String::from), parent.map(String::from));
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        let limit = Some(crate::limits::page(limit, crate::limits::LIST_PAGE_DEFAULT, crate::limits::LIST_PAGE_MAX));
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let keep = if filter.is_some() { kept(&store, &caller, keep.as_deref()) } else { None };
            let filter = match caller.scoped(filter.as_deref()) {
                Ok(v) => v,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            if filter.is_some() || at.is_some() {
                // FILTER-4: from the cells.
                if after.is_some() && at.is_some() {
                    yield TicketsEvent::Error { message: StoreError::AtAndAfter.to_string() };
                    return;
                }
                let lens = match store.lens_or_all(filter.as_deref()) {
                    Ok(lens) => lens,
                    Err(e) => {
                        yield veiled(&caller, e);
                        return;
                    }
                };
                let lens = match clock_of(&caller, tz.as_deref()) {
                    Ok(clock) => lens.with_clock(clock),
                    Err(message) => {
                        yield TicketsEvent::Error { message };
                        return;
                    }
                };
                let q = Filtered { lens, epic, status, parent, keep };
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
    ///
    /// MENGER-13: the change is a transition — the caller's (`anonymous`
    /// with login off), from `source` (`api` unless the call says `app` or
    /// `pipeline`), with its `message` — kept in the log and said on `watch`
    /// right after the `StatusChanged`. Into `Canceled`, the ticket's open
    /// inputs are withdrawn and every descendant still open work is
    /// canceled in the same write, each a transition of `system`'s naming
    /// the cause. Out of `Canceled`, its withdrawn inputs come back; with
    /// `restore_children`, so do the descendants its cancel canceled (and
    /// nothing has moved since), to the statuses they had.
    #[plexus_macros::method(
        group = "write",
        target = "id",
        description = "Set a ticket's status (not a doc's: a doc has no lifecycle); Canceled cancels its open descendants in the same write, and leaving Canceled may restore them (restore_children)",
        params(
            id = "Ticket id, e.g. MNG-2",
            status = "New status: Pending | Ready | Coding | In Review | Blocked | Complete | Idea | Epic | Superseded | Canceled (the canonical spelling; through -p, the app or REST any case, `active` is Coding; never Doc, a doc's own)",
            restore_children = "Leaving Canceled: also put back, to the statuses they had, the descendants its cancel canceled and nothing has moved since (optional, default false; history's history_end names them)",
            source = "Where the change comes from: app | api | pipeline (optional, default api) — kept in the transition",
            message = "Why, kept in the transition (optional)"
        ),
        values(status = ["Pending", "Ready", "Coding", "In Review", "Blocked", "Complete", "Idea", "Epic", "Superseded", "Canceled"], source = ["app", "api", "pipeline"]),
        example(note = "start work on MENGER-40", params = r#"{"id": "MENGER-40", "status": "Coding"}"#),
        example(note = "hand it to review", params = r#"{"id": "MENGER-40", "status": "In Review"}"#),
        example(note = "cancel a scope and everything open under it", params = r#"{"id": "MENGER-7", "status": "Canceled", "message": "folded into MENGER-9"}"#),
        example(note = "undo it, children too", params = r#"{"id": "MENGER-7", "status": "Pending", "restore_children": true}"#)
    )]
    pub async fn set_status(
        &self, auth: Option<&AuthContext>, reach: Reach,
        id: TicketRef,
        status: Status,
        restore_children: Option<bool>,
        source: Option<CallerSource>,
        message: Option<Message>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            // AUTH-7: the gate held the grant on the target; the sign-in
            // must still stand.
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let act = match act_of(&caller, source, message) {
                Ok(a) => a,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            match store.set_status_by(&id, status, act, restore_children.unwrap_or(false)).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: id.into_string() },
                Err(e) => yield veiled(&caller, e),
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
        group = "write",
        target = "id",
        description = "Set a ticket's kind; everything kept by kind moves with it (to a doc: status Doc; from a doc: the status given, else Epic for a scope and Pending otherwise)",
        params(
            id = "Ticket id, e.g. IDENTITY-1",
            kind = "New kind: scope | execution | build | gap | spike | doc (case-insensitive)",
            status = "Its status with the new kind (optional): Pending | Ready | Coding | In Review | Blocked | Complete | Idea | Epic | Superseded | Canceled — or Doc, the only one a doc takes; omitted, a doc becomes Doc, a doc becoming work becomes Epic (a scope) or Pending, and work keeps its own",
            source = "Where the change comes from: app | api | pipeline (optional, default api) — kept in the transitions",
            message = "Why, kept in the transitions (optional)"
        ),
        values(kind = ["scope", "execution", "build", "gap", "spike", "doc"], status = ["Pending", "Ready", "Coding", "In Review", "Blocked", "Complete", "Idea", "Epic", "Superseded", "Canceled", "Doc"], source = ["app", "api", "pipeline"]),
        example(note = "make a ticket a scope", params = r#"{"id": "IDENTITY-1", "kind": "scope"}"#),
        example(note = "make it a gap, ready to start", params = r#"{"id": "MENGER-40", "kind": "gap", "status": "Ready"}"#)
    )]
    pub async fn set_kind(
        &self, auth: Option<&AuthContext>, reach: Reach,
        id: TicketRef,
        kind: Kind,
        status: Option<Status>,
        source: Option<CallerSource>,
        message: Option<Message>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            // AUTH-7: the gate held the grant on the target; the sign-in
            // must still stand.
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            // MENGER-13: the kind's change (and the status's) is the caller's.
            let act = match act_of(&caller, source, message) {
                Ok(a) => a,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            match store.set_kind_by(&id, kind, status, act).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: id.into_string() },
                Err(e) => yield veiled(&caller, e),
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
        group = "write",
        target = "id",
        description = "Set a ticket's parent — the ticket it is an output of; adopts a parentless ticket or moves one",
        params(
            id = "Ticket id to file, e.g. AGS-2",
            parent = "Id of the ticket to file it under; any epic, must exist"
        )
    )]
    pub async fn set_parent(
        &self, auth: Option<&AuthContext>, reach: Reach,
        id: TicketRef,
        parent: TicketRef,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            // IDENTITY-37: a shortcode's root moves the shortcode — and
            // whoever administers its new host administers it — so it takes
            // admin on the shortcode and on where it goes; any other ticket
            // moves inside its shortcode, and takes `tickets.set_parent` on
            // both sides. AUTH-7: the gate held the grant on `id` (the
            // target); the rest is its own question, as it stands now.
            let root = store.groups().into_iter().any(|g| g.root.as_deref().is_some_and(|r| r.eq_ignore_ascii_case(id.trim())));
            let wanted = if root { Some(Grant::new(ADMINISTER)) } else { caller.grant.clone() };
            if let Some(g) = wanted {
                if let Err(message) = caller.check(&g, &id).and_then(|()| caller.check(&g, &parent)) {
                    let message = if root { format!("moving a shortcode's root takes admin on it and on its new host: {message}") } else { message };
                    yield TicketsEvent::Error { message };
                    return;
                }
            }
            match store.set_parent(&id, &parent).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: id.into_string() },
                Err(e) => yield veiled(&caller, e),
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
        group = "write",
        target = "id",
        description = "Replace a ticket's title and/or body (whole values); yields amended { ticket, changed }",
        params(
            id = "Ticket id, e.g. MNG-2",
            title = "New title (optional; must not be blank)",
            body = "New markdown body, the whole of it (optional)"
        ),
        example(note = "retitle a ticket", params = r#"{"id": "MENGER-40", "title": "Wrap long titles on the board and in the nav"}"#)
    )]
    pub async fn amend(
        &self, auth: Option<&AuthContext>, reach: Reach,
        id: TicketRef,
        title: Option<Title>,
        body: Option<Body>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            // AUTH-7: the gate held the grant on the target; the sign-in
            // must still stand.
            if let Err(message) = admitted {
                yield TicketsEvent::Error { message };
                return;
            }
            match store.amend(&id, title.map(String::from), body.map(String::from)).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: id.into_string() },
                Err(e @ StoreError::BodyTooLarge { .. }) => yield too_large(e),
                Err(e) => yield refusal(e),
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
    /// `CommitsLinked` that added something. ACT-2: `ids` instead of `id`
    /// links them to each, in one write, all or none, one `CommitsLinked`
    /// per ticket (`cells_stream`).
    #[plexus_macros::method(
        group = "write",
        target = "id",
        description = "Link commits to a ticket, or to many (ids; one write, all or none) — the commits that finished it; yields commits_linked { ticket, added } per ticket",
        params(
            id = "Ticket id, e.g. MNG-10 (or ids)",
            ids = "Ticket ids, instead of id: every one gets the commits, in one write, all or none",
            commits = "Commit references, in link order: a hex object name of 7-64 characters, optionally repo@ before it — e.g. [\"77df9a0\", \"plexus-macros@4f7114a\"]"
        ),
        example(note = "link the commit that finished it", params = r#"{"id": "MENGER-40", "commits": ["77df9a0"]}"#),
        example(note = "and one in another repository", params = r#"{"id": "MENGER-40", "commits": ["77df9a0", "plexus-macros@4f7114a"]}"#)
    )]
    pub async fn link_commits(
        &self, auth: Option<&AuthContext>, reach: Reach,
        id: Option<TicketRef>,
        ids: Option<Vec<TicketRef>>,
        commits: Vec<CommitRef>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        // AUTH-7: the gate held the grant on `id`; `ids` are asked there.
        cells_stream(self.store.clone(), self.access.admit(auth, &reach), id, ids, CellWrite::Commits(commits))
    }

    /// SDLC-9: the inverse of `link_commits` — every entry the ticket
    /// carries comes off. The same references, compared the same way, all
    /// or nothing. Yields exactly one event: `CommitsUnlinked { ticket,
    /// removed }` — `removed` empty when none of them was linked, which
    /// writes and broadcasts nothing — `NotFound`, or an in-band `Error`
    /// naming every entry that is not a reference. `watch` carries every
    /// `CommitsUnlinked` that removed something.
    #[plexus_macros::method(
        group = "write",
        target = "id",
        description = "Unlink commits from a ticket; yields commits_unlinked { ticket, removed }",
        params(
            id = "Ticket id, e.g. MNG-10",
            commits = "Commit references to remove, as linked (the hash is compared lowercase)"
        )
    )]
    pub async fn unlink_commits(
        &self, auth: Option<&AuthContext>, reach: Reach,
        id: TicketRef,
        commits: Vec<CommitRef>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            // AUTH-7: the gate held the grant on the target; the sign-in
            // must still stand.
            if let Err(message) = admitted {
                yield TicketsEvent::Error { message };
                return;
            }
            match store.unlink_commits(&id, texts(commits)).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: id.into_string() },
                Err(e) => yield refusal(e),
            }
        }
    }

    /// MENGER-12: moves a ticket's labels — free-form tags such as
    /// `needs-you`, which puts the ticket on Home's "Needs you" panel.
    /// `add` and `remove` are label names (letters, digits and `-`, at most
    /// 32 characters, folded to lower case); a label in both is added. The
    /// ticket keeps its labels sorted, each once, at most 16. Yields exactly
    /// one event: `LabelsChanged { ticket, added, removed }` — both empty
    /// when nothing moved (every added label already held, every removed one
    /// not), which writes and broadcasts nothing — `NotFound` if `id` names
    /// no ticket, or an in-band `Error` naming every entry that is not a
    /// label, or a result past 16; an error writes nothing. Moves
    /// `updated_unix` and the ticket's filter cell (`label <name>`), nothing
    /// else. `watch` carries every `LabelsChanged` that moved something.
    /// ACT-2: `ids` instead of `id` moves each one's, in one write, all or
    /// none, one `LabelsChanged` per ticket (`cells_stream`).
    #[plexus_macros::method(
        group = "write",
        target = "id",
        description = "Add or remove a ticket's labels (e.g. needs-you), or many tickets' (ids; one write, all or none); yields labels_changed { ticket, added, removed } per ticket",
        params(
            id = "Ticket id, e.g. MENGER-12 (or ids)",
            ids = "Ticket ids, instead of id: every one's labels move, in one write, all or none",
            add = "Labels to add — letters, digits and -, at most 32 characters, lower-cased (e.g. [\"needs-you\"]) (optional)",
            remove = "Labels to remove (optional)"
        ),
        example(note = "put a ticket in front of the owner", params = r#"{"id": "MENGER-40", "add": ["needs-you"]}"#),
        example(note = "and take it off once acted on", params = r#"{"id": "MENGER-40", "remove": ["needs-you"]}"#)
    )]
    pub async fn label(
        &self, auth: Option<&AuthContext>, reach: Reach,
        id: Option<TicketRef>,
        ids: Option<Vec<TicketRef>>,
        add: Option<Vec<Label>>,
        remove: Option<Vec<Label>>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        // AUTH-7: the gate held the grant on `id`; `ids` are asked there.
        let write = CellWrite::Labels(add.unwrap_or_default(), remove.unwrap_or_default());
        cells_stream(self.store.clone(), self.access.admit(auth, &reach), id, ids, write)
    }

    /// MENGER-30: any part of a ticket in one write, all of it or none — the
    /// rules, the steps and the events are [`crate::store::Update`]'s
    /// module's. Yields exactly one event: `Updated { ticket, changed }`
    /// (`changed` empty: nothing written or sent), `NotFound`,
    /// `BodyTooLarge`, or one `Error` naming every refusal.
    #[plexus_macros::method(
        group = "write",
        target = "id",
        description = "Change any part of a ticket in one write — title, body, kind, status, parent, severity, blockers (block / unblock); all lands or none does; yields updated { ticket, changed }",
        params(
            id = "Ticket id, e.g. MENGER-40",
            title = "New title (optional; must not be blank)",
            body = "New markdown body, the whole of it (optional)",
            kind = "New kind: scope | execution | build | gap | spike | doc (optional; set_kind's rules — to a doc the status becomes Doc)",
            status = "New status (optional; with a new kind, its status as set_kind takes it; alone, as set_status takes it — Canceled cascades)",
            parent = "Id of the ticket to file it under (optional; set_parent's rules)",
            severity = "Critical | High | Medium | Low — or none, to clear it (optional)",
            block = "Ticket ids it waits on from now on (optional; never itself, a cycle or a doc; an id not born yet is a forward reference)",
            unblock = "Ticket ids it no longer waits on (optional; an id in both block and unblock is refused)",
            source = "Where the change comes from: app | api | pipeline (optional, default api) — kept in its transitions",
            message = "Why, kept in its transitions (optional)"
        ),
        values(kind = ["scope", "execution", "build", "gap", "spike", "doc"], status = ["Pending", "Ready", "Coding", "In Review", "Blocked", "Complete", "Idea", "Epic", "Superseded", "Canceled", "Doc"], severity = ["Critical", "High", "Medium", "Low", "none"], source = ["app", "api", "pipeline"]),
        example(note = "add a blocker after filing", params = r#"{"id": "THEME-4", "block": ["THEME-7"]}"#),
        example(note = "swap a blocker and set the severity", params = r#"{"id": "THEME-4", "unblock": ["THEME-3"], "severity": "High"}"#),
        example(note = "several aspects at once", params = r#"{"id": "MENGER-40", "title": "Board loses focus on reload", "kind": "gap", "status": "Ready", "parent": "MENGER-2"}"#)
    )]
    pub async fn update(
        &self, auth: Option<&AuthContext>, reach: Reach,
        id: TicketRef,
        title: Option<Title>,
        body: Option<Body>,
        kind: Option<Kind>,
        status: Option<Status>,
        parent: Option<TicketRef>,
        severity: Option<SeverityChange>,
        block: Option<Vec<TicketRef>>,
        unblock: Option<Vec<TicketRef>>,
        source: Option<CallerSource>,
        message: Option<Message>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            // AUTH-7: the gate held the grant on the target; a parent or a
            // blocker beyond it is asked here (`update_reach`).
            let asked = update_of(title, body, kind, status, parent, severity, block, unblock);
            let caller = match admitted.and_then(|c| update_reach(&store, &c, &id, &asked).map(|()| c)) {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let (act, sight) = match (act_of(&caller, source, message), sight_lens(&caller)) {
                (Ok(a), Ok(s)) => (a, s),
                (Err(message), _) | (_, Err(message)) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            match store.update_by(&id, asked, act).await {
                Ok(crate::store::Updated { ticket, changed }) => yield TicketsEvent::Updated { ticket, changed },
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: id.into_string() },
                Err(e @ StoreError::BodyTooLarge { .. }) => yield too_large(e),
                Err(e) => yield TicketsEvent::Error { message: redacted(e, |id| sees_id(&store, sight.as_ref(), id)) },
            }
        }
    }

    /// ACT-2: `update`'s change to many tickets in one write, all or none —
    /// the selection, the rules, the steps and the events are
    /// [`crate::store::UpdatedMany`]'s module's. Yields exactly one event:
    /// `UpdatedMany { ids, changed, tickets, unchanged }`, or one `Error`
    /// naming every refused ticket and why.
    #[plexus_macros::method(
        group = "write",
        target = "ids",
        description = "The same update to many tickets in one write — kind, status, parent, severity, blockers, labels; by ids or a filter; all lands or none does; yields updated_many { ids, changed, tickets, unchanged }",
        params(
            ids = "Ticket ids, e.g. [\"THEME-9\", \"THEME-10\"] (or filter)",
            filter = "Instead of ids: the tickets this filter admits at the write, e.g. 'label stale and not category done' (FILTER-3's language)",
            max = "With a filter: the most tickets it may change (default 500); more is refused, naming the count",
            tz = "The time zone a history filter's days are read in (optional; default UTC)",
            kind = "New kind (optional; set_kind's rules)",
            status = "New status (optional; set_status's rules — Canceled cascades)",
            parent = "Id of the ticket to file every one under (optional; set_parent's rules — a cycle among them refused)",
            severity = "Critical | High | Medium | Low — or none, to clear it (optional)",
            block = "Ticket ids every one waits on from now on (optional)",
            unblock = "Ticket ids none waits on any more (optional)",
            label = "Labels to add to every one (optional; label's rules)",
            unlabel = "Labels to take off every one (optional)",
            source = "app | api | pipeline (optional, default api) — kept in its transitions",
            message = "Why, kept in every transition the write makes (optional)"
        ),
        values(kind = ["scope", "execution", "build", "gap", "spike", "doc"], status = ["Pending", "Ready", "Coding", "In Review", "Blocked", "Complete", "Idea", "Epic", "Superseded", "Canceled", "Doc"], severity = ["Critical", "High", "Medium", "Low", "none"], source = ["app", "api", "pipeline"]),
        example(note = "promote three tickets", params = r#"{"ids": ["THEME-9", "THEME-10", "THEME-11"], "status": "Ready"}"#),
        example(note = "sweep by a filter", params = r#"{"filter": "label stale and not category done", "status": "Canceled", "message": "swept"}"#)
    )]
    pub async fn update_many(
        &self, auth: Option<&AuthContext>, reach: Reach,
        ids: Option<Vec<TicketRef>>,
        filter: Option<FilterText>,
        max: Option<u32>,
        tz: Option<Zone>,
        kind: Option<Kind>,
        status: Option<Status>,
        parent: Option<TicketRef>,
        severity: Option<SeverityChange>,
        block: Option<Vec<TicketRef>>,
        unblock: Option<Vec<TicketRef>>,
        label: Option<Vec<Label>>,
        unlabel: Option<Vec<Label>>,
        source: Option<CallerSource>,
        message: Option<Message>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        // AUTH-7: the gate held the grant on each of `ids`; a filter's
        // tickets, and a parent or blocker beyond them, are asked there.
        let mut asked = update_of(None, None, kind, status, parent, severity, block, unblock);
        (asked.label, asked.unlabel) = (texts(label.unwrap_or_default()), texts(unlabel.unwrap_or_default()));
        update_many_stream(self.store.clone(), self.access.admit(auth, &reach), ids, filter, max, tz, asked, source, message)
    }

    /// ACT-5: tickets gone for good — refused while something points at
    /// them unless told what moves; a tombstone kept, the body 30 days. The
    /// rules, the write and the events are [`crate::store::Deleted`]'s
    /// module's. Admin only. Yields `Deleted { ids, tombstones }`, or one
    /// `Error` naming every refused ticket and why.
    #[plexus_macros::method(
        group = "write",
        target = "ids",
        description = "Delete tickets for good (admin): refused while children, blockers, a shortcode, a component, a theme or a live input point at one, unless children reparent|delete and unblock say what moves; a tombstone kept (get answers deleted), the body 30 days for undelete; yields deleted { ids, tombstones }",
        params(
            ids = "Ticket ids, e.g. [\"MENGER-40\"] (or filter)",
            filter = "Instead of ids: the tickets this filter admits at the write (FILTER-3's language)",
            max = "With a filter: the most tickets it may delete (default 500)",
            tz = "The time zone a history filter's days are read in (optional; default UTC)",
            children = "reparent (they move to its parent) | delete (they go too) — required when one has children",
            unblock = "Drop the blocks of the tickets that wait on one (optional; default false: refused)",
            message = "Why — kept in the tombstone (optional)"
        ),
        values(children = ["reparent", "delete"]),
        example(note = "filed twice", params = r#"{"ids": ["MENGER-40"], "message": "filed twice"}"#),
        example(note = "its children move to its parent", params = r#"{"ids": ["THEME-12"], "children": "reparent"}"#)
    )]
    pub async fn delete(
        &self, auth: Option<&AuthContext>, reach: Reach,
        ids: Option<Vec<TicketRef>>,
        filter: Option<FilterText>,
        max: Option<u32>,
        tz: Option<Zone>,
        children: Option<DeleteChildren>,
        unblock: Option<bool>,
        message: Option<Message>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        delete_stream(self.store.clone(), self.access.admit(auth, &reach), ids, filter, max, tz, children, unblock, message)
    }

    /// ACT-5: deleted tickets brought back while their tombstones keep the
    /// body — each child its delete moved and each block it dropped put back
    /// where nothing has moved them since. Admin only. Yields `Undeleted {
    /// ids, tickets }`, or one `Error` naming every refused ticket and why.
    #[plexus_macros::method(
        group = "write",
        target = "ids",
        description = "Bring deleted tickets back (admin) within 30 days of the delete, whole, children and blocks put back; yields undeleted { ids, tickets }",
        params(ids = "Deleted ticket ids, e.g. [\"MENGER-40\"]"),
        example(note = "undo a delete", params = r#"{"ids": ["MENGER-40"]}"#)
    )]
    pub async fn undelete(&self, auth: Option<&AuthContext>, reach: Reach, ids: Vec<TicketRef>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        undelete_stream(self.store.clone(), self.access.admit(auth, &reach), texts(ids))
    }

    /// ACT-6: tickets moved into another shortcode ([`crate::store::Reshortcoded`]'s module).
    #[plexus_macros::method(
        group = "write",
        target = "ids",
        description = "Move tickets into another shortcode, renumbered there in the order given (a scope moved into a new code becomes its root; --new-scope makes the shortcode first; a root moves with --with-children as a rename); old ids keep resolving (get says moved_from); all or none; yields reshortcoded { moves, tickets }",
        params(
            ids = "Ticket ids, in the order they take their new numbers, e.g. [\"THEME-9\", \"THEME-10\"]",
            to = "The shortcode they move into, e.g. LOOK (any short form of its address)",
            new_scope = "Make the shortcode first: its root a new scope of this title under parent (optional)",
            parent = "Where the moved tickets hang, inside the target (optional; default its root) — with new_scope, where the new scope hangs",
            with_children = "Each one's descendants in its own shortcode move with it (optional; default false: they stay, under the nearest ancestor that stays)",
            message = "Why (optional)"
        ),
        example(note = "two tickets into LOOK", params = r#"{"ids": ["THEME-9", "THEME-10"], "to": "LOOK"}"#),
        example(note = "a scope out into a shortcode of its own", params = r#"{"ids": ["MENGER-2"], "to": "APP", "with_children": true}"#)
    )]
    pub async fn reshortcode(
        &self, auth: Option<&AuthContext>, reach: Reach, ids: Vec<TicketRef>, to: ShortcodeRef, new_scope: Option<Title>, parent: Option<TicketRef>, with_children: Option<bool>, message: Option<Message>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let (ids, to, new_scope, parent) = (texts(ids), to.into_string(), new_scope.map(String::from), parent.map(String::from));
        let ask = crate::store::Reshortcode { ids, to, new_scope, parent, with_children: with_children.unwrap_or(false), orgs: None };
        reshortcode_stream(self.store.clone(), self.access.admit(auth, &reach), ask, message)
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
        group = "read",
        description = "Tickets that can start now: Ready, with every blocker Complete, Superseded or Canceled; optionally under a filter",
        params(
            epic = "Only tickets in this epic (optional; blockers still resolve globally)",
            after = "Cursor: start strictly after this ticket id, e.g. MNG-9 (optional)",
            limit = "At most this many tickets; the page event carries the next cursor (optional)",
            summary = "Yield summary rows (every field but the body) instead of full tickets (optional)",
            filter = "A filter, e.g. 'kind build and in SCALE' (FILTER-3's language; optional): the ready set and the filter",
            tz = "The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC)"
        ),
        example(note = "what can start now in SCALE", params = r#"{"filter": "kind build and in SCALE", "summary": true}"#)
    )]
    pub async fn ready_set(
        &self, auth: Option<&AuthContext>, reach: Reach,
        epic: Option<ShortcodeRef>,
        after: Option<Cursor>,
        limit: Option<u32>,
        summary: Option<bool>,
        filter: Option<FilterText>,
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let epic = epic.map(String::from);
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
            let clock = match clock_of(&caller, tz.as_deref()) {
                Ok(c) => c,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            // FILTER-15: from the cells.
            match store.narrowing(filter.as_deref()).and_then(|f| f.map(|f| ready_and(&f, epic.clone())).transpose()) {
                Err(e) => {
                    yield veiled(&caller, e);
                    return;
                }
                Ok(Some(q)) => {
                    let q = Filtered { lens: q.lens.with_clock(clock), ..q };
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
        group = "read",
        description = "Find tickets whose id or title contains the query (case-insensitive), optionally under a filter; summary rows, paged with after + limit",
        params(
            query = "Substring of the ticket id or title, case-insensitive (ASCII), trimmed; empty matches every ticket",
            after = "Cursor: start strictly after this ticket id, e.g. MNG-9 (optional)",
            limit = "At most this many tickets (default 50); the page event carries the next cursor (optional)",
            filter = "A filter, e.g. 'not category done' (FILTER-3's language; optional): only the matches it admits",
            tz = "The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC)"
        ),
        example(note = "jump to a ticket by part of its title", params = r#"{"query": "watch", "filter": "not category done"}"#)
    )]
    pub async fn find(
        &self, auth: Option<&AuthContext>, reach: Reach,
        query: Query,
        after: Option<Cursor>,
        limit: Option<u32>,
        filter: Option<FilterText>,
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
            let page = match zoned_lens(&store, &caller, filter.as_deref(), tz.as_deref()) {
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
        group = "read",
        description = "Search inside tickets: the tickets whose title or body holds every word of the query (prefixes and typos too), best first, each with the passage that matched; optionally under a filter, paged with offset + limit",
        params(
            query = "Words to find, e.g. 'plcement wave' — each also as a prefix; a misspelt one also as the words a typo away; FILTER-23 or \"a phrase\" as a phrase",
            filter = "A filter, e.g. 'not category done' (FILTER-3's language; optional): only the matches it admits",
            limit = "At most this many hits (default 20, at most 100) (optional)",
            offset = "Skip this many matches — the searched event's next (optional)",
            tz = "The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC)"
        ),
        example(note = "tickets that mention placement waves (typos too)", params = r#"{"query": "plcement wave"}"#),
        example(note = "the five best open matches", params = r#"{"query": "set_status", "filter": "not category done", "limit": 5}"#)
    )]
    pub async fn search(
        &self, auth: Option<&AuthContext>, reach: Reach,
        query: Query,
        filter: Option<FilterText>,
        limit: Option<u32>,
        offset: Option<u32>,
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
            let lens = match zoned_lens(&store, &caller, filter.as_deref(), tz.as_deref()) {
                Ok(lens) => lens,
                Err(e) => {
                    yield veiled(&caller, e);
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
                Err(e) => yield veiled(&caller, e),
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
        group = "read",
        description = "Which of these tickets a filter admits: one member { id, matched } per id, in the order given (an unknown id: false); at most 1000 ids",
        params(
            ids = "Ticket ids (or SHORTCODE/path addresses), e.g. [\"FILTER-4\", \"FILTER-5\"]; at most 1000",
            filter = "A filter, e.g. 'status Ready' (FILTER-3's language; empty: every ticket)",
            tz = "The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC)"
        )
    )]
    pub async fn member(&self, auth: Option<&AuthContext>, reach: Reach, ids: Vec<TicketRef>, filter: FilterText, tz: Option<Zone>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let ids = texts(ids);
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
            let lens = match zoned_lens(&store, &caller, Some(&filter), tz.as_deref()) {
                Ok(lens) => lens.unwrap_or_else(Lens::all),
                Err(e) => {
                    yield veiled(&caller, e);
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
        group = "overview",
        description = "One roll-up per shortcode: done/total over its subtree's work, its docs, its root (with the root's title, status and kind), and the host it nests under"
    )]
    pub async fn groups(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
                    if !v.contains(&group.shortcode) {
                        continue;
                    }
                    // IDENTITY-21: hosted under a ticket out of reach, a
                    // shortcode stands at the top for this caller.
                    if group.host.as_deref().is_some_and(|h| store.shortcode_of(h).is_none_or(|sc| !v.contains(&sc))) {
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
    /// IDENTITY-37: a caller who cannot view every shortcode gets the
    /// components whose owner it can view — no orphan — each with only the
    /// references it can view.
    #[plexus_macros::method(
        group = "overview",
        description = "Every component in the registry, in name order: its source, its owner (null: an orphan), what references it, its docs (summary, typed attributes, slots, examples, help; null without <docs>) and its preview attributes"
    )]
    pub async fn components(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let reach = match view_lens(&caller) {
                Ok(r) => r,
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            for component in visible_components(&store, reach.as_ref(), store.components()) {
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
        group = "inputs",
        description = "Every input (a body's <m-decision id> / <m-input id kind>) with its kind, question, options, recommendation, state, answer and history; filtered by ticket, shortcode, state and a filter on its ticket",
        params(
            ticket = "Only this ticket's inputs, e.g. INPUT-9 (optional)",
            epic = "Only inputs of tickets in this shortcode (optional)",
            state = "Only inputs in this state: open | answered | closed | removed | withdrawn (optional; withdrawn: its ticket is canceled)",
            filter = "A filter on the input's ticket, e.g. 'kind build' (FILTER-3's language; optional): only inputs of tickets it admits",
            tz = "The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC)"
        ),
        values(state = ["open", "answered", "closed", "removed", "withdrawn"]),
        example(note = "everything waiting on the owner", params = r#"{"state": "open"}"#),
        example(note = "one ticket's inputs, answers and history", params = r#"{"ticket": "RETURNS-13"}"#)
    )]
    pub async fn inputs(
        &self, auth: Option<&AuthContext>, reach: Reach,
        ticket: Option<TicketRef>,
        epic: Option<ShortcodeRef>,
        state: Option<InputState>,
        filter: Option<FilterText>,
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
                if caller.require(&[t], &store).is_err() {
                    yield TicketsEvent::NotFound { id: t.to_string() };
                    return;
                }
            }
            // FILTER-15: of the tickets the filter admits, by the cells.
            let answer = match zoned_lens(&store, &caller, filter.as_deref(), tz.as_deref()) {
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
                Err(e) => yield veiled(&caller, e),
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
        group = "inputs",
        target = "ticket",
        description = "Answer an input: record a value (checked against its kind and options) with a rationale, a source and the time; yields input_changed",
        params(
            ticket = "Ticket id, e.g. INPUT-9",
            input = "The input's id in that ticket's body, e.g. landing",
            value = "Typed by the input's kind: {\"type\": \"choice\", \"option\": \"archive\"} | {\"type\": \"multi\", \"options\": [\"a\", \"b\"]} | {\"type\": \"text\", \"text\": \"a reply\"}; through synapse, --value.type choice --value.option archive",
            rationale = "Why (optional)",
            source = "app (a viewer's click, said by the host) | api (default)"
        ),
        values(source = ["api", "app"]),
        example(note = "pick an option, saying why", params = r#"{"ticket": "RETURNS-13", "input": "landing", "value": {"type": "choice", "option": "archive"}, "rationale": "holds are real"}"#),
        example(note = "reply in text", params = r#"{"ticket": "RETURNS-13", "input": "notes", "value": {"type": "text", "text": "ship it"}}"#),
        example(note = "tick several (a list inside --value goes whole, with -p)", params = r#"{"ticket": "RETURNS-13", "input": "checks", "value": {"type": "multi", "options": ["unit", "wire"]}}"#)
    )]
    pub async fn answer(
        &self, auth: Option<&AuthContext>, reach: Reach,
        ticket: TicketRef,
        input: InputId,
        value: InputValue,
        rationale: Option<Message>,
        source: Option<InputSource>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            // AUTH-7: the gate held the grant on the target; the sign-in
            // must still stand.
            if let Err(message) = admitted {
                yield TicketsEvent::Error { message };
                return;
            }
            let source = AnswerSource::from(source.unwrap_or_default());
            yield input_event(store.answer(&ticket, &input, value, rationale.and_then(Message::given), source).await, ticket.into_string());
        }
    }

    /// INPUT-3: clears an input's answer — `open` again, the clear in its
    /// history, with its `source` (INPUT-9: `app`, a viewer's click, which
    /// the host says for a frame's `menger.input.clear`; `api`, the default:
    /// anything else). Yields exactly one event: `InputChanged`; `Input`
    /// when it held no answer (nothing written); `NotFound`; or an in-band
    /// `Error` — no such input, closed, removed, an unknown source.
    #[plexus_macros::method(
        group = "inputs",
        target = "ticket",
        description = "Clear an input's answer (open again; kept in its history, with a source); yields input_changed",
        params(
            ticket = "Ticket id, e.g. INPUT-9",
            input = "The input's id in that ticket's body",
            source = "app (a viewer's click, said by the host) | api (default)"
        ),
        values(source = ["api", "app"])
    )]
    pub async fn clear_answer(
        &self, auth: Option<&AuthContext>, reach: Reach,
        ticket: TicketRef,
        input: InputId,
        source: Option<InputSource>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            // AUTH-7: the gate held the grant on the target; the sign-in
            // must still stand.
            if let Err(message) = admitted {
                yield TicketsEvent::Error { message };
                return;
            }
            let source = AnswerSource::from(source.unwrap_or_default());
            yield input_event(store.clear_answer(&ticket, &input, source).await, ticket.into_string());
        }
    }

    /// INPUT-3: closes an input — frozen (a decision's DECIDED), with its
    /// answer or without one; an answer or a clear is refused until it is
    /// reopened. Yields exactly one event: `InputChanged`; `Input` when it
    /// was closed already; `NotFound`; or an in-band `Error` — no such
    /// input, removed.
    #[plexus_macros::method(
        group = "inputs",
        target = "ticket",
        description = "Close an input: freeze its answer (or its lack of one); yields input_changed",
        params(ticket = "Ticket id, e.g. INPUT-9", input = "The input's id in that ticket's body")
    )]
    pub async fn close_input(&self, auth: Option<&AuthContext>, reach: Reach, ticket: TicketRef, input: InputId) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            // AUTH-7: the gate held the grant on the target; the sign-in
            // must still stand.
            if let Err(message) = admitted {
                yield TicketsEvent::Error { message };
                return;
            }
            yield input_event(store.close_input(&ticket, &input).await, ticket.into_string());
        }
    }

    /// INPUT-3: reopens a closed input — `answered` again when it holds an
    /// answer, else `open`. Yields exactly one event: `InputChanged`;
    /// `Input` when it was not closed; `NotFound`; or an in-band `Error` —
    /// no such input, removed.
    #[plexus_macros::method(
        group = "inputs",
        target = "ticket",
        description = "Reopen a closed input; yields input_changed",
        params(ticket = "Ticket id, e.g. INPUT-9", input = "The input's id in that ticket's body")
    )]
    pub async fn reopen_input(&self, auth: Option<&AuthContext>, reach: Reach, ticket: TicketRef, input: InputId) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
        stream! {
            // AUTH-7: the gate held the grant on the target; the sign-in
            // must still stand.
            if let Err(message) = admitted {
                yield TicketsEvent::Error { message };
                return;
            }
            yield input_event(store.reopen_input(&ticket, &input).await, ticket.into_string());
        }
    }

    /// SCALE-16: where each shortcode ends in the store's order. Yields
    /// one `Extent` per shortcode that holds a ticket, in shortcode order
    /// — its highest sequence, read from the per-shortcode sequence
    /// `create` allocates from (SCALE-3), so the cost is the number of
    /// shortcodes. ACT-5: past every deleted number, which a shortcode's
    /// `holes` name — sequences are dense but for them, so with these a
    /// client turns a position in the store's order into a cursor without
    /// walking to it:
    /// prefix sums find the shortcode E and the row k, and `list { after:
    /// "E-k" }` starts at row k + 1. Nothing trails the extents — there is
    /// no cursor to hand back — so the stream's own `done` is the whole
    /// answer, as for `groups` and `blockers`. No watch event keeps them:
    /// `created` already carries the newborn's `epic` and `seq`.
    #[plexus_macros::method(
        group = "overview",
        description = "Each shortcode's last sequence (a deleted ticket's too: numbers are never reused) and the holes below it, in shortcode order — where each shortcode ends in the store's order"
    )]
    pub async fn extents(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
            for (shortcode, epic, last_seq, holes) in store.extents_holed() {
                if visible.as_ref().is_some_and(|v| !v.contains(&shortcode)) {
                    continue;
                }
                yield TicketsEvent::Extent { epic, shortcode, last_seq, holes };
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
    /// `error` and nothing else. FILTER-24: `keep` (a ticket id inside the
    /// caller's reach) is counted as a match whatever the filter says of it —
    /// once — as `list { filter, keep }` pages it.
    #[plexus_macros::method(
        group = "overview",
        description = "Each shortcode's number of tickets matching a filter, in shortcode order — the filtered extents (every ticket without one)",
        params(
            filter = "A filter, e.g. 'ready and kind build' (FILTER-3's language; optional: every ticket)",
            keep = "This ticket id is counted as a match whatever the filter says of it, e.g. the open ticket (optional)",
            tz = "The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC)"
        )
    )]
    pub async fn counts(&self, auth: Option<&AuthContext>, reach: Reach, filter: Option<FilterText>, keep: Option<TicketRef>, tz: Option<Zone>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        counts_stream(self.store.clone(), self.access.admit(auth, &reach), filter, keep, tz)
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
        group = "overview",
        description = "Menu counts: per field (status, category, kind, severity, blocked, ready, has commits, menger doc, in), the matches of base and filter without filter's own groups for that field, by value",
        params(
            filter = "The filter whose menus are counted (optional): its own groups for a field are set aside for that field's counts",
            base = "A filter that always applies, e.g. the global one (optional): never set aside",
            tz = "The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC)"
        )
    )]
    pub async fn facets(&self, auth: Option<&AuthContext>, reach: Reach, filter: Option<FilterText>, base: Option<FilterText>, tz: Option<Zone>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        facets_stream(self.store.clone(), self.access.admit(auth, &reach), filter, base, tz)
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
    ///
    /// FILTER-24: `keep`, with `matches_only` — a ticket id (the nav's open
    /// ticket) answered as if it matched: a row where a match would be, its
    /// own nearest matches answered under it rather than above, a hosted
    /// shortcode placed at it; its node still says `matched: false`. A
    /// match, a shortcode's root, an id naming no ticket, or one outside the
    /// caller's reach changes nothing.
    #[plexus_macros::method(
        group = "read",
        description = "The nav under a filter: a parent's children (or a shortcode's top level) that match or hold a match, each with matched and below (matches strictly under it), then a page; matches_only: the matches nearest below instead, then the hosted shortcodes drawn there",
        params(
            parent = "Whose children, e.g. FILTER-9 (or give epic)",
            epic = "A shortcode's top level: its root, or its tickets when it has none (or give parent)",
            filter = "A filter, e.g. 'status Ready and under SCALE' (FILTER-3's language; empty: every ticket)",
            after = "Cursor: start strictly after this ticket id (optional)",
            limit = "At most this many rows; the page event carries the next cursor (optional)",
            matches_only = "Only matches (optional, default false): a ticket that does not match is not a row, and the matches below it are answered in its place, at the nearest ancestor that matches or at the top of the shortcode; then the hosted shortcodes drawn there",
            keep = "With matches_only: this ticket id is drawn as if it matched — where a match would be, its nearest matches under it — e.g. the open ticket (optional; its node still says matched false)",
            tz = "The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC)"
        ),
        example(note = "FILTER's top level, under a filter", params = r#"{"epic": "FILTER", "filter": "status Ready"}"#)
    )]
    pub async fn tree(
        &self, auth: Option<&AuthContext>, reach: Reach,
        parent: Option<TicketRef>,
        epic: Option<ShortcodeRef>,
        filter: FilterText,
        after: Option<Cursor>,
        limit: Option<u32>,
        matches_only: Option<bool>,
        keep: Option<TicketRef>,
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let (parent, epic) = (parent.map(String::from), epic.map(String::from));
        let store = self.store.clone();
        let admitted = self.access.admit(auth, &reach);
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
            let lens = match store.lens(&filter) {
                Ok(lens) => lens,
                Err(e) => {
                    yield veiled(&caller, e);
                    return;
                }
            };
            let lens = match clock_of(&caller, tz.as_deref()) {
                Ok(clock) => lens.with_clock(clock),
                Err(message) => {
                    yield TicketsEvent::Error { message };
                    return;
                }
            };
            let keep = kept(&store, &caller, keep.as_deref());
            let q = TreeQuery { parent, epic, lens, matches_only: matches_only.unwrap_or(false), keep };
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
        group = "read",
        description = "The order of work: each ticket of the shortcodes placed in its wave over the whole blocked_by graph (or missing_blocker, or done) with what it waits on, then the wave counts",
        params(
            epics = "Shortcodes whose tickets are placed, e.g. [\"PLEXUS\", \"MACRO\"] (case-insensitive; at least one); blockers are followed into every shortcode",
            kinds = "Kinds to place: scope | execution | build | gap | spike (optional; default build, spike, gap — a doc is never placed)"
        ),
        values(kinds = ["scope", "execution", "build", "gap", "spike"])
    )]
    pub async fn waves(
        &self, auth: Option<&AuthContext>, reach: Reach,
        epics: Vec<ShortcodeRef>,
        kinds: Option<Vec<Kind>>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        futures::stream::iter(waves_answer(&self.store, self.access.admit(auth, &reach), texts(epics), kinds))
    }

    /// MENGER-31: the wave board of `id`'s subtree (activation/waves.rs).
    #[plexus_macros::method(
        group = "read",
        description = "The order of work under one ticket: each ticket of its subtree placed in its wave (or missing_blocker, or done) with what it waits on, then the wave counts",
        params(
            id = "Ticket whose subtree is placed, e.g. MENGER-2 (it and every ticket under it, any depth, hosted shortcodes included)",
            kinds = "Kinds to place: scope | execution | build | gap | spike (optional; default build, spike, gap — a doc is never placed)"
        ),
        values(kinds = ["scope", "execution", "build", "gap", "spike"])
    )]
    pub async fn waves_under(&self, auth: Option<&AuthContext>, reach: Reach, id: TicketRef, kinds: Option<Vec<Kind>>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        futures::stream::iter(waves_under_answer(&self.store, self.access.admit(auth, &reach), id.into_string(), kinds))
    }

    /// MENGER-31: the wave board whose final state is `id` (activation/waves.rs).
    #[plexus_macros::method(
        group = "read",
        description = "The wave board whose final state is one ticket: its blockers transitively (and a scope's open descendants) placed in waves, done blockers done, the ticket alone in the last wave; cycles named; then the wave counts",
        params(
            id = "The final ticket, e.g. MENGER-31",
            kinds = "Kinds to yield: scope | execution | build | gap | spike (optional; default all five — never a doc; the ticket itself always; every kind is followed)"
        ),
        values(kinds = ["scope", "execution", "build", "gap", "spike"])
    )]
    pub async fn waves_to(&self, auth: Option<&AuthContext>, reach: Reach, id: TicketRef, kinds: Option<Vec<Kind>>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        futures::stream::iter(waves_to_answer(&self.store, self.access.admit(auth, &reach), id.into_string(), kinds))
    }

    /// "Why can't this start?" in one call. Yields, in `blocked_by`
    /// order, one `Blocker` event per blocker that exists (the full
    /// ticket, at its current status) and one `DanglingBlocker` event per
    /// id that names no ticket yet; `NotFound` if the queried ticket
    /// itself doesn't exist. An empty `blocked_by` yields nothing.
    /// IDENTITY-37: a blocker the caller cannot view is a `DanglingBlocker`
    /// too — its id, which the ticket's own `blocked_by` already shows, and
    /// nothing of it: exactly what an id naming no ticket gets.
    #[plexus_macros::method(
        group = "read",
        description = "One ticket's blockers resolved to current statuses; dangling ids marked",
        params(id = "Ticket id whose blockers to resolve, e.g. MNG-2")
    )]
    pub async fn blockers(
        &self, auth: Option<&AuthContext>, reach: Reach,
        id: TicketRef,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        blockers_stream(self.store.clone(), self.access.admit(auth, &reach), id.into_string())
    }

    /// MENGER-13: a ticket's history — every change of its status and its
    /// kind since the log began, as the store keeps them. Yields one
    /// `Transition` per transition of the ticket's, oldest first — `from` →
    /// `to`, when, who (`actor`: a principal, `system`, or `anonymous` with
    /// login off; `name`), from where (`source`: app, api, pipeline,
    /// cascade), why (`message`) and what caused it (`cause`, a transition
    /// id) — then one per transition those caused in other tickets (a
    /// cancel's cascade, a restore's), in the order written, then exactly one
    /// `HistoryEnd { began_unix, restorable }`: when the log began (nothing
    /// before it has a history), and for a canceled ticket the descendants
    /// `set_status { restore_children: true }` would put back. `NotFound` for
    /// an id that names no ticket (or one out of the caller's reach). From
    /// the cache; `watch` carries each new `Transition`.
    #[plexus_macros::method(
        group = "read",
        description = "A ticket's history: each status and kind transition (from, to, when, who, source, message, cause), oldest first, then those it caused in other tickets, then its queue records (MENGER-39), then history_end (when the log began; what a restore would put back; since when it has held its status)",
        params(id = "Ticket id, e.g. MENGER-7"),
        example(note = "who moved MENGER-7, and what its cancel canceled", params = r#"{"id": "MENGER-7"}"#)
    )]
    pub async fn history(&self, auth: Option<&AuthContext>, reach: Reach, id: TicketRef) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        history_stream(self.store.clone(), self.access.admit(auth, &reach), id.into_string())
    }

    /// MENGER-39: puts a ticket in front of the owner — at an urgency, for a
    /// reason; `blocking` names the input that answers it. Queued again: the
    /// urgency raised, never lowered. Yields one `queued`, or an `error`.
    #[plexus_macros::method(
        group = "write",
        target = "id",
        description = "Queue a ticket for the owner — urgency whenever | today | now | blocking (blocking names the input that answers it), and why; queued again it is raised, never lowered; yields queued { entry, ticket, action }",
        params(
            id = "Ticket id, e.g. MENGER-39",
            urgency = "whenever | today | now | blocking",
            reason = "Why the owner should look (required)",
            input = "The input on the ticket whose answer is asked (required for blocking); closing it unqueues the ticket",
            source = "app | api | pipeline (optional, default api)"
        ),
        values(urgency = ["whenever", "today", "now", "blocking"], source = ["app", "api", "pipeline"]),
        example(note = "an agent idle until a decision is answered", params = r#"{"id": "RETURNS-13", "urgency": "blocking", "reason": "the import waits on this", "input": "landing"}"#)
    )]
    pub async fn queue(
        &self, auth: Option<&AuthContext>, reach: Reach,
        id: TicketRef, urgency: Urgency, reason: Reason, input: Option<InputId>, source: Option<CallerSource>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        queue_stream(self.store.clone(), self.access.admit(auth, &reach), id.into_string(), Ask::Queue { urgency, reason, input }, source)
    }

    /// MENGER-39: takes a ticket out of the queue. Yields one `unqueued`,
    /// `not_queued` (nothing written), or an `error`.
    #[plexus_macros::method(
        group = "write",
        target = "id",
        description = "Take a ticket out of the owner's queue (done with it, or withdrawn); yields unqueued { id, entry, action, by, at_unix }, or not_queued",
        params(id = "Ticket id, e.g. MENGER-39", source = "app | api | pipeline (optional, default api)"),
        values(source = ["app", "api", "pipeline"])
    )]
    pub async fn unqueue(&self, auth: Option<&AuthContext>, reach: Reach, id: TicketRef, source: Option<CallerSource>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        queue_stream(self.store.clone(), self.access.admit(auth, &reach), id.into_string(), Ask::Unqueue, source)
    }

    /// MENGER-39: a queued ticket off the alerting surfaces for a while; it
    /// stays queued. Yields one `snoozed`, or an `error`.
    #[plexus_macros::method(
        group = "write",
        target = "id",
        description = "Snooze a queued ticket for some minutes (1 to a week): off the alerting surfaces, still queued; yields snoozed { entry, ticket }",
        params(id = "Ticket id, e.g. MENGER-39", minutes = "How long, in minutes", source = "app | api | pipeline (optional, default api)"),
        values(source = ["app", "api", "pipeline"])
    )]
    pub async fn snooze(&self, auth: Option<&AuthContext>, reach: Reach, id: TicketRef, minutes: u32, source: Option<CallerSource>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        queue_stream(self.store.clone(), self.access.admit(auth, &reach), id.into_string(), Ask::Snooze { minutes }, source)
    }

    /// APP-23: hands a queued ticket back — a comment (intent `changes`) on its conversation and the entry unqueued, one write.
    #[plexus_macros::method(
        group = "write",
        target = "id",
        description = "Request changes on a queued ticket: the body kept as a comment on its conversation (intent changes) and the ticket unqueued (action changes_requested), in one write; its input stays open; yields changes_requested { id, comment, entry, by, at_unix }",
        params(id = "The queued ticket, e.g. APP-21", body = "What to change (markdown)", source = "app | api | pipeline (optional, default api)"),
        values(source = ["app", "api", "pipeline"])
    )]
    pub async fn request_changes(&self, auth: Option<&AuthContext>, reach: Reach, id: TicketRef, body: Body, source: Option<CallerSource>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        request_changes_stream(self.store.clone(), self.access.admit(auth, &reach), id.into_string(), body.into_string(), source)
    }

    /// APP-24: after queueing, an agent waits here — `waiting`, then one `replied` (or `timed_out`), then the end.
    #[plexus_macros::method(
        group = "read",
        streaming,
        description = "Wait for the owner's response on a ticket: waiting { ticket, queued } at once, then one replied { ticket, reply } — an input answered, changes requested, a comment or an unqueue by another, the ticket canceled or deleted — and the end; timed_out after timeout_minutes",
        params(id = "The ticket, e.g. APP-21", since = "Responses from this unix time, so a reconnect loses nothing (optional; default now)", input = "Only this input's answer counts as answered (optional)", timeout_minutes = "Give up after this many minutes with timed_out (optional; default never)")
    )]
    pub async fn await_reply(&self, auth: Option<&AuthContext>, reach: Reach, id: TicketRef, since: Option<i64>, input: Option<InputId>, timeout_minutes: Option<u32>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let (id, input) = (id.into_string(), input.map(String::from));
        await_reply_stream(self.store.clone(), self.access.clone(), auth.cloned(), reach, await_reply::Ask { id, since, input, timeout_minutes })
    }

    /// MENGER-39: the owner's queue, most urgent first, then the oldest.
    #[plexus_macros::method(
        group = "read",
        description = "The owner's queue, most urgent first, then oldest: a queue_row { entry, ticket } each, then queue_end { count, snoozed, now_unix }",
        params(snoozed = "Include the snoozed ones (optional; default true — each marked by its snoozed_until_unix)")
    )]
    pub async fn queued(&self, auth: Option<&AuthContext>, reach: Reach, snoozed: Option<bool>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        queued_stream(self.store.clone(), self.access.admit(auth, &reach), snoozed.unwrap_or(true))
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
        group = "read",
        streaming,
        description = "Subscribe to ticket changes (Created / StatusChanged / KindChanged / Amended / CommitsLinked / CommitsUnlinked / LabelsChanged / UnlocksChanged / UnlocksRemoved / BlockersChanged / SeverityChanged / ChildrenChanged / ParentChanged / ChildRemoved / GroupChanged / PlacementsChanged / ComponentChanged / ComponentRemoved / InputChanged / UpdatedMany / Deleted / Undeleted / Reshortcoded), until disconnect; Lagged { missed } where a slow subscriber lost changes; with a filter, Entered / Left / Recount as each write's last events",
        params(
            epic = "Only changes in this epic (optional; a GroupChanged by its group's shortcode; an InputChanged by its ticket's; component changes always)",
            filter = "A filter, e.g. 'category in (active, review)' (optional): adds Entered / Left for each ticket a write moves into or out of it, or Recount past 256",
            membership = "Send only Entered, Left, Recount and Lagged (optional; default false)",
            tz = "The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC)"
        ),
        example(note = "follow active and in-review work", params = r#"{"filter": "category in (active, review)"}"#),
        example(note = "what entered Coding in the last hour — and what ages out of it", params = r#"{"filter": "entered Coding within 1h", "membership": true}"#)
    )]
    pub async fn watch(
        &self, auth: Option<&AuthContext>, reach: Reach,
        epic: Option<ShortcodeRef>,
        filter: Option<FilterText>,
        membership: Option<bool>,
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let membership = membership.unwrap_or(false);
        let (epic, ambiguous) = sight::watched_shortcode(&self.store, epic.as_deref());
        // IDENTITY-19, IDENTITY-21: who is watching, and through what reach.
        let access = self.access.clone();
        let who = auth.cloned();
        let asked = filter;
        let opened = access.admit(who.as_ref(), &reach).and_then(|caller| {
            if let Some(why) = &ambiguous {
                return Err(why.clone());
            }
            let filter = caller.scoped(asked.as_deref())?;
            // MENGER-13: how its filter reads time.
            let clock = clock_of(&caller, tz.as_deref())?;
            Ok((filter, Sight::of(&self.store, &caller)?, clock))
        });
        let (filter, sight, clock, refused) = match opened {
            Ok((filter, sight, clock)) => (filter, Some(sight), clock, None),
            Err(e) => (None, None, Clock::default(), Some(e)),
        };
        // FILTER-4: a filtered watch reads the filtered channel, which
        // carries every change and each write's cell moves after them.
        let filtered = (filter.is_some() || membership).then(|| {
            (self.store.subscribe_filtered(), watcher_for(&self.store, filter.as_deref(), epic.as_ref(), &clock))
        });
        let store = Arc::clone(&self.store);
        let plain = filtered.is_none().then(|| self.store.subscribe());
        // IDENTITY-19: admitted now, and asked again at every change it
        // relays, so a watch ends when its session does. IDENTITY-21: and a
        // restricted caller hears only of the tickets it can see.
        // IDENTITY-37: as its grants stand at that change — a grant made
        // or revoked moves what the watch relays (and a filtered watch's
        // filter) from the next change on, and a watch whose caller can
        // see nothing any more ends with an in-band `error`.
        stream! {
            if let Some(message) = refused {
                yield TicketsEvent::Error { message };
                return;
            }
            let Some(mut sight) = sight else { return };
            if let Some((mut rx, watcher)) = filtered {
                let mut watcher = match watcher {
                    Ok(w) => w,
                    Err(message) => {
                        yield TicketsEvent::Error { message };
                        return;
                    }
                };
                loop {
                    // MENGER-13: a filter with a relative window is read
                    // again as time passes, between writes.
                    let next = if watcher.relative() {
                        tokio::select! {
                            r = rx.recv() => Some(r),
                            _ = tokio::time::sleep(watcher.wait(&store, AGE_AT_MOST)) => None,
                        }
                    } else {
                        Some(rx.recv().await)
                    };
                    let Some(next) = next else {
                        for m in watcher.tick(&store, epic.as_ref()) {
                            yield membership_event(&store, m);
                        }
                        continue;
                    };
                    match sight.again(&access, who.as_ref(), &reach, &store) {
                        Err(message) => {
                            yield TicketsEvent::Error { message };
                            return;
                        }
                        Ok(None) => {}
                        Ok(Some((caller, gone))) => {
                            // The filter again, under the reach as it stands.
                            let rebuilt = caller.scoped(asked.as_deref()).and_then(|f| watcher_for(&store, f.as_deref(), epic.as_ref(), &clock));
                            match rebuilt {
                                Ok(w) => watcher = w,
                                Err(message) => {
                                    yield TicketsEvent::Error { message };
                                    return;
                                }
                            }
                            if !membership {
                                for name in gone {
                                    yield TicketsEvent::ComponentRemoved { name };
                                }
                            }
                        }
                    }
                    match next {
                        Ok(Change::CellsMoved { moves }) => {
                            for m in watcher.membership(&store, &moves, epic.as_ref()) {
                                yield membership_event(&store, m);
                            }
                        }
                        // FILTER-13: a filter asking `under` — what the write
                        // worked out for this watch.
                        Ok(Change::UnderMoved { moves }) => {
                            for m in watcher.under_membership(&moves) {
                                yield membership_event(&store, m);
                            }
                        }
                        Ok(change) if !membership => {
                            if let Some(ev) = change.in_epic(epic.as_ref()).and_then(wire_event) {
                                if let Some(ev) = sight.relay(&store, ev) {
                                    yield ev;
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                            yield TicketsEvent::Lagged { missed };
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                    // FILTER-36: a saved filter it names, changed — said
                    // after the change that changed it.
                    match watcher.resaved(&store, epic.as_ref()) {
                        Ok(moved) => for m in moved { yield membership_event(&store, m); },
                        Err(message) => { yield TicketsEvent::Error { message }; return; }
                    }
                }
                return;
            }
            let Some(mut rx) = plain else { return };
            loop {
                let next = rx.recv().await;
                match sight.again(&access, who.as_ref(), &reach, &store) {
                    Err(message) => {
                        yield TicketsEvent::Error { message };
                        return;
                    }
                    Ok(None) => {}
                    Ok(Some((_, gone))) => {
                        for name in gone {
                            yield TicketsEvent::ComponentRemoved { name };
                        }
                    }
                }
                match next {
                    Ok(change) => {
                        // WAVE-6: of a `placements_changed`, the changes in
                        // the epic.
                        if let Some(ev) = change.in_epic(epic.as_ref()).and_then(wire_event) {
                            if let Some(ev) = sight.relay(&store, ev) {
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
