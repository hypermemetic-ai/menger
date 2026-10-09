//! IDENTITY-14: the `session` activation — **the one declaration**, as
//! `tickets` and `comments` are (MFE-11): the server's [`Session`] and the
//! browser's `SessionClient`.
//!
//! `whoami` reads the connection's
//! [`AuthContext`](plexus_core::plexus::AuthContext), which the transport
//! attached at the upgrade, and reports it as Plexus types. IDENTITY-15:
//! `providers` lists what menger was configured to admit, so a sign-in knows
//! where to go.

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

/// The activation. Server-only, like the others; it holds only the
/// providers it offers.
#[cfg(feature = "server")]
#[derive(Clone, Default)]
pub struct Session {
    offers: Arc<Vec<ProviderOffer>>,
}

#[cfg(feature = "server")]
impl Session {
    /// A session offering no provider: `whoami` is always `anonymous`.
    pub fn new() -> Self {
        Self::default()
    }

    /// A session offering these providers.
    pub fn offering(offers: Vec<ProviderOffer>) -> Self {
        Self { offers: Arc::new(offers) }
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
        description = "Who this connection is: identity { principal, name, roles, tenant, provider }, or anonymous"
    )]
    pub async fn whoami(&self, auth: Option<&AuthContext>) -> impl Stream<Item = SessionEvent> + Send + 'static {
        let event = identity(auth);
        stream! {
            yield event;
        }
    }

    /// IDENTITY-15: the providers this menger admits callers through, one
    /// `provider { name, kind, issuer, audience }` each, in the order they
    /// were configured; none when login is off. Nothing here is secret: it
    /// is what a sign-in needs to find the provider.
    #[plexus_macros::method(
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
