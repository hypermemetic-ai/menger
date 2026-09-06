//! Plexus RPC surface for tickets.
//!
//! `create` names one optional parameter per ticket field (AGS-19 added
//! `parent`) — that is the wire surface talking, here and in the wrapper
//! the activation macro generates, not a call signature asking to become
//! a struct.
#![allow(clippy::too_many_arguments)]

use super::types::TicketsEvent;
use crate::store::{BlockerRef, Change, Kind, NewTicket, Severity, Status, Store, StoreError};
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
    /// naming the reason. Every shortcode starts with a scope (MNG-12):
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

    /// Yields one `Ticket` event per match, ordered by (epic, seq). The
    /// `parent` filter (AGS-19) yields exactly that ticket's children.
    #[plexus_macros::method(
        description = "List tickets, optionally filtered by epic, status and/or parent",
        params(
            epic = "Epic prefix filter (optional)",
            status = "Status filter: Pending | Ready | Blocked | Complete | Idea | Epic | Superseded (optional)",
            parent = "Only this ticket's children, e.g. PERF-3 (optional)"
        )
    )]
    pub async fn list(
        &self,
        epic: Option<String>,
        status: Option<String>,
        parent: Option<String>,
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
            for ticket in store.list(epic.as_deref(), status, parent.as_deref()) {
                yield TicketsEvent::Ticket { ticket };
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

    /// The DAG query: which tickets can start now? Yields one `Ticket`
    /// event per member of the ready set — status `Ready` and every
    /// `blocked_by` entry `Complete` or `Superseded` — ordered by
    /// (epic, seq). A dangling blocker keeps its ticket out (use
    /// `blockers` to see which ids dangle). The epic filter applies to
    /// membership; blockers resolve globally.
    #[plexus_macros::method(
        description = "Tickets that can start now: Ready, with every blocker Complete or Superseded",
        params(epic = "Only tickets in this epic (optional; blockers still resolve globally)")
    )]
    pub async fn ready_set(
        &self,
        epic: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        stream! {
            for ticket in store.ready_set(epic.as_deref()) {
                yield TicketsEvent::Ticket { ticket };
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

        let ready: Vec<_> = Box::pin(hub.list(None, Some("Ready".into()), None).await)
            .collect()
            .await;
        assert_eq!(ready.len(), 1);
        match &ready[0] {
            TicketsEvent::Ticket { ticket } => assert_eq!(ticket.id, "MNG-1"),
            other => panic!("expected Ticket, got {other:?}"),
        }
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

    /// Ids of a stream of `Ticket` events, in order.
    async fn ticket_ids(s: impl Stream<Item = TicketsEvent> + Send + 'static) -> Vec<String> {
        Box::pin(s)
            .map(|ev| match ev {
                TicketsEvent::Ticket { ticket } => ticket.id,
                other => panic!("expected Ticket, got {other:?}"),
            })
            .collect()
            .await
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

        assert_eq!(ticket_ids(hub.ready_set(None).await).await, vec!["DAG-1"]);

        one(hub.set_status("DAG-1".into(), "Complete".into()).await).await;
        assert_eq!(ticket_ids(hub.ready_set(None).await).await, vec!["DAG-2"]);
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
            ticket_ids(hub.ready_set(None).await).await,
            vec!["ONE-2", "TWO-1"]
        );
        assert_eq!(
            ticket_ids(hub.ready_set(Some("one".into())).await).await,
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

        assert!(ticket_ids(hub.ready_set(None).await).await.is_empty());

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

        assert_eq!(
            ticket_ids(hub.list(None, None, Some("LP-1".into())).await).await,
            vec!["LP-3", "ZZ-1"]
        );
        assert!(ticket_ids(hub.list(None, None, Some("LP-2".into())).await)
            .await
            .is_empty());
        assert_eq!(
            ticket_ids(hub.list(None, None, None).await).await.len(),
            4,
            "unfiltered list is unchanged"
        );
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
    /// MNG-12: `set_parent` over the activation — `ParentChanged` on
    /// success, `NotFound` for a missing child, in-band errors for a
    /// missing parent and for ancestry; `watch` carries the parent's side.
    #[tokio::test]
    async fn set_parent_yields_parent_changed_and_maps_errors_in_band() {
        let hub = hub();
        one(hub.create("SP".into(), "root".into(), Some("scope".into()), None, None, None, None).await).await;
        one(hub.create("SP".into(), "loose".into(), None, None, None, None, None).await).await;
        let mut watch = Box::pin(hub.watch(None).await);

        match one(hub.set_parent("sp-2".into(), "sp-1".into()).await).await {
            TicketsEvent::ParentChanged { ticket, previous } => {
                assert_eq!(ticket.id, "SP-2");
                assert_eq!(ticket.parent.as_deref(), Some("SP-1"));
                assert_eq!(previous, None);
            }
            other => panic!("expected ParentChanged, got {other:?}"),
        }
        assert!(matches!(watch.next().await, Some(TicketsEvent::ParentChanged { .. })));
        match watch.next().await {
            Some(TicketsEvent::ChildrenChanged { ticket, added }) => {
                assert_eq!(ticket.id, "SP-1");
                assert_eq!(added, "SP-2");
            }
            other => panic!("expected ChildrenChanged, got {other:?}"),
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
        match one(hub.set_parent("SP-1".into(), "SP-2".into()).await).await {
            TicketsEvent::Error { message } => assert!(message.contains("own ancestor"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
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
