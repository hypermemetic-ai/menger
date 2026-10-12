//! SCAFFOLD-3 (S1): a second hub on plexus-login — not menger — with an
//! in-memory implementation of [`LoginAccess`], signing in through a
//! plexus-idp you run.
//!
//! ```sh
//! plexus-idp --port 4590 --http-port 4591 --db /tmp/toy-idp.db --key-dir /tmp/toy-idp-keys \
//!     --audience plexus:toy --registration open &
//! cargo run -p plexus-login --example toy -- ws://127.0.0.1:4590 http://127.0.0.1:4591 4592 4593
//! ```
//!
//! It registers `alice` at plexus-idp (the dev sign-in's `register`, over
//! plexus-idp's socket), serves the toy hub (`auth` only, the gate at the
//! upgrade) and plexus-login's routes, then signs in as a browser would —
//! `POST /login` with the password form, the session cookie back — opens
//! the socket with that cookie and asks `auth.whoami`.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use plexus_core::plexus::{
    AuthContext, AuthSetup, DynamicHub, MemoryMemberships, MemoryRoles, Resolve, ScopeRef, Session, Waiting,
};
use plexus_login::auth_backend::{BrowserLogin, LoginAuth};
use plexus_login::dev_login::DevLogin;
use plexus_login::login::{self, Provider};
use plexus_login::sessions::{self, Gate, Sessions, SESSION_KEY};
use plexus_login::web_login::{self, Login, LoginConfig};
use plexus_login::{Branding, LoginAccess};
use plexus_transport::TransportServer;
use serde_json::{json, Value};

// ── The toy's access rules, in memory ──────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToyRole {
    Member,
    Owner,
}

impl fmt::Display for ToyRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Member => "member",
            Self::Owner => "owner",
        })
    }
}

impl FromStr for ToyRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.trim() {
            "member" => Ok(Self::Member),
            "owner" => Ok(Self::Owner),
            other => Err(format!("unknown role `{other}` — the roles are member, owner")),
        }
    }
}

#[derive(Clone)]
struct ToyAccess(Arc<ToyInner>);

struct ToyInner {
    sessions: Arc<Sessions>,
    grants: Mutex<HashMap<String, Vec<(ToyRole, String)>>>,
    waiting: Mutex<HashMap<String, Waiting>>,
}

fn principal_of(ctx: &AuthContext) -> String {
    ctx.principal().map(|p| p.to_string()).unwrap_or_else(|| ctx.user_id.clone())
}

impl ToyAccess {
    fn new(sessions: Arc<Sessions>) -> Self {
        Self(Arc::new(ToyInner { sessions, grants: Mutex::default(), waiting: Mutex::default() }))
    }

    fn start(&self, ctx: &AuthContext, ttl: i64, ceiling: Vec<String>) -> Session {
        let principal = principal_of(ctx);
        let mut meta = ctx.metadata.clone();
        if let Some(m) = meta.as_object_mut() {
            m.remove(SESSION_KEY);
            m.remove("expires_at");
            m.insert("principal".into(), principal.clone().into());
        }
        let minted = AuthContext::new(ctx.user_id.clone(), "toy".into(), ctx.roles.clone(), meta);
        let (token, expires_at) = self.0.sessions.mint(minted, ttl);
        Session { token, principal, agent: None, name: ctx.get_metadata_string("name"), expires_at, ceiling }
    }
}

#[async_trait]
impl LoginAccess for ToyAccess {
    type Role = ToyRole;

    fn scope_of(text: &str) -> Result<String, String> {
        let t = text.trim();
        if t == "*" || (!t.is_empty() && t.chars().all(|c| c.is_ascii_lowercase())) {
            Ok(t.to_string())
        } else {
            Err(format!("`{t}` is not a scope: `*` or a lowercase word"))
        }
    }

    async fn dev_grant(&self, principal: &str, scope: &str, role: ToyRole) -> Result<(), String> {
        self.0.grants.lock().unwrap().entry(principal.into()).or_default().push((role, scope.into()));
        self.0.waiting.lock().unwrap().remove(principal);
        Ok(())
    }

