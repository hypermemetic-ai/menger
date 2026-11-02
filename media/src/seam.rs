//! **The seam** — the media service as a client of the tickets service it is
//! mounted beside (V2-1: "each reaching the others as a client through the
//! seam"; the ui service's `seam.rs` is the pattern). The running service
//! is built after this activation is, so the activation holds an empty
//! [`Seam`] the builder fills once the service exists ([`Seam::attach`]).
//! A question about a ticket — may this caller edit it, may it see it —
//! goes through plexus-core's `HubTransport::as_(service, caller)` **as
//! the caller**, to `tickets.may`: the same route and gate a socket
//! reaches, the tickets service deciding by the caller's reach; the media
//! service never opens the tickets file and never reads its store.
//!
//! The shell is a client the same way: [`fetch`] asks `media.get` as the
//! session's identity, for `GET /media/<hash>`.

use crate::{MediaClient, MediaEvent, MediaHash};
use menger_tickets_api::members::Role;
use menger_tickets_api::vocab::TicketRef;
use menger_tickets_api::{TicketsClient, TicketsEvent};
use plexus_core::plexus::{AuthContext, DynamicHub, HubTransport};
use std::sync::{Arc, OnceLock, Weak};

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

/// What the tickets service answered about a ticket and a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The caller sees the ticket and holds the role on it: its id, as the
    /// tickets service names it.
    Held(String),
    /// The caller sees the ticket and does not hold the role on it.
    NotHeld,
    /// No such ticket in the caller's reach (ACCESS-10: not there and not
    /// theirs, one answer).
    NotFound,
}

impl Seam {
    /// The service this one is registered in; the first attach holds.
    pub fn attach(&self, service: &Arc<DynamicHub>) {
        let _ = self.at.set(Arc::downgrade(service));
    }

    /// The running service, while it runs.
    pub fn service(&self) -> Option<Arc<DynamicHub>> {
        self.at.get().and_then(Weak::upgrade)
    }

    /// `tickets.may { id: ticket, role }` asked as `caller`: whether it
    /// holds `role` on the ticket, by the tickets service's word. `Err`:
    /// the seam itself failed (not attached, the call refused or broken),
    /// in words — never a ticket's absence.
    pub async fn may(&self, caller: &AuthContext, ticket: &str, role: Role) -> Result<Answer, String> {
        let service = self.service().ok_or("the media service is not attached to a running service")?;
        let id: TicketRef = ticket.parse().map_err(|e: String| format!("{ticket}: {e}"))?;
        let tickets = TicketsClient::new(HubTransport::as_(service, caller.clone()));
        match tickets.may_all(id, role).await {
            Ok(events) => match events.as_slice() {
                [TicketsEvent::May { id, held: true, .. }] => Ok(Answer::Held(id.to_string())),
                [TicketsEvent::May { held: false, .. }] => Ok(Answer::NotHeld),
                [TicketsEvent::NotFound { .. }] => Ok(Answer::NotFound),
                [TicketsEvent::Error { message }] => Err(format!("tickets.may: {message}")),
                other => Err(format!("tickets.may answered {other:?}")),
            },
            Err(e) => Err(format!("tickets.may through the seam: {e}")),
        }
    }

    /// V2-29: `docs.may { id: doc, role }` asked as `caller`: whether it
    /// holds `role` on the document, by the docs service's word. `Err`: the
    /// seam itself failed, in words — never a document's absence.
    pub async fn doc_may(&self, caller: &AuthContext, doc: &str, role: Role) -> Result<menger_docs::seam::Answer, String> {
        let service = self.service().ok_or("the media service is not attached to a running service")?;
        menger_docs::seam::may(service, caller.clone(), doc, role).await
    }

    /// Whether `caller` holds `grant` (a method's dotted path, `media.get`)
    /// on `*`, as the running service's gate answers it now — what an image
    /// put with no ticket is answered to besides its putter. A service with
    /// no gate holds everything open; `None` before the builder attached it.
    pub fn held_everywhere(&self, caller: &AuthContext, grant: &str) -> Option<bool> {
        let service = self.service()?;
        Some(match service.gate() {
            None => true,
            Some(gate) => gate.held_on(caller, &plexus_auth_core::grants::Grant::new(grant)).iter().any(|s| s.is_everywhere()),
        })
    }
}

/// What `GET /media/<hash>` found, asking `media.info` as the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// The file the session may see: what it is. Its bytes are read by
    /// range from the store ([`crate::MediaStore::read_at`]).
    Found(crate::MediaInfo),
    /// No such file, or none the caller may see — one answer.
    NotFound,
    /// The call failed or was refused (the gate's word, a broken hash).
    Refused(String),
}

/// `media.info { hash }` through `service` as `caller` — the shell's route,
/// a client like any other: the gate admits or refuses it, the service
/// decides whose the file is.
pub async fn fetch(service: Arc<DynamicHub>, caller: AuthContext, hash: &str) -> Fetched {
    let hash: MediaHash = match hash.parse() {
        Ok(h) => h,
        Err(_) => return Fetched::NotFound,
    };
    let media = MediaClient::new(HubTransport::as_(service, caller));
    match media.info_all(hash).await {
        Ok(events) => match events.as_slice() {
            [MediaEvent::Info { hash, mime, bytes, width, height, kind, name }] => {
                Fetched::Found(crate::MediaInfo { hash: hash.clone(), mime: mime.clone(), bytes: *bytes, width: *width, height: *height, kind: kind.clone(), name: name.clone() })
            }
            [MediaEvent::NotFound { .. }] => Fetched::NotFound,
            [MediaEvent::Error { message }] => Fetched::Refused(message.clone()),
            other => Fetched::Refused(format!("media.info answered {}", other.len())),
        },
        Err(e) => Fetched::Refused(e.to_string()),
    }
}
