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
//! `Embedded`; else → [`default_app_dist`] ([`AppSource::preferred`]).

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

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
}

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

/// What both pages carry from the server: the WebSocket port (MFE-9) and
/// the tab's title (MFE-23), stamped into `{{WS_PORT}}` / `{{TITLE}}`.
fn stamp(text: &str, ws_port: u16, title: &str) -> String {
    text.replace("{{WS_PORT}}", &ws_port.to_string()).replace("{{TITLE}}", title)
}

/// Where `trunk build` puts the app when run from a checkout. Overridable
/// via `--app-dist` / `MENGER_APP_DIST` for deployments that move dist.
pub fn default_app_dist() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend/dist")
}

// DIST-3: `EMBEDDED_APP`, the app's build as build.rs found it — empty
// unless the `embedded-app` feature is on and frontend/dist held a build.
include!(concat!(env!("OUT_DIR"), "/embedded_app.rs"));

/// DIST-3: where the app's build is served from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppSource {
    /// A `trunk build` output directory on disk, read into the table and
    /// read again when its entrypoint changes (ACCESS-33).
    Dir(PathBuf),
    /// The build compiled into this binary (`embedded-app`). With nothing
    /// embedded it is "not built", as an empty directory is.
    Embedded,
}

impl AppSource {
    /// Whether this binary carries the app's build: the `embedded-app`
    /// feature was on and frontend/dist held a build when it compiled.
    pub fn embedded_available() -> bool {
        EMBEDDED_APP.iter().any(|(key, _)| *key == "index.html")
    }

    /// The source when none is named: the embedded build when there is
    /// one, else the checkout's `frontend/dist` ([`default_app_dist`]).
    pub fn preferred() -> Self {
        if Self::embedded_available() {
            Self::Embedded
        } else {
            Self::Dir(default_app_dist())
        }
    }
}

/// One file of the app's build, as served.
struct Asset {
    bytes: Bytes,
    mime: &'static str,
}

impl Asset {
    /// A build's file as served: the entrypoint stamped (MFE-9, MFE-23),
    /// everything else byte-exact; its MIME type from its key.
    fn of(key: &str, data: Vec<u8>, ws_port: u16, title: &str) -> Self {
        let data = if key == "index.html" {
            match String::from_utf8(data) {
                Ok(text) => stamp(&text, ws_port, title).into_bytes(),
                Err(e) => e.into_bytes(),
            }
        } else {
            data
        };
        Asset { bytes: Bytes::from(data), mime: mime_of(key) }
    }
}

/// ACCESS-33: the app's build as a table — the only files `/app/…` can
/// answer. A request path is a key into it, never a path on disk: what is
/// not in the table is 404 whatever the filesystem holds, so there is no
/// path to sanitise. Read whole from dist (regular files only — a symlink
/// is not followed), with the entrypoint stamped (MFE-9, MFE-23).
#[derive(Default)]
struct AppFiles {
    /// The entrypoint's modification time when this table was read —
    /// `None` when there was no build.
    built: Option<SystemTime>,
    files: HashMap<String, Asset>,
}

/// A table bigger than this is not an app build (a mistaken `--app-dist`):
/// refused whole rather than read into memory. The real build is ~25
/// files, ~6 MB.
const MAX_FILES: usize = 4096;
const MAX_BYTES: u64 = 256 << 20;

