//! PERF-23: a call's acknowledgement on a local socket — the time from its
//! frame sent to its subscription id back, what a page waits on before any
//! data — is the service's own and well under 5 ms: the gate's check, the
//! dispatch, the activation's start, the first write. PERF-22's 45 ms on the
//! page's clock is the page's main thread (the engine's mount and the first
//! frame's layout and paint run before the socket's messages are handed to
//! it); the service answered each of those calls in under a millisecond. This
//! holds that floor: the board's first calls one by one on a warm
//! connection, and the boot's burst — the watches and calls a page opens at
//! once — each acknowledged, by its median, under 5 ms (a Nagle stall or a
//! per-call wait is 40 ms and more).

mod support;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const ACK: Duration = Duration::from_millis(5);

fn params(method: &str, p: Value) -> ObjectParams {
    let mut o = ObjectParams::new();
    o.insert("method", method).unwrap();
    o.insert("params", p).unwrap();
    o
}

fn median(mut xs: Vec<Duration>) -> Duration {
    xs.sort();
    xs[xs.len() / 2]
}

/// The board's first calls, as the page asks them.
fn board_calls() -> Vec<(&'static str, Value)> {
    vec![
        // V2-25: the top asks no epics.current any more — the board is the organization's whole set.
        ("tickets.health", json!({})),
        ("auth.whoami", json!({})),
        ("tickets.get", json!({ "id": "BAG-2" })),
        ("tickets.list", json!({ "limit": 1, "summary": true })),
        ("tickets.nav.groups", json!({})),
        ("ui.versions", json!({})),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_is_acknowledged_in_under_5_ms() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let port = support::free_port().await;
    let seed = ["--seed-synthetic", "1000", "--seed-value", "6"].map(String::from);
    let _server = support::spawn_menger(port, &dir.path().join("t.db"), &seed).await;
    let c = support::connect(port).await;
    // Warm: the first call of a connection pays the connection's own setup.
    assert!(!support::call(&c, "tickets.health", json!({})).await.is_empty());

    // One by one: each call's frame → its acknowledgement (subscribe returns
    // when the subscription id is back), then drained to its end.
    let mut worst = Vec::new();
    for (method, p) in board_calls() {
        let mut acks = Vec::new();
        for _ in 0..15 {
            let t = Instant::now();
            let mut s: Subscription<Value> = c.subscribe("menger.call", params(method, p.clone()), "menger.call_unsub").await.unwrap();
            acks.push(t.elapsed());
            while let Ok(Some(Ok(item))) = tokio::time::timeout(Duration::from_secs(5), s.next()).await {
                assert_ne!(item["type"], "error", "{method}: {item}");
                if item["type"] != "data" {
                    break;
                }
            }
        }
        let m = median(acks);
        worst.push((method, m));
    }
    eprintln!("one by one, the median acknowledgement: {worst:?}");
    for (method, m) in &worst {
        assert!(*m < ACK, "{method}'s acknowledgement: median {m:?} (every call's: {worst:?})");
    }

    // The boot's burst: the page's watches open, then its calls sent at once
    // on the same connection, each acknowledged under 5 ms of its send.
    let mut bursts: Vec<Vec<Duration>> = vec![Vec::new(); board_calls().len()];
    for _ in 0..9 {
        let _ui = c.subscribe::<Value, _>("menger.call", params("ui.watch", json!({})), "menger.call_unsub").await.unwrap();
        let _tickets = c.subscribe::<Value, _>("menger.call", params("tickets.watch", json!({})), "menger.call_unsub").await.unwrap();
        let t = Instant::now();
        let acks = futures::future::join_all(board_calls().into_iter().map(|(method, p)| {
            let c = &c;
            async move {
                let s: Subscription<Value> = c.subscribe("menger.call", params(method, p), "menger.call_unsub").await.unwrap();
                (t.elapsed(), s)
            }
        }))
        .await;
        for (i, (d, _)) in acks.iter().enumerate() {
            bursts[i].push(*d);
        }
    }
    let burst: Vec<_> = board_calls().iter().map(|(m, _)| *m).zip(bursts.into_iter().map(median)).collect();
    eprintln!("in the boot's burst: {burst:?}");
    for (method, m) in &burst {
        assert!(*m < ACK, "{method} in the boot's burst: median {m:?} (every call's: {burst:?})");
    }
}
