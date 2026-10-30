//! TICKETS-28: `tickets subtasks` — a ticket's subtasks, named by the
//! store: `create` (the next free `<parent>.N`, hidden) and `list` (a
//! parent's, in `.N` order).
//!
//! A `#[plexus_macros::child]` of `tickets` on the frame (ACCESS-13), as
//! `commits` is: `tickets.subtasks.create` / `list` on the wire, its client
//! `SubtasksClient` reached as `client.subtasks()`. Its grants are its own
//! `roles(...)`: `list` is the viewer's, `create` the editor's (filed under
//! the parent as `tickets.create` is, ACCESS-21).

use crate::types::{Kind, TicketsEvent};
use crate::vocab::{Body, Cursor, TicketRef, Title};
#[cfg(feature = "server")]
use super::*;
#[cfg(feature = "server")]
use crate::access::Access;
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use std::sync::Arc;

/// A ticket's subtasks: made and listed.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct SubtasksActivation {
    pub(super) store: Arc<Store>,
    pub(super) access: Access,
}

/// A ticket's subtasks — create the next `<parent>.N` (hidden), list a parent's.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(viewer = ["list"], editor = [viewer, "create"])
)]
impl SubtasksActivation {
    /// Create a subtask of a ticket: the next free `<parent>.N` (from .1; a number is never reused), hidden from the nav's views; yields created { ticket }.
    ///
    /// TICKETS-28: one write, `tickets.create`'s rules and permits (editor
    /// on what is below the parent) with the naming done by the store: the
    /// id is the parent's with `.N` — the number past the highest the parent
    /// holds and past every one a delete or a rename took out — and
    /// `hidden` is true (TICKETS-27), so it is in no list, tree or count
    /// but its parent's children (`subtasks list`, `list --parent`). It is
    /// in the parent's shortcode, born as `create` births (not a scope
    /// or a doc: they root a shortcode). A parent that names no ticket is an
    /// in-band error and nothing is written. Yields exactly one event:
    /// `Created` or `Error`.
    #[plexus_macros::method(
        group = "write",
        values(kind = ["execution", "build", "gap", "spike"]),
        example(note = "a step of TICKETS-10", params = r#"{"parent": "TICKETS-10", "title": "Draw the sheet"}"#)
    )]
    pub async fn create(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The ticket it is a subtask of, e.g. TICKETS-10 (a subtask may have subtasks: TICKETS-10.1).
        parent: TicketRef,
        /// Short description.
        title: Title,
        /// Markdown body (optional).
        body: Option<Body>,
        /// execution | build | gap | spike (default: build).
        kind: Option<Kind>,
        /// Ticket ids that must complete first (optional; never a doc).
        blocked_by: Option<Vec<TicketRef>>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        answered_then!(store = self.store.clone(), user, None, then => {
            let ps = Some(parent.as_str());
            let at = filing(&store, &user, "", ps, blocked_by.as_deref())?;
            let (act, chain) = (act_of(&user, None, None)?, chain_under(&store, &user, "", ps)?);
            let new = NewTicket { epic: String::new(), title: title.into_string(), kind: kind.unwrap_or(Kind::Build), severity: None, blocked_by: blocked_by.unwrap_or_default(), parent: Some(parent.to_string()), path: None, body: body.map(String::from).unwrap_or_default() };
            let ticket = store.create_subtask_by(at, new, act).await?;
            then = restore_chain(&store, &user, chain, format!("{} was filed under it", ticket.id)).await;
            Ok(TicketsEvent::Created { ticket })
        })
    }

    /// A ticket's subtasks in `.N` order, as summaries (hidden ones included), then one page event.
    ///
    /// TICKETS-28: the children `tickets.subtasks.create` made — and any
    /// other child of the parent, as `list --parent` reads them — through the
    /// caller's reach. `after` and `limit` page as `list`'s do.
    #[plexus_macros::method(group = "read", example(note = "TICKETS-10's subtasks", params = r#"{"parent": "TICKETS-10"}"#))]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The ticket whose subtasks to list, e.g. TICKETS-10.
        parent: TicketRef,
        /// Cursor: start strictly after this ticket id (optional).
        after: Option<Cursor>,
        /// At most this many (default 1000, at most 10000; optional).
        limit: Option<u32>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        // PERF-15: one `rows`, then the page — as `listed`, the reach first.
        let limit = Some(crate::limits::page(limit, crate::limits::LIST_PAGE_DEFAULT, crate::limits::LIST_PAGE_MAX));
        let events: Vec<TicketsEvent> = match user.read_filter() {
            Err(message) => vec![TicketsEvent::Error { message }],
            Ok(_) => page_events(self.store.page(&user, PageQuery { epic: None, status: None, parent: Some(parent.as_str()), after: after.as_deref(), limit }), None).collect(),
        };
        futures::stream::iter(events)
    }
}
