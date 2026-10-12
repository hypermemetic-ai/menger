//! SCAFFOLD-9: a ready-made [`LoginAccess`] — the rules an app starts from
//! before it has rules of its own.
//!
//! [`SimpleAccess`] keeps everything in memory, over plexus-core's own
//! reference stores ([`MemoryRoles`], [`MemoryMemberships`]): roles are
//! **global** (every grant is on `*`), the role vocabulary is the house's
//! three ([`SimpleRole`]: `viewer`, `editor` — as each activation's
//! `roles(...)` declares them — and `admin`, which holds every grant), the
//! first admins are named at startup (`--admin <principal>`), and everyone
//! else who signs in waits for an admin (`auth.waiting`; an admin answers
//! with `auth.grant`). Sessions are the login's own table. A restart
//! forgets every grant but the named admins'.
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use plexus_core::plexus::DynamicHub;
//! # use plexus_login::{Branding, LoginOptions, Wiring, simple::SimpleAccess};
//! # async fn run() -> anyhow::Result<()> {
//! const BRANDING: Branding = Branding { app: "myapp", version: "0.1.0", mark: "m", motto: "" };
//! let opts = LoginOptions::new(BRANDING, 4470, 4471).with_providers(["workos=client_01ABC"])?;
//! let wiring = Wiring::new(opts)?;
//! let access = SimpleAccess::new(wiring.sessions(), &["oidc:workos/user_01ADMIN".to_string()])?;
//! let wired = wiring.finish(access.clone(), None)?;
//! let hub = DynamicHub::new("myapp").with_auth(access.auth_setup(wired.backend.clone()));
//! access.seed_roles(&hub);
//! # Ok(()) }
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use plexus_auth_core::identity::Principal;
use plexus_auth_core::RoleName;
use plexus_core::plexus::{
    AuthBackend, AuthContext, AuthSetup, Ceiling, DynamicHub, Membership, MembershipStore, Memberships, MemoryMemberships,
    MemoryRoles, Resolve, ScopeRef, Session, Waiting, ADMIN, CEILING_KEY,
};
use serde_json::Value;

use crate::access::LoginAccess;
use crate::sessions::{self, Sessions};

/// A role [`SimpleAccess`] grants: the house's three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimpleRole {
    Viewer,
    Editor,
    Admin,
}

impl fmt::Display for SimpleRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Viewer => "viewer",
            Self::Editor => "editor",
            Self::Admin => ADMIN,
        })
    }
}

impl FromStr for SimpleRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "viewer" => Ok(Self::Viewer),
            "editor" => Ok(Self::Editor),
            "admin" => Ok(Self::Admin),
            other => Err(format!("unknown role `{other}` — the roles are viewer, editor, admin")),
        }
    }
}

/// How long a minted session lasts unless asked, and at most.
pub const MINT_TTL_DEFAULT_SECS: u64 = 12 * 60 * 60;
pub const MINT_TTL_MAX_SECS: u64 = 24 * 60 * 60;

/// The most principals the waiting list keeps; past it, the one waiting
/// longest is forgotten (it is listed again when it comes back).
pub const WAITING_MAX: usize = 1000;

/// The metadata a minted session keeps from the identity that asked for it.
const KEPT_ON_MINT: &[&str] = &["principal", "agent", "name", "username", "provider", "auth_method", "org_id", "tenant_id", "email"];

/// In-memory, global-role rules. See the module docs. Cheap to clone.
#[derive(Clone)]
pub struct SimpleAccess(Arc<Inner>);

struct Inner {
    sessions: Arc<Sessions>,
    roles: Arc<MemoryRoles>,
    memberships: Arc<MemoryMemberships>,
    waiting: Mutex<BTreeMap<String, Waiting>>,
}

/// Every scope lies in `*`: the only scope [`SimpleAccess`] grants on.
struct Global;

impl Resolve for Global {
    fn chain(&self, target: &str) -> Option<Vec<ScopeRef>> {
        Some(vec![ScopeRef::parse(target), ScopeRef::Everywhere])
    }
}

fn principal_of(ctx: &AuthContext) -> String {
    ctx.principal().map(|p| p.to_string()).unwrap_or_else(|| ctx.user_id.clone())
}

