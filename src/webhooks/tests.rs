//! DEPLOY-25: the webhook against a local receiver — the signature a
//! receiver checks, a retry after a 500, giving up after the last attempt,
//! a cancel that stops the retries, and a URL that is neither https nor
//! loopback refused. The secret in no answer, event or log line is
//! `tests/webhook_secret.rs`'s — a binary of its own, so its log is the
//! worker's alone.

use super::*;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use menger_tickets_api::deployments::{Record, RequestState, WebhookState};
use menger_tickets_api::store::{NewTicket, RequestAsk};
use menger_tickets_api::types::Kind;
use std::sync::Mutex as StdMutex;

const SECRET: &str = "a-test-secret-0123456789abcdef";

/// What the receiver heard: each POST's headers and body.
#[derive(Clone, Default)]
struct Heard {
    posts: Arc<StdMutex<Vec<(HeaderMap, Bytes)>>>,
    /// The status of each answer, in turn; the last repeats.
    answers: Arc<StdMutex<Vec<u16>>>,
}

impl Heard {
    fn posts(&self) -> Vec<(HeaderMap, Bytes)> {
        self.posts.lock().unwrap().clone()
    }
}

async fn hook(State(h): State<Heard>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let n = {
        let mut posts = h.posts.lock().unwrap();
        posts.push((headers, body));
        posts.len()
    };
    let answers = h.answers.lock().unwrap();
    let code = answers.get(n - 1).or(answers.last()).copied().unwrap_or(200);
    StatusCode::from_u16(code).unwrap()
}

/// A receiver on a loopback port, answering `answers` in turn; its URL.
async fn receiver(answers: &[u16]) -> (String, Heard) {
    let heard = Heard { answers: Arc::new(StdMutex::new(answers.to_vec())), ..Heard::default() };
    let app = axum::Router::new().route("/hook", axum::routing::post(hook)).with_state(heard.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, heard)
}

fn ticket(title: &str, kind: Kind) -> NewTicket {
    NewTicket { epic: "MENGER".into(), title: title.into(), kind, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() }
}

/// MENGER-1, MENGER-2 linked to `aaaaaaa`; test runs `aaaaaaa`; prod
/// promotes from test; prod's webhook is `url`.
async fn tracker(url: Option<&str>) -> (Arc<Store>, Arc<System>) {
    let store = Store::new();
    let system = Arc::new(store.system().unwrap());
    store.create(&*system, ticket("menger", Kind::Scope)).await.unwrap();
    store.create(&*system, ticket("work", Kind::Build)).await.unwrap();
    store.link_commits(system.permit("MENGER-2"), vec!["aaaaaaa".parse().unwrap()]).await.unwrap();
    let rec = Record { project: "menger".into(), environment: "test".into(), state: "succeeded".into(), revision: "aaaaaaa".into(), release: Some("r1".into()), ..Record::default() };
    store.record_deployment(&*system, rec, "ci".into()).await.unwrap();
    store.set_promotes(&*system, "menger", "prod", Some("test"), "owner".into()).await.unwrap();
    if let Some(url) = url {
        store.set_webhook(&*system, "menger", "prod", url, SECRET.into(), "owner".into()).await.unwrap();
    }
    (Arc::new(store), system)
}

async fn ask(store: &Store, system: &System) -> u64 {
    let ask = RequestAsk { project: "menger".into(), environment: "prod".into(), note: Some("ship it".into()), ..RequestAsk::default() };
    match store.request_deployment(system, ask, "owner".into()).await.unwrap() {
        DeploymentsEvent::Request { deployment, .. } => deployment.id,
        other => panic!("{other:?}"),
    }
}

fn webhook_of(store: &Store, id: u64) -> menger_tickets_api::deployments::DeployRequest {
    let page = store.deployments(&Default::default(), None, None, None).unwrap();
    page.rows
        .into_iter()
        .find_map(|e| match e {
            DeploymentsEvent::Deployment { deployment, .. } if deployment.id == id => deployment.requested,
            _ => None,
        })
        .expect("the request")
}

