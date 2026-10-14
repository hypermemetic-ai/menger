//! AUTH-7: menger adopts the AUTH protocol — black-box over the built
//! binary, as `main` wires it (`--auth-provider key --admin key:<boss>`),
//! and once without any provider (login off, as prod runs today).
//!
//! - every method's grant is the role IDENTITY-21 asked of it
//!   ([`the_roles_are_what_identity_21_asked_method_by_method`], in process);
//! - an anonymous connection is admitted and sees only the door
//!   (`auth.providers`, `auth.login`, `auth.signup`), and can call nothing
//!   else;
//! - a signed-in caller's view lists only what it holds, with `held_on`, and
//!   its hash differs by holdings;
//! - a refusal is typed (`error_kind`, `grant`, `scope`, `roles`,
//!   `held_on`); `auth.whoami` and `auth.requires` say who and what;
//! - an admin's `auth.role_set` changes a caller's view and calls, and is
//!   kept in the store across a restart;
//! - login off: no gate — every method's params as they were, `_info` as it
//!   was, anonymous calls answered.
//!
//! Every server has its own throwaway database and free ports; prod and test
//! are never touched. The keys are fixtures.

#![cfg(unix)]

use ed25519_dalek::SigningKey;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use plexus_auth_core::adapters::{key_principal, sign_key_proof};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The operator's key (`--admin`): admin on `*`.
const BOSS: [u8; 32] = [0xb0; 32];
/// Agents' keys.
const AGENT: [u8; 32] = [0xa9; 32];
const OTHER: [u8; 32] = [0xa7; 32];

fn principal(seed: &[u8; 32]) -> String {
    key_principal(SigningKey::from_bytes(seed).verifying_key().as_bytes()).to_string()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn nonce() -> [u8; 32] {
    static N: AtomicU8 = AtomicU8::new(1);
    let mut n = [0u8; 32];
    n[0] = N.fetch_add(1, Ordering::Relaxed);
    n[1] = std::process::id() as u8;
    n[2..10].copy_from_slice(&std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos().to_le_bytes()[..8]);
    n
}

async fn free_port() -> u16 {
    static HANDED: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let mut handed = HANDED.lock().unwrap();
        if !handed.contains(&port) {
            handed.push(port);
            return port;
        }
    }
}

