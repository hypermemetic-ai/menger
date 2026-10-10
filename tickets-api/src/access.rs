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
//!
//! IDENTITY-42: a session minted by `session.login` may carry a **ceiling**
//! ([`CEILING_KEY`]) — `role@SCOPE` entries, the same roles and the same
//! `under` coverage as grants (a SCOPE may also be one ticket, covering it
//! and its descendants). Every check then passes only if the principal's
//! grants AND the ceiling allow it: [`Caller::reach_filter`] is the grants'
//! filter ANDed with each ceiling's, so `scoped`, `require`, the comment
//! roles and every lens built from it inherit it; [`Caller::scopes`] and
//! [`Caller::everywhere`] (the members checks) meet the two the same way.
//! A session minted from a session keeps its ceiling and adds the new one,
//! so it is never wider.

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

/// IDENTITY-35: `granted_by` of the grants the dev sign-in makes.
pub const DEV_LOGIN: &str = "dev-login";

/// IDENTITY-37: the most principals [`Access::waiting`] keeps — past it,
/// the one waiting longest is forgotten (it is listed again when it comes
/// back), so sign-ins without a grant cannot grow it without bound.
pub const WAITING_MAX: usize = 1000;

/// IDENTITY-42: the metadata key a minted session's ceiling rides under:
/// a list of layers, each a list of `role@SCOPE`; every layer must allow a
/// check.
pub const CEILING_KEY: &str = "ceiling";

/// IDENTITY-42: the metadata key a key proof's agent id rides under.
pub const AGENT_KEY: &str = "agent";

/// IDENTITY-42: how long a minted session lasts unless asked, and at most.
pub const MINT_TTL_DEFAULT_SECS: u64 = 12 * 60 * 60;
pub const MINT_TTL_MAX_SECS: u64 = 24 * 60 * 60;

/// The metadata a minted session keeps from the identity that asked for it.
const KEPT_ON_MINT: &[&str] = &["principal", AGENT_KEY, "name", "username", "provider", "auth_method", "org_id", "tenant_id"];

/// Whether a session the server issued still stands.
pub trait SessionCheck: Send + Sync {
    fn alive(&self, key: &str) -> bool;
}

/// IDENTITY-42: the session table, as `session.login` and `session.logout`
/// reach it (menger's `sessions`).
pub trait SessionMint: Send + Sync {
    /// Starts a session for `ctx`, lasting `ttl_secs`: the token that opens
    /// it and when it ends. The token is kept only hashed; `ctx` is stamped
    /// with [`SESSION_KEY`] and `expires_at`.
    fn mint(&self, ctx: AuthContext, ttl_secs: i64) -> (String, i64);
    /// Ends the session whose [`SESSION_KEY`] is `key`; whether it stood.
    fn end_key(&self, key: &str) -> bool;
}

/// One ceiling: `(scope, role)` entries.
pub type Ceiling = Vec<(String, Role)>;

/// A ceiling's scope: `*`, a shortcode, or one ticket (`SHORTCODE-<n>`),
/// in upper case.
pub fn ceiling_scope_of(text: &str) -> Result<String, String> {
    let t = text.trim();
    let ticket = t.split_once('-').filter(|(code, n)| {
        crate::members::scope_of(code).is_ok_and(|c| c != EVERYWHERE) && !n.is_empty() && n.len() <= 12 && n.bytes().all(|b| b.is_ascii_digit())
    });
    match ticket {
        Some((code, n)) => Ok(format!("{}-{n}", code.to_ascii_uppercase())),
        None => crate::members::scope_of(t)
            .map_err(|_| format!("`{t}` is not a scope: a shortcode (e.g. IDENTITY), a ticket (e.g. IDENTITY-12) or `*`")),
    }
}

/// One `role@SCOPE` entry of a ceiling.
pub fn ceiling_entry(text: &str) -> Result<(String, Role), String> {
    let (role, scope) = text
        .trim()
        .split_once('@')
        .ok_or_else(|| format!("`{}` is not role@SCOPE (e.g. editor@IDENTITY, viewer@*)", text.trim()))?;
    Ok((ceiling_scope_of(scope)?, role.parse()?))
}

