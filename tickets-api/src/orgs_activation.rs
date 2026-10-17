//! ORG-4: the `orgs` activation — **the one declaration**, as `members`
//! and `session` are (MFE-11): the server's [`Orgs`] and the browser's
//! `OrgsClient` (the app's org switcher).
//!
//! Its methods hold only the call: who may, and what the store answers —
//! the logic is the store's (`store/orgs.rs`, MENGER-29).
//!
//! **Under login** (AUTH-7): `list` is a viewer's (and an editor's) — the
//! organizations the caller holds a role in, every one for a role on `*`.
//! `create`, `rename` and `delete` are unlisted, so admin's alone (admin
//! holds every grant): `create` needs admin on `*` (a site admin), and
//! names the new organization's first admin; `rename` and `delete` need
//! admin on the organization (`acme:`) or on `*`. An organization the
//! caller holds no role in is `not_found`, never refused by name (ORG-4:
//! another org's names are not a stranger's to learn). Login off: every
//! caller reaches every organization, as everything else.

use crate::orgs::OrgsEvent;
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::address::OrgId;
use crate::vocab::Principal;
#[cfg(feature = "server")]
use crate::access::{Access, VerifiedUser};
#[cfg(feature = "server")]
use crate::vocab::GrantedBy;
#[cfg(feature = "server")]
use crate::store::{Store, StoreError};
#[cfg(feature = "server")]
use async_stream::stream;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use plexus_core::plexus::{AuthContext, Reach};
#[cfg(feature = "server")]
use std::sync::Arc;

/// The activation. Server-only: it holds the store and the rule.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct OrgsActivation {
    store: Arc<Store>,
    access: Access,
}

#[cfg(feature = "server")]
impl OrgsActivation {
    /// ACCESS-24: a test's, behind [`Access::for_tests`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn new(store: Arc<Store>) -> Self {
        let access = Access::for_tests();
        access.attach(Arc::clone(&store));
        Self::with_access(store, access)
    }

    /// Behind [`Access`].
    pub fn with_access(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

/// A store refusal as the one event it is — an organization there is not
/// is `not_found`.
#[cfg(feature = "server")]
fn refused(e: StoreError, name: &str) -> OrgsEvent {
    match e {
        StoreError::NoOrg(_) => OrgsEvent::NotFound { name: name.trim().to_string() },
        e => OrgsEvent::Error { message: e.to_string() },
    }
}

/// The organization `name` names, when `caller` administers it (or `*`):
/// `Err` is the event to answer — `not_found` when it holds no role there
/// (or there is none), a refusal when it holds one but not admin.
#[cfg(feature = "server")]
fn administered(store: &Store, caller: &VerifiedUser, name: &str) -> Result<(crate::orgs::Org, crate::access::AdminOf), OrgsEvent> {
    let not_found = || OrgsEvent::NotFound { name: name.trim().to_string() };
    let org = store.org(name).ok_or_else(not_found)?;
    if caller.orgs().is_some_and(|held| !held.contains(&org.name)) {
        return Err(not_found());
    }
    // ACCESS-11: admin on it, the proof the store's write takes.
    match caller.admin_of(&crate::members::Scope::Org(org.name.clone()), store) {
        Ok(a) => Ok((org, a)),
        Err(_) => Err(OrgsEvent::Error { message: format!("{} needs admin on {}: (or on *)", caller.principal().as_str(), org.name) }),
    }
}

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
#[plexus_macros::activation(
    description = "Organizations: every ticket and shortcode is in one; list yours, and (admin) create, rename, delete",
    typed_params,
    // AUTH-7: any member lists the organizations it is in; create, rename and
    // delete are admin's (unlisted: admin holds every grant).
    roles(
        viewer = ["list"],
        editor = [viewer],
    )
)]
impl OrgsActivation {
    /// The organizations the caller holds a role in — every one for a role
    /// on `*` — in the order they were made: one `org
    /// { name, aliases, roots, shortcodes, tickets }` each.
    #[plexus_macros::method(group = "read", description = "The organizations you hold a role in")]
    pub async fn list(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = OrgsEvent> + Send + 'static {
        let admitted = self.access.verify_call(auth, &reach);
        let store = self.store.clone();
        stream! {
            let caller = match admitted {
                Ok(c) => c,
                Err(message) => {
                    yield OrgsEvent::Error { message };
                    return;
                }
            };
            let held = caller.orgs();
            for org in store.orgs() {
                if held.as_ref().is_none_or(|h| h.contains(&org.name)) {
                    yield OrgsEvent::Org { org };
                }
            }
        }
    }

