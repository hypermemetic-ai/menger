//! IDENTITY-19: sessions — what a person holds once they have signed in.
//!
//! Every way in (a password, the IdP's code flow, a nostr key) ends here:
//! the identity the provider's adapter proved is kept on the server, and
//! the browser gets only a random session id in an `HttpOnly` cookie,
//! which no script on any page can read. The provider's token never leaves
//! the shell.
//!
//! The id is kept hashed, so the table holds nothing a browser could
//! present. A session ends at its expiry or at sign-out; [`Sessions::alive`]
//! is how the activations notice, on every call, that a connection's
//! session is gone (sign-out stops its sockets answering).
//!
//! [`Gate`] is the WebSocket upgrade's validator: the session cookie first,
//! then (IDENTITY-42) a minted session token (`mgs_…`, from
//! `session.login`) as the bearer value or the `access_token` cookie, then a
//! provider's own credential (synapse's `-t` token, a NIP-98 event, an
//! agent's key proof) through the adapters' [`Carriers`].
//!
//! In memory for now: a restart signs everyone out. IDENTITY-18's G2 asks
//! whether sessions should live in the database instead.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine as _;
use plexus_auth_core::adapters::Carriers;
use menger_tickets_api::access::Access;
use plexus_core::plexus::{AuthContext, SessionValidator};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// The session cookie's name when none is chosen ([`Sessions::new`]).
/// A server names its own with [`cookie_name_for`] (IDENTITY-38).
pub const COOKIE: &str = "menger_session";

/// IDENTITY-38: the session cookie's name for a shell reached at
/// `public_url`. Cookies ignore ports, so two menger on one host (prod on
/// 127.0.0.1:4456, test on :4451) sharing one name would sign each other
/// out and hand each other their ids: over http the name carries the
/// port; over https it is `__Host-` (Secure, host-only, `Path=/`), which no
/// other origin can set or shadow.
pub fn cookie_name_for(public_url: &str) -> String {
    match url::Url::parse(public_url) {
        Ok(u) if u.scheme() == "https" => format!("__Host-{COOKIE}"),
        Ok(u) => match u.port_or_known_default() {
            Some(p) => format!("{COOKIE}_{p}"),
            None => COOKIE.to_string(),
        },
        Err(_) => COOKIE.to_string(),
    }
}

/// IDENTITY-38: at most this many login transactions wait at once — each
/// is one anonymous `GET /login/start` — and at most this many sessions
/// live; past either, the one nearest its end makes room.
const MAX_PENDING: usize = 10_000;
const MAX_LIVE: usize = 100_000;

/// IDENTITY-42: what a session token `session.login` mints starts with.
pub const TOKEN_PREFIX: &str = "mgs_";

/// The marker a session-backed identity carries in its metadata, beside
/// `expires_at`: the hash [`Sessions::alive`] is asked about.
pub const SESSION_KEY: &str = "menger_session";

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// 32 random bytes, URL-safe base64: a session id, a PKCE verifier, a
/// login transaction's state.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn hash(id: &str) -> String {
    let digest = Sha256::digest(id.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// A named cookie's value in a `Cookie` header.
pub fn cookie<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|pair| {
        let (n, v) = pair.trim().split_once('=')?;
        (n == name && !v.is_empty()).then_some(v.trim())
    })
}

#[derive(Clone)]
struct Session {
    ctx: AuthContext,
    expires: i64,
}

/// A login on its way out to an IdP and back: what the callback needs to
/// finish it. Keyed by its `state`.
#[derive(Clone, Debug)]
pub struct Pending {
    pub provider: String,
    pub verifier: String,
    pub next: String,
    pub expires: i64,
}

/// AUTH-7: how long a loopback sign-in (`auth.login {browser}`) waits for
/// the person, and how long the one-time code it ends with lasts.
pub const CLI_STATE_SECS: i64 = 600;
pub const CLI_CODE_SECS: i64 = 60;

/// AUTH-7: a loopback sign-in on its way through the login page: where the
/// browser goes back to when the person has signed in.
#[derive(Clone, Debug)]
struct CliPending {
    redirect: String,
    expires: i64,
}

