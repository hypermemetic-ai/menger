//! Day-one web shell, plus (MFE-2) the Leptos app and its data plane.
//!
//! menger's rule (since 0.04): frontend and backend are online from the
//! first commit, so every noun is visible the day it is written. Three
//! surfaces share this one listener, and nothing else is served on it
//! (ACCESS-33):
//!
//! - `/` — **the Leptos app** (MFE-4). It is the frontend; a browser
//!   pointed at the server gets it without being told a path. When there
//!   is no build output the root *degrades to the vanilla page* rather
//!   than to an error — the day-one property is that the binary alone
//!   always serves something live, and that is what keeps it true.
//! - `/vanilla` — the day-one page, embedded at compile time, speaking
//!   the Plexus wire protocol over WebSocket. Unchanged since day one
//!   and fully functional; it moved off `/` with MFE-4 and stays until an
//!   epic retires it.
//! - `/app/` — the Leptos CSR app (MFE-2), its build in `frontend/dist`
//!   read into a table (ACCESS-33): a request path is a key into it, never
//!   a path on disk. Still the app's own root: it is where its assets
//!   live (`Trunk.toml`'s `public_url`, so a built `index.html` names
//!   `/app/…` whichever URL served it) and it is the deep-link prefix
//!   MFE-1 pinned (`/app/#/ticket/{id}`), which the vanilla page's rows
//!   still link to. The frontend is deliberately not part of this
//!   crate's build: assets are an input to *serving*, so `cargo build`
//!   and `cargo test` never need the wasm toolchain. When the output is
//!   missing **this** URL answers with a plain "frontend not built" page
//!   (HTTP 200 — a deliberate state with build instructions, not an
//!   error), never a crash or a 500; the root is the one that degrades
//!   to something usable instead.
//!
//! DIST-3: where the table comes from is an [`AppSource`]. `Dir` is the
//! above, unchanged — dist on disk, read again when its entrypoint's
//! modification time changes. `Embedded` is the build compiled into the
//! binary by `build.rs` under the `embedded-app` feature (off by default),
//! for a menger with no checkout beside it (a published crate, a desktop
//! host): the same table — the entrypoint stamped with `{{WS_PORT}}` and
//! `{{TITLE}}`, the same MIME types, a request path a key and never a path —
//! read once, since a binary's build never changes. The binary chooses:
//! `--app-dist` given → that directory; else, with the build embedded →
//! `Embedded`; else → [`default_app_dist`] ([`preferred_app`]).
//!
//! TICKETS-36: a path no route claims that names a ticket goes to its route
//! in the app — the link to share, `/t/<uid>/<ID>-<slug>`, to
//! `/app/#/t/<uid>/…`; a ticket's name at the root (`/PERF-19`,
//! `/hypermemetic:PLEXUS/PERF-19`) to `/app/#/ticket/<name>`, which the app
//! resolves (or lists, when it fits several) — `302`, behind the session as
//! `/app/` is ([`ticket_redirect`]). Every other path is `404` as before.
//!
//! SCAFFOLD-5: the table, the stamping, the app's routes, the hardening
//! (IDENTITY-38) and [`AppSource`] are `plexus-shell`'s. This module is
//! menger's part: the `/vanilla` page (and `/` falling back to it), the
//! favicon, the tab's title, the "not built" page's words, the embedded
//! table, the pages' policy and the frame document (ACCESS-28), and the
//! login routes and checks layered around the crate's router.
//!
//! ACCESS-28: **every page goes out under a policy.** The app's pages
//! (`/`, `/app/`, `/vanilla`, "not built") under [`PAGE_CSP`] — scripts
//! only the app's own files, the wasm and the page's hashed inline blocks;
//! connections only to itself and its service; images only its own; nothing
//! frames it. The sign-in pages under plexus-login's own (their blocks
//! hashed, AUTH-22). A ticket body's frame is not a srcdoc any more — a
//! srcdoc inherits its parent's policy, and a body's inline scripts and
//! esm.sh imports are the product — but [`FRAME`], served at
//! [`FRAME_PATH`] under [`FRAME_CSP`], which writes the body's document
//! over itself (frontend/src/frame.rs).

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use plexus_shell::EmbeddedTable;
use std::path::PathBuf;
use std::sync::Arc;

/// DIST-3: where the Leptos app's build is served from — `plexus-shell`'s.
pub use plexus_shell::AppSource;

