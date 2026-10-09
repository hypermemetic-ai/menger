//! menger's tickets activation — the vocabulary, the activation, and the
//! client that speaks it, from **one** declaration (MFE-9, MFE-11).
//!
//! | Consumer | Features | Gets |
//! |---|---|---|
//! | `menger` (the server binary) | *(default: `server`)* | [`types`], [`store`], [`Tickets`] — the activation with its real bodies — and [`TicketsClient`]; COMMENT-2's [`comments`], [`Comments`] and [`CommentsClient`] beside them |
//! | `menger-app` (wasm32) | `default-features = false` | [`types`] and [`TicketsClient`] over four wasm-safe crates: no plexus-core, tokio, jsonrpsee or rusqlite; [`comments`] and [`CommentsClient`] the same way |
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
//! let mut page = client.list(None, None, None, None, Some(500), Some(true), None, None);
//! # let _ = &mut page;
//! # Ok(())
//! # }
//! ```

pub mod types;
/// COMP-8: a component's help — its source's `<docs>` and its block's
/// preview line, parsed. Wasm-safe: the store reads definitions with it, and
/// the app renders a definition's card (COMP-9) from the block it shows.
pub mod component_docs;
/// ART-5: a body's fields — its elements marked `data-field`, or a legacy
/// body's `##` headings read through ought's field set — the piece COMPILE
/// checks. Wasm-safe, like `component_docs`.
pub mod fields;
/// INPUT-3: a body's inputs — `<m-decision id>` and `<m-input id kind>` with
/// their options, question and recommendation — read as the app renders the
/// body, and an answer's value checked against them. Wasm-safe, like
/// `fields`, whose scan it reads with.
pub mod inputs;
/// FILTER-10: the filter language — manifest's view syntax with menger's
/// fields: parse, format, the lens's `and`, contradiction, and the
/// reference evaluator. Wasm-safe, with no feature gate: the server
/// compiles it against its cells, the app parses what a reader types.
pub mod filter;
pub use types::{
    is_menger_doc, is_menger_path, path_address, Answer, AnswerSource, AttrDoc, AttrKind, AttrState, Component, ComponentDocs, Group, Input,
    InputAction, InputHistoryEntry, InputKind, InputOption, InputState, InputValue, KnowNothing, Known, SlotDoc, Kind, Placement,
    PlacementChange, RootRow, Severity, Status, Ticket, TicketSummary, TicketsEvent, MENGER_PATH_PREFIX,
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

/// COMMENT-2: the comments vocabulary — [`Anchor`], [`Comment`],
/// [`CommentState`], [`CommentsEvent`] — and the rules `add` keeps them by.
/// Wasm-safe, like [`types`].
pub mod comments;
pub use comments::{Anchor, Comment, CommentState, CommentsEvent};
/// COMMENT-2: the `comments` activation, declared once as `tickets` is: the
/// server's [`Comments`] and the browser's [`CommentsClient`].
pub mod comments_activation;
pub use comments_activation::CommentsClient;
#[cfg(feature = "server")]
pub use comments_activation::Comments;
