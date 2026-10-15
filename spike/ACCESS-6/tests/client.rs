//! Question 3: how the emitted Rust client reaches a child method.
//!
//! A recording transport shows the method each client puts on the wire;
//! `live_*` (ignored unless the spike server runs on :4670, `run.sh`)
//! sends it to the hub.

use access6_spike::{InputsClient, Status, TicketId, TicketsClient};
use futures::StreamExt;
use plexus_client::{ItemStream, MethodRef, Transport};
use serde_json::Value;
use std::sync::Mutex;

#[derive(Default)]
struct Recording(Mutex<Vec<(String, Value)>>);

impl Transport for Recording {
    fn call(&self, method: MethodRef, params: Value) -> ItemStream<'_> {
        self.0.lock().unwrap().push((method.qualified(), params));
        Box::pin(futures::stream::empty())
    }
}

fn id(s: &str) -> TicketId {
    TicketId::try_from(s.to_string()).unwrap()
}

#[tokio::test]
async fn the_parent_client_sends_its_namespace_and_typed_params() {
    let t = Recording::default();
    let client = TicketsClient::new(&t);
    let _ = client.set_status(id("mng-1"), Status::Coding).collect::<Vec<_>>().await;
    let sent = t.0.lock().unwrap().clone();
    println!("parent: {sent:?}");
    assert_eq!(sent[0].0, "tickets.set_status");
    assert_eq!(sent[0].1, serde_json::json!({"id": "MNG-1", "status": "Coding"}));
}

#[tokio::test]
async fn the_child_client_sends_its_own_namespace_not_the_path() {
    let t = Recording::default();
    let inputs = InputsClient::new(&t);
    let _ = inputs.answer(id("ACCESS-6"), Status::InReview).collect::<Vec<_>>().await;
    let sent = t.0.lock().unwrap().clone();
    println!("child: {sent:?}");
    // The hub routes `tickets.inputs.answer`; the child's client says
    // `inputs.answer` — its own `namespace`, with no parent in it.
    assert_eq!(sent[0].0, "inputs.answer");
}

#[tokio::test]
#[ignore = "needs the spike server on :4670 (run.sh)"]
async fn live_child_client_against_the_hub() {
    let ws = plexus_client::WsTransport::connect("ws://127.0.0.1:4670", "spike").await.unwrap();
    let inputs = InputsClient::new(&ws);
    let got = inputs.answer_all(id("ACCESS-6"), Status::InReview).await;
    println!("child via its own client: {got:?}");
    let parent = TicketsClient::new(&ws);
    let got = parent.get_all(id("MNG-1")).await;
    println!("parent, anonymous: {got:?}");
}

/// What the frame can do today: a transport that puts the parent's path in
/// front of the child client's method (`inputs.answer` → `tickets.inputs.answer`).
struct Under<'a, T> {
    inner: &'a T,
    path: &'static str,
}

impl<T: Transport> Transport for Under<'_, T> {
    fn call(&self, method: MethodRef, params: Value) -> ItemStream<'_> {
        self.inner.call(MethodRef::new(self.path, method.method()), params)
    }
}

#[tokio::test]
async fn a_path_prefixing_transport_reaches_the_child() {
    let t = Recording::default();
    let inputs = InputsClient::new(Under { inner: &t, path: "tickets.inputs" });
    let _ = inputs.answer(id("ACCESS-6"), Status::InReview).collect::<Vec<_>>().await;
    let sent = t.0.lock().unwrap().clone();
    assert_eq!(sent[0].0, "tickets.inputs.answer");
}

#[tokio::test]
#[ignore = "needs the spike server on :4670 (run.sh)"]
async fn live_child_client_under_the_parents_path() {
    let ws = plexus_client::WsTransport::connect("ws://127.0.0.1:4670", "spike").await.unwrap();
    let inputs = InputsClient::new(Under { inner: &ws, path: "tickets.inputs" });
    let got = inputs.answer_all(id("ACCESS-6"), Status::InReview).await;
    println!("child under tickets.inputs, anonymous: {got:?}");
}
