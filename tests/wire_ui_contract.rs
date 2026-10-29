//! UI-11: **one hash** — a bundle's `manifest.contract` is the hash `ui.push`
//! compares against, and `plexus-ui build --contract-from URL` stamps that
//! one. Over the socket, a spawned `menger` under the fixture operator with
//! its own ui file and the test signer (no `--ui-compiler`: the trees are
//! taken as given, and the contract check still runs).
//!
//! FRAME-8: the bundle is built by the framework's tool, `plexus-ui build`
//! (`scripts/plexus-ui`, ../plexus-ui's cli/ built into this checkout's
//! target) — `--ui` a one-component fixture compiled for real, `--engine` a
//! stand-in engine — and `--contract-from` signs in to the spawned server as
//! the fixture operator's key file and asks it `tickets.schema` (its `hash`
//! is what the push's seam reads, `../plexus-ui/service/src/seam.rs`), never the whole
//! service's hash. The bundle it builds is then pushed to that server.
//! Servers on ports the OS hands out, their files under /tmp/claude-1000.
//!
//! UI-12: `--contract HASH` stamps HASH as it is given — no server asked: the
//! build runs with no key to sign in with at all — and refuses `unknown`
//! passed explicitly (the literal is what a build writes when it is told
//! nothing, never what it is told).

#![cfg(unix)]

mod support;

use jsonrpsee::ws_client::WsClient;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use support::{call, connect, free_port, json_manifest, signed_params, spawn_menger, ui_args};

/// The one component the fixture UI holds (a component's name holds a hyphen).
const X: &str = "x-row";

fn scratch(prefix: &str) -> tempfile::TempDir {
    let root = Path::new("/tmp/claude-1000");
    std::fs::create_dir_all(root).unwrap();
    tempfile::Builder::new().prefix(prefix).tempdir_in(root).unwrap()
}

struct Server {
    s: support::Spawned,
    dir: tempfile::TempDir,
}

/// The binary with its own ui file and the test signer; no compiler, no shell.
async fn start() -> Server {
    let dir = scratch("menger-ui-contract-");
    let mut extra = vec!["--no-shell".to_string()];
    extra.extend(ui_args(&dir.path().join("ui.db")));
    let s = spawn_menger(free_port().await, &dir.path().join("t.db"), &extra).await;
    Server { s, dir }
}

/// The one data item `method` answers over the socket.
async fn one(c: &WsClient, method: &str) -> Value {
    let evs = call(c, method, json!({})).await;
    assert_eq!(evs.len(), 1, "{method} answers one item: {evs:?}");
    assert!(evs[0].get("type").and_then(Value::as_str) != Some("error"), "{method} answered an error: {evs:?}");
    evs[0].clone()
}

/// The hash the push compares against: the running tickets service's, as
/// its schema says it (`tickets.schema`'s `hash`, what the seam reads).
async fn tickets_hash(c: &WsClient) -> String {
    let schema = one(c, "tickets.schema").await;
    schema["hash"].as_str().unwrap_or_else(|| panic!("tickets.schema answers its hash: {schema}")).to_string()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The fixture operator's seed in a key file (0600), what `--key-file` reads.
fn admin_key(dir: &Path) -> PathBuf {
    use base64::Engine;
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("admin.key");
    std::fs::write(&path, format!("{}\n", base64::engine::general_purpose::STANDARD.encode(support::ADMIN))).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    path
}

/// The fixture UI (one component) and a stand-in engine under `dir`. The
/// component calls `tickets.list` (FRAME-5: a manifest naming no service
/// answers `contract_matches: null`, so the contract it is stamped with is
/// compared only when it names one — here `tickets` alone, whose hash the
/// contract is).
fn fixture(dir: &Path) {
    write(&dir.join("ui/index.html"), "<!doctype html><title>contract</title>\n");
    write(&dir.join("ui/routes.json"), &json!({ "#/": X }).to_string());
    write(&dir.join("ui/slots.json"), "{}");
    write(&dir.join(format!("ui/src/{X}.svelte")), "<script>\n  prop(\"id\");\n  fn open(state, ev) { call(\"tickets.list\", #{ id: ev.props.id }); state }\n</script>\n<p class=\"row\" on:click={open}>{id}</p>\n");
    write(&dir.join("engine/engine.js"), "export default function () {}\n");
    write(&dir.join("engine/engine.wasm"), "\0asm");
}

/// `plexus-ui build` on the fixture with `args` after the fixture's own:
/// whether it succeeded, and what it said.
fn ui_build_run(dir: &Path, args: &[String]) -> (bool, String) {
    let root = env!("CARGO_MANIFEST_DIR");
    let run = Command::new("bash")
        .arg(format!("{root}/scripts/plexus-ui"))
        .arg("build")
        .arg(dir.join("ui"))
        .arg("--engine").arg(dir.join("engine"))
        .arg("-o").arg(dir.join("out/ui.tar"))
        .args(args)
        .env_remove("PLEXUS_KEY")
        .output()
        .expect("bash runs scripts/plexus-ui");
    let said = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    (run.status.success(), said)
}

/// The text of `manifest.json` inside the tar `bytes`.
fn manifest_in(bytes: &[u8]) -> String {
    let mut a = tar::Archive::new(bytes);
    for e in a.entries().unwrap() {
        let mut e = e.unwrap();
        if e.path().unwrap().to_str() == Some("manifest.json") {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut e, &mut s).unwrap();
            return s;
        }
    }
    panic!("the bundle holds no manifest.json")
}

