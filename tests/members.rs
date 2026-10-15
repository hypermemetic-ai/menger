//! IDENTITY-21: members and roles per shortcode, over the real socket.
//!
//! Ada is the operator's admin (`--admin`). Alice is granted editor on
//! IDENTITY (hosted under PLEXUS-1) and viewer on OUGHT; Bob starts with
//! nothing. Every read is scoped, every write checked, every comment signed.
//! Tokens come from a fixture plexus-idp-shaped issuer on loopback.

use axum::{routing::get, Json, Router};
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use menger::build_rpc_offering;
use menger::login::{self, Provider};
use menger::sessions::{Gate, Sessions};
use menger_tickets_api::access::Access;
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

const KEY_2_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC/iZM3Lp2jX5h6
T5hf/XtB9Vw2qSyEHXgvDMh7djnhOJnBm7g0375g+xtv1T2MuJgomsaU5WPnOUq1
8imYFlQPjstvWSU8Kw7JKYmdkMhjlyOQJFfH/Cj2BvoQE3HE67juVOKsybX0FamA
PPg4/e8RDMz5EvKVNNnfH/Eo71rqb0fX57BCxTcQscZUOmayi9XjyMq35dUeGFhT
GAbITVrfI+57TuADpXbotljeJI8E0knAgz/hzGgucl03sA19UEk+k5tHVBauNcFG
/7HH7C3DUsWDxnRMrTZ5m18DsFwkE7s+TT/r9fcZh32kt1rQ5CwWCf4uyZVOJOsM
RJLX6UaBAgMBAAECggEAMqU9AP1Zf2Z6mfTL9K3A1rr7DBUFiVWFfuNha4viWBQw
S8pSFeEHpPsg0RxQbxIsYagzVBGnre8vOxbyOp3E0mxOjH3E47j66uQJ2Fj9M6A2
Lhn+AApEBnHn0zJhBdHSj2pwmYGolAbaT+dPNzql6Rs6Y63H6P4VkfMPQGSx5ITc
6B+//A3LiZAPWbe4QDu4dN3PAykzjTEvs2O+qXSG2Bj+bv4d5zG6Lr4YQrfVNUO1
edH40uDwgt41TEwdNPH0cDwdaW3fLS7xw8Htmyy2QKK1ABxq36qoJaNIu7jaym1I
+JAIPLnCtoQFilTCDSz2B8F80nR5ZcqyjZczuwPVlQKBgQDrN6s9ov7IUlikhCY2
hjcnrzCxOKzYT7s3Jrr4kcBt7ulQgolo6c07F3rTSh2RVe5TCczU+nQ3ajIZ9+Z0
ntILTpnSp9+EIfpt+k0NzKiGOKzDq00bGIDlt0gTNvJ0vnHH40x1jQuZrepzXEJu
zfRY7z889AyP5/tWXKMyGeBV+wKBgQDQdeqxWlSzrvm61YOmfX5fz8egOfqPt47q
ebU7/c4OEJlNvX6G9mUlot74KOhPBX2XWMAKPyS7C0TEF1VIT71vUguEM5CDjW0z
FZHea/KLd8i21NVtpfYK4PkK9hjpjLh8+3dvUGTL42Gm9Z7bShzR8b/jXtqRsY/g
0kqRRKb4swKBgEUHXFDFYeImEG+PfKtprgwOZMrNqCP/GiEwU5SZKZDZmU0QUgUh
ACLEXD5ftNevETb7XEpwieStXLC0SMSWy2uYEJp6u6TKV/UojK5tDlP9k+4Eeqdm
BIXlyNgiuvq53ShdM1YYI3xhRrm+LJzaAkiLRdK8iGc/HEqW+ym74FM7AoGAavbW
gkJzi++QvMmqT9e87LTVHeYiJ3RspOvmju3guV7TCwzcy6vKotE7z+JNsZ6DnxEv
GRLlagSSOHwwinZAIcrble5PjPEYw0miG5sQTXgdSZNUIHs0EMj3gSReDBjk4Vy3
ICsETYpTJTSLWsJgn2mIqMaXKIMP7LB7CqdLdfkCgYEAmj473lfYT1iLmEocy/+C
b/5rWyUCu4XxBmLVdJcJVezQpXg3Xyr9v84jqHg8PFEpIAEqE1uykJ+i3GaSYJae
y76z91PE0xvQex21id3PvJoJo5w4FUsVWAx0HorzOILaLYohoYvLfEe9xEvlHFrd
h+pCFWMecrkGFQZJTLoU4FY=
-----END PRIVATE KEY-----";

