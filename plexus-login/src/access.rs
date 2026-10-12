//! SCAFFOLD-3: the seam — what plexus-login asks of the app.
//!
//! The login layer knows who someone is; the app decides what they may do.
//! Everything the login layer needs of that decision goes through one
//! trait, [`LoginAccess`], which the app implements over its own rules
//! (menger: over its `Access` — the members table, the waiting list, the
//! session minting). Nothing else of the app crosses.
//!
//! The other direction — the app's rules asking the session table whether a
//! session stands, minting one, ending one — is [`crate::sessions::Sessions`]'
//! own inherent methods (`alive`, `mint`, `end_key`); the app adapts them to
//! whatever its rules take.

use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
use plexus_core::plexus::{AuthContext, Session, Waiting};

/// The app's access rules, as the login layer reaches them. Cheap to clone
/// (a handle): the login page, the gate and the auth backend each hold one.
#[async_trait]
pub trait LoginAccess: Clone + Send + Sync + 'static {
    // ── the vocabulary: what the dev sign-in's roster and form name ──

    /// A role a grant gives (menger: `viewer`, `editor`, `admin`). Parsed
    /// from a dev user's `role@SCOPE` and the form's role field; its
    /// `Display` is what the page shows and the roster prints.
    type Role: Copy + Eq + fmt::Debug + fmt::Display + FromStr<Err = String> + Send + Sync + 'static;

    /// A grant's scope as the app stores it (menger: `*` or a shortcode,
    /// upper-cased), or why the text is not one.
    fn scope_of(text: &str) -> Result<String, String>;

    // ── the dev sign-in's writes (test only; `/login/dev`) ──

    /// Grants `principal` `role` on `scope`, recorded as the dev sign-in's.
    /// Whoever was waiting for an admin waits no longer.
    async fn dev_grant(&self, principal: &str, scope: &str, role: Self::Role) -> Result<(), String>;

    /// Makes a seeded user's grants exactly `grants`: each kept, every other
    /// grant of theirs revoked.
    async fn dev_set_grants(&self, principal: &str, grants: &[(Self::Role, String)]) -> Result<(), String>;

    // ── the gate (the WebSocket upgrade) ──

    /// A connection opened as `ctx`: holding nothing, it joins the waiting
    /// list for an admin. Called for every identity the gate admits.
    fn notice(&self, ctx: &AuthContext);

    // ── the auth backend (`auth.login`, `auth.signup`, `auth.logout`) ──

    /// Whether a signed-in identity's sign-in still stands (not expired,
    /// its session not signed out). `Err`: why not, in words for the caller.
    fn alive(&self, ctx: &AuthContext) -> Result<(), String>;

    /// `auth.login {session {ttl_secs, ceiling}}`: a session for the
    /// connection's own identity, held to `ceiling` and never wider than the
    /// caller's own.
    fn mint(&self, caller: Option<&AuthContext>, ttl_secs: Option<u64>, ceiling: Option<Vec<String>>) -> Result<Session, String>;

    /// `auth.login {code}`: a session for an identity a browser sign-in
    /// proved (the loopback flow's code), `ttl_secs` long, no ceiling.
    fn mint_for(&self, ctx: AuthContext, ttl_secs: u64) -> Result<Session, String>;

    /// `auth.signup` by someone signed in holding nothing: on the waiting
    /// list.
    fn ask_access(&self, ctx: &AuthContext) -> Result<Waiting, String>;

    /// `auth.waiting`: everyone signed in who holds nothing yet.
    fn waiting(&self) -> Vec<Waiting>;

    /// `auth.logout`: ends the session the caller's connection is on.
    fn logout(&self, caller: Option<&AuthContext>) -> Result<(), String>;
}

/// What the login page says of the app it signs in to.
#[derive(Debug, Clone, Copy)]
pub struct Branding {
    /// The app's name, in the page's own words ("sign in from menger's own
    /// page", "menger did not accept the signature") and its theme key.
    pub app: &'static str,
    /// The app's version, as the page's side column lists it.
    pub version: &'static str,
    /// The brand mark beside the title (menger: `m`).
    pub mark: &'static str,
}

/// A stub of the app's rules for the crate's own unit tests: menger's role
/// vocabulary, nothing kept.
#[cfg(test)]
pub(crate) mod test_access {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TestRole {
        Viewer,
        Editor,
        Admin,
    }

    impl fmt::Display for TestRole {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(match self {
                Self::Viewer => "viewer",
                Self::Editor => "editor",
                Self::Admin => "admin",
            })
        }
    }

    impl FromStr for TestRole {
        type Err = String;
        fn from_str(s: &str) -> Result<Self, String> {
            match s.trim().to_ascii_lowercase().as_str() {
                "viewer" => Ok(Self::Viewer),
                "editor" => Ok(Self::Editor),
                "admin" => Ok(Self::Admin),
                other => Err(format!("unknown role `{other}`")),
            }
        }
    }

    #[derive(Clone)]
    pub struct TestAccess;

    #[async_trait]
    impl LoginAccess for TestAccess {
        type Role = TestRole;
        fn scope_of(text: &str) -> Result<String, String> {
            let t = text.trim();
            if t == "*" {
                return Ok(t.into());
            }
            if !t.is_empty() && t.len() <= 32 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                Ok(t.to_ascii_uppercase())
            } else {
                Err(format!("`{t}` is not a scope"))
            }
        }
        async fn dev_grant(&self, _: &str, _: &str, _: TestRole) -> Result<(), String> {
            Ok(())
        }
        async fn dev_set_grants(&self, _: &str, _: &[(TestRole, String)]) -> Result<(), String> {
            Ok(())
        }
        fn notice(&self, _: &AuthContext) {}
        fn alive(&self, _: &AuthContext) -> Result<(), String> {
            Ok(())
        }
        fn mint(&self, _: Option<&AuthContext>, _: Option<u64>, _: Option<Vec<String>>) -> Result<Session, String> {
            Err("no".into())
        }
        fn mint_for(&self, _: AuthContext, _: u64) -> Result<Session, String> {
            Err("no".into())
        }
        fn ask_access(&self, _: &AuthContext) -> Result<Waiting, String> {
            Err("no".into())
        }
        fn waiting(&self) -> Vec<Waiting> {
            Vec::new()
        }
        fn logout(&self, _: Option<&AuthContext>) -> Result<(), String> {
            Ok(())
        }
    }
}
