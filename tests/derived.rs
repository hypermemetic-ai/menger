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
    spawn_with(db, log_dir, tag, &[]).await
}

/// A server on `db` with `extra` arguments before `--db-path` (a seed).
async fn spawn_with(db: &Path, log_dir: &Path, tag: &str, extra: &[&str]) -> Server {
    let port = free_port().await;
    let stderr_path = log_dir.join(format!("stderr-{tag}.log"));
    let child = Command::new(env!("CARGO_BIN_EXE_menger"))
        .args(["--no-shell", "--no-rest", "--port", &port.to_string()])
        .args(extra)
        .arg("--db-path")
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

/// One call, every event it answers before its stream is done.
async fn all(client: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut sub = call(client, method, params).await.expect("subscribed");
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

/// SDLC-13: how many acknowledged `set_kind`s changed a kind, across the
/// bursts.
static KINDS_CHANGED: AtomicUsize = AtomicUsize::new(0);

/// One client's share of a burst: writes of every kind, back to back, until
/// the server goes away — creates (under a parent in the shortcode, a new
/// sub-shortcode's root hosted in another, blocked by a ticket or by an id
/// not born yet), status changes into and out of `Complete`, moves, amends,
/// links and unlinks, and (SDLC-13) kind changes. Each acknowledged write counts in `acked`. The client
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
        let (method, params) = match rng.below(14) {
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
            11 => (
                "tickets.unlink_commits",
                json!({"id": target, "commits": [format!("{:07x}", 0xabc0000 + rng.below(4))]}),
            ),
            // SDLC-13: a kind changed — any of the six, with no status or one
            // (`Doc` among them); a root made work, a doc that blocks, a
            // status a kind does not take are refused in-band, and a lower
            // ticket made a scope or a doc would re-root its shortcode.
            _ => {
                let kind = ["scope", "execution", "build", "gap", "spike", "doc"][rng.below(6)];
                let status = [Value::Null, json!("Ready"), json!("Complete"), json!("Doc")][rng.below(4)].clone();
                ("tickets.set_kind", json!({"id": target, "kind": kind, "status": status}))
            }
        };
        let Some(ev) = one(&client, method, params).await else { return };
        if ev["type"] == "kind_changed" && ev["previous"] != ev["ticket"]["kind"] {
            KINDS_CHANGED.fetch_add(1, Ordering::SeqCst);
        }
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
    assert!(KINDS_CHANGED.load(Ordering::SeqCst) > 0, "a set_kind changed a kind among the writes");
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

/// FILTER-13, acceptance 2, through the binary: a nested seed's file — its
/// kept tree stored (form 5), lists past 256 children holding their
/// holders — verifies; one `tree_tallies` row, and then one `tree_holders`
/// container, altered by hand are each named — the parent, the shortcode,
/// the cell (id and what it is), what is stored and what a scan gives —
/// with exit 1; and a file in form 4 (the kept tree's tables gone) is built
/// again once on its first open, saying so, and verifies.
#[tokio::test]
async fn verify_derived_names_an_altered_tally_and_holder_and_a_form_4_file_is_built_once() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");
    let port = free_port().await;
    let stderr_path = dir.path().join("stderr-seed.log");
    let child = Command::new(env!("CARGO_BIN_EXE_menger"))
        .args(["--no-shell", "--no-rest", "--port", &port.to_string(), "--seed-synthetic", "6000", "--seed-shape", "nested", "--db-path"])
        .arg(&db)
        .env_remove("MENGER_DB_PATH")
        .env_remove("RUST_LOG")
        .env("PLEXUS_NO_REGISTRY", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(&stderr_path).unwrap()))
        .spawn()
        .expect("spawn menger");
    let s = Server { child, port, stderr_path };
    let c = connect(s.port).await;
    one(&c, "tickets.health", json!({})).await.unwrap();
    drop(c);
    s.kill9();
    let (code, out, err) = verify(&db);
    assert_eq!(code, Some(0), "{out}\n{err}");
    assert!(out.contains("tally entries") && out.contains("holder containers"), "{out}");

    let conn = rusqlite::Connection::open(&db).unwrap();
    let holders: i64 = conn.query_row("SELECT count(*) FROM tree_holders", [], |r| r.get(0)).unwrap();
    assert!(holders > 0, "a nested seed's roots keep holders");
    let (p, e, cell, held): (String, String, u32, u32) = conn
        .query_row("SELECT parent, epic, cell, held FROM tree_tallies ORDER BY parent, epic, cell LIMIT 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap();
    conn.execute(
        "UPDATE tree_tallies SET held = ?4 WHERE parent = ?1 AND epic = ?2 AND cell = ?3",
        rusqlite::params![p, e, cell, held + 5],
    )
    .unwrap();
    drop(conn);
    let (code, out, _) = verify(&db);
    assert_eq!(code, Some(1), "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with(&format!("tree tally of {p}: {e} cell {cell} (")), "{out}");
    assert!(lines[0].ends_with(&format!(": stored {}; a scan gives {held}", held + 5)), "{out}");
    assert!(lines[1].ends_with("1 difference(s) between the stored derived state and a scan"), "{out}");

    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE tree_tallies SET held = ?4 WHERE parent = ?1 AND epic = ?2 AND cell = ?3",
        rusqlite::params![p, e, cell, held],
    )
    .unwrap();
    let (hp, hk, he, hc, hi): (String, String, String, u32, u32) = conn
        .query_row(
            "SELECT parent, kids, epic, cell, hi FROM tree_holders WHERE card >= 2 AND card <= 4096 ORDER BY parent LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    conn.execute(
        "UPDATE tree_holders SET card = 1, bits = substr(bits, 1, 2) WHERE parent = ?1 AND kids = ?2 AND epic = ?3 AND cell = ?4 AND hi = ?5",
        rusqlite::params![hp, hk, he, hc, hi],
    )
    .unwrap();
    drop(conn);
    let (code, out, _) = verify(&db);
    assert_eq!(code, Some(1), "{out}");
    let first = out.lines().next().unwrap();
    assert!(first.starts_with(&format!("tree holders of {hp}'s children in {hk}: {he} cell {hc} (")), "{out}");
    assert!(first.contains(&format!("container {hi}: stored 1 members [")), "{out}");

    // Form 4: the file as the commit before wrote it — no kept tree.
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("DROP TABLE tree_tallies; DROP TABLE tree_holders; UPDATE derived_state SET form = 4;").unwrap();
    drop(conn);
    let (code, out, _) = verify(&db);
    assert_eq!(code, Some(1), "{out}");
    assert!(out.contains("stored in form 4; this build reads 6"), "{out}");
    let s = spawn_server(&db, dir.path(), "form4").await;
    let c = connect(s.port).await;
    one(&c, "tickets.health", json!({})).await.unwrap();
    let log = s.log();
    assert!(log.contains("it was stored in form 4, this build writes 6"), "{log}");
    assert!(log.contains("kept the tree over 6000 tickets"), "{log}");
    drop(c);
    s.kill9();
    let (code, out, err) = verify(&db);
    assert_eq!(code, Some(0), "{out}\n{err}");
    let s = spawn_server(&db, dir.path(), "form5").await;
    let c = connect(s.port).await;
    one(&c, "tickets.health", json!({})).await.unwrap();
    assert!(s.log().contains("the kept tree's tallies of"), "read, not built: {}", s.log());
    assert!(!s.log().contains("SCALE-28: stored"), "built once");
    drop(c);
    s.kill9();
}

/// FILTER-21, "a file in form 5 is opened", through the binary: a nested
/// seed with menger's own documents beside ordinary docs (two shortcodes'
/// board notes), its answers taken over the wire; then the file as a form-5
/// build leaves it — the cell table without its `menger` column, a board
/// note in the same cell as an ordinary doc, the derived state saying form
/// 5. `--verify-derived` says it is not this build's form; the first open
/// gains form 6 once — the column, the derived state built again, logged —
/// and answers exactly what it answered before, `menger doc` and `not menger
/// doc` included; it then verifies, and the next open reads it.
#[tokio::test]
async fn a_form_5_file_gains_form_6_once_and_answers_as_before() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");
    let s = spawn_with(&db, dir.path(), "seed", &["--seed-synthetic", "3000", "--seed-shape", "nested"]).await;
    let c = connect(s.port).await;
    for (epic, title, kind, path, parent) in [
        ("PLX", "the platform", "scope", None, None),
        ("PLX", "board: PLX", "doc", Some("_MENGER/board"), None),
        ("PLX", "a finding", "doc", None, Some("PLX-1")),
        ("PLX", "build the board", "build", None, Some("PLX-1")),
        ("PLX", "notes on notes", "doc", Some("_MENGER/notes"), None),
        ("MNG", "menger", "scope", None, None),
        ("MNG", "board: MNG", "doc", Some("_MENGER/board"), None),
        ("MNG", "another finding", "doc", None, Some("MNG-1")),
    ] {
        let ev = one(&c, "tickets.create", json!({"epic": epic, "title": title, "kind": kind, "path": path, "parent": parent})).await.unwrap();
        assert_eq!(ev["type"], "created", "{ev}");
    }
    let answers = |c: WsClient| async move {
        let mut out = Vec::new();
        for filter in [
            "",
            "menger doc",
            "not menger doc",
            "kind doc",
            "not menger doc and kind doc",
            "(menger doc or ready)",
            "status Pending and not menger doc",
            "not menger doc and under PLX-1",
        ] {
            out.push(all(&c, "tickets.counts", json!({"filter": filter})).await);
            out.push(all(&c, "tickets.list", json!({"filter": filter, "summary": true, "limit": 40})).await);
            out.push(all(&c, "tickets.list", json!({"filter": filter, "summary": true, "at": 7, "limit": 5})).await);
            out.push(all(&c, "tickets.facets", json!({"filter": filter})).await);
            out.push(all(&c, "tickets.tree", json!({"epic": "PLX", "filter": filter})).await);
            out.push(all(&c, "tickets.tree", json!({"parent": "PLX-1", "filter": filter})).await);
        }
        out
    };
    let before = answers(c).await;
    let mengers = all(&connect(s.port).await, "tickets.counts", json!({"filter": "menger doc"})).await;
    assert_eq!(
        mengers,
        [json!({"type": "count", "epic": "MNG", "held": 1}), json!({"type": "count", "epic": "PLX", "held": 2})],
        "the board notes, and only them"
    );
    s.kill9();

    // The file as a form-5 build leaves it: its derived state built with no
    // path in sight — no cell a menger doc — then the cell table as form 5
    // declared it, the paths back, and the form 5.
    let conn = rusqlite::Connection::open(&db).unwrap();
    let paths: Vec<(String, String)> = conn
        .prepare("SELECT id, path FROM tickets WHERE path IS NOT NULL ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(paths.len(), 3);
    conn.execute_batch("UPDATE tickets SET path = NULL WHERE path IS NOT NULL; UPDATE derived_state SET form = 5;").unwrap();
    drop(conn);
    let s = spawn_server(&db, dir.path(), "pathless").await;
    one(&connect(s.port).await, "tickets.health", json!({})).await.unwrap();
    s.kill9();
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch(
        "CREATE TABLE cells_form5 (
             id       INTEGER PRIMARY KEY,
             state    TEXT NOT NULL,
             kind     TEXT NOT NULL,
             severity TEXT,
             blocked  INTEGER NOT NULL,
             commits  INTEGER NOT NULL
         );
         INSERT INTO cells_form5 SELECT id, state, kind, severity, blocked, commits FROM cells;
         DROP TABLE cells;
         ALTER TABLE cells_form5 RENAME TO cells;
         UPDATE derived_state SET form = 5;",
    )
    .unwrap();
    for (id, path) in &paths {
        conn.execute("UPDATE tickets SET path = ?2 WHERE id = ?1", rusqlite::params![id, path]).unwrap();
    }
    let docs_cells = |conn: &rusqlite::Connection| -> i64 {
        conn.query_row(
            "SELECT count(*) FROM cells WHERE state = 'Doc' AND kind = 'doc' AND severity IS NULL AND blocked = 0 AND commits = 0",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(docs_cells(&conn), 1, "in form 5 a board note and an ordinary doc share one cell");
    drop(conn);
    let (code, out, _) = verify(&db);
    assert_eq!(code, Some(1), "{out}");
    assert!(out.contains("stored in form 5; this build reads 6"), "{out}");

    // The first open gains form 6, once, and answers as before.
    let s = spawn_server(&db, dir.path(), "form5").await;
    let after = answers(connect(s.port).await).await;
    let log = s.log();
    let built = log.lines().find(|l| l.contains("it was stored in form 5, this build writes 6")).unwrap_or_else(|| panic!("{log}"));
    eprintln!("form 5 → 6: {}", &built[built.find("built once").unwrap_or(0)..]);
    assert_eq!(after.len(), before.len());
    for (i, (a, b)) in after.iter().zip(&before).enumerate() {
        assert_eq!(a, b, "answer {i} after the upgrade");
    }
    s.kill9();
    let (code, out, err) = verify(&db);
    assert_eq!(code, Some(0), "{out}\n{err}");
    let conn = rusqlite::Connection::open(&db).unwrap();
    let form: i64 = conn.query_row("SELECT form FROM derived_state", [], |r| r.get(0)).unwrap();
    assert_eq!(form, 6);
    assert_eq!(docs_cells(&conn), 2, "the board notes' cell and the ordinary docs' apart");
    let menger: i64 = conn.query_row("SELECT count(*) FROM cells WHERE menger = 1", [], |r| r.get(0)).unwrap();
    assert!(menger >= 1);
    drop(conn);
    let s = spawn_server(&db, dir.path(), "form6").await;
    let again = answers(connect(s.port).await).await;
    assert_eq!(again, before, "the next open reads what the upgrade stored");
    assert!(!s.log().contains("SCALE-28: stored"), "built once: {}", s.log());
    s.kill9();
}
