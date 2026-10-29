//! UI-11: **one hash** — a bundle's `manifest.contract` is the hash `ui.push`
//! compares against, and `scripts/ui-build.sh --contract-from PORT` stamps
//! that one. Over the socket, a spawned `menger` under the fixture operator
//! with its own ui file and the test signer (no `--ui-compiler`: the trees
//! are taken as given, and the contract check still runs).
//!
//! The script is run for real — `--ui` a one-component fixture, `--engine`
//! a stand-in engine, `UI_COMPILER` a stand-in compiler, so no cargo runs —
//! with a stand-in `synapse` first on its PATH. The stand-in answers the
//! calls the script may make with what the spawned server answered over the
//! socket to the same call — `menger hash` the whole service's hash, `menger
//! tickets schema` the tickets service's schema (its `hash` is what the
//! push's seam reads, `../plexus-ui/service/src/seam.rs`) — and refuses any other call
//! or port, naming it. The bundle it builds is then pushed to that server.
//! Servers on ports the OS hands out, their files under /tmp/claude-1000.
//!
//! UI-12: `--contract HASH` stamps HASH as it is given — no server asked,
//! so the pipeline's codegen container needs no socket to the build's
//! menger — and refuses `unknown` passed explicitly (the literal is what
//! the script writes when it is told nothing, never what it is told).

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

fn executable(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    write(path, text);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// What `synapse -j` prints for one data item and the end.
fn jsonl(content: &Value) -> String {
    format!("{}\n{}\n", json!({ "type": "data", "content": content }), json!({ "type": "done" }))
}

/// The stand-in `synapse` in `dir/bin`: every call logged to `dir/calls.log`;
/// `-P <port>` must be the server's; the rest of the line, joined by `_`,
/// names its answer in `dir/answers/` — none there, refused, naming it.
async fn stand_in(dir: &Path, port: u16, c: &WsClient) -> PathBuf {
    executable(
        &dir.join("bin/synapse"),
        r#"#!/usr/bin/env bash
dir=$(cd "$(dirname "$0")/.." && pwd)
printf '%s\n' "$*" >>"$dir/calls.log"
port= rest=()
while [ $# -gt 0 ]; do
  case "$1" in
    -P) port=$2; shift 2 ;;
    -j) shift ;;
    --as) shift 2 ;;
    *) rest+=("$1"); shift ;;
  esac
done
[ "$port" = "$(cat "$dir/port")" ] || { echo "stand-in synapse: -P $port is not the server's" >&2; exit 1; }
key=$(IFS=_; echo "${rest[*]}")
[ -f "$dir/answers/$key.jsonl" ] || { echo "stand-in synapse: no answer for: ${rest[*]}" >&2; exit 1; }
cat "$dir/answers/$key.jsonl"
"#,
    );
    write(&dir.join("port"), &port.to_string());
    // The two the script may ask, each answered as the server answers it.
    write(&dir.join("answers/menger_hash.jsonl"), &jsonl(&one(c, "menger.hash").await));
    write(&dir.join("answers/menger_tickets_schema.jsonl"), &jsonl(&one(c, "tickets.schema").await));
    dir.join("bin")
}

/// The fixture UI (one component), a stand-in engine and a stand-in
/// compiler (a tree that uses `tickets.list`: FRAME-5, a manifest naming no
/// service answers `contract_matches: null`, so the contract it is stamped
/// with is compared only when it names one — here, `tickets` alone, whose
/// hash the contract is) under `dir`.
fn fixture(dir: &Path) {
    write(&dir.join("ui/index.html"), "<!doctype html><title>contract</title>\n");
    write(&dir.join("ui/routes.json"), &json!({ "#/": X }).to_string());
    write(&dir.join("ui/slots.json"), "{}");
    write(&dir.join(format!("ui/src/{X}.svelte")), "<script>\n  state(#{ n: 0 });\n</script>\n<p>{state.n}</p>\n");
    write(&dir.join("engine/engine.js"), "export default function () {}\n");
    write(&dir.join("engine/engine.wasm"), "\0asm");
    executable(
        &dir.join("compiler"),
        r#"#!/usr/bin/env bash
# the stand-in compiler: <src> -o <dir> → <dir>/<name>.json, <dir>/<name>.rhai
name=$(basename "$1" .svelte)
printf '{"name":"%s","uses":["tickets.list"]}\n' "$name" >"$3/$name.json"
printf '\n' >"$3/$name.rhai"
"#,
    );
}

/// `scripts/ui-build.sh` on the fixture with `args` after the fixture's own,
/// the stand-in first on PATH: whether it succeeded, and what it said.
fn ui_build_run(dir: &Path, bin: &Path, args: &[String]) -> (bool, String) {
    let root = env!("CARGO_MANIFEST_DIR");
    let mut cmd = Command::new("bash");
    cmd.arg(format!("{root}/scripts/ui-build.sh"))
        .arg("--ui").arg(dir.join("ui"))
        .arg("--engine").arg(dir.join("engine"))
        .arg("-o").arg(dir.join("out/ui.tar"))
        .args(args);
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let run = cmd.env("PATH", path).env("UI_COMPILER", dir.join("compiler")).env("CARGO_TARGET_DIR", dir.join("target")).output().expect("bash runs scripts/ui-build.sh");
    let said = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    (run.status.success(), said)
}

