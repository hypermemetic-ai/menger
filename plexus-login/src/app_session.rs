//! AUTH-22: the per-app half of our own sign-in page — what an app keeps
//! once [`SignInPage`](crate::signin::SignInPage) has signed someone in,
//! whichever [`IdentityBackend`] did it.
//!
//! - **The session** is the app's own ([`Sessions`]), as every other way in
//!   ends: an `HttpOnly` cookie naming a hashed id, which the gate takes
//!   for a browser.
//! - **The backend's tokens** stay here, server side, beside the session
//!   ([`TokenStore`]) — never in the browser — with the backend that issued
//!   them, so a swapped backend leaves existing sessions in the hands of
//!   the one that made them. Every 30 s, an access token within two minutes
//!   of expiry is refreshed through its backend. A refused refresh — the
//!   session revoked or expired at the backend — ends the app's session
//!   too, which closes its sockets (AUTH-11). The app's session ending
//!   (sign-out, expiry) drops the tokens and ends the backend's session.
//!
//! AUTH-17 goes further: the browser's credential *is* the identity
//! provider's token and the app mints no session. This is the step before:
//! the tokens are already kept and refreshed; what remains is for the
//! cookie to carry them (or a sealed form) instead of the session id.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use crate::identity::{IdentityBackend, IdentityError, RequestInfo, SignedIn, TokenSet};
use crate::page::{self, Body, Chrome};
use crate::sessions::{self, now, Sessions};
use crate::signin::{AccountHolder, Continuation};

const REFRESH_EVERY_SECS: u64 = 30;
const REFRESH_WITHIN_SECS: i64 = 120;

/// One session's tokens, and the backend that issued them.
#[derive(Clone)]
pub struct Held {
    pub tokens: TokenSet,
    pub principal: String,
    pub backend: Arc<dyn IdentityBackend>,
}

/// What a refresh pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Refreshed {
    pub refreshed: usize,
    /// Sessions ended because the backend refused their refresh.
    pub ended: usize,
    /// Tokens dropped because their app session had ended.
    pub dropped: usize,
}

/// The backends' tokens, by the app session's key. See the module docs.
pub struct TokenStore {
    map: Mutex<HashMap<String, Held>>,
    sessions: Arc<Sessions>,
    running: AtomicBool,
}

impl TokenStore {
    pub fn new(sessions: Arc<Sessions>) -> Arc<Self> {
        Arc::new(Self { map: Mutex::new(HashMap::new()), sessions, running: AtomicBool::new(false) })
    }

    pub fn insert(&self, session_key: &str, held: Held) {
        self.map.lock().unwrap().insert(session_key.to_string(), held);
    }

    /// A session's tokens (a copy: they never leave the server).
    pub fn get(&self, session_key: &str) -> Option<Held> {
        self.map.lock().unwrap().get(session_key).cloned()
    }

