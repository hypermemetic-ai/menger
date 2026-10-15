//! IDENTITY-19, IDENTITY-21, AUTH-7: **access** — what menger keeps beside
//! the protocol's gate.
//!
//! AUTH-7: the rule in front of every call is no longer menger's. With login
//! on, the hub carries plexus-core's gate (AUTH-2): every call needs the
//! method's grant (`tickets.set_status`), held through a role on the scope
//! its target lives in, within the session's ceiling — default deny, decided
//! before a method runs. What menger supplies the gate lives here, over the
//! store ([`Book`]):
//!
//! - **roles** ([`plexus_core::plexus::Roles`] / `RoleStore`): each
//!   activation's declared `roles(...)` — `viewer` the reads, `editor` the
//!   reads and the writes; `admin`, built in, every grant — and an admin's
//!   edits (`auth.role_set`), kept in the store's `roles` table, which win;
//! - **memberships** (`Memberships` / `MembershipStore`): the members table
//!   (IDENTITY-21's rows keep their meaning: `viewer`, `editor`, `admin` on
//!   a shortcode or `*`) and the operator's `--admin` principals, admin on
//!   `*`;
//! - **scopes** (`Resolve`): a ticket lives in its shortcode, itself, its
//!   ancestors and the shortcodes hosting its shortcode — the filter
//!   language's `under`, read backwards ([`Store::scope_chain`]).
//!
//! What the gate does not do stays here, asked by each method as it starts
//! ([`Access::admit`]): a connection's session may have been signed out or
//! have expired since its upgrade (IDENTITY-19); someone signed in holding
//! nothing waits for an admin ([`Access::waiting`]); and a read without a
//! target — `list`, `search`, `watch` — answers only from where its grant is
//! held ([`Caller::scoped`]: the caller's filter AND `under` those scopes,
//! AND each ceiling layer's), as IDENTITY-21 and IDENTITY-37 made it.
//! Streams ask again at every event, as the grants stand then.
//!
//! **Without login** ([`Access::open`]) there is no gate and every caller
//! reaches everything, as menger always did.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use async_trait::async_trait;
use plexus_auth_core::identity::Principal;
use plexus_auth_core::RoleName;
use plexus_core::plexus::{
    AuthContext, Ceiling as GateCeiling, Gate, Grant as GateGrant, GrantSet, Membership, MembershipStore, Memberships, Reach,
    Resolve, RoleOrGrant, RoleStore, Roles, ScopeRef, StoreError as GateStoreError, ADMIN,
};

use crate::filter::Filter;
use crate::members::{Grant, Role, EVERYWHERE};
use crate::store::{Lens, Store};

/// The grantor named on `--admin` grants.
pub const OPERATOR: &str = "--admin";

/// IDENTITY-35: `granted_by` of the grants the dev sign-in makes.
pub const DEV_LOGIN: &str = "dev-login";

/// AUTH-7: `granted_by` of a grant made through `auth.grant`, which does not
/// say who asked.
pub const AUTH_GRANT: &str = "auth.grant";

/// IDENTITY-37: the most principals [`Access::waiting`] keeps — past it,
/// the one waiting longest is forgotten (it is listed again when it comes
/// back), so sign-ins without a grant cannot grow it without bound.
pub const WAITING_MAX: usize = 1000;

/// IDENTITY-42: the metadata key a minted session's ceiling rides under:
/// a list of layers, each a list of `role@SCOPE` (AUTH-7: or
/// `grant@SCOPE`); every layer must allow a check. The gate reads the same
/// key ([`plexus_core::plexus::CEILING_KEY`]).
pub const CEILING_KEY: &str = plexus_core::plexus::CEILING_KEY;

/// IDENTITY-42: the metadata key a key proof's agent id rides under.
pub const AGENT_KEY: &str = "agent";

/// IDENTITY-42: how long a minted session lasts unless asked, and at most.
pub const MINT_TTL_DEFAULT_SECS: u64 = 12 * 60 * 60;
pub const MINT_TTL_MAX_SECS: u64 = 24 * 60 * 60;

/// AUTH-7: a grant no method is and no role lists — only `admin` holds it
/// (admin holds every grant), so "is this caller admin on X" is the gate's
/// question for it on X, ceilings included. Moving a shortcode's root takes
/// it on both sides (IDENTITY-37), and `members.mine` says whether it is
/// held on `*`. `auth.role_set` cannot list it: it names no method.
pub const ADMINISTER: &str = "menger.administer";

/// AUTH-7: what menger calls "can see this ticket" outside a read of its
/// own — a write naming a blocker, a comment's ticket: where `tickets.get`
/// is held.
pub const SEES: &str = "tickets.get";

/// The metadata a minted session keeps from the identity that asked for it.
const KEPT_ON_MINT: &[&str] = &["principal", AGENT_KEY, "name", "username", "provider", "auth_method", "org_id", "tenant_id"];

/// Whether a session the server issued still stands.
pub trait SessionCheck: Send + Sync {
    fn alive(&self, key: &str) -> bool;
    /// SCAFFOLD-9: the metadata key a session-backed identity carries — the
    /// value [`SessionCheck::alive`] is asked about. The session table's own
    /// (menger's: plexus-login's `Sessions::session_key`), its one
    /// definition: this crate keeps no copy.
    fn key(&self) -> &str;
}

/// IDENTITY-42: the session table, as `session.login` and `session.logout`
/// reach it (menger's `sessions`).
pub trait SessionMint: Send + Sync {
    /// Starts a session for `ctx`, lasting `ttl_secs`: the token that opens
    /// it and when it ends. The token is kept only hashed; `ctx` is stamped
    /// with its session key ([`SessionCheck::key`]) and `expires_at`.
    fn mint(&self, ctx: AuthContext, ttl_secs: i64) -> (String, i64);
    /// Ends the session whose session key is `key`; whether it stood.
    fn end_key(&self, key: &str) -> bool;
}

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

/// Someone signed in with no grant yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    pub principal: String,
    pub name: Option<String>,
    pub provider: Option<String>,
    pub since: i64,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `under` each of `scopes`, or'd: the filter text one membership list or
