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
    ask_at(std::net::Ipv4Addr::LOCALHOST.into(), port, path, host, extra).await
}

/// [`ask`], at `ip` (RUN-8: a listener bound beyond loopback).
async fn ask_at(ip: std::net::IpAddr, port: u16, path: &str, host: &str, extra: &str) -> (u16, String) {
    let mut s = tokio::net::TcpStream::connect((ip, port)).await.unwrap();
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

/// RUN-8: this machine's address on the interface that routes outward, if
/// it has one that is not loopback (a UDP socket "connected" sends nothing).
fn lan_address() -> Option<std::net::IpAddr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    Some(s.local_addr().ok()?.ip()).filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
}

/// RUN-8: the binary on three free ports with `args` (`{ws}`, `{shell}`,
/// `{rest}` replaced by the ports), once every port accepts at `reach`.
async fn spawn_at(reach: std::net::IpAddr, args: &[&str]) -> (Server, u16, u16, u16) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let (ws, shell, rest) = (free_port().await, free_port().await, free_port().await);
    let fill = |a: &str| a.replace("{ws}", &ws.to_string()).replace("{shell}", &shell.to_string()).replace("{rest}", &rest.to_string());
    let child = Command::new(env!("CARGO_BIN_EXE_menger"))
        .args(["--port", &ws.to_string(), "--shell-port", &shell.to_string(), "--rest-port", &rest.to_string()])
        .arg("--db-path")
        .arg(dir.path().join("tickets.db"))
        .args(args.iter().map(|a| fill(a)))
        .env("PLEXUS_NO_REGISTRY", "1")
        .env_remove("MENGER_DB_PATH")
        .env_remove("MENGER_ALLOWED_HOSTS")
        .env_remove("MENGER_ALLOWED_ORIGINS")
        .env_remove("MENGER_PUBLIC_URL")
        .env_remove("MENGER_BIND")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn menger");
    let server = Server(child, dir);
    for port in [ws, shell, rest] {
        let mut up = false;
        for _ in 0..200 {
            if tokio::net::TcpStream::connect((reach, port)).await.is_ok() {
                up = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(up, "port {port} never came up at {reach}");
    }
    (server, ws, shell, rest)
}

#[tokio::test]
async fn bound_to_every_address_each_port_answers_its_names_there_and_refuses_others() {
    let lan = lan_address();
    let loopback: std::net::IpAddr = std::net::Ipv4Addr::LOCALHOST.into();
    let args = ["--bind", "0.0.0.0", "--allowed-host", "box.lan:{ws}", "--allowed-host", "box.lan:{shell}", "--allowed-host", "box.lan:{rest}"];
    let (_server, ws, shell, rest) = spawn_at(lan.unwrap_or(loopback), &args).await;
    for ip in [Some(loopback), lan].into_iter().flatten() {
        for (port, path, extra, served) in [(ws, "/", UPGRADE, 101), (shell, "/vanilla", "", 200), (rest, "/debug", "", 200)] {
            let mut evil = vec!["evil.example".to_string(), format!("evil.example:{port}")];
            // An unspecified bind names no address of its own.
            evil.extend(lan.map(|lan| format!("{lan}:{port}")));
            for host in evil {
                let (code, text) = ask_at(ip, port, path, &host, extra).await;
                assert_eq!(code, 421, "{ip} {port} {host}: {text}");
            }
            for host in [format!("box.lan:{port}"), format!("127.0.0.1:{port}")] {
                let (code, text) = ask_at(ip, port, path, &host, extra).await;
                assert_eq!(code, served, "{ip} {port} {host}: {text}");
            }
        }
    }
}

#[tokio::test]
async fn bound_to_one_address_it_answers_its_literal_and_its_shell_s_pages_open_the_socket() {
    let Some(lan) = lan_address() else {
        eprintln!("no address beyond loopback here: skipped");
        return;
    };
    let bind = lan.to_string();
    let (_server, ws, shell, rest) = spawn_at(lan, &["--bind", &bind]).await;
    for (port, path, extra, served) in [(ws, "/", UPGRADE, 101), (shell, "/vanilla", "", 200), (rest, "/debug", "", 200)] {
        let (code, text) = ask_at(lan, port, path, &format!("{lan}:{port}"), extra).await;
        assert_eq!(code, served, "{port}: {text}");
        let (code, _) = ask_at(lan, port, path, "evil.example", extra).await;
        assert_eq!(code, 421, "{port}");
        // Bound to that address alone: nothing listens on loopback.
        assert!(tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err(), "{port} on loopback");
    }
    // IDENTITY-29: the shell's own origin at the address opens the socket; another does not.
    let host = format!("{lan}:{ws}");
    let ours = format!("{UPGRADE}Origin: http://{lan}:{shell}\r\n");
    assert_eq!(ask_at(lan, ws, "/", &host, &ours).await.0, 101);
    let theirs = format!("{UPGRADE}Origin: http://evil.example\r\n");
    assert_eq!(ask_at(lan, ws, "/", &host, &theirs).await.0, 403);
}

#[tokio::test]
async fn without_bind_every_port_is_loopback_only() {
    let Some(lan) = lan_address() else {
        eprintln!("no address beyond loopback here: skipped");
        return;
    };
    let (_server, ws, shell, rest) = spawn_at(std::net::Ipv4Addr::LOCALHOST.into(), &[]).await;
    for port in [ws, shell, rest] {
        assert!(tokio::net::TcpStream::connect((lan, port)).await.is_err(), "{port} reachable at {lan} with no --bind");
    }
}
