//! MNG-2 durability, black-box: spawns the built `menger` binary, speaks
//! the real wire protocol, kills it with SIGKILL, restarts it on the same
//! database, and asserts what survived. Every test uses its own isolated
//! location under the target dir — never the user's default.

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

/// A spawned `menger` process. Dropping it SIGKILLs the child so a failed
/// assertion never leaks a server between runs.
struct Server {
    child: Child,
    port: u16,
    stderr_path: PathBuf,
}

impl Server {
    /// The point of the exercise: SIGKILL, no chance to flush anything.
    fn kill9(mut self) {
        self.child.kill().expect("SIGKILL");
        self.child.wait().expect("reap");
    }

    fn startup_output(&self) -> String {
        std::fs::read_to_string(&self.stderr_path).expect("read server stderr")
    }

    /// Waits (bounded) for the process to exit on its own — for a server
    /// that must refuse to start — returning its exit status and stderr.
    fn wait_exit(mut self, within: Duration) -> (std::process::ExitStatus, String) {
        let deadline = std::time::Instant::now() + within;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return (status, self.startup_output());
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

/// Spawns the built binary. `db_path: None` means no override — the
/// server must fall back to its per-user default (the test then points
/// that default somewhere hermetic via `envs`, e.g. XDG_DATA_HOME).
async fn spawn_server(
    db_path: Option<&Path>,
    envs: &[(&str, &str)],
    log_dir: &Path,
    tag: &str,
) -> Server {
    let port = free_port().await;
    let stderr_path = log_dir.join(format!("stderr-{tag}.log"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_menger"));
    cmd.arg("--no-shell")
        // No REST here: with it enabled, every spawned server would bind
        // the fixed default 4457 and parallel tests would collide on it.
        // (tests/rest.rs exercises REST, on a free port per server.)
        .arg("--no-rest")
        .arg("--port")
        .arg(port.to_string())
        // Hermetic: never inherit an override or a filter from the
        // developer's environment.
        .env_remove("MENGER_DB_PATH")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(&stderr_path).unwrap()));
    if let Some(db) = db_path {
        cmd.arg("--db-path").arg(db);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let child = cmd.spawn().expect("spawn menger");
    Server {
        child,
        port,
        stderr_path,
    }
}

async fn connect(port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..100 {
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

/// One call, one event — the pattern every mutation and `get` follows.
async fn one(client: &WsClient, method: &str, params: Value) -> Value {
    let mut sub = call(client, method, params).await;
    let item = sub.next().await.expect("an event").expect("clean item");
    assert_eq!(item["type"], "data", "expected data item, got {item}");
    item["content"].clone()
}

/// Drains a finite call: collects every `data` item's content until `done`.
async fn drain(client: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut sub = call(client, method, params).await;
    let mut out = Vec::new();
    while let Some(item) = sub.next().await {
        let item = item.expect("clean item");
        match item["type"].as_str() {
            Some("done") => return out,
            Some("data") => out.push(item["content"].clone()),
            other => panic!("unexpected stream item type {other:?}: {item}"),
        }
    }
    panic!("stream ended without a done item");
}

/// Criteria 1, 2, 4: acknowledged mutations survive SIGKILL with every
/// field intact, the startup output reports the loaded count, and id
/// allocation continues from the persisted maximum.
#[tokio::test]
async fn acknowledged_mutations_survive_sigkill() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");

    // Run 1: create two tickets, flip one, and capture the acked state.
    let s1 = spawn_server(Some(&db), &[], dir.path(), "run1").await;
    let c1 = connect(s1.port).await;

    let ev = one(
        &c1,
        "tickets.create",
        json!({
            "epic": "DUR",
            "title": "first — survives ✓",
            "kind": "spike",
            "severity": "Critical",
            "blocked_by": ["DUR-0"],
            "body": "## Problem\n\nmultiline body"
        }),
    )
    .await;
    assert_eq!(ev["type"], "created");
    assert_eq!(ev["ticket"]["id"], "DUR-1");
    assert_eq!(ev["ticket"]["kind"], "spike");

    let ev = one(
        &c1,
        "tickets.create",
        json!({"epic": "DUR", "title": "second"}),
    )
    .await;
    assert_eq!(ev["type"], "created");
    assert_eq!(ev["ticket"]["id"], "DUR-2");
    let dur2_acked = ev["ticket"].clone();

    // Let the clock tick so the updated timestamp is distinguishable.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let ev = one(
        &c1,
        "tickets.set_status",
        json!({"id": "DUR-1", "status": "Ready"}),
    )
    .await;
    assert_eq!(ev["type"], "status_changed");
    assert_eq!(ev["previous"], "Pending");
    let dur1_acked = ev["ticket"].clone();
    assert!(
        dur1_acked["updated_unix"].as_i64() > dur1_acked["created_unix"].as_i64(),
        "status flip bumped updated_unix"
    );

    // The moment after the acks: SIGKILL.
    drop(c1);
    s1.kill9();

    // Run 2: same database. Everything acked is back, fields identical.
    let s2 = spawn_server(Some(&db), &[], dir.path(), "run2").await;
    let c2 = connect(s2.port).await;

    let log = s2.startup_output();
    assert!(
        log.contains("loaded 2 tickets"),
        "startup reports the loaded count, got:\n{log}"
    );

    let mut sub = call(&c2, "tickets.list", json!({"epic": "DUR"})).await;
    let first = sub.next().await.unwrap().unwrap()["content"].clone();
    let second = sub.next().await.unwrap().unwrap()["content"].clone();
    assert_eq!(first["type"], "ticket");
    assert_eq!(
        first["ticket"], dur1_acked,
        "DUR-1 identical to its last acknowledged state (criterion 4)"
    );
    assert_eq!(
        second["ticket"], dur2_acked,
        "DUR-2 identical to its acknowledged state (criterion 1)"
    );

    let ev = one(&c2, "tickets.get", json!({"id": "DUR-1"})).await;
    assert_eq!(ev["ticket"]["status"], "Ready");
    assert_eq!(ev["ticket"]["updated_unix"], dur1_acked["updated_unix"]);

    // Criterion 2: the next create continues the sequence, not restarts it.
    let ev = one(&c2, "tickets.create", json!({"epic": "DUR", "title": "third"})).await;
    assert_eq!(ev["type"], "created");
    assert_eq!(ev["ticket"]["id"], "DUR-3");
    assert_eq!(ev["ticket"]["seq"], 3);
}

/// The MNG-2 × MNG-4 intersection: acknowledged DAG maintenance is
/// durable. `create B blocked_by [A]` commits B and A's gained `unlocks`
/// edge in one transaction — after SIGKILL both edges are back,
/// byte-identical, and drive the ready set.
#[tokio::test]
async fn dag_edges_survive_sigkill() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");

    let s1 = spawn_server(Some(&db), &[], dir.path(), "dag1").await;
    let c1 = connect(s1.port).await;

    let ev = one(&c1, "tickets.create", json!({"epic": "EDGE", "title": "A"})).await;
    assert_eq!(ev["ticket"]["id"], "EDGE-1");
    let ev = one(
        &c1,
        "tickets.create",
        json!({"epic": "EDGE", "title": "B", "blocked_by": ["EDGE-1"]}),
    )
    .await;
    assert_eq!(ev["ticket"]["id"], "EDGE-2");

    // The acknowledged symmetry: A's unlocks gained B.
    let a_acked = one(&c1, "tickets.get", json!({"id": "EDGE-1"})).await["ticket"].clone();
    assert_eq!(a_acked["unlocks"], json!(["EDGE-2"]));

    drop(c1);
    s1.kill9();

    // Restart: both directions of the edge are back, A byte-identical.
    let s2 = spawn_server(Some(&db), &[], dir.path(), "dag2").await;
    let c2 = connect(s2.port).await;
    let a = one(&c2, "tickets.get", json!({"id": "EDGE-1"})).await["ticket"].clone();
    assert_eq!(a, a_acked, "A (with its unlocks edge) survived SIGKILL");
    let b = one(&c2, "tickets.get", json!({"id": "EDGE-2"})).await["ticket"].clone();
    assert_eq!(b["blocked_by"], json!(["EDGE-1"]));

    // The persisted edges drive the ready set after the restart.
    let ev = one(
        &c2,
        "tickets.set_status",
        json!({"id": "EDGE-2", "status": "Ready"}),
    )
    .await;
    assert_eq!(ev["type"], "status_changed");
    let ready = drain(&c2, "tickets.ready_set", json!({"epic": "EDGE"})).await;
    assert!(ready.is_empty(), "B is blocked by incomplete A, got {ready:?}");

    one(
        &c2,
        "tickets.set_status",
        json!({"id": "EDGE-1", "status": "Complete"}),
    )
    .await;
    let ready = drain(&c2, "tickets.ready_set", json!({"epic": "EDGE"})).await;
    assert_eq!(ready.len(), 1, "completing A unblocks B, got {ready:?}");
    assert_eq!(ready[0]["ticket"]["id"], "EDGE-2");
}

/// AGS-13 criterion 1, black-box: `tests/fixtures/pre-ags13.db` was
/// written by the pre-AGS-13 binary (schema column `ticket_type`) and
/// holds every legacy value — epic, implementation, analysis, checkpoint,
/// spike — plus a default-typed row, DAG edges, and three statuses. On
/// first open the server migrates it durably: `list` shows the pinned
/// kinds and no `ticket_type`; after SIGKILL and a restart the kinds are
/// still there, the file's column is `kind`, and no second migration ran.
#[tokio::test]
async fn legacy_database_migrates_on_first_open_and_survives_sigkill() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pre-ags13.db"),
        &db,
    )
    .unwrap();

