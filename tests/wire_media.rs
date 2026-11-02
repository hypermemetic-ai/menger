//! V2-24: `media` over the real protocol — a real TransportServer, a real
//! WebSocket client, the `menger.call` envelope (tests/wire.rs's), the
//! service as `main` builds it (the media service beside `tickets`, its
//! file beside the tickets file): v1's wire tests (tests/wire_media.rs at
//! v0.0.1), and the seam — an image put for a ticket and read back by its
//! hash, byte for byte; the same image twice one image; an SVG, a
//! non-image, a mismatch and an image past the cap refused in band naming
//! why; a hash that names nothing `not_found`; what the transport does with
//! an image too large for one message; and **whose a ticket is, asked of
//! the tickets service over the seam as the caller**: a stranger's put on a
//! ticket is `not_found`, an editor's succeeds, a put with no ticket is the
//! putter's and readable by them (and the admins of `*`), a viewer's put is
//! the gate's `forbidden`.

use base64::Engine;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

mod support;

/// An editor on `A` (and nothing on `B`).
const EDITOR_A: [u8; 32] = [0xea; 32];
/// An editor on `B` — a stranger to `A`.
const STRANGER: [u8; 32] = [0x5b; 32];
/// A viewer on `A`.
const VIEWER_A: [u8; 32] = [0x4a; 32];

/// A server on an isolated, throwaway database — the service as `main`
/// builds it (`support::service`: `tickets`, `ui`, `media`, …), behind the
/// gate under the fixture operator.
async fn start_server() -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    let port = support::free_port().await;
    let login = support::Login::new();
    let service = support::service(&db_path, &login).await;
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(service, conv)
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

/// A client as the key `seed` (the fixture operator by default), taking an
/// answer up to 16 MiB — an image is 7 MiB in base64 and more.
async fn connect_key(seed: &[u8; 32], port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..100 {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_str(&format!("Bearer {}", support::proof_of(seed, port))).unwrap());
        if let Ok(c) = WsClientBuilder::default().set_headers(h).max_response_size(16 * 1024 * 1024).build(&url).await {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never came up on {url}");
}

async fn connect(port: u16) -> WsClient {
    connect_key(&support::ADMIN, port).await
}

async fn call(client: &WsClient, method: &str, params: Value) -> Result<Subscription<Value>, jsonrpsee::core::ClientError> {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    client.subscribe("menger.call", p, "menger.call_unsub").await
}

/// The one event a call answers with — a data item's content, or the
/// gate's error item whole.
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
/// A real 1×1 GIF.
const GIF: &str = "R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";

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
    assert_eq!((got["mime"].as_str(), got["width"].as_u64(), got["height"].as_u64(), got["bytes"].as_u64()), (Some("image/png"), Some(2), Some(1), Some(bytes.len() as u64)));
    assert_eq!(one(&client, "media.get", json!({"hash": format!("media:{}", hash.to_uppercase())})).await["hash"], hash, "media: and any case");

    // The same image twice: one image.
    let again = one(&client, "media.put", json!({"ticket": "KILN-1", "mime": "image/png", "data": PNG_2X1})).await;
    assert_eq!((again["hash"].as_str(), again["duplicate"].as_bool()), (Some(hash.as_str()), Some(true)));

    // Not there: not_found, naming the hash; a ticket that is none, naming it.
    let none = "0".repeat(64);
    assert_eq!(one(&client, "media.get", json!({"hash": none})).await, json!({"type": "not_found", "hash": none}));
    assert_eq!(one(&client, "media.put", json!({"ticket": "KILN-9", "mime": "image/png", "data": PNG_2X1})).await, json!({"type": "not_found", "ticket": "KILN-9"}));
    // A hash that is none is refused before the method runs, naming it.
    let bad = one(&client, "media.get", json!({"hash": "abc"})).await;
    assert!(bad.to_string().contains("hash"), "{bad}");
    // A key no parameter claims is refused (MACRO-31): v1's `get { ticket }` went.
    let unknown = one(&client, "media.get", json!({"hash": hash, "ticket": "KILN-1"})).await;
    assert!(unknown.to_string().contains("unknown field"), "{unknown}");
}