    async fn dev_set_grants(&self, principal: &str, grants: &[(ToyRole, String)]) -> Result<(), String> {
        self.0.grants.lock().unwrap().insert(principal.into(), grants.to_vec());
        Ok(())
    }

    fn notice(&self, ctx: &AuthContext) {
        let p = principal_of(ctx);
        if ctx.is_authenticated() && !self.0.grants.lock().unwrap().contains_key(&p) {
            let w = Waiting { principal: p.clone(), name: ctx.get_metadata_string("name"), provider: ctx.get_metadata_string("provider"), since: sessions::now() };
            self.0.waiting.lock().unwrap().entry(p).or_insert(w);
        }
    }

    fn alive(&self, ctx: &AuthContext) -> Result<(), String> {
        if ctx.metadata.get("expires_at").and_then(Value::as_i64).is_some_and(|e| e <= sessions::now()) {
            return Err("your sign-in has expired".into());
        }
        match ctx.get_metadata_string(SESSION_KEY) {
            Some(key) if !self.0.sessions.alive(&key) => Err("you have signed out".into()),
            _ => Ok(()),
        }
    }

    fn mint(&self, caller: Option<&AuthContext>, ttl_secs: Option<u64>, ceiling: Option<Vec<String>>) -> Result<Session, String> {
        let ctx = caller.filter(|c| c.is_authenticated()).ok_or("sign in first")?;
        self.alive(ctx)?;
        Ok(self.start(ctx, ttl_secs.unwrap_or(3600) as i64, ceiling.unwrap_or_default()))
    }

    fn mint_for(&self, ctx: AuthContext, ttl_secs: u64) -> Result<Session, String> {
        Ok(self.start(&ctx, ttl_secs as i64, Vec::new()))
    }

    fn ask_access(&self, ctx: &AuthContext) -> Result<Waiting, String> {
        self.notice(ctx);
        self.0.waiting.lock().unwrap().get(&principal_of(ctx)).cloned().ok_or_else(|| "you hold a role already".into())
    }

    fn waiting(&self) -> Vec<Waiting> {
        self.0.waiting.lock().unwrap().values().cloned().collect()
    }

    fn logout(&self, caller: Option<&AuthContext>) -> Result<(), String> {
        let key = caller.and_then(|c| c.get_metadata_string(SESSION_KEY)).ok_or("no session to log out")?;
        self.0.sessions.end_key(&key);
        Ok(())
    }
}

/// Every scope lies in `*`.
struct Flat;

impl Resolve for Flat {
    fn chain(&self, target: &str) -> Option<Vec<ScopeRef>> {
        Some(vec![ScopeRef::parse(target), ScopeRef::Everywhere])
    }
}

// ── The toy ────────────────────────────────────────────────────────────