const JWK_2: &str = r#"{"kty": "RSA", "use": "sig", "alg": "RS256", "kid": "idp-1", "n": "v4mTNy6do1-Yek-YX_17QfVcNqkshB14LwzIe3Y54TiZwZu4NN--YPsbb9U9jLiYKJrGlOVj5zlKtfIpmBZUD47Lb1klPCsOySmJnZDIY5cjkCRXx_wo9gb6EBNxxOu47lTirMm19BWpgDz4OP3vEQzM-RLylTTZ3x_xKO9a6m9H1-ewQsU3ELHGVDpmsovV48jKt-XVHhhYUxgGyE1a3yPue07gA6V26LZY3iSPBNJJwIM_4cxoLnJdN7ANfVBJPpObR1QWrjXBRv-xx-wtw1LFg8Z0TK02eZtfA7BcJBO7Pk0_6_X3GYd9pLda0OQsFgn-LsmVTiTrDESS1-lGgQ", "e": "AQAB"}"#;


const ADA: &str = "a0000000-0000-4000-8000-00000000000a";
const ALICE: &str = "a0000000-0000-4000-8000-0000000000a1";
const BOB: &str = "b0000000-0000-4000-8000-0000000000b0";

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
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

struct World {
    port: u16,
    iss: String,
}

async fn start() -> World {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let iss = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let discovery = json!({ "issuer": iss, "jwks_uri": format!("{iss}/jwks.json") });
    let jwks = json!({ "keys": [serde_json::from_str::<Value>(JWK_2).unwrap()] });
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(move || async move { Json(discovery) }))
        .route("/jwks.json", get(move || async move { Json(jwks) }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let port = free_port().await;
    let providers: Vec<Provider> = vec![format!("idp={iss},plexus:menger").parse().unwrap()];
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let sessions = Arc::new(Sessions::new(3600, &menger::login_names()));
    let access = Access::with_login(menger::session_table(&sessions), true, vec![format!("idp:{ADA}")]);
    let offers = providers.iter().map(menger::offer).collect();
    let hub = build_rpc_offering(&dir.path().join("t.db"), None, Default::default(), offers, access.clone()).await.unwrap();
    // AUTH-7: as main wires it, the upgrade notices who signs in holding nothing.
    let gate = Arc::new(Gate::new(sessions, login::boundary(&providers).unwrap()).noticing(menger::MengerAccess(access)));
    let conv = |arc: Arc<DynamicHub>| DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(hub, conv)
            .with_websocket(port)
            .with_session_validator(gate)
            .reject_upgrade_on_auth_failure()
            .build().await.unwrap().serve().await.unwrap();
    });
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    World { port, iss }
}

async fn as_(w: &World, sub: &str, name: &str) -> WsClient {
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some("idp-1".into());
    let token = encode(&h, &json!({ "iss": w.iss, "aud": "plexus:menger", "sub": sub, "exp": now() + 600,
        "iat": now() - 5, "username": name }), &EncodingKey::from_rsa_pem(KEY_2_PEM.as_bytes()).unwrap()).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", HeaderValue::from_str(&format!("Bearer {token}")).unwrap());
    WsClientBuilder::default().set_headers(headers).build(format!("ws://127.0.0.1:{}", w.port)).await.unwrap()
}

async fn sub(c: &WsClient, method: &str, params: Value) -> Subscription<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap()
}

async fn call(c: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut s = sub(c, method, params).await;
    let mut out = Vec::new();
    while let Some(Ok(item)) = s.next().await {
        match item["type"].as_str() {
            Some("data") => out.push(item["content"].clone()),
            // AUTH-7: the gate's refusal, typed, is the stream's error item.
            Some("error") => {
                out.push(item);
                break;
            }
            _ => break,
        }
    }
    out
}

/// AUTH-7: the gate's typed refusal: `(grant, scope)`.
fn refused(evs: &[Value]) -> (String, String) {
    let e = &evs[0];
    assert_eq!(e["error_kind"], "forbidden", "{evs:?}");
    (e["grant"].as_str().unwrap().to_string(), e["scope"].as_str().unwrap().to_string())
}

fn ids(evs: &[Value]) -> BTreeSet<String> {
    evs.iter().filter_map(|e| e["ticket"]["id"].as_str().or(e["summary"]["id"].as_str()).map(str::to_string)).collect()
}

fn set(xs: &[&str]) -> BTreeSet<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// PLEXUS-1 · PLEXUS-2; IDENTITY-1 (hosted under PLEXUS-1) · IDENTITY-2;
/// OUGHT-1 · OUGHT-2 — made by Ada.
async fn tickets(ada: &WsClient) {
    for (epic, parent) in [("PLEXUS", None), ("IDENTITY", Some("PLEXUS-1")), ("OUGHT", None)] {
        let made = call(ada, "tickets.create", json!({ "epic": epic, "title": format!("{epic} root"), "kind": "scope", "parent": parent })).await;
        assert_eq!(made[0]["type"], "created", "{made:?}");
        let made = call(ada, "tickets.create", json!({ "epic": epic, "title": format!("{epic} work"), "parent": format!("{epic}-1") })).await;
        assert_eq!(made[0]["type"], "created", "{made:?}");
    }
}

fn grant(principal: &str, scope: &str, role: &str) -> Value {
    json!({ "principal": format!("idp:{principal}"), "scope": scope, "role": role })
}

