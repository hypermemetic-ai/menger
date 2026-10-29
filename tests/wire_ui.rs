//! UI-3: the `ui` activation over the socket — a spawned `menger` under the
//! fixture operator (`--admin`, `support::login_args`) with its own ui file
//! and the test signer (`support::ui_args`). Bundles are small tars built
//! here; a version's name is the sha256 of its bytes; the signature is
//! Ed25519 over that digest. Each server on ports the OS hands out.

mod support;

use jsonrpsee::ws_client::WsClient;
use serde_json::{json, Value};
use support::{call, connect, connect_as, free_port, push_params, sha_hex, spawn_menger, ui_args, NOT_A_SIGNER, UI_SIGNER};

/// A key that holds editor on `*` and is no admin.
const EDITOR: [u8; 32] = [0xed; 32];

async fn start_with(signer: bool) -> (support::Spawned, tempfile::TempDir) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let mut extra = vec!["--no-shell".to_string()];
    match signer {
        true => extra.extend(ui_args(&dir.path().join("ui.db"))),
        false => extra.extend(["--ui-db-path".to_string(), dir.path().join("ui.db").display().to_string()]),
    }
    let s = spawn_menger(free_port().await, &dir.path().join("t.db"), &extra).await;
    (s, dir)
}

async fn start() -> (support::Spawned, tempfile::TempDir) {
    start_with(true).await
}

/// The `version` events `versions` answers, in order; panics on anything else.
async fn versions(c: &WsClient) -> Vec<Value> {
    let evs = call(c, "ui.versions", json!({})).await;
    assert!(evs.iter().all(|e| e["type"] == "version"), "versions answers only version events: {evs:?}");
    evs
}

/// The hash `versions` says is live.
async fn live(c: &WsClient) -> Option<String> {
    versions(c).await.iter().find(|v| v["live"] == true).and_then(|v| v["hash"].as_str()).map(str::to_string)
}

async fn push(c: &WsClient, n: u32) {
    let pushed = call(c, "ui.push", push_params(n, &UI_SIGNER)).await;
    assert_eq!(pushed.first().and_then(|e| e["type"].as_str()), Some("pushed"), "{pushed:?}");
}

/// The gate's refusal of `grant`: one error item, forbidden, naming it —
/// and nothing (no `pushed`, no `refused`) before it.
fn forbidden(evs: &[Value], grant: &str) {
    assert_eq!(evs.len(), 1, "the gate's refusal alone: {evs:?}");
    let e = &evs[0];
    assert_eq!(e["type"], "error", "the envelope is the gate's error: {e}");
    assert_eq!((e["error_kind"].as_str(), e["grant"].as_str()), (Some("forbidden"), Some(grant)), "{e}");
}

/// One in-band refusal for `reason`.
fn refused(evs: &[Value], reason: &str) {
    assert_eq!(evs.len(), 1, "{evs:?}");
    assert_eq!((evs[0]["type"].as_str(), evs[0]["reason"].as_str()), (Some("refused"), Some(reason)), "{evs:?}");
    assert!(evs[0]["message"].as_str().is_some_and(|m| !m.is_empty()), "it says why: {evs:?}");
}

#[tokio::test]
async fn an_admin_pushes_and_the_version_goes_live() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    let hash = sha_hex(&support::bundle(1));
    let pushed = call(&admin, "ui.push", push_params(1, &UI_SIGNER)).await;
    assert_eq!(pushed.len(), 1, "{pushed:?}");
    let p = &pushed[0];
    assert_eq!(p["type"], "pushed", "{p}");
    assert_eq!(p["version"], hash.as_str(), "{p}");
    assert_eq!(p["live"], true, "{p}");
    assert_eq!(p["new"], true, "{p}");
    let listed = versions(&admin).await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!((listed[0]["hash"].as_str(), listed[0]["live"].as_bool()), (Some(hash.as_str()), Some(true)), "listed live: {listed:?}");
    assert_eq!(listed[0]["pushed_by"], support::admin(), "pushed by the admin");
    assert_eq!((listed[0]["contract"].as_str(), listed[0]["abi"].as_str(), listed[0]["commit"].as_str()), (Some("c1"), Some("a1"), Some("0000001")));
}

#[tokio::test]
async fn a_non_admin_push_is_refused_by_the_gate() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    push(&admin, 1).await;
    let granted = call(&admin, "auth.grant", json!({ "principal": support::principal_of(&EDITOR), "role": "editor", "scope": "*" })).await;
    assert!(!granted.is_empty() && granted[0].get("error_kind").is_none(), "editor on *: {granted:?}");

    let editor = connect_as(&EDITOR, s.port).await;
    forbidden(&call(&editor, "ui.push", push_params(2, &UI_SIGNER)).await, "ui.push");
    assert_eq!(versions(&admin).await.len(), 1, "the row count is unchanged");
    assert_eq!(versions(&editor).await.len(), 1, "versions is the editor's to read");
}

