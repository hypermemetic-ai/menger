use clap::Parser;
use menger::store::{SeedShape, SeedSpec, Store, Verdict};
use menger::login::{self, LoginButton, Provider};
use menger::sessions::{Gate, Sessions};
use menger::web_login::{Login, LoginConfig};
use menger_tickets_api::access::Access;
use menger::{build_rpc_offering, default_db_path, shell};
use plexus_core::plexus::DynamicHub;
use plexus_transport::{RestHttpConfig, RouteFn, TransportServer};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "menger")]
#[command(about = "A plan is a program whose leaves are prose — ticketing on Plexus RPC")]
struct Args {
    /// WebSocket port (4440-4459 is the synapse scan range)
    #[arg(short, long, default_value = "4455")]
    port: u16,

    /// Web shell port
    #[arg(long, default_value = "4456")]
    shell_port: u16,

    /// REST HTTP port (POST /rest/{namespace}/{method}; streaming methods
    /// respond as SSE)
    #[arg(long, default_value = "4457")]
    rest_port: u16,

    /// Disable the REST HTTP listener. IDENTITY-23: also `MENGER_NO_REST`
    /// (1/true/yes), so an environment that requires login can say it in
    /// its env file.
    #[arg(
        long,
        env = "MENGER_NO_REST",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::FalseyValueParser::new()
    )]
    no_rest: bool,

    /// Directory holding the built Leptos app (frontend/dist), served at
    /// /app/ on the shell port. Missing output degrades to a "frontend
    /// not built" page, never an error.
    #[arg(long, env = "MENGER_APP_DIST", value_name = "DIR")]
    app_dist: Option<PathBuf>,

    /// MFE-23: the environment's name (`prod`, `test`, …); the deploy env
    /// files set `MENGER_ENV`. `test` titles the browser tab `menger.test`,
    /// anything else (or none) `menger`.
    #[arg(long, env = "MENGER_ENV", value_name = "NAME")]
    env: Option<String>,

    /// Run in stdio mode (MCP-compatible); implies no web shell
    #[arg(long)]
    stdio: bool,

    /// Disable the web shell
    #[arg(long)]
    no_shell: bool,

    /// Database file (the system of record). Defaults to a per-user data
    /// directory; a fresh location starts empty.
    #[arg(long, env = "MENGER_DB_PATH", value_name = "PATH")]
    db_path: Option<PathBuf>,

    /// PERF-2: seed N synthetic tickets into the database before serving
    /// (bulk — a few transactions, no per-row fsync; deterministic per
    /// --seed-value). Refuses a database that already holds tickets
    /// unless --seed-into-existing.
    #[arg(long, env = "MENGER_SEED_SYNTHETIC", value_name = "N")]
    seed_synthetic: Option<usize>,

    /// RNG seed for --seed-synthetic: the same value reproduces the same
    /// tickets.
    #[arg(long, env = "MENGER_SEED_VALUE", default_value = "1", value_name = "U64")]
    seed_value: u64,

    /// FILTER-13: where --seed-synthetic hangs its tickets: `flat` (every
    /// ticket under its shortcode's root, PERF-2's) or `nested` (FILTER-3
    /// §6.5's tree: executions under the root, builds under executions,
    /// docs under builds).
    #[arg(long, env = "MENGER_SEED_SHAPE", default_value = "flat", value_name = "SHAPE")]
    seed_shape: SeedShape,

    /// Let --seed-synthetic add to a database that already holds tickets
    /// (ids continue after the existing ones). As an env var, 1/true/yes
    /// enable it.
    #[arg(
        long,
        env = "MENGER_SEED_INTO_EXISTING",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::FalseyValueParser::new()
    )]
    seed_into_existing: bool,

    /// SCALE-28: check the database (--db-path, or the default) instead of
    /// serving it: rebuild its derived state — shortcode runs, dangling
    /// forward references, roll-ups, children and child_count, and
    /// (SCALE-32) each ticket's unsatisfied blockers, the ready set and
    /// every shortcode's status counts, and (FILTER-4) the cell table with
    /// every shortcode's bitmap and count of each cell, and (FILTER-13) the
    /// kept tree's tallies and holders — by a scan
    /// of its tickets, compare with what it stores, print each difference
    /// or "consistent", and exit 0 when consistent, 1 on any difference
    /// (or none stored), 2 when it cannot be read. Read-only: changes
    /// nothing, migrates nothing, serves nothing.
    #[arg(long)]
    verify_derived: bool,

    /// IDENTITY-14: admit callers through this identity provider
    /// (repeatable): `idp=<issuer>,<audience>`,
    /// `auth0=<domain>,<audience>[,<roles-claim>]`,
    /// `oidc:<name>=<issuer>,<audience>[,<roles-claim>]`,
    /// `workos=<client-id>[,<api-base>]` (IDENTITY-30) or
    /// `nostr=<ws-url>[,<ws-url>…]`. A caller's credential rides the
    /// `access_token` or `nostr` cookie, or `Authorization: Bearer`, and its
    /// provider's adapter makes it a Plexus identity at the WebSocket
    /// upgrade. In the env var, specs are separated by `;`. None: no login,
    /// as before.
    #[arg(long = "auth-provider", env = "MENGER_AUTH_PROVIDERS", value_name = "SPEC", value_delimiter = ';')]
    auth_provider: Vec<Provider>,

    /// IDENTITY-14: refuse a WebSocket upgrade that proves no one. Needs an
    /// --auth-provider, and --no-rest: the REST bridge cannot carry an
    /// identity yet. As an env var, 1/true/yes enable it.
    #[arg(
        long,
        env = "MENGER_REQUIRE_LOGIN",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::FalseyValueParser::new()
    )]
    require_login: bool,

    /// IDENTITY-19: a button on the login page (repeatable),
    /// `<Label>=<provider>[:<connection>]`, e.g. `Google=auth0:google-oauth2`,
    /// `Apple=auth0:apple`, `SSO=auth0:samlp` (asks for a work email); for
    /// WorkOS `Google=workos:GoogleOAuth`, `Apple=workos:AppleOAuth`,
    /// `Email=workos:authkit`, `SSO=workos:authkit`. The provider needs
    /// `client=<id>` in its `--auth-provider` (WorkOS's is its spec). In the
    /// env var, separated by `;`.
    #[arg(long = "login-button", env = "MENGER_LOGIN_BUTTONS", value_name = "SPEC", value_delimiter = ';')]
    login_button: Vec<LoginButton>,

    /// IDENTITY-30: an email field on the login page, `<provider>[:<connection>]`,
    /// e.g. `workos:authkit`: the address goes to that IdP as `login_hint`,
    /// so an SSO domain goes straight to its company's IdP and anyone else
    /// finishes on the IdP's page with the address filled in. Beside it, the
    /// password form shows only at `/login?password` (automation's way in).
    #[arg(long, env = "MENGER_LOGIN_EMAIL", value_name = "SPEC", value_parser = parse_email)]
    login_email: Option<LoginButton>,

    /// IDENTITY-19: the provider the login page's password form goes to;
    /// default the first OIDC `--auth-provider` that takes passwords (not
    /// WorkOS). `none`: no password form (IDENTITY-30).
    #[arg(long, env = "MENGER_LOGIN_PASSWORD", value_name = "PROVIDER")]
    login_password: Option<String>,

    /// IDENTITY-19: a "Forgot password?" link on the login page.
    #[arg(long, env = "MENGER_LOGIN_FORGOT", value_name = "URL")]
    login_forgot: Option<String>,

    /// IDENTITY-19: where people reach this shell (the login page's base for
    /// the code flow's redirect URI and the nostr URL); default
    /// http://127.0.0.1:<shell-port>.
    #[arg(long, env = "MENGER_PUBLIC_URL", value_name = "URL")]
    public_url: Option<String>,

    /// IDENTITY-29: another origin whose pages may open the socket
    /// (repeatable); the shell's own are always allowed.
    #[arg(long = "allowed-origin", env = "MENGER_ALLOWED_ORIGINS", value_name = "ORIGIN", value_delimiter = ';')]
    allowed_origin: Vec<String>,

    /// IDENTITY-21: a principal who is admin on every shortcode from the
    /// operator (repeatable; `idp:<uuid>`, `oidc:<provider>/<sub>`,
    /// `nostr:<hex>`) — how the first admin exists. In the env var,
    /// separated by `;`.
    #[arg(long = "admin", env = "MENGER_ADMINS", value_name = "PRINCIPAL", value_delimiter = ';')]
    admin: Vec<String>,

    /// IDENTITY-35: the dev sign-in, test only (only with `--env test`, IDENTITY-38):
    /// the login page's checkbox sends this browser's sign-in to this
    /// `--auth-provider` (plexus-idp's name, e.g. `plexus-idp`), where a
    /// seeded user is one click and any username can be signed in as.
    #[arg(long, env = "MENGER_DEV_LOGIN", value_name = "PROVIDER", requires_all = ["dev_password", "dev_idp_ws"])]
    dev_login: Option<String>,

    /// IDENTITY-35: the password every dev user has at plexus-idp.
    #[arg(long, env = "MENGER_DEV_PASSWORD", value_name = "PASSWORD", hide_env_values = true)]
    dev_password: Option<String>,

    /// IDENTITY-35: plexus-idp's Plexus socket, where the dev sign-in
    /// registers accounts, e.g. `ws://127.0.0.1:4480`.
    #[arg(long, env = "MENGER_DEV_IDP_WS", value_name = "URL")]
    dev_idp_ws: Option<String>,

    /// IDENTITY-35: a seeded dev user (repeatable),
    /// `name=role@SCOPE[,role@SCOPE…]`, e.g. `viewer=viewer@*`,
    /// `mixed=editor@OUGHT,viewer@PLEXUS`, `nobody=`. In the env var,
    /// separated by `;`.
    #[arg(long = "dev-user", env = "MENGER_DEV_USERS", value_name = "SPEC", value_delimiter = ';')]
    dev_user: Vec<menger::dev_login::DevUser>,

    /// IDENTITY-19: how long a sign-in lasts.
    #[arg(long, env = "MENGER_SESSION_HOURS", default_value = "12", value_name = "HOURS")]
    session_hours: i64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let filter = if args.stdio {
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into())
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,menger=debug".into())
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    let db_path = args.db_path.unwrap_or_else(default_db_path);
    if args.verify_derived {
        std::process::exit(verify_derived(db_path).await);
    }
    let seed = args.seed_synthetic.map(|count| SeedSpec {
        count,
        seed: args.seed_value,
        into_existing: args.seed_into_existing,
    });
    // IDENTITY-14: the login boundary is settled before anything opens, so
    // a bad --auth-provider stops startup rather than serving without it.
    if args.require_login {
        if args.auth_provider.is_empty() {
            anyhow::bail!("--require-login needs at least one --auth-provider to log in with");
        }
        if !args.no_rest && !args.stdio {
            anyhow::bail!(
                "--require-login closes the WebSocket, but the REST bridge cannot carry an \
                 identity yet and would stay open: add --no-rest"
            );
        }
    }
    let boundary = login::boundary(&args.auth_provider)?;
    let offers = args.auth_provider.iter().map(Provider::offer).collect();
    // IDENTITY-19: sessions, the rule every method asks, and the page.
    let login_on = !args.auth_provider.is_empty();
    let public_url = args.public_url.clone().unwrap_or_else(|| format!("http://127.0.0.1:{}", args.shell_port));
    // IDENTITY-38: the session cookie named for this shell, so prod and test
    // on one host keep apart.
    let sessions = Arc::new(Sessions::named(args.session_hours * 3600, &menger::sessions::cookie_name_for(&public_url)));
    let access = if login_on {
        Access::with_login(sessions.clone(), args.require_login, args.admin.clone())
    } else {
        Access::open()
    };
    let shell_origins = vec![
        format!("http://127.0.0.1:{}", args.shell_port),
        format!("http://localhost:{}", args.shell_port),
        menger::web_login::origin_of(&public_url),
    ];
    // IDENTITY-35: the dev sign-in is the test environment's, never prod's.
    // IDENTITY-38: allowed only where named for it, `--env test` — not
    // refused only where named prod, so an unnamed server is not open.
    let dev = match &args.dev_login {
        None => None,
        Some(_) if !dev_login_allowed(args.env.as_deref()) => {
            anyhow::bail!("--dev-login needs --env test: it lets anyone be anyone, and is the test environment's alone")
        }
        Some(provider) => {
            tracing::warn!("dev sign-in on through {provider}: anyone who reaches the login page can be anyone");
            Some(menger::dev_login::DevLogin {
                provider: provider.clone(),
                password: args.dev_password.clone().expect("clap requires it"),
                idp_ws: args.dev_idp_ws.clone().expect("clap requires it"),
                users: args.dev_user.clone(),
            })
        }
    };
    let web_login = if login_on && !args.no_shell && !args.stdio {
        Some(Arc::new(Login::new(
            LoginConfig {
                providers: args.auth_provider.clone(),
                buttons: args.login_button.clone(),
                email: args.login_email.clone(),
                password: args.login_password.clone(),
                forgot: args.login_forgot.clone(),
                public_url: public_url.clone(),
                also: shell_origins.clone(),
                require: args.require_login,
                title: shell::tab_title(args.env.as_deref()),
                dev,
                access: Some(access.clone()),
            },
            sessions.clone(),
        )?))
    } else {
        None
    };
    let hub = build_rpc_offering(&db_path, seed, args.seed_shape, offers, access).await?;

    let activations = hub.list_activations_info();
    tracing::info!("Activations ({}):", activations.len());
    for a in &activations {
        tracing::info!("  {} v{} - {}", a.namespace, a.version, a.description);
        for method in &a.methods {
            tracing::info!("    - {}.{}", a.namespace, method);
        }
    }

    let rpc_converter = |arc: Arc<DynamicHub>| {
        DynamicHub::arc_into_rpc_module(arc)
            .map_err(|e| anyhow::anyhow!("rpc module build failed: {e}"))
    };

    let mut builder = TransportServer::builder(hub.clone(), rpc_converter);
    if args.stdio {
        builder = builder.with_stdio();
        tracing::info!("Starting stdio transport");
        if boundary.is_some() {
            tracing::warn!("--auth-provider is ignored over stdio: there is no upgrade to carry a credential");
        }
    } else {
        builder = builder.with_websocket(args.port);
        tracing::info!("WebSocket: ws://127.0.0.1:{}", args.port);
        // IDENTITY-29: only menger's own pages may open the socket from a
        // browser, login or not; synapse sends no Origin and is unaffected.
        let mut origins = shell_origins.clone();
        origins.extend(args.allowed_origin.iter().cloned());
        builder = builder.with_allowed_origins(origins);
        if let Some(carriers) = boundary {
            let gate = Gate::new(sessions.clone(), Some(carriers));
            builder = builder.with_session_validator(Arc::new(gate));
            if args.require_login {
                builder = builder.reject_upgrade_on_auth_failure();
            }
            let names: Vec<&str> = args.auth_provider.iter().map(|p| p.name()).collect();
            tracing::info!(
                "Login:     {} — anonymous callers {}",
                names.join(", "),
                if args.require_login { "refused" } else { "admitted" }
            );
        }
        if !args.no_rest {
            let route_hub = hub.clone();
            let route_fn: RouteFn = Arc::new(move |method, params| {
                let hub = route_hub.clone();
                Box::pin(async move { hub.route(&method, params, None).await })
            });
            builder = builder
                .with_rest_http_config(
                    RestHttpConfig::new(args.rest_port)
                        .with_server_name("menger".into())
                        .with_server_version(env!("CARGO_PKG_VERSION").into()),
                )
                .with_flat_schemas(hub.list_plugin_schemas()) // registers /rest/tickets/*
                .with_route_fn(route_fn); // dispatches via hub.route()
            tracing::info!("REST:      http://127.0.0.1:{}/rest", args.rest_port);
        }
        if !args.no_shell {
            let cfg = shell::ShellConfig {
                port: args.shell_port,
                ws_port: args.port,
                app_dist: args.app_dist.unwrap_or_else(shell::default_app_dist),
                env: args.env.clone(),
                login: web_login,
            };
            tokio::spawn(async move {
                if let Err(e) = shell::serve(cfg).await {
                    tracing::error!("shell server exited: {e}");
                }
            });
        }
    }

    builder.build().await?.serve().await
}

