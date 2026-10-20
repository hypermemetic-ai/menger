use std::path::PathBuf;

use clap::Parser;
use plexus_core::plexus::DynamicHub;
use plexus_login::login::Provider;
use plexus_transport::TransportServer;
use tether::login::{LoginConfig, StartGrant};

/// menger tether: agent sessions on this machine over Plexus RPC, run by
/// the substrate inside it.
#[derive(Parser, Debug)]
#[command(name = "tether")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    #[command(flatten)]
    serve: Args,
}

#[derive(clap::Subcommand, Debug)]
enum Cmd {
    /// TETHER-11: enrol this machine's key with a remote menger — a code to
    /// compare, the app's #/pair printed (opened with --open), a person grants it.
    Pair(PairArgs),
}

#[derive(clap::Args, Debug)]
struct PairArgs {
    /// The remote menger: its app (https://app.menger.sh) or its socket (wss://…/ws).
    #[arg(long, default_value = "https://app.menger.sh")]
    remote: String,

    /// The app's origin when it is not the remote's (a local menger's shell port).
    #[arg(long)]
    app: Option<String>,

    /// This machine's name there — what it signs in as and its key's name
    /// (default: the host name).
    #[arg(long)]
    name: Option<String>,

    /// Its key (default `~/.plexus/keys/<name>.key`, made when there is none).
    #[arg(long)]
    key: Option<PathBuf>,

    // DIST-14: the house rule — print the URL, code, key and QR always; open
    // the browser only when asked.
    /// Also open the pairing page in this machine's browser.
    #[arg(long)]
    open: bool,

    // DIST-14: opening was the default (TETHER-11); it is not now, so
    // `--no-browser` changes nothing, and is accepted so nothing breaks.
    #[arg(long, hide = true)]
    no_browser: bool,

    /// How long to wait for the grant.
    #[arg(long, default_value = "600")]
    wait_secs: u64,

    /// How often to ask whether it came.
    #[arg(long, default_value = "3", hide = true)]
    poll_secs: u64,

    /// Where TETHER_REMOTE / TETHER_REMOTE_AS are written, where missing
    /// (default `~/.config/menger/tether.env`).
    #[arg(long)]
    env_file: Option<PathBuf>,
}

#[derive(clap::Args, Debug)]
struct Args {
    /// The WebSocket's port.
    #[arg(short, long, env = "TETHER_PORT", default_value = "4486")]
    port: u16,

    /// Admit callers through this login provider (repeatable; `;`-separated
    /// in the variable): `key` for agents' Ed25519 key proofs (synapse
    /// `_login tether --key NAME`), and people's providers as plexus-login
    /// takes them. At least one: there is no open mode.
    #[arg(long = "auth-provider", env = "TETHER_AUTH_PROVIDERS", value_name = "SPEC", value_delimiter = ';')]
    auth_provider: Vec<Provider>,

    /// A principal that is admin from the start (`key:<hex>` — `synapse
    /// _key show NAME`).
    #[arg(long, env = "TETHER_ADMINS", value_name = "PRINCIPAL", value_delimiter = ';')]
    admin: Vec<String>,

    /// A standing grant, `ROLE=PRINCIPAL` (`viewer`, `editor`, `admin`).
    #[arg(long, env = "TETHER_GRANTS", value_name = "ROLE=PRINCIPAL", value_delimiter = ';')]
    grant: Vec<StartGrant>,

    /// Where tether keeps its substrate's databases (default
    /// `~/.local/share/menger-tether`).
    #[arg(long, env = "TETHER_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Where sessions work unless a start names a directory (default: here).
    #[arg(long, env = "TETHER_WORK_DIR")]
    work_dir: Option<PathBuf>,

    /// The Claude Code binary (default: found on this machine).
    #[arg(long, env = "TETHER_CLAUDE_PATH")]
    claude_path: Option<String>,

    /// Claude Code's --permission-mode for every session (`bypassPermissions`
    /// for builders that run unattended, as the chat bridge's unit has it).
    #[arg(long, env = "TETHER_PERMISSION_MODE")]
    permission_mode: Option<String>,

    /// TETHER-9: the remote menger's socket (`wss://app.menger.sh/ws`):
    /// tether signs in there as `--remote-as`, listens, and posts runs'
    /// reports.
    #[arg(long, env = "TETHER_REMOTE")]
    remote: Option<String>,

    /// The agent tether is on the remote: synapse's key name (`--as`).
    #[arg(long, env = "TETHER_REMOTE_AS", default_value = "builder")]
    remote_as: String,

    /// The key it signs in with (default `~/.plexus/keys/<remote-as>.key`,
    /// synapse's; 0600). Read from the file, never passed as a value.
    #[arg(long, env = "TETHER_REMOTE_KEY")]
    remote_key: Option<PathBuf>,

