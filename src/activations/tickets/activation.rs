//! Plexus RPC surface for tickets.

use super::types::TicketsEvent;
use crate::store::{Change, NewTicket, Severity, Status, Store, StoreError, TicketType};
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
    /// id; status starts at `Pending` (only a human promotes to `Ready`).
    /// Yields exactly one event: `Created` or `Error`.
    #[plexus_macros::method(
        description = "Create a ticket; allocates the next EPIC-N id, status starts Pending",
        params(
            epic = "Epic prefix, e.g. MNG (case-insensitive, stored uppercase)",
            title = "Short description",
            ticket_type = "implementation | analysis | spike | epic | checkpoint (default: implementation)",
            severity = "Critical | High | Medium | Low (optional)",
            blocked_by = "Ticket ids that must complete first (optional)",
            body = "Markdown body (optional)"
        )
    )]
    pub async fn create(
        &self,
        epic: String,
        title: String,
        ticket_type: Option<String>,
        severity: Option<String>,
        blocked_by: Option<Vec<String>>,
        body: Option<String>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let store = self.store.clone();
        stream! {
            let ticket_type = match ticket_type.as_deref().map(str::parse::<TicketType>) {
                None => TicketType::Implementation,
                Some(Ok(t)) => t,
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
                ticket_type,
                severity,
                blocked_by: blocked_by.unwrap_or_default(),
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

    /// Yields one `Ticket` event per match, ordered by (epic, seq).
    #[plexus_macros::method(
        description = "List tickets, optionally filtered by epic and/or status",
        params(
            epic = "Epic prefix filter (optional)",
            status = "Status filter: Pending | Ready | Blocked | Complete | Idea | Epic | Superseded (optional)"
        )
    )]
    pub async fn list(
        &self,
        epic: Option<String>,
        status: Option<String>,
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
            for ticket in store.list(epic.as_deref(), status) {
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

    /// The reactive primitive: an open-ended stream of every mutation,
    /// as `Created` / `StatusChanged` events. Runs until the client
    /// disconnects. A slow consumer that lags the broadcast buffer skips
    /// the missed changes and keeps receiving from the current position.
    #[plexus_macros::method(
        streaming,
        description = "Subscribe to ticket changes (Created / StatusChanged), until disconnect",
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
        let ev = one(hub.create("mng".into(), "first".into(), None, None, None, None).await).await;
        match ev {
            TicketsEvent::Created { ticket } => {
                assert_eq!(ticket.id, "MNG-1");
                assert_eq!(ticket.status, Status::Pending);
            }
            other => panic!("expected Created, got {other:?}"),
        }
        let ev = one(hub.create("MNG".into(), "second".into(), None, None, None, None).await).await;
        match ev {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.id, "MNG-2"),
            other => panic!("expected Created, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_rejects_unknown_ticket_type() {
        let hub = hub();
        let ev = one(
            hub.create("MNG".into(), "x".into(), Some("wibble".into()), None, None, None)
                .await,
        )
        .await;
        assert!(matches!(ev, TicketsEvent::Error { .. }), "got {ev:?}");
    }

    #[tokio::test]
    async fn get_distinguishes_found_from_missing() {
        let hub = hub();
        one(hub.create("MNG".into(), "x".into(), None, None, None, None).await).await;
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
        one(hub.create("MNG".into(), "a".into(), None, None, None, None).await).await;
        one(hub.create("MNG".into(), "b".into(), None, None, None, None).await).await;
        one(hub.set_status("MNG-1".into(), "Ready".into()).await).await;

        let ready: Vec<_> = Box::pin(hub.list(None, Some("Ready".into())).await)
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
        one(hub.create("MNG".into(), "a".into(), None, None, None, None).await).await;
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

        one(hub.create("MNG".into(), "a".into(), None, None, None, None).await).await;
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

    #[tokio::test]
    async fn watch_epic_filter_drops_other_epics() {
        let hub = hub();
        let mut watch = Box::pin(hub.watch(Some("mng".into())).await);

        one(hub.create("OTHER".into(), "x".into(), None, None, None, None).await).await;
        one(hub.create("MNG".into(), "y".into(), None, None, None, None).await).await;

        match watch.next().await.expect("change") {
            TicketsEvent::Created { ticket } => assert_eq!(ticket.epic, "MNG"),
            other => panic!("expected Created, got {other:?}"),
        }
    }
}
