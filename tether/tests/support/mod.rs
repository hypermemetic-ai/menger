//! TETHER-9's harness: a throwaway menger (its binary, built from this
//! checkout, on its own port and database, `--auth-provider key`) and the
//! tether binary dialing it, each a child process killed on drop; clients
//! signed in to either by key proof; a stub Claude Code that writes down
//! every call's arguments.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use plexus_auth_core::adapters::{key_principal, sign_key_proof};
use serde_json::Value;

/// tether's own operator (`--admin` on tether).
pub const OPERATOR: [u8; 32] = [0x11; 32];
/// The owner on menger: answers, comments, requests changes (a key, so
/// tether runs with `--accept-keys`).
pub const OWNER: [u8; 32] = [0x44; 32];
/// tether's key on menger (another agent's would be the same to tether).
pub const AGENT: [u8; 32] = [0x55; 32];

pub fn principal_of(seed: &[u8; 32]) -> String {
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

/// A test's directory — kept, and named on stderr, under
/// `TETHER_TEST_KEEP=1` (its logs: menger.log, tether.log, calls.log).
pub fn scratch() -> tempfile::TempDir {
    let mut d = tempfile::Builder::new().prefix("tether-test-").tempdir().unwrap();
    if std::env::var_os("TETHER_TEST_KEEP").is_some() {
        d.disable_cleanup(true);
        eprintln!("kept: {}", d.path().display());
    }
    d
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// The menger binary: `TETHER_TEST_MENGER`, else built from the checkout
/// tether sits in (once per test binary).
pub fn menger_bin() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Some(p) = std::env::var_os("TETHER_TEST_MENGER") {
            return PathBuf::from(p);
        }
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("Cargo.toml");
        let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["build", "--bin", "menger", "--message-format=json", "--manifest-path"])
            .arg(&manifest)
            .stderr(Stdio::inherit())
            .output()
            .expect("cargo builds menger");
        assert!(out.status.success(), "menger did not build");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| v["executable"].as_str().map(PathBuf::from))
            .find(|p| p.file_name().is_some_and(|n| n == "menger"))
            .expect("cargo named menger's executable")
    })
    .clone()
}

/// A child process, killed (and reaped) when dropped.
pub struct Proc(pub Child);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn wait_for_port(port: u16, what: &str) {
    for _ in 0..600 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{what} never listened on {port}");
}

/// A throwaway menger: OWNER and AGENT both admin.
pub async fn menger(dir: &Path) -> (Proc, u16) {
    let port = free_port();
    let child = Command::new(menger_bin())
        .env("PLEXUS_NO_REGISTRY", "1")
        .args(["--auth-provider", "key", "--admin", &principal_of(&OWNER), "--admin", &principal_of(&AGENT), "--no-shell", "--port"])
        .arg(port.to_string())
        .arg("--db-path")
        .arg(dir.join("menger.db"))
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(dir.join("menger.log")).unwrap())
        .spawn()
        .expect("menger starts");
    let p = Proc(child);
    wait_for_port(port, "menger").await;
    (p, port)
}

/// The stub Claude Code: each call's arguments appended to `<dir>/calls.log`
/// (`=====` between calls); a prompt saying SLOW takes 3 s; it answers
/// `stub did it`.
pub fn stub(dir: &Path) -> PathBuf {
    let stub = dir.join("claude-stub");
    std::fs::write(
        &stub,
        format!(
            concat!(
                "#!/bin/sh\n",
                "printf '%s\\n' \"$*\" >> {log}\n",
                "echo ===== >> {log}\n",
                "echo '{{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"stub-session\",\"model\":\"stub\",\"cwd\":\".\",\"tools\":[]}}'\n",
                "case \"$*\" in *SLOW*) sleep 3;; esac\n",
                "case \"$*\" in *FOREVER*) exec sleep 60;; esac\n",
                "echo '{{\"type\":\"assistant\",\"message\":{{\"id\":\"m1\",\"role\":\"assistant\",\"model\":\"stub\",\"content\":[{{\"type\":\"text\",\"text\":\"stub did it\"}}]}}}}'\n",
                "echo '{{\"type\":\"result\",\"subtype\":\"success\",\"session_id\":\"stub-session\",\"is_error\":false,\"result\":\"stub did it\"}}'\n",
            ),
            log = dir.join("calls.log").display(),
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    stub
}

/// Every call the stub had, its arguments each.
pub fn calls(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("calls.log")).unwrap_or_default().split("=====\n").filter(|c| !c.trim().is_empty()).map(String::from).collect()
}

/// AGENT's key, as synapse keeps one (0600, base64 seed).
pub fn agent_key(dir: &Path) -> PathBuf {
    use base64::Engine as _;
    let f = dir.join("agent.key");
    std::fs::write(&f, base64::engine::general_purpose::STANDARD.encode(AGENT)).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    f
}

/// The tether binary, dialing menger on `menger_port`, its data in
/// `dir/tether`; `extra` flags after.
pub async fn tether(dir: &Path, menger_port: u16, port: u16, extra: &[&str]) -> Proc {
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_tether"))
        .env("PLEXUS_NO_REGISTRY", "1")
        .env("RUST_LOG", "warn,tether=debug")
        .args(["--auth-provider", "key", "--admin", &principal_of(&OPERATOR), "--port", &port.to_string()])
        .arg("--data-dir")
        .arg(dir.join("tether"))
        .arg("--work-dir")
        .arg(&work)
        .arg("--claude-path")
        .arg(stub(dir))
        .args(["--remote", &format!("ws://127.0.0.1:{menger_port}"), "--remote-as", "tether", "--remote-key"])
        .arg(agent_key(dir))
        .arg("--archetypes")
        .arg(dir.join("archetypes.env"))
        .args(extra)
        .stdout(Stdio::null())
        .stderr(std::fs::OpenOptions::new().create(true).append(true).open(dir.join("tether.log")).unwrap())
        .spawn()
        .expect("tether starts");
    let p = Proc(child);
    wait_for_port(port, "tether").await;
    p
}

/// A client signed in by `seed` to the socket on `port`.
pub async fn signed_in(seed: &[u8; 32], port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    let proof = sign_key_proof(seed, &url, "test", &nonce(), now());
    let mut h = HeaderMap::new();
    h.insert("authorization", HeaderValue::from_str(&format!("Bearer {proof}")).unwrap());
    WsClientBuilder::default().set_headers(h).build(url).await.expect("a key proof is let in")
}

/// `<hub>.call`: its data items, or the error it was refused with.
pub async fn call(c: &WsClient, hub: &str, method: &str, params: Value) -> Result<Vec<Value>, String> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut sub: Subscription<Value> =
        c.subscribe(&format!("{hub}.call"), p, &format!("{hub}.call_unsub")).await.map_err(|e| e.to_string())?;
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

/// The one answer of a call that must succeed, not an in-band error.
pub async fn one(c: &WsClient, hub: &str, method: &str, params: Value) -> Value {
    let items = call(c, hub, method, params.clone()).await.unwrap_or_else(|e| panic!("{method} {params}: {e}"));
    let first = items.first().cloned().unwrap_or_else(|| panic!("{method}: nothing answered"));
    assert_ne!(first["type"], "error", "{method} {params}: {first}");
    first
}

/// Poll `check` every 100 ms until it gives something (or 30 s pass).
pub async fn until<T, F, Fut>(what: &str, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    for _ in 0..300 {
        if let Some(t) = check().await {
            return t;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("never: {what}");
}
