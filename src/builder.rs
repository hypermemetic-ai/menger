use menger_tickets_api::access::Access;
// V2-4: `tickets`, and the addresses, the grants its reach reads and the
// browser's sign-in — `members`, `orgs`, `session` (V2-25: `epics` went; the
// board is the organization's whole set).
use menger_tickets_api::{MembersActivation, OrgsActivation, ProviderOffer, SessionActivation, TicketsActivation};
use plexus_ui_service::{Compiler, Signers, UiActivation, UiStore, DEFAULT_CHANNEL, UI_DB_FILE};
// V2-24: the media service — a service of v2's own, its file its own, the
// ticket's permit asked of `tickets` through the seam.
use menger_media::{MediaActivation, MediaStore, MEDIA_DB_FILE};
// V2-29: the docs service — documents on their own API and file.
use menger_docs::{DocsActivation, DocsStore, DOCS_DB_FILE};
// V2-31: the quota service — allowances per principal and source, its
// ledger its own file; every write reserved against the cap through the seam.
use menger_quota::{DiskUsage, Ledger, Plan, QuotaActivation, QuotaSeam, Statvfs, QUOTA_DB_FILE};
use menger_tickets_api::store::{OpenPurge, SeedShape, SeedSpec, Store, StoreError, System, PURGE_EVERY};
use plexus_auth_core::grants::GrantsGeneration;
use plexus_core::plexus::{Activation, AuthBackend, AuthSetup, ChildRouter, DynamicHub, Provider as AuthProvider, ProviderKind as AuthKind};

use crate::auth_backend::MengerAuth;

/// AUTH-7: the methods any signed-in caller reaches under login, whatever
/// it holds or its ceiling — beside the protocol's own (`auth.whoami`,
/// `auth.logout`, `auth.requires`): who it is, its own grants (the app asks
/// before anything, and shows "no access yet" from it), and IDENTITY-42's
/// sessions (an agent's key holds nothing until an admin grants it).
/// V2-31: and their own usage (`quota.usage`) — a person with no grant yet
/// still sees what they store.
pub const SIGNED_IN_GRANTS: [&str; 6] = ["session.whoami", "session.login", "session.logout", "session.providers", "members.mine", "quota.usage"];
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// The default database location when no override is given: a per-user
/// data directory's `local.db` (`~/.local/share/menger/local.db` on Linux).
/// DIST-14: never `tickets.db`, prod's file on a machine that runs it
/// (prod, test and the image name theirs).
pub fn default_db_path() -> PathBuf {
    crate::first_run::default_data_dir().join(crate::first_run::DB_FILE)
}

/// UI-3: the ui service's file when `--ui-db-path` names none — `ui.db`
/// beside the tickets file `db_path`, never the same file.
pub fn default_ui_db_path(db_path: &Path) -> PathBuf {
    let beside = |name: &str| db_path.parent().map_or_else(|| PathBuf::from(name), |d| d.join(name));
    match beside(UI_DB_FILE) {
        // A tickets file that is itself named ui.db: the ui file goes beside it under another name.
        same if same == db_path => beside("ui-service.db"),
        ui => ui,
    }
}

/// UI-15: the ui service's channel when `--ui-channel` names none — the
/// environment's own name for `test` and `prod` (`--env`), else the default
/// channel (the plain `live` row), so a dev server and every test from
/// before serve the pointer they always did.
pub fn default_ui_channel(env: Option<&str>) -> &'static str {
    match env {
        Some("test") => "test",
        Some("prod") => "prod",
        _ => DEFAULT_CHANNEL,
    }
}

/// V2-24: the media service's file when `--media-db-path` names none —
/// `media.db` beside the tickets file `db_path`, never the same file (and
/// never the ui file).
pub fn default_media_db_path(db_path: &Path) -> PathBuf {
    let beside = |name: &str| db_path.parent().map_or_else(|| PathBuf::from(name), |d| d.join(name));
    match beside(MEDIA_DB_FILE) {
        // A tickets file that is itself named media.db: the media file goes beside it under another name.
        same if same == db_path => beside("media-service.db"),
        media => media,
    }
}

