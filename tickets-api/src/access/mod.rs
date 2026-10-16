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
//! ([`Access::verify`]): a connection's session may have been signed out or
//! have expired since its upgrade (IDENTITY-19); someone signed in holding
//! nothing waits for an admin ([`Access::waiting`]); and a read without a
//! target — `list`, `search`, `watch` — answers only from where its grant is
//! held ([`VerifiedUser::scoped`]: the caller's filter AND `under` those
//! scopes, AND each ceiling layer's), as IDENTITY-21 and IDENTITY-37 made
//! it. Streams ask again at every event, as the grants stand then.
//!
//! ACCESS-9: what `verify` answers is a **capability** ([`user`]): a
//! [`VerifiedUser`], holding its [`Reach`] — its grants (direct ∪ its
//! teams'), compiled against the kept tree ([`reach`]), never as filter
//! text — from which a [`Permit`] for one ticket, or an [`AdminOf`] a
//! scope, is minted; [`Hidden`] when the ticket is not there or not its.
//! The store mints the one [`System`], for what acts for no user.
//!
//! **Without login** ([`Access::open`]) there is no gate and every caller
//! reaches everything, as menger always did.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use async_trait::async_trait;
use plexus_auth_core::identity::Principal;
use plexus_auth_core::RoleName;
use plexus_core::plexus::{
    AuthContext, Ceiling as GateCeiling, Gate, Grant as GateGrant, GrantSet, Membership, MembershipStore, Memberships,
    Resolve, RoleOrGrant, RoleStore, Roles, ScopeRef, StoreError as GateStoreError, ADMIN,
};

use crate::filter::Filter;
use crate::members::{Grant, Role, Scope, EVERYWHERE};
use crate::vocab::{GrantedBy, Grantee};
use crate::store::{Lens, Store};

pub mod reach;
pub mod user;
/// ACCESS-12: an open watch's caller, followed as the grants move.
pub mod following;
pub use following::{Closed, Followed, Following};
/// ACCESS-13: the frame's per-call request.
pub mod request;
pub use request::MengerRequest;
pub use crate::store::System;
pub use reach::{Held, Reach, ReachKey};
pub use user::{Admin, AdminOf, Deployer, DeployerOf, Editor, EditorOf, Forward, Hidden, On, Permit, ReadingAs, Refusal, RoleMark, VerifiedUser, Viewer};

/// The gate's reach for one call (plexus-core's): the grant a method was
/// admitted for, and where.
pub use plexus_core::plexus::Reach as GateReach;

/// The grantor named on `--admin` grants.
pub use crate::vocab::OPERATOR;

/// IDENTITY-35: `granted_by` of the grants the dev sign-in makes; AUTH-7:
/// of a grant made through `auth.grant`, which does not say who asked
/// ([`crate::vocab::GrantedBy`]).
pub use crate::vocab::{AUTH_GRANT, DEV_LOGIN};

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

/// A ceiling's scope — the vocabulary's rule (ACCESS-8 moved it there).
pub use crate::vocab::ceiling_scope_of;

