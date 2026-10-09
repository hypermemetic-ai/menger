//! IDENTITY-14: login at menger's boundary, over the real wire.
//!
//! Three providers, each through its own adapter, each ending as the same
//! Plexus identity on the connection: a plexus-idp token, an Auth0 token
//! and a nostr key. The two OIDC issuers are fixtures served over HTTP on
//! loopback (discovery and JWKS, as a provider publishes them), so the
//! adapters fetch keys exactly as they would from the real thing. The
//! test keys are synthetic, plexus-auth-core's test pair. Every server has
//! its own throwaway database; prod and test are never touched.

use axum::{routing::get, Json, Router};
use base64::Engine as _;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use menger::build_rpc_offering;
use menger::login::{self, Provider};
use menger::sessions::{Gate, Sessions};
use menger_tickets_api::access::Access;
use nostr::event::{EventBuilder, FinalizeEvent, Kind, Tag};
use nostr::key::Keys;
use nostr::nips::nip19::ToBech32;
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const KEY_1_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQDRK77i+LgwjT95
0Uq+AQQe9Ls/PamwwyIrbCK+23//1oufPKD6x8TSWBVRXX5adtks140U6WU89Chs
sZTqyGFP23RjXrXTE9YXUAxL+5SKfoLVmZ0frz1kaCA09oW7+J6GczbYCIp9HNr3
dF4klIh2zqzoEYS4bNgZpI/xWHIWX2BAmXOg3Nb1/F3Z6rFWgxft3QkG9+/SftJ5
wmUVqvwa9+IG/TkNhE8VTfmfm/KRvMIbinckoUDoiawAf2sk3SOS1r6XHwPV2AjO
mukGfPQ+6BaggFOqzls7mH2n9KEsASNHDAKz242sSR+mYPF4afrXSzeDsUenMK0l
4kmzw1cVAgMBAAECggEABLrihCtvrtli2BRdhlJrj2+lVFbGoZKoESdO2dYI3PYz
DhTG5yThVIhdYwukMdOCMbtmG1Tzzx8OUvbpES4a1T13MlAP+If4TWqn/Ifh4gfe
WYoxvWevEbgxEkGI4KlMnGm6kcQPraibYwEkp9scAuPFkTHkOG9tq5bHEoQXgF35
Q1OxUI+S2YFH+5gY9pUWqLTkhc1iCb2b0YWZeYVs7eQvblajiCZagNXgWDBf05sE
AylUFMUzJV12Z0udyQal8H7ObuISxa6o57124Ib4x0V/LVXtAbdZQMru/Ymx6xxk
haBt8htLcFZdRC/RDCPNsd0fjsrqpARIF8jo+rgzeQKBgQD9BDU9zph03HYqHQZU
caizwE+G7fXswuhqlxmy94L/zL/Y2XpLMzDaPjv3S8wOrYQNx8uJpxycgJ2K7slM
M9qKL3UlkjsuS7DtGLFrvBQyC2KVMrMPBaSNOwnhSVWCyCRsSiiuACIFAWS9icS3
Eb1MqYAg2gdtB3QvhDb75IBYwwKBgQDToy3b8/h6ybIqmqz9bJ4QQwzHEs43uZ/P
VXVkiXHXHJ9zfq/dIjSYkIbsNJhD8CrzhOrvM11Fpah0i1EN5uqYJ3wKC13ZCWMe
wDZstYipoVYY80/M02Vnt0iEGsoNUojTqMWxixbkSHbvUUgQQpkEQrnqHEfV+LXo
X4JnyNbTRwKBgFZsw4rzMNxqGerUsz7Q/CE6RW//hIt1IFKYfmzFYvfhhn6Z+s4J
FFzX+T/FolQ5LOxQHNROQtWqkSXN3vCqnbGp+Ef3JUPxEuRKFQCJ5BQcE3aHNOai
tMyRKBTOKelcWCStSCv3W6d+DF052/n0k0bGdz/BedviOeupK+bq7HRlAoGAeaxA
+0miO4WmBtRyTCicHyFNQU5QfL0dYafyG+DhMBjmmxHkra+yqVu+FiKOv9BeAS8T
mn3fS+FXndlSujld+igJKgUq6VJ6R/2dzJX5gfydcS7BXDLVA/HdoQV90Hb47ycC
sXYTrR70MdZ7Jc4EBu0N0ch8jEm222e9o0lWKJUCgYBXxOLoBstghiOh7+q2C3l4
g8jv2q8ISuQn2Ov+e6w9TLX9FDAGMBt+7XTeQvB4UHnzugPL4ThHPzaHSZZ2MSe0
cc/8jPBcPehKNhhgDUFkmptSZn7hFSAkzdjoC4yYxu9OsftU9M3Fvg2awbJ/j78w
ibtF9Ayucd2RXBc2n+6vZg==
-----END PRIVATE KEY-----";

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