/// V2-29: the docs service's file when `--docs-db-path` names none —
/// `docs.db` beside the tickets file `db_path`, never the same file (nor
/// the ui or media file).
pub fn default_docs_db_path(db_path: &Path) -> PathBuf {
    let beside = |name: &str| db_path.parent().map_or_else(|| PathBuf::from(name), |d| d.join(name));
    match beside(DOCS_DB_FILE) {
        // A tickets file that is itself named docs.db: the docs file goes beside it under another name.
        same if same == db_path => beside("docs-service.db"),
        docs => docs,
    }
}

/// V2-31: the quota service's file when `--quota-db-path` names none —
/// `quota.db` beside the tickets file `db_path`, never the same file.
pub fn default_quota_db_path(db_path: &Path) -> PathBuf {
    let beside = |name: &str| db_path.parent().map_or_else(|| PathBuf::from(name), |d| d.join(name));
    match beside(QUOTA_DB_FILE) {
        same if same == db_path => beside("quota-service.db"),
        quota => quota,
    }
}

/// FRAME-21: the login's session journal's suffix — `<stem>.sessions.jsonl`
/// beside the tickets file (`tickets.db` → `tickets.sessions.jsonl`), so a
/// restart (a deploy) signs no one out.
pub const SESSIONS_FILE: &str = "sessions.jsonl";

/// FRAME-21: where the sessions of the service on `db_path` are kept: beside
/// it, named for it — two servers whose databases share a directory
/// (`/tmp/menger-<gate>.db`) never share, or lock each other out of, one
/// table.
pub fn sessions_file(db_path: &Path) -> PathBuf {
    let stem = db_path.file_stem().map_or_else(|| "menger".into(), |s| s.to_string_lossy().into_owned());
    let name = format!("{stem}.{SESSIONS_FILE}");
    db_path.parent().map_or_else(|| PathBuf::from(&name), |d| d.join(&name))
}

/// UI-3: what the `ui` activation and the shell's `/app/*` are built over —
/// the store on its own file and the signers a push may be signed by.
#[derive(Clone)]
pub struct UiSetup {
    pub store: Arc<UiStore>,
    pub signers: Signers,
    /// UI-7: the compiler module (`--ui-compiler`) every push compiles its
    /// sources in; none: the trees are stored as given (`compiled: false`).
    pub compiler: Option<Compiler>,
    /// STUDIO-3: the key `ui.author` signs an editor's version with
    /// (`--ui-author-key`); none: every author refused `no_signer`.
    pub author: Option<plexus_ui_service::AuthorKey>,
}

impl UiSetup {
    /// The store at `path` (`--ui-db-path`), pushes signed by `signers`
    /// (`--ui-signer`) — on the default channel.
    pub fn open(path: &Path, signers: Signers) -> anyhow::Result<Self> {
        Self::open_on(path, signers, DEFAULT_CHANNEL)
    }

    /// UI-15: [`Self::open`] on `channel` (`--ui-channel`): the live pointer
    /// this environment serves and moves in a file another environment may
    /// share.
    pub fn open_on(path: &Path, signers: Signers, channel: &str) -> anyhow::Result<Self> {
        // The data directory may not exist yet on a first start (the
        // tickets store makes it only when it opens, after this): make it.
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
        }
        let store = UiStore::open_channel(path, channel).map_err(|e| anyhow::anyhow!("{} (--ui-channel {channel}): {e}", path.display()))?;
        tracing::info!("ui:        {} (channel {channel})", path.display());
        Ok(Self { store: Arc::new(store), signers, compiler: None, author: None })
    }

    /// UI-15: the channel the store is on.
    pub fn channel(&self) -> &str {
        self.store.channel()
    }

    /// UI-7: pushes compile in `compiler` (`--ui-compiler`).
    pub fn compiling(mut self, compiler: Option<Compiler>) -> Self {
        self.compiler = compiler;
        self
    }

    /// STUDIO-3: `ui.author` signs with `key` (`--ui-author-key`).
    pub fn authoring(mut self, key: Option<plexus_ui_service::AuthorKey>) -> Self {
        self.author = key;
        self
    }

    /// The store beside the tickets file ([`default_ui_db_path`]), with no
    /// signer — every push refused `no_signer`.
    pub fn beside(db_path: &Path) -> anyhow::Result<Self> {
        Self::open(&default_ui_db_path(db_path), Signers::none())
    }
}

