//! DIST-3: menger as a library call — the whole server, started from code.
//!
//! [`LocalServer::start`] is what the binary does with no `--auth-provider`,
//! `--no-rest` and no `--stdio`: the store opened (its directory made if
//! missing), every activation on the `menger` hub, the Plexus WebSocket and
//! the web shell (the app, `/vanilla`) on loopback — and nothing else: no
//! login, no REST bridge, no registry announcement. A host (DIST-1's
//! desktop app) runs menger without copying `main.rs`.
//!
//! A port of 0 is a free one: the shell's listener and the WebSocket's are
//! bound before anything serves (the WebSocket by plexus-transport's
//! `serve_websocket_with`, which reports the port it bound), so the ports a
//! [`LocalServer`] reports are the ones it holds, with no window in which
//! another process can take them. `start` returns once both listeners are
//! bound and serving; [`LocalServer::shutdown`] stops both and releases
//! their ports (dropping the server stops them too, without waiting).
//!
//! The socket admits browser pages from the shell's own origin
//! (`http://127.0.0.1:<shell>`, `http://localhost:<shell>`, as the binary
//! computes them — [`shell_origins`]) and from [`LocalConfig::extra_origins`]
//! — a Tauri webview's are [`TAURI_ORIGINS`]. A caller with no `Origin`
//! (synapse, a native client) is admitted as always.
//!
//! The wiring the binary shares with this module is here too: the hub's RPC
//! module ([`rpc_module`]), the shell's origins ([`shell_origins`]) and the
//! startup log of the activations ([`log_activations`]).

use crate::shell::{self, AppSource, ShellConfig};
use jsonrpsee::server::ServerHandle;
use plexus_core::plexus::DynamicHub;
use plexus_transport::websocket::{serve_websocket_with, UpgradeOptions};
use plexus_transport::WebSocketConfig;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// The origins a Tauri 2 webview loads its page from (custom protocol on
/// macOS/Linux, `http(s)://tauri.localhost` on Windows and Android) — for
/// [`LocalConfig::extra_origins`] when the shell's page is opened in one.
pub const TAURI_ORIGINS: [&str; 3] = ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"];

/// How to run a [`LocalServer`].
#[derive(Debug, Clone)]
pub struct LocalConfig {
    /// The database file (the system of record); its directory is made if
    /// missing, and a fresh location starts empty.
    pub db_path: PathBuf,
    /// The Plexus WebSocket's port on 127.0.0.1; 0 picks a free one.
    pub ws_port: u16,
    /// The web shell's port on 127.0.0.1; 0 picks a free one.
    pub shell_port: u16,
    /// The environment's name, which titles the tab (MFE-23: `test` →
    /// `menger.test`, anything else `menger`).
    pub env: Option<String>,
    /// Where the app's build is served from ([`AppSource::preferred`]: the
    /// embedded build when this binary carries one).
    pub app: AppSource,
    /// Origins beyond the shell's own whose pages may open the socket.
    pub extra_origins: Vec<String>,
}

impl LocalConfig {
    /// `db_path` on free ports, no environment named, the preferred app
    /// source, no extra origins.
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
            ws_port: 0,
            shell_port: 0,
            env: None,
            app: AppSource::preferred(),
            extra_origins: Vec::new(),
        }
    }
}

/// A running menger: its WebSocket and its shell, on loopback.
pub struct LocalServer {
    ws_port: u16,
    shell_port: u16,
    ws: Option<ServerHandle>,
    shell_stop: Option<oneshot::Sender<()>>,
    shell_task: Option<JoinHandle<()>>,
}

