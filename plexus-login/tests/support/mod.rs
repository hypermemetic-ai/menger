//! The sign-in tests' harness: an app on plexus-login with an identity
//! backend, a browser with a cookie jar, the fakes, and a log capture.

#![allow(dead_code)]

pub mod fake_workos;

use std::collections::HashMap;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use axum::extract::{Form, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use plexus_core::plexus::DynamicHub;
use plexus_login::backends::memory::{Memory, MemoryOptions};
use plexus_login::backends::plexus_idp::PlexusIdp;
use plexus_login::backends::workos::{ApiKey, WorkOs, WorkOsConfig};
use plexus_login::client_ip::TrustedProxies;
use plexus_login::identity::{IdentityBackend, Social};
use plexus_login::simple::SimpleAccess;
use plexus_login::throttle::Limits;
use plexus_login::web_login::Login;
use plexus_login::{Branding, LoginOptions, Wiring};
use plexus_transport::TransportServer;
use serde_json::{json, Value};

pub use fake_workos::{FakeWorkOs, KEY};

pub const KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
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

pub const JWK: &str = r#"{"kty": "RSA", "use": "sig", "alg": "RS256", "kid": "fake-1", "n": "v4mTNy6do1-Yek-YX_17QfVcNqkshB14LwzIe3Y54TiZwZu4NN--YPsbb9U9jLiYKJrGlOVj5zlKtfIpmBZUD47Lb1klPCsOySmJnZDIY5cjkCRXx_wo9gb6EBNxxOu47lTirMm19BWpgDz4OP3vEQzM-RLylTTZ3x_xKO9a6m9H1-ewQsU3ELHGVDpmsovV48jKt-XVHhhYUxgGyE1a3yPue07gA6V26LZY3iSPBNJJwIM_4cxoLnJdN7ANfVBJPpObR1QWrjXBRv-xx-wtw1LFg8Z0TK02eZtfA7BcJBO7Pk0_6_X3GYd9pLda0OQsFgn-LsmVTiTrDESS1-lGgQ", "e": "AQAB"}"#;

pub const BRANDING: Branding = Branding { app: "toy", version: "0.0.1", mark: "t", motto: "" };

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

pub fn random() -> String {
    plexus_login::sessions::random_token()
}

/// A fresh TOTP secret, base32.
pub fn new_secret() -> String {
    let mut b = [0u8; 20];
    for (i, x) in random().bytes().take(20).enumerate() {
        b[i] = x;
    }
    plexus_login::crypto::base32(&b)
}

pub fn totp_ok(secret_b32: &str, code: &str) -> bool {
    plexus_login::crypto::unbase32(secret_b32).is_some_and(|s| plexus_login::crypto::totp_matches(&s, code, now()))
}

/// The code an authenticator app would show now.
pub fn totp_now(secret_b32: &str) -> String {
    plexus_login::crypto::totp(&plexus_login::crypto::unbase32(secret_b32).unwrap(), now())
}

/// A code that is never the current one.
pub fn wrong_totp(secret_b32: &str) -> String {
    let right = totp_now(secret_b32);
    let n: u32 = right.parse().unwrap();
    format!("{:06}", (n + 500_000) % 1_000_000)
}

// ── the log ─────────────────────────────────────────────────────────────

pub static LOG: Mutex<Vec<u8>> = Mutex::new(Vec::new());

struct LogWriter;

impl Write for LogWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        LOG.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Every test's log goes to [`LOG`]: plexus-login's at trace, every other
/// crate's at debug.
pub fn capture_logs() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter("debug,plexus_login=trace")
            .with_ansi(false)
            .with_writer(|| LogWriter)
            .init();
    });
}

pub fn log_text() -> String {
    String::from_utf8_lossy(&LOG.lock().unwrap()).to_string()
}

// ── a fixture plexus-idp: discovery, JWKS, the password grant ───────────

pub const ALICE: &str = "c952acd3-60aa-4534-b2e3-83634591fee1";

async fn idp_token(State(iss): State<String>, Form(f): Form<HashMap<String, String>>) -> axum::response::Response {
    let ok = match (f.get("grant_type").map(String::as_str), f.get("username").map(String::as_str), f.get("password").map(String::as_str)) {
        (Some("password"), Some("alice"), Some("s3cret")) => true,
        (Some("refresh_token"), _, _) => f.get("refresh_token").is_some_and(|r| r.starts_with("idp_rt_")),
        _ => false,
    };
    if !ok {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_grant" }))).into_response();
    }
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some("fake-1".into());
    let claims = json!({ "iss": iss, "aud": "plexus:toy", "sub": ALICE, "exp": now() + 600, "iat": now() - 5, "username": "alice" });
    let t = encode(&h, &claims, &EncodingKey::from_rsa_pem(KEY_PEM.as_bytes()).unwrap()).unwrap();
    Json(json!({ "access_token": t, "refresh_token": format!("idp_rt_{}", &random()[..10]), "expires_in": 600 })).into_response()
}