/// `plexus-ui build` on the fixture with `args`, which must succeed: the
/// tar's bytes and its manifest's text.
fn ui_build_with(dir: &Path, args: &[String]) -> (Vec<u8>, String) {
    let out = dir.join("out/ui.tar");
    let (ok, said) = ui_build_run(dir, args);
    assert!(ok, "plexus-ui build {args:?} failed:\n{said}");
    let bytes = std::fs::read(&out).unwrap_or_else(|e| panic!("no {}: {e}\n{said}", out.display()));
    let manifest = manifest_in(&bytes);
    (bytes, manifest.trim_end().to_string())
}

/// One `pushed`; it.
fn pushed(evs: &[Value]) -> &Value {
    assert_eq!(evs.len(), 1, "{evs:?}");
    assert_eq!(evs[0]["type"], "pushed", "{evs:?}");
    &evs[0]
}

/// `contract_matches` is there and `null`: unknown, never `false`.
fn unknown(p: &Value) {
    assert!(p.as_object().is_some_and(|o| o.contains_key("contract_matches")), "pushed says contract_matches: {p}");
    assert!(p["contract_matches"].is_null(), "a contract of unknown is unknown (null), never false: {p}");
}

#[tokio::test]
async fn a_bundle_stamped_from_the_service_matches() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let want = tickets_hash(&admin).await;
    let whole = one(&admin, "menger.hash").await;
    assert_ne!(whole["value"].as_str(), Some(want.as_str()), "the whole service's hash is another, so only the right call can pass: {whole}");
    fixture(m.dir.path());
    let key = admin_key(m.dir.path());

    let url = format!("ws://127.0.0.1:{}", m.s.port);
    let (bytes, manifest) = ui_build_with(m.dir.path(), &["--contract-from".into(), url, "--key-file".into(), key.display().to_string()]);
    let doc: Value = serde_json::from_str(&manifest).expect("the manifest is JSON");
    assert_eq!(doc["contract"].as_str(), Some(want.as_str()), "--contract-from stamps the hash the push compares against (tickets.schema's), not another: {doc}");

    let evs = call(&admin, "ui.push", signed_params(&bytes, &manifest, 1)).await;
    let p = pushed(&evs);
    assert_eq!(p["contract_matches"], true, "a bundle stamped from this server matches it: {p}");
}

