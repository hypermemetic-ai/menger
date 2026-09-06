//! Plexus RPC surface for tickets.
//!
//! `create` names one optional parameter per ticket field (AGS-19 added
//! `parent`) — that is the wire surface talking, here and in the wrapper
//! the activation macro generates, not a call signature asking to become
//! a struct.
#![allow(clippy::too_many_arguments)]

use super::types::TicketsEvent;
use crate::store::{
    BlockerRef, Change, Kind, NewTicket, Page, PageQuery, Severity, Status, Store, StoreError,
};
use async_stream::stream;
use futures::Stream;
use std::sync::Arc;

#[derive(Clone)]
pub struct Tickets {
    store: Arc<Store>,
}

impl Tickets {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }
}

/// MNG-9: one page of the store, as the events that carry it — a row per
/// ticket (`summary` decides which shape) and then exactly one `page`
/// event, which is what tells a client whether to ask for more. A failed
/// cursor is the one in-band `error` instead, with no page event: nothing
/// was walked, so there is nowhere to resume from.
fn page_events(
    page: Result<Page, StoreError>,
    summary: Option<bool>,
) -> Box<dyn Iterator<Item = TicketsEvent> + Send> {
    let page = match page {
        Ok(page) => page,
        Err(e) => {
            return Box::new(std::iter::once(TicketsEvent::Error {
                message: e.to_string(),
            }))
        }
    };
    let rows: Box<dyn Iterator<Item = TicketsEvent> + Send> = if summary.unwrap_or(false) {
        Box::new(page.tickets.into_iter().map(|ticket| TicketsEvent::Summary {
            summary: ticket.into(),
        }))
    } else {
        Box::new(
            page.tickets
                .into_iter()
                .map(|ticket| TicketsEvent::Ticket { ticket }),
        )
    };
    Box::new(rows.chain(std::iter::once(TicketsEvent::Page {
        next: page.next,
        remaining: page.remaining,
    })))
}

fn change_event(change: Change) -> TicketsEvent {
    match change {
        Change::Created { ticket } => TicketsEvent::Created { ticket },
        Change::StatusChanged { ticket, previous } => {
            TicketsEvent::StatusChanged { ticket, previous }
        }
        Change::UnlocksChanged { ticket, added } => TicketsEvent::UnlocksChanged { ticket, added },
        Change::ChildrenChanged { ticket, added } => TicketsEvent::ChildrenChanged { ticket, added },
        Change::ParentChanged { ticket, previous } => {
            TicketsEvent::ParentChanged { ticket, previous }
        }
        Change::ChildRemoved { ticket, removed } => TicketsEvent::ChildRemoved { ticket, removed },
    }
}

