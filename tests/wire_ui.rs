//! UI-3: the `ui` activation over the socket — a spawned `menger` under the
//! fixture operator (`--admin`, `support::login_args`) with its own ui file
//! and the test signer (`support::ui_args`). Bundles are small tars built
//! here; a version's name is the sha256 of its bytes; the signature is
//! Ed25519 over that digest.
//!
//! Ports 4490–4493, one per test.

mod support;

use serde_json::{json, Value};
use support::{call, connect, connect_as, push_params, sha_hex, spawn_menger, ui_args, NOT_A_SIGNER, UI_SIGNER};

/// A key that holds editor on `*` and is no admin.
const EDITOR: [u8; 32] = [0xed; 32];

async fn start(port: u16) -> (support::Spawned, tempfile::TempDir) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let mut extra = vec!["--no-shell".to_string()];
    extra.extend(ui_args(&dir.path().join("ui.db")));
    let s = spawn_menger(port, &dir.path().join("t.db"), &extra).await;
    (s, dir)
}

/// The `version` events `versions` answers, in order; panics on anything else.
async fn versions(c: &jsonrpsee::ws_client::WsClient) -> Vec<Value> {
    let evs = call(c, "ui.versions", json!({})).await;
    assert!(evs.iter().all(|e| e["type"] == "version"), "versions answers only version events: {evs:?}");
    evs
}

#[tokio::test]
async fn an_admin_pushes_and_the_version_goes_live() {
    let (s, _dir) = start(4490).await;
    let admin = connect(s.port).await;
    let hash = sha_hex(&support::bundle(1));
    let pushed = call(&admin, "ui.push", push_params(1, &UI_SIGNER)).await;
    assert_eq!(pushed, vec![json!({ "type": "pushed", "version": hash, "live": true, "new": true })], "{pushed:?}");
    let listed = versions(&admin).await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!((listed[0]["hash"].as_str(), listed[0]["live"].as_bool()), (Some(hash.as_str()), Some(true)), "listed live: {listed:?}");
    assert_eq!(listed[0]["pushed_by"], support::admin(), "pushed by the admin");
    assert_eq!((listed[0]["contract"].as_str(), listed[0]["abi"].as_str(), listed[0]["commit"].as_str()), (Some("c1"), Some("a1"), Some("0000001")));
}

#[tokio::test]
async fn a_non_admin_push_is_refused_by_the_gate() {
    let (s, _dir) = start(4491).await;
    let admin = connect(s.port).await;
    let first = call(&admin, "ui.push", push_params(1, &UI_SIGNER)).await;
    assert_eq!(first[0]["type"], "pushed", "{first:?}");
    let granted = call(&admin, "auth.grant", json!({ "principal": support::principal_of(&EDITOR), "role": "editor", "scope": "*" })).await;
    assert!(!granted.is_empty() && granted[0].get("error_kind").is_none(), "editor on *: {granted:?}");

    let editor = connect_as(&EDITOR, s.port).await;
    let refused = call(&editor, "ui.push", push_params(2, &UI_SIGNER)).await;
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!((refused[0]["error_kind"].as_str(), refused[0]["grant"].as_str()), (Some("forbidden"), Some("ui.push")), "the gate refuses: {refused:?}");
    assert_eq!(versions(&admin).await.len(), 1, "the row count is unchanged");
    assert_eq!(versions(&editor).await.len(), 1, "versions is the editor's to read");
}

#[tokio::test]
async fn a_bad_signature_is_refused_in_band() {
    let (s, _dir) = start(4492).await;
    let admin = connect(s.port).await;
    let refused = call(&admin, "ui.push", push_params(1, &NOT_A_SIGNER)).await;
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!((refused[0]["type"].as_str(), refused[0]["reason"].as_str()), (Some("refused"), Some("bad_signature")), "{refused:?}");
    assert!(refused[0]["message"].as_str().is_some_and(|m| m.contains("signature")), "it names it: {refused:?}");
    assert!(versions(&admin).await.is_empty(), "nothing written");
}

#[tokio::test]
async fn a_rollback_over_the_wire_moves_live() {
    let (s, _dir) = start(4493).await;
    let admin = connect(s.port).await;
    let (one, two) = (sha_hex(&support::bundle(1)), sha_hex(&support::bundle(2)));
    for n in [1, 2] {
        let pushed = call(&admin, "ui.push", push_params(n, &UI_SIGNER)).await;
        assert_eq!(pushed[0]["type"], "pushed", "{pushed:?}");
    }
    let rolled = call(&admin, "ui.rollback", json!({ "to": one })).await;
    assert_eq!(rolled, vec![json!({ "type": "live", "version": one })], "{rolled:?}");
    let listed = versions(&admin).await;
    let live: Vec<(&str, bool)> = listed.iter().map(|v| (v["hash"].as_str().unwrap(), v["live"].as_bool().unwrap())).collect();
    assert_eq!(live, vec![(two.as_str(), false), (one.as_str(), true)], "newest first; the first is live");
}
