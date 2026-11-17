//! NOTES-3: **documents become notes — what was kept moves at the first
//! start** (`src/notes_move.rs`, `quota/src/ledger/renamed.rs`), over the
//! real protocol.
//!
//! The data directory the release before wrote is made the way it wrote
//! it: a server is run on a fresh directory (a note made, its body and a
//! question; a cap set; the `editor` role edited by `auth.role_set`), shut
//! down, and every name it kept is put back as the release before kept it
//! — the file `notes.db` (and its `-wal`, `-shm`) renamed `docs.db`, the
//! quota rows keyed `notes.*` rekeyed `docs.*`, the stored role's grants
//! `notes.*` written `docs.*`. The tables in the file are the same tables:
//! nothing of them was renamed. Then:
//!
//! - **the first start** finds every note with its id, history and thread,
//!   the same usage and caps under `notes.bytes` and `notes.count`, and the
//!   stored `editor` role writing notes (the gate would refuse it without the
//!   move: its grants named only `docs.*`); `docs.db` is a relative link;
//! - **the second start** moves nothing and writes nothing;
//! - **the release before, started again**, opens the same notes through
//!   `docs.db` (its store is the same code over the same tables);
//! - **a fresh data directory** makes `notes.db` and nothing named docs.

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use plexus_rpc::core::plexus::DynamicHub;
use plexus_rpc::transport::TransportServer;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

mod support;

/// An editor on the organization.
const EDITOR: [u8; 32] = [0xe1; 32];

/// A server on `db_path`, in the runtime it is called in: dropping that
/// runtime stops it and closes its files.
async fn start(db_path: &Path) -> u16 {
    let port = support::free_port().await;
    let login = support::Login::new();
    let service = support::service(db_path, &login).await;
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        TransportServer::builder(service, conv)
            .with_websocket(port)
            .with_session_validator(gate)
            .anonymous_view()
            .build()
            .await
            .expect("build server")
            .serve()
            .await
            .expect("serve");
    });
    port
}

async fn connect(seed: &[u8; 32], port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..100 {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_str(&format!("Bearer {}", support::proof_of(seed, port))).unwrap());
        if let Ok(c) = WsClientBuilder::default().set_headers(h).build(&url).await {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never came up on {url}");
}

async fn all(client: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut sub: Subscription<Value> = client.subscribe("menger.call", p, "menger.call_unsub").await.expect("subscribe");
    let mut out = Vec::new();
    while let Some(item) = tokio::time::timeout(Duration::from_secs(20), sub.next()).await.expect("an answer") {
        let item = item.unwrap();
        match item["type"].as_str() {
            Some("done") => break,
            Some("data") => out.push(item["content"].clone()),
            _ => {
                out.push(item);
                break;
            }
        }
    }
    out
}

async fn one(client: &WsClient, method: &str, params: Value) -> Value {
    let mut out = all(client, method, params).await;
    assert_eq!(out.len(), 1, "{method}: {out:?}");
    out.remove(0)
}

fn source<'a>(usage: &'a Value, name: &str) -> &'a Value {
    usage["sources"].as_array().unwrap().iter().find(|s| s["source"] == name).unwrap_or_else(|| panic!("{name} in {usage}"))
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
}

/// What one server kept, as the first start left it: the note's id, its
/// revision, the editor's usage.
struct Kept {
    id: String,
    revision: u64,
    bytes: u64,
}

