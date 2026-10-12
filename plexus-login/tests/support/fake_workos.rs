//! A fake WorkOS API: the documented shapes of the User Management
//! endpoints the WorkOS backend calls, its refusals, and a record of every
//! request — so a test can check what was sent (the IP, the user agent, the
//! secret) and what was issued (every code and token, to look for in logs).
//!
//! Who an address is picks a scenario:
//!
//! | address | first authenticate answers |
//! | --- | --- |
//! | `verify@example.test` | `email_verification_required` (code `424242`) |
//! | `orgs@example.test` | `organization_selection_required` (two organizations) |
//! | `radar@example.test` | `radar_email_challenge` (code `135790`) |
//! | `sms@example.test` | `radar_sms_challenge` |
//! | `enrol@example.test` | `mfa_enrollment` until it has a factor |
//! | any with a factor | `mfa_challenge` |
//! | `nobody…` with a wrong code | 404 `entity_not_found` (as WorkOS answers an unknown address) |
//!
//! Social: `GoogleOAuth` signs in `google-user@example.test`, `GitHubOAuth`
//! `verify@example.test`, `AppleOAuth` `orgs@example.test`; an
//! `organization_id` signs in the `login_hint`. Domains: `sso.example` is a
//! verified domain with an active OktaSAML connection; `pending.example`
//! has an active connection but an unverified domain; `nossoconn.example`
//! is verified with no connection.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::Engine as _;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{now, KEY_PEM, JWK};

pub const CLIENT: &str = "client_01FAKECLIENT";
pub const KEY: &str = "sk_test_fakeworkoskey0123456789";

#[derive(Clone)]
struct Pending {
    email: String,
    kind: &'static str,
    passed: Vec<&'static str>,
}

#[derive(Default)]
pub struct FakeState {
    codes: HashMap<String, String>,
    pub sends: Vec<Value>,
    pub auths: Vec<Value>,
    users: HashMap<String, String>,
    /// factor id → (user id, base32 secret)
    factors: HashMap<String, (String, String)>,
    challenges: HashMap<String, String>,
    pendings: HashMap<String, Pending>,
    oauth: HashMap<String, (String, String, String)>,
    refresh: HashMap<String, (String, String)>,
    pub revoked: HashSet<String>,
    pub authorizes: Vec<HashMap<String, String>>,
    /// Every secret the fake issued: codes, tokens, pending tokens, TOTP
    /// secrets.
    pub issued: Vec<String>,
    pub calls: Vec<String>,
    n: u64,
    /// The access tokens' lifetime.
    pub token_secs: i64,
    iss: String,
}

#[derive(Clone)]
pub struct FakeWorkOs {
    pub base: String,
    pub state: Arc<Mutex<FakeState>>,
}

impl FakeWorkOs {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let issuer = format!("{base}/user_management/{CLIENT}");
        let w = FakeWorkOs { base: base.clone(), state: Arc::new(Mutex::new(FakeState { token_secs: 300, iss: issuer.clone(), ..Default::default() })) };
        let discovery = json!({ "issuer": issuer, "jwks_uri": format!("{base}/sso/jwks/{CLIENT}"),
            "authorization_endpoint": format!("{base}/user_management/authorize"),
            "token_endpoint": format!("{base}/user_management/authenticate"), "response_types_supported": ["code"] });
        let jwks = json!({ "keys": [serde_json::from_str::<Value>(JWK).unwrap()] });
        let app = Router::new()
            .route(&format!("/user_management/{CLIENT}/.well-known/openid-configuration"), get(move || async move { Json(discovery) }))
            .route(&format!("/sso/jwks/{CLIENT}"), get(move || async move { Json(jwks) }))
            .route("/user_management/magic_auth", post(magic_auth))
            .route("/user_management/authenticate", post(authenticate))
            .route("/user_management/authorize", get(authorize))
            .route("/organizations", get(organizations))
            .route("/connections", get(connections))
            .route("/user_management/users/{id}/auth_factors", post(enroll).get(list_factors))
            .route("/auth/factors/{id}/challenge", post(challenge))
            .route("/auth/challenges/{id}/verify", post(verify))
            .route("/auth/factors/{id}", delete(delete_factor))
            .route("/user_management/sessions/revoke", post(revoke))
            .with_state(w.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        w
    }

