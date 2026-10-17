//! DIST-3: menger as a library call — the whole server, started from code.
//!
//! [`LocalServer::start`]: the store opened (its directory made if
//! missing), every activation on the `menger` hub, the Plexus WebSocket and
//! the web shell (the app, `/vanilla`) on loopback — and nothing else: no
//! login page, no REST bridge, no registry announcement. A host (DIST-1's
//! desktop app) runs menger without copying `main.rs`.
//!
//! ACCESS-24: **behind the gate, as every menger is** — there is no open
//! mode. The server has one owner, [`LocalConfig::owner`] (a principal, or
//! one made for this start), admin on `*` as an operator's `--admin` is;
//! every other caller reaches the anonymous view and nothing else. The host
//! signs its own connections in with [`LocalServer::owner_token`] — a
//! session minted for the owner, sent as `Authorization: Bearer <token>` or
//! the `access_token` cookie (a webview's: set it for `127.0.0.1` before
//! opening the shell). The upgrade admits that token and nothing else: no
//! provider's credential, since nothing here is a provider.
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
use menger_tickets_api::access::Access;
use plexus_core::plexus::AuthContext;
use plexus_transport::websocket::UpgradeOptions;
use std::path::PathBuf;
use std::sync::Arc;

/// ACCESS-24: how long the local server's sessions can last at most (its
/// session table's own lifetime; each token asks for its own, ≤ 24 h).
const SESSION_SECS: i64 = 24 * 60 * 60;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

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
    /// ACCESS-24: the owner — the principal admin on `*` here (`key:<hex>`,
    /// `idp:<uuid>`, …), whose sessions [`LocalServer::owner_token`] mints.
    /// `None`: a `key:` principal made for this start, held by no key.
    pub owner: Option<String>,
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
            owner: None,
        }
    }
}

/// A running menger: its WebSocket and its shell, on loopback, behind the
/// gate (ACCESS-24).
pub struct LocalServer {
    server: plexus_shell::LocalServer,
    access: Access,
    owner: String,
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
        // ACCESS-24: the owner is this server's one `--admin`; its sessions
        // live in a table of the server's own, and the upgrade admits them.
        let owner = match cfg.owner {
            Some(p) => p.parse::<menger_tickets_api::vocab::Principal>().map_err(|e| anyhow::anyhow!("owner: {e}"))?.as_str().to_string(),
            None => format!("key:{}", hex(&rand::random::<[u8; 32]>())),
        };
        let sessions = Arc::new(crate::sessions::Sessions::new(SESSION_SECS, &crate::login_names()));
        let table = crate::session_table(&sessions);
        let access = Access::with_login(table.clone(), vec![owner.clone()]).minting(table);
        let hub = crate::build_rpc(&cfg.db_path, access.clone()).await?;
        let mut app = shell::app_shell(cfg.env.as_deref(), cfg.app);
        // ACCESS-28: and the document a ticket body's frame starts as.
        app.routes = shell::favicon_routes().merge(shell::frame_routes());
        let local = plexus_shell::LocalConfig {
            ws_port: cfg.ws_port,
            shell_port: cfg.shell_port,
            extra_origins: cfg.extra_origins,
            shell: app,
        };
        // ACCESS-13: menger's own module — its calls' limits.
        let module = crate::calls::rpc_module(hub.clone(), crate::calls::CallLimits::default())?;
        let gate: Arc<dyn plexus_core::plexus::SessionValidator> = Arc::new(crate::sessions::Gate::new(sessions, None).noticing(crate::MengerAccess(access.clone())));
        let options = UpgradeOptions { anonymous_view: true, credential_cookies: vec!["access_token".into()], ..UpgradeOptions::default() };
        let server = plexus_shell::LocalServer::start_guarded(hub, module, local, Some(gate), options).await?;
        Ok(LocalServer { server, access, owner })
    }

    /// ACCESS-24: the owner's principal ([`LocalConfig::owner`]).
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// ACCESS-24: a session for the owner, lasting `ttl_secs` (1 s to 24 h)
    /// — what the host's connections carry, as `Authorization: Bearer
    /// <token>` or the `access_token` cookie. Mint another before it ends;
    /// a restart ends them all.
    pub fn owner_token(&self, ttl_secs: u64) -> anyhow::Result<String> {
        let meta = serde_json::json!({ "principal": self.owner, "agent": "local", "provider": "local", "name": "owner" });
        let ctx = AuthContext::new(self.owner.clone(), "local".into(), vec![], meta);
        Ok(self.access.mint_for(ctx, ttl_secs).map_err(anyhow::Error::msg)?.token)
    }

    /// The WebSocket's port.
    pub fn ws_port(&self) -> u16 {
        self.server.ws_port()
    }

    /// The shell's port.
    pub fn shell_port(&self) -> u16 {
        self.server.shell_port()
    }

    /// The app's address, `http://127.0.0.1:<shell>/`.
    pub fn shell_url(&self) -> String {
        self.server.shell_url()
    }

    /// The Plexus WebSocket's address, `ws://127.0.0.1:<ws>`.
    pub fn ws_url(&self) -> String {
        self.server.ws_url()
    }

    /// Stops both servers and waits until their ports are released (the
    /// shell's open connections get five seconds to finish).
    pub async fn shutdown(self) {
        self.server.shutdown().await
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