#[tokio::test]
async fn a_member_sees_and_changes_only_what_their_roles_cover() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    let alice = as_(&w, ALICE, "alice").await;

    // No grant yet: nothing to see, and Alice waits for an admin. AUTH-7:
    // the gate's typed refusal — tickets.list, held nowhere.
    let none = call(&alice, "tickets.list", json!({})).await;
    assert_eq!(none[0]["type"], "error");
    assert_eq!(call(&alice, "tickets.search", json!({ "query": "root" })).await[0]["type"], "error", "SEARCH-2: nor search");
    assert_eq!(refused(&none), ("tickets.list".to_string(), "*".to_string()));
    assert_eq!((none[0]["roles"].clone(), none[0]["held_on"].clone()), (json!(["editor", "viewer", "admin"]), json!([])), "{none:?}");
    let waiting = call(&ada, "members.requests", json!({})).await;
    assert_eq!(waiting[0]["principal"], format!("idp:{ALICE}"));

    // Ada grants; Alice's next calls see IDENTITY and OUGHT, not PLEXUS.
    assert_eq!(call(&ada, "members.grant", grant(ALICE, "IDENTITY", "editor")).await[0]["type"], "grant");
    assert_eq!(call(&ada, "members.grant", grant(ALICE, "ought", "viewer")).await[0]["grant"]["scope"], "hypermemetic:OUGHT");
    let seen = ids(&call(&alice, "tickets.list", json!({})).await);
    assert_eq!(seen, set(&["IDENTITY-1", "IDENTITY-2", "OUGHT-1", "OUGHT-2"]));
    let ready = ids(&call(&alice, "tickets.list", json!({ "filter": "kind build" })).await);
    assert_eq!(ready, set(&["IDENTITY-2", "OUGHT-2"]), "her filter, within her reach");
    // SEARCH-2: a search inside tickets sees her reach too — hits and count.
    let found = call(&alice, "tickets.search", json!({ "query": "root" })).await;
    assert_eq!(ids(&found), set(&["IDENTITY-1", "OUGHT-1"]), "{found:?}");
    assert_eq!(found.last().unwrap()["matched"], 2, "PLEXUS-1 is not counted either");
    assert_eq!(call(&alice, "tickets.get", json!({ "id": "PLEXUS-2" })).await[0]["type"], "not_found", "no leak of existence");
    let groups = call(&alice, "tickets.groups", json!({})).await;
    let epics: BTreeSet<String> = groups.iter().map(|g| g["group"]["epic"].as_str().unwrap().to_string()).collect();
    assert_eq!(epics, set(&["IDENTITY", "OUGHT"]));
    let identity = groups.iter().find(|g| g["group"]["epic"] == "IDENTITY").unwrap();
    assert_eq!(identity["group"]["host"], Value::Null, "its host, PLEXUS-1, is out of reach: IDENTITY stands at the top");

    // Writes: editor on IDENTITY only.
    let ok = call(&alice, "tickets.amend", json!({ "id": "IDENTITY-2", "title": "renamed" })).await;
    assert_eq!(ok[0]["type"], "amended", "{ok:?}");
    let no = call(&alice, "tickets.amend", json!({ "id": "OUGHT-2", "title": "no" })).await;
    // ORG-4: refused by the ticket as written — the same words for one out
    // of reach as for none; held on IDENTITY in its organization.
    assert_eq!(refused(&no), ("tickets.amend".to_string(), "OUGHT-2".to_string()), "AUTH-7: was \"needs editor on OUGHT\"");
    assert_eq!(no[0]["held_on"], json!(["hypermemetic:IDENTITY"]), "{no:?}");
    assert_eq!(call(&alice, "tickets.set_status", json!({ "id": "PLEXUS-2", "status": "Ready" })).await[0]["type"], "error");
    assert_eq!(call(&alice, "tickets.create", json!({ "epic": "IDENTITY", "title": "new", "parent": "IDENTITY-1" })).await[0]["type"], "created");
    assert_eq!(call(&alice, "tickets.create", json!({ "epic": "OUGHT", "title": "no" })).await[0]["type"], "error");
    assert_eq!(call(&alice, "members.grant", grant(BOB, "IDENTITY", "viewer")).await[0]["type"], "error", "not an admin");

    // A grant on PLEXUS reaches PLEXUS; a revoke takes OUGHT away.
    call(&ada, "members.grant", grant(ALICE, "PLEXUS", "viewer")).await;
    call(&ada, "members.revoke", json!({ "principal": format!("idp:{ALICE}"), "scope": "OUGHT" })).await;
    let seen = ids(&call(&alice, "tickets.list", json!({})).await);
    assert_eq!(seen, set(&["PLEXUS-1", "PLEXUS-2", "IDENTITY-1", "IDENTITY-2", "IDENTITY-3"]));
    let mine = call(&alice, "members.mine", json!({})).await;
    assert_eq!(mine[0]["type"], "me");
    assert_eq!(mine.len(), 3, "me, then IDENTITY editor and PLEXUS viewer: {mine:?}");
}