impl SimpleAccess {
    /// Rules over the login's session table, `admins` (principals:
    /// `idp:<uuid>`, `oidc:<provider>/<sub>`, `nostr:<hex>`, `key:<hex>`)
    /// admin everywhere from the start.
    pub fn new(sessions: &Arc<Sessions>, admins: &[String]) -> anyhow::Result<Self> {
        let mut memberships = MemoryMemberships::new();
        for a in admins {
            a.parse::<Principal>().map_err(|e| anyhow::anyhow!("--admin {a}: not a principal ({e})"))?;
            memberships = memberships.with(a, ADMIN, "*");
        }
        Ok(Self(Arc::new(Inner {
            sessions: sessions.clone(),
            roles: Arc::new(MemoryRoles::default()),
            memberships: Arc::new(memberships),
            waiting: Mutex::default(),
        })))
    }

    /// What the hub's `auth` activation takes
    /// (`DynamicHub::with_auth`): `backend` (the login's,
    /// [`crate::Wired::backend`]) over these roles and memberships, every
    /// scope in `*`.
    pub fn auth_setup(&self, backend: Arc<dyn AuthBackend>) -> AuthSetup {
        AuthSetup::new(backend, self.0.roles.clone(), self.0.memberships.clone(), Arc::new(Global))
    }

    /// The roles every activation of `hub` declares (`roles(...)`), as the
    /// role table's defaults — after its activations are registered. A
    /// [`SimpleRole`] no activation declares is still a role (holding no
    /// grant), so a grant of it is never refused.
    pub fn seed_roles(&self, hub: &DynamicHub) {
        let mut declared = hub.declared_roles();
        for role in [SimpleRole::Viewer, SimpleRole::Editor] {
            declared.entry(role.to_string()).or_default();
        }
        self.0.roles.seed(&declared);
    }

    /// The role table (`auth.roles`, `auth.role_set`).
    pub fn roles(&self) -> &Arc<MemoryRoles> {
        &self.0.roles
    }

    /// The memberships (`auth.grant`, `auth.revoke`).
    pub fn memberships(&self) -> &Arc<MemoryMemberships> {
        &self.0.memberships
    }

    /// Whether `principal` holds any role.
    pub fn holds_any(&self, principal: &str) -> bool {
        principal.parse::<Principal>().is_ok_and(|p| !self.0.memberships.held(&p).is_empty())
    }

    fn wait(&self, ctx: &AuthContext) -> Waiting {
        let principal = principal_of(ctx);
        let mut waiting = self.0.waiting.lock().unwrap_or_else(|p| p.into_inner());
        if !waiting.contains_key(&principal) && waiting.len() >= WAITING_MAX {
            if let Some(oldest) = waiting.values().min_by_key(|w| w.since).map(|w| w.principal.clone()) {
                waiting.remove(&oldest);
            }
        }
        waiting
            .entry(principal.clone())
            .or_insert_with(|| Waiting {
                principal,
                name: ctx.get_metadata_string("name"),
                provider: ctx.get_metadata_string("provider"),
                since: sessions::now(),
            })
            .clone()
    }

    fn start(&self, ctx: &AuthContext, meta: serde_json::Map<String, Value>, ttl_secs: u64, shown: Vec<String>) -> Session {
        let principal = principal_of(ctx);
        let minted = AuthContext::new(ctx.user_id.clone(), "mgs".into(), ctx.roles.clone(), Value::Object(meta));
        let (token, expires_at) = self.0.sessions.mint(minted, ttl_secs.clamp(1, MINT_TTL_MAX_SECS) as i64);
        Session { token, principal, agent: ctx.get_metadata_string("agent"), name: ctx.get_metadata_string("name"), expires_at, ceiling: shown }
    }

    fn membership(principal: &str, role: SimpleRole) -> Result<Membership, String> {
        Ok(Membership {
            principal: principal.parse().map_err(|e| format!("`{principal}` is not a principal ({e})"))?,
            role: RoleName::new(role.to_string()),
            scope: ScopeRef::Everywhere,
        })
    }
}

/// A ceiling entry, `role@*` or `grant@*`: what a minted session is held to.
fn ceiling_entry(text: &str) -> Result<String, String> {
    let (left, scope) =
        text.trim().split_once('@').ok_or_else(|| format!("`{}` is not role@SCOPE (e.g. viewer@*)", text.trim()))?;
    let word = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    let (left, scope) = (left.trim(), scope.trim());
    if !word(left) {
        return Err(format!("`{left}` is not a role or a grant"));
    }
    if !(scope == "*" || word(scope)) {
        return Err(format!("`{scope}` is not a scope"));
    }
    Ok(format!("{}@{scope}", left.to_ascii_lowercase()))
}

#[async_trait]
impl LoginAccess for SimpleAccess {
    type Role = SimpleRole;

