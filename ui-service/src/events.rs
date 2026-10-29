//! UI-3: what the `ui` activation streams — [`UiEvent`], tagged by `type`,
//! a refusal in band.

use serde::{Deserialize, Serialize};

/// Why a push, a rollback or a preview is refused in band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// No `--ui-signer` was named at start: every push is refused.
    NoSigner,
    /// The signature does not verify over the bundle's digest by any signer.
    BadSignature,
    /// The bundle is not a tar archive with an `index.html`.
    BadBundle,
    /// The manifest names no `contract` or no `abi`.
    BadManifest,
    /// The version named is not kept.
    UnknownVersion,
}

/// Every event the `ui` activation sends (`{"type": …}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    /// `push`: the version kept (`new: false` when the same bytes were kept
    /// already) and the pointer on it.
    Pushed { version: String, live: bool, new: bool },
    /// `rollback`: the version the pointer names now.
    Live { version: String },
    /// `versions`: one per kept version, newest first.
    Version {
        hash: String,
        commit: Option<String>,
        contract: String,
        abi: String,
        pushed_by: String,
        pushed_at: i64,
        live: bool,
    },
    /// `preview`: the caller's preview — `version: null` when cleared.
    Preview { principal: String, version: Option<String> },
    /// Refused in band, naming why; nothing written.
    Refused { reason: Reason, message: String },
    /// Anything else that went wrong; nothing written.
    Error { message: String },
}

/// The emitted `UiClient`'s transport failures, in band.
impl plexus_client::FromClientError for UiEvent {
    fn from_client_error(error: plexus_client::ClientError) -> Self {
        UiEvent::Error { message: error.to_string() }
    }
}