/// NOTE-2 under login: a note is on no shortcode, so any member's — added,
/// listed and read with a role held anywhere, signed and named by its
/// writer; nobody's without one. An upgrade makes a ticket, so it needs
/// what tickets.create needs where the ticket lands.
#[tokio::test]
async fn notes_are_any_members_and_an_upgrade_needs_what_create_needs() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    let alice = as_(&w, ALICE, "alice").await;
    assert_eq!(call(&alice, "notes.add", json!({ "body": "before any grant" })).await[0]["type"], "error", "no role: no notes");
    assert_eq!(call(&alice, "notes.list", json!({})).await[0]["type"], "error");

    call(&ada, "members.grant", grant(ALICE, "IDENTITY", "editor")).await;
    call(&ada, "members.grant", grant(ALICE, "OUGHT", "viewer")).await;
    let hers = call(&alice, "notes.add", json!({ "body": "alice's thought", "author": "someone else" })).await;
    assert_eq!((&hers[0]["type"], &hers[0]["author"], &hers[0]["principal"]), (&json!("note"), &json!("alice"), &json!(format!("idp:{ALICE}"))), "{hers:?}");
    let adas = call(&ada, "notes.add", json!({ "body": "ada's thought" })).await;
    let listed: Vec<Value> = call(&alice, "notes.list", json!({})).await.into_iter().filter(|e| e["type"] == "note").collect();
    assert_eq!(listed.len(), 2, "one place: every member's notes, {listed:?}");

    // Editor on IDENTITY: the ticket lands there. Viewer on OUGHT: refused,
    // and the note stays open. A new shortcode needs tickets.create on *.
    let up = call(&alice, "notes.upgrade", json!({ "id": hers[0]["id"], "epic": "IDENTITY", "parent": "IDENTITY-1" })).await;
    assert_eq!(up[0]["ticket"], "IDENTITY-3", "{up:?}");
    let no = call(&alice, "notes.upgrade", json!({ "id": adas[0]["id"], "epic": "OUGHT" })).await;
    assert_eq!(no[0]["type"], "error", "{no:?}");
    let new = call(&alice, "notes.upgrade", json!({ "id": adas[0]["id"], "epic": "NEWCODE" })).await;
    assert!(new[0]["message"].as_str().unwrap_or_default().contains("on *"), "{new:?}");
    assert!(call(&alice, "notes.get", json!({ "id": adas[0]["id"] })).await[0]["ticket"].is_null(), "still open");
    let made = call(&ada, "notes.upgrade", json!({ "id": adas[0]["id"], "epic": "OUGHT" })).await;
    assert_eq!(made[0]["ticket"], "OUGHT-3", "an admin's upgrade: {made:?}");
}

#[tokio::test]
async fn a_watch_hears_only_what_the_watcher_can_see() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    call(&ada, "members.grant", grant(BOB, "OUGHT", "viewer")).await;
    let bob = as_(&w, BOB, "bob").await;
    let mut watch = sub(&bob, "tickets.watch", json!({})).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    call(&ada, "tickets.amend", json!({ "id": "IDENTITY-2", "title": "unseen" })).await;
    call(&ada, "tickets.amend", json!({ "id": "OUGHT-2", "title": "seen" })).await;
    let mut heard = Vec::new();
    while let Ok(Some(Ok(item))) = tokio::time::timeout(Duration::from_millis(800), watch.next()).await {
        if item["type"] == "data" && item["content"]["type"] == "amended" {
            heard.push(item["content"]["ticket"]["id"].as_str().unwrap().to_string());
        }
    }
    assert_eq!(heard, vec!["OUGHT-2".to_string()], "IDENTITY-2's change is not Bob's to hear");
}

#[tokio::test]
async fn comments_are_signed_and_scoped() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    call(&ada, "members.grant", grant(ALICE, "IDENTITY", "editor")).await;
    call(&ada, "members.grant", grant(BOB, "OUGHT", "viewer")).await;
    let (alice, bob) = (as_(&w, ALICE, "alice").await, as_(&w, BOB, "bob").await);
    let on = |t: &str| json!({ "anchor": { "route": format!("#/ticket/{t}"), "path": "main", "label": "body", "excerpt": "", "ticket": t }, "body": "note", "author": "ada" });

    let a = call(&alice, "comments.add", on("IDENTITY-2")).await;
    assert_eq!((a[0]["author"].as_str(), a[0]["principal"].as_str()), (Some("alice"), Some(format!("idp:{ALICE}").as_str())), "the name claimed is ignored: {a:?}");
    assert_eq!(call(&bob, "comments.add", on("IDENTITY-2")).await[0]["type"], "error", "Bob cannot see IDENTITY-2");
    assert_eq!(call(&bob, "comments.add", on("OUGHT-2")).await[0]["principal"], format!("idp:{BOB}"), "a viewer comments (G1)");
    let bobs: Vec<Value> = call(&bob, "comments.list", json!({})).await.into_iter().filter(|e| e["type"] == "comment").collect();
    assert_eq!(bobs.len(), 1, "Bob lists only the comment on OUGHT: {bobs:?}");
    let id = a[0]["id"].as_u64().unwrap();
    assert_eq!(call(&bob, "comments.resolve", json!({ "id": id })).await[0]["type"], "error");
    assert_eq!(call(&alice, "comments.resolve", json!({ "id": id })).await[0]["state"], "resolved");
}