/// V2-24: what the `media` activation and the shell's `/media/<hash>` are
/// built over — the store on its own file, and (EDITOR-10) where its bytes
/// live, the cap on one upload if the server names one, the links a test
/// lets through, and the allowance every byte is asked of.
#[derive(Clone)]
pub struct MediaSetup {
    pub store: Arc<MediaStore>,
    pub max_upload: Option<u64>,
    pub remote: menger_media::Remote,
    /// The allowance every byte is asked of — V2-31: the quota service's
    /// meter ([`MediaSetup::metered`]; the builder meters it always).
    pub allowance: Arc<dyn menger_media::Allowance>,
}

/// EDITOR-10: the blob store `--media-store` names — `disk` (at
/// `--media-blob-dir`, or `media/blobs` beside the media file); `s3` is
/// refused until it is built.
pub fn media_blobs(kind: &str, dir: Option<&Path>, media_db: &Path) -> anyhow::Result<Arc<dyn menger_media::BlobStore>> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "disk" => {
            let root = dir.map(Path::to_path_buf).unwrap_or_else(|| menger_media::default_blob_dir(media_db));
            let disk = menger_media::DiskStore::open(&root).map_err(|e| anyhow::anyhow!("--media-blob-dir {}: {e}", root.display()))?;
            Ok(Arc::new(disk))
        }
        "s3" => anyhow::bail!("--media-store s3: not built yet (V2-32); use --media-store disk"),
        other => anyhow::bail!("--media-store {other}: disk or s3"),
    }
}

impl MediaSetup {
    /// The store at `path` (`--media-db-path`), its bytes on the disk beside
    /// it; the directory made if missing.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        Self::open_with(path, media_blobs("disk", None, path)?)
    }

    /// The store at `path`, its bytes in `blobs`.
    pub fn open_with(path: &Path, blobs: Arc<dyn menger_media::BlobStore>) -> anyhow::Result<Self> {
        let described = blobs.describe();
        let store = MediaStore::open_with(path, blobs).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        tracing::info!("media:     {} ({} files; bytes on {described})", path.display(), store.count());
        Ok(Self { store: Arc::new(store), max_upload: None, remote: menger_media::Remote::new(), allowance: Arc::new(menger_media::Unlimited) })
    }

    /// The store beside the tickets file ([`default_media_db_path`]).
    pub fn beside(db_path: &Path) -> anyhow::Result<Self> {
        Self::open(&default_media_db_path(db_path))
    }

    /// The cap on one upload (`--media-max-bytes`); none: the allowance alone.
    pub fn max_upload(mut self, max: Option<u64>) -> Self {
        self.max_upload = max;
        self
    }

    /// V2-31: every byte asked of the quota service, through `meter` (the
    /// slot the builder attaches the seam to).
    pub fn metered(mut self, meter: &menger_quota::MeterSlot) -> Self {
        self.allowance = Arc::new(crate::quota_allowance::Metered(meter.clone()));
        self
    }

    /// For tests alone: `ip:port` pairs links may be read from although not public.
    pub fn fetch_allow(mut self, allow: Vec<std::net::SocketAddr>) -> Self {
        if !allow.is_empty() {
            tracing::warn!("media:     reading links from {allow:?} although not public (--media-fetch-allow: a test's)");
            self.remote = menger_media::Remote::allowing(allow);
        }
        self
    }
}

