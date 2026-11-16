//! **The caller, as the gate admitted it** (FRAME-5's pattern, the media
//! service's `caller.rs`). plexus-core's gate decides every call before a
//! method runs — the method's grant, held through a role menger's rules
//! give, within the session's ceiling — so this service asks nothing of
//! the tickets service's access layer: its resolver ([`Callers::verify`],
//! every method's `#[from_auth(self.callers.verify)]`) takes the identity
//! the upgrade proved and refuses only what the gate cannot have admitted
//! (no one signed in, no principal). A kept commit holds only what GitHub
//! shows anyone, so who may read it is the gate's word alone.

use plexus_rpc::core::plexus::{AuthContext, PlexusError};
use plexus_rpc::macros::PlexusRequest;

/// The per-call context every method of the service reads: the identity the
/// connection's upgrade proved, when one did.
#[derive(PlexusRequest)]
pub struct CodeRequest {
    /// The identity the connection's upgrade proved (none: anonymous).
    #[from_auth_context]
    pub auth: Option<AuthContext>,
}

/// The resolver every method verifies its caller by.
#[derive(Clone, Copy, Debug, Default)]
pub struct Callers;

/// A caller the gate admitted: its principal.
#[derive(Clone, Debug)]
pub struct Caller {
    principal: String,
}

impl Callers {
    /// The caller of the call being dispatched: signed in, naming a
    /// principal. Refused typed (`unauthenticated`) otherwise — the gate
    /// refuses those first.
    pub async fn verify(&self, ctx: &AuthContext) -> Result<Caller, PlexusError> {
        if !ctx.is_authenticated() {
            return Err(PlexusError::Unauthenticated("sign in first".into()));
        }
        let principal = ctx.principal().map(|p| p.to_string()).ok_or_else(|| PlexusError::Unauthenticated("this identity names no principal".into()))?;
        Ok(Caller { principal })
    }
}

impl Caller {
    /// Its principal (`key:…`, `idp:…`) — what a fetch is recorded and
    /// charged as.
    pub fn principal(&self) -> &str {
        &self.principal
    }
}
