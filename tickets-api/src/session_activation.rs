//! IDENTITY-14: the `session` activation — **the one declaration**, as
//! `tickets` and `comments` are (MFE-11): the server's [`Session`] and the
//! browser's `SessionClient`.
//!
//! `whoami` reads the connection's
//! [`AuthContext`](plexus_core::plexus::AuthContext), which the transport
//! attached at the upgrade, and reports it as Plexus types. IDENTITY-15:
//! `providers` lists what menger was configured to admit, so a sign-in knows
//! where to go. IDENTITY-42: `login` mints a session token for the caller
//! (an agent that opened the socket with a key proof, or anyone signed in),
//! held to a scope when asked; `logout` ends the caller's own session.
//!
//! AUTH-7: the protocol's `auth` activation answers the same — `auth.whoami`,
//! `auth.login {session}`, `auth.logout`, `auth.providers` — over the same
//! session table; this one stays, unchanged on the wire, for the browser app
//! and IDENTITY-42's agents. Under login every method here is one any
//! signed-in caller reaches (the hub's signed-in grants), whatever it holds.
//!
//! ACCESS-19: **on the frame** (README "The frame", ACCESS-13) —
//! `auth = "required"`, `request = MengerRequest`; `whoami`, `login` and
//! `logout` take the [`VerifiedUser`] the dispatch resolves (ACCESS-3 #14: a
//! sign-in that ended, or expired, proves no one — refused
//! `unauthenticated` before the body); `providers`, what the login page
//! needs before anyone has signed in, is `public`.

use crate::session::SessionEvent;
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::vocab::Ceiling;
#[cfg(feature = "server")]
use crate::session::ProviderOffer;
#[cfg(feature = "server")]
use std::sync::Arc;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use plexus_core::plexus::AuthContext;
#[cfg(feature = "server")]
use crate::access::{Access, MengerRequest, VerifiedUser};

/// The activation. Server-only, like the others; it holds the providers it
/// offers and the rule its callers are verified by.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct SessionActivation {
    offers: Arc<Vec<ProviderOffer>>,
    /// IDENTITY-37 / ACCESS-19: the rule `whoami`, `login` and `logout`'s
    /// callers are verified by.
    access: Access,
}

#[cfg(feature = "server")]
impl SessionActivation {
    /// ACCESS-24: a test's — offering no provider, behind
    /// [`Access::for_tests`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn new() -> Self {
        Self::offering(Vec::new(), Access::for_tests())
    }

    /// A session offering these providers, IDENTITY-37: behind the same
    /// rule as every other method — a connection whose session was signed
    /// out, or whose credential expired, proves no one.
    pub fn offering(offers: Vec<ProviderOffer>, access: Access) -> Self {
        Self { offers: Arc::new(offers), access }
    }

    /// The same session behind another rule.
    pub fn with_access(mut self, access: Access) -> Self {
        self.access = access;
        self
    }
}

/// A connection's identity as the `session` vocabulary says it. An absent
/// or anonymous context is [`SessionEvent::Anonymous`].
#[cfg(feature = "server")]
pub fn identity(proved: Option<&AuthContext>) -> SessionEvent {
    let Some(ctx) = proved.filter(|c| c.is_authenticated()) else {
        return SessionEvent::Anonymous;
    };
    let principal = ctx.principal().map(|p| p.to_string()).unwrap_or_else(|| ctx.user_id.clone());
    SessionEvent::Identity {
        name: ctx.get_metadata_string("name").unwrap_or_else(|| principal.clone()),
        principal,
        roles: ctx.roles.clone(),
        tenant: ctx.tenant_id().map(|t| t.as_str().to_string()),
        provider: ctx.get_metadata_string("provider"),
    }
}

/// `session.login`'s answer: the minted session, or the refusal in band.
#[cfg(feature = "server")]
fn minted(access: &Access, user: &VerifiedUser, ttl_secs: Option<u64>, scope: Option<Vec<Ceiling>>) -> SessionEvent {
    let scope = scope.map(|s| s.into_iter().map(String::from).collect());
    match access.mint(user.context(), ttl_secs, scope) {
        Ok(m) => SessionEvent::Login { token: m.token, principal: m.principal, agent: m.agent, expires_at: m.expires_at, scope: m.scope },
        Err(message) => SessionEvent::Error { message },
    }
}

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
/// Who this connection is: its Plexus principal, name, roles and tenant.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
)]
impl SessionActivation {
    /// Who this connection is: identity { principal, name, roles, tenant, provider }, or anonymous.
    ///
    /// Yields exactly one event: `identity` — `principal` (`idp:<uuid>`,
    /// `oidc:<provider>/<sub>` or `nostr:<hex>`), `name`, `roles`, `tenant`
    /// and `provider` — always someone: menger has no open mode (ACCESS-24),
    /// and an anonymous connection reaches only `auth.*`. The identity is
    /// settled when the connection opens; reconnect to change it.
    /// ACCESS-19 (ACCESS-3 #14): the caller is verified as every method's
    /// is — a sign-in signed out or expired is refused `unauthenticated`,
    /// never reported as signed in.
    #[plexus_macros::method(group = "you")]
    pub async fn whoami(&self, #[from_auth(self.access.verify)] user: VerifiedUser) -> impl Stream<Item = SessionEvent> + Send + 'static {
        futures::stream::once(std::future::ready(identity(user.context())))
    }

