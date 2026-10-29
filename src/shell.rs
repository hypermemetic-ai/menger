//! The web shell: the login routes, and the app's build when there is one.
//!
//! v2: the frontend is deleted (tag v0.0.1 holds v1's Leptos app, the
//! `/vanilla` page and the body frame). Pages come back one at a time, from
//! the mockups on their tickets. Until then:
//!
//! - `/` — [`PLACEHOLDER`], what is running and where its socket is;
//! - `/app/` — the app's build in `frontend/dist` read into a table
//!   (ACCESS-33: a request path is a key into it, never a path on disk), or
//!   the "not built" page (200) while there is none;
//! - the login routes (IDENTITY-19), unchanged — the auth surface is the
//!   next thing v2 reworks.
//!
//! SCAFFOLD-5: the table, the stamping, the app's routes, the hardening
//! (IDENTITY-38) and [`AppSource`] are `plexus-shell`'s; this module is
//! menger's part around them.
//!
//! ACCESS-28: every page goes out under a policy — the app's under
//! [`PAGE_CSP`], the sign-in pages under plexus-login's own.

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
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
    /// UI-3: the ui service's store — `/app/` serves the live version's
    /// entrypoint (a session's preview to that session), `/app/<hash>/…` a
    /// version's files ([`ui_app`]). None: the build in [`Self::app`], as before.
    pub ui: Option<Arc<menger_ui_service::UiStore>>,
}

/// UI-3: a version's files never change under its hash — kept a year.
pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// UI-3: the entrypoint names whichever version is live — never kept.
pub const NO_STORE: &str = "no-store";

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


/// v2: the page at `/` while there is no frontend — what is running and
/// where its socket is, nothing else (stamped as the app's entrypoint is).
pub const PLACEHOLDER: &str = concat!(
    "<!doctype html><meta charset=\"utf-8\"><title>{{TITLE}}</title>",
    "<body style=\"font: 14px/1.6 monospace; padding: 2rem; max-width: 46rem\">",
    "<h1 style=\"font-size:1.1rem\">menger v2</h1>",
    "<p>No frontend yet — each page is built from the mockups on its ticket.</p>",
    "<p>The service is on port {{WS_PORT}}: <code>synapse -P {{WS_PORT}} menger tickets</code>.</p>",
);

/// APP-64: the app build's files served at the root too, public — what
/// Add to Home Screen and an installed app ask the origin for (iOS takes
/// no SVG icon; v1 drew them, at tag v0.0.1). No data in any of them,
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
/// hub's WebSocket origins at the name the page was asked for):
///
/// - scripts: the app's files, the wasm, the hashed inline blocks — no
///   other inline script, no other origin;
/// - styles `'unsafe-inline'` (the app writes `style` and `<style>`: the
///   theme blocks) and `https:` with fonts — a theme may `@import` a face's
///   sheet (THEME-3: `@import url("https://…")`);
/// - images: its own and `data:` — a remote image in a body shown in the
///   app is refused, so a body cannot learn who reads it (ACCESS-4 M5);
/// - connections: itself, its hub, and a WebSocket on loopback — MFE-21's
///   settings point the app at another menger on this machine;
/// - frames: its own; nothing frames it.
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

/// The degraded answer when `frontend/dist` holds no build. 200 on
/// purpose: "not built yet" is a documented state of a fresh checkout,
/// and the page itself is the clear indication MFE-2 requires.
const NOT_BUILT: &str = concat!(
    "<!doctype html><meta charset=\"utf-8\"><title>menger — frontend not built</title>",
    "<body style=\"font: 14px/1.6 monospace; padding: 2rem; max-width: 46rem\">",
    "<h1 style=\"font-size:1.1rem\">frontend not built</h1>",
    "<p>The app's build output is missing.</p>",
    "<p>v2 has no frontend yet; the server itself is fine.</p>",
);

/// Where `trunk build` puts the app when run from a checkout. Overridable
/// via `--app-dist` / `MENGER_APP_DIST` for deployments that move dist.
pub fn default_app_dist() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend/dist")
}