    /// The last code sent to `email`, once it has been (the send runs after
    /// the page answers).
    pub async fn code_for(&self, email: &str) -> String {
        for _ in 0..100 {
            if let Some(c) = self.state.lock().unwrap().codes.get(email).cloned() {
                return c;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("no code was sent to {email}");
    }

    pub fn sends_to(&self, email: &str) -> Vec<Value> {
        self.state.lock().unwrap().sends.iter().filter(|s| s["email"] == email).cloned().collect()
    }

    pub fn last_auth(&self, grant: &str) -> Option<Value> {
        self.state.lock().unwrap().auths.iter().rev().find(|a| a["grant_type"] == grant).cloned()
    }

    pub fn issued(&self) -> Vec<String> {
        self.state.lock().unwrap().issued.clone()
    }

    pub fn revoke_all_sessions(&self) {
        let mut st = self.state.lock().unwrap();
        let sids: Vec<String> = st.refresh.values().map(|(_, s)| s.clone()).collect();
        st.revoked.extend(sids);
    }
}

fn bearer_ok(h: &HeaderMap) -> bool {
    h.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {KEY}"))
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "message": "Unauthorized" }))).into_response()
}

fn digits(n: u64) -> String {
    format!("{:06}", (n * 7919 + 104729) % 1_000_000)
}

async fn magic_auth(State(w): State<FakeWorkOs>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if !bearer_ok(&h) {
        return unauthorized();
    }
    let mut st = w.state.lock().unwrap();
    st.calls.push("magic_auth".into());
    st.n += 1;
    let email = b["email"].as_str().unwrap_or_default().to_string();
    let code = digits(st.n + now() as u64);
    st.codes.insert(email.clone(), code.clone());
    st.issued.push(code.clone());
    st.sends.push(b.clone());
    (StatusCode::CREATED, Json(json!({ "object": "magic_auth", "id": format!("magic_auth_{}", st.n), "user_id": "user_x", "email": email, "expires_at": "2099-01-01T00:00:00.000Z", "code": code }))).into_response()
}

fn user_id(st: &mut FakeState, email: &str) -> String {
    if let Some(id) = st.users.get(email) {
        return id.clone();
    }
    st.n += 1;
    let id = format!("user_{:04}", st.n);
    st.users.insert(email.to_string(), id.clone());
    id
}

fn user_json(st: &mut FakeState, email: &str) -> Value {
    let id = user_id(st, email);
    json!({ "object": "user", "id": id, "email": email, "first_name": "Test", "last_name": "Person", "email_verified": true })
}

fn pending(st: &mut FakeState, email: &str, kind: &'static str, passed: &[&'static str]) -> String {
    st.n += 1;
    let t = format!("pat_{}_{}", st.n, &super::random()[..12]);
    st.pendings.insert(t.clone(), Pending { email: email.into(), kind, passed: passed.to_vec() });
    st.issued.push(t.clone());
    t
}