// ── IDENTITY-37: every read and every stream within the grants as they stand ──

/// A watch's data items until `quiet` passes with nothing; whether it ended.
async fn drain(watch: &mut Subscription<Value>, quiet: Duration) -> (Vec<Value>, bool) {
    let mut heard = Vec::new();
    loop {
        match tokio::time::timeout(quiet, watch.next()).await {
            Ok(Some(Ok(item))) if item["type"] == "data" => heard.push(item["content"].clone()),
            Ok(Some(Ok(_))) | Ok(None) | Ok(Some(Err(_))) => return (heard, true),
            Err(_) => return (heard, false),
        }
    }
}

fn amended(heard: &[Value]) -> Vec<String> {
    heard.iter().filter(|e| e["type"] == "amended").map(|e| e["ticket"]["id"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn blockers_answer_only_what_the_caller_can_view_and_blocked_by_names_only_that() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    let made = call(&ada, "tickets.create", json!({ "epic": "IDENTITY", "title": "waits", "parent": "IDENTITY-1", "blocked_by": ["OUGHT-2", "IDENTITY-2"] })).await;
    assert_eq!(made[0]["ticket"]["id"], "IDENTITY-3", "{made:?}");
    call(&ada, "members.grant", grant(ALICE, "IDENTITY", "editor")).await;
    let alice = as_(&w, ALICE, "alice").await;

    let b = call(&alice, "tickets.blockers", json!({ "id": "IDENTITY-3" })).await;
    assert_eq!(b.len(), 2, "{b:?}");
    assert_eq!((b[0]["type"].as_str(), b[0]["id"].as_str()), (Some("dangling_blocker"), Some("OUGHT-2")), "nothing of OUGHT-2 but its id: {b:?}");
    assert!(b[0].get("ticket").is_none());
    assert_eq!((b[1]["type"].as_str(), b[1]["ticket"]["id"].as_str()), (Some("blocker"), Some("IDENTITY-2")));
    // Ada sees both.
    let b = call(&ada, "tickets.blockers", json!({ "id": "IDENTITY-3" })).await;
    assert_eq!(b.iter().filter(|e| e["type"] == "blocker").count(), 2, "{b:?}");

    // blocked_by: a hidden ticket, one naming none in a hidden shortcode, and
    // a shortcode that does not exist get the same answer, by shortcode.
    let refused = |id: &str| json!({ "epic": "IDENTITY", "title": "x", "parent": "IDENTITY-1", "blocked_by": [id] });
    let hidden = call(&alice, "tickets.create", refused("OUGHT-2")).await;
    let none = call(&alice, "tickets.create", refused("OUGHT-99")).await;
    assert_eq!(hidden[0]["type"], "error", "{hidden:?}");
    assert_eq!(
        hidden[0]["message"].as_str().unwrap().replace("OUGHT-2", "#"),
        none[0]["message"].as_str().unwrap().replace("OUGHT-99", "#"),
        "an existing hidden ticket and a missing one are told apart by nothing"
    );
    assert!(hidden[0]["message"].as_str().unwrap().contains("needs tickets.get on OUGHT"), "{hidden:?}");
    let nowhere = call(&alice, "tickets.create", refused("NOWHERE-1")).await;
    assert!(nowhere[0]["message"].as_str().unwrap().contains("needs tickets.get on NOWHERE"), "{nowhere:?}");
    // In her own shortcode, a forward reference stands as it always did.
    assert_eq!(call(&alice, "tickets.create", refused("IDENTITY-99")).await[0]["type"], "created");
}

#[tokio::test]
async fn a_watch_follows_the_grants_as_they_stand_and_ends_when_nothing_is_left() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    call(&ada, "members.grant", grant(BOB, "OUGHT", "viewer")).await;
    let bob = as_(&w, BOB, "bob").await;
    let mut watch = sub(&bob, "tickets.watch", json!({})).await;
    let mut comments = sub(&bob, "comments.watch", json!({})).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    call(&ada, "tickets.amend", json!({ "id": "OUGHT-2", "title": "seen" })).await;
    let (heard, ended) = drain(&mut watch, Duration::from_millis(500)).await;
    assert_eq!((amended(&heard), ended), (vec!["OUGHT-2".to_string()], false));

    // OUGHT revoked, IDENTITY granted: the open watch follows.
    call(&ada, "members.revoke", json!({ "principal": format!("idp:{BOB}"), "scope": "OUGHT" })).await;
    call(&ada, "members.grant", grant(BOB, "IDENTITY", "viewer")).await;
    call(&ada, "tickets.amend", json!({ "id": "OUGHT-2", "title": "no longer Bob's", "body": "secret" })).await;
    call(&ada, "tickets.amend", json!({ "id": "IDENTITY-2", "title": "now Bob's" })).await;
    let (heard, ended) = drain(&mut watch, Duration::from_millis(500)).await;
    assert_eq!((amended(&heard), ended), (vec!["IDENTITY-2".to_string()], false), "{heard:?}");
    assert!(!heard.iter().any(|e| e.to_string().contains("secret")), "{heard:?}");

    // Comments follow too.
    let on = |t: &str| json!({ "anchor": { "route": format!("#/ticket/{t}"), "path": "main", "label": "body", "excerpt": "", "ticket": t }, "body": format!("on {t}") });
    call(&ada, "comments.add", on("OUGHT-2")).await;
    call(&ada, "comments.add", on("IDENTITY-2")).await;
    let (heard, _) = drain(&mut comments, Duration::from_millis(500)).await;
    let bodies: Vec<&str> = heard.iter().filter_map(|e| e["comment"]["body"].as_str()).collect();
    assert_eq!(bodies, vec!["on IDENTITY-2"], "{heard:?}");

    // Nothing left: both end, in band.
    call(&ada, "members.revoke", json!({ "principal": format!("idp:{BOB}"), "scope": "IDENTITY" })).await;
    call(&ada, "tickets.amend", json!({ "id": "IDENTITY-2", "title": "gone" })).await;
    call(&ada, "comments.add", on("IDENTITY-2")).await;
    let (heard, ended) = drain(&mut watch, Duration::from_millis(800)).await;
    assert!(ended, "the watch ends: {heard:?}");
    assert!(amended(&heard).is_empty(), "{heard:?}");
    assert!(heard.last().unwrap()["message"].as_str().unwrap().contains("your access changed"), "{heard:?}");
    let (heard, ended) = drain(&mut comments, Duration::from_millis(800)).await;
    assert!(ended && heard.last().unwrap()["type"] == "error", "{heard:?}");
}

#[tokio::test]
async fn a_filtered_watch_is_refiltered_when_the_grants_move() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    call(&ada, "members.grant", grant(BOB, "OUGHT", "viewer")).await;
    let bob = as_(&w, BOB, "bob").await;
    let mut watch = sub(&bob, "tickets.watch", json!({ "filter": "kind build" })).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    call(&ada, "members.revoke", json!({ "principal": format!("idp:{BOB}"), "scope": "OUGHT" })).await;
    call(&ada, "members.grant", grant(BOB, "IDENTITY", "viewer")).await;
    // The first change after the grants moved re-filters the watch.
    call(&ada, "tickets.create", json!({ "epic": "OUGHT", "title": "hidden", "parent": "OUGHT-1" })).await;
    call(&ada, "tickets.create", json!({ "epic": "IDENTITY", "title": "seen", "parent": "IDENTITY-1" })).await;
    let (heard, ended) = drain(&mut watch, Duration::from_millis(600)).await;
    assert!(!ended);
    let named: Vec<String> = heard
        .iter()
        .filter_map(|e| e["ticket"]["id"].as_str().or(e["id"].as_str()).map(str::to_string))
        .collect();
    assert!(!named.iter().any(|i| i.starts_with("OUGHT")), "{heard:?}");
    assert!(heard.iter().any(|e| e["type"] == "entered" && e["id"] == "IDENTITY-3"), "{heard:?}");
}

