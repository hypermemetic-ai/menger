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

use crate::session::SessionEvent;
#[cfg(feature = "server")]
use crate::session::ProviderOffer;
#[cfg(feature = "server")]
use std::sync::Arc;
#[cfg(feature = "server")]
use async_stream::stream;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use plexus_core::plexus::AuthContext;
#[cfg(feature = "server")]
use crate::access::Access;

/// The activation. Server-only, like the others; it holds only the
/// providers it offers.
#[cfg(feature = "server")]
#[derive(Clone, Default)]
pub struct Session {
    offers: Arc<Vec<ProviderOffer>>,
    /// IDENTITY-37: when given, `whoami` asks it whether the connection's
    /// sign-in still stands — signed out or expired, it proves no one.
    access: Option<Access>,
}

#[cfg(feature = "server")]
impl Session {
    /// A session offering no provider: `whoami` is always `anonymous`.
    pub fn new() -> Self {
        Self::default()
    }

    /// A session offering these providers.
    pub fn offering(offers: Vec<ProviderOffer>) -> Self {
        Self { offers: Arc::new(offers), access: None }
    }

    /// IDENTITY-37: `whoami` behind the same rule as every other method: a
    /// connection whose session was signed out, or whose credential expired,
    /// is `anonymous`.
    pub fn with_access(mut self, access: Access) -> Self {
        self.access = Some(access);
        self
    }
}

/// A connection's identity as the `session` vocabulary says it. An absent
/// or anonymous context is [`SessionEvent::Anonymous`].
#[cfg(feature = "server")]
pub fn identity(auth: Option<&AuthContext>) -> SessionEvent {
    let Some(ctx) = auth.filter(|c| c.is_authenticated()) else {
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

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
#[plexus_macros::activation(
    namespace = "session",
    description = "Who this connection is: its Plexus principal, name, roles and tenant",
    client,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core"
)]
impl Session {
    /// Who this connection is. Yields exactly one event: `identity` —
    /// `principal` (`idp:<uuid>`, `oidc:<provider>/<sub>` or
    /// `nostr:<hex>`), `name`, `roles`, `tenant` and `provider` — or
    /// `anonymous` when the upgrade proved no one. The identity is settled
    /// when the connection opens; reconnect to change it.
    #[plexus_macros::method(
        group = "you",
        description = "Who this connection is: identity { principal, name, roles, tenant, provider }, or anonymous"
    )]
    pub async fn whoami(&self, auth: Option<&AuthContext>) -> impl Stream<Item = SessionEvent> + Send + 'static {
        // IDENTITY-37: a sign-in that no longer stands proves no one.
        let ended = self.access.as_ref().is_some_and(|a| a.login() && auth.filter(|c| c.is_authenticated()).is_some_and(|c| a.alive(c).is_err()));
        let event = if ended { SessionEvent::Anonymous } else { identity(auth) };
        stream! {
            yield event;
        }
    }

    /// IDENTITY-42: a session for this connection's identity. Yields
    /// `login { token, principal, agent, expires_at, scope }`, or an in-band
    /// `error`. The token (`mgs_…`) opens later connections as this
    /// identity — `Authorization: Bearer mgs_…` or `Cookie:
    /// access_token=mgs_…` — until `expires_at` or `logout`. `ttl_secs`:
    /// default 43200 (12 h), at most 86400. `scope`: the session's ceiling,
    /// `role@SCOPE` each (`editor@IDENTITY`, `viewer@*`, `editor@MENGER-12`):
    /// every check then passes only if the principal's grants AND the scope
    /// allow it. A session minted from a scoped session keeps that scope too.
    #[plexus_macros::method(
        group = "sign in",
        description = "Mint a session for this identity: login { token, principal, agent, expires_at, scope }",
        params(
            ttl_secs = "How long it lasts, seconds (default 43200, at most 86400)",
            scope = "Its ceiling, role@SCOPE each (e.g. editor@IDENTITY); default none beyond your grants"
        )
    )]
    pub async fn login(
        &self,
        auth: Option<&AuthContext>,
        ttl_secs: Option<u64>,
        scope: Option<Vec<String>>,
    ) -> impl Stream<Item = SessionEvent> + Send + 'static {
        let event = match &self.access {
            None => SessionEvent::Error { message: "login is off on this menger: there is no session to log in".into() },
            Some(a) => match a.mint(auth, ttl_secs, scope) {
                Ok(m) => SessionEvent::Login { token: m.token, principal: m.principal, agent: m.agent, expires_at: m.expires_at, scope: m.scope },
                Err(message) => SessionEvent::Error { message },
            },
        };
        stream! {
            yield event;
        }
    }

    /// IDENTITY-42: ends the session this connection is on (its token stops
    /// opening connections, and its open connections stop answering).
    /// Yields `logged_out`, or an in-band `error` when the connection holds
    /// no session.
    #[plexus_macros::method(group = "you", description = "End this connection's session: logged_out")]
    pub async fn logout(&self, auth: Option<&AuthContext>) -> impl Stream<Item = SessionEvent> + Send + 'static {
        let event = match &self.access {
            None => SessionEvent::Error { message: "login is off on this menger: there is no session to log out".into() },
            Some(a) => match a.logout(auth) {
                Ok(()) => SessionEvent::LoggedOut,
                Err(message) => SessionEvent::Error { message },
            },
        };
        stream! {
            yield event;
        }
    }

    /// IDENTITY-15: the providers this menger admits callers through, one
    /// `provider { name, kind, issuer, audience }` each, in the order they
    /// were configured; none when login is off. Nothing here is secret: it
    /// is what a sign-in needs to find the provider.
    #[plexus_macros::method(
        group = "sign in",
        description = "The providers this menger admits callers through: provider { name, kind, issuer, audience }"
    )]
    pub async fn providers(&self) -> impl Stream<Item = SessionEvent> + Send + 'static {
        let offers = Arc::clone(&self.offers);
        stream! {
            for offer in offers.iter() {
                yield SessionEvent::from(offer.clone());
            }
        }
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

    async fn whoami(s: &Session, ctx: &AuthContext) -> SessionEvent {
        Box::pin(s.whoami(Some(ctx)).await).next().await.unwrap()
    }

    #[tokio::test]
    async fn whoami_after_sign_out_is_anonymous() {
        let live = Arc::new(Live(Mutex::new(true)));
        let access = Access::with_login(live.clone(), true, vec![]);
        let s = Session::new().with_access(access);
        let ctx = AuthContext::new("u1".into(), "s".into(), vec![], serde_json::json!({ SESSION_KEY: "k", "name": "alice" }));
        assert_eq!(whoami(&s, &ctx).await.name(), Some("alice"));
        *live.0.lock().unwrap() = false;
        assert_eq!(whoami(&s, &ctx).await, SessionEvent::Anonymous, "signed out: it proves no one");
        // Without the rule, as before.
        assert_eq!(whoami(&Session::new(), &ctx).await.name(), Some("alice"));
    }
}
