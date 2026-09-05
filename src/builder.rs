use crate::activations::Tickets;
use crate::store::Store;
use plexus_core::plexus::DynamicHub;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The default database location when no override is given: a per-user
/// data directory (`~/.local/share/menger/tickets.db` on Linux).
pub fn default_db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("menger")
        .join("tickets.db")
}

/// Builds the `menger` hub with every activation registered.
///
/// Opens the database at `db_path` (the system of record — a fresh
/// location is an empty store), loads every ticket into the service-wide
/// cache, and reports the effective location and how many tickets loaded.
/// The cache keeps the only database handle in the process.
pub async fn build_rpc(db_path: &Path) -> anyhow::Result<Arc<DynamicHub>> {
    let path = db_path.to_path_buf();
    let store = tokio::task::spawn_blocking(move || Store::open(&path)).await??;
    tracing::info!("database: {}", db_path.display());
    tracing::info!("loaded {} tickets from the database", store.count());
    Ok(Arc::new(
        DynamicHub::new("menger").register(Tickets::new(Arc::new(store))),
    ))
}
