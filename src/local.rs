//! DIST-3: menger as a library call — the whole server, started from code.
//!
//! [`LocalServer::start`] is what the binary does with no `--auth-provider`,
//! `--no-rest` and no `--stdio`: the store opened (its directory made if
//! missing), every activation on the `menger` hub, the Plexus WebSocket and
//! the web shell (the app, `/vanilla`) on loopback — and nothing else: no
//! login, no REST bridge, no registry announcement. A host (DIST-1's
//! desktop app) runs menger without copying `main.rs`.
//!
//! SCAFFOLD-5: the serving is `plexus-shell`'s [`plexus_shell::LocalServer`]
//! — any hub, any app — and this module is menger's part of it: the store
//! and the hub, and menger's shell ([`crate::shell::app_shell`], the
//! favicon). Its API is DIST-3's, unchanged.
//!
//! A port of 0 is a free one: the shell's listener and the WebSocket's are
//! bound before anything serves, so the ports a [`LocalServer`] reports are
//! the ones it holds, with no window in which another process can take
//! them. `start` returns once both listeners are bound and serving;
//! [`LocalServer::shutdown`] stops both and releases their ports (dropping
//! the server stops them too, without waiting).
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

use crate::shell::{self, AppSource};
use std::path::PathBuf;

/// The origins a Tauri 2 webview loads its page from — plexus-shell's.
pub use plexus_shell::TAURI_ORIGINS;
/// The hub as the WebSocket (and stdio) transport serves it, and the
/// startup log of its activations — plexus-shell's.
pub use plexus_shell::{log_activations, rpc_module};

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
    /// Where the app's build is served from ([`shell::preferred_app`]: the
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
            app: shell::preferred_app(),
            extra_origins: Vec::new(),
        }
    }
}

/// A running menger: its WebSocket and its shell, on loopback.
pub struct LocalServer(plexus_shell::LocalServer);

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
        let mut app = shell::app_shell(cfg.env.as_deref(), cfg.app);
        // ACCESS-28: and the document a ticket body's frame starts as.
        app.routes = shell::favicon_routes().merge(shell::frame_routes());
        let local = plexus_shell::LocalConfig {
            ws_port: cfg.ws_port,
            shell_port: cfg.shell_port,
            extra_origins: cfg.extra_origins,
            shell: app,
        };
        // ACCESS-13: menger's own module — its calls' limits, and (login is
        // off here) the anonymous caller for a call with no identity.
        let module = crate::calls::rpc_module(hub.clone(), crate::calls::CallLimits::default())?;
        Ok(LocalServer(plexus_shell::LocalServer::start_module(hub, module, local).await?))
    }

    /// The WebSocket's port.
    pub fn ws_port(&self) -> u16 {
        self.0.ws_port()
    }

    /// The shell's port.
    pub fn shell_port(&self) -> u16 {
        self.0.shell_port()
    }

    /// The app's address, `http://127.0.0.1:<shell>/`.
    pub fn shell_url(&self) -> String {
        self.0.shell_url()
    }

    /// The Plexus WebSocket's address, `ws://127.0.0.1:<ws>`.
    pub fn ws_url(&self) -> String {
        self.0.ws_url()
    }

    /// Stops both servers and waits until their ports are released (the
    /// shell's open connections get five seconds to finish).
    pub async fn shutdown(self) {
        self.0.shutdown().await
    }
}

/// IDENTITY-29: the shell's own origins, which may always open the socket —
/// its port on 127.0.0.1 and on localhost, and the origin of the public URL
/// people reach it at, when there is one.
pub fn shell_origins(shell_port: u16, public_url: Option<&str>) -> Vec<String> {
    let mut origins = plexus_shell::loopback_origins(shell_port);
    if let Some(url) = public_url {
        let origin = crate::web_login::origin_of(url);
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }
    origins
}

/// ACCESS-25: the `Host` values a listener on loopback `port` answers to —
/// `127.0.0.1:<port>` and `localhost:<port>` always; the public URL's host
/// as it names it, and its hostname on this listener's own port (the app
/// opens `wss://<page's host>:<ws port>`), when there is one; and every
/// `--allowed-host`, as given. Anything else is refused 421 before routing
/// (DNS rebinding: a page that points its own name here names its own host).
pub fn listener_hosts(port: u16, public_url: Option<&str>, extra: &[String]) -> Vec<String> {
    let mut hosts = plexus_transport::host::loopback_hosts(port);
    let mut add = |h: String| {
        let h = h.trim().to_ascii_lowercase();
        if !h.is_empty() && !hosts.contains(&h) {
            hosts.push(h);
        }
    };
    if let Some(u) = public_url.and_then(|u| url::Url::parse(u).ok()) {
        if let Some(name) = u.host_str() {
            add(match u.port() {
                Some(p) => format!("{name}:{p}"),
                None => name.to_string(),
            });
            add(format!("{name}:{port}"));
        }
    }
    for h in extra {
        add(h.clone());
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::listener_hosts;

    #[test]
    fn a_listener_answers_loopback_on_its_port_the_public_host_and_the_extras() {
        assert_eq!(listener_hosts(4455, None, &[]), ["127.0.0.1:4455", "localhost:4455"]);
        assert_eq!(
            listener_hosts(4455, Some("https://Menger.Example/"), &["proxy.lan:8080".into()]),
            ["127.0.0.1:4455", "localhost:4455", "menger.example", "menger.example:4455", "proxy.lan:8080"]
        );
        assert_eq!(
            listener_hosts(4456, Some("http://127.0.0.1:4456"), &[]),
            ["127.0.0.1:4456", "localhost:4456"],
            "the default public URL adds nothing"
        );
        assert_eq!(
            listener_hosts(4455, Some("http://box.lan:9000"), &[]),
            ["127.0.0.1:4455", "localhost:4455", "box.lan:9000", "box.lan:4455"]
        );
    }
}