pub struct ShellConfig {
    pub port: u16,
    pub ws_port: u16,
    /// DIST-3: where the Leptos app's build output is read from.
    pub app: AppSource,
    /// MFE-23: the environment's name (`MENGER_ENV`), which titles the
    /// browser tab ([`tab_title`]).
    pub env: Option<String>,
    /// IDENTITY-19: the login page and its routes, when providers are
    /// configured; with `require`, the app's routes need a session.
    pub login: Option<Arc<crate::web_login::Login>>,
    /// ACCESS-25: the `Host` values the shell answers to, login or not
    /// ([`crate::local::listener_hosts`]); with login, the hosts its
    /// origins name are added. Empty: `127.0.0.1:<port>`, `localhost:<port>`.
    pub allowed_hosts: Vec<String>,
    /// ACCESS-30: the shell is reached over https (`--public-url https://…`,
    /// a proxy terminating TLS in front): every answer carries
    /// [`HSTS`], so a browser that has been here once never asks over http
    /// again.
    pub https: bool,
    /// RUN-9: RFC 9728's document on the public origin
    /// ([`plexus_login::Wired::metadata_route`]) — behind an https proxy
    /// only `/ws` reaches the socket's port, so the metadata its 401s name
    /// is answered here. Public: never behind a session.
    pub metadata: Option<Router>,
    /// APP-110: who may frame this shell's pages, and whose pages it frames
    /// ([`Framing`]) — nobody and nothing by default.
    pub framing: Framing,
    /// TRANSPORT-15: the MCP endpoint's router — `/mcp` and, under OAuth,
    /// its RFC 9728 document — on the shell's own origin, so the edge needs
    /// no route of its own. Public, like the metadata: the bridge's identify
    /// step names the caller from the request's Bearer and refuses with a
    /// 401 of its own; the session check never sends it to the login page.
    pub mcp: Option<Router>,
}

/// ACCESS-30: Strict-Transport-Security, a year, on every answer of a shell
/// reached over https. The host alone (no `includeSubDomains`): menger's
/// hostname is its own, and says nothing for its parent's other names.
pub const HSTS: &str = "max-age=31536000";

/// MFE-23: the tab's title — `menger.test` on the test environment,
/// `menger` everywhere else (prod, a dev server, none named).
pub fn tab_title(env: Option<&str>) -> &'static str {
    match env {
        Some("test") => "menger.test",
        _ => "menger",
    }
}

/// MFE-23: the tab's icon, a Menger-sponge face (the Sierpinski carpet,
/// level 2), embedded so the binary alone always has it.
const FAVICON: &str = include_str!("../web/favicon.svg");

/// The day-one page, served at `/vanilla` and at `/` when there is no app
/// build (MFE-4); stamped as the app's entrypoint is (MFE-9, MFE-23).
const VANILLA: &str = include_str!("../web/index.html");

/// APP-64: the app build's files served at the root too, public — what
/// Add to Home Screen and an installed app ask the origin for (iOS takes
/// no SVG icon; frontend/icons/, scripts/icons.sh). No data in any of them,
/// and a manifest is fetched without credentials, so none is behind a
/// session.
///
/// APP-92: and the service worker (`sw.js`, written by the app's build,
/// frontend/boot/split.mjs) — at the root, so its scope is the whole
/// origin, and public: the sign-in page registers it.
pub const ROOT_FILES: [&str; 6] =
    ["apple-touch-icon.png", "icon-192.png", "icon-512.png", "icon-512-maskable.png", "manifest.webmanifest", "sw.js"];

/// APP-92: where the app's service worker is served ([`ROOT_FILES`]).
pub const SERVICE_WORKER: &str = "/sw.js";

/// ACCESS-28: the app's pages' Content-Security-Policy (plexus-shell fills
/// `{{SCRIPTS}}` with the page's inline-script hashes and `{{HUB}}` with the
/// service's WebSocket origins at the name the page was asked for):
///
/// - scripts: the app's files, the wasm, the hashed inline blocks — no
///   other inline script, no other origin;
/// - styles `'unsafe-inline'` (the app writes `style` and `<style>`: the
///   theme blocks) and `https:` with fonts — a theme may `@import` a face's
///   sheet (THEME-3: `@import url("https://…")`);
/// - images: its own and `data:` — a remote image in a body shown in the
///   app is refused, so a body cannot learn who reads it (ACCESS-4 M5);
/// - connections: itself, its service, and a WebSocket on loopback — MFE-21's
///   settings point the app at another menger on this machine;
/// - frames: its own (the body frames, [`FRAME_PATH`]); nothing frames it.
///
/// No `form-action`: the sign-out form's answer may go on to a provider's
/// own sign-out page (IDENTITY-30), which the browser checks against it.
pub const PAGE_CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval' {{SCRIPTS}}; \
style-src 'self' 'unsafe-inline' https:; font-src 'self' data: https:; img-src 'self' data: blob:; \
connect-src 'self' {{HUB}} ws://127.0.0.1:* ws://localhost:*; frame-src 'self'; \
object-src 'none'; base-uri 'self'; frame-ancestors 'none'";