    /// Roles are global: the one scope is `*`.
    fn scope_of(text: &str) -> Result<String, String> {
        match text.trim() {
            "" | "*" => Ok("*".into()),
            other => Err(format!("`{other}`: roles here are global — the scope is `*`")),
        }
    }

    async fn dev_grant(&self, principal: &str, _scope: &str, role: SimpleRole) -> Result<(), String> {
        self.0.memberships.grant(Self::membership(principal, role)?).await.map_err(|e| e.to_string())?;
        self.0.waiting.lock().unwrap_or_else(|p| p.into_inner()).remove(principal);
        Ok(())
    }

    async fn dev_set_grants(&self, principal: &str, grants: &[(SimpleRole, String)]) -> Result<(), String> {
        let who: Principal = principal.parse().map_err(|e| format!("`{principal}` is not a principal ({e})"))?;
        for m in self.0.memberships.held(&who) {
            self.0.memberships.revoke(&who, Some(&m.role), &m.scope).await.map_err(|e| e.to_string())?;
        }
        for (role, _) in grants {
            self.0.memberships.grant(Self::membership(principal, *role)?).await.map_err(|e| e.to_string())?;
        }
        if !grants.is_empty() {
            self.0.waiting.lock().unwrap_or_else(|p| p.into_inner()).remove(principal);
        }
        Ok(())
    }

    fn notice(&self, ctx: &AuthContext) {
        if ctx.is_authenticated() && !self.holds_any(&principal_of(ctx)) {
            self.wait(ctx);
        }
    }

    fn alive(&self, ctx: &AuthContext) -> Result<(), String> {
        if ctx.metadata.get("expires_at").and_then(Value::as_i64).is_some_and(|e| e <= sessions::now()) {
            return Err("your sign-in has expired: sign in again".into());
        }
        match ctx.get_metadata_string(self.0.sessions.session_key()) {
            Some(key) if !self.0.sessions.alive(&key) => Err("you have signed out: sign in again".into()),
            _ => Ok(()),
        }
    }

    fn mint(&self, caller: Option<&AuthContext>, ttl_secs: Option<u64>, ceiling: Option<Vec<String>>) -> Result<Session, String> {
        let ctx = caller.filter(|c| c.is_authenticated()).ok_or("sign in first: only a signed-in caller can log in a session")?;
        self.alive(ctx)?;
        let ttl = ttl_secs.unwrap_or(MINT_TTL_DEFAULT_SECS);
        if ttl == 0 || ttl > MINT_TTL_MAX_SECS {
            return Err(format!("ttl_secs is 1 to {MINT_TTL_MAX_SECS} (24 h); default {MINT_TTL_DEFAULT_SECS} (12 h)"));
        }
        // Never wider than the caller's own: its ceiling's layers, then the
        // one asked for.
        let mut layers: Vec<Vec<String>> = Ceiling::layers_of(ctx).map_err(|e| e.to_string())?.iter().map(|l| l.text()).collect();
        let asked = match ceiling {
            None => None,
            Some(entries) => {
                let entries: Vec<String> = entries.iter().map(|e| e.trim()).filter(|e| !e.is_empty()).map(ceiling_entry).collect::<Result<_, _>>()?;
                if entries.is_empty() {
                    return Err("a ceiling names at least one role@SCOPE (e.g. viewer@*)".into());
                }
                Some(entries)
            }
        };
        let shown = asked.clone().or_else(|| layers.last().cloned()).unwrap_or_default();
        layers.extend(asked);
        let mut meta = serde_json::Map::new();
        for key in KEPT_ON_MINT {
            if let Some(v) = ctx.metadata.get(*key) {
                meta.insert((*key).to_string(), v.clone());
            }
        }
        meta.insert("principal".into(), principal_of(ctx).into());
        if !layers.is_empty() {
            meta.insert(CEILING_KEY.into(), layers.into_iter().map(Value::from).collect::<Vec<_>>().into());
        }
        Ok(self.start(ctx, meta, ttl, shown))
    }

