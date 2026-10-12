//! SCAFFOLD-9: the toy (examples/toy.rs) as a test — a hub that is not
//! menger takes plexus-login with the crate's own rules, in the few lines
//! README.md shows, and a person signs in through it: the page, the
//! password form, the session cookie (named for the app), the socket's
//! gate, `auth.whoami`, the waiting list, sign-out.
//!
//! A fixture OIDC issuer stands in for plexus-idp (discovery, a JWKS, the
//! password grant), as menger's tests/login_page.rs does; the key is
//! synthetic. Every server is on a free port.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Form, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use plexus_core::plexus::DynamicHub;
use plexus_login::simple::SimpleAccess;
use plexus_login::{Branding, LoginAccess, LoginOptions, Wiring};
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::collections::HashMap;

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

const ALICE_SUB: &str = "c952acd3-60aa-4534-b2e3-83634591fee1";
const ADMIN_SUB: &str = "0b6d1c0e-3f0a-4d55-9d1e-6f0c2a7b9e01";

const BRANDING: Branding = Branding { app: "toy", version: "0.0.1", mark: "t", motto: "" };

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

async fn listen() -> tokio::net::TcpListener {
    tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap()
}

async fn free_port() -> u16 {
    listen().await.local_addr().unwrap().port()
}

// ── A fixture plexus-idp: discovery, JWKS, the password grant ─────────────

async fn token(State(iss): State<String>, Form(f): Form<HashMap<String, String>>) -> axum::response::Response {
    let sub = match (f.get("grant_type").map(String::as_str), f.get("username").map(String::as_str), f.get("password").map(String::as_str)) {
        (Some("password"), Some("alice"), Some("s3cret")) => ALICE_SUB,
        (Some("password"), Some("root"), Some("s3cret")) => ADMIN_SUB,
        _ => return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_grant" }))).into_response(),
    };
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some("idp-1".into());
    let claims = json!({ "iss": iss, "aud": f["audience"], "sub": sub, "exp": now() + 600, "iat": now() - 5, "username": f["username"] });
    let t = encode(&h, &claims, &EncodingKey::from_rsa_pem(KEY_2_PEM.as_bytes()).unwrap()).unwrap();
    Json(json!({ "access_token": t, "expires_in": 600 })).into_response()
}

async fn issuer() -> String {
    let listener = listen().await;
    let iss = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let discovery = json!({ "issuer": iss, "jwks_uri": format!("{iss}/jwks.json"), "token_endpoint": format!("{iss}/token") });
    let jwks = json!({ "keys": [serde_json::from_str::<Value>(JWK_2).unwrap()] });
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(move || async move { Json(discovery) }))
        .route("/jwks.json", get(move || async move { Json(jwks) }))
        .route("/token", post(token))
        .with_state(iss.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    iss
}

// ── The toy: a hub with only `auth`, on plexus-login ──────────────────────

struct Toy {
    ws: u16,
    public_url: String,
    cookie: String,
    access: SimpleAccess,
    http: reqwest::Client,
}

async fn toy() -> Toy {
    let iss = issuer().await;
    let (ws, shell) = (free_port().await, free_port().await);
    let admin = format!("idp:{ADMIN_SUB}");

    // ── what README.md shows: login in a dozen lines ──
    let opts = LoginOptions::new(BRANDING, ws, shell).with_providers([format!("idp={iss},plexus:toy")]).unwrap();
    let wiring = Wiring::new(opts).unwrap();
    let access = SimpleAccess::new(wiring.sessions(), &[admin]).unwrap();
    let wired = wiring.finish(access.clone(), None).unwrap();
    let hub = DynamicHub::new("toy").with_auth(access.auth_setup(wired.backend.clone()));
    access.seed_roles(&hub);
    let server = wired.apply(
        TransportServer::builder(Arc::new(hub), |h| DynamicHub::arc_into_rpc_module(h).map_err(|e| anyhow::anyhow!("{e}"))).with_websocket(ws),
    );
    let router = wired.router().expect("a provider and a shell: a page");
    // ──

    tokio::spawn(async move { server.build().await.unwrap().serve().await.unwrap() });
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", shell)).await.unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    for p in [ws, shell] {
        for _ in 0..100 {
            if tokio::net::TcpStream::connect(("127.0.0.1", p)).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    Toy {
        ws,
        public_url: wired.public_url.clone(),
        cookie: wired.sessions.cookie_name().to_string(),
        access,
        http: reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap(),
    }
}

impl Toy {
    /// Signs in with the page's password form: the `Cookie` header the
    /// session rides.
    async fn sign_in(&self, user: &str, password: &str) -> Option<String> {
        let r = self
            .http
            .post(format!("{}/login", self.public_url))
            .header("origin", &self.public_url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("username={user}&password={password}&next=%2F"))
            .send()
            .await
            .unwrap();
        r.headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|c| c.starts_with(&format!("{}=", self.cookie)))
            .and_then(|c| c.split(';').next())
            .map(str::to_string)
    }

    async fn socket(&self, cookie: Option<&str>) -> WsClient {
        let mut h = HeaderMap::new();
        if let Some(c) = cookie {
            h.insert("cookie", HeaderValue::from_str(c).unwrap());
        }
        WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{}", self.ws)).await.unwrap()
    }
}

