//! Day-one web shell, plus (MFE-2) the Leptos app and its data plane.
//!
//! menger's rule (since 0.04): frontend and backend are online from the
//! first commit, so every noun is visible the day it is written. Three
//! surfaces share this one listener:
//!
//! - `/` — the vanilla page, embedded at compile time, speaking the
//!   Plexus wire protocol over WebSocket. Unchanged since day one.
//! - `/app/` — the Leptos CSR app (MFE-2), served **from disk** out of
//!   `frontend/dist`. The frontend is deliberately not part of this
//!   crate's build: assets are an input to *serving*, so `cargo build`
//!   and `cargo test` never need the wasm toolchain. When the output is
//!   missing the URL answers with a plain "frontend not built" page
//!   (HTTP 200 — a deliberate state with build instructions, not an
//!   error), never a crash or a 500.
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
}

/// Where `trunk build` puts the app when run from a checkout. Overridable
/// via `--app-dist` / `MENGER_APP_DIST` for deployments that move dist.
pub fn default_app_dist() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend/dist")
}

struct ShellState {
    rest_port: Option<u16>,
    app_dist: PathBuf,
    client: ProxyClient,
}

pub async fn serve(cfg: ShellConfig) -> anyhow::Result<()> {
    let html = include_str!("../web/index.html").replace("{{WS_PORT}}", &cfg.ws_port.to_string());
    let state = Arc::new(ShellState {
        rest_port: cfg.rest_port,
        app_dist: cfg.app_dist,
        client: hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build_http(),
    });
    let app = Router::new()
        .route("/", get(move || std::future::ready(Html(html.clone()))))
        .route("/app", get(app_index))
        .route("/app/", get(app_index))
        .route("/app/{*path}", get(app_asset))
        .route("/rest/{*path}", any(proxy_rest))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.port)).await?;
    tracing::info!("Shell:     http://127.0.0.1:{}/", cfg.port);
    tracing::info!("App:       http://127.0.0.1:{}/app/", cfg.port);
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
        "The server itself is fine — the vanilla shell is at <a href=\"/\">/</a>.</p>",
    ))
    .into_response()
}

async fn app_index(State(st): State<Arc<ShellState>>) -> Response {
    serve_dist_file(&st.app_dist, "index.html").await
}

async fn app_asset(
    State(st): State<Arc<ShellState>>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    serve_dist_file(&st.app_dist, &path).await
}

async fn serve_dist_file(dist: &std::path::Path, rel: &str) -> Response {
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
            let mime = match rel.rsplit('.').next() {
                Some("html") => "text/html; charset=utf-8",
                Some("js") => "text/javascript",
                Some("wasm") => "application/wasm",
                Some("css") => "text/css",
                Some("json") | Some("map") => "application/json",
                Some("svg") => "image/svg+xml",
                Some("ico") => "image/x-icon",
                Some("txt") => "text/plain; charset=utf-8",
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
