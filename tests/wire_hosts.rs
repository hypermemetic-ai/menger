//! ACCESS-25: every listener answers only its configured hosts. The built
//! binary on its two ports (WebSocket, shell; ACCESS-24: no REST bridge),
//! behind its test provider: a request naming
//! `Host: evil.example` — what a page that rebinds its own name to
//! 127.0.0.1 sends — is refused 421 on each before anything routes it;
//! `127.0.0.1:<port>`, `localhost:<port>`, `--public-url`'s host and an
//! `--allowed-host` are served.

#![cfg(unix)]

/// ACCESS-24: the test provider.
mod support;

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
    let (ws, shell) = (free_port().await, free_port().await);
    let child = Command::new(env!("CARGO_BIN_EXE_menger")).args(support::login_args())
        .args(["--port", &ws.to_string(), "--shell-port", &shell.to_string()])
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
    for port in [ws, shell] {
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
    let ports = [(ws, "/", UPGRADE, 101), (shell, "/login", "", 200)];
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

/// RUN-8: the binary on two free ports with `args` (`{ws}`, `{shell}`
/// replaced by the ports), once every port accepts at `reach`.
async fn spawn_at(reach: std::net::IpAddr, args: &[&str]) -> (Server, u16, u16) {
    // A port handed out is free only until something binds it: another
    // test process on this machine may take it first, and then our child
    // exits and the ports answer someone else's server. A child that is
    // not alive once its ports answer is spawned again on fresh ones.
    for _ in 0..5 {
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let (ws, shell) = (free_port().await, free_port().await);
        let fill = |a: &str| a.replace("{ws}", &ws.to_string()).replace("{shell}", &shell.to_string());
        let child = Command::new(env!("CARGO_BIN_EXE_menger")).args(support::login_args())
            .args(["--port", &ws.to_string(), "--shell-port", &shell.to_string()])
            .arg("--db-path")
            .arg(dir.path().join("tickets.db"))
            .args(args.iter().map(|a| fill(a)))
            .env("PLEXUS_NO_REGISTRY", "1")
            .env_remove("MENGER_DB_PATH")
            .env_remove("MENGER_ALLOWED_HOSTS")
            .env_remove("MENGER_ALLOWED_ORIGINS")
            .env_remove("MENGER_PUBLIC_URL")
            .env_remove("MENGER_BIND")
            .env_remove("MENGER_TRUSTED_PROXIES")
            .env_remove("MENGER_AUTH_PROVIDERS")
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn menger");
        let mut server = Server(child, dir);
        for port in [ws, shell] {
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
        tokio::time::sleep(Duration::from_millis(100)).await;
        if server.0.try_wait().unwrap().is_none() {
            return (server, ws, shell);
        }
    }
    panic!("menger never kept its ports: each was taken first");
}

#[tokio::test]
async fn bound_to_every_address_each_port_answers_its_names_there_and_refuses_others() {
    let lan = lan_address();
    let loopback: std::net::IpAddr = std::net::Ipv4Addr::LOCALHOST.into();
    // ACCESS-24/ACCESS-30: login is always on, so a server beyond loopback names its https URL.
    let args = ["--bind", "0.0.0.0", "--public-url", "https://menger.test", "--allowed-host", "box.lan:{ws}", "--allowed-host", "box.lan:{shell}"];
    let (_server, ws, shell) = spawn_at(lan.unwrap_or(loopback), &args).await;
    for ip in [Some(loopback), lan].into_iter().flatten() {
        for (port, path, extra, served) in [(ws, "/", UPGRADE, 101), (shell, "/login", "", 200)] {
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
    // ACCESS-24/ACCESS-30: login is always on — a LAN URL in plain http is served, and said.
    let public = format!("http://{lan}:{{shell}}");
    let (_server, ws, shell) = spawn_at(lan, &["--bind", &bind, "--public-url", &public]).await;
    for (port, path, extra, served) in [(ws, "/", UPGRADE, 101), (shell, "/login", "", 200)] {
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
    let (_server, ws, shell) = spawn_at(std::net::Ipv4Addr::LOCALHOST.into(), &[]).await;
    for port in [ws, shell] {
        assert!(tokio::net::TcpStream::connect((lan, port)).await.is_err(), "{port} reachable at {lan} with no --bind");
    }
}

/// TRANSPORT-7: a socket opened through a fake proxy — this test, on
/// loopback — that says it forwards `client` (`X-Forwarded-For`), held open
/// while the stream lives; the upgrade's status code.
async fn open_as(port: u16, client: Option<&str>) -> (u16, tokio::net::TcpStream) {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let xff = client.map(|c| format!("X-Forwarded-For: {c}\r\n")).unwrap_or_default();
    s.write_all(format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{UPGRADE}{xff}\r\n").as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let mut out = Vec::new();
    while !out.windows(4).any(|w| w == b"\r\n\r\n") {
        match tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => out.extend_from_slice(&buf[..n]),
            _ => break,
        }
    }
    if !out.starts_with(b"HTTP/1.1 101") {
        if let Ok(Ok(n)) = tokio::time::timeout(Duration::from_millis(300), s.read(&mut buf)).await {
            out.extend_from_slice(&buf[..n]);
        }
        // Shown when a test fails: which quota refused, and for whom.
        eprintln!("{port} as {client:?}: {}", String::from_utf8_lossy(&out));
    }
    let text = String::from_utf8_lossy(&out).to_string();
    (text.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0), s)
}

/// TRANSPORT-7: the codes of `n` sockets opened as `client`, all held.
async fn open_n(port: u16, client: Option<&str>, n: usize, held: &mut Vec<tokio::net::TcpStream>) -> Vec<u16> {
    let mut codes = Vec::new();
    for _ in 0..n {
        let (code, s) = open_as(port, client).await;
        codes.push(code);
        held.push(s);
    }
    codes
}

/// TRANSPORT-7: behind a `--trusted-proxy`, AUTH-12's per-IP quota and,
/// (ACCESS-24: login always on) its anonymous-per-IP quota count the client the proxy
/// forwards: two forwarded clients each get their own two places, and the
/// proxy's own socket (no header) is its own.
#[tokio::test]
async fn behind_a_trusted_proxy_each_forwarded_client_has_its_own_quota() {
    for quota in [
        &["--max-connections-per-ip", "2"][..],
        &["--max-anonymous-connections-per-ip", "2"][..],
    ] {
        let mut args = vec!["--trusted-proxy", "127.0.0.1"];
        args.extend_from_slice(quota);
        let (_server, ws, _) = spawn_at(std::net::Ipv4Addr::LOCALHOST.into(), &args).await;
        let mut held = Vec::new();
        assert_eq!(open_n(ws, Some("203.0.113.1"), 3, &mut held).await, [101, 101, 429], "{quota:?}: the first client");
        assert_eq!(open_n(ws, Some("203.0.113.2"), 2, &mut held).await, [101, 101], "{quota:?}: the second client");
        // The rightmost hop that is not a proxy is the client: a hop the
        // first client prepends does not move it.
        assert_eq!(open_n(ws, Some("198.51.100.9, 203.0.113.1"), 1, &mut held).await, [429], "{quota:?}: prepended");
        assert_eq!(open_n(ws, None, 1, &mut held).await, [101], "{quota:?}: the proxy itself");
        // A closed socket gives its client's place back.
        held.remove(0);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(open_n(ws, Some("203.0.113.1"), 1, &mut held).await, [101], "{quota:?}: a place given back");
    }
}

/// TRANSPORT-7: a forwarded header from a peer that is not a trusted proxy
/// is ignored — whatever it names, every socket is the peer's — with a
/// proxy named elsewhere, with `none`, and with nothing said.
#[tokio::test]
async fn a_forged_forwarded_header_from_an_untrusted_peer_is_ignored() {
    for proxies in [&["--trusted-proxy", "10.9.9.9"][..], &["--trusted-proxy", "none"][..], &[][..]] {
        for quota in [
            &["--max-connections-per-ip", "2"][..],
            &["--max-anonymous-connections-per-ip", "2"][..],
        ] {
            let mut args = proxies.to_vec();
            args.extend_from_slice(quota);
            let (_server, ws, _) = spawn_at(std::net::Ipv4Addr::LOCALHOST.into(), &args).await;
            let mut held = Vec::new();
            assert_eq!(open_n(ws, Some("203.0.113.1"), 1, &mut held).await, [101], "{args:?}");
            assert_eq!(open_n(ws, Some("203.0.113.2"), 1, &mut held).await, [101], "{args:?}");
            assert_eq!(open_n(ws, Some("203.0.113.3"), 1, &mut held).await, [429], "{args:?}: one peer, one quota");
            assert_eq!(open_n(ws, None, 1, &mut held).await, [429], "{args:?}");
        }
    }
}
