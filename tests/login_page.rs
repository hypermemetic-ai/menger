//! IDENTITY-19: the login page, sessions and the gate, over HTTP and the
//! real socket.
//!
//! Two fixture OIDC issuers stand in for plexus-idp (password grant) and an
//! Auth0 tenant (authorization code with PKCE, `connection` hints); both
//! serve discovery and a JWKS on loopback, so the shell asks them as it
//! would the real ones. Keys are synthetic. Every server is on free ports
//! with a throwaway database.

use axum::extract::{Form, Query, State};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use menger::build_rpc_offering;
use menger::login::{self, LoginButton, Provider};
use menger::sessions::{Gate, Sessions};
use menger::shell::{self, ShellConfig};
use menger::web_login::{Login, LoginConfig};
use menger_tickets_api::access::Access;
use nostr::event::{EventBuilder, FinalizeEvent, Kind, Tag};
use nostr::key::Keys;
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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


const ALICE: &str = "c952acd3-60aa-4534-b2e3-83634591fee1";

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

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

// ── A fixture issuer: discovery, JWKS, password grant, code flow ──────────

#[derive(Clone)]
struct Issuer {
    iss: String,
    kid: &'static str,
    pem: &'static str,
    codes: Arc<Mutex<HashMap<String, (String, String, String, Option<String>)>>>, // code → (challenge, redirect, client, connection)
}

fn mint(i: &Issuer, sub: &str, aud: &str, name: &str) -> String {
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some(i.kid.into());
    encode(&h, &json!({ "iss": i.iss, "aud": aud, "sub": sub, "exp": now() + 600, "iat": now() - 5,
        "username": name, "nickname": name }), &EncodingKey::from_rsa_pem(i.pem.as_bytes()).unwrap()).unwrap()
}

async fn authorize(State(i): State<Issuer>, Query(q): Query<HashMap<String, String>>) -> impl IntoResponse {
    let code = format!("code-{}", now());
    i.codes.lock().unwrap().insert(code.clone(), (
        q["code_challenge"].clone(), q["redirect_uri"].clone(), q["client_id"].clone(), q.get("connection").cloned(),
    ));
    Redirect::to(&format!("{}?code={code}&state={}", q["redirect_uri"], q["state"]))
}

async fn token(State(i): State<Issuer>, Form(f): Form<HashMap<String, String>>) -> impl IntoResponse {
    let refuse = |e: &str| (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response();
    match f.get("grant_type").map(String::as_str) {
        Some("password") => {
            if f.get("username").map(String::as_str) != Some("alice") || f.get("password").map(String::as_str) != Some("s3cret") {
                return refuse("invalid_grant");
            }
            Json(json!({ "access_token": mint(&i, ALICE, &f["audience"], "alice"), "expires_in": 600 })).into_response()
        }
        Some("authorization_code") => {
            let Some((challenge, redirect, client, conn)) = i.codes.lock().unwrap().remove(&f["code"]) else {
                return refuse("invalid_grant");
            };
            let got = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(f["code_verifier"].as_bytes()));
            if got != challenge || f["redirect_uri"] != redirect || f["client_id"] != client {
                return refuse("invalid_grant");
            }
            let sub = format!("{}|ada", conn.unwrap_or_default());
            Json(json!({ "access_token": mint(&i, &sub, "https://menger.example", "ada"), "expires_in": 600 })).into_response()
        }
        _ => refuse("unsupported_grant_type"),
    }
}

async fn issuer(slash: bool, kid: &'static str, pem: &'static str, jwk: &'static str) -> Issuer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let iss = if slash { format!("{base}/") } else { base.clone() };
    let i = Issuer { iss: iss.clone(), kid, pem, codes: Arc::default() };
    let discovery = json!({ "issuer": iss, "jwks_uri": format!("{base}/jwks.json"),
        "authorization_endpoint": format!("{base}/authorize"), "token_endpoint": format!("{base}/token") });
    let jwks = json!({ "keys": [serde_json::from_str::<Value>(jwk).unwrap()] });
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(move || async move { Json(discovery) }))
        .route("/jwks.json", get(move || async move { Json(jwks) }))
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .with_state(i.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    i
}

// ── menger: the socket and the shell, as main wires them ──────────────────

struct Menger {
    ws: u16,
    shell: u16,
    http: reqwest::Client,
}

impl Menger {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.shell)
    }
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.shell)
    }
}

