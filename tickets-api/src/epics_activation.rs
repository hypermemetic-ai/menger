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
//! description a `///` doc. `list`, `get`, `current`, `switch`, `dismiss`
//! and `watch` are a viewer's — `switch` and `dismiss` write the caller's
//! own setting, held on the organization; `create`, `rename`, `delete`,
//! `archive`, `restore` and `set_default` (ORG-23) are unlisted, so admin's,
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
/// Epics: a level of the organization that holds shortcodes; a new one starts fresh, its shortcodes numbered from 1. Each person stays on their own epic (the org's default until they switch); list, choose yours, and (admin) create, rename, archive, restore, delete.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(
        viewer = ["list", "get", "current", "switch", "dismiss", "watch"],
        editor = [viewer],
    )
)]
impl EpicsActivation {
    /// The organization's live epics, oldest first: one `epic { org, name, first, default, current, members_on, shortcodes, tickets, created_unix, archived? }` each (--archived true: the archived ones too).
    #[plexus_macros::method(group = "read")]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
        /// List the archived epics too (optional; default false).
        archived: Option<bool>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        let all = archived.unwrap_or(false);
        streams::read(self.store.clone(), user, org, move |store, org, me| Ok(store.epics(org, me, all)?.into_iter().map(|epic| EpicsEvent::Epic { epic }).collect()))
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

    /// The epic you are in, in the organization — your choice (switch), else its default; yields current { epic, moved? } (moved: an archive moved you here, until dismiss). A filtered read answers there unless its filter names epic (epic any: every live one).
    #[plexus_macros::method(group = "read")]
    pub async fn current(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::read(self.store.clone(), user, org, |store, org, me| Ok(vec![EpicsEvent::Current { epic: store.current_epic(org, me)?, moved: store.epic_move(org, me) }]))
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

    /// Delete an empty epic (admin on its organization, or on *) — never the organization's first; whoever was in it is on the default again. Yields deleted { org, name }.
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

    /// Archive an epic (admin on its organization, or on *), in one write: everyone on it moves to the next newer epic (else the next older), the default too if it was this one; its tickets leave every view unless a filter names the epic or archived. The last live epic is refused. Yields archived { epic, moved, to, tickets, dry_run }.
    #[plexus_macros::method(group = "admin", example(note = "what archiving spring would do", params = r#"{"name": "spring", "dry_run": true}"#))]
    pub async fn archive(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The epic, e.g. spring.
        name: EpicName,
        /// Answer what it would do and write nothing (optional; default false).
        dry_run: Option<bool>,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        let dry = dry_run.unwrap_or(false);
        streams::admin(self.store.clone(), user, org, move |store, by, org, me| async move {
            let a = store.archive_epic(by, &org, &name, dry, &me).await?;
            Ok(EpicsEvent::Archived { epic: a.epic, moved: a.moved, to: a.to, tickets: a.tickets, dry_run: dry })
        })
    }

    /// Restore an archived epic (admin on its organization, or on *): back in the picker and the views; nobody moves. Yields restored { epic }.
    #[plexus_macros::method(group = "admin")]
    pub async fn restore(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The epic, e.g. spring.
        name: EpicName,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::admin(self.store.clone(), user, org, move |store, by, org, me| async move {
            store.restore_epic(by, &org, &name, &me).await.map(|epic| EpicsEvent::Restored { epic })
        })
    }

    /// Make an epic the organization's default (admin on it, or on *) — where everyone who never switched is. Yields default_set { epic }.
    #[plexus_macros::method(group = "admin")]
    pub async fn set_default(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The epic, e.g. fresh.
        name: EpicName,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::admin(self.store.clone(), user, org, move |store, by, org, me| async move {
            store.set_default_epic(by, &org, &name, &me).await.map(|epic| EpicsEvent::DefaultSet { epic })
        })
    }

    /// Dismiss the notice that an archive moved you (your own setting): current stops carrying moved. Yields dismissed { org }.
    #[plexus_macros::method(group = "write")]
    pub async fn dismiss(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The organization, e.g. hypermemetic (optional: your own).
        org: Option<OrgId>,
    ) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::dismiss(self.store.clone(), user, org)
    }

    /// Follow your organizations' epics, open-ended: epic_changed { org, name, what } (created, renamed, deleted, archived, restored, default_moved), and moved { org, moved } when an archive moves you; lagged { missed }; watch_closed { reason, message } when your grants end it.
    #[plexus_macros::method(group = "read", streaming)]
    pub async fn watch(&self, #[from_auth(self.access.verify)] user: VerifiedUser) -> impl Stream<Item = EpicsEvent> + Send + 'static {
        streams::watch(self.access.clone(), self.store.clone(), user)
    }
}
