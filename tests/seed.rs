//! PERF-2 synthetic seeding, black-box: spawns the built `menger` binary
//! with the seed option on an isolated database and ephemeral ports, and
//! checks what the startup log and the wire report. Every server here
//! uses its own temp location — never the user's default database.

#![cfg(unix)]


/// ACCESS-24: the test provider — every server runs `key`, every client is its `--admin`.
mod support;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// A port no other test in this binary has been handed (MNG-19). Binding
/// `:0` and dropping the listener only *suggests* a port: the kernel can
/// suggest the same one to a second test before the first test's server
/// has bound it — one server then fails to start and both tests talk to
/// the other, each seeing a store that is not its own.
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

struct Server {
    child: Child,
    port: u16,
    stderr_path: PathBuf,
}

impl Server {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.stderr_path).expect("read server stderr")
    }

    fn wait_exit(mut self, within: Duration) -> (std::process::ExitStatus, String) {
        let deadline = std::time::Instant::now() + within;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return (status, self.log());
            }
            assert!(std::time::Instant::now() < deadline, "server did not exit within {within:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_server(db: &Path, log_dir: &Path, tag: &str, args: &[&str], envs: &[(&str, &str)], port: u16) -> Server {
    let stderr_path = log_dir.join(format!("stderr-{tag}.log"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_menger"));
    cmd.args(support::login_args());
    cmd.arg("--no-shell")
        .arg("--no-rest")
        .arg("--port")
        .arg(port.to_string())
        .arg("--db-path")
        .arg(db)
        .args(args)
        .env_remove("MENGER_DB_PATH")
        .env_remove("MENGER_SEED_SYNTHETIC")
        .env_remove("MENGER_SEED_VALUE")
        .env_remove("MENGER_SEED_INTO_EXISTING")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(&stderr_path).unwrap()));
    for (k, v) in envs {
        cmd.env(k, v);
    }
    Server {
        child: cmd.spawn().expect("spawn menger"),
        port,
        stderr_path,
    }
}

async fn connect(port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..200 {
        if let Ok(c) = WsClientBuilder::default().set_headers(support::admin_headers(port)).build(&url).await {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never came up on {url}");
}

async fn call(client: &WsClient, method: &str, params: Value) -> Subscription<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    client
        .subscribe("menger.call", p, "menger.call_unsub")
        .await
        .expect("subscribe")
}

async fn one(client: &WsClient, method: &str, params: Value) -> Value {
    let mut sub = call(client, method, params).await;
    let item = sub.next().await.expect("an event").expect("clean item");
    assert_eq!(item["type"], "data", "expected data item, got {item}");
    item["content"].clone()
}

/// Criteria 1–4 through the binary: the flag seeds, the log carries
/// count + time, `health` reports the count, `ready_set` is non-empty;
/// a restart loads what was seeded (with the load time logged); seeding
/// again refuses with a clear message and leaves the database alone;
/// the env override allows it and ids keep going.
#[tokio::test]
async fn seed_flag_seeds_logs_persists_and_refuses_existing_unless_allowed() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");

    // Run 1: seed.
    let s1 = spawn_server(
        &db,
        dir.path(),
        "seed",
        &["--seed-synthetic", "300", "--seed-value", "5"],
        &[],
        free_port().await,
    );
    let c1 = connect(s1.port).await;
    let ev = one(&c1, "tickets.health", json!({})).await;
    assert_eq!(ev["ticket_count"], 300);
    let log = s1.log();
    assert!(
        log.contains("seeded 300 synthetic tickets in ") && log.contains(" ms"),
        "log reports count + time:\n{log}"
    );
    assert!(log.contains("seed value 5"), "{log}");
    let mut sub = call(&c1, "tickets.work.ready", json!({})).await;
    let first = sub.next().await.unwrap().unwrap();
    assert_eq!(first["type"], "data", "ready_set non-empty, got {first}");
    assert_eq!(first["content"]["ticket"]["status"], "Ready");
    drop(c1);
    drop(s1);

    // Run 2: plain restart loads the seeded tickets.
    let s2 = spawn_server(&db, dir.path(), "reload", &[], &[], free_port().await);
    let c2 = connect(s2.port).await;
    assert_eq!(one(&c2, "tickets.health", json!({})).await["ticket_count"], 300);
    let log = s2.log();
    assert!(log.contains("loaded 300 tickets from the database in "), "{log}");
    assert!(!log.contains("seeded"), "no seeding without the flag:\n{log}");
    drop(c2);
    drop(s2);

    // Run 3: seeding into the existing database refuses, naming the path.
    let s3 = spawn_server(
        &db,
        dir.path(),
        "refuse",
        &["--seed-synthetic", "10"],
        &[],
        free_port().await,
    );
    let (status, log) = s3.wait_exit(Duration::from_secs(10));
    assert!(!status.success(), "must refuse to start, got {status}");
    assert!(log.contains("refusing to seed 10 synthetic tickets"), "{log}");
    assert!(log.contains("already holds 300 ticket"), "{log}");
    assert!(log.contains("--seed-into-existing"), "names the override:\n{log}");

    // Run 4: the env override allows it (value `1`), same seed value —
    // the same epics come back and their numbering continues.
    let s4 = spawn_server(
        &db,
        dir.path(),
        "allowed",
        &["--seed-synthetic", "10", "--seed-value", "5"],
        &[("MENGER_SEED_INTO_EXISTING", "1")],
        free_port().await,
    );
    let c4 = connect(s4.port).await;
    assert_eq!(one(&c4, "tickets.health", json!({})).await["ticket_count"], 310);
    assert!(s4.log().contains("seeded 10 synthetic tickets"), "{}", s4.log());
}