impl AppFiles {
    /// Reads `dist` (blocking). A dist without an entrypoint is no build,
    /// and is not walked; an oversized one is refused whole, logged. Both
    /// are an empty table — "not built".
    fn load(dist: &Path, ws_port: u16, title: &str) -> Self {
        let built = std::fs::metadata(dist.join("index.html")).and_then(|m| m.modified()).ok();
        if built.is_none() {
            return Self::default();
        }
        let mut files = HashMap::new();
        let mut bytes = 0u64;
        let mut dirs = vec![(dist.to_path_buf(), String::new())];
        while let Some((dir, prefix)) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let (Ok(kind), Some(name)) = (entry.file_type(), entry.file_name().to_str().map(str::to_string)) else {
                    continue;
                };
                let key = format!("{prefix}{name}");
                if kind.is_dir() {
                    dirs.push((entry.path(), format!("{key}/")));
                } else if kind.is_file() {
                    bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                    if files.len() >= MAX_FILES || bytes > MAX_BYTES {
                        tracing::error!("{dist:?} is not an app build (over {MAX_FILES} files or {MAX_BYTES} bytes): serving none of it");
                        return Self::default();
                    }
                    let Ok(data) = std::fs::read(entry.path()) else { continue };
                    files.insert(key.clone(), Asset::of(&key, data, ws_port, title));
                }
            }
        }
        Self { built, files }
    }

    /// DIST-3: the table from a build compiled into the binary — the same
    /// keys, stamping and MIME types as [`Self::load`]. Without an
    /// entrypoint it is no build, as on disk. Its `built` is a fixed time:
    /// an embedded build never changes.
    fn embedded(table: &[(&str, &[u8])], ws_port: u16, title: &str) -> Self {
        if !table.iter().any(|(key, _)| *key == "index.html") {
            return Self::default();
        }
        let files = table
            .iter()
            .map(|(key, data)| (key.to_string(), Asset::of(key, data.to_vec(), ws_port, title)))
            .collect();
        Self { built: Some(SystemTime::UNIX_EPOCH), files }
    }

    fn get(&self, key: &str) -> Option<&Asset> {
        self.files.get(key)
    }
}

fn mime_of(key: &str) -> &'static str {
    match key.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css",
        Some("json") | Some("map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        // COMP-10: the SDK's TypeScript declarations (sdk/*.d.ts),
        // shown as text.
        Some("txt") | Some("ts") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The app's table, and when to read it again. A release's dist never
/// changes, so the table is read once; in a checkout `trunk watch` rewrites
/// dist, and a new entrypoint (its modification time — one fixed file,
/// never a request path) has the table read again, so a reload picks up a
/// rebuild without a restart. DIST-3: an embedded build is read once and
/// never again.
struct App {
    source: AppSource,
    ws_port: u16,
    title: &'static str,
    files: RwLock<Arc<AppFiles>>,
}

impl App {
    async fn new(source: AppSource, ws_port: u16, title: &'static str) -> Self {
        let app = Self { source, ws_port, title, files: RwLock::default() };
        app.reload().await;
        app
    }

    fn table(&self) -> Arc<AppFiles> {
        self.files.read().expect("app table lock poisoned").clone()
    }

    async fn reload(&self) -> Arc<AppFiles> {
        let (ws_port, title) = (self.ws_port, self.title);
        let fresh = Arc::new(match &self.source {
            AppSource::Dir(dist) => {
                let dist = dist.clone();
                tokio::task::spawn_blocking(move || AppFiles::load(&dist, ws_port, title)).await.unwrap_or_default()
            }
            AppSource::Embedded => AppFiles::embedded(EMBEDDED_APP, ws_port, title),
        });
        tracing::debug!("app build read from {:?}: {} files", self.source, fresh.files.len());
        *self.files.write().expect("app table lock poisoned") = fresh.clone();
        fresh
    }

    /// The table, read again first if the build's entrypoint changed.
    async fn current(&self) -> Arc<AppFiles> {
        let table = self.table();
        let AppSource::Dir(dist) = &self.source else {
            return table;
        };
        let built = tokio::fs::metadata(dist.join("index.html")).await.and_then(|m| m.modified()).ok();
        if built == table.built {
            table
        } else {
            self.reload().await
        }
    }
}

struct ShellState {
    /// The day-one page, WS port already substituted. Served at
    /// `/vanilla`, and at `/` when there is no app build (MFE-4).
    vanilla: String,
    /// The app's build (ACCESS-33). MFE-9: the app talks to the Plexus
    /// WebSocket directly, and its page cannot know which port that is, so
    /// the entrypoint is stamped with it as it is read — exactly as the
    /// vanilla page has always been stamped. One convention, not two.
    app: App,
}

pub async fn serve(cfg: ShellConfig) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.port)).await?;
    serve_on(listener, cfg, std::future::pending()).await
}