/// A server of this release on a fresh `dir`: a note made and saved, a
/// question on it, a cap set on the editor, the editor role edited.
fn write_a_directory(dir: &Path) -> Kept {
    let rt = runtime();
    let kept = rt.block_on(async {
        let port = start(&dir.join("tickets.db")).await;
        let admin = connect(&support::ADMIN, port).await;
        assert_eq!(one(&admin, "tickets.create", json!({"epic": "N", "title": "n", "kind": "scope"})).await["type"], "created");
        let org = all(&admin, "orgs.list", json!({})).await[0]["org"]["name"].as_str().unwrap().to_string();
        let g = one(&admin, "auth.grant", json!({"principal": support::principal_of(&EDITOR), "role": "editor", "scope": format!("{org}:")})).await;
        assert!(g.get("error_kind").is_none(), "{g}");
        // The editor role, edited by an admin: the reads, and the notes it writes.
        let set = one(&admin, "auth.role_set", json!({"role": "editor", "grants": ["viewer", "notes.create", "notes.save", "notes.update", "notes.ask"]})).await;
        assert!(set.get("error_kind").is_none() && set["type"] != "error", "{set}");
        let editor = connect(&EDITOR, port).await;
        let made = one(&editor, "notes.create", json!({"title": "The kiln", "body": "Fire twice a week.\n"})).await;
        let id = made["note"]["id"].as_str().unwrap_or_else(|| panic!("{made}")).to_string();
        assert_eq!(one(&editor, "notes.save", json!({"id": id, "body": "Fire twice a week, cone 6.\n"})).await["type"], "saved");
        assert_eq!(one(&editor, "notes.ask", json!({"id": id, "from": 0, "to": 4, "text": "Twice?"})).await["type"], "asked");
        one(&admin, "quota.set", json!({"principal": support::principal_of(&EDITOR), "source": "notes.bytes", "cap": "10 MB"})).await;
        let usage = one(&editor, "quota.usage", json!({})).await;
        let revision = one(&editor, "notes.get", json!({"id": id})).await["note"]["revision"].as_u64().unwrap();
        Kept { id, revision, bytes: source(&usage, "notes.bytes")["used"].as_u64().unwrap() }
    });
    drop(rt);
    kept
}

/// Every name the release before kept, put back on `dir` as it kept it.
fn as_the_release_before(dir: &Path) {
    for s in ["", "-wal", "-shm"] {
        let from = dir.join(format!("notes.db{s}"));
        if from.exists() {
            std::fs::rename(&from, dir.join(format!("docs.db{s}"))).unwrap();
        }
    }
    let q = rusqlite::Connection::open(dir.join("quota.db")).unwrap();
    for table in ["usage", "holds", "allowances", "history"] {
        q.execute(&format!("UPDATE {table} SET source = 'docs.' || substr(source, 7) WHERE source LIKE 'notes.%'"), []).unwrap();
    }
    let t = rusqlite::Connection::open(dir.join("tickets.db")).unwrap();
    assert_eq!(t.execute("UPDATE roles SET grants = replace(grants, 'notes.', 'docs.') WHERE role = 'editor'", []).unwrap(), 1);
}

/// The rows a second start must leave as they are.
fn snapshot(dir: &Path) -> (Vec<String>, Vec<String>) {
    let q = rusqlite::Connection::open(dir.join("quota.db")).unwrap();
    let mut quota = Vec::new();
    for table in ["usage", "allowances", "history"] {
        let mut st = q.prepare(&format!("SELECT * FROM {table} ORDER BY 1, 2, 3")).unwrap();
        let n = st.column_count();
        let rows = st.query_map([], |r| Ok((0..n).map(|i| format!("{:?}", r.get::<_, rusqlite::types::Value>(i).unwrap())).collect::<Vec<_>>().join("|"))).unwrap();
        quota.extend(rows.map(|r| format!("{table}:{}", r.unwrap())));
    }
    let t = rusqlite::Connection::open(dir.join("tickets.db")).unwrap();
    let mut st = t.prepare("SELECT role, grants, at FROM roles ORDER BY role").unwrap();
    let roles = st.query_map([], |r| Ok(format!("{}|{}|{}", r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))).unwrap().map(Result::unwrap).collect();
    (quota, roles)
}