#[tokio::test]
async fn components_reach_only_those_who_can_view_their_owner() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    call(&ada, "members.grant", grant(ALICE, "IDENTITY", "viewer")).await;
    call(&ada, "members.grant", grant(BOB, "OUGHT", "viewer")).await;
    let (alice, bob) = (as_(&w, ALICE, "alice").await, as_(&w, BOB, "bob").await);
    let mut watch = sub(&alice, "tickets.watch", json!({})).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let a = call(&ada, "tickets.amend", json!({ "id": "OUGHT-2", "body": "```component m-secret\n<b>secret source</b>\n```\n" })).await;
    assert_eq!(a[0]["type"], "amended", "{a:?}");
    call(&ada, "tickets.amend", json!({ "id": "IDENTITY-2", "body": "```component m-mine\n<p>mine</p>\n```\n\n<m-secret></m-secret>\n" })).await;

    let names = |evs: &[Value]| -> Vec<String> { evs.iter().filter_map(|e| e["component"]["name"].as_str().map(str::to_string)).collect() };
    let hers = call(&alice, "tickets.components", json!({})).await;
    assert_eq!(names(&hers), vec!["m-mine".to_string()], "{hers:?}");
    let his = call(&bob, "tickets.components", json!({})).await;
    assert_eq!(names(&his), vec!["m-secret".to_string()], "{his:?}");
    assert_eq!(his[0]["component"]["references"], json!([]), "IDENTITY-2's use of it is not Bob's to see: {his:?}");
    let all = call(&ada, "tickets.components", json!({})).await;
    assert_eq!(names(&all).len(), 2);

    let (heard, _) = drain(&mut watch, Duration::from_millis(500)).await;
    let changed: Vec<&str> = heard.iter().filter(|e| e["type"] == "component_changed").filter_map(|e| e["component"]["name"].as_str()).collect();
    assert_eq!(changed, vec!["m-mine"], "{heard:?}");
    assert!(!heard.iter().any(|e| e.to_string().contains("secret source")), "{heard:?}");
}

