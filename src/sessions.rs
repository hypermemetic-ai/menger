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
//! then a provider's own credential (synapse's `-t` token, a NIP-98 event)
//! through the adapters' [`Carriers`], as IDENTITY-14 left them.
//!
//! In memory for now: a restart signs everyone out. IDENTITY-18's G2 asks
//! whether sessions should live in the database instead.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine as _;
use plexus_auth_core::adapters::Carriers;
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

/// The session table. See the module docs.
pub struct Sessions {
    live: Mutex<HashMap<String, Session>>,
    pending: Mutex<HashMap<String, Pending>>,
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
        Self { live: Mutex::new(HashMap::new()), pending: Mutex::new(HashMap::new()), ttl_secs, cookie: cookie.to_string() }
    }

    /// The session cookie's name.
    pub fn cookie_name(&self) -> &str {
        &self.cookie
    }

    /// Starts a session for an identity a provider proved: the cookie value
    /// to set and when it ends. The identity is stamped with the session's
    /// key and expiry, so a call can ask whether it still stands.
    pub fn create(&self, mut ctx: AuthContext) -> (String, i64) {
        let id = random_token();
        let key = hash(&id);
        let expires = now() + self.ttl_secs;
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

/// The WebSocket upgrade's validator: a session cookie, else a provider's
/// own credential through the carriers.
pub struct Gate {
    sessions: Arc<Sessions>,
    carriers: Option<Arc<Carriers>>,
}

impl Gate {
    pub fn new(sessions: Arc<Sessions>, carriers: Option<Arc<Carriers>>) -> Self {
        Self { sessions, carriers }
    }
}

#[async_trait]
impl SessionValidator for Gate {
    async fn validate(&self, value: &str) -> Option<AuthContext> {
        if let Some(ctx) = cookie(value, self.sessions.cookie_name()).and_then(|id| self.sessions.get(id)) {
            return Some(ctx);
        }
        match &self.carriers {
            Some(c) => c.validate(value).await,
            None => None,
        }
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
