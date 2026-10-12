//! SCAFFOLD-3 (S1), SCAFFOLD-9 (B5): a second hub on plexus-login — not
//! menger — with the crate's ready-made rules ([`SimpleAccess`]), signing in
//! through a plexus-idp you run.
//!
//! ```sh
//! plexus-idp --port 4590 --http-port 4591 --db ~/.cache/toy-idp.db --key-dir ~/.cache/toy-idp-keys \
//!     --audience plexus:toy --registration open &
//! cargo run -p plexus-login --example toy -- ws://127.0.0.1:4590 http://127.0.0.1:4591 4592 4593
//! ```
//!
//! It registers `alice` at plexus-idp (the dev sign-in's `register`, over
//! plexus-idp's socket), serves the toy hub (`auth` only, the gate at the
//! upgrade) and plexus-login's routes, then signs in as a browser would —
//! `POST /login` with the password form, the session cookie back — opens
//! the socket with that cookie and asks `auth.whoami`. The same flow runs
//! in `cargo test` against a fixture issuer: tests/simple_login.rs.

use std::sync::Arc;

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use plexus_core::plexus::DynamicHub;
use plexus_login::dev_login::DevLogin;
use plexus_login::simple::SimpleAccess;
use plexus_login::{Branding, LoginAccess, LoginOptions, Wiring};
use plexus_transport::TransportServer;
use serde_json::{json, Value};

const BRANDING: Branding = Branding { app: "toy", version: "0.0.1", mark: "t", motto: "" };

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::new("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [idp_ws, issuer, ws_port, shell_port] = args.as_slice() else {
        anyhow::bail!("usage: toy <plexus-idp ws url> <plexus-idp issuer> <ws port> <shell port>");
    };
    let (ws_port, shell_port): (u16, u16) = (ws_port.parse()?, shell_port.parse()?);

    // The account to sign in as, registered at plexus-idp through the
    // crate's own dev sign-in (`identity.register` over its socket).
    let dev: DevLogin<SimpleAccess> =
        DevLogin { provider: "plexus-idp".into(), password: "toy-pass-1".into(), idp_ws: idp_ws.clone(), users: Vec::new() };
    tracing::info!("toy: registering alice at plexus-idp → {:?}", dev.register("alice").await);

    // ── plexus-login, the whole of it ──
    let opts = LoginOptions::new(BRANDING, ws_port, shell_port).with_providers([format!("idp={issuer},plexus:toy")])?;
    let wiring = Wiring::new(opts)?;
    let access = SimpleAccess::new(wiring.sessions(), &[])?;
    let wired = wiring.finish(access.clone(), None)?;
    let hub = DynamicHub::new("toy").with_auth(access.auth_setup(wired.backend.clone()));
    access.seed_roles(&hub);
    let server = wired.apply(TransportServer::builder(Arc::new(hub), |h| DynamicHub::arc_into_rpc_module(h).map_err(|e| anyhow::anyhow!("{e}"))).with_websocket(ws_port));
    tokio::spawn(async move { server.build().await.expect("the toy's socket").serve().await.expect("the toy's socket serves") });
    let (public_url, sessions) = (wired.public_url.clone(), wired.sessions.clone());
    let router = wired.router().expect("a provider and a shell: a page");
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", shell_port)).await?;
    tokio::spawn(async move { axum::serve(listener, router).await });
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