/// V2-29: what the `docs` activation is built over — the store on its own
/// file.
#[derive(Clone)]
pub struct DocsSetup {
    pub store: Arc<DocsStore>,
}

impl DocsSetup {
    /// The store at `path` (`--docs-db-path`); the directory made if missing.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let store = DocsStore::open(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        tracing::info!("docs:      {} ({} documents)", path.display(), store.count());
        Ok(Self { store: Arc::new(store) })
    }

    /// The store beside the tickets file ([`default_docs_db_path`]).
    pub fn beside(db_path: &Path) -> anyhow::Result<Self> {
        Self::open(&default_docs_db_path(db_path))
    }
}

/// V2-31: what the `quota` activation is built over — the ledger on its own
/// file, from a plan when the file is new, and the disk the box guard reads.
#[derive(Clone)]
pub struct QuotaSetup {
    pub ledger: Arc<Ledger>,
    pub disk: Arc<dyn DiskUsage>,
}

impl QuotaSetup {
    /// The ledger at `path` (`--quota-db-path`), a fresh file taking `plan`
    /// (`--quota-plan`; the built-in free tier); the box guard reads the disk
    /// that holds it.
    pub fn open(path: &Path, plan: &Plan) -> anyhow::Result<Self> {
        let ledger = Ledger::open(path, plan).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        tracing::info!("quota:     {} (plan {}; {} reservation(s) open)", path.display(), ledger.settings().plan, ledger.open_holds());
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        Ok(Self { ledger: Arc::new(ledger), disk: Arc::new(Statvfs::new(dir)) })
    }

    /// The ledger beside the tickets file ([`default_quota_db_path`]), the built-in plan.
    pub fn beside(db_path: &Path) -> anyhow::Result<Self> {
        Self::open(&default_quota_db_path(db_path), &Plan::built_in())
    }

    /// The box guard reads `disk` instead (a test's fake).
    pub fn guarding(mut self, disk: Arc<dyn DiskUsage>) -> Self {
        self.disk = disk;
        self
    }
}

/// ACT-5: the purge of deleted tickets' bodies past their 30 days — at start,
/// then every [`PURGE_EVERY`], for the life of the process. A failure is
/// logged and tried again at the next turn: a body kept a day longer harms
/// nothing.
/// ACCESS-11: as the store's [`System`] — the server's own upkeep.
fn purge_daily(store: Arc<Store>, system: Arc<System>) {
    tokio::spawn(async move {
        loop {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
            match store.purge(&system, now).await {
                Ok(ids) if !ids.is_empty() => tracing::info!("ACT-5: purged the bodies of {} deleted ticket(s): {}", ids.len(), ids.join(", ")),
                Ok(_) => {}
                Err(e) => tracing::warn!("ACT-5: the purge of deleted tickets' bodies failed ({e}) — tried again in a day"),
            }
            tokio::time::sleep(PURGE_EVERY).await;
        }
    });
}

/// Builds the `menger` hub with every activation registered — `tickets` and
/// (COMMENT-2) `comments`, over one store, and (IDENTITY-14) `session` —
/// behind `access` (ACCESS-24: always the gate; there is no open mode).
///
/// Opens the database at `db_path` (the system of record — a fresh
/// location is an empty store), loads every ticket into the service-wide
/// cache, and reports the effective location, how many tickets loaded,
/// and how long the load took. The cache keeps the only database handle
/// in the process. SCALE-14: the store does — the writer and its read-only
/// connections — and the cache holds each ticket's row, not its body.
pub async fn build_rpc(db_path: &Path, access: Access) -> anyhow::Result<Arc<DynamicHub>> {
    build_rpc_with(db_path, None, access).await
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
    access: Access,
) -> anyhow::Result<Arc<DynamicHub>> {
    build_rpc_seeded(db_path, seed, SeedShape::Flat, access).await
}

