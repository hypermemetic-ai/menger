//! RUN-8: menger's listeners on the address `--bind` names (`MENGER_BIND`,
//! default `127.0.0.1`; `0.0.0.0`, `::` and any address of this machine,
//! IPv6 too).
//!
//! The shell binds it itself ([`crate::shell::serve_at`]) and the REST
//! bridge's config carries it (`RestHttpConfig::addr`). The WebSocket is the
//! one plexus-transport's `TransportServer` will not bind anywhere but
//! loopback (`with_websocket(port)`; TRANSPORT-7 adds the address to the
//! builder), so menger serves it through the transport's own lower-level
//! entry, `websocket::serve_websocket_with`, which takes the address — the
//! same upgrade every check included (the hosts it answers to, the origins
//! that may open it, the quotas, the login's validator, the anonymous view,
//! the credential cookie, RFC 9728's metadata) and the same accept loop —
//! and does here the two things `TransportServer::serve` adds: the
//! registry's announcement (Z2H-6, off with `PLEXUS_NO_REGISTRY=1`) and a
//! stop on SIGTERM or ctrl-c. The scaffold's base did the same
//! (plexus-starter `layers/base/backend/src/serve.rs`); when TRANSPORT-7
//! lands this module goes back to the builder.

use jsonrpsee::server::ServerHandle;
use jsonrpsee::RpcModule;
use plexus_core::plexus::PluginSchema;
use plexus_transport::registry::{self, RegistryRegistration};
use plexus_transport::websocket::serve_websocket_with;
use plexus_transport::{ConnectionQuotas, WebSocketConfig};
use std::net::{IpAddr, SocketAddr};
use tokio::task::JoinHandle;

/// `--bind`'s value: an IP address, IPv6 with or without its brackets.
pub fn parse_bind(s: &str) -> Result<IpAddr, String> {
    let t = s.trim();
    let t = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')).unwrap_or(t);
    t.parse().map_err(|_| format!("`{s}` is not an IP address (e.g. 127.0.0.1, 0.0.0.0, ::, 192.168.1.10)"))
}

/// The `Host` a client names reaching `bind` on `port` by its address —
/// `192.168.1.10:4455`, `[fd00::1]:4455` — answered beside
/// [`crate::local::listener_hosts`]' (ACCESS-25). Loopback is already
/// answered, and an unspecified address (`0.0.0.0`, `::`) is every address
/// this machine has: none is assumed, each is an `--allowed-host`.
pub fn bind_hosts(bind: IpAddr, port: u16) -> Vec<String> {
    if bind.is_loopback() || bind.is_unspecified() {
        return Vec::new();
    }
    vec![SocketAddr::new(bind, port).to_string()]
}

/// The shell's origin at a specific bound address (`http://192.168.1.10:4456`),
/// whose pages may open the socket (IDENTITY-29); none for loopback (already
/// admitted) or an unspecified address (`--allowed-origin` names them).
pub fn bind_origin(bind: IpAddr, shell_port: u16) -> Option<String> {
    (!bind.is_loopback() && !bind.is_unspecified()).then(|| format!("http://{}", SocketAddr::new(bind, shell_port)))
}

/// The host announced to the registry: the bound address when it is one
/// (Z2H-6's resolver dials it), else loopback.
pub fn advertised_host(bind: IpAddr) -> String {
    if bind.is_loopback() || bind.is_unspecified() {
        "127.0.0.1".into()
    } else {
        bind.to_string()
    }
}

/// The WebSocket: where, the names it answers to, its quotas, and the
/// login's half of the upgrade.
pub struct Socket {
    pub addr: SocketAddr,
    pub hosts: Vec<String>,
    pub quotas: ConnectionQuotas,
    pub upgrade: crate::wiring::Upgrade,
}

/// Binds and serves the WebSocket; the bound address (port 0: a free one).
pub async fn start_socket(module: RpcModule<()>, socket: Socket) -> anyhow::Result<(ServerHandle, SocketAddr)> {
    let mut config = WebSocketConfig::new(socket.addr.port()).with_allowed_hosts(socket.hosts);
    config.addr = socket.addr;
    config.allowed_origins = socket.upgrade.origins;
    config.quotas = socket.quotas;
    serve_websocket_with(module, config, socket.upgrade.validator, socket.upgrade.options)
        .await
        .map_err(|e| anyhow::anyhow!("cannot serve the WebSocket on {}: {e}", socket.addr))
}

