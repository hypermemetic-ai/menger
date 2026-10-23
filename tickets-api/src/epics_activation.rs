//! ORG-13: the `epics` activation — **the one declaration**, as `orgs` and
//! `members` are (MFE-11): the server's [`EpicsActivation`] and the
//! browser's `EpicsClient` (the nav's epic switcher).
//!
//! An epic is a level of the organization that holds shortcodes; a new one
//! starts empty, its shortcodes numbered from 1 ([`crate::epics`]). Its
//! methods hold only the call; the logic is the store's (`store/eras.rs`)
//! and the streams beside this file (`epics_activation/streams.rs`).
//!
//! **On the frame** (ACCESS-13): `auth = "required"`, `request =
//! MengerRequest`, every method's caller a [`VerifiedUser`], every
//! description a `///` doc. `list`, `get`, `current` and `switch` are a
//! viewer's — `switch` writes the caller's own setting, held on the
//! organization; `create`, `rename` and `delete` are unlisted, so admin's,
//! on the organization (or `*`). Every method reads in `org` — the
//! caller's organization when it names none; one it holds no role in is
//! `not_found`.

use crate::epics::EpicsEvent;
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::address::{EpicName, OrgId};
#[cfg(feature = "server")]
use crate::access::{Access, MengerRequest, VerifiedUser};
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use std::sync::Arc;

// The methods' logic, beside the activation (MENGER-29, ACCESS-13).
#[cfg(feature = "server")]
mod streams;

/// The activation. Server-only: it holds the store and the rule.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct EpicsActivation {
    store: Arc<Store>,
    access: Access,
}

#[cfg(feature = "server")]
impl EpicsActivation {
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

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
/// Epics: a level of the organization that holds shortcodes; a new one starts fresh, its shortcodes numbered from 1. List, choose yours, and (admin) create, rename, delete.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(
        viewer = ["list", "get", "current", "switch"],
        editor = [viewer],
    )
)]
impl EpicsActivation {
    /// The organization's epics, oldest first: one `epic { org, name, first, current, shortcodes, tickets, created_unix }` each.
    #[plexus_macros::method(group = "read")]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::read(self.store.clone(), user, org, |store, org, me| Ok(store.epics(org, me)?.into_iter().map(|epic| EpicsEvent::Epic { epic }).collect()))
    }

    /// One epic of the organization by name; `not_found` when it has none of that name.
    #[plexus_macros::method(group = "read", example(note = "the first epic", params = r#"{"name": "genesis"}"#))]
    pub async fn get(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The epic, e.g. genesis.
        name: EpicName,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::read(self.store.clone(), user, org, move |store, org, me| Ok(vec![EpicsEvent::Epic { epic: store.epic(org, &name, me)? }]))
    }

    /// The epic you are in, in the organization — your choice (switch), else its newest; yields current { epic }. A filtered read answers there unless its filter names epic (epic any: every one).
    #[plexus_macros::method(group = "read")]
    pub async fn current(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::read(self.store.clone(), user, org, |store, org, me| Ok(vec![EpicsEvent::Current { epic: store.current_epic(org, me)? }]))
    }

    /// Be in another epic of the organization (your own setting): short forms resolve there first, and filtered reads answer there. Yields current { epic }.
    #[plexus_macros::method(group = "write", example(note = "start reading a new epic", params = r#"{"name": "fresh"}"#))]
    pub async fn switch(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The epic, e.g. fresh.
        name: EpicName,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::switch(self.store.clone(), user, org, name)
    }

    /// Make an epic in the organization, empty (admin on it, or on *): its shortcodes start at 1. Move tickets in with tickets reshortcode --to org:epic:CODE; create in it with create --epic org:epic:CODE. Yields created { epic }.
    #[plexus_macros::method(group = "admin", example(note = "a fresh epic", params = r#"{"name": "fresh"}"#))]
    pub async fn create(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Its name: lower-case letters, digits and -, unique in the organization, e.g. fresh.
        name: EpicName,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::admin(self.store.clone(), user, org, move |store, by, org, me| async move {
            store.create_epic(by, &org, &name, &me).await.map(|epic| EpicsEvent::Created { epic })
        })
    }

    /// Rename an epic (admin on its organization, or on *): every address naming it moves to the new name. Yields renamed { from, epic }.
    #[plexus_macros::method(group = "admin")]
    pub async fn rename(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The epic now, e.g. fresh.
        name: EpicName,
        /// Its new name, unique in the organization.
        to: EpicName,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::admin(self.store.clone(), user, org, move |store, by, org, me| async move {
            store.rename_epic(by, &org, &name, &to, &me).await.map(|epic| EpicsEvent::Renamed { from: name, epic })
        })
    }

    /// Delete an empty epic (admin on its organization, or on *) — never the organization's first; whoever was in it is in the newest again. Yields deleted { org, name }.
    #[plexus_macros::method(group = "admin")]
    pub async fn delete(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The epic, e.g. fresh.
        name: EpicName,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::admin(self.store.clone(), user, org, move |store, by, org, _| async move {
            store.delete_epic(by, &org, &name).await.map(|(org, name)| EpicsEvent::Deleted { org, name })
        })
    }
}
