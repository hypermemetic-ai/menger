//! ACCESS-22: `members teams` — the teams a grant may name (a team's grants
//! held by each of its members).
//!
//! **A shell** (ACCESS-19, as ACCESS-13's children are): the child's struct,
//! here so ACCESS-22 fills a file of its own. It is not an activation yet:
//! plexus-macros refuses an `impl` with no method (MACRO-18), so it is not
//! mounted, and nothing moved on the wire.
//!
//! To fill it, in one build:
//!
//! 1. put the frame's attribute on an `impl Teams` here —
//!    `#[plexus_macros::activation(namespace = "teams", auth = "required",
//!    request = MengerRequest, client, typed_params, server_cfg = "feature
//!    = \"server\"", crate_path = "plexus_core", roles(...))]` — its grants
//!    are `teams.<method>`, listed in its own `roles(...)` (none listed:
//!    admin's alone);
//! 2. declare each method as `members_activation.rs` declares its own:
//!    `///` docs on the method and every parameter,
//!    `#[from_auth(self.access.verify)] user: VerifiedUser`, typed
//!    parameters, a body of a few lines through helpers beside it;
//! 3. mount it on `members` — `#[plexus_macros::child] fn teams(&self) ->
//!    Teams` — and `pub use teams::TeamsClient`. **Mind MACRO-20**: under
//!    login, a `#[child]` of an `auth = "required"` activation cannot be
//!    called until the parent's `route_to_child` carries the principal,
//!    session and ceiling — land MACRO-20 first (or prove the route) and
//!    test the child behind login over the wire;
//! 4. rerun `scripts/sdk-codegen.sh`.

#[cfg(feature = "server")]
use crate::access::Access;
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use std::sync::Arc;

/// The teams a grant may name (ACCESS-22).
#[cfg(feature = "server")]
#[derive(Clone)]
#[allow(dead_code)] // a shell: mounted when ACCESS-22 fills it
pub struct Teams {
    pub(super) store: Arc<Store>,
    pub(super) access: Access,
}
