//! TETHER-7: tether over its own socket, signed in, with substrate inside.
//!
//! One tether for the file, on its own port and data directory, wired as
//! `main` wires it: `--auth-provider key`, an operator's key as `--admin`,
//! the builder's key `editor`, a watcher's key `viewer`. substrate's
//! `claudecode` runs a stub Claude Code: one turn in stream-json, or — when
//! the prompt says SLOW — its pid written down and a long sleep, for stop.

use std::sync::{mpsc, LazyLock};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use plexus_auth_core::adapters::{key_principal, sign_key_proof};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use tether::login::{LoginConfig, StartGrant};

const OPERATOR: [u8; 32] = [0x11; 32];
const BUILDER: [u8; 32] = [0x22; 32];
const WATCHER: [u8; 32] = [0x33; 32];

fn principal_of(seed: &[u8; 32]) -> String {
    key_principal(SigningKey::from_bytes(seed).verifying_key().as_bytes()).to_string()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn nonce() -> [u8; 32] {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let mut n = [0u8; 32];
    n[..4].copy_from_slice(&N.fetch_add(1, std::sync::atomic::Ordering::Relaxed).to_le_bytes());
    n[4..8].copy_from_slice(&std::process::id().to_le_bytes());
    n[8..24].copy_from_slice(&std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos().to_le_bytes());
    n
}

struct Server {
    port: u16,
    dir: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

static SERVER: LazyLock<Server> = LazyLock::new(|| {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let stub = dir.join("claude-stub");
    std::fs::write(
        &stub,
        format!(
            concat!(
                "#!/bin/sh\n",
                "echo '{{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"stub-session\",\"model\":\"stub\",\"cwd\":\".\",\"tools\":[]}}'\n",
                "case \"$*\" in *SLOW*) echo $$ > {pids}/slow.pid; exec sleep 60;; esac\n",
                "echo '{{\"type\":\"assistant\",\"message\":{{\"id\":\"m1\",\"role\":\"assistant\",\"model\":\"stub\",\"content\":[{{\"type\":\"text\",\"text\":\"stubbed answer\"}}]}}}}'\n",
                "echo '{{\"type\":\"result\",\"subtype\":\"success\",\"session_id\":\"stub-session\",\"is_error\":false,\"result\":\"stubbed answer\"}}'\n",
            ),
            pids = dir.display(),
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert!(plexus_substrate::activations::storage::set_data_dir(dir.join("data")), "the data directory is this file's");
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let stub = stub.to_string_lossy().into_owned();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let opts = tether::Options {
                login: LoginConfig {
                    providers: vec!["key".parse().unwrap()],
                    admins: vec![principal_of(&OPERATOR)],
                    grants: vec![
                        format!("editor={}", principal_of(&BUILDER)).parse::<StartGrant>().unwrap(),
                        format!("viewer={}", principal_of(&WATCHER)).parse::<StartGrant>().unwrap(),
                    ],
                    ws_port: port,
                },
                work_dir: work,
                claude_path: Some(stub),
            };
            let (hub, login) = tether::build(opts).await.expect("tether");
            let builder = TransportServer::builder(hub, |h| DynamicHub::arc_into_rpc_module(h).map_err(|e| anyhow::anyhow!("{e}")))
                .with_websocket(port)
                .without_registry();
            let server = login.apply(builder).build().await.expect("the socket");
            tx.send(()).unwrap();
            server.serve().await.expect("serves");
        });
    });
    rx.recv_timeout(Duration::from_secs(120)).expect("tether was built");
    let up = (0..200).any(|_| {
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() || {
            std::thread::sleep(Duration::from_millis(50));
            false
        }
    });
    assert!(up, "tether listens on {port}");
    Server { port, dir, _dir: tmp }
});

fn url() -> String {
    format!("ws://127.0.0.1:{}", SERVER.port)
}

async fn signed_in(seed: &[u8; 32]) -> WsClient {
    let proof = sign_key_proof(seed, &url(), "test", &nonce(), now());
    let mut h = HeaderMap::new();
    h.insert("authorization", HeaderValue::from_str(&format!("Bearer {proof}")).unwrap());
    WsClientBuilder::default().set_headers(h).build(url()).await.expect("a key proof is let in")
}

async fn anonymous() -> WsClient {
    WsClientBuilder::default().build(url()).await.expect("the anonymous view")
}

/// `tether.call`: its data items, or the error it was refused with.
async fn call(c: &WsClient, method: &str, params: Value) -> Result<Vec<Value>, String> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut sub: Subscription<Value> = c.subscribe("tether.call", p, "tether.call_unsub").await.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(item) = tokio::time::timeout(Duration::from_secs(60), sub.next()).await.expect("an item in time") {
        let item = item.map_err(|e| e.to_string())?;
        match item["type"].as_str() {
            Some("data") => out.push(item["content"].clone()),
            Some("error") => return Err(item.to_string()),
            Some("done") => break,
            _ => {}
        }
    }
    Ok(out)
}

fn one(items: &[Value]) -> &Value {
    assert_eq!(items.len(), 1, "one answer: {items:?}");
    &items[0]
}

async fn start(c: &WsClient, ticket: &str, prompt: &str) -> Value {
    let items = call(c, "sessions.start", json!({ "ticket": ticket, "prompt": prompt, "model": "haiku" })).await.expect("start");
    let started = one(&items).clone();
    assert_eq!(started["type"], "started", "{started}");
    started["session"].clone()
}