pub async fn fixture_idp() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let iss = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let discovery = json!({ "issuer": iss, "jwks_uri": format!("{iss}/jwks.json"), "token_endpoint": format!("{iss}/oauth/token") });
    let jwks = json!({ "keys": [serde_json::from_str::<Value>(JWK).unwrap()] });
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(move || async move { Json(discovery) }))
        .route("/jwks.json", get(move || async move { Json(jwks) }))
        .route("/oauth/token", post(idp_token))
        .with_state(iss.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    iss
}

// ── the backends under test ─────────────────────────────────────────────

/// A backend under test, with what a test needs to reach behind it: the
/// codes it "sent".
#[derive(Clone)]
pub enum Under {
    WorkOs(FakeWorkOs, Arc<dyn IdentityBackend>),
    Memory(Arc<Memory>),
    PlexusIdp(Arc<dyn IdentityBackend>),
}

impl Under {
    pub async fn workos() -> Self {
        Self::workos_with(KEY, Social::ALL[..3].to_vec()).await
    }

    pub async fn workos_with(key: &str, social: Vec<Social>) -> Self {
        let fake = FakeWorkOs::start().await;
        let b = WorkOs::new(WorkOsConfig { client: fake_workos::CLIENT.into(), base: Some(fake.base.clone()), key: ApiKey::new(key).unwrap(), social, sso: true, email_code: true, totp_issuer: "toy".into() }).unwrap();
        Self::WorkOs(fake, Arc::new(b))
    }

    pub fn memory() -> Self {
        Self::Memory(Arc::new(Memory::new(MemoryOptions::default())))
    }

    pub fn memory_mfa() -> Self {
        Self::Memory(Arc::new(Memory::new(MemoryOptions { require_mfa: true, ..Default::default() })))
    }

    pub async fn plexus_idp() -> Self {
        let iss = fixture_idp().await;
        Self::PlexusIdp(Arc::new(PlexusIdp::new(&iss, "plexus:toy").unwrap()))
    }

    pub fn backend(&self) -> Arc<dyn IdentityBackend> {
        match self {
            Self::WorkOs(_, b) => b.clone(),
            Self::Memory(m) => m.clone(),
            Self::PlexusIdp(b) => b.clone(),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::WorkOs(..) => "workos",
            Self::Memory(_) => "memory",
            Self::PlexusIdp(_) => "plexus-idp",
        }
    }