/// one ceiling covers.
fn under_text(scopes: &[String]) -> String {
    match scopes {
        [one] => format!("under {one}"),
        many => format!("({})", many.iter().map(|s| format!("under {s}")).collect::<Vec<_>>().join(" or ")),
    }
}

// ===========================================================================
// The book: what the gate reads
// ===========================================================================

/// AUTH-7: menger's roles, memberships and scopes, over the store — the
/// three things the gate asks (and `auth.role_set`, `auth.grant`,
/// `auth.revoke` write). One value implements every trait; the hub is
/// handed it once per trait. Answers from memory: the store's cache.
pub struct Book {
    store: OnceLock<Arc<Store>>,
    admins: Vec<String>,
    /// Each role's grants as the activations declare them
    /// (`roles(...)`, merged by name) — seeded once the hub is built
    /// ([`Book::seed`]).
    defaults: RwLock<BTreeMap<String, BTreeSet<String>>>,
    waiting: Mutex<BTreeMap<String, Waiting>>,
}

impl Book {
    fn new(admins: Vec<String>) -> Self {
        Self { store: OnceLock::new(), admins, defaults: RwLock::new(BTreeMap::new()), waiting: Mutex::new(BTreeMap::new()) }
    }

    fn store(&self) -> Option<&Arc<Store>> {
        self.store.get()
    }

    /// The declared roles (`DynamicHub::declared_roles`): what a role holds
    /// until an admin edits it.
    pub fn seed(&self, declared: BTreeMap<String, Vec<String>>) {
        let mut d = self.defaults.write().unwrap_or_else(|p| p.into_inner());
        for (role, grants) in declared {
            if role == ADMIN {
                continue;
            }
            d.entry(role).or_default().extend(grants);
        }
    }

    /// A principal's grants: the stored ones, and admin on `*` for an
    /// operator's `--admin`.
    pub fn grants_of(&self, principal: &str) -> Vec<Grant> {
        let mut grants = self.store().map(|s| s.grants_of(principal)).unwrap_or_default();
        if self.admins.iter().any(|a| a == principal) {
            grants.push(Grant { principal: principal.into(), scope: EVERYWHERE.into(), role: Role::Admin, granted_by: OPERATOR.into(), at: 0 });
        }
        grants
    }

    fn granted(&self, principal: &str) {
        self.waiting.lock().unwrap().remove(principal);
    }

    /// Puts `principal` on the waiting list, unless it is there (IDENTITY-37:
    /// bounded — the one waiting longest goes).
    fn wait(&self, principal: &str, name: Option<String>, provider: Option<String>) -> Waiting {
        let mut waiting = self.waiting.lock().unwrap();
        if let Some(w) = waiting.get(principal) {
            return w.clone();
        }
        while waiting.len() >= WAITING_MAX {
            let oldest = waiting.values().min_by_key(|w| w.since).map(|w| w.principal.clone());
            match oldest {
                Some(p) => waiting.remove(&p),
                None => break,
            };
        }
        let w = Waiting { principal: principal.to_string(), name, provider, since: now() };
        waiting.insert(principal.to_string(), w.clone());
        w
    }
}

impl Roles for Book {
    fn grants(&self, role: &RoleName) -> Option<GrantSet> {
        if role.as_str() == ADMIN {
            return Some(GrantSet::All);
        }
        if let Some(edited) = self.store().and_then(|s| s.role_edits().remove(role.as_str())) {
            return Some(GrantSet::listed(edited));
        }
        let d = self.defaults.read().unwrap_or_else(|p| p.into_inner());
        d.get(role.as_str()).map(|g| GrantSet::listed(g.iter().cloned()))
    }

    fn names(&self) -> Vec<RoleName> {
        let mut names: BTreeSet<String> = self.defaults.read().unwrap_or_else(|p| p.into_inner()).keys().cloned().collect();
        if let Some(s) = self.store() {
            names.extend(s.role_edits().into_keys());
        }
        names.insert(ADMIN.to_string());
        names.into_iter().map(RoleName::new).collect()
    }
}

#[async_trait]
impl RoleStore for Book {
    async fn set(&self, role: &RoleName, grants: GrantSet) -> Result<(), GateStoreError> {
        let GrantSet::Listed(set) = grants else {
            return Err(GateStoreError::new("only admin holds every grant"));
        };
        let store = self.store().ok_or_else(|| GateStoreError::new("no store is attached"))?;
        let grants: Vec<String> = set.iter().map(|g| g.as_str().to_string()).collect();
        store.set_role(role.as_str().to_string(), grants).await.map_err(|e| GateStoreError::new(e.to_string()))
    }
}

impl Memberships for Book {
    fn held(&self, who: &Principal) -> Vec<Membership> {
        self.grants_of(&who.to_string())
            .into_iter()
            .map(|g| Membership { principal: who.clone(), role: RoleName::new(g.role.as_str()), scope: ScopeRef::parse(&g.scope) })
            .collect()
    }
}

#[async_trait]
impl MembershipStore for Book {
    async fn grant(&self, m: Membership) -> Result<(), GateStoreError> {
        let store = self.store().ok_or_else(|| GateStoreError::new("no store is attached"))?;
        // IDENTITY-21's rows hold viewer, editor or admin (DEPLOY-14: or
        // deployer); a role an admin defined with auth.role_set is not a
        // membership menger keeps yet.
        let role: Role = m.role.as_str().parse().map_err(|_| {
            GateStoreError::new(format!("menger keeps viewer, editor, admin and deployer memberships; {} is not one of them", m.role))
        })?;
        let scope = crate::members::scope_of(m.scope.as_str()).map_err(GateStoreError::new)?;
        let principal = m.principal.to_string();
        let g = Grant { principal: principal.clone(), scope, role, granted_by: AUTH_GRANT.into(), at: now() };
        store.grant(g).await.map_err(|e| GateStoreError::new(e.to_string()))?;
        self.granted(&principal);
        Ok(())
    }

