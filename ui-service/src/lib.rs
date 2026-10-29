//! UI-3: **the ui service** — a pushed UI is a row by content hash in this
//! service's own file, and `/app/*` serves whichever row the live pointer
//! names; the pointer is the deploy.
//!
//! | module | what it holds |
//! |---|---|
//! | [`vocab`] | the parameters' types — [`VersionHash`], [`Bundle`], [`Manifest`], [`Signature`] |
//! | [`events`] | [`UiEvent`], what every method streams |
//! | [`signers`] | the signers named at start (`--ui-signer`), and the check of a signature |
//! | [`store`] | [`UiStore`] over `ui.db`: `ui_versions`, `ui_pointer`, `ui_preview`, `ui_slots` (UI-9) — opened by this service alone |
//! | [`compiler`] | UI-7: [`Compiler`], `dist/ui-compiler.wasm` in wasmtime, nothing linked, under fuel and a memory limit |
//! | [`seam`] | UI-7: [`Seam`], the running hub, called as the pusher ([`plexus_core::plexus::HubTransport`]) |
//! | [`activation`] | [`UiActivation`]: `push`, `rollback`, `versions`, `preview`, `watch`; its child [`SlotsActivation`] (`ui.slots.*`, UI-9) |
//!
//! The tickets file is never opened here.

pub mod vocab;
pub use vocab::{Bundle, ComponentName, LayoutName, Manifest, Signature, SlotName, VersionHash};

pub mod events;
pub use events::{Reason, UiEvent};

pub mod signers;
pub use signers::Signers;

pub mod store;
pub use store::{Assignment, ManifestDoc, NewVersion, Pointer, Pushed, Section, UiChange, UiError, UiStore, UiVersion, LIVE, UI_DB_FILE};

// UI-7: the sealed compiler, the seam to the running service, the push's steps.
pub mod compiler;
pub use compiler::{CompileFailed, Compiled, Compiler, ModuleRefused, Session};

pub mod seam;
pub use seam::Seam;

mod push;
pub use push::PushSteps;

pub mod activation;
pub use activation::{SlotsActivation, SlotsClient, UiActivation};
