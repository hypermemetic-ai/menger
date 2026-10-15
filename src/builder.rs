use menger_tickets_api::access::Access;
use menger_tickets_api::{Comments, Deployments, Members, Notes, Orgs, ProviderOffer, Session, Themes, Tickets};
use menger_tickets_api::store::{SeedShape, SeedSpec, Store, StoreError, PURGE_EVERY};
use plexus_core::plexus::{AuthBackend, AuthSetup, DynamicHub, Provider as AuthProvider, ProviderKind as AuthKind};

use crate::auth_backend::MengerAuth;

/// AUTH-7: the methods any signed-in caller reaches under login, whatever
/// it holds or its ceiling — beside the protocol's own (`auth.whoami`,
/// `auth.logout`, `auth.requires`): who it is, its own grants (the app asks
/// before anything, and shows "no access yet" from it), and IDENTITY-42's
/// sessions (an agent's key holds nothing until an admin grants it).
pub const SIGNED_IN_GRANTS: [&str; 5] = ["session.whoami", "session.login", "session.logout", "session.providers", "members.mine"];
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

/// ACT-5: the purge of deleted tickets' bodies past their 30 days — at start,
/// then every [`PURGE_EVERY`], for the life of the process. A failure is
/// logged and tried again at the next turn: a body kept a day longer harms
/// nothing.
fn purge_daily(store: Arc<Store>) {
    tokio::spawn(async move {
        loop {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
            match store.purge(now).await {
                Ok(ids) if !ids.is_empty() => tracing::info!("ACT-5: purged the bodies of {} deleted ticket(s): {}", ids.len(), ids.join(", ")),
                Ok(_) => {}
                Err(e) => tracing::warn!("ACT-5: the purge of deleted tickets' bodies failed ({e}) — tried again in a day"),
            }
            tokio::time::sleep(PURGE_EVERY).await;
        }
    });
}

/// Builds the `menger` hub with every activation registered — `tickets` and
/// (COMMENT-2) `comments`, over one store, and (IDENTITY-14) `session`.
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
    build_rpc_offering(db_path, seed, shape, Vec::new(), Access::open()).await
}

/// IDENTITY-15: [`build_rpc_seeded`], with `session.providers` offering
/// these providers (main's `--auth-provider`s). IDENTITY-19: and every
/// `tickets` and `comments` method behind `access`.
///
/// AUTH-7: with login on (`Access::with_login`), the hub carries the
/// protocol's gate and `auth` activation ([`build_rpc_login`]) over a
/// backend that mints sessions on a signed-in connection but has no login
/// page to send a person to (`auth.login {browser}` is refused) — what the
/// tests that wire their own transport need.
pub async fn build_rpc_offering(
    db_path: &Path,
    seed: Option<SeedSpec>,
    shape: SeedShape,
    offers: Vec<ProviderOffer>,
    access: Access,
) -> anyhow::Result<Arc<DynamicHub>> {
    let providers: Vec<AuthProvider> =
        offers.iter().map(|o| AuthProvider { name: o.name.clone(), kind: AuthKind::Browser, label: None }).collect();
    let backend = Arc::new(MengerAuth::new(access.clone(), providers, None, crate::login_access::BRANDING.app));
    build_rpc_login(db_path, seed, shape, offers, access, backend, None, menger_tickets_api::store::BODY_MAX_DEFAULT).await
}

/// AUTH-7: the hub, and — when `access` has login on — the protocol's gate
/// in front of every call and its `auth` activation, over `backend` and the
/// store's roles, members and scopes ([`Access::book`]): `with_auth`, the
/// declared roles seeded, menger's own signed-in methods
/// ([`SIGNED_IN_GRANTS`]), and `resource_metadata` (RFC 9728's URL) named to
/// an anonymous `_info`. Without login: the hub as it always was, no gate.
#[allow(clippy::too_many_arguments)]
pub async fn build_rpc_login(
    db_path: &Path,
    seed: Option<SeedSpec>,
    shape: SeedShape,
    offers: Vec<ProviderOffer>,
    access: Access,
    backend: Arc<dyn AuthBackend>,
    resource_metadata: Option<String>,
    body_max: usize,
) -> anyhow::Result<Arc<DynamicHub>> {
    let path = db_path.to_path_buf();
    let started = Instant::now();
    // MENGER-15: and its cap on a body (`--max-body-bytes`).
    let store = tokio::task::spawn_blocking(move || Store::open(&path)).await??.with_body_max(body_max);
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
    // cache, one database handle. IDENTITY-14: `session`, holding nothing,
    // reports who each connection is.
    let store = Arc::new(store);
    // ACT-5: a deleted ticket's body is kept 30 days for its undo — purged
    // past that now, and every day after, inside the server.
    purge_daily(Arc::clone(&store));
    // IDENTITY-21: the members that decide, from the same store.
    access.attach(Arc::clone(&store));
    let mut hub = DynamicHub::new("menger")
        .register(Tickets::with_access(Arc::clone(&store), access.clone()))
        .register(Comments::with_access(Arc::clone(&store), access.clone()))
        // NOTE-2: `notes` beside them, over the same store.
        .register(Notes::with_access(Arc::clone(&store), access.clone()))
        // THEME-8: `themes` beside them — the themes and what each level adopts.
        .register(Themes::with_access(Arc::clone(&store), access.clone()))
        // DEPLOY-11: `deployments` beside them — every deploy, where tickets run.
        .register(Deployments::with_access(Arc::clone(&store), access.clone()))
        // ORG-4: `orgs` beside `members` — the organizations, and their making.
        .register(Orgs::with_access(Arc::clone(&store), access.clone()))
        .register(Members::new(store, access.clone()))
        // IDENTITY-37: a signed-out connection is `anonymous` to whoami too.
        .register(Session::offering(offers).with_access(access.clone()));
    if access.login() {
        // AUTH-7: the gate reads the same book the methods ask; the roles
        // the activations declare are what a role holds until an admin
        // edits it.
        let book = access.book();
        hub = hub
            .with_auth(AuthSetup::new(backend, book.clone(), book.clone(), book.clone()))
            .with_signed_in_grants(SIGNED_IN_GRANTS);
        if let Some(url) = resource_metadata {
            hub = hub.with_resource_metadata(url);
        }
        book.seed(hub.declared_roles());
    }
    Ok(Arc::new(hub))
}