async fn start() -> Menger {
    let idp = issuer(false, "idp-1", KEY_2_PEM, JWK_2).await;
    let auth0 = issuer(true, "acme-1", KEY_1_PEM, JWK_1).await;
    let (ws, shell_port) = (free_port().await, free_port().await);
    let public_url = format!("http://127.0.0.1:{shell_port}");
    let providers: Vec<Provider> = [
        format!("idp={},plexus:menger", idp.iss),
        format!("auth0={},https://menger.example,client=menger", auth0.iss),
        format!("nostr=ws://127.0.0.1:{ws}/"),
    ]
    .iter()
    .map(|s| s.parse().unwrap())
    .collect();
    let buttons: Vec<LoginButton> = ["Google=auth0:google-oauth2", "SSO=auth0:samlp"].iter().map(|s| s.parse().unwrap()).collect();

    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let sessions = Arc::new(Sessions::new(3600));
    let access = Access::with_login(sessions.clone(), true, Vec::new());
    let offers = providers.iter().map(Provider::offer).collect();
    let hub = build_rpc_offering(&dir.path().join("t.db"), None, Default::default(), offers, access).await.unwrap();
    let carriers = login::boundary(&providers).unwrap();
    let origins = vec![public_url.clone(), format!("http://localhost:{shell_port}")];
    let gate = Arc::new(Gate::new(sessions.clone(), carriers));
    let conv = |arc: Arc<DynamicHub>| DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"));
    let ws_origins = origins.clone();
    tokio::spawn(async move {
        TransportServer::builder(hub, conv)
            .with_websocket(ws)
            .with_allowed_origins(ws_origins)
            .with_session_validator(gate)
            .reject_upgrade_on_auth_failure()
            .build().await.unwrap().serve().await.unwrap();
    });
    let login = Login::new(
        LoginConfig {
            providers,
            buttons,
            password: None,
            forgot: Some("https://idp.example/reset".into()),
            public_url,
            also: origins,
            require: true,
            title: "menger",
        },
        sessions,
    )
    .unwrap();
    let app_dist = dir.path().join("no-dist");
    tokio::spawn(async move {
        let _keep = dir;
        shell::serve(ShellConfig { port: shell_port, ws_port: ws, rest_port: None, app_dist, env: None, login: Some(Arc::new(login)) })
            .await
            .unwrap();
    });
    for p in [ws, shell_port] {
        for _ in 0..100 {
            if tokio::net::TcpStream::connect(("127.0.0.1", p)).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Menger { ws, shell: shell_port, http }
}

/// The `menger_session` a response sets, with its attributes checked.
fn session_from(r: &reqwest::Response) -> Option<String> {
    r.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok()).find_map(|c| {
        let value = c.strip_prefix("menger_session=")?.split(';').next()?.to_string();
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Lax"), "{c}");
        (!value.is_empty()).then_some(value)
    })
}

fn location(r: &reqwest::Response) -> String {
    r.headers().get("location").and_then(|l| l.to_str().ok()).unwrap_or("").to_string()
}

async fn sign_in(m: &Menger) -> String {
    let r = m.http.post(m.url("/login")).form(&[("username", "alice"), ("password", "s3cret"), ("next", "/app/")]).send().await.unwrap();
    assert_eq!(r.status(), 303);
    assert_eq!(location(&r), "/app/");
    session_from(&r).expect("a session cookie")
}

async fn socket(m: &Menger, cookie: Option<&str>, origin: Option<&str>) -> Result<WsClient, String> {
    let mut h = HeaderMap::new();
    if let Some(c) = cookie {
        h.insert("cookie", HeaderValue::from_str(&format!("menger_session={c}")).unwrap());
    }
    if let Some(o) = origin {
        h.insert("origin", HeaderValue::from_str(o).unwrap());
    }
    WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{}", m.ws)).await.map_err(|e| e.to_string())
}

async fn call(c: &WsClient, method: &str) -> Vec<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", json!({})).unwrap();
    let mut sub: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let mut out = Vec::new();
    while let Some(Ok(item)) = sub.next().await {
        match item["type"].as_str() {
            Some("data") => out.push(item["content"].clone()),
            _ => break,
        }
    }
    out
}

// ── The tests ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn nobody_gets_in_anonymously() {
    let m = start().await;
    for path in ["/", "/app/", "/vanilla"] {
        let r = m.http.get(m.url(path)).send().await.unwrap();
        assert_eq!(r.status(), 303, "{path}");
        assert!(location(&r).starts_with("/login?next="), "{path}: {}", location(&r));
    }
    assert!(socket(&m, None, Some(&m.origin())).await.unwrap_err().contains("401"));
    let page = m.http.get(m.url("/login")).send().await.unwrap().text().await.unwrap();
    for want in ["Sign in", "name=\"password\"", "Continue with Google", "Continue with SSO", "Continue with nostr", "Forgot password?"] {
        assert!(page.contains(want), "the page shows {want}");
    }
}