    /// Makes the organization `name`, empty — lower-case letters, digits
    /// and `-`, unique app-wide (among every name an organization has or
    /// had). `admin` names its first admin, who holds admin on `name:`
    /// from this grant. Needs admin on `*`. Yields `created { org, admin }`.
    #[plexus_macros::method(
        group = "admin",
        description = "Make an organization (admin on *), naming its first admin",
        params(
            name = "Its name: lower-case letters, digits and -, unique, e.g. acme",
            admin = "The Plexus principal who administers it, e.g. key:9f… (optional: else only the site's admins do)"
        ),
        example(note = "acme, administered by one key", params = r#"{"name": "acme", "admin": "key:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"}"#)
    )]
    pub async fn create(&self, auth: Option<&AuthContext>, reach: Reach, name: OrgId, admin: Option<Principal>) -> impl Stream<Item = OrgsEvent> + Send + 'static {
        let admitted = self.access.verify_call(auth, &reach);
        let (store, access) = (self.store.clone(), self.access.clone());
        stream! {
            let caller = match admitted {
                Ok(c) => c,
                Err(message) => {
                    yield OrgsEvent::Error { message };
                    return;
                }
            };
            let Ok(by) = caller.admin_of(&crate::members::Scope::Everywhere, &store) else {
                yield OrgsEvent::Error { message: "a new organization needs admin on *".into() };
                return;
            };
            let admin = admin.map(|p| (p, GrantedBy::Principal(caller.principal().clone())));
            match store.create_org(by, &name, admin).await {
                Ok((org, admin)) => {
                    if let Some(g) = &admin {
                        access.granted(&g.principal.to_string());
                    }
                    yield OrgsEvent::Created { org, admin }
                }
                Err(e) => yield refused(e, &name),
            }
        }
    }

    /// Renames the organization `name` to `to`. The old name stays an
    /// alias: every address and grant scope written with it keeps
    /// resolving; every grant on it is rewritten to the new name. Needs
    /// admin on it (`name:`) or on `*`. Yields `renamed { from, org }`.
    #[plexus_macros::method(
        group = "admin",
        description = "Rename an organization; its old name keeps resolving",
        params(name = "The organization, e.g. acme", to = "Its new name: lower-case letters, digits and -, unique")
    )]
    pub async fn rename(&self, auth: Option<&AuthContext>, reach: Reach, name: OrgId, to: OrgId) -> impl Stream<Item = OrgsEvent> + Send + 'static {
        let admitted = self.access.verify_call(auth, &reach);
        let store = self.store.clone();
        stream! {
            let caller = match admitted {
                Ok(c) => c,
                Err(message) => {
                    yield OrgsEvent::Error { message };
                    return;
                }
            };
            let (org, by) = match administered(&store, &caller, &name) {
                Ok(o) => o,
                Err(ev) => {
                    yield ev;
                    return;
                }
            };
            match store.rename_org(by, org.name.as_str(), &to).await {
                Ok(now) => yield OrgsEvent::Renamed { from: org.name.to_string(), org: now },
                Err(e) => yield refused(e, &name),
            }
        }
    }

    /// Deletes the organization `name` — refused while it holds a shortcode
    /// (a ticket, or a deleted ticket kept for its undo), and for the first
    /// organization. Every grant on it goes with it. Needs admin on it or
    /// on `*`. Yields `deleted { name, grants }`.
    #[plexus_macros::method(
        group = "admin",
        description = "Delete an empty organization, and every grant on it",
        params(name = "The organization, e.g. acme")
    )]
    pub async fn delete(&self, auth: Option<&AuthContext>, reach: Reach, name: OrgId) -> impl Stream<Item = OrgsEvent> + Send + 'static {
        let admitted = self.access.verify_call(auth, &reach);
        let store = self.store.clone();
        stream! {
            let caller = match admitted {
                Ok(c) => c,
                Err(message) => {
                    yield OrgsEvent::Error { message };
                    return;
                }
            };
            let (org, by) = match administered(&store, &caller, &name) {
                Ok(o) => o,
                Err(ev) => {
                    yield ev;
                    return;
                }
            };
            match store.delete_org(by, org.name.as_str()).await {
                Ok((name, grants)) => yield OrgsEvent::Deleted { name, grants },
                Err(e) => yield refused(e, &name),
            }
        }
    }
}