/// Waits until `f` holds of the request's rest (at most 10 s).
async fn until(store: &Store, id: u64, f: impl Fn(&menger_tickets_api::deployments::DeployRequest) -> bool) -> menger_tickets_api::deployments::DeployRequest {
    for _ in 0..500 {
        let q = webhook_of(store, id);
        if f(&q) {
            return q;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("not reached: {:?}", webhook_of(store, id));
}

fn quick() -> Vec<Duration> {
    vec![Duration::from_millis(30); 3]
}

#[test]
fn the_hmac_is_rfc_4231s() {
    let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
    let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    // A key longer than the block is hashed first (RFC 4231 case 6).
    let mac = hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First");
    let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54");
}

/// What a receiver checks: the signature is the HMAC of the exact body,
/// keyed by the secret; the delivery id is the request's; the body says
/// the request whole.
#[tokio::test]
async fn a_delivery_is_signed_and_says_the_request() {
    let (url, heard) = receiver(&[200]).await;
    let (store, system) = tracker(Some(&url)).await;
    spawn_with(Arc::clone(&store), Arc::clone(&system), quick());
    let id = ask(&store, &system).await;
    let q = until(&store, id, |q| q.webhook == WebhookState::Delivered).await;
    let posts = heard.posts();
    assert_eq!(posts.len(), 1);
    let (headers, body) = &posts[0];
    let h = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    assert_eq!(h("x-menger-signature"), signature(SECRET, body), "sha256=<hex HMAC of the body>");
    assert!(h("x-menger-signature").starts_with("sha256=") && h("x-menger-signature").len() == 7 + 64);
    assert_ne!(h("x-menger-signature"), signature("another-secret-0123456789", body));
    assert_eq!(Some(h("x-menger-delivery")), q.delivery);
    assert_eq!((h("x-menger-event").as_str(), h("x-menger-attempt").as_str(), h("content-type").as_str()), (EVENT, "1", "application/json"));
    let json: serde_json::Value = serde_json::from_slice(body).unwrap();
    assert_eq!(json["request"], id);
    assert_eq!((json["project"].as_str(), json["environment"].as_str()), (Some("MENGER"), Some("prod")));
    assert_eq!((json["revision"].as_str(), json["release"].as_str(), json["from"].as_str()), (Some("aaaaaaa"), Some("r1"), Some("test")));
    assert_eq!((json["requested_by"].as_str(), json["note"].as_str()), (Some("owner"), Some("ship it")));
    assert!(json["at"].as_str().is_some_and(|a| a.ends_with('Z')), "{json}");
    assert_eq!(q.attempts.len(), 1);
    assert_eq!((q.attempts[0].status, q.attempts[0].error.as_deref()), (Some(200), None));
    assert_eq!(q.state, RequestState::Pending, "delivered is not picked up");
}

/// A 500 is retried after the schedule's wait; the next 2xx delivers it.
#[tokio::test]
async fn a_500_is_retried_then_delivered() {
    let (url, heard) = receiver(&[500, 200]).await;
    let (store, system) = tracker(Some(&url)).await;
    spawn_with(Arc::clone(&store), Arc::clone(&system), quick());
    let id = ask(&store, &system).await;
    let q = until(&store, id, |q| q.webhook == WebhookState::Delivered).await;
    assert_eq!(heard.posts().len(), 2);
    let statuses: Vec<Option<u16>> = q.attempts.iter().map(|a| a.status).collect();
    assert_eq!(statuses, [Some(500), Some(200)]);
    assert!(q.attempts[0].error.as_deref().is_some_and(|e| e.contains("500")), "{:?}", q.attempts[0]);
    assert_eq!(q.next_attempt_at, None);
    let ids: HashSet<String> = heard.posts().iter().map(|(h, _)| h["x-menger-delivery"].to_str().unwrap().to_string()).collect();
    assert_eq!(ids.len(), 1, "one delivery id on every attempt");
}

/// After the last wait's attempt fails, the delivery is failed and no more
/// attempts are made — a receiver answering 500, and one not there at all.
#[tokio::test]
async fn it_gives_up_after_the_last_attempt() {
    let (url, heard) = receiver(&[500]).await;
    let (store, system) = tracker(Some(&url)).await;
    spawn_with(Arc::clone(&store), Arc::clone(&system), quick());
    let id = ask(&store, &system).await;
    let q = until(&store, id, |q| q.webhook == WebhookState::Failed).await;
    assert_eq!(q.attempts.len(), 4, "the first, then one after each of three waits");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(heard.posts().len(), 4, "no attempt after the last");
    assert_eq!(webhook_of(&store, id).attempts.len(), 4);

    // Nothing listening: no status, the cause in a line.
    let gone = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", gone.local_addr().unwrap());
    drop(gone);
    let (store, system) = tracker(Some(&url)).await;
    spawn_with(Arc::clone(&store), Arc::clone(&system), vec![Duration::from_millis(10)]);
    let id = ask(&store, &system).await;
    let q = until(&store, id, |q| q.webhook == WebhookState::Failed).await;
    assert_eq!(q.attempts.len(), 2);
    assert!(q.attempts.iter().all(|a| a.status.is_none() && a.error.as_ref().is_some_and(|e| !e.is_empty() && e.len() <= 200)), "{:?}", q.attempts);
}

/// A request canceled while its delivery waits to retry sends no more.
#[tokio::test]
async fn a_cancel_stops_the_retries() {
    let (url, heard) = receiver(&[503]).await;
    let (store, system) = tracker(Some(&url)).await;
    spawn_with(Arc::clone(&store), Arc::clone(&system), vec![Duration::from_millis(400); 5]);
    let id = ask(&store, &system).await;
    until(&store, id, |q| q.webhook == WebhookState::Retrying).await;
    store.cancel_request(&*system, id, "owner".into()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(900)).await;
    let q = webhook_of(&store, id);
    assert_eq!((q.state, q.webhook), (RequestState::Canceled, WebhookState::Stopped));
    assert_eq!(heard.posts().len(), 1);
}

/// No webhook: kept, `none`, never sent.
#[tokio::test]
async fn no_webhook_is_no_ci_configured() {
    let (store, system) = tracker(None).await;
    spawn_with(Arc::clone(&store), Arc::clone(&system), quick());
    let id = ask(&store, &system).await;
    let q = webhook_of(&store, id);
    assert_eq!((q.webhook, q.delivery.as_deref(), q.attempts.len()), (WebhookState::None, None, 0));
    assert!(store.deliveries_due().is_empty());
}

/// A webhook is https, or http to this machine: anywhere else over plain
/// http is refused, and nothing is set.
#[tokio::test]
async fn a_plain_http_url_elsewhere_is_refused() {
    let (store, system) = tracker(None).await;
    for url in ["http://ci.example.com/hook", "http://10.1.2.3/hook", "ftp://ci.example.com/"] {
        let e = store.set_webhook(&*system, "menger", "prod", url, SECRET.into(), "owner".into()).await.unwrap_err();
        assert!(e.to_string().contains("not a webhook URL"), "{e}");
    }
    let DeploymentsEvent::Webhook { webhook } = store.webhook(&*system, "menger", "prod").unwrap() else { panic!() };
    assert!(webhook.url.is_none() && !webhook.secret_set);
    store.set_webhook(&*system, "menger", "prod", "https://ci.example.com/hook", SECRET.into(), "owner".into()).await.unwrap();
    store.set_webhook(&*system, "menger", "prod", "http://localhost:6091/hook", SECRET.into(), "owner".into()).await.unwrap();
}
