//! menger — a plan is a program whose leaves are prose.
//!
//! Plexus RPC backend for tickets, and the seed of a Plexus-native
//! reactive web framework: the same event streams that answer RPC calls
//! drive the browser shell.
//!
//! MFE-11: the tickets vocabulary, the store and the `tickets` activation
//! live in `menger-tickets-api` — the crate the browser app depends on
//! too, with the server half off — so there is exactly one declaration of
//! every method and event. This crate is the binary's half: the transport
//! wiring, the shell, and the builder that opens the store and registers
//! the activation.

mod builder;

pub mod dev_login;
pub mod login;
pub mod sessions;
pub mod web_login;

pub mod shell;

/// The cache and its storage, re-exported from the activation crate so
/// `menger::store::SeedSpec` and friends keep their paths.
pub use menger_tickets_api::store;
pub use menger_tickets_api::{Comments, CommentsClient, Members, MembersClient, Session, SessionClient, Tickets, TicketsClient};

pub use builder::{build_rpc, build_rpc_offering, build_rpc_seeded, build_rpc_with, default_db_path};