#[tokio::test]
async fn moving_a_shortcode_s_root_takes_admin_on_both_sides() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    // Editor on OUGHT, admin on PLEXUS: hosting OUGHT under PLEXUS would
    // make her its admin.
    call(&ada, "members.grant", grant(ALICE, "OUGHT", "editor")).await;
    call(&ada, "members.grant", grant(ALICE, "PLEXUS", "admin")).await;
    let alice = as_(&w, ALICE, "alice").await;
    let r = call(&alice, "tickets.set_parent", json!({ "id": "OUGHT-1", "parent": "PLEXUS-2" })).await;
    assert_eq!(r[0]["type"], "error", "{r:?}");
    assert!(r[0]["message"].as_str().unwrap().contains("takes admin"), "{r:?}");
    assert_eq!(call(&alice, "members.grant", grant(BOB, "OUGHT", "viewer")).await[0]["type"], "error", "still not OUGHT's admin");
    // A ticket inside its shortcode moves with editor, as before.
    call(&ada, "tickets.create", json!({ "epic": "OUGHT", "title": "three", "parent": "OUGHT-1" })).await;
    assert_eq!(call(&alice, "tickets.set_parent", json!({ "id": "OUGHT-3", "parent": "OUGHT-2" })).await[0]["type"], "parent_changed");
    // Admin on both sides: it moves.
    call(&ada, "members.grant", grant(ALICE, "OUGHT", "admin")).await;
    let r = call(&alice, "tickets.set_parent", json!({ "id": "OUGHT-1", "parent": "PLEXUS-2" })).await;
    assert_eq!(r[0]["type"], "parent_changed", "{r:?}");
}

#[tokio::test]
async fn the_waiting_list_takes_admin_on_everything() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    let bob = as_(&w, BOB, "bob").await;
    assert_eq!(call(&bob, "tickets.list", json!({})).await[0]["type"], "error");
    call(&ada, "members.grant", grant(ALICE, "IDENTITY", "admin")).await;
    let alice = as_(&w, ALICE, "alice").await;
    let r = call(&alice, "members.requests", json!({})).await;
    assert!(r[0]["message"].as_str().unwrap().contains("admin on *"), "{r:?}");
    let r = call(&ada, "members.requests", json!({})).await;
    assert_eq!(r[0]["principal"], format!("idp:{BOB}"), "{r:?}");
}

#[tokio::test]
async fn a_comment_on_a_ticket_s_page_is_that_ticket_s() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    call(&ada, "members.grant", grant(BOB, "OUGHT", "viewer")).await;
    let bob = as_(&w, BOB, "bob").await;
    let app = |route: &str| json!({ "anchor": { "route": route, "path": "header", "label": "title", "excerpt": "" }, "body": "on the page" });
    let r = call(&bob, "comments.add", app("#/ticket/IDENTITY-2")).await;
    assert_eq!(r[0]["type"], "error", "Bob cannot see IDENTITY-2's page: {r:?}");
    let r = call(&ada, "comments.add", app("#/ticket/IDENTITY-2")).await;
    assert_eq!(r[0]["anchor"]["ticket"], Value::Null, "the anchor as given: {r:?}");
    let id = r[0]["id"].as_u64().unwrap();
    assert_eq!(call(&bob, "comments.resolve", json!({ "id": id })).await[0]["type"], "error");
    assert_eq!(call(&bob, "comments.add", app("#/")).await[0]["type"], "comment", "a comment on the board is the app's");
    let his: Vec<Value> = call(&bob, "comments.list", json!({})).await.into_iter().filter(|e| e["type"] == "comment").collect();
    assert_eq!(his.len(), 1, "{his:?}");
}

