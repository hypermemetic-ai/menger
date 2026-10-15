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
//!
//! ACCESS-29: every table here is bounded and kept by when its entries end
//! ([`Ending`]): an insert drops what has ended and makes room in O(log n),
//! never by a scan of the table — so a flood of anonymous
//! `GET /login/start`s costs each one its own entry and nothing more. A
//! browser's session also ends when it goes unused for
//! [`IDLE_SECS_DEFAULT`] (`--session-idle-minutes`): no socket open on it
//! and no page loaded with it.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine as _;
use plexus_auth_core::adapters::Carriers;
use plexus_core::plexus::{AuthContext, SessionValidator};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::access::LoginAccess;
use crate::names::LoginNames;

/// IDENTITY-38: at most this many login transactions wait at once — each
/// is one anonymous `GET /login/start` — and at most this many sessions
/// live; past either, the one nearest its end makes room (ACCESS-29: found
/// by the index, not a scan). The loopback sign-ins and their one-time codes
/// are held to [`MAX_PENDING`] each too.
pub const MAX_PENDING: usize = 10_000;
pub const MAX_LIVE: usize = 100_000;

/// ACCESS-29 (ACCESS-4 L2): a browser's session ends after this long unused
/// — no socket open on it (an open socket asks after it every second) and no
/// page loaded with it — as well as at its expiry. Two hours; 0 turns it
/// off. A minted token (`session.login`, an agent's) keeps only its expiry.
pub const IDLE_SECS_DEFAULT: i64 = 2 * 3600;

/// IDENTITY-42: what a session token `session.login` mints starts with.
pub const TOKEN_PREFIX: &str = "mgs_";

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
    /// ACCESS-29: a browser's session, which ends when unused
    /// ([`Sessions::with_idle`]); a minted token is not.
    idles: bool,
    /// When it was last presented or asked after.
    seen: i64,
}

/// ACCESS-29: a table of entries that end, kept beside an index by when —
/// an insert drops the ended from the index's front and, at its cap, the
/// entry nearest its end: O(log n), never a scan of the table.
struct Ending<V> {
    map: HashMap<String, (V, i64)>,
    by_end: BTreeSet<(i64, String)>,
}

impl<V> Ending<V> {
    fn new() -> Self {
        Self { map: HashMap::new(), by_end: BTreeSet::new() }
    }

    /// Keeps `v` under `key` until `ends`, `cap` entries at most.
    fn insert(&mut self, key: String, v: V, ends: i64, cap: usize) {
        self.purge(now());
        self.remove(&key);
        while self.map.len() >= cap.max(1) {
            let Some((_, k)) = self.by_end.pop_first() else { break };
            self.map.remove(&k);
        }
        self.by_end.insert((ends, key.clone()));
        self.map.insert(key, (v, ends));
    }

    /// Drops every entry ended by `t`, from the front of the index.
    fn purge(&mut self, t: i64) {
        while self.by_end.first().is_some_and(|(e, _)| *e <= t) {
            let (_, k) = self.by_end.pop_first().expect("checked");
            self.map.remove(&k);
        }
    }

    /// The entry under `key`, while it lasts.
    fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        self.map.get_mut(key).filter(|(_, e)| *e > now()).map(|(v, _)| v)
    }

    fn get(&self, key: &str) -> Option<&V> {
        self.map.get(key).filter(|(_, e)| *e > now()).map(|(v, _)| v)
    }

    /// Takes the entry under `key` out, ended or not: it and when it ends.
    fn remove(&mut self, key: &str) -> Option<(V, i64)> {
        let (v, e) = self.map.remove(key)?;
        self.by_end.remove(&(e, key.to_string()));
        Some((v, e))
    }

    /// Takes the entry under `key` out, if it still lasts.
    fn take(&mut self, key: &str) -> Option<V> {
        self.remove(key).filter(|(_, e)| *e > now()).map(|(v, _)| v)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
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
}

/// AUTH-7: the one-time code a finished loopback sign-in hands the
/// browser: who signed in, bound to the sign-in's state.
#[derive(Clone)]
struct CliCode {
    state: String,
    ctx: AuthContext,
}

