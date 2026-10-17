//! MFE-2 shell serving, black-box: spawns the built `menger` binary with
//! the web shell enabled on a free port and speaks plain HTTP to it —
//! exactly what a browser does. Covers the ticket's serving guarantees
//! without requiring the frontend toolchain:
//!
//! - `/app/` with no build output answers a clear "not built" page (200),
//!   never a crash or a 500 (criterion 4)
//! - `/app/` with build output on disk serves it with sane content types
//!   (a synthetic dist — real trunk output is criterion 1, verified live)
//! - ACCESS-33: nothing else — `/app/` answers only the build the server
//!   read (a request path is never a path on disk), and there is no
//!   `/rest` pass-through to the REST port

#![cfg(unix)]

/// ACCESS-24: the test provider — the shell is behind login, always.
mod support;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::{header, Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::net::TcpStream;

/// A port no other test in this binary has been handed (MNG-19). Binding
/// `:0` and dropping the listener only *suggests* a port: the kernel can
/// suggest the same one to a second test before the first test's server
/// has bound it — one server then fails to start and both tests talk to
/// the other, each seeing a store that is not its own.
async fn free_port() -> u16 {
    static HANDED: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut handed = HANDED.lock().unwrap();
        if !handed.contains(&port) {
            handed.push(port);
            return port;
        }
    }
}

/// ACCESS-24: each shell's session cookie (`menger_session_<port>=<token>`,
/// minted for the test's operator), sent with every request to it.
fn cookies() -> &'static std::sync::Mutex<std::collections::HashMap<u16, String>> {
    static C: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u16, String>>> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

/// A spawned `menger` with the shell on a free port. Dropping SIGKILLs.
struct Server {
    child: Child,
    shell_port: u16,
    _dir: tempfile::TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Spawns the binary with shell + (optionally) REST on free ports, the
/// app dist pointed at `app_dist` (hermetic — never the checkout's real
/// frontend/dist, whose built-ness must not sway a test), and waits for
/// every listener.
///
/// Free ports are picked by bind-and-release, so two concurrently
/// starting tests can race for the same port; the loser exits at bind —
/// and because the shell listener starts before the WS/REST binds, it
/// can die *after* the shell port already answered. So: wait for shell
/// AND REST, confirm the child is still alive, and on any startup death
/// retry from scratch with fresh ports.
async fn spawn_server(app_dist: &Path) -> Server {
    spawn_server_in(app_dist, None).await
}

/// MFE-23: a server that believes it is the named environment.
async fn spawn_server_in(app_dist: &Path, env: Option<&str>) -> Server {
    for attempt in 0..3 {
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let ws_port = free_port().await;
        let shell_port = free_port().await;
        let db: PathBuf = dir.path().join("tickets.db");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_menger"));
        cmd.args(support::login_args());
        cmd.arg("--port")
            .arg(ws_port.to_string())
            .arg("--shell-port")
            .arg(shell_port.to_string())
            .arg("--app-dist")
            .arg(app_dist)
            .arg("--db-path")
            .arg(&db)
            .env_remove("MENGER_DB_PATH")
            .env_remove("MENGER_APP_DIST")
            .env_remove("RUST_LOG")
            .env_remove("MENGER_ENV")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(env) = env {
            cmd.arg("--env").arg(env);
        }
        let child = cmd.spawn().expect("spawn menger");

        let mut server = Server {
            child,
            shell_port,
            _dir: dir,
        };
        for _ in 0..100 {
            if server.child.try_wait().expect("try_wait").is_some() {
                break; // died at startup (port race) — retry with fresh ports
            }
            let shell_up = TcpStream::connect(("127.0.0.1", server.shell_port))
                .await
                .is_ok();
            // ACCESS-46: the WebSocket listener is a TCP listener too — probe
            // it, so the loser of a port race (it dies after the shell
            // answered) is never trusted; then a beat long enough under a
            // loaded machine for a failed bind to take the process down.
            let ws_up = TcpStream::connect(("127.0.0.1", ws_port)).await.is_ok();
            if shell_up && ws_up {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if server.child.try_wait().expect("try_wait").is_none() {
                    let token = support::mint_session(ws_port).await;
                    cookies().lock().unwrap().insert(shell_port, format!("menger_session_{shell_port}={token}"));
                    return server;
                }
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        drop(server); // kills a still-live child that never got healthy
        eprintln!("menger startup attempt {attempt} failed; retrying with fresh ports");
    }
    panic!("menger failed to start after 3 attempts");
}

async fn connect(port: u16) -> http1::SendRequest<Full<Bytes>> {
    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (sender, conn) = http1::handshake(TokioIo::new(stream)).await.unwrap();
    tokio::spawn(conn);
    sender
}

fn request(port: u16, method: Method, path: &str, body: &str) -> Request<Full<Bytes>> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, format!("127.0.0.1:{port}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, cookies().lock().unwrap().get(&port).cloned().unwrap_or_default())
        .body(Full::new(Bytes::from(body.to_owned())))
        .unwrap()
}

/// Send and drain: (status, content-type, body bytes).
async fn fetch(port: u16, method: Method, path: &str, body: &str) -> (StatusCode, String, Bytes) {
    let mut sender = connect(port).await;
    let resp = sender
        .send_request(request(port, method, path, body))
        .await
        .expect("send request");
    let status = resp.status();
    let ctype = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, ctype, bytes)
}

/// Criterion 4: nothing built — the app URL answers a clear "not built"
/// indication, not a crash and not a 500. Assets 404 rather than lying.
#[tokio::test]
async fn app_url_without_build_output_says_not_built() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let s = spawn_server(empty.path()).await;

    for path in ["/app", "/app/"] {
        let (status, ctype, body) = fetch(s.shell_port, Method::GET, path, "").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{path}: deliberate state, not an error"
        );
        assert!(ctype.starts_with("text/html"), "{path}: got {ctype}");
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("frontend not built"),
            "{path}: names the state, got:\n{text}"
        );
        assert!(
            text.contains("trunk build"),
            "{path}: tells the reader how to fix it"
        );
        assert!(
            text.contains("/vanilla"),
            "{path}: points at the page that does work"
        );
    }
    let (status, _, _) = fetch(s.shell_port, Method::GET, "/app/missing.js", "").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "missing asset is a plain 404"
    );
}

