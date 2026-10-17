//! IDENTITY-14: the `session` vocabulary — who a connection proved it is,
//! in Plexus terms. Wasm-safe, like [`crate::comments`]: the app reads it
//! through the generated `SessionClient`.
//!
//! A connection's identity is settled once, at the WebSocket upgrade, by
//! whichever provider adapter admitted it (plexus-auth-core's `adapters`).
//! Nothing here names a provider's own types: an Auth0 user, a plexus-idp
//! user and a nostr key all arrive as the same [`SessionEvent::Identity`].

use serde::{Deserialize, Serialize};

/// What the `session` activation's methods yield.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    /// The connection proved who it is.
    Identity {
        /// The Plexus principal: `idp:<uuid>`, `oidc:<provider>/<sub>` or
        /// `nostr:<hex>`.
        principal: String,
        /// The name to show: a username, a nickname, or an npub.
        name: String,
        /// The roles the provider granted, as given.
        roles: Vec<String>,
        /// The tenant the identity belongs to, when its provider names one.
        tenant: Option<String>,
        /// The provider adapter that admitted it: `plexus-idp`, `auth0`,
        /// `nostr`, or a configured OIDC provider's name.
        provider: Option<String>,
    },
    /// The connection proved no one.
    Anonymous,
    /// IDENTITY-15, `providers`: one provider this menger admits callers
    /// through — what a sign-in needs to reach it, and nothing secret.
    Provider {
        /// Its name, as `identity`'s `provider` says it.
        name: String,
        kind: ProviderKind,
        /// An OIDC provider's issuer: discovery is under it.
        issuer: Option<String>,
        /// The audience its tokens must be minted for.
        audience: Option<String>,
    },
    /// IDENTITY-42, `login`: a session minted for the caller. `token`
    /// (`mgs_…`) opens later connections as this identity, presented as
    /// `Authorization: Bearer mgs_…` or `Cookie: access_token=mgs_…`, until
    /// `expires_at` (unix seconds) or `logout`.
    Login {
        token: String,
        /// The caller's principal, which the session acts as.
        principal: String,
        /// The agent id a key proof named, when one did.
        agent: Option<String>,
        expires_at: i64,
        /// The session's ceiling, `role@SCOPE` each: every check passes only
        /// if the principal's grants AND this allow it. Empty: no ceiling
        /// beyond the principal's grants.
        scope: Vec<String>,
    },
    /// IDENTITY-42, `logout`: the caller's session has ended.
    LoggedOut,
    /// A refusal, naming the problem; nothing was changed.
    Error { message: String },
}

/// How a provider proves a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// An OIDC token, in the `access_token` cookie or a bearer header.
    Oidc,
    /// A NIP-98 event for the WebSocket URL, in the `nostr` cookie.
    Nostr,
}

/// A provider as `session.providers` offers it ([`SessionEvent::Provider`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderOffer {
    pub name: String,
    pub kind: ProviderKind,
    pub issuer: Option<String>,
    pub audience: Option<String>,
}

impl From<ProviderOffer> for SessionEvent {
    fn from(o: ProviderOffer) -> Self {
        Self::Provider { name: o.name, kind: o.kind, issuer: o.issuer, audience: o.audience }
    }
}

impl SessionEvent {
    /// The identity's name, when there is one.
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Identity { name, .. } => Some(name),
            _ => None,
        }
    }
}

/// MACRO-21: the emitted client implements the activation's trait; a client
/// failure is the in-band error, the last item of its stream.
impl plexus_client::FromClientError for SessionEvent {
    fn from_client_error(error: plexus_client::ClientError) -> Self {
        SessionEvent::Error { message: error.to_string() }
    }
}