/// The next answer for `email`, having passed the steps in `passed`.
fn gate(st: &mut FakeState, email: &str, passed: &[&'static str], method: &str) -> Response {
    let bad = |v: Value| (StatusCode::BAD_REQUEST, Json(v)).into_response();
    let has_factor = {
        let id = user_id(st, email);
        st.factors.values().any(|(u, _)| *u == id)
    };
    if email == "verify@example.test" && !passed.contains(&"verify") {
        let p = pending(st, email, "verify", passed);
        return bad(json!({ "code": "email_verification_required", "message": "Email ownership must be verified before authentication.", "pending_authentication_token": p, "email": email, "email_verification_id": "email_verification_1" }));
    }
    if email == "orgs@example.test" && !passed.contains(&"org") {
        let p = pending(st, email, "org", passed);
        let user = user_json(st, email);
        return bad(json!({ "code": "organization_selection_required", "message": "The user must choose an organization to finish their authentication.", "pending_authentication_token": p,
            "organizations": [{ "id": "org_FOO", "name": "Foo Corp" }, { "id": "org_BAR", "name": "Bar <Corp>" }], "user": user }));
    }
    if email == "radar@example.test" && !passed.contains(&"radar") {
        let p = pending(st, email, "radar", passed);
        let user = user_json(st, email);
        return bad(json!({ "code": "radar_email_challenge", "message": "The user must complete a Radar challenge to finish authenticating.", "pending_authentication_token": p, "radar_challenge_id": "radar_challenge_1", "user": user }));
    }
    if email == "sms@example.test" {
        let p = pending(st, email, "sms", passed);
        let user = user_json(st, email);
        return bad(json!({ "code": "radar_sms_challenge", "message": "The user must complete a Radar challenge to finish authenticating.", "pending_authentication_token": p, "user": user }));
    }
    if email == "enrol@example.test" && !has_factor && !passed.contains(&"mfa") {
        let p = pending(st, email, "mfa", passed);
        let user = user_json(st, email);
        return bad(json!({ "code": "mfa_enrollment", "message": "The user must enroll in MFA to finish authenticating.", "pending_authentication_token": p, "user": user }));
    }
    if has_factor && !passed.contains(&"mfa") {
        let p = pending(st, email, "mfa", passed);
        let id = user_id(st, email);
        let factors: Vec<Value> = st.factors.iter().filter(|(_, (u, _))| *u == id).map(|(f, _)| json!({ "id": f, "type": "totp" })).collect();
        let user = user_json(st, email);
        return bad(json!({ "code": "mfa_challenge", "message": "The user must complete an MFA challenge to finish authenticating.", "pending_authentication_token": p, "authentication_factors": factors, "user": user }));
    }
    success(st, email, None, method)
}

fn success(st: &mut FakeState, email: &str, sid: Option<String>, method: &str) -> Response {
    let user = user_json(st, email);
    st.n += 1;
    let sid = sid.unwrap_or_else(|| format!("session_{:04}", st.n));
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some("fake-1".into());
    let iss = st.iss.clone();
    let token = encode(
        &h,
        &json!({ "iss": iss, "sub": user["id"], "sid": sid, "jti": format!("jti{}", st.n), "client_id": CLIENT, "exp": now() + st.token_secs, "iat": now() - 5 }),
        &EncodingKey::from_rsa_pem(KEY_PEM.as_bytes()).unwrap(),
    )
    .unwrap();
    let refresh = format!("rt_{}_{}", st.n, &super::random()[..16]);
    st.refresh.insert(refresh.clone(), (email.to_string(), sid));
    st.issued.push(token.clone());
    st.issued.push(refresh.clone());
    Json(json!({ "user": user, "organization_id": null, "access_token": token, "refresh_token": refresh, "authentication_method": method })).into_response()
}

async fn authenticate(State(w): State<FakeWorkOs>, Json(b): Json<Value>) -> Response {
    let bad = |v: Value| (StatusCode::BAD_REQUEST, Json(v)).into_response();
    let mut st = w.state.lock().unwrap();
    st.auths.push(b.clone());
    if b["client_id"] != CLIENT || b["client_secret"] != KEY {
        return bad(json!({ "error": "invalid_client", "error_description": "Invalid client secret." }));
    }
    let s = |k: &str| b[k].as_str().unwrap_or_default().to_string();
    match s("grant_type").as_str() {
        "urn:workos:oauth:grant-type:magic-auth:code" => {
            let email = s("email");
            if st.codes.get(&email).is_some_and(|c| *c == s("code")) {
                st.codes.remove(&email);
                return gate(&mut st, &email, &[], "MagicAuth");
            }
            if email.starts_with("nobody") {
                return (StatusCode::NOT_FOUND, Json(json!({ "message": format!("User not found: '{email}'."), "code": "entity_not_found", "entity_id": email }))).into_response();
            }
            bad(json!({ "code": "invalid_one_time_code", "message": "Invalid one-time code." }))
        }
        "authorization_code" => {
            let Some((challenge, redirect, email)) = st.oauth.remove(&s("code")) else {
                return bad(json!({ "error": "invalid_grant", "error_description": "The code has expired or is invalid." }));
            };
            let got = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(s("code_verifier").as_bytes()));
            if got != challenge || redirect.is_empty() {
                return bad(json!({ "error": "invalid_grant", "error_description": "Invalid code verifier." }));
            }
            gate(&mut st, &email, &[], "OAuth")
        }
        "urn:workos:oauth:grant-type:mfa-totp" => {
            let Some(p) = st.pendings.get(&s("pending_authentication_token")).cloned().filter(|p| p.kind == "mfa") else {
                return bad(json!({ "error": "invalid_grant", "error_description": "Invalid pending authentication token." }));
            };
            let ok = st.challenges.get(&s("authentication_challenge_id")).and_then(|f| st.factors.get(f)).is_some_and(|(_, secret)| super::totp_ok(secret, &s("code")));
            if !ok {
                return bad(json!({ "code": "invalid_one_time_code", "message": "Invalid one-time code." }));
            }
            st.pendings.remove(&s("pending_authentication_token"));
            let mut passed = p.passed.clone();
            passed.push("mfa");
            gate(&mut st, &p.email, &passed, "MagicAuth")
        }
        "urn:workos:oauth:grant-type:email-verification:code" => match st.pendings.get(&s("pending_authentication_token")).cloned() {
            Some(p) if p.kind == "verify" && s("code") == "424242" => {
                st.pendings.remove(&s("pending_authentication_token"));
                let mut passed = p.passed.clone();
                passed.push("verify");
                gate(&mut st, &p.email, &passed, "OAuth")
            }
            _ => bad(json!({ "code": "invalid_one_time_code", "message": "Invalid one-time code." })),
        },
        "urn:workos:oauth:grant-type:organization-selection" => match st.pendings.get(&s("pending_authentication_token")).cloned() {
            Some(p) if p.kind == "org" && ["org_FOO", "org_BAR"].contains(&s("organization_id").as_str()) => {
                st.pendings.remove(&s("pending_authentication_token"));
                let mut passed = p.passed.clone();
                passed.push("org");
                gate(&mut st, &p.email, &passed, "OAuth")
            }
            _ => bad(json!({ "error": "invalid_grant", "error_description": "Invalid pending authentication token." })),
        },
        "urn:workos:oauth:grant-type:radar-email-challenge:code" => match st.pendings.get(&s("pending_authentication_token")).cloned() {
            Some(p) if p.kind == "radar" && s("radar_challenge_id") == "radar_challenge_1" && s("code") == "135790" => {
                st.pendings.remove(&s("pending_authentication_token"));
                let mut passed = p.passed.clone();
                passed.push("radar");
                gate(&mut st, &p.email, &passed, "MagicAuth")
            }
            _ => bad(json!({ "code": "invalid_one_time_code", "message": "Invalid one-time code." })),
        },
        "refresh_token" => {
            let Some((email, sid)) = st.refresh.remove(&s("refresh_token")) else {
                return bad(json!({ "error": "invalid_grant", "error_description": "Invalid refresh token." }));
            };
            if st.revoked.contains(&sid) {
                return bad(json!({ "error": "invalid_grant", "error_description": "Session has already ended." }));
            }
            success(&mut st, &email, Some(sid), "MagicAuth")
        }
        _ => bad(json!({ "error": "unsupported_grant_type" })),
    }
}

