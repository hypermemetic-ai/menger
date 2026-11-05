//! **The seam** — the docs service as a client of the service it is
//! mounted beside (V2-1; the media service's `seam.rs` is the pattern). The
//! running service is built after this activation is, so the activation
//! holds an empty [`Seam`] the builder fills ([`Seam::attach`]).
//!
//! Two questions go through it, never through another service's file:
//! **may this caller do this to a document of `acme`** — the gate's word,
//! the method's grant (`docs.save`) asked on the organization's scope
//! (`acme:`), within the session's ceiling ([`Seam::holds`]); and **which
//! organizations is it in** — `orgs.list`, asked of the tickets service as
//! the caller ([`Seam::orgs`]).
//!
//! The media service is a client of this one the same way: [`may`] asks
//! `docs.may` as the caller before it keeps an image for a document.

use crate::{DocsClient, DocsEvent};
use futures::TryStreamExt;
use menger_tickets_api::members::Role;
use menger_tickets_api::{OrgsClient, OrgsEvent};
use plexus_auth_core::grants::Grant;
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

/// The grant a role's question about a document is asked as: a viewer
/// reads (`docs.get`), an editor writes (`docs.save`), an admin deletes
/// (`docs.delete`). Any other role holds nothing of a document.
pub fn grant_of(role: Role) -> Option<&'static str> {
    match role {
        Role::Viewer => Some("docs.get"),
        Role::Editor => Some("docs.save"),
        Role::Admin => Some("docs.delete"),
        _ => None,
    }
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

    /// Whether `caller` holds `grant` (`docs.save`) on the organization
    /// `org`, as the gate answers it now — through a role held on `org:`
    /// or on `*`, within the session's ceiling. A service with no gate
    /// holds everything open. `Err`: not attached.
    pub fn holds(&self, caller: &AuthContext, grant: &str, org: &str) -> Result<bool, String> {
        let service = self.service().ok_or("the docs service is not attached to a running service")?;
        Ok(match service.gate() {
            None => true,
            Some(gate) => gate.check(Some(caller), &Grant::new(grant), Some(&format!("{org}:"))).is_ok(),
        })
    }

    /// V2-31: whether `caller` holds `grant` on `*` — an admin of
    /// everything, as the gate answers now. No gate: everyone does.
    pub fn everywhere(&self, caller: &AuthContext, grant: &str) -> bool {
        self.service().is_some_and(|s| s.gate().is_none_or(|g| g.held_on(caller, &Grant::new(grant)).iter().any(|h| h.is_everywhere())))
    }

    /// The organizations `caller` is in, in the order they were made —
    /// `orgs.list` asked as the caller, the tickets service's word.
    pub async fn orgs(&self, caller: &AuthContext) -> Result<Vec<String>, String> {
        let service = self.service().ok_or("the docs service is not attached to a running service")?;
        let orgs = OrgsClient::new(HubTransport::as_(service, caller.clone()));
        match orgs.list().try_collect::<Vec<_>>().await {
            Ok(events) => {
                let mut out = Vec::new();
                for e in events {
                    match e {
                        OrgsEvent::Org { org } => out.push(org.name.as_str().to_string()),
                        OrgsEvent::Error { message } => return Err(format!("orgs.list: {message}")),
                        _ => {}
                    }
                }
                Ok(out)
            }
            Err(e) => Err(format!("orgs.list through the seam: {e}")),
        }
    }
}

/// What the docs service answered a `may`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The caller reads the document and holds the role on it: its id.
    Held(String),
    /// The caller reads it and does not hold the role.
    NotHeld,
    /// No such document in the caller's reach (not there and not theirs, one answer).
    NotFound,
}

/// `docs.may { id, role }` through `service` as `caller` — the media
/// service's question before it keeps an image for a document, or answers
/// one. `Err`: the call itself failed, in words.
pub async fn may(service: Arc<DynamicHub>, caller: AuthContext, id: &str, role: Role) -> Result<Answer, String> {
    let doc: crate::DocId = id.parse()?;
    let docs = DocsClient::new(HubTransport::as_(service, caller));
    match docs.may(doc, role).try_collect::<Vec<_>>().await {
        Ok(events) => match events.as_slice() {
            [DocsEvent::May { id, held: true, .. }] => Ok(Answer::Held(id.clone())),
            [DocsEvent::May { held: false, .. }] => Ok(Answer::NotHeld),
            [DocsEvent::NotFound { .. }] => Ok(Answer::NotFound),
            [DocsEvent::Error { message }] => Err(format!("docs.may: {message}")),
            other => Err(format!("docs.may answered {other:?}")),
        },
        Err(e) => Err(format!("docs.may through the seam: {e}")),
    }
}
