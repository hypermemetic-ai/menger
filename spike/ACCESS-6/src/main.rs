//! The spike's server: a hub `spike` holding `tickets` (and its child
//! `inputs`) on throwaway ports — ws 4670, REST 4672 (4671 unused: no shell).

use access6_spike::{SpikeSessions, Tickets};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into())).init();
    let port: u16 = std::env::var("SPIKE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4670);
    let hub = Arc::new(DynamicHub::new("spike").register(Tickets::new()));
    TransportServer::builder(hub, |h| DynamicHub::arc_into_rpc_module(h).map_err(Into::into))
        .with_websocket(port)
        .with_session_validator(Arc::new(SpikeSessions))
        .anonymous_view()
        .credential_cookies(["access_token"])
        .without_registry()
        .build()
        .await?
        .serve()
        .await
}
