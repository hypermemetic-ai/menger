//! DEPLOY-25: **the webhook worker** — what POSTs each deploy request to its
//! environment's CI.
//!
//! The store keeps the request, its delivery's state and every attempt
//! ([`Store::delivery`], [`Store::delivery_attempted`]); this worker makes
//! the attempts, with no lock held: a request is never delivered on the
//! write that made it. It starts every delivery due when the server starts
//! ([`Store::deliveries_due`] — one interrupted by a restart picks up where
//! its last attempt left it), then each a `request` on `deployments.watch`
//! says is due. One task per delivery; a delivery already under way is not
//! started twice.
//!
//! **An attempt** POSTs the request as JSON ([`WebhookPayload`]) to the
//! webhook's URL — redirects not followed, [`ATTEMPT_TIMEOUT`] at most —
//! with the headers a receiver checks:
//!
//! | header | value |
//! |---|---|
//! | `X-Menger-Signature` | `sha256=` + the lower-case hex HMAC-SHA256 of the exact body, keyed by the webhook's secret |
//! | `X-Menger-Delivery` | the delivery's id — the same on every attempt of one request |
//! | `X-Menger-Event` | `deploy.requested` |
//! | `X-Menger-Attempt` | `1`, `2`, … |
//! | `Content-Type` | `application/json` |
//!
//! A 2xx is delivered. Anything else — another status, no answer — is
//! retried after [`SCHEDULE`]'s next wait (1 s, 5 s, 30 s, 2 min, 10 min);
//! after the last, the delivery is `failed` and no more are made. A request
//! canceled, or picked up by a CI's record, stops its delivery between
//! attempts.
//!
//! **The secret** keys the HMAC and goes nowhere else: not into a log line,
//! an error or an event (the store's [`Due`] does not print it).

use menger_tickets_api::deployments::{DeliveryAttempt, DeploymentsEvent, WebhookPayload};
use menger_tickets_api::store::{Due, Store, System};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

/// The waits after each failed attempt, in order: six attempts in all.
pub const SCHEDULE: [Duration; 5] = [Duration::from_secs(1), Duration::from_secs(5), Duration::from_secs(30), Duration::from_secs(120), Duration::from_secs(600)];

/// The longest one attempt waits for an answer.
pub const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);

/// `X-Menger-Event`'s value.
pub const EVENT: &str = "deploy.requested";

/// HMAC-SHA256 (RFC 2104) of `message` keyed by `key`.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    match key.len() > 64 {
        true => k[..32].copy_from_slice(&Sha256::digest(key)),
        false => k[..key.len()].copy_from_slice(key),
    }
    let (mut inner, mut outer) = ([0x36u8; 64], [0x5cu8; 64]);
    for i in 0..64 {
        inner[i] ^= k[i];
        outer[i] ^= k[i];
    }
    let inside = Sha256::new().chain_update(inner).chain_update(message).finalize();
    Sha256::new().chain_update(outer).chain_update(inside).finalize().into()
}

/// `X-Menger-Signature`'s value for `body`, keyed by `secret`.
pub fn signature(secret: &str, body: &[u8]) -> String {
    let mac = hmac_sha256(secret.as_bytes(), body);
    let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256={hex}")
}

/// The body a delivery POSTs — what the signature is of.
pub fn body(payload: &WebhookPayload) -> Vec<u8> {
    serde_json::to_vec(payload).expect("a payload serializes")
}

/// The worker, as its tasks share it.
struct Worker {
    store: Arc<Store>,
    system: Arc<System>,
    client: reqwest::Client,
    schedule: Vec<Duration>,
    under_way: Mutex<HashSet<u64>>,
}

/// Starts the worker for the life of the process, as the store's [`System`]
/// — the server's own upkeep — with [`SCHEDULE`].
pub fn spawn(store: Arc<Store>, system: Arc<System>) -> tokio::task::JoinHandle<()> {
    spawn_with(store, system, SCHEDULE.to_vec())
}

