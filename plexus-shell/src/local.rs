//! A hub and its shell, started from code (menger's DIST-3 `menger::local`,
//! generalised over the hub).
//!
//! [`LocalServer::start`] serves any [`DynamicHub`] on a Plexus WebSocket
//! and the shell beside it, on loopback — and nothing else: no login, no
//! REST bridge, no registry announcement. A port of 0 is a free one: the
//! shell's listener and the WebSocket's are bound before anything serves
//! (the WebSocket by plexus-transport's `serve_websocket_with`, which
//! reports the port it bound), so the ports a [`LocalServer`] reports are
//! the ones it holds, with no window in which another process can take
//! them. `start` returns once both listeners are bound and serving;
//! [`LocalServer::shutdown`] stops both and releases their ports (dropping
//! the server stops them too, without waiting).
//!
//! The socket admits browser pages from the shell's own origin
//! ([`loopback_origins`]) and from [`LocalConfig::extra_origins`] — a Tauri
//! webview's are [`crate::TAURI_ORIGINS`]. A caller with no `Origin`
//! (synapse, a native client) is admitted as always.

use crate::ShellConfig;
use jsonrpsee::server::ServerHandle;
use plexus_core::plexus::DynamicHub;
use plexus_transport::websocket::{serve_websocket_with, UpgradeOptions};
use plexus_transport::WebSocketConfig;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// How to run a [`LocalServer`].
#[derive(Debug, Clone)]
pub struct LocalConfig {
    /// The Plexus WebSocket's port on 127.0.0.1; 0 picks a free one.
    pub ws_port: u16,
    /// The shell's port on 127.0.0.1; 0 picks a free one.
    pub shell_port: u16,
    /// Origins beyond the shell's own whose pages may open the socket.
    pub extra_origins: Vec<String>,
    /// What the shell serves.
    pub shell: ShellConfig,
}

impl LocalConfig {
    /// `shell` on free ports, no extra origins.
    pub fn new(shell: ShellConfig) -> Self {
        Self { ws_port: 0, shell_port: 0, extra_origins: Vec::new(), shell }
    }
}

/// A running hub: its WebSocket and its shell, on loopback.
pub struct LocalServer {
    ws_port: u16,
    shell_port: u16,
    ws: Option<ServerHandle>,
    shell_stop: Option<oneshot::Sender<()>>,
    shell_task: Option<JoinHandle<()>>,
}

impl LocalServer {
    /// Starts `hub`'s WebSocket and the shell. Needs a Tokio runtime (it
    /// spawns the servers onto it). Fails if a port cannot be bound.
    pub async fn start(hub: Arc<DynamicHub>, cfg: LocalConfig) -> anyhow::Result<LocalServer> {
        log_activations(&hub);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.shell_port))
            .await
            .map_err(|e| anyhow::anyhow!("cannot bind the shell to 127.0.0.1:{}: {e}", cfg.shell_port))?;
        let shell_port = listener.local_addr()?.port();

        let mut ws_config = WebSocketConfig::new(cfg.ws_port);
        ws_config.allowed_origins = loopback_origins(shell_port);
        ws_config.allowed_origins.extend(cfg.extra_origins);
        let (ws, addr) = serve_websocket_with(rpc_module(hub)?, ws_config, None, UpgradeOptions::default()).await?;
        let ws_port = addr.port();
        tracing::info!("WebSocket: ws://127.0.0.1:{ws_port}");

        let router = crate::router(cfg.shell, ws_port).await;
        let (stop, stopped) = oneshot::channel::<()>();
        let shell_task = tokio::spawn(async move {
            let until = async {
                let _ = stopped.await;
            };
            if let Err(e) = crate::serve_on(listener, router, until).await {
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

/// A shell's own origins on loopback, which may always open the socket:
/// its port on 127.0.0.1 and on localhost.
pub fn loopback_origins(shell_port: u16) -> Vec<String> {
    vec![format!("http://127.0.0.1:{shell_port}"), format!("http://localhost:{shell_port}")]
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
