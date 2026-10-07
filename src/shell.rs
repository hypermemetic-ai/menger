//! Day-one web shell, plus (MFE-2) the Leptos app and its data plane.
//!
//! menger's rule (since 0.04): frontend and backend are online from the
//! first commit, so every noun is visible the day it is written. Four
//! surfaces share this one listener:
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
//! - `/app/` — the Leptos CSR app (MFE-2), served **from disk** out of
//!   `frontend/dist`. Still the app's own root: it is where its assets
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
//! - `/rest/{*}` — a same-origin reverse proxy to the local REST port.
//!   The REST transport emits no CORS headers (verified against
//!   plexus-transport 0.2.3 src/http/), so a browser page from this
//!   origin could not call :4457 directly; the proxy forwards verbatim —
//!   bodies stream both ways, so `tickets.watch` SSE flows through live.
//!   The REST surface stays the contract; this adds no data path of its
//!   own.

use axum::body::Body;
use axum::extract::{Path as AxumPath, Request, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use futures::StreamExt;
use http_body_util::BodyExt;
use std::path::PathBuf;
use std::sync::Arc;

type ProxyClient =
    hyper_util::client::legacy::Client<hyper_util::client::legacy::connect::HttpConnector, Body>;

pub struct ShellConfig {
    pub port: u16,
    pub ws_port: u16,
    /// REST port that `/rest/{*}` proxies to; `None` (--no-rest) makes the
    /// proxy answer 503 instead of dialing a dead upstream.
    pub rest_port: Option<u16>,
    /// Directory holding the Leptos app's build output.
    pub app_dist: PathBuf,
    /// MFE-23: the environment's name (`MENGER_ENV`), which titles the
    /// browser tab ([`tab_title`]).
    pub env: Option<String>,
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

struct ShellState {
    rest_port: Option<u16>,
    /// The day-one page, WS port already substituted. Served at
    /// `/vanilla`, and at `/` when there is no app build (MFE-4).
    vanilla: String,
    /// MFE-9: the app talks to the Plexus WebSocket directly, and a page
    /// served from disk cannot know which port that is. Substituted into
    /// the app's `index.html` on the way out, exactly as the vanilla shell
    /// has always had it substituted into `web/index.html` — one
    /// convention, not two.
    ws_port: u16,
    /// MFE-23: the tab's title, stamped into both pages.
    title: &'static str,
    app_dist: PathBuf,
    client: ProxyClient,
}

pub async fn serve(cfg: ShellConfig) -> anyhow::Result<()> {
    let title = tab_title(cfg.env.as_deref());
    let state = Arc::new(ShellState {
        rest_port: cfg.rest_port,
        vanilla: stamp(include_str!("../web/index.html"), cfg.ws_port, title),
        ws_port: cfg.ws_port,
        title,
        app_dist: cfg.app_dist,
        client: hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build_http(),
    });
    let app = Router::new()
        .route("/", get(root_index))
        .route("/vanilla", get(vanilla))
        .route("/favicon.svg", get(favicon))
        .route("/app", get(app_index))
        .route("/app/", get(app_index))
        .route("/app/{*path}", get(app_asset))
        .route("/rest/{*path}", any(proxy_rest))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.port)).await?;
    tracing::info!("App:       http://127.0.0.1:{}/", cfg.port);
    tracing::info!("Vanilla:   http://127.0.0.1:{}/vanilla", cfg.port);
    axum::serve(listener, app).await?;
    Ok(())
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

/// The app's entrypoint with the WebSocket port substituted, or `None`
/// when `frontend/dist` holds no build. Separate from [`serve_dist_file`]
/// because the two callers differ on what "missing" means: `/app/` says
/// so (MFE-2), `/` falls back to the vanilla page (MFE-4).
async fn app_entrypoint(dist: &std::path::Path, ws_port: u16, title: &str) -> Option<String> {
    let bytes = tokio::fs::read(dist.join("index.html")).await.ok()?;
    // MFE-9: the entrypoint carries the WebSocket port. The app's
    // index.html declares it as `{{WS_PORT}}` (see frontend/index.html);
    // every other asset is served byte-exact.
    Some(stamp(&String::from_utf8(bytes).ok()?, ws_port, title))
}

/// MFE-4: the root is the app. No build output on disk is not an error
/// here — it is the day-one page, which needs no build and works.
async fn root_index(State(st): State<Arc<ShellState>>) -> Response {
    match app_entrypoint(&st.app_dist, st.ws_port, st.title).await {
        Some(html) => Html(html).into_response(),
        None => {
            tracing::debug!("no app build at {:?}: / degrades to the vanilla page", st.app_dist);
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
    match app_entrypoint(&st.app_dist, st.ws_port, st.title).await {
        Some(html) => Html(html).into_response(),
        None => not_built(),
    }
}

async fn app_asset(
    State(st): State<Arc<ShellState>>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    serve_dist_file(&st.app_dist, &path, st.ws_port, st.title).await
}

async fn serve_dist_file(dist: &std::path::Path, rel: &str, ws_port: u16, title: &str) -> Response {
    // No traversal: the wildcard must stay inside dist.
    if rel.split(['/', '\\']).any(|c| c == "..") || rel.contains('\0') {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }
    let with_index;
    let rel = if rel.is_empty() || rel.ends_with('/') {
        with_index = format!("{rel}index.html");
        with_index.as_str()
    } else {
        rel
    };
    match tokio::fs::read(dist.join(rel)).await {
        Ok(bytes) => {
            // An access line per served asset (RUST_LOG=menger=debug): the
            // one place to see which assets a page actually pulled — AGS-14
            // pins that the mermaid bundle is fetched only by pages that
            // hold a diagram.
            tracing::debug!("app asset served: /app/{rel} ({} bytes)", bytes.len());
            // MFE-9: the entrypoint carries the WebSocket port. The app's
            // index.html declares it as `{{WS_PORT}}` (see
            // frontend/index.html); every other asset is served byte-exact.
            let bytes = if rel == "index.html" {
                match String::from_utf8(bytes) {
                    Ok(text) => stamp(&text, ws_port, title).into_bytes(),
                    Err(e) => e.into_bytes(),
                }
            } else {
                bytes
            };
            let mime = match rel.rsplit('.').next() {
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
            };
            ([(header::CONTENT_TYPE, mime)], bytes).into_response()
        }
        // The entrypoint missing means "not built"; a missing asset next
        // to a present index.html is an ordinary 404 (stale link, typo).
        Err(_) if rel == "index.html" => not_built(),
        Err(_) => (StatusCode::NOT_FOUND, "no such asset").into_response(),
    }
}

async fn proxy_rest(State(st): State<Arc<ShellState>>, mut req: Request) -> Response {
    let Some(port) = st.rest_port else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "REST transport disabled (--no-rest): the /rest proxy has no upstream",
        )
            .into_response();
    };
    let path_q = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_owned())
        .unwrap_or_else(|| "/".into());
    let uri: Uri = match format!("http://127.0.0.1:{port}{path_q}").parse() {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, "bad proxy path").into_response(),
    };
    *req.uri_mut() = uri;
    // Let the client re-derive Host for the upstream authority.
    req.headers_mut().remove(header::HOST);
    match st.client.request(req).await {
        // Unary JSON passes through untouched; SSE streams through
        // unbuffered with one addition: a leading SSE *comment* frame.
        // Firefox does not resolve a `fetch()` for a chunked response
        // until the first body bytes arrive (verified live: the promise
        // resolved only when the first watch event was pushed), and
        // `tickets.watch` sends no preamble of its own — without this, a
        // quiet stream leaves the app stuck at "connecting…". A `:` line
        // is spec-defined as ignorable, so no event is added or altered;
        // the REST surface stays the contract.
        Ok(resp) => {
            let is_sse = resp
                .headers()
                .get(header::CONTENT_TYPE)
                .is_some_and(|v| v.as_bytes().starts_with(b"text/event-stream"));
            if is_sse {
                let (parts, body) = resp.into_parts();
                let preamble = futures::stream::once(async {
                    Ok(hyper::body::Bytes::from_static(b":proxy stream open\n\n"))
                });
                let stream = preamble.chain(BodyExt::into_data_stream(body));
                Response::from_parts(parts, Body::from_stream(stream)).into_response()
            } else {
                resp.map(Body::new).into_response()
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("REST upstream unreachable: {e}"),
        )
            .into_response(),
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
