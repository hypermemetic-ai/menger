//! AUTH-24: the adversarial review of AUTH-22, its reproductions as tests —
//! each one the reviewer's probe, run against the fix.
//!
//! | # | finding | test |
//! | --- | --- | --- |
//! | 1 | a burst of guesses outran the backoff (132 codes, 155 TOTP codes reached WorkOS where 5 were allowed) | `a_burst_of_wrong_codes_reaches_the_backend_no_more_than_the_allowance`, `a_burst_of_wrong_totp_codes_…` |
//! | 2 | behind the edge, every client one bucket | `wiring::tests` (startup refusal); here `an_untrusted_forwarded_header_is_not_believed` |
//! | 3 | a third party locks the owner out | `a_third_party_cannot_lock_the_owner_out` |
//! | 4 | a flood of social round trips evicts sign-ins in progress | `a_flood_cannot_evict_another_browser_s_flow`, `flows_are_throttled_per_client` |
//! | 5 | a dropped sign-out leaves the session alive | `sign_out_ends_the_session_before_the_backend_is_told` |
//! | 6 | IPv6: each address its own bucket | `an_ipv6_client_is_its_network` |
//! | 7 | the CSP hashed whatever the response held | `page::tests` |
//! | 8 | development backends anywhere | `wiring::tests` |
//! | 9 | the address in the SSO URL | `signin.rs`'s `an_sso_domain_goes_to_its_idp…` |
//! | 11 | a swap midway labelled a sign-in with the new backend | `a_swap_midway_finishes_with_the_backend_that_started_it` |
//! | 12 | a non-ASCII `next`; a fail-open refresh | `a_non_ascii_next_is_percent_encoded`, `a_refresh_that_cannot_be_made_after_expiry_ends_the_session` |

mod support;

use std::time::{Duration, Instant};

use plexus_login::throttle::{Limits, Rate};
use support::*;

const WORKOS_MAGIC: &str = "urn:workos:oauth:grant-type:magic-auth:code";
const WORKOS_TOTP: &str = "urn:workos:oauth:grant-type:mfa-totp";

#[tokio::test]
async fn a_burst_of_wrong_codes_reaches_the_backend_no_more_than_the_allowance() {
    let under = Under::workos().await;
    let a = app(under.clone()).await;
    let b = Browser::new(&a);
    let csrf = b.csrf().await;
    let sent = b.post("/login/email", &[("csrf", &csrf), ("email", "burst@example.test")]).await;
    let token = sent.csrf();
    let statuses = futures::future::join_all((0..200).map(|i| {
        let (b, token) = (&b, token.clone());
        async move { b.post("/login/code", &[("csrf", &token), ("code", &format!("{:06}", 900_000 + i))]).await.status }
    }))
    .await;
    let reached = under.fake().auths(WORKOS_MAGIC);
    let allowance = Limits::default().try_per_address_client.free as usize;
    assert!(reached <= allowance, "{reached} guesses reached WorkOS; the allowance is {allowance}");
    assert_eq!(statuses.iter().filter(|s| **s == 401).count(), reached);
    assert_eq!(statuses.iter().filter(|s| **s == 429).count(), 200 - reached);
}

