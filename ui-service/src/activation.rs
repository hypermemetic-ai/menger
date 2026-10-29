//! UI-3: the `ui` activation — `push`, `rollback`, `versions`, `preview` —
//! on the frame (ACCESS-13): `auth = "required"`, `request = MengerRequest`,
//! `typed_params`, every method's caller the `VerifiedUser` the gate's
//! resolver answers. `push`, `rollback` and `preview` are admin's alone
//! (unlisted in `roles`, so only `admin`, which holds every grant, holds
//! them); `versions` is a viewer's.

use crate::events::UiEvent;
use crate::signers::Signers;
use crate::store::UiStore;
use crate::vocab::{Bundle, Manifest, Signature, VersionHash};
use futures::Stream;
use menger_tickets_api::access::{Access, MengerRequest, VerifiedUser};
use menger_tickets_api::vocab::CommitRef;
use std::sync::Arc;

/// The activation: the store over `ui.db`, the signers named at start, and
/// the rule every caller is verified by.
#[derive(Clone)]
pub struct UiActivation {
    store: Arc<UiStore>,
    signers: Arc<Signers>,
    access: Access,
}

impl UiActivation {
    /// Over `store`, accepting pushes signed by `signers`, behind `access`.
    pub fn new(store: Arc<UiStore>, signers: Signers, access: Access) -> Self {
        Self { store, signers: Arc::new(signers), access }
    }

    /// The store it keeps.
    pub fn store(&self) -> &Arc<UiStore> {
        &self.store
    }
}

/// UI-3 phase 1: every method answers this until the build.
fn unimplemented(method: &str) -> impl Stream<Item = UiEvent> + Send + 'static {
    futures::stream::once(std::future::ready(UiEvent::Error { message: format!("ui.{method}: unimplemented (UI-3 phase 1)") }))
}

/// The UI's versions by content hash and the live pointer — push one, roll back to one, list them, preview one.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(
        viewer = ["versions"],
        editor = [viewer],
    )
)]
impl UiActivation {
    /// Push a UI: its bundle (a tar, base64), manifest and signature; kept once per hash and made live in one transaction; yields pushed { version, live, new }.
    ///
    /// The bundle is hashed (sha256 of its bytes); the signature must be
    /// Ed25519 over that digest by a signer named at start (`--ui-signer`).
    /// The same bytes pushed again are the same version (`new: false`).
    /// The row — with its commit, the manifest's `contract` and `abi`, who
    /// and when — and the live pointer move in one transaction. Refused in
    /// band (`refused { reason }`: `no_signer`, `bad_signature`,
    /// `bad_bundle`, `bad_manifest`) with nothing written. Admin only.
    #[plexus_macros::method(group = "write")]
    pub async fn push(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The bundle: a tar archive whose index.html is the entrypoint, standard base64, at most 16 MiB.
        bundle: Bundle,
        /// The bundle's manifest, as text (its contract: and abi: lines are kept).
        manifest: Manifest,
        /// Ed25519 over the bundle's sha256 digest by a --ui-signer, 64 bytes in base64.
        signature: Signature,
        /// The commit the bundle was built from, e.g. 77df9a0 (optional).
        commit: Option<CommitRef>,
    ) -> impl Stream<Item = UiEvent> + Send + 'static {
        let _ = (user, bundle, manifest, signature, commit, &self.store, &self.signers);
        unimplemented("push")
    }

    /// Move the live pointer to a kept version; no other row changes; yields live { version }.
    ///
    /// Refused in band (`refused { reason: unknown_version }`) for a hash
    /// not kept, and the pointer stays. Admin only.
    #[plexus_macros::method(group = "write")]
    pub async fn rollback(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The version to make live: its hash, as push answered it.
        to: VersionHash,
    ) -> impl Stream<Item = UiEvent> + Send + 'static {
        let _ = (user, to);
        unimplemented("rollback")
    }

    /// Every kept version, newest first; yields one version { hash, commit, contract, abi, pushed_by, pushed_at, live } each.
    ///
    /// Exactly one is `live: true` when any is live; none at all when the
    /// file holds no version.
    #[plexus_macros::method(group = "read")]
    pub async fn versions(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
    ) -> impl Stream<Item = UiEvent> + Send + 'static {
        let _ = user;
        unimplemented("versions")
    }

    /// Preview a kept version as the caller: /app/ serves it to the caller's sessions alone; no version clears it; yields preview { principal, version }.
    ///
    /// Nobody else's page changes, and no pointer moves. Refused in band
    /// (`unknown_version`) for a hash not kept. Admin only.
    #[plexus_macros::method(group = "write")]
    pub async fn preview(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The version to preview, by hash (optional: none clears the caller's preview).
        version: Option<VersionHash>,
    ) -> impl Stream<Item = UiEvent> + Send + 'static {
        let _ = (user, version);
        unimplemented("preview")
    }
}
