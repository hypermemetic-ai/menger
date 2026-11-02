//! **The caller, as the gate admitted it** (FRAME-5's pattern: the ui
//! service's and the media service's `caller.rs`). plexus-core's gate
//! decides every call before a method runs — the method's grant held
//! somewhere, within the session's ceiling — so this service's resolver
//! ([`Callers::verify`]) takes the identity the upgrade proved and the
//! reach the gate answered (`current_reach()`), and refuses only what the
//! gate cannot have admitted. Where a document is — its organization — is
//! asked of the gate again by the methods ([`crate::seam::Seam::holds`]).

use crate::events::WatchEnd;
use plexus_core::plexus::{current_reach, AuthContext, PlexusError, Reach, Refusal};
use plexus_macros::PlexusRequest;

/// The per-call context every method of the service reads: the identity the
/// connection's upgrade proved, when one did.
#[derive(PlexusRequest)]
pub struct DocsRequest {
    /// The identity the connection's upgrade proved (none: anonymous).
    #[from_auth_context]
    pub auth: Option<AuthContext>,
}

/// The resolver every method verifies its caller by.
#[derive(Clone, Copy, Debug, Default)]
pub struct Callers;

/// A caller the gate admitted: its principal, its identity, and the reach
/// the gate answered for the call.
#[derive(Clone, Debug)]
pub struct Caller {
    principal: String,
    ctx: AuthContext,
    reach: Reach,
}

impl Callers {
    /// The caller of the call being dispatched: signed in, naming a
    /// principal. Refused typed (`unauthenticated`) otherwise.
    pub async fn verify(&self, ctx: &AuthContext) -> Result<Caller, PlexusError> {
        if !ctx.is_authenticated() {
            return Err(PlexusError::Unauthenticated("sign in first".into()));
        }
        let principal = ctx.principal().map(|p| p.to_string()).ok_or_else(|| PlexusError::Unauthenticated("this identity names no principal".into()))?;
        Ok(Caller { principal, ctx: ctx.clone(), reach: current_reach() })
    }
}

impl Caller {
    /// Its principal (`key:…`, `idp:…`).
    pub fn principal(&self) -> &str {
        &self.principal
    }

    /// Its name — the agent it signed in as (`builder`), else the name its
    /// identity carries, else its principal: what a write says `by`.
    pub fn name(&self) -> String {
        ["agent", "name"].iter().find_map(|k| self.ctx.get_metadata_string(k).filter(|n| !n.is_empty())).unwrap_or_else(|| self.principal.clone())
    }

    /// Whether it signed in as an agent's key (its proof names one).
    pub fn agent(&self) -> bool {
        self.ctx.get_metadata_string("agent").is_some_and(|a| !a.is_empty())
    }

    /// Who it is, as a presence says it.
    pub fn who(&self) -> crate::presence::Who {
        crate::presence::Who { principal: self.principal.clone(), name: self.name(), agent: self.agent() }
    }

    /// The identity the gate admitted — what the gate is asked of again.
    pub fn context(&self) -> &AuthContext {
        &self.ctx
    }

    /// The reach the gate answered for the call.
    pub fn reach(&self) -> &Reach {
        &self.reach
    }

    /// When its sign-in expires (unix seconds), if it says.
    pub fn expires_at(&self) -> Option<i64> {
        self.ctx.metadata.get("expires_at").and_then(|v| v.as_i64())
    }

    /// The caller as it stands now: its sign-in not expired, its grant still
    /// held somewhere. `Err`: why a watch ends.
    pub fn again(&self) -> Result<(), (WatchEnd, String)> {
        if self.expires_at().is_some_and(|at| at <= crate::store::now()) {
            return Err((WatchEnd::Expired, "your access changed: your sign-in has expired".into()));
        }
        match self.reach.recheck() {
            Ok(_) => Ok(()),
            Err(r) => {
                let reason = match &r {
                    Refusal::Ended { reason } if reason.contains("expired") => WatchEnd::Expired,
                    Refusal::Ended { .. } => WatchEnd::SignedOut,
                    Refusal::MissingGrant { .. } => WatchEnd::Revoked,
                    _ => WatchEnd::Refused,
                };
                Err((reason, format!("your access changed: {r}")))
            }
        }
    }
}
