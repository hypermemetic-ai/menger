//! IDENTITY-19: the login page, sessions and the gate, over HTTP and the
//! real socket.
//!
//! Two fixture OIDC issuers stand in for plexus-idp (password grant) and an
//! Auth0 tenant (authorization code with PKCE, `connection` hints); both
//! serve discovery and a JWKS on loopback, so the shell asks them as it
//! would the real ones. IDENTITY-30: a third stands in for WorkOS, shaped
//! as a real staging sign-in answered (a per-client issuer under
//! `/user_management/`, no `aud`, `client_id`, the person in the response's
//! `user`). Keys are synthetic. Every server is on free ports with a
//! throwaway database.

use axum::extract::{Form, Query, State};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use menger::auth_backend::{BrowserLogin, MengerAuth};
use menger::build_rpc_login;
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

/// A fixture's listener on a port [`free_port`] handed out, so no other
/// test's server is handed the same one between its pick and its bind.
async fn listen() -> tokio::net::TcpListener {
    loop {
        if let Ok(l) = tokio::net::TcpListener::bind(("127.0.0.1", free_port().await)).await {
            return l;
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
            // IDENTITY-35: the dev accounts plexus-idp holds, with the dev
            // password.
            if let (Some((_, sub)), Some(DEV_PASSWORD)) =
                (DEV_ACCOUNTS.iter().find(|(n, _)| Some(*n) == f.get("username").map(String::as_str)), f.get("password").map(String::as_str))
            {
                return Json(json!({ "access_token": mint(&i, sub, &f["audience"], &f["username"]), "expires_in": 600 })).into_response();
            }
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
    let listener = listen().await;
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
    /// IDENTITY-38: this shell's session cookie's name, as main names it.
    fn cookie(&self) -> String {
        menger::sessions::cookie_name_for(&self.url(""))
    }
    /// A `Cookie` header carrying session `id`.
    fn jar(&self, id: &str) -> String {
        format!("{}={id}", self.cookie())
    }
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.shell)
    }
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.shell)
    }
}

// ── IDENTITY-30: a WorkOS-shaped issuer ───────────────────────────────────

const WORKOS_CLIENT: &str = "client_01TESTCLIENT";

#[derive(Clone)]
struct WorkOs {
    base: String,
    codes: Arc<Mutex<HashMap<String, (String, String, String, String)>>>, // code → (challenge, redirect, client, provider)
}

async fn workos_authorize(State(w): State<WorkOs>, Query(q): Query<HashMap<String, String>>) -> impl IntoResponse {
    let code = format!("code-{}-{}", now(), w.codes.lock().unwrap().len());
    let way = q.get("provider").or(q.get("connection_id")).or(q.get("organization_id")).cloned().unwrap_or_default();
    w.codes.lock().unwrap().insert(code.clone(), (q["code_challenge"].clone(), q["redirect_uri"].clone(), q["client_id"].clone(), way));
    Redirect::to(&format!("{}?code={code}&state={}", q["redirect_uri"], q["state"]))
}

async fn workos_authenticate(State(w): State<WorkOs>, Form(f): Form<HashMap<String, String>>) -> impl IntoResponse {
    let refuse = |e: &str| (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response();
    if f.get("grant_type").map(String::as_str) != Some("authorization_code") || f.contains_key("client_secret") {
        return refuse("invalid_request");
    }
    let Some((challenge, redirect, client, way)) = w.codes.lock().unwrap().remove(&f["code"]) else {
        return refuse("invalid_grant");
    };
    let got = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(f["code_verifier"].as_bytes()));
    if got != challenge || f["redirect_uri"] != redirect || f["client_id"] != client {
        return refuse("invalid_grant");
    }
    // `OtherClient`: a token another client of the account was issued.
    let for_client = if way == "OtherClient" { "client_01SOMEONEELSE" } else { WORKOS_CLIENT };
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some("acme-1".into());
    let token = encode(&h, &json!({
        "iss": format!("{}/user_management/{WORKOS_CLIENT}", w.base), "sub": "user_01ADA", "sid": "session_01ADA",
        "jti": "01JTI", "client_id": for_client, "org_id": "org_01ACME", "role": "member", "roles": ["member"],
        "permissions": [], "exp": now() + 300, "iat": now() - 5,
    }), &EncodingKey::from_rsa_pem(KEY_1_PEM.as_bytes()).unwrap()).unwrap();
    Json(json!({
        "user": { "object": "user", "id": "user_01ADA", "email": "ada@example.com", "first_name": "Ada", "last_name": "Lovelace" },
        "organization_id": "org_01ACME", "access_token": token, "refresh_token": "r", "authentication_method": way,
    }))
    .into_response()
}

