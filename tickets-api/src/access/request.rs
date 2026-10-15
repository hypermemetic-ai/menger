//! ACCESS-13: **the frame's request** — what every activation on the frame
//! names as `request = MengerRequest`: the per-call context the dispatch
//! extracts before a method runs. Only optional fields (ACCESS-43): a
//! required one would refuse the call before the resolver
//! ([`super::Access::verify`]) could say why.

use plexus_core::plexus::AuthContext;
use plexus_macros::PlexusRequest;

/// The per-call context: the identity the upgrade proved, when one did.
/// What decides a call is the resolver's [`super::VerifiedUser`], never
/// this; it is declared so every activation reads its context one way.
#[derive(PlexusRequest)]
pub struct MengerRequest {
    /// The identity the connection's upgrade proved (none: anonymous).
    #[from_auth_context]
    pub auth: Option<AuthContext>,
}
