use clap::{Parser, Subcommand};
use menger::store::{SeedShape, SeedSpec, Store, Verdict};
use menger::login::{LoginButton, Provider};
use menger::wiring::{LoginOptions, Wiring};
use menger_tickets_api::access::Access;
use menger::first_run::{self, Mode};
use menger::{build_rpc_ui, local, shell, MengerAccess};
use plexus_transport::{ConnectionQuotas, TransportServer};
use std::path::PathBuf;
use std::sync::Arc;

// DIST-14: `menger --help` is short — what menger is and the flags a person
// uses; `--help-all` is everything, grouped by topic. User-facing text names
// no tickets: the ids live in these `//` comments.
const RUN: &str = "Run";
const SIGN_IN: &str = "Sign-in";
const LIMITS: &str = "Limits";
const MAINTENANCE: &str = "Maintenance and testing";

#[derive(Parser, Debug)]
#[command(name = "menger", version)]
#[command(about = "menger — a plan is a program whose leaves are prose. The tickets service.")]
#[command(long_about = None)]
#[command(disable_help_flag = true)]
#[command(after_help = "Start it with `menger`: it prints a one-time sign-in link (`menger --open` opens it for you).\n`menger login` prints another. `menger --help-all` for every option.")]
#[command(after_long_help = "Start it with `menger`: it prints a one-time sign-in link (`menger --open` opens it for you).\n`menger login` prints another.")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Print help (`--help-all` for every option)
    #[arg(short = 'h', long = "help", action = clap::ArgAction::HelpShort, global = true)]
    help: Option<bool>,

    /// Print every option, grouped by topic
    #[arg(long = "help-all", action = clap::ArgAction::HelpLong, global = true)]
    help_all: Option<bool>,

    // DIST-14: print the URL always; open it only when asked.
    /// Open menger in your browser once it is running
    #[arg(long, help_heading = RUN)]
    open: bool,

    // DIST-14: a port not given that is taken falls back to the next free
    // one, said; one given is bound or startup fails (prod names both).
    /// The WebSocket's port (default 4455, or the next free one)
    #[arg(short, long, value_name = "PORT", help_heading = RUN)]
    port: Option<u16>,

    /// The web app's port (default 4456, or the next free one)
    #[arg(long, value_name = "PORT", help_heading = RUN, hide_short_help = true)]
    shell_port: Option<u16>,

    // DIST-14: the data directory — the database (unless --db-path), the
    // local admin key and the one-time links.
    /// Where menger keeps its data (default ~/.local/share/menger)
    #[arg(long, env = "MENGER_DATA_DIR", value_name = "DIR", help_heading = RUN, global = true)]
    data_dir: Option<PathBuf>,

    /// The database file (default <data-dir>/local.db; a new file starts empty)
    #[arg(long, env = "MENGER_DB_PATH", value_name = "PATH", help_heading = RUN)]
    db_path: Option<PathBuf>,

    // UI-3: the ui service's own file, and who may sign a push.
    /// The ui service's file (default: ui.db beside the database file)
    #[arg(long, env = "MENGER_UI_DB_PATH", value_name = "PATH", help_heading = RUN, hide_short_help = true)]
    ui_db_path: Option<PathBuf>,

    /// A public key (64 hex digits, Ed25519) a `ui.push` may be signed by
    ///
    /// Repeatable; in the env var, separated by `;`. With none, every push is
    /// refused (`no_signer`).
    #[arg(long = "ui-signer", env = "MENGER_UI_SIGNERS", value_name = "HEX", value_delimiter = ';', help_heading = RUN, hide_short_help = true)]
    ui_signer: Vec<String>,

    // UI-7: the compiler module a push recompiles every source in.
    /// The ui compiler module (dist/ui-compiler.wasm) every `ui.push` compiles its sources in
    ///
    /// Run in wasmtime with nothing linked, under fuel and a memory limit;
    /// a module that imports anything is refused at start. Without it, a
    /// push stores the bundle's trees as given and says `compiled: false`.
    #[arg(long = "ui-compiler", env = "MENGER_UI_COMPILER", value_name = "PATH", help_heading = RUN, hide_short_help = true)]
    ui_compiler: Option<PathBuf>,

    // STUDIO-3: the key the ui service signs an editor's authored version with.
    /// The ui service's author key: a file holding an Ed25519 seed `ui.author` signs an editor's version with
    ///
    /// Made at first start (64 hex digits, 0600) when the file is missing.
    /// Never a --ui-signer: it signs only what the service compiled itself.
    /// Without it, every `ui.author` is refused (`no_signer`).
    #[arg(long = "ui-author-key", env = "MENGER_UI_AUTHOR_KEY", value_name = "PATH", help_heading = RUN, hide_short_help = true)]
    ui_author_key: Option<PathBuf>,

    // RUN-8: names stay ACCESS-25's: a specific address adds its own literal
    // (and the shell's origin at it may open the socket); any other name is
    // an --allowed-host (and an --allowed-origin).
    /// The address to listen on (default: this machine only)
    ///
    /// `0.0.0.0` / `::` for every address this machine has, or one of them
    /// (IPv6 with or without brackets). Beyond it, people need a way to sign in
    /// (--auth-provider) and an https --public-url.
    #[arg(long, env = "MENGER_BIND", default_value = "127.0.0.1", value_name = "ADDR", value_parser = menger::bind::parse_bind, help_heading = RUN, hide_short_help = true)]
    bind: std::net::IpAddr,

    // ACCESS-24: the REST bridge cannot carry an identity, and menger has no
    // open mode, so it is not served at all; `--no-rest` (and
    // `MENGER_NO_REST`) is accepted and changes nothing, so a launcher or an
    // env file that says it still starts.
    #[arg(
        long,
        env = "MENGER_NO_REST",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::FalseyValueParser::new(),
        hide = true
    )]
    no_rest: bool,

    // DIST-3: without it, the checkout's frontend/dist.
    /// Directory holding the web app's build (frontend/dist). A missing build
    /// shows a "frontend not built" page.
    #[arg(long, env = "MENGER_APP_DIST", value_name = "DIR", help_heading = RUN, hide_short_help = true)]
    app_dist: Option<PathBuf>,

    // MFE-23: the deploy env files set `MENGER_ENV`.
    /// The environment's name (`prod`, `test`, …). `test` titles the browser
    /// tab `menger.test`.
    #[arg(long, env = "MENGER_ENV", value_name = "NAME", help_heading = RUN, hide_short_help = true)]
    env: Option<String>,

    // ACCESS-24: stdio carries no credential — refused with the reason.
    #[arg(long, hide = true)]
    stdio: bool,

    /// Serve no web app, only the WebSocket
    #[arg(long, help_heading = RUN, hide_short_help = true)]
    no_shell: bool,

    // IDENTITY-14 / IDENTITY-42 / IDENTITY-30. ACCESS-24: menger has no open
    // mode; DIST-14: with none on loopback, the local admin signs in by a
    // one-time link. Login is always required (`--require-login` went).
    /// How people sign in, e.g. `workos=<client-id>` or `key` (repeatable)
    ///
    /// The specs: `workos=<client-id>`,
    /// `idp=<issuer>,<audience>`, `auth0=<domain>,<audience>[,<roles-claim>]`,
    /// `oidc:<name>=<issuer>,<audience>[,<roles-claim>]`,
    /// `nostr=<ws-url>[,…]`, or `key` (agents' Ed25519 keys, e.g. synapse's).
    /// With none, on this machine only, menger makes a local admin and prints
    /// a one-time sign-in link. In the env var, separated by `;`.
    #[arg(long = "auth-provider", env = "MENGER_AUTH_PROVIDERS", value_name = "SPEC", value_delimiter = ';', help_heading = SIGN_IN)]
    auth_provider: Vec<Provider>,

    // IDENTITY-21.
    /// A principal who is admin everywhere (repeatable; `key:<hex>`,
    /// `idp:<uuid>`, `oidc:<provider>/<sub>`, `nostr:<hex>`) — how the first
    /// admin exists. In the env var, separated by `;`.
    #[arg(long = "admin", env = "MENGER_ADMINS", value_name = "PRINCIPAL", value_delimiter = ';', help_heading = SIGN_IN, hide_short_help = true)]
    admin: Vec<String>,

    // IDENTITY-19. ACCESS-30: beyond loopback it is required and https.
    /// Where people reach menger, e.g. `https://tickets.example.com`
    ///
    /// Default http://127.0.0.1:<shell-port>. Reached beyond this machine it must be
    /// https, behind a proxy that terminates TLS; the provider's redirect URI
    /// is `<this>/login/callback`.
    #[arg(long, env = "MENGER_PUBLIC_URL", value_name = "URL", help_heading = SIGN_IN)]
    public_url: Option<String>,

    // IDENTITY-19.
    /// A button on the sign-in page (repeatable), `<Label>=<provider>[:<connection>]`,
    /// e.g. `Google=workos:GoogleOAuth`, `Apple=auth0:apple`. In the env var,
    /// separated by `;`.
    #[arg(long = "login-button", env = "MENGER_LOGIN_BUTTONS", value_name = "SPEC", value_delimiter = ';', help_heading = SIGN_IN, hide_short_help = true)]
    login_button: Vec<LoginButton>,

    // IDENTITY-30.
    /// An email field on the sign-in page, `<provider>[:<connection>]`, e.g.
    /// `workos:authkit`: the address goes to that provider. The password form
    /// then shows only at `/login?password`.
    #[arg(long, env = "MENGER_LOGIN_EMAIL", value_name = "SPEC", value_parser = parse_email, help_heading = SIGN_IN, hide_short_help = true)]
    login_email: Option<LoginButton>,

    // IDENTITY-19 / IDENTITY-30.
    /// The provider the sign-in page's password form goes to (default: the
    /// first OIDC provider that takes passwords; `none`: no form).
    #[arg(long, env = "MENGER_LOGIN_PASSWORD", value_name = "PROVIDER", help_heading = SIGN_IN, hide_short_help = true)]
    login_password: Option<String>,

    // IDENTITY-19.
    /// A "Forgot password?" link on the sign-in page.
    #[arg(long, env = "MENGER_LOGIN_FORGOT", value_name = "URL", help_heading = SIGN_IN, hide_short_help = true)]
    login_forgot: Option<String>,

    // AUTH-22. `memory` and `plexus-idp` only with `--env test`.
    /// Menger's own sign-in page over an identity backend:
    /// `workos:<client_…>[,social=google+github+apple][,sso=off][,key=@<file>]`
    /// (the API key from MENGER_WORKOS_API_KEY or the file, never the flag),
    /// `plexus-idp:<issuer>,<audience>` or `memory` (both for testing only).
    #[arg(long, env = "MENGER_IDENTITY", value_name = "SPEC", help_heading = SIGN_IN, hide_short_help = true)]
    identity: Option<plexus_login::backends::IdentitySpec>,

    // IDENTITY-45.
    /// A file holding the WorkOS API key (one line, 0600), or the key in
    /// MENGER_WORKOS_API_KEY (never a flag's value): a `workos=` provider's
    /// social buttons then complete on menger's own page.
    #[arg(long, env = "MENGER_WORKOS_API_KEY_FILE", value_name = "FILE", help_heading = SIGN_IN, hide_short_help = true)]
    workos_api_key_file: Option<std::path::PathBuf>,

    // IDENTITY-29.
    /// Another origin whose pages may open the WebSocket (repeatable); the
    /// app's own always may.
    #[arg(long = "allowed-origin", env = "MENGER_ALLOWED_ORIGINS", value_name = "ORIGIN", value_delimiter = ';', help_heading = SIGN_IN, hide_short_help = true)]
    allowed_origin: Vec<String>,

    // APP-110.
    /// An origin whose pages may frame this server's (repeatable): on the
    /// test environment, the tracker's — `https://app.menger.sh` — so a
    /// ticket body can draw the live app. Default none: nothing frames it.
    /// In the env var, separated by `;`.
    #[arg(long = "frame-ancestor", env = "MENGER_FRAME_ANCESTORS", value_name = "ORIGIN", value_delimiter = ';', help_heading = SIGN_IN, hide_short_help = true)]
    frame_ancestor: Vec<String>,

    // APP-110.
    /// The origin of the test app this server's ticket bodies draw in device
    /// frames (`live` blocks), e.g. `https://app-test.menger.sh`. Default
    /// none: a `live` block says no test app is configured. The test server
    /// must name this one's origin with --frame-ancestor.
    #[arg(long = "live-origin", env = "MENGER_LIVE_ORIGIN", value_name = "ORIGIN", help_heading = SIGN_IN, hide_short_help = true)]
    live_origin: Option<String>,

    // ACCESS-25: DNS rebinding.
    /// Another `Host` menger answers to (repeatable; `host:port`, or `host`).
    /// `127.0.0.1`, `localhost` and --public-url's host always are; any other
    /// is refused. In the env var, separated by `;`.
    #[arg(long = "allowed-host", env = "MENGER_ALLOWED_HOSTS", value_name = "HOST", value_delimiter = ';', help_heading = SIGN_IN, hide_short_help = true)]
    allowed_host: Vec<String>,

    // AUTH-22 / TRANSPORT-7. AUTH-24 #2: with --identity and a --public-url
    // off loopback, one of the two is required.
    /// The proxy in front (CIDR or address, repeatable) whose X-Forwarded-For
    /// is believed for the client's IP, or `none`. In the env var, separated
    /// by `;`.
    #[arg(long = "trusted-proxy", env = "MENGER_TRUSTED_PROXIES", value_name = "CIDR|none", value_delimiter = ';', help_heading = SIGN_IN, hide_short_help = true)]
    trusted_proxy: Vec<String>,

    // AUTH-22.
    /// Another origin a sign-in may send the browser back to (repeatable).
    #[arg(long = "return-origin", env = "MENGER_RETURN_ORIGINS", value_name = "ORIGIN", value_delimiter = ';', help_heading = SIGN_IN, hide_short_help = true)]
    return_origin: Vec<String>,

    // IDENTITY-19.
    /// How long a sign-in lasts.
    #[arg(long, env = "MENGER_SESSION_HOURS", default_value = "12", value_name = "HOURS", help_heading = SIGN_IN, hide_short_help = true)]
    session_hours: i64,

    // ACCESS-29.
    /// A browser's sign-in ends after this long unused (0: only when it
    /// expires).
    #[arg(long, env = "MENGER_SESSION_IDLE_MINUTES", default_value_t = menger::sessions::IDLE_SECS_DEFAULT / 60, value_name = "MINUTES", help_heading = SIGN_IN, hide_short_help = true)]
    session_idle_minutes: i64,

    // IDENTITY-35 / IDENTITY-38: the dev sign-in, only with `--env test`.
    /// Testing only (with `--env test`): the sign-in page can send anyone's
    /// sign-in to this provider (plexus-idp's name), as any user.
    #[arg(long, env = "MENGER_DEV_LOGIN", value_name = "PROVIDER", requires_all = ["dev_password", "dev_idp_ws"], help_heading = MAINTENANCE, hide_short_help = true)]
    dev_login: Option<String>,

    /// Testing only: the password every dev user has at plexus-idp.
    #[arg(long, env = "MENGER_DEV_PASSWORD", value_name = "PASSWORD", hide_env_values = true, help_heading = MAINTENANCE, hide_short_help = true)]
    dev_password: Option<String>,

    /// Testing only: plexus-idp's socket, e.g. `ws://127.0.0.1:4480`.
    #[arg(long, env = "MENGER_DEV_IDP_WS", value_name = "URL", help_heading = MAINTENANCE, hide_short_help = true)]
    dev_idp_ws: Option<String>,

    /// Testing only: a seeded dev user (repeatable), `name=role@SCOPE[,…]`,
    /// e.g. `viewer=viewer@*`. In the env var, separated by `;`.
    #[arg(long = "dev-user", env = "MENGER_DEV_USERS", value_name = "SPEC", value_delimiter = ';', help_heading = MAINTENANCE, hide_short_help = true)]
    dev_user: Vec<menger::dev_login::DevUser>,

    // SCALE-28 (+ SCALE-32, FILTER-4, FILTER-13): read-only — changes nothing,
    // migrates nothing, serves nothing.
    /// Check the database's derived state against a scan of its tickets and
    /// exit: 0 consistent, 1 a difference, 2 unreadable. Changes nothing.
    #[arg(long, help_heading = MAINTENANCE, hide_short_help = true)]
    verify_derived: bool,

    // PERF-2.
    /// Seed N synthetic tickets before serving (into an empty database unless
    /// --seed-into-existing).
    #[arg(long, env = "MENGER_SEED_SYNTHETIC", value_name = "N", help_heading = MAINTENANCE, hide_short_help = true)]
    seed_synthetic: Option<usize>,

    /// The seed of --seed-synthetic: the same value, the same tickets.
    #[arg(long, env = "MENGER_SEED_VALUE", default_value = "1", value_name = "U64", help_heading = MAINTENANCE, hide_short_help = true)]
    seed_value: u64,

    // FILTER-13: flat is PERF-2's; nested FILTER-3 §6.5's tree.
    /// Where --seed-synthetic hangs its tickets: `flat` or `nested`.
    #[arg(long, env = "MENGER_SEED_SHAPE", default_value = "flat", value_name = "SHAPE", help_heading = MAINTENANCE, hide_short_help = true)]
    seed_shape: SeedShape,

    /// Let --seed-synthetic add to a database that already holds tickets.
    #[arg(
        long,
        env = "MENGER_SEED_INTO_EXISTING",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::FalseyValueParser::new(),
        help_heading = MAINTENANCE,
        hide_short_help = true
    )]
    seed_into_existing: bool,

    // MENGER-15: bodies are not cached (SCALE-14); above ~9 MB the
    // WebSocket's own 10 MB request limit refuses first.
    /// The longest ticket body stored, in bytes (default 8 MiB).
    #[arg(long, env = "MENGER_MAX_BODY_BYTES", value_name = "BYTES", default_value_t = menger_tickets_api::store::BODY_MAX_DEFAULT, help_heading = LIMITS, hide_short_help = true)]
    max_body_bytes: usize,

    // ACCESS-29.
    /// Calls one connection may have open at once.
    #[arg(long, env = "MENGER_MAX_CALLS_PER_CONNECTION", default_value_t = menger::calls::CALLS_PER_CONNECTION, value_name = "N", help_heading = LIMITS, hide_short_help = true)]
    max_calls_per_connection: usize,

    /// Close a connection with no call open for this long (0: never).
    #[arg(long, env = "MENGER_IDLE_TIMEOUT_SECS", default_value_t = menger::calls::IDLE_SECS, value_name = "SECS", help_heading = LIMITS, hide_short_help = true)]
    idle_timeout_secs: u64,

    // ACCESS-29 (AUTH-12's quotas).
    /// Connections open in all.
    #[arg(long, env = "MENGER_MAX_CONNECTIONS", default_value_t = ConnectionQuotas::default().max_connections, value_name = "N", help_heading = LIMITS, hide_short_help = true)]
    max_connections: u32,

    /// Connections from one IP address (behind a proxy, the client's when it
    /// is a --trusted-proxy).
    #[arg(long, env = "MENGER_MAX_CONNECTIONS_PER_IP", default_value_t = ConnectionQuotas::default().max_per_ip, value_name = "N", help_heading = LIMITS, hide_short_help = true)]
    max_connections_per_ip: u32,

    /// Connections one signed-in principal holds.
    #[arg(long, env = "MENGER_MAX_CONNECTIONS_PER_PRINCIPAL", default_value_t = ConnectionQuotas::default().max_per_principal, value_name = "N", help_heading = LIMITS, hide_short_help = true)]
    max_connections_per_principal: u32,

    /// Connections that prove no one, in all (held below --max-connections).
    #[arg(long, env = "MENGER_MAX_ANONYMOUS_CONNECTIONS", default_value_t = ConnectionQuotas::default().max_anonymous, value_name = "N", help_heading = LIMITS, hide_short_help = true)]
    max_anonymous_connections: u32,

    /// Anonymous connections from one IP address.
    #[arg(long, env = "MENGER_MAX_ANONYMOUS_CONNECTIONS_PER_IP", default_value_t = ConnectionQuotas::default().max_anonymous_per_ip, value_name = "N", help_heading = LIMITS, hide_short_help = true)]
    max_anonymous_connections_per_ip: u32,
}