/// `scripts/ui-build.sh` on the fixture, `--contract-from port` when given,
/// the stand-in first on PATH: the tar's bytes and its manifest's text.
fn ui_build(dir: &Path, bin: &Path, contract_from: Option<u16>) -> (Vec<u8>, String) {
    let args: Vec<String> = contract_from.map(|p| vec!["--contract-from".to_string(), p.to_string()]).unwrap_or_default();
    ui_build_with(dir, bin, &args)
}

/// `scripts/ui-build.sh` on the fixture with `args`, which must succeed: the
/// tar's bytes and its manifest's text.
fn ui_build_with(dir: &Path, bin: &Path, args: &[String]) -> (Vec<u8>, String) {
    let out = dir.join("out/ui.tar");
    let (ok, said) = ui_build_run(dir, bin, args);
    assert!(ok, "scripts/ui-build.sh {args:?} failed:\n{said}");
    let bytes = std::fs::read(&out).unwrap_or_else(|e| panic!("no {}: {e}\n{said}", out.display()));
    let manifest = std::fs::read_to_string(dir.join("out/ui.d/manifest.json")).expect("the manifest beside the tar");
    (bytes, manifest)
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
    let bin = stand_in(m.dir.path(), m.s.port, &admin).await;
    fixture(m.dir.path());

    let (bytes, manifest) = ui_build(m.dir.path(), &bin, Some(m.s.port));
    let calls = std::fs::read_to_string(m.dir.path().join("calls.log")).unwrap_or_default();
    let doc: Value = serde_json::from_str(&manifest).expect("the manifest is JSON");
    assert_eq!(
        doc["contract"].as_str(),
        Some(want.as_str()),
        "--contract-from stamps the hash the push compares against (tickets.schema's), not another; the script asked: {calls}"
    );

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
    // The literal the script writes without --contract-from: pushed, unknown.
    let manifest = json_manifest("unknown", "engine/1");
    let evs = call(&admin, "ui.push", signed_params(&support::bundle(3), &manifest, 3)).await;
    unknown(pushed(&evs));
}

#[tokio::test]
async fn a_bundle_built_without_contract_from_is_unknown() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let bin = stand_in(m.dir.path(), m.s.port, &admin).await;
    fixture(m.dir.path());

    let (bytes, manifest) = ui_build(m.dir.path(), &bin, None);
    assert!(!m.dir.path().join("calls.log").exists(), "without --contract-from the script asks no server");
    let doc: Value = serde_json::from_str(&manifest).expect("the manifest is JSON");
    assert_eq!(doc["contract"], "unknown", "without --contract-from the script writes unknown: {doc}");
    let evs = call(&admin, "ui.push", signed_params(&bytes, &manifest, 1)).await;
    unknown(pushed(&evs));
}

/// UI-12: `--contract HASH` stamps HASH and asks no server — the pipeline
/// reads the hash from the binary it built and hands it to the codegen
/// container, which has no socket to it. The hash is the one the push
/// compares against, so the bundle matches.
#[tokio::test]
async fn a_contract_passed_straight_is_stamped_and_asks_no_server() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let want = tickets_hash(&admin).await;
    let bin = stand_in(m.dir.path(), m.s.port, &admin).await;
    fixture(m.dir.path());

    let (bytes, manifest) = ui_build_with(m.dir.path(), &bin, &["--contract".to_string(), want.clone()]);
    let calls = std::fs::read_to_string(m.dir.path().join("calls.log")).unwrap_or_default();
    assert!(calls.is_empty(), "--contract HASH asks no server; the script asked: {calls}");
    let doc: Value = serde_json::from_str(&manifest).expect("the manifest is JSON");
    assert_eq!(doc["contract"].as_str(), Some(want.as_str()), "--contract HASH stamps HASH: {doc}");

    let evs = call(&admin, "ui.push", signed_params(&bytes, &manifest, 1)).await;
    let p = pushed(&evs);
    assert_eq!(p["contract_matches"], true, "a bundle stamped with this server's hash matches it: {p}");
}

/// UI-12: `unknown` is what the script writes when it is told nothing; told
/// it explicitly (`--contract unknown`), the script refuses, naming the
/// flag, and writes no bundle — a build that meant to stamp a hash and has
/// none stops there, never shipping a bundle that says unknown.
#[tokio::test]
async fn unknown_passed_explicitly_is_refused_naming_the_flag() {
    let dir = scratch("menger-ui-contract-unknown-");
    // A stand-in synapse that answers nothing: no server is to be asked.
    executable(&dir.path().join("bin/synapse"), "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>\"$(dirname \"$0\")/../calls.log\"\nexit 1\n");
    fixture(dir.path());
    for value in ["unknown", ""] {
        let (ok, said) = ui_build_run(dir.path(), &dir.path().join("bin"), &["--contract".to_string(), value.to_string()]);
        assert!(!ok, "--contract {value:?} is refused:\n{said}");
        assert!(!said.contains("unknown argument"), "the script knows --contract (UI-12); it refused {value:?} as an unknown argument:\n{said}");
        assert!(said.contains("--contract"), "the refusal of {value:?} names the flag:\n{said}");
        assert!(!dir.path().join("out/ui.tar").exists(), "--contract {value:?}: no bundle written");
    }
    assert!(!dir.path().join("calls.log").exists(), "no server asked");
}