/// FILTER-13: [`build_rpc_with`], the seed in a given shape ([`SeedShape`]).
pub async fn build_rpc_seeded(
    db_path: &Path,
    seed: Option<SeedSpec>,
    shape: SeedShape,
    access: Access,
) -> anyhow::Result<Arc<DynamicHub>> {
    build_rpc_offering(db_path, seed, shape, Vec::new(), access).await
}

/// IDENTITY-15: [`build_rpc_seeded`], with `session.providers` offering
/// these providers (main's `--auth-provider`s). IDENTITY-19: and every
/// `tickets` and `comments` method behind `access`.
///
/// AUTH-7: the hub carries the protocol's gate and `auth` activation
/// ([`build_rpc_login`]) over a backend that mints sessions on a signed-in
/// connection but has no login page to send a person to (`auth.login
/// {browser}` is refused) — what the tests that wire their own transport
/// need.
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

/// AUTH-7: the hub, the protocol's gate in front of every call and its
/// `auth` activation, over `backend` and the store's roles, members and
/// scopes ([`Access::book`]): `with_auth`, the declared roles seeded,
/// menger's own signed-in methods ([`SIGNED_IN_GRANTS`]), and
/// `resource_metadata` (RFC 9728's URL) named to an anonymous `_info`.
/// ACCESS-24: always — there is no hub without the gate — and before it is
/// built, what open mode planted is purged ([`purge_planted`]).
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
    let ui = UiSetup::beside(db_path)?;
    build_rpc_ui(db_path, seed, shape, offers, access, backend, resource_metadata, body_max, ui).await
}

/// UI-3: [`build_rpc_login`], the `ui` activation over `ui` (main's
/// `--ui-db-path` and `--ui-signer`) registered beside `tickets`.
#[allow(clippy::too_many_arguments)]
pub async fn build_rpc_ui(
    db_path: &Path,
    seed: Option<SeedSpec>,
    shape: SeedShape,
    offers: Vec<ProviderOffer>,
    access: Access,
    backend: Arc<dyn AuthBackend>,
    resource_metadata: Option<String>,
    body_max: usize,
    ui: UiSetup,
) -> anyhow::Result<Arc<DynamicHub>> {
    build_rpc_ui_over(db_path, seed, shape, offers, access, backend, resource_metadata, body_max, ui, |store, access| {
        TicketsActivation::with_access(store, access)
    })
    .await
}

/// UI-7: [`build_rpc_ui`] with the activation registered as `tickets` made
/// by `tickets` over the store and the rule — main's is
/// [`TicketsActivation`]; a test's may be a reduced service, to show the
/// ui service reads the running one through the seam, never a copy. The
/// ui activation's seam is attached to the hub built here. V2-24: the
/// media service on its file beside the tickets file
/// ([`MediaSetup::beside`]); main names it ([`build_rpc_services`]).
#[allow(clippy::too_many_arguments)]
pub async fn build_rpc_ui_over<T, F>(
    db_path: &Path,
    seed: Option<SeedSpec>,
    shape: SeedShape,
    offers: Vec<ProviderOffer>,
    access: Access,
    backend: Arc<dyn AuthBackend>,
    resource_metadata: Option<String>,
    body_max: usize,
    ui: UiSetup,
    tickets: F,
) -> anyhow::Result<Arc<DynamicHub>>
where
    T: Activation + ChildRouter + Clone + 'static,
    F: FnOnce(Arc<Store>, Access) -> T,
{
    let media = MediaSetup::beside(db_path)?;
    let docs = DocsSetup::beside(db_path)?;
    let quota = QuotaSetup::beside(db_path)?;
    build_rpc_services(db_path, seed, shape, offers, access, backend, resource_metadata, body_max, ui, media, docs, quota, tickets).await
}