/// A ceiling as the wire says it: `role@SCOPE` each.
pub fn ceiling_text(ceiling: &[(String, Role)]) -> Vec<String> {
    ceiling.iter().map(|(s, r)| format!("{r}@{s}")).collect()
}

/// The ceilings a session's metadata carries: `[]` when none; `Err` when
/// they cannot be read (the caller is then refused, never widened).
fn ceilings_of(ctx: &AuthContext) -> Result<Vec<Ceiling>, String> {
    let Some(v) = ctx.metadata.get(CEILING_KEY) else { return Ok(Vec::new()) };
    let unreadable = || "this session's scope cannot be read: sign in again".to_string();
    v.as_array()
        .ok_or_else(unreadable)?
        .iter()
        .map(|layer| {
            layer
                .as_array()
                .ok_or_else(unreadable)?
                .iter()
                .map(|e| e.as_str().ok_or_else(unreadable).and_then(|e| ceiling_entry(e).map_err(|_| unreadable())))
                .collect()
        })
        .collect()
}

/// The scopes `entries` hold `role` (or more) on; `None`: everywhere.
fn holding(entries: &[(String, Role)], role: Role) -> Option<Vec<String>> {
    if entries.iter().any(|(s, r)| s == EVERYWHERE && *r >= role) {
        return None;
    }
    Some(entries.iter().filter(|(_, r)| *r >= role).map(|(s, _)| s.clone()).collect())
}

/// `under` each of `scopes`, or'd: the filter text one grant list or one
/// ceiling covers.
fn under_text(scopes: &[String]) -> String {
    match scopes {
        [one] => format!("under {one}"),
        many => format!("({})", many.iter().map(|s| format!("under {s}")).collect::<Vec<_>>().join(" or ")),
    }
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
    /// IDENTITY-42: the session's ceilings — every one must allow a check,
    /// as the grants must. Empty: none. Fixed for a connection's life.
    pub ceiling: Vec<Ceiling>,
}

impl Caller {
    fn everyone() -> Self {
        Self { principal: None, name: None, reach: Reach::All, ceiling: Vec::new() }
    }

    /// The scopes the grants alone give `role` (or more) on; `None`:
    /// everywhere.
    fn granted(&self, role: Role) -> Option<Vec<String>> {
        match &self.reach {
            Reach::All => None,
            Reach::Grants(g) => holding(g, role),
        }
    }