#[tokio::test]
async fn the_editor_can_neither_roll_back_nor_preview() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    push(&admin, 1).await;
    push(&admin, 2).await;
    call(&admin, "auth.grant", json!({ "principal": support::principal_of(&EDITOR), "role": "editor", "scope": "*" })).await;
    let editor = connect_as(&EDITOR, s.port).await;
    let one = sha_hex(&support::bundle(1));
    forbidden(&call(&editor, "ui.rollback", json!({ "to": one })).await, "ui.rollback");
    forbidden(&call(&editor, "ui.preview", json!({ "version": one })).await, "ui.preview");
    assert_eq!(live(&admin).await, Some(sha_hex(&support::bundle(2))), "the pointer where it was");
}

#[tokio::test]
async fn a_bad_signature_is_refused_in_band() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    let evs = call(&admin, "ui.push", push_params(1, &NOT_A_SIGNER)).await;
    refused(&evs, "bad_signature");
    assert!(evs[0]["message"].as_str().unwrap().contains("signature"), "it names it: {evs:?}");
    assert!(versions(&admin).await.is_empty(), "nothing written");
}

#[tokio::test]
async fn a_replayed_signature_is_refused_in_band() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    // A valid signature, over bundle 1's digest, sent with bundle 2.
    let mut params = push_params(2, &UI_SIGNER);
    params["signature"] = push_params(1, &UI_SIGNER)["signature"].clone();
    refused(&call(&admin, "ui.push", params).await, "bad_signature");
    assert!(versions(&admin).await.is_empty(), "nothing written");
}

#[tokio::test]
async fn with_no_signer_every_push_is_refused() {
    let (s, _dir) = start_with(false).await;
    let admin = connect(s.port).await;
    refused(&call(&admin, "ui.push", push_params(1, &UI_SIGNER)).await, "no_signer");
    assert!(versions(&admin).await.is_empty(), "nothing written");
}

#[tokio::test]
async fn a_rollback_over_the_wire_moves_live() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    let (one, two) = (sha_hex(&support::bundle(1)), sha_hex(&support::bundle(2)));
    let health = call(&admin, "tickets.health", json!({})).await;
    assert!(!health.is_empty(), "{health:?}");
    push(&admin, 1).await;
    push(&admin, 2).await;
    let rolled = call(&admin, "ui.rollback", json!({ "to": one })).await;
    assert_eq!(rolled.len(), 1, "{rolled:?}");
    assert_eq!(rolled[0]["type"], "live", "{rolled:?}");
    assert_eq!(rolled[0]["version"], one.as_str(), "{rolled:?}");
    let listed = versions(&admin).await;
    let live: Vec<(&str, bool)> = listed.iter().map(|v| (v["hash"].as_str().unwrap(), v["live"].as_bool().unwrap())).collect();
    assert_eq!(live, vec![(two.as_str(), false), (one.as_str(), true)], "newest first; the first is live");
    // Its clock aside (`unix_now` moves on its own): the tickets service untouched.
    let untimed = |evs: Vec<Value>| evs.into_iter().map(|mut e| { e.as_object_mut().map(|o| o.remove("unix_now")); e }).collect::<Vec<_>>();
    assert_eq!(untimed(call(&admin, "tickets.health", json!({})).await), untimed(health), "the tickets service untouched");
}

#[tokio::test]
async fn a_rollback_to_an_unknown_version_is_refused() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    push(&admin, 1).await;
    refused(&call(&admin, "ui.rollback", json!({ "to": "f".repeat(64) })).await, "unknown_version");
    assert_eq!(live(&admin).await, Some(sha_hex(&support::bundle(1))), "the pointer unchanged");
}

/// `ui.push`'s params for `bytes` with `manifest`, signed by the test signer.
fn signed(bytes: &[u8], manifest: &str) -> Value {
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::Digest;
    let signature = SigningKey::from_bytes(&UI_SIGNER).sign(&sha2::Sha256::digest(bytes));
    let b64 = base64::engine::general_purpose::STANDARD;
    json!({ "bundle": b64.encode(bytes), "manifest": manifest, "signature": b64.encode(signature.to_bytes()) })
}

#[tokio::test]
async fn a_bundle_without_an_index_is_refused_in_band() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    push(&admin, 1).await;
    let before = versions(&admin).await;
    // A valid tar holding only an asset — no index.html.
    let mut b = tar::Builder::new(Vec::new());
    let mut h = tar::Header::new_gnu();
    h.set_size(5);
    h.set_mode(0o644);
    h.set_cksum();
    b.append_data(&mut h, "app.js", &b"x();\n"[..]).unwrap();
    let bytes = b.into_inner().unwrap();
    refused(&call(&admin, "ui.push", signed(&bytes, "contract: c\nabi: a\n")).await, "bad_bundle");
    assert_eq!(versions(&admin).await, before, "nothing written");
}

#[tokio::test]
async fn a_manifest_without_contract_or_abi_is_refused_in_band() {
    let (s, _dir) = start().await;
    let admin = connect(s.port).await;
    let bytes = support::bundle(1);
    for manifest in ["contract: c1\n", "abi: a1\n"] {
        refused(&call(&admin, "ui.push", signed(&bytes, manifest)).await, "bad_manifest");
        assert!(versions(&admin).await.is_empty(), "nothing written ({manifest:?})");
    }
}
