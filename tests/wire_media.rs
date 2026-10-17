//! COMMENT-5: `media` over the real protocol — a real TransportServer, a
//! real WebSocket client, the `menger.call` envelope (tests/wire.rs's): an
//! image put for a ticket and read back by its hash, byte for byte; the
//! same image twice one image; a comment that names it; an SVG, a
//! non-image, a mismatch and an image past the cap refused in band naming
//! why; a hash that names nothing `not_found`; and what the transport does
//! with an image too large for one message. (A reader without the ticket's
//! reach is tests/auth_protocol.rs's, over a server with login.)

use base64::Engine;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

mod support;

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

/// A server on an isolated, throwaway database.
async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    let port = free_port().await;
    // ACCESS-24: login always — the support harness's key login, its admin.
    let login = support::Login::new();
    let hub = support::hub(&db_path, &login).await;
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(hub, conv)
            .with_websocket(port)
            .with_session_validator(gate)
            .anonymous_view()
            .credential_cookies(["access_token"])
            .build()
            .await
            .expect("build server")
            .serve()
            .await
            .expect("serve");
    });
    port
}

async fn connect(port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..50 {
        if let Ok(c) = WsClientBuilder::default().set_headers(support::admin_headers(port)).max_response_size(16 * 1024 * 1024).build(&url).await {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never came up on {url}");
}

async fn call(client: &WsClient, method: &str, params: Value) -> Result<Subscription<Value>, jsonrpsee::core::ClientError> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    client.subscribe("menger.call", p, "menger.call_unsub").await
}

/// The one event a call answers with.
async fn one(client: &WsClient, method: &str, params: Value) -> Value {
    let mut sub = call(client, method, params).await.expect("subscribe");
    let mut out = Vec::new();
    while let Some(item) = tokio::time::timeout(Duration::from_secs(20), sub.next()).await.expect("an answer") {
        let item = item.unwrap();
        match item["type"].as_str() {
            Some("done") => break,
            Some("data") => out.push(item["content"].clone()),
            _ => {
                out.push(item);
                break;
            }
        }
    }
    assert_eq!(out.len(), 1, "{method}: {out:?}");
    out.remove(0)
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// A real 2×1 PNG (signature, IHDR, IDAT, IEND).
const PNG_2X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR42mP8z8DwnwEJMAIAKQgD/2zM2wkAAAAASUVORK5CYII=";

/// A PNG's header for `w`×`h`, then `len` bytes in all.
fn png_of(w: u32, h: u32, len: usize) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    b.extend_from_slice(b"IHDR");
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.resize(len, 7);
    b
}

#[tokio::test]
async fn an_image_is_put_for_a_ticket_and_read_back_by_its_hash() {
    let port = start_server().await;
    let client = connect(port).await;
    assert_eq!(one(&client, "tickets.create", json!({"epic": "KILN", "title": "kiln", "kind": "scope"})).await["type"], "created");

    let stored = one(&client, "media.put", json!({"ticket": "kiln-1", "mime": "image/png", "data": PNG_2X1})).await;
    assert_eq!(stored["type"], "stored", "{stored}");
    let hash = stored["hash"].as_str().unwrap().to_string();
    assert_eq!(hash.len(), 64);
    assert_eq!((stored["mime"].as_str(), stored["width"].as_u64(), stored["height"].as_u64(), stored["duplicate"].as_bool()), (Some("image/png"), Some(2), Some(1), Some(false)));
    let bytes = base64::engine::general_purpose::STANDARD.decode(PNG_2X1).unwrap();
    assert_eq!(stored["bytes"].as_u64(), Some(bytes.len() as u64));

    let got = one(&client, "media.get", json!({"hash": hash})).await;
    assert_eq!(got["type"], "media", "{got}");
    assert_eq!(got["data"].as_str(), Some(PNG_2X1), "the bytes as put");
    assert_eq!(one(&client, "media.get", json!({"hash": format!("media:{}", hash.to_uppercase())})).await["hash"], hash, "media: and any case");

    // The same image twice: one image.
    let again = one(&client, "media.put", json!({"ticket": "KILN-1", "mime": "image/png", "data": PNG_2X1})).await;
    assert_eq!((again["hash"].as_str(), again["duplicate"].as_bool()), (Some(hash.as_str()), Some(true)));

    // A comment that shows it, as the ticket's example writes one.
    let body = format!("the hold drifts here ![the curve](media:{hash})");
    let c = one(&client, "comments.add", json!({"anchor": {"route": "#/ticket/KILN-1", "path": "body", "label": "Body", "excerpt": "", "ticket": "KILN-1"}, "body": body})).await;
    assert_eq!((c["type"].as_str(), c["body"].as_str()), (Some("comment"), Some(body.as_str())));

    // Not there: not_found, naming the hash.
    let none = "0".repeat(64);
    assert_eq!(one(&client, "media.get", json!({"hash": none})).await, json!({"type": "not_found", "hash": none}));
    assert_eq!(one(&client, "media.put", json!({"ticket": "KILN-9", "mime": "image/png", "data": PNG_2X1})).await, json!({"type": "not_found", "ticket": "KILN-9"}));
    // A hash that is none is refused before the method runs, naming it.
    let bad = one(&client, "media.get", json!({"hash": "abc"})).await;
    assert!(bad.to_string().contains("hash"), "{bad}");
}

#[tokio::test]
async fn an_svg_a_non_image_a_mismatch_and_too_much_are_refused_naming_why() {
    let port = start_server().await;
    let client = connect(port).await;
    one(&client, "tickets.create", json!({"epic": "KILN", "title": "kiln", "kind": "scope"})).await;
    let put = |mime: &str, data: String| json!({"ticket": "KILN-1", "mime": mime, "data": data});

    let svg = b64(br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#);
    let r = one(&client, "media.put", put("image/svg+xml", svg)).await;
    assert_eq!((r["type"].as_str(), r["reason"].as_str()), (Some("refused"), Some("not_an_image")), "{r}");
    assert!(r["message"].as_str().unwrap().contains("an SVG is not kept"), "{r}");

    let r = one(&client, "media.put", put("application/pdf", b64(b"%PDF-1.7 a document"))).await;
    assert_eq!(r["reason"], "not_an_image", "{r}");
    assert!(r["message"].as_str().unwrap().contains("image/png, image/jpeg, image/gif, image/webp"), "{r}");

    let r = one(&client, "media.put", put("image/jpeg", PNG_2X1.into())).await;
    assert_eq!((r["reason"].as_str(), r["message"].as_str()), (Some("type_mismatch"), Some("the bytes are image/png, not image/jpeg as declared (nothing was kept)")), "{r}");

    // Past the cap but within one message (7 MiB + 1 KiB): refused in band,
    // naming the cap.
    let over = crate_max() + 1024;
    let r = one(&client, "media.put", put("image/png", b64(&png_of(10, 10, over)))).await;
    assert_eq!((r["reason"].as_str(), r["bytes"].as_u64(), r["max"].as_u64()), (Some("too_large"), Some(over as u64), Some(crate_max() as u64)), "{r}");
    assert!(r["message"].as_str().unwrap().contains("at most 7.0 MB"), "{r}");

    // 9 MB is 12 MB as base64: more than one WebSocket message takes (10
    // MiB) — the transport refuses the call before menger sees it.
    let nine = b64(&png_of(10, 10, 9 * 1000 * 1000));
    let refused = match call(&client, "media.put", put("image/png", nine)).await {
        Err(e) => e.to_string(),
        Ok(mut sub) => match tokio::time::timeout(Duration::from_secs(20), sub.next()).await {
            Ok(Some(Ok(item))) => item.to_string(),
            Ok(Some(Err(e))) => e.to_string(),
            Ok(None) => "closed".into(),
            Err(_) => panic!("no answer to a 12 MB call"),
        },
    };
    // jsonrpsee closes the socket ("The message was too large"): so the app
    // refuses such a file itself, before sending (media::check).
    assert!(refused.contains("too large"), "{refused}");
    // And nothing was kept: the cap's own image is still the only kind there.
    let fresh = connect(port).await;
    let ok = one(&fresh, "media.put", put("image/png", b64(&png_of(3, 3, 64)))).await;
    assert_eq!(ok["type"], "stored", "the server goes on: {ok}");
}

fn crate_max() -> usize {
    menger_tickets_api::media::MAX_BYTES
}
