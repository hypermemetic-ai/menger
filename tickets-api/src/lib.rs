//! menger's tickets API — the vocabulary, and the client that speaks it.
//!
//! MFE-9 split this out of the server so the browser app can consume the
//! *same declarations* the server does instead of a hand-written mirror
//! of the JSON they arrive as. Two profiles:
//!
//! | Consumer | Features | Gets |
//! |---|---|---|
//! | `menger` (the server) | *(default)* | [`types`] — `Ticket`, `Status`, `Kind`, `Severity`, `TicketSummary`, `TicketsEvent` |
//! | `menger-app` (wasm32) | `client` | the same types **plus** `MengerTicketsClient<T>` |
//!
//! The server re-exports the types from `menger::store` and its tickets
//! activation, so nothing on that side changed name. The browser app reads
//! them directly — which is what makes a renamed server field a wasm
//! compile error rather than a blank cell.
//!
//! ```no_run
//! # #[cfg(feature = "client")]
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use menger_tickets_api::{MengerTicketsClient, TicketsEvent};
//! use plexus_client::WsTransport;
//!
//! let transport = WsTransport::connect("ws://127.0.0.1:4455", "menger").await?;
//! let client = MengerTicketsClient::new(&transport);
//! let mut page = client.list(None, None, None, None, Some(500), Some(true));
//! # let _ = &mut page;
//! # Ok(())
//! # }
//! ```

pub mod types;

pub use types::{Kind, Severity, Status, Ticket, TicketSummary, TicketsEvent};

#[cfg(feature = "client")]
mod client;

#[cfg(feature = "client")]
pub use client::MengerTicketsClient;