#[derive(Subcommand, Debug)]
enum Command {
    // DIST-14: against the server running on --data-dir (its `local.json`).
    /// Print a new one-time sign-in link for the menger running here
    Login {
        // DIST-17.
        /// Print the line an agent runs to sign in with the local key, not a link
        #[arg(long)]
        agent: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    // DIST-14: refusals are said plainly, on stderr, and exit 1 — no
    // `Error:` debug dump.
    if let Err(e) = run(args).await {
        eprintln!("menger: {e:#}");
        std::process::exit(1);
    }
    Ok(())
}

async fn run(args: Args) -> anyhow::Result<()> {
    let data_dir = args.data_dir.clone().unwrap_or_else(first_run::default_data_dir);
    // DIST-14: `menger login` — a fresh link for the server running here.
    if let Some(Command::Login { agent }) = args.command {
        // DIST-17: `--agent` prints the line an agent signs in with.
        if agent {
            println!("{}", first_run::login_agent(&data_dir).map_err(anyhow::Error::msg)?);
            return Ok(());
        }
        let link = first_run::login(&data_dir).map_err(anyhow::Error::msg)?;
        println!("{link}");
        println!("  (one-time, 10 minutes)");
        if args.open && !plexus_login::browser::open(&link) {
            println!("  (no browser to open here: open the link yourself)");
        }
        return Ok(());
    }
    // ACCESS-24: no open mode. DIST-14: with no provider, on loopback, the
    // local admin and its one-time links; beyond it, a refusal naming the
    // flag to add.
    let mode = first_run::mode(&args.auth_provider, args.identity.is_some(), args.stdio, args.bind, args.public_url.as_deref()).map_err(anyhow::Error::msg)?;

    // DIST-14: a first run's terminal shows the banner, not the log; RUST_LOG
    // says otherwise.
    let quiet = mode == Mode::Local && !args.verify_derived;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| if quiet { "error".into() } else { "info,menger=debug".into() });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    // DIST-14: the default database is the data directory's `local.db`, never
    // prod's tickets.db (prod, test and the image name theirs).
    let db_path = args.db_path.clone().unwrap_or_else(|| data_dir.join(first_run::DB_FILE));
    if args.verify_derived {
        std::process::exit(verify_derived(db_path).await);
    }
    // DIST-14: the local admin — its key made on the first start — and the
    // `key` provider, so synapse signs in with that key too.
    let mut providers = args.auth_provider.clone();
    let mut admins = args.admin.clone();
    let once = match mode {
        Mode::Providers => None,
        Mode::Local => {
            let principal = first_run::local_admin(&data_dir)?;
            providers.push("key".parse().map_err(anyhow::Error::msg)?);
            admins.push(principal.clone());
            Some(Arc::new(first_run::OnceLinks::new(&data_dir, principal)))
        }
    };
    // DIST-14: ports not given fall back to the next free one, said.
    let (ws_port, ws_moved) = first_run::pick_port(args.bind, args.port, 4455, &[])?;
    let (shell_port, shell_moved) = first_run::pick_port(args.bind, args.shell_port, 4456, &[ws_port])?;
    let moved: Vec<String> = [(ws_moved, "WebSocket", 4455, ws_port), (shell_moved, "app", 4456, shell_port)]
        .into_iter()
        .filter(|m| m.0)
        .map(|(_, what, from, to)| format!("port {from} was taken: the {what} is on {to}"))
        .collect();

