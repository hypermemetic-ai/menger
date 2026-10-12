//! plexus-login — menger's login, lifted as it is (SCAFFOLD-3).
//!
//! | module | what it is |
//! | --- | --- |
//! | [`login`] | the `--auth-provider` specs, `--login-button`s, and the adapter chain at the WebSocket upgrade |
//! | [`sessions`] | the session table and [`sessions::Gate`], the upgrade's validator |
//! | [`web_login`] | the login page and its routes ([`web_login::router`]) |
//! | [`dev_login`] | the test environment's sign-in as anyone, through plexus-idp |
//! | [`auth_backend`] | the protocol's `auth` activation's backend ([`auth_backend::LoginAuth`]) |
//! | [`access`] | the seam: [`LoginAccess`], what the app implements |
//!
//! Every type that touches the app's rules is generic over `A: LoginAccess`.

pub mod access;
pub mod auth_backend;
pub mod dev_login;
pub mod login;
pub mod sessions;
pub mod web_login;

pub use access::{Branding, LoginAccess};
