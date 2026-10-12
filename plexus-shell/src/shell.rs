//! The shell: the app's build, the app's own routes and pages, one router.

use crate::app::{App, Asset};
use crate::AppSource;
use axum::extract::{Path as AxumPath, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use std::future::Future;
use std::sync::Arc;

/// What the shell serves, beside the hub.
///
/// The routes [`router`] builds:
///
/// - `/` — the app's entrypoint; with no build, [`Self::root_fallback`]
///   (stamped) when given, else the "not built" page.
/// - `{mount}`, `{mount}/` — the entrypoint; with no build the "not built"
///   page (HTTP 200: a documented state, with build instructions, never an
///   error).
/// - `{mount}/{*path}` — a file of the build, by key; a miss is 404.
/// - each of [`Self::pages`], stamped, and [`Self::routes`] as given.
///
/// The entrypoint and the pages are stamped with `{{WS_PORT}}` (the hub's
/// WebSocket port), `{{TITLE}}` ([`Self::title`]) and [`Self::stamps`].
#[derive(Clone)]
pub struct ShellConfig {
    /// Where the app's build is read from.
    pub app: AppSource,
    /// The tab's title, stamped into `{{TITLE}}`.
    pub title: String,
    /// The path the build is served under, and its assets' prefix — what
    /// the frontend's build names them by (trunk's `public_url`, vite's
    /// `base`). Default `/app`; `/` serves the build's files at the root.
    pub mount: String,
    /// Placeholders beyond `{{WS_PORT}}` and `{{TITLE}}`: `(placeholder,
    /// value)`, replaced in order after those two.
    pub stamps: Vec<(String, String)>,
    /// Extra HTML pages: `(route, template)`, each stamped as the
    /// entrypoint is.
    pub pages: Vec<(String, String)>,
    /// The template served at `/` when there is no build (stamped); `None`
    /// serves the "not built" page there too.
    pub root_fallback: Option<String>,
    /// The "not built" page's HTML; `None` is [`not_built_page`].
    pub not_built: Option<String>,
    /// The app's own routes, merged in as they are.
    pub routes: Router,
}

impl ShellConfig {
    /// The build at `app` under `/app`, titled `title`, and nothing else.
    pub fn new(title: impl Into<String>, app: AppSource) -> Self {
        Self {
            app,
            title: title.into(),
            mount: "/app".into(),
            stamps: Vec::new(),
            pages: Vec::new(),
            root_fallback: None,
            not_built: None,
            routes: Router::new(),
        }
    }

    /// Every placeholder and its value, for a hub on `ws_port`: `{{WS_PORT}}`,
    /// `{{TITLE}}`, then [`Self::stamps`].
    pub fn stamps_for(&self, ws_port: u16) -> Vec<(String, String)> {
        let mut all = vec![("{{WS_PORT}}".to_string(), ws_port.to_string()), ("{{TITLE}}".to_string(), self.title.clone())];
        all.extend(self.stamps.iter().cloned());
        all
    }
}

impl std::fmt::Debug for ShellConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellConfig")
            .field("app", &self.app)
            .field("title", &self.title)
            .field("mount", &self.mount)
            .field("stamps", &self.stamps)
            .field("pages", &self.pages.iter().map(|(route, _)| route).collect::<Vec<_>>())
            .field("root_fallback", &self.root_fallback.is_some())
            .finish_non_exhaustive()
    }
}

/// The default "not built" page: the app's build output is missing, and
/// the server itself is fine.
pub fn not_built_page(title: &str) -> String {
    let title = title.replace('&', "&amp;").replace('<', "&lt;");
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{title} — frontend not built</title>\
         <body style=\"font: 14px/1.6 monospace; padding: 2rem; max-width: 46rem\">\
         <h1 style=\"font-size:1.1rem\">frontend not built</h1>\
         <p>The app's build output is missing. Build the frontend, then reload.</p>\
         <p>The server itself is fine: its WebSocket is up.</p>"
    )
}

struct ShellState {
    app: App,
    root_fallback: Option<String>,
    not_built: String,
    mount: String,
}

/// The shell's router for a hub on `ws_port`: the app's routes, the pages
/// and [`ShellConfig::routes`]. Not yet hardened — [`serve_on`] does that,
/// so a caller can layer its own around the app's routes first (a session
/// check with `route_layer`, say) and merge public ones after.
pub async fn router(cfg: ShellConfig, ws_port: u16) -> Router {
    let stamps = cfg.stamps_for(ws_port);
    let mount = match cfg.mount.trim_end_matches('/') {
        "" => String::new(),
        m if m.starts_with('/') => m.to_string(),
        m => format!("/{m}"),
    };
    let state = Arc::new(ShellState {
        root_fallback: cfg.root_fallback.as_deref().map(|t| crate::stamp(t, &stamps)),
        not_built: cfg.not_built.clone().unwrap_or_else(|| not_built_page(&cfg.title)),
        app: App::new(cfg.app.clone(), stamps.clone()).await,
        mount: mount.clone(),
    });
    let mut r = Router::new().route("/", get(root_index));
    if mount.is_empty() {
        r = r.route("/{*path}", get(app_asset));
    } else {
        r = r
            .route(&mount, get(app_index))
            .route(&format!("{mount}/"), get(app_index))
            .route(&format!("{mount}/{{*path}}"), get(app_asset));
    }
    let mut r = r.with_state(state);
    for (route, template) in &cfg.pages {
        let page = crate::stamp(template, &stamps);
        r = r.route(route, get(move || async move { Html(page) }));
    }
    tracing::info!("App:       {} ({:?})", if mount.is_empty() { "/" } else { &mount }, cfg.app);
    r.merge(cfg.routes)
}

/// Serves `router` on a listener the caller bound (port 0 binds a free one,
/// learned before anything starts), [`harden`]ed, until `shutdown`
/// resolves.
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    if let Ok(addr) = listener.local_addr() {
        tracing::info!("Shell:     http://{addr}/");
    }
    // AUTH-22: with the peer's address, which a sign-in page's throttles
    // read (`ConnectInfo<SocketAddr>`).
    axum::serve(listener, router.layer(axum::middleware::from_fn(harden)).into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown)
        .await
}

/// Every answer the shell gives refuses to be framed, is read as the type
/// it says, and sends no path to another origin (menger's IDENTITY-38). A
/// response that set its own keeps it.
pub async fn harden(req: Request, next: axum::middleware::Next) -> Response {
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

fn asset_response(asset: &Asset) -> Response {
    ([(header::CONTENT_TYPE, asset.mime)], asset.bytes.clone()).into_response()
}

/// The root is the app; with no build, the fallback page (or "not built").
async fn root_index(State(st): State<Arc<ShellState>>) -> Response {
    match st.app.current().await.get("index.html") {
        Some(index) => asset_response(index),
        None => {
            tracing::debug!("no app build at {:?}: / serves the fallback", st.app.source);
            Html(st.root_fallback.clone().unwrap_or_else(|| st.not_built.clone())).into_response()
        }
    }
}

async fn app_index(State(st): State<Arc<ShellState>>) -> Response {
    match st.app.current().await.get("index.html") {
        Some(index) => asset_response(index),
        None => Html(st.not_built.clone()).into_response(),
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
            tracing::debug!("app asset served: {}/{key} ({} bytes)", st.mount, asset.bytes.len());
            asset_response(asset)
        }
        // The entrypoint missing means "not built"; a missing asset next
        // to a present index.html is an ordinary 404 (stale link, typo).
        None if key == "index.html" => Html(st.not_built.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "no such asset").into_response(),
    }
}