const BRANDING: Branding = Branding { app: "toy", version: "0.0.1", mark: "t" };

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::new("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [idp_ws, issuer, ws_port, shell_port] = args.as_slice() else {
        anyhow::bail!("usage: toy <plexus-idp ws url> <plexus-idp issuer> <ws port> <shell port>");
    };
    let (ws_port, shell_port): (u16, u16) = (ws_port.parse()?, shell_port.parse()?);
    let public_url = format!("http://127.0.0.1:{shell_port}");

    // The account to sign in as, registered at plexus-idp through the
    // crate's own dev sign-in (`identity.register` over its socket).
    let dev: DevLogin<ToyAccess> =
        DevLogin { provider: "plexus-idp".into(), password: "toy-pass-1".into(), idp_ws: idp_ws.clone(), users: Vec::new() };
    tracing::info!("toy: registering alice at plexus-idp → {:?}", dev.register("alice").await);

    // plexus-login, over the toy's rules.
    let provider: Provider = format!("idp={issuer},plexus:toy").parse().map_err(anyhow::Error::msg)?;
    let providers = vec![provider];
    let sessions = Arc::new(Sessions::named(3600, &sessions::cookie_name_for(&public_url)));
    let access = ToyAccess::new(sessions.clone());
    let backend = Arc::new(LoginAuth::<ToyAccess>::new(
        access.clone(),
        LoginAuth::<ToyAccess>::offered(&providers),
        Some(BrowserLogin { sessions: sessions.clone(), public_url: public_url.clone() }),
    ));
    let roles = Arc::new(MemoryRoles::default());
    let hub = DynamicHub::new("toy").with_auth(AuthSetup::new(backend, roles.clone(), Arc::new(MemoryMemberships::new()), Arc::new(Flat)));
    roles.seed(&hub.declared_roles());
    let hub = Arc::new(hub);
    let carriers = login::boundary(&providers)?;
    let gate = Gate::new(sessions.clone(), carriers).noticing(access.clone());
    tokio::spawn(async move {
        let server = TransportServer::builder(hub, |h| DynamicHub::arc_into_rpc_module(h).map_err(|e| anyhow::anyhow!("{e}")))
            .with_websocket(ws_port)
            .with_session_validator(Arc::new(gate))
            .anonymous_view()
            .credential_cookies(["access_token"])
            .build()
            .await
            .expect("the toy's socket");
        server.serve().await.expect("the toy's socket serves");
    });
    let login = Arc::new(Login::new(
        LoginConfig {
            providers,
            buttons: Vec::new(),
            password: None,
            forgot: None,
            public_url: public_url.clone(),
            also: Vec::new(),
            require: false,
            title: "toy",
            branding: BRANDING,
            email: None,
            dev: None,
            access: Some(access.clone()),
        },
        sessions.clone(),
    )?);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", shell_port)).await?;
    tokio::spawn(async move { axum::serve(listener, web_login::router(login)).await });
    tracing::info!("toy: ws://127.0.0.1:{ws_port} · login page {public_url}/login");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Anonymous first: the socket admits it into the anonymous view.
    let anon = WsClientBuilder::default().build(format!("ws://127.0.0.1:{ws_port}")).await?;
    tracing::info!("toy: anonymous auth.whoami → {}", call(&anon, "auth.whoami").await);

    // Sign in as a browser does: the page, then its password form.
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build()?;
    let page = http.get(format!("{public_url}/login")).send().await?.text().await?;
    tracing::info!(
        "toy: GET /login → title {:?}, password form {}",
        page.split("<title>").nth(1).and_then(|t| t.split("</title>").next()).unwrap_or("?"),
        page.contains(r#"name="password""#)
    );
    let r = http
        .post(format!("{public_url}/login"))
        .header("origin", &public_url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body("username=alice&password=toy-pass-1&next=%2F")
        .send()
        .await?;
    let cookie = r
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|c| c.starts_with(sessions.cookie_name()))
        .and_then(|c| c.split(';').next())
        .map(str::to_string);
    tracing::info!("toy: POST /login → {} {:?}, session cookie {}", r.status(), r.headers().get("location"), cookie.as_deref().map(|c| c.split('=').next().unwrap_or("")).unwrap_or("none"));
    let cookie = cookie.ok_or_else(|| anyhow::anyhow!("no session cookie: the sign-in failed"))?;
    let me: Value = http.get(format!("{public_url}/login/me")).header("cookie", &cookie).send().await?.json().await?;
    tracing::info!("toy: GET /login/me → {me}");

    // The socket, with the session cookie: the gate knows who it is.
    let mut h = HeaderMap::new();
    h.insert("cookie", HeaderValue::from_str(&cookie)?);
    let ws = WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{ws_port}")).await?;
    let who = call(&ws, "auth.whoami").await;
    tracing::info!("toy: signed-in auth.whoami → {who}");
    let ok = who.to_string().contains(me["principal"].as_str().unwrap_or("\u{0}"));
    tracing::info!("toy: whoami names the signed-in principal: {ok}; waiting list: {:?}", access.waiting().iter().map(|w| &w.principal).collect::<Vec<_>>());
    anyhow::ensure!(ok, "auth.whoami did not name the signed-in principal");
    Ok(())
}

async fn call(c: &WsClient, method: &str) -> Value {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", json!({})).unwrap();
    let mut sub: Subscription<Value> = match c.subscribe("toy.call", p, "toy.call_unsub").await {
        Ok(s) => s,
        Err(e) => return json!({ "subscribe_error": e.to_string() }),
    };
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