#[tokio::test]
async fn another_hash_does_not_match() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let other = "0".repeat(16);
    assert_ne!(tickets_hash(&admin).await, other);

    // FRAME-5: a component naming `tickets` — the contract is `tickets`' hash.
    let mut manifest: Value = serde_json::from_str(&json_manifest(&other, "engine/1")).unwrap();
    manifest["components"] = json!({ X: { "tree": "t", "handlers": "h", "uses": ["tickets.list"] } });
    let evs = call(&admin, "ui.push", signed_params(&support::bundle(1), &manifest.to_string(), 1)).await;
    let p = pushed(&evs);
    assert_eq!(p["contract_matches"], false, "another hash is a difference, said: {p}");

    // A manifest naming no service is compared with nothing: null, even
    // with the tickets service's own hash.
    let manifest = json_manifest(&tickets_hash(&admin).await, "engine/1");
    let evs = call(&admin, "ui.push", signed_params(&support::bundle(2), &manifest, 2)).await;
    assert!(pushed(&evs)["contract_matches"].is_null(), "no service named: null: {evs:?}");
}

#[tokio::test]
async fn an_empty_or_absent_contract_stays_refused() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    // UI-3: a manifest names what it was built against, or says `unknown`.
    let absent = json!({ "abi": "engine/1", "routes": {}, "components": {}, "slots": {} }).to_string();
    let empty = json_manifest("", "engine/1");
    for (n, manifest) in [(1, absent), (2, empty)] {
        let evs = call(&admin, "ui.push", signed_params(&support::bundle(n), &manifest, n)).await;
        assert_eq!(evs.len(), 1, "{evs:?}");
        assert_eq!((evs[0]["type"].as_str(), evs[0]["reason"].as_str()), (Some("refused"), Some("bad_manifest")), "{evs:?}");
        assert!(call(&admin, "ui.versions", json!({})).await.is_empty(), "nothing stored");
    }
    // The literal a build writes without --contract-from: pushed, unknown.
    let manifest = json_manifest("unknown", "engine/1");
    let evs = call(&admin, "ui.push", signed_params(&support::bundle(3), &manifest, 3)).await;
    unknown(pushed(&evs));
}

#[tokio::test]
async fn a_bundle_built_without_contract_from_is_unknown() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    fixture(m.dir.path());

    let (bytes, manifest) = ui_build_with(m.dir.path(), &[]);
    let doc: Value = serde_json::from_str(&manifest).expect("the manifest is JSON");
    assert_eq!(doc["contract"], "unknown", "without --contract-from the build writes unknown: {doc}");
    let evs = call(&admin, "ui.push", signed_params(&bytes, &manifest, 1)).await;
    unknown(pushed(&evs));
}

/// UI-12: `--contract HASH` stamps HASH and asks no server — the pipeline
/// reads the hash from the binary it built and hands it to the build, which
/// is given no key to sign in with, so it could ask none. The hash is the
/// one the push compares against, so the bundle matches.
#[tokio::test]
async fn a_contract_passed_straight_is_stamped_and_asks_no_server() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let want = tickets_hash(&admin).await;
    fixture(m.dir.path());

    let (bytes, manifest) = ui_build_with(m.dir.path(), &["--contract".to_string(), want.clone()]);
    let doc: Value = serde_json::from_str(&manifest).expect("the manifest is JSON");
    assert_eq!(doc["contract"].as_str(), Some(want.as_str()), "--contract HASH stamps HASH: {doc}");

    let evs = call(&admin, "ui.push", signed_params(&bytes, &manifest, 1)).await;
    let p = pushed(&evs);
    assert_eq!(p["contract_matches"], true, "a bundle stamped with this server's hash matches it: {p}");
}

/// UI-12: `unknown` is what a build writes when it is told nothing; told
/// it explicitly (`--contract unknown`), the build refuses, naming the
/// flag, and writes no bundle — a build that meant to stamp a hash and has
/// none stops there, never shipping a bundle that says unknown.
#[tokio::test]
async fn unknown_passed_explicitly_is_refused_naming_the_flag() {
    let dir = scratch("menger-ui-contract-unknown-");
    fixture(dir.path());
    for value in ["unknown", ""] {
        let (ok, said) = ui_build_run(dir.path(), &["--contract".to_string(), value.to_string()]);
        assert!(!ok, "--contract {value:?} is refused:\n{said}");
        assert!(!said.contains("unexpected argument"), "the build knows --contract (UI-12); it refused {value:?} as an unknown argument:\n{said}");
        assert!(said.contains("--contract"), "the refusal of {value:?} names the flag:\n{said}");
        assert!(!dir.path().join("out/ui.tar").exists(), "--contract {value:?}: no bundle written");
    }
}
