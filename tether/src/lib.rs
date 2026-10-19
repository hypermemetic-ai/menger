//! menger tether (TETHER-7): the local side of menger as a Plexus RPC tool
//! of its own — `synapse tether …` — with substrate imported as a crate and
//! called in memory ([`engine`]). Its first activation is [`sessions`].

pub mod engine;
pub mod login;
pub mod sessions;
pub mod vocab;

use std::path::PathBuf;
use std::sync::Arc;

use plexus_core::plexus::DynamicHub;

pub use engine::{Engine, TETHER_PRINCIPAL};
pub use sessions::{Session, SessionState, SessionsActivation, SessionsClient, SessionsEvent, TailLine};

/// What a tether is built with.
pub struct Options {
    pub login: login::LoginConfig,
    /// Where sessions work unless a start names a directory.
    pub work_dir: PathBuf,
    /// The Claude Code substrate runs (default: found on this machine).
    pub claude_path: Option<String>,
}

/// tether's hub (`tether`): `sessions`, the `auth` activation, the gate —
/// and the substrate inside it.
pub async fn build(opts: Options) -> anyhow::Result<(Arc<DynamicHub>, login::Login)> {
    let login = login::Login::new(&opts.login).await?;
    let engine = Engine::build(opts.claude_path).await;
    let hub = DynamicHub::new("tether")
        .register(SessionsActivation::new(engine, opts.work_dir))
        .with_auth(login.auth_setup());
    login.seed(&hub);
    Ok((Arc::new(hub), login))
}
