//! The typed Plexus client for menger's `tickets` activation.
//!
//! This is the **declaration half** of the activation: the same method
//! names, the same parameters in the same order, the same event type —
//! and no implementation. `server_cfg = "feature = \"server\""` names a
//! feature this crate declares and never enables, so the server half of
//! the expansion (and the `unreachable!()` bodies below) is never
//! compiled. What is compiled is `MengerTicketsClient<T>`.
//!
//! ```text
//!   menger/src/activations/tickets/activation.rs   the server
//!            │  same names · same params · same TicketsEvent
//!            ▼
//!   this file  ──#[activation(client)]──►  MengerTicketsClient<T>
//!                                              │
//!                                    WsTransport (ws-web)
//!                                              │
//!                                   frontend/src/session.rs
//! ```
//!
//! # Why the declarations are here and not on the server's own `impl`
//!
//! One `#[activation(client, …)]` on menger's real `impl Tickets` would
//! make this file unnecessary — one definition, no chance of drift. That
//! needs `plexus-macros` 0.6 in menger's *server* graph, and 0.6's
//! expansion calls `MethodSchema::with_public`, which lands in
//! `plexus-core` 0.6; `plexus-core` 0.6 needs a `plexus-transport` pinned
//! to it, and the only such build is unreleased. MFE-9's probe measured
//! the whole of that drift as **one missing method** and 80/80 menger
//! tests green once it is bridged — so the collapse is mechanical, and it
//! is the promoted MFE-11 build, not a rewrite.
//!
//! Until then the split is: **types are shared, signatures are declared
//! twice.** A renamed *field* is a compile error in the browser (that is
//! what MFE-9 criterion 4 asks for, and it holds). A renamed *parameter*
//! is not — it is a runtime in-band error from the server, and
//! `frontend`'s browser regressions are what catch it.
//!
//! Anything added to the server activation belongs here in the same
//! commit. The order of parameters is the wire order; the macro emits
//! them positionally.

use crate::types::TicketsEvent;

/// The activation being declared. Never constructed: the server half is
/// behind a `cfg` that is never satisfied, and menger's real `Tickets`
/// lives in the server crate.
#[cfg(feature = "server")]
#[derive(Debug, Clone, Default)]
pub struct MengerTickets;

#[plexus_macros::activation(
    namespace = "tickets",
    description = "Plans as data — create, query, and watch the work DAG",
    client,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core"
)]
impl MengerTickets {
    /// Heartbeat. Returns the binary version, the server's unix time, and
    /// how many tickets the store holds.
    #[plexus_macros::method(description = "Heartbeat — version, unix time, ticket count")]
    pub async fn health(&self) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        unreachable!("declaration only — the server is menger's own activation")
    }

    /// Creates a ticket in an epic. Yields exactly one event: `Created` or
    /// `Error`.
    #[plexus_macros::method(
        description = "Create a ticket; allocates the next EPIC-N id, status starts Pending",
        params(
            epic = "Epic prefix, e.g. MNG (case-insensitive, stored uppercase)",
            title = "Short description",
            kind = "scope | execution | build | gap | spike (default: build)",
            severity = "Critical | High | Medium | Low (optional)",
            blocked_by = "Ticket ids that must complete first (optional)",
            body = "Markdown body (optional)",
            parent = "Id of the ticket this one is an output of; any epic, must exist (optional)"
        )
    )]
    pub async fn create(
        &self,
        epic: String,
        title: String,
        kind: Option<String>,
        severity: Option<String>,
        blocked_by: Option<Vec<String>>,
        body: Option<String>,
        parent: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        unreachable!("declaration only — the server is menger's own activation")
    }

    /// Yields exactly one event: `Ticket` if found, `NotFound` otherwise.
    /// This is the only call that carries a body.
    #[plexus_macros::method(
        description = "Get one ticket by id",
        params(id = "Ticket id, e.g. MNG-2")
    )]
    pub async fn get(&self, id: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        unreachable!("declaration only — the server is menger's own activation")
    }

    /// One row event per match in (epic, seq) order, then exactly one
    /// `Page` event saying where the walk stands (MNG-9).
    #[plexus_macros::method(
        description = "List tickets, optionally filtered by epic, status and/or parent; paged with after + limit",
        params(
            epic = "Epic prefix filter (optional)",
            status = "Status filter: Pending | Ready | Blocked | Complete | Idea | Epic | Superseded (optional)",
            parent = "Only this ticket's children, e.g. PERF-3 (optional)",
            after = "Cursor: start strictly after this ticket id, e.g. MNG-9 (optional)",
            limit = "At most this many tickets; the page event carries the next cursor (optional)",
            summary = "Yield summary rows (every field but the body) instead of full tickets (optional)"
        )
    )]
    pub async fn list(
        &self,
        epic: Option<String>,
        status: Option<String>,
        parent: Option<String>,
        after: Option<String>,
        limit: Option<u32>,
        summary: Option<bool>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        unreachable!("declaration only — the server is menger's own activation")
    }

    /// Yields exactly one event: `StatusChanged`, `NotFound`, or `Error`.
    #[plexus_macros::method(
        description = "Set a ticket's status",
        params(
            id = "Ticket id, e.g. MNG-2",
            status = "New status: Pending | Ready | Blocked | Complete | Idea | Epic | Superseded"
        )
    )]
    pub async fn set_status(
        &self,
        id: String,
        status: String,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        unreachable!("declaration only — the server is menger's own activation")
    }

    /// The DAG query: which tickets can start now? Paged exactly as
    /// `list` (MNG-9).
    #[plexus_macros::method(
        description = "Tickets that can start now: Ready, with every blocker Complete or Superseded",
        params(
            epic = "Only tickets in this epic (optional; blockers still resolve globally)",
            after = "Cursor: start strictly after this ticket id, e.g. MNG-9 (optional)",
            limit = "At most this many tickets; the page event carries the next cursor (optional)",
            summary = "Yield summary rows (every field but the body) instead of full tickets (optional)"
        )
    )]
    pub async fn ready_set(
        &self,
        epic: Option<String>,
        after: Option<String>,
        limit: Option<u32>,
        summary: Option<bool>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        unreachable!("declaration only — the server is menger's own activation")
    }

    /// "Why can't this start?" — one `Blocker` per resolved blocker and
    /// one `DanglingBlocker` per id that names no ticket.
    #[plexus_macros::method(
        description = "One ticket's blockers resolved to current statuses; dangling ids marked",
        params(id = "Ticket id whose blockers to resolve, e.g. MNG-2")
    )]
    pub async fn blockers(
        &self,
        id: String,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        unreachable!("declaration only — the server is menger's own activation")
    }

    /// The reactive primitive: every mutation, until the client
    /// disconnects.
    #[plexus_macros::method(
        streaming,
        description = "Subscribe to ticket changes (Created / StatusChanged / UnlocksChanged / ChildrenChanged), until disconnect",
        params(epic = "Only changes in this epic (optional)")
    )]
    pub async fn watch(
        &self,
        epic: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        unreachable!("declaration only — the server is menger's own activation")
    }
}