    /// The scopes this caller holds `role` (or more) on; `None`: everywhere.
    /// IDENTITY-42: under a ceiling, a scope both the grants and every
    /// ceiling name (or that one names and the other holds everywhere) — a
    /// conservative meet; [`Caller::reach_filter`] is the exact one.
    pub fn scopes(&self, role: Role) -> Option<Vec<String>> {
        let mut acc = self.granted(role);
        for layer in &self.ceiling {
            acc = match (acc, holding(layer, role)) {
                (None, other) | (other, None) => other,
                (Some(a), Some(b)) => Some(a.into_iter().filter(|s| b.contains(s)).collect()),
            };
        }
        acc
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
    ///
    /// IDENTITY-42: the grants' filter AND each ceiling's: a ticket is in
    /// reach only when both cover it.
    pub fn reach_filter(&self, role: Role) -> Result<Option<Filter>, String> {
        let mut parts = Vec::new();
        if let Some(scopes) = self.granted(role) {
            if scopes.is_empty() {
                return Err(self.no_access());
            }
            parts.push(under_text(&scopes));
        }
        for layer in &self.ceiling {
            if let Some(scopes) = holding(layer, role) {
                if scopes.is_empty() {
                    return Err(format!("this session's scope ({}) holds no {role}", ceiling_text(layer).join(", ")));
                }
                parts.push(under_text(&scopes));
            }
        }
        if parts.is_empty() {
            return Ok(None);
        }
        Filter::parse(&parts.join(" and ")).map(Some).map_err(|e| e.to_string())
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

    /// Why this caller may not `role` the ticket `id` (or anything here).
    /// IDENTITY-37: it names only the id's own shortcode, read from the id,
    /// so it is the same whether the ticket exists or not.
    pub fn lacks(&self, role: Role, id: Option<&str>) -> String {
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
    minter: Option<Arc<dyn SessionMint>>,
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

    /// IDENTITY-42: `session.login` and `session.logout` mint and end
    /// sessions in `minter` (menger's session table).
    pub fn minting(mut self, minter: Arc<dyn SessionMint>) -> Self {
        self.minter = Some(minter);
        self
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

    /// IDENTITY-35: a grant the operator's dev sign-in makes (test only),
    /// kept in the store like any other and signed `granted_by` `dev-login`.
    pub async fn dev_grant(&self, principal: &str, scope: &str, role: Role) -> Result<Grant, String> {
        let store = self.store().ok_or("no store is attached")?;
        let scope = crate::members::scope_of(scope)?;
        let g = Grant { principal: principal.into(), scope, role, granted_by: DEV_LOGIN.into(), at: now() };
        let g = store.grant(g).await.map_err(|e| e.to_string())?;
        self.granted(principal);
        Ok(g)
    }

    /// IDENTITY-35: a roster user's grants made exactly `grants`: each kept,
    /// and every other stored grant of theirs revoked.
    pub async fn dev_set_grants(&self, principal: &str, grants: &[(Role, String)]) -> Result<(), String> {
        let store = self.store().ok_or("no store is attached")?;
        for g in store.grants_of(principal) {
            if !grants.iter().any(|(_, s)| *s == g.scope) {
                store.revoke(principal.into(), g.scope).await.map_err(|e| e.to_string())?;
            }
        }
        for (role, scope) in grants {
            self.dev_grant(principal, scope, *role).await?;
        }
        Ok(())
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
                Ok(Caller { principal: None, name: None, reach: Reach::Grants(Vec::new()), ceiling: Vec::new() })
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
        let ceiling = ceilings_of(ctx)?;
        let principal = ctx.principal().map(|p| p.to_string()).unwrap_or_else(|| ctx.user_id.clone());
        let name = ctx.get_metadata_string("name");
        let grants: Vec<(String, Role)> = self.grants_of(&principal).into_iter().map(|g| (g.scope, g.role)).collect();
        {
            let mut waiting = self.waiting.lock().unwrap();
            if grants.is_empty() {
                if !waiting.contains_key(&principal) {
                    // IDENTITY-37: bounded — the one waiting longest goes.
                    while waiting.len() >= WAITING_MAX {
                        let oldest = waiting.values().min_by_key(|w| w.since).map(|w| w.principal.clone());
                        match oldest {
                            Some(p) => waiting.remove(&p),
                            None => break,
                        };
                    }
                    waiting.insert(
                        principal.clone(),
                        Waiting { principal: principal.clone(), name: name.clone(), provider: ctx.get_metadata_string("provider"), since: now() },
                    );
                }
            } else {
                waiting.remove(&principal);
            }
        }
        Ok(Caller { principal: Some(principal), name, reach: Reach::Grants(grants), ceiling })
    }

    /// IDENTITY-42, `session.login`: a session for the caller, lasting
    /// `ttl_secs` (default 12 h, at most 24 h), held to `scope` when given —
    /// and to any ceiling the caller's own session holds, so it is never
    /// wider. Any signed-in caller may mint one; it acts as the caller's
    /// principal, agent, name and provider.
    pub fn mint(&self, auth: Option<&AuthContext>, ttl_secs: Option<u64>, scope: Option<Vec<String>>) -> Result<Minted, String> {
        let Some(minter) = self.minter.as_ref().filter(|_| self.login) else {
            return Err("login is off on this menger: there is no session to log in".into());
        };
        let caller = self.admit(auth)?;
        let (Some(principal), Some(ctx)) = (caller.principal.clone(), auth.filter(|c| c.is_authenticated())) else {
            return Err("sign in first: only a signed-in caller can log in a session".into());
        };
        let ttl = ttl_secs.unwrap_or(MINT_TTL_DEFAULT_SECS);
        if ttl == 0 || ttl > MINT_TTL_MAX_SECS {
            return Err(format!("ttl_secs is 1 to {MINT_TTL_MAX_SECS} (24 h); default {MINT_TTL_DEFAULT_SECS} (12 h)"));
        }
        let asked: Option<Ceiling> = match scope {
            None => None,
            Some(entries) => {
                let entries: Vec<String> = entries.iter().map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect();
                if entries.is_empty() {
                    return Err("a scope names at least one role@SCOPE (e.g. editor@IDENTITY)".into());
                }
                Some(entries.iter().map(|e| ceiling_entry(e)).collect::<Result<_, _>>()?)
            }
        };
        let mut ceiling = caller.ceiling.clone();
        ceiling.extend(asked.clone());
        let shown = asked.or_else(|| caller.ceiling.last().cloned()).map(|c| ceiling_text(&c)).unwrap_or_default();

        let mut meta = serde_json::Map::new();
        for key in KEPT_ON_MINT {
            if let Some(v) = ctx.metadata.get(*key) {
                meta.insert((*key).to_string(), v.clone());
            }
        }
        meta.insert("principal".into(), principal.clone().into());
        if !ceiling.is_empty() {
            let layers: Vec<serde_json::Value> = ceiling.iter().map(|c| ceiling_text(c).into()).collect();
            meta.insert(CEILING_KEY.into(), layers.into());
        }
        let agent = ctx.get_metadata_string(AGENT_KEY);
        // A session id that says what it is (the session's key rides in the
        // metadata): an empty one would read as no one.
        let minted = AuthContext::new(ctx.user_id.clone(), "mgs".into(), ctx.roles.clone(), serde_json::Value::Object(meta));
        let (token, expires_at) = minter.mint(minted, ttl as i64);
        Ok(Minted { token, principal, agent, expires_at, scope: shown })
    }

    /// IDENTITY-42, `session.logout`: ends the session the caller's
    /// connection is on. A connection a credential opened directly (a key
    /// proof, a provider's token) holds no session to end.
    pub fn logout(&self, auth: Option<&AuthContext>) -> Result<(), String> {
        let Some(minter) = self.minter.as_ref().filter(|_| self.login) else {
            return Err("login is off on this menger: there is no session to log out".into());
        };
        match auth.filter(|c| c.is_authenticated()).and_then(|c| c.get_metadata_string(SESSION_KEY)) {
            Some(key) => {
                minter.end_key(&key);
                Ok(())
            }
            None => Err("this connection holds no session to log out (session.login mints one)".into()),
        }
    }
}

/// IDENTITY-42: what `session.login` minted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Minted {
    pub token: String,
    pub principal: String,
    pub agent: Option<String>,
    pub expires_at: i64,
    pub scope: Vec<String>,
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
    async fn the_waiting_list_is_capped_and_forgets_the_longest_waiting() {
        let a = login();
        a.attach(store().await);
        let as_ = |p: String| AuthContext::new(p, "s".into(), vec![], json!({}));
        for i in 0..WAITING_MAX + 5 {
            a.admit(Some(&as_(format!("user-{i}")))).unwrap();
        }
        let w = a.waiting();
        assert_eq!(w.len(), WAITING_MAX);
        assert!(w.iter().any(|w| w.principal == format!("user-{}", WAITING_MAX + 4)), "the newest waits");
        // One already waiting is not added twice, and evicts no one.
        a.admit(Some(&as_(format!("user-{}", WAITING_MAX + 4)))).unwrap();
        assert_eq!(a.waiting().len(), WAITING_MAX);
    }

    #[tokio::test]
    async fn an_operator_admin_is_admin_everywhere() {
        let a = Access::with_login(Arc::new(Live::default()), true, vec![ALICE.into()]);
        a.attach(store().await);
        let c = a.admit(Some(&ctx(json!({})))).unwrap();
        assert!(c.everywhere(Role::Admin));
        assert_eq!(a.grants_of(ALICE)[0].granted_by, OPERATOR);
    }

    // ── IDENTITY-42: a session's ceiling ─────────────────────────────

    #[derive(Default)]
    struct Mint(Mutex<Vec<AuthContext>>);
    impl SessionMint for Mint {
        fn mint(&self, ctx: AuthContext, ttl: i64) -> (String, i64) {
            self.0.lock().unwrap().push(ctx);
            ("mgs_t".into(), now() + ttl)
        }
        fn end_key(&self, _: &str) -> bool {
            true
        }
    }

    fn ceiling(layers: &[&[&str]]) -> serde_json::Value {
        json!(layers.iter().map(|l| l.to_vec()).collect::<Vec<_>>())
    }

    #[test]
    fn a_ceiling_entry_is_role_at_scope() {
        assert_eq!(ceiling_entry("editor@identity"), Ok(("IDENTITY".into(), Role::Editor)));
        assert_eq!(ceiling_entry(" viewer@* "), Ok(("*".into(), Role::Viewer)));
        assert_eq!(ceiling_entry("Admin@menger-12"), Ok(("MENGER-12".into(), Role::Admin)));
        for bad in ["editor", "owner@X", "editor@", "editor@a b", "editor@X-", "editor@X-1a", "editor@*-1", "editor@under X"] {
            assert!(ceiling_entry(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn every_check_needs_the_grants_and_the_ceiling() {
        let a = Access::with_login(Arc::new(Live::default()), true, vec![ALICE.into()]);
        let s = store().await;
        a.attach(s.clone());
        // Admin on * from the operator, held to editor on IDENTITY.
        let c = a.admit(Some(&ctx(json!({ CEILING_KEY: ceiling(&[&["editor@IDENTITY"]]) })))).unwrap();
        assert!(c.require(Role::Editor, &["IDENTITY-2"], &s).is_ok());
        assert!(c.require(Role::Editor, &["OUGHT-2"], &s).is_err(), "elsewhere");
        assert!(c.require(Role::Viewer, &["PLEXUS-2"], &s).is_err(), "not even to read");
        assert!(c.require(Role::Admin, &["IDENTITY-2"], &s).is_err(), "no more than editor");
        assert!(!c.everywhere(Role::Viewer) && !c.everywhere(Role::Admin));
        assert_eq!(c.scopes(Role::Editor), Some(vec!["IDENTITY".into()]));
        assert_eq!(c.scopes(Role::Admin), Some(vec![]));
        assert_eq!(c.scoped(Some("status Ready")).unwrap().unwrap(), "status Ready and under IDENTITY");
        // One ticket: it and its descendants.
        let c = a.admit(Some(&ctx(json!({ CEILING_KEY: ceiling(&[&["editor@IDENTITY-2"]]) })))).unwrap();
        assert!(c.require(Role::Editor, &["IDENTITY-2"], &s).is_ok());
        assert!(c.require(Role::Editor, &["IDENTITY-1"], &s).is_err());
        // `*` in the ceiling is no ceiling at that role.
        let c = a.admit(Some(&ctx(json!({ CEILING_KEY: ceiling(&[&["admin@*"]]) })))).unwrap();
        assert!(c.everywhere(Role::Admin));
    }

    #[tokio::test]
    async fn a_ceiling_wider_than_the_grants_gives_no_more_than_the_grants() {
        let a = login();
        let s = store().await;
        s.grant(grant(ALICE, "OUGHT", Role::Viewer)).await.unwrap();
        a.attach(s.clone());
        let c = a.admit(Some(&ctx(json!({ CEILING_KEY: ceiling(&[&["admin@*"]]) })))).unwrap();
        assert!(c.require(Role::Viewer, &["OUGHT-2"], &s).is_ok());
        assert!(c.require(Role::Editor, &["OUGHT-2"], &s).is_err());
        assert!(c.require(Role::Viewer, &["IDENTITY-2"], &s).is_err());
    }

    #[tokio::test]
    async fn layered_ceilings_all_hold_and_an_unreadable_one_refuses() {
        let a = Access::with_login(Arc::new(Live::default()), true, vec![ALICE.into()]);
        let s = store().await;
        a.attach(s.clone());
        let c = a.admit(Some(&ctx(json!({ CEILING_KEY: ceiling(&[&["editor@PLEXUS"], &["admin@IDENTITY", "editor@OUGHT"]]) })))).unwrap();
        assert!(c.require(Role::Editor, &["IDENTITY-2"], &s).is_ok(), "IDENTITY is hosted under PLEXUS");
        assert!(c.require(Role::Admin, &["IDENTITY-2"], &s).is_err(), "the first holds editor");
        assert!(c.require(Role::Viewer, &["OUGHT-2"], &s).is_err(), "the first does not reach OUGHT");
        assert!(c.require(Role::Viewer, &["PLEXUS-2"], &s).is_err(), "the second does not reach PLEXUS-2");
        for bad in [json!("editor@X"), json!([["owner@X"]]), json!([[1]])] {
            assert!(a.admit(Some(&ctx(json!({ CEILING_KEY: bad.clone() })))).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_minted_session_keeps_the_caller_and_never_widens() {
        let mint = Arc::new(Mint::default());
        let a = Access::with_login(Arc::new(Live::default()), true, vec![ALICE.into()]).minting(mint.clone());
        a.attach(store().await);
        let agent = ctx(json!({ "agent": "worker-3", "name": "worker-3", "provider": "key", "auth_method": "key", "expires_at": now() + 300 }));
        let m = a.mint(Some(&agent), None, Some(vec!["editor@identity".into()])).unwrap();
        assert_eq!((m.principal.as_str(), m.agent.as_deref(), m.scope.clone()), (ALICE, Some("worker-3"), vec!["editor@IDENTITY".to_string()]));
        assert!(m.expires_at >= now() + MINT_TTL_DEFAULT_SECS as i64 - 5);
        let minted = mint.0.lock().unwrap().last().cloned().unwrap();
        assert_eq!(minted.metadata["agent"], "worker-3");
        assert_eq!(minted.metadata["provider"], "key");
        assert_eq!(minted.metadata["principal"], ALICE);
        assert_eq!(minted.metadata[CEILING_KEY], json!([["editor@IDENTITY"]]));

        // From that session: its ceiling stays, the new one is added.
        let m2 = a.mint(Some(&minted), Some(60), Some(vec!["admin@*".into()])).unwrap();
        assert_eq!(m2.scope, vec!["admin@*".to_string()]);
        let again = mint.0.lock().unwrap().last().cloned().unwrap();
        assert_eq!(again.metadata[CEILING_KEY], json!([["editor@IDENTITY"], ["admin@*"]]));
        assert!(!a.admit(Some(&again)).unwrap().everywhere(Role::Viewer), "still held to IDENTITY");
        // Asking nothing inherits it, and says so.
        assert_eq!(a.mint(Some(&minted), None, None).unwrap().scope, vec!["editor@IDENTITY".to_string()]);
        assert!(a.mint(Some(&agent), None, None).unwrap().scope.is_empty(), "no ceiling");

        for (ttl, scope) in [(Some(0), None), (Some(MINT_TTL_MAX_SECS + 1), None), (None, Some(vec![])), (None, Some(vec!["owner@X".into()]))] {
            assert!(a.mint(Some(&agent), ttl, scope.clone()).is_err(), "{ttl:?} {scope:?}");
        }
        assert!(a.mint(Some(&AuthContext::anonymous()), None, None).is_err());
        assert!(login().mint(Some(&agent), None, None).is_err(), "nothing to mint in");
        assert!(a.logout(Some(&agent)).is_err(), "a proof-opened connection holds no session");
        assert!(a.logout(Some(&ctx(json!({ SESSION_KEY: "k" })))).is_ok());
    }
}