/// DIST-3: [`serve`] on a listener the caller bound (`menger::local` binds
/// port 0 and learns the port before anything starts), until `shutdown`
/// resolves. `cfg.port` is the listener's port.
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    cfg: ShellConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let app = router(&cfg).await;
    tracing::info!("App:       http://127.0.0.1:{}/", cfg.port);
    tracing::info!("Vanilla:   http://127.0.0.1:{}/vanilla", cfg.port);
    axum::serve(listener, app).with_graceful_shutdown(shutdown).await?;
    Ok(())
}

async fn router(cfg: &ShellConfig) -> Router {
    let title = tab_title(cfg.env.as_deref());
    let state = Arc::new(ShellState {
        vanilla: stamp(include_str!("../web/index.html"), cfg.ws_port, title),
        app: App::new(cfg.app.clone(), cfg.ws_port, title).await,
    });
    // ACCESS-33: menger's own pages and the app's build — nothing else. The
    // `/rest/{*}` pass-through to the REST port went: the app has spoken
    // the WebSocket since MFE-9, and nothing else called it. IDENTITY-19:
    // behind a session when login is required — no anonymous view.
    let mut app = Router::new()
        .route("/", get(root_index))
        .route("/vanilla", get(vanilla))
        .route("/app", get(app_index))
        .route("/app/", get(app_index))
        .route("/app/{*path}", get(app_asset))
        .with_state(state.clone());
    if let Some(login) = cfg.login.as_ref().filter(|l| l.require) {
        app = app.route_layer(axum::middleware::from_fn_with_state(login.clone(), require_session));
    }
    let mut app = app.merge(Router::new().route("/favicon.svg", get(favicon)));
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
                .with_state(login.clone()),
        );
        // DNS rebinding: a page on another name resolving here gets nothing.
        let hosts = Arc::new(allowed_hosts(cfg.port, &login));
        app = app.layer(axum::middleware::from_fn_with_state(hosts, host_is_ours));
        tracing::info!("Login:     http://127.0.0.1:{}/login", cfg.port);
    }
    app.layer(axum::middleware::from_fn(harden))
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
    if login.session(req.headers()).is_some() || req.uri().path().starts_with("/app/vendor/") {
        return next.run(req).await;
    }
    let path = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/");
    if path.starts_with("/rest/") {
        return (StatusCode::UNAUTHORIZED, "sign in first").into_response();
    }
    let to = format!("/login?next={}", url::form_urlencoded::byte_serialize(path.as_bytes()).collect::<String>());
    (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response()
}

/// IDENTITY-38: every answer the shell gives refuses to be framed (the app
/// holds Grant buttons, and another port on this host is same-site), is
/// read as the type it says, and sends no path to another origin. A
/// response that set its own keeps it.
async fn harden(req: Request, next: axum::middleware::Next) -> Response {
    let mut r = next.run(req).await;
    let h = r.headers_mut();
    for (name, value) in [
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::CONTENT_SECURITY_POLICY, "frame-ancestors 'none'"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "same-origin"),
    ] {
        if !h.contains_key(&name) {
            h.insert(name, header::HeaderValue::from_static(value));
        }
    }
    r
}

/// The `Host` values this shell answers to: its port on 127.0.0.1 and
/// localhost, and the public URL's host.
fn allowed_hosts(port: u16, login: &crate::web_login::Login) -> Vec<String> {
    let mut hosts = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    hosts.extend(login.hosts());
    hosts
}

/// IDENTITY-24 #1: a request naming another host (DNS rebinding) is refused.
async fn host_is_ours(State(hosts): State<Arc<Vec<String>>>, req: Request, next: axum::middleware::Next) -> Response {
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).map(str::to_ascii_lowercase);
    match host {
        Some(h) if hosts.iter().any(|a| *a == h) => next.run(req).await,
        _ => (StatusCode::MISDIRECTED_REQUEST, "this server does not answer to that name").into_response(),
    }
}

