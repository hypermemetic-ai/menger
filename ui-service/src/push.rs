//! UI-3 / UI-7: **a push** — the signer checked, then (UI-7) every
//! component's source compiled in the sealed compiler ([`Compiler`]) and
//! compared with the tree the bundle claims (or the tree filled in when the
//! bundle holds none), every `manifest.components.<name>.uses` checked
//! against the running tickets service through the [`Seam`], and the
//! manifest's `contract` compared with that service's hash — all before the
//! store's one transaction (UI-3's) opens.

use crate::compiler::Compiler;
use crate::events::{Reason, UiEvent};
use crate::seam::Seam;
use crate::signers::Signers;
use crate::store::{digest_of, NewVersion, UiError, UiStore};
use crate::vocab::{Bundle, Manifest, Signature};
use menger_tickets_api::vocab::CommitRef;
use plexus_core::plexus::AuthContext;
use std::sync::Arc;

/// What a push runs before the store: the compiler (`--ui-compiler`; none:
/// the trees are stored as given, `compiled: false`) and the seam to the
/// running tickets service.
#[derive(Clone, Debug, Default)]
pub struct PushSteps {
    pub compiler: Option<Arc<Compiler>>,
    pub seam: Seam,
}

/// The push as the activation runs it: who pushed (`by`, and the identity
/// the seam calls tickets as), what, signed how.
pub(crate) struct Push {
    pub by: String,
    pub caller: Option<AuthContext>,
    pub bundle: Bundle,
    pub manifest: Manifest,
    pub signature: Signature,
    pub commit: Option<CommitRef>,
}

/// The store's refusal, in band: the caller's mistakes `refused`, the
/// file's own failures `error`.
pub(crate) fn refusal(e: UiError) -> UiEvent {
    let reason = match &e {
        UiError::BadBundle(_) => Reason::BadBundle,
        UiError::BadManifest(_) => Reason::BadManifest,
        UiError::UnknownVersion(_) => Reason::UnknownVersion,
        UiError::UnknownComponent { .. } => Reason::UnknownComponent,
        UiError::UnknownSlot { .. } => Reason::UnknownSlot,
        UiError::Db(_) => return UiEvent::Error { message: e.to_string() },
    };
    UiEvent::refused(reason, e.to_string())
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// `push`: a signer named, the signature over the bundle's digest by one of
/// them, UI-7's steps, then the store's one transaction.
pub(crate) async fn push(store: Arc<UiStore>, signers: Arc<Signers>, steps: PushSteps, p: Push) -> UiEvent {
    if signers.is_empty() {
        let message = "no --ui-signer is configured on this server: every push is refused";
        return UiEvent::refused(Reason::NoSigner, message);
    }
    let bytes = p.bundle.bytes();
    if !signers.verify(&digest_of(&bytes), &p.signature.bytes()) {
        let message = "the signature does not verify: Ed25519 over the bundle's sha256 by a --ui-signer";
        return UiEvent::refused(Reason::BadSignature, message);
    }
    // UI-7 phase 1: the compile, the comparison, the fill-in, the lint and
    // the contract's hash are not built yet.
    if steps.compiler.is_some() {
        let _ = (&steps.seam, &p.caller);
        return UiEvent::Error { message: "unimplemented: UI-7's push steps (compile, compare, lint) are not built yet".into() };
    }
    let v = NewVersion { bundle: bytes, manifest: p.manifest.into_string(), commit: p.commit.map(|c| c.to_string()), pushed_by: p.by, pushed_at: now() };
    let stored = tokio::task::spawn_blocking(move || store.push(v)).await;
    match stored {
        Ok(Ok(s)) => UiEvent::Pushed { version: s.version, live: s.live, new: s.new, contract_matches: false, compiled: false },
        Ok(Err(e)) => refusal(e),
        Err(e) => UiEvent::Error { message: format!("the ui store failed: {e}") },
    }
}