/// With output on disk, /app/ serves it: index at the root, assets with
/// correct content types, and traversal out of dist rejected.
#[tokio::test]
async fn app_url_serves_dist_from_disk() {
    let dist = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    std::fs::write(
        dist.path().join("index.html"),
        "<html>menger app stub</html>",
    )
    .unwrap();
    std::fs::write(dist.path().join("app.js"), "console.log('stub')").unwrap();
    std::fs::write(dist.path().join("app_bg.wasm"), b"\0asm").unwrap();
    let s = spawn_server(dist.path()).await;

    let (status, ctype, body) = fetch(s.shell_port, Method::GET, "/app/", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ctype.starts_with("text/html"));
    assert!(String::from_utf8_lossy(&body).contains("menger app stub"));

    let (status, ctype, _) = fetch(s.shell_port, Method::GET, "/app/app.js", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ctype, "text/javascript");

    let (status, ctype, _) = fetch(s.shell_port, Method::GET, "/app/app_bg.wasm", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ctype, "application/wasm");

    let (status, _, _) = fetch(s.shell_port, Method::GET, "/app/../Cargo.toml", "").await;
    assert_ne!(
        status,
        StatusCode::OK,
        "traversal out of dist must not serve"
    );
}

/// MNG-22: an absolute path in the wildcard — `/app//abs` or its
/// percent-encoded `/app/%2Fabs`, both decoded to `/abs` — once replaced
/// dist outright (`dist.join("/abs")` is `/abs`) and served any file the
/// process could read. A secret beside dist, by its absolute path, is
/// refused; dist itself still serves.
#[tokio::test]
async fn app_refuses_an_absolute_path_out_of_dist() {
    let root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let dist = root.path().join("dist");
    std::fs::create_dir(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html>stub</html>").unwrap();
    std::fs::write(dist.join("app.js"), "console.log('stub')").unwrap();
    let secret = root.path().join("secret.txt");
    std::fs::write(&secret, "the secret").unwrap();
    let secret = secret.canonicalize().unwrap();
    let abs = secret.to_str().unwrap();
    assert!(abs.starts_with('/'));
    let s = spawn_server(&dist).await;

    let encoded = abs.replace('/', "%2F");
    for path in [format!("/app/{abs}"), format!("/app/{encoded}"), format!("/app/%2E%2E/secret.txt")] {
        let (status, _, body) = fetch(s.shell_port, Method::GET, &path, "").await;
        assert_ne!(status, StatusCode::OK, "{path} must not serve");
        assert!(!String::from_utf8_lossy(&body).contains("the secret"), "{path} leaked the file");
    }
    let (status, _, _) = fetch(s.shell_port, Method::GET, "/app/app.js", "").await;
    assert_eq!(status, StatusCode::OK, "dist still serves");
}

/// AGS-14: the vendored mermaid bundle is a multi-megabyte file under a
/// `vendor/` subdirectory of dist (trunk copies `frontend/vendor/mermaid/`
/// there). The wildcard route must serve a nested path, as
/// `text/javascript`, byte-exact — the browser lazy-loads it by URL, so a
/// truncated or mistyped body would fail every diagram. Synthetic dist:
/// the served file is an input to serving, never a compile input.
#[tokio::test]
async fn app_serves_nested_vendor_asset_whole() {
    let dist = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    std::fs::write(dist.path().join("index.html"), "<html>stub</html>").unwrap();
    std::fs::create_dir(dist.path().join("vendor")).unwrap();
    // ~3 MiB, the order of the real bundle; a repeating pattern whose
    // boundaries would reveal any dropped or duplicated chunk.
    const PATTERN: &[u8] = b"mermaid.initialize({securityLevel:'strict'});\n";
    let bundle: Vec<u8> = (0..3 * 1024 * 1024)
        .map(|i| PATTERN[i % PATTERN.len()])
        .collect();
    std::fs::write(dist.path().join("vendor/mermaid-1.2.3.min.js"), &bundle).unwrap();
    let s = spawn_server(dist.path()).await;

    let (status, ctype, body) = fetch(
        s.shell_port,
        Method::GET,
        "/app/vendor/mermaid-1.2.3.min.js",
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "nested dist path serves");
    assert_eq!(ctype, "text/javascript", "scripts get a script MIME");
    assert_eq!(body.len(), bundle.len(), "whole file, no truncation");
    assert!(body.as_ref() == bundle.as_slice(), "byte-exact body");

    // A sibling that is not there is an ordinary 404, not "not built".
    let (status, _, text) = fetch(s.shell_port, Method::GET, "/app/vendor/nope.js", "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!String::from_utf8_lossy(&text).contains("frontend not built"));
}

/// ACCESS-33: the shell is not a way to anything but menger. The `/rest`
/// pass-through to the REST port is gone — the REST port itself, where it
/// runs, is unchanged — and every method on it answers 404.
#[tokio::test]
async fn the_shell_has_no_rest_pass_through() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let s = spawn_server(empty.path()).await;
    for (method, path) in [(Method::POST, "/rest/tickets/list"), (Method::GET, "/rest/tickets/health"), (Method::POST, "/rest/tickets/watch")] {
        let (status, _, _) = fetch(s.shell_port, method.clone(), path, "{}").await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }
}

/// ACCESS-33: `/app/` answers the files of the build the server read, by
/// their paths in it — a request path is a key, never a path on disk. A
/// file dropped into dist without a rebuild is not served; a rebuild (a
/// new entrypoint, as `trunk watch` writes one) is read whole, so a reload
/// picks it up without a restart.
#[tokio::test]
async fn app_answers_only_the_build_it_read() {
    let dist = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    std::fs::write(dist.path().join("index.html"), "<html>build one</html>").unwrap();
    std::fs::write(dist.path().join("app.js"), "console.log('one')").unwrap();
    let s = spawn_server(dist.path()).await;

    let (status, _, _) = fetch(s.shell_port, Method::GET, "/app/app.js", "").await;
    assert_eq!(status, StatusCode::OK);

    std::fs::write(dist.path().join("dropped.js"), "console.log('dropped')").unwrap();
    let (status, _, _) = fetch(s.shell_port, Method::GET, "/app/dropped.js", "").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "not in the build the server read");

    std::fs::write(dist.path().join("two.js"), "console.log('two')").unwrap();
    std::fs::write(dist.path().join("index.html"), "<html>build two</html>").unwrap();
    let index = std::fs::File::options().write(true).open(dist.path().join("index.html")).unwrap();
    index.set_modified(std::time::SystemTime::now() + Duration::from_secs(60)).unwrap();
    let (status, _, body) = fetch(s.shell_port, Method::GET, "/app/", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains("build two"), "the rebuild is served");
    let (status, _, _) = fetch(s.shell_port, Method::GET, "/app/two.js", "").await;
    assert_eq!(status, StatusCode::OK, "with its assets");
}


