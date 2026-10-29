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
//! push's seam reads, `ui-service/src/seam.rs`) — and refuses any other call
//! or port, naming it. The bundle it builds is then pushed to that server.
//! Servers on ports the OS hands out, their files under /tmp/claude-1000.

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
/// compiler (a tree that uses nothing) under `dir`.
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
printf '{"name":"%s","uses":[]}\n' "$name" >"$3/$name.json"
printf '\n' >"$3/$name.rhai"
"#,
    );
}

/// `scripts/ui-build.sh` on the fixture, `--contract-from port` when given,
/// the stand-in first on PATH: the tar's bytes and its manifest's text.
fn ui_build(dir: &Path, bin: &Path, contract_from: Option<u16>) -> (Vec<u8>, String) {
    let root = env!("CARGO_MANIFEST_DIR");
    let out = dir.join("out/ui.tar");
    let mut cmd = Command::new("bash");
    cmd.arg(format!("{root}/scripts/ui-build.sh"))
        .arg("--ui").arg(dir.join("ui"))
        .arg("--engine").arg(dir.join("engine"))
        .arg("-o").arg(&out);
    if let Some(port) = contract_from {
        cmd.arg("--contract-from").arg(port.to_string());
    }
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let run = cmd.env("PATH", path).env("UI_COMPILER", dir.join("compiler")).env("CARGO_TARGET_DIR", dir.join("target")).output().expect("bash runs scripts/ui-build.sh");
    let said = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    assert!(run.status.success(), "scripts/ui-build.sh failed ({}):\n{said}", run.status);
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

    let manifest = json_manifest(&other, "engine/1");
    let evs = call(&admin, "ui.push", signed_params(&support::bundle(1), &manifest, 1)).await;
    let p = pushed(&evs);
    assert_eq!(p["contract_matches"], false, "another hash is a difference, said: {p}");
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