    let expected = [
        ("LEG-1", "scope"),     // epic
        ("LEG-2", "build"),     // implementation
        ("LEG-3", "spike"),     // analysis
        ("LEG-4", "execution"), // checkpoint
        ("LEG-5", "spike"),     // spike
        ("OLD-1", "build"),     // the old default
    ];
    let check = |tickets: &[Value], when: &str| {
        assert_eq!(tickets.len(), expected.len(), "{when}: {tickets:?}");
        for (t, (id, kind)) in tickets.iter().zip(expected) {
            assert_eq!(t["ticket"]["id"], id, "{when}");
            assert_eq!(t["ticket"]["kind"], kind, "{when}: {id}");
            assert!(
                t["ticket"].get("ticket_type").is_none(),
                "{when}: {id} still carries ticket_type: {t}"
            );
        }
    };

    // Run 1: first open of the old database.
    let s1 = spawn_server(Some(&db), &[], dir.path(), "legacy1").await;
    let c1 = connect(s1.port).await;
    let log = s1.startup_output();
    assert!(
        log.contains("migrated 6 tickets: ticket_type -> kind"),
        "startup reports the migration, got:\n{log}"
    );
    let tickets = drain(&c1, "tickets.list", json!({})).await;
    check(&tickets, "after first open");
    // Everything else survived the rewrite: statuses, DAG edges, bodies.
    assert_eq!(tickets[0]["ticket"]["status"], "Epic");
    assert_eq!(tickets[1]["ticket"]["status"], "Ready");
    assert_eq!(tickets[1]["ticket"]["unlocks"], json!(["LEG-4"]));
    assert_eq!(tickets[3]["ticket"]["blocked_by"], json!(["LEG-2", "LEG-3"]));
    assert!(
        tickets[0]["ticket"]["body"].as_str().unwrap().contains("type: epic"),
        "body frontmatter is the raw record, untouched: {}",
        tickets[0]["ticket"]["body"]
    );