/// A spawned `menger`. Dropping SIGKILLs.
struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The binary on `db`, its socket on a free port; `login`: with `--auth-provider
/// key --admin key:<boss>`.
async fn spawn(db: &Path, login: bool) -> Server {
    for _ in 0..3 {
        let port = free_port().await;
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_menger"));
        cmd.arg("--port")
            .arg(port.to_string())
            .arg("--no-shell")
            .arg("--no-rest")
            .arg("--db-path")
            .arg(db)
            .env("PLEXUS_NO_REGISTRY", "1")
            .env_remove("MENGER_DB_PATH")
            .env_remove("MENGER_AUTH_PROVIDERS")
            .env_remove("MENGER_ADMINS")
            .env_remove("MENGER_REQUIRE_LOGIN")
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if login {
            cmd.arg("--auth-provider").arg("key").arg("--admin").arg(principal(&BOSS));
        }
        let mut server = Server { child: cmd.spawn().expect("spawn menger"), port };
        for _ in 0..200 {
            if server.child.try_wait().unwrap().is_some() {
                break;
            }
            if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if server.child.try_wait().unwrap().is_none() {
                    return server;
                }
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        drop(server);
    }
    panic!("menger failed to start");
}

async fn connect(port: u16, bearer: Option<&str>) -> WsClient {
    let mut h = HeaderMap::new();
    if let Some(b) = bearer {
        h.insert("authorization", HeaderValue::from_str(&format!("Bearer {b}")).unwrap());
    }
    WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{port}")).await.expect("admitted")
}

/// A connection as the key `seed`, by its proof at the upgrade.
async fn as_key(port: u16, seed: &[u8; 32], agent: &str) -> WsClient {
    connect(port, Some(&sign_key_proof(seed, &format!("ws://127.0.0.1:{port}"), agent, &nonce(), now()))).await
}

/// A call's items: each data item's content; an error item whole (and the
/// stream ends there).
async fn call(c: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let mut out = Vec::new();
    while let Ok(Some(Ok(item))) = tokio::time::timeout(Duration::from_secs(5), s.next()).await {
        match item["type"].as_str() {
            Some("data") => out.push(item["content"].clone()),
            Some("error") => {
                out.push(item);
                break;
            }
            _ => break,
        }
    }
    out
}

async fn one(c: &WsClient, method: &str, params: Value) -> Value {
    let mut evs = call(c, method, params).await;
    assert_eq!(evs.len(), 1, "{method}: {evs:?}");
    evs.remove(0)
}

async fn info(c: &WsClient) -> Value {
    let mut s: Subscription<Value> = c.subscribe("_info", ObjectParams::new(), "_info_unsub").await.unwrap();
    let item = tokio::time::timeout(Duration::from_secs(5), s.next()).await.unwrap().unwrap().unwrap();
    item["content"].clone()
}

/// Each activation the caller's `menger.schema` shows, with its methods.
async fn view(c: &WsClient) -> BTreeMap<String, BTreeSet<String>> {
    let hub = one(c, "menger.schema", json!({})).await;
    let mut out = BTreeMap::new();
    for child in hub["children"].as_array().cloned().unwrap_or_default() {
        let ns = child["namespace"].as_str().unwrap().to_string();
        let schema = one(c, &format!("{ns}.schema"), json!({})).await;
        let methods = schema["methods"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap().to_string()).filter(|m| m != "schema").collect();
        out.insert(ns, methods);
    }
    out
}

fn names(xs: &[&str]) -> BTreeSet<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// A, B and C, each a scope with one build under it (C hosted under A-1).
async fn tickets(boss: &WsClient) {
    for (epic, parent) in [("A", None), ("B", None), ("C", Some("A-1"))] {
        let made = call(boss, "tickets.create", json!({ "epic": epic, "title": format!("{epic} root"), "kind": "scope", "parent": parent })).await;
        assert_eq!(made[0]["type"], "created", "{made:?}");
        let made = call(boss, "tickets.create", json!({ "epic": epic, "title": format!("{epic} work"), "parent": format!("{epic}-1") })).await;
        assert_eq!(made[0]["type"], "created", "{made:?}");
    }
}

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

// ── In process: the method → grant mapping ───────────────────────────────

/// What IDENTITY-21/IDENTITY-37 asked of a caller, method by method, before
/// AUTH-7 (`Access::admit` + `Caller::require`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Today {
    /// Any connection admitted at all (anonymous ones included when login
    /// was on but not required).
    Anyone,
    /// Any signed-in caller, whatever it held.
    SignedIn,
    Viewer,
    Editor,
    Admin,
}

