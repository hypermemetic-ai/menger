//! The web shell: the login routes, and the app's build when there is one.
//!
//! v2: the frontend is deleted (tag v0.0.1 holds v1's Leptos app, the
//! `/vanilla` page and the body frame). Pages come back one at a time, from
//! the mockups on their tickets. Until then:
//!
//! - `/` — the live UI's entrypoint, as `/app/` (UI-13); [`PLACEHOLDER`] with no version live;
//! - `/app/` — the app's build in `frontend/dist` read into a table
//!   (ACCESS-33: a request path is a key into it, never a path on disk), or
//!   the "not built" page (200) while there is none;
//! - the login routes (IDENTITY-19), unchanged — the auth surface is the
//!   next thing v2 reworks;
//! - V2-24: `/media/<hash>` — an image a body shows, from the media service
//!   asked as the request's session ([`media_get`]), behind the session as
//!   the ui routes are; content-addressed, so kept forever.
//! - V2-50 (TICKETS-36 on v2): a path no route claims that names a ticket
//!   goes to its route in the app — the link Copy link gives,
//!   `/t/<uid>/<ID>-<slug>`, to `/app/#/t/<uid>/…`; a ticket's name at the
//!   root (`/PERF-19`, `/hypermemetic:PLEXUS/PERF-19`) to
//!   `/app/#/ticket/<name>`, which the app resolves (or lists, when it fits
//!   several) — `302`, behind the session as `/app/` is
//!   ([`ticket_redirect`]). Every other path is `404` as before.
//!
//! SCAFFOLD-5: the table, the stamping, the app's routes, the hardening
//! (IDENTITY-38) and [`AppSource`] are `plexus-webapp`'s; this module is
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

/// DIST-3: where the Leptos app's build is served from — `plexus-webapp`'s.
pub use plexus_webapp::AppSource;

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
    /// UI-3: the ui service's store — `/` and `/app/` serve the live
    /// version's entrypoint (a session's preview to that session),
    /// `/app/<hash>/…` a version's files: the framework's routes
    /// (FRAME-5, `plexus_ui_service::routes::UiRoutes`), mounted here with
    /// menger's session, stamps, policy and [`PLACEHOLDER`]. None: the build
    /// in [`Self::app`], as before.
    pub ui: Option<Arc<plexus_ui_service::UiStore>>,
    /// STUDIO-2: the studio at `/studio/` (the framework's own bundle, kept
    /// as `bundle:studio`), beside `/app/` — for a session [`Studio::may`]
    /// admits, with the compiler module it compiles in. None: no studio.
    pub studio: Option<Studio>,
    /// V2-24, EDITOR-10: `GET /media/<hash>` — the media service asked
    /// `media.info` as the request's session, the bytes from its blob
    /// store ([`crate::media_route`]). None: no route.
    pub media: Option<crate::media_route::MediaServe>,
    /// TRANSPORT-15 (V2-43): the MCP endpoint's router — `/mcp` and, under
    /// OAuth, its RFC 9728 document — on the shell's own origin, so the edge
    /// needs no route of its own. Public, like the metadata: the bridge's
    /// identify step names the caller from the request's Bearer and refuses
    /// with a 401 of its own; the session check never sends it to the login
    /// page.
    pub mcp: Option<Router>,
}

/// V2-24: what `GET /media/<hash>` is served under — the bytes never change
/// under their hash (the framework's [`IMMUTABLE`]); a miss is never kept.
pub const MEDIA_ROUTE: &str = "/media/{hash}";

/// EDITOR-14, COMPS-2: **a full page in a body** (an `<m-mockup>`, a
/// ```` ```mockup ```` fence, an html page) is drawn in a frame — the
/// framework's (`plexus_ui_service::frame`): the ui routes serve its
/// document at [`FRAME_ROUTE`] under its own policy, and the engine's
/// `bus.frame` draws it (COMPS-3: ui/src/m-mockup.svelte, m-live, m-page). Public: the same bytes
/// for everyone, no data in them — [`is_public_asset`] lets it through.
pub use plexus_ui_service::frame::{FRAME_CSP, FRAME_ROUTE};

/// STUDIO-2: who opens `/studio/`, and the module the studio compiles in.
#[derive(Clone)]
pub struct Studio {
    /// Whether a session's identity may open it (menger: the framework's
    /// `routes::may_author` over the service's gate — `ui.author` held on `*`).
    pub may: Arc<dyn Fn(&plexus_core::plexus::AuthContext) -> bool + Send + Sync>,
    /// `--ui-compiler`'s bytes, served at `/studio/ui-compiler.wasm`.
    pub compiler: Option<Vec<u8>>,
}

