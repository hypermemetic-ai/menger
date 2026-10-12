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
//!
//! AUTH-25, the re-review:
//!
//! | # | finding | test |
//! | --- | --- | --- |
//! | N1 | a full throttle table refused new visitors for a day | `throttle::tests` (100k keys), `a_key_flood_leaves_new_visitors_served` |
//! | N2 | a full flow table answered everyone 503 | `a_thousand_slash_64_flow_flood_leaves_an_unrelated_visitor_able_to_sign_in`, `a_flood_cannot_evict_another_browser_s_flow` |
//! | N3 | the global send ceiling stopped every code and SSO | `a_hundred_slash_64_send_flood_leaves_another_network_able_to_send_and_sso_to_route` |
//! | N4, #3 | ten clients locked the owner out | `ten_foreign_clients_cannot_stop_the_owner_s_code_in_the_owner_s_flow` |
//! | N5 | sends to one address unbounded | `strangers_send_an_address_three_codes_a_day` |
//! | N6 | a NAT neighbour's flows evicted | `a_nat_neighbour_s_flow_survives_twenty_of_another_s` |
//! | N8 | a 401/403 refunded the guess | `signin::routes::tests`, `throttle::tests` |
//! | N9 | the proxy check read only --public-url; `none` silenced the warning | `wiring::tests`, `none_still_warns_when_a_forwarded_header_arrives` |
//! | social token | one page's token drove a flood | `one_page_s_token_starts_one_flow` |
//!
//! AUTH-26, the third review:
//!
//! | # | finding | test |
//! | --- | --- | --- |
//! | R1 | wide guess tiers refused a whole /16 or v6 /32 the right code | `a_slash_16_of_wrong_guesses_cannot_stop_a_bystander_s_right_code`, `throttle::tests` |
//! | R3 | eviction of all-locked tables spared the flood | `throttle::tests::churn_cannot_push_out_an_owner_s_remembered_network` |
//! | R6 | a stale page answered 403; a double submit answered "already used" | `a_page_left_open_is_drawn_again_not_refused`, `a_double_submitted_email_form_shows_the_code_step` |

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
    let allowance = Limits::default().code_tries_per_flow as usize;
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
    let big = Rate { max: 100_000, window: 600 };
    let limits = Limits { flows_total: 50, flows_per_client: [big, big, big], ..Limits::default() };
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], limits, ..Tweaks::default() }).await;
    let mut victim = Browser::new(&a);
    victim.xff = Some("203.0.113.5".into());
    let out = victim.social("github", "/app/v").await;
    assert_eq!(out.status, 303);
    let back = out.location.unwrap();
    // 120 clients of one neighbourhood flood a 50-flow table: nothing is
    // refused, and room comes from the flooders' own newest flows.
    for i in 0..120 {
        let mut b = Browser::new(&a);
        b.xff = Some(format!("192.0.2.{i}"));
        assert_eq!(b.social("google", "/").await.status, 303, "no flow is ever refused");
    }
    assert!(a.login.signin().unwrap().flows_held() <= 50);
    let done = victim.get(&back).await;
    assert_eq!((done.status, done.location.as_deref()), (303, Some("/app/v")), "{}", done.body);
}

#[tokio::test]
async fn flows_are_throttled_per_client() {
    let under = Under::memory();
    let limits = Limits { flows_per_client: [Rate { max: 5, window: 600 }, Rate { max: 1000, window: 600 }, Rate { max: 1000, window: 600 }], ..Limits::default() };
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
    let per_client = Limits::default().send_per_client[0].max;
    assert_eq!(ok, per_client, "one /64, one client");
    assert!(under.fake().calls_starting("organizations:") <= Limits::default().sso_per_client[0].max, "SSO lookups bounded by their own limit");
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
    let limits = Limits { send_per_client: [Rate { max: 2, window: 600 }, Rate { max: 100, window: 600 }, Rate { max: 500, window: 600 }], ..Limits::default() };
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

// ── AUTH-25 ────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_key_flood_leaves_new_visitors_served() {
    // 3,000 keys from distinct /64s in distinct /48s into tables of 500:
    // the tables evict the flood's unlocked keys and a new visitor is
    // served (the 100,000-key version is throttle::tests).
    let under = Under::memory();
    let limits = Limits { table_cap: 500, ..Limits::default() };
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], limits, ..Tweaks::default() }).await;
    let page = Browser::new(&a).get("/login").await;
    let token = page.csrf();
    let cookie = page.headers.get_all("set-cookie").iter().map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).find(|c| c.starts_with("toy_flow")).unwrap();
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    for i in 0..3000u32 {
        let _ = http
            .post(format!("{}/login/social", a.base))
            .header("cookie", &cookie)
            .header("origin", &a.base)
            .header("x-forwarded-for", format!("2001:db8:{:x}:{:x}::1", i / 7, i))
            .form(&[("csrf", token.as_str()), ("provider", "google")])
            .send()
            .await;
    }
    let mut fresh = Browser::new(&a);
    fresh.xff = Some("100.64.9.9".into());
    let out = fresh.social("google", "/app/f").await;
    assert_eq!(out.status, 303, "{}", out.body);
    let done = fresh.get(&out.location.unwrap()).await;
    assert_eq!(done.location.as_deref(), Some("/app/f"));
}