#[tokio::test]
async fn a_burst_of_wrong_totp_codes_reaches_the_backend_no_more_than_the_allowance() {
    let under = Under::workos().await;
    let a = app(under.clone()).await;
    let b = Browser::new(&a);
    let email = "burst-totp@example.test";
    b.sign_in_by_email(&under, email, "/").await;
    let acct = b.get("/login/account").await;
    let shown = b.post("/login/account/totp", &[("csrf", &acct.csrf())]).await;
    let secret = between(&shown.body, r#"id="totp-secret">"#, "<").unwrap();
    b.post("/login/account/totp/confirm", &[("csrf", &shown.csrf()), ("code", &totp_now(&secret))]).await;
    b.post("/logout", &[]).await;
    let csrf = b.csrf().await;
    let sent = b.post("/login/email", &[("csrf", &csrf), ("email", email)]).await;
    let code = under.code_for(email).await;
    let challenge = b.post("/login/code", &[("csrf", &sent.csrf()), ("code", &code)]).await;
    assert!(challenge.body.contains("Two-step verification"));
    let token = challenge.csrf();
    let wrong = wrong_totp(&secret);
    futures::future::join_all((0..200).map(|_| {
        let (b, token, wrong) = (&b, token.clone(), wrong.clone());
        async move { b.post("/login/continue", &[("csrf", &token), ("code", &wrong)]).await.status }
    }))
    .await;
    let reached = under.fake().auths(WORKOS_TOTP);
    let allowance = Limits::default().totp_per_user.free as usize;
    assert!(reached <= allowance, "{reached} TOTP guesses reached WorkOS; the allowance is {allowance}");
    assert!(b.me().await.is_none());
}

#[tokio::test]
async fn a_third_party_cannot_lock_the_owner_out() {
    let under = Under::workos().await;
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], ..Tweaks::default() }).await;
    let mut attacker = Browser::new(&a);
    attacker.xff = Some("198.51.100.66".into());
    let csrf = attacker.csrf().await;
    let sent = attacker.post("/login/email", &[("csrf", &csrf), ("email", "victim@example.test")]).await;
    for i in 0..6 {
        attacker.post("/login/code", &[("csrf", &sent.csrf()), ("code", &format!("00000{i}"))]).await;
    }
    let r = attacker.post("/login/code", &[("csrf", &sent.csrf()), ("code", "000009")]).await;
    assert_eq!(r.status, 429, "the attacker is locked");
    let mut victim = Browser::new(&a);
    victim.xff = Some("203.0.113.5".into());
    let r = victim.sign_in_by_email(&under, "victim@example.test", "/app/v").await;
    assert_eq!((r.status, r.location.as_deref()), (303, Some("/app/v")), "{}", r.body);
}

#[tokio::test]
async fn a_flood_cannot_evict_another_browser_s_flow() {
    let under = Under::memory();
    let limits = Limits { flows_total: 50, flows_held_per_client: 5, flows_per_client: Rate { max: 10_000, window: 600 }, ..Limits::default() };
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], limits, ..Tweaks::default() }).await;
    let mut victim = Browser::new(&a);
    victim.xff = Some("203.0.113.5".into());
    let out = victim.social("github", "/app/v").await;
    assert_eq!(out.status, 303);
    let back = out.location.unwrap();
    // One client floods: it only rotates its own few flows.
    let mut one = Browser::new(&a);
    one.xff = Some("198.51.100.1".into());
    for _ in 0..100 {
        one.social("google", "/").await;
    }
    assert!(a.login.signin().unwrap().flows_held() <= 1 + 5, "one client holds at most its share");
    // Many clients flood: the table fills and refuses; nobody is evicted.
    let mut refused = 0;
    for i in 0..120 {
        let mut b = Browser::new(&a);
        b.xff = Some(format!("192.0.2.{i}"));
        if b.social("google", "/").await.status == 503 {
            refused += 1;
        }
    }
    assert!(refused > 0, "a full table refuses");
    assert!(a.login.signin().unwrap().flows_held() <= 50);
    let done = victim.get(&back).await;
    assert_eq!((done.status, done.location.as_deref()), (303, Some("/app/v")), "{}", done.body);
}

#[tokio::test]
async fn flows_are_throttled_per_client() {
    let under = Under::memory();
    let limits = Limits { flows_per_client: Rate { max: 5, window: 600 }, ..Limits::default() };
    let a = app_with(under.clone(), Tweaks { limits, ..Tweaks::default() }).await;
    // Eight browsers of one client (one address).
    let mut statuses = Vec::new();
    for _ in 0..8 {
        statuses.push(Browser::new(&a).social("google", "/").await.status);
    }
    assert_eq!(statuses.iter().filter(|s| **s == 303).count(), 5, "{statuses:?}");
    assert!(statuses.contains(&429));
}