/// AUTH-7: the one-time code a finished loopback sign-in hands the
/// browser: who signed in, bound to the sign-in's state.
#[derive(Clone)]
struct CliCode {
    state: String,
    ctx: AuthContext,
    expires: i64,
}

/// The session table. See the module docs.
pub struct Sessions {
    live: Mutex<HashMap<String, Session>>,
    pending: Mutex<HashMap<String, Pending>>,
    /// AUTH-7: loopback sign-ins, by state.
    cli: Mutex<HashMap<String, CliPending>>,
    /// AUTH-7: their codes, by the code's hash.
    codes: Mutex<HashMap<String, CliCode>>,
    ttl_secs: i64,
    cookie: String,
}

impl Sessions {
    /// Sessions that last `ttl_secs` from sign-in, in the cookie [`COOKIE`].
    pub fn new(ttl_secs: i64) -> Self {
        Self::named(ttl_secs, COOKIE)
    }

    /// Sessions in a cookie of this name ([`cookie_name_for`]).
    pub fn named(ttl_secs: i64, cookie: &str) -> Self {
        Self {
            live: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            cli: Mutex::new(HashMap::new()),
            codes: Mutex::new(HashMap::new()),
            ttl_secs,
            cookie: cookie.to_string(),
        }
    }

    /// The session cookie's name.
    pub fn cookie_name(&self) -> &str {
        &self.cookie
    }

    /// Starts a session for an identity a provider proved: the cookie value
    /// to set and when it ends. The identity is stamped with the session's
    /// key and expiry, so a call can ask whether it still stands.
    pub fn create(&self, ctx: AuthContext) -> (String, i64) {
        self.start(random_token(), ctx, self.ttl_secs)
    }

    /// IDENTITY-42: mints a session token (`mgs_…`) for an identity, lasting
    /// `ttl_secs`: the token and when it ends. Kept hashed, as a cookie's
    /// session is; it ends at its expiry or [`Sessions::end_key`].
    pub fn mint(&self, ctx: AuthContext, ttl_secs: i64) -> (String, i64) {
        self.start(format!("{TOKEN_PREFIX}{}", random_token()), ctx, ttl_secs)
    }

    fn start(&self, id: String, mut ctx: AuthContext, ttl_secs: i64) -> (String, i64) {
        let key = hash(&id);
        let expires = now() + ttl_secs;
        if let Some(meta) = ctx.metadata.as_object_mut() {
            meta.insert(SESSION_KEY.into(), key.clone().into());
            meta.insert("expires_at".into(), expires.into());
        }
        let mut live = self.live.lock().unwrap();
        live.retain(|_, s| s.expires > now());
        make_room(&mut live, MAX_LIVE, |s| s.expires);
        live.insert(key, Session { ctx, expires });
        (id, expires)
    }

    /// The identity a session id stands for, while it lasts.
    pub fn get(&self, id: &str) -> Option<AuthContext> {
        let live = self.live.lock().unwrap();
        live.get(&hash(id)).filter(|s| s.expires > now()).map(|s| s.ctx.clone())
    }

    /// Ends a session (sign-out). Its sockets stop answering on their next
    /// call ([`Sessions::alive`]).
    pub fn end(&self, id: &str) {
        self.live.lock().unwrap().remove(&hash(id));
    }

    /// IDENTITY-42: ends the session whose key (the hash its identity
    /// carries as [`SESSION_KEY`]) this is; whether it stood.
    pub fn end_key(&self, key: &str) -> bool {
        self.live.lock().unwrap().remove(key).is_some_and(|s| s.expires > now())
    }

    /// Whether the session a connection's identity names still stands.
    pub fn alive(&self, key: &str) -> bool {
        self.live.lock().unwrap().get(key).is_some_and(|s| s.expires > now())
    }

    /// Keeps a login transaction for ten minutes; its state.
    pub fn begin(&self, provider: &str, verifier: String, next: String) -> String {
        let state = random_token();
        let mut pending = self.pending.lock().unwrap();
        pending.retain(|_, p| p.expires > now());
        make_room(&mut pending, MAX_PENDING, |p| p.expires);
        pending.insert(state.clone(), Pending { provider: provider.into(), verifier, next, expires: now() + 600 });
        state
    }

