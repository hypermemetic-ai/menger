//! PERF-2 synthetic seeding, black-box: spawns the built `menger` binary
//! with the seed option on an isolated database and ephemeral ports, and
//! checks what the startup log and the wire report. Every server here
//! uses its own temp location — never the user's default database.

#![cfg(unix)]

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
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
        if let Ok(c) = WsClientBuilder::default().build(&url).await {
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
    let mut sub = call(&c1, "tickets.ready_set", json!({})).await;
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
