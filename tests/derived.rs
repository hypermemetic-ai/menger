//! SCALE-28, black-box: the derived state the server stores beside the
//! tickets survives a SIGKILL at any moment — a server under a burst of
//! writes from several clients is killed, round after round, and
//! `menger --verify-derived` on the file it left (before anything reopens
//! it) finds the stored state equal to a scan; a file from before SCALE-28
//! is built once on first open and read after; and the verdict names an
//! altered number, exits non-zero, and writes nothing. Every server and
//! every file here is isolated under the target dir — never the user's
//! default database — and no server registers with a Plexus registry.

#![cfg(unix)]

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A port no other test in this binary has been handed (MNG-19).
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

/// A spawned `menger`. Dropping it SIGKILLs the child.
struct Server {
    child: Child,
    port: u16,
    stderr_path: PathBuf,
}

impl Server {
    fn kill9(mut self) {
        self.child.kill().expect("SIGKILL");
        self.child.wait().expect("reap");
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.stderr_path).expect("read server stderr")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn spawn_server(db: &Path, log_dir: &Path, tag: &str) -> Server {
    let port = free_port().await;
    let stderr_path = log_dir.join(format!("stderr-{tag}.log"));
    let child = Command::new(env!("CARGO_BIN_EXE_menger"))
        .args(["--no-shell", "--no-rest", "--port", &port.to_string(), "--db-path"])
        .arg(db)
        .env_remove("MENGER_DB_PATH")
        .env_remove("RUST_LOG")
        .env("PLEXUS_NO_REGISTRY", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(&stderr_path).unwrap()))
        .spawn()
        .expect("spawn menger");
    Server {
        child,
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
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("server never came up on {url}");
}

async fn call(client: &WsClient, method: &str, params: Value) -> Option<Subscription<Value>> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    client.subscribe("menger.call", p, "menger.call_unsub").await.ok()
}

/// One call, its first event — or `None` once the server is gone.
async fn one(client: &WsClient, method: &str, params: Value) -> Option<Value> {
    let item = call(client, method, params).await?.next().await?.ok()?;
    (item["type"] == "data").then(|| item["content"].clone())
}

/// `menger --verify-derived` on `db`: the exit status, stdout and stderr.
fn verify(db: &Path) -> (Option<i32>, String, String) {
    let Output { status, stdout, stderr } = Command::new(env!("CARGO_BIN_EXE_menger"))
        .arg("--verify-derived")
        .arg("--db-path")
        .arg(db)
        .env_remove("MENGER_DB_PATH")
        .env("RUST_LOG", "warn")
        .output()
        .expect("run menger --verify-derived");
    (
        status.code(),
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

/// The file and its write-ahead log, byte for byte.
fn bytes(db: &Path) -> Vec<Vec<u8>> {
    ["", "-wal"]
        .iter()
        .map(|suffix| std::fs::read(format!("{}{suffix}", db.display())).unwrap_or_default())
        .collect()
}

/// SplitMix64: each client's writes are reproducible per round.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % n as u64) as usize
    }
}

const EPICS: [&str; 3] = ["KA", "KB", "KC"];

/// One client's share of a burst: writes of every kind, back to back, until
/// the server goes away — creates (under a parent in the shortcode, a new
/// sub-shortcode's root hosted in another, blocked by a ticket or by an id
/// not born yet), status changes into and out of `Complete`, moves, amends,
/// links and unlinks. Each acknowledged write counts in `acked`. The client
/// names tickets it knows exist — each shortcode's highest it has seen,
/// starting from `known`.
async fn burst(port: u16, seed: u64, known: Vec<usize>, acked: Arc<AtomicUsize>) {
    let client = connect(port).await;
    let mut rng = Rng(seed);
    let mut known = known;
    loop {
        let e = rng.below(EPICS.len());
        let epic = EPICS[e];
        let any = |rng: &mut Rng| {
            let k = rng.below(EPICS.len());
            format!("{}-{}", EPICS[k], 1 + rng.below(known[k]))
        };
        let target = format!("{epic}-{}", 1 + rng.below(known[e]));
        let (method, params) = match rng.below(12) {
            0..=3 => {
                let parent = match rng.below(3) {
                    0 => Value::Null,
                    _ => json!(format!("{epic}-{}", 1 + rng.below(known[e]))),
                };
                let blocked_by = match rng.below(3) {
                    0 => json!([]),
                    1 => json!([any(&mut rng)]),
                    // Not born yet: some later create in the burst may be.
                    _ => json!([format!("{}-{}", EPICS[rng.below(EPICS.len())], known[e] + 1 + rng.below(4))]),
                };
                ("tickets.create", json!({"epic": epic, "title": "burst", "parent": parent, "blocked_by": blocked_by}))
            }
            4 => {
                // A sub-shortcode: a new prefix's root, hosted anywhere.
                let sub = format!("S{epic}{}", rng.below(6));
                ("tickets.create", json!({"epic": sub, "title": "hosted", "kind": "scope", "parent": any(&mut rng)}))
            }
            5..=6 => {
                let status = if rng.below(2) == 0 { "Complete" } else { "Ready" };
                ("tickets.set_status", json!({"id": target, "status": status}))
            }
            7..=8 => (
                "tickets.set_parent",
                json!({"id": target, "parent": format!("{epic}-{}", 1 + rng.below(known[e]))}),
            ),
            9 => ("tickets.amend", json!({"id": target, "title": format!("amended {}", rng.below(99))})),
            10 => (
                "tickets.link_commits",
                json!({"id": target, "commits": [format!("{:07x}", 0xabc0000 + rng.below(4))]}),
            ),
            _ => (
                "tickets.unlink_commits",
                json!({"id": target, "commits": [format!("{:07x}", 0xabc0000 + rng.below(4))]}),
            ),
        };
        let Some(ev) = one(&client, method, params).await else { return };
        if ev["type"] == "created" {
            if let Some(k) = EPICS.iter().position(|x| ev["ticket"]["epic"] == *x) {
                known[k] = known[k].max(ev["ticket"]["seq"].as_u64().unwrap() as usize);
            }
        }
        acked.fetch_add(1, Ordering::SeqCst);
    }
}