    drop(c1);
    s1.kill9();

    // Run 2: the migration was durable — the file holds kinds now.
    let s2 = spawn_server(Some(&db), &[], dir.path(), "legacy2").await;
    let c2 = connect(s2.port).await;
    let log = s2.startup_output();
    assert!(!log.contains("migrated"), "no second migration, got:\n{log}");
    assert!(log.contains("loaded 6 tickets"), "got:\n{log}");
    check(&drain(&c2, "tickets.list", json!({})).await, "after SIGKILL + restart");
    let ev = one(&c2, "tickets.get", json!({"id": "LEG-4"})).await;
    assert_eq!(ev["ticket"]["kind"], "execution");

    // And the system of record itself: column `kind`, no `ticket_type`.
    let conn = rusqlite::Connection::open(&db).unwrap();
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(tickets)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(cols.contains(&"kind".into()), "{cols:?}");
    assert!(!cols.contains(&"ticket_type".into()), "{cols:?}");
    let stored: Vec<String> = conn
        .prepare("SELECT kind FROM tickets ORDER BY epic, seq")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(stored, ["scope", "build", "spike", "execution", "spike", "build"]);
}

/// AGS-19 criterion 3, black-box: `tests/fixtures/pre-ags19.db` was
/// written by the pre-AGS-19 binary (no `parent` column) and holds bodies
/// whose frontmatter carries `parent:` — one resolvable pair, a
/// three-deep chain, one naming a ticket that does not exist, and one
/// body with no frontmatter at all. On first open the server backfills
/// the typed parents durably, logging one line per ticket; the
/// unresolvable one is logged and left empty, never fatal. After SIGKILL
/// and a restart the parents are still there and no second migration
/// runs.
#[tokio::test]
async fn pre_ags19_database_backfills_parents_on_first_open_and_survives_sigkill() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pre-ags19.db"),
        &db,
    )
    .unwrap();

    // The fixture predates the column entirely.
    let cols: Vec<String> = rusqlite::Connection::open(&db)
        .unwrap()
        .prepare("PRAGMA table_info(tickets)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(!cols.contains(&"parent".into()), "{cols:?}");

    // id, backfilled parent, children.
    let expected: [(&str, Value, Value); 5] = [
        ("PRE-1", Value::Null, json!(["PRE-2"])),
        ("PRE-2", json!("PRE-1"), json!(["PRE-4"])),
        ("PRE-3", Value::Null, json!([])), // frontmatter names PRE-99
        ("PRE-4", json!("PRE-2"), json!([])),
        ("PRE-5", Value::Null, json!([])), // no frontmatter at all
    ];
    let check = |tickets: &[Value], when: &str| {
        assert_eq!(tickets.len(), expected.len(), "{when}: {tickets:?}");
        for (t, (id, parent, children)) in tickets.iter().zip(&expected) {
            let t = &t["ticket"];
            assert_eq!(t["id"], *id, "{when}");
            assert_eq!(t["parent"], *parent, "{when}: {id} parent");
            assert_eq!(t["children"], *children, "{when}: {id} children");
        }
    };

    // Run 1: first open of the pre-AGS-19 database.
    let s1 = spawn_server(Some(&db), &[], dir.path(), "parent1").await;
    let c1 = connect(s1.port).await;
    let log = s1.startup_output();
    assert!(
        log.contains("PRE-2.parent = PRE-1 (from body frontmatter)"),
        "one log line per backfilled ticket, got:\n{log}"
    );
    assert!(
        log.contains("PRE-4.parent = PRE-2 (from body frontmatter)"),
        "got:\n{log}"
    );
    assert!(
        log.contains("PRE-3 names parent 'PRE-99'") && log.contains("left empty"),
        "the unresolvable one is logged and left empty, got:\n{log}"
    );
    assert!(
        log.contains("backfilling 2 typed parent(s)"),
        "the summary counts them, got:\n{log}"
    );
    check(&drain(&c1, "tickets.list", json!({})).await, "after first open");

    // The relationship answers queries immediately: PRE-1 > PRE-2 > PRE-4.
    let ev = one(&c1, "tickets.get", json!({"id": "PRE-4"})).await;
    assert_eq!(ev["ticket"]["parent"], "PRE-2");
    let kids = drain(&c1, "tickets.list", json!({"parent": "PRE-1"})).await;
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0]["ticket"]["id"], "PRE-2");
    // The body is the raw record, untouched by the migration.
    assert!(
        ev["ticket"]["body"].as_str().unwrap().contains("parent: PRE-2"),
        "{}",
        ev["ticket"]["body"]
    );

    drop(c1);
    s1.kill9();

    // Run 2: the backfill was durable and does not run again.
    let s2 = spawn_server(Some(&db), &[], dir.path(), "parent2").await;
    let c2 = connect(s2.port).await;
    let log = s2.startup_output();
    assert!(!log.contains("from body frontmatter"), "no second migration:\n{log}");
    assert!(!log.contains("AGS-19:"), "no second migration:\n{log}");
    assert!(log.contains("loaded 5 tickets"), "got:\n{log}");
    check(&drain(&c2, "tickets.list", json!({})).await, "after SIGKILL + restart");

    // And the system of record itself.
    let conn = rusqlite::Connection::open(&db).unwrap();
    let stored: Vec<(String, Option<String>, String)> = conn
        .prepare("SELECT id, parent, children FROM tickets ORDER BY epic, seq")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        stored,
        vec![
            ("PRE-1".to_string(), None, "[\"PRE-2\"]".to_string()),
            ("PRE-2".to_string(), Some("PRE-1".into()), "[\"PRE-4\"]".to_string()),
            ("PRE-3".to_string(), None, "[]".to_string()),
            ("PRE-4".to_string(), Some("PRE-2".into()), "[]".to_string()),
            ("PRE-5".to_string(), None, "[]".to_string()),
        ]
    );
}