impl LocalServer {
    /// Opens the store and starts the WebSocket and the shell. Needs a
    /// Tokio runtime (it spawns the servers onto it). Fails if the store
    /// cannot be opened or a port cannot be bound.
    pub async fn start(cfg: LocalConfig) -> anyhow::Result<LocalServer> {
        if let Some(dir) = cfg.db_path.parent().filter(|d| !d.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|e| anyhow::anyhow!("cannot create {}: {e}", dir.display()))?;
        }
        let hub = crate::build_rpc(&cfg.db_path).await?;
        log_activations(&hub);

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.shell_port))
            .await
            .map_err(|e| anyhow::anyhow!("cannot bind the shell to 127.0.0.1:{}: {e}", cfg.shell_port))?;
        let shell_port = listener.local_addr()?.port();

        let mut ws_config = WebSocketConfig::new(cfg.ws_port);
        ws_config.allowed_origins = shell_origins(shell_port, None);
        ws_config.allowed_origins.extend(cfg.extra_origins);
        let (ws, addr) = serve_websocket_with(rpc_module(hub)?, ws_config, None, UpgradeOptions::default()).await?;
        let ws_port = addr.port();
        tracing::info!("WebSocket: ws://127.0.0.1:{ws_port}");

        let (stop, stopped) = oneshot::channel::<()>();
        let shell_cfg = ShellConfig { port: shell_port, ws_port, app: cfg.app, env: cfg.env, login: None };
        let shell_task = tokio::spawn(async move {
            let until = async {
                let _ = stopped.await;
            };
            if let Err(e) = shell::serve_on(listener, shell_cfg, until).await {
                tracing::error!("shell server exited: {e}");
            }
        });
        Ok(LocalServer { ws_port, shell_port, ws: Some(ws), shell_stop: Some(stop), shell_task: Some(shell_task) })
    }

    /// The WebSocket's port.
    pub fn ws_port(&self) -> u16 {
        self.ws_port
    }

    /// The shell's port.
    pub fn shell_port(&self) -> u16 {
        self.shell_port
    }

    /// The app's address, `http://127.0.0.1:<shell>/`.
    pub fn shell_url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.shell_port)
    }

    /// The Plexus WebSocket's address, `ws://127.0.0.1:<ws>`.
    pub fn ws_url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.ws_port)
    }

    /// Stops both servers and waits until their ports are released (the
    /// shell's open connections get five seconds to finish).
    pub async fn shutdown(mut self) {
        if let Some(ws) = self.ws.take() {
            let _ = ws.stop();
            ws.stopped().await;
        }
        if let Some(stop) = self.shell_stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut task) = self.shell_task.take() {
            if tokio::time::timeout(Duration::from_secs(5), &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
        }
    }
}

impl Drop for LocalServer {
    /// Dropped without [`LocalServer::shutdown`]: both servers are told to
    /// stop, and nothing waits for them.
    fn drop(&mut self) {
        if let Some(ws) = self.ws.take() {
            let _ = ws.stop();
        }
        if let Some(stop) = self.shell_stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.shell_task.take() {
            task.abort();
        }
    }
}

/// The hub as the WebSocket (and stdio) transport serves it.
pub fn rpc_module(hub: Arc<DynamicHub>) -> anyhow::Result<jsonrpsee::RpcModule<()>> {
    DynamicHub::arc_into_rpc_module(hub).map_err(|e| anyhow::anyhow!("rpc module build failed: {e}"))
}

/// IDENTITY-29: the shell's own origins, which may always open the socket —
/// its port on 127.0.0.1 and on localhost, and the origin of the public URL
/// people reach it at, when there is one.
pub fn shell_origins(shell_port: u16, public_url: Option<&str>) -> Vec<String> {
    let mut origins = vec![format!("http://127.0.0.1:{shell_port}"), format!("http://localhost:{shell_port}")];
    if let Some(url) = public_url {
        let origin = crate::web_login::origin_of(url);
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }
    origins
}

/// The startup log: each activation and its methods.
pub fn log_activations(hub: &DynamicHub) {
    let activations = hub.list_activations_info();
    tracing::info!("Activations ({}):", activations.len());
    for a in &activations {
        tracing::info!("  {} v{} - {}", a.namespace, a.version, a.description);
        for method in &a.methods {
            tracing::info!("    - {}.{}", a.namespace, method);
        }
    }
}
