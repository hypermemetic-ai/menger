//! plexus-login — login for a Plexus RPC hub: provider specs, the login
//! page and its routes, sessions, the WebSocket gate and the protocol's
//! auth backend, generic over the app's access rules. menger's login,
//! lifted as it is (SCAFFOLD-3), named for whichever app takes it
//! (SCAFFOLD-9).
//!
//! | module | what it is |
//! | --- | --- |
//! | [`wiring`] | [`Wiring`] / [`Wired`]: the whole login in two calls — the gate, the anonymous view, the page's router, the auth backend |
//! | [`access`] | the seam: [`LoginAccess`], what the app implements; [`Branding`] |
//! | [`simple`] | [`simple::SimpleAccess`]: ready-made in-memory rules, roles global |
//! | [`names`] | [`LoginNames`]: the cookies and the session metadata key, the app's |
//! | [`login`] | the `--auth-provider` specs, `--login-button`s, and the adapter chain at the WebSocket upgrade |
//! | [`sessions`] | the session table and [`sessions::Gate`], the upgrade's validator |
//! | [`web_login`] | the login page and its routes ([`web_login::router`]) |
//! | [`dev_login`] | the test environment's sign-in as anyone, through plexus-idp |
//! | [`auth_backend`] | the protocol's `auth` activation's backend ([`auth_backend::LoginAuth`]) |
//!
//! Every type that touches the app's rules is generic over `A: LoginAccess`.
//! README.md has the walk-through.

pub mod access;
pub mod auth_backend;
pub mod dev_login;
pub mod login;
pub mod names;
pub mod sessions;
pub mod simple;
pub mod web_login;
pub mod wiring;

pub use access::{Branding, LoginAccess};
pub use names::LoginNames;
pub use wiring::{LoginOptions, Wired, Wiring};
