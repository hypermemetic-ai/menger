//! MFE-2 shell serving, black-box: spawns the built `menger` binary with
//! the web shell enabled on a free port and speaks plain HTTP to it —
//! exactly what a browser does. Covers the ticket's serving guarantees
//! without requiring the frontend toolchain:
//!
//! - `/app/` with no build output answers a clear "not built" page (200),
//!   never a crash or a 500 (criterion 4)
//! - `/app/` with build output on disk serves it with sane content types
//!   (a synthetic dist — real trunk output is criterion 1, verified live)
//! - `/rest/{*}` on the shell port reverse-proxies to the REST port:
//!   unary calls round-trip and `watch` streams SSE through the proxy
//!   (the same-origin data plane the Leptos app rides)
//! - `--no-rest` turns the proxy into a clear 503, and the vanilla page
//!   at `/` keeps working throughout (criterion 5)

#![cfg(unix)]

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::{header, Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::net::TcpStream;

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
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
async fn spawn_server(app_dist: &Path, rest: bool) -> Server {
    for attempt in 0..3 {
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let ws_port = free_port().await;
        let shell_port = free_port().await;
        let rest_port = free_port().await;
        let db: PathBuf = dir.path().join("tickets.db");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_menger"));
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
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if rest {
            cmd.arg("--rest-port").arg(rest_port.to_string());
        } else {
            cmd.arg("--no-rest");
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
            let rest_up = !rest || TcpStream::connect(("127.0.0.1", rest_port)).await.is_ok();
            if shell_up && rest_up {
                // The WS bind has no probe of its own; give a failed one
                // a beat to take the process down before trusting it.
                tokio::time::sleep(Duration::from_millis(50)).await;
                if server.child.try_wait().expect("try_wait").is_none() {
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
    let s = spawn_server(empty.path(), true).await;

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
    let s = spawn_server(dist.path(), true).await;

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
    let s = spawn_server(dist.path(), true).await;

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

/// The same-origin data plane: a unary REST call through the shell port's
/// /rest proxy round-trips exactly like a direct REST call.
#[tokio::test]
async fn rest_proxy_round_trips_unary_calls() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let s = spawn_server(empty.path(), true).await;

    let (status, _, body) = fetch(
        s.shell_port,
        Method::POST,
        "/rest/tickets/create",
        &json!({"epic": "PRX", "title": "born through the proxy", "kind": "scope"}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["data"][0]["type"], "created");
    assert_eq!(v["data"][0]["ticket"]["id"], "PRX-1");

    // Transport-level mappings survive the hop.
    let (status, _, _) = fetch(s.shell_port, Method::POST, "/rest/tickets/frobnicate", "{}").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "404 passes through");
}

/// The live half of the data plane: `watch` opened through the proxy
/// BEFORE a create receives the created event promptly, as SSE — proving
/// the proxy streams instead of buffering (what the Leptos app rides).
#[tokio::test]
async fn rest_proxy_streams_watch_sse() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let s = spawn_server(empty.path(), true).await;

    let mut sender = connect(s.shell_port).await;
    let resp = sender
        .send_request(request(
            s.shell_port,
            Method::POST,
            "/rest/tickets/watch",
            &json!({"epic": "PRX"}).to_string(),
        ))
        .await
        .expect("open watch through proxy");
    assert_eq!(resp.status(), StatusCode::OK);
    let ctype = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        ctype.starts_with("text/event-stream"),
        "SSE content type survives the proxy, got {ctype:?}"
    );
    let mut watch_body = resp.into_body();

    let (status, _, body) = fetch(
        s.shell_port,
        Method::POST,
        "/rest/tickets/create",
        &json!({"epic": "PRX", "title": "watched through the proxy", "kind": "scope"}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let sse = tokio::time::timeout(Duration::from_secs(5), async {
        let mut buf = String::new();
        loop {
            let frame = watch_body
                .frame()
                .await
                .expect("watch stream stays open")
                .expect("clean frame");
            if let Ok(data) = frame.into_data() {
                buf.push_str(std::str::from_utf8(&data).expect("utf-8 SSE"));
                // A complete frame carrying data (the proxy prepends an
                // ignorable `:` comment frame — see src/shell.rs — so the
                // first blank line alone is not the event).
                if let Some(complete) = buf.rfind("\n\n") {
                    if buf[..complete].contains("data: ") {
                        return buf;
                    }
                }
            }
        }
    })
    .await
    .expect("created event streams through the proxy promptly (within 5s)");

    assert!(
        sse.starts_with(':'),
        "proxy's SSE open-comment preamble leads the stream, got:\n{sse}"
    );
    let payload = sse
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .expect("SSE data line");
    let event: Value = serde_json::from_str(payload).expect("SSE data is JSON");
    assert_eq!(event["content"]["type"], "created");
    assert_eq!(event["content"]["ticket"]["id"], "PRX-1");
}

/// MFE-3's data plane, end to end through the proxy: `set_status`
/// round-trips (the board's per-row control) and the change is broadcast
/// to an already-open `watch` stream as `status_changed` — how one
/// window's edit reaches every other open client.
#[tokio::test]
async fn rest_proxy_set_status_round_trips_and_watch_broadcasts_it() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let s = spawn_server(empty.path(), true).await;

    let (status, _, body) = fetch(
        s.shell_port,
        Method::POST,
        "/rest/tickets/create",
        &json!({"epic": "PRX", "title": "status via proxy", "kind": "scope"}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // Watch opened before the mutation, like a second browser window.
    let mut sender = connect(s.shell_port).await;
    let resp = sender
        .send_request(request(
            s.shell_port,
            Method::POST,
            "/rest/tickets/watch",
            "{}",
        ))
        .await
        .expect("open watch through proxy");
    assert_eq!(resp.status(), StatusCode::OK);
    let mut watch_body = resp.into_body();

    let (status, _, body) = fetch(
        s.shell_port,
        Method::POST,
        "/rest/tickets/set_status",
        &json!({"id": "PRX-1", "status": "Ready"}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["data"][0]["type"], "status_changed");
    assert_eq!(v["data"][0]["ticket"]["status"], "Ready");
    assert_eq!(v["data"][0]["previous"], "Pending");

    let sse = tokio::time::timeout(Duration::from_secs(5), async {
        let mut buf = String::new();
        loop {
            let frame = watch_body
                .frame()
                .await
                .expect("watch stream stays open")
                .expect("clean frame");
            if let Ok(data) = frame.into_data() {
                buf.push_str(std::str::from_utf8(&data).expect("utf-8 SSE"));
                if buf.contains("status_changed") && buf.rfind("\n\n").is_some() {
                    return buf;
                }
            }
        }
    })
    .await
    .expect("status_changed reaches the open watch within 5s");
    let payload = sse
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .expect("SSE data line");
    let event: Value = serde_json::from_str(payload).expect("SSE data is JSON");
    assert_eq!(event["content"]["type"], "status_changed");
    assert_eq!(event["content"]["ticket"]["id"], "PRX-1");
    assert_eq!(event["content"]["ticket"]["status"], "Ready");
}

/// The server stays the validator through the proxy: a status outside
/// the closed vocabulary comes back as an in-band `error` event inside a
/// 2xx — the exact shape the app's error banner consumes — and the
/// ticket is left unchanged.
#[tokio::test]
async fn rest_proxy_passes_in_band_error_for_bad_status() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let s = spawn_server(empty.path(), true).await;

    let (status, _, _) = fetch(
        s.shell_port,
        Method::POST,
        "/rest/tickets/create",
        &json!({"epic": "PRX", "title": "rejects bad status", "kind": "scope"}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, body) = fetch(
        s.shell_port,
        Method::POST,
        "/rest/tickets/set_status",
        &json!({"id": "PRX-1", "status": "Bogus"}).to_string(),
    )
    .await;
    assert!(status.is_success(), "in-band error rides a 2xx, got {status}");
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["data"][0]["type"], "error", "got {v}");
    assert!(
        v["data"][0]["message"]
            .as_str()
            .unwrap_or_default()
            // The server normalizes case before validating, and reports
            // the normalized value.
            .contains("unknown status 'bogus'"),
        "error names the offending value, got {v}"
    );

    let (_, _, body) = fetch(
        s.shell_port,
        Method::POST,
        "/rest/tickets/get",
        &json!({"id": "PRX-1"}).to_string(),
    )
    .await;
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["data"][0]["ticket"]["status"], "Pending", "unchanged");
}

/// --no-rest: the proxy has no upstream and says so — a deliberate 503,
/// not a hang or a connection error surfaced as a 500.
#[tokio::test]
async fn rest_proxy_without_rest_is_clear_503() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let s = spawn_server(empty.path(), false).await;

    let (status, _, body) = fetch(s.shell_port, Method::POST, "/rest/tickets/health", "{}").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        String::from_utf8_lossy(&body).contains("--no-rest"),
        "names the cause"
    );
}

/// MFE-2 criterion 5 + MFE-4 acceptance 2: the vanilla page still loads
/// on the same listener, WS port injected, untouched by the /app and
/// /rest additions — at its own URL, and at the root when there is no app
/// build to serve there. The binary alone always serves something live.
#[tokio::test]
async fn vanilla_page_is_served_at_its_url_and_is_the_rootless_root() {
    let empty = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let s = spawn_server(empty.path(), true).await;

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
    let s = spawn_server(dist.path(), true).await;

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
    let s = spawn_server(empty.path(), true).await;

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
