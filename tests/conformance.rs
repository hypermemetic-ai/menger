//! POLYGLOT-6 — menger held to the wire's conformance kit
//! (`plexus-wire/conformance`, the sibling checkout; written from
//! `plexus-wire/spec/`, nothing read from here): the binary started as
//! `tests/support` starts it — `key`, the fixture operator as `--admin` — with
//! its limits small enough to test in seconds, and `run.py run --role service`
//! against it. What menger answers differently is listed, each line naming
//! its gap; an entry that starts passing fails this test ("fixed — remove the
//! entry"). On v2 that list is `tests/conformance-v2.expect`, beside this
//! file: plexus-wire's `conformance/expect/menger.txt` is main's.
//!
//! V2-44: main's test, on v2. The service under test is every one v2 runs in
//! its one process — `tickets`, `ui`, `media`, `docs`, `quota` beside
//! `orgs`, `members`, `session` and `auth` — each on its own file in the
//! test's directory; the kit's schema walk reads the whole tree, and the test
//! holds that tree to v2's services, so the kit is never green on less.
//!
//! `python3` or the plexus-wire checkout missing fails, naming it — never a
//! silent skip. `MENGER_BIN` names a copy of this build's binary (the shared
//! target's `debug/menger` is another worktree's the moment it builds).

mod support;

use serde_json::json;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn wire() -> PathBuf {
    let p = std::env::var_os("PLEXUS_WIRE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plexus-wire"));
    assert!(p.join("conformance/run.py").exists(), "the conformance kit is not at {} (a plexus-wire checkout beside menger, or PLEXUS_WIRE)", p.display());
    p
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// The fixture operator's seed as synapse keeps a key: base64 text, 0600.
fn key_file(dir: &std::path::Path) -> PathBuf {
    use base64::Engine;
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("operator.key");
    std::fs::write(&p, base64::engine::general_purpose::STANDARD.encode(support::ADMIN) + "\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    p
}

/// v2's services: what the kit's service must be (V2-1, V2-24, V2-29, V2-31).
const V2_SERVICES: &[&str] = &["tickets", "ui", "media", "docs", "quota", "orgs", "members", "session"];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kit_green_on_menger() {
    let dir = tempfile::tempdir().unwrap();
    let (port, shell) = (free_port(), free_port());
    let bin = std::env::var_os("MENGER_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_menger")));
    let _menger = Kill(
        Command::new(bin)
            .args(support::login_args())
            .args(["--port", &port.to_string(), "--shell-port", &shell.to_string(), "--no-rest"])
            .arg("--db-path")
            .arg(dir.path().join("conformance.db"))
            .arg("--ui-db-path")
            .arg(dir.path().join("conformance-ui.db"))
            .args(["--max-calls-per-connection", "8", "--idle-timeout-secs", "3", "--max-connections-per-principal", "8"])
            // V2-54: the message limit small too, and the kit told it (errors.request_too_big sends one past it).
            .args(["--max-message-bytes", "1048576"])
            // V2-31: the kit's key is a `key:` principal — an agent, held to its own quota.
            .args(["--max-connections-per-agent", "8"])
            .env("PLEXUS_NO_REGISTRY", "1")
            .env_remove("MENGER_DB_PATH")
            .env_remove("MENGER_UI_DB_PATH")
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn menger"),
    );
    let t0 = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(30), "menger did not listen on {port}");
        std::thread::sleep(Duration::from_millis(50));
    }

    // The service the kit is held to is v2's whole: every service in its schema.
    let admin = support::connect(port).await;
    let schema = support::call(&admin, "menger.schema", json!({})).await;
    let names: Vec<String> = schema
        .iter()
        .flat_map(|i| i["children"].as_array().cloned().unwrap_or_default())
        .filter_map(|c| c["namespace"].as_str().map(str::to_string))
        .collect();
    for want in V2_SERVICES {
        assert!(names.iter().any(|n| n == want), "V2-44: `{want}` is not in the schema the kit walks: {names:?}");
    }
    drop(admin);

    let key = key_file(dir.path());
    let expect = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/conformance-v2.expect");
    let out = tokio::task::spawn_blocking(move || {
        Command::new("python3")
            .arg("-I")
            .arg(wire().join("conformance/run.py"))
            .args(["run", "--role", "service", "--url", &format!("ws://127.0.0.1:{port}"), "--key", key.to_str().unwrap()])
            .args(["--max-calls", "8", "--idle-secs", "3", "--max-per-principal", "8", "--origin-allowed", &format!("http://127.0.0.1:{shell}")])
            .args(["--max-message-bytes", "1048576"])
            // A debug build answers its self-description in seconds (07's schema walk): four times the kit's budgets.
            .args(["--expect-fail", expect.to_str().unwrap(), "--budget-scale", "4"])
            .output()
            .unwrap_or_else(|e| panic!("python3 (the conformance kit's interpreter) did not run: {e}"))
    })
    .await
    .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    println!("{text}");
    assert!(out.status.success(), "the conformance kit is not green on menger:\n{text}");
}