/// A method's whole answer: its data items, or the error that ended it.
async fn call(c: &WsClient, method: &str, params: Value) -> Value {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut sub: Subscription<Value> = c.subscribe("toy.call", p, "toy.call_unsub").await.unwrap();
    let mut out = Vec::new();
    while let Some(Ok(item)) = sub.next().await {
        match item["type"].as_str() {
            Some("data") => out.push(item["content"].clone()),
            Some("error") => {
                out.push(item);
                break;
            }
            Some("done") => break,
            _ => {}
        }
    }
    Value::Array(out)
}

#[tokio::test]
async fn a_hub_that_is_not_menger_signs_a_person_in_through_plexus_login() {
    let toy = toy().await;
    assert_eq!(toy.cookie, format!("toy_session_{}", toy.public_url.rsplit(':').next().unwrap()), "the app's cookie, not menger's");

    // Anonymous: the anonymous view — the auth door, nothing else.
    let anon = toy.socket(None).await;
    let who = call(&anon, "auth.whoami", json!({})).await;
    assert_eq!(who[0]["error_kind"], "unauthenticated", "{who}");

    // The page, branded for the toy.
    let page = toy.http.get(format!("{}/login", toy.public_url)).send().await.unwrap().text().await.unwrap();
    assert!(page.contains("<title>Sign in · toy</title>") && page.contains(r#"name="password""#));
    assert!(!page.contains("menger"), "no menger name on the toy's page");

    // A wrong password stays on the page; the right one is a session.
    assert!(toy.sign_in("alice", "nope").await.is_none());
    let cookie = toy.sign_in("alice", "s3cret").await.expect("signed in");
    let me: Value = toy.http.get(format!("{}/login/me", toy.public_url)).header("cookie", &cookie).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["principal"], format!("idp:{ALICE_SUB}"));
    assert_eq!(me["provider"], "plexus-idp");

    // The socket, with the cookie: the gate knows who it is, and she waits
    // for an admin.
    let ws = toy.socket(Some(&cookie)).await;
    let who = call(&ws, "auth.whoami", json!({})).await;
    assert_eq!(who[0]["principal"], format!("idp:{ALICE_SUB}"), "{who}");
    assert_eq!(toy.access.waiting().iter().map(|w| w.principal.clone()).collect::<Vec<_>>(), [format!("idp:{ALICE_SUB}")]);

    // The named admin sees the waiting list and grants her a role.
    let root = toy.sign_in("root", "s3cret").await.expect("the admin signs in");
    let admin = toy.socket(Some(&root)).await;
    let waiting = call(&admin, "auth.waiting", json!({})).await;
    assert!(waiting.to_string().contains(ALICE_SUB), "{waiting}");
    let granted = call(&admin, "auth.grant", json!({ "principal": format!("idp:{ALICE_SUB}"), "role": "viewer", "scope": "*" })).await;
    assert!(!granted.to_string().contains("\"error\""), "{granted}");
    assert!(toy.access.waiting().is_empty() && toy.access.holds_any(&format!("idp:{ALICE_SUB}")));

    // Sign-out ends the session: the cookie is no one.
    let out = toy.http.post(format!("{}/logout", toy.public_url)).header("origin", &toy.public_url).header("cookie", &cookie).send().await.unwrap();
    assert_eq!(out.status(), 303);
    let me = toy.http.get(format!("{}/login/me", toy.public_url)).header("cookie", &cookie).send().await.unwrap();
    assert_eq!(me.status(), 401);
}