#[tokio::test]
async fn a_thousand_slash_64_flow_flood_leaves_an_unrelated_visitor_able_to_sign_in() {
    let under = Under::memory();
    let limits = Limits { flows_total: 300, ..Limits::default() };
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], limits, ..Tweaks::default() }).await;
    let mut early = Browser::new(&a);
    early.xff = Some("203.0.113.5".into());
    let back = early.social("github", "/app/e").await.location.unwrap();
    let mut started = 0;
    for i in 0..1000u32 {
        let mut b = Browser::new(&a);
        b.xff = Some(format!("2001:db8:1:{i:x}::1"));
        let s = b.social("google", "/").await.status;
        assert_ne!(s, 503, "never a global refusal");
        if s == 303 {
            started += 1;
        }
    }
    assert!(started <= Limits::default().flows_per_client[1].max, "one /48's budget: {started}");
    assert!(a.login.signin().unwrap().flows_held() <= 300);
    let mut fresh = Browser::new(&a);
    fresh.xff = Some("198.51.100.200".into());
    let r = fresh.sign_in_by_email(&under, "fresh@example.test", "/app/n").await;
    assert_eq!((r.status, r.location.as_deref()), (303, Some("/app/n")), "an unrelated visitor signs in");
    let done = early.get(&back).await;
    assert_eq!(done.location.as_deref(), Some("/app/e"), "a sign-in under way survived");
}

#[tokio::test]
async fn a_hundred_slash_64_send_flood_leaves_another_network_able_to_send_and_sso_to_route() {
    let under = Under::workos().await;
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], ..Tweaks::default() }).await;
    for n in 0..100u32 {
        for j in 0..10u32 {
            let mut b = Browser::new(&a);
            b.xff = Some(format!("2001:db8:7:{n:x}::{:x}", j + 1));
            let csrf = b.csrf().await;
            b.post("/login/email", &[("csrf", &csrf), ("email", &format!("junk{n}-{j}@junk{n}.example"))]).await;
        }
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let sent = under.fake().state.lock().unwrap().sends.len();
    assert!(sent <= Limits::default().send_per_client[1].max, "one /48's sends: {sent}");
    // Another network sends.
    let mut other = Browser::new(&a);
    other.xff = Some("198.51.100.9".into());
    let csrf = other.csrf().await;
    assert_eq!(other.post("/login/email", &[("csrf", &csrf), ("email", "someone@example.test")]).await.status, 200);
    // SSO routes from another network even when that network has used up
    // its own code sends (AUTH-25 N3: SSO before any send limit).
    let mut corp = Browser::new(&a);
    corp.xff = Some("198.51.100.50".into());
    let mut last = 0;
    for j in 0..25 {
        let csrf = corp.csrf().await;
        last = corp.post("/login/email", &[("csrf", &csrf), ("email", &format!("c{j}@junk-c.example"))]).await.status;
    }
    assert_eq!(last, 429, "its sends are used up");
    let csrf = corp.csrf().await;
    let r = corp.post("/login/email", &[("csrf", &csrf), ("email", "ceo@sso.example")]).await;
    assert_eq!(r.status, 303, "a company domain goes to its IdP whatever the send limits say");
}

