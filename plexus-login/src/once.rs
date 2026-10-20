//! DIST-14: one-time sign-in links — `GET /login?once=<token>`.
//!
//! An app that runs on loopback with no login provider (menger's first run)
//! still never opens without a sign-in: it prints a link carrying a token,
//! and the login page trades the token, once, for a session as the app's
//! local admin. What a token is, where it is kept and who it signs in is the
//! app's ([`OnceLinks`]); the page only asks.
//!
//! The page asks only when the app gave it [`OnceLinks`] (`LoginOptions::once`)
//! and only for a caller on loopback; anywhere else `?once=` is an unknown
//! query and the plain page is served.

use plexus_core::plexus::AuthContext;

/// The app's one-time links.
pub trait OnceLinks: Send + Sync {
    /// The identity `token` signs in, taking the token: it answers once,
    /// and never after its expiry.
    fn redeem(&self, token: &str) -> Option<AuthContext>;

    /// What the page says to a link that was used, expired or never was —
    /// with the command that prints another.
    fn spent(&self) -> String;

    /// What the page says to anyone who reaches it without a link — how
    /// to get one.
    fn hint(&self) -> String;
}