/// MFE-2 criterion 5 + MFE-4 acceptance 2: the vanilla page still loads
/// on the same listener, WS port injected, untouched by the /app and
/// /rest additions — at its own URL, and at the root when there is no app
/// build to serve there. The binary alone always serves something live.
#[tokio::test]
async fn vanilla_page_is_served_at_its_url_and_is_the_rootless_root() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let s = spawn_server(empty.path()).await;

    for path in ["/vanilla", "/"] {
        let (status, ctype, body) = fetch(s.shell_port, Method::GET, path, "").await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(ctype.starts_with("text/html"), "{path}: got {ctype}");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("<h1>menger</h1>"), "{path}: vanilla shell markup");
        assert!(
            !text.contains("{{WS_PORT}}"),
            "{path}: WS port placeholder is substituted"
        );
        assert!(text.contains("new WebSocket"), "{path}: still the WS client");
        assert!(
            !text.contains("frontend not built"),
            "{path}: the root degrades to a working page, not to the notice"
        );
    }
}

/// MFE-4 acceptance 1: with a build on disk the root **is** the app, and
/// the vanilla page is still whole at its own URL. The app's own root
/// (`/app/`) keeps serving it too — it is the asset prefix
/// (`Trunk.toml`'s `public_url`) and the deep-link prefix MFE-1 pinned,
/// so both URLs must answer with the same entrypoint.
#[tokio::test]
async fn the_root_is_the_app_when_it_is_built() {
    let dist = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    std::fs::write(
        dist.path().join("index.html"),
        "<html><meta name=\"menger-ws-port\" content=\"{{WS_PORT}}\">menger app stub</html>",
    )
    .unwrap();
    let s = spawn_server(dist.path()).await;

    for path in ["/", "/app/"] {
        let (status, ctype, body) = fetch(s.shell_port, Method::GET, path, "").await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(ctype.starts_with("text/html"), "{path}: got {ctype}");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("menger app stub"), "{path}: the app, got:\n{text}");
        assert!(
            !text.contains("{{WS_PORT}}"),
            "{path}: MFE-9's WS port is substituted at the root too"
        );
    }

    // …and the day-one page has not been stranded by the swap.
    let (status, _, body) = fetch(s.shell_port, Method::GET, "/vanilla", "").await;
    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("<h1>menger</h1>"), "vanilla shell markup");
    assert!(text.contains("new WebSocket"), "still the WS client");
}

