//! UI-7: **the seam** — the ui service as a client of the service it is
//! registered beside. The process's [`DynamicHub`] is built after the ui
//! activation is, so the activation holds an empty [`Seam`] that the
//! builder fills once the hub exists ([`Seam::attach`]); a push then calls
//! `tickets` through [`HubTransport::as_`] as the pusher — the same route,
//! gate and schema a socket reaches, never a copy of the schema.

use plexus_core::plexus::{AuthContext, DynamicHub, HubTransport};
use std::sync::{Arc, OnceLock, Weak};

/// Where the running service is, once the builder says. Weak: the hub owns
/// the activation that holds this.
#[derive(Clone, Default)]
pub struct Seam {
    hub: Arc<OnceLock<Weak<DynamicHub>>>,
}

impl std::fmt::Debug for Seam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seam").field("attached", &self.hub.get().is_some()).finish()
    }
}

impl Seam {
    /// The hub this service is registered in; the first attach holds.
    pub fn attach(&self, hub: &Arc<DynamicHub>) {
        let _ = self.hub.set(Arc::downgrade(hub));
    }

    /// A transport into the running hub as `caller`; `None` before the
    /// builder attached it (or after the hub is gone).
    pub fn as_(&self, caller: AuthContext) -> Option<HubTransport> {
        self.hub.get().and_then(Weak::upgrade).map(|hub| HubTransport::as_(hub, caller))
    }
}