/// APP-110: **the live test app in a ticket.** The tracker draws test's app
/// in device frames in a ticket body, so two policies move, each by one
/// origin the operator names and nothing else:
///
/// - the tracker's `frame-src` gains `--live-origin` (the test app's origin);
/// - test's `frame-ancestors` gains each `--frame-ancestor` (the tracker's
///   origin) in place of `'none'`.
///
/// Both default to nothing: a server that is told neither is as it was —
/// frames only its own body frames, and is framed by no one. The sign-in
/// pages keep `frame-ancestors 'none'` (plexus-login), so a reader with no
/// session on test signs in in a tab, not in the frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Framing {
    /// Origins that may frame this server's pages (`--frame-ancestor`).
    pub ancestors: Vec<String>,
    /// The origin of the test app this server's bodies frame (`--live-origin`).
    pub live: Option<String>,
}

/// APP-110: `s` as an origin — `http` or `https`, a host, an optional port,
/// no path, query, fragment or credentials — written canonically
/// (`https://app.menger.sh`). Nothing that could end a CSP source or start
/// another directive gets through.
pub fn check_origin(s: &str) -> Result<String, String> {
    let bad = |why: &str| Err(format!("{s:?} is not an origin ({why}): write scheme://host[:port], e.g. https://app.menger.sh"));
    let Ok(u) = url::Url::parse(s.trim()) else { return bad("not a URL") };
    if !matches!(u.scheme(), "http" | "https") {
        return bad("http or https only");
    }
    if u.host_str().is_none_or(str::is_empty) || !u.username().is_empty() || u.password().is_some() {
        return bad("a host, no credentials");
    }
    if u.path() != "/" || u.query().is_some() || u.fragment().is_some() {
        return bad("no path, query or fragment");
    }
    let origin = u.origin().ascii_serialization();
    if origin.contains(['*', ' ', ';', ',', '\\']) {
        return bad("no wildcard");
    }
    Ok(origin)
}

/// APP-110: [`PAGE_CSP`] under `framing`.
pub fn page_csp(framing: &Framing) -> String {
    let mut csp = PAGE_CSP.to_string();
    if let Some(live) = &framing.live {
        csp = csp.replace("frame-src 'self'", &format!("frame-src 'self' {live}"));
    }
    if !framing.ancestors.is_empty() {
        csp = csp.replace("frame-ancestors 'none'", &format!("frame-ancestors {}", framing.ancestors.join(" ")));
    }
    csp
}

/// ACCESS-28: where a ticket body's frame starts ([`FRAME`]).
pub const FRAME_PATH: &str = "/app/frame";

/// ACCESS-28: the frame document — it asks the host for its document and
/// writes it over itself (its header is the protocol's reference).
const FRAME: &str = include_str!("../web/frame.html");

/// ACCESS-28: the frame document's policy — IDENTITY-28's (no connection,
/// no form; scripts, styles, images and fonts left alone: bodies import
/// modules from esm.sh and show images from anywhere), and framed only by
/// the app.
pub const FRAME_CSP: &str = "connect-src 'none'; form-action 'none'; frame-ancestors 'self'";

/// The degraded answer when `frontend/dist` holds no build. 200 on
/// purpose: "not built yet" is a documented state of a fresh checkout,
/// and the page itself is the clear indication MFE-2 requires.
const NOT_BUILT: &str = concat!(
    "<!doctype html><meta charset=\"utf-8\"><title>menger — frontend not built</title>",
    "<body style=\"font: 14px/1.6 monospace; padding: 2rem; max-width: 46rem\">",
    "<h1 style=\"font-size:1.1rem\">frontend not built</h1>",
    "<p>The Leptos app's build output is missing. Build it once:</p>",
    "<pre>cd frontend && trunk build --release</pre>",
    "<p>then reload. Toolchain setup is in README.md → Frontend. ",
    "The server itself is fine — the vanilla shell is at ",
    "<a href=\"/vanilla\">/vanilla</a>, and <a href=\"/\">/</a> serves it ",
    "until this build exists.</p>",
);

/// Where `trunk build` puts the app when run from a checkout. Overridable
/// via `--app-dist` / `MENGER_APP_DIST` for deployments that move dist.
pub fn default_app_dist() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend/dist")
}

/// DIST-3: the app's build as build.rs found it (plexus-shell's embedding)
/// — empty unless the `embedded-app` feature is on and frontend/dist held
/// a build.
static EMBEDDED_APP: EmbeddedTable = plexus_shell::embedded_app!();

/// Whether this binary carries the app's build: the `embedded-app`
/// feature was on and frontend/dist held a build when it compiled.
pub fn embedded_available() -> bool {
    plexus_shell::has_entrypoint(EMBEDDED_APP)
}

/// The source when none is named: the embedded build when there is one,
/// else the checkout's `frontend/dist` ([`default_app_dist`]).
pub fn preferred_app() -> AppSource {
    AppSource::embedded_or(EMBEDDED_APP, default_app_dist())
}

/// menger's shell as plexus-shell serves it: the app under `/app/` (where
/// `Trunk.toml`'s `public_url` puts its assets, and MFE-1's deep links),
/// titled for the environment (MFE-23), `/vanilla`, and `/` falling back
/// to it when there is no build (MFE-4). No other routes: the favicon,
/// the frame document and the login routes are added around it
/// ([`favicon_routes`], [`frame_routes`], [`serve_on`]).
pub fn app_shell(env: Option<&str>, app: AppSource) -> plexus_shell::ShellConfig {
    app_shell_framed(env, app, &Framing::default())
}