/// ORG-4: the `under` name a held scope reads as — `*` none (everything);
/// an organization, `acme:`, itself (the filter's `under acme:`: every
/// ticket in it); a shortcode in one, `acme:MENGER.12`, its identity; a
/// ticket (a ceiling's) or a scope from before ORG-4, itself.
fn under_name(scope: &str) -> String {
    match scope.trim().split_once(':') {
        Some((org, "")) => format!("{}:", org.to_ascii_lowercase()),
        Some((_, id)) => id.to_ascii_uppercase(),
        None => scope.trim().to_ascii_uppercase(),
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

// ===========================================================================
// The book: what the gate reads
// ===========================================================================

/// AUTH-7: menger's roles, memberships and scopes, over the store — the
/// three things the gate asks (and `auth.role_set`, `auth.grant`,
/// `auth.revoke` write). One value implements every trait; the hub is
/// handed it once per trait. Answers from memory: the store's cache.
pub struct Book {
    store: OnceLock<Arc<Store>>,
    /// ACCESS-9: the store's [`System`], taken at [`Access::attach`] — what
    /// the access layer's own reads of the grants act as (ACCESS-10).
    system: OnceLock<Arc<System>>,
    admins: Vec<String>,
    /// Each role's grants as the activations declare them
    /// (`roles(...)`, merged by name) — seeded once the hub is built
    /// ([`Book::seed`]).
    defaults: RwLock<BTreeMap<String, BTreeSet<String>>>,
    waiting: Mutex<WaitingList>,
}

/// The waiting list, and (ACCESS-29) its order by arrival beside it: at the
/// cap, the one waiting longest is found in O(log n), not by a scan.
#[derive(Default)]
struct WaitingList {
    by_principal: BTreeMap<String, Waiting>,
    by_since: BTreeSet<(i64, String)>,
}

impl WaitingList {
    fn remove(&mut self, principal: &str) {
        if let Some(w) = self.by_principal.remove(principal) {
            self.by_since.remove(&(w.since, w.principal));
        }
    }
}

impl Book {
    fn new(admins: Vec<String>) -> Self {
        Self { store: OnceLock::new(), system: OnceLock::new(), admins, defaults: RwLock::new(BTreeMap::new()), waiting: Mutex::new(WaitingList::default()) }
    }

    fn store(&self) -> Option<&Arc<Store>> {
        self.store.get()
    }

    /// ACCESS-11: the store and the [`System`] the access layer's own
    /// writes act as — `auth.grant`, `auth.revoke`, `auth.role_set` (the
    /// gate decided them), the operator's dev sign-in.
    fn writer(&self) -> Result<(&Arc<Store>, &System), GateStoreError> {
        match (self.store(), self.system.get()) {
            (Some(store), Some(system)) => Ok((store, &**system)),
            _ => Err(GateStoreError::new("no store is attached")),
        }
    }

    /// ACCESS-10: the stored grants of `principal`, read as the store's
    /// [`System`] (none before a store is attached with it).
    fn stored_grants(&self, principal: &str) -> Vec<Grant> {
        match (self.store(), self.system.get()) {
            (Some(store), Some(system)) => store.grants_of(&**system, principal),
            _ => Vec::new(),
        }
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

    /// A principal's grants: the stored ones, ACCESS-9: its teams' (each
    /// a grant to `team:<name>`; the store says which teams it is in), and
    /// admin on `*` for an operator's `--admin`.
    pub fn grants_of(&self, principal: &str) -> Vec<Grant> {
        let mut grants = self.stored_grants(principal);
        if let Some(store) = self.store() {
            for team in store.teams_of(principal) {
                grants.extend(self.stored_grants(&crate::vocab::Grantee::Team(team).to_string()));
            }
        }
        if self.admins.iter().any(|a| a == principal) {
            // ACCESS-48: an operator's `--admin` is a principal (the command
            // line refuses one that is not), so it is one here.
            if let Ok(p) = principal.parse::<crate::vocab::Grantee>() {
                grants.push(Grant { principal: p, scope: Scope::Everywhere, role: Role::Admin, granted_by: GrantedBy::Operator, at: 0 });
            }
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
        if let Some(w) = waiting.by_principal.get(principal) {
            return w.clone();
        }
        while waiting.by_principal.len() >= WAITING_MAX {
            let Some((_, oldest)) = waiting.by_since.pop_first() else { break };
            waiting.by_principal.remove(&oldest);
        }
        let w = Waiting { principal: principal.to_string(), name, provider, since: now() };
        waiting.by_since.insert((w.since, w.principal.clone()));
        waiting.by_principal.insert(principal.to_string(), w.clone());
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
        let (store, system) = self.writer()?;
        let grants: Vec<String> = set.iter().map(|g| g.as_str().to_string()).collect();
        store.set_role(system, role.as_str().to_string(), grants).await.map_err(|e| GateStoreError::new(e.to_string()))
    }
}

impl Memberships for Book {
    fn held(&self, who: &Principal) -> Vec<Membership> {
        self.grants_of(&who.to_string())
            .into_iter()
            .map(|g| Membership { principal: who.clone(), role: RoleName::new(g.role.as_str()), scope: ScopeRef::parse(&g.scope.text()) })
            .collect()
    }
}

#[async_trait]
impl MembershipStore for Book {
    async fn grant(&self, m: Membership) -> Result<(), GateStoreError> {
        let (store, system) = self.writer()?;
        // IDENTITY-21's rows hold viewer, editor or admin (DEPLOY-14: or
        // deployer); a role an admin defined with auth.role_set is not a
        // membership menger keeps yet.
        let role: Role = m.role.as_str().parse().map_err(|_| {
            GateStoreError::new(format!("menger keeps viewer, editor, admin and deployer memberships; {} is not one of them", m.role))
        })?;
        // ORG-4: kept as the store keeps a scope — the org, the identity.
        let scope = store.scope_named(m.scope.as_str()).map_err(|e| GateStoreError::new(e.to_string()))?;
        let principal = crate::vocab::Principal::from(&m.principal);
        let g = Grant { principal: Grantee::Principal(principal.clone()), scope, role, granted_by: GrantedBy::AuthGrant, at: now() };
        store.grant(system, g).await.map_err(|e| GateStoreError::new(e.to_string()))?;
        self.granted(principal.as_str());
        Ok(())
    }

    async fn revoke(&self, principal: &Principal, role: Option<&RoleName>, scope: &ScopeRef) -> Result<Vec<RoleName>, GateStoreError> {
        let (store, system) = self.writer()?;
        let scope = store.scope_named(scope.as_str()).map_err(|e| GateStoreError::new(e.to_string()))?;
        let who = principal.to_string();
        let held = self.stored_grants(&who).into_iter().find(|g| g.scope == scope);
        let Some(held) = held.filter(|g| role.is_none_or(|r| r.as_str() == g.role.as_str())) else {
            return Ok(Vec::new());
        };
        store.revoke(system, held.principal.clone(), scope).await.map_err(|e| GateStoreError::new(e.to_string()))?;
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
        // ORG-4: each shortcode as `org:ID`, then the organization, `org:`.
        let mut out: Vec<ScopeRef> = store.gate_chain(target).unwrap_or_default().into_iter().map(ScopeRef::Named).collect();
        out.push(ScopeRef::Everywhere);
        Some(out)
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
    /// ACCESS-12: moved by every sign-out through this access layer — what
    /// wakes an open watch to verify its caller again.
    ended: Arc<tokio::sync::watch::Sender<u64>>,
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
        Self { login, require_login, sessions, minter: None, book, gate, ended: Arc::new(tokio::sync::watch::channel(0).0) }
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

    /// ACCESS-10: login on, for a test in another module (compiled out of
    /// the server): no session to end, no operator, and the roles as the
    /// activations declare a part of them — `viewer` the reads
    /// (`tickets.get`, `tickets.list`), `editor` those and `tickets.set_status`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_tests() -> Self {
        struct Never;
        impl SessionCheck for Never {
            fn alive(&self, _: &str) -> bool {
                false
            }
            fn key(&self) -> &str {
                "menger_session"
            }
        }
        let a = Self::with_login(Arc::new(Never), false, Vec::new());
        let viewer = vec!["tickets.get".to_string(), "tickets.list".to_string()];
        let mut editor = viewer.clone();
        editor.push("tickets.set_status".into());
        a.book().seed(BTreeMap::from([("viewer".to_string(), viewer), ("editor".to_string(), editor)]));
        a
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
    /// ACCESS-9: the access layer takes the store's [`System`] here, when
    /// no one has taken it before.
    pub fn attach(&self, store: Arc<Store>) {
        if let Some(system) = store.system() {
            let _ = self.book.system.set(Arc::new(system));
        }
        let _ = self.book.store.set(store);
    }

    /// ACCESS-10: [`Access::attach`], handed the store's [`System`] by
    /// whoever took it at the open (the builder, which read the store as
    /// the system first).
    /// ACCESS-11: shared — the builder keeps the store's upkeep (the purge)
    /// acting as it too.
    pub fn attach_with(&self, store: Arc<Store>, system: impl Into<Arc<System>>) {
        let _ = self.book.system.set(system.into());
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

    /// ACCESS-10: every stored grant, by principal then scope — read as the
    /// store's [`System`] (`members.list` cuts it to where its caller is
    /// admin).
    pub fn stored(&self) -> Vec<Grant> {
        match (self.book.store(), self.book.system.get()) {
            (Some(store), Some(system)) => store.grants(system),
            _ => Vec::new(),
        }
    }

    /// The operator's `--admin` principals.
    pub fn admins(&self) -> Vec<String> {
        self.book.admins.clone()
    }

    /// IDENTITY-35: a grant the operator's dev sign-in makes (test only),
    /// kept in the store like any other and signed `granted_by` `dev-login`.
    pub async fn dev_grant(&self, principal: &str, scope: &str, role: Role) -> Result<Grant, String> {
        let (store, system) = self.book.writer().map_err(|e| e.to_string())?;
        let scope = store.scope_named(scope).map_err(|e| e.to_string())?;
        let who = principal.parse::<crate::vocab::Grantee>()?;
        let g = Grant { principal: who, scope, role, granted_by: GrantedBy::DevLogin, at: now() };
        let g = store.grant(system, g).await.map_err(|e| e.to_string())?;
        self.granted(principal);
        Ok(g)
    }

    /// IDENTITY-35: a roster user's grants made exactly `grants`: each kept,
    /// and every other stored grant of theirs revoked.
    pub async fn dev_set_grants(&self, principal: &str, grants: &[(Role, String)]) -> Result<(), String> {
        let (store, system) = self.book.writer().map_err(|e| e.to_string())?;
        let named: Vec<Scope> = grants.iter().map(|(_, s)| store.scope_named(s).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
        for g in self.book.stored_grants(principal) {
            if !named.contains(&g.scope) {
                store.revoke(system, g.principal, g.scope).await.map_err(|e| e.to_string())?;
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
        let mut w: Vec<Waiting> = self.book.waiting.lock().unwrap().by_principal.values().cloned().collect();
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
    /// nothing, it waits for an admin ([`Access::waiting`]). ACCESS-9: an
    /// identity naming no principal is no one to wait.
    pub fn notice(&self, ctx: &AuthContext) {
        if !self.login || !ctx.is_authenticated() {
            return;
        }
        let Some(principal) = ctx.principal().map(|p| p.to_string()) else { return };
        if self.book.grants_of(&principal).is_empty() {
            self.book.wait(&principal, ctx.get_metadata_string("name"), ctx.get_metadata_string("provider"));
        }
    }

    /// Whether a signed-in identity's sign-in still stands: not expired, its
    /// session not signed out. `Err`: why not.
    pub fn alive(&self, ctx: &AuthContext) -> Result<(), String> {
        self.liveness(ctx).map_err(String::from)
    }

    fn liveness(&self, ctx: &AuthContext) -> Result<(), Refusal> {
        if let Some(exp) = ctx.metadata.get("expires_at").and_then(|v| v.as_i64()) {
            if exp <= now() {
                return Err(Refusal::Expired);
            }
        }
        if let Some((sessions, key)) = self.sessions.as_ref().and_then(|s| Some((s, ctx.get_metadata_string(s.key())?))) {
            if !sessions.alive(&key) {
                return Err(Refusal::SignedOut);
            }
        }
        Ok(())
    }

    /// ACCESS-9: the one door to a [`VerifiedUser`]. What the gate does not
    /// ask, asked here (it has admitted the call already): the identity is
    /// signed in, its sign-in alive and unexpired, its principal parsed by
    /// the identity layer (`vocab::Principal`'s grammar; never its
    /// `user_id` instead), its ceiling readable; its grants resolved —
    /// direct ∪ its teams' ∪ the operator's — at the grants generation of
    /// now. A signed-in user holding nothing joins the waiting list (and
    /// is still verified: a read then says it holds nothing). Login off:
    /// anyone, everything. Synchronous: it reads the cache and nothing
    /// else, so a method verifies before its stream starts.
    pub fn verify_now(&self, ctx: &AuthContext) -> Result<VerifiedUser, Refusal> {
        if !self.login {
            return Ok(VerifiedUser::anyone());
        }
        if !ctx.is_authenticated() {
            return Err(Refusal::SignIn);
        }
        self.liveness(ctx)?;
        let p = ctx.principal().ok_or(Refusal::NoPrincipal)?;
        let principal: crate::vocab::Principal = p.to_string().parse().map_err(|_| Refusal::NoPrincipal)?;
        let reach = Reach::resolve(&self.book, &self.gate, principal.as_str(), ctx)?;
        let name = ctx.get_metadata_string("name");
        if self.book.grants_of(principal.as_str()).is_empty() {
            self.book.wait(principal.as_str(), name.clone(), ctx.get_metadata_string("provider"));
        } else {
            self.book.granted(principal.as_str());
        }
        Ok(VerifiedUser::verified(principal, name, reach, ctx.clone()))
    }

    /// [`Access::verify`] for a method's call: `auth` as the method got it
    /// (none, or anonymous, is refused with login on), and the grant the
    /// gate admitted it for (`reach`), which the user's reads answer under.
    /// Its refusal in words, as an in-band error says it.
    pub fn verify_call(&self, auth: Option<&AuthContext>, reach: &GateReach) -> Result<VerifiedUser, String> {
        if !self.login {
            return Ok(VerifiedUser::anyone());
        }
        let ctx = auth.filter(|c| c.is_authenticated()).ok_or(Refusal::SignIn)?;
        Ok(self.verify_now(ctx)?.for_call(reach.grant().cloned()))
    }

    /// ACCESS-13: **the frame's resolver** — every method of an activation
    /// on the frame takes `#[from_auth(self.access.verify)] user:
    /// VerifiedUser`, so the dispatch runs this before the method's body
    /// (and before its parameters are read: auth before params, ACCESS-6).
    /// [`Access::verify_now`] for the call being dispatched: the identity the
    /// upgrade proved, the grant the gate admitted the call for
    /// ([`plexus_core::plexus::current_reach`], read here, in the dispatch's
    /// scope) — kept on the user, with that reach, so an open watch can ask
    /// the gate again as the grants move ([`Following::of`]). Its refusal is
    /// typed on the wire (MACRO-14): a sign-in that does not stand is
    /// `unauthenticated` (-32001), anything else `forbidden` (-32003).
    /// Login off: anyone, everything — whatever identity the transport
    /// handed it (menger's own hand the anonymous one: `menger::calls`).
    pub async fn verify(&self, ctx: &AuthContext) -> Result<VerifiedUser, plexus_core::plexus::PlexusError> {
        if !self.login {
            return Ok(VerifiedUser::anyone());
        }
        let reach = plexus_core::plexus::current_reach();
        let user = self.verify_now(ctx).map_err(plexus_core::plexus::PlexusError::from)?;
        Ok(user.for_call(reach.grant().cloned()).following(reach))
    }

    /// The grants generation now ([`Store::grants_generation`]) — `0`
    /// before a store is attached.
    pub fn generation(&self) -> u64 {
        self.book.store().map(|s| s.grants_generation()).unwrap_or(0)
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
        // ORG-4: a shortcode or an organization as the store keeps it, so
        // the gate's chain holds it (a ticket stays its key-like spelling).
        // ORG-9: a key splits at its last hyphen (`SYNAPSE-CC-1`).
        let ticket = crate::address::split_key(&scope).is_some();
        let scope = match self.book.store() {
            Some(store) if !ticket => store.scope_named(&scope).map_err(|e| e.to_string())?.text(),
            // ACCESS-21: a ticket in a ceiling is it and its subtree, as
            // IDENTITY-42 made it — kept `under` its key, which the gate's
            // chain holds for it and every ticket below it (its key bare is
            // the ticket alone).
            _ if ticket => format!("{}{scope}", crate::members::UNDER),
            _ => scope,
        };
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
        let principal = ctx.principal().map(|p| p.to_string()).ok_or_else(|| Refusal::NoPrincipal.to_string())?;
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
        let principal = ctx.principal().map(|p| p.to_string()).ok_or_else(|| Refusal::NoPrincipal.to_string())?;
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
                // ACCESS-12: the open watches look again.
                self.ended.send_modify(|n| *n += 1);
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
mod tests;