    /// Takes a login transaction back, once.
    pub fn finish(&self, state: &str) -> Option<Pending> {
        self.pending.lock().unwrap().remove(state).filter(|p| p.expires > now())
    }

    /// AUTH-7: how long a browser's sign-in lasts.
    pub fn ttl_secs(&self) -> i64 {
        self.ttl_secs
    }

    /// AUTH-7, `auth.login {browser {redirect}}`: keeps a loopback sign-in
    /// for ten minutes; its state.
    pub fn begin_cli(&self, redirect: String) -> String {
        let state = format!("s_{}", random_token());
        let mut cli = self.cli.lock().unwrap();
        cli.retain(|_, p| p.expires > now());
        make_room(&mut cli, MAX_PENDING, |p| p.expires);
        cli.insert(state.clone(), CliPending { redirect, expires: now() + CLI_STATE_SECS });
        state
    }

    /// AUTH-7: whether `state` names a loopback sign-in still waiting.
    pub fn cli_waiting(&self, state: &str) -> bool {
        self.cli.lock().unwrap().get(state).is_some_and(|p| p.expires > now())
    }

    /// AUTH-7: the person signed in as `ctx`: the loopback sign-in `state`
    /// is taken (once) and a one-time code bound to it issued, good for
    /// [`CLI_CODE_SECS`] — the redirect to send the browser to, and the
    /// code. `None`: no such sign-in waits.
    pub fn finish_cli(&self, state: &str, ctx: AuthContext) -> Option<(String, String)> {
        let pending = self.cli.lock().unwrap().remove(state).filter(|p| p.expires > now())?;
        let code = format!("c_{}", random_token());
        let mut codes = self.codes.lock().unwrap();
        codes.retain(|_, c| c.expires > now());
        make_room(&mut codes, MAX_PENDING, |c| c.expires);
        codes.insert(hash(&code), CliCode { state: state.to_string(), ctx, expires: now() + CLI_CODE_SECS });
        Some((pending.redirect, code))
    }

    /// AUTH-7, `auth.login {code {code, state}}`: who signed in, once — the
    /// code must be the one issued for `state`, and fresh.
    pub fn take_code(&self, code: &str, state: &str) -> Option<AuthContext> {
        let c = self.codes.lock().unwrap().remove(&hash(code))?;
        (c.expires > now() && c.state == state).then_some(c.ctx)
    }
}

/// AUTH-7: a loopback sign-in's redirect, as `auth.login {browser}` takes it:
/// `http://127.0.0.1:<port>/…`, `http://localhost:<port>/…` or
/// `http://[::1]:<port>/…` — the client's own port on this machine, nothing
/// else (no user, no fragment). The normalized URL, or why not.
pub fn loopback_redirect(text: &str) -> Result<String, String> {
    let refuse = || format!("`{}` is not a loopback redirect: http://127.0.0.1:<port>/… or http://localhost:<port>/…", text.trim());
    let u = url::Url::parse(text.trim()).map_err(|_| refuse())?;
    let host_ok = matches!(u.host(), Some(url::Host::Ipv4(ip)) if ip == std::net::Ipv4Addr::LOCALHOST)
        || matches!(u.host(), Some(url::Host::Ipv6(ip)) if ip == std::net::Ipv6Addr::LOCALHOST)
        || matches!(u.host(), Some(url::Host::Domain(d)) if d.eq_ignore_ascii_case("localhost"));
    if u.scheme() != "http" || !host_ok || u.port().is_none() || !u.username().is_empty() || u.password().is_some() || u.fragment().is_some() {
        return Err(refuse());
    }
    Ok(u.to_string())
}

/// Drops the entry nearest its end while `map` is at `cap`.
fn make_room<V>(map: &mut HashMap<String, V>, cap: usize, expires: impl Fn(&V) -> i64) {
    while map.len() >= cap {
        let Some(k) = map.iter().min_by_key(|(_, v)| expires(v)).map(|(k, _)| k.clone()) else { return };
        map.remove(&k);
    }
}

/// The activations' view of the session table: is this connection's
/// session still standing? (tickets-api's `SessionCheck`.)
impl menger_tickets_api::access::SessionCheck for Sessions {
    fn alive(&self, key: &str) -> bool {
        Sessions::alive(self, key)
    }
}