/// The session table. See the module docs.
pub struct Sessions {
    live: Mutex<Ending<Session>>,
    pending: Mutex<Ending<Pending>>,
    /// AUTH-7: loopback sign-ins, by state.
    cli: Mutex<Ending<CliPending>>,
    /// AUTH-7: their codes, by the code's hash.
    codes: Mutex<Ending<CliCode>>,
    ttl_secs: i64,
    /// ACCESS-29: how long a browser's session may go unused (0: forever).
    idle_secs: i64,
    /// The session cookie's name (this shell's, IDENTITY-38).
    cookie: String,
    /// The login transaction cookie's name (this shell's).
    login_cookie: String,
    names: LoginNames,
}

impl Sessions {
    /// Sessions that last `ttl_secs` from sign-in, in cookies of the base
    /// names `names` gives (no shell: nothing per shell).
    pub fn new(ttl_secs: i64, names: &LoginNames) -> Self {
        Self::build(ttl_secs, names, names.session_cookie.clone(), names.login_cookie.clone())
    }

    /// Sessions for a shell reached at `public_url`: its cookies named for it
    /// ([`LoginNames::session_cookie_for`], IDENTITY-38), so two servers on
    /// one host keep apart.
    pub fn for_shell(ttl_secs: i64, names: &LoginNames, public_url: &str) -> Self {
        Self::build(ttl_secs, names, names.session_cookie_for(public_url), names.login_cookie_for(public_url))
    }

    fn build(ttl_secs: i64, names: &LoginNames, cookie: String, login_cookie: String) -> Self {
        Self {
            live: Mutex::new(Ending::new()),
            pending: Mutex::new(Ending::new()),
            cli: Mutex::new(Ending::new()),
            codes: Mutex::new(Ending::new()),
            ttl_secs,
            idle_secs: IDLE_SECS_DEFAULT,
            cookie,
            login_cookie,
            names: names.clone(),
        }
    }

    /// ACCESS-29: a browser's session ends after `secs` unused
    /// ([`IDLE_SECS_DEFAULT`] unless said; 0: only at its expiry).
    pub fn with_idle(mut self, secs: i64) -> Self {
        self.idle_secs = secs.max(0);
        self
    }

    /// The session cookie's name.
    pub fn cookie_name(&self) -> &str {
        &self.cookie
    }

    /// The login transaction cookie's name: binds a code-flow callback to
    /// the browser that started it.
    pub fn login_cookie_name(&self) -> &str {
        &self.login_cookie
    }

    /// The metadata key a session-backed identity carries, beside
    /// `expires_at`: the hash [`Sessions::alive`] is asked about. Its one
    /// definition ([`LoginNames::session_key`]).
    pub fn session_key(&self) -> &str {
        &self.names.session_key
    }

    /// Every name this table writes.
    pub fn names(&self) -> &LoginNames {
        &self.names
    }

    /// Starts a session for an identity a provider proved: the cookie value
    /// to set and when it ends. The identity is stamped with the session's
    /// key and expiry, so a call can ask whether it still stands.
    pub fn create(&self, ctx: AuthContext) -> (String, i64) {
        self.start(random_token(), ctx, self.ttl_secs, true)
    }

    /// IDENTITY-42: mints a session token (`mgs_…`) for an identity, lasting
    /// `ttl_secs`: the token and when it ends. Kept hashed, as a cookie's
    /// session is; it ends at its expiry or [`Sessions::end_key`].
    pub fn mint(&self, ctx: AuthContext, ttl_secs: i64) -> (String, i64) {
        self.start(format!("{TOKEN_PREFIX}{}", random_token()), ctx, ttl_secs, false)
    }

    fn start(&self, id: String, mut ctx: AuthContext, ttl_secs: i64, idles: bool) -> (String, i64) {
        let key = hash(&id);
        let expires = now() + ttl_secs;
        if let Some(meta) = ctx.metadata.as_object_mut() {
            meta.insert(self.names.session_key.clone(), key.clone().into());
            meta.insert("expires_at".into(), expires.into());
        }
        self.live.lock().unwrap().insert(key, Session { ctx, idles, seen: now() }, expires, MAX_LIVE);
        (id, expires)
    }

    /// The session under `key` while it stands — before its expiry and, a
    /// browser's, used within [`Sessions::with_idle`]'s time — marked used
    /// now. One gone idle is dropped.
    fn used(&self, key: &str) -> Option<AuthContext> {
        let mut live = self.live.lock().unwrap();
        let t = now();
        let s = live.get_mut(key)?;
        if s.idles && self.idle_secs > 0 && t - s.seen > self.idle_secs {
            live.remove(key);
            return None;
        }
        s.seen = t;
        Some(s.ctx.clone())
    }