#[tokio::test]
async fn a_password_signs_in_and_the_session_opens_the_app_and_the_socket() {
    let m = start().await;
    let bad = m.http.post(m.url("/login")).form(&[("username", "alice"), ("password", "nope")]).send().await.unwrap();
    assert_eq!(bad.status(), 401);
    assert!(bad.text().await.unwrap().contains("Wrong username or password."));

    let id = sign_in(&m).await;
    let app = m.http.get(m.url("/app/")).header("cookie", format!("menger_session={id}")).send().await.unwrap();
    assert_eq!(app.status(), 200, "the app, for a session");
    let me: Value = m.http.get(m.url("/login/me")).header("cookie", format!("menger_session={id}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["principal"], format!("idp:{ALICE}"));

    let ws = socket(&m, Some(&id), Some(&m.origin())).await.expect("the socket admits the session");
    assert_eq!(call(&ws, "session.whoami").await[0]["principal"], format!("idp:{ALICE}"));
    assert_eq!(call(&ws, "tickets.health").await[0]["type"], "health");
}

#[tokio::test]
async fn a_post_from_another_page_is_refused() {
    let m = start().await;
    let r = m.http.post(m.url("/login")).header("origin", "http://127.0.0.1:3000")
        .form(&[("username", "alice"), ("password", "s3cret")]).send().await.unwrap();
    assert_eq!(r.status(), 403);
    assert!(session_from(&r).is_none());
}

#[tokio::test]
async fn google_signs_in_through_the_code_flow_bound_to_this_browser() {
    let m = start().await;
    let out = m.http.get(m.url("/login/start?button=Google&next=/app/")).send().await.unwrap();
    assert_eq!(out.status(), 302);
    let to = url::Url::parse(&location(&out)).unwrap();
    let q: HashMap<_, _> = to.query_pairs().into_owned().collect();
    assert_eq!(q["connection"], "google-oauth2");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["client_id"], "menger");
    let txn = out.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok())
        .find_map(|c| c.strip_prefix("menger_login=").map(|v| v.split(';').next().unwrap().to_string())).unwrap();

    // The IdP sends the browser back with a code.
    let back = m.http.get(to.as_str()).send().await.unwrap();
    let callback = location(&back);
    assert!(callback.contains("/login/callback?code="));

    // Without the browser's own transaction cookie: refused.
    let stray = m.http.get(&callback).send().await.unwrap();
    assert_eq!(stray.status(), 401);

    // A second start, finished in the browser that started it.
    let out = m.http.get(m.url("/login/start?button=Google&next=/app/")).send().await.unwrap();
    let to = location(&out);
    let txn2 = out.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok())
        .find_map(|c| c.strip_prefix("menger_login=").map(|v| v.split(';').next().unwrap().to_string())).unwrap();
    assert_ne!(txn, txn2);
    let callback = location(&m.http.get(&to).send().await.unwrap());
    let done = m.http.get(&callback).header("cookie", format!("menger_login={txn2}")).send().await.unwrap();
    assert_eq!(done.status(), 303);
    assert_eq!(location(&done), "/app/");
    let id = session_from(&done).expect("a session");
    let ws = socket(&m, Some(&id), Some(&m.origin())).await.unwrap();
    assert_eq!(call(&ws, "session.whoami").await[0]["principal"], "oidc:auth0/google-oauth2|ada");
}

#[tokio::test]
async fn a_nostr_extension_signs_in() {
    let m = start().await;
    let keys = Keys::generate();
    let url = m.url("/login/nostr");
    let tag = |t: [&str; 2]| Tag::parse(t).unwrap();
    let ev = EventBuilder::new(Kind::HttpAuth, "").tags([tag(["u", &url]), tag(["method", "POST"])]).finalize(&keys).unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(serde_json::to_string(&ev).unwrap());
    let r = m.http.post(format!("{url}?next=/app/")).header("authorization", format!("Nostr {b64}")).header("origin", m.origin()).send().await.unwrap();
    assert_eq!(r.status(), 303);
    let id = session_from(&r).unwrap();
    let ws = socket(&m, Some(&id), Some(&m.origin())).await.unwrap();
    assert_eq!(call(&ws, "session.whoami").await[0]["principal"], format!("nostr:{}", keys.public_key().to_hex()));
}

#[tokio::test]
async fn sign_out_ends_the_session_and_its_open_socket_stops_answering() {
    let m = start().await;
    let id = sign_in(&m).await;
    let ws = socket(&m, Some(&id), Some(&m.origin())).await.unwrap();
    assert_eq!(call(&ws, "tickets.health").await[0]["type"], "health");
    let out = m.http.post(m.url("/logout")).header("cookie", format!("menger_session={id}")).header("origin", m.origin()).send().await.unwrap();
    assert_eq!(out.status(), 303);
    let after = call(&ws, "tickets.health").await;
    assert_eq!(after[0]["type"], "error");
    assert!(after[0]["message"].as_str().unwrap().contains("signed out"), "{after:?}");
    assert!(socket(&m, Some(&id), Some(&m.origin())).await.is_err(), "and no new socket");
    assert_eq!(m.http.get(m.url("/login/me")).header("cookie", format!("menger_session={id}")).send().await.unwrap().status(), 401);
}

#[tokio::test]
async fn other_pages_and_other_names_are_refused() {
    let m = start().await;
    let id = sign_in(&m).await;
    let foreign = socket(&m, Some(&id), Some("http://127.0.0.1:3000")).await.unwrap_err();
    assert!(foreign.contains("403"), "{foreign}");
    assert!(socket(&m, Some(&id), Some("null")).await.unwrap_err().contains("403"), "a sandboxed frame");
    let rebound = m.http.get(m.url("/login")).header("host", "evil.example").send().await.unwrap();
    assert_eq!(rebound.status(), 421, "DNS rebinding");
}