/// Every method of menger's four activations, with what it asked before
/// AUTH-7, and its target now. Reads asked viewer on what they answered
/// (scoped, never refused but for holding nothing); writes editor on the
/// ticket they touch (`create`: on its parent or shortcode root;
/// `set_parent`: on both sides, admin on both for a shortcode's root);
/// members' admin methods admin on the scope.
const TODAY: &[(&str, Today, Option<&str>)] = &[
    ("tickets.health", Today::Anyone, None),
    ("tickets.get", Today::Viewer, None),
    ("tickets.list", Today::Viewer, None),
    ("tickets.ready_set", Today::Viewer, None),
    ("tickets.find", Today::Viewer, None),
    ("tickets.search", Today::Viewer, None),
    ("tickets.member", Today::Viewer, None),
    ("tickets.groups", Today::Viewer, None),
    ("tickets.components", Today::Viewer, None),
    ("tickets.inputs", Today::Viewer, None),
    ("tickets.extents", Today::Viewer, None),
    ("tickets.counts", Today::Viewer, None),
    ("tickets.facets", Today::Viewer, None),
    ("tickets.tree", Today::Viewer, None),
    ("tickets.waves", Today::Viewer, None),
    // MENGER-31: a ticket's boards — reads, a viewer's.
    ("tickets.waves_under", Today::Viewer, None),
    ("tickets.waves_to", Today::Viewer, None),
    ("tickets.blockers", Today::Viewer, None),
    ("tickets.watch", Today::Viewer, None),
    // MENGER-13: a ticket's history is a read.
    ("tickets.history", Today::Viewer, None),
    ("tickets.create", Today::Editor, None),
    ("tickets.set_status", Today::Editor, Some("id")),
    ("tickets.set_kind", Today::Editor, Some("id")),
    ("tickets.set_parent", Today::Editor, Some("id")),
    ("tickets.amend", Today::Editor, Some("id")),
    ("tickets.link_commits", Today::Editor, Some("id")),
    ("tickets.unlink_commits", Today::Editor, Some("id")),
    // MENGER-12: an editor's, on the ticket it labels.
    ("tickets.label", Today::Editor, Some("id")),
    // MENGER-30: any part of a ticket in one write — an editor's, on it.
    ("tickets.update", Today::Editor, Some("id")),
    ("tickets.update_many", Today::Editor, Some("ids")),
    // ACT-5: gone for good, and back — admin's alone.
    ("tickets.delete", Today::Admin, Some("ids")),
    ("tickets.undelete", Today::Admin, Some("ids")),
    ("tickets.answer", Today::Editor, Some("ticket")),
    ("tickets.clear_answer", Today::Editor, Some("ticket")),
    ("tickets.close_input", Today::Editor, Some("ticket")),
    ("tickets.reopen_input", Today::Editor, Some("ticket")),
    // MENGER-39: the owner's queue — a write an editor's, on the ticket; the
    // queue itself a read.
    ("tickets.queue", Today::Editor, Some("id")),
    ("tickets.unqueue", Today::Editor, Some("id")),
    ("tickets.snooze", Today::Editor, Some("id")),
    ("tickets.queued", Today::Viewer, None),
    ("comments.add", Today::Viewer, None),
    ("comments.list", Today::Viewer, None),
    ("comments.watch", Today::Viewer, None),
    ("comments.resolve", Today::Editor, None),
    ("comments.reopen", Today::Editor, None),
    // NOTE-2: a note is on no shortcode — any member's; an upgrade an editor's.
    ("notes.add", Today::Viewer, None),
    ("notes.list", Today::Viewer, None),
    ("notes.get", Today::Viewer, None),
    ("notes.upgrade", Today::Editor, None),
    ("members.mine", Today::Anyone, None),
    ("members.list", Today::Admin, None),
    ("members.grant", Today::Admin, Some("scope")),
    ("members.revoke", Today::Admin, Some("scope")),
    ("members.requests", Today::Admin, None),
    ("session.whoami", Today::Anyone, None),
    ("session.login", Today::SignedIn, None),
    ("session.logout", Today::SignedIn, None),
    ("session.providers", Today::Anyone, None),
];

/// AUTH-7's one deliberate narrowing of the table: what an anonymous or
/// grantless caller reached before now needs a grant (or a sign-in).
fn now_needs(today: Today, method: &str) -> Today {
    match (today, method) {
        // A heartbeat that answered a grantless caller "0 tickets" is a
        // viewer's read now.
        (Today::Anyone, "tickets.health") => Today::Viewer,
        // Who am I, my grants, the providers: a signed-in caller's; an
        // anonymous one asks auth.providers / auth.login.
        (Today::Anyone, _) => Today::SignedIn,
        (t, _) => t,
    }
}

