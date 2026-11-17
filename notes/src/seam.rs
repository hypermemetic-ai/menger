//! **The seam** — the notes service as a client of the service it is
//! mounted beside (V2-1; the media service's `seam.rs` is the pattern). The
//! running service is built after this activation is, so the activation
//! holds an empty [`Seam`] the builder fills ([`Seam::attach`]).
//!
//! Two questions go through it, never through another service's file:
//! **may this caller do this to a note of `acme`** — the gate's word,
//! the method's grant (`notes.save`) asked on the organization's scope
//! (`acme:`), within the session's ceiling ([`Seam::holds`]); and **which
//! organizations is it in** — `orgs.list`, asked of the tickets service as
//! the caller ([`Seam::orgs`]).
//!
//! The media service is a client of this one the same way: [`may`] asks
//! `notes.may` as the caller before it keeps an image for a note.
//!
//! NOTES-4: a third — **does this kept version hold this component** —
//! asked of the ui service before a slot is filled ([`Seam::component`]),
//! through what the builder attaches ([`Components`]): the ui service's own
//! store, in the process. The ui service of plexus-ui 0.2.6 answers no wire
//! method a version's components (`ui.components` resolves the live bundle's
//! and an author's own); the notes service opens no ui file either way.

use crate::{NotesClient, NotesEvent};
use futures::TryStreamExt;
use menger_tickets_api::members::Role;
use menger_tickets_api::{OrgsClient, OrgsEvent};
use plexus_rpc::auth_core::grants::Grant;
use plexus_rpc::core::plexus::{AuthContext, DynamicHub, HubTransport};
use std::sync::{Arc, OnceLock, Weak};

/// Where the running service is, once the builder says. Weak: the service
/// owns the activation that holds this.
#[derive(Clone, Default)]
pub struct Seam {
    at: Arc<OnceLock<Weak<DynamicHub>>>,
    components: Arc<OnceLock<Arc<dyn Components>>>,
}

/// NOTES-4: what the ui service says of a component of a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// The version is kept (its whole hash) and holds the component.
    Holds(String),
    /// The version is kept (its whole hash) and holds no such component.
    NoComponent(String),
    /// The ui service keeps no such version (none, or a prefix naming two).
    NoVersion,
}

/// NOTES-4: the ui service's answer to "does `version` hold `component`" —
/// the builder attaches the running ui service's ([`Seam::attach_components`]).
pub trait Components: Send + Sync {
    /// Whether the kept version `version` (a hash, or its first 6 or more
    /// hex digits) holds `component`. `Err`: the ui store failed, in words.
    fn holds(&self, version: &str, component: &str) -> Result<Held, String>;
}

impl std::fmt::Debug for Seam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seam").field("attached", &self.at.get().is_some()).finish()
    }
}

/// The grant a role's question about a note is asked as: a viewer
/// reads (`notes.get`), an editor writes (`notes.save`), an admin deletes
/// (`notes.delete`). Any other role holds nothing of a note.
pub fn grant_of(role: Role) -> Option<&'static str> {
    match role {
        Role::Viewer => Some("notes.get"),
        Role::Editor => Some("notes.save"),
        Role::Admin => Some("notes.delete"),
        _ => None,
    }
}

impl Seam {
    /// The service this one is registered in; the first attach holds.
    pub fn attach(&self, service: &Arc<DynamicHub>) {
        let _ = self.at.set(Arc::downgrade(service));
    }

    /// NOTES-4: the ui service's word on versions; the first attach holds.
    pub fn attach_components(&self, components: Arc<dyn Components>) {
        let _ = self.components.set(components);
    }

    /// NOTES-4: whether the kept version `version` holds `component`, as the
    /// ui service says (off the runtime). `Err`: none attached, or it failed.
    pub async fn component(&self, version: &str, component: &str) -> Result<Held, String> {
        let ui = self.components.get().cloned().ok_or("the notes service is not attached to the ui service")?;
        let (version, component) = (version.to_string(), component.to_string());
        tokio::task::spawn_blocking(move || ui.holds(&version, &component)).await.map_err(|e| format!("the ui service's answer failed: {e}"))?
    }

    /// The running service, while it runs.
    pub fn service(&self) -> Option<Arc<DynamicHub>> {
        self.at.get().and_then(Weak::upgrade)
    }

    /// Whether `caller` holds `grant` (`notes.save`) on the organization
    /// `org`, as the gate answers it now — through a role held on `org:`
    /// or on `*`, within the session's ceiling. A service with no gate
    /// holds everything open. `Err`: not attached.
    pub fn holds(&self, caller: &AuthContext, grant: &str, org: &str) -> Result<bool, String> {
        let service = self.service().ok_or("the notes service is not attached to a running service")?;
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
        let service = self.service().ok_or("the notes service is not attached to a running service")?;
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

/// What the notes service answered a `may`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The caller reads the note and holds the role on it: its id.
    Held(String),
    /// The caller reads it and does not hold the role.
    NotHeld,
    /// No such note in the caller's reach (not there and not theirs, one answer).
    NotFound,
}

/// `notes.may { id, role }` through `service` as `caller` — the media
/// service's question before it keeps an image for a note, or answers
/// one. `Err`: the call itself failed, in words.
pub async fn may(service: Arc<DynamicHub>, caller: AuthContext, id: &str, role: Role) -> Result<Answer, String> {
    let doc: crate::NoteId = id.parse()?;
    let notes = NotesClient::new(HubTransport::as_(service, caller));
    match notes.may(doc, role).try_collect::<Vec<_>>().await {
        Ok(events) => match events.as_slice() {
            [NotesEvent::May { id, held: true, .. }] => Ok(Answer::Held(id.clone())),
            [NotesEvent::May { held: false, .. }] => Ok(Answer::NotHeld),
            [NotesEvent::NotFound { .. }] => Ok(Answer::NotFound),
            [NotesEvent::Error { message }] => Err(format!("notes.may: {message}")),
            other => Err(format!("notes.may answered {other:?}")),
        },
        Err(e) => Err(format!("notes.may through the seam: {e}")),
    }
}
