//! menger — a plan is a program whose leaves are prose.
//!
//! Plexus RPC backend for tickets, and the seed of a Plexus-native
//! reactive web framework: the same event streams that answer RPC calls
//! drive the browser shell.

mod builder;

pub mod activations;
pub mod shell;
pub mod store;

pub use builder::{build_rpc, default_db_path};