/// Drains a finite call: every `data` item's content, up to `done`.
async fn collect(client: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut sub = call(client, method, params).await;
    let mut out = Vec::new();
    while let Some(item) = sub.next().await {
        let item = item.unwrap();
        match item["type"].as_str() {
            Some("done") => return out,
            Some("data") => out.push(item["content"].clone()),
            other => panic!("unexpected stream item type {other:?}: {item}"),
        }
    }
    panic!("stream ended without a done item");
}

/// SCALE-16 through the binary, on a seeded store: `extents` names every
/// shortcode the seed made (the log's epic count), in shortcode order,
/// and its `last_seq`s sum to `health`'s count — before and after a
/// restart, which rebuilds the sequences from the file. Acceptance 2 on
/// seeded data: for sampled rows k of every shortcode E, `list { epic: E,
/// after: "E-k", limit: 1 }` yields `E-(k+1)`.
#[tokio::test]
async fn extents_of_a_seeded_store_sum_to_its_count_and_turn_rows_into_cursors() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");
    let s = spawn_server(&db, dir.path(), "seed", &["--seed-synthetic", "3000"], &[], free_port().await);
    let c = connect(s.port).await;
    let count = one(&c, "tickets.health", json!({})).await["ticket_count"].as_u64().unwrap();
    assert_eq!(count, 3000);
    let log = s.log();
    let epics: usize = log
        .split(" epics, ")
        .next()
        .and_then(|head| head.rsplit(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("the seed logs its epic count:\n{log}"));

    let extents = collect(&c, "tickets.nav.extents", json!({})).await;
    assert_eq!(extents.len(), epics, "one per shortcode");
    assert!(extents.iter().all(|ev| ev["type"] == "extent"), "{extents:?}");
    let named: Vec<&str> = extents.iter().map(|ev| ev["epic"].as_str().unwrap()).collect();
    assert!(named.windows(2).all(|w| w[0] < w[1]), "shortcode order: {named:?}");
    let sum: u64 = extents.iter().map(|ev| ev["last_seq"].as_u64().unwrap()).sum();
    assert_eq!(sum, count);

    for ev in &extents {
        let (epic, last) = (ev["epic"].as_str().unwrap(), ev["last_seq"].as_u64().unwrap());
        for k in [1, last / 2, last - 1].into_iter().filter(|k| (1..last).contains(k)) {
            let rows = collect(
                &c,
                "tickets.list",
                json!({"epic": epic, "after": format!("{epic}-{k}"), "limit": 1, "summary": true}),
            )
            .await;
            assert_eq!(rows[0]["summary"]["id"], format!("{epic}-{}", k + 1), "{epic} after row {k}");
            assert_eq!(rows[1]["type"], "page");
        }
    }
    drop(c);
    drop(s);

    let s = spawn_server(&db, dir.path(), "reload", &[], &[], free_port().await);
    let c = connect(s.port).await;
    assert_eq!(collect(&c, "tickets.nav.extents", json!({})).await, extents, "rebuilt from the file");
}