#[tokio::test]
async fn the_roles_are_what_identity_21_asked_method_by_method() {
    let dir = tmp();
    let sessions = Arc::new(menger::sessions::Sessions::new(3600, &menger::login_names()));
    let access = menger_tickets_api::access::Access::with_login(menger::session_table(&sessions), true, Vec::new()).minting(menger::session_table(&sessions));
    let hub = menger::build_rpc_offering(&dir.path().join("t.db"), None, Default::default(), Vec::new(), access).await.unwrap();
    let declared = hub.declared_roles();
    let viewer: BTreeSet<String> = declared["viewer"].iter().cloned().collect();
    let editor: BTreeSet<String> = declared["editor"].iter().cloned().collect();
    assert!(viewer.is_subset(&editor), "an editor holds every viewer grant");
    let signed_in: BTreeSet<&str> = menger::SIGNED_IN_GRANTS.into_iter().collect();

    // Every method of menger's activations is in the table, once.
    let mut seen = BTreeSet::new();
    for schema in hub.list_plugin_schemas() {
        if !["tickets", "comments", "notes", "members", "session"].contains(&schema.namespace.as_str()) {
            continue;
        }
        for m in &schema.methods {
            if m.name == "schema" {
                continue;
            }
            let grant = format!("{}.{}", schema.namespace, m.name);
            assert_eq!(m.grant.as_deref(), Some(grant.as_str()), "the grant is the method");
            let (_, today, target) = TODAY.iter().find(|(g, _, _)| *g == grant).unwrap_or_else(|| panic!("{grant} is not in the table"));
            assert_eq!(m.target.as_deref(), *target, "{grant}'s target");
            let now = if signed_in.contains(grant.as_str()) {
                Today::SignedIn
            } else if viewer.contains(&grant) {
                Today::Viewer
            } else if editor.contains(&grant) {
                Today::Editor
            } else {
                Today::Admin
            };
            assert_eq!(now, now_needs(*today, &grant), "{grant}: asked {today:?} before AUTH-7");
            seen.insert(grant);
        }
    }
    assert_eq!(seen.len(), TODAY.len(), "every row is a method: {seen:?}");
    // admin is built in, never declared; nothing else is — but DEPLOY-11's
    // deployer: a CI's key, recording deployments and reading them.
    assert_eq!(declared.keys().cloned().collect::<BTreeSet<_>>(), names(&["deployer", "editor", "member", "viewer"]));
    let deployer: BTreeSet<String> = declared["deployer"].iter().cloned().collect();
    assert!(deployer.contains("deployments.record") && deployer.contains("deployments.list"), "{deployer:?}");
    assert!(!editor.contains("deployments.record") && !deployer.contains("tickets.create"), "record is its own grant");
}

// ── Over the wire, login on ──────────────────────────────────────────────

#[tokio::test]
async fn an_anonymous_connection_sees_only_the_door_and_can_call_nothing_else() {
    let dir = tmp();
    let s = spawn(&dir.path().join("t.db"), true).await;
    let anon = connect(s.port, None).await;

    let i = info(&anon).await;
    assert_eq!(i["view"], "anonymous", "{i}");
    assert_eq!(i["auth"]["methods"], json!(["auth.providers", "auth.login", "auth.signup"]), "{i}");
    let metadata = format!("http://127.0.0.1:{}/.well-known/oauth-protected-resource", s.port);
    assert_eq!(i["auth"]["resource_metadata"], metadata, "{i}");
    assert!(i["view_hash"].as_str().unwrap().starts_with("v-anon-"), "{i}");
    assert_eq!(view(&anon).await, BTreeMap::from([("auth".to_string(), names(&["providers", "login", "signup"]))]));

    // RFC 9728's document, on the socket's port.
    let doc: Value = reqwest::get(&metadata).await.unwrap().json().await.unwrap();
    assert_eq!(doc["resource"], format!("http://127.0.0.1:{}", s.port), "{doc}");

    assert_eq!(one(&anon, "auth.providers", json!({})).await, json!({ "type": "provider", "name": "key", "kind": "key" }));
    for (method, params) in [
        ("tickets.list", json!({})),
        ("tickets.health", json!({})),
        ("tickets.set_status", json!({ "id": "A-1", "status": "Ready" })),
        ("comments.list", json!({})),
        ("members.mine", json!({})),
        ("session.whoami", json!({})),
        ("auth.whoami", json!({})),
        ("auth.requires", json!({ "method": "tickets.list" })),
        ("auth.roles", json!({})),
    ] {
        let e = one(&anon, method, params).await;
        assert_eq!((e["type"].as_str(), e["error_kind"].as_str()), (Some("error"), Some("unauthenticated")), "{method}: {e}");
    }
    // A schema it may not see is not there.
    assert_eq!(one(&anon, "tickets.schema", json!({})).await["type"], "error");
    // A credential presented and refused is still a 401 at the upgrade.
    let mut h = HeaderMap::new();
    h.insert("authorization", HeaderValue::from_static("Bearer mgs_forged"));
    let bad = WsClientBuilder::default()
        .set_headers(h)
        .build(format!("ws://127.0.0.1:{}", s.port))
        .await
        .unwrap_err()
        .to_string();
    assert!(bad.contains("401"), "{bad}");
}

