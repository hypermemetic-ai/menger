//! Server-only: the session check at the upgrade, the request struct, and
//! the resolver that mints a `VerifiedUser`.

use plexus_core::plexus::{AuthContext, AuthzDenyReason, PlexusError, SessionValidator};
use plexus_macros::PlexusRequest;

/// The fixed session cookies the spike knows: `access_token=spike-alice`
/// (admitted by the resolver) and `access_token=spike-mallory` (a valid
/// session the resolver refuses). Anything else is no session at all.
pub const ALICE: &str = "spike-alice";
pub const MALLORY: &str = "spike-mallory";

/// The upgrade's session check: a known cookie becomes an AuthContext.
pub struct SpikeSessions;

#[async_trait::async_trait]
impl SessionValidator for SpikeSessions {
    async fn validate(&self, cookie: &str) -> Option<AuthContext> {
        let token = cookie
            .split(';')
            .filter_map(|kv| kv.trim().split_once('='))
            .find(|(k, _)| *k == "access_token")
            .map(|(_, v)| v)?;
        let user = match token {
            ALICE => "alice",
            MALLORY => "mallory",
            _ => return None,
        };
        Some(AuthContext {
            user_id: user.into(),
            session_id: format!("s-{user}"),
            roles: vec![],
            metadata: serde_json::json!({}),
        })
    }
}

/// The per-call request context, as ACCESS-13's `MengerRequest` would be.
#[derive(PlexusRequest)]
pub struct SpikeRequest {
    #[from_auth_context]
    pub auth: Option<AuthContext>,
    #[from_header("origin")]
    pub origin: Option<String>,
}

/// The capability every method receives (ACCESS-9's, in miniature).
#[derive(Debug, Clone)]
pub struct VerifiedUser {
    name: String,
}

impl VerifiedUser {
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The rule every method asks first: only alice is admitted. A refusal is
/// the typed `PlexusError::Forbidden` the grant gate answers with — what
/// question 4 asks is whether it reaches the wire typed.
#[derive(Clone, Copy, Debug)]
pub struct Access;

impl Access {
    pub async fn verify(&self, ctx: &AuthContext) -> Result<VerifiedUser, PlexusError> {
        if ctx.user_id == "alice" {
            Ok(VerifiedUser { name: ctx.user_id.clone() })
        } else {
            Err(PlexusError::Forbidden { reason: AuthzDenyReason::NotAccepted })
        }
    }
}