const JWK_1: &str = r#"{"kty": "RSA", "use": "sig", "alg": "RS256", "kid": "acme-1", "n": "0Su-4vi4MI0_edFKvgEEHvS7Pz2psMMiK2wivtt__9aLnzyg-sfE0lgVUV1-WnbZLNeNFOllPPQobLGU6shhT9t0Y1610xPWF1AMS_uUin6C1ZmdH689ZGggNPaFu_iehnM22AiKfRza93ReJJSIds6s6BGEuGzYGaSP8VhyFl9gQJlzoNzW9fxd2eqxVoMX7d0JBvfv0n7SecJlFar8GvfiBv05DYRPFU35n5vykbzCG4p3JKFA6ImsAH9rJN0jkta-lx8D1dgIzprpBnz0PugWoIBTqs5bO5h9p_ShLAEjRwwCs9uNrEkfpmDxeGn610s3g7FHpzCtJeJJs8NXFQ", "e": "AQAB"}"#;

const JWK_2: &str = r#"{"kty": "RSA", "use": "sig", "alg": "RS256", "kid": "idp-1", "n": "v4mTNy6do1-Yek-YX_17QfVcNqkshB14LwzIe3Y54TiZwZu4NN--YPsbb9U9jLiYKJrGlOVj5zlKtfIpmBZUD47Lb1klPCsOySmJnZDIY5cjkCRXx_wo9gb6EBNxxOu47lTirMm19BWpgDz4OP3vEQzM-RLylTTZ3x_xKO9a6m9H1-ewQsU3ELHGVDpmsovV48jKt-XVHhhYUxgGyE1a3yPue07gA6V26LZY3iSPBNJJwIM_4cxoLnJdN7ANfVBJPpObR1QWrjXBRv-xx-wtw1LFg8Z0TK02eZtfA7BcJBO7Pk0_6_X3GYd9pLda0OQsFgn-LsmVTiTrDESS1-lGgQ", "e": "AQAB"}"#;


const AUTH0_AUD: &str = "https://menger.example";
const ROLES_CLAIM: &str = "https://menger.example/roles";
const IDP_AUD: &str = "plexus:menger";
const ALICE: &str = "c952acd3-60aa-4534-b2e3-83634591fee1";

/// A port no other test in this binary has been handed (MNG-19).
async fn free_port() -> u16 {
    static HANDED: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut handed = HANDED.lock().unwrap();
        if !handed.contains(&port) {
            handed.push(port);
            return port;
        }
    }
}

/// A fixture OIDC issuer: discovery and one key, as a provider publishes
/// them. Auth0's issuer ends in a slash; plexus-idp's does not.
async fn issuer(trailing_slash: bool, jwk: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let iss = if trailing_slash { format!("{base}/") } else { base.clone() };
    let discovery = json!({ "issuer": iss, "jwks_uri": format!("{base}/jwks.json") });
    let jwks: Value = json!({ "keys": [serde_json::from_str::<Value>(jwk).unwrap()] });
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(move || async move { Json(discovery) }))
        .route("/jwks.json", get(move || async move { Json(jwks) }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    iss
}

/// The three providers, with fixture issuers, and the port the server
/// will listen on (a nostr event names it).
struct Providers {
    port: u16,
    auth0: String,
    idp: String,
    list: Vec<Provider>,
}

async fn providers() -> Providers {
    let port = free_port().await;
    let auth0 = issuer(true, JWK_1).await;
    let idp = issuer(false, JWK_2).await;
    let list = [
        format!("idp={idp},{IDP_AUD}"),
        format!("auth0={auth0},{AUTH0_AUD},{ROLES_CLAIM}"),
        format!("nostr=ws://127.0.0.1:{port}/"),
    ]
    .iter()
    .map(|s| s.parse().unwrap())
    .collect();
    Providers { port, auth0, idp, list }
}

/// menger as `main` wires it: the boundary from the providers, strict when
/// `require_login`.
async fn start(port: u16, providers: &[Provider], require_login: bool) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let offers = providers.iter().map(Provider::offer).collect();
    // As main wires it (IDENTITY-19): sessions, the per-call rule, the gate.
    let sessions = Arc::new(Sessions::new(3600));
    let access = if providers.is_empty() { Access::open() } else { Access::with_login(sessions.clone(), require_login) };
    let hub = build_rpc_offering(&dir.path().join("tickets.db"), None, Default::default(), offers, access)
        .await
        .expect("open database");
    let boundary = login::boundary(providers).expect("providers");
    let conv = |arc: Arc<DynamicHub>| DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        let mut builder = TransportServer::builder(hub, conv).with_websocket(port);
        if let Some(carriers) = boundary {
            builder = builder.with_session_validator(Arc::new(Gate::new(sessions, Some(carriers))));
            if require_login {
                builder = builder.reject_upgrade_on_auth_failure();
            }
        }
        builder.build().await.expect("build server").serve().await.expect("serve");
    });
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never came up on {port}");
}