    let seed = args.seed_synthetic.map(|count| SeedSpec {
        count,
        seed: args.seed_value,
        into_existing: args.seed_into_existing,
    });
    // IDENTITY-14: the login boundary is settled before anything opens, so
    // a bad --auth-provider stops startup rather than serving without it.
    // SCAFFOLD-9: plexus-login wires it from the flags — the providers'
    // adapters (IDENTITY-42: a key proof names this socket), the session
    // table (IDENTITY-38: its cookie named for this shell, so prod and test
    // on one host keep apart), the page, the gate and the auth backend.
    let wiring = Wiring::new(LoginOptions {
        providers,
        buttons: args.login_button.clone(),
        email: args.login_email.clone(),
        password: args.login_password.clone(),
        forgot: args.login_forgot.clone(),
        public_url: args.public_url.clone(),
        // RUN-8: the shell's pages at a specific bound address may open the socket.
        allowed_origins: args.allowed_origin.iter().cloned().chain(menger::bind::bind_origin(args.bind, shell_port)).collect(),
        // AUTH-25 N9 / RUN-8: a shell off loopback names its proxy.
        shell_loopback: args.bind.is_loopback(),
        session_ttl_secs: args.session_hours * 3600,
        session_idle_secs: args.session_idle_minutes * 60,
        // FRAME-21: kept beside the database, so a restart signs no one out.
        sessions_file: Some(menger::sessions_file(&db_path)),
        require: true,
        page: !args.no_shell,
        title: shell::tab_title(args.env.as_deref()),
        // APP-92: the sign-in page registers the app's service worker, so
        // the app downloads while the person signs in.
        service_worker: Some(shell::SERVICE_WORKER),
        names: menger::login_names(),
        identity: args.identity.as_ref().map(|s| s.build(menger::login_access::BRANDING.app)).transpose()?,
        trusted_proxies: plexus_login::client_ip::TrustedProxies::parse(&args.trusted_proxy).map_err(anyhow::Error::msg)?,
        return_origins: args.return_origin.clone(),
        // AUTH-24 #8: `memory` and `plexus-idp` sign people in only where
        // the dev sign-in may (IDENTITY-38: `--env test`).
        allow_development_backends: dev_login_allowed(args.env.as_deref()),
        // IDENTITY-45: the WorkOS key, when the box holds it.
        workos_key: match &args.identity {
            Some(_) if args.workos_api_key_file.is_some() => anyhow::bail!("--workos-api-key-file is for the workos= --auth-provider's buttons; --identity names its own key (key=@<file>)"),
            Some(_) => None,
            None => plexus_login::backends::workos::ApiKey::from_env_or_file(menger::login_access::BRANDING.app, args.workos_api_key_file.as_deref()).map_err(anyhow::Error::msg)?,
        },
        // DIST-14: the one-time links, on loopback alone (`mode` refused the rest).
        once: once.clone().map(|o| o as Arc<dyn plexus_login::once::OnceLinks>),
        ..LoginOptions::new(menger::login_access::BRANDING, ws_port, shell_port)
    })?;
    // ACCESS-30: beyond loopback, a sign-in is made over https — the public
    // URL says where, and the cookies, the redirect URI and the app's socket
    // follow it. An http URL on the LAN is served, and said.
    match menger::bind::check_public_url(args.bind, args.public_url.as_deref(), wiring.login_on()) {
        Err(why) => anyhow::bail!(why),
        Ok(Some(warning)) => tracing::warn!("{warning}"),
        Ok(None) => {}
    }
    let https = args.public_url.as_deref().is_some_and(|u| u.starts_with("https://"));
    if https && args.trusted_proxy.is_empty() {
        tracing::warn!(
            "--public-url is https, so a proxy is in front, but no --trusted-proxy names it: \
             every client is counted as the proxy's address (quotas, throttles)"
        );
    }
    if args.no_rest {
        tracing::debug!("--no-rest: there is no REST bridge to turn off (ACCESS-24)");
    }
    // IDENTITY-19: the rule every method asks. IDENTITY-42: session.login
    // mints its tokens in the login's own table.
    let table = menger::session_table(wiring.sessions());
    let access = Access::with_login(table.clone(), admins.clone()).minting(table);
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
    // AUTH-7: the protocol's auth — the gate and the `auth` activation —
    // over menger's sessions, login page and members; RFC 9728's metadata
    // names the OIDC issuers a client can sign in at.
    let wired = wiring.finish(MengerAccess(access.clone()), dev)?;
    let offers = wired.offered().map(menger::offer).collect();
    let metadata = wired.metadata_url();
    // UI-3: the ui service's file (beside the tickets file unless named) and
    // its signers, shared by the `ui` activation and the shell's `/app/*`.
    let ui_db_path = args.ui_db_path.clone().unwrap_or_else(|| menger::default_ui_db_path(&db_path));
    if ui_db_path == db_path {
        anyhow::bail!("--ui-db-path {} is the database file: the ui service keeps a file of its own", ui_db_path.display());
    }
    let ui = menger::UiSetup::open(&ui_db_path, plexus_ui_service::Signers::parse(&args.ui_signer).map_err(anyhow::Error::msg)?)?;
    // UI-7: refused at start when the module imports anything.
    let compiler = match &args.ui_compiler {
        Some(path) => Some(plexus_ui_service::Compiler::open(path).map_err(|e| anyhow::anyhow!("--ui-compiler {}: {e}", path.display()))?),
        None => None,
    };
    let ui = ui.compiling(compiler);
    // STUDIO-3: the author key, made at first start when its file is missing.
    let author = match &args.ui_author_key {
        Some(path) => {
            let key = plexus_ui_service::AuthorKey::open_or_create(path).map_err(|e| anyhow::anyhow!("--ui-author-key {e}"))?;
            tracing::info!("ui author: {} ({})", key.public_hex(), path.display());
            Some(key)
        }
        None => None,
    };
    let ui = ui.authoring(author);
    let ui_store = ui.store.clone();
    let hub = build_rpc_ui(&db_path, seed, args.seed_shape, offers, access, wired.backend.clone(), metadata, args.max_body_bytes, ui).await?;