/// Acceptance 2: a server under a burst of mixed writes from four clients
/// is SIGKILLed — after a different number of acknowledged writes each
/// round, so the kill lands at a different instant of a different write —
/// and the file it leaves passes `--verify-derived` before anything reopens
/// it. 24 rounds on one file, each round's server reopening what the last
/// one's kill left (and reading the derived state it stored).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_killed_mid_burst_leaves_a_file_whose_derived_state_verifies() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");
    {
        let s = spawn_server(&db, dir.path(), "setup").await;
        let c = connect(s.port).await;
        for epic in EPICS {
            let ev = one(&c, "tickets.create", json!({"epic": epic, "title": "root", "kind": "scope"})).await.unwrap();
            assert_eq!(ev["type"], "created", "{ev}");
            for _ in 0..5 {
                one(&c, "tickets.create", json!({"epic": epic, "title": "seed"})).await.unwrap();
            }
        }
        s.kill9();
    }
    let mut total = 0;
    for round in 0..24u64 {
        let s = spawn_server(&db, dir.path(), &format!("round{round}")).await;
        // What every client may name: each shortcode's highest sequence.
        let known: Vec<usize> = {
            let conn = rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            EPICS
                .iter()
                .map(|e| conn.query_row("SELECT max(seq) FROM tickets WHERE epic = ?1", [e], |r| r.get::<_, i64>(0)).unwrap() as usize)
                .collect()
        };
        let acked = Arc::new(AtomicUsize::new(0));
        let clients: Vec<_> = (0..4)
            .map(|k| tokio::spawn(burst(s.port, round * 16 + k, known.clone(), Arc::clone(&acked))))
            .collect();
        let target = 3 + (round as usize * 7) % 61;
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while acked.load(Ordering::SeqCst) < target {
            assert!(std::time::Instant::now() < deadline, "round {round}: only {} acked", acked.load(Ordering::SeqCst));
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        if round > 0 {
            assert!(
                s.log().contains("read the derived state from the database"),
                "round {round}: the reopen read what the last kill left:\n{}",
                s.log()
            );
        }
        s.kill9();
        total += acked.load(Ordering::SeqCst);
        for client in clients {
            let _ = client.await;
        }
        let (code, out, err) = verify(&db);
        assert_eq!(code, Some(0), "round {round} (killed after {target}+ acknowledged writes):\n{out}\n{err}");
        assert!(out.starts_with("consistent: "), "round {round}: {out}");
    }
    assert!(total > 24 * 3, "{total} writes acknowledged across the rounds");
}

/// Acceptance 3, through the binary: a consistent file verifies (exit 0,
/// "consistent"); one stored number changed by hand is named — structure
/// and key, what is stored and what a scan gives — with exit 1; neither run
/// writes a byte; a missing file is exit 2.
#[tokio::test]
async fn verify_derived_names_an_altered_number_exits_non_zero_and_writes_nothing() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");
    let s = spawn_server(&db, dir.path(), "alter").await;
    let c = connect(s.port).await;
    for (epic, kind, parent) in [("ALT", "scope", None), ("ALT", "build", None), ("ALT", "build", Some("ALT-2"))] {
        one(&c, "tickets.create", json!({"epic": epic, "title": "t", "kind": kind, "parent": parent})).await.unwrap();
    }
    one(&c, "tickets.set_status", json!({"id": "ALT-3", "status": "Complete"})).await.unwrap();
    drop(c);
    s.kill9();

    let before = bytes(&db);
    let (code, out, _) = verify(&db);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.starts_with("consistent: ") && out.contains("3 tickets"), "{out}");
    assert_eq!(bytes(&db), before, "verifying wrote nothing");

    rusqlite::Connection::open(&db)
        .unwrap()
        .execute("UPDATE tickets SET child_count = 5 WHERE id = 'ALT-2'", [])
        .unwrap();
    let altered = bytes(&db);
    let (code, out, _) = verify(&db);
    assert_eq!(code, Some(1), "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "child_count of ALT-2: stored 5; a scan gives 1", "{out}");
    assert!(lines[1].ends_with("1 difference(s) between the stored derived state and a scan"), "{out}");
    assert_eq!(bytes(&db), altered, "verifying an altered file wrote nothing either");

    let (code, _, err) = verify(&dir.path().join("nothing-here.db"));
    assert_eq!(code, Some(2), "{err}");
    assert!(!dir.path().join("nothing-here.db").exists(), "verifying creates no file");
}

