//! POLYGLOT-5 — `menger --child NAME=stdio:CMD`: an activation in another
//! process mounted under NAME (plexus-core's `RemoteChild`, flags from its
//! `ChildArgs`), behind menger's gate like any activation. The child is
//! plexus-wire's reference child, `conformance/examples/stub_child.py`.
//! V2-44: main's test, on v2 — beside v2's services.

#[path = "support/ports.rs"]
mod ports;
mod support;

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use support::{principal_of, proof_of, ADMIN};

/// A key that holds nothing.
const STRANGER: [u8; 32] = [0x11; 32];

fn stub() -> PathBuf {
    let p = std::env::var_os("PLEXUS_STUB_CHILD").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plexus-wire/conformance/examples/stub_child.py")
    });
    assert!(p.exists(), "the stub child is not at {} (a plexus-wire checkout beside menger, or PLEXUS_STUB_CHILD)", p.display());
    p
}

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    ports::free_port()
}

fn menger(db: &Path, port: u16, extra: &[String]) -> Command {
    // `MENGER_BIN`: a copy of this build's binary — the shared target's
    // `debug/menger` is another worktree's the moment it builds.
    let bin = std::env::var_os("MENGER_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_menger")));
    let mut cmd = Command::new(bin);
    cmd.args(["--port", &port.to_string(), "--no-shell", "--db-path"])
        .arg(db)
        .args(["--auth-provider", "key", "--admin", &principal_of(&ADMIN)])
        .args(extra)
        .env("PLEXUS_NO_REGISTRY", "1")
        .env_remove("MENGER_DB_PATH")
        .env_remove("MENGER_AUTH_PROVIDERS")
        .env_remove("MENGER_ADMINS")
        .env_remove("PLEXUS_CHILDREN")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    cmd
}

async fn spawn(db: &Path, extra: &[String]) -> Server {
    let port = free_port();
    let mut server = Server { child: menger(db, port, extra).stderr(Stdio::null()).spawn().expect("spawn menger"), port };
    for _ in 0..300 {
        assert!(server.child.try_wait().unwrap().is_none(), "menger exited at start");
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return server;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let _ = server.child.kill();
    panic!("menger never listened on {port}");
}

async fn connect_as(seed: &[u8; 32], port: u16) -> WsClient {
    let mut h = HeaderMap::new();
    h.insert("authorization", HeaderValue::from_str(&format!("Bearer {}", proof_of(seed, port))).unwrap());
    WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{port}")).await.expect("signed in")
}

/// `menger.call {method, params}`: every item, to the end.
async fn call(c: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let mut out = vec![];
    while let Ok(Some(item)) = tokio::time::timeout(Duration::from_secs(10), s.next()).await {
        let item = item.unwrap();
        let end = matches!(item["type"].as_str(), Some("done") | Some("error"));
        out.push(item);
        if end {
            break;
        }
    }
    out
}

fn child_flag(log: &Path) -> Vec<String> {
    vec!["--child".into(), format!("echo=stdio:python3 -I {} --stdio --log {}", stub().display(), log.display())]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_flag_mounts_and_gates() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("child.log");
    let server = spawn(&dir.path().join("t.db"), &child_flag(&log)).await;
    let admin = connect_as(&ADMIN, server.port).await;
    let said = call(&admin, "echo.echo", json!({ "text": "hi", "times": 2 })).await;
    let kinds: Vec<_> = said.iter().map(|i| i["type"].as_str().unwrap().to_string()).collect();
    assert_eq!(kinds, ["data", "data", "done"], "{said:?}");
    assert_eq!(said[0]["content"], json!({ "type": "said", "text": "hi" }));
    // The hop names the service and the caller; never a credential.
    let hop = call(&admin, "echo.whoami", json!({})).await;
    assert_eq!(hop[0]["content"]["hop"]["principal"], "service:menger");
    assert_eq!(hop[0]["content"]["hop"]["for"], json!([principal_of(&ADMIN)]));
    // In menger's schema, under its mount name.
    let schema = call(&admin, "echo.schema", json!({})).await;
    assert_eq!(schema[0]["content"]["namespace"], "echo");
    // A key holding no grant: menger's gate refuses it; the child never hears of it.
    let stranger = connect_as(&STRANGER, server.port).await;
    let refused = call(&stranger, "echo.echo", json!({ "text": "never-sent-5c1e", "times": 1 })).await;
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!((refused[0]["error_kind"].clone(), refused[0]["grant"].clone()), (json!("forbidden"), json!("echo.echo")), "{refused:?}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let heard = std::fs::read_to_string(&log).unwrap();
    assert!(!heard.contains("never-sent-5c1e"), "the child heard a refused call");
    for secret in ["mgs_", "plexus-key.", "Bearer"] {
        assert!(!heard.contains(secret), "{secret} reached the child");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_child_no_mount() {
    let dir = tempfile::tempdir().unwrap();
    let server = spawn(&dir.path().join("t.db"), &[]).await;
    let admin = connect_as(&ADMIN, server.port).await;
    let e = call(&admin, "echo.echo", json!({ "text": "x", "times": 1 })).await;
    assert_eq!(e[0]["type"], "error", "{e:?}");
    let schema = call(&admin, "menger.schema", json!({})).await;
    assert!(!schema.iter().any(|i| i.to_string().contains("\"namespace\":\"echo\"")));
}

/// A child that does not answer, or does not read, stops the start: exit 2,
/// named; one named like menger's own activation is refused too.
#[test]
fn a_silent_child_refuses_the_start() {
    let dir = tempfile::tempdir().unwrap();
    for (args, code, says) in [
        (vec!["--child".to_string(), "mute=stdio:sleep 30".into(), "--child-timeout".into(), "300ms".into()], Some(2), "mute"),
        (vec!["--child".to_string(), format!("tickets=stdio:python3 -I {} --stdio", stub().display())], Some(1), "one of menger's own activations"),
        // V2-44: v2's own services are menger's too (media, docs, quota, ui).
        (vec!["--child".to_string(), format!("quota=stdio:python3 -I {} --stdio", stub().display())], Some(1), "`quota` is one of menger's own activations"),
        (vec!["--child".to_string(), "a.b=stdio:x".into()], Some(2), "dotted"),
    ] {
        let mut child = menger(&dir.path().join("t.db"), free_port(), &args).stderr(Stdio::piped()).spawn().unwrap();
        let mut ended = None;
        for _ in 0..200 {
            if let Some(s) = child.try_wait().unwrap() {
                ended = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if ended.is_none() {
            let _ = child.kill();
        }
        let out = child.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), code, "{args:?}: {err}");
        assert!(err.contains(says), "{args:?}: {err}");
    }
}