/// MFE-7, universal click-through (epic user story 6): the vanilla page's
/// row renderer links every ticket to the app's deep-link URL. This pins
/// the URL scheme MFE-7 owns (`/app/#/ticket/{id}`, recorded in MFE-1) as
/// served — a change to the scheme must show up here.
#[tokio::test]
async fn vanilla_page_rows_link_to_app_ticket_pages() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let s = spawn_server(empty.path()).await;

    let (status, _, body) = fetch(s.shell_port, Method::GET, "/vanilla", "").await;
    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("/app/#/ticket/${t.id}"),
        "row renderer hrefs the app's deep-link scheme, got:\n{text}"
    );
    // The links must not have cost the page its status control.
    assert!(
        text.contains("<select data-id="),
        "per-row status select is still rendered"
    );
}

/// SDLC-11: the vanilla page renders a doc — its kind offered by the create
/// form, its status a neutral chip where a work ticket's status control is,
/// and that control offering the lifecycle, never `Doc`.
#[tokio::test]
async fn vanilla_page_renders_a_doc_with_a_chip_not_a_status_control() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let s = spawn_server(empty.path()).await;

    let (status, _, body) = fetch(s.shell_port, Method::GET, "/vanilla", "").await;
    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("<option>doc</option>"), "the create form offers the sixth kind");
    assert!(
        text.contains(r#"t.kind === "doc""#) && text.contains(r#"<span class="doc-chip">"#),
        "a doc's row shows its status as a chip"
    );
    let statuses = text
        .lines()
        .find(|l| l.starts_with("const STATUSES"))
        .expect("the status control's vocabulary");
    assert!(statuses.contains("\"Superseded\"") && !statuses.contains("\"Doc\""), "{statuses}");
}