/// Acceptance 5, through the binary: every fixture written before SCALE-28
/// is reported as holding no stored state (exit 1, untouched); its first
/// open builds the state once and says so; it then verifies; and the next
/// open reads it, building nothing.
#[tokio::test]
async fn every_fixture_from_before_it_is_built_on_first_open_verifies_and_is_read_after() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in ["pre-ags13.db", "pre-ags19.db", "pre-scale4.db", "pre-sdlc7.db"] {
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let db = dir.path().join(name);
        std::fs::copy(fixtures.join(name), &db).unwrap();
        let before = bytes(&db);
        let (code, out, _) = verify(&db);
        assert_eq!(code, Some(1), "{name}: {out}");
        assert!(out.contains("written before SCALE-28"), "{name}: {out}");
        assert_eq!(bytes(&db), before, "{name}: verify did not migrate it");

        let s = spawn_server(&db, dir.path(), "first").await;
        let c = connect(s.port).await;
        one(&c, "tickets.health", json!({})).await.unwrap();
        let log = s.log();
        assert!(log.contains("SCALE-28: stored the derived state for the first time"), "{name}:\n{log}");
        drop(c);
        s.kill9();
        let (code, out, err) = verify(&db);
        assert_eq!(code, Some(0), "{name}: {out}\n{err}");

        let s = spawn_server(&db, dir.path(), "second").await;
        let c = connect(s.port).await;
        one(&c, "tickets.health", json!({})).await.unwrap();
        let log = s.log();
        assert!(!log.contains("SCALE-28: stored"), "{name}: built once:\n{log}");
        assert!(log.contains("read the derived state from the database"), "{name}:\n{log}");
        assert!(!log.contains("rolled up"), "{name}: nothing rebuilt:\n{log}");
        drop(c);
        s.kill9();
    }
}

/// FILTER-4, acceptance 3, through the binary: a file whose cells are
/// stored (form 4) verifies; one `cell_bits` container altered by hand, and
/// then one `cell_counts` row, are each named — the shortcode, the cell (id
/// and what it is) and the block, what is stored and what a scan gives —
/// with exit 1.
#[tokio::test]
async fn verify_derived_names_an_altered_cell_container_and_count() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");
    let s = spawn_server(&db, dir.path(), "cells").await;
    let c = connect(s.port).await;
    one(&c, "tickets.create", json!({"epic": "CEL", "title": "root", "kind": "scope"})).await.unwrap();
    for _ in 0..3 {
        one(&c, "tickets.create", json!({"epic": "CEL", "title": "t"})).await.unwrap();
    }
    one(&c, "tickets.link_commits", json!({"id": "CEL-3", "commits": ["abc1234"]})).await.unwrap();
    drop(c);
    s.kill9();
    let (code, out, _) = verify(&db);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("cells' ") && out.contains("bitmaps"), "{out}");

    let conn = rusqlite::Connection::open(&db).unwrap();
    let cell: u32 = conn
        .query_row("SELECT id FROM cells WHERE state = 'Pending' AND kind = 'build' AND commits = 0", [], |r| r.get(0))
        .unwrap();
    // CEL-2 and CEL-4 are that cell's; the container is made to say CEL-2.
    conn.execute("UPDATE cell_bits SET card = 1, bits = x'0200' WHERE epic = 'CEL' AND cell = ?1", [cell]).unwrap();
    drop(conn);
    let (code, out, _) = verify(&db);
    assert_eq!(code, Some(1), "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0],
        format!("cell_bits CEL cell {cell} (Pending, build, none, not blocked, no commits) container 0: stored 1 members [2]; a scan gives 2 members [2, 4]"),
        "{out}"
    );
    assert!(lines[1].ends_with("1 difference(s) between the stored derived state and a scan"), "{out}");

    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute("UPDATE cell_bits SET card = 2, bits = x'02000400' WHERE epic = 'CEL' AND cell = ?1", [cell]).unwrap();
    conn.execute("UPDATE cell_counts SET held = 9 WHERE epic = 'CEL' AND cell = ?1", [cell]).unwrap();
    drop(conn);
    let (code, out, _) = verify(&db);
    assert_eq!(code, Some(1), "{out}");
    assert_eq!(
        out.lines().next().unwrap(),
        format!("cell_counts CEL cell {cell} (Pending, build, none, not blocked, no commits): stored 9; a scan gives 2"),
        "{out}"
    );
}