/// The degraded answer when `frontend/dist` holds no build. 200 on
/// purpose: "not built yet" is a documented state of a fresh checkout,
/// and the page itself is the clear indication MFE-2 requires.
fn not_built() -> Response {
    Html(concat!(
        "<!doctype html><meta charset=\"utf-8\"><title>menger — frontend not built</title>",
        "<body style=\"font: 14px/1.6 monospace; padding: 2rem; max-width: 46rem\">",
        "<h1 style=\"font-size:1.1rem\">frontend not built</h1>",
        "<p>The Leptos app's build output is missing. Build it once:</p>",
        "<pre>cd frontend && trunk build --release</pre>",
        "<p>then reload. Toolchain setup is in README.md → Frontend. ",
        "The server itself is fine — the vanilla shell is at ",
        "<a href=\"/vanilla\">/vanilla</a>, and <a href=\"/\">/</a> serves it ",
        "until this build exists.</p>",
    ))
    .into_response()
}

fn asset_response(asset: &Asset) -> Response {
    ([(header::CONTENT_TYPE, asset.mime)], asset.bytes.clone()).into_response()
}

/// MFE-4: the root is the app. No build is not an error here — it is the
/// day-one page, which needs no build and works.
async fn root_index(State(st): State<Arc<ShellState>>) -> Response {
    match st.app.current().await.get("index.html") {
        Some(index) => asset_response(index),
        None => {
            tracing::debug!("no app build at {:?}: / degrades to the vanilla page", st.app.source);
            Html(st.vanilla.clone()).into_response()
        }
    }
}

/// MFE-23: the tab's icon, for both pages.
async fn favicon() -> Response {
    (
        [(header::CONTENT_TYPE, "image/svg+xml"), (header::CACHE_CONTROL, "public, max-age=86400")],
        FAVICON,
    )
        .into_response()
}

/// The day-one page, wherever the app stands.
async fn vanilla(State(st): State<Arc<ShellState>>) -> Response {
    Html(st.vanilla.clone()).into_response()
}

async fn app_index(State(st): State<Arc<ShellState>>) -> Response {
    match st.app.current().await.get("index.html") {
        Some(index) => asset_response(index),
        None => not_built(),
    }
}