    local::log_activations(&hub);

    // DIST-3: the wiring shared with `menger::local` (the library's
    // no-login server) lives there; the flags' variations stay here.
    // ACCESS-29: `menger.call` held to the calls a connection may have open,
    // and a connection idle with none closed (menger::calls).
    let limits = menger::calls::CallLimits {
        per_connection: args.max_calls_per_connection,
        idle: (args.idle_timeout_secs > 0).then(|| std::time::Duration::from_secs(args.idle_timeout_secs)),
    };
    // ACCESS-25: each listener answers only its own names — RUN-8: and a
    // specific --bind address by its literal.
    let hosts = |port| {
        let mut extra = args.allowed_host.clone();
        extra.extend(menger::bind::bind_hosts(args.bind, port));
        local::listener_hosts(port, args.public_url.as_deref(), &extra)
    };
    // TRANSPORT-7: one builder for the WebSocket and the REST bridge, both
    // on --bind (RUN-8; the registry is told a specific address), with every
    // check it keeps — ACCESS-25's hosts, AUTH-12's quotas, IDENTITY-29's
    // origins, the login gate, RED-4's check at build.
    // The per-IP and anonymous-per-IP quotas count the client a
    // --trusted-proxy forwards; a forwarded header from any other peer is
    // ignored. ClientIp and SecureTransport read the same set.
    let mut builder = TransportServer::builder(hub.clone(), move |hub| menger::calls::rpc_module(hub, limits))
        .bind(args.bind)
        .with_trusted_proxies(plexus_transport::TrustedProxies::parse(&args.trusted_proxy).map_err(anyhow::Error::msg)?)
        .with_websocket(ws_port)
        .with_connection_quotas(ConnectionQuotas {
            max_connections: args.max_connections,
            max_per_ip: args.max_connections_per_ip,
            max_per_principal: args.max_connections_per_principal,
            max_anonymous: args.max_anonymous_connections,
            max_anonymous_per_ip: args.max_anonymous_connections_per_ip,
            // BIDIR-8: the transport holds the calls a connection has open too.
            max_calls_per_connection: args.max_calls_per_connection,
        })
        .with_allowed_hosts(hosts(ws_port));
    tracing::info!("WebSocket: ws://{}", std::net::SocketAddr::new(args.bind, ws_port));
    // IDENTITY-29: only menger's own pages may open the socket from a
    // browser, login or not; synapse sends no Origin and is unaffected.
    // AUTH-6/AUTH-7: no credential → admitted into the anonymous view (only the auth door); a credential presented and
    // refused → 401 with RFC 6750's challenge (plexus-login's `apply`).
    builder = wired.apply(builder);
    let names: Vec<&str> = wired.providers.iter().map(|p| p.name()).collect();
    tracing::info!("Login:     {} — anonymous callers see only the auth door", names.join(", "));
    // ACCESS-30: the one redirect URI a provider must list, derived.
    tracing::info!("Sign-in:   returns to {}/login/callback", wiring_public_url(&args.public_url, shell_port));
    if let Some(spec) = &args.identity {
        tracing::info!("Sign-in:   our own page over {spec}");
    }
    // ACCESS-24: no REST bridge — it cannot carry an identity.
    if !args.no_shell {
        let cfg = shell::ShellConfig {
            port: shell_port,
            ws_port,
            // DIST-3: --app-dist given → that directory; else the
            // checkout's frontend/dist (v2: nothing is embedded).
            app: args.app_dist.clone().map(shell::AppSource::Dir).unwrap_or_else(shell::preferred_app),
            env: args.env.clone(),
            login: wired.login.clone(),
            allowed_hosts: hosts(shell_port),
            // ACCESS-30: reached over https — every answer says so (HSTS).
            https,
            // RUN-9: RFC 9728's document on the public origin, behind https.
            metadata: wired.metadata_route(),
            // APP-110: checked before anything opens, as the other origins are.
            // UI-3: `/app/` and `/app/<hash>/…` from the ui service's store.
            ui: Some(ui_store),
            framing: shell::Framing {
                ancestors: args.frame_ancestor.iter().map(|o| shell::check_origin(o)).collect::<Result<_, _>>().map_err(|e| anyhow::anyhow!("--frame-ancestor: {e}"))?,
                live: args.live_origin.as_deref().map(shell::check_origin).transpose().map_err(|e| anyhow::anyhow!("--live-origin: {e}"))?,
            },
        };
        // DIST-14: bound before the banner, so the link answers when printed.
        let at = std::net::SocketAddr::new(args.bind, shell_port);
        let listener = tokio::net::TcpListener::bind(at).await.map_err(|e| anyhow::anyhow!("can't listen on {at} for the app: {e} — give another with --shell-port"))?;
        tokio::spawn(async move {
            if let Err(e) = shell::serve_on(listener, cfg, std::future::pending()).await {
                tracing::error!("shell server exited: {e}");
            }
        });
    }