async fn workos() -> WorkOs {
    let listener = listen().await;
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let w = WorkOs { base: base.clone(), codes: Arc::default() };
    let issuer = format!("{base}/user_management/{WORKOS_CLIENT}");
    let discovery = json!({ "issuer": issuer, "jwks_uri": format!("{base}/sso/jwks/{WORKOS_CLIENT}"),
        "authorization_endpoint": format!("{base}/user_management/authorize"),
        "token_endpoint": format!("{base}/user_management/authenticate"), "response_types_supported": ["code"] });
    let jwks = json!({ "keys": [serde_json::from_str::<Value>(JWK_1).unwrap()] });
    let app = Router::new()
        .route(&format!("/user_management/{WORKOS_CLIENT}/.well-known/openid-configuration"), get(move || async move { Json(discovery) }))
        .route(&format!("/sso/jwks/{WORKOS_CLIENT}"), get(move || async move { Json(jwks) }))
        .route("/user_management/authorize", get(workos_authorize))
        .route("/user_management/authenticate", post(workos_authenticate))
        .with_state(w.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    w
}

async fn start() -> Menger {
    start_with(false).await.0
}

/// IDENTITY-35: the accounts the fixture plexus-idp holds with the dev
/// password, and their subjects.
const DEV_PASSWORD: &str = "dev-pass";
const DEV_ACCOUNTS: &[(&str, &str)] = &[
    ("viewer", "0b6d1c0e-3f0a-4d55-9d1e-6f0c2a7b9e01"),
    ("mixed", "0b6d1c0e-3f0a-4d55-9d1e-6f0c2a7b9e02"),
    ("drifter", "0b6d1c0e-3f0a-4d55-9d1e-6f0c2a7b9e03"),
];

/// menger with the dev sign-in on: plexus-idp is the fixture issuer; its
/// socket (for registering) is a port nothing listens on.
async fn start_dev() -> Menger {
    let dev = menger::dev_login::DevLogin {
        provider: "plexus-idp".into(),
        password: DEV_PASSWORD.into(),
        idp_ws: format!("ws://127.0.0.1:{}", free_port().await),
        users: vec!["viewer=viewer@*".parse().unwrap(), "mixed=editor@OUGHT,viewer@PLEXUS".parse().unwrap()],
    };
    start_full(false, Some(dev)).await.0
}

/// menger as main wires it: plexus-idp, an Auth0 tenant and nostr; or,
/// with `with_workos`, menger's page with an email field handed to WorkOS,
/// WorkOS's social buttons, nostr, and plexus-idp's password form at
/// `/login?password` for automation.
async fn start_with(with_workos: bool) -> (Menger, Option<WorkOs>) {
    start_full(with_workos, None).await
}

async fn start_full(with_workos: bool, dev: Option<menger::dev_login::DevLogin>) -> (Menger, Option<WorkOs>) {
    let idp = issuer(false, "idp-1", KEY_2_PEM, JWK_2).await;
    let auth0 = issuer(true, "acme-1", KEY_1_PEM, JWK_1).await;
    let (ws, shell_port) = (free_port().await, free_port().await);
    let public_url = format!("http://127.0.0.1:{shell_port}");
    let w = if with_workos { Some(workos().await) } else { None };
    let specs = match &w {
        None => vec![
            format!("idp={},plexus:menger", idp.iss),
            format!("auth0={},https://menger.example,client=menger", auth0.iss),
            format!("nostr=ws://127.0.0.1:{ws}/"),
        ],
        Some(w) => vec![
            format!("workos={WORKOS_CLIENT},{}", w.base),
            format!("idp={},plexus:menger", idp.iss),
            format!("nostr=ws://127.0.0.1:{ws}/"),
        ],
    };
    let providers: Vec<Provider> = specs.iter().map(|s| s.parse().unwrap()).collect();
    let button_specs: &[&str] = match &w {
        None => &["Google=auth0:google-oauth2", "SSO=auth0:samlp"],
        Some(_) => &["Google=workos:GoogleOAuth", "Apple=workos:AppleOAuth", "Other=workos:OtherClient"],
    };
    let buttons: Vec<LoginButton> = button_specs.iter().map(|s| s.parse().unwrap()).collect();
    let email: Option<LoginButton> = w.as_ref().map(|_| "email=workos:authkit".parse().unwrap());
    let password = None;

    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let sessions = Arc::new(Sessions::named(3600, &menger::sessions::cookie_name_for(&public_url)));
    let access = Access::with_login(menger::session_table(&sessions), true, Vec::new()).minting(menger::session_table(&sessions));
    let offers = providers.iter().map(menger::offer).collect();
    // AUTH-7: as main wires it — the protocol's auth over menger's backend,
    // whose loopback sign-in goes through this shell's login page.
    let backend = Arc::new(MengerAuth::new(
        access.clone(),
        MengerAuth::offered(&providers),
        Some(BrowserLogin { sessions: sessions.clone(), public_url: public_url.clone() }),
    ));
    let hub = build_rpc_login(&dir.path().join("t.db"), None, Default::default(), offers, access.clone(), backend, None).await.unwrap();
    let carriers = login::boundary(&providers).unwrap();
    let origins = vec![public_url.clone(), format!("http://localhost:{shell_port}")];
    // AUTH-7: as main wires it, the upgrade notices who signs in holding nothing.
    let gate = Arc::new(Gate::new(sessions.clone(), carriers).noticing(menger::MengerAccess(access.clone())));
    let conv = |arc: Arc<DynamicHub>| DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"));
    let ws_origins = origins.clone();
    tokio::spawn(async move {
        TransportServer::builder(hub, conv)
            .with_websocket(ws)
            .with_allowed_origins(ws_origins)
            .with_session_validator(gate)
            // AUTH-6/AUTH-7: as main wires it — no credential is the
            // anonymous view; only a bearer or the access_token cookie is a
            // credential, so a stale session cookie is no one, never a 401.
            .anonymous_view()
            .credential_cookies(["access_token"])
            .build().await.unwrap().serve().await.unwrap();
    });
    let login = Login::new(
        LoginConfig {
            providers,
            buttons,
            password,
            forgot: Some("https://idp.example/reset".into()),
            public_url,
            also: origins,
            require: true,
            title: "menger",
            branding: menger::login_access::BRANDING,
            email,
            access: dev.as_ref().map(|_| access.into()),
            dev,
        },
        sessions,
    )
    .unwrap();
    let app_dist = dir.path().join("no-dist");
    tokio::spawn(async move {
        let _keep = dir;
        shell::serve(ShellConfig { port: shell_port, ws_port: ws, app: shell::AppSource::Dir(app_dist), env: None, login: Some(Arc::new(login)) })
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
    (Menger { ws, shell: shell_port, http }, w)
}

/// The session cookie (`menger_session_<port>`, IDENTITY-38) a response
/// sets, with its attributes checked.
fn session_from(r: &reqwest::Response) -> Option<String> {
    let port = r.url().port().expect("a test shell has a port");
    let name = format!("menger_session_{port}=");
    r.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok()).find_map(|c| {
        let value = c.strip_prefix(name.as_str())?.split(';').next()?.to_string();
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
        h.insert("cookie", HeaderValue::from_str(&m.jar(c)).unwrap());
    }
    if let Some(o) = origin {
        h.insert("origin", HeaderValue::from_str(o).unwrap());
    }
    WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{}", m.ws)).await.map_err(|e| e.to_string())
}

async fn call(c: &WsClient, method: &str) -> Vec<Value> {
    call_with(c, method, json!({})).await
}

async fn call_with(c: &WsClient, method: &str, params: Value) -> Vec<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut sub: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let mut out = Vec::new();
    while let Some(Ok(item)) = sub.next().await {
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

/// AUTH-7: whether a connection is no one — the anonymous view, which
/// reaches only the auth door.
async fn anonymous(c: &WsClient) -> bool {
    call(c, "session.whoami").await[0]["error_kind"] == "unauthenticated"
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
    // AUTH-7: the socket admits an upgrade that proves no one into the
    // anonymous view (it was refused 401): it can call nothing but the door.
    let ws = socket(&m, None, Some(&m.origin())).await.expect("the anonymous view");
    assert!(anonymous(&ws).await);
    assert_eq!(call(&ws, "tickets.health").await[0]["error_kind"], "unauthenticated");
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
    let app = m.http.get(m.url("/app/")).header("cookie", m.jar(&id)).send().await.unwrap();
    assert_eq!(app.status(), 200, "the app, for a session");
    let me: Value = m.http.get(m.url("/login/me")).header("cookie", m.jar(&id)).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["principal"], format!("idp:{ALICE}"));

    let ws = socket(&m, Some(&id), Some(&m.origin())).await.expect("the socket admits the session");
    assert_eq!(call(&ws, "session.whoami").await[0]["principal"], format!("idp:{ALICE}"));
    // AUTH-7: Alice holds nothing yet — a typed refusal (health answered
    // her a count of 0 before), and she waits for an admin.
    let health = call(&ws, "tickets.health").await;
    assert_eq!((health[0]["error_kind"].as_str(), health[0]["grant"].as_str()), (Some("forbidden"), Some("tickets.health")), "{health:?}");
    assert_eq!(call(&ws, "members.mine").await[0]["principal"], format!("idp:{ALICE}"));
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
        .find_map(|c| c.strip_prefix(format!("menger_login_{}=", m.shell).as_str()).map(|v| v.split(';').next().unwrap().to_string())).unwrap();

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
        .find_map(|c| c.strip_prefix(format!("menger_login_{}=", m.shell).as_str()).map(|v| v.split(';').next().unwrap().to_string())).unwrap();
    assert_ne!(txn, txn2);
    let callback = location(&m.http.get(&to).send().await.unwrap());
    let done = m.http.get(&callback).header("cookie", format!("menger_login_{}={txn2}", m.shell)).send().await.unwrap();
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
async fn the_page_s_nostr_script_gets_json_and_reasons() {
    let m = start().await;
    let keys = Keys::generate();
    let tag = |t: [&str; 2]| Tag::parse(t).unwrap();
    let sign = |u: &str| {
        let ev = EventBuilder::new(Kind::HttpAuth, "").tags([tag(["u", u]), tag(["method", "POST"])]).finalize(&keys).unwrap();
        base64::engine::general_purpose::STANDARD.encode(serde_json::to_string(&ev).unwrap())
    };
    let post = |b64: String, origin: String| {
        m.http.post(m.url("/login/nostr?next=/app/")).header("authorization", format!("Nostr {b64}"))
            .header("origin", origin).header("accept", "application/json").send()
    };
    // Signed for localhost, the shell's other name: accepted, as JSON.
    let local = format!("http://localhost:{}/login/nostr", m.shell);
    let b64 = sign(&local);
    let r = post(b64.clone(), format!("http://localhost:{}", m.shell)).await.unwrap();
    assert_eq!(r.status(), 200);
    let id = session_from(&r).expect("a session cookie with the JSON");
    assert_eq!(r.json::<Value>().await.unwrap()["next"], "/app/");
    let ws = socket(&m, Some(&id), Some(&m.origin())).await.unwrap();
    assert_eq!(call(&ws, "session.whoami").await[0]["principal"], format!("nostr:{}", keys.public_key().to_hex()));
    // The same signature again: refused, and the reason said.
    let again = post(b64, m.origin()).await.unwrap();
    assert_eq!(again.status(), 401);
    let why = again.json::<Value>().await.unwrap()["error"].as_str().unwrap().to_string();
    assert!(why.contains("already used"), "{why}");
}

#[tokio::test]
async fn sign_out_ends_the_session_and_its_open_socket_stops_answering() {
    let m = start().await;
    let id = sign_in(&m).await;
    let ws = socket(&m, Some(&id), Some(&m.origin())).await.unwrap();
    // AUTH-7: members.mine, which any signed-in caller reaches (Alice holds
    // nothing, so tickets.health is refused her by the gate).
    assert_eq!(call(&ws, "members.mine").await[0]["type"], "me");
    let out = m.http.post(m.url("/logout")).header("cookie", m.jar(&id)).header("origin", m.origin()).send().await.unwrap();
    assert_eq!(out.status(), 303);
    let after = call(&ws, "members.mine").await;
    assert_eq!(after[0]["type"], "error");
    assert!(after[0]["message"].as_str().unwrap().contains("signed out"), "{after:?}");
    // IDENTITY-37: whoami too — the connection is no one now (AUTH-11: and
    // is refused as anyone anonymous is).
    let who = call(&ws, "session.whoami").await;
    assert_eq!(who[0]["error_kind"], "unauthenticated", "{who:?}");
    // AUTH-7: a new socket on the ended session's cookie is the anonymous
    // view (it was refused 401): the browser's cookie is no credential.
    assert!(anonymous(&socket(&m, Some(&id), Some(&m.origin())).await.unwrap()).await, "and no new socket as Alice");
    assert_eq!(m.http.get(m.url("/login/me")).header("cookie", m.jar(&id)).send().await.unwrap().status(), 401);
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

// ── IDENTITY-30: WorkOS ───────────────────────────────────────────────────

/// A button's whole round trip through the WorkOS fixture: the authorize
/// request's parameters, and the callback's answer.
async fn through_workos(m: &Menger, query: &str) -> (HashMap<String, String>, reqwest::Response) {
    let out = m.http.get(m.url(&format!("/login/start?{query}"))).send().await.unwrap();
    assert_eq!(out.status(), 302, "{query}");
    let to = url::Url::parse(&location(&out)).unwrap();
    let q: HashMap<_, _> = to.query_pairs().into_owned().collect();
    let txn = out.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok())
        .find_map(|c| c.strip_prefix(format!("menger_login_{}=", m.shell).as_str()).map(|v| v.split(';').next().unwrap().to_string())).unwrap();
    let callback = location(&m.http.get(to.as_str()).send().await.unwrap());
    let done = m.http.get(&callback).header("cookie", format!("menger_login_{}={txn}", m.shell)).send().await.unwrap();
    (q, done)
}

#[tokio::test]
async fn our_page_first_and_the_email_goes_to_workos_which_names_the_person() {
    let (m, w) = start_with(true).await;
    let w = w.unwrap();
    // menger's own page first: an email field, the curated ways in, nostr;
    // no password form for people.
    let page = m.http.get(m.url("/login?next=/app/")).send().await.unwrap();
    assert_eq!(page.status(), 200);
    let page = page.text().await.unwrap();
    for want in ["id=\"email\"", "name=\"login_hint\"", "Continue with Google", "Continue with Apple", "Continue with nostr"] {
        assert!(page.contains(want), "the page shows {want}");
    }
    assert!(!page.contains("name=\"password\""), "no password form for people");

    assert!(page.contains("Create an account"), "an address with no account yet can make one");
    // "Create an account": AuthKit's sign-up, the address filled in.
    let (q, _) = through_workos(&m, "button=email&signup=1&login_hint=new%40example.com").await;
    assert_eq!((q["provider"].as_str(), q["screen_hint"].as_str(), q["login_hint"].as_str()), ("authkit", "sign-up", "new@example.com"));

    // The address goes to AuthKit as login_hint, `next` carried through.
    let (q, done) = through_workos(&m, "button=email&login_hint=ada%40example.com&next=/app/").await;
    assert_eq!(q["provider"], "authkit");
    assert_eq!(q["login_hint"], "ada@example.com");
    assert!(!q.contains_key("screen_hint"), "signing in, not up");
    assert_eq!((q["client_id"].as_str(), q["code_challenge_method"].as_str()), (WORKOS_CLIENT, "S256"));
    assert_eq!(done.status(), 303, "the fixture refuses a client secret, so none was sent");
    assert_eq!(location(&done), "/app/");
    let id = session_from(&done).expect("a session");

    let me: Value = m.http.get(m.url("/login/me")).header("cookie", m.jar(&id)).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["principal"], "oidc:workos/user_01ADA");
    assert_eq!(me["name"], "Ada Lovelace", "named by the response's user, not the token");
    assert_eq!(me["provider"], "workos");
    let ws = socket(&m, Some(&id), Some(&m.origin())).await.unwrap();
    assert_eq!(call(&ws, "session.whoami").await[0]["principal"], "oidc:workos/user_01ADA");

    // Signing out ends menger's session, then WorkOS's.
    let out = m.http.post(m.url("/logout")).header("cookie", m.jar(&id)).header("origin", m.origin()).send().await.unwrap();
    assert_eq!(out.status(), 303);
    let to = url::Url::parse(&location(&out)).unwrap();
    assert_eq!(to.as_str().split('?').next().unwrap(), format!("{}/user_management/sessions/logout", w.base));
    let q: HashMap<_, _> = to.query_pairs().into_owned().collect();
    assert_eq!(q["session_id"], "session_01ADA");
    assert_eq!(q["return_to"], m.url("/login"));
    assert_eq!(m.http.get(m.url("/login/me")).header("cookie", m.jar(&id)).send().await.unwrap().status(), 401);
}

#[tokio::test]
async fn automation_signs_in_with_plexus_idp_at_login_password() {
    let (m, _) = start_with(true).await;
    let page = m.http.get(m.url("/login?password")).send().await.unwrap().text().await.unwrap();
    for want in ["name=\"password\"", "With a plexus-idp account", "id=\"email\""] {
        assert!(page.contains(want), "/login?password shows {want}");
    }
    // plexus-idp's password grant: what the smoke and the e2e scripts use.
    let id = sign_in(&m).await;
    let ws = socket(&m, Some(&id), Some(&m.origin())).await.unwrap();
    assert_eq!(call(&ws, "session.whoami").await[0]["principal"], format!("idp:{ALICE}"));
}

#[tokio::test]
async fn each_workos_button_names_its_way_in() {
    let (m, _) = start_with(true).await;
    let (q, done) = through_workos(&m, "button=Google").await;
    assert_eq!(q["provider"], "GoogleOAuth");
    assert_eq!(done.status(), 303);
    let (q, _) = through_workos(&m, "button=Apple").await;
    assert_eq!(q["provider"], "AppleOAuth");
    let (q, _) = through_workos(&m, "button=email&login_hint=ada%40acme.example").await;
    assert_eq!((q["provider"].as_str(), q["login_hint"].as_str()), ("authkit", "ada@acme.example"), "AuthKit routes by the email's domain");
}

#[tokio::test]
async fn a_workos_token_for_another_client_signs_no_one_in() {
    let (m, _) = start_with(true).await;
    let (_, done) = through_workos(&m, "button=Other").await;
    assert_eq!(done.status(), 401);
    assert!(session_from(&done).is_none());
}

// ── IDENTITY-35: the dev sign-in ──────────────────────────────────────────

async fn dev_post(m: &Menger, form: &[(&str, &str)]) -> reqwest::Response {
    m.http.post(m.url("/login/dev")).header("origin", m.origin()).form(form).send().await.unwrap()
}

#[tokio::test]
async fn the_checkbox_sends_this_browser_s_sign_in_to_plexus_idp() {
    let m = start_dev().await;
    let plain = m.http.get(m.url("/login")).send().await.unwrap().text().await.unwrap();
    assert!(plain.contains(r#"id="dev-login""#) && !plain.contains(r#"id="dev-login" checked"#), "the checkbox, unticked");
    assert!(plain.contains("Continue with Google") && !plain.contains("/login/dev"), "the ordinary page");

    let dev = m.http.get(m.url("/login")).header("cookie", "menger_dev_login=1").send().await.unwrap().text().await.unwrap();
    assert!(dev.contains(r#"id="dev-login" checked"#));
    for want in [r#"<span class="uname">viewer</span>"#, "<b>viewer</b>everywhere", r#"<span class="uname">mixed</span>"#, "editor on OUGHT, viewer on PLEXUS", "<b>editor</b><code>OUGHT</code>", r#"name="role""#, r#"name="password""#] {
        assert!(dev.contains(want), "the dev panel shows {want}");
    }
    assert!(!dev.contains("Continue with Google") && !dev.contains("Continue with nostr"), "plexus-idp alone");

    let r = m.http.get(m.url("/login/start?button=Google&next=/app/")).header("cookie", "menger_dev_login=1").send().await.unwrap();
    assert_eq!(r.status(), 303);
    assert_eq!(location(&r), "/login?next=%2Fapp%2F", "every way in is plexus-idp");
}

#[tokio::test]
async fn a_seeded_user_signs_in_with_its_roles() {
    let m = start_dev().await;
    let r = dev_post(&m, &[("user", "mixed"), ("role", "admin"), ("scope", "*"), ("next", "/app/")]).await;
    assert_eq!(r.status(), 303, "{}", r.text().await.unwrap());
    assert_eq!(location(&r), "/app/");
    let id = session_from(&r).unwrap();
    let ws = socket(&m, Some(&id), Some(&m.origin())).await.unwrap();
    let mine = call(&ws, "members.mine").await;
    let grants: Vec<(String, String, String)> = mine
        .iter()
        .filter(|e| e["type"] == "grant")
        .map(|e| (e["grant"]["scope"].as_str().unwrap().into(), e["grant"]["role"].as_str().unwrap().into(), e["grant"]["granted_by"].as_str().unwrap().into()))
        .collect();
    assert_eq!(
        grants,
        vec![("OUGHT".into(), "editor".into(), "dev-login".into()), ("PLEXUS".into(), "viewer".into(), "dev-login".into())],
        "the roster's roles, not the form's"
    );
}

#[tokio::test]
async fn anyone_signs_in_with_the_role_asked_for() {
    let m = start_dev().await;
    let r = dev_post(&m, &[("user", "Drifter"), ("role", "editor"), ("scope", "identity"), ("next", "/")]).await;
    assert_eq!(r.status(), 303, "{}", r.text().await.unwrap());
    let ws = socket(&m, Some(&session_from(&r).unwrap()), Some(&m.origin())).await.unwrap();
    let mine = call(&ws, "members.mine").await;
    assert!(mine.iter().any(|e| e["grant"]["scope"] == "IDENTITY" && e["grant"]["role"] == "editor"), "{mine:?}");

    // An account plexus-idp lacks is registered there; this one's socket is
    // down, and the page says so.
    let r = dev_post(&m, &[("user", "newcomer"), ("role", "none")]).await;
    assert_eq!(r.status(), 502);
    assert!(r.text().await.unwrap().contains("plexus-idp&#39;s socket"));

    // An account by its own password: exactly that account, no grants.
    let r = dev_post(&m, &[("user", "alice"), ("password", "s3cret")]).await;
    assert_eq!(r.status(), 303);
    let r = dev_post(&m, &[("user", "alice"), ("password", "nope")]).await;
    assert_eq!(r.status(), 401);
}

#[tokio::test]
async fn the_dev_sign_in_is_not_offered_unless_configured() {
    let m = start().await;
    assert!(!m.http.get(m.url("/login")).header("cookie", "menger_dev_login=1").send().await.unwrap().text().await.unwrap().contains(r#"id="dev-login""#));
    assert_eq!(dev_post(&m, &[("user", "viewer")]).await.status(), 404);
}

// ── IDENTITY-38: ready for prod behind the login ───────────────────────────

#[tokio::test]
async fn each_shell_s_session_is_its_own_cookie() {
    let m = start().await;
    let id = sign_in(&m).await;
    assert_eq!(m.cookie(), format!("menger_session_{}", m.shell));
    // Another server's cookie of the same value, or the old shared name,
    // opens nothing here: on one host, prod and test keep apart.
    for other in [format!("menger_session={id}"), format!("menger_session_1={id}")] {
        let r = m.http.get(m.url("/login/me")).header("cookie", other.as_str()).send().await.unwrap();
        assert_eq!(r.status(), 401, "{other}");
        let mut h = HeaderMap::new();
        h.insert("cookie", HeaderValue::from_str(&other).unwrap());
        h.insert("origin", HeaderValue::from_str(&m.origin()).unwrap());
        let ws = WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{}", m.ws)).await;
        // AUTH-7: another server's cookie proves no one here — the anonymous
        // view (it was a 401).
        assert!(anonymous(&ws.unwrap()).await, "{other}: the socket takes no other server's cookie");
    }
    assert!(socket(&m, Some(&id), Some(&m.origin())).await.is_ok());
    // Sign-out clears this shell's cookie by name.
    let out = m.http.post(m.url("/logout")).header("cookie", m.jar(&id)).header("origin", m.origin()).send().await.unwrap();
    let cleared = out.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok()).any(|c| c.starts_with(&format!("{}=;", m.cookie())) && c.contains("Max-Age=0"));
    assert!(cleared, "sign-out clears {}", m.cookie());
}

/// IDENTITY-38: a frame's opaque origin sends no session cookie; the
/// vendored libraries it loads (mermaid) answer without one. Nothing else
/// under /app does.
#[tokio::test]
async fn a_frame_loads_the_vendored_libraries_without_a_session() {
    let m = start().await;
    let lib = m.http.get(m.url("/app/vendor/mermaid/mermaid.min.js")).header("origin", "null").send().await.unwrap();
    assert_ne!(lib.status(), 303, "no detour to the login page");
    for path in ["/app/", "/app/index.html", "/app/vendorx/a.js", "/app/sdk/menger-sdk.js"] {
        let r = m.http.get(m.url(path)).send().await.unwrap();
        assert_eq!(r.status(), 303, "{path} still needs a session");
    }
}

#[tokio::test]
async fn every_page_refuses_to_be_framed() {
    let m = start().await;
    let id = sign_in(&m).await;
    for path in ["/", "/app/", "/vanilla", "/login", "/favicon.svg", "/login/me"] {
        let r = m.http.get(m.url(path)).header("cookie", m.jar(&id)).send().await.unwrap();
        let h = r.headers();
        assert_eq!(h.get("x-frame-options").and_then(|v| v.to_str().ok()), Some("DENY"), "{path}");
        assert_eq!(h.get("x-content-type-options").and_then(|v| v.to_str().ok()), Some("nosniff"), "{path}");
        assert_eq!(h.get("referrer-policy").and_then(|v| v.to_str().ok()), Some("same-origin"), "{path}");
        let csp = h.get("content-security-policy").and_then(|v| v.to_str().ok()).unwrap_or_default();
        assert!(csp.contains("frame-ancestors 'none'"), "{path}: {csp}");
    }
}

#[tokio::test]
async fn the_login_page_says_only_its_own_words() {
    let m = start().await;
    let lure = "Accounts moved: sign in at evil.example";
    let q = url::form_urlencoded::byte_serialize(lure.as_bytes()).collect::<String>();
    let page = m.http.get(m.url(&format!("/login?error={q}"))).send().await.unwrap().text().await.unwrap();
    assert!(!page.contains("evil.example"), "a link's error text is not shown");
    let back = m.http.get(m.url(&format!("/login/callback?error=server_error&error_description={q}"))).send().await.unwrap();
    assert_eq!(back.status(), 401);
    let text = back.text().await.unwrap();
    assert!(!text.contains("evil.example"), "a provider's description is logged, not shown");
    assert!(text.contains("did not sign you in"));
    let cancelled = m.http.get(m.url("/login/callback?error=access_denied&error_description=x")).send().await.unwrap().text().await.unwrap();
    assert!(cancelled.contains("The sign-in was cancelled."));
}

#[tokio::test]
async fn a_storm_of_anonymous_starts_is_bounded() {
    // Each anonymous GET /login/start keeps a transaction; they are capped
    // (sessions.rs, MAX_PENDING) — here, only that a burst is all answered
    // and the latest still completes its round trip.
    let m = start().await;
    for _ in 0..200 {
        let r = m.http.get(m.url("/login/start?button=Google&next=/app/")).send().await.unwrap();
        assert_eq!(r.status(), 302);
    }
}


// ── AUTH-7: a person signs in from a client, through the browser ─────────

/// AUTH-7 (AUTH-5's loopback flow), end to end: a client with no
/// credential asks `auth.login {browser {redirect}}` on the anonymous view;
/// the person signs in on menger's page as anyone does (here plexus-idp's
/// password form, the fixture issuer); the shell sends the browser on to the
/// client's loopback with a one-time code for its state; the client trades
/// it for an `mgs_` session, which opens later connections as the person.
#[tokio::test]
async fn a_client_signs_a_person_in_through_the_browser_and_a_loopback_code() {
    let m = start().await;
    let client = socket(&m, None, None).await.expect("the anonymous view");
    let redirect = "http://127.0.0.1:53817/cb";
    let url = call_with(&client, "auth.login", json!({ "flow": { "browser": { "redirect": redirect } } })).await;
    assert_eq!(url[0]["type"], "login_url", "{url:?}");
    let state = url[0]["state"].as_str().unwrap().to_string();
    let page_url = url[0]["url"].as_str().unwrap().to_string();
    assert_eq!(page_url, format!("{}/login?cli=1&state={state}", m.origin()));

    // Anything but a loopback port is refused, in band.
    for bad in ["https://evil.example/cb", "http://127.0.0.1/cb", "http://10.0.0.1:53817/cb"] {
        let r = call_with(&client, "auth.login", json!({ "flow": { "browser": { "redirect": bad } } })).await;
        assert_eq!(r[0]["type"], "error", "{bad}: {r:?}");
    }

    // The page carries the sign-in on: every way in goes to /auth/cli.
    let page = m.http.get(&page_url).send().await.unwrap().text().await.unwrap();
    let next = format!("/auth/cli?state={state}");
    assert!(page.contains(&format!(r#"name="next" value="{next}""#)), "{page}");
    let signed = m.http.post(m.url("/login")).form(&[("username", "alice"), ("password", "s3cret"), ("next", next.as_str())]).send().await.unwrap();
    assert_eq!((signed.status().as_u16(), location(&signed)), (303, next.clone()));
    let id = session_from(&signed).expect("the browser is signed in too");

    // Just signed in: straight back to the client, with a code for its state.
    let back = m.http.get(m.url(&next)).header("cookie", m.jar(&id)).send().await.unwrap();
    assert_eq!(back.status(), 303);
    let to = url::Url::parse(&location(&back)).unwrap();
    assert_eq!(format!("{}://{}:{}{}", to.scheme(), to.host_str().unwrap(), to.port().unwrap(), to.path()), redirect);
    let q: HashMap<_, _> = to.query_pairs().into_owned().collect();
    assert_eq!(q["state"], state);
    let code = q["code"].clone();
    // The state is spent: the link does nothing twice.
    assert_eq!(m.http.get(m.url(&next)).header("cookie", m.jar(&id)).send().await.unwrap().status(), 400);

    // The code, for another state: refused — and spent.
    let wrong = call_with(&client, "auth.login", json!({ "flow": { "code": { "code": code, "state": "s_other" } } })).await;
    assert_eq!(wrong[0]["type"], "error", "{wrong:?}");
    // A fresh sign-in, traded properly.
    let url = call_with(&client, "auth.login", json!({ "flow": { "browser": { "redirect": redirect } } })).await;
    let state = url[0]["state"].as_str().unwrap().to_string();
    let next = format!("/auth/cli?state={state}");
    let signed = m.http.post(m.url("/login")).form(&[("username", "alice"), ("password", "s3cret"), ("next", next.as_str())]).send().await.unwrap();
    let id = session_from(&signed).unwrap();
    let back = m.http.get(m.url(&next)).header("cookie", m.jar(&id)).send().await.unwrap();
    let to = url::Url::parse(&location(&back)).unwrap();
    let code = to.query_pairs().find(|(k, _)| k == "code").unwrap().1.to_string();
    let session = call_with(&client, "auth.login", json!({ "flow": { "code": { "code": code, "state": state } } })).await;
    assert_eq!(session[0]["type"], "session", "{session:?}");
    assert_eq!((session[0]["principal"].as_str(), session[0]["name"].as_str()), (Some(format!("idp:{ALICE}").as_str()), Some("alice")));
    let token = session[0]["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("mgs_"), "{token}");
    let again = call_with(&client, "auth.login", json!({ "flow": { "code": { "code": code, "state": state } } })).await;
    assert_eq!(again[0]["type"], "error", "a code works once: {again:?}");

    // The session opens a later connection as the person.
    let mut h = HeaderMap::new();
    h.insert("authorization", HeaderValue::from_str(&format!("Bearer {token}")).unwrap());
    let ws = WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{}", m.ws)).await.unwrap();
    assert_eq!(call(&ws, "session.whoami").await[0]["principal"], format!("idp:{ALICE}"));
    assert_eq!(call(&ws, "auth.whoami").await[0]["principal"], format!("idp:{ALICE}"));
}

/// AUTH-7: a loopback sign-in whose browser signed in earlier is asked
/// first — a button POSTed from menger's own page — so a link alone never
/// hands a sign-in to a program on the machine; a POST from another page is
/// refused.
#[tokio::test]
async fn a_loopback_sign_in_on_an_older_session_asks_first() {
    let m = start().await;
    let id = sign_in(&m).await;
    let client = socket(&m, None, None).await.unwrap();
    let url = call_with(&client, "auth.login", json!({ "flow": { "browser": { "redirect": "http://localhost:4999/cb" } } })).await;
    let state = url[0]["state"].as_str().unwrap().to_string();
    // Signed in already: the page goes straight to /auth/cli.
    let page = m.http.get(url[0]["url"].as_str().unwrap()).header("cookie", m.jar(&id)).send().await.unwrap();
    assert_eq!(location(&page), format!("/auth/cli?state={state}"));
    let foreign = m.http.post(m.url("/auth/cli")).header("cookie", m.jar(&id)).header("origin", "http://127.0.0.1:3000")
        .form(&[("state", state.as_str())]).send().await.unwrap();
    assert_eq!(foreign.status(), 403);
    let ok = m.http.post(m.url("/auth/cli")).header("cookie", m.jar(&id)).header("origin", m.origin())
        .form(&[("state", state.as_str())]).send().await.unwrap();
    assert_eq!(ok.status(), 303);
    assert!(location(&ok).starts_with("http://localhost:4999/cb?code=c_"), "{}", location(&ok));
}