    async fn revoke(&self, principal: &Principal, role: Option<&RoleName>, scope: &ScopeRef) -> Result<Vec<RoleName>, GateStoreError> {
        let store = self.store().ok_or_else(|| GateStoreError::new("no store is attached"))?;
        let scope = crate::members::scope_of(scope.as_str()).map_err(GateStoreError::new)?;
        let who = principal.to_string();
        let held = store.grants_of(&who).into_iter().find(|g| g.scope == scope);
        let Some(held) = held.filter(|g| role.is_none_or(|r| r.as_str() == g.role.as_str())) else {
            return Ok(Vec::new());
        };
        store.revoke(who, scope).await.map_err(|e| GateStoreError::new(e.to_string()))?;
        Ok(vec![RoleName::new(held.role.as_str())])
    }
}

impl Resolve for Book {
    fn chain(&self, target: &str) -> Option<Vec<ScopeRef>> {
        if target.trim() == EVERYWHERE {
            return Some(vec![ScopeRef::Everywhere]);
        }
        let store = self.store()?;
        // A target no scope could hold (`a b`) lives only in `*`: an admin
        // of `*` reaches the method, which says what is wrong with it.
        let mut out: Vec<ScopeRef> = store.scope_chain(target).unwrap_or_default().into_iter().map(ScopeRef::Named).collect();
        out.push(ScopeRef::Everywhere);
        Some(out)
    }
}

// ===========================================================================
// Caller
// ===========================================================================

/// Who a call acts as, once admitted, and for which grant.
#[derive(Clone)]
pub struct Caller {
    /// The Plexus principal, or `None` when login is off.
    pub principal: Option<String>,
    /// The name to show, when signed in.
    pub name: Option<String>,
    /// The grant the call was admitted for (the method's); `None` when
    /// login is off.
    pub grant: Option<GateGrant>,
    ctx: Option<AuthContext>,
    /// `None`: login is off — everything, as always.
    gate: Option<(Gate, Arc<Book>)>,
}

impl std::fmt::Debug for Caller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Caller").field("principal", &self.principal).field("grant", &self.grant).finish()
    }
}

impl Caller {
    fn everyone() -> Self {
        Self { principal: None, name: None, grant: None, ctx: None, gate: None }
    }

    fn who(&self) -> String {
        self.principal.clone().unwrap_or_else(|| "an anonymous caller".into())
    }

    /// The scopes this caller's memberships hold `grant` on — `None` when
    /// one is `*` — and each ceiling layer's, the same way. `Err` when it
    /// holds it nowhere (or the ceiling cannot be read).
    fn parts(&self, grant: &GateGrant) -> Result<Vec<Vec<String>>, String> {
        let (Some((gate, book)), Some(ctx)) = (&self.gate, &self.ctx) else { return Ok(Vec::new()) };
        let Some(p) = ctx.principal() else { return Err("sign in first".into()) };
        let mut parts = Vec::new();
        let held: Vec<ScopeRef> = book.held(&p).into_iter().filter(|m| gate.lists(&m.role, grant)).map(|m| m.scope).collect();
        if held.is_empty() {
            return Err(format!("no access yet: {} holds {grant} nowhere — ask an admin for a role", self.who()));
        }
        if !held.iter().any(ScopeRef::is_everywhere) {
            parts.push(held.iter().map(|s| s.as_str().to_string()).collect());
        }
        let layers = GateCeiling::layers_of(ctx).map_err(|e| e.to_string())?;
        for layer in layers {
            let allowed: Vec<&ScopeRef> = layer
                .0
                .iter()
                .filter(|(rg, _)| match rg {
                    RoleOrGrant::Role(r) => gate.lists(r, grant),
                    RoleOrGrant::Grant(g) => g == grant,
                })
                .map(|(_, s)| s)
                .collect();
            if allowed.is_empty() {
                return Err(format!("this session's scope ({}) holds no {grant}", layer.text().join(", ")));
            }
            if !allowed.iter().any(|s| s.is_everywhere()) {
                parts.push(allowed.iter().map(|s| s.as_str().to_ascii_uppercase()).collect());
            }
        }
        Ok(parts)
    }

    /// The filter `under` the scopes this caller holds `grant` on (within
    /// its ceiling): `None` everywhere, `Err` nowhere. IDENTITY-42: the
    /// memberships' filter AND each ceiling layer's — a ticket is in reach
    /// only when every one covers it.
    pub fn filter_for(&self, grant: &GateGrant) -> Result<Option<Filter>, String> {
        let parts = self.parts(grant)?;
        if parts.is_empty() {
            return Ok(None);
        }
        let text = parts.iter().map(|p| under_text(p)).collect::<Vec<_>>().join(" and ");
        Filter::parse(&text).map(Some).map_err(|e| e.to_string())
    }

    /// [`Caller::filter_for`] the grant the call was admitted for.
    pub fn reach_filter(&self) -> Result<Option<Filter>, String> {
        match &self.grant {
            Some(g) => self.filter_for(g),
            None => Ok(None),
        }
    }

    /// [`Caller::filter_for`] [`SEES`]: what this caller can see of the
    /// tickets, outside a read of its own.
    pub fn sight_filter(&self) -> Result<Option<Filter>, String> {
        if self.gate.is_none() {
            return Ok(None);
        }
        self.filter_for(&GateGrant::new(SEES))
    }

    /// What [`Caller::reach_filter`] says, as text — what a stream compares
    /// at each event to know the reach moved.
    pub fn reach_key(&self) -> Result<Option<String>, String> {
        Ok(self.reach_filter()?.map(|f| f.format()))
    }

