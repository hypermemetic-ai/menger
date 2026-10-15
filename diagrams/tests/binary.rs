//! COMPILE-4: the `menger-diagrams` binary, as an operator runs it — its own
//! process, its CLI, its WebSocket, never registered with a Plexus registry
//! (`PLEXUS_NO_REGISTRY=1`).
//!
//! `without_node_the_service_starts_and_says_so` deliberately runs with
//! Node hidden from `PATH`, so it runs everywhere. The others need Node and
//! jsdom and skip, saying so, without them.
#![cfg(feature = "server")]

mod support;

use menger_diagrams::{MermaidClient, MermaidEvent};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use support::{alive, children_of, connect, free_port};

/// The binary, killed when the test is done with it.
struct Service(Child);

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn service(port: u16, path: Option<&str>) -> Service {
    let mut command = Command::new(env!("CARGO_BIN_EXE_menger-diagrams"));
    command
        .args(["--port", &port.to_string()])
        .env("PLEXUS_NO_REGISTRY", "1")
        .env_remove("MENGER_DIAGRAMS_NODE")
        .env_remove("MENGER_DIAGRAMS_NODE_DIR")
        .env_remove("MENGER_DIAGRAMS_BUNDLE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(path) = path {
        command.env("PATH", path);
    }
    Service(command.spawn().expect("the binary starts"))
}

/// `PATH` with every directory holding a `node` taken out.
fn path_without_node() -> String {
    let path = std::env::var("PATH").unwrap_or_default();
    let kept: Vec<&str> = path
        .split(':')
        .filter(|dir| !std::path::Path::new(dir).join("node").exists())
        .collect();
    kept.join(":")
}

// -- criterion 4 ------------------------------------------------------------------

/// No `node` on `PATH`: the service starts and serves, and a call answers
/// one in-band `error` naming Node and how to get it — `parse`, and
/// `check` of a body with a diagram. A body with none needs no Node and
/// yields nothing.
#[tokio::test]
async fn without_node_the_service_starts_and_says_so() {
    let port = free_port().await;
    let _service = service(port, Some(&path_without_node()));
    let transport = connect(port).await;
    let client = MermaidClient::new(&transport);

    let events = client
        .parse_all("graph LR\n  A --> B".into())
        .await
        .expect("the call succeeds");
    let [MermaidEvent::Error { message }] = events.as_slice() else {
        panic!("one in-band error: {events:?}");
    };
    assert!(
        message.contains("Node.js is not installed: `node` is not on PATH"),
        "{message}"
    );
    assert!(message.contains("https://nodejs.org"), "{message}");
    assert!(message.contains("--node"), "{message}");

    let events = client
        .check_all("# T\n\n```mermaid\ngraph LR\n```\n".into())
        .await
        .unwrap();
    assert!(
        matches!(events.as_slice(), [MermaidEvent::Error { message }] if message.contains("Node.js")),
        "{events:?}"
    );
    assert!(client
        .check_all("no diagrams".into())
        .await
        .unwrap()
        .is_empty());

    // Still serving after all of that.
    assert_eq!(client.parse_all("pie".into()).await.unwrap().len(), 1);
}

// -- ACCESS-25 ----------------------------------------------------------------------

/// The status code a raw request to the service naming `host` gets.
async fn status_for_host(port: u16, host: &str) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = format!(
        "GET / HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 512];
    let n = s.read(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf[..n]).to_string();
    (text.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0), text)
}

/// Loopback only: an upgrade naming another host (DNS rebinding) is
/// refused 421; the loopback names on its port are served.
#[tokio::test]
async fn the_service_answers_only_loopback_names() {
    let port = free_port().await;
    let _service = service(port, Some(&path_without_node()));
    let _ = connect(port).await; // up
    let (code, text) = status_for_host(port, "evil.example").await;
    assert_eq!(code, 421, "{text}");
    assert!(text.contains("this server does not answer to that name"), "{text}");
    assert_eq!(status_for_host(port, &format!("evil.example:{port}")).await.0, 421);
    assert_eq!(status_for_host(port, &format!("127.0.0.1:{port}")).await.0, 101);
    assert_eq!(status_for_host(port, &format!("localhost:{port}")).await.0, 101);
}

// -- the end of the service is the end of its Node -------------------------------------

/// The service killed outright — no shutdown, no destructors — still
/// leaves no Node behind: the program sees its stdin close and exits.
#[tokio::test]
async fn killed_outright_the_service_leaves_no_node_behind() {
    needs_node!();
    let port = free_port().await;
    let mut service = service(port, None);
    let transport = connect(port).await;
    let client = MermaidClient::new(&transport);
    let events = client
        .parse_all("graph LR\n  A --> B".into())
        .await
        .unwrap();
    assert!(
        matches!(events.as_slice(), [MermaidEvent::Parsed { .. }]),
        "{events:?}"
    );

    let node = children_of(service.0.id());
    assert_eq!(
        node.len(),
        1,
        "one Node program, the service's child: {node:?}"
    );
    let node = node[0];
    assert!(alive(node));

    service.0.kill().unwrap(); // SIGKILL
    service.0.wait().unwrap();
    assert!(
        support::dies_within(node, Duration::from_secs(10)).await,
        "the Node program (pid {node}) outlived its service"
    );
}

/// `--stdio`: the transport's line-delimited JSON-RPC on stdin and stdout
/// (the subscription envelope `<hub>.call`, as on the WebSocket); stdout
/// carries nothing else — not the log, not Node's output. Closing stdin
/// ends the service, and its Node with it.
#[test]
fn stdio_serves_one_json_rpc_message_per_line() {
    needs_node!();
    let mut child = Command::new(env!("CARGO_BIN_EXE_menger-diagrams"))
        .arg("--stdio")
        .env("PLEXUS_NO_REGISTRY", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "diagrams.call",
                           "params": {"method": "mermaid.parse", "params": {"source": "graph LR\n  A --> B"}}})
    )
    .unwrap();
    let lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut items = Vec::new();
    for line in lines {
        let message: serde_json::Value =
            serde_json::from_str(&line.unwrap()).expect("every stdout line is JSON-RPC");
        if let Some(item) = message.pointer("/params/result") {
            items.push(item.clone());
            if item["type"] == "done" {
                break;
            }
        }
    }
    assert_eq!(items.len(), 2, "{items:?}");
    assert_eq!(
        items[0]["content"],
        serde_json::json!({"type": "parsed", "diagram_type": "flowchart-v2"})
    );
    let node = children_of(child.id());
    drop(stdin);
    assert!(
        child.wait().unwrap().success(),
        "stdin closed: the service ends cleanly"
    );
    for pid in node {
        assert!(!alive(pid), "its Node program (pid {pid}) went with it");
    }
}