    // DIST-14: serving, and the banner once the socket answers — the link
    // works the moment it is printed.
    let serving = tokio::spawn(builder.build().await?.serve());
    let ws_at = std::net::SocketAddr::new(if args.bind.is_unspecified() { std::net::Ipv4Addr::LOCALHOST.into() } else { args.bind }, ws_port);
    for _ in 0..200 {
        if serving.is_finished() || tokio::net::TcpStream::connect(ws_at).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    if serving.is_finished() {
        return serving.await?;
    }
    // DIST-14: what a person reads — it runs, where to open it, the data,
    // how to stop it; with --open, the browser too.
    let mut handed = first_run::Handed::Linked;
    let url = match &once {
        Some(o) => {
            first_run::Running { pid: std::process::id(), ws_port, shell_port }.write(&data_dir)?;
            handed = first_run::hand_key_to_synapse(&data_dir);
            first_run::link(shell_port, &o.mint()?)
        }
        None => format!("{}/", wiring_public_url(&args.public_url, shell_port)),
    };
    if !args.no_shell {
        // DIST-17: a local start hands its key to synapse and says how to sign in.
        let agents = if once.is_some() { first_run::agent_lines(&handed, &data_dir, ws_port) } else { vec![] };
        print!("{}", first_run::banner(env!("CARGO_PKG_VERSION"), &url, once.is_some(), &agents, &db_path, &moved));
        if args.open && !plexus_login::browser::open(&url) {
            println!("  (no browser to open here: open the link above yourself)");
        }
    }
    serving.await?
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
            transitions,
            tombstones,
            shortcodes,
            revisions,
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
            // MENGER-13: the transition log, when the file has one.
            match transitions {
                Some(n) => println!(
                    "{}: the transition log's {n} transitions agree with the tickets and with each other (MENGER-13), and so do the births and status-since kept from them (FILTER-27) and the last touches (FILTER-30)",
                    db_path.display()
                ),
                None => println!("{}: no transition log yet (MENGER-13) — the next open begins it", db_path.display()),
            }
            // ACT-5: the tombstones, none of them a ticket.
            if let Some(n) = tombstones {
                println!("{}: {n} tombstone(s) of deleted tickets, none of them a ticket still (ACT-5)", db_path.display());
            }
            // ORG-3: the shortcodes and their namespaces.
            match shortcodes {
                Some(n) => println!(
                    "{}: {n} shortcode(s), each in the namespace where its root hangs gives it, no two of one code in one namespace, every full address distinct, every ticket's shortcode one of them and every pin a ticket or a tombstone (ORG-3)",
                    db_path.display()
                ),
                None => println!("{}: no shortcodes table yet (ORG-3) — the next open makes it", db_path.display()),
            }
            // TICKETS-7: the revisions, each ticket's latest the ticket as it is.
            match revisions {
                Some(n) => println!(
                    "{}: {n} revision(s), each ticket's numbered from 1 with none missing, its latest the ticket as it is, and every body they name kept under its hash (TICKETS-7)",
                    db_path.display()
                ),
                None => println!("{}: no revisions tables yet (TICKETS-7) — the next open makes them", db_path.display()),
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

/// ACCESS-30: the public URL as the login reads it (plexus-login's default
/// when none is given), without a trailing `/`.
fn wiring_public_url(public_url: &Option<String>, shell_port: u16) -> String {
    public_url.clone().unwrap_or_else(|| format!("http://127.0.0.1:{shell_port}")).trim_end_matches('/').to_string()
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