    /// A read's filter text, ANDed with the scopes this caller holds the
    /// call's grant on: what the method answers under instead. Unchanged
    /// when it holds it everywhere; `Err` when nowhere.
    pub fn scoped(&self, text: Option<&str>) -> Result<Option<String>, String> {
        let Some(reach) = self.reach_filter()? else { return Ok(text.map(str::to_string)) };
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

    /// Whether this caller holds `grant` on every one of `ids` — by the
    /// tickets' place in the tree, as a read sees them — `Err` naming the
    /// first it does not, and the shortcode it would be needed on.
    pub fn require_for(&self, grant: &GateGrant, ids: &[&str], store: &Store) -> Result<(), String> {
        let Some(reach) = self.filter_for(grant).map_err(|_| self.lacks(grant, ids.first().copied()))? else {
            return Ok(());
        };
        let lens = Lens::of(&reach).map_err(|e| e.to_string())?;
        let owned: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        match store.member(&owned, &lens).iter().position(|ok| !ok) {
            None => Ok(()),
            Some(i) => Err(self.lacks(grant, Some(ids[i]))),
        }
    }

    /// [`Caller::require_for`] the call's own grant.
    pub fn require(&self, ids: &[&str], store: &Store) -> Result<(), String> {
        match self.grant.clone() {
            Some(g) => self.require_for(&g, ids, store),
            None => Ok(()),
        }
    }

    /// Why this caller may not use `grant` on the ticket `id` (or anything
    /// here). IDENTITY-37: it names only the id's own shortcode, read from
    /// the id, so it is the same whether the ticket exists or not.
    pub fn lacks(&self, grant: &GateGrant, id: Option<&str>) -> String {
        match id {
            Some(id) => {
                // ORG-3: the shortcode the address names it in, by its
                // letters — never the text before a `-` (an org's name).
                let shortcode = crate::address::Address::parse(id)
                    .map(|a| a.code.as_str().to_string())
                    .unwrap_or_else(|_| id.split(['-', '/']).next().unwrap_or(id).to_ascii_uppercase());
                format!("{} needs {grant} on {shortcode} for {id}", self.who())
            }
            None => format!("{} needs {grant} here", self.who()),
        }
    }

    /// The gate's own question for `grant` on `target` (`*` for
    /// everywhere), as it stands now: `Err` is its refusal, said in words
    /// (`needs tickets.create on MENGER`).
    pub fn check(&self, grant: &GateGrant, target: &str) -> Result<(), String> {
        let Some((gate, _)) = &self.gate else { return Ok(()) };
        gate.check(self.ctx.as_ref(), grant, Some(target)).map(|_| ()).map_err(|r| r.to_string())
    }

    /// Whether this caller is admin on `target` (`*`: everywhere), within
    /// its ceiling ([`ADMINISTER`]).
    pub fn administers(&self, target: &str) -> bool {
        self.check(&GateGrant::new(ADMINISTER), target).is_ok()
    }
}

// ===========================================================================
// Access
// ===========================================================================

/// The rule's state. Cheap to clone: one per activation, all sharing it.
#[derive(Clone)]
pub struct Access {
    login: bool,
    require_login: bool,
    sessions: Option<Arc<dyn SessionCheck>>,
    minter: Option<Arc<dyn SessionMint>>,
    book: Arc<Book>,
    gate: Gate,
}

impl Default for Access {
    fn default() -> Self {
        Self::open()
    }
}

impl Access {
    fn build(login: bool, require_login: bool, sessions: Option<Arc<dyn SessionCheck>>, admins: Vec<String>) -> Self {
        let book = Arc::new(Book::new(admins));
        let gate = Gate::new(book.clone(), book.clone(), book.clone());
        Self { login, require_login, sessions, minter: None, book, gate }
    }

    /// No login: every call is admitted and reaches everything; the hub
    /// carries no gate.
    pub fn open() -> Self {
        Self::build(false, false, None, Vec::new())
    }

    /// Login is on: the hub carries the gate over this rule's [`Book`],
    /// sessions are checked on every call, and `admins` hold admin on `*`
    /// from the operator. `require_login` is kept for what still reads it
    /// (the shell's redirects); an anonymous caller reaches only the door.
    pub fn with_login(sessions: Arc<dyn SessionCheck>, require_login: bool, admins: Vec<String>) -> Self {
        Self::build(true, require_login, Some(sessions), admins)
    }

    /// IDENTITY-42: `session.login` / `auth.login {session}` and the logouts
    /// mint and end sessions in `minter` (menger's session table).
    pub fn minting(mut self, minter: Arc<dyn SessionMint>) -> Self {
        self.minter = Some(minter);
        self
    }

    /// SCAFFOLD-9: the metadata key a session-backed identity carries — the
    /// session table's ([`SessionCheck::key`]); `None` with login off.
    pub fn session_key(&self) -> Option<&str> {
        self.sessions.as_ref().map(|s| s.key())
    }

    /// The store whose members and roles decide (the builder attaches it).
    pub fn attach(&self, store: Arc<Store>) {
        let _ = self.book.store.set(store);
    }

    /// Whether login is on.
    pub fn login(&self) -> bool {
        self.login
    }

    /// Whether `--require-login` was given.
    pub fn require_login(&self) -> bool {
        self.require_login
    }

    /// AUTH-7: the roles, memberships and scopes the gate reads.
    pub fn book(&self) -> Arc<Book> {
        self.book.clone()
    }

    /// AUTH-7: a gate over the same book as the hub's — the same answers,
    /// for the questions a method asks itself.
    pub fn gate(&self) -> &Gate {
        &self.gate
    }

    /// A principal's grants: the stored ones, and admin on `*` for an
    /// operator's `--admin`.
    pub fn grants_of(&self, principal: &str) -> Vec<Grant> {
        self.book.grants_of(principal)
    }

    /// The operator's `--admin` principals.
    pub fn admins(&self) -> Vec<String> {
        self.book.admins.clone()
    }

    /// IDENTITY-35: a grant the operator's dev sign-in makes (test only),
    /// kept in the store like any other and signed `granted_by` `dev-login`.
    pub async fn dev_grant(&self, principal: &str, scope: &str, role: Role) -> Result<Grant, String> {
        let store = self.book.store().cloned().ok_or("no store is attached")?;
        let scope = crate::members::scope_of(scope)?;
        let g = Grant { principal: principal.into(), scope, role, granted_by: DEV_LOGIN.into(), at: now() };
        let g = store.grant(g).await.map_err(|e| e.to_string())?;
        self.granted(principal);
        Ok(g)
    }

    /// IDENTITY-35: a roster user's grants made exactly `grants`: each kept,
    /// and every other stored grant of theirs revoked.
    pub async fn dev_set_grants(&self, principal: &str, grants: &[(Role, String)]) -> Result<(), String> {
        let store = self.book.store().cloned().ok_or("no store is attached")?;
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
        self.book.granted(principal);
    }