/// Serves until the WebSocket or the REST bridge stops, or the process is
/// told to (SIGTERM, ctrl-c) — announced to the registry as `schema`'s
/// namespace at `host` meanwhile, unless `PLEXUS_NO_REGISTRY`, and
/// withdrawn on the way out. `TransportServer::serve`'s loop.
pub async fn run(
    schema: PluginSchema,
    host: String,
    ws: (ServerHandle, SocketAddr),
    rest: Option<JoinHandle<std::io::Result<()>>>,
) -> anyhow::Result<()> {
    let (handle, addr) = ws;
    let reg = (!registry::registry_disabled_by_env()).then(|| RegistryRegistration {
        url: registry::registry_url_from_env(),
        name: schema.namespace.clone(),
        host,
        port: addr.port(),
        version: schema.version.clone(),
        schema_hash: schema.hash.clone(),
    });
    let registered = reg.clone().map(|reg| tokio::spawn(async move { registry::register_with_retry(&reg).await }));
    let rest_done = async {
        match rest {
            Some(rest) => match rest.await {
                Ok(Ok(())) => tracing::info!("REST server stopped"),
                Ok(Err(e)) => tracing::error!("REST server error: {e}"),
                Err(e) => tracing::error!("REST server task failed: {e}"),
            },
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = handle.clone().stopped() => tracing::info!("WebSocket server stopped"),
        _ = rest_done => {}
        _ = shutdown_signal() => tracing::info!("shutdown signal received — stopping transports"),
    }
    let _ = handle.stop();
    if let (Some(reg), Some(registered)) = (reg, registered) {
        if registered.await.unwrap_or(false) {
            match tokio::time::timeout(std::time::Duration::from_secs(3), registry::deregister(&reg)).await {
                Ok(Ok(())) => tracing::info!("deregistered '{}' from registry at {}", reg.name, reg.url),
                Ok(Err(e)) => tracing::warn!("could not deregister '{}' from registry: {e:#}", reg.name),
                Err(_) => tracing::warn!("timed out deregistering '{}' from registry", reg.name),
            }
        }
    }
    Ok(())
}

/// ctrl-c, or SIGTERM on unix (what systemd and `docker stop` send).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_takes_v4_v6_bracketed_and_unspecified_and_refuses_names() {
        assert_eq!(parse_bind("127.0.0.1").unwrap(), IpAddr::from([127, 0, 0, 1]));
        assert!(parse_bind("0.0.0.0").unwrap().is_unspecified());
        assert!(parse_bind("::").unwrap().is_unspecified());
        assert_eq!(parse_bind("[::1]").unwrap(), parse_bind("::1").unwrap());
        assert_eq!(parse_bind(" 192.168.8.117 ").unwrap().to_string(), "192.168.8.117");
        for bad in ["localhost", "", "dream.local", "1.2.3", "127.0.0.1:4455"] {
            assert!(parse_bind(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_specific_address_is_answered_by_its_literal_loopback_and_unspecified_add_nothing() {
        let ip = |s| parse_bind(s).unwrap();
        assert_eq!(bind_hosts(ip("192.168.8.117"), 4455), ["192.168.8.117:4455"]);
        assert_eq!(bind_hosts(ip("fd00::1"), 4455), ["[fd00::1]:4455"]);
        for none in ["127.0.0.1", "::1", "0.0.0.0", "::"] {
            assert!(bind_hosts(ip(none), 4455).is_empty(), "{none}");
            assert_eq!(bind_origin(ip(none), 4456), None, "{none}");
            assert_eq!(advertised_host(ip(none)), "127.0.0.1", "{none}");
        }
        assert_eq!(bind_origin(ip("192.168.8.117"), 4456).as_deref(), Some("http://192.168.8.117:4456"));
        assert_eq!(bind_origin(ip("fd00::1"), 4456).as_deref(), Some("http://[fd00::1]:4456"));
        assert_eq!(advertised_host(ip("192.168.8.117")), "192.168.8.117");
    }
}