#[plexus_macros::activation(
    namespace = "tickets",
    description = "Plans as data — create, query, and watch the work DAG",
    crate_path = "plexus_core"
)]
impl Tickets {
    /// Heartbeat. Returns the binary version, the server's unix time, and
    /// how many tickets the store holds.
    #[plexus_macros::method(description = "Heartbeat — version, unix time, ticket count")]
    pub async fn health(&self) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        stream! {
            yield TicketsEvent::Health {
                version: env!("CARGO_PKG_VERSION").to_string(),
                unix_now: chrono::Utc::now().timestamp(),
                ticket_count: store.count() as u64,
            };
        }
    }

    /// Creates a ticket in an epic. The store allocates the next `EPIC-N`
    /// id; status starts at `Pending` (only a human promotes to `Ready`);
    /// `kind` defaults to `build`, and one outside the five yields the
    /// in-band error naming them. `parent` (AGS-19) files the ticket as an
    /// output of an existing ticket in any epic; one that names no ticket,
    /// or that would make the newborn its own ancestor, is an in-band error
    /// naming the reason. MNG-14: omitted, `parent` defaults to the
    /// shortcode's scope root; given, it must be inside the same shortcode
    /// — except for the scope root itself, whose parent is the shortcode
    /// it hangs under. Every shortcode starts with a scope (MNG-12):
    /// the first ticket of a prefix must be `kind: scope`, and anything
    /// else is an in-band error naming the rule, with nothing allocated.
    /// Yields exactly one event: `Created` or `Error`.
    #[plexus_macros::method(
        description = "Create a ticket; allocates the next EPIC-N id, status starts Pending; the first ticket of a prefix must be a scope",
        params(
            epic = "Epic prefix, e.g. MNG (case-insensitive, stored uppercase)",
            title = "Short description",
            kind = "scope | execution | build | gap | spike (default: build; the first ticket of a prefix must be scope)",
            severity = "Critical | High | Medium | Low (optional)",
            blocked_by = "Ticket ids that must complete first (optional)",
            body = "Markdown body (optional)",
            parent = "Id of the ticket this one is an output of; must exist and be in the same shortcode (default: the shortcode's scope root). The shortcode's own scope root may name a ticket in another shortcode: the one it hangs under"
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
        let store = self.store.clone();
        stream! {
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
                body: body.unwrap_or_default(),
            }).await {
                Ok(ticket) => yield TicketsEvent::Created { ticket },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// Yields exactly one event: `Ticket` if found, `NotFound` otherwise.
    #[plexus_macros::method(
        description = "Get one ticket by id",
        params(id = "Ticket id, e.g. MNG-2")
    )]
    pub async fn get(&self, id: String) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        stream! {
            match store.get(&id) {
                Some(ticket) => yield TicketsEvent::Ticket { ticket },
                None => yield TicketsEvent::NotFound { id },
            }
        }
    }

    /// Yields one row event per match, ordered by (epic, seq), then
    /// exactly one `page` event saying where the walk stands. The
    /// `parent` filter (AGS-19) yields exactly that ticket's children.
    ///
    /// MNG-9 adds the page: `after` is the exclusive cursor (a ticket id
    /// — the order is total and unique, so an id is a position), `limit`
    /// caps the page, and `summary` swaps the `ticket` rows for `summary`
    /// rows that carry every field but the body. Without them the call is
    /// what it always was — every match, with bodies — followed by
    /// `page { next: null, remaining: 0 }`. An `after` naming no ticket
    /// is one in-band `error` and nothing else.
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
        let store = self.store.clone();
        stream! {
            let status = match status.as_deref().map(str::parse::<Status>) {
                None => None,
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    yield TicketsEvent::Error { message: e };
                    return;
                }
            };
            let page = store.page(PageQuery {
                epic: epic.as_deref(),
                status,
                parent: parent.as_deref(),
                after: after.as_deref(),
                limit,
            });
            for ev in page_events(page, summary) {
                yield ev;
            }
        }
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
        let store = self.store.clone();
        stream! {
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

    /// MNG-12: files an existing ticket as an output of `parent` — the
    /// verb AGS-19 left out. Adopts a ticket born parentless, or moves
    /// one. Yields exactly one event: `ParentChanged` (the child, with the
    /// parent it had before), `NotFound` if `id` names no ticket, or an
    /// in-band `Error` when the parent does not exist or the chain would
    /// make the ticket its own ancestor. `watch` also carries the parents'
    /// side: `ChildrenChanged` for the new parent and `ChildRemoved` for
    /// the old one.
    #[plexus_macros::method(
        description = "Set a ticket's parent — the ticket it is an output of; adopts a parentless ticket or moves one",
        params(
            id = "Ticket id to file, e.g. AGS-2",
            parent = "Id of the ticket to file it under; any epic, must exist"
        )
    )]
    pub async fn set_parent(
        &self,
        id: String,
        parent: String,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        stream! {
            match store.set_parent(&id, &parent).await {
                Ok(change) => yield change_event(change),
                Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
                Err(e) => yield TicketsEvent::Error { message: e.to_string() },
            }
        }
    }

    /// The DAG query: which tickets can start now? Yields one row event
    /// per member of the ready set — status `Ready` and every
    /// `blocked_by` entry `Complete` or `Superseded` — ordered by
    /// (epic, seq), then one `page` event. A dangling blocker keeps its
    /// ticket out (use `blockers` to see which ids dangle). The epic
    /// filter applies to membership; blockers resolve globally.
    /// `after` / `limit` / `summary` page it exactly as `list` (MNG-9).
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
        let store = self.store.clone();
        stream! {
            let page = store.ready_page(epic.as_deref(), after.as_deref(), limit);
            for ev in page_events(page, summary) {
                yield ev;
            }
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
        &self,
        id: String,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        stream! {
            match store.blockers(&id) {
                Ok(refs) => {
                    for r in refs {
                        yield match r {
                            BlockerRef::Resolved(ticket) => TicketsEvent::Blocker { ticket },
                            BlockerRef::Dangling(id) => TicketsEvent::DanglingBlocker { id },
                        };
                    }
                }
                Err(_) => yield TicketsEvent::NotFound { id },
            }
        }
    }

    /// The reactive primitive: an open-ended stream of every mutation, as
    /// `Created` / `StatusChanged` / `UnlocksChanged` / `ChildrenChanged`
    /// events. Runs until the client disconnects. Every event carries the
    /// full affected ticket, so a client that listed once and then only
    /// watches can compute the ready set — and the parent tree — itself. A slow consumer that
    /// lags the broadcast buffer skips the missed changes and keeps
    /// receiving from the current position.
    #[plexus_macros::method(
        streaming,
        description = "Subscribe to ticket changes (Created / StatusChanged / UnlocksChanged / ChildrenChanged / ParentChanged / ChildRemoved), until disconnect",
        params(epic = "Only changes in this epic (optional)")
    )]
    pub async fn watch(
        &self,
        epic: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let mut rx = self.store.subscribe();
        let epic = epic.map(|e| e.to_ascii_uppercase());
        stream! {
            loop {
                match rx.recv().await {
                    Ok(change) => {
                        if epic.as_deref().is_none_or(|e| change.ticket().epic == e) {
                            yield change_event(change);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

#[cfg(test)]
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
        match one(hub.health().await).await {
            TicketsEvent::Health { ticket_count, .. } => assert_eq!(ticket_count, 0),
            other => panic!("expected Health, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_allocates_sequential_ids() {
        let hub = hub();
        let ev = one(hub.create("mng".into(), "first".into(), Some("scope".into()), None, None, None, None).await).await;
        match ev {
            TicketsEvent::Created { ticket } => {
                assert_eq!(ticket.id, "MNG-1");
                assert_eq!(ticket.status, Status::Pending);
            }
            other => panic!("expected Created, got {other:?}"),
        }
        let ev = one(hub.create("MNG".into(), "second".into(), None, None, None, None, None).await).await;
        match ev {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "MNG-2"),
            other => panic!("expected Created, got {other:?}"),
        }
    }

    /// AGS-13 criterion 2: no kind → `build`; `gap` is accepted; a kind
    /// outside the five (`bug` — rim's name for gap) is the in-band error
    /// that names all five, and nothing is created.
    #[tokio::test]
    async fn create_defaults_kind_to_build_accepts_gap_and_rejects_unknown_naming_the_five() {
        let hub = hub();
        // MNG-12: the prefix's scope root first; the default is measured
        // on the ticket after it.
        one(hub.create("MNG".into(), "root".into(), Some("scope".into()), None, None, None, None).await).await;
        match one(hub.create("MNG".into(), "no kind".into(), None, None, None, None, None).await).await {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.kind, Kind::Build),
            other => panic!("expected Created, got {other:?}"),
        }
        match one(hub.create("MNG".into(), "a gap".into(), Some("gap".into()), None, None, None, None).await)
            .await
        {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.kind, Kind::Gap),
            other => panic!("expected Created, got {other:?}"),
        }
        let ev = one(
            hub.create("MNG".into(), "x".into(), Some("bug".into()), None, None, None, None)
                .await,
        )
        .await;
        match ev {
            TicketsEvent::Error { message } => {
                assert!(message.contains("'bug'"), "{message}");
                for name in ["scope", "execution", "build", "gap", "spike"] {
                    assert!(message.contains(name), "names {name}: {message}");
                }
            }
            other => panic!("expected Error, got {other:?}"),
        }
        assert!(matches!(
            one(hub.get("MNG-4".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn get_distinguishes_found_from_missing() {
        let hub = hub();
        one(hub.create("MNG".into(), "x".into(), Some("scope".into()), None, None, None, None).await).await;
        assert!(matches!(
            one(hub.get("MNG-1".into()).await).await,
            TicketsEvent::Ticket { .. }
        ));
        assert!(matches!(
            one(hub.get("MNG-99".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn list_filters_by_status() {
        let hub = hub();
        one(hub.create("MNG".into(), "a".into(), Some("scope".into()), None, None, None, None).await).await;
        one(hub.create("MNG".into(), "b".into(), None, None, None, None, None).await).await;
        one(hub.set_status("MNG-1".into(), "Ready".into()).await).await;

        let (ids, page) =
            ids_and_page(hub.list(None, Some("Ready".into()), None, None, None, None).await).await;
        assert_eq!(ids, vec!["MNG-1"]);
        assert_eq!(page, Some((None, 0)), "one match, nothing beyond it");
    }

    #[tokio::test]
    async fn set_status_reports_previous() {
        let hub = hub();
        one(hub.create("MNG".into(), "a".into(), Some("scope".into()), None, None, None, None).await).await;
        match one(hub.set_status("MNG-1".into(), "ready".into()).await).await {
            TicketsEvent::StatusChanged { ticket, previous } => {
                assert_eq!(ticket.status, Status::Ready);
                assert_eq!(previous, Status::Pending);
            }
            other => panic!("expected StatusChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn watch_sees_creations_and_status_changes() {
        let hub = hub();
        let mut watch = Box::pin(hub.watch(None).await);

        one(hub.create("MNG".into(), "a".into(), Some("scope".into()), None, None, None, None).await).await;
        match watch.next().await.expect("change") {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "MNG-1"),
            other => panic!("expected Created, got {other:?}"),
        }

        one(hub.set_status("MNG-1".into(), "Ready".into()).await).await;
        match watch.next().await.expect("change") {
            TicketsEvent::StatusChanged { previous, .. } => assert_eq!(previous, Status::Pending),
            other => panic!("expected StatusChanged, got {other:?}"),
        }
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
                TicketsEvent::Page { next, remaining } => page = Some((next, remaining)),
                other => panic!("expected a row or the page, got {other:?}"),
            }
        }
        (ids, page)
    }

    #[tokio::test]
    async fn ready_set_gates_on_blockers() {
        let hub = hub();
        one(hub.create("DAG".into(), "a".into(), Some("scope".into()), None, None, None, None).await).await;
        one(hub
            .create("DAG".into(), "b".into(), None, None, Some(vec!["DAG-1".into()]), None, None)
            .await)
        .await;
        one(hub.set_status("DAG-1".into(), "Ready".into()).await).await;
        one(hub.set_status("DAG-2".into(), "Ready".into()).await).await;

        assert_eq!(ticket_ids(hub.ready_set(None, None, None, None).await).await, vec!["DAG-1"]);

        one(hub.set_status("DAG-1".into(), "Complete".into()).await).await;
        assert_eq!(ticket_ids(hub.ready_set(None, None, None, None).await).await, vec!["DAG-2"]);
    }

    #[tokio::test]
    async fn ready_set_accepts_superseded_blockers_and_filters_by_epic() {
        let hub = hub();
        one(hub.create("ONE".into(), "a".into(), Some("scope".into()), None, None, None, None).await).await;
        one(hub
            .create("ONE".into(), "b".into(), None, None, Some(vec!["ONE-1".into()]), None, None)
            .await)
        .await;
        one(hub.create("TWO".into(), "c".into(), Some("scope".into()), None, None, None, None).await).await;
        one(hub.set_status("ONE-1".into(), "Superseded".into()).await).await;
        one(hub.set_status("ONE-2".into(), "Ready".into()).await).await;
        one(hub.set_status("TWO-1".into(), "Ready".into()).await).await;

        assert_eq!(
            ticket_ids(hub.ready_set(None, None, None, None).await).await,
            vec!["ONE-2", "TWO-1"]
        );
        assert_eq!(
            ticket_ids(hub.ready_set(Some("one".into()), None, None, None).await).await,
            vec!["ONE-2"]
        );
    }

    #[tokio::test]
    async fn dangling_blocker_excludes_from_ready_set_and_is_marked() {
        let hub = hub();
        one(hub
            .create("DAG".into(), "c".into(), Some("scope".into()), None, Some(vec!["DAG-99".into()]), None, None)
            .await)
        .await;
        one(hub.set_status("DAG-1".into(), "Ready".into()).await).await;

        assert!(ticket_ids(hub.ready_set(None, None, None, None).await).await.is_empty());

        let evs: Vec<_> = Box::pin(hub.blockers("DAG-1".into()).await).collect().await;
        assert_eq!(evs.len(), 1);
        match &evs[0] {
            TicketsEvent::DanglingBlocker { id } => assert_eq!(id, "DAG-99"),
            other => panic!("expected DanglingBlocker, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn blockers_resolve_to_current_statuses_in_order() {
        let hub = hub();
        one(hub.create("DAG".into(), "a".into(), Some("scope".into()), None, None, None, None).await).await;
        one(hub.create("DAG".into(), "b".into(), None, None, None, None, None).await).await;
        one(hub
            .create(
                "DAG".into(),
                "c".into(),
                None,
                None,
                Some(vec!["DAG-1".into(), "DAG-2".into(), "DAG-99".into()]),
                None, None,
            )
            .await)
        .await;
        one(hub.set_status("DAG-1".into(), "Complete".into()).await).await;

        let evs: Vec<_> = Box::pin(hub.blockers("DAG-3".into()).await).collect().await;
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
            one(hub.blockers("DAG-42".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn create_adds_new_id_to_blockers_unlocks_and_watch_carries_it() {
        let hub = hub();
        one(hub.create("DAG".into(), "a".into(), Some("scope".into()), None, None, None, None).await).await;
        let mut watch = Box::pin(hub.watch(None).await);

        one(hub
            .create("DAG".into(), "b".into(), None, None, Some(vec!["DAG-1".into()]), None, None)
            .await)
        .await;

        match watch.next().await.expect("change") {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "DAG-2"),
            other => panic!("expected Created, got {other:?}"),
        }
        // MNG-14: DAG-2 named no parent, so it hangs under the root — which
        // is also its blocker: one updated ticket, both edges, two events.
        match watch.next().await.expect("change") {
            TicketsEvent::ChildrenChanged { ticket, added } => {
                assert_eq!(ticket.id, "DAG-1");
                assert_eq!(ticket.children, vec!["DAG-2"]);
                assert_eq!(ticket.unlocks, vec!["DAG-2"], "both edges on the one ticket");
                assert_eq!(added, "DAG-2");
            }
            other => panic!("expected ChildrenChanged, got {other:?}"),
        }
        match watch.next().await.expect("change") {
            TicketsEvent::UnlocksChanged { ticket, added } => {
                assert_eq!(ticket.id, "DAG-1");
                assert_eq!(ticket.unlocks, vec!["DAG-2"]);
                assert_eq!(added, "DAG-2");
            }
            other => panic!("expected UnlocksChanged, got {other:?}"),
        }

        match one(hub.get("DAG-1".into()).await).await {
            TicketsEvent::Ticket { ticket } => assert_eq!(ticket.unlocks, vec!["DAG-2"]),
            other => panic!("expected Ticket, got {other:?}"),
        }
    }

    /// AGS-19 acceptance 1: a child's birth is visible from both ends —
    /// `get C` shows the parent, `get P` shows the child — and a watcher
    /// sees the parent's update as `children_changed` with the full
    /// ticket, the same discipline `unlocks_changed` follows.
    #[tokio::test]
    async fn create_with_parent_updates_children_and_watch_carries_the_parent() {
        let hub = hub();
        one(hub.create("FAM".into(), "P".into(), Some("scope".into()), None, None, None, None).await).await;
        let mut watch = Box::pin(hub.watch(None).await);

        match one(
            hub.create(
                "FAM".into(),
                "C".into(),
                Some("execution".into()),
                None,
                None,
                None,
                Some("fam-1".into()),
            )
            .await,
        )
        .await
        {
            TicketsEvent::Created { ticket } => {
                assert_eq!(ticket.id, "FAM-2");
                assert_eq!(ticket.parent.as_deref(), Some("FAM-1"), "case-insensitive");
                assert!(ticket.children.is_empty());
            }
            other => panic!("expected Created, got {other:?}"),
        }

        match watch.next().await.expect("change") {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "FAM-2"),
            other => panic!("expected Created, got {other:?}"),
        }
        match watch.next().await.expect("change") {
            TicketsEvent::ChildrenChanged { ticket, added } => {
                assert_eq!(ticket.id, "FAM-1");
                assert_eq!(added, "FAM-2");
                assert_eq!(ticket.children, vec!["FAM-2"]);
                assert_eq!(ticket.title, "P", "the full parent rides along");
            }
            other => panic!("expected ChildrenChanged, got {other:?}"),
        }

        match one(hub.get("FAM-1".into()).await).await {
            TicketsEvent::Ticket { ticket } => {
                assert_eq!(ticket.children, vec!["FAM-2"]);
                assert_eq!(ticket.parent, None);
            }
            other => panic!("expected Ticket, got {other:?}"),
        }
        match one(hub.get("FAM-2".into()).await).await {
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
            hub.create(
                "ORPH".into(),
                "no such parent".into(),
                Some("scope".into()),
                None,
                None,
                None,
                Some("ORPH-9".into()),
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
            one(hub.get("ORPH-1".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    /// AGS-19 acceptance 4: `list {parent: P}` yields exactly P's
    /// children, in (epic, seq) order.
    #[tokio::test]
    async fn list_filters_by_parent() {
        let hub = hub();
        one(hub.create("LP".into(), "P".into(), Some("scope".into()), None, None, None, None).await).await;
        one(hub.create("LP".into(), "unrelated".into(), None, None, None, None, None).await).await;
        // ZZ's first ticket is its scope root (MNG-12), filed under LP-1.
        for (epic, title, kind) in [("LP", "child a", None), ("ZZ", "child b", Some("scope"))] {
            one(hub
                .create(
                    epic.into(),
                    title.into(),
                    kind.map(String::from),
                    None,
                    None,
                    None,
                    Some("LP-1".into()),
                )
                .await)
            .await;
        }

        // MNG-14: "unrelated" (LP-2) named no parent and hangs under the root.
        assert_eq!(
            ticket_ids(hub.list(None, None, Some("LP-1".into()), None, None, None).await).await,
            vec!["LP-2", "LP-3", "ZZ-1"]
        );
        assert!(ticket_ids(hub.list(None, None, Some("LP-2".into()), None, None, None).await)
            .await
            .is_empty());
        assert_eq!(
            ticket_ids(hub.list(None, None, None, None, None, None).await).await.len(),
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
            .create(
                "SUM".into(),
                "with a body".into(),
                Some("scope".into()), // MNG-12: a prefix's first ticket is its scope
                Some("High".into()),
                None,
                Some("## Problem\n\nthe weight of a list".into()),
                None,
            )
            .await)
        .await;

        let evs: Vec<_> = Box::pin(hub.list(None, None, None, None, None, Some(true)).await)
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
        assert_eq!(summary["children"], serde_json::json!([]));

        // The full ticket still carries it, unchanged.
        let evs: Vec<_> = Box::pin(hub.list(None, None, None, None, None, None).await)
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
        one(hub.create("CUR".into(), "a".into(), None, None, None, None, None).await).await;

        for evs in [
            Box::pin(hub.list(None, None, None, Some("ZZZ-9".into()), None, None).await)
                .collect::<Vec<_>>()
                .await,
            Box::pin(hub.ready_set(None, Some("ZZZ-9".into()), None, None).await)
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
            one(hub.create("RS".into(), "r".into(), kind, None, None, None, None).await).await;
        }
        one(hub.create("RS".into(), "still pending".into(), None, None, None, None, None).await).await;
        for id in ["RS-1", "RS-2", "RS-3"] {
            one(hub.set_status(id.into(), "Ready".into()).await).await;
        }

        let (ids, page) = ids_and_page(hub.ready_set(None, None, Some(2), Some(true)).await).await;
        assert_eq!(ids, vec!["RS-1", "RS-2"]);
        assert_eq!(
            page,
            Some((Some("RS-2".into()), 1)),
            "one ready ticket left, not two store rows"
        );

        let (ids, page) =
            ids_and_page(hub.ready_set(None, Some("RS-2".into()), Some(2), Some(true)).await).await;
        assert_eq!(ids, vec!["RS-3"]);
        assert_eq!(page, Some((None, 0)));
    }

    #[tokio::test]
    async fn dangling_forward_reference_resolves_at_birth() {
        let hub = hub();
        one(hub
            .create("DAG".into(), "d1".into(), Some("scope".into()), None, Some(vec!["DAG-2".into()]), None, None)
            .await)
        .await;
        // The ticket that becomes DAG-2 has DAG-1 in unlocks from birth.
        match one(hub.create("DAG".into(), "d2".into(), None, None, None, None, None).await).await {
            TicketsEvent::Created { ticket } => {
                assert_eq!(ticket.id, "DAG-2");
                assert_eq!(ticket.unlocks, vec!["DAG-1"]);
            }
            other => panic!("expected Created, got {other:?}"),
        }
        match one(hub.get("DAG-2".into()).await).await {
            TicketsEvent::Ticket { ticket } => assert_eq!(ticket.unlocks, vec!["DAG-1"]),
            other => panic!("expected Ticket, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_rejects_self_reference() {
        let hub = hub();
        // First create in the epic allocates DAG-1; naming it is a self-cycle.
        let ev = one(hub
            .create("DAG".into(), "me".into(), Some("scope".into()), None, Some(vec!["DAG-1".into()]), None, None)
            .await)
        .await;
        match ev {
            TicketsEvent::Error { message } => assert!(message.contains("cycle"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        assert!(matches!(
            one(hub.get("DAG-1".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn create_rejects_cycle_through_dangling_forward_reference() {
        let hub = hub();
        // DAG-1 blocked_by the not-yet-existing DAG-2.
        one(hub
            .create("DAG".into(), "d1".into(), Some("scope".into()), None, Some(vec!["DAG-2".into()]), None, None)
            .await)
        .await;
        // The create that would become DAG-2, blocked_by DAG-1, closes the loop.
        let ev = one(hub
            .create("DAG".into(), "d2".into(), None, None, Some(vec!["DAG-1".into()]), None, None)
            .await)
        .await;
        match ev {
            TicketsEvent::Error { message } => assert!(message.contains("cycle"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        // Nothing was stored.
        assert!(matches!(
            one(hub.get("DAG-2".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn create_rejects_longer_cycles() {
        let hub = hub();
        one(hub
            .create("DAG".into(), "p1".into(), Some("scope".into()), None, Some(vec!["DAG-3".into()]), None, None)
            .await)
        .await;
        one(hub
            .create("DAG".into(), "p2".into(), None, None, Some(vec!["DAG-1".into()]), None, None)
            .await)
        .await;
        // Would become DAG-3: DAG-3 -> DAG-2 -> DAG-1 -> DAG-3.
        let ev = one(hub
            .create("DAG".into(), "p3".into(), None, None, Some(vec!["DAG-2".into()]), None, None)
            .await)
        .await;
        assert!(matches!(ev, TicketsEvent::Error { .. }), "got {ev:?}");
        assert!(matches!(
            one(hub.get("DAG-3".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn watch_epic_filter_drops_other_epics() {
        let hub = hub();
        let mut watch = Box::pin(hub.watch(Some("mng".into())).await);

        one(hub.create("OTHER".into(), "x".into(), Some("scope".into()), None, None, None, None).await).await;
        one(hub.create("MNG".into(), "y".into(), Some("scope".into()), None, None, None, None).await).await;

        match watch.next().await.expect("change") {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.epic, "MNG"),
            other => panic!("expected Created, got {other:?}"),
        }
    }
    /// MNG-12/14: `set_parent` over the activation — `ParentChanged` on a
    /// move, `NotFound` for a missing child, in-band errors for a missing
    /// parent, a parent outside the shortcode, and ancestry; `watch`
    /// carries both parents' sides.
    #[tokio::test]
    async fn set_parent_yields_parent_changed_and_maps_errors_in_band() {
        let hub = hub();
        one(hub.create("SP".into(), "root".into(), Some("scope".into()), None, None, None, None).await).await;
        one(hub.create("SP".into(), "a".into(), None, None, None, None, None).await).await; // SP-2 → SP-1
        one(hub.create("SP".into(), "b".into(), None, None, None, None, None).await).await; // SP-3 → SP-1
        one(hub.create("OT".into(), "other".into(), Some("scope".into()), None, None, None, None).await).await;
        let mut watch = Box::pin(hub.watch(None).await);

        match one(hub.set_parent("sp-3".into(), "sp-2".into()).await).await {
            TicketsEvent::ParentChanged { ticket, previous } => {
                assert_eq!(ticket.id, "SP-3");
                assert_eq!(ticket.parent.as_deref(), Some("SP-2"));
                assert_eq!(previous.as_deref(), Some("SP-1"));
            }
            other => panic!("expected ParentChanged, got {other:?}"),
        }
        assert!(matches!(watch.next().await, Some(TicketsEvent::ParentChanged { .. })));
        match watch.next().await {
            Some(TicketsEvent::ChildrenChanged { ticket, added }) => {
                assert_eq!(ticket.id, "SP-2");
                assert_eq!(added, "SP-3");
            }
            other => panic!("expected ChildrenChanged, got {other:?}"),
        }
        match watch.next().await {
            Some(TicketsEvent::ChildRemoved { ticket, removed }) => {
                assert_eq!(ticket.id, "SP-1");
                assert_eq!(removed, "SP-3");
                assert_eq!(ticket.children, vec!["SP-2"]);
            }
            other => panic!("expected ChildRemoved, got {other:?}"),
        }

        assert!(matches!(
            one(hub.set_parent("SP-9".into(), "SP-1".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
        match one(hub.set_parent("SP-2".into(), "SP-9".into()).await).await {
            TicketsEvent::Error { message } => {
                assert!(message.contains("SP-9") && message.contains("must exist"), "{message}")
            }
            other => panic!("expected Error, got {other:?}"),
        }
        match one(hub.set_parent("SP-2".into(), "OT-1".into()).await).await {
            TicketsEvent::Error { message } => {
                assert!(message.contains("OT-1") && message.contains("outside"), "{message}")
            }
            other => panic!("expected Error, got {other:?}"),
        }
        match one(hub.set_parent("SP-1".into(), "SP-3".into()).await).await {
            TicketsEvent::Error { message } => assert!(message.contains("own ancestor"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        // The root may be hosted by another shortcode.
        match one(hub.set_parent("SP-1".into(), "OT-1".into()).await).await {
            TicketsEvent::ParentChanged { ticket, .. } => assert_eq!(ticket.parent.as_deref(), Some("OT-1")),
            other => panic!("expected ParentChanged, got {other:?}"),
        }
    }

    /// MNG-12: the first ticket of a prefix must be a scope — over the
    /// activation the refusal is the in-band error naming the rule.
    #[tokio::test]
    async fn first_ticket_of_a_prefix_must_be_a_scope_in_band() {
        let hub = hub();
        match one(hub.create("FRESH".into(), "a build".into(), None, None, None, None, None).await).await {
            TicketsEvent::Error { message } => {
                assert!(message.contains("FRESH") && message.contains("scope"), "{message}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
        assert!(matches!(
            one(hub.get("FRESH-1".into()).await).await,
            TicketsEvent::NotFound { .. }
        ));
    }

}