#[tokio::test]
async fn a_caller_s_view_lists_only_what_it_holds_and_refusals_are_typed() {
    let dir = tmp();
    let s = spawn(&dir.path().join("t.db"), true).await;
    let boss = as_key(s.port, &BOSS, "boss").await;
    tickets(&boss).await;

    // Nothing held: the signed-in door — whoami, its own grants, sessions.
    let agent = as_key(s.port, &AGENT, "worker-1").await;
    assert_eq!(
        view(&agent).await,
        BTreeMap::from([
            ("auth".to_string(), names(&["providers", "login", "signup", "whoami", "logout", "requires"])),
            ("members".to_string(), names(&["mine"])),
            ("session".to_string(), names(&["whoami", "login", "logout", "providers"])),
        ])
    );
    let no = one(&agent, "tickets.list", json!({})).await;
    assert_eq!(
        (no["error_kind"].as_str(), no["grant"].as_str(), no["scope"].as_str(), no["roles"].clone(), no["held_on"].clone()),
        (Some("forbidden"), Some("tickets.list"), Some("*"), json!(["editor", "viewer", "admin"]), json!([])),
        "{no}"
    );
    // It asked by signing in: it waits for an admin.
    let waiting = call(&boss, "auth.waiting", json!({})).await;
    assert!(waiting.iter().any(|w| w["principal"] == principal(&AGENT)), "{waiting:?}");

    // Viewer on A: the reads, held on A — each with its grant; no write.
    one(&boss, "members.grant", json!({ "principal": principal(&AGENT), "scope": "A", "role": "viewer" })).await;
    let v = view(&agent).await;
    assert!(v["tickets"].contains("list") && v["tickets"].contains("watch") && !v["tickets"].contains("set_status"), "{v:?}");
    assert_eq!(v["comments"], names(&["add", "list", "watch"]));
    assert_eq!(v["members"], names(&["mine"]), "no admin method");
    let schema = one(&agent, "tickets.schema", json!({})).await;
    let get = schema["methods"].as_array().unwrap().iter().find(|m| m["name"] == "list").unwrap();
    assert_eq!((get["grant"].as_str(), get["held_on"].clone()), (Some("tickets.list"), json!(["A"])), "{get}");
    assert!(schema.get("roles").is_none(), "declared roles are not shown in a view");
    let listed: BTreeSet<String> = call(&agent, "tickets.list", json!({ "summary": true })).await.iter().filter_map(|e| e["summary"]["id"].as_str().map(str::to_string)).collect();
    assert_eq!(listed, names(&["A-1", "A-2", "C-1", "C-2"]), "answered from where it is held (C is hosted under A)");
    let no = one(&agent, "tickets.set_status", json!({ "id": "A-2", "status": "Ready" })).await;
    assert_eq!(
        (no["error_kind"].as_str(), no["grant"].as_str(), no["scope"].as_str(), no["roles"].clone(), no["held_on"].clone()),
        (Some("forbidden"), Some("tickets.set_status"), Some("A"), json!(["editor", "admin"]), json!([])),
        "{no}"
    );
    assert_eq!(no["message"], "needs tickets.set_status on A");

    // Editor on A: the writes there, typed refusals elsewhere, held_on said.
    one(&boss, "members.grant", json!({ "principal": principal(&AGENT), "scope": "A", "role": "editor" })).await;
    assert_eq!(one(&agent, "tickets.set_status", json!({ "id": "C-2", "status": "Ready" })).await["type"], "status_changed");
    let no = one(&agent, "tickets.set_status", json!({ "id": "B-2", "status": "Ready" })).await;
    assert_eq!((no["scope"].as_str(), no["held_on"].clone()), (Some("B"), json!(["A"])), "{no}");

    // View hashes: by holdings — another key holding the same shares it.
    let other = as_key(s.port, &OTHER, "worker-2").await;
    let (h_agent, h_boss, h_other) = (info(&agent).await["view_hash"].clone(), info(&boss).await["view_hash"].clone(), info(&other).await["view_hash"].clone());
    assert_ne!(h_agent, h_boss);
    assert_ne!(h_agent, h_other);
    one(&boss, "members.grant", json!({ "principal": principal(&OTHER), "scope": "A", "role": "editor" })).await;
    assert_eq!(info(&other).await["view_hash"], h_agent, "equal holdings, one view");

    // auth.whoami and auth.requires.
    let me = one(&agent, "auth.whoami", json!({})).await;
    assert_eq!(
        (me["principal"].as_str(), me["agent"].as_str(), me["memberships"].clone(), me["ceiling"].clone()),
        (Some(principal(&AGENT).as_str()), Some("worker-1"), json!([{ "role": "editor", "scope": "A" }]), json!([])),
        "{me}"
    );
    let req = one(&agent, "auth.requires", json!({ "method": "tickets.set_status" })).await;
    assert_eq!(req, json!({ "type": "requires", "method": "tickets.set_status", "grant": "tickets.set_status", "target": "id", "roles": ["editor", "admin"] }));
    let req = one(&agent, "auth.requires", json!({ "method": "members.grant" })).await;
    assert_eq!((req["target"].as_str(), req["roles"].clone()), (Some("scope"), json!(["admin"])));

    // A session's ceiling (IDENTITY-42's, minted by auth.login): the gate reads it.
    let session = one(&agent, "auth.login", json!({ "flow": { "session": { "ceiling": ["viewer@A"] } } })).await;
    assert_eq!((session["type"].as_str(), session["ceiling"].clone()), (Some("session"), json!(["viewer@A"])), "{session}");
    let scoped = connect(s.port, Some(session["token"].as_str().unwrap())).await;
    let no = one(&scoped, "tickets.set_status", json!({ "id": "A-2", "status": "Pending" })).await;
    assert_eq!((no["error_kind"].as_str(), no["held_on"].clone()), (Some("forbidden"), json!([])), "editor on A, within viewer@A: {no}");
    assert!(!view(&scoped).await["tickets"].contains("set_status"), "the view follows the ceiling");
    assert_eq!(one(&scoped, "auth.whoami", json!({})).await["ceiling"], json!(["viewer@A"]));
    // session.login (IDENTITY-42's, kept) mints the same way.
    let compat = one(&agent, "session.login", json!({ "scope": ["viewer@A"] })).await;
    assert_eq!((compat["type"].as_str(), compat["scope"].clone()), (Some("login"), json!(["viewer@A"])));
}

