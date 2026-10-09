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
}

impl SessionEvent {
    /// The identity's name, when there is one.
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Identity { name, .. } => Some(name),
            Self::Anonymous => None,
        }
    }
}
