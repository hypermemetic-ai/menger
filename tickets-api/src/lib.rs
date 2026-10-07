//! menger's tickets activation — the vocabulary, the activation, and the
//! client that speaks it, from **one** declaration (MFE-9, MFE-11).
//!
//! | Consumer | Features | Gets |
//! |---|---|---|
//! | `menger` (the server binary) | *(default: `server`)* | [`types`], [`store`], [`Tickets`] — the activation with its real bodies — and [`TicketsClient`] |
//! | `menger-app` (wasm32) | `default-features = false` | [`types`] and [`TicketsClient`] over four wasm-safe crates: no plexus-core, tokio, jsonrpsee or rusqlite |
//!
//! The client is emitted by `#[plexus_macros::activation(client, …)]` on
//! the activation's own `impl`, so its parameters and events *are* the
//! server's: a renamed field or parameter is a compile error in the
//! browser, never a runtime surprise. The `server_cfg` flag keeps the
//! server half of that expansion behind the `server` feature.
//!
//! ```no_run
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use menger_tickets_api::{TicketsClient, TicketsEvent};
//! use plexus_client::WsTransport;
//!
//! let transport = WsTransport::connect("ws://127.0.0.1:4455", "menger").await?;
//! let client = TicketsClient::new(&transport);
//! let mut page = client.list(None, None, None, None, Some(500), Some(true));
//! # let _ = &mut page;
//! # Ok(())
//! # }
//! ```

pub mod types;
pub use types::{
    is_menger_path, path_address, AttrDoc, AttrKind, Component, ComponentDocs, Group, SlotDoc, Kind, Placement, PlacementChange, RootRow, Severity, Status, Ticket, TicketSummary,
    TicketsEvent, MENGER_PATH_PREFIX,
};

/// The cache and its storage — the server's system of record (MNG-2).
#[cfg(feature = "server")]
pub mod store;
#[cfg(feature = "server")]
pub use store::Store;

/// MNG-10, test-only: AGS-20's nav `plan()`, transcribed — the oracle the
/// store's roll-ups are held equal to.
#[cfg(test)]
pub(crate) mod nav_plan;

pub mod activation;
pub use activation::TicketsClient;
#[cfg(feature = "server")]
pub use activation::Tickets;