/// [`app_shell`] under APP-110's [`Framing`]: the policy as [`page_csp`] says,
/// and the app told which test app its bodies may draw (`{{LIVE_ORIGIN}}`,
/// empty when none).
pub fn app_shell_framed(env: Option<&str>, app: AppSource, framing: &Framing) -> plexus_shell::ShellConfig {
    let mut cfg = plexus_shell::ShellConfig::new(tab_title(env), app);
    cfg.stamps.push(("{{LIVE_ORIGIN}}".into(), framing.live.clone().unwrap_or_default()));
    cfg.pages.push(("/vanilla".into(), VANILLA.into()));
    cfg.root_fallback = Some(VANILLA.into());
    cfg.not_built = Some(NOT_BUILT.into());
    cfg.csp = page_csp(framing);
    cfg.root_files = ROOT_FILES.iter().map(|f| f.to_string()).collect();
    cfg
}

/// ACCESS-28: the frame document, public — it holds no data, and the
/// frame's first navigation is the app's.
pub fn frame_routes() -> Router {
    Router::new().route(FRAME_PATH, get(frame))
}

/// MFE-23: the tab's icon, for both pages — public, never behind a session.
pub fn favicon_routes() -> Router {
    Router::new().route("/favicon.svg", get(favicon))
}

pub async fn serve(cfg: ShellConfig) -> anyhow::Result<()> {
    serve_at(std::net::Ipv4Addr::LOCALHOST.into(), cfg).await
}

/// RUN-8: [`serve`] on `bind` (`--bind`) rather than loopback. The names it
/// answers to are still `cfg.allowed_hosts`' (ACCESS-25).
pub async fn serve_at(bind: std::net::IpAddr, cfg: ShellConfig) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind((bind, cfg.port))
        .await
        .map_err(|e| anyhow::anyhow!("cannot bind the shell to {}: {e}", std::net::SocketAddr::new(bind, cfg.port)))?;
    serve_on(listener, cfg, std::future::pending()).await
}

/// DIST-3: [`serve`] on a listener the caller bound, until `shutdown`
/// resolves. `cfg.port` is the listener's port.
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    cfg: ShellConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    // RUN-8: the startup log names the address the shell is bound to.
    let at = listener.local_addr()?;
    let app = router(cfg, at).await;
    // IDENTITY-38: plexus-shell's `serve_on` hardens every answer.
    plexus_shell::serve_on(listener, app, shutdown).await?;
    Ok(())
}

async fn router(cfg: ShellConfig, at: std::net::SocketAddr) -> Router {
    tracing::info!("App:       http://{at}/");
    tracing::info!("Vanilla:   http://{at}/vanilla");
    // ACCESS-33: menger's own pages and the app's build — nothing else. The
    // `/rest/{*}` pass-through to the REST port went: the app has spoken
    // the WebSocket since MFE-9, and nothing else called it. IDENTITY-19:
    // behind a session when login is required — no anonymous view.
    let mut app = plexus_shell::router(app_shell_framed(cfg.env.as_deref(), cfg.app, &cfg.framing), cfg.ws_port).await;
    if let Some(login) = cfg.login.as_ref().filter(|l| l.require) {
        app = app.route_layer(axum::middleware::from_fn_with_state(login.clone(), require_session));
    }
    let mut app = app.merge(favicon_routes()).merge(frame_routes());
    if let Some(metadata) = cfg.metadata {
        app = app.merge(metadata);
    }
    // TRANSPORT-15: `/mcp` beside the metadata — after the session check,
    // which must not redirect an MCP client to the login page.
    if let Some(mcp) = cfg.mcp {
        app = app.merge(mcp);
    }
    // DESIGN-40: a framed body's document has an opaque origin, so the
    // house faces it loads from `/app/fonts/` are cross-origin fetches —
    // answered with CORS (`*`: public files, no data).
    app = app.layer(axum::middleware::from_fn(fonts_cors));
    if let Some(login) = cfg.login.clone() {
        use crate::web_login as w;
        app = app.merge(
            Router::new()
                .route("/login", get(w::page).post(w::password))
                .route("/login/start", get(w::start))
                .route("/login/callback", get(w::callback))
                .route("/login/nostr", axum::routing::post(w::nostr))
                .route("/login/nostr/trace", axum::routing::post(w::nostr_trace))
                .route("/login/me", get(w::me))
                .route("/login/dev", axum::routing::post(w::dev))
                .route("/logout", axum::routing::post(w::logout))
                // AUTH-7: a loopback sign-in's way back to its client.
                .route("/auth/cli", get(w::cli).post(w::cli_confirm))
                .with_state(login.clone())
                // AUTH-22: our own sign-in page's steps, with an identity backend.
                .merge(login.signin_routes()),
        );
        tracing::info!("Login:     http://{at}/login");
    }
    // TICKETS-36: a path no route claims that names a ticket — `/t/<uid>/…`,
    // `/PERF-19` — goes to its route in the app; any other is 404 as before.
    let fallback_login = cfg.login.clone();
    app = app.fallback(move |req: Request| ticket_or_not_found(fallback_login.clone(), req));
    // DNS rebinding: a page on another name resolving here gets nothing —
    // login or not (ACCESS-25), and before anything routes.
    let hosts = Arc::new(allowed_hosts(cfg.port, &cfg.allowed_hosts, cfg.login.as_deref()));
    let app = app.layer(axum::middleware::from_fn_with_state(hosts, host_is_ours));
    // ACCESS-30: over https, every answer — a refusal too — says to stay there.
    if cfg.https {
        app.layer(axum::middleware::from_fn(hsts))
    } else {
        app
    }
}

