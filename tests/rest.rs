//! MNG-8 REST + SSE transport, black-box: spawns the built `menger`
//! binary with REST enabled and speaks plain HTTP to it — the same wire a
//! browser `fetch` or `curl` uses. Every server gets its own free REST
//! port and an isolated database, so the suite runs fully parallel.
//!
//! Covers the ticket's acceptance criteria machine-checkably:
//! - create over POST returns the created ticket's JSON; list includes it
//! - an SSE consumer on `watch`, opened BEFORE a create, receives the
//!   `created` event promptly (live stream, not batched at close)
//! - unknown method → 404; malformed params → 400
//! - an in-band `error` event (unknown status value) arrives inside a 2xx,
//!   distinguishable from transport-level HTTP errors

#![cfg(unix)]

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::{header, Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use std::path::PathBuf;
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

/// A spawned `menger` process with REST enabled on its own free port.
/// Dropping it SIGKILLs the child so a failed assertion never leaks a
/// server between runs.
struct Server {
    child: Child,
    rest_port: u16,
    _dir: tempfile::TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Spawns the built binary with `--rest-port <free>` (never the fixed
/// default — parallel test servers must not collide on 4457) and waits
/// until the REST listener accepts connections.
async fn spawn_server() -> Server {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let ws_port = free_port().await;
    let rest_port = free_port().await;
    let db: PathBuf = dir.path().join("tickets.db");
    let child = Command::new(env!("CARGO_BIN_EXE_menger"))
        .arg("--no-shell")
        .arg("--port")
        .arg(ws_port.to_string())
        .arg("--rest-port")
        .arg(rest_port.to_string())
        .arg("--db-path")
        .arg(&db)
        // Hermetic: never inherit an override or a filter from the
        // developer's environment.
        .env_remove("MENGER_DB_PATH")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn menger");

    let server = Server {
        child,
        rest_port,
        _dir: dir,
    };
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", server.rest_port))
            .await
            .is_ok()
        {
            return server;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("REST listener never came up on {}", server.rest_port);
}

/// One HTTP/1.1 connection to the server's REST port.
async fn connect(port: u16) -> http1::SendRequest<Full<Bytes>> {
    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (sender, conn) = http1::handshake(TokioIo::new(stream)).await.unwrap();
    tokio::spawn(conn);
    sender
}

fn post_request(port: u16, path: &str, body: &str) -> Request<Full<Bytes>> {
    Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::HOST, format!("127.0.0.1:{port}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(body.to_owned())))
        .unwrap()
}

/// POST a JSON body, drain the whole response. Non-JSON bodies (e.g. the
/// plain-text 400 for malformed JSON) come back as `Value::Null`.
async fn post(port: u16, path: &str, body: &str) -> (StatusCode, Value) {
    let mut sender = connect(port).await;
    let resp = sender
        .send_request(post_request(port, path, body))
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Criterion 1: create over REST returns the created ticket's JSON, and a
/// follow-up list over REST includes it.
#[tokio::test]
async fn create_via_rest_returns_ticket_and_list_includes_it() {
    let s = spawn_server().await;

    let (status, body) = post(
        s.rest_port,
        "/rest/tickets/create",
        &json!({
            "epic": "RST",
            "title": "born over HTTP",
            "kind": "scope",
            "severity": "High",
            "body": "## Problem\n\ncreated by tests/rest.rs"
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create is a 2xx, got {body}");
    let ev = &body["data"][0];
    assert_eq!(ev["type"], "created", "created event JSON, got {body}");
    assert_eq!(ev["ticket"]["id"], "RST-1");
    assert_eq!(ev["ticket"]["status"], "Pending");
    assert_eq!(ev["ticket"]["severity"], "High");

    let (status, body) = post(
        s.rest_port,
        "/rest/tickets/list",
        &json!({"epic": "RST"}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let data = body["data"].as_array().expect("data array");
    let ids: Vec<&str> = data
        .iter()
        .filter(|ev| ev["type"] == "ticket")
        .map(|ev| ev["ticket"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["RST-1"], "list includes the created ticket");
    // MNG-9: the gateway collects the trailing page item into the same
    // array as any other event — no transport-level paging, just data.
    assert_eq!(
        data.last().expect("at least one event")["type"],
        "page",
        "the page item rides in the array: {body}"
    );
}

/// Criterion 2: an SSE consumer on the watch endpoint, opened BEFORE the
/// create, receives the `created` event promptly — while the stream is
/// still open, not batched at close (watch never closes on its own).
#[tokio::test]
async fn watch_sse_streams_created_event_promptly() {
    let s = spawn_server().await;

    // Open the watch stream first. Headers arrive before any event.
    let mut sender = connect(s.rest_port).await;
    let resp = sender
        .send_request(post_request(
            s.rest_port,
            "/rest/tickets/watch",
            &json!({"epic": "RST"}).to_string(),
        ))
        .await
        .expect("open watch");
    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        content_type.starts_with("text/event-stream"),
        "watch responds as SSE, got content-type {content_type:?}"
    );
    let mut watch_body = resp.into_body();

    // Now create — the already-open consumer must see it promptly.
    let (status, body) = post(
        s.rest_port,
        "/rest/tickets/create",
        &json!({"epic": "RST", "title": "watched", "kind": "scope"}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create failed: {body}");

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
                // A complete SSE event block ends with a blank line.
                if buf.contains("\n\n") {
                    return buf;
                }
            }
        }
    })
    .await
    .expect("created event arrives promptly (within 5s), not batched at close");

    assert!(
        sse.contains("event: data"),
        "SSE event field names the item type, got:\n{sse}"
    );
    let payload = sse
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .expect("SSE data line");
    let event: Value = serde_json::from_str(payload).expect("SSE data is JSON");
    assert_eq!(event["content"]["type"], "created");
    assert_eq!(event["content"]["ticket"]["id"], "RST-1");
}

/// Criterion 3a: unknown method maps to HTTP 404.
#[tokio::test]
async fn unknown_method_is_404() {
    let s = spawn_server().await;
    let (status, _) = post(s.rest_port, "/rest/tickets/frobnicate", "{}").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Criterion 3b: malformed params map to HTTP 400 — both a body that is
/// not JSON and a body whose fields have the wrong types.
#[tokio::test]
async fn malformed_params_are_400() {
    let s = spawn_server().await;

    let (status, _) = post(s.rest_port, "/rest/tickets/create", "{not json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "unparseable body");

    let (status, body) = post(
        s.rest_port,
        "/rest/tickets/create",
        &json!({"epic": 42, "title": "wrong type"}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "wrong-typed params: {body}");
}

/// Criterion 3c: an in-band `error` event (unknown status value reaches
/// the activation as valid params) arrives inside a 2xx response,
/// distinguishable from transport-level HTTP errors.
#[tokio::test]
async fn in_band_error_event_is_distinguishable_in_2xx() {
    let s = spawn_server().await;

    let (status, body) = post(
        s.rest_port,
        "/rest/tickets/create",
        &json!({"epic": "RST", "title": "target", "kind": "scope"}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create failed: {body}");

    let (status, body) = post(
        s.rest_port,
        "/rest/tickets/set_status",
        &json!({"id": "RST-1", "status": "Bogus"}).to_string(),
    )
    .await;
    assert!(status.is_success(), "in-band error rides a 2xx, got {status}");
    let ev = &body["data"][0];
    assert_eq!(ev["type"], "error", "in-band error event, got {body}");
    assert!(
        ev["message"]
            .as_str()
            .unwrap_or_default()
            .contains("unknown status"),
        "error names the offending value, got {ev}"
    );
}