/// FILTER-24: `keep` never answers a ticket outside the caller's reach — a
/// viewer on A keeping B's work is answered as without it.
#[tokio::test]
async fn keep_answers_nothing_outside_the_reach() {
    let dir = tmp();
    let s = spawn(&dir.path().join("t.db"), true).await;
    let boss = as_key(s.port, &BOSS, "boss").await;
    tickets(&boss).await;
    one(&boss, "members.grant", json!({ "principal": principal(&AGENT), "scope": "A", "role": "viewer" })).await;
    let agent = as_key(s.port, &AGENT, "worker-1").await;
    let ids = |evs: Vec<Value>| evs.iter().filter_map(|e| e["summary"]["id"].as_str().map(str::to_string)).collect::<Vec<_>>();
    let held = |evs: Vec<Value>| evs.iter().filter_map(|e| Some((e["epic"].as_str()?.to_string(), e["held"].as_u64()?))).collect::<Vec<_>>();
    // Nothing is Ready: only what is kept is listed.
    let list = |keep: &str| json!({ "filter": "status Ready", "keep": keep, "summary": true });
    assert_eq!(ids(call(&agent, "tickets.list", list("A-2")).await), ["A-2"]);
    assert_eq!(ids(call(&agent, "tickets.list", list("B-2")).await), Vec::<String>::new(), "B is out of reach");
    assert_eq!(ids(call(&boss, "tickets.list", list("B-2")).await), ["B-2"], "the admin's reach covers it");
    assert_eq!(held(call(&agent, "tickets.counts", json!({ "filter": "status Ready", "keep": "A-2" })).await), [("A".to_string(), 1)]);
    assert_eq!(held(call(&agent, "tickets.counts", json!({ "filter": "status Ready", "keep": "B-2" })).await), Vec::<(String, u64)>::new());
    let tree = |epic: &str, keep: &str| json!({ "epic": epic, "filter": "status Ready", "matches_only": true, "keep": keep });
    assert_eq!(ids(call(&agent, "tickets.tree", tree("A", "A-2")).await), ["A-2", "C-1"], "and C's row, hosted at A's root");
    assert_eq!(ids(call(&agent, "tickets.tree", tree("B", "B-2")).await), Vec::<String>::new());
}

