//! V2-54: a message past plexus-transport's default 10 MiB reaches menger and is answered — a UI bundle
//! past 7.5 MB is pushed in one such message, base64 — and `--max-message-bytes` sets the limit.

mod support;

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use serde_json::{json, Value};
use std::time::Duration;
use support::{free_port, proof_of, spawn_menger, ADMIN};

/// The admin's client, itself allowed messages of 64 MiB (jsonrpsee's own default is 10 MiB too).
async fn client(port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    let mut h = HeaderMap::new();
    h.insert("authorization", HeaderValue::from_str(&format!("Bearer {}", proof_of(&ADMIN, port))).unwrap());
    WsClientBuilder::default().set_headers(h).max_request_size(64 << 20).max_response_size(64 << 20).build(&url).await.expect("connect")
}

/// One call of `size` bytes or more: `tickets.list` with a parameter no one claims, which the typed params
/// refuse in band — an answer, so the message arrived. `None`: nothing came back.
async fn answered(c: &WsClient, size: usize) -> Option<Value> {
    let mut p = ObjectParams::new();
    p.insert("method", "tickets.list").unwrap();
    p.insert("params", json!({ "padding": "x".repeat(size) })).unwrap();
    let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.ok()?;
    match tokio::time::timeout(Duration::from_secs(20), s.next()).await {
        Ok(Some(Ok(item))) => Some(item),
        _ => None,
    }
}

/// One test, the servers one after the other: two at once can be handed the same free port.
#[tokio::test(flavor = "multi_thread")]
async fn a_message_past_ten_mib_is_answered_and_the_flag_sets_the_limit() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    {
        let s = spawn_menger(free_port().await, &dir.path().join("a.db"), &[]).await;
        let c = client(s.port).await;
        let item = answered(&c, 11 << 20).await.expect("an 11 MiB message answered (plexus-transport's default drops it)");
        assert!(item.to_string().contains("padding"), "refused by its unknown parameter, so it was read: {item}");
    }
    // The same 11 MiB under the transport's own 10 MiB: what release cf06e82's push met.
    let s = spawn_menger(free_port().await, &dir.path().join("b.db"), &["--max-message-bytes".into(), (10u32 << 20).to_string()]).await;
    let c = client(s.port).await;
    assert!(answered(&c, 4096).await.is_some(), "a small message still answered");
    assert!(answered(&c, 11 << 20).await.is_none(), "an 11 MiB message past a 10 MiB limit is not taken");
}
