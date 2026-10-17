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

/// SCAFFOLD-3: menger's side of plexus-login's seam — `MengerAccess`
/// (the `LoginAccess` impl over `Access`), `SessionTable`, `offer`.
pub mod login_access;
pub use login_access::{login_names, offer, session_table, MengerAccess, SessionTable};

/// SCAFFOLD-3: the login lives in plexus-login; these keep its paths
/// (`menger::login::Provider`, `menger::sessions::Gate`, …), its generic
/// types named for menger's rules.
pub use plexus_login::{login, sessions};

/// AUTH-7: the protocol's `auth` activation's backend, over menger's rules.
pub mod auth_backend {
    pub use plexus_login::auth_backend::*;
    pub type MengerAuth = plexus_login::auth_backend::LoginAuth<crate::MengerAccess>;
}

/// IDENTITY-35: the dev sign-in, in menger's roles.
pub mod dev_login {
    pub use plexus_login::dev_login::*;
    pub type DevUser = plexus_login::dev_login::DevUser<crate::MengerAccess>;
    pub type DevLogin = plexus_login::dev_login::DevLogin<crate::MengerAccess>;
}

/// IDENTITY-19: the login page and its routes, over menger's rules.
pub mod web_login {
    pub use plexus_login::web_login::*;
    pub type Login = plexus_login::web_login::Login<crate::MengerAccess>;
    pub type LoginConfig = plexus_login::web_login::LoginConfig<crate::MengerAccess>;
}

/// SCAFFOLD-9: the login wired in two calls (`main` runs on it), over
/// menger's rules.
pub mod wiring {
    pub use plexus_login::wiring::*;
    pub type Wired = plexus_login::wiring::Wired<crate::MengerAccess>;
}

pub mod shell;

/// RUN-8: `--bind`'s address and the names a listener there answers to;
/// the builder binds it (TRANSPORT-7).
pub mod bind;

/// ACCESS-29: `menger.call` held to the calls one connection may have open,
/// and a connection idle with none closed.
pub mod calls;

/// DEPLOY-25: the webhook worker — each deploy request POSTed, signed, to
/// its environment's CI, off the store's write path.
pub mod webhooks;

/// DIST-3: the whole server started from code, without login or REST —
/// `LocalServer::start(LocalConfig)`.
pub mod local;

/// The cache and its storage, re-exported from the activation crate so
/// `menger::store::SeedSpec` and friends keep their paths.
pub use menger_tickets_api::store;
pub use menger_tickets_api::{ChatsActivation, ChatsClient, CommentsActivation, CommentsClient, MembersActivation, MembersClient, SessionActivation, SessionClient, TicketsActivation, TicketsClient};

pub use builder::{build_rpc, build_rpc_login, build_rpc_offering, build_rpc_seeded, build_rpc_with, default_db_path, SIGNED_IN_GRANTS};