/// MFE-23: the tab says which environment it is — `menger.test` on test,
/// `menger` otherwise — and the icon is served from the binary.
#[tokio::test]
async fn the_tab_title_names_the_environment_and_the_icon_is_served() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    for (env, want) in [(None, "<title>menger</title>"), (Some("test"), "<title>menger.test</title>"), (Some("prod"), "<title>menger</title>")] {
        let s = spawn_server_in(empty.path(), env).await;
        let (status, _, body) = fetch(s.shell_port, Method::GET, "/vanilla", "").await;
        assert_eq!(status, StatusCode::OK);
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains(want), "{env:?}: wants {want}");
        assert!(!text.contains("{{TITLE}}"), "{env:?}: no title placeholder left");
        let (status, ctype, body) = fetch(s.shell_port, Method::GET, "/favicon.svg", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(ctype, "image/svg+xml");
        assert!(body.starts_with(b"<svg"));
    }
}


/// Send and drain, with the headers: (status, headers, body bytes).
async fn fetch_with_headers(port: u16, path: &str) -> (StatusCode, hyper::HeaderMap, Bytes) {
    let mut sender = connect(port).await;
    let resp = sender.send_request(request(port, Method::GET, path, "")).await.expect("send request");
    let (status, headers) = (resp.status(), resp.headers().clone());
    (status, headers, resp.into_body().collect().await.unwrap().to_bytes())
}

fn header_of<'a>(h: &'a hyper::HeaderMap, name: &str) -> &'a str {
    h.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default()
}

/// `'sha256-…'` of an inline block, as a policy names it.
fn sha256_source(block: &str) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;
    format!("'sha256-{}'", base64::engine::general_purpose::STANDARD.encode(sha2::Sha256::digest(block.as_bytes())))
}