#[tokio::test]
async fn any_file_is_kept_by_its_bytes_and_too_much_for_one_message_is_refused_naming_why() {
    let port = start_server().await;
    let client = connect(port).await;
    one(&client, "tickets.create", json!({"epic": "KILN", "title": "kiln", "kind": "scope"})).await;
    let put = |mime: &str, data: String| json!({"ticket": "KILN-1", "mime": mime, "data": data});

    // EDITOR-10: an SVG is kept — as a file, never as itself.
    let svg = b64(br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#);
    let r = one(&client, "media.put", json!({"ticket": "KILN-1", "mime": "image/svg+xml", "data": svg, "name": "x.svg"})).await;
    assert_eq!((r["type"].as_str(), r["mime"].as_str(), r["kind"].as_str(), r["name"].as_str()), (Some("stored"), Some("application/octet-stream"), Some("file"), Some("x.svg")), "{r}");
    let r = one(&client, "media.put", json!({"ticket": "KILN-1", "data": b64(b"%PDF-1.7 a document"), "name": "deck.pdf"})).await;
    assert_eq!((r["mime"].as_str(), r["kind"].as_str()), (Some("application/pdf"), Some("pdf")), "{r}");
    // The bytes decide: a PNG said to be a JPEG is a PNG.
    let r = one(&client, "media.put", put("image/jpeg", PNG_2X1.into())).await;
    assert_eq!((r["mime"].as_str(), r["kind"].as_str(), r["width"].as_u64()), (Some("image/png"), Some("image"), Some(2)), "{r}");

    // Past the cap of one message but within one (7 MiB + 1 KiB): refused in band, naming the cap.
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
    // jsonrpsee closes the socket ("The message was too large"): so a client
    // sends such a file in chunks.
    assert!(refused.contains("too large"), "{refused}");
    let fresh = connect(port).await;
    let ok = one(&fresh, "media.put", put("image/png", b64(&png_of(3, 3, 64)))).await;
    assert_eq!(ok["type"], "stored", "the server goes on: {ok}");
}

/// A file of `n` bytes that is an MP4 by its first ones.
fn mp4(n: usize) -> Vec<u8> {
    let mut b = [&[0, 0, 0, 0x20][..], b"ftypisom"].concat();
    b.extend((0..n - 12).map(|i| (i * 7 % 253) as u8));
    b
}

/// EDITOR-10: a file larger than one message goes up in chunks, resumes on
/// a new connection from what it has, is kept by its hash and drawn as its
/// kind; `info` says what it is; `get` points past one message to the
/// route; another's upload is none; `usage` counts it once.
#[tokio::test]
async fn a_large_file_goes_up_in_chunks_resumes_and_is_kept() {
    let port = start_server().await;
    let client = connect(port).await;
    one(&client, "tickets.create", json!({"epic": "KILN", "title": "kiln", "kind": "scope"})).await;
    let file = mp4(9 * 1024 * 1024 + 5);
    let begun = one(&client, "media.begin", json!({"ticket": "KILN-1", "name": "walk.mp4", "mime": "video/mp4", "size": file.len()})).await;
    assert_eq!((begun["type"].as_str(), begun["received"].as_u64()), (Some("upload"), Some(0)), "{begun}");
    let upload = begun["upload"].as_str().unwrap().to_string();
    let step = begun["chunk"].as_u64().unwrap() as usize;
    assert_eq!(step, 4 * 1024 * 1024);
    let c = one(&client, "media.chunk", json!({"upload": upload, "offset": 0, "data": b64(&file[..step])})).await;
    assert_eq!((c["type"].as_str(), c["received"].as_u64(), c["duplicate"].as_bool()), (Some("chunked"), Some(step as u64), Some(false)), "{c}");
    let gap = one(&client, "media.chunk", json!({"upload": upload, "offset": 2 * step, "data": b64(&file[..10])})).await;
    assert_eq!(gap["reason"], "out_of_order", "{gap}");
    // The connection drops; a new one resumes from what the upload has.
    drop(client);
    let client = connect(port).await;
    let again = one(&client, "media.begin", json!({"upload": upload})).await;
    assert_eq!((again["received"].as_u64(), again["size"].as_u64()), (Some(step as u64), Some(file.len() as u64)), "{again}");
    let stranger = connect_key(&STRANGER, port).await;
    let theirs = one(&stranger, "media.begin", json!({"upload": upload})).await;
    assert!(theirs["type"] == "not_found" || theirs["error_kind"] == "forbidden", "not theirs: {theirs}");
    let early = one(&client, "media.finish", json!({"upload": upload})).await;
    assert_eq!(early["reason"], "incomplete", "{early}");
    let mut at = step;
    while at < file.len() {
        let end = (at + step).min(file.len());
        let c = one(&client, "media.chunk", json!({"upload": upload, "offset": at, "data": b64(&file[at..end])})).await;
        assert_eq!(c["type"], "chunked", "{c}");
        at = end;
    }
    let stored = one(&client, "media.finish", json!({"upload": upload})).await;
    assert_eq!(
        (stored["type"].as_str(), stored["mime"].as_str(), stored["kind"].as_str(), stored["name"].as_str(), stored["bytes"].as_u64()),
        (Some("stored"), Some("video/mp4"), Some("video"), Some("walk.mp4"), Some(file.len() as u64)),
        "{stored}"
    );
    let hash = stored["hash"].as_str().unwrap().to_string();
    let info = one(&client, "media.info", json!({"hash": hash})).await;
    assert_eq!((info["type"].as_str(), info["kind"].as_str(), info["bytes"].as_u64()), (Some("info"), Some("video"), Some(file.len() as u64)), "{info}");
    let got = one(&client, "media.get", json!({"hash": hash})).await;
    assert_eq!(got["reason"], "too_large", "past one message, read by range: {got}");
    assert!(got["message"].as_str().unwrap().contains(&format!("/media/{hash}")), "{got}");
    let usage = one(&client, "media.usage", json!({})).await;
    assert_eq!((usage["type"].as_str(), usage["files"].as_u64(), usage["bytes"].as_u64()), (Some("usage"), Some(1), Some(file.len() as u64)), "{usage}");
}

/// EDITOR-10: a link the server would read from a private address is
/// refused over the wire, naming why (the allowed case, against a local
/// server, is `media/tests/unfurl.rs`'s).
#[tokio::test]
async fn an_unfurl_or_fetch_of_a_private_address_is_refused_in_band() {
    let port = start_server().await;
    let client = connect(port).await;
    for url in ["http://127.0.0.1:1/", "http://169.254.169.254/latest/meta-data/", "http://[::1]/", "http://10.0.0.1/x.png", "http://localhost/"] {
        let r = one(&client, "media.unfurl", json!({"url": url})).await;
        assert_eq!((r["type"].as_str(), r["reason"].as_str()), (Some("refused"), Some("blocked_address")), "{url}: {r}");
        let r = one(&client, "media.fetch", json!({"url": url})).await;
        assert_eq!(r["reason"], "blocked_address", "{url}: {r}");
    }
    let bad = one(&client, "media.unfurl", json!({"url": "file:///etc/passwd"})).await;
    assert!(bad.to_string().contains("web address"), "refused before the method runs: {bad}");
}

fn crate_max() -> usize {
    menger_media::image::MAX_BYTES
}

/// A grant by the operator: `role` on `scope` for the key `seed`.
async fn grant(admin: &WsClient, seed: &[u8; 32], role: &str, scope: &str) {
    let g = one(admin, "auth.grant", json!({ "principal": support::principal_of(seed), "role": role, "scope": scope })).await;
    assert!(g.get("error_kind").is_none(), "{role} on {scope}: {g}");
}

/// The permit is asked of the tickets service over the seam, as the
/// caller: a stranger's put on a ticket is `not_found` (the tickets service
/// does not know them there), an editor's succeeds, a viewer's is the
/// gate's `forbidden` (`media.put` is an editor's grant); a put with no
/// ticket is the putter's, readable by them and the admins of `*`, by no
/// one else; `tickets.may` itself answers as the permit would.
#[tokio::test]
async fn whose_a_ticket_is_is_asked_of_the_tickets_service_over_the_seam() {
    let port = start_server().await;
    let admin = connect(port).await;
    for (epic, title, kind) in [("A", "a", "scope"), ("A", "a build", "build"), ("B", "b", "scope"), ("B", "b build", "build")] {
        let made = one(&admin, "tickets.create", json!({"epic": epic, "title": title, "kind": kind})).await;
        assert_eq!(made["type"], "created", "{made}");
    }
    grant(&admin, &EDITOR_A, "editor", "A").await;
    grant(&admin, &STRANGER, "editor", "B").await;
    grant(&admin, &VIEWER_A, "viewer", "A").await;
    let (editor, stranger, viewer) = (connect_key(&EDITOR_A, port).await, connect_key(&STRANGER, port).await, connect_key(&VIEWER_A, port).await);

    // The question itself, over the wire: the tickets service's word.
    async fn may(c: &WsClient, id: &str, role: &str) -> Value {
        one(c, "tickets.may", json!({"id": id, "role": role})).await
    }
    assert_eq!(may(&editor, "A-2", "editor").await, json!({"type": "may", "id": "A-2", "role": "editor", "held": true}));
    assert_eq!(may(&editor, "a-2", "viewer").await, json!({"type": "may", "id": "A-2", "role": "viewer", "held": true}), "any spelling; the id as the store keeps it");
    assert_eq!(may(&viewer, "A-2", "editor").await, json!({"type": "may", "id": "A-2", "role": "editor", "held": false}), "seen, not editable");
    assert_eq!(may(&stranger, "A-2", "editor").await, json!({"type": "not_found", "id": "A-2"}), "not theirs: not there");
    assert_eq!(may(&stranger, "A-9", "viewer").await, json!({"type": "not_found", "id": "A-9"}), "and the same for none");
    assert_eq!(may(&editor, "A-2", "admin").await["held"], false, "an editor is no admin");

    // A stranger's put on A-2: the tickets service does not know them there.
    let no = one(&stranger, "media.put", json!({"ticket": "A-2", "mime": "image/png", "data": PNG_2X1})).await;
    assert_eq!(no, json!({"type": "not_found", "ticket": "A-2"}), "{no}");
    // A viewer's: the gate's refusal — media.put is an editor's grant.
    let no = one(&viewer, "media.put", json!({"ticket": "A-2", "mime": "image/png", "data": PNG_2X1})).await;
    assert_eq!((no["type"].as_str(), no["error_kind"].as_str(), no["grant"].as_str()), (Some("error"), Some("forbidden"), Some("media.put")), "{no}");
    // The editor's succeeds.
    let mine = one(&editor, "media.put", json!({"ticket": "A-2", "mime": "image/png", "data": PNG_2X1})).await;
    assert_eq!((mine["type"].as_str(), mine["width"].as_u64()), (Some("stored"), Some(2)), "{mine}");
    let hash = mine["hash"].as_str().unwrap().to_string();
    // …and A's viewer reads it, the admin reads it, the stranger does not.
    assert_eq!(one(&viewer, "media.get", json!({"hash": hash})).await["data"], PNG_2X1, "A's viewer sees A-2's image");
    assert_eq!(one(&admin, "media.get", json!({"hash": hash})).await["type"], "media", "the admin's");
    assert_eq!(one(&stranger, "media.get", json!({"hash": hash})).await, json!({"type": "not_found", "hash": hash}), "B's editor does not see A-2: not found, as for none");
    // The same bytes, put by the stranger for its own ticket: one image, B-2's too — theirs to read now.
    let theirs = one(&stranger, "media.put", json!({"ticket": "B-2", "mime": "image/png", "data": PNG_2X1})).await;
    assert_eq!((theirs["hash"].as_str(), theirs["duplicate"].as_bool()), (Some(hash.as_str()), Some(true)), "{theirs}");
    assert_eq!(one(&stranger, "media.get", json!({"hash": hash})).await["type"], "media", "B-2's now");

    // A put with no ticket: the putter's, and the admins' of `*` — no one else's.
    let own = one(&editor, "media.put", json!({"mime": "image/gif", "data": GIF})).await;
    assert_eq!((own["type"].as_str(), own["mime"].as_str(), own["width"].as_u64()), (Some("stored"), Some("image/gif"), Some(1)), "{own}");
    let own = own["hash"].as_str().unwrap().to_string();
    assert_eq!(one(&editor, "media.get", json!({"hash": own})).await["data"], GIF, "the putter reads it");
    assert_eq!(one(&admin, "media.get", json!({"hash": own})).await["type"], "media", "and the admin of *");
    assert_eq!(one(&stranger, "media.get", json!({"hash": own})).await, json!({"type": "not_found", "hash": own}), "no one else");
    assert_eq!(one(&viewer, "media.get", json!({"hash": own})).await["type"], "not_found", "not even a viewer of the putter's shortcode");
}