async fn authorize(State(w): State<FakeWorkOs>, Query(q): Query<HashMap<String, String>>) -> Response {
    let mut st = w.state.lock().unwrap();
    st.authorizes.push(q.clone());
    let email = match (q.get("provider").map(String::as_str), q.get("organization_id"), q.get("connection_id")) {
        (Some("GoogleOAuth"), _, _) => "google-user@example.test".to_string(),
        (Some("GitHubOAuth"), _, _) => "verify@example.test".to_string(),
        (Some("AppleOAuth"), _, _) => "orgs@example.test".to_string(),
        (_, Some(_), _) => q.get("login_hint").cloned().unwrap_or_else(|| "sso-user@sso.example".into()),
        (_, _, Some(_)) => "conn-user@sso.example".to_string(),
        _ => return Redirect::to(&format!("{}?error=invalid_connection_selector&state={}", q["redirect_uri"], q["state"])).into_response(),
    };
    if q.get("code_challenge_method").map(String::as_str) != Some("S256") || q.get("client_id").map(String::as_str) != Some(CLIENT) {
        return (StatusCode::BAD_REQUEST, "bad request").into_response();
    }
    st.n += 1;
    let code = format!("oauthcode_{}_{}", st.n, &super::random()[..10]);
    st.issued.push(code.clone());
    st.oauth.insert(code.clone(), (q["code_challenge"].clone(), q["redirect_uri"].clone(), email));
    Redirect::to(&format!("{}?code={code}&state={}", q["redirect_uri"], q["state"])).into_response()
}