    /// Everyone signed in who holds no grant yet, by when they first came.
    pub fn waiting(&self) -> Vec<Waiting> {
        let mut w: Vec<Waiting> = self.book.waiting.lock().unwrap().values().cloned().collect();
        w.sort_by_key(|w| w.since);
        w
    }

    /// AUTH-7, `auth.signup` by someone signed in holding nothing: on the
    /// waiting list (asking again keeps the first `since`).
    pub fn ask_access(&self, ctx: &AuthContext) -> Result<Waiting, String> {
        let principal = ctx.principal().map(|p| p.to_string()).ok_or("sign in first")?;
        Ok(self.book.wait(&principal, ctx.get_metadata_string("name"), ctx.get_metadata_string("provider")))
    }

    /// AUTH-7: a connection opened as `ctx` (at the upgrade): holding
    /// nothing, it waits for an admin ([`Access::waiting`]).
    pub fn notice(&self, ctx: &AuthContext) {
        if !self.login || !ctx.is_authenticated() {
            return;
        }
        let principal = ctx.principal().map(|p| p.to_string()).unwrap_or_else(|| ctx.user_id.clone());
        if self.book.grants_of(&principal).is_empty() {
            self.book.wait(&principal, ctx.get_metadata_string("name"), ctx.get_metadata_string("provider"));
        }
    }

    /// Whether a signed-in identity's sign-in still stands: not expired, its
    /// session not signed out. `Err`: why not.
    pub fn alive(&self, ctx: &AuthContext) -> Result<(), String> {
        if let Some(exp) = ctx.metadata.get("expires_at").and_then(|v| v.as_i64()) {
            if exp <= now() {
                return Err("your sign-in has expired: sign in again".into());
            }
        }
        if let Some((sessions, key)) = self.sessions.as_ref().and_then(|s| Some((s, ctx.get_metadata_string(s.key())?))) {
            if !sessions.alive(&key) {
                return Err("you have signed out: sign in again".into());
            }
        }
        Ok(())
    }

    /// The caller a call acts as, or why it is refused (an in-band error's
    /// message). The gate has admitted the call already; this asks what it
    /// does not: whether the sign-in still stands, and whether the ceiling
    /// can be read. A signed-in caller holding nothing joins the waiting
    /// list. `reach` names the grant the call was admitted for.
    pub fn admit(&self, auth: Option<&AuthContext>, reach: &Reach) -> Result<Caller, String> {
        if !self.login {
            return Ok(Caller::everyone());
        }
        let Some(ctx) = auth.filter(|c| c.is_authenticated()) else {
            return Err("sign in first: this menger requires it".into());
        };
        self.alive(ctx)?;
        GateCeiling::layers_of(ctx).map_err(|e| e.to_string())?;
        let principal = ctx.principal().map(|p| p.to_string()).unwrap_or_else(|| ctx.user_id.clone());
        let name = ctx.get_metadata_string("name");
        if self.book.grants_of(&principal).is_empty() {
            self.book.wait(&principal, name.clone(), ctx.get_metadata_string("provider"));
        } else {
            self.book.granted(&principal);
        }
        Ok(Caller {
            principal: Some(principal),
            name,
            grant: reach.grant().cloned(),
            ctx: Some(ctx.clone()),
            gate: Some((self.gate.clone(), self.book.clone())),
        })
    }

    /// One `role@SCOPE` or `grant@SCOPE` ceiling entry, as a minted session
    /// keeps it: the role one this menger knows (or a dotted grant), the
    /// scope `*`, a shortcode or a ticket, upper case.
    pub fn ceiling_entry(&self, text: &str) -> Result<String, String> {
        let (left, scope) = text
            .trim()
            .split_once('@')
            .ok_or_else(|| format!("`{}` is not role@SCOPE (e.g. editor@IDENTITY, viewer@*)", text.trim()))?;
        let left = left.trim();
        let scope = ceiling_scope_of(scope)?;
        if left.contains('.') {
            let ok = left.split_once('.').is_some_and(|(a, m)| {
                let word = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                word(a) && word(m)
            });
            if !ok {
                return Err(format!("`{left}` is not a grant (e.g. tickets.get)"));
            }
            return Ok(format!("{left}@{scope}"));
        }
        let role = left.to_ascii_lowercase();
        if !self.book.names().iter().any(|r| r.as_str() == role) {
            let known: Vec<String> = self.book.names().iter().map(|r| r.as_str().to_string()).collect();
            return Err(format!("unknown role `{left}` — the roles are {}", known.join(", ")));
        }
        Ok(format!("{role}@{scope}"))
    }