/// The source when none is named: the checkout's `frontend/dist`
/// ([`default_app_dist`]). v2: nothing is embedded (build.rs went with the
/// frontend).
pub fn preferred_app() -> AppSource {
    AppSource::Dir(default_app_dist())
}

/// menger's shell as plexus-shell serves it: the app under `/app/` (where
/// `Trunk.toml`'s `public_url` puts its assets, and MFE-1's deep links),
/// titled for the environment (MFE-23), and `/` falling back to
/// [`PLACEHOLDER`] when there is no build (MFE-4). No other routes: the
/// login routes are added around it ([`serve_on`]).
pub fn app_shell(env: Option<&str>, app: AppSource) -> plexus_shell::ShellConfig {
    app_shell_framed(env, app, &Framing::default())
}

/// [`app_shell`] under APP-110's [`Framing`]: the policy as [`page_csp`] says,
/// and the app told which test app its bodies may draw (`{{LIVE_ORIGIN}}`,
/// empty when none).
pub fn app_shell_framed(env: Option<&str>, app: AppSource, framing: &Framing) -> plexus_shell::ShellConfig {
    let mut cfg = plexus_shell::ShellConfig::new(tab_title(env), app);
    cfg.stamps.push(("{{LIVE_ORIGIN}}".into(), framing.live.clone().unwrap_or_default()));
    cfg.root_fallback = Some(PLACEHOLDER.into());
    cfg.not_built = Some(NOT_BUILT.into());
    cfg.csp = page_csp(framing);
    cfg.root_files = ROOT_FILES.iter().map(|f| f.to_string()).collect();
    cfg
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
    // ACCESS-33: menger's own pages and the app's build — nothing else. The
    // `/rest/{*}` pass-through to the REST port went: the app has spoken
    // the WebSocket since MFE-9, and nothing else called it. IDENTITY-19:
    // behind a session when login is required — no anonymous view.
    let shell = app_shell_framed(cfg.env.as_deref(), cfg.app, &cfg.framing);
    let (stamps, csp) = (shell.stamps_for(cfg.ws_port), shell.csp.clone());
    let mut app = plexus_shell::router(shell, cfg.ws_port).await;
    // UI-3: `/app/*` from the ui store — inside the session check (layered
    // first, so it runs after it).
    if let Some(store) = cfg.ui.clone() {
        let ui = Arc::new(UiServe { store, login: cfg.login.clone(), stamps, csp, ws_port: cfg.ws_port });
        app = app.route_layer(axum::middleware::from_fn_with_state(ui, ui_app));
    }
    if let Some(login) = cfg.login.as_ref().filter(|l| l.require) {
        app = app.route_layer(axum::middleware::from_fn_with_state(login.clone(), require_session));
    }
    if let Some(metadata) = cfg.metadata {
        app = app.merge(metadata);
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

/// UI-3: what `/app/*` is served from — the store, the login (whose session
/// names a preview's principal) and the entrypoint's stamps.
struct UiServe {
    store: Arc<menger_ui_service::UiStore>,
    login: Option<Arc<crate::web_login::Login>>,
    stamps: Vec<(String, String)>,
    /// The pages' policy template ([`page_csp`]), filled per page.
    csp: String,
    ws_port: u16,
}

/// UI-3: `/app/<hash>/<path>` — that file of that version's bundle,
/// [`IMMUTABLE`]; `/app/` — the entrypoint for the request's session (its
/// preview, else live), stamped, [`NO_STORE`]; with no version live,
/// [`PLACEHOLDER`]. Anything else goes on to the shell's own routes.
async fn ui_app(State(ui): State<Arc<UiServe>>, req: Request, next: axum::middleware::Next) -> Response {
    let path = req.uri().path().to_string();
    if path == "/app" || path == "/app/" {
        let principal = ui.login.as_ref().and_then(|l| l.session(req.headers())).map(|ctx| ctx.principal().map(|p| p.to_string()).unwrap_or(ctx.user_id));
        let store = ui.store.clone();
        let entry = tokio::task::spawn_blocking(move || store.entry_for(principal.as_deref())).await;
        let (html, version) = match entry {
            Ok(Ok(Some((hash, html)))) => (String::from_utf8_lossy(&html).into_owned(), hash),
            Ok(Ok(None)) => (PLACEHOLDER.to_string(), String::new()),
            Ok(Err(e)) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("the ui store: {e}")).into_response(),
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("the ui store: {e}")).into_response(),
        };
        // UI-6: `{{UI_VERSION}}` — the served version's hash, where the
        // entrypoint loads the engine from (`/app/<hash>/engine.js`).
        let mut stamps = ui.stamps.clone();
        stamps.push(("{{UI_VERSION}}".to_string(), version));
        return ui.page(plexus_shell::stamp(&html, &stamps), req.headers());
    }
    let Some((hash, file)) = path.strip_prefix("/app/").and_then(|rest| rest.split_once('/')) else { return next.run(req).await };
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) || file.is_empty() || file.split('/').any(|s| s == "..") {
        return next.run(req).await;
    }
    let (store, hash, file) = (ui.store.clone(), hash.to_ascii_lowercase(), file.to_string());
    let key = file.clone();
    match tokio::task::spawn_blocking(move || store.file(&hash, &file)).await {
        Ok(Ok(Some(bytes))) => {
            let mut r = ([(header::CONTENT_TYPE, plexus_shell::mime_of(&key))], bytes).into_response();
            r.headers_mut().insert(header::CACHE_CONTROL, header::HeaderValue::from_static(IMMUTABLE));
            r
        }
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "no such file in that version").into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("the ui store: {e}")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("the ui store: {e}")).into_response(),
    }
}

