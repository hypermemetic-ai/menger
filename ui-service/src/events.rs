//! UI-3: what the `ui` activation streams — [`UiEvent`], tagged by `type`,
//! a refusal in band.

use menger_tickets_api::WatchEnd;
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
    /// The manifest is not the contract's JSON, or names no `contract` or no `abi`.
    BadManifest,
    /// The version named is not kept (or, a set naming none, nothing is live).
    UnknownVersion,
    /// UI-9: the component named is not in the version's manifest.
    UnknownComponent,
    /// UI-9: the slot named is not declared for the layout by the live
    /// version's manifest (`slots`).
    UnknownSlot,
    /// UI-7: a component's source (`src/<name>.svelte`) did not compile in
    /// the server's sealed compiler — refused at a line, or out of fuel or
    /// memory; `name` and `line` say where.
    CompileFailed,
    /// UI-7: the bundle's `components/<name>.json` is not, byte for byte,
    /// the tree the server compiled from its source; `name` says which.
    CompileMismatch,
    /// UI-7: `manifest.components.<name>.uses` names a `method` the running
    /// tickets service does not expose.
    UnknownMethod,
    /// UI-7: the push's `manifest` or the bundle's `manifest.json` does not
    /// say, for the component `name`, the `uses` and the `tree` / `handlers`
    /// hashes of what compiled — the compiled tree is the truth.
    ManifestMismatch,
}

/// Every event the `ui` activation sends (`{"type": …}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    /// `push`: the version kept (`new: false` when the same bytes were kept
    /// already) and the pointer on it. UI-7: `contract_matches` — the
    /// manifest's `contract` is the running tickets service's hash (a
    /// difference is said, never refused: the support policy's, PLEXUS-1);
    /// `compiled` — the server compiled every source (`false`: no
    /// `--ui-compiler`, the trees stored as given).
    /// `uploaded` — the upload's sha256 (what the signature is over): the
    /// `version` when stored exactly as signed, another when the server
    /// filled in the trees.
    Pushed { version: String, uploaded: String, live: bool, new: bool, contract_matches: bool, compiled: bool },
    /// `rollback`: the version the pointer names now. UI-9: and on
    /// `ui.watch` — first, the pointer as it stands (`null` when nothing is
    /// live), then on every pointer move (a push, a rollback).
    Live { version: Option<String> },
    /// UI-9 `ui.slots.set`: the assignment written — `version` the stored
    /// hash it names (the live one when none was given).
    Set { layout: String, slot: String, component: String, version: String },
    /// UI-9 `ui.slots.clear`: the assignment removed; the section shows its
    /// default again — the component the live manifest's `slots` names for
    /// it, in the live version (`null` when it names none, or none is live).
    Cleared { layout: String, slot: String, component: Option<String>, version: Option<String> },
    /// UI-9 `ui.slots.list`: one per assignment, with the default it
    /// overrides (the live manifest's `slots[layout][slot]`; `null`: none).
    Assignment { layout: String, slot: String, component: String, version: String, default: Option<String> },
    /// UI-9 `ui.watch`: what a section shows now, on every set or clear —
    /// on a clear, its default (`null`s when it has none).
    SlotChanged { layout: String, slot: String, component: Option<String>, version: Option<String> },
    /// UI-9 `ui.watch`: the watch ends, as the tickets watch does (ACCESS-12)
    /// — signed out, expired, revoked or refused; nothing after it.
    WatchClosed { reason: WatchEnd, message: String },
    /// UI-9 `ui.watch`: the watch fell behind by `missed` changes; read
    /// `versions` and `slots.list` again.
    Lagged { missed: u64 },
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
    /// Refused in band, naming why; nothing written. UI-7: a component's
    /// refusal names it (`name`), a compile's its `line`, an unknown
    /// method the `method`; absent otherwise.
    Refused {
        reason: Reason,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        method: Option<String>,
    },
    /// Anything else that went wrong; nothing written.
    Error { message: String },
}

impl UiEvent {
    /// A refusal naming no component: `refused { reason, message }`.
    pub fn refused(reason: Reason, message: impl Into<String>) -> Self {
        UiEvent::Refused { reason, message: message.into(), name: None, line: None, method: None }
    }
}

/// The emitted `UiClient`'s transport failures, in band.
impl plexus_client::FromClientError for UiEvent {
    fn from_client_error(error: plexus_client::ClientError) -> Self {
        UiEvent::Error { message: error.to_string() }
    }
}
