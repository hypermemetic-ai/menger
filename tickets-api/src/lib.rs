//! menger's tickets activation — the vocabulary, the activation, and the
//! client that speaks it, from **one** declaration (MFE-9, MFE-11).
//!
//! | Consumer | Features | Gets |
//! |---|---|---|
//! | `menger` (the server binary) | *(default: `server`)* | [`types`], [`store`], [`Tickets`] — the activation with its real bodies — and [`TicketsClient`]; COMMENT-2's [`comments`], [`Comments`] and [`CommentsClient`] beside them; IDENTITY-14's [`session`], [`Session`] and [`SessionClient`] |
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
//! let mut page = client.list(None, None, None, None, Some(500), Some(true), None, None, None, None);
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
/// COMP-11: a blank line inside a body's HTML does not end it — an element
/// opened in an HTML block runs to its close — read for the app's parser.
/// Wasm-safe, like `component_docs`: the frame renders with it.
pub mod html_runs;
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
/// MENGER-12: a ticket's labels — what a label is, and how `label` moves a
/// ticket's set. Wasm-safe: the app checks what a reader types with it.
pub mod labels;
pub use types::{
    is_menger_doc, is_menger_path, path_address, Answer, AnswerSource, AttrDoc, AttrKind, AttrState, Component, ComponentDocs, Group, Input,
    InputAction, InputHistoryEntry, InputKind, InputOption, InputState, InputValue, KnowNothing, Known, SlotDoc, Kind, Marked, Placement,
    PlacementChange, RootRow, SearchWord, Severity, Status, Ticket, TicketSummary, TicketsEvent, MENGER_PATH_PREFIX,
};
// MENGER-13: the transition log's vocabulary.
pub use types::{Transition, TransitionField, TransitionSource, ANONYMOUS_ACTOR, SYSTEM_ACTOR};

/// IDENTITY-19: the rule every method asks first — who a call acts as, or
/// why it is refused. Server-only.
#[cfg(feature = "server")]
pub mod access;

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

/// NOTE-2: the notes vocabulary — [`Note`], [`NoteState`], [`NotesEvent`]
/// — the rules `add` keeps a note by, and the title a note gives its
/// ticket. Wasm-safe, like [`comments`].
pub mod notes;
pub use notes::{Note, NoteState, NotesEvent};
/// NOTE-2: the `notes` activation, declared once: the server's [`Notes`]
/// and the browser's [`NotesClient`].
pub mod notes_activation;
pub use notes_activation::NotesClient;
#[cfg(feature = "server")]
pub use notes_activation::Notes;

/// THEME-2: the token contract — every token menger draws with, its group
/// and its defaults, read from `tokens.toml` ([`tokens::TOKENS`]), and
/// index.html's `:root` blocks as it says them ([`tokens::root_css`]).
pub mod tokens;
/// THEME-8: the themes vocabulary — [`Theme`], the levels that adopt one,
/// the tokens a theme may set ([`themes::TOKENS`]) and the rule that decides
/// who wears what ([`themes::resolve`]). Wasm-safe: the app resolves with it.
pub mod themes;
pub use themes::{Theme, ThemesEvent};
/// THEME-8: the `themes` activation, declared once: the server's [`Themes`]
/// and the browser's [`ThemesClient`].
pub mod themes_activation;
pub use themes_activation::ThemesClient;
#[cfg(feature = "server")]
pub use themes_activation::Themes;

/// IDENTITY-21: the members vocabulary — [`Role`], [`Grant`],
/// [`MembersEvent`]. Wasm-safe, like [`comments`].
pub mod members;
pub use members::{Grant, MembersEvent, Role};
/// IDENTITY-21: the `members` activation, declared once: the server's
/// [`Members`] and the browser's [`MembersClient`].
pub mod members_activation;
pub use members_activation::MembersClient;
#[cfg(feature = "server")]
pub use members_activation::Members;

/// IDENTITY-14: who a connection is — [`SessionEvent`], wasm-safe like
/// [`comments`].
pub mod session;
pub use session::{ProviderKind, ProviderOffer, SessionEvent};
/// IDENTITY-14: the `session` activation, declared once: the server's
/// [`Session`] and the browser's [`SessionClient`].
pub mod session_activation;
pub use session_activation::SessionClient;
#[cfg(feature = "server")]
pub use session_activation::Session;