/// ACCESS-30: [`HSTS`] on the answer.
async fn hsts(req: Request, next: axum::middleware::Next) -> Response {
    let mut r = next.run(req).await;
    r.headers_mut().insert(header::STRICT_TRANSPORT_SECURITY, header::HeaderValue::from_static(HSTS));
    r
}

/// IDENTITY-19: the app's routes without a session go to the login page;
/// the REST proxy answers 401.
async fn require_session(
    State(login): State<Arc<crate::web_login::Login>>,
    req: Request,
    next: axum::middleware::Next,
) -> Response {
    // IDENTITY-38: the vendored libraries (`/app/vendor/…`: mermaid) are
    // public code, no data — and a ticket body's frame loads them from an
    // opaque origin, which sends no SameSite cookie, so behind the session
    // they were a redirect to the login page and no diagram drew.
    // DESIGN-40: so are the faces (`/app/fonts/…`), for the same frames.
    if login.session(req.headers()).is_some() || is_public_asset(req.uri().path()) {
        return next.run(req).await;
    }
    let path = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/");
    if path.starts_with("/rest/") {
        return (StatusCode::UNAUTHORIZED, "sign in first").into_response();
    }
    to_login(&req)
}

/// IDENTITY-19: the sign-in page, with the path asked for as `next`.
fn to_login(req: &Request) -> Response {
    let path = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let to = format!("/login?next={}", url::form_urlencoded::byte_serialize(path.as_bytes()).collect::<String>());
    (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response()
}

/// IDENTITY-38 / DESIGN-40: the app's files a ticket body's frame loads
/// from its opaque origin — public code and faces, no data. APP-64: and the
/// home-screen icons and manifest, at the root and under `/app/`. APP-92:
/// and the app's code itself — its script, wasm and stylesheet, every file
/// of the build but a page — so it downloads behind the sign-in page; the
/// entrypoint stays behind the session.
fn is_public_asset(path: &str) -> bool {
    let root_file = |p: &str| ROOT_FILES.contains(&p);
    path.starts_with("/app/vendor/")
        || is_font(path)
        || path.strip_prefix("/app/").or(path.strip_prefix('/')).is_some_and(root_file)
        || is_app_code(path)
}

/// APP-92: a file of the app's build that is code, not a page — under
/// `/app/`, named with an extension, and not HTML.
fn is_app_code(path: &str) -> bool {
    let Some(file) = path.strip_prefix("/app/") else { return false };
    let name = file.rsplit('/').next().unwrap_or("");
    !file.contains("..") && name.contains('.') && !name.ends_with(".html") && !name.ends_with(".htm")
}

/// DESIGN-40: the house faces, served from the app's build.
fn is_font(path: &str) -> bool {
    path.starts_with("/app/fonts/")
}

/// DESIGN-40: a face is fetched with CORS (a font always is), and a framed
/// body's document has an opaque origin: `/app/fonts/…` says any origin may
/// read it.
async fn fonts_cors(req: Request, next: axum::middleware::Next) -> Response {
    let font = is_font(req.uri().path());
    let mut r = next.run(req).await;
    if font {
        r.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, header::HeaderValue::from_static("*"));
    }
    r
}

/// The `Host` values this shell answers to: its port on 127.0.0.1 and
/// localhost, the configured ones (ACCESS-25: the public URL's host and
/// `--allowed-host`), and with login the hosts its origins name.
fn allowed_hosts(port: u16, configured: &[String], login: Option<&crate::web_login::Login>) -> Vec<String> {
    let mut hosts = plexus_transport::host::loopback_hosts(port);
    for h in configured.iter().cloned().chain(login.map(|l| l.hosts()).unwrap_or_default()) {
        let h = h.to_ascii_lowercase();
        if !hosts.contains(&h) {
            hosts.push(h);
        }
    }
    hosts
}