/// IDENTITY-42: `session.login` and `session.logout` mint and end sessions
/// here (tickets-api's `SessionMint`).
impl menger_tickets_api::access::SessionMint for Sessions {
    fn mint(&self, ctx: AuthContext, ttl_secs: i64) -> (String, i64) {
        Sessions::mint(self, ctx, ttl_secs)
    }

    fn end_key(&self, key: &str) -> bool {
        Sessions::end_key(self, key)
    }
}

/// A minted session token a value the transport offers carries: the bearer
/// value itself, or the `access_token` cookie of a `Cookie` header.
fn minted_token(value: &str) -> Option<&str> {
    let v = value.trim();
    if v.starts_with(TOKEN_PREFIX) && !v.contains([';', '=', ' ']) {
        return Some(v);
    }
    cookie(v, "access_token").filter(|t| t.starts_with(TOKEN_PREFIX))
}

/// The WebSocket upgrade's validator: a session cookie, else a minted
/// session token, else a provider's own credential through the carriers.
pub struct Gate {
    sessions: Arc<Sessions>,
    carriers: Option<Arc<Carriers>>,
    /// AUTH-7: who signs in holding nothing is put on the waiting list here,
    /// as it opens the socket — the gate refuses its calls before any method
    /// could notice it.
    access: Option<Access>,
}

impl Gate {
    pub fn new(sessions: Arc<Sessions>, carriers: Option<Arc<Carriers>>) -> Self {
        Self { sessions, carriers, access: None }
    }

    /// AUTH-7: a sign-in holding no grant waits for an admin
    /// ([`Access::notice`]).
    pub fn noticing(mut self, access: Access) -> Self {
        self.access = Some(access);
        self
    }

    async fn identify(&self, value: &str) -> Option<AuthContext> {
        if let Some(ctx) = cookie(value, self.sessions.cookie_name()).and_then(|id| self.sessions.get(id)) {
            return Some(ctx);
        }
        // IDENTITY-42: a minted token is a session or nobody — an ended or
        // expired one proves no one (refused 401: AUTH-6, a credential
        // presented and refused).
        if let Some(token) = minted_token(value) {
            return self.sessions.get(token);
        }
        match &self.carriers {
            Some(c) => c.validate(value).await,
            None => None,
        }
    }
}