    /// IDENTITY-42, `session.login` (AUTH-7: and `auth.login {session}`): a
    /// session for the caller, lasting `ttl_secs` (default 12 h, at most
    /// 24 h), held to `scope` when given — and to any ceiling the caller's
    /// own session holds, so it is never wider. Any signed-in caller may
    /// mint one; it acts as the caller's principal, agent, name and
    /// provider.
    pub fn mint(&self, auth: Option<&AuthContext>, ttl_secs: Option<u64>, scope: Option<Vec<String>>) -> Result<Minted, String> {
        let Some(minter) = self.minter.as_ref().filter(|_| self.login) else {
            return Err("login is off on this menger: there is no session to log in".into());
        };
        let Some(ctx) = auth.filter(|c| c.is_authenticated()) else {
            return Err("sign in first: only a signed-in caller can log in a session".into());
        };
        self.alive(ctx)?;
        let layers: Vec<Vec<String>> = GateCeiling::layers_of(ctx).map_err(|e| e.to_string())?.iter().map(|l| l.text()).collect();
        let principal = ctx.principal().map(|p| p.to_string()).unwrap_or_else(|| ctx.user_id.clone());
        let ttl = ttl_secs.unwrap_or(MINT_TTL_DEFAULT_SECS);
        if ttl == 0 || ttl > MINT_TTL_MAX_SECS {
            return Err(format!("ttl_secs is 1 to {MINT_TTL_MAX_SECS} (24 h); default {MINT_TTL_DEFAULT_SECS} (12 h)"));
        }
        let asked: Option<Vec<String>> = match scope {
            None => None,
            Some(entries) => {
                let entries: Vec<String> = entries.iter().map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect();
                if entries.is_empty() {
                    return Err("a scope names at least one role@SCOPE (e.g. editor@IDENTITY)".into());
                }
                Some(entries.iter().map(|e| self.ceiling_entry(e)).collect::<Result<_, _>>()?)
            }
        };
        let mut ceiling = layers.clone();
        ceiling.extend(asked.clone());
        let shown = asked.or_else(|| layers.last().cloned()).unwrap_or_default();

        let mut meta = serde_json::Map::new();
        for key in KEPT_ON_MINT {
            if let Some(v) = ctx.metadata.get(*key) {
                meta.insert((*key).to_string(), v.clone());
            }
        }
        meta.insert("principal".into(), principal.clone().into());
        if !ceiling.is_empty() {
            let layers: Vec<serde_json::Value> = ceiling.iter().map(|c| c.clone().into()).collect();
            meta.insert(CEILING_KEY.into(), layers.into());
        }
        let agent = ctx.get_metadata_string(AGENT_KEY);
        let name = ctx.get_metadata_string("name");
        // A session id that says what it is (the session's key rides in the
        // metadata): an empty one would read as no one.
        let minted = AuthContext::new(ctx.user_id.clone(), "mgs".into(), ctx.roles.clone(), serde_json::Value::Object(meta));
        let (token, expires_at) = minter.mint(minted, ttl as i64);
        Ok(Minted { token, principal, agent, name, expires_at, scope: shown })
    }

    /// AUTH-7: a session for an identity a browser sign-in proved (the
    /// loopback flow's code): no ceiling, `ttl_secs` long. Its metadata is
    /// the sign-in's, its session key and expiry the new session's own.
    pub fn mint_for(&self, ctx: AuthContext, ttl_secs: u64) -> Result<Minted, String> {
        let Some(minter) = self.minter.as_ref().filter(|_| self.login) else {
            return Err("login is off on this menger: there is no session to log in".into());
        };
        let principal = ctx.principal().map(|p| p.to_string()).unwrap_or_else(|| ctx.user_id.clone());
        let mut meta = serde_json::Map::new();
        if let Some(obj) = ctx.metadata.as_object() {
            for (k, v) in obj {
                if Some(k.as_str()) != self.session_key() && k != "expires_at" && k != CEILING_KEY {
                    meta.insert(k.clone(), v.clone());
                }
            }
        }
        meta.insert("principal".into(), principal.clone().into());
        let agent = ctx.get_metadata_string(AGENT_KEY);
        let name = ctx.get_metadata_string("name");
        let minted = AuthContext::new(ctx.user_id.clone(), "mgs".into(), ctx.roles.clone(), serde_json::Value::Object(meta));
        let (token, expires_at) = minter.mint(minted, ttl_secs.clamp(1, MINT_TTL_MAX_SECS) as i64);
        Ok(Minted { token, principal, agent, name, expires_at, scope: Vec::new() })
    }