/// IDENTITY-24 #1: a request naming another host (DNS rebinding) is refused.
async fn host_is_ours(State(hosts): State<Arc<Vec<String>>>, req: Request, next: axum::middleware::Next) -> Response {
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).map(str::to_ascii_lowercase);
    match host {
        Some(h) if hosts.iter().any(|a| *a == h) => next.run(req).await,
        _ => (StatusCode::MISDIRECTED_REQUEST, plexus_transport::host::REFUSAL).into_response(),
    }
}

/// TICKETS-36: where a path that names a ticket goes in the app — `None`
/// for any other path, which is answered as it was.
///
/// - `/t/<uid>[/…]` — the shareable link (Copy link): the app's route by
///   uid, `/app/#/t/<uid>/…`, the tail kept as written (the app reads the
///   uid alone and rewrites a stale tail);
/// - `/<name>` — a ticket's id (`/PERF-19`), a short form of its address
///   (`/PLEXUS/PERF-19`) or the whole of it (`/hypermemetic:PLEXUS/PERF-19`):
///   `/app/#/ticket/<name>`, which the app resolves — to the ticket's own
///   route, or to the collision page when the name fits several.
///
/// The address is read as the API reads a ticket's name (`TicketRef`), so a
/// shortcode (`/PERF`), a bare word (`/ws`, `/robots.txt`) or a bare uid
/// names no ticket here. Nothing is looked up: whether the ticket exists,
/// and whether the reader may see it, is the app's `get` to say.
pub fn ticket_redirect(path: &str) -> Option<String> {
    use menger_tickets_api::vocab::Uid;
    if let Some(rest) = path.strip_prefix("/t/") {
        let (uid, tail) = rest.split_once('/').unwrap_or((rest, ""));
        let uid: Uid = uid.parse().ok()?;
        let tail = tail.trim_end_matches('/');
        let safe = |c: char| c.is_ascii_alphanumeric() || "-._~".contains(c);
        return Some(match tail.is_empty() || !tail.chars().all(safe) {
            true => format!("/app/#/t/{uid}"),
            false => format!("/app/#/t/{uid}/{tail}"),
        });
    }
    let raw = path.strip_prefix('/')?;
    let raw = raw.strip_suffix('/').unwrap_or(raw);
    let name = unescaped(raw)?;
    let ok = |c: char| c.is_ascii_alphanumeric() || "-.:/_".contains(c);
    if name.is_empty() || name.len() > 512 || !name.chars().all(ok) || name.contains("//") || name.starts_with('/') {
        return None;
    }
    if menger_tickets_api::vocab::uid_named(&name).is_some() {
        return None;
    }
    let address = menger_tickets_api::Address::parse(&name).ok().filter(|a| a.names_ticket())?;
    Some(format!("/app/#/ticket/{address}"))
}

/// A path's `%XX` escapes read (`hypermemetic%3APLEXUS` is
/// `hypermemetic:PLEXUS`); `None` for a broken escape or text that is not
/// UTF-8.
fn unescaped(raw: &str) -> Option<String> {
    let (bytes, mut out, mut i) = (raw.as_bytes(), Vec::with_capacity(raw.len()), 0);
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = raw.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// TICKETS-36: the shell's answer to a path no route claims — the app's
/// route for a ticket the path names ([`ticket_redirect`], `302`), behind the
/// sign-in as `/app/` is; any other, as before: `404`, and nothing more.
async fn ticket_or_not_found(login: Option<Arc<crate::web_login::Login>>, req: Request) -> Response {
    let to = matches!(*req.method(), axum::http::Method::GET | axum::http::Method::HEAD)
        .then(|| ticket_redirect(req.uri().path()))
        .flatten();
    let Some(to) = to else { return StatusCode::NOT_FOUND.into_response() };
    if let Some(login) = login.filter(|l| l.require) {
        if login.session(req.headers()).is_none() {
            return to_login(&req);
        }
    }
    (StatusCode::FOUND, [(header::LOCATION, to)]).into_response()
}

/// ACCESS-28: the frame document, under its own policy: framed by the app
/// alone. Small and fixed per release: an hour in the cache.
async fn frame() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CONTENT_SECURITY_POLICY, FRAME_CSP),
            (header::X_FRAME_OPTIONS, "SAMEORIGIN"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        FRAME,
    )
        .into_response()
}