/// Open a connection carrying `headers` on its upgrade.
async fn connect(port: u16, headers: &[(&'static str, String)]) -> Result<WsClient, String> {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        map.insert(*k, HeaderValue::from_str(v).unwrap());
    }
    WsClientBuilder::default()
        .set_headers(map)
        .build(format!("ws://127.0.0.1:{port}"))
        .await
        .map_err(|e| e.to_string())
}

async fn call(client: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut sub: Subscription<Value> = client.subscribe("menger.call", p, "menger.call_unsub").await.expect("subscribe");
    let mut out = Vec::new();
    while let Some(item) = sub.next().await {
        let item = item.unwrap();
        match item["type"].as_str() {
            Some("done") => return out,
            Some("data") => out.push(item["content"].clone()),
            other => panic!("unexpected stream item type {other:?}: {item}"),
        }
    }
    panic!("stream ended without a done item");
}

async fn whoami(client: &WsClient) -> Value {
    let mut evs = call(client, "session.whoami", json!({})).await;
    assert_eq!(evs.len(), 1, "exactly one event: {evs:?}");
    evs.remove(0)
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn mint(kid: &str, pem: &str, claims: Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.into());
    encode(&header, &claims, &EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap()).unwrap()
}

/// What Auth0 issues for a user who logged in with a username and password.
fn auth0_token(iss: &str) -> String {
    mint("acme-1", KEY_1_PEM, json!({
        "iss": iss, "aud": AUTH0_AUD, "sub": "auth0|64f1ab", "exp": now() + 600, "iat": now() - 5,
        "nickname": "ada", ROLES_CLAIM: ["admin", "editor"],
    }))
}

/// What plexus-idp issues, by its password grant or its nostr grant.
fn idp_token(iss: &str, exp: i64) -> String {
    mint("idp-1", KEY_2_PEM, json!({
        "iss": iss, "aud": IDP_AUD, "sub": ALICE, "exp": exp, "iat": now() - 5,
        "username": "alice", "roles": ["user"], "org_id": "acme",
    }))
}

/// A NIP-98 event for a WebSocket upgrade: `u` the service, method GET.
fn nip98(keys: &Keys, port: u16) -> String {
    let tag = |t: [&str; 2]| Tag::parse(t).unwrap();
    let u = format!("ws://127.0.0.1:{port}/");
    let event = EventBuilder::new(Kind::HttpAuth, "")
        .tags([tag(["u", &u]), tag(["method", "GET"])])
        .finalize(keys)
        .unwrap();
    base64::engine::general_purpose::STANDARD.encode(serde_json::to_string(&event).unwrap())
}

fn comment_by(author: &str) -> Value {
    json!({
        "anchor": { "route": "#/comments", "path": "main", "label": "page", "excerpt": "" },
        "body": "looks right",
        "author": author,
    })
}

#[tokio::test]
async fn with_no_provider_menger_is_as_it_was() {
    let port = free_port().await;
    start(port, &[], false).await;
    let client = connect(port, &[("authorization", "Bearer whatever".into())]).await.unwrap();
    assert_eq!(whoami(&client).await, json!({ "type": "anonymous" }));
    let added = call(&client, "comments.add", comment_by("mallory")).await;
    assert_eq!(added[0]["author"], "mallory", "{added:?}");
}

#[tokio::test]
async fn each_provider_becomes_the_same_kind_of_plexus_identity() {
    let p = providers().await;
    start(p.port, &p.list, false).await;

    let idp = connect(p.port, &[("cookie", format!("theme=dark; access_token={}", idp_token(&p.idp, now() + 600)))]).await.unwrap();
    assert_eq!(whoami(&idp).await, json!({
        "type": "identity", "principal": format!("idp:{ALICE}"), "name": "alice",
        "roles": ["user"], "tenant": "acme", "provider": "plexus-idp",
    }));

    let auth0 = connect(p.port, &[("authorization", format!("Bearer {}", auth0_token(&p.auth0)))]).await.unwrap();
    assert_eq!(whoami(&auth0).await, json!({
        "type": "identity", "principal": "oidc:auth0/auth0|64f1ab", "name": "ada",
        "roles": ["admin", "editor"], "tenant": null, "provider": "auth0",
    }));

    let keys = Keys::generate();
    let nostr = connect(p.port, &[("cookie", format!("nostr={}", nip98(&keys, p.port)))]).await.unwrap();
    assert_eq!(whoami(&nostr).await, json!({
        "type": "identity", "principal": format!("nostr:{}", keys.public_key().to_hex()),
        "name": keys.public_key().to_bech32().unwrap(), "roles": [], "tenant": null, "provider": "nostr",
    }));

    let anonymous = connect(p.port, &[]).await.unwrap();
    assert_eq!(whoami(&anonymous).await, json!({ "type": "anonymous" }));
}

#[tokio::test]
async fn a_signed_in_comment_carries_its_identity() {
    let p = providers().await;
    start(p.port, &p.list, false).await;
    let alice = connect(p.port, &[("cookie", format!("access_token={}", idp_token(&p.idp, now() + 600)))]).await.unwrap();
    let added = call(&alice, "comments.add", comment_by("mallory")).await;
    assert_eq!(added[0]["author"], "alice", "the identity, not the claim: {added:?}");
    let anonymous = connect(p.port, &[]).await.unwrap();
    let added = call(&anonymous, "comments.add", comment_by("mallory")).await;
    assert_eq!(added[0]["author"], "mallory", "anonymous keeps the given author: {added:?}");
}

#[tokio::test]
async fn a_bad_credential_is_anonymous() {
    let p = providers().await;
    start(p.port, &p.list, false).await;
    let expired = idp_token(&p.idp, now() - 60);
    let mut tampered = auth0_token(&p.auth0);
    tampered.push('x');
    let keys = Keys::generate();
    let replayed = nip98(&keys, p.port);
    connect(p.port, &[("cookie", format!("nostr={replayed}"))]).await.unwrap();
    for headers in [
        vec![("cookie", format!("access_token={expired}"))],
        vec![("authorization", format!("Bearer {tampered}"))],
        vec![("cookie", format!("nostr={replayed}"))],
        vec![("cookie", format!("nostr={}", nip98(&keys, 1)))],
    ] {
        let client = connect(p.port, &headers).await.unwrap();
        assert_eq!(whoami(&client).await, json!({ "type": "anonymous" }), "{headers:?}");
    }
}

#[tokio::test]
async fn require_login_refuses_an_upgrade_that_proves_no_one() {
    let p = providers().await;
    start(p.port, &p.list, true).await;
    let refused = connect(p.port, &[]).await.unwrap_err();
    assert!(refused.contains("401"), "{refused}");
    let refused = connect(p.port, &[("cookie", format!("access_token={}", idp_token(&p.idp, now() - 60)))]).await.unwrap_err();
    assert!(refused.contains("401"), "{refused}");
    let alice = connect(p.port, &[("cookie", format!("access_token={}", idp_token(&p.idp, now() + 600)))]).await.unwrap();
    assert_eq!(whoami(&alice).await["principal"], format!("idp:{ALICE}"));
}

#[tokio::test]
async fn providers_says_what_a_sign_in_needs_and_nothing_more() {
    let p = providers().await;
    start(p.port, &p.list, false).await;
    let client = connect(p.port, &[]).await.unwrap();
    assert_eq!(call(&client, "session.providers", json!({})).await, vec![
        json!({ "type": "provider", "name": "plexus-idp", "kind": "oidc", "issuer": p.idp, "audience": IDP_AUD }),
        json!({ "type": "provider", "name": "auth0", "kind": "oidc", "issuer": p.auth0, "audience": AUTH0_AUD }),
        json!({ "type": "provider", "name": "nostr", "kind": "nostr", "issuer": null, "audience": null }),
    ]);

    let port = free_port().await;
    start(port, &[], false).await;
    let client = connect(port, &[]).await.unwrap();
    assert!(call(&client, "session.providers", json!({})).await.is_empty(), "login off: none");
}
