//! DIST-3: `menger::local` — the whole server started from code, on free
//! ports, over a database in a directory that does not exist yet; the shell
//! answers `/`, the socket answers `tickets.health` (and admits the shell's
//! own origin and the extra ones, refusing others), and a shutdown releases
//! both ports.

use jsonrpsee::core::client::SubscriptionClientT;
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::WsClientBuilder;
use menger::local::{LocalConfig, LocalServer, TAURI_ORIGINS};
use menger::shell::AppSource;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn get(port: u16, path: &str) -> String {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.expect("the shell accepts");
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n").as_bytes()).await.unwrap();
    let mut resp = Vec::new();
    s.read_to_end(&mut resp).await.unwrap();
    String::from_utf8_lossy(&resp).into_owned()
}

async fn health(url: &str, origin: Option<&str>) -> Result<Value, String> {
    let mut b = WsClientBuilder::default();
    if let Some(o) = origin {
        let mut h = jsonrpsee::ws_client::HeaderMap::new();
        h.insert("origin", o.parse().unwrap());
        b = b.set_headers(h);
    }
    let client = b.build(url).await.map_err(|e| e.to_string())?;
    let mut p = ObjectParams::new();
    p.insert("method", "tickets.health").unwrap();
    p.insert("params", serde_json::json!({})).unwrap();
    let mut sub = client.subscribe::<Value, _>("menger.call", p, "menger.call_unsub").await.map_err(|e| e.to_string())?;
    let item = sub.next().await.expect("an item").map_err(|e| e.to_string())?;
    assert_eq!(item["type"], "data", "{item}");
    Ok(item["content"].clone())
}

#[tokio::test]
async fn a_local_server_starts_on_free_ports_serves_both_and_releases_them() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let dist = dir.path().join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<title>{{TITLE}}</title><meta name=ws content=\"{{WS_PORT}}\">").unwrap();
    let db = dir.path().join("not/yet/there/tickets.db");

    let mut cfg = LocalConfig::new(&db);
    cfg.app = AppSource::Dir(dist);
    cfg.extra_origins = TAURI_ORIGINS.iter().map(|o| o.to_string()).collect();
    assert_eq!((cfg.ws_port, cfg.shell_port), (0, 0), "free ports by default");
    let server = LocalServer::start(cfg).await.expect("start");
    let (ws, shell) = (server.ws_port(), server.shell_port());
    assert!(ws != 0 && shell != 0 && ws != shell);
    assert_eq!(server.shell_url(), format!("http://127.0.0.1:{shell}/"));
    assert_eq!(server.ws_url(), format!("ws://127.0.0.1:{ws}"));
    assert!(db.parent().unwrap().is_dir(), "the database's directory is made");

    // The shell answers `/` at once — the app, stamped with the socket's port.
    let page = get(shell, "/").await;
    assert!(page.starts_with("HTTP/1.1 200") || page.starts_with("HTTP/1.0 200"), "{page}");
    assert!(page.contains(&format!("<title>menger</title><meta name=ws content=\"{ws}\">")), "{page}");
    assert!(get(shell, "/vanilla").await.contains("menger.call"));

    // The socket answers, with no Origin (a CLI), the shell's and Tauri's.
    let url = server.ws_url();
    let h = health(&url, None).await.expect("health");
    assert_eq!(h["type"], "health", "{h}");
    for origin in [format!("http://127.0.0.1:{shell}"), format!("http://localhost:{shell}"), "tauri://localhost".into()] {
        health(&url, Some(&origin)).await.unwrap_or_else(|e| panic!("{origin}: {e}"));
    }
    assert!(health(&url, Some("http://evil.example")).await.is_err(), "another origin is refused");
    assert!(db.is_file(), "the store is on disk");

    server.shutdown().await;
    for port in [ws, shell] {
        tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap_or_else(|e| panic!("port {port} released after shutdown: {e}"));
    }
}

/// Fixed ports are taken as given; a port in use is an error, not a panic.
#[tokio::test]
async fn a_port_in_use_is_an_error() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let held = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut cfg = LocalConfig::new(dir.path().join("tickets.db"));
    cfg.shell_port = held.local_addr().unwrap().port();
    assert!(LocalServer::start(cfg).await.is_err());
}
