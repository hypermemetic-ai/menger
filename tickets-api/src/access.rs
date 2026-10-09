//! IDENTITY-19: **access** — the one rule in front of every method.
//!
//! A connection's identity is settled at the WebSocket upgrade and then
//! rides every call on it, so a rule about *who* has to be asked per call:
//! a session can end (sign-out), and a credential can expire, while the
//! socket stays open. [`Access::admit`] is that rule, and every method of
//! `tickets` and `comments` asks it first; the answer is the [`Caller`] the
//! method acts as, or the in-band error it yields instead.
//!
//! | menger runs | anonymous call | signed-out or expired identity |
//! | --- | --- | --- |
//! | without login ([`Access::open`]) | admitted | (none arrive) |
//! | with providers, anonymous admitted | admitted | refused |
//! | with `--require-login` | refused | refused |
//!
//! IDENTITY-21 adds the members and their roles per shortcode to it.

use std::sync::Arc;

use plexus_core::plexus::AuthContext;

/// The metadata key a session-backed identity carries: the key
/// [`SessionCheck::alive`] is asked about (menger's `sessions`).
pub const SESSION_KEY: &str = "menger_session";

/// Whether a session the server issued still stands.
pub trait SessionCheck: Send + Sync {
    fn alive(&self, key: &str) -> bool;
}

/// Who a call acts as, once admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// The Plexus principal, or `None` for an anonymous caller.
    pub principal: Option<String>,
    /// The name to show, when signed in.
    pub name: Option<String>,
}

impl Caller {
    fn anonymous() -> Self {
        Self { principal: None, name: None }
    }
}

/// The rule. Cheap to clone: one per activation.
#[derive(Clone, Default)]
pub struct Access {
    require_login: bool,
    sessions: Option<Arc<dyn SessionCheck>>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Access {
    /// No login: every call is admitted, as menger always was.
    pub fn open() -> Self {
        Self::default()
    }

    /// Login is on: sessions are checked on every call, and with
    /// `require_login` an anonymous call is refused.
    pub fn with_login(sessions: Arc<dyn SessionCheck>, require_login: bool) -> Self {
        Self { require_login, sessions: Some(sessions) }
    }

    /// The caller a call acts as, or why it is refused (an in-band error's
    /// message).
    pub fn admit(&self, auth: Option<&AuthContext>) -> Result<Caller, String> {
        let Some(ctx) = auth.filter(|c| c.is_authenticated()) else {
            return if self.require_login { Err("sign in first: this menger requires it".into()) } else { Ok(Caller::anonymous()) };
        };
        if let Some(exp) = ctx.metadata.get("expires_at").and_then(|v| v.as_i64()) {
            if exp <= now() {
                return Err("your sign-in has expired: sign in again".into());
            }
        }
        if let Some(key) = ctx.get_metadata_string(SESSION_KEY) {
            if !self.sessions.as_ref().is_some_and(|s| s.alive(&key)) {
                return Err("you have signed out: sign in again".into());
            }
        }
        let principal = ctx.principal().map(|p| p.to_string()).unwrap_or_else(|| ctx.user_id.clone());
        Ok(Caller { principal: Some(principal), name: ctx.get_metadata_string("name") })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Live(Mutex<HashSet<String>>);
    impl SessionCheck for Live {
        fn alive(&self, key: &str) -> bool {
            self.0.lock().unwrap().contains(key)
        }
    }

    fn ctx(meta: serde_json::Value) -> AuthContext {
        AuthContext::new("idp:c952acd3-60aa-4534-b2e3-83634591fee1".into(), "s".into(), vec![], meta)
    }

    #[test]
    fn without_login_everyone_is_admitted() {
        assert_eq!(Access::open().admit(None), Ok(Caller::anonymous()));
    }

    #[test]
    fn require_login_refuses_the_anonymous() {
        let a = Access::with_login(Arc::new(Live::default()), true);
        assert!(a.admit(None).is_err());
        assert!(a.admit(Some(&AuthContext::anonymous())).is_err());
        let b = Access::with_login(Arc::new(Live::default()), false);
        assert_eq!(b.admit(None), Ok(Caller::anonymous()));
    }

    #[test]
    fn a_session_signed_out_or_expired_is_refused() {
        let live = Arc::new(Live::default());
        live.0.lock().unwrap().insert("k1".into());
        let a = Access::with_login(live.clone(), true);
        let ok = ctx(json!({ SESSION_KEY: "k1", "expires_at": now() + 60, "name": "alice" }));
        let caller = a.admit(Some(&ok)).unwrap();
        assert_eq!(caller.name.as_deref(), Some("alice"));
        assert!(caller.principal.unwrap().starts_with("idp:"));
        live.0.lock().unwrap().clear();
        assert!(a.admit(Some(&ok)).unwrap_err().contains("signed out"));
        let expired = ctx(json!({ "expires_at": now() - 1 }));
        assert!(a.admit(Some(&expired)).unwrap_err().contains("expired"));
        let token = ctx(json!({ "expires_at": now() + 60 }));
        assert!(a.admit(Some(&token)).is_ok(), "a token's identity needs no session");
    }
}