/// AGS-13: a legacy value the pinned mapping does not cover refuses the
/// open — the process exits non-zero naming the row — and the database
/// is left untouched for a human to fix.
#[tokio::test]
async fn legacy_database_with_unmappable_value_refuses_to_start() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("tickets.db");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pre-ags13.db"),
        &db,
    )
    .unwrap();
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute(
            "UPDATE tickets SET ticket_type = 'wibble' WHERE id = 'LEG-3'",
            [],
        )
        .unwrap();
    }

    let s = spawn_server(Some(&db), &[], dir.path(), "unmappable").await;
    let (status, log) = s.wait_exit(Duration::from_secs(10));
    assert!(!status.success(), "must refuse to start, got {status}");
    assert!(log.contains("LEG-3"), "names the row:\n{log}");
    assert!(log.contains("'wibble'"), "names the value:\n{log}");

    let conn = rusqlite::Connection::open(&db).unwrap();
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(tickets)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(cols.contains(&"ticket_type".into()), "untouched: {cols:?}");
    let still: String = conn
        .query_row("SELECT ticket_type FROM tickets WHERE id = 'LEG-1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(still, "epic", "no row was rewritten");
}

/// Criterion 3: a fresh, empty location is an empty store, not an error.
#[tokio::test]
async fn fresh_location_starts_empty_and_writable() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let db = dir.path().join("nested").join("never-seen-before.db");

    let s = spawn_server(Some(&db), &[], dir.path(), "fresh").await;
    let c = connect(s.port).await;

    let ev = one(&c, "tickets.health", json!({})).await;
    assert_eq!(ev["type"], "health");
    assert_eq!(ev["ticket_count"], 0);

    let ev = one(&c, "tickets.create", json!({"epic": "NEW", "title": "works"})).await;
    assert_eq!(ev["type"], "created");
    assert_eq!(ev["ticket"]["id"], "NEW-1");

    let log = s.startup_output();
    assert!(log.contains("loaded 0 tickets"), "got:\n{log}");
}

/// Criterion 6: with no override, the startup output names the default
/// location and it is under the user's data directory. Hermetic: the data
/// directory itself is redirected via XDG_DATA_HOME.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn default_location_is_under_the_user_data_dir() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let data_home = dir.path().join("xdg-data");
    std::fs::create_dir_all(&data_home).unwrap();

    let s = spawn_server(
        None, // no --db-path, no MENGER_DB_PATH: the default must apply
        &[("XDG_DATA_HOME", data_home.to_str().unwrap())],
        dir.path(),
        "default",
    )
    .await;
    let c = connect(s.port).await;

    let expected = data_home.join("menger").join("tickets.db");
    let log = s.startup_output();
    assert!(
        log.contains(&format!("database: {}", expected.display())),
        "startup output names the default location {}, got:\n{log}",
        expected.display()
    );
    assert!(expected.exists(), "database created under the data dir");

    let ev = one(&c, "tickets.create", json!({"epic": "DFL", "title": "lands in the default db"})).await;
    assert_eq!(ev["type"], "created");
}
