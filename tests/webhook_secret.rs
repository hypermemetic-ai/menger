//! DEPLOY-25: a webhook's secret goes nowhere but the HMAC — no answer
//! (`set`, `show`, `request`, `list`), no `watch` event, no log line at any
//! level, through a failed attempt and a delivered one, and no `Debug` of
//! what the worker holds; and over the wire, `webhook show` says only that
//! one is set.
//!
//! A binary of its own, with a global subscriber at TRACE: every line the
//! process logs is in the capture, and no other test's thread shares it.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use menger::webhooks::{signature, spawn_with};
use menger_tickets_api::deployments::{DeploymentsEvent, Record, WebhookState};
use menger_tickets_api::store::{NewTicket, RequestAsk, Store};
use menger_tickets_api::types::Kind;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SECRET: &str = "the-secret-nobody-sees-0123456789";

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Log {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Log {
    type Writer = Log;
    fn make_writer(&'a self) -> Log {
        self.clone()
    }
}

type Posts = Arc<Mutex<Vec<(HeaderMap, Bytes)>>>;

/// 500 first, then 200.
async fn hook(State(posts): State<Posts>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let mut p = posts.lock().unwrap();
    p.push((headers, body));
    if p.len() == 1 { StatusCode::INTERNAL_SERVER_ERROR } else { StatusCode::OK }
}

fn ticket(title: &str, kind: Kind) -> NewTicket {
    NewTicket { epic: "MENGER".into(), title: title.into(), kind, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() }
}

#[tokio::test]
async fn the_secret_is_in_no_answer_event_or_log() {
    let log = Log::default();
    let subscriber = tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).with_writer(log.clone()).with_ansi(false).finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let posts: Posts = Posts::default();
    let app = axum::Router::new().route("/hook", axum::routing::post(hook)).with_state(Arc::clone(&posts));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let store = Store::new();
    let system = Arc::new(store.system().unwrap());
    store.create(&*system, ticket("menger", Kind::Scope)).await.unwrap();
    let rec = Record { project: "menger".into(), environment: "test".into(), state: "succeeded".into(), revision: "aaaaaaa".into(), ..Record::default() };
    store.record_deployment(&*system, rec, "ci".into()).await.unwrap();
    let store = Arc::new(store);
    let mut events = store.subscribe_deployments();
    let mut said = vec![serde_json::to_string(&store.set_webhook(&*system, "menger", "prod", &url, SECRET.into(), "owner".into()).await.unwrap()).unwrap()];
    said.push(serde_json::to_string(&store.webhook(&*system, "menger", "prod").unwrap()).unwrap());
    spawn_with(Arc::clone(&store), Arc::clone(&system), vec![Duration::from_millis(30); 3]);
    let ask = RequestAsk { project: "menger".into(), environment: "prod".into(), revision: Some("aaaaaaa".into()), ..RequestAsk::default() };
    let answer = store.request_deployment(&*system, ask, "owner".into()).await.unwrap();
    said.push(serde_json::to_string(&answer).unwrap());
    let DeploymentsEvent::Request { deployment, .. } = answer else { panic!() };
    let delivered = |s: &Store| {
        s.deployments(&Default::default(), None, None, None).unwrap().rows.into_iter().any(|e| {
            matches!(e, DeploymentsEvent::Deployment { deployment: d, .. } if d.requested.as_ref().is_some_and(|q| q.webhook == WebhookState::Delivered))
        })
    };
    for _ in 0..500 {
        if delivered(&store) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(delivered(&store), "delivered on the second attempt");
    said.push(format!("{:?}", store.delivery(deployment.id)));
    said.push(serde_json::to_string(&store.deployments(&Default::default(), None, None, None).unwrap().rows).unwrap());
    while let Ok(ev) = events.try_recv() {
        said.push(serde_json::to_string(&ev).unwrap());
    }
    let posts = posts.lock().unwrap().clone();
    assert_eq!(posts.len(), 2);
    // The receiver could check it: the secret is what signed the body.
    assert_eq!(posts[1].0["x-menger-signature"].to_str().unwrap(), signature(SECRET, &posts[1].1));
    let log = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("failed (attempt 1)") && log.contains("delivered to 127.0.0.1"), "the worker logged both attempts: {log}");
    assert!(said.len() >= 6, "{said:?}");
    for s in said.iter().chain([&log]) {
        assert!(!s.contains(SECRET), "the secret in: {s}");
    }
    assert!(said[1].contains("\"secret_set\":true"), "{}", said[1]);
}
