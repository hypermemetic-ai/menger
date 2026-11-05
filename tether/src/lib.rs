//! menger tether (TETHER-7, TETHER-9): the local side of menger as a Plexus
//! RPC tool of its own — `synapse tether …` — with substrate imported as a
//! crate and called in memory ([`engine`]). Its activations are
//! [`sessions`], [`runs`] and [`listen`]; with a remote, it signs in to the
//! remote menger as an agent and acts on what is answered there
//! ([`listener`]), and keeps what it runs across restarts ([`state`]).

pub mod archetypes;
pub mod engine;
pub mod listen;
pub mod listener;
pub mod login;
pub mod pair;
pub mod prompts;
pub mod remote;
pub mod runner;
pub mod runs;
pub mod sessions;
pub mod state;
pub mod vocab;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use plexus_core::plexus::DynamicHub;

pub use engine::{Engine, TETHER_PRINCIPAL};
pub use sessions::{Session, SessionState, SessionsActivation, SessionsClient, SessionsEvent, TailLine};

/// What a tether is built with.
#[derive(Default)]
pub struct Options {
    pub login: login::LoginConfig,
    /// Where sessions work unless a start names a directory.
    pub work_dir: PathBuf,
    /// The Claude Code substrate runs (default: found on this machine).
    pub claude_path: Option<String>,
    /// TETHER-9: where tether keeps its state (`state.json`); none: nothing
    /// is kept across a restart.
    pub data_dir: Option<PathBuf>,
    /// TETHER-9: the remote menger — runs post their reports there.
    pub remote: Option<remote::Remote>,
    /// TETHER-9: listen to the remote (needs `remote` and `data_dir`).
    pub listen: Option<listener::ListenConfig>,
    /// CHAT-45's archetypes file (default `~/.config/menger/archetypes.env`).
    pub archetypes: Option<PathBuf>,
    /// TETHER-9: Claude Code's `--permission-mode` for every session
    /// (`bypassPermissions` for a builder that runs unattended).
    pub permission_mode: Option<String>,
}

/// `~/.config/menger/archetypes.env`.
pub fn default_archetypes() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".config/menger/archetypes.env")
}

/// substrate runs one Claude Code for every session; a permission mode is
/// tether's argument, so tether runs it through a two-line script in its
/// data dir that passes the mode on.
fn with_permission_mode(dir: &Path, claude: Option<String>, mode: &str) -> anyhow::Result<String> {
    if mode.is_empty() || !mode.chars().all(|c| c.is_ascii_alphanumeric()) {
        anyhow::bail!("--permission-mode {mode}: one of Claude Code's modes (default, acceptEdits, plan, bypassPermissions)");
    }
    let claude = claude.unwrap_or_else(|| "claude".into());
    let quoted = format!("'{}'", claude.replace('\'', r"'\''"));
    let script = dir.join(format!("claude-{mode}"));
    std::fs::write(&script, format!("#!/bin/sh\n# tether: every session's Claude Code, in --permission-mode {mode}\nexec {quoted} --permission-mode {mode} \"$@\"\n"))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
    Ok(script.to_string_lossy().into_owned())
}

/// tether's service (`tether`): `sessions`, `runs`, `listen`, the `auth`
/// activation, the gate — and the substrate inside it. With a remote and a
/// listen config, the listener is running when this returns.
pub async fn build(opts: Options) -> anyhow::Result<(Arc<DynamicHub>, login::Login)> {
    let login = login::Login::new(&opts.login).await?;
    let store = match &opts.data_dir {
        Some(d) => Some(Arc::new(state::Store::open(d)?)),
        None => None,
    };
    let claude = match (&opts.permission_mode, &opts.data_dir) {
        (Some(mode), Some(dir)) => Some(with_permission_mode(dir, opts.claude_path, mode)?),
        (Some(_), None) => anyhow::bail!("--permission-mode needs a data dir for its script"),
        (None, _) => opts.claude_path,
    };
    let engine = Engine::build(claude).await;
    let runner = runner::Runner::new(engine, opts.work_dir, opts.remote.clone(), store.clone());
    let archetypes = opts.archetypes.unwrap_or_else(default_archetypes);
    let listener = match (opts.remote, opts.listen, store) {
        (Some(remote), Some(cfg), Some(store)) => {
            let l = listener::Listener::new(remote, runner.clone(), store, listener::ListenConfig { archetypes: archetypes.clone(), ..cfg });
            l.spawn();
            Some(l)
        }
        (Some(_), Some(_), None) => anyhow::bail!("listening needs a data dir to keep its position"),
        _ => None,
    };
    let service = DynamicHub::new("tether")
        .register(SessionsActivation::new(runner.clone()))
        .register(runs::RunsActivation::new(runner, archetypes))
        .register(listen::ListenActivation::new(listener))
        .with_auth(login.auth_setup());
    login.seed(&service);
    Ok((Arc::new(service), login))
}