impl UiServe {
    /// UI-3: the entrypoint (or the placeholder) as a page — under the
    /// pages' policy (ACCESS-28), its own inline scripts hashed, never kept.
    fn page(&self, html: String, headers: &axum::http::HeaderMap) -> Response {
        let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
        let policy = plexus_shell::page_policy(&self.csp, &plexus_shell::script_hashes(&html), host, self.ws_port);
        let mut r = axum::response::Html(html).into_response();
        let h = r.headers_mut();
        h.insert(header::CACHE_CONTROL, header::HeaderValue::from_static(NO_STORE));
        if let Ok(v) = header::HeaderValue::from_str(&policy) {
            h.insert(header::CONTENT_SECURITY_POLICY, v);
        }
        r
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

#[cfg(test)]
mod tests {
    use super::*;

    /// MFE-23: `menger.test` on the test environment, `menger` otherwise.
    #[test]
    fn the_tab_title_names_only_the_test_environment() {
        assert_eq!(tab_title(Some("test")), "menger.test");
        assert_eq!(tab_title(Some("prod")), "menger");
        assert_eq!(tab_title(Some("dev")), "menger");
        assert_eq!(tab_title(None), "menger");
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

    /// v2: the placeholder is stamped as the app's entrypoint is, and is
    /// what `/` answers with no build.
    #[test]
    fn the_placeholder_is_stamped_and_is_the_root_fallback() {
        let cfg = app_shell(Some("test"), AppSource::Dir("/nowhere".into()));
        let stamped = plexus_shell::stamp(PLACEHOLDER, &cfg.stamps_for(4450));
        assert!(stamped.contains("<title>menger.test</title>") && stamped.contains("-P 4450") && !stamped.contains("{{"));
        assert_eq!(cfg.root_fallback.as_deref(), Some(PLACEHOLDER));
        assert!(cfg.pages.is_empty());
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
        let on = app_shell_framed(None, AppSource::Dir("/nowhere".into()), &Framing { ancestors: vec![], live: Some("https://t.example".into()) });
        assert_eq!(plexus_shell::stamp("{{LIVE_ORIGIN}}", &on.stamps_for(4450)), "https://t.example");
        assert_eq!(plexus_shell::stamp("[{{LIVE_ORIGIN}}]", &app_shell(None, AppSource::Dir("/nowhere".into())).stamps_for(4450)), "[]");
        assert!(on.csp.contains("frame-src 'self' https://t.example"));
    }

}
