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
    let sessions = Arc::new(Sessions::new(3600));
    let access = Access::with_login(sessions.clone(), true, vec![format!("idp:{ADA}")]);
    let offers = providers.iter().map(Provider::offer).collect();
    let hub = build_rpc_offering(&dir.path().join("t.db"), None, Default::default(), offers, access).await.unwrap();
    let gate = Arc::new(Gate::new(sessions, login::boundary(&providers).unwrap()));
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
            _ => break,
        }
    }
    out
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

    // No grant yet: nothing to see, and Alice waits for an admin.
    let none = call(&alice, "tickets.list", json!({})).await;
    assert_eq!(none[0]["type"], "error");
    assert_eq!(call(&alice, "tickets.search", json!({ "query": "root" })).await[0]["type"], "error", "SEARCH-2: nor search");
    assert!(none[0]["message"].as_str().unwrap().contains("ask an admin"), "{none:?}");
    let waiting = call(&ada, "members.requests", json!({})).await;
    assert_eq!(waiting[0]["principal"], format!("idp:{ALICE}"));

    // Ada grants; Alice's next calls see IDENTITY and OUGHT, not PLEXUS.
    assert_eq!(call(&ada, "members.grant", grant(ALICE, "IDENTITY", "editor")).await[0]["type"], "grant");
    assert_eq!(call(&ada, "members.grant", grant(ALICE, "ought", "viewer")).await[0]["grant"]["scope"], "OUGHT");
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
    let refused = call(&alice, "tickets.amend", json!({ "id": "OUGHT-2", "title": "no" })).await;
    assert!(refused[0]["message"].as_str().unwrap().contains("needs editor on OUGHT"), "{refused:?}");
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