#[tokio::test]
async fn anonymous_reaches_only_the_auth_door() {
    let c = anonymous().await;
    let refused = call(&c, "sessions.list", json!({})).await.expect_err("sign in first");
    assert!(refused.contains("-32001"), "{refused}");
    assert!(!call(&c, "auth.providers", json!({})).await.expect("the door").is_empty());
}

#[tokio::test]
async fn a_viewer_lists_and_watches_but_does_not_start() {
    let c = signed_in(&WATCHER).await;
    call(&c, "sessions.list", json!({})).await.expect("a viewer lists");
    let refused = call(&c, "sessions.start", json!({ "ticket": "MENGER-1" })).await.expect_err("an editor's");
    assert!(refused.contains("forbidden") || refused.contains("-32003"), "{refused}");
    let gone = call(&c, "sessions.watch", json!({ "name": "nobody" })).await.unwrap();
    assert_eq!(one(&gone)["type"], "not_found");
}

#[tokio::test]
async fn a_parameter_its_type_refuses_is_invalid_params() {
    let c = signed_in(&BUILDER).await;
    let refused = call(&c, "sessions.start", json!({ "ticket": "not a ticket" })).await.expect_err("typed");
    assert!(refused.contains("-32602") && refused.contains("not a ticket"), "{refused}");
    let refused = call(&c, "sessions.start", json!({ "ticket": "MENGER-1", "working_dir": "relative" })).await.expect_err("typed");
    assert!(refused.contains("-32602"), "{refused}");
}

#[tokio::test]
async fn a_session_runs_through_substrate_and_its_tail_is_watched() {
    let c = signed_in(&BUILDER).await;
    let session = start(&c, "MENGER-12", "say hello").await;
    let name = session["name"].as_str().unwrap().to_string();
    assert!(name.starts_with("menger-12-"), "{name}");
    assert_eq!(session["state"], "running");
    assert_eq!(session["started_by"], principal_of(&BUILDER));
    assert!(!session["substrate_id"].as_str().unwrap().is_empty(), "a claudecode session in the substrate");

    // The tail, from the start, to the end of the turn.
    let tail = call(&c, "sessions.watch", json!({ "name": name })).await.expect("watch");
    let ended = tail.last().expect("an end");
    assert_eq!(ended["type"], "ended", "{tail:?}");
    assert_eq!(ended["session"]["state"], "finished", "{ended}");
    assert!(
        tail.iter().any(|e| e["type"] == "line" && e["line"]["kind"] == "content" && e["line"]["text"] == "stubbed answer"),
        "the stub's answer, through claudecode: {tail:?}"
    );
    let seqs: Vec<u64> = tail.iter().filter_map(|e| e["line"]["seq"].as_u64()).collect();
    assert_eq!(seqs, (0..seqs.len() as u64).collect::<Vec<_>>(), "in order, none missing");

    // A watch from a later line answers only what follows it.
    let later = call(&c, "sessions.watch", json!({ "name": name, "since": 1 })).await.unwrap();
    assert_eq!(later.len(), tail.len() - 1);

    // list, by ticket, as a viewer too.
    let w = signed_in(&WATCHER).await;
    let listed = call(&w, "sessions.list", json!({ "ticket": "MENGER-12" })).await.unwrap();
    assert!(listed.iter().any(|e| e["session"]["name"] == name && e["session"]["state"] == "finished"), "{listed:?}");
    assert!(call(&w, "sessions.list", json!({ "ticket": "NOPE-1" })).await.unwrap().is_empty());
}

#[tokio::test]
async fn stop_ends_the_turn_and_its_claude_code() {
    let c = signed_in(&BUILDER).await;
    let session = start(&c, "TETHER-7", "SLOW work").await;
    let name = session["name"].as_str().unwrap().to_string();

    // The stub is running, and said so.
    let pid_file = SERVER.dir.join("slow.pid");
    let mut pid = None;
    for _ in 0..200 {
        if let Ok(s) = std::fs::read_to_string(&pid_file) {
            if let Ok(p) = s.trim().parse::<u32>() {
                pid = Some(p);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let pid = pid.expect("the stub started");
    let proc_dir = std::path::PathBuf::from(format!("/proc/{pid}"));
    assert!(proc_dir.exists(), "the stub runs");

    // A watcher, waiting on the live tail.
    let w = signed_in(&WATCHER).await;
    let watching = tokio::spawn({
        let name = name.clone();
        async move { call(&w, "sessions.watch", json!({ "name": name })).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let stopped = call(&c, "sessions.stop", json!({ "name": name })).await.unwrap();
    assert_eq!(one(&stopped)["type"], "stopped", "{stopped:?}");
    assert_eq!(one(&stopped)["session"]["state"], "stopped");

    let tail = watching.await.unwrap().expect("the watch ends");
    assert_eq!(tail.last().unwrap()["type"], "ended", "{tail:?}");
    assert_eq!(tail.last().unwrap()["session"]["state"], "stopped");

    // Claude Code went with the turn (a zombie reaped is gone too).
    let gone = (0..100).any(|_| {
        let state = std::fs::read_to_string(proc_dir.join("stat")).ok();
        let dead = state.as_deref().is_none_or(|s| s.split_whitespace().nth(2) == Some("Z"));
        dead || {
            std::thread::sleep(Duration::from_millis(50));
            false
        }
    });
    assert!(gone, "the stub {pid} was killed with its stream");

    // Again: already ended, said as it is; a name never started: not found.
    let again = call(&c, "sessions.stop", json!({ "name": name })).await.unwrap();
    assert_eq!(one(&again)["type"], "session");
    let none = call(&c, "sessions.stop", json!({ "name": "never-1" })).await.unwrap();
    assert_eq!(one(&none)["type"], "not_found");
}