/// UI-3: a version's files never change under its hash — kept a year
/// (the framework's, FRAME-5).
pub use plexus_ui_service::routes::{IMMUTABLE, NO_STORE};

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

/// ACCESS-28: the app's pages' Content-Security-Policy (plexus-webapp fills
/// `{{SCRIPTS}}` with the page's inline-script hashes and `{{SERVICE}}` with the
/// service's WebSocket origins at the name the page was asked for):
///
/// - scripts: the app's files, the wasm, the hashed inline blocks — no
///   other inline script, no other origin;
/// - styles `'unsafe-inline'` (the app writes `style` and `<style>`: the
///   theme blocks) and `https:` with fonts — a theme may `@import` a face's
///   sheet (THEME-3: `@import url("https://…")`);
/// - images: its own and `data:` — a remote image in a body shown in the
///   app is refused, so a body cannot learn who reads it (ACCESS-4 M5);
///   EDITOR-10: a pasted page's images are fetched and kept, never hot-linked;
/// - media (video, audio): its own (`/media/<hash>`) and `blob:` alone;
/// - connections: itself, its service, and a WebSocket on loopback — MFE-21's
///   settings point the app at another menger on this machine;
/// - frames: its own (a PDF from `/media/<hash>` in the browser's viewer)
///   and, EDITOR-10, the four providers' players a link card draws —
///   [`menger_media::unfurl::EMBED_ORIGINS`], each by its exact origin;
///   nothing frames it;
/// - objects: none (a PDF is framed, never an `<object>`).
///
/// No `form-action`: the sign-out form's answer may go on to a provider's
/// own sign-out page (IDENTITY-30), which the browser checks against it.
pub const PAGE_CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval' {{SCRIPTS}}; \
style-src 'self' 'unsafe-inline' https:; font-src 'self' data: https:; img-src 'self' data: blob:; \
media-src 'self' blob:; connect-src 'self' {{SERVICE}} ws://127.0.0.1:* ws://localhost:*; \
frame-src 'self' https://www.youtube-nocookie.com https://player.vimeo.com https://platform.twitter.com https://www.figma.com; \
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
        csp = csp.replace("frame-src 'self' ", &format!("frame-src 'self' {live} "));
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

/// menger's shell as plexus-webapp serves it: the app under `/app/` (where
/// `Trunk.toml`'s `public_url` puts its assets, and MFE-1's deep links),
/// titled for the environment (MFE-23), and `/` falling back to
/// [`PLACEHOLDER`] when there is no build (MFE-4). No other routes: the
/// login routes are added around it ([`serve_on`]).
pub fn app_shell(env: Option<&str>, app: AppSource) -> plexus_webapp::ShellConfig {
    app_shell_framed(env, app, &Framing::default())
}

