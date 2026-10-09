//! IDENTITY-19, IDENTITY-21: **access** — the one rule in front of every
//! method.
//!
//! A connection's identity is settled at the WebSocket upgrade and then
//! rides every call on it, so the rule is asked per call: a session can end
//! (sign-out), a credential can expire, and a role can be granted or
//! revoked while the socket stays open. [`Access::admit`] answers who the
//! call acts as — a [`Caller`] — or why it is refused, and every method of
//! `tickets`, `comments` and `members` asks it first.
//!
//! **Without login** ([`Access::open`]) every caller reaches everything, as
//! menger always did. **With login**, menger's own members decide
//! (IDENTITY-21): a principal holds `viewer`, `editor` or `admin` on a
//! scope — a shortcode, covering the shortcodes hosted under it, or `*`.
//! Reads are answered under the caller's filter AND their viewer scopes
//! ([`Caller::scoped`]); a write needs the role on the tickets it touches
//! ([`Caller::require`]). Someone signed in with no grant reaches nothing,
//! and waits in [`Access::waiting`] for an admin.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use plexus_core::plexus::AuthContext;

use crate::filter::Filter;
use crate::members::{Grant, Role, EVERYWHERE};
use crate::store::{Lens, Store};

/// The metadata key a session-backed identity carries: the key
/// [`SessionCheck::alive`] is asked about (menger's `sessions`).
pub const SESSION_KEY: &str = "menger_session";

/// The grantor named on `--admin` grants.
pub const OPERATOR: &str = "--admin";

/// Whether a session the server issued still stands.
pub trait SessionCheck: Send + Sync {
    fn alive(&self, key: &str) -> bool;
}

/// Someone signed in with no grant yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    pub principal: String,
    pub name: Option<String>,
    pub provider: Option<String>,
    pub since: i64,
}

/// How far a caller reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// Login is off: everything, as always.
    All,
    /// The caller's grants, `(scope, role)`.
    Grants(Vec<(String, Role)>),
}

/// Who a call acts as, once admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// The Plexus principal, or `None` for an anonymous caller.
    pub principal: Option<String>,
    /// The name to show, when signed in.
    pub name: Option<String>,
    pub reach: Reach,
}

impl Caller {
    fn everyone() -> Self {
        Self { principal: None, name: None, reach: Reach::All }
    }

    /// The scopes this caller holds `role` (or more) on; `None`: everywhere.
    pub fn scopes(&self, role: Role) -> Option<Vec<String>> {
        match &self.reach {
            Reach::All => None,
            Reach::Grants(g) if g.iter().any(|(s, r)| s == EVERYWHERE && *r >= role) => None,
            Reach::Grants(g) => Some(g.iter().filter(|(_, r)| *r >= role).map(|(s, _)| s.clone()).collect()),
        }
    }

    /// Whether this caller holds `role` everywhere.
    pub fn everywhere(&self, role: Role) -> bool {
        self.scopes(role).is_none()
    }

    fn who(&self) -> String {
        self.principal.clone().unwrap_or_else(|| "an anonymous caller".into())
    }

    fn no_access(&self) -> String {
        match &self.principal {
            Some(p) => format!("no access yet: ask an admin to grant {p} a role"),
            None => "sign in first".into(),
        }
    }

    /// The filter `under` the scopes this caller holds `role` on: `None`
    /// everywhere, `Err` when none.
    pub fn reach_filter(&self, role: Role) -> Result<Option<Filter>, String> {
        let Some(scopes) = self.scopes(role) else { return Ok(None) };
        if scopes.is_empty() {
            return Err(self.no_access());
        }
        let text = match scopes.as_slice() {
            [one] => format!("under {one}"),
            many => format!("({})", many.iter().map(|s| format!("under {s}")).collect::<Vec<_>>().join(" or ")),
        };
        Filter::parse(&text).map(Some).map_err(|e| e.to_string())
    }

    /// A read's filter text, ANDed with the scopes this caller may see:
    /// what the method answers under instead. Unchanged when the caller
    /// sees everything; `Err` when it sees nothing.
    pub fn scoped(&self, text: Option<&str>) -> Result<Option<String>, String> {
        let Some(reach) = self.reach_filter(Role::Viewer)? else { return Ok(text.map(str::to_string)) };
        let asked = match text.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => Filter::parse(t).map_err(|e| e.to_string())?,
            None => return Ok(Some(reach.format())),
        };
        Ok(Some(asked.and(&reach).format()))
    }

    /// [`Caller::scoped`] for a filter that must be given (`""`: none).
    pub fn scoped_text(&self, text: &str) -> Result<String, String> {
        Ok(self.scoped(Some(text))?.unwrap_or_default())
    }

    /// Whether this caller may `role` every one of `ids` — `Err` naming the
    /// first it may not, and the shortcode a grant would be needed on.
    pub fn require(&self, role: Role, ids: &[&str], store: &Store) -> Result<(), String> {
        let Some(reach) = self.reach_filter(role).map_err(|_| self.lacks(role, ids.first().copied()))? else {
            return Ok(());
        };
        let lens = Lens::of(&reach).map_err(|e| e.to_string())?;
        let owned: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        match store.member(&owned, &lens).iter().position(|ok| !ok) {
            None => Ok(()),
            Some(i) => Err(self.lacks(role, Some(ids[i]))),
        }
    }

    fn lacks(&self, role: Role, id: Option<&str>) -> String {
        match id {
            Some(id) => {
                let shortcode = id.split(['-', '/']).next().unwrap_or(id);
                format!("{} needs {role} on {shortcode} for {id}", self.who())
            }
            None => format!("{} needs {role} here", self.who()),
        }
    }
}