    pub async fn code_for(&self, email: &str) -> String {
        match self {
            Self::WorkOs(f, _) => f.code_for(email).await,
            Self::Memory(m) => {
                for _ in 0..100 {
                    if let Some(c) = m.last_code(email) {
                        return c;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                panic!("no code for {email}");
            }
            Self::PlexusIdp(_) => panic!("plexus-idp sends no codes"),
        }
    }

    pub fn fake(&self) -> &FakeWorkOs {
        match self {
            Self::WorkOs(f, _) => f,
            _ => panic!("not WorkOS"),
        }
    }
}

/// Every backend that offers email codes and TOTP.
pub async fn code_backends() -> Vec<Under> {
    vec![Under::workos().await, Under::memory()]
}

// ── the app ─────────────────────────────────────────────────────────────

pub struct App {
    pub base: String,
    pub ws: u16,
    pub login: Arc<Login<SimpleAccess>>,
    pub wired: Arc<plexus_login::Wired<SimpleAccess>>,
    pub under: Under,
}

pub struct Tweaks {
    pub trusted: Vec<&'static str>,
    pub limits: Limits,
    pub return_origins: Vec<String>,
}

impl Default for Tweaks {
    fn default() -> Self {
        Self { trusted: vec![], limits: Limits::default(), return_origins: vec!["https://app.example".into()] }
    }
}

pub async fn app(under: Under) -> App {
    app_with(under, Tweaks::default()).await
}

pub async fn app_with(under: Under, t: Tweaks) -> App {
    capture_logs();
    let ws = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let shell = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let shell_port = shell.local_addr().unwrap().port();
    let opts = LoginOptions {
        identity: Some(under.backend()),
        trusted_proxies: TrustedProxies::parse(t.trusted).unwrap(),
        limits: t.limits,
        return_origins: t.return_origins,
        allow_development_backends: true,
        ..LoginOptions::new(BRANDING, ws, shell_port)
    };
    let wiring = Wiring::new(opts).unwrap();
    let access = SimpleAccess::new(wiring.sessions(), &[]).unwrap();
    let wired = wiring.finish(access.clone(), None).unwrap();
    let hub = DynamicHub::new("toy").with_auth(access.auth_setup(wired.backend.clone()));
    access.seed_roles(&hub);
    let server = wired.apply(TransportServer::builder(Arc::new(hub), |h| DynamicHub::arc_into_rpc_module(h).map_err(|e| anyhow::anyhow!("{e}"))).with_websocket(ws));
    tokio::spawn(async move { server.build().await.unwrap().serve().await.unwrap() });
    let router = wired.router().expect("a page");
    tokio::spawn(async move { axum::serve(shell, router.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", ws)).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let login = wired.login.clone().unwrap();
    App { base: wired.public_url.clone(), ws, login, wired: Arc::new(wired), under }
}

// ── a browser ───────────────────────────────────────────────────────────

pub struct Resp {
    pub status: u16,
    pub location: Option<String>,
    pub body: String,
    pub headers: reqwest::header::HeaderMap,
}

impl Resp {
    pub fn csrf(&self) -> String {
        between(&self.body, r#"name="csrf" value=""#, "\"").unwrap_or_else(|| panic!("no csrf in {}", self.body))
    }
}

pub fn between(s: &str, a: &str, b: &str) -> Option<String> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(s[i..j].to_string())
}

pub struct Browser {
    pub base: String,
    http: reqwest::Client,
    pub jar: Mutex<HashMap<String, String>>,
    pub xff: Option<String>,
    pub origin: Option<String>,
}

impl Browser {
    pub fn new(app: &App) -> Self {
        Self {
            base: app.base.clone(),
            http: reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).user_agent("TestBrowser/1.0").build().unwrap(),
            jar: Mutex::new(HashMap::new()),
            xff: None,
            origin: Some(app.base.clone()),
        }
    }

    fn cookie_header(&self) -> String {
        self.jar.lock().unwrap().iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
    }

    fn keep(&self, r: &reqwest::Response) {
        for c in r.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok()) {
            let first = c.split(';').next().unwrap();
            let (k, v) = first.split_once('=').unwrap();
            if v.is_empty() || c.contains("Max-Age=0") {
                self.jar.lock().unwrap().remove(k);
            } else {
                self.jar.lock().unwrap().insert(k.to_string(), v.to_string());
            }
        }
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with("http") { path.to_string() } else { format!("{}{path}", self.base) }
    }

    async fn finish(&self, r: reqwest::Response) -> Resp {
        self.keep(&r);
        let status = r.status().as_u16();
        let headers = r.headers().clone();
        let location = headers.get("location").and_then(|v| v.to_str().ok()).map(str::to_string);
        Resp { status, location, body: r.text().await.unwrap_or_default(), headers }
    }

    pub async fn get(&self, path: &str) -> Resp {
        let mut req = self.http.get(self.url(path));
        if path.starts_with('/') || path.starts_with(&self.base) {
            req = req.header("cookie", self.cookie_header());
            if let Some(x) = &self.xff {
                req = req.header("x-forwarded-for", x);
            }
        }
        let r = req.send().await.unwrap();
        self.finish(r).await
    }

    pub async fn post(&self, path: &str, form: &[(&str, &str)]) -> Resp {
        let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
        let mut req = self.http.post(self.url(path)).header("content-type", "application/x-www-form-urlencoded").header("cookie", self.cookie_header()).body(body);
        if let Some(o) = &self.origin {
            req = req.header("origin", o);
        }
        if let Some(x) = &self.xff {
            req = req.header("x-forwarded-for", x);
        }
        let r = req.send().await.unwrap();
        self.finish(r).await
    }

    /// Who `/login/me` says this browser is.
    pub async fn me(&self) -> Option<Value> {
        let r = self.get("/login/me").await;
        (r.status == 200).then(|| serde_json::from_str(&r.body).unwrap())
    }

    /// The sign-in page's CSRF token (planting the flow cookie).
    pub async fn csrf(&self) -> String {
        self.get("/login").await.csrf()
    }

    /// Presses a social button (the page's CSRF'd form): the answer.
    pub async fn social(&self, provider: &str, next: &str) -> Resp {
        let csrf = match self.get("/login").await {
            r if r.status == 200 => r.csrf(),
            _ => panic!("signed in already"),
        };
        self.post("/login/social", &[("csrf", &csrf), ("provider", provider), ("next", next)]).await
    }

    /// Signs in with an emailed code; the answer to the code.
    pub async fn sign_in_by_email(&self, under: &Under, email: &str, next: &str) -> Resp {
        let csrf = self.csrf().await;
        let r = self.post("/login/email", &[("csrf", &csrf), ("email", email), ("next", next)]).await;
        assert_eq!(r.status, 200, "{}", r.body);
        let code = under.code_for(email).await;
        self.post("/login/code", &[("csrf", &r.csrf()), ("code", &code)]).await
    }

    /// Follows a redirect chain while it stays on hosts the test runs (the
    /// app and the fakes); the last answer.
    pub async fn follow(&self, mut r: Resp) -> Resp {
        for _ in 0..6 {
            match r.location.clone() {
                Some(l) if (300..400).contains(&r.status) && l.starts_with("http://127.0.0.1") => r = self.get(&l).await,
                Some(l) if (300..400).contains(&r.status) && l.starts_with('/') && !l.starts_with("//") && l.starts_with("/login") => r = self.get(&l).await,
                _ => return r,
            }
        }
        r
    }
}
