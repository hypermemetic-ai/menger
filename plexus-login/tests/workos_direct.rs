//! IDENTITY-45: a `workos=` `--auth-provider`'s direct social buttons
//! (`GitHub=workos:GitHubOAuth`), completed through the WorkOS backend once
//! the app holds the environment's API key — each pending answer WorkOS
//! gives a social sign-in taken on the app's own page, against a fake of
//! WorkOS's API. Without the key the buttons are the PKCE path, unchanged.

mod support;

use support::*;

const BUTTONS: &[&str] = &["GitHub=workos:GitHubOAuth", "Apple=workos:AppleOAuth", "Microsoft=workos:MicrosoftOAuth", "Google=workos:GoogleOAuth"];

/// Presses `provider`'s button and follows the round trip through the
/// fake: the page the callback answers.
async fn round_trip(b: &Browser, provider: &str) -> Resp {
    let out = b.social(provider, "/app/x").await;
    assert_eq!(out.status, 303, "{}", out.body);
    let back = b.get(&out.location.unwrap()).await.location.expect("the fake sends the browser back");
    b.get(&back).await
}

#[tokio::test]
async fn with_a_key_the_buttons_are_our_page_s_and_without_one_the_pkce_path() {
    let (a, fake) = app_direct(Some(KEY), BUTTONS).await;
    let page = Browser::new(&a).get("/login").await;
    assert_eq!(page.status, 200);
    for p in ["github", "apple", "microsoft", "google"] {
        assert!(page.body.contains(&format!(r#"name="provider" value="{p}""#)), "{p}: {}", page.body);
    }
    assert!(!page.body.contains(r#"action="/login/start""#), "no provider button left: {}", page.body);
    assert!(!page.body.contains(r#"action="/login/email""#), "no email field: the buttons only");
    assert!(page.body.find("GitHub").unwrap() < page.body.find("Apple").unwrap(), "the buttons' order");
    let out = Browser::new(&a).social("github", "/").await;
    let to = out.location.unwrap();
    assert!(to.starts_with(&format!("{}/user_management/authorize?", fake.base)) && to.contains("provider=GitHubOAuth") && to.contains("code_challenge_method=S256"), "{to}");
    assert!(!to.contains(KEY), "the key never rides a URL");

    // No key: the same flags draw today's page — the PKCE path.
    let (a, fake) = app_direct(None, BUTTONS).await;
    let b = Browser::new(&a);
    let page = b.get("/login").await;
    assert!(page.body.contains(r#"action="/login/start""#) && page.body.contains(r#"value="GitHub""#), "{}", page.body);
    assert!(!page.body.contains(r#"action="/login/social""#), "{}", page.body);
    let out = b.get("/login/start?button=GitHub&next=%2F").await;
    let to = out.location.expect("out to WorkOS");
    assert!(to.starts_with(&format!("{}/user_management/authorize?", fake.base)) && to.contains("provider=GitHubOAuth"), "{to}");
}

#[tokio::test]
async fn email_verification_shows_the_code_page_the_right_code_signs_in_and_a_wrong_one_says_so() {
    let (a, fake) = app_direct(Some(KEY), BUTTONS).await;
    let b = Browser::new(&a);
    // GitHub's account at the fake is unverified: email_verification_required.
    let verify = round_trip(&b, "github").await;
    assert_eq!(verify.status, 200, "{}", verify.body);
    assert!(verify.body.contains("Verify your email") && verify.body.contains("We sent a code to verify@example.test"), "{}", verify.body);
    assert!(verify.body.contains(r#"action="/login/continue/resend""#), "Resend: {}", verify.body);
    assert!(b.me().await.is_none(), "not signed in before the code");
    // The pending token stays server-side: in no page and no cookie.
    let pending: Vec<String> = fake.issued().into_iter().filter(|s| s.starts_with("pat_")).collect();
    assert_eq!(pending.len(), 1);
    assert!(!verify.body.contains(&pending[0]));
    assert!(!b.jar.lock().unwrap().values().any(|v| v.contains(&pending[0])));

    // A wrong (or expired) code says so, and stays on the step.
    let wrong = b.post("/login/continue", &[("csrf", &verify.csrf()), ("code", "111111")]).await;
    assert_eq!(wrong.status, 401);
    assert!(wrong.body.contains("it is wrong, or it has expired") && wrong.body.contains("Verify your email"), "{}", wrong.body);

    // Another code: WorkOS sends it to the verification's user.
    let again = b.post("/login/continue/resend", &[("csrf", &wrong.csrf())]).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert!(again.body.contains("We sent another code"), "{}", again.body);
    assert_eq!(fake.state.lock().unwrap().verification_sends.len(), 1);

    // The right one signs in, where the sign-in was headed.
    let r = b.post("/login/continue", &[("csrf", &again.csrf()), ("code", "424242")]).await;
    assert_eq!((r.status, r.location.as_deref()), (303, Some("/app/x")), "{}", r.body);
    let me = b.me().await.expect("signed in");
    assert!(me.to_string().contains("workos"), "{me}");
    // Both grants carried the secret; the code grant PKCE's verifier.
    let code = fake.last_auth("authorization_code").unwrap();
    assert_eq!(code["client_secret"], KEY);
    assert!(code["code_verifier"].as_str().is_some_and(|v| v.len() >= 43));
    let ev = fake.last_auth("urn:workos:oauth:grant-type:email-verification:code").unwrap();
    assert_eq!((ev["client_secret"].as_str(), ev["code"].as_str()), (Some(KEY), Some("424242")));
    // The step is taken once: the flow ended with the sign-in.
    assert_ne!(b.post("/login/continue", &[("csrf", &again.csrf()), ("code", "424242")]).await.status, 303);
}

#[tokio::test]
async fn radar_s_extra_code_is_taken_on_our_page() {
    let (a, fake) = app_direct(Some(KEY), BUTTONS).await;
    fake.state.lock().unwrap().social_as.insert("MicrosoftOAuth".into(), "radar@example.test".into());
    let b = Browser::new(&a);
    let check = round_trip(&b, "microsoft").await;
    assert!(check.body.contains("To keep your account safe"), "{}", check.body);
    let wrong = b.post("/login/continue", &[("csrf", &check.csrf()), ("code", "000000")]).await;
    assert_eq!(wrong.status, 401);
    assert!(wrong.body.contains("That code didn"), "{}", wrong.body);
    let r = b.post("/login/continue", &[("csrf", &check.csrf()), ("code", "135790")]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert!(b.me().await.is_some());
    assert_eq!(fake.last_auth("urn:workos:oauth:grant-type:radar-email-challenge:code").unwrap()["client_secret"], KEY);

    // Radar's text-message check is not offered: refused cleanly.
    fake.state.lock().unwrap().social_as.insert("GoogleOAuth".into(), "sms@example.test".into());
    let s = Browser::new(&a);
    let r = round_trip(&s, "google").await;
    assert_eq!(r.status, 401);
    assert!(r.body.contains("isn&#39;t offered here") || r.body.contains("isn't offered here"), "{}", r.body);
    assert!(s.me().await.is_none());
}

#[tokio::test]
async fn an_organization_is_picked_on_our_page() {
    let (a, _fake) = app_direct(Some(KEY), BUTTONS).await;
    let b = Browser::new(&a);
    let pick = round_trip(&b, "apple").await;
    assert!(pick.body.contains("Choose an organization") && pick.body.contains("Foo Corp"), "{}", pick.body);
    let r = b.post("/login/choose", &[("csrf", &pick.csrf()), ("index", "0")]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert!(b.me().await.is_some());
}

#[tokio::test]
async fn mfa_is_enrolled_then_challenged_on_our_page() {
    let (a, fake) = app_direct(Some(KEY), BUTTONS).await;
    fake.state.lock().unwrap().social_as.insert("GoogleOAuth".into(), "enrol@example.test".into());
    // mfa_enrollment: the key shown, its first code signs in.
    let b = Browser::new(&a);
    let enrol = round_trip(&b, "google").await;
    assert!(enrol.body.contains("Set up two-step verification"), "{}", enrol.body);
    let secret = between(&enrol.body, r#"id="totp-secret">"#, "<").expect("the key");
    assert_eq!(b.post("/login/continue", &[("csrf", &enrol.csrf()), ("code", &wrong_totp(&secret))]).await.status, 401);
    let r = b.post("/login/continue", &[("csrf", &enrol.csrf()), ("code", &totp_now(&secret))]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    // mfa_challenge, next time.
    let c = Browser::new(&a);
    let challenge = round_trip(&c, "google").await;
    assert!(challenge.body.contains("Two-step verification"), "{}", challenge.body);
    assert!(c.me().await.is_none());
    let r = c.post("/login/continue", &[("csrf", &challenge.csrf()), ("code", &totp_now(&secret))]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert!(c.me().await.is_some());
}

#[tokio::test]
async fn the_key_the_pending_token_and_the_codes_are_never_logged() {
    let (a, fake) = app_direct(Some(KEY), BUTTONS).await;
    let b = Browser::new(&a);
    let verify = round_trip(&b, "github").await;
    b.post("/login/continue", &[("csrf", &verify.csrf()), ("code", "111111")]).await;
    let again = b.post("/login/continue/resend", &[("csrf", &verify.csrf())]).await;
    b.post("/login/continue", &[("csrf", &again.csrf()), ("code", "424242")]).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let log = log_text();
    assert!(log.contains("another email-verification code was sent"), "the log is captured");
    assert!(!log.contains(KEY), "the API key");
    for secret in fake.issued() {
        assert!(!log.contains(&secret), "an issued secret ({} chars) is in the log", secret.len());
    }
    for needle in ["verify@example.test", "424242", "client_secret", "pending_authentication_token\":"] {
        assert!(!log.contains(needle), "{needle} is in the log");
    }
}