#[tokio::test]
async fn an_admin_s_role_set_changes_a_caller_s_view_and_is_kept_across_a_restart() {
    let dir = tmp();
    let db: PathBuf = dir.path().join("t.db");
    let s = spawn(&db, true).await;
    let boss = as_key(s.port, &BOSS, "boss").await;
    tickets(&boss).await;
    one(&boss, "members.grant", json!({ "principal": principal(&AGENT), "scope": "*", "role": "viewer" })).await;
    let agent = as_key(s.port, &AGENT, "worker-1").await;
    assert!(view(&agent).await["tickets"].contains("list"));
    let before = info(&agent).await["view_hash"].clone();

    // A role no admin may set: a viewer is refused, typed.
    let no = one(&agent, "auth.role_set", json!({ "role": "viewer", "grants": ["tickets.get"] })).await;
    assert_eq!((no["error_kind"].as_str(), no["grant"].as_str()), (Some("forbidden"), Some("auth.role_set")), "{no}");

    let set = one(&boss, "auth.role_set", json!({ "role": "viewer", "grants": ["tickets.get", "comments.list"] })).await;
    assert_eq!(set, json!({ "type": "role", "role": "viewer", "grants": ["comments.list", "tickets.get"] }));
    let v = view(&agent).await;
    assert_eq!(v["tickets"], names(&["get"]), "{v:?}");
    assert_eq!(v["comments"], names(&["list"]));
    assert_ne!(info(&agent).await["view_hash"], before);
    assert_eq!(one(&agent, "tickets.list", json!({})).await["error_kind"], "forbidden");
    assert_eq!(one(&agent, "tickets.get", json!({ "id": "A-2" })).await["type"], "ticket");
    // A grant that names no method is refused.
    assert_eq!(one(&boss, "auth.role_set", json!({ "role": "viewer", "grants": ["tickets.nothing"] })).await["type"], "error");

    // Kept in the store: the next process reads it.
    drop((boss, agent));
    drop(s);
    let s = spawn(&db, true).await;
    let boss = as_key(s.port, &BOSS, "boss").await;
    let roles = call(&boss, "auth.roles", json!({})).await;
    let viewer = roles.iter().find(|r| r["role"] == "viewer").unwrap();
    assert_eq!(viewer["grants"], json!(["comments.list", "tickets.get"]), "{roles:?}");
    let editor = roles.iter().find(|r| r["role"] == "editor").unwrap();
    assert!(editor["grants"].as_array().unwrap().iter().any(|g| g == "tickets.list"), "an edit of one role leaves the rest as declared");
    let agent = as_key(s.port, &AGENT, "worker-1").await;
    assert_eq!(view(&agent).await["tickets"], names(&["get"]));
}

