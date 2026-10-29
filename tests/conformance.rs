//! POLYGLOT-6 — menger held to the wire's conformance kit
//! (`plexus-wire/conformance`, the sibling checkout; written from
//! `plexus-wire/spec/`, nothing read from here): the binary started as
//! `tests/support` starts it — `key`, the fixture operator as `--admin` — with
//! its limits small enough to test in seconds, and `run.py run --role service`
//! against it. What menger answers differently is listed in
//! `plexus-wire/conformance/expect/menger.txt`, each line naming its gap; an
//! entry that starts passing fails this test ("fixed — remove the entry").
//!
//! `python3` or the plexus-wire checkout missing fails, naming it — never a
//! silent skip.

mod support;

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

#[test]
fn kit_green_on_menger() {
    let dir = tempfile::tempdir().unwrap();
    let (port, shell) = (free_port(), free_port());
    let _menger = Kill(
        Command::new(env!("CARGO_BIN_EXE_menger"))
            .args(support::login_args())
            .args(["--port", &port.to_string(), "--shell-port", &shell.to_string(), "--no-rest"])
            .arg("--db-path")
            .arg(dir.path().join("conformance.db"))
            .args(["--max-calls-per-connection", "8", "--idle-timeout-secs", "3", "--max-connections-per-principal", "8"])
            .env("PLEXUS_NO_REGISTRY", "1")
            .env_remove("MENGER_DB_PATH")
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
    let key = key_file(dir.path());
    let expect = wire().join("conformance/expect/menger.txt");
    let out = Command::new("python3")
        .arg("-I")
        .arg(wire().join("conformance/run.py"))
        .args(["run", "--role", "service", "--url", &format!("ws://127.0.0.1:{port}"), "--key", key.to_str().unwrap()])
        .args(["--max-calls", "8", "--idle-secs", "3", "--max-per-principal", "8", "--origin-allowed", &format!("http://127.0.0.1:{shell}")])
        // A debug build answers its self-description in seconds (07's schema walk): four times the kit's budgets.
        .args(["--expect-fail", expect.to_str().unwrap(), "--budget-scale", "4"])
        .output()
        .unwrap_or_else(|e| panic!("python3 (the conformance kit's interpreter) did not run: {e}"));
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    println!("{text}");
    assert!(out.status.success(), "the conformance kit is not green on menger:\n{text}");
}