#[test]
fn the_first_start_moves_what_the_release_before_kept_and_the_second_moves_nothing() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let kept = write_a_directory(dir.path());
    as_the_release_before(dir.path());
    assert!(!dir.path().join("notes.db").exists() && dir.path().join("docs.db").is_file(), "the release before's layout");

    // The first start.
    let rt = runtime();
    rt.block_on(async {
        let port = start(&dir.path().join("tickets.db")).await;
        let editor = connect(&EDITOR, port).await;
        let admin = connect(&support::ADMIN, port).await;
        let rows = all(&editor, "notes.list", json!({})).await;
        let ids: Vec<&str> = rows[0]["items"].as_array().unwrap().iter().map(|i| i["note"]["id"].as_str().unwrap()).collect();
        assert_eq!(ids, [kept.id.as_str()], "every note, with its id");
        let got = one(&editor, "notes.get", json!({"id": kept.id})).await;
        assert_eq!((got["note"]["body"].as_str(), got["note"]["revision"].as_u64()), (Some("Fire twice a week, cone 6.\n"), Some(kept.revision)), "{got}");
        assert_eq!(all(&editor, "notes.history", json!({"id": kept.id})).await.len() as u64, kept.revision, "its history");
        assert_eq!(all(&editor, "notes.threads", json!({"id": kept.id})).await[0]["thread"]["question"], "Twice?", "its threads");
        let usage = one(&editor, "quota.usage", json!({})).await;
        assert_eq!(source(&usage, "notes.bytes")["used"].as_u64(), Some(kept.bytes), "the same usage: {usage}");
        assert_eq!((source(&usage, "notes.bytes")["cap"].as_u64(), source(&usage, "notes.bytes")["from"].as_str()), (Some(10_000_000), Some("own")), "the same cap");
        assert_eq!(source(&usage, "notes.count")["used"].as_u64(), Some(1));
        // The stored editor role writes notes: its docs.* grants gained their notes.*.
        let saved = one(&editor, "notes.save", json!({"id": kept.id, "body": "Fire twice a week, cone 6. Moved.\n"})).await;
        assert_eq!(saved["type"], "saved", "a stored editor writes notes: {saved}");
        let roles = json!(all(&admin, "auth.roles", json!({})).await).to_string();
        assert!(roles.contains("notes.save") && roles.contains("docs.save"), "the new grant beside the old: {roles}");
        // Nothing answers to the old name.
        let old = one(&editor, "docs.list", json!({})).await;
        assert_eq!(old["type"], "error", "docs.list is no method: {old}");
    });
    drop(rt);
    let link = std::fs::read_link(dir.path().join("docs.db")).expect("docs.db is a link");
    assert_eq!(link, Path::new("notes.db"), "a relative link to the new name");
    assert!(dir.path().join("notes.db").is_file());

    // The second start: nothing moved, nothing written.
    let before = snapshot(dir.path());
    let rt = runtime();
    rt.block_on(async {
        let port = start(&dir.path().join("tickets.db")).await;
        let editor = connect(&EDITOR, port).await;
        assert_eq!(one(&editor, "notes.get", json!({"id": kept.id})).await["note"]["body"], "Fire twice a week, cone 6. Moved.\n");
    });
    drop(rt);
    assert_eq!(snapshot(dir.path()), before, "a second start writes nothing");
    assert_eq!(std::fs::read_link(dir.path().join("docs.db")).unwrap(), Path::new("notes.db"));

    // The release before, started again: its store opens docs.db — the link —
    // and finds the same notes, the move's write among them.
    let rt = runtime();
    rt.block_on(async {
        let before = Arc::new(menger_notes::NotesStore::open(&dir.path().join("docs.db")).unwrap());
        let doc = before.get(&kept.id).await.unwrap();
        assert_eq!(doc.body.as_deref(), Some("Fire twice a week, cone 6. Moved.\n"));
    });
}

#[test]
fn a_fresh_data_directory_makes_notes_db_and_nothing_named_docs() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let rt = runtime();
    rt.block_on(async {
        start(&dir.path().join("tickets.db")).await;
    });
    drop(rt);
    assert!(dir.path().join("notes.db").is_file());
    let named_docs: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.contains("docs")).collect();
    assert!(named_docs.is_empty(), "{named_docs:?}");
}