async fn app_asset(State(st): State<Arc<ShellState>>, AxumPath(path): AxumPath<String>) -> Response {
    let key = if path.is_empty() || path.ends_with('/') { format!("{path}index.html") } else { path };
    // A miss asks whether the build changed before it is a 404: a page
    // loaded from a rebuilt entrypoint names assets the old table lacks.
    let mut table = st.app.table();
    if table.get(&key).is_none() {
        table = st.app.current().await;
    }
    match table.get(&key) {
        Some(asset) => {
            // An access line per served asset (RUST_LOG=menger=debug): the
            // one place to see which assets a page actually pulled — AGS-14
            // pins that the mermaid bundle is fetched only by pages that
            // hold a diagram.
            tracing::debug!("app asset served: /app/{key} ({} bytes)", asset.bytes.len());
            asset_response(asset)
        }
        // The entrypoint missing means "not built"; a missing asset next
        // to a present index.html is an ordinary 404 (stale link, typo).
        None if key == "index.html" => not_built(),
        None => (StatusCode::NOT_FOUND, "no such asset").into_response(),
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

    /// ACCESS-33: the table holds the build's regular files by their
    /// '/'-joined paths, the entrypoint stamped — and nothing a request
    /// path could reach beyond them.
    #[test]
    fn the_app_table_holds_the_build_and_nothing_else() {
        let root = tempfile::tempdir().unwrap();
        let dist = root.path().join("dist");
        std::fs::create_dir_all(dist.join("vendor")).unwrap();
        std::fs::write(dist.join("index.html"), "<title>{{TITLE}}</title>{{WS_PORT}}").unwrap();
        std::fs::write(dist.join("app.js"), "js").unwrap();
        std::fs::write(dist.join("vendor/mermaid.min.js"), "m").unwrap();
        std::fs::write(root.path().join("secret.txt"), "s").unwrap();
        std::os::unix::fs::symlink(root.path().join("secret.txt"), dist.join("link.txt")).unwrap();

        let t = AppFiles::load(&dist, 4450, "menger.test");
        let mut keys: Vec<_> = t.files.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["app.js", "index.html", "vendor/mermaid.min.js"], "a symlink is not followed");
        assert_eq!(&t.get("index.html").unwrap().bytes[..], b"<title>menger.test</title>4450");
        assert_eq!(t.get("vendor/mermaid.min.js").unwrap().mime, "text/javascript");
        assert!(t.built.is_some());
        for miss in ["/etc/hostname", "../secret.txt", "secret.txt", "link.txt", "vendor", "./app.js", ""] {
            assert!(t.get(miss).is_none(), "{miss:?}");
        }
        let none = AppFiles::load(&root.path().join("missing"), 4450, "menger");
        assert!(none.files.is_empty() && none.built.is_none());
        std::fs::remove_file(dist.join("index.html")).unwrap();
        let headless = AppFiles::load(&dist, 4450, "menger");
        assert!(headless.files.is_empty(), "no entrypoint, no build: nothing under it is served");
    }

    /// ACCESS-33: a rebuilt entrypoint has the table read again; an asset
    /// dropped into dist without a rebuild is not.
    #[tokio::test]
    async fn the_table_follows_a_rebuild_and_nothing_less() {
        let dist = tempfile::tempdir().unwrap();
        std::fs::write(dist.path().join("index.html"), "v1").unwrap();
        let app = App::new(AppSource::Dir(dist.path().to_path_buf()), 1, "menger").await;
        std::fs::write(dist.path().join("late.js"), "x").unwrap();
        assert!(app.current().await.get("late.js").is_none(), "no rebuild, no new file");
        let index = std::fs::File::options().write(true).open(dist.path().join("index.html")).unwrap();
        index.set_modified(SystemTime::now() + std::time::Duration::from_secs(60)).unwrap();
        let t = app.current().await;
        assert!(t.get("late.js").is_some(), "a rebuild is read whole");
    }

    /// DIST-3: an embedded build is the same table a directory gives — the
    /// entrypoint stamped, MIME types by key — and one with no entrypoint
    /// is no build.
    #[tokio::test]
    async fn an_embedded_build_is_the_same_table() {
        let table: &[(&str, &[u8])] =
            &[("index.html", b"<title>{{TITLE}}</title>{{WS_PORT}}"), ("app.wasm", b"\0asm"), ("vendor/m.js", b"m")];
        let t = AppFiles::embedded(table, 4471, "menger");
        let mut keys: Vec<_> = t.files.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["app.wasm", "index.html", "vendor/m.js"]);
        assert_eq!(&t.get("index.html").unwrap().bytes[..], b"<title>menger</title>4471");
        assert_eq!(t.get("app.wasm").unwrap().mime, "application/wasm");
        assert_eq!(t.get("vendor/m.js").unwrap().mime, "text/javascript");
        assert!(t.get("../index.html").is_none());
        let headless = AppFiles::embedded(&[("app.js", b"x")], 1, "menger");
        assert!(headless.files.is_empty() && headless.built.is_none());
        // Without the feature (the default test build) nothing is embedded.
        if !cfg!(feature = "embedded-app") {
            assert!(!AppSource::embedded_available());
            assert_eq!(AppSource::preferred(), AppSource::Dir(default_app_dist()));
            let app = App::new(AppSource::Embedded, 1, "menger").await;
            assert!(app.current().await.get("index.html").is_none(), "nothing embedded is not built");
        }
    }

    #[test]
    fn a_page_is_stamped_with_its_port_and_title() {
        let page = "<title>{{TITLE}}</title><meta content=\"{{WS_PORT}}\">";
        assert_eq!(stamp(page, 4450, "menger.test"), "<title>menger.test</title><meta content=\"4450\">");
    }

    #[test]
    fn both_pages_carry_the_title_placeholder_and_the_icon() {
        for page in [include_str!("../web/index.html"), include_str!("../frontend/index.html")] {
            assert!(page.contains("<title>{{TITLE}}</title>"));
            assert!(page.contains(r#"<link rel="icon" type="image/svg+xml" href="/favicon.svg">"#));
        }
        assert!(FAVICON.starts_with("<svg") && FAVICON.contains("viewBox"));
    }
}