async fn organizations(State(w): State<FakeWorkOs>, h: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Response {
    if !bearer_ok(&h) {
        return unauthorized();
    }
    w.state.lock().unwrap().calls.push(format!("organizations:{}", q.get("domains").cloned().unwrap_or_default()));
    let org = |id: &str, domain: &str, state: &str| json!({ "object": "organization", "id": id, "name": id, "domains": [{ "object": "organization_domain", "id": format!("org_domain_{id}"), "organization_id": id, "domain": domain, "state": state }] });
    let data = match q.get("domains").map(String::as_str) {
        Some("sso.example") => vec![org("org_SSO", "sso.example", "verified")],
        Some("pending.example") => vec![org("org_PENDING", "pending.example", "pending")],
        Some("nossoconn.example") => vec![org("org_NOCONN", "nossoconn.example", "verified")],
        _ => vec![],
    };
    Json(json!({ "object": "list", "data": data, "list_metadata": { "before": null, "after": null } })).into_response()
}

async fn connections(h: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Response {
    if !bearer_ok(&h) {
        return unauthorized();
    }
    let data = match q.get("organization_id").map(String::as_str) {
        Some(o @ ("org_SSO" | "org_PENDING")) => vec![json!({ "object": "connection", "id": "conn_1", "organization_id": o, "connection_type": "OktaSAML", "state": "active", "name": "Okta" })],
        _ => vec![],
    };
    Json(json!({ "object": "list", "data": data, "list_metadata": { "before": null, "after": null } })).into_response()
}

async fn enroll(State(w): State<FakeWorkOs>, h: HeaderMap, Path(user): Path<String>, Json(b): Json<Value>) -> Response {
    if !bearer_ok(&h) {
        return unauthorized();
    }
    if b["type"] != "totp" {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "message": "type" }))).into_response();
    }
    let mut st = w.state.lock().unwrap();
    st.n += 1;
    let factor = format!("auth_factor_{:04}", st.n);
    let challenge = format!("auth_challenge_{:04}", st.n);
    let secret = super::new_secret();
    st.factors.insert(factor.clone(), (user, secret.clone()));
    st.challenges.insert(challenge.clone(), factor.clone());
    st.issued.push(secret.clone());
    let uri = format!("otpauth://totp/{}:{}?secret={secret}&issuer={}", b["totp_issuer"].as_str().unwrap_or(""), b["totp_user"].as_str().unwrap_or(""), b["totp_issuer"].as_str().unwrap_or(""));
    Json(json!({
        "authentication_factor": { "object": "authentication_factor", "id": factor, "type": "totp", "totp": { "issuer": b["totp_issuer"], "user": b["totp_user"], "qr_code": "data:image/png;base64,iVBORw0KGgo=", "secret": secret, "uri": uri } },
        "authentication_challenge": { "object": "authentication_challenge", "id": challenge, "authentication_factor_id": factor },
    }))
    .into_response()
}

async fn list_factors(State(w): State<FakeWorkOs>, h: HeaderMap, Path(user): Path<String>) -> Response {
    if !bearer_ok(&h) {
        return unauthorized();
    }
    let st = w.state.lock().unwrap();
    let data: Vec<Value> = st.factors.iter().filter(|(_, (u, _))| *u == user).map(|(f, _)| json!({ "object": "authentication_factor", "id": f, "type": "totp", "created_at": "2026-10-11T00:00:00.000Z" })).collect();
    Json(json!({ "object": "list", "data": data })).into_response()
}

async fn challenge(State(w): State<FakeWorkOs>, h: HeaderMap, Path(factor): Path<String>) -> Response {
    if !bearer_ok(&h) {
        return unauthorized();
    }
    let mut st = w.state.lock().unwrap();
    if !st.factors.contains_key(&factor) {
        return (StatusCode::NOT_FOUND, Json(json!({ "message": "not found" }))).into_response();
    }
    st.n += 1;
    let id = format!("auth_challenge_{:04}", st.n);
    st.challenges.insert(id.clone(), factor.clone());
    Json(json!({ "object": "authentication_challenge", "id": id, "authentication_factor_id": factor })).into_response()
}

async fn verify(State(w): State<FakeWorkOs>, h: HeaderMap, Path(ch): Path<String>, Json(b): Json<Value>) -> Response {
    if !bearer_ok(&h) {
        return unauthorized();
    }
    let st = w.state.lock().unwrap();
    let Some(factor) = st.challenges.get(&ch) else {
        return (StatusCode::NOT_FOUND, Json(json!({ "message": "not found" }))).into_response();
    };
    let valid = st.factors.get(factor).is_some_and(|(_, s)| super::totp_ok(s, b["code"].as_str().unwrap_or("")));
    Json(json!({ "challenge": { "object": "authentication_challenge", "id": ch, "authentication_factor_id": factor }, "valid": valid })).into_response()
}

async fn delete_factor(State(w): State<FakeWorkOs>, h: HeaderMap, Path(factor): Path<String>) -> Response {
    if !bearer_ok(&h) {
        return unauthorized();
    }
    match w.state.lock().unwrap().factors.remove(&factor) {
        Some(_) => StatusCode::ACCEPTED.into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({ "message": "not found" }))).into_response(),
    }
}

async fn revoke(State(w): State<FakeWorkOs>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if !bearer_ok(&h) {
        return unauthorized();
    }
    let mut st = w.state.lock().unwrap();
    st.calls.push("revoke".into());
    st.revoked.insert(b["session_id"].as_str().unwrap_or_default().to_string());
    StatusCode::ACCEPTED.into_response()
}