    fn mint_for(&self, ctx: AuthContext, ttl_secs: u64) -> Result<Session, String> {
        let key = self.0.sessions.session_key();
        let mut meta: serde_json::Map<String, Value> = ctx
            .metadata
            .as_object()
            .map(|o| o.iter().filter(|(k, _)| *k != key && *k != "expires_at" && *k != CEILING_KEY).map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        meta.insert("principal".into(), principal_of(&ctx).into());
        Ok(self.start(&ctx, meta, ttl_secs, Vec::new()))
    }

    fn ask_access(&self, ctx: &AuthContext) -> Result<Waiting, String> {
        ctx.principal().ok_or("sign in first")?;
        if self.holds_any(&principal_of(ctx)) {
            return Err("you hold a role here already: auth.whoami says which".into());
        }
        Ok(self.wait(ctx))
    }

    fn waiting(&self) -> Vec<Waiting> {
        let mut w: Vec<Waiting> = self
            .0
            .waiting
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter(|w| !self.holds_any(&w.principal))
            .cloned()
            .collect();
        w.sort_by_key(|w| w.since);
        w
    }

    fn logout(&self, caller: Option<&AuthContext>) -> Result<(), String> {
        let key = caller.and_then(|c| c.get_metadata_string(self.0.sessions.session_key())).ok_or("this connection holds no session to log out")?;
        self.0.sessions.end_key(&key);
        Ok(())
    }

    fn session_key(&self) -> Option<&str> {
        Some(self.0.sessions.session_key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::LoginNames;
    use serde_json::json;

    const ADA: &str = "idp:c952acd3-60aa-4534-b2e3-83634591fee1";
    const BOB: &str = "idp:0b6d1c0e-3f0a-4d55-9d1e-6f0c2a7b9e01";

    fn signed_in(principal: &str, name: &str) -> AuthContext {
        let p: Principal = principal.parse().unwrap();
        plexus_auth_core::identity::Authenticated::new(p, "sid", vec![], json!({ "name": name, "provider": "plexus-idp" })).into_auth_context()
    }

    fn access() -> SimpleAccess {
        let sessions = Arc::new(Sessions::new(3600, &LoginNames::for_app("toy")));
        SimpleAccess::new(&sessions, &[ADA.to_string()]).unwrap()
    }

    #[tokio::test]
    async fn the_named_admin_holds_and_anyone_else_waits_until_granted() {
        let a = access();
        a.notice(&signed_in(ADA, "ada"));
        a.notice(&signed_in(BOB, "bob"));
        assert_eq!(a.waiting().iter().map(|w| w.principal.as_str()).collect::<Vec<_>>(), [BOB]);
        assert!(a.ask_access(&signed_in(ADA, "ada")).is_err(), "an admin asks for nothing");
        a.dev_grant(BOB, "*", SimpleRole::Viewer).await.unwrap();
        assert!(a.waiting().is_empty() && a.holds_any(BOB));
        a.dev_set_grants(BOB, &[]).await.unwrap();
        assert!(!a.holds_any(BOB));
        assert!(SimpleAccess::new(&a.0.sessions, &["nobody".into()]).is_err(), "an admin is a principal");
    }

    #[test]
    fn roles_are_global() {
        assert_eq!(SimpleAccess::scope_of("*").unwrap(), "*");
        assert!(SimpleAccess::scope_of("IDENTITY").is_err());
        assert_eq!("Editor".parse::<SimpleRole>().unwrap(), SimpleRole::Editor);
        assert!("owner".parse::<SimpleRole>().is_err());
    }

    #[test]
    fn a_session_is_minted_held_to_its_ceiling_and_logged_out() {
        let a = access();
        let s = a.mint(Some(&signed_in(BOB, "bob")), Some(60), Some(vec!["viewer@*".into()])).unwrap();
        assert!(s.token.starts_with("mgs_") && s.principal == BOB && s.ceiling == ["viewer@*"]);
        let ctx = a.0.sessions.get(&s.token).unwrap();
        assert_eq!(ctx.metadata[CEILING_KEY], json!([["viewer@*"]]));
        assert!(ctx.metadata["toy_session"].is_string());
        assert!(a.alive(&ctx).is_ok());
        // A session minted on that one keeps its ceiling, and the new one.
        let again = a.mint(Some(&ctx), None, Some(vec!["admin@*".into()])).unwrap();
        assert_eq!(a.0.sessions.get(&again.token).unwrap().metadata[CEILING_KEY], json!([["viewer@*"], ["admin@*"]]));
        a.logout(Some(&ctx)).unwrap();
        assert!(a.alive(&ctx).is_err(), "signed out");
        assert!(a.mint(None, None, None).is_err() && a.mint(Some(&signed_in(BOB, "b")), Some(0), None).is_err());
        let code = a.mint_for(ctx, 30).unwrap();
        assert!(code.ceiling.is_empty() && a.0.sessions.get(&code.token).unwrap().metadata.get(CEILING_KEY).is_none());
    }
}
