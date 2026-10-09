use menger_tickets_api::{Comments, Tickets};
use menger_tickets_api::store::{SeedShape, SeedSpec, Store, StoreError};
use plexus_core::plexus::DynamicHub;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// The default database location when no override is given: a per-user
/// data directory (`~/.local/share/menger/tickets.db` on Linux).
pub fn default_db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("menger")
        .join("tickets.db")
}

/// Builds the `menger` hub with every activation registered — `tickets` and
/// (COMMENT-2) `comments`, over one store.
///
/// Opens the database at `db_path` (the system of record — a fresh
/// location is an empty store), loads every ticket into the service-wide
/// cache, and reports the effective location, how many tickets loaded,
/// and how long the load took. The cache keeps the only database handle
/// in the process. SCALE-14: the store does — the writer and its read-only
/// connections — and the cache holds each ticket's row, not its body.
pub async fn build_rpc(db_path: &Path) -> anyhow::Result<Arc<DynamicHub>> {
    build_rpc_with(db_path, None).await
}

/// [`build_rpc`], optionally seeding synthetic tickets first (PERF-2).
/// Seeding happens after the load and before anything serves: the
/// store's bulk path writes through to the database in a few
/// transactions, then the cache holds them like any other ticket. A
/// database that already holds tickets refuses the seed — with the
/// count, the path, and the flag that overrides — unless
/// `SeedSpec::into_existing` is set.
pub async fn build_rpc_with(
    db_path: &Path,
    seed: Option<SeedSpec>,
) -> anyhow::Result<Arc<DynamicHub>> {
    build_rpc_seeded(db_path, seed, SeedShape::Flat).await
}

/// FILTER-13: [`build_rpc_with`], the seed in a given shape ([`SeedShape`]).
pub async fn build_rpc_seeded(
    db_path: &Path,
    seed: Option<SeedSpec>,
    shape: SeedShape,
) -> anyhow::Result<Arc<DynamicHub>> {
    let path = db_path.to_path_buf();
    let started = Instant::now();
    let store = tokio::task::spawn_blocking(move || Store::open(&path)).await??;
    tracing::info!("database: {}", db_path.display());
    tracing::info!(
        "loaded {} tickets from the database in {:.1} ms",
        store.count(),
        started.elapsed().as_secs_f64() * 1000.0
    );

    let store = match seed {
        None => store,
        Some(spec) => {
            let (store, report) = tokio::task::spawn_blocking(move || {
                let outcome = store.seed_synthetic_shaped(spec, shape);
                (store, outcome)
            })
            .await?;
            match report {
                Ok(r) => {
                    tracing::info!(
                        "seeded {} synthetic tickets in {:.1} ms (seed value {}, shape {:?}, {} epics, \
                         {} blocked_by edges; generate {:.1} ms, persist {:.1} ms); \
                         store now holds {} tickets",
                        r.count,
                        r.elapsed.as_secs_f64() * 1000.0,
                        spec.seed,
                        shape,
                        r.epics,
                        r.edges,
                        r.generate.as_secs_f64() * 1000.0,
                        r.persist.as_secs_f64() * 1000.0,
                        store.count()
                    );
                }
                Err(StoreError::SeedRefused { existing }) => anyhow::bail!(
                    "refusing to seed {} synthetic tickets: {} already holds {existing} ticket(s). \
                     Point --db-path at a fresh file, or pass --seed-into-existing \
                     (MENGER_SEED_INTO_EXISTING=1) to add them anyway. Nothing was changed.",
                    spec.count,
                    db_path.display()
                ),
                Err(e) => anyhow::bail!("seeding failed: {e}"),
            }
            store
        }
    };

    // COMMENT-2: `comments` beside `tickets`, over the same store — one
    // cache, one database handle.
    let store = Arc::new(store);
    Ok(Arc::new(
        DynamicHub::new("menger")
            .register(Tickets::new(Arc::clone(&store)))
            .register(Comments::new(store)),
    ))
}
