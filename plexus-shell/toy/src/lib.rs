//! plexus-shell's toy: a hub named `toy` with one activation (`echo.say`)
//! and a two-file app embedded at build time, started by
//! [`plexus_shell::LocalServer`].

use async_stream::stream;
use futures::Stream;
use plexus_core::plexus::DynamicHub;
use plexus_shell::{AppSource, EmbeddedTable, LocalConfig, LocalServer, ShellConfig};
use std::sync::Arc;

/// app/, as build.rs embedded it.
pub static APP: EmbeddedTable = plexus_shell::embedded_app!();

/// One method: say something back.
#[derive(Clone, Default)]
pub struct Echo;

/// What `say` says back (ACCESS-8: a parameter is a type, never a bare
/// `String`) — any text, as JSON carries it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct Said(String);

#[plexus_macros::activation(namespace = "echo", version = "0.1.0", description = "says it back", typed_params, crate_path = "plexus_core")]
impl Echo {
    /// Answers `text`, prefixed.
    #[plexus_macros::method]
    async fn say(&self, text: Said) -> impl Stream<Item = String> + Send + 'static {
        stream! { yield format!("echo: {}", text.0); }
    }
}

/// The toy's hub.
pub fn hub() -> Arc<DynamicHub> {
    Arc::new(DynamicHub::new("toy").register(Echo))
}

/// The toy on free loopback ports: the embedded app, titled `toy`, its
/// page greeting through an app-added placeholder.
pub async fn start() -> anyhow::Result<LocalServer> {
    let mut shell = ShellConfig::new("toy", AppSource::Embedded(APP));
    shell.stamps.push(("{{GREETING}}".into(), "hello from plexus-shell".into()));
    LocalServer::start(hub(), LocalConfig::new(shell)).await
}
