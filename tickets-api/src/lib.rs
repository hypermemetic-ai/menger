//! menger's tickets activation — the vocabulary, the activation, and the
//! client that speaks it, from **one** declaration (MFE-9, MFE-11).
//!
//! | Consumer | Features | Gets |
//! |---|---|---|
//! | `menger` (the server binary) | *(default: `server`)* | [`types`], [`store`], [`Tickets`] — the activation with its real bodies — and [`TicketsClient`]; COMMENT-2's [`comments`], [`Comments`] and [`CommentsClient`] beside them; IDENTITY-14's [`session`], [`Session`] and [`SessionClient`] |
//! | `menger-app` (wasm32) | `default-features = false` | [`types`] and [`TicketsClient`] over four wasm-safe crates: no plexus-core, tokio, jsonrpsee or rusqlite; [`comments`] and [`CommentsClient`] the same way |
//!
//! The client is emitted by `#[plexus_macros::activation]` on the
//! activation's own `impl TicketsActivation` (the crate depends on
//! plexus-client), so its parameters and events *are* the server's: a
//! renamed field or parameter is a compile error in the browser, never a
//! runtime surprise. The crate's `server` feature keeps the server half of
//! that expansion behind it (MACRO-13's default).
//!
//! ```no_run
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use menger_tickets_api::{TicketsClient, TicketsEvent};
//! use plexus_client::WsTransport;
//!
//! let transport = WsTransport::connect("ws://127.0.0.1:4455", "menger").await?;
//! let client = TicketsClient::new(&transport);
//! let mut page = client.list(None, None, None, None, Some(500), Some(true), None, None, None, None, None, None);
//! # let _ = &mut page;
//! # Ok(())
//! # }
//! ```

pub mod types;
/// TICKETS-7: line and word diffs, and a body's visible text. Wasm-safe.
pub mod diff;
/// TICKETS-7: a ticket's revisions — the vocabulary. Wasm-safe.
pub mod revisions;
pub use revisions::{FieldChange, Revision, RevisionDiff, RevisionSource};
/// ACCESS-8: the vocabulary — every domain value a method takes, as a type
/// validated once at the wire ([`vocab::TicketRef`], [`vocab::Principal`],
/// [`vocab::FilterText`], …). Wasm-safe.
#[macro_use]
pub mod vocab;
/// V2-20: a body's marked elements, read as the app renders it: v1's reader,
/// bounded. Wasm-safe, like [`inputs`].
pub mod marks;
/// INPUT-3: a body's inputs — `<m-decision id>` and `<m-input id kind>` with
/// their options, question and recommendation — and an answer's value
/// checked against them. Wasm-safe.
pub mod inputs;
/// ORG-3: what names a shortcode and a ticket — [`OrgId`], [`Code`],
/// [`ShortcodeId`], [`Namespace`], [`Shortcode`], [`TicketId`] — and
/// [`Address::parse`], the one parser from text to an address. Wasm-safe.
pub mod address;
pub use address::{Address, At, Code, EpicName, Namespace, OrgId, Shortcode, ShortcodeId, TicketId, TicketName};
/// FILTER-10: the filter language — manifest's view syntax with menger's
/// fields: parse, format, the lens's `and`, contradiction, and the
/// reference evaluator. Wasm-safe, with no feature gate: the server
/// compiles it against its cells, the app parses what a reader types.
pub mod filter;
/// TETHER-11: a machine key's pairing code — what `tether pair` prints and
/// `#/pair` shows, so a person checks one against the other. Wasm-safe.
pub mod pairing;
/// FILTER-36: where a saved filter lives (FILTER-6) and how a name is
/// matched — the slug the app paths its docs by and the server resolves
/// `saved "Name"` by. Wasm-safe: one rule for both.
pub mod saved_filters;
/// ACCESS-29: the sizes a request is held to — a title, a filter's text,
/// `list`'s and `comments.list`'s pages. Wasm-safe.
pub mod limits;
/// MENGER-12: a ticket's labels — what a label is, and how `label` moves a
/// ticket's set. Wasm-safe: the app checks what a reader types with it.
pub mod labels;
/// MENGER-39: the queue — a ticket put in front of the owner with an
/// urgency. Wasm-safe: the store keeps it, the queue view draws it.
pub mod queue;
pub use queue::{Ending, QueueAction, QueueEntry, QueueRecord, Reply, Urgency};
pub use types::{
    Answer, AnswerSource, Input, InputAction, InputHistoryEntry, InputKind, InputOption, InputState, InputValue,
    is_menger_doc, is_menger_path, path_address, Group, Kind, Marked, Placement, PlacementChange, RootRow, SearchWord, Severity, Status, Ticket,
    TicketSummary, TicketsEvent, MENGER_PATH_PREFIX,
};
// MENGER-13: the transition log's vocabulary.
pub use types::{ShortcodeMove, Tombstone, WatchEnd, Transition, TransitionField, TransitionSource, ANONYMOUS_ACTOR, SYSTEM_ACTOR};
// TICKETS-35: what `ambiguous` says of each ticket a name fits.
pub use types::{Choice, ChoiceParent};
pub use vocab::Uid;

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
// ACCESS-16: `tickets nav`, a child of `tickets` (`TicketsClient::nav()`).
pub use activation::NavClient;
pub use activation::FiltersClient;
#[cfg(feature = "server")]
pub use activation::{NavActivation, TicketsActivation};

/// IDENTITY-21: the members vocabulary — [`Role`], [`Grant`],
/// [`MembersEvent`]. Wasm-safe, like [`comments`].
pub mod members;
pub use members::{Grant, MembersEvent, Role};
/// IDENTITY-21: the `members` activation, declared once: the server's
/// [`Members`] and the browser's [`MembersClient`].
pub mod members_activation;
pub use members_activation::{MembersClient, TeamsClient};
#[cfg(feature = "server")]
pub use members_activation::MembersActivation;

/// IDENTITY-14: who a connection is — [`SessionEvent`], wasm-safe like
/// [`comments`].
// ORG-4: organizations, and the activation that makes them.
pub mod orgs;
pub use orgs::{Org, OrgsEvent};
pub mod orgs_activation;
pub use orgs_activation::OrgsClient;
#[cfg(feature = "server")]
pub use orgs_activation::OrgsActivation;

// ORG-13: epics — a level of the organization above its shortcodes: the
// store's vocabulary (the eras a file holds, `org:epic:CODE` addresses).
// V2-25: the `epics` activation that listed, switched and made them went;
// nothing on the wire says these.
pub mod epics;
pub use epics::{Epic, EpicArchived, EpicChange, EpicMove, EpicsEvent};

pub mod session;
pub use session::{ProviderKind, ProviderOffer, SessionEvent};
/// IDENTITY-14: the `session` activation, declared once: the server's
/// [`Session`] and the browser's [`SessionClient`].
pub mod session_activation;
pub use session_activation::SessionClient;
#[cfg(feature = "server")]
pub use session_activation::SessionActivation;