/// V2-24: [`build_rpc_ui_over`] with the media service over `media` (main's
/// `--media-db-path`) — V2-31: and the quota service over `quota` (main's
/// `--quota-db-path`, `--quota-plan`) — registered beside `tickets` and `ui` — every service
/// of v2's, in one process (V2-1): the media service's seam is attached to
/// the hub built here, so a put asks `tickets.may` through it as the caller.
/// V2-29: and the docs service over `docs` (`--docs-db-path`), its seam
/// attached the same way (the gate on a document's organization,
/// `orgs.list` as the caller).
#[allow(clippy::too_many_arguments)]
pub async fn build_rpc_services<T, F>(
    db_path: &Path,
    seed: Option<SeedSpec>,
    shape: SeedShape,
    offers: Vec<ProviderOffer>,
    access: Access,
    backend: Arc<dyn AuthBackend>,
    resource_metadata: Option<String>,
    body_max: usize,
    ui: UiSetup,
    media: MediaSetup,
    docs: DocsSetup,
    quota: QuotaSetup,
    tickets: F,
) -> anyhow::Result<Arc<DynamicHub>>
where
    T: Activation + ChildRouter + Clone + 'static,
    F: FnOnce(Arc<Store>, Access) -> T,
{
    let path = db_path.to_path_buf();
    let started = Instant::now();
    // MENGER-15: and its cap on a body (`--max-body-bytes`).
    let store = tokio::task::spawn_blocking(move || Store::open(&path)).await??.with_body_max(body_max);
    // ACCESS-10: the store's System — the builder reads as it, then hands
    // it to the access layer.
    let system = Arc::new(store.system().expect("a store just opened holds its System"));
    tracing::info!("database: {}", db_path.display());
    tracing::info!(
        "loaded {} tickets from the database in {:.1} ms",
        store.count(&*system),
        started.elapsed().as_secs_f64() * 1000.0
    );

    let store = match seed {
        None => store,
        Some(spec) => {
            let seeding = Arc::clone(&system);
            let (store, report) = tokio::task::spawn_blocking(move || {
                let outcome = store.seed_synthetic_shaped(&seeding, spec, shape);
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
                        store.count(&*system)
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
    purge_daily(Arc::clone(&store), Arc::clone(&system));
    // ACCESS-24: grants planted while open, gone before anything is served.
    purge_planted(&store, &system, &access.admins()).await?;
    // IDENTITY-21: the members that decide, from the same store.
    access.attach_with(Arc::clone(&store), system);
    // UI-7: the activation registered as `tickets` is the caller's to make
    // (main's: TicketsActivation).
    let tickets = tickets(Arc::clone(&store), access.clone());
    // V2-4: `tickets` — its children with it — and the addresses, the
    // grants its reach reads and the browser's sign-in, until the identity
    // service takes them. UI-3: `ui` beside them — the UI's versions, over
    // its own file.
    // UI-7: and its pushes compiled in `--ui-compiler`'s module.
    // FRAME-5: the framework's, its caller the gate's (no access layer of
    // menger's in it).
    let ui = UiActivation::new(ui.store, ui.signers).compiling(ui.compiler).authoring(ui.author);
    // FRAME-5: the gate's grants generation moves with the store's count, so
    // a watch held by the gate (`ui.watch`) hears `members.*` too.
    let generation = grants_generation(&store);
    let seam = ui.seam().clone();
    // V2-24: `media` beside them — the images a body shows, over its own
    // file; whose a ticket is, it asks `tickets` through its seam.
    // V2-31: every byte the media service takes, fetches or serves is asked of the quota service.
    let media = media.metered(access.meter());
    let media = MediaActivation::new(media.store).with_max_upload(media.max_upload).with_remote(media.remote).with_allowance(media.allowance);
    let media_seam = media.seam().clone();
    // V2-29: `docs` beside them — documents, over their own file; who may is
    // the gate's word on a document's organization, asked through its seam.
    let docs = DocsActivation::new(docs.store);
    let docs_seam = docs.seam().clone();
    // V2-31: `quota` beside them — every principal's usage per source, over
    // its own file; what it knows of teams and stores it asks through its
    // seam, and the tickets service reserves through the meter it is given.
    let quota = QuotaActivation::new(quota.ledger, quota.disk);
    let (quota_seam, meter) = (quota.seam().clone(), QuotaSeam::default());
    menger_quota::upkeep(quota.clone());
    let mut hub = DynamicHub::new("menger")
        .register(tickets)
        .register(ui)
        .register(media)
        .register(docs)
        .register(quota)
        // ORG-4: `orgs` beside `members` — the organizations, and their making.
        .register(OrgsActivation::with_access(Arc::clone(&store), access.clone()))
        .register(MembersActivation::new(store, access.clone()))
        // IDENTITY-37: a signed-out connection is `anonymous` to whoami too.
        .register(SessionActivation::offering(offers, access.clone()));
    // AUTH-7: the gate reads the same book the methods ask; the roles the
    // activations declare are what a role holds until an admin edits it.
    let book = access.book();
    hub = hub
        .with_auth(AuthSetup::new(backend, book.clone(), book.clone(), book.clone()).with_generation(generation))
        .with_signed_in_grants(SIGNED_IN_GRANTS);
    if let Some(url) = resource_metadata {
        hub = hub.with_resource_metadata(url);
    }
    // MACRO-20: `declared_roles` holds the children's too, each grant
    // under its path (`tickets.commits.link`).
    book.seed(hub.declared_roles());
    let hub = Arc::new(hub);
    // UI-7, FRAME-5: the ui service asks the services a manifest names
    // (`tickets`) through this service, as the server's own caller (its full
    // view, whoever pushes).
    seam.attach(&hub, access.system_caller());
    // V2-24: the media service asks `tickets.may` through this service, as
    // the caller it serves (its permit is the tickets service's word).
    media_seam.attach(&hub);
    // V2-29: the docs service asks the gate and `orgs.list` through it.
    docs_seam.attach(&hub);
    // V2-31: the quota service asks `members` and the stores through this
    // service as the server's own caller; the tickets service's writes
    // reserve through the meter, asking `quota.*` the same way.
    quota_seam.attach(&hub, access.system_caller());
    meter.attach(&hub, access.system_caller());
    access.meter().attach(Arc::new(meter));
    Ok(hub)
}

/// FRAME-5: menger's grants count handed to the gate — a
/// [`GrantsGeneration`] for the call gate (`AuthSetup::with_generation`)
/// that moves every time the store's count does (`Store::grants_moved`: a
/// grant, a revoke, a team or an organization changed, a role's grants
/// edited, through `members.*` or `auth.*` alike). A watch the gate holds —
/// the framework's `ui.watch` — is asked again on every move. The gate's own
/// moves (`auth.logout`) count in it too. The forwarder holds the channel,
/// never the store, and ends with it.
fn grants_generation(store: &Arc<Store>) -> GrantsGeneration {
    let generation = GrantsGeneration::new();
    let mut moved = store.grants_moved();
    moved.mark_unchanged();
    let forward = generation.clone();
    tokio::spawn(async move {
        while moved.changed().await.is_ok() {
            forward.bump();
        }
    });
    generation
}

/// ACCESS-24 (ACCESS-4 C2): the grants open mode planted — every stored
/// grant and team membership signed `--admin` that none of `admins` (this
/// start's `--admin`) accounts for — removed, once per database and durably
/// ([`Store::purge_open_grants`]), each one logged. A purge that fails
/// stops the start: a planted grant is never served.
async fn purge_planted(store: &Store, system: &System, admins: &[String]) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    match store.purge_open_grants(system, admins, now).await {
        Ok(OpenPurge::Done { at }) => tracing::debug!("ACCESS-24: grants planted while open were purged at {at} (unix)"),
        Ok(OpenPurge::Ran { removed, kept }) => {
            for r in &removed {
                tracing::warn!("ACCESS-24: removed, planted while open: {r}");
            }
            for k in &kept {
                tracing::info!("ACCESS-24: kept, an --admin accounts for it: {k}");
            }
            tracing::info!("ACCESS-24: grants planted while open purged — {} removed, {} kept (recorded in open_purge)", removed.len(), kept.len());
        }
        Err(e) => anyhow::bail!("ACCESS-24: the purge of grants planted while open failed ({e}); refusing to serve them"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// V2-24: the media file is beside the tickets file, never the same file.
    #[test]
    fn the_media_db_is_beside_the_tickets_file_never_it() {
        for tickets in ["/var/lib/menger/tickets.db", "/tmp/x/local.db", "/tmp/x/media.db.d/t.db"] {
            let tickets = Path::new(tickets);
            let media = default_media_db_path(tickets);
            assert_eq!(media, tickets.parent().unwrap().join("media.db"), "{}", tickets.display());
            assert_ne!(media, tickets);
            assert_ne!(media, default_ui_db_path(tickets), "nor the ui file");
        }
        let tickets = Path::new("/tmp/x/media.db");
        let media = default_media_db_path(tickets);
        assert_ne!(media, tickets);
        assert_eq!(media.parent(), tickets.parent(), "still beside it");
    }

    /// V2-29: the docs file is beside the tickets file, never it, nor the ui or media file.
    #[test]
    fn the_docs_db_is_beside_the_tickets_file_never_it() {
        for tickets in ["/var/lib/menger/tickets.db", "/tmp/x/local.db", "/tmp/x/docs.db.d/t.db"] {
            let tickets = Path::new(tickets);
            let docs = default_docs_db_path(tickets);
            assert_eq!(docs, tickets.parent().unwrap().join("docs.db"), "{}", tickets.display());
            assert!(docs != tickets && docs != default_ui_db_path(tickets) && docs != default_media_db_path(tickets));
        }
        let tickets = Path::new("/tmp/x/docs.db");
        let docs = default_docs_db_path(tickets);
        assert_ne!(docs, tickets);
        assert_eq!(docs.parent(), tickets.parent(), "still beside it");
    }

    /// V2-31: the quota file is beside the tickets file, never the same file.
    #[test]
    fn the_quota_db_is_beside_the_tickets_file_never_it() {
        for tickets in ["/var/lib/menger/tickets.db", "/tmp/x/local.db"] {
            let tickets = Path::new(tickets);
            let quota = default_quota_db_path(tickets);
            assert_eq!(quota, tickets.parent().unwrap().join("quota.db"));
            assert_ne!(quota, default_media_db_path(tickets));
            assert_ne!(quota, default_ui_db_path(tickets));
        }
        let tickets = Path::new("/tmp/x/quota.db");
        assert_ne!(default_quota_db_path(tickets), tickets);
    }

    /// UI-3: the ui file is beside the tickets file, never the same file.
    #[test]
    fn the_ui_db_is_beside_the_tickets_file_never_it() {
        for tickets in ["/var/lib/menger/tickets.db", "/tmp/x/local.db", "/tmp/x/ui.db.d/t.db"] {
            let tickets = Path::new(tickets);
            let ui = default_ui_db_path(tickets);
            assert_eq!(ui, tickets.parent().unwrap().join("ui.db"), "{}", tickets.display());
            assert_ne!(ui, tickets);
        }
        // A tickets file that is itself named ui.db: still never the same file.
        let tickets = Path::new("/tmp/x/ui.db");
        let ui = default_ui_db_path(tickets);
        assert_ne!(ui, tickets);
        assert_eq!(ui.parent(), tickets.parent(), "still beside it");
    }

    /// UI-15: the channel follows the environment's name for test and prod,
    /// and is the default — the plain `live` row — anywhere else.
    #[test]
    fn the_ui_channel_is_the_environments_or_the_default() {
        assert_eq!(default_ui_channel(Some("test")), "test");
        assert_eq!(default_ui_channel(Some("prod")), "prod");
        assert_eq!(default_ui_channel(Some("staging")), "default");
        assert_eq!(default_ui_channel(None), "default");
        assert_eq!(plexus_ui_service::live_pointer(default_ui_channel(None)), "live");
        assert_eq!(plexus_ui_service::live_pointer(default_ui_channel(Some("prod"))), "live:prod");
    }
}
