//! **A log's reach is its ticket's** (LOGS-1 › reach), and the tickets
//! service decides it, asked **as the caller** — the media service's seam,
//! its pattern. [`Reach`] is the question the service asks; [`Seam`]
//! answers it through the running service, plexus-core's
//! `HubTransport::as_(service, caller)` to `tickets.get` (which ticket a
//! name is: its id now and its uid) and `tickets.may` (whether the caller
//! holds a role on it) — the same route and gate a socket reaches. The logs
//! service never opens the tickets file and never reads its store. A test
//! answers [`Reach`] with a fake.

use crate::caller::Caller;
use async_trait::async_trait;
use futures::TryStreamExt;
use menger_tickets_api::members::Role;
use menger_tickets_api::vocab::TicketRef;
use menger_tickets_api::{TicketsClient, TicketsEvent};
use plexus_rpc::core::plexus::{DynamicHub, HubTransport};
use std::sync::{Arc, OnceLock, Weak};

/// A ticket the caller sees: its id now, and its uid, which no move changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub id: String,
    pub uid: String,
}

/// What the tickets service answered about a ticket and a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The caller sees the ticket and holds the role on it: its id now.
    Held(String),
    /// The caller sees the ticket and does not hold the role on it.
    NotHeld,
    /// No such ticket in the caller's reach (ACCESS-10: not there and not
    /// theirs, one answer).
    NotFound,
}

/// What the logs service asks of the tickets service, as the caller.
/// `Err`: the asking itself failed, in words — never a ticket's absence.
#[async_trait]
pub trait Reach: Send + Sync + std::fmt::Debug {
    /// The ticket `ticket` (any name of it: an id, an address, a uid) names,
    /// as `caller` sees it.
    async fn seen(&self, caller: &Caller, ticket: &str) -> Result<Option<Seen>, String>;
    /// Whether `caller` holds `role` on the ticket `ticket` names.
    async fn may(&self, caller: &Caller, ticket: &str, role: Role) -> Result<Answer, String>;
}

/// Where the running service is, once the builder says. Weak: the service
/// owns the activation that holds this.
#[derive(Clone, Default)]
pub struct Seam {
    at: Arc<OnceLock<Weak<DynamicHub>>>,
}

impl std::fmt::Debug for Seam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seam").field("attached", &self.at.get().is_some()).finish()
    }
}

impl Seam {
    /// The service this one is registered in; the first attach holds.
    pub fn attach(&self, service: &Arc<DynamicHub>) {
        let _ = self.at.set(Arc::downgrade(service));
    }

    fn tickets(&self, caller: &Caller) -> Result<TicketsClient<HubTransport>, String> {
        let service = self.at.get().and_then(Weak::upgrade).ok_or("the logs service is not attached to a running service")?;
        Ok(TicketsClient::new(HubTransport::as_(service, caller.context().clone())))
    }
}

fn named(ticket: &str) -> Result<TicketRef, String> {
    ticket.parse().map_err(|e: String| format!("{ticket}: {e}"))
}

#[async_trait]
impl Reach for Seam {
    async fn seen(&self, caller: &Caller, ticket: &str) -> Result<Option<Seen>, String> {
        let events = self.tickets(caller)?.get(named(ticket)?, None).try_collect::<Vec<_>>().await.map_err(|e| format!("tickets.get through the seam: {e}"))?;
        for ev in &events {
            match ev {
                TicketsEvent::Ticket { ticket: t } if t.uid.is_some() => return Ok(Some(Seen { id: t.id.to_string(), uid: t.uid.as_str().to_string() })),
                TicketsEvent::Ticket { ticket: t } => return Err(format!("tickets.get: {} has no uid", t.id)),
                TicketsEvent::NotFound { .. } | TicketsEvent::Deleted { .. } => return Ok(None),
                TicketsEvent::Error { message } => return Err(format!("tickets.get: {message}")),
                _ => {}
            }
        }
        Err(format!("tickets.get answered {} events and no ticket", events.len()))
    }

    async fn may(&self, caller: &Caller, ticket: &str, role: Role) -> Result<Answer, String> {
        let events = self.tickets(caller)?.may(named(ticket)?, role).try_collect::<Vec<_>>().await.map_err(|e| format!("tickets.may through the seam: {e}"))?;
        match events.as_slice() {
            [TicketsEvent::May { id, held: true, .. }] => Ok(Answer::Held(id.to_string())),
            [TicketsEvent::May { held: false, .. }] => Ok(Answer::NotHeld),
            [TicketsEvent::NotFound { .. }] => Ok(Answer::NotFound),
            [TicketsEvent::Error { message }] => Err(format!("tickets.may: {message}")),
            other => Err(format!("tickets.may answered {other:?}")),
        }
    }
}