#[tokio::test]
async fn ten_foreign_clients_cannot_stop_the_owner_s_code_in_the_owner_s_flow() {
    let under = Under::workos().await;
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], ..Tweaks::default() }).await;
    // The owner has signed in from home before.
    let mut owner = Browser::new(&a);
    owner.xff = Some("203.0.113.5".into());
    assert_eq!(owner.sign_in_by_email(&under, "owner@example.test", "/").await.status, 303);
    owner.post("/logout", &[]).await;
    // Ten clients on ten networks: a send each (where allowed) and five
    // wrong guesses each.
    for i in 0..10 {
        let mut x = Browser::new(&a);
        x.xff = Some(format!("198.51.{i}.66"));
        let csrf = x.csrf().await;
        let sent = x.post("/login/email", &[("csrf", &csrf), ("email", "owner@example.test")]).await;
        if sent.status == 200 {
            for g in 0..6 {
                x.post("/login/code", &[("csrf", &sent.csrf()), ("code", &format!("10000{g}"))]).await;
            }
        }
    }
    let wrong = under.fake().auths(WORKOS_MAGIC) - 1;
    assert!(wrong <= 15, "strangers: 3 codes a day × 5 guesses each — {wrong}");
    // The owner, in the owner's own flow, from the owner's network.
    let r = owner.sign_in_by_email(&under, "owner@example.test", "/app/o").await;
    assert_eq!((r.status, r.location.as_deref()), (303, Some("/app/o")), "{}", r.body);
}

#[tokio::test]
async fn strangers_send_an_address_three_codes_a_day() {
    let under = Under::workos().await;
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], ..Tweaks::default() }).await;
    for i in 0..50 {
        let mut b = Browser::new(&a);
        b.xff = Some(format!("10.{i}.0.1"));
        let csrf = b.csrf().await;
        b.post("/login/email", &[("csrf", &csrf), ("email", "inbox@example.test")]).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(under.fake().sends_to("inbox@example.test").len(), Limits::default().send_per_address_foreign_day.max);
}

#[tokio::test]
async fn a_nat_neighbour_s_flow_survives_twenty_of_another_s() {
    let under = Under::memory();
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], ..Tweaks::default() }).await;
    let mut victim = Browser::new(&a);
    victim.xff = Some("100.64.0.1".into());
    let back = victim.social("github", "/app/n").await.location.unwrap();
    for _ in 0..20 {
        let mut b = Browser::new(&a);
        b.xff = Some("100.64.0.1".into());
        b.social("google", "/").await;
    }
    let done = victim.get(&back).await;
    assert_eq!(done.location.as_deref(), Some("/app/n"), "{}", done.body);
}

#[tokio::test]
async fn one_page_s_token_starts_one_flow() {
    let under = Under::memory();
    let a = app(under.clone()).await;
    let b = Browser::new(&a);
    let page = b.get("/login").await;
    let token = page.csrf();
    let original = b.jar.lock().unwrap().iter().find(|(k, _)| k.starts_with("toy_flow")).map(|(k, v)| (k.clone(), v.clone())).unwrap();
    assert_eq!(b.post("/login/social", &[("csrf", &token), ("provider", "google")]).await.status, 303);
    // The same page's cookie and token, replayed.
    b.jar.lock().unwrap().insert(original.0.clone(), original.1.clone());
    let again = b.post("/login/social", &[("csrf", &token), ("provider", "google")]).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert!(again.body.contains("already used"), "no second flow — a fresh page instead (AUTH-26 R6)");
}

#[tokio::test]
async fn none_still_warns_when_a_forwarded_header_arrives() {
    let under = Under::memory();
    let a = app_with(under.clone(), Tweaks { trusted: vec!["none"], ..Tweaks::default() }).await;
    let mut b = Browser::new(&a);
    b.xff = Some("203.0.113.1".into());
    b.get("/login").await;
    let csrf = b.csrf().await;
    b.post("/login/email", &[("csrf", &csrf), ("email", "n@example.test")]).await;
    assert!(log_text().contains("X-Forwarded-For arrives from a peer that is not a trusted proxy"));
}

// ── AUTH-26 ────────────────────────────────────────────────────────────