/// [`spawn`] with its own waits between attempts (a test's are short).
pub fn spawn_with(store: Arc<Store>, system: Arc<System>, schedule: Vec<Duration>) -> tokio::task::JoinHandle<()> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(ATTEMPT_TIMEOUT)
        .user_agent(concat!("menger/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("an HTTP client");
    let worker = Arc::new(Worker { store, system, client, schedule, under_way: Mutex::new(HashSet::new()) });
    tokio::spawn(worker.run())
}

impl Worker {
    async fn run(self: Arc<Self>) {
        // Subscribed first: nothing asked between the scan and the watch is lost.
        let mut heard = self.store.subscribe_deployments();
        self.start_due();
        loop {
            match heard.recv().await {
                Ok(DeploymentsEvent::Request { deployment, .. }) if deployment.requested.as_ref().is_some_and(|q| q.webhook.due()) => {
                    Arc::clone(&self).start(deployment.id)
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => self.start_due(),
                Err(RecvError::Closed) => break,
            }
        }
    }

    fn start_due(self: &Arc<Self>) {
        for id in self.store.deliveries_due() {
            Arc::clone(self).start(id);
        }
    }

    /// `request`'s delivery, unless it is under way already.
    fn start(self: Arc<Self>, request: u64) {
        if self.under_way.lock().expect("worker lock").insert(request) {
            tokio::spawn(async move {
                self.deliver(request).await;
                self.under_way.lock().expect("worker lock").remove(&request);
            });
        }
    }

    /// Every attempt `request` is due, until one is answered 2xx, the
    /// schedule ends, or it is due no more.
    async fn deliver(&self, request: u64) {
        let mut resumed = true;
        loop {
            let due = match self.store.delivery(request) {
                Ok(Some(due)) => due,
                Ok(None) => return,
                Err((attempt, why)) => {
                    self.kept(request, DeliveryAttempt { attempt, at: now(), status: None, error: Some(why) }, None).await;
                    return;
                }
            };
            // A delivery a restart interrupted waits out what is left of its wait.
            if std::mem::take(&mut resumed) {
                if let (Some(last), Some(wait)) = (due.last_at, self.wait_after(due.attempt - 1)) {
                    let left = (last + wait.as_secs() as i64 - now()).max(0);
                    tokio::time::sleep(Duration::from_secs(left as u64)).await;
                    continue;
                }
            }
            let attempt = self.post(&due).await;
            let delivered = attempt.status.is_some_and(|s| (200..300).contains(&s));
            let wait = (!delivered).then(|| self.wait_after(due.attempt)).flatten();
            let next_at = wait.map(|w| now() + w.as_secs().max(1) as i64);
            self.kept(request, attempt, next_at).await;
            match wait {
                Some(w) => tokio::time::sleep(w).await,
                None => return,
            }
        }
    }

    /// The wait after attempt `n` failed — `None` after the last.
    fn wait_after(&self, n: u32) -> Option<Duration> {
        (n >= 1).then(|| self.schedule.get(n as usize - 1).copied()).flatten()
    }

    /// One attempt: the POST, and what came of it.
    async fn post(&self, due: &Due) -> DeliveryAttempt {
        let body = body(&due.payload);
        let at = now();
        let sent = self
            .client
            .post(&due.url)
            .header("content-type", "application/json")
            .header("x-menger-event", EVENT)
            .header("x-menger-delivery", &due.delivery)
            .header("x-menger-attempt", due.attempt.to_string())
            .header("x-menger-signature", signature(due.secret(), &body))
            .body(body)
            .send()
            .await;
        let (status, error) = match sent {
            Ok(r) if r.status().is_success() => (Some(r.status().as_u16()), None),
            Ok(r) => (Some(r.status().as_u16()), Some(format!("answered {}", r.status()))),
            Err(e) => (None, Some(cause(&e.without_url()))),
        };
        match &error {
            None => tracing::info!("DEPLOY-25: request {} delivered to {} (attempt {})", due.request, host(&due.url), due.attempt),
            Some(e) => tracing::warn!("DEPLOY-25: request {}'s delivery to {} failed (attempt {}): {e}", due.request, host(&due.url), due.attempt),
        }
        DeliveryAttempt { attempt: due.attempt, at, status, error }
    }

    async fn kept(&self, request: u64, attempt: DeliveryAttempt, next_at: Option<i64>) {
        if let Err(e) = self.store.delivery_attempted(&self.system, request, attempt, next_at).await {
            tracing::warn!("DEPLOY-25: request {request}'s attempt was not kept: {e}");
        }
    }
}

/// An error in its own words — its innermost cause, which says what
/// happened (`Connection refused`, `operation timed out`).
fn cause(e: &(dyn std::error::Error + 'static)) -> String {
    let mut at = e;
    while let Some(next) = at.source() {
        at = next;
    }
    match at.to_string() {
        inner if inner == e.to_string() => inner,
        inner => format!("{e}: {inner}"),
    }
}

/// A URL's host, for a log line.
fn host(url: &str) -> &str {
    url.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or(url)
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests;
