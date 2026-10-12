//! AUTH-7: menger's half of the protocol's `auth` activation — the
//! [`AuthBackend`] plexus-core's `auth.providers`, `auth.login`,
//! `auth.signup` and `auth.logout` ask (AUTH-5).
//!
//! | flow | what menger does |
//! | --- | --- |
//! | `providers` | each `--auth-provider`: `key` for agents, every other a person's way in through the login page (`browser`) |
//! | `login {browser {redirect}}` | a loopback sign-in: a state bound to `redirect` (`http://127.0.0.1:<port>/…` or `localhost`, nothing else) and the login page's URL, `<public-url>/login?cli=1&state=…`; the person signs in there as anyone does, and the shell sends the browser on to `redirect?code=…&state=…` ([`crate::web_login`]'s `/auth/cli`) |
//! | `login {code {code, state}}` | the code, once, within 60 s, for its state: an `mgs_` session for whoever signed in |
//! | `login {session {ttl_secs, ceiling}}` | IDENTITY-42's `session.login`: a session for the connection's identity (a key proof, or a session), held to the ceiling and never wider than the caller's own |
//! | `login {password}` | refused: the login page takes passwords (`/login?password`) |
//! | `signup` | someone signed in holding nothing joins the waiting list (`auth.waiting`, `members.requests`); anyone else is told what to do |
//! | `logout` | the connection's session ends (its sockets stop answering); a connection a credential opened directly has none, which is not an error |

use std::sync::Arc;

use async_trait::async_trait;
use plexus_core::plexus::{
    AuthBackend, AuthContext, AuthFailure, LoginAnswer, LoginFlow, Provider as AuthProvider, ProviderKind,
    SignupAnswer, Waiting as AuthWaiting,
};

use crate::access::LoginAccess;
use crate::login::Provider;
use crate::sessions::{loopback_redirect, Sessions, SESSION_KEY};

/// The loopback flow's half: the session table its states and codes live
/// in, and where the login page is reached.
#[derive(Clone)]
pub struct BrowserLogin {
    pub sessions: Arc<Sessions>,
    /// The shell's public URL (`--public-url`), the login page's base.
    pub public_url: String,
}

/// The [`AuthBackend`] over the app's rules (menger's: `MengerAuth`). See
/// the module docs.
pub struct LoginAuth<A: LoginAccess> {
    access: A,
    providers: Vec<AuthProvider>,
    browser: Option<BrowserLogin>,
}

impl<A: LoginAccess> LoginAuth<A> {
    /// A backend over `access` (its sessions, waiting list and minting),
    /// offering `providers`; `browser` when there is a login page to sign
    /// a person in through.
    pub fn new(access: impl Into<A>, providers: Vec<AuthProvider>, browser: Option<BrowserLogin>) -> Self {
        Self { access: access.into(), providers, browser }
    }

    /// `auth.providers` from `--auth-provider`s: `key` an agent's, every
    /// other a person's through the login page.
    pub fn offered(providers: &[Provider]) -> Vec<AuthProvider> {
        providers
            .iter()
            .map(|p| AuthProvider {
                name: p.name().to_string(),
                kind: if p.is_key() { ProviderKind::Key } else { ProviderKind::Browser },
                label: None,
            })
            .collect()
    }
}

#[async_trait]
impl<A: LoginAccess> AuthBackend for LoginAuth<A> {
    fn providers(&self) -> Vec<AuthProvider> {
        self.providers.clone()
    }

    async fn login(&self, caller: Option<&AuthContext>, flow: LoginFlow) -> Result<LoginAnswer, AuthFailure> {
        match flow {
            LoginFlow::Browser { redirect } => {
                let Some(b) = &self.browser else {
                    return Err(AuthFailure::new("this menger has no login page to sign a person in through"));
                };
                let redirect = loopback_redirect(&redirect).map_err(AuthFailure::new)?;
                let state = b.sessions.begin_cli(redirect);
                let url = format!("{}/login?cli=1&state={state}", b.public_url.trim_end_matches('/'));
                Ok(LoginAnswer::LoginUrl { url, state })
            }
            LoginFlow::Code { code, state } => {
                let Some(b) = &self.browser else {
                    return Err(AuthFailure::new("this menger has no login page: there is no code to trade"));
                };
                let ctx = b
                    .sessions
                    .take_code(code.trim(), state.trim())
                    .ok_or_else(|| AuthFailure::new("that code is unknown, used, expired or not this sign-in's: sign in again"))?;
                let secs = b.sessions.ttl_secs().max(1) as u64;
                self.access.mint_for(ctx, secs).map(LoginAnswer::Session).map_err(AuthFailure::new)
            }
            LoginFlow::Password { .. } => Err(AuthFailure::new(
                "this menger takes passwords on its login page (/login?password); from a client, sign in with auth.login {browser}",
            )),
            LoginFlow::Session { ttl_secs, ceiling } => {
                self.access.mint(caller, ttl_secs, ceiling).map(LoginAnswer::Session).map_err(AuthFailure::new)
            }
        }
    }

    async fn signup(&self, caller: Option<&AuthContext>, _via: Option<String>) -> Result<SignupAnswer, AuthFailure> {
        match caller.and_then(|c| c.principal()) {
            None => Err(AuthFailure::new(
                "sign in first (auth.login): signing in with a provider is how an account is made here; then auth.signup asks an admin for access",
            )),
            Some(_) => Err(AuthFailure::new("you hold a role here already: auth.whoami says which")),
        }
    }

    async fn ask_access(&self, caller: &AuthContext, _via: Option<String>) -> Result<AuthWaiting, AuthFailure> {
        self.access.alive(caller).map_err(AuthFailure::new)?;
        self.access.ask_access(caller).map_err(AuthFailure::new)
    }

    fn waiting(&self) -> Vec<AuthWaiting> {
        self.access.waiting()
    }

    async fn logout(&self, caller: &AuthContext) -> Result<(), AuthFailure> {
        if caller.get_metadata_string(SESSION_KEY).is_none() {
            return Ok(());
        }
        self.access.logout(Some(caller)).map_err(AuthFailure::new)
    }

    /// AUTH-11: the gate asks before every call — a signed-out or expired
    /// session holds nothing, `auth.*` included.
    fn alive(&self, caller: &AuthContext) -> Result<(), String> {
        self.access.alive(caller)
    }
}
