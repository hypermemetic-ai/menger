//! UI-3: the `ui` activation — `push`, `rollback`, `versions`, `preview` —
//! on the frame (ACCESS-13): `auth = "required"`, `request = MengerRequest`,
//! `typed_params`, every method's caller the `VerifiedUser` the gate's
//! resolver answers. `push`, `rollback` and `preview` are admin's alone
//! (unlisted in `roles`, so only `admin`, which holds every grant, holds
//! them); `versions` is a viewer's. UI-9: `watch` a viewer's too, and the
//! child `slots` (`ui.slots.list|set|clear`, [`SlotsActivation`]).

use crate::events::{Reason, UiEvent};
use crate::signers::Signers;
use crate::store::{digest_of, NewVersion, UiError, UiStore};
use crate::vocab::{Bundle, Manifest, Signature, VersionHash};
use futures::{Stream, StreamExt};
use menger_tickets_api::access::{Access, MengerRequest, VerifiedUser};
use menger_tickets_api::vocab::CommitRef;
use menger_tickets_api::store::Store;
use std::sync::Arc;

// UI-9: `ui.slots.*`, the child; `ui.watch`'s stream.
mod slots;
pub use slots::{SlotsActivation, SlotsClient};
mod watch;

/// The activation: the store over `ui.db`, the signers named at start, and
/// the rule every caller is verified by.
#[derive(Clone)]
pub struct UiActivation {
    store: Arc<UiStore>,
    signers: Arc<Signers>,
    access: Access,
    /// UI-9: the tickets store, whose grants generation `ui.watch` follows
    /// its caller by (`access::Following`); never read for tickets.
    tickets: Option<Arc<Store>>,
}

impl UiActivation {
    /// Over `store`, accepting pushes signed by `signers`, behind `access`.
    pub fn new(store: Arc<UiStore>, signers: Signers, access: Access) -> Self {
        Self { store, signers: Arc::new(signers), access, tickets: None }
    }

    /// UI-9: `ui.watch` follows its caller through `tickets`' grants (as
    /// `tickets.watch` does); without it, a watch is refused in band.
    pub fn following(mut self, tickets: Arc<Store>) -> Self {
        self.tickets = Some(tickets);
        self
    }

    /// The store it keeps.
    pub fn store(&self) -> &Arc<UiStore> {
        &self.store
    }
}

/// One answer, computed off the async threads (the store is SQLite).
fn answer(work: impl FnOnce() -> UiEvent + Send + 'static) -> impl Stream<Item = UiEvent> + Send + 'static {
    futures::stream::once(async move {
        tokio::task::spawn_blocking(work).await.unwrap_or_else(|e| UiEvent::Error { message: format!("the ui store failed: {e}") })
    })
}

/// The store's refusal, in band: the caller's mistakes `refused`, the
/// file's own failures `error`.
fn refusal(e: UiError) -> UiEvent {
    let reason = match &e {
        UiError::BadBundle(_) => Reason::BadBundle,
        UiError::BadManifest(_) => Reason::BadManifest,
        UiError::UnknownVersion(_) => Reason::UnknownVersion,
        UiError::UnknownComponent { .. } => Reason::UnknownComponent,
        UiError::UnknownSlot { .. } => Reason::UnknownSlot,
        UiError::Db(_) => return UiEvent::Error { message: e.to_string() },
    };
    UiEvent::Refused { reason, message: e.to_string() }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// `push`: a signer named, the signature over the bundle's digest by one of
/// them, then the store's one transaction.
fn push(store: &UiStore, signers: &Signers, by: String, bundle: Bundle, manifest: Manifest, signature: Signature, commit: Option<CommitRef>) -> UiEvent {
    if signers.is_empty() {
        let message = "no --ui-signer is configured on this server: every push is refused".to_string();
        return UiEvent::Refused { reason: Reason::NoSigner, message };
    }
    let bytes = bundle.bytes();
    if !signers.verify(&digest_of(&bytes), &signature.bytes()) {
        let message = "the signature does not verify: Ed25519 over the bundle's sha256 by a --ui-signer".to_string();
        return UiEvent::Refused { reason: Reason::BadSignature, message };
    }
    let v = NewVersion { bundle: bytes, manifest: manifest.into_string(), commit: commit.map(|c| c.to_string()), pushed_by: by, pushed_at: now() };
    match store.push(v) {
        Ok(p) => UiEvent::Pushed { version: p.version, live: p.live, new: p.new },
        Err(e) => refusal(e),
    }
}

/// The UI's versions by content hash and the live pointer — push one, roll back to one, list them, preview one.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(
        viewer = ["versions", "watch"],
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
        let (store, signers, by) = (self.store.clone(), self.signers.clone(), user.principal_text());
        answer(move || push(&store, &signers, by, bundle, manifest, signature, commit))
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
        let _ = user;
        let store = self.store.clone();
        answer(move || match store.rollback(to.as_str()) {
            Ok(version) => UiEvent::Live { version: Some(version) },
            Err(e) => refusal(e),
        })
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
        let store = self.store.clone();
        let listed = async move { tokio::task::spawn_blocking(move || store.versions()).await };
        futures::stream::once(listed).flat_map(|r| {
            let events: Vec<UiEvent> = match r {
                Ok(Ok(rows)) => rows
                    .into_iter()
                    .map(|(v, live)| UiEvent::Version { hash: v.hash, commit: v.commit, contract: v.contract, abi: v.abi, pushed_by: v.pushed_by, pushed_at: v.pushed_at, live })
                    .collect(),
                Ok(Err(e)) => vec![refusal(e)],
                Err(e) => vec![UiEvent::Error { message: format!("the ui store failed: {e}") }],
            };
            futures::stream::iter(events)
        })
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
        let (store, principal) = (self.store.clone(), user.principal_text());
        answer(move || match store.set_preview(&principal, version.as_ref().map(VersionHash::as_str)) {
            Ok(version) => UiEvent::Preview { principal, version },
            Err(e) => refusal(e),
        })
    }

    /// Follow the UI: live { version } on every pointer move (a push, a rollback), slot_changed { layout, slot, component, version } on every set or clear; open-ended, ends with watch_closed { reason, message }.
    ///
    /// Held to its caller as `tickets.watch` is (ACCESS-12): verified again
    /// when the grants move, a sign-out or the expiry comes, and closed
    /// with a typed reason (`signed_out`, `expired`, `revoked`, `refused`).
    /// `lagged { missed }` when it fell behind: read `versions` and
    /// `slots.list` again.
    #[plexus_macros::method(group = "read", streaming)]
    pub async fn watch(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
    ) -> impl Stream<Item = UiEvent> + Send + 'static {
        watch::watch_stream(self.store.clone(), self.access.clone(), self.tickets.clone(), user)
    }

    /// What fills each section of each layout — `ui.slots.list|set|clear` (UI-9).
    #[plexus_macros::child]
    fn slots(&self) -> SlotsActivation {
        SlotsActivation { store: Arc::clone(&self.store), access: self.access.clone() }
    }
}