#[tokio::test]
async fn sign_out_ends_the_session_before_the_backend_is_told() {
    let under = Under::workos().await;
    under.fake().state.lock().unwrap().revoke_delay_ms = 3000;
    let a = app(under.clone()).await;
    let b = Browser::new(&a);
    b.sign_in_by_email(&under, "leaving@example.test", "/").await;
    let started = Instant::now();
    let r = b.post("/logout", &[]).await;
    assert_eq!(r.status, 303);
    assert!(started.elapsed() < Duration::from_millis(1500), "the answer does not wait on WorkOS");
    assert!(b.me().await.is_none(), "ended at once");
    let revoked = || under.fake().state.lock().unwrap().revoked.len();
    for _ in 0..50 {
        if revoked() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(revoked(), 1, "and WorkOS was told, after");
}

#[tokio::test]
async fn an_ipv6_client_is_its_network() {
    let under = Under::workos().await;
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], ..Tweaks::default() }).await;
    let mut ok = 0;
    for i in 1..=40 {
        let mut b = Browser::new(&a);
        b.xff = Some(format!("2001:db8:1:2::{i:x}"));
        let csrf = b.csrf().await;
        if b.post("/login/email", &[("csrf", &csrf), ("email", &format!("v6-{i}@d{i}.example"))]).await.status == 200 {
            ok += 1;
        }
    }
    let per_client = Limits::default().send_per_client.max;
    assert_eq!(ok, per_client, "one /64, one client");
    assert!(under.fake().calls_starting("organizations:") <= per_client, "SSO lookups bounded with the sends");
    let mut other = Browser::new(&a);
    other.xff = Some("2001:db8:1:3::1".into());
    let csrf = other.csrf().await;
    assert_eq!(other.post("/login/email", &[("csrf", &csrf), ("email", "v6-other@example.test")]).await.status, 200, "another /64 is another client");
}

#[tokio::test]
async fn an_untrusted_forwarded_header_is_not_believed() {
    // Loopback (no --trusted-proxy needed): the peer is the client, whatever
    // it forwards; and the server says so in its log.
    let under = Under::memory();
    let limits = Limits { send_per_client: Rate { max: 2, window: 600 }, ..Limits::default() };
    let a = app_with(under.clone(), Tweaks { limits, ..Tweaks::default() }).await;
    let mut statuses = Vec::new();
    for i in 0..3 {
        let mut b = Browser::new(&a);
        b.xff = Some(format!("203.0.113.{i}"));
        let csrf = b.csrf().await;
        statuses.push(b.post("/login/email", &[("csrf", &csrf), ("email", &format!("x{i}@example.test"))]).await.status);
    }
    assert_eq!(statuses, [200, 200, 429]);
    assert!(log_text().contains("X-Forwarded-For arrives from a peer that is not a trusted proxy"));
}

#[tokio::test]
async fn a_swap_midway_finishes_with_the_backend_that_started_it() {
    let mem = Under::memory();
    let a = app(mem.clone()).await;
    let b = Browser::new(&a);
    let csrf = b.csrf().await;
    let sent = b.post("/login/email", &[("csrf", &csrf), ("email", "midway@example.test")]).await;
    let code = mem.code_for("midway@example.test").await;
    let workos = Under::workos().await;
    a.wired.swap_identity(workos.backend());
    let r = b.post("/login/code", &[("csrf", &sent.csrf()), ("code", &code)]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert!(b.me().await.unwrap()["principal"].as_str().unwrap().starts_with("oidc:memory/"), "labelled by the backend that signed it in");
    assert_eq!(workos.fake().auths(WORKOS_MAGIC), 0, "the new backend was never asked");
}

#[tokio::test]
async fn a_non_ascii_next_is_percent_encoded() {
    let under = Under::memory();
    let a = app(under.clone()).await;
    let r = Browser::new(&a).sign_in_by_email(&under, "accent@example.test", "/é").await;
    assert_eq!(r.location.as_deref(), Some("/%C3%A9"));
}

#[tokio::test]
async fn a_refresh_that_cannot_be_made_after_expiry_ends_the_session() {
    let under = Under::workos().await;
    let a = app(under.clone()).await;
    let tokens = a.login.tokens().unwrap().clone();
    let fresh = Browser::new(&a);
    fresh.sign_in_by_email(&under, "fresh@example.test", "/").await;
    under.fake().state.lock().unwrap().token_secs = 2;
    let stale = Browser::new(&a);
    stale.sign_in_by_email(&under, "stale@example.test", "/").await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    under.fake().state.lock().unwrap().refresh_unavailable = true;
    let r = tokens.refresh_due(100_000).await;
    assert_eq!(r.ended, 1, "{r:?}");
    assert!(stale.me().await.is_none(), "expired and unrefreshable: fail closed");
    assert!(fresh.me().await.is_some(), "still valid: tried again next pass");
}