#[async_trait]
impl SessionValidator for Gate {
    async fn validate(&self, value: &str) -> Option<AuthContext> {
        let ctx = self.identify(value).await?;
        if let Some(a) = &self.access {
            a.notice(&ctx);
        }
        Some(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn alice() -> AuthContext {
        AuthContext::new("alice".into(), "sid".into(), vec![], json!({ "name": "alice" }))
    }

    #[test]
    fn a_session_stands_until_it_ends() {
        let s = Sessions::new(3600);
        let (id, expires) = s.create(alice());
        let ctx = s.get(&id).expect("signed in");
        assert_eq!(ctx.user_id, "alice");
        assert_eq!(ctx.metadata["expires_at"], expires);
        let key = ctx.metadata[SESSION_KEY].as_str().unwrap().to_string();
        assert!(s.alive(&key));
        assert_ne!(key, id, "the table keeps a hash, not the cookie");
        s.end(&id);
        assert!(s.get(&id).is_none());
        assert!(!s.alive(&key), "its sockets see it gone");
    }

    #[test]
    fn an_expired_session_is_nobody() {
        let s = Sessions::new(-1);
        let (id, _) = s.create(alice());
        assert!(s.get(&id).is_none());
    }

    #[test]
    fn a_login_transaction_is_taken_once() {
        let s = Sessions::new(3600);
        let state = s.begin("auth0", "verifier".into(), "/".into());
        assert_eq!(s.finish(&state).unwrap().verifier, "verifier");
        assert!(s.finish(&state).is_none());
        assert!(s.finish("forged").is_none());
    }

    #[test]
    fn each_shell_names_its_own_cookie() {
        assert_eq!(cookie_name_for("http://127.0.0.1:4456"), "menger_session_4456");
        assert_eq!(cookie_name_for("http://127.0.0.1:4451/"), "menger_session_4451");
        assert_eq!(cookie_name_for("http://menger.localhost"), "menger_session_80");
        assert_eq!(cookie_name_for("https://menger.example"), "__Host-menger_session");
    }

    #[test]
    fn waiting_logins_are_capped() {
        let s = Sessions::new(3600);
        let first = s.begin("auth0", "v".into(), "/".into());
        for _ in 0..MAX_PENDING + 50 {
            s.begin("auth0", "v".into(), "/".into());
        }
        assert_eq!(s.pending.lock().unwrap().len(), MAX_PENDING);
        let _ = first;
    }

    #[tokio::test]
    async fn the_gate_reads_only_its_own_cookie() {
        let s = Arc::new(Sessions::named(3600, "menger_session_4456"));
        let (id, _) = s.create(alice());
        let gate = Gate::new(s.clone(), None);
        assert!(gate.validate(&format!("menger_session_4451=x; menger_session_4456={id}")).await.is_some());
        assert!(gate.validate(&format!("menger_session={id}")).await.is_none(), "another server's name is not ours");
    }

    #[tokio::test]
    async fn a_minted_token_opens_as_a_bearer_or_the_access_token_cookie_until_it_ends() {
        let s = Arc::new(Sessions::new(3600));
        let (token, expires) = s.mint(alice(), 60);
        assert!(token.starts_with("mgs_") && token.len() >= 47, "{token}");
        assert!(expires <= now() + 60);
        let gate = Gate::new(s.clone(), None);
        let ctx = gate.validate(&token).await.expect("bearer");
        assert_eq!(ctx.user_id, "alice");
        assert!(gate.validate(&format!("theme=dark; access_token={token}")).await.is_some(), "cookie");
        assert!(gate.validate("mgs_forged").await.is_none());
        let key = ctx.metadata[SESSION_KEY].as_str().unwrap().to_string();
        assert!(s.end_key(&key));
        assert!(gate.validate(&token).await.is_none(), "ended");
        assert!(!s.end_key(&key), "once");
        let (old, _) = s.mint(alice(), -1);
        assert!(gate.validate(&old).await.is_none(), "expired");
    }

    #[test]
    fn a_loopback_sign_in_ends_in_one_code_for_its_state() {
        let s = Sessions::new(3600);
        let state = s.begin_cli("http://127.0.0.1:53817/cb".into());
        assert!(s.cli_waiting(&state));
        let (redirect, code) = s.finish_cli(&state, alice()).expect("waiting");
        assert_eq!(redirect, "http://127.0.0.1:53817/cb");
        assert!(!s.cli_waiting(&state) && s.finish_cli(&state, alice()).is_none(), "the state is taken once");
        assert!(s.take_code(&code, "s_other").is_none(), "bound to its state");
        let s2 = Sessions::new(3600);
        let st = s2.begin_cli("http://localhost:1/x".into());
        let (_, c) = s2.finish_cli(&st, alice()).unwrap();
        assert_eq!(s2.take_code(&c, &st).unwrap().user_id, "alice");
        assert!(s2.take_code(&c, &st).is_none(), "once");
    }

    #[test]
    fn only_a_loopback_port_is_a_redirect() {
        for ok in ["http://127.0.0.1:53817/cb", "http://localhost:9/", "http://[::1]:8080/cb?x=1"] {
            assert!(loopback_redirect(ok).is_ok(), "{ok}");
        }
        for bad in ["https://127.0.0.1:1/cb", "http://127.0.0.1/cb", "http://evil.example:1/", "http://127.0.0.2:1/", "http://u@127.0.0.1:1/", "http://127.0.0.1:1/#x", "nope"] {
            assert!(loopback_redirect(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ids_are_random_and_long() {
        let (a, b) = (random_token(), random_token());
        assert_ne!(a, b);
        assert!(a.len() >= 43);
    }

    #[tokio::test]
    async fn the_gate_takes_the_session_cookie_among_others() {
        let s = Arc::new(Sessions::new(3600));
        let (id, _) = s.create(alice());
        let gate = Gate::new(s.clone(), None);
        assert!(gate.validate(&format!("theme=dark; {COOKIE}={id}")).await.is_some());
        assert!(gate.validate(&format!("{COOKIE}=forged")).await.is_none());
        assert!(gate.validate(&id).await.is_none(), "a bare id is not a cookie");
    }
}
