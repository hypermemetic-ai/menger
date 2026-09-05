use crate::activations::Tickets;
use crate::store::Store;
use plexus_core::plexus::DynamicHub;
use std::sync::Arc;

/// Builds the `menger` hub with every activation registered.
pub async fn build_rpc() -> Arc<DynamicHub> {
    let store = Arc::new(Store::new());
    Arc::new(DynamicHub::new("menger").register(Tickets::new(store)))
}