    /// Don't listen on the remote (runs still post their reports).
    #[arg(long, env = "TETHER_NO_LISTEN")]
    no_listen: bool,

    /// Act on answers given through the API and comments by keys too, not
    /// only people's (tether's own key never).
    #[arg(long, env = "TETHER_ACCEPT_KEYS")]
    accept_keys: bool,

    /// Seconds an answer or reply no session of tether's asked for waits
    /// for another agent to act on it before tether starts a session.
    #[arg(long, env = "TETHER_GRACE_SECS", default_value = "600")]
    grace_secs: u64,

    /// CHAT-45's archetypes file (default `~/.config/menger/archetypes.env`).
    #[arg(long, env = "TETHER_ARCHETYPES")]
    archetypes: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn,tether=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    if let Some(Cmd::Pair(p)) = cli.cmd {
        return pair(p).await;
    }
    let args = cli.serve;

    if let Some(why) = tether::login::refusal(&args.auth_provider) {
        anyhow::bail!(why);
    }
    let data = match args.data_dir {
        Some(d) => d,
        None => {
            let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| anyhow::anyhow!("no HOME: give --data-dir"))?;
            home.join(".local/share/menger-tether")
        }
    };
    std::fs::create_dir_all(&data)?;
    // substrate keeps its databases under this, never ~/.plexus/substrate:
    // a substrate running beside tether keeps its own.
    plexus_substrate::activations::storage::set_data_dir(data.join("substrate"));
    let work_dir = match args.work_dir {
        Some(w) => w,
        None => std::env::current_dir()?,
    };

    let remote = match &args.remote {
        Some(url) => {
            let key = match args.remote_key.clone() {
                Some(k) => k,
                None => tether::remote::default_key_file(&args.remote_as).ok_or_else(|| anyhow::anyhow!("no HOME: give --remote-key"))?,
            };
            Some(tether::remote::Remote::new(url, &args.remote_as, &key)?)
        }
        None => None,
    };
    let listen = (remote.is_some() && !args.no_listen).then(|| tether::listener::ListenConfig {
        accept_keys: args.accept_keys,
        grace: std::time::Duration::from_secs(args.grace_secs),
        archetypes: args.archetypes.clone().unwrap_or_else(tether::default_archetypes),
    });
    if let Some(r) = &remote {
        tracing::info!("remote {} as {} ({}){}", r.url(), r.agent(), r.principal(), if listen.is_some() { ", listening" } else { "" });
    }
    let opts = tether::Options {
        login: LoginConfig { providers: args.auth_provider.clone(), admins: args.admin, grants: args.grant, ws_port: args.port },
        work_dir,
        claude_path: args.claude_path,
        data_dir: Some(data.clone()),
        remote,
        listen,
        archetypes: args.archetypes,
        permission_mode: args.permission_mode,
    };
    let (hub, login) = tether::build(opts).await?;
    let names: Vec<&str> = args.auth_provider.iter().map(Provider::name).collect();
    tracing::info!("tether on ws://127.0.0.1:{} — login: {}; data: {}", args.port, names.join(", "), data.display());
    let builder = TransportServer::builder(hub, |h| {
        DynamicHub::arc_into_rpc_module(h).map_err(|e| anyhow::anyhow!("Failed to create RPC module: {e}"))
    })
    .with_websocket(args.port);
    login.apply(builder).build().await?.serve().await
}

/// TETHER-11: `tether pair`.
async fn pair(p: PairArgs) -> anyhow::Result<()> {
    use tether::pair;
    let endpoints = pair::endpoints(&p.remote, p.app.as_deref()).map_err(anyhow::Error::msg)?;
    let name = pair::check_name(&p.name.unwrap_or_else(pair::default_name)).map_err(anyhow::Error::msg)?;
    let key_file = match p.key {
        Some(k) => k,
        None => tether::remote::default_key_file(&name).ok_or_else(|| anyhow::anyhow!("no HOME: give --key"))?,
    };
    let cfg = pair::Config {
        endpoints,
        name,
        key_file,
        // DIST-14: `--no-browser` is the default now.
        open: p.open && !p.no_browser,
        wait: std::time::Duration::from_secs(p.wait_secs),
        poll: std::time::Duration::from_secs(p.poll_secs.max(1)),
        env_file: p.env_file.or_else(pair::default_env_file),
        // TETHER-12: colours only where a person reads it, and not when told not to.
        paint: match std::io::IsTerminal::is_terminal(&std::io::stdout()) && std::env::var_os("NO_COLOR").is_none() {
            true => pair::Paint::Ansi,
            false => pair::Paint::Plain,
        },
    };
    pair::run(&cfg, &mut std::io::stdout()).await.map(|_| ())
}