/// One client: sends and wrong guesses until its sends run out.
async fn spend(a: &App, xff: String, sends: usize, tag: &str) {
    let mut b = Browser::new(a);
    b.xff = Some(xff);
    for s in 0..sends {
        let csrf = b.csrf().await;
        let sent = b.post("/login/email", &[("csrf", &csrf), ("email", &format!("{tag}-{s}@junk.example"))]).await;
        if sent.status != 200 {
            continue;
        }
        for g in 0..5 {
            b.post("/login/code", &[("csrf", &sent.csrf()), ("code", &format!("90000{g}"))]).await;
        }
    }
}

#[tokio::test]
async fn a_slash_16_of_wrong_guesses_cannot_stop_a_bystander_s_right_code() {
    let under = Under::memory();
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], ..Tweaks::default() }).await;
    // IPv4: 20 clients in 20 /24s of 10.7/16, 6 sends × 5 wrong each = 600.
    for i in 0..20 {
        spend(&a, format!("10.7.{i}.1"), 6, &format!("v4c{i}")).await;
    }
    let mut bystander = Browser::new(&a);
    bystander.xff = Some("10.7.200.5".into());
    let r = bystander.sign_in_by_email(&under, "bystander4@example.test", "/app/b").await;
    assert_eq!((r.status, r.location.as_deref()), (303, Some("/app/b")), "{}", r.body);
    // IPv6: 20 /64s in 4 /48s of 2001:db8::/32.
    for i in 0..20u32 {
        spend(&a, format!("2001:db8:{:x}:{:x}::1", i % 4, i), 6, &format!("v6c{i}")).await;
    }
    let mut bystander = Browser::new(&a);
    bystander.xff = Some("2001:db8:ff00:1::7".into());
    let r = bystander.sign_in_by_email(&under, "bystander6@example.test", "/app/b").await;
    assert_eq!((r.status, r.location.as_deref()), (303, Some("/app/b")), "{}", r.body);
}

#[tokio::test]
async fn a_page_left_open_is_drawn_again_not_refused() {
    let under = Under::memory();
    let limits = Limits { form_token_secs: 1, ..Limits::default() };
    let a = app_with(under.clone(), Tweaks { limits, ..Tweaks::default() }).await;
    let b = Browser::new(&a);
    let csrf = b.csrf().await;
    let sent = b.post("/login/email", &[("csrf", &csrf), ("email", "slow@example.test")]).await;
    let code = under.code_for("slow@example.test").await;
    tokio::time::sleep(Duration::from_millis(2200)).await;
    let again = b.post("/login/code", &[("csrf", &sent.csrf()), ("code", &code)]).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert!(again.body.contains("That page expired") && again.body.contains("Check your email"), "the same step, drawn again");
    let r = b.post("/login/code", &[("csrf", &again.csrf()), ("code", &code)]).await;
    assert_eq!(r.status, 303, "the fresh page's form works: {}", r.body);
    // A stale first page, too.
    let c = Browser::new(&a);
    let csrf = c.csrf().await;
    tokio::time::sleep(Duration::from_millis(2200)).await;
    let r = c.post("/login/email", &[("csrf", &csrf), ("email", "later@example.test")]).await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("That page expired"));
    // A forged token is still refused.
    assert_eq!(c.post("/login/email", &[("csrf", "1.forged"), ("email", "x@example.test")]).await.status, 403);
}

#[tokio::test]
async fn a_double_submitted_email_form_shows_the_code_step() {
    let under = Under::memory();
    let a = app(under.clone()).await;
    let b = Browser::new(&a);
    let page = b.get("/login").await;
    let token = page.csrf();
    let original = b.jar.lock().unwrap().iter().find(|(k, _)| k.starts_with("toy_flow")).map(|(k, v)| (k.clone(), v.clone())).unwrap();
    let first = b.post("/login/email", &[("csrf", &token), ("email", "twice@example.test")]).await;
    assert_eq!(first.status, 200);
    // The browser sends the same form again, with the page's own cookie.
    b.jar.lock().unwrap().insert(original.0.clone(), original.1.clone());
    let second = b.post("/login/email", &[("csrf", &token), ("email", "twice@example.test")]).await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert!(second.body.contains("Check your email") && !second.body.contains("already used"), "{}", second.body);
    let code = under.code_for("twice@example.test").await;
    // Either response's form now works.
    let r = b.post("/login/code", &[("csrf", &first.csrf()), ("code", &code)]).await;
    assert_eq!(r.status, 303, "{}", r.body);
}