    pub fn len(&self) -> usize {
        self.map.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The session signs out: its tokens dropped and its backend session
    /// ended (best effort, logged).
    pub async fn sign_out(&self, session_key: &str) {
        let held = self.map.lock().unwrap().remove(session_key);
        if let Some(h) = held {
            match h.backend.sign_out(&h.tokens).await {
                Ok(()) => tracing::info!(backend = h.backend.name(), "the identity backend's session was ended"),
                Err(e) => tracing::warn!(backend = h.backend.name(), kind = e.kind(), "the identity backend's session could not be ended"),
            }
        }
    }

    /// Starts the refresher (once).
    pub fn start(self: &Arc<Self>) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(REFRESH_EVERY_SECS)).await;
                let Some(s) = me.upgrade() else { return };
                s.refresh_due(REFRESH_WITHIN_SECS).await;
            }
        });
    }

    /// One pass: tokens of ended sessions dropped (their backend sessions
    /// ended); access tokens expiring within `within` seconds refreshed; a
    /// refused refresh ends the app's session.
    pub async fn refresh_due(&self, within: i64) -> Refreshed {
        let mut out = Refreshed::default();
        let snapshot: Vec<(String, Held)> = self.map.lock().unwrap().iter().map(|(k, h)| (k.clone(), h.clone())).collect();
        for (key, h) in snapshot {
            if !self.sessions.alive(&key) {
                self.sign_out(&key).await;
                out.dropped += 1;
                continue;
            }
            if h.tokens.expires_at - now() > within || h.tokens.refresh.is_none() || !h.backend.capabilities().refresh {
                continue;
            }
            match h.backend.refresh(&h.tokens, &RequestInfo::default()).await {
                Ok(fresh) if fresh.ctx.principal().map(|p| p.to_string()).as_deref() == Some(h.principal.as_str()) => {
                    if let (Some(slot), Some(t)) = (self.map.lock().unwrap().get_mut(&key), fresh.tokens) {
                        slot.tokens = TokenSet { refresh: t.refresh.or(h.tokens.refresh.clone()), ..t };
                    }
                    out.refreshed += 1;
                }
                Ok(_) => {
                    tracing::warn!("a refresh named someone else: the session ends");
                    self.end(&key);
                    out.ended += 1;
                }
                Err(IdentityError::Unavailable(_) | IdentityError::RateLimited { .. }) => {
                    tracing::warn!(backend = h.backend.name(), "a session could not be refreshed now; trying again next pass");
                }
                Err(e) => {
                    tracing::info!(backend = h.backend.name(), refusal = e.kind(), "the identity backend refused a session's refresh: signed out here too");
                    self.end(&key);
                    out.ended += 1;
                }
            }
        }
        out
    }

    fn end(&self, key: &str) {
        self.map.lock().unwrap().remove(key);
        self.sessions.end_key(key);
    }
}

/// plexus-login's own [`Continuation`]: a signed-in person gets the app's
/// session and its cookie; the backend's tokens stay in the [`TokenStore`].
pub struct AppContinuation {
    pub sessions: Arc<Sessions>,
    pub tokens: Arc<TokenStore>,
    pub chrome: Chrome,
    /// Over https: `Secure` cookies.
    pub secure: bool,
}

#[async_trait]
impl Continuation for AppContinuation {
    async fn signed_in(&self, who: SignedIn, backend: Arc<dyn IdentityBackend>, next: &str) -> Response {
        let principal = who.ctx.principal().map(|p| p.to_string()).unwrap_or_default();
        let (id, expires) = self.sessions.create(who.ctx);
        if let (Some(tokens), Some(key)) = (who.tokens, self.sessions.get(&id).and_then(|c| c.get_metadata_string(self.sessions.session_key()))) {
            self.tokens.insert(&key, Held { tokens, principal, backend });
            self.tokens.start();
        }
        let secure = if self.secure { "; Secure" } else { "" };
        let max_age = (expires - now()).max(0);
        let mut r = (StatusCode::SEE_OTHER, [(header::LOCATION, next.to_string()), (header::CACHE_CONTROL, "no-store".to_string())]).into_response();
        if let Ok(v) = HeaderValue::from_str(&format!("{}={id}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}", self.sessions.cookie_name())) {
            r.headers_mut().append(header::SET_COOKIE, v);
        }
        r
    }

    fn account(&self, headers: &HeaderMap) -> Option<AccountHolder> {
        let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
        let ctx = self.sessions.get(sessions::cookie(cookies, self.sessions.cookie_name())?)?;
        let int = |k: &str| ctx.metadata.get(k).and_then(Value::as_i64);
        Some(AccountHolder {
            subject: ctx.get_metadata_string("subject")?,
            backend: ctx.get_metadata_string("identity_backend")?,
            email: ctx.get_metadata_string("email"),
            signed_in_at: int("auth_time").unwrap_or(0),
        })
    }

    fn draw(&self, body: Body) -> String {
        page::render(&self.chrome, body)
    }
}
