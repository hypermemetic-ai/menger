//! ACCESS-17: `tickets work` — the order of work (WAVE-4, MENGER-31): `ready` (was `ready_set`), `waves`, `blockers`.
//!
//! **A shell** (ACCESS-13): the child's struct, here so ACCESS-17 fills a file
//! of its own. Until it does, these methods stay on `tickets` (`ready_set`, `waves`, `blockers`),
//! so the frame's build moves nothing on the wire. It is not an activation
//! yet: plexus-macros refuses an `impl` with no method ("No
//! #[plexus::method] functions found" — the MACRO build ACCESS-13 filed).
//!
//! To fill it, in one build:
//!
//! 1. put the frame's attribute on an `impl Work` here —
//!    `#[plexus_macros::activation(namespace = "work", auth = "required",
//!    request = MengerRequest, client, typed_params, server_cfg = "feature
//!    = \"server\"", crate_path = "plexus_core", roles(viewer = [...],
//!    editor = [...]))]` — a child's grants are `work.<method>` (its own
//!    namespace, unique across the hub), listed in its own `roles(...)`;
//! 2. move each method here under its new name, as `activation/mod.rs`
//!    declares them: `///` docs on the method and every parameter (no
//!    `description =`, no `params(...)`), `#[from_auth(self.access.verify)]
//!    user: VerifiedUser`, typed parameters, a body of a few lines through
//!    the helpers beside `activation/mod.rs` (`answered`, the `*_stream`s);
//! 3. take them off `tickets` (and its `roles(...)`), mount the child there
//!    — `#[plexus_macros::child] fn work(&self) -> Work` beside `commits`'
//!    place in `activation/mod.rs` — and `pub use work::WorkClient` (the
//!    parent's client then reaches it as `client.work()`, MACRO-16);
//! 4. move its tests from `activation/tests/` and the wire tests to the new
//!    paths, rerun `scripts/sdk-codegen.sh`.

#[cfg(feature = "server")]
use crate::access::Access;
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use std::sync::Arc;

/// The order of work: what can start now, the waves, and what a ticket waits on.
#[cfg(feature = "server")]
#[derive(Clone)]
#[allow(dead_code)] // a shell: mounted when ACCESS-17 fills it
pub struct Work {
    pub(super) store: Arc<Store>,
    pub(super) access: Access,
}
