//! AUTH-22: our own sign-in page, end to end, over every identity backend
//! that claims each capability — the same tests against WorkOS (a fake of
//! its documented API), the in-process memory backend and plexus-idp (a
//! fixture issuer). Then what is WorkOS's alone (SSO by domain, its
//! refusals), the safety list (CSRF, state, return_to, throttles behind
//! trusted proxies, enumeration), the swap, and the log.

mod support;

use std::sync::Arc;

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClientBuilder};
use plexus_login::identity::Social;
use plexus_login::throttle::{Backoff, Limits, Rate};
use serde_json::{json, Value};
use support::*;

// ── every backend: the capabilities, drawn ──────────────────────────────

#[tokio::test]
async fn the_page_offers_exactly_what_the_backend_can_do() {
    for under in [Under::workos_with(fake_workos::KEY, vec![Social::Google]).await, Under::memory(), Under::plexus_idp().await] {
        let a = app(under.clone()).await;
        let b = Browser::new(&a);
        let page = b.get("/login").await;
        assert_eq!(page.status, 200);
        let caps = under.backend().capabilities();
        assert_eq!(page.body.contains(r#"action="/login/email""#), caps.email_code, "{}", under.name());
        assert_eq!(page.body.contains(r#"action="/login/password""#), caps.password, "{}", under.name());
        for s in Social::ALL {
            assert_eq!(page.body.contains(&format!("/login/social?provider={}", s.name())), caps.social.contains(&s), "{} {}", under.name(), s.name());
        }
        assert!(!page.body.to_lowercase().contains("authkit"), "never AuthKit");
        // Form hygiene: not stored, not framed, a strict CSP, the flow
        // cookie HttpOnly and SameSite=Lax.
        assert_eq!(page.headers["cache-control"], "no-store");
        let csp = page.headers["content-security-policy"].to_str().unwrap();
        assert!(csp.contains("default-src 'none'") && csp.contains("script-src 'sha256-") && csp.contains("frame-ancestors 'none'") && !csp.contains("unsafe-inline"), "{csp}");
        let cookie = page.headers.get_all("set-cookie").iter().map(|v| v.to_str().unwrap().to_string()).find(|c| c.starts_with("toy_flow_")).expect("the flow cookie");
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"), "{cookie}");
    }
}

// ── every backend with email codes ──────────────────────────────────────

#[tokio::test]
async fn an_emailed_code_signs_in_and_a_wrong_one_does_not() {
    for under in code_backends().await {
        let a = app(under.clone()).await;
        let b = Browser::new(&a);
        let email = format!("ada-{}@example.test", under.name());
        let csrf = b.csrf().await;
        let sent = b.post("/login/email", &[("csrf", &csrf), ("email", &email), ("next", "/app/x")]).await;
        assert_eq!(sent.status, 200, "{}", sent.body);
        assert!(sent.body.contains("Check your email"));
        let code = under.code_for(&email).await;
        let wrong = if code == "000000" { "111111" } else { "000000" };
        let r = b.post("/login/code", &[("csrf", &sent.csrf()), ("code", wrong)]).await;
        assert_eq!(r.status, 401, "{}", under.name());
        assert!(r.body.contains("That code didn&#39;t work") || r.body.contains("That code didn't work"));
        let r = b.post("/login/code", &[("csrf", &sent.csrf()), ("code", &code)]).await;
        assert_eq!((r.status, r.location.as_deref()), (303, Some("/app/x")), "{}: {}", under.name(), r.body);
        let me = b.me().await.expect("signed in");
        assert!(me["principal"].as_str().unwrap().starts_with(&format!("oidc:{}/", under.name())), "{me}");
        // A code is used once.
        let again = Browser::new(&a);
        let csrf = again.csrf().await;
        let r = again.post("/login/code", &[("csrf", &csrf), ("code", &code)]).await;
        assert_eq!(r.status, 400, "no code sent in this browser");
    }
}

#[tokio::test]
async fn sending_a_code_answers_the_same_for_any_address() {
    for under in code_backends().await {
        let a = app(under.clone()).await;
        // One address that has signed in before, one never seen.
        let known = Browser::new(&a);
        assert_eq!(known.sign_in_by_email(&under, "known@example.test", "/").await.status, 303);
        let pages = futures::future::join_all(["known@example.test", "nobody-here@example.test"].map(|e| {
            let a = &a;
            async move {
                let b = Browser::new(a);
                let csrf = b.csrf().await;
                let r = b.post("/login/email", &[("csrf", &csrf), ("email", e), ("next", "/")]).await;
                (r.status, r.body.replace(e, "ADDRESS").replace(&r.csrf(), "CSRF"), b)
            }
        }))
        .await;
        assert_eq!(pages[0].0, pages[1].0);
        assert_eq!(pages[0].1, pages[1].1, "the same page, word for word");
        // A wrong code: the same words whether or not the address exists
        // (WorkOS answers an unknown one 404 entity_not_found).
        let mut words = Vec::new();
        for (_, body, b) in &pages {
            let csrf = between(body, r#"name="csrf" value=""#, "\"").unwrap();
            let _ = csrf;
            let r = b.post("/login/code", &[("csrf", &b.csrf().await), ("code", "000001")]).await;
            words.push((r.status, between(&r.body, r#"role="alert">"#, "<").unwrap_or_default()));
        }
        assert_eq!(words[0], words[1], "{}", under.name());
    }
}

// ── every backend with TOTP ─────────────────────────────────────────────

#[tokio::test]
async fn totp_is_enrolled_challenged_at_sign_in_and_removed() {
    for under in code_backends().await {
        let a = app(under.clone()).await;
        let b = Browser::new(&a);
        let email = format!("mfa-{}@example.test", under.name());
        assert_eq!(b.sign_in_by_email(&under, &email, "/").await.status, 303);

        // The account page: off; enrol; the key shown once.
        let acct = b.get("/login/account").await;
        assert!(acct.body.contains("two-step verification is off"), "{}", acct.body);
        let shown = b.post("/login/account/totp", &[("csrf", &acct.csrf())]).await;
        assert_eq!(shown.status, 200, "{}", shown.body);
        assert_eq!(shown.headers["cache-control"], "no-store");
        let secret = between(&shown.body, r#"id="totp-secret">"#, "<").expect("the key");
        let r = b.post("/login/account/totp/confirm", &[("csrf", &shown.csrf()), ("code", &wrong_totp(&secret))]).await;
        assert_eq!(r.status, 401);
        assert!(!r.body.contains(&secret), "the key is shown once");
        let r = b.post("/login/account/totp/confirm", &[("csrf", &shown.csrf()), ("code", &totp_now(&secret))]).await;
        assert!(r.body.contains("two-step verification is on"), "{}: {}", under.name(), r.body);

        // Sign out, and in again: challenged on our page.
        assert_eq!(b.post("/logout", &[]).await.status, 303);
        assert!(b.me().await.is_none());
        let csrf = b.csrf().await;
        let sent = b.post("/login/email", &[("csrf", &csrf), ("email", &email), ("next", "/app/y")]).await;
        let code = under.code_for(&email).await;
        let challenge = b.post("/login/code", &[("csrf", &sent.csrf()), ("code", &code)]).await;
        assert_eq!(challenge.status, 200, "{}", challenge.body);
        assert!(challenge.body.contains("Two-step verification"), "{}", challenge.body);
        assert!(b.me().await.is_none(), "not signed in before the second factor");
        let r = b.post("/login/continue", &[("csrf", &challenge.csrf()), ("code", &wrong_totp(&secret))]).await;
        assert_eq!(r.status, 401);
        let r = b.post("/login/continue", &[("csrf", &challenge.csrf()), ("code", &totp_now(&secret))]).await;
        assert_eq!((r.status, r.location.as_deref()), (303, Some("/app/y")), "{}: {}", under.name(), r.body);

        // Remove it with a code from it.
        let acct = b.get("/login/account").await;
        let factor = between(&acct.body, r#"name="factor" value=""#, "\"").unwrap();
        let r = b.post("/login/account/totp/remove", &[("csrf", &acct.csrf()), ("factor", &factor), ("code", &wrong_totp(&secret))]).await;
        assert_eq!(r.status, 401);
        let r = b.post("/login/account/totp/remove", &[("csrf", &acct.csrf()), ("factor", "auth_factor_not_mine"), ("code", &totp_now(&secret))]).await;
        assert_eq!(r.status, 400, "only a factor of the user's");
        let r = b.post("/login/account/totp/remove", &[("csrf", &acct.csrf()), ("factor", &factor), ("code", &totp_now(&secret))]).await;
        assert!(r.body.contains("two-step verification is off"), "{}", r.body);
    }
}

#[tokio::test]
async fn an_unconfirmed_factor_is_deleted() {
    for under in code_backends().await {
        let a = app(under.clone()).await;
        let b = Browser::new(&a);
        let email = format!("cancel-{}@example.test", under.name());
        b.sign_in_by_email(&under, &email, "/").await;
        let acct = b.get("/login/account").await;
        let shown = b.post("/login/account/totp", &[("csrf", &acct.csrf())]).await;
        assert!(shown.body.contains("totp-secret"));
        let r = b.post("/login/account/totp/cancel", &[("csrf", &shown.csrf())]).await;
        assert!(r.body.contains("two-step verification is off"), "{}", r.body);
        // Signing in again asks for no code: the factor is gone.
        b.post("/logout", &[]).await;
        assert_eq!(b.sign_in_by_email(&under, &email, "/").await.status, 303, "{}", under.name());
    }
}

#[tokio::test]
async fn a_backend_that_requires_mfa_enrols_at_sign_in() {
    for (under, email) in [(Under::workos().await, "enrol@example.test"), (Under::memory_mfa(), "enrol-mem@example.test")] {
        let a = app(under.clone()).await;
        let b = Browser::new(&a);
        let csrf = b.csrf().await;
        let sent = b.post("/login/email", &[("csrf", &csrf), ("email", email), ("next", "/")]).await;
        let code = under.code_for(email).await;
        let enrol = b.post("/login/code", &[("csrf", &sent.csrf()), ("code", &code)]).await;
        assert!(enrol.body.contains("Set up two-step verification"), "{}: {}", under.name(), enrol.body);
        let secret = between(&enrol.body, r#"id="totp-secret">"#, "<").unwrap();
        let r = b.post("/login/continue", &[("csrf", &enrol.csrf()), ("code", &totp_now(&secret))]).await;
        assert_eq!(r.status, 303, "{}: {}", under.name(), r.body);
    }
}

#[tokio::test]
async fn enrolling_needs_a_recent_sign_in() {
    let under = Under::memory();
    let a = app(under.clone()).await;
    let b = Browser::new(&a);
    b.sign_in_by_email(&under, "stale@example.test", "/").await;
    // The session's sign-in time, pushed back past the window.
    let cookie = b.jar.lock().unwrap().iter().find(|(k, _)| k.starts_with("toy_session_")).map(|(_, v)| v.clone()).unwrap();
    let ctx = a.wired.sessions.get(&cookie).unwrap();
    a.wired.sessions.end(&cookie);
    let mut old = ctx.clone();
    old.metadata["auth_time"] = json!(now() - 3600);
    let (id, _) = a.wired.sessions.create(old);
    b.jar.lock().unwrap().insert(a.wired.sessions.cookie_name().to_string(), id);
    let acct = b.get("/login/account").await;
    let r = b.post("/login/account/totp", &[("csrf", &acct.csrf())]).await;
    assert_eq!(r.status, 403);
    assert!(r.body.contains("sign in again"), "{}", r.body);
}

// ── every backend with social sign-in ───────────────────────────────────

#[tokio::test]
async fn a_social_button_goes_out_and_back_bound_to_this_browser() {
    for under in code_backends().await {
        let a = app(under.clone()).await;
        let b = Browser::new(&a);
        b.get("/login").await;
        let out = b.get("/login/social?provider=google&next=%2Fapp%2Fg").await;
        assert_eq!(out.status, 303);
        let to = out.location.clone().unwrap();
        if let Under::WorkOs(f, _) = &under {
            // Straight to the provider through WorkOS: provider=GoogleOAuth,
            // state, PKCE, the exact redirect URI — no AuthKit.
            assert!(to.starts_with(&format!("{}/user_management/authorize?", f.base)), "{to}");
            let q: std::collections::HashMap<String, String> = url::Url::parse(&to).unwrap().query_pairs().into_owned().collect();
            assert_eq!(q["provider"], "GoogleOAuth");
            assert_eq!(q["redirect_uri"], format!("{}/login/callback", a.base));
            assert_eq!(q["code_challenge_method"], "S256");
            assert!(!q.contains_key("client_secret") && !to.contains(fake_workos::KEY));
        }
        // The provider sends the browser back; another browser cannot use
        // the callback, and this one only once.
        let back = if to.contains("/login/callback") { to.clone() } else { b.get(&to).await.location.unwrap() };
        let thief = Browser::new(&a);
        thief.get("/login").await;
        assert_eq!(thief.get(&back).await.status, 400, "another browser's state");
        let tampered = back.replace("state=", "state=x");
        assert_eq!(b.get(&tampered).await.status, 400, "a state not ours");
        let done = b.get(&back).await;
        assert_eq!((done.status, done.location.as_deref()), (303, Some("/app/g")), "{}: {}", under.name(), done.body);
        assert!(b.me().await.is_some());
        let again = Browser::new(&a);
        for (k, v) in b.jar.lock().unwrap().iter().filter(|(k, _)| k.starts_with("toy_flow")) {
            again.jar.lock().unwrap().insert(k.clone(), v.clone());
        }
        assert_eq!(again.get(&back).await.status, 400, "taken once");
        // A provider the backend does not offer.
        assert_eq!(b.get("/login/social?provider=myspace").await.status, 404);
    }
}

// ── plexus-idp: passwords ───────────────────────────────────────────────

#[tokio::test]
async fn plexus_idp_signs_in_with_a_password_and_backs_off() {
    let under = Under::plexus_idp().await;
    let limits = Limits { try_per_address: Backoff { free: 2, base: 60, cap: 60, window: 600 }, ..Limits::default() };
    let a = app_with(under.clone(), Tweaks { limits, ..Tweaks::default() }).await;
    let b = Browser::new(&a);
    let csrf = b.csrf().await;
    let r = b.post("/login/password", &[("csrf", &csrf), ("username", "alice"), ("password", "nope"), ("next", "/")]).await;
    assert_eq!(r.status, 401);
    assert!(r.body.contains("Wrong username or password"));
    // Each flow starts under a fresh id: the page answered carries its token.
    let r = b.post("/login/password", &[("csrf", &r.csrf()), ("username", "alice"), ("password", "s3cret"), ("next", "/app/p")]).await;
    assert_eq!((r.status, r.location.as_deref()), (303, Some("/app/p")), "{}", r.body);
    assert_eq!(b.me().await.unwrap()["principal"], format!("idp:{ALICE}"));
    // Wrong passwords back off, per username.
    let m = Browser::new(&a);
    let mut csrf = m.csrf().await;
    for _ in 0..2 {
        let r = m.post("/login/password", &[("csrf", &csrf), ("username", "alice"), ("password", "x")]).await;
        assert_eq!(r.status, 401);
        csrf = r.csrf();
    }
    let r = m.post("/login/password", &[("csrf", &csrf), ("username", "alice"), ("password", "x")]).await;
    assert!(r.body.contains("Too many attempts"), "{}", r.body);
    let r = m.post("/login/password", &[("csrf", &r.csrf()), ("username", "alice"), ("password", "s3cret")]).await;
    assert!(r.body.contains("Too many attempts"), "locked even for the right password");
    // Email codes are not offered by plexus-idp.
    assert_eq!(m.post("/login/email", &[("csrf", &r.csrf()), ("email", "a@example.test")]).await.status, 404);
}

// ── WorkOS: what is WorkOS's alone ──────────────────────────────────────

#[tokio::test]
async fn an_sso_domain_goes_to_its_idp_and_a_pending_one_does_not() {
    let under = Under::workos().await;
    let a = app(under.clone()).await;
    let b = Browser::new(&a);
    let csrf = b.csrf().await;
    let r = b.post("/login/email", &[("csrf", &csrf), ("email", "ceo@sso.example"), ("next", "/app/s")]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let to = r.location.unwrap();
    let q: std::collections::HashMap<String, String> = url::Url::parse(&to).unwrap().query_pairs().into_owned().collect();
    assert_eq!((q["organization_id"].as_str(), q["login_hint"].as_str()), ("org_SSO", "ceo@sso.example"));
    let back = b.get(&to).await.location.unwrap();
    let done = b.get(&back).await;
    assert_eq!((done.status, done.location.as_deref()), (303, Some("/app/s")), "{}", done.body);
    assert!(under.fake().sends_to("ceo@sso.example").is_empty(), "no code for an SSO domain");
    // An unverified domain routes no one; a verified one with no connection
    // neither: both get a code.
    for e in ["x@pending.example", "y@nossoconn.example"] {
        let b = Browser::new(&a);
        let csrf = b.csrf().await;
        let r = b.post("/login/email", &[("csrf", &csrf), ("email", e)]).await;
        assert_eq!(r.status, 200, "{e}");
        under.code_for(e).await;
    }
}

#[tokio::test]
async fn with_magic_auth_off_the_email_field_is_for_sso_alone() {
    let fake = FakeWorkOs::start().await;
    let b = plexus_login::backends::workos::WorkOs::new(plexus_login::backends::workos::WorkOsConfig {
        client: fake_workos::CLIENT.into(),
        base: Some(fake.base.clone()),
        key: plexus_login::backends::workos::ApiKey::new(fake_workos::KEY).unwrap(),
        social: vec![Social::Google],
        sso: true,
        email_code: false,
        totp_issuer: "toy".into(),
    })
    .unwrap();
    let under = Under::WorkOs(fake.clone(), Arc::new(b));
    let a = app(under.clone()).await;
    let br = Browser::new(&a);
    let page = br.get("/login").await;
    assert!(page.body.contains("Your company&#39;s sign-in, by your work email") || page.body.contains("Your company's sign-in, by your work email"), "{}", page.body);
    let r = br.post("/login/email", &[("csrf", &page.csrf()), ("email", "someone@example.test")]).await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("email sign-in is for company sign-in here"), "{}", r.body);
    let r = br.post("/login/email", &[("csrf", &r.csrf()), ("email", "ceo@sso.example")]).await;
    assert_eq!(r.status, 303, "an SSO domain still goes to its IdP");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(fake.state.lock().unwrap().sends.is_empty(), "no code is asked for");
}

#[tokio::test]
async fn every_workos_step_is_drawn_on_our_page() {
    let under = Under::workos().await;
    let a = app(under.clone()).await;

    // email_verification_required (GitHub's account is unverified).
    let b = Browser::new(&a);
    b.get("/login").await;
    let back = b.get(&b.get("/login/social?provider=github").await.location.unwrap()).await.location.unwrap();
    let verify = b.get(&back).await;
    assert!(verify.body.contains("Verify your email"), "{}", verify.body);
    assert_eq!(b.post("/login/continue", &[("csrf", &verify.csrf()), ("code", "111111")]).await.status, 401);
    let r = b.post("/login/continue", &[("csrf", &verify.csrf()), ("code", "424242")]).await;
    assert_eq!(r.status, 303, "{}", r.body);

    // organization_selection_required (Apple's account is in two).
    let b = Browser::new(&a);
    b.get("/login").await;
    let back = b.get(&b.get("/login/social?provider=apple").await.location.unwrap()).await.location.unwrap();
    let pick = b.get(&back).await;
    assert!(pick.body.contains("Choose an organization") && pick.body.contains("Bar &lt;Corp&gt;"), "{}", pick.body);
    let r = b.post("/login/choose", &[("csrf", &pick.csrf()), ("index", "1")]).await;
    assert_eq!(r.status, 303, "{}", r.body);

    // radar_email_challenge.
    let b = Browser::new(&a);
    let csrf = b.csrf().await;
    let sent = b.post("/login/email", &[("csrf", &csrf), ("email", "radar@example.test")]).await;
    let code = under.code_for("radar@example.test").await;
    let check = b.post("/login/code", &[("csrf", &sent.csrf()), ("code", &code)]).await;
    assert!(check.body.contains("To keep your account safe"), "{}", check.body);
    assert_eq!(b.post("/login/continue", &[("csrf", &check.csrf()), ("code", "135790")]).await.status, 303);

    // radar_sms_challenge: refused cleanly.
    let b = Browser::new(&a);
    let csrf = b.csrf().await;
    let sent = b.post("/login/email", &[("csrf", &csrf), ("email", "sms@example.test")]).await;
    let code = under.code_for("sms@example.test").await;
    let r = b.post("/login/code", &[("csrf", &sent.csrf()), ("code", &code)]).await;
    assert_eq!(r.status, 401);
    assert!(r.body.contains("isn&#39;t offered here") || r.body.contains("isn't offered here"), "{}", r.body);
    assert!(b.me().await.is_none());

    // Every authenticate call carried the IP, the user agent and the secret.
    let auth = under.fake().last_auth("urn:workos:oauth:grant-type:magic-auth:code").unwrap();
    assert_eq!((auth["ip_address"].as_str(), auth["user_agent"].as_str()), (Some("127.0.0.1"), Some("TestBrowser/1.0")));
    assert_eq!(auth["client_secret"], fake_workos::KEY);
    let code_grant = under.fake().last_auth("authorization_code").unwrap();
    assert!(code_grant["code_verifier"].as_str().is_some_and(|v| v.len() >= 43), "PKCE beside the secret");
}

#[tokio::test]
async fn sign_out_revokes_and_a_revoked_session_ends_at_the_next_refresh() {
    let under = Under::workos().await;
    let a = app(under.clone()).await;
    let tokens = a.login.tokens().unwrap().clone();

    // Refresh: due tokens are refreshed (rotated) server side.
    let b = Browser::new(&a);
    b.sign_in_by_email(&under, "refresh@example.test", "/").await;
    assert_eq!(tokens.len(), 1);
    let r = tokens.refresh_due(100_000).await;
    assert_eq!(r.refreshed, 1, "{r:?}");
    assert!(b.me().await.is_some());

    // Revoked at WorkOS (an admin, another device): the next refresh ends
    // the session here, and its socket with it.
    under.fake().revoke_all_sessions();
    let r = tokens.refresh_due(100_000).await;
    assert_eq!(r.ended, 1, "{r:?}");
    assert!(b.me().await.is_none(), "signed out here too");

    // Sign-out revokes the WorkOS session and lands on the page (or a
    // return the rules allow).
    let c = Browser::new(&a);
    c.sign_in_by_email(&under, "out@example.test", "/").await;
    let before = under.fake().state.lock().unwrap().revoked.len();
    let r = c.post("/logout", &[("next", "https://evil.example/")]).await;
    assert_eq!((r.status, r.location.as_deref()), (303, Some("/login")));
    assert_eq!(under.fake().state.lock().unwrap().revoked.len(), before + 1);
    assert!(tokens.is_empty());
    let d = Browser::new(&a);
    d.sign_in_by_email(&under, "out2@example.test", "/").await;
    let r = d.post("/logout", &[("next", "https://app.example/bye")]).await;
    assert_eq!(r.location.as_deref(), Some("https://app.example/bye"), "a listed return origin");
}

#[tokio::test]
async fn a_wrong_api_key_is_the_operator_s_problem_never_the_person_s() {
    let under = Under::workos_with("sk_test_thewrongkey000000000", vec![]).await;
    let a = app(under.clone()).await;
    let b = Browser::new(&a);
    let csrf = b.csrf().await;
    // The SSO lookup is refused 401: unavailable, the same for any address.
    let r = b.post("/login/email", &[("csrf", &csrf), ("email", "a@example.test")]).await;
    assert_eq!(r.status, 503, "{}", r.body);
    assert!(r.body.contains("Sign-in is unavailable right now"));
    assert!(!r.body.contains("sk_test"));
    assert!(log_text().contains("the identity backend refuses this app"));
}

// ── the safety list ─────────────────────────────────────────────────────

#[tokio::test]
async fn a_form_without_this_browser_s_csrf_token_or_from_another_origin_is_refused() {
    let under = Under::memory();
    let a = app(under.clone()).await;
    let b = Browser::new(&a);
    let csrf = b.csrf().await;
    assert_eq!(b.post("/login/email", &[("email", "a@example.test")]).await.status, 403, "no token");
    assert_eq!(b.post("/login/email", &[("csrf", "forged"), ("email", "a@example.test")]).await.status, 403, "a forged token");
    let other = Browser::new(&a);
    let theirs = other.csrf().await;
    assert_eq!(b.post("/login/email", &[("csrf", &theirs), ("email", "a@example.test")]).await.status, 403, "another browser's token");
    let mut evil = Browser::new(&a);
    evil.origin = Some("https://evil.example".into());
    for (k, v) in b.jar.lock().unwrap().iter() {
        evil.jar.lock().unwrap().insert(k.clone(), v.clone());
    }
    assert_eq!(evil.post("/login/email", &[("csrf", &csrf), ("email", "a@example.test")]).await.status, 403, "another origin");
    assert_eq!(b.post("/login/email", &[("csrf", &csrf), ("email", "a@example.test")]).await.status, 200);
    for path in ["/login/code", "/login/continue", "/login/choose", "/login/resend", "/login/account/totp", "/login/account/totp/confirm", "/login/account/totp/remove"] {
        assert_eq!(b.post(path, &[("code", "123456")]).await.status, 403, "{path}");
    }
}

#[tokio::test]
async fn a_flow_id_planted_in_a_browser_never_carries_its_sign_in() {
    let under = Under::memory();
    let a = app(under.clone()).await;
    // An attacker learns a flow id and its token, and plants the id in the
    // victim's browser (a cookie set from another local port, say).
    let attacker = Browser::new(&a);
    let csrf = attacker.csrf().await;
    let planted = attacker.jar.lock().unwrap().iter().find(|(k, _)| k.starts_with("toy_flow_")).map(|(k, v)| (k.clone(), v.clone())).unwrap();
    let victim = Browser::new(&a);
    victim.jar.lock().unwrap().insert(planted.0.clone(), planted.1.clone());
    let sent = victim.post("/login/email", &[("csrf", &csrf), ("email", "victim@example.test")]).await;
    assert_eq!(sent.status, 200);
    let fresh = victim.jar.lock().unwrap().get(&planted.0).cloned().unwrap();
    assert_ne!(fresh, planted.1, "the flow started under a fresh id");
    // The planted id holds nothing: the attacker cannot reach the step.
    let code = under.code_for("victim@example.test").await;
    let r = attacker.post("/login/code", &[("csrf", &csrf), ("code", &code)]).await;
    assert_eq!(r.status, 400, "{}", r.body);
    assert!(attacker.me().await.is_none());
}

#[tokio::test]
async fn return_to_stays_on_this_origin_or_a_listed_one() {
    let under = Under::memory();
    let a = app(under.clone()).await;
    for (next, lands) in [
        ("/app/ok?x=1#y", "/app/ok?x=1#y"),
        ("https://app.example/in", "https://app.example/in"),
        ("https://evil.example/", "/"),
        ("//evil.example/", "/"),
        ("/\\evil.example/", "/"),
        ("\\\\evil.example", "/"),
        ("%2F%2Fevil.example", "/"),
        ("/%2F%2Fevil.example", "/%2F%2Fevil.example"),
        ("https://app.example@evil.example/", "/"),
        ("javascript:alert(1)", "/"),
        ("/\t/evil.example", "/"),
        ("/login?next=//evil.example", "/"),
    ] {
        let b = Browser::new(&a);
        let email = format!("ret-{}@example.test", &random()[..8]);
        let r = b.sign_in_by_email(&under, &email, next).await;
        assert_eq!((r.status, r.location.as_deref()), (303, Some(lands)), "{next:?}");
    }
    // The social round trip checks it too.
    let b = Browser::new(&a);
    b.get("/login").await;
    let out = b.get("/login/social?provider=github&next=https%3A%2F%2Fevil.example%2F").await;
    let done = b.get(&out.location.unwrap()).await;
    assert_eq!(done.location.as_deref(), Some("/"));
}

#[tokio::test]
async fn codes_sent_and_tried_are_throttled_per_address_and_per_ip_through_trusted_proxies_only() {
    let limits = Limits {
        send_per_address: Rate { max: 2, window: 600 },
        send_per_ip: Rate { max: 3, window: 600 },
        try_per_address: Backoff { free: 3, base: 60, cap: 600, window: 3600 },
        try_per_ip: Backoff { free: 100, base: 60, cap: 600, window: 3600 },
        totp_per_user: Backoff { free: 3, base: 60, cap: 600, window: 3600 },
    };
    let under = Under::workos().await;
    // Behind a trusted proxy (the test's own 127.0.0.1): X-Forwarded-For
    // names the client.
    let a = app_with(under.clone(), Tweaks { trusted: vec!["127.0.0.1/32"], limits, ..Tweaks::default() }).await;
    let send = |xff: &'static str, email: &'static str| {
        let a = &a;
        async move {
            let mut b = Browser::new(a);
            b.xff = Some(xff.into());
            let csrf = b.csrf().await;
            b.post("/login/email", &[("csrf", &csrf), ("email", email)]).await.status
        }
    };
    // Per address: two codes, then a wait — whichever IP asks.
    assert_eq!(send("198.51.100.1", "t1@example.test").await, 200);
    assert_eq!(send("198.51.100.2", "t1@example.test").await, 200);
    assert_eq!(send("198.51.100.3", "t1@example.test").await, 429);
    // Per IP: three sends from one client, then a wait.
    assert_eq!(send("203.0.113.9", "t2@example.test").await, 200);
    assert_eq!(send("203.0.113.9", "t3@example.test").await, 200);
    assert_eq!(send("203.0.113.9", "t4@example.test").await, 200);
    assert_eq!(send("203.0.113.9", "t5@example.test").await, 429);
    assert_eq!(send("203.0.113.10", "t5@example.test").await, 200, "another client is not held up");
    // The client's own claim left of the proxy's is not believed.
    assert_eq!(send("6.6.6.6, 203.0.113.9", "t6@example.test").await, 429, "the rightmost untrusted hop is the client");
    // WorkOS was told the forwarded client.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(under.fake().sends_to("t5@example.test")[0]["ip_address"], "203.0.113.10");

    // Not behind a trusted proxy: X-Forwarded-For is ignored, the peer is
    // the client.
    let plain = app_with(Under::workos().await, Tweaks { limits, ..Tweaks::default() }).await;
    let mut statuses = Vec::new();
    for (i, xff) in ["1.1.1.1", "2.2.2.2", "3.3.3.3", "4.4.4.4"].iter().enumerate() {
        let mut b = Browser::new(&plain);
        b.xff = Some(xff.to_string());
        let csrf = b.csrf().await;
        statuses.push(b.post("/login/email", &[("csrf", &csrf), ("email", &format!("u{i}@example.test"))]).await.status);
    }
    assert_eq!(statuses, [200, 200, 200, 429], "spoofed addresses are one peer");

    // Codes tried: three wrong, then the address is locked — even for the
    // right code, from anywhere.
    let b = Browser::new(&a);
    let csrf = b.csrf().await;
    let sent = b.post("/login/email", &[("csrf", &csrf), ("email", "locked@example.test")]).await;
    let code = under.code_for("locked@example.test").await;
    for _ in 0..3 {
        assert_eq!(b.post("/login/code", &[("csrf", &sent.csrf()), ("code", "000001")]).await.status, 401);
    }
    let r = b.post("/login/code", &[("csrf", &sent.csrf()), ("code", "000002")]).await;
    assert_eq!(r.status, 429);
    let r = b.post("/login/code", &[("csrf", &sent.csrf()), ("code", &code)]).await;
    assert_eq!(r.status, 429, "locked: WorkOS is not even asked");
    assert!(b.me().await.is_none());
}

#[tokio::test]
async fn totp_tries_are_throttled_per_user() {
    let limits = Limits { totp_per_user: Backoff { free: 2, base: 60, cap: 600, window: 3600 }, ..Limits::default() };
    let under = Under::memory();
    let a = app_with(under.clone(), Tweaks { limits, ..Tweaks::default() }).await;
    let b = Browser::new(&a);
    b.sign_in_by_email(&under, "tt@example.test", "/").await;
    let acct = b.get("/login/account").await;
    let shown = b.post("/login/account/totp", &[("csrf", &acct.csrf())]).await;
    let secret = between(&shown.body, r#"id="totp-secret">"#, "<").unwrap();
    for _ in 0..2 {
        assert_eq!(b.post("/login/account/totp/confirm", &[("csrf", &shown.csrf()), ("code", &wrong_totp(&secret))]).await.status, 401);
    }
    assert_eq!(b.post("/login/account/totp/confirm", &[("csrf", &shown.csrf()), ("code", &wrong_totp(&secret))]).await.status, 429);
    assert_eq!(b.post("/login/account/totp/confirm", &[("csrf", &shown.csrf()), ("code", &totp_now(&secret))]).await.status, 429, "locked");
}

// ── the swap, and the gate ──────────────────────────────────────────────

#[tokio::test]
async fn the_backend_swaps_without_a_restart_and_sessions_survive_it() {
    let mem = Under::memory();
    let a = app(mem.clone()).await;
    let b = Browser::new(&a);
    assert_eq!(b.sign_in_by_email(&mem, "before@example.test", "/").await.status, 303);
    let principal = b.me().await.unwrap()["principal"].clone();

    let workos = Under::workos_with(fake_workos::KEY, vec![Social::GitHub]).await;
    assert!(a.wired.swap_identity(workos.backend()));
    let page = Browser::new(&a).get("/login").await;
    assert!(page.body.contains("provider=github") && !page.body.contains("provider=google"), "the new backend's capabilities");
    assert_eq!(b.me().await.unwrap()["principal"], principal, "a session made before the swap stands");
    let c = Browser::new(&a);
    assert_eq!(c.sign_in_by_email(&workos, "after@example.test", "/").await.status, 303);
    assert!(c.me().await.unwrap()["principal"].as_str().unwrap().starts_with("oidc:workos/"));
}

#[tokio::test]
async fn a_session_from_our_page_opens_the_socket() {
    let under = Under::memory();
    let a = app(under.clone()).await;
    let b = Browser::new(&a);
    b.sign_in_by_email(&under, "sock@example.test", "/").await;
    let cookie = b.jar.lock().unwrap().iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ");
    let mut h = HeaderMap::new();
    h.insert("cookie", HeaderValue::from_str(&cookie).unwrap());
    let ws = WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{}", a.ws)).await.unwrap();
    let mut p = ObjectParams::new();
    p.insert("method", "auth.whoami").unwrap();
    p.insert("params", json!({})).unwrap();
    let mut sub: Subscription<Value> = ws.subscribe("toy.call", p, "toy.call_unsub").await.unwrap();
    let first = sub.next().await.unwrap().unwrap();
    assert!(first["content"]["principal"].as_str().unwrap_or_default().starts_with("oidc:memory/"), "{first}");
}

// ── the log ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn no_code_token_address_or_key_is_ever_logged() {
    let under = Under::workos().await;
    let a = app(under.clone()).await;
    // Every flow: a wrong code, a right one, TOTP, social, SSO, refresh,
    // sign-out, a refusal.
    let b = Browser::new(&a);
    let csrf = b.csrf().await;
    let sent = b.post("/login/email", &[("csrf", &csrf), ("email", "secretive@example.test")]).await;
    let code = under.code_for("secretive@example.test").await;
    b.post("/login/code", &[("csrf", &sent.csrf()), ("code", "000009")]).await;
    b.post("/login/code", &[("csrf", &sent.csrf()), ("code", &code)]).await;
    let acct = b.get("/login/account").await;
    let shown = b.post("/login/account/totp", &[("csrf", &acct.csrf())]).await;
    let secret = between(&shown.body, r#"id="totp-secret">"#, "<").unwrap();
    b.post("/login/account/totp/confirm", &[("csrf", &shown.csrf()), ("code", &totp_now(&secret))]).await;
    a.login.tokens().unwrap().refresh_due(100_000).await;
    b.post("/logout", &[]).await;
    let s = Browser::new(&a);
    s.get("/login").await;
    let back = s.get(&s.get("/login/social?provider=google").await.location.unwrap()).await.location.unwrap();
    s.get(&back).await;
    let o = Browser::new(&a);
    let csrf = o.csrf().await;
    let sso = o.post("/login/email", &[("csrf", &csrf), ("email", "boss@sso.example")]).await;
    o.get(&o.get(&sso.location.unwrap()).await.location.unwrap()).await;
    let n = Browser::new(&a);
    let csrf = n.csrf().await;
    n.post("/login/email", &[("csrf", &csrf), ("email", "nobody-x@example.test")]).await;
    n.post("/login/code", &[("csrf", &n.csrf().await), ("code", "123456")]).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let log = log_text();
    assert!(log.contains("signed in") && log.contains("the backend took a sign-in code request"), "the log is captured");
    assert!(!log.contains(fake_workos::KEY), "the API key");
    for secret in under.fake().issued() {
        assert!(!log.contains(&secret), "an issued secret ({} chars) is in the log", secret.len());
    }
    assert!(!log.contains(&secret), "the TOTP key");
    assert!(!log.contains(&code), "the emailed code");
    for needle in ["@example.test", "@sso.example", "secretive", "client_secret", "pending_authentication_token", "refresh_token\":"] {
        assert!(!log.contains(needle), "{needle} is in the log");
    }
    // The page never put a secret in a URL: its redirects carry state and
    // a PKCE challenge, never a code or token of ours.
    for q in under.fake().state.lock().unwrap().authorizes.iter() {
        assert!(!q.values().any(|v| v.contains(fake_workos::KEY)));
    }
    let _ = Arc::new(());
}