/// [`app_shell`] under APP-110's [`Framing`]: the policy as [`page_csp`] says,
/// and the app told which test app its bodies may draw (`{{LIVE_ORIGIN}}`,
/// empty when none).
pub fn app_shell_framed(env: Option<&str>, app: AppSource, framing: &Framing) -> plexus_webapp::ShellConfig {
    let mut cfg = plexus_webapp::ShellConfig::new(tab_title(env), app);
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
    // IDENTITY-38: plexus-webapp's `serve_on` hardens every answer.
    plexus_webapp::serve_on(listener, app, shutdown).await?;
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
    let mut app = plexus_webapp::router(shell, cfg.ws_port).await;
    // UI-3, FRAME-5: `/`, `/app/` and `/app/*` from the ui store — the
    // framework's routes, inside the session check (mounted first, so it
    // runs after it). The login's session names whose preview to serve.
    if let Some(store) = cfg.ui.clone() {
        let login = cfg.login.clone();
        let routes = plexus_ui_service::routes::UiRoutes::new(store, cfg.ws_port)
            .session(move |headers| login.as_ref().and_then(|l| l.session(headers)))
            .stamps(stamps)
            .csp(csp)
            .placeholder(PLACEHOLDER);
        // STUDIO-2: /studio/ beside /app/, inside the same session check.
        let routes = match cfg.studio.clone() {
            Some(s) => {
                let may = s.may.clone();
                let routes = routes.studio(move |ctx| may(ctx));
                match s.compiler {
                    Some(bytes) => routes.compiler_module(bytes),
                    None => routes,
                }
            }
            None => routes,
        };
        app = routes.mount(app);
    }
    // V2-24: `/media/<hash>` from the media service, as the session —
    // inside the session check below, like the ui routes.
    if let Some(serve) = cfg.media.clone() {
        app = app.merge(Router::new().route(MEDIA_ROUTE, get(crate::media_route::get)).with_state((serve, cfg.login.clone())));
    }
    if let Some(login) = cfg.login.as_ref().filter(|l| l.require) {
        app = app.route_layer(axum::middleware::from_fn_with_state(login.clone(), require_session));
    }
    if let Some(metadata) = cfg.metadata {
        app = app.merge(metadata);
    }
    // TRANSPORT-15 (V2-43): `/mcp` beside the metadata — after the session
    // check, which must not redirect an MCP client to the login page.
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
    // V2-50 (TICKETS-36): a path no route claims that names a ticket —
    // `/t/<uid>/…`, `/PERF-19` — goes to its route in the app; any other is
    // 404 as before.
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

/// V2-50 (main's TICKETS-36): where a path that names a ticket goes in the
/// app — `None` for any other path, which is answered as it was.
///
/// - `/t/<uid>[/…]` — the shareable link (Copy link): the app's route by
///   uid, `/app/#/t/<uid>/…`, the tail kept as written (the app reads the
///   uid alone and rewrites a stale tail);
/// - `/<name>` — a ticket's id (`/PERF-19`), a short form of its address
///   (`/PLEXUS/PERF-19`) or the whole of it (`/hypermemetic:PLEXUS/PERF-19`):
///   `/app/#/ticket/<name>`, which the app resolves — to the ticket's own
///   route, or to the collision page when the name fits several.
///
/// The address is read as the API reads a ticket's name, so a shortcode
/// (`/PERF`), a bare word (`/ws`, `/robots.txt`) or a bare uid names no
/// ticket here. Nothing is looked up: whether the ticket exists, and whether
/// the reader may see it, is the app's `get` to say.
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

/// V2-50: the shell's answer to a path no route claims — the app's route
/// for a ticket the path names ([`ticket_redirect`], `302`), behind the
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

/// IDENTITY-38 / DESIGN-40: the app's files a ticket body's frame loads
/// from its opaque origin — public code and faces, no data. COMPS-2: and
/// the frame's own document (`/frame`, the framework's). APP-64: and the
/// home-screen icons and manifest, at the root and under `/app/`. APP-92:
/// and the app's code itself — its script, wasm and stylesheet, every file
/// of the build but a page — so it downloads behind the sign-in page; the
/// entrypoint stays behind the session.
fn is_public_asset(path: &str) -> bool {
    let root_file = |p: &str| ROOT_FILES.contains(&p);
    path.starts_with("/app/vendor/")
        || path == FRAME_ROUTE
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

    /// V2-50 (main's TICKETS-36): a link by uid, `/t/<uid>[/…]`, goes to the
    /// app's route by uid, its tail kept; a path that names a ticket — its
    /// id, a short form, its full address — goes to `#/ticket/<name>`, which
    /// the app resolves (or lists, when it fits several). Every other path is
    /// answered as it was: no redirect.
    #[test]
    fn a_path_that_names_a_ticket_goes_to_the_app() {
        let to = |p: &str| ticket_redirect(p);
        assert_eq!(to("/t/7k3mq2xa"), Some("/app/#/t/7k3mq2xa".into()));
        assert_eq!(to("/t/7k3mq2xa/"), Some("/app/#/t/7k3mq2xa".into()));
        assert_eq!(to("/t/7k3mq2xa/PERF-19-compression-on-the-websocket"), Some("/app/#/t/7k3mq2xa/PERF-19-compression-on-the-websocket".into()));
        assert_eq!(to("/t/7K3MQ2XA/x"), Some("/app/#/t/7k3mq2xa/x".into()), "a uid reads case-blind");
        assert_eq!(to("/t/7k3mq2xa/a<b"), Some("/app/#/t/7k3mq2xa".into()), "a tail of other characters is dropped");
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
            "/manifest.webmanifest", "/favicon.svg", "/robots.txt", "/PERF", "/MENGER/STORE", "/t/", "/t",
            "/t/not-a-uid/x", "/t/7k3mq2x", "/7k3mq2xa", "//evil.example/PERF-1", "/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/PERF-1x",
            "/PERF-19\\x", "/%2e%2e/PERF-1", "/PERF-1%0a", "/media/abc", "/studio/",
        ] {
            assert_eq!(to(other), None, "{other}");
        }
    }

    /// EDITOR-14, COMPS-2: the frames' document is the framework's, and
    /// public here — a frame's opaque origin carries no session — while
    /// the app's own policy is not loosened: the frame is its own origin's,
    /// which `frame-src 'self'` already admits.
    #[test]
    fn the_frame_document_is_the_frameworks_and_public() {
        assert_eq!(FRAME_ROUTE, "/frame");
        assert!(is_public_asset(FRAME_ROUTE) && !is_public_asset("/frame/x") && !is_public_asset("/app/"));
        assert!(FRAME_CSP.contains("connect-src 'none'") && !FRAME_CSP.contains("allow-same-origin"), "{FRAME_CSP}");
        assert!(PAGE_CSP.contains("frame-src 'self' ") && !PAGE_CSP.contains("'unsafe-eval'"));
    }

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

    /// EDITOR-10: the page frames its own documents and the four
    /// providers' players by their exact origins — the ones a card's
    /// `embed` is built on — and nothing broader; media from itself.
    #[test]
    fn the_policy_frames_the_players_by_origin_and_nothing_broader() {
        let frame = PAGE_CSP.split(';').map(str::trim).find(|d| d.starts_with("frame-src")).unwrap();
        let mut named: Vec<&str> = frame.split_whitespace().skip(1).collect();
        named.sort();
        let mut want = vec!["'self'"];
        want.extend(menger_media::unfurl::EMBED_ORIGINS);
        want.sort();
        assert_eq!(named, want);
        assert!(!frame.split_whitespace().any(|w| w == "https:" || w == "*" || w.contains('*')), "no scheme-wide source or wildcard in frames");
        assert!(PAGE_CSP.contains("media-src 'self' blob:;") && PAGE_CSP.contains("object-src 'none'") && PAGE_CSP.contains("img-src 'self' data: blob:;"));
    }

    /// APP-110: unconfigured, the policy is [`PAGE_CSP`] to the byte — frames
    /// only its own, framed by no one. Configured, exactly two directives move,
    /// each by the origins named, and nothing else changes.
    #[test]
    fn the_page_policy_moves_only_what_framing_names() {
        assert_eq!(page_csp(&Framing::default()), PAGE_CSP);
        assert!(page_csp(&Framing::default()).contains("frame-ancestors 'none'"));
        let tracker = page_csp(&Framing { ancestors: vec![], live: Some("https://app-test.menger.sh".into()) });
        assert!(tracker.contains("frame-src 'self' https://app-test.menger.sh https://www.youtube-nocookie.com") && tracker.contains("frame-ancestors 'none'"), "{tracker}");
        let test = page_csp(&Framing { ancestors: vec!["https://app.menger.sh".into(), "http://127.0.0.1:4456".into()], live: None });
        assert!(test.contains("frame-ancestors https://app.menger.sh http://127.0.0.1:4456") && test.contains("frame-src 'self' https://www.youtube-nocookie.com"), "{test}");
        // Nothing else moved.
        assert_eq!(test.replace("frame-ancestors https://app.menger.sh http://127.0.0.1:4456", "frame-ancestors 'none'"), PAGE_CSP);
        assert_eq!(tracker.replace("frame-src 'self' https://app-test.menger.sh ", "frame-src 'self' "), PAGE_CSP);
    }

    /// v2: the placeholder is stamped as the app's entrypoint is, and is
    /// what `/` answers with no build.
    #[test]
    fn the_placeholder_is_stamped_and_is_the_root_fallback() {
        let cfg = app_shell(Some("test"), AppSource::Dir("/nowhere".into()));
        let stamped = plexus_webapp::stamp(PLACEHOLDER, &cfg.stamps_for(4450));
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
        assert_eq!(plexus_webapp::stamp("{{LIVE_ORIGIN}}", &on.stamps_for(4450)), "https://t.example");
        assert_eq!(plexus_webapp::stamp("[{{LIVE_ORIGIN}}]", &app_shell(None, AppSource::Dir("/nowhere".into())).stamps_for(4450)), "[]");
        assert!(on.csp.contains("frame-src 'self' https://t.example"));
    }

}