    /// IDENTITY-42, `session.logout`: ends the session the caller's
    /// connection is on. A connection a credential opened directly (a key
    /// proof, a provider's token) holds no session to end.
    pub fn logout(&self, auth: Option<&AuthContext>) -> Result<(), String> {
        let Some(minter) = self.minter.as_ref().filter(|_| self.login) else {
            return Err("login is off on this menger: there is no session to log out".into());
        };
        match auth.filter(|c| c.is_authenticated()).and_then(|c| c.get_metadata_string(self.session_key()?)) {
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
    pub name: Option<String>,
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
        fn key(&self) -> &str {
            SESSION_KEY
        }
    }

    /// The session table's metadata key, as menger's names it.
    const SESSION_KEY: &str = "menger_session";

    const ALICE: &str = "idp:c952acd3-60aa-4534-b2e3-83634591fee1";

    /// Alice's identity, stamped with her principal as every adapter stamps it.
    fn ctx(meta: serde_json::Value) -> AuthContext {
        let mut meta = meta;
        if let Some(m) = meta.as_object_mut() {
            m.entry("principal").or_insert(ALICE.into());
        }
        AuthContext::new(ALICE.into(), "s".into(), vec![], meta)
    }

    fn grant(principal: &str, scope: &str, role: Role) -> Grant {
        Grant { principal: principal.into(), scope: scope.into(), role, granted_by: "test".into(), at: 1 }
    }

    /// The default roles, as tickets declares them (a part of them).
    fn declared() -> BTreeMap<String, Vec<String>> {
        let viewer = vec!["tickets.get".to_string(), "tickets.list".to_string()];
        let mut editor = viewer.clone();
        editor.push("tickets.set_status".into());
        BTreeMap::from([("viewer".to_string(), viewer), ("editor".to_string(), editor)])
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

    fn login(admins: Vec<String>) -> Access {
        let a = Access::with_login(Arc::new(Live::default()), true, admins);
        a.book().seed(declared());
        a
    }

    fn reach_of(a: &Access, c: &AuthContext, grant: &str) -> Reach {
        a.gate().check(Some(c), &GateGrant::new(grant), None).unwrap_or_else(|_| Reach::everywhere())
    }

    #[test]
    fn without_login_everyone_reaches_everything() {
        let c = Access::open().admit(None, &Reach::everywhere()).unwrap();
        assert!(c.administers("*"));
        assert_eq!(c.scoped(Some("status Ready")).unwrap().as_deref(), Some("status Ready"));
    }

    #[test]
    fn an_anonymous_call_is_refused() {
        let a = login(vec![]);
        assert!(a.admit(None, &Reach::everywhere()).is_err());
        assert!(a.admit(Some(&AuthContext::anonymous()), &Reach::everywhere()).is_err());
    }

    #[test]
    fn a_session_signed_out_or_expired_is_refused() {
        let live = Arc::new(Live::default());
        live.0.lock().unwrap().insert("k1".into());
        let a = Access::with_login(live.clone(), true, vec![]);
        let ok = ctx(json!({ SESSION_KEY: "k1", "expires_at": now() + 60, "name": "alice" }));
        assert_eq!(a.admit(Some(&ok), &Reach::everywhere()).unwrap().name.as_deref(), Some("alice"));
        live.0.lock().unwrap().clear();
        assert!(a.admit(Some(&ok), &Reach::everywhere()).unwrap_err().contains("signed out"));
        let expired = ctx(json!({ "expires_at": now() - 1 }));
        assert!(a.admit(Some(&expired), &Reach::everywhere()).unwrap_err().contains("expired"));
    }

    #[tokio::test]
    async fn no_grant_waits() {
        let a = login(vec![]);
        a.attach(store().await);
        let c = a.admit(Some(&ctx(json!({ "name": "alice", "provider": "plexus-idp" }))), &Reach::everywhere()).unwrap();
        assert!(c.filter_for(&GateGrant::new("tickets.list")).unwrap_err().contains("ask an admin"));
        assert_eq!(a.waiting()[0].principal, ALICE);
    }

    #[tokio::test]
    async fn reads_are_anded_with_where_the_grant_is_held() {
        let a = login(vec![]);
        let s = store().await;
        s.grant(grant(ALICE, "IDENTITY", Role::Editor)).await.unwrap();
        s.grant(grant(ALICE, "OUGHT", Role::Viewer)).await.unwrap();
        a.attach(s);
        let me = ctx(json!({}));
        let c = a.admit(Some(&me), &reach_of(&a, &me, "tickets.list")).unwrap();
        // The language's canonical spelling of (under IDENTITY or under OUGHT).
        assert_eq!(c.scoped(None).unwrap().unwrap(), "under in (IDENTITY, OUGHT)");
        assert_eq!(c.scoped(Some("status Ready")).unwrap().unwrap(), "status Ready and under in (IDENTITY, OUGHT)");
        // set_status is held on IDENTITY alone.
        assert_eq!(c.filter_for(&GateGrant::new("tickets.set_status")).unwrap().unwrap().format(), "under IDENTITY");
        assert!(a.waiting().is_empty(), "a member does not wait");
    }

    #[tokio::test]
    async fn the_gate_resolves_a_ticket_through_its_shortcode_and_hosts() {
        let a = login(vec![]);
        let s = store().await;
        s.grant(grant(ALICE, "PLEXUS", Role::Editor)).await.unwrap();
        a.attach(s.clone());
        let g = a.gate();
        let me = ctx(json!({}));
        let set = GateGrant::new("tickets.set_status");
        assert!(g.check(Some(&me), &set, Some("IDENTITY-2")).is_ok(), "IDENTITY is hosted under PLEXUS-1");
        assert!(g.check(Some(&me), &set, Some("IDENTITY-99")).is_ok(), "a missing ticket resolves by its shortcode");
        let refused = g.check(Some(&me), &set, Some("OUGHT-2")).unwrap_err();
        assert_eq!(refused.to_string(), "needs tickets.set_status on OUGHT");
        assert_eq!(
            s.scope_chain("identity-2").unwrap(),
            ["IDENTITY", "IDENTITY-2", "IDENTITY-1", "PLEXUS-1", "PLEXUS"].map(String::from).to_vec()
        );
        assert_eq!(s.scope_chain("IDENTITY/_MENGER/board").unwrap(), ["IDENTITY", "PLEXUS"].map(String::from).to_vec());
        assert!(s.scope_chain("a b").is_none());
    }

    #[tokio::test]
    async fn the_waiting_list_is_capped_and_forgets_the_longest_waiting() {
        let a = login(vec![]);
        a.attach(store().await);
        let as_ = |p: String| AuthContext::new(p, "s".into(), vec![], json!({}));
        for i in 0..WAITING_MAX + 5 {
            a.admit(Some(&as_(format!("user-{i}"))), &Reach::everywhere()).unwrap();
        }
        let w = a.waiting();
        assert_eq!(w.len(), WAITING_MAX);
        assert!(w.iter().any(|w| w.principal == format!("user-{}", WAITING_MAX + 4)), "the newest waits");
        a.admit(Some(&as_(format!("user-{}", WAITING_MAX + 4))), &Reach::everywhere()).unwrap();
        assert_eq!(a.waiting().len(), WAITING_MAX);
    }

    #[tokio::test]
    async fn an_operator_admin_is_admin_everywhere() {
        let a = login(vec![ALICE.into()]);
        a.attach(store().await);
        let me = ctx(json!({}));
        let c = a.admit(Some(&me), &reach_of(&a, &me, "tickets.list")).unwrap();
        assert!(c.administers("*"));
        assert_eq!(a.grants_of(ALICE)[0].granted_by, OPERATOR);
        assert_eq!(c.reach_filter().unwrap(), None);
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
    fn a_ceiling_entry_is_role_or_grant_at_scope() {
        let a = login(vec![]);
        assert_eq!(a.ceiling_entry("editor@identity"), Ok("editor@IDENTITY".into()));
        assert_eq!(a.ceiling_entry(" viewer@* "), Ok("viewer@*".into()));
        assert_eq!(a.ceiling_entry("Admin@menger-12"), Ok("admin@MENGER-12".into()));
        assert_eq!(a.ceiling_entry("tickets.get@menger"), Ok("tickets.get@MENGER".into()));
        for bad in ["editor", "owner@X", "editor@", "editor@a b", "editor@X-", "editor@X-1a", "editor@*-1", "editor@under X", "a.b c@X"] {
            assert!(a.ceiling_entry(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn every_check_needs_the_memberships_and_the_ceiling() {
        let a = login(vec![ALICE.into()]);
        let s = store().await;
        a.attach(s.clone());
        let g = a.gate();
        let set = GateGrant::new("tickets.set_status");
        // Admin on * from the operator, held to editor on IDENTITY.
        let me = ctx(json!({ CEILING_KEY: ceiling(&[&["editor@IDENTITY"]]) }));
        assert!(g.check(Some(&me), &set, Some("IDENTITY-2")).is_ok());
        assert!(g.check(Some(&me), &set, Some("OUGHT-2")).is_err(), "elsewhere");
        let c = a.admit(Some(&me), &reach_of(&a, &me, "tickets.list")).unwrap();
        assert!(!c.administers("IDENTITY"), "no more than editor");
        assert_eq!(c.scoped(Some("status Ready")).unwrap().unwrap(), "status Ready and under IDENTITY");
        // One ticket: it and its descendants.
        let me = ctx(json!({ CEILING_KEY: ceiling(&[&["editor@IDENTITY-2"]]) }));
        assert!(g.check(Some(&me), &set, Some("IDENTITY-2")).is_ok());
        assert!(g.check(Some(&me), &set, Some("IDENTITY-1")).is_err());
        // `*` in the ceiling is no ceiling at that role.
        let me = ctx(json!({ CEILING_KEY: ceiling(&[&["admin@*"]]) }));
        assert!(a.admit(Some(&me), &Reach::everywhere()).unwrap().administers("*"));
    }

    #[tokio::test]
    async fn a_ceiling_wider_than_the_memberships_gives_no_more() {
        let a = login(vec![]);
        let s = store().await;
        s.grant(grant(ALICE, "OUGHT", Role::Viewer)).await.unwrap();
        a.attach(s.clone());
        let g = a.gate();
        let me = ctx(json!({ CEILING_KEY: ceiling(&[&["admin@*"]]) }));
        assert!(g.check(Some(&me), &GateGrant::new("tickets.get"), Some("OUGHT-2")).is_ok());
        assert!(g.check(Some(&me), &GateGrant::new("tickets.set_status"), Some("OUGHT-2")).is_err());
        assert!(g.check(Some(&me), &GateGrant::new("tickets.get"), Some("IDENTITY-2")).is_err());
    }

    #[tokio::test]
    async fn layered_ceilings_all_hold_and_an_unreadable_one_refuses() {
        let a = login(vec![ALICE.into()]);
        let s = store().await;
        a.attach(s.clone());
        let g = a.gate();
        let me = ctx(json!({ CEILING_KEY: ceiling(&[&["editor@PLEXUS"], &["admin@IDENTITY", "editor@OUGHT"]]) }));
        let set = GateGrant::new("tickets.set_status");
        assert!(g.check(Some(&me), &set, Some("IDENTITY-2")).is_ok(), "IDENTITY is hosted under PLEXUS");
        assert!(g.check(Some(&me), &GateGrant::new(ADMINISTER), Some("IDENTITY-2")).is_err(), "the first holds editor");
        assert!(g.check(Some(&me), &GateGrant::new("tickets.get"), Some("OUGHT-2")).is_err(), "the first does not reach OUGHT");
        assert!(g.check(Some(&me), &GateGrant::new("tickets.get"), Some("PLEXUS-2")).is_err(), "the second does not reach PLEXUS-2");
        for bad in [json!("editor@X"), json!([[1]])] {
            assert!(a.admit(Some(&ctx(json!({ CEILING_KEY: bad.clone() }))), &Reach::everywhere()).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_minted_session_keeps_the_caller_and_never_widens() {
        let mint = Arc::new(Mint::default());
        let a = login(vec![ALICE.into()]).minting(mint.clone());
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
        // The gate reads it as one layer.
        assert_eq!(GateCeiling::layers_of(&minted).unwrap().len(), 1);

        // From that session: its ceiling stays, the new one is added.
        let m2 = a.mint(Some(&minted), Some(60), Some(vec!["admin@*".into()])).unwrap();
        assert_eq!(m2.scope, vec!["admin@*".to_string()]);
        let again = mint.0.lock().unwrap().last().cloned().unwrap();
        assert_eq!(again.metadata[CEILING_KEY], json!([["editor@IDENTITY"], ["admin@*"]]));
        assert!(!a.admit(Some(&again), &Reach::everywhere()).unwrap().administers("*"), "still held to IDENTITY");
        // Asking nothing inherits it, and says so.
        assert_eq!(a.mint(Some(&minted), None, None).unwrap().scope, vec!["editor@IDENTITY".to_string()]);
        assert!(a.mint(Some(&agent), None, None).unwrap().scope.is_empty(), "no ceiling");

        for (ttl, scope) in [(Some(0), None), (Some(MINT_TTL_MAX_SECS + 1), None), (None, Some(vec![])), (None, Some(vec!["owner@X".into()]))] {
            assert!(a.mint(Some(&agent), ttl, scope.clone()).is_err(), "{ttl:?} {scope:?}");
        }
        assert!(a.mint(Some(&AuthContext::anonymous()), None, None).is_err());
        assert!(login(vec![]).mint(Some(&agent), None, None).is_err(), "nothing to mint in");
        assert!(a.logout(Some(&agent)).is_err(), "a proof-opened connection holds no session");
        assert!(a.logout(Some(&ctx(json!({ SESSION_KEY: "k" })))).is_ok());
    }

    #[test]
    fn an_edited_role_is_kept_in_the_file_across_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let rt = tokio::runtime::Runtime::new().unwrap();
        {
            let s = Store::open(&path).unwrap();
            rt.block_on(s.set_role("viewer".into(), vec!["tickets.get".into(), "comments.list".into()])).unwrap();
            rt.block_on(s.set_role("viewer".into(), vec!["tickets.get".into()])).unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.role_edits(), BTreeMap::from([("viewer".to_string(), vec!["tickets.get".to_string()])]));
    }

    #[tokio::test]
    async fn an_admin_s_edit_wins_over_the_declared_role_and_survives_in_the_store() {
        let a = login(vec![]);
        let s = store().await;
        a.attach(s.clone());
        let book = a.book();
        let viewer = RoleName::new("viewer");
        assert!(book.grants(&viewer).unwrap().contains(&GateGrant::new("tickets.list")));
        RoleStore::set(&*book, &viewer, GrantSet::listed(["tickets.get"])).await.unwrap();
        assert!(!book.grants(&viewer).unwrap().contains(&GateGrant::new("tickets.list")));
        assert_eq!(s.role_edits()["viewer"], vec!["tickets.get".to_string()]);
        assert!(book.names().iter().any(|r| r.as_str() == "editor"));
    }
}
