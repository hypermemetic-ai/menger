//! SCAFFOLD-3: menger's side of plexus-login's seam.
//!
//! plexus-login (menger's login, lifted as it is) asks the app one trait,
//! [`plexus_login::LoginAccess`]; menger answers it over its `Access` —
//! the members table, the waiting list, session minting. Both are foreign
//! here (`Access` is tickets-api's, the trait plexus-login's), so the impl
//! is on a newtype, [`MengerAccess`]. The other direction — `Access` asking
//! the session table — is tickets-api's `SessionCheck` / `SessionMint` on
//! another newtype, [`SessionTable`].

use std::sync::Arc;

use async_trait::async_trait;
use menger_tickets_api::access::{Access, Minted, SessionCheck, SessionMint, Waiting as MengerWaiting};
use menger_tickets_api::members::{scope_of, Role};
use menger_tickets_api::{ProviderKind, ProviderOffer};
use plexus_core::plexus::{AuthContext, Session, Waiting};
use plexus_login::login::{OfferKind, Provider};
use plexus_login::sessions::Sessions;
use plexus_login::{Branding, LoginAccess, LoginNames};

/// menger's access rules, as plexus-login reaches them.
#[derive(Clone)]
pub struct MengerAccess(pub Access);

impl From<Access> for MengerAccess {
    fn from(a: Access) -> Self {
        Self(a)
    }
}

fn session(m: Minted) -> Session {
    Session { token: m.token, principal: m.principal, agent: m.agent, name: m.name, expires_at: m.expires_at, ceiling: m.scope }
}

fn waiting(w: MengerWaiting) -> Waiting {
    Waiting { principal: w.principal, name: w.name, provider: w.provider, since: w.since }
}

#[async_trait]
impl LoginAccess for MengerAccess {
    type Role = Role;

    fn scope_of(text: &str) -> Result<String, String> {
        scope_of(text)
    }

    async fn dev_grant(&self, principal: &str, scope: &str, role: Role) -> Result<(), String> {
        self.0.dev_grant(principal, scope, role).await.map(|_| ())
    }

    async fn dev_set_grants(&self, principal: &str, grants: &[(Role, String)]) -> Result<(), String> {
        self.0.dev_set_grants(principal, grants).await
    }

    fn notice(&self, ctx: &AuthContext) {
        self.0.notice(ctx)
    }

    fn alive(&self, ctx: &AuthContext) -> Result<(), String> {
        self.0.alive(ctx)
    }

    fn mint(&self, caller: Option<&AuthContext>, ttl_secs: Option<u64>, ceiling: Option<Vec<String>>) -> Result<Session, String> {
        self.0.mint(caller, ttl_secs, ceiling).map(session)
    }

    fn mint_for(&self, ctx: AuthContext, ttl_secs: u64) -> Result<Session, String> {
        self.0.mint_for(ctx, ttl_secs).map(session)
    }

    fn ask_access(&self, ctx: &AuthContext) -> Result<Waiting, String> {
        self.0.ask_access(ctx).map(waiting)
    }

    fn waiting(&self) -> Vec<Waiting> {
        self.0.waiting().into_iter().map(waiting).collect()
    }

    fn logout(&self, caller: Option<&AuthContext>) -> Result<(), String> {
        self.0.logout(caller)
    }

    fn session_key(&self) -> Option<&str> {
        self.0.session_key()
    }
}

/// plexus-login's session table, as menger's `Access` asks it: is this
/// connection's session still standing (`SessionCheck`); `session.login`
/// and `session.logout` mint and end sessions in it (`SessionMint`).
pub struct SessionTable(pub Arc<Sessions>);

impl SessionCheck for SessionTable {
    fn alive(&self, key: &str) -> bool {
        self.0.alive(key)
    }

    /// SCAFFOLD-9: the table's own metadata key — its one definition.
    fn key(&self) -> &str {
        self.0.session_key()
    }
}

impl SessionMint for SessionTable {
    fn mint(&self, ctx: AuthContext, ttl_secs: i64) -> (String, i64) {
        self.0.mint(ctx, ttl_secs)
    }

    fn end_key(&self, key: &str) -> bool {
        self.0.end_key(key)
    }
}

/// The session table, for `Access::with_login(…)` and `.minting(…)`.
pub fn session_table(sessions: &Arc<Sessions>) -> Arc<SessionTable> {
    Arc::new(SessionTable(sessions.clone()))
}

/// What `session.providers` says of a provider (IDENTITY-15), in
/// tickets-api's wire type.
pub fn offer(p: &Provider) -> ProviderOffer {
    let o = p.offer();
    let kind = match o.kind {
        OfferKind::Oidc => ProviderKind::Oidc,
        OfferKind::Nostr => ProviderKind::Nostr,
    };
    ProviderOffer { name: o.name, kind, issuer: o.issuer, audience: o.audience }
}

/// What the login page says of menger.
pub const BRANDING: Branding = Branding { app: "menger", version: env!("CARGO_PKG_VERSION"), mark: "m", motto: "A plan is a program whose leaves are prose." };

/// SCAFFOLD-9: the names menger's login writes — `menger_session[_<port>]`,
/// `menger_login[_<port>]`, `menger_dev_login`, and `menger_session` as the
/// session metadata key: what they were before the lift, so a signed-in
/// browser stays signed in across the upgrade.
pub fn login_names() -> LoginNames {
    LoginNames::for_app(BRANDING.app)
}