/// The rule. Cheap to clone: one per activation, all sharing state.
#[derive(Clone, Default)]
pub struct Access {
    login: bool,
    require_login: bool,
    sessions: Option<Arc<dyn SessionCheck>>,
    store: Arc<Mutex<Option<Arc<Store>>>>,
    admins: Arc<Vec<String>>,
    waiting: Arc<Mutex<BTreeMap<String, Waiting>>>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Access {
    /// No login: every call is admitted and reaches everything.
    pub fn open() -> Self {
        Self::default()
    }

    /// Login is on: sessions are checked on every call, members decide
    /// what each caller reaches, and with `require_login` an anonymous
    /// call is refused. `admins` hold admin on `*` from the operator.
    pub fn with_login(sessions: Arc<dyn SessionCheck>, require_login: bool, admins: Vec<String>) -> Self {
        Self { login: true, require_login, sessions: Some(sessions), admins: Arc::new(admins), ..Self::default() }
    }

    /// The store whose members decide (the builder attaches it).
    pub fn attach(&self, store: Arc<Store>) {
        *self.store.lock().unwrap() = Some(store);
    }

    /// Whether login is on.
    pub fn login(&self) -> bool {
        self.login
    }

    fn store(&self) -> Option<Arc<Store>> {
        self.store.lock().unwrap().clone()
    }

    /// A principal's grants: the stored ones, and admin on `*` for an
    /// operator's `--admin`.
    pub fn grants_of(&self, principal: &str) -> Vec<Grant> {
        let mut grants = self.store().map(|s| s.grants_of(principal)).unwrap_or_default();
        if self.admins.iter().any(|a| a == principal) {
            grants.push(Grant {
                principal: principal.into(),
                scope: EVERYWHERE.into(),
                role: Role::Admin,
                granted_by: OPERATOR.into(),
                at: 0,
            });
        }
        grants
    }

    /// The operator's `--admin` principals.
    pub fn admins(&self) -> Vec<String> {
        self.admins.as_ref().clone()
    }

    /// A grant was made: its principal waits no longer.
    pub fn granted(&self, principal: &str) {
        self.waiting.lock().unwrap().remove(principal);
    }

    /// Everyone signed in who holds no grant yet, by when they first came.
    pub fn waiting(&self) -> Vec<Waiting> {
        let mut w: Vec<Waiting> = self.waiting.lock().unwrap().values().cloned().collect();
        w.sort_by_key(|w| w.since);
        w
    }

    /// The caller a call acts as, or why it is refused (an in-band error's
    /// message).
    pub fn admit(&self, auth: Option<&AuthContext>) -> Result<Caller, String> {
        if !self.login {
            return Ok(Caller::everyone());
        }
        let Some(ctx) = auth.filter(|c| c.is_authenticated()) else {
            return if self.require_login {
                Err("sign in first: this menger requires it".into())
            } else {
                Ok(Caller { principal: None, name: None, reach: Reach::Grants(Vec::new()) })
            };
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
        let name = ctx.get_metadata_string("name");
        let grants: Vec<(String, Role)> = self.grants_of(&principal).into_iter().map(|g| (g.scope, g.role)).collect();
        {
            let mut waiting = self.waiting.lock().unwrap();
            if grants.is_empty() {
                waiting.entry(principal.clone()).or_insert_with(|| Waiting {
                    principal: principal.clone(),
                    name: name.clone(),
                    provider: ctx.get_metadata_string("provider"),
                    since: now(),
                });
            } else {
                waiting.remove(&principal);
            }
        }
        Ok(Caller { principal: Some(principal), name, reach: Reach::Grants(grants) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Kind;
    use serde_json::json;
    use std::collections::HashSet;

    #[derive(Default)]
    struct Live(Mutex<HashSet<String>>);
    impl SessionCheck for Live {
        fn alive(&self, key: &str) -> bool {
            self.0.lock().unwrap().contains(key)
        }
    }

    const ALICE: &str = "idp:c952acd3-60aa-4534-b2e3-83634591fee1";

    fn ctx(meta: serde_json::Value) -> AuthContext {
        AuthContext::new(ALICE.into(), "s".into(), vec![], meta)
    }

    fn grant(principal: &str, scope: &str, role: Role) -> Grant {
        Grant { principal: principal.into(), scope: scope.into(), role, granted_by: "test".into(), at: 1 }
    }

    /// Two shortcodes, IDENTITY hosted under PLEXUS-1, and OUGHT apart.
    async fn store() -> Arc<Store> {
        let s = Arc::new(Store::new());
        let mk = |e: &str, parent: Option<&str>| (e.to_string(), parent.map(str::to_string));
        let new = |epic: &str, kind: Kind, parent: Option<String>| crate::store::NewTicket {
            epic: epic.into(),
            title: format!("{epic} {kind:?}"),
            kind,
            severity: None,
            blocked_by: Vec::new(),
            parent,
            path: None,
            body: String::new(),
        };
        for (epic, parent) in [mk("PLEXUS", None), mk("IDENTITY", Some("PLEXUS-1")), mk("OUGHT", None)] {
            s.create(new(&epic, Kind::Scope, parent)).await.unwrap();
            s.create(new(&epic, Kind::Build, Some(format!("{epic}-1")))).await.unwrap();
        }
        s
    }

    fn login() -> Access {
        Access::with_login(Arc::new(Live::default()), true, vec![])
    }

    #[test]
    fn without_login_everyone_reaches_everything() {
        let c = Access::open().admit(None).unwrap();
        assert!(c.everywhere(Role::Admin));
        assert_eq!(c.scoped(Some("status Ready")).unwrap().as_deref(), Some("status Ready"));
    }

    #[test]
    fn require_login_refuses_the_anonymous() {
        let a = Access::with_login(Arc::new(Live::default()), true, vec![]);
        assert!(a.admit(None).is_err());
        assert!(a.admit(Some(&AuthContext::anonymous())).is_err());
    }

    #[test]
    fn a_session_signed_out_or_expired_is_refused() {
        let live = Arc::new(Live::default());
        live.0.lock().unwrap().insert("k1".into());
        let a = Access::with_login(live.clone(), true, vec![]);
        let ok = ctx(json!({ SESSION_KEY: "k1", "expires_at": now() + 60, "name": "alice" }));
        assert_eq!(a.admit(Some(&ok)).unwrap().name.as_deref(), Some("alice"));
        live.0.lock().unwrap().clear();
        assert!(a.admit(Some(&ok)).unwrap_err().contains("signed out"));
        let expired = ctx(json!({ "expires_at": now() - 1 }));
        assert!(a.admit(Some(&expired)).unwrap_err().contains("expired"));
    }

    #[tokio::test]
    async fn no_grant_reaches_nothing_and_waits() {
        let a = login();
        a.attach(store().await);
        let c = a.admit(Some(&ctx(json!({ "name": "alice", "provider": "plexus-idp" })))).unwrap();
        assert!(c.scoped(None).unwrap_err().contains("ask an admin to grant"));
        assert_eq!(a.waiting()[0].principal, ALICE);
    }

    #[tokio::test]
    async fn reads_are_anded_with_the_viewer_scopes() {
        let a = login();
        let s = store().await;
        s.grant(grant(ALICE, "IDENTITY", Role::Editor)).await.unwrap();
        s.grant(grant(ALICE, "OUGHT", Role::Viewer)).await.unwrap();
        a.attach(s);
        let c = a.admit(Some(&ctx(json!({})))).unwrap();
        // The language's canonical spelling of (under IDENTITY or under OUGHT).
        assert_eq!(c.scoped(None).unwrap().unwrap(), "under in (IDENTITY, OUGHT)");
        assert_eq!(c.scoped(Some("status Ready")).unwrap().unwrap(), "status Ready and under in (IDENTITY, OUGHT)");
        assert!(a.waiting().is_empty(), "a member does not wait");
    }

    #[tokio::test]
    async fn writes_need_the_role_on_the_ticket_s_shortcode_or_a_host_of_it() {
        let a = login();
        let s = store().await;
        s.grant(grant(ALICE, "IDENTITY", Role::Editor)).await.unwrap();
        s.grant(grant(ALICE, "OUGHT", Role::Viewer)).await.unwrap();
        a.attach(s.clone());
        let c = a.admit(Some(&ctx(json!({})))).unwrap();
        assert!(c.require(Role::Editor, &["IDENTITY-2"], &s).is_ok());
        assert!(c.require(Role::Viewer, &["OUGHT-2"], &s).is_ok());
        let e = c.require(Role::Editor, &["OUGHT-2"], &s).unwrap_err();
        assert!(e.contains("needs editor on OUGHT"), "{e}");
        assert!(c.require(Role::Viewer, &["PLEXUS-2"], &s).is_err());
        // A grant on PLEXUS covers IDENTITY, hosted under PLEXUS-1.
        s.grant(grant(ALICE, "PLEXUS", Role::Viewer)).await.unwrap();
        let c = a.admit(Some(&ctx(json!({})))).unwrap();
        assert!(c.require(Role::Viewer, &["IDENTITY-1", "PLEXUS-2"], &s).is_ok());
    }

    #[tokio::test]
    async fn an_operator_admin_is_admin_everywhere() {
        let a = Access::with_login(Arc::new(Live::default()), true, vec![ALICE.into()]);
        a.attach(store().await);
        let c = a.admit(Some(&ctx(json!({})))).unwrap();
        assert!(c.everywhere(Role::Admin));
        assert_eq!(a.grants_of(ALICE)[0].granted_by, OPERATOR);
    }
}