    /// The identity a session id stands for, while it lasts.
    pub fn get(&self, id: &str) -> Option<AuthContext> {
        self.used(&hash(id))
    }

    /// Ends a session (sign-out). Its sockets stop answering on their next
    /// call ([`Sessions::alive`]).
    pub fn end(&self, id: &str) {
        self.live.lock().unwrap().remove(&hash(id));
    }

    /// IDENTITY-42: ends the session whose key (the hash its identity
    /// carries under [`Sessions::session_key`]) this is; whether it stood.
    pub fn end_key(&self, key: &str) -> bool {
        self.live.lock().unwrap().take(key).is_some()
    }

    /// Whether the session a connection's identity names still stands.
    pub fn alive(&self, key: &str) -> bool {
        self.used(key).is_some()
    }

    /// Keeps a login transaction for ten minutes; its state.
    pub fn begin(&self, provider: &str, verifier: String, next: String) -> String {
        let state = random_token();
        let expires = now() + 600;
        let p = Pending { provider: provider.into(), verifier, next, expires };
        self.pending.lock().unwrap().insert(state.clone(), p, expires, MAX_PENDING);
        state
    }

    /// Takes a login transaction back, once.
    pub fn finish(&self, state: &str) -> Option<Pending> {
        self.pending.lock().unwrap().take(state)
    }

    /// AUTH-7: how long a browser's sign-in lasts.
    pub fn ttl_secs(&self) -> i64 {
        self.ttl_secs
    }

    /// AUTH-7, `auth.login {browser {redirect}}`: keeps a loopback sign-in
    /// for ten minutes; its state.
    pub fn begin_cli(&self, redirect: String) -> String {
        let state = format!("s_{}", random_token());
        let expires = now() + CLI_STATE_SECS;
        self.cli.lock().unwrap().insert(state.clone(), CliPending { redirect }, expires, MAX_PENDING);
        state
    }

    /// AUTH-7: whether `state` names a loopback sign-in still waiting.
    pub fn cli_waiting(&self, state: &str) -> bool {
        self.cli.lock().unwrap().get(state).is_some()
    }

    /// AUTH-7: the person signed in as `ctx`: the loopback sign-in `state`
    /// is taken (once) and a one-time code bound to it issued, good for
    /// [`CLI_CODE_SECS`] — the redirect to send the browser to, and the
    /// code. `None`: no such sign-in waits.
    pub fn finish_cli(&self, state: &str, ctx: AuthContext) -> Option<(String, String)> {
        let pending = self.cli.lock().unwrap().take(state)?;
        let code = format!("c_{}", random_token());
        let c = CliCode { state: state.to_string(), ctx };
        self.codes.lock().unwrap().insert(hash(&code), c, now() + CLI_CODE_SECS, MAX_PENDING);
        Some((pending.redirect, code))
    }