/// SCALE-28: `--verify-derived` — the verdict on stdout, one difference a
/// line, and the exit code it means. Run off the async runtime: it reads
/// and scans the whole file.
async fn verify_derived(db_path: PathBuf) -> i32 {
    let path = db_path.clone();
    let verdict = tokio::task::spawn_blocking(move || Store::verify_derived(&path))
        .await
        .map_err(anyhow::Error::from)
        .and_then(|v| v);
    match verdict {
        Ok(Verdict::Consistent {
            tickets,
            runs,
            refs,
            groups,
            parents,
            blocked,
            ready,
            cells,
            bitmaps,
            tallies,
            holders,
            indexed,
        }) => {
            // SEARCH-2: and the search index, when the file has one.
            let search = match indexed {
                Some(n) => format!(", and the search index's {n} tickets' titles and text"),
                None => String::new(),
            };
            println!(
                "consistent: {} — {tickets} tickets; {runs} shortcode runs, {refs} dangling forward \
                 references, {groups} roll-up groups, the children and child_count of {parents} \
                 parents, the unsatisfied blockers of {blocked} tickets, a ready set of {ready}, \
                 every shortcode's status counts, {cells} cells' {bitmaps} bitmaps and counts, \
                 and the kept tree's {tallies} tally entries and {holders} holder containers{search} \
                 are what a scan of the tickets gives",
                db_path.display()
            );
            if indexed.is_none() {
                println!("{}: no search index yet (SEARCH-2) — the next open builds it", db_path.display());
            }
            0
        }
        Ok(Verdict::Differs(differences)) => {
            for d in &differences {
                println!("{d}");
            }
            println!("{}: {} difference(s) between the stored derived state and a scan", db_path.display(), differences.len());
            1
        }
        Ok(Verdict::NotStored(why)) => {
            println!("{}: {why}", db_path.display());
            1
        }
        Err(e) => {
            eprintln!("cannot verify {}: {e:#}", db_path.display());
            2
        }
    }
}

/// IDENTITY-30: `--login-email <provider>[:<connection>]`, a button with no
/// label of its own.
fn parse_email(spec: &str) -> Result<LoginButton, String> {
    format!("email={spec}").parse()
}

/// IDENTITY-38: the dev sign-in runs only on the environment named test.
fn dev_login_allowed(env: Option<&str>) -> bool {
    env == Some("test")
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_dev_sign_in_is_test_s_alone() {
        assert!(super::dev_login_allowed(Some("test")));
        for env in [None, Some("prod"), Some("Test"), Some("dev"), Some("")] {
            assert!(!super::dev_login_allowed(env), "{env:?}");
        }
    }
}