/// MFE-23: the tab's icon, for both pages.
async fn favicon() -> Response {
    (
        [(header::CONTENT_TYPE, "image/svg+xml"), (header::CACHE_CONTROL, "public, max-age=86400")],
        FAVICON,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TICKETS-36: a link by uid, `/t/<uid>[/…]`, goes to the app's route
    /// by uid, its tail kept; a path that names a ticket — its id, a short
    /// form, its full address — goes to `#/ticket/<name>`, which the app
    /// resolves (or lists, when it fits several). Every other path is
    /// answered as it was: no redirect.
    #[test]
    fn a_path_that_names_a_ticket_goes_to_the_app() {
        let to = |p: &str| ticket_redirect(p);
        assert_eq!(to("/t/7k3mq2xa"), Some("/app/#/t/7k3mq2xa".into()));
        assert_eq!(to("/t/7k3mq2xa/"), Some("/app/#/t/7k3mq2xa".into()));
        assert_eq!(to("/t/7k3mq2xa/PERF-19-compression-on-the-websocket"), Some("/app/#/t/7k3mq2xa/PERF-19-compression-on-the-websocket".into()));
        assert_eq!(to("/t/7K3MQ2XA/x"), Some("/app/#/t/7k3mq2xa/x".into()), "a uid reads case-blind");
        assert_eq!(to("/PERF-19"), Some("/app/#/ticket/PERF-19".into()));
        assert_eq!(to("/perf-19"), Some("/app/#/ticket/PERF-19".into()), "folded as the store spells it");
        assert_eq!(to("/PLEXUS/PERF-19"), Some("/app/#/ticket/PLEXUS/PERF-19".into()));
        assert_eq!(to("/hypermemetic:PLEXUS/PERF-19"), Some("/app/#/ticket/hypermemetic:PLEXUS/PERF-19".into()));
        assert_eq!(to("/hypermemetic%3APLEXUS/PERF-19"), Some("/app/#/ticket/hypermemetic:PLEXUS/PERF-19".into()), "an escaped colon");
        assert_eq!(to("/SYNAPSE-CC-9"), Some("/app/#/ticket/SYNAPSE-CC-9".into()));
        assert_eq!(to("/KILN-2.1"), Some("/app/#/ticket/KILN-2.1".into()), "a subtask");
        assert_eq!(to("/PERF-19/"), Some("/app/#/ticket/PERF-19".into()), "one trailing slash");
        for other in [
            "/", "/app", "/app/", "/app/index.html", "/app/#/ticket/X-1", "/login", "/login/start", "/ws", "/mcp", "/sw.js",
            "/manifest.webmanifest", "/favicon.svg", "/vanilla", "/robots.txt", "/PERF", "/MENGER/STORE", "/t/", "/t",
            "/t/not-a-uid/x", "/t/7k3mq2x", "/7k3mq2xa", "//evil.example/PERF-1", "/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/PERF-1x",
            "/PERF-19\\x", "/%2e%2e/PERF-1", "/PERF-1%0a",
        ] {
            assert_eq!(to(other), None, "{other}");
        }
    }

    /// MFE-23: `menger.test` on the test environment, `menger` otherwise.
    #[test]
    fn the_tab_title_names_only_the_test_environment() {
        assert_eq!(tab_title(Some("test")), "menger.test");
        assert_eq!(tab_title(Some("prod")), "menger");
        assert_eq!(tab_title(Some("dev")), "menger");
        assert_eq!(tab_title(None), "menger");
    }

    /// DIST-3: without the feature (the default test build) nothing is
    /// embedded, and the checkout's dist is preferred; with it and a build,
    /// the embedded table is.
    #[test]
    fn the_embedded_build_is_preferred_only_when_there_is_one() {
        if !cfg!(feature = "embedded-app") {
            assert!(!embedded_available());
            assert_eq!(preferred_app(), AppSource::Dir(default_app_dist()));
        } else if embedded_available() {
            assert!(matches!(preferred_app(), AppSource::Embedded(_)));
        }
    }

    /// The vanilla page and the app's entrypoint are stamped with the
    /// socket's port and the environment's title.
    #[test]
    fn the_vanilla_page_is_stamped_as_the_app_is() {
        let cfg = app_shell(Some("test"), AppSource::Dir("/nowhere".into()));
        let stamped = plexus_shell::stamp(VANILLA, &cfg.stamps_for(4450));
        assert!(stamped.contains("<title>menger.test</title>") && !stamped.contains("{{WS_PORT}}"));
        assert_eq!(cfg.pages[0].0, "/vanilla");
    }

    /// DESIGN-40: the faces and the vendored code are the public assets a
    /// frame loads; APP-64: and the home-screen files; APP-92: and the app's
    /// code and its service worker, which download behind the sign-in page.
    /// No page of the app is.
    #[test]
    fn only_the_faces_and_the_vendored_code_are_public_assets() {
        for code in ["/app/menger-app-6ba34624f6d33bb3.js", "/app/menger-app-6ba34624f6d33bb3_bg.wasm", "/app/app-0123456789abcdef.css", "/sw.js", "/app/sdk/menger-sdk.js"] {
            assert!(is_public_asset(code), "{code}");
        }
        for page in ["/app/", "/app", "/app/index.html", "/app/sdk/x.html", "/app/../index.html", "/index.html", "/login/me"] {
            assert!(!is_public_asset(page), "{page}");
        }
        assert!(is_public_asset("/app/fonts/geist-latin.woff2"));
        assert!(is_public_asset("/app/vendor/mermaid.min.js"));
        assert!(!is_public_asset("/app/index.html"));
        assert!(!is_public_asset("/app/fontsx"));
        assert!(!is_public_asset("/"));
        assert!(is_public_asset("/apple-touch-icon.png") && is_public_asset("/app/manifest.webmanifest"));
        assert!(!is_public_asset("/app/apple-touch-icon.png/x") && !is_public_asset("/vendor/manifest.webmanifest"));
    }

    /// DESIGN-40: a face is answered with `Access-Control-Allow-Origin: *`;
    /// any other path is not.
    #[tokio::test]
    async fn a_face_is_readable_from_any_origin() {
        let app = Router::new()
            .route("/app/fonts/geist-latin.woff2", get(|| async { "font" }))
            .route("/app/index.html", get(|| async { "page" }))
            .layer(axum::middleware::from_fn(fonts_cors));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let at = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let ask = |p: &str| reqwest::get(format!("http://{at}{p}"));
        let font = ask("/app/fonts/geist-latin.woff2").await.unwrap();
        assert_eq!(font.headers().get("access-control-allow-origin").unwrap(), "*");
        let page = ask("/app/index.html").await.unwrap();
        assert!(page.headers().get("access-control-allow-origin").is_none());
    }

    /// APP-110: unconfigured, the policy is [`PAGE_CSP`] to the byte — frames
    /// only its own, framed by no one. Configured, exactly two directives move,
    /// each by the origins named, and nothing else changes.
    #[test]
    fn the_page_policy_moves_only_what_framing_names() {
        assert_eq!(page_csp(&Framing::default()), PAGE_CSP);
        assert!(page_csp(&Framing::default()).contains("frame-ancestors 'none'"));
        let tracker = page_csp(&Framing { ancestors: vec![], live: Some("https://app-test.menger.sh".into()) });
        assert!(tracker.contains("frame-src 'self' https://app-test.menger.sh;") && tracker.contains("frame-ancestors 'none'"), "{tracker}");
        let test = page_csp(&Framing { ancestors: vec!["https://app.menger.sh".into(), "http://127.0.0.1:4456".into()], live: None });
        assert!(test.contains("frame-ancestors https://app.menger.sh http://127.0.0.1:4456") && test.contains("frame-src 'self';"), "{test}");
        // Nothing else moved.
        assert_eq!(test.replace("frame-ancestors https://app.menger.sh http://127.0.0.1:4456", "frame-ancestors 'none'"), PAGE_CSP);
        assert_eq!(tracker.replace("frame-src 'self' https://app-test.menger.sh", "frame-src 'self'"), PAGE_CSP);
    }

    /// APP-110: an origin is scheme, host and port — written canonically;
    /// a path, a wildcard, a credential or another scheme is refused.
    #[test]
    fn an_origin_is_checked_before_it_reaches_a_policy() {
        assert_eq!(check_origin("https://app.menger.sh"), Ok("https://app.menger.sh".into()));
        assert_eq!(check_origin("https://App.Menger.sh/"), Ok("https://app.menger.sh".into()));
        assert_eq!(check_origin(" http://127.0.0.1:4456 "), Ok("http://127.0.0.1:4456".into()));
        assert_eq!(check_origin("https://app.menger.sh:443"), Ok("https://app.menger.sh".into()));
        for bad in ["", "app.menger.sh", "*", "https://*.menger.sh", "https://app.menger.sh/x", "https://app.menger.sh?x", "https://app.menger.sh#x",
            "https://u:p@app.menger.sh", "ftp://app.menger.sh", "https://app.menger.sh; script-src *", "'none'", "https://a.sh https://b.sh"] {
            assert!(check_origin(bad).is_err(), "{bad:?}");
        }
    }

    /// APP-110: the stamp says which test app the app may draw — empty when
    /// none — and the entrypoint carries its placeholder.
    #[test]
    fn the_app_is_told_the_live_origin() {
        assert!(include_str!("../frontend/index.html").contains(r#"<meta name="menger-live-origin" content="{{LIVE_ORIGIN}}">"#));
        let on = app_shell_framed(None, AppSource::Dir("/nowhere".into()), &Framing { ancestors: vec![], live: Some("https://t.example".into()) });
        assert_eq!(plexus_shell::stamp("{{LIVE_ORIGIN}}", &on.stamps_for(4450)), "https://t.example");
        assert_eq!(plexus_shell::stamp("[{{LIVE_ORIGIN}}]", &app_shell(None, AppSource::Dir("/nowhere".into())).stamps_for(4450)), "[]");
        assert!(on.csp.contains("frame-src 'self' https://t.example"));
    }

    #[test]
    fn both_pages_carry_the_title_placeholder_and_the_icon() {
        for page in [VANILLA, include_str!("../frontend/index.html")] {
            assert!(page.contains("<title>{{TITLE}}</title>"));
            assert!(page.contains(r#"<link rel="icon" type="image/svg+xml" href="/favicon.svg">"#));
        }
        assert!(FAVICON.starts_with("<svg") && FAVICON.contains("viewBox"));
    }
}
