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
//! | [`identity`] | AUTH-22: [`IdentityBackend`] — who signs people in behind our own page, provider-neutral, with [`Capabilities`] |
//! | [`backends`] | its implementations — WorkOS, plexus-idp, memory — and the `--identity` spec that picks one |
//! | [`signin`] | our own sign-in page over any backend: email codes, passwords, social and SSO round trips, TOTP, the account page |
//! | [`app_session`] | what an app keeps once the page signs someone in: its session, the backend's tokens (refreshed, revoked) |
//! | [`throttle`], [`client_ip`], [`crypto`], [`page`] | the page's throttles, its client IP through trusted proxies, HMAC/TOTP, and its chrome and CSP |
//!
//! Every type that touches the app's rules is generic over `A: LoginAccess`.
//! README.md has the walk-through.

pub mod access;
pub mod app_session;
pub mod auth_backend;
pub mod backends;
pub mod client_ip;
pub mod crypto;
pub mod dev_login;
pub mod identity;
pub mod login;
pub mod names;
pub mod page;
pub mod sessions;
pub mod signin;
pub mod simple;
pub mod throttle;
pub mod web_login;
pub mod wiring;

pub use access::{Branding, LoginAccess};
pub use identity::{Capabilities, IdentityBackend, IdentityError};
pub use names::LoginNames;
pub use wiring::{LoginOptions, Wired, Wiring};
