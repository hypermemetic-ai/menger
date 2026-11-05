//! substrate under the hood (TETHER-7): its whole service built inside tether,
//! and called with substrate's own client over LOCAL-6's in-memory
//! transport — no substrate process, no port between them.
//!
//! The service is gated as substrate's binary gates it: every call goes through
//! its gate, as [`TETHER_PRINCIPAL`], tether's own identity there, which
//! holds `editor` on `*` and nothing else (claudecode sessions of its own;
//! `bash`, `sessions_*` and the rest stay admin's, and nobody is). The
//! person or agent who asked is tether's to know ([`crate::sessions`]), not
//! substrate's.

use std::sync::Arc;

use plexus_core::plexus::{
    AuthContext, AuthSetup, DynamicHub, HubTransport, MemoryAuthBackend, MemoryMemberships, MemoryRoles, Resolve,
    ScopeRef,
};
use plexus_substrate::activations::claudecode::ClaudeCodeClient;
use plexus_substrate::{build_plexus_rpc_with, BuildOptions};

/// Who tether is to the substrate inside it.
pub const TETHER_PRINCIPAL: &str = "apikey:menger-tether";

/// Every scope lies in `*` — substrate's roles are global (PLEXUS-6).
struct Global;

impl Resolve for Global {
    fn chain(&self, target: &str) -> Option<Vec<ScopeRef>> {
        Some(vec![ScopeRef::parse(target), ScopeRef::Everywhere])
    }
}

/// The substrate inside tether.
#[derive(Clone)]
pub struct Engine {
    transport: HubTransport,
}

impl Engine {
    /// Build substrate's service in this process, behind its gate. Its
    /// databases go where `plexus_substrate::activations::storage::set_data_dir`
    /// was pointed (the binary's `--data-dir`); `claude_path` is the Claude
    /// Code it runs (a stub in tests).
    pub async fn build(claude_path: Option<String>) -> Self {
        let roles = Arc::new(MemoryRoles::default());
        let members = Arc::new(MemoryMemberships::new().with(TETHER_PRINCIPAL, "editor", "*"));
        // No one signs in to it: the backend only answers the gate's
        // "is this session alive" for a session it never minted.
        let backend = Arc::new(MemoryAuthBackend::new("about:blank"));
        let setup = AuthSetup::new(backend, roles.clone(), members, Arc::new(Global));
        let service = build_plexus_rpc_with(BuildOptions { auth: Some(setup), claude_path }).await;
        roles.seed(&service.declared_roles());
        Self::over(service)
    }

    /// Call an already built substrate service as tether.
    pub fn over(service: Arc<DynamicHub>) -> Self {
        let me = TETHER_PRINCIPAL.parse().expect("tether's principal");
        let caller = AuthContext::for_principal(&me, "tether", vec![], serde_json::json!({ "name": "tether" }));
        Self { transport: HubTransport::as_(service, caller) }
    }

    /// substrate's `claudecode`, through its own client.
    pub fn claudecode(&self) -> ClaudeCodeClient<HubTransport> {
        ClaudeCodeClient::new(self.transport.clone())
    }

    /// The service, for what has no client yet.
    pub fn service(&self) -> &Arc<DynamicHub> {
        self.transport.hub()
    }
}