    /// Mint a session for this identity: login { token, principal, agent, expires_at, scope }.
    ///
    /// IDENTITY-42: a session for this connection's identity. Yields
    /// `login { token, principal, agent, expires_at, scope }`, or an in-band
    /// `error`. The token (`mgs_…`) opens later connections as this
    /// identity — `Authorization: Bearer mgs_…` or `Cookie:
    /// access_token=mgs_…` — until `expires_at` or `logout`. `ttl_secs`:
    /// default 43200 (12 h), at most 86400. `scope`: the session's ceiling,
    /// `role@SCOPE` each (`editor@IDENTITY`, `viewer@*`, `editor@MENGER-12`):
    /// every check then passes only if the principal's grants AND the scope
    /// allow it. A session minted from a scoped session keeps that scope too.
    #[plexus_macros::method(group = "sign in")]
    pub async fn login(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// How long it lasts, seconds (default 43200, at most 86400).
        ttl_secs: Option<u64>,
        /// Its ceiling, role@SCOPE each (e.g. editor@IDENTITY); default none beyond your grants.
        scope: Option<Vec<Ceiling>>,
    ) -> impl Stream<Item = SessionEvent> + Send + 'static {
        futures::stream::once(std::future::ready(minted(&self.access, &user, ttl_secs, scope)))
    }

    /// End this connection's session: logged_out.
    ///
    /// IDENTITY-42: ends the session this connection is on (its token stops
    /// opening connections, and its open connections stop answering).
    /// Yields `logged_out`, or an in-band `error` when the connection holds
    /// no session.
    #[plexus_macros::method(group = "you")]
    pub async fn logout(&self, #[from_auth(self.access.verify)] user: VerifiedUser) -> impl Stream<Item = SessionEvent> + Send + 'static {
        let event = match self.access.logout(user.context()) {
            Ok(()) => SessionEvent::LoggedOut,
            Err(message) => SessionEvent::Error { message },
        };
        futures::stream::once(std::future::ready(event))
    }

    /// The providers this menger admits callers through: provider { name, kind, issuer, audience }.
    ///
    /// IDENTITY-15: one `provider { name, kind, issuer, audience }` each, in
    /// the order they were configured; none when only agents' keys are
    /// admitted (`key` is offered to no person, ACCESS-24). Nothing here
    /// is secret: it is what a sign-in needs to find the provider — so it is
    /// `public` (ACCESS-19), answered before anyone has signed in.
    #[plexus_macros::method(group = "sign in", public)]
    pub async fn providers(&self) -> impl Stream<Item = SessionEvent> + Send + 'static {
        let offers: Vec<SessionEvent> = self.offers.iter().cloned().map(SessionEvent::from).collect();
        futures::stream::iter(offers)
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use crate::access::SessionCheck;

    const SESSION_KEY: &str = "menger_session";
    use futures::StreamExt;
    use std::sync::Mutex;

    struct Live(Mutex<bool>);
    impl SessionCheck for Live {
        fn alive(&self, _: &str) -> bool {
            *self.0.lock().unwrap()
        }
        fn key(&self) -> &str {
            SESSION_KEY
        }
    }

    async fn whoami(s: &SessionActivation, user: VerifiedUser) -> SessionEvent {
        Box::pin(s.whoami(user).await).next().await.unwrap()
    }

    /// ACCESS-19 (ACCESS-3 #14): whoami's caller is verified as every
    /// method's is — signed out, it is refused before the body, as
    /// `unauthenticated`, never reported as signed in.
    #[tokio::test]
    async fn whoami_after_sign_out_is_refused_unauthenticated() {
        let live = Arc::new(Live(Mutex::new(true)));
        let access = Access::with_login(live.clone(), vec![]);
        let s = SessionActivation::new().with_access(access.clone());
        let principal = "idp:c952acd3-0000-4000-8000-000000000001";
        let ctx = AuthContext::new("u1".into(), "s".into(), vec![], serde_json::json!({ SESSION_KEY: "k", "name": "alice", "principal": principal }));
        let user = access.verify_now(&ctx).expect("signed in");
        assert_eq!(whoami(&s, user).await.name(), Some("alice"));
        *live.0.lock().unwrap() = false;
        let refused = access.verify_now(&ctx).expect_err("signed out: it proves no one");
        let wire = plexus_core::plexus::PlexusError::from(refused);
        assert!(matches!(wire, plexus_core::plexus::PlexusError::Unauthenticated(_)), "{wire:?}");
        // ACCESS-24: no open mode — an identity that proves no one is no user.
        assert!(access.verify_now(&AuthContext::anonymous()).is_err());
    }
}