/// ACCESS-28: every page the shell serves carries its policy — scripts its
/// own files, the wasm and its own hashed inline blocks; connections to
/// itself and its hub; images its own; framed by nothing — and every answer
/// `nosniff` and a referrer kept to the origin. The frame document is the
/// one page with a policy of its own: no connection, no form, framed by the
/// app alone.
#[tokio::test]
async fn every_page_goes_out_under_its_policy() {
    let dist = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let loader = "import init from '/app/app.js'; await init();";
    std::fs::write(
        dist.path().join("index.html"),
        format!("<!-- no <script> here --><script>window.t=1</script><script type=\"module\">{loader}</script><script src=\"/app/app.js\"></script>"),
    )
    .unwrap();
    std::fs::write(dist.path().join("app.js"), "export default async function () {}").unwrap();
    let s = spawn_server(dist.path()).await;
    let hub = |csp: &str| -> u16 {
        let at = csp.find("ws://127.0.0.1:").expect("the hub's origin") + "ws://127.0.0.1:".len();
        csp[at..].split(' ').next().unwrap().parse().unwrap()
    };

    for path in ["/", "/app", "/app/", "/app/index.html"] {
        let (status, h, _) = fetch_with_headers(s.shell_port, path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        let csp = header_of(&h, "content-security-policy");
        let script_src = csp.split(';').map(str::trim).find(|d| d.starts_with("script-src ")).unwrap_or_default();
        assert_eq!(
            script_src,
            format!("script-src 'self' 'wasm-unsafe-eval' {} {}", sha256_source("window.t=1"), sha256_source(loader)),
            "{path}: only its own inline blocks, by hash"
        );
        for directive in [
            "default-src 'self'",
            "img-src 'self' data: blob:",
            "object-src 'none'",
            "base-uri 'self'",
            "frame-src 'self'",
            "frame-ancestors 'none'",
        ] {
            assert!(csp.split(';').map(str::trim).any(|d| d == directive), "{path}: {directive} in {csp}");
        }
        let ws = hub(csp);
        assert!(
            csp.contains(&format!(
                "connect-src 'self' ws://127.0.0.1:{ws} wss://127.0.0.1:{ws} wss://127.0.0.1:{} ws://127.0.0.1:* ws://localhost:*;",
                s.shell_port
            )),
            "{path}: itself, its hub at the name asked for, its own origin over TLS (ACCESS-30), and loopback: {csp}"
        );
        assert!(!csp.contains("'unsafe-eval'") && !script_src.contains("'unsafe-inline'"), "{path}: {csp}");
        assert_eq!(header_of(&h, "x-content-type-options"), "nosniff", "{path}");
        assert_eq!(header_of(&h, "referrer-policy"), "same-origin", "{path}");
        assert_eq!(header_of(&h, "x-frame-options"), "DENY", "{path}");
    }

    // The vanilla page: its own inline script hashed, the same policy.
    let (_, h, body) = fetch_with_headers(s.shell_port, "/vanilla").await;
    let page = String::from_utf8_lossy(&body);
    let block = &page[page.find("<script>").unwrap() + 8..page.find("</script>").unwrap()];
    let csp = header_of(&h, "content-security-policy");
    assert!(csp.contains(&format!("script-src 'self' 'wasm-unsafe-eval' {};", sha256_source(block))), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'") && header_of(&h, "x-content-type-options") == "nosniff");

    // A file of the build: no page policy, still hardened.
    let (_, h, _) = fetch_with_headers(s.shell_port, "/app/app.js").await;
    assert_eq!(header_of(&h, "content-security-policy"), "frame-ancestors 'none'");
    assert_eq!(header_of(&h, "x-content-type-options"), "nosniff");
    assert_eq!(header_of(&h, "referrer-policy"), "same-origin");

    // The frame document: IDENTITY-28's policy, framed by the app alone.
    let (status, h, body) = fetch_with_headers(s.shell_port, "/app/frame").await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_of(&h, "content-type").starts_with("text/html"));
    assert_eq!(header_of(&h, "content-security-policy"), "connect-src 'none'; form-action 'none'; frame-ancestors 'self'");
    assert_eq!(header_of(&h, "x-frame-options"), "SAMEORIGIN");
    assert_eq!(header_of(&h, "x-content-type-options"), "nosniff");
    assert_eq!(header_of(&h, "referrer-policy"), "same-origin");
    assert!(String::from_utf8_lossy(&body).contains("menger:boot"));

    // With no build: the "not built" page and the vanilla root, the same way.
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let bare = spawn_server(empty.path()).await;
    for path in ["/", "/app/"] {
        let (_, h, _) = fetch_with_headers(bare.shell_port, path).await;
        let csp = header_of(&h, "content-security-policy");
        assert!(csp.starts_with("default-src 'self'; script-src 'self' 'wasm-unsafe-eval'") && csp.ends_with("frame-ancestors 'none'"), "{path}: {csp}");
    }
}