    /// AUTH-7, `auth.login {code {code, state}}`: who signed in, once — the
    /// code must be the one issued for `state`, and fresh.
    pub fn take_code(&self, code: &str, state: &str) -> Option<AuthContext> {
        let c = self.codes.lock().unwrap().take(&hash(code))?;
        (c.state == state).then_some(c.ctx)
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

/// A minted session token a value the transport offers carries: the bearer
/// value itself, or the `access_token` cookie of a `Cookie` header.
fn minted_token(value: &str) -> Option<&str> {
    let v = value.trim();
    if v.starts_with(TOKEN_PREFIX) && !v.contains([';', '=', ' ']) {
        return Some(v);
    }
    cookie(v, "access_token").filter(|t| t.starts_with(TOKEN_PREFIX))
}

/// What the gate calls for every identity it admits (the app's
/// [`LoginAccess::notice`]).
type Notice = Arc<dyn Fn(&AuthContext) + Send + Sync>;

/// The WebSocket upgrade's validator: a session cookie, else a minted
/// session token, else a provider's own credential through the carriers.
pub struct Gate {
    sessions: Arc<Sessions>,
    carriers: Option<Arc<Carriers>>,
    /// AUTH-7: who signs in holding nothing is put on the waiting list here,
    /// as it opens the socket — the gate refuses its calls before any method
    /// could notice it. SCAFFOLD-3: the app's [`LoginAccess::notice`].
    notice: Option<Notice>,
}

impl Gate {
    pub fn new(sessions: Arc<Sessions>, carriers: Option<Arc<Carriers>>) -> Self {
        Self { sessions, carriers, notice: None }
    }

    /// AUTH-7: a sign-in holding no grant waits for an admin
    /// ([`LoginAccess::notice`]).
    pub fn noticing<A: LoginAccess>(mut self, access: A) -> Self {
        self.notice = Some(Arc::new(move |ctx: &AuthContext| access.notice(ctx)));
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
        if let Some(notice) = &self.notice {
            notice(&ctx);
        }
        Some(ctx)
    }

    /// AUTH-11: the transport asks while the socket is open, and closes it
    /// once its session is signed out or expired.
    fn alive(&self, ctx: &AuthContext) -> Result<(), String> {
        if ctx.expired() {
            return Err("your sign-in has expired: sign in again".into());
        }
        match ctx.get_metadata_string(self.sessions.session_key()) {
            Some(key) if !self.sessions.alive(&key) => Err("you have signed out: sign in again".into()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn names() -> LoginNames {
        LoginNames::for_app("menger")
    }

    fn alice() -> AuthContext {
        AuthContext::new("alice".into(), "sid".into(), vec![], json!({ "name": "alice" }))
    }

    #[test]
    fn a_session_stands_until_it_ends() {
        let s = Sessions::new(3600, &names());
        let (id, expires) = s.create(alice());
        let ctx = s.get(&id).expect("signed in");
        assert_eq!(ctx.user_id, "alice");
        assert_eq!(ctx.metadata["expires_at"], expires);
        let key = ctx.metadata["menger_session"].as_str().unwrap().to_string();
        assert!(s.alive(&key));
        assert_ne!(key, id, "the table keeps a hash, not the cookie");
        s.end(&id);
        assert!(s.get(&id).is_none());
        assert!(!s.alive(&key), "its sockets see it gone");
    }

    #[test]
    fn an_expired_session_is_nobody() {
        let s = Sessions::new(-1, &names());
        let (id, _) = s.create(alice());
        assert!(s.get(&id).is_none());
    }

    #[test]
    fn a_login_transaction_is_taken_once() {
        let s = Sessions::new(3600, &names());
        let state = s.begin("auth0", "verifier".into(), "/".into());
        assert_eq!(s.finish(&state).unwrap().verifier, "verifier");
        assert!(s.finish(&state).is_none());
        assert!(s.finish("forged").is_none());
    }

    #[test]
    fn a_table_names_its_cookies_for_its_shell() {
        let s = Sessions::for_shell(3600, &names(), "http://127.0.0.1:4456");
        assert_eq!((s.cookie_name(), s.login_cookie_name(), s.session_key()), ("menger_session_4456", "menger_login_4456", "menger_session"));
        let s = Sessions::for_shell(3600, &LoginNames::for_app("toy"), "https://toy.example");
        assert_eq!((s.cookie_name(), s.login_cookie_name(), s.session_key()), ("__Host-toy_session", "__Secure-toy_login", "toy_session"));
        let (id, _) = s.create(alice());
        assert!(s.get(&id).unwrap().metadata["toy_session"].is_string(), "stamped under the app's key");
    }

    #[test]
    fn waiting_logins_are_capped() {
        let s = Sessions::new(3600, &names());
        let first = s.begin("auth0", "v".into(), "/".into());
        for _ in 0..MAX_PENDING + 50 {
            s.begin("auth0", "v".into(), "/".into());
        }
        assert_eq!(s.pending.lock().unwrap().len(), MAX_PENDING);
        let _ = first;
    }

    #[test]
    fn a_flood_of_logins_is_held_to_the_cap_without_a_scan_per_insert() {
        // ACCESS-29: ten times the cap, each insert O(log n) — the scan the
        // table did before made this a billion steps.
        let s = Sessions::new(3600, &names());
        let started = std::time::Instant::now();
        let first = s.begin("auth0", "v".into(), "/".into());
        for _ in 0..MAX_PENDING * 10 {
            s.begin("auth0", "v".into(), "/".into());
        }
        let p = s.pending.lock().unwrap();
        assert_eq!((p.len(), p.by_end.len()), (MAX_PENDING, MAX_PENDING), "the index and the table agree");
        drop(p);
        assert!(s.finish(&first).is_none(), "the oldest made room");
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "{:?}", started.elapsed());
    }

    #[test]
    fn a_browser_session_unused_past_its_idle_time_ends_and_a_minted_token_does_not() {
        // ACCESS-29 (ACCESS-4 L2).
        let s = Sessions::new(3600, &names()).with_idle(60);
        let (id, _) = s.create(alice());
        let (token, _) = s.mint(alice(), 3600);
        let key = s.get(&id).unwrap().metadata["menger_session"].as_str().unwrap().to_string();
        let minted = s.get(&token).unwrap().metadata["menger_session"].as_str().unwrap().to_string();
        assert!(s.alive(&key), "used: it stands");
        // An hour unused.
        for k in [&key, &minted] {
            s.live.lock().unwrap().get_mut(k).unwrap().seen -= 3000;
        }
        assert!(!s.alive(&key) && s.get(&id).is_none(), "idle: ended");
        assert!(s.alive(&minted) && s.get(&token).is_some(), "a minted token keeps only its expiry");
        let off = Sessions::new(3600, &names()).with_idle(0);
        let (id, _) = off.create(alice());
        let key = off.get(&id).unwrap().metadata["menger_session"].as_str().unwrap().to_string();
        off.live.lock().unwrap().get_mut(&key).unwrap().seen -= 3000;
        assert!(off.get(&id).is_some(), "0: no idle end");
    }

    #[test]
    fn asking_after_a_session_keeps_it_in_use() {
        let s = Sessions::new(3600, &names()).with_idle(60);
        let (id, _) = s.create(alice());
        let key = s.get(&id).unwrap().metadata["menger_session"].as_str().unwrap().to_string();
        s.live.lock().unwrap().get_mut(&key).unwrap().seen -= 50;
        assert!(s.alive(&key), "an open socket's ask is a use");
        s.live.lock().unwrap().get_mut(&key).unwrap().seen -= 50;
        assert!(s.get(&id).is_some(), "the ask moved its last use to now");
    }

    #[tokio::test]
    async fn the_gate_reads_only_its_own_cookie() {
        let s = Arc::new(Sessions::for_shell(3600, &names(), "http://127.0.0.1:4456"));
        let (id, _) = s.create(alice());
        let gate = Gate::new(s.clone(), None);
        assert!(gate.validate(&format!("menger_session_4451=x; menger_session_4456={id}")).await.is_some());
        assert!(gate.validate(&format!("menger_session={id}")).await.is_none(), "another server's name is not ours");
    }

    #[tokio::test]
    async fn a_minted_token_opens_as_a_bearer_or_the_access_token_cookie_until_it_ends() {
        let s = Arc::new(Sessions::new(3600, &names()));
        let (token, expires) = s.mint(alice(), 60);
        assert!(token.starts_with("mgs_") && token.len() >= 47, "{token}");
        assert!(expires <= now() + 60);
        let gate = Gate::new(s.clone(), None);
        let ctx = gate.validate(&token).await.expect("bearer");
        assert_eq!(ctx.user_id, "alice");
        assert!(gate.validate(&format!("theme=dark; access_token={token}")).await.is_some(), "cookie");
        assert!(gate.validate("mgs_forged").await.is_none());
        let key = ctx.metadata["menger_session"].as_str().unwrap().to_string();
        assert!(s.end_key(&key));
        assert!(gate.validate(&token).await.is_none(), "ended");
        assert!(!s.end_key(&key), "once");
        let (old, _) = s.mint(alice(), -1);
        assert!(gate.validate(&old).await.is_none(), "expired");
    }

    #[test]
    fn a_loopback_sign_in_ends_in_one_code_for_its_state() {
        let s = Sessions::new(3600, &names());
        let state = s.begin_cli("http://127.0.0.1:53817/cb".into());
        assert!(s.cli_waiting(&state));
        let (redirect, code) = s.finish_cli(&state, alice()).expect("waiting");
        assert_eq!(redirect, "http://127.0.0.1:53817/cb");
        assert!(!s.cli_waiting(&state) && s.finish_cli(&state, alice()).is_none(), "the state is taken once");
        assert!(s.take_code(&code, "s_other").is_none(), "bound to its state");
        let s2 = Sessions::new(3600, &names());
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
        let s = Arc::new(Sessions::new(3600, &names()));
        let (id, _) = s.create(alice());
        let gate = Gate::new(s.clone(), None);
        assert!(gate.validate(&format!("theme=dark; menger_session={id}")).await.is_some());
        assert!(gate.validate("menger_session=forged").await.is_none());
        assert!(gate.validate(&id).await.is_none(), "a bare id is not a cookie");
    }
}
