use clap::Parser;
use menger::store::{SeedShape, SeedSpec, Store, Verdict};
use menger::login::{LoginButton, Provider};
use menger::wiring::{LoginOptions, Wiring};
use menger_tickets_api::access::Access;
use menger::{build_rpc_login, default_db_path, local, shell, MengerAccess};
use plexus_transport::{ConnectionQuotas, TransportServer};
use std::path::PathBuf;

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

    /// RUN-8: the address the WebSocket and the shell listen on — `0.0.0.0` / `::` for every address this machine has, or
    /// one of them (IPv6 with or without brackets). Default loopback. The
    /// names they answer to are still ACCESS-25's: a specific address adds
    /// its own literal (and the shell's origin at it may open the socket);
    /// any other name is an --allowed-host (and an --allowed-origin).
    #[arg(long, env = "MENGER_BIND", default_value = "127.0.0.1", value_name = "ADDR", value_parser = menger::bind::parse_bind)]
    bind: std::net::IpAddr,

    /// ACCESS-24: the REST bridge cannot carry an identity, and menger has
    /// no open mode, so it is not served at all; `--no-rest` (and
    /// `MENGER_NO_REST`) is accepted and changes nothing, so a launcher or
    /// an env file that says it still starts.
    #[arg(
        long,
        env = "MENGER_NO_REST",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::FalseyValueParser::new(),
        hide = true
    )]
    no_rest: bool,

    /// Directory holding the built Leptos app (frontend/dist), served at
    /// /app/ on the shell port. Missing output degrades to a "frontend
    /// not built" page, never an error. DIST-3: without it, a binary built
    /// with `embedded-app` serves the build compiled into it, any other the
    /// checkout's frontend/dist.
    #[arg(long, env = "MENGER_APP_DIST", value_name = "DIR")]
    app_dist: Option<PathBuf>,

    /// MFE-23: the environment's name (`prod`, `test`, …); the deploy env
    /// files set `MENGER_ENV`. `test` titles the browser tab `menger.test`,
    /// anything else (or none) `menger`.
    #[arg(long, env = "MENGER_ENV", value_name = "NAME")]
    env: Option<String>,

    /// ACCESS-24: stdio carries no credential, and menger has no open mode
    /// — `--stdio` is refused with the reason.
    #[arg(long, hide = true)]
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

    /// MENGER-15: the longest ticket body `create` and `amend` store, in
    /// bytes (default 8 MiB: a whole page with its media inlined). A larger
    /// body is refused with the typed `body_too_large { bytes, max }`.
    /// Bodies are not cached (SCALE-14), so this bounds one read or write,
    /// not the process. Above ~9 MB the WebSocket's own 10 MB request limit
    /// refuses first (JSON escaping grows a body), and the REST bridge's
    /// request limit (2 MB, axum's default) is lower still.
    #[arg(long, env = "MENGER_MAX_BODY_BYTES", value_name = "BYTES", default_value_t = menger_tickets_api::store::BODY_MAX_DEFAULT)]
    max_body_bytes: usize,

    /// IDENTITY-14: admit callers through this identity provider
    /// (repeatable): `idp=<issuer>,<audience>`,
    /// `auth0=<domain>,<audience>[,<roles-claim>]`,
    /// `oidc:<name>=<issuer>,<audience>[,<roles-claim>]`,
    /// `workos=<client-id>[,<api-base>]` (IDENTITY-30),
    /// `nostr=<ws-url>[,<ws-url>…]`, or (IDENTITY-42) `key` /
    /// `key=<ws-url>[,<ws-url>…]` — agents' Ed25519 key proofs naming this
    /// socket (ws://127.0.0.1:<port>, ws://localhost:<port>, the ws form of
    /// --public-url) or a URL given; not offered on the login page. A caller's credential rides the
    /// `access_token` or `nostr` cookie, or `Authorization: Bearer`, and its
    /// provider's adapter makes it a Plexus identity at the WebSocket
    /// upgrade. In the env var, specs are separated by `;`. ACCESS-24: at
    /// least one (or --identity) — menger has no open mode and refuses to
    /// start without a login provider (a development server runs `key`).
    /// Login is always required: the app's routes go to the login page, an
    /// upgrade that proves no one is admitted into the anonymous view
    /// (auth.providers, auth.login, auth.signup, and nothing else), and the
    /// gate decides every call (`--require-login` went: it is the only mode).
    #[arg(long = "auth-provider", env = "MENGER_AUTH_PROVIDERS", value_name = "SPEC", value_delimiter = ';')]
    auth_provider: Vec<Provider>,


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
    /// http://127.0.0.1:<shell-port>. ACCESS-30: with login on and the
    /// server reached beyond loopback (`--bind`, or this URL's host), it is
    /// required and `https://` — a proxy terminates TLS in front, the app
    /// opens `wss://<host>/ws`, cookies are `Secure` and `__Host-`, and the
    /// provider's redirect URI is `<this>/login/callback`. An `http://` URL
    /// on the LAN is served with a warning.
    #[arg(long, env = "MENGER_PUBLIC_URL", value_name = "URL")]
    public_url: Option<String>,

    /// IDENTITY-29: another origin whose pages may open the socket
    /// (repeatable); the shell's own are always allowed.
    #[arg(long = "allowed-origin", env = "MENGER_ALLOWED_ORIGINS", value_name = "ORIGIN", value_delimiter = ';')]
    allowed_origin: Vec<String>,

    /// ACCESS-25: another `Host` every listener (WebSocket, shell, REST)
    /// answers to (repeatable; `host:port`, or `host` for a default port).
    /// Always answered: `127.0.0.1:<its port>`, `localhost:<its port>` and
    /// `--public-url`'s host; anything else is refused 421 before routing
    /// (DNS rebinding). In the env var, separated by `;`.
    #[arg(long = "allowed-host", env = "MENGER_ALLOWED_HOSTS", value_name = "HOST", value_delimiter = ';')]
    allowed_host: Vec<String>,

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

    /// AUTH-22: our own sign-in page over an identity backend, chosen by
    /// configuration: `workos:<client_…>[,social=google+github+apple][,sso=off][,key=@<file>]`
    /// (WorkOS's User Management API, no AuthKit screen; the API key from
    /// MENGER_WORKOS_API_KEY or the file `key=@` names, never the flag),
    /// `plexus-idp:<issuer>,<audience>` (passwords; local only), or `memory`
    /// (tests and development) — those two only with `--env test`. Its tokens are admitted at the socket through
    /// its provider, as any --auth-provider's. Changing it needs a restart.
    #[arg(long, env = "MENGER_IDENTITY", value_name = "SPEC")]
    identity: Option<plexus_login::backends::IdentitySpec>,

    /// AUTH-22: a proxy (CIDR or address, repeatable) whose X-Forwarded-For
    /// is believed for the client's IP — the edge in front of the shell and
    /// the WebSocket (TRANSPORT-7: its per-IP and anonymous quotas count the
    /// forwarded client; a header from any other peer is ignored) —
    /// or `none`: people connect directly. AUTH-24 #2: with --identity and a
    /// --public-url off loopback, one of the two is required. In the env
    /// var, separated by `;`.
    #[arg(long = "trusted-proxy", env = "MENGER_TRUSTED_PROXIES", value_name = "CIDR|none", value_delimiter = ';')]
    trusted_proxy: Vec<String>,

    /// AUTH-22: another origin a sign-in may send the browser back to
    /// (repeatable); the shell's own always may.
    #[arg(long = "return-origin", env = "MENGER_RETURN_ORIGINS", value_name = "ORIGIN", value_delimiter = ';')]
    return_origin: Vec<String>,

    /// IDENTITY-19: how long a sign-in lasts.
    #[arg(long, env = "MENGER_SESSION_HOURS", default_value = "12", value_name = "HOURS")]
    session_hours: i64,

    /// ACCESS-29: a browser's sign-in ends after this long unused — no
    /// socket open on it and no page loaded with it — as well as at its
    /// --session-hours. 0: only at its expiry. A minted token (an agent's)
    /// keeps only its expiry.
    #[arg(long, env = "MENGER_SESSION_IDLE_MINUTES", default_value_t = menger::sessions::IDLE_SECS_DEFAULT / 60, value_name = "MINUTES")]
    session_idle_minutes: i64,

    /// ACCESS-29: the calls one WebSocket connection may have open at once
    /// (each call — a `watch` for as long as it runs — is a subscription);
    /// past it a call is answered `error_kind: too_many_calls`.
    #[arg(long, env = "MENGER_MAX_CALLS_PER_CONNECTION", default_value_t = menger::calls::CALLS_PER_CONNECTION, value_name = "N")]
    max_calls_per_connection: usize,

    /// ACCESS-29: a WebSocket connection with no call open for this long is
    /// closed. 0: never.
    #[arg(long, env = "MENGER_IDLE_TIMEOUT_SECS", default_value_t = menger::calls::IDLE_SECS, value_name = "SECS")]
    idle_timeout_secs: u64,

    /// ACCESS-29 (AUTH-12's quotas): WebSocket connections open in all.
    #[arg(long, env = "MENGER_MAX_CONNECTIONS", default_value_t = ConnectionQuotas::default().max_connections, value_name = "N")]
    max_connections: u32,

    /// ACCESS-29: connections from one IP address (the peer's: behind a
    /// proxy, the proxy's).
    #[arg(long, env = "MENGER_MAX_CONNECTIONS_PER_IP", default_value_t = ConnectionQuotas::default().max_per_ip, value_name = "N")]
    max_connections_per_ip: u32,

    /// ACCESS-29: connections one signed-in principal holds.
    #[arg(long, env = "MENGER_MAX_CONNECTIONS_PER_PRINCIPAL", default_value_t = ConnectionQuotas::default().max_per_principal, value_name = "N")]
    max_connections_per_principal: u32,

    /// ACCESS-29: connections that prove no one, in all — held below
    /// --max-connections, so they never lock a signed-in caller out.
    #[arg(long, env = "MENGER_MAX_ANONYMOUS_CONNECTIONS", default_value_t = ConnectionQuotas::default().max_anonymous, value_name = "N")]
    max_anonymous_connections: u32,

    /// ACCESS-29: anonymous connections from one IP address.
    #[arg(long, env = "MENGER_MAX_ANONYMOUS_CONNECTIONS_PER_IP", default_value_t = ConnectionQuotas::default().max_anonymous_per_ip, value_name = "N")]
    max_anonymous_connections_per_ip: u32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,menger=debug".into());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    let db_path = args.db_path.unwrap_or_else(default_db_path);
    if args.verify_derived {
        std::process::exit(verify_derived(db_path).await);
    }
    // ACCESS-24: no open mode — no provider, no start; and nothing that
    // cannot carry a credential (stdio) is served.
    if let Some(refusal) = refusal(&args.auth_provider, args.identity.is_some(), args.stdio) {
        anyhow::bail!(refusal);
    }
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
        providers: args.auth_provider.clone(),
        buttons: args.login_button.clone(),
        email: args.login_email.clone(),
        password: args.login_password.clone(),
        forgot: args.login_forgot.clone(),
        public_url: args.public_url.clone(),
        // RUN-8: the shell's pages at a specific bound address may open the socket.
        allowed_origins: args.allowed_origin.iter().cloned().chain(menger::bind::bind_origin(args.bind, args.shell_port)).collect(),
        // AUTH-25 N9 / RUN-8: a shell off loopback names its proxy.
        shell_loopback: args.bind.is_loopback(),
        session_ttl_secs: args.session_hours * 3600,
        session_idle_secs: args.session_idle_minutes * 60,
        require: true,
        page: !args.no_shell,
        title: shell::tab_title(args.env.as_deref()),
        names: menger::login_names(),
        identity: args.identity.as_ref().map(|s| s.build(menger::login_access::BRANDING.app)).transpose()?,
        trusted_proxies: plexus_login::client_ip::TrustedProxies::parse(&args.trusted_proxy).map_err(anyhow::Error::msg)?,
        return_origins: args.return_origin.clone(),
        // AUTH-24 #8: `memory` and `plexus-idp` sign people in only where
        // the dev sign-in may (IDENTITY-38: `--env test`).
        allow_development_backends: dev_login_allowed(args.env.as_deref()),
        ..LoginOptions::new(menger::login_access::BRANDING, args.port, args.shell_port)
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
    let access = Access::with_login(table.clone(), args.admin.clone()).minting(table);
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
    let hub = build_rpc_login(&db_path, seed, args.seed_shape, offers, access, wired.backend.clone(), metadata, args.max_body_bytes).await?;

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
        .with_websocket(args.port)
        .with_connection_quotas(ConnectionQuotas {
            max_connections: args.max_connections,
            max_per_ip: args.max_connections_per_ip,
            max_per_principal: args.max_connections_per_principal,
            max_anonymous: args.max_anonymous_connections,
            max_anonymous_per_ip: args.max_anonymous_connections_per_ip,
        })
        .with_allowed_hosts(hosts(args.port));
    tracing::info!("WebSocket: ws://{}", std::net::SocketAddr::new(args.bind, args.port));
    // IDENTITY-29: only menger's own pages may open the socket from a
    // browser, login or not; synapse sends no Origin and is unaffected.
    // AUTH-6/AUTH-7: no credential → admitted into the anonymous view (only the auth door); a credential presented and
    // refused → 401 with RFC 6750's challenge (plexus-login's `apply`).
    builder = wired.apply(builder);
    let names: Vec<&str> = wired.providers.iter().map(|p| p.name()).collect();
    tracing::info!("Login:     {} — anonymous callers see only the auth door", names.join(", "));
    // ACCESS-30: the one redirect URI a provider must list, derived.
    tracing::info!("Sign-in:   returns to {}/login/callback", wiring_public_url(&args.public_url, args.shell_port));
    if let Some(spec) = &args.identity {
        tracing::info!("Sign-in:   our own page over {spec}");
    }
    // ACCESS-24: no REST bridge — it cannot carry an identity.
    if !args.no_shell {
        let cfg = shell::ShellConfig {
            port: args.shell_port,
            ws_port: args.port,
            // DIST-3: --app-dist given → that directory; else the build
            // embedded in this binary (`embedded-app`), if any; else the
            // checkout's frontend/dist.
            app: args.app_dist.map(shell::AppSource::Dir).unwrap_or_else(shell::preferred_app),
            env: args.env.clone(),
            login: wired.login.clone(),
            allowed_hosts: hosts(args.shell_port),
            // ACCESS-30: reached over https — every answer says so (HSTS).
            https,
            // RUN-9: RFC 9728's document on the public origin, behind https.
            metadata: wired.metadata_route(),
        };
        let bind = args.bind;
        tokio::spawn(async move {
            if let Err(e) = shell::serve_at(bind, cfg).await {
                tracing::error!("shell server exited: {e}");
            }
        });
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
            transitions,
            tombstones,
            deployments,
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
                    "{}: the transition log's {n} transitions agree with the tickets and with each other (MENGER-13), and so do the births and status-since kept from them (FILTER-27) and the last touches, with the comments and answers (FILTER-30)",
                    db_path.display()
                ),
                None => println!("{}: no transition log yet (MENGER-13) — the next open begins it", db_path.display()),
            }
            // ACT-5: the tombstones, none of them a ticket.
            if let Some(n) = tombstones {
                println!("{}: {n} tombstone(s) of deleted tickets, none of them a ticket still (ACT-5)", db_path.display());
            }
            // DEPLOY-11: the environments and the deployed cells.
            if let Some(n) = deployments {
                println!(
                    "{}: {n} deployment(s): every environment is what its deployments fold to, and every deployed cell what a scan of the tickets against them gives (DEPLOY-11)",
                    db_path.display()
                );
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

/// ACCESS-24: why this start is refused, if it is — no login provider
/// (menger has no open mode), or stdio (no credential rides it).
fn refusal(providers: &[Provider], identity: bool, stdio: bool) -> Option<String> {
    if stdio {
        return Some(
            "--stdio is gone: menger has no open mode, and stdio carries no credential (ACCESS-24) — \
             reach it over the WebSocket with a key (synapse _login, or --auth-provider key)"
                .into(),
        );
    }
    (providers.is_empty() && !identity).then(|| {
        "menger has no open mode: give it a login provider with --auth-provider <SPEC> \
         (MENGER_AUTH_PROVIDERS) — `--auth-provider key` admits agents' Ed25519 keys, enough for \
         a development server with `--admin key:<hex>` — or --identity (ACCESS-24)"
            .into()
    })
}

/// IDENTITY-38: the dev sign-in runs only on the environment named test.
fn dev_login_allowed(env: Option<&str>) -> bool {
    env == Some("test")
}

#[cfg(test)]
mod tests {
    /// ACCESS-24: no provider, no start — the message names the flag;
    /// stdio is refused with or without one.
    #[test]
    fn a_start_without_a_provider_is_refused_naming_the_flag() {
        let none = super::refusal(&[], false, false).expect("refused");
        assert!(none.contains("--auth-provider") && none.contains("no open mode"), "{none}");
        let key: Vec<menger::login::Provider> = vec!["key".parse().unwrap()];
        assert_eq!(super::refusal(&key, false, false), None);
        assert_eq!(super::refusal(&[], true, false), None, "--identity is a provider");
        assert!(super::refusal(&key, false, true).expect("stdio").contains("--stdio"));
    }

    #[test]
    fn the_dev_sign_in_is_test_s_alone() {
        assert!(super::dev_login_allowed(Some("test")));
        for env in [None, Some("prod"), Some("Test"), Some("dev"), Some("")] {
            assert!(!super::dev_login_allowed(env), "{env:?}");
        }
    }
}