#[tokio::test]
async fn an_agent_asks_for_access_with_auth_signup() {
    let dir = tmp();
    let s = spawn(&dir.path().join("t.db"), true).await;
    let boss = as_key(s.port, &BOSS, "boss").await;
    let agent = as_key(s.port, &AGENT, "worker-1").await;
    let w = one(&agent, "auth.signup", json!({})).await;
    assert_eq!((w["type"].as_str(), w["principal"].as_str()), (Some("waiting"), Some(principal(&AGENT).as_str())), "{w}");
    let listed = call(&boss, "members.requests", json!({})).await;
    assert!(listed.iter().any(|r| r["principal"] == principal(&AGENT)), "{listed:?}");
    one(&boss, "auth.grant", json!({ "principal": principal(&AGENT), "role": "viewer", "scope": "A" })).await;
    assert!(call(&boss, "auth.waiting", json!({})).await.is_empty(), "granted: it waits no longer");
    let mine = call(&agent, "members.mine", json!({})).await;
    assert_eq!((mine[1]["grant"]["scope"].as_str(), mine[1]["grant"]["role"].as_str()), (Some("A"), Some("viewer")), "{mine:?}");
}

// ── Over the wire, login off ─────────────────────────────────────────────

/// Login off (prod today): no gate. `_info` is what it was, every method's
/// params are what they were (no `reach` leaks into a schema), the schema
/// carries no view, and an anonymous caller is answered.
#[tokio::test]
async fn login_off_menger_is_as_it_was() {
    let dir = tmp();
    let s = spawn(&dir.path().join("t.db"), false).await;
    let c = connect(s.port, None).await;
    assert_eq!(info(&c).await.to_string(), r#"{"auth_capabilities":{"mechanisms":[{"kind":"anonymous"}]},"backend":"menger"}"#);
    let hub = one(&c, "menger.schema", json!({})).await;
    assert!(hub.get("view").is_none());
    let kids: BTreeSet<String> = hub["children"].as_array().unwrap().iter().map(|k| k["namespace"].as_str().unwrap().to_string()).collect();
    assert_eq!(kids, names(&["comments", "deployments", "members", "notes", "session", "themes", "tickets"]), "no auth activation without login");
    for ns in ["tickets", "comments", "notes", "members", "session", "themes", "deployments"] {
        let schema = one(&c, &format!("{ns}.schema"), json!({})).await;
        assert!(schema.get("view").is_none());
        for m in schema["methods"].as_array().unwrap() {
            assert!(m.get("held_on").is_none(), "{m}");
            let props = m["params"]["properties"].as_object().cloned().unwrap_or_default();
            // THEME-8: `themes.set`'s `reach` is the setting's own (how far a
            // theme takes over — a string), declared by the method; the
            // gate's answer it does not take.
            let own_reach = ns == "themes" && m["name"] == "set" && props["reach"]["type"] != json!(null);
            assert!((own_reach || !props.contains_key("reach")) && !props.contains_key("auth"), "{ns}.{}: {props:?}", m["name"]);
        }
    }
    let tickets = one(&c, "tickets.schema", json!({})).await;
    let set_status = tickets["methods"].as_array().unwrap().iter().find(|m| m["name"] == "set_status").unwrap();
    let mut props: Vec<&String> = set_status["params"]["properties"].as_object().unwrap().keys().collect();
    props.sort();
    assert_eq!(props, ["id", "message", "restore_children", "source", "status"]);
    assert_eq!(set_status["params"]["required"], json!(["id", "status"]));
    // Additive only: the grant and target the protocol adds.
    assert_eq!((set_status["grant"].as_str(), set_status["target"].as_str()), (Some("tickets.set_status"), Some("id")));

    // Anonymous calls answered, writes included.
    assert_eq!(one(&c, "tickets.create", json!({ "epic": "A", "title": "root", "kind": "scope" })).await["type"], "created");
    assert_eq!(one(&c, "tickets.set_status", json!({ "id": "A-1", "status": "Ready" })).await["type"], "status_changed");
    assert_eq!(call(&c, "tickets.list", json!({})).await.len(), 2, "a ticket and its page");
    assert_eq!(one(&c, "session.whoami", json!({})).await, json!({ "type": "anonymous" }));
    assert!(call(&c, "members.requests", json!({})).await.is_empty(), "everyone is admin everywhere, and no one waits");
    let e = one(&c, "auth.providers", json!({})).await;
    assert_eq!(e["type"], "error", "no auth activation: {e}");
}
