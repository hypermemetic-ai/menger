//! ACCESS-25: every listener answers only its configured hosts. The built
//! binary on its three ports (WebSocket, shell, REST): a request naming
//! `Host: evil.example` — what a page that rebinds its own name to
//! 127.0.0.1 sends — is refused 421 on each before anything routes it;
//! `127.0.0.1:<port>`, `localhost:<port>`, `--public-url`'s host and an
//! `--allowed-host` are served.

#![cfg(unix)]

use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn free_port() -> u16 {
    static HANDED: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut handed = HANDED.lock().unwrap();
        if !handed.contains(&port) {
            handed.push(port);
            return port;
        }
    }
}

/// The child, killed when done; its database's directory, removed after.
struct Server(Child, #[allow(dead_code)] tempfile::TempDir);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const UPGRADE: &str = "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n";

/// The status code and the head of the answer to `GET path` naming `host`.
async fn ask(port: u16, path: &str, host: &str, extra: &str) -> (u16, String) {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut buf = vec![0u8; 4096];
    let mut out = Vec::new();
    // An upgrade answered 101 stays open: read what came, not to the end.
    loop {
        match tokio::time::timeout(Duration::from_millis(500), s.read(&mut buf)).await {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(n)) => out.extend_from_slice(&buf[..n]),
            Ok(Err(_)) => break,
        }
        if out.windows(4).any(|w| w == b"\r\n\r\n") && !out.starts_with(b"HTTP/1.1 421") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&out).to_string();
    (text.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0), text)
}

#[tokio::test]
async fn each_port_refuses_another_host_before_routing() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let (ws, shell, rest) = (free_port().await, free_port().await, free_port().await);
    let child = Command::new(env!("CARGO_BIN_EXE_menger"))
        .args(["--port", &ws.to_string(), "--shell-port", &shell.to_string(), "--rest-port", &rest.to_string()])
        .arg("--db-path")
        .arg(dir.path().join("tickets.db"))
        .args(["--public-url", "https://menger.example", "--allowed-host", "Proxy.Lan:8080"])
        .env("PLEXUS_NO_REGISTRY", "1")
        .env_remove("MENGER_DB_PATH")
        .env_remove("MENGER_ALLOWED_HOSTS")
        .env_remove("MENGER_PUBLIC_URL")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn menger");
    let _server = Server(child, dir);
    for port in [ws, shell, rest] {
        let mut up = false;
        for _ in 0..200 {
            if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                up = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(up, "port {port} never came up");
    }

    // (port, a path it serves, the request's extra headers, what served looks like)
    let ports = [(ws, "/", UPGRADE, 101), (shell, "/vanilla", "", 200), (rest, "/debug", "", 200)];
    for (port, path, extra, served) in ports {
        for evil in ["evil.example", &format!("evil.example:{port}"), "127.0.0.1:1"] {
            let (code, text) = ask(port, path, evil, extra).await;
            assert_eq!(code, 421, "{port} {evil}: {text}");
            assert!(text.contains("this server does not answer to that name"), "{text}");
        }
        for ours in [
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
            "menger.example".to_string(),
            format!("menger.example:{port}"),
            "proxy.lan:8080".to_string(),
        ] {
            let (code, text) = ask(port, path, &ours, extra).await;
            assert_eq!(code, served, "{port} {ours}: {text}");
        }
    }
}