/// ORG-4: Ada (admin on `*`) makes the organization acme with Bob as its
/// admin. Bob files acme's own root MENGER without naming the org (his
/// only one), grants Alice viewer on acme, and can reach nothing of
/// hypermemetic — its tickets, counts and names are not found to him, and
/// a write there is refused naming only what he wrote.
#[tokio::test]
async fn org_4_a_reader_of_one_org_sees_nothing_of_the_other() {
    let w = start().await;
    let ada = as_(&w, ADA, "ada").await;
    tickets(&ada).await;
    for title in ["hypermemetic's menger", "a build in it"] {
        let kind = if title.starts_with("hyper") { "scope" } else { "build" };
        assert_eq!(call(&ada, "tickets.create", json!({ "epic": "MENGER", "title": title, "kind": kind })).await[0]["type"], "created");
    }
    let made = call(&ada, "orgs.create", json!({ "name": "acme", "admin": format!("idp:{BOB}") })).await;
    assert_eq!((made[0]["type"].as_str(), made[0]["admin"]["scope"].as_str()), (Some("created"), Some("acme:")), "{made:?}");
    assert_eq!(call(&ada, "orgs.create", json!({ "name": "acme" })).await[0]["type"], "error", "a name is unique");

    let bob = as_(&w, BOB, "bob").await;
    let orgs: Vec<String> = call(&bob, "orgs.list", json!({})).await.iter().filter_map(|e| e["org"]["name"].as_str().map(str::to_string)).collect();
    assert_eq!(orgs, ["acme"], "Bob holds a role in acme alone");
    // No --org: Bob's only organization — and acme has a root MENGER of its own.
    let root = call(&bob, "tickets.create", json!({ "epic": "MENGER", "title": "acme's menger", "kind": "scope" })).await;
    assert_eq!(root[0]["ticket"]["address"], "acme:MENGER-1", "{root:?}");
    let build = call(&bob, "tickets.create", json!({ "epic": "acme:MENGER", "title": "acme's build" })).await;
    assert_eq!(build[0]["ticket"]["address"], "acme:MENGER-2", "{build:?}");
    let acme_ids = ids(&call(&bob, "tickets.list", json!({})).await);
    assert_eq!(acme_ids.len(), 2, "{acme_ids:?}");
    assert!(acme_ids.iter().all(|id| id.starts_with("MENGER.")), "acme's MENGER is a namesake: {acme_ids:?}");
    assert_eq!(call(&bob, "tickets.get", json!({ "id": "PLEXUS-1" })).await[0]["type"], "not_found");
    assert_eq!(call(&bob, "tickets.get", json!({ "id": "hypermemetic:MENGER-1" })).await[0]["type"], "not_found");
    let counted: u64 = call(&bob, "tickets.counts", json!({})).await.iter().filter_map(|e| e["count"].as_u64().or(e["held"].as_u64())).sum();
    assert_eq!(counted, 2, "counts: acme's alone");
    // The bare id fits both orgs: read in Bob's, it is acme's; Ada, of `*`,
    // is told both by their full addresses.
    let his = call(&bob, "tickets.get", json!({ "id": "MENGER-2" })).await;
    assert_eq!(his[0]["ticket"]["address"], "acme:MENGER-2", "{his:?}");
    let both = call(&ada, "tickets.get", json!({ "id": "MENGER-2" })).await;
    assert_eq!(both[0]["candidates"], json!(["hypermemetic:MENGER-2", "acme:MENGER-2"]), "{both:?}");
    // A write in hypermemetic is refused by what Bob wrote, nothing more.
    let refused_write = call(&bob, "tickets.set_status", json!({ "id": "PLEXUS-2", "status": "Ready" })).await;
    assert_eq!(refused(&refused_write), ("tickets.set_status".to_string(), "PLEXUS-2".to_string()));
    assert!(!refused_write[0].to_string().contains("hypermemetic:"), "{refused_write:?}");

    // Bob grants inside acme, and cannot reach hypermemetic.
    let g = call(&bob, "members.grant", grant(ALICE, "acme:", "viewer")).await;
    assert_eq!(g[0]["grant"]["scope"], "acme:", "{g:?}");
    assert_eq!(call(&bob, "members.grant", grant(ALICE, "hypermemetic:", "viewer")).await[0]["type"], "error");
    assert_eq!(call(&bob, "members.grant", grant(ALICE, "IDENTITY", "viewer")).await[0]["type"], "error");
    let listed: Vec<String> = call(&bob, "members.list", json!({})).await.iter().filter_map(|e| e["grant"]["scope"].as_str().map(str::to_string)).collect();
    assert!(listed.iter().all(|s| s.starts_with("acme:")), "Bob lists acme's grants alone: {listed:?}");
    assert_eq!(call(&bob, "orgs.rename", json!({ "name": "hypermemetic", "to": "mine" })).await[0]["type"], "not_found");
    assert_eq!(call(&bob, "orgs.create", json!({ "name": "beta" })).await[0]["type"], "error", "a new org takes admin on *");
    assert!(call(&bob, "orgs.delete", json!({ "name": "acme" })).await[0]["message"].as_str().unwrap().contains("holds 1 shortcode"));

    let alice = as_(&w, ALICE, "alice").await;
    assert_eq!(ids(&call(&alice, "tickets.list", json!({})).await), acme_ids, "a viewer of acme sees acme");
    assert_eq!(call(&alice, "tickets.get", json!({ "id": "acme:MENGER-2" })).await[0]["ticket"]["title"], "acme's build");
    // Ada reads both, and a filter reads one org.
    assert_eq!(ids(&call(&ada, "tickets.list", json!({ "filter": "under acme:" })).await), acme_ids);
    let renamed = call(&ada, "orgs.rename", json!({ "name": "acme", "to": "acme-co" })).await;
    assert_eq!(renamed[0]["org"]["aliases"], json!(["acme"]), "{renamed:?}");
    assert_eq!(call(&alice, "tickets.get", json!({ "id": "acme:MENGER-2" })).await[0]["ticket"]["address"], "acme-co:MENGER-2");
}
