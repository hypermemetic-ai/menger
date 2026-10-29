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

pub mod activation;
pub use activation::{SlotsActivation, SlotsClient, UiActivation};
