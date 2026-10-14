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
//! SCAFFOLD-5: the table, the stamping, the app's routes, the hardening
//! (IDENTITY-38) and [`AppSource`] are `plexus-shell`'s. This module is
//! menger's part: the `/vanilla` page (and `/` falling back to it), the
//! favicon, the tab's title, the "not built" page's words, the embedded
//! table, and the login routes and checks layered around the crate's
//! router.

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

/// The day-one page, served at `/vanilla` and at `/` when there is no app
/// build (MFE-4); stamped as the app's entrypoint is (MFE-9, MFE-23).
const VANILLA: &str = include_str!("../web/index.html");

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
/// to it when there is no build (MFE-4). No other routes: the favicon and
/// the login routes are added around it ([`favicon_routes`], [`serve_on`]).
pub fn app_shell(env: Option<&str>, app: AppSource) -> plexus_shell::ShellConfig {
    let mut cfg = plexus_shell::ShellConfig::new(tab_title(env), app);
    cfg.pages.push(("/vanilla".into(), VANILLA.into()));
    cfg.root_fallback = Some(VANILLA.into());
    cfg.not_built = Some(NOT_BUILT.into());
    cfg
}

/// MFE-23: the tab's icon, for both pages — public, never behind a session.
pub fn favicon_routes() -> Router {
    Router::new().route("/favicon.svg", get(favicon))
}

pub async fn serve(cfg: ShellConfig) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.port)).await?;
    serve_on(listener, cfg, std::future::pending()).await
}

/// DIST-3: [`serve`] on a listener the caller bound, until `shutdown`
/// resolves. `cfg.port` is the listener's port.
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    cfg: ShellConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let app = router(cfg).await;
    // IDENTITY-38: plexus-shell's `serve_on` hardens every answer.
    plexus_shell::serve_on(listener, app, shutdown).await?;
    Ok(())
}

async fn router(cfg: ShellConfig) -> Router {
    tracing::info!("App:       http://127.0.0.1:{}/", cfg.port);
    tracing::info!("Vanilla:   http://127.0.0.1:{}/vanilla", cfg.port);
    // ACCESS-33: menger's own pages and the app's build — nothing else. The
    // `/rest/{*}` pass-through to the REST port went: the app has spoken
    // the WebSocket since MFE-9, and nothing else called it. IDENTITY-19:
    // behind a session when login is required — no anonymous view.
    let mut app = plexus_shell::router(app_shell(cfg.env.as_deref(), cfg.app), cfg.ws_port).await;
    if let Some(login) = cfg.login.as_ref().filter(|l| l.require) {
        app = app.route_layer(axum::middleware::from_fn_with_state(login.clone(), require_session));
    }
    let mut app = app.merge(favicon_routes());
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
        // DNS rebinding: a page on another name resolving here gets nothing.
        let hosts = Arc::new(allowed_hosts(cfg.port, &login));
        app = app.layer(axum::middleware::from_fn_with_state(hosts, host_is_ours));
        tracing::info!("Login:     http://127.0.0.1:{}/login", cfg.port);
    }
    app
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
/// from its opaque origin — public code and faces, no data.
fn is_public_asset(path: &str) -> bool {
    path.starts_with("/app/vendor/") || is_font(path)
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
    /// frame loads; nothing else of the app is.
    #[test]
    fn only_the_faces_and_the_vendored_code_are_public_assets() {
        assert!(is_public_asset("/app/fonts/geist-latin.woff2"));
        assert!(is_public_asset("/app/vendor/mermaid.min.js"));
        assert!(!is_public_asset("/app/index.html"));
        assert!(!is_public_asset("/app/fontsx"));
        assert!(!is_public_asset("/"));
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

    #[test]
    fn both_pages_carry_the_title_placeholder_and_the_icon() {
        for page in [VANILLA, include_str!("../frontend/index.html")] {
            assert!(page.contains("<title>{{TITLE}}</title>"));
            assert!(page.contains(r#"<link rel="icon" type="image/svg+xml" href="/favicon.svg">"#));
        }
        assert!(FAVICON.starts_with("<svg") && FAVICON.contains("viewBox"));
    }
}
