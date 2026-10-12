//! The sign-in page's routes: each step of a sign-in, and the account
//! page's TOTP. See the parent module.

use axum::extract::{Form, Query, State};
use serde::Deserialize;

use super::*;
use crate::throttle::Reservation;

#[derive(Deserialize)]
pub struct CsrfForm {
    #[serde(default)]
    csrf: String,
}

#[derive(Deserialize)]
pub struct EmailForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    email: String,
    next: Option<String>,
}

#[derive(Deserialize)]
pub struct PasswordForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    next: Option<String>,
}

#[derive(Deserialize)]
pub struct CodeForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    code: String,
}

#[derive(Deserialize)]
pub struct ChooseForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    index: usize,
}

#[derive(Deserialize)]
pub struct RemoveForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    factor: String,
    #[serde(default)]
    code: String,
}

#[derive(Deserialize)]
pub struct SocialForm {
    #[serde(default)]
    csrf: String,
    provider: Option<String>,
    next: Option<String>,
}

/// `GET /login/callback`'s query.
#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct PageQuery {
    next: Option<String>,
}

/// A refused form: another site's `Origin`, or no (or a wrong) CSRF token.
fn forbidden() -> Response {
    page::respond(
        StatusCode::FORBIDDEN,
        page::Page::plain("<!doctype html><title>Forbidden</title><p>Forbidden: that form did not come from this sign-in page. Go back to the sign-in page and start again.</p>".into()),
    )
}

const WRONG: &str = "That code didn't work. Check it and try again.";
const STALE: &str = "That sign-in took too long. Start again.";

fn with_cookie(mut r: Response, set: Option<HeaderValue>) -> Response {
    if let Some(c) = set {
        r.headers_mut().append(header::SET_COOKIE, c);
    }
    r
}

impl SignInPage {
    /// The routes: every step of a sign-in and the account page — beside an
    /// owner's own `GET /login` (drawing [`SignInPage::entry`]) and
    /// `GET /login/callback` (calling [`SignInPage::callback`] when
    /// [`SignInPage::owns_callback`]).
    pub fn routes(self: &Arc<Self>) -> axum::Router {
        use axum::routing::{get, post};
        axum::Router::new()
            .route("/login/email", post(email))
            .route("/login/resend", post(resend))
            .route("/login/code", post(code))
            .route("/login/password", post(password))
            .route("/login/continue", post(continue_code))
            .route("/login/choose", post(choose))
            .route("/login/social", post(social))
            .route("/login/account", get(account))
            .route("/login/account/totp", post(enroll))
            .route("/login/account/totp/confirm", post(confirm))
            .route("/login/account/totp/cancel", post(cancel))
            .route("/login/account/totp/remove", post(remove))
            .with_state(self.clone())
    }

    /// [`SignInPage::routes`] with its own `GET /login` and
    /// `GET /login/callback`: the whole sign-in, for a service whose only
    /// way in is this page (a central sign-in).
    pub fn standalone_router(self: &Arc<Self>) -> axum::Router {
        use axum::routing::get;
        self.routes().merge(axum::Router::new().route("/login", get(entry_page)).route("/login/callback", get(callback)).with_state(self.clone()))
    }

    /// Whether a callback's `state` is this browser's round trip.
    pub fn owns_callback(&self, headers: &HeaderMap, state: Option<&str>) -> bool {
        let (Some(id), Some(state)) = (self.cookie_flow(headers), state) else { return false };
        self.with_flow(&id, |f| f.oauth.as_ref().is_some_and(|o| constant_eq(o.state.as_bytes(), state.as_bytes()))).unwrap_or(false)
    }

    /// `GET /login/callback` for a round trip this page started.
    pub async fn callback(self: Arc<Self>, headers: HeaderMap, peer: Peer, q: CallbackQuery) -> Response {
        let r = self.req(&headers, &peer);
        let Some(id) = r.flow_id.clone() else {
            let (id, set) = self.flow_id(&headers);
            return with_cookie(self.restart(&id, "/", "That sign-in was not started in this browser. Start again.", StatusCode::BAD_REQUEST), set);
        };
        let next = self.next_of(&id);
        // The state must be this browser's, and is taken once.
        let taken = self.with_flow(&id, |f| match (&f.oauth, q.state.as_deref()) {
            (Some(o), Some(s)) if constant_eq(o.state.as_bytes(), s.as_bytes()) && o.expires > now() => f.oauth.take(),
            _ => None,
        });
        let Some(Some(oauth)) = taken else {
            tracing::warn!("a sign-in callback whose state is not this browser's was refused");
            return self.restart(&id, &next, "That sign-in was not started in this browser, took too long, or was already used. Start again.", StatusCode::BAD_REQUEST);
        };
        if let Some(e) = q.error.as_deref() {
            // A word; the description goes nowhere (a link can carry any
            // text here).
            let clean: String = e.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(64).collect();
            tracing::info!(error = %clean, "the provider sent the browser back without a sign-in");
            let words = match clean.as_str() {
                "access_denied" => "The sign-in was cancelled.",
                "organization_invalid" | "connection_invalid" | "connection_unlinked" | "ambiguous_connection_selector" | "invalid_connection_selector" | "connection_strategy_invalid" => {
                    "That sign-in isn't set up correctly. Ask your administrator, or try another way in."
                }
                _ => "The sign-in service did not sign you in. Start again.",
            };
            return self.restart(&id, &next, words, StatusCode::UNAUTHORIZED);
        }
        let Some(code) = q.code.filter(|c| !c.is_empty() && c.len() <= 512) else {
            return self.restart(&id, &next, "The sign-in service sent back nothing to sign in with.", StatusCode::BAD_REQUEST);
        };
        let r2 = self.flow_backend(&id).complete_federated(&code, &oauth.verifier, &self.redirect_uri, &r.info).await;
        self.answer(&id, r2, |p| p.restart(&id, &next, "That sign-in didn't work. Start again.", StatusCode::UNAUTHORIZED)).await
    }

    /// The flow id of a POST whose origin and CSRF token check out.
    #[allow(clippy::result_large_err)]
    fn posted(&self, headers: &HeaderMap, peer: &Peer, csrf: &str) -> Result<(Req, String), Response> {
        let r = self.req(headers, peer);
        if r.foreign || !self.csrf_ok(r.flow_id.as_deref(), csrf) {
            tracing::warn!(foreign = r.foreign, "a sign-in form was refused: another origin, or no CSRF token of this browser's");
            return Err(forbidden());
        }
        let id = r.flow_id.clone().expect("csrf_ok needs a flow id");
        Ok((r, id))
    }

    /// AUTH-24 #1: what a backend's answer does to the attempt reserved
    /// for it. A wrong code stays counted; a right one (or a first factor
    /// that was right, asking for a next step) is refunded and forgiven; an
    /// answer that is no verdict (the backend unavailable, refusing this
    /// app) is refunded.
    fn settle<T>(&self, res: Reservation, outcome: &Result<T, IdentityError>) {
        match outcome {
            Err(IdentityError::InvalidCode) => drop(res),
            Err(IdentityError::Unavailable(_) | IdentityError::RateLimited { .. } | IdentityError::Misconfigured(_) | IdentityError::Unsupported(_)) => self.throttles.refund(res, false),
            _ => self.throttles.refund(res, true),
        }
    }
}

/// The words for a reservation refused.
fn wait_page_words(w: i64) -> String {
    SignInPage::wait_words(w)
}

/// `GET /login` of [`SignInPage::standalone_router`].
async fn entry_page(State(p): State<Arc<SignInPage>>, headers: HeaderMap, Query(q): Query<PageQuery>) -> Response {
    let next = p.cfg.return_to.check(q.next.as_deref());
    let (id, set) = p.flow_id(&headers);
    let (main, alts) = p.entry(&id, &next);
    with_cookie(page::respond(StatusCode::OK, p.cont.draw(Body { heading: p.sign_in_heading(), error: None, main, alts, extra: String::new() })), set)
}

async fn callback(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Query(q): Query<CallbackQuery>) -> Response {
    p.callback(headers, peer, q).await
}

/// `POST /login/email`: an SSO domain goes to its IdP; any other address
/// is sent a code, and every address gets the same page.
async fn email(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Form(f): Form<EmailForm>) -> Response {
    let (r, id) = match p.posted(&headers, &peer, &f.csrf) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    let next = p.cfg.return_to.check(f.next.as_deref());
    if !p.backend().capabilities().email_code {
        return p.restart(&id, &next, "Signing in by email isn't offered here.", StatusCode::NOT_FOUND);
    }
    let Some(domain) = email_domain(&f.email) else {
        return p.restart(&id, &next, "Enter an email address.", StatusCode::BAD_REQUEST);
    };
    let address = normalize(&f.email);
    if let Verdict::Wait(w) = p.throttles.send(&address, r.info.ip) {
        tracing::info!("a code send throttled");
        return p.restart(&id, &next, &SignInPage::wait_words(w), StatusCode::TOO_MANY_REQUESTS);
    }
    let (id, set) = match p.fresh_flow(Some(&id), next.clone(), &r) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    p.sweep_later();
    match p.sso_for(&domain).await {
        // Revealing that a domain has SSO is the trade-off every SSO
        // sign-in makes; whether an address has an account is not. The
        // address stays out of the URL (AUTH-24 #9): the organization
        // routes, and the IdP asks for the account itself.
        Ok(Some(target)) => p.federated(&id, &target, None, Some(set)),
        Ok(None) if p.backend().sends_codes() => with_cookie(send_and_ask(&p, &id, address, r.info), Some(set)),
        // An email field for SSO alone: any other address is told so, the
        // same words for any address.
        Ok(None) => with_cookie(p.restart(&id, &next, "Sign in with one of the buttons below; email sign-in is for company sign-in here.", StatusCode::OK), Some(set)),
        Err(e) => with_cookie(p.failed(&id, &next, e), Some(set)),
    }
}

/// Sends a code to `address` after answering: the same page for any
/// address, at the same speed.
fn send_and_ask(p: &Arc<SignInPage>, id: &str, address: String, info: RequestInfo) -> Response {
    p.with_flow(id, |f| f.step = Step::CodeSent { email: address.clone() });
    let backend = p.flow_backend(id);
    let to = address.clone();
    tokio::spawn(async move {
        match backend.send_email_code(&to, &info).await {
            Ok(()) => tracing::info!(backend = backend.name(), "the backend took a sign-in code request"),
            Err(IdentityError::Misconfigured(problem)) => {
                tracing::error!(backend = backend.name(), %problem, "no sign-in code was sent: the backend refuses this app's request")
            }
            Err(e) => tracing::warn!(backend = backend.name(), refusal = e.kind(), "no sign-in code was sent"),
        }
    });
    p.email_code_page(id, &address, None, StatusCode::OK)
}

/// `POST /login/resend`: another code to the flow's address.
async fn resend(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Form(f): Form<CsrfForm>) -> Response {
    let (r, id) = match p.posted(&headers, &peer, &f.csrf) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    let found = p.with_flow(&id, |f| match &f.step {
        Step::CodeSent { email } => Some(email.clone()),
        _ => None,
    });
    let Some(Some(address)) = found else { return p.restart(&id, "/", STALE, StatusCode::BAD_REQUEST) };
    if let Verdict::Wait(w) = p.throttles.send(&address, r.info.ip) {
        return p.email_code_page(&id, &address, Some(&SignInPage::wait_words(w)), StatusCode::TOO_MANY_REQUESTS);
    }
    send_and_ask(&p, &id, address, r.info)
}

/// `POST /login/code`: the emailed code, for the flow's address.
async fn code(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Form(f): Form<CodeForm>) -> Response {
    let (r, id) = match p.posted(&headers, &peer, &f.csrf) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    let found = p.with_flow(&id, |fl| match &fl.step {
        Step::CodeSent { email } => Some(email.clone()),
        _ => None,
    });
    let Some(Some(address)) = found else { return p.restart(&id, "/", STALE, StatusCode::BAD_REQUEST) };
    // AUTH-24 #1: the guess is counted before it is made.
    let reserved = match p.throttles.reserve_code(&address, r.info.ip) {
        Ok(x) => x,
        Err(w) => return p.email_code_page(&id, &address, Some(&wait_page_words(w)), StatusCode::TOO_MANY_REQUESTS),
    };
    let Some(c) = clean_code(&f.code) else {
        return p.email_code_page(&id, &address, Some(WRONG), StatusCode::UNAUTHORIZED);
    };
    let res = p.flow_backend(&id).verify_email_code(&address, &c, &r.info).await;
    p.settle(reserved, &res);
    p.answer(&id, res, |p| {
        tracing::info!("a wrong email code");
        p.email_code_page(&id, &address, Some(WRONG), StatusCode::UNAUTHORIZED)
    })
    .await
}

/// `POST /login/password`: a username and password.
async fn password(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Form(f): Form<PasswordForm>) -> Response {
    let (r, id) = match p.posted(&headers, &peer, &f.csrf) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    let next = p.cfg.return_to.check(f.next.as_deref());
    if !p.backend().capabilities().password {
        return p.restart(&id, &next, "Passwords aren't taken here.", StatusCode::NOT_FOUND);
    }
    let user = f.username.trim().to_ascii_lowercase();
    if user.is_empty() || user.len() > 254 || f.password.is_empty() || f.password.len() > 1024 {
        return p.restart(&id, &next, "Enter your username and password.", StatusCode::BAD_REQUEST);
    }
    let key = format!("user:{user}");
    let (id, set) = match p.fresh_flow(Some(&id), next.clone(), &r) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    let reserved = match p.throttles.reserve_code(&key, r.info.ip) {
        Ok(x) => x,
        Err(w) => return with_cookie(p.restart(&id, &next, &wait_page_words(w), StatusCode::TOO_MANY_REQUESTS), Some(set)),
    };
    let res = p.flow_backend(&id).password(f.username.trim(), &f.password, &r.info).await;
    p.settle(reserved, &res);
    let resp = p.answer(&id, res, |p| p.restart(&id, &next, "Wrong username or password.", StatusCode::UNAUTHORIZED)).await;
    with_cookie(resp, Some(set))
}

/// `POST /login/continue`: a code a step waits for — a TOTP code, a factor
/// enrolled at sign-in, an emailed verification or extra-check code.
async fn continue_code(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Form(f): Form<CodeForm>) -> Response {
    let (r, id) = match p.posted(&headers, &peer, &f.csrf) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    let found = p.with_flow(&id, |fl| match &fl.step {
        Step::Code { ticket, kind, subject } => Some((ticket.clone(), *kind, subject.clone())),
        _ => None,
    });
    let Some(Some((ticket, kind, subject))) = found else { return p.restart(&id, "/", STALE, StatusCode::BAD_REQUEST) };
    let totp = matches!(kind, CodeKind::Totp | CodeKind::EnrollAtSignIn);
    let key = subject.clone().unwrap_or_else(|| format!("flow:{}", SignInPage::key(&id)));
    let reserved = if totp { p.throttles.reserve_totp(&key, r.info.ip) } else { p.throttles.reserve_code(&key, r.info.ip) };
    // A retry shows no enrolment again: its key was shown once.
    let reserved = match reserved {
        Ok(x) => x,
        Err(w) => return p.continue_page(&id, kind, None, Some(&wait_page_words(w)), StatusCode::TOO_MANY_REQUESTS),
    };
    let wrong = |p: &SignInPage| p.continue_page(&id, kind, None, Some(WRONG), StatusCode::UNAUTHORIZED);
    let Some(c) = clean_code(&f.code) else { return wrong(&p) };
    let res = p.flow_backend(&id).continue_with_code(&ticket, &c, &r.info).await;
    p.settle(reserved, &res);
    if res.is_ok() {
        if let (CodeKind::EnrollAtSignIn, Some(s)) = (kind, &subject) {
            p.enrolling.lock().unwrap().remove(s);
        }
    }
    p.answer(&id, res, wrong).await
}

/// `POST /login/choose`: the organization picked.
async fn choose(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Form(f): Form<ChooseForm>) -> Response {
    let (r, id) = match p.posted(&headers, &peer, &f.csrf) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    let found = p.with_flow(&id, |fl| match &fl.step {
        Step::Choose { ticket, choices } => choices.get(f.index).map(|c| (ticket.clone(), c.id.clone())),
        _ => None,
    });
    let Some(Some((ticket, choice))) = found else { return p.restart(&id, "/", STALE, StatusCode::BAD_REQUEST) };
    let next = p.next_of(&id);
    let res = p.flow_backend(&id).choose(&ticket, &choice, &r.info).await;
    p.answer(&id, res, |p| p.restart(&id, &next, "That sign-in didn't work. Start again.", StatusCode::UNAUTHORIZED)).await
}

/// `POST /login/social` (`provider=google`): out to the provider. A form
/// of this page's, with its CSRF token (AUTH-24 #4).
async fn social(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Form(f): Form<SocialForm>) -> Response {
    let (r, id) = match p.posted(&headers, &peer, &f.csrf) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    let next = p.cfg.return_to.check(f.next.as_deref());
    let offered = social_of(f.provider.as_deref()).filter(|s| p.backend().capabilities().social.contains(s));
    let Some(s) = offered else {
        return p.restart(&id, &next, "That sign-in is not offered here.", StatusCode::NOT_FOUND);
    };
    let (id, set) = match p.fresh_flow(Some(&id), next, &r) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    p.sweep_later();
    p.federated(&id, &Federated::Social(s), None, Some(set))
}

// ── the account page: TOTP ─────────────────────────────────────────────

impl SignInPage {
    fn account_page(&self, flow_id: &str, who: &AccountHolder, factors: &[crate::identity::Factor], note: Option<&str>, error: Option<&str>, status: StatusCode) -> Response {
        let csrf = self.csrf(flow_id);
        let list: String = factors
            .iter()
            .filter(|f| f.kind == "totp")
            .map(|f| {
                format!(
                    r#"<li><span>Authenticator app</span>
        <form method="post" action="/login/account/totp/remove"><input type="hidden" name="csrf" value="{csrf}"><input type="hidden" name="factor" value="{}">
        <label>Code from this app, to remove it<input name="code" class="code" type="text" inputmode="numeric" maxlength="8" autocomplete="one-time-code" required></label>
        <button class="alt" type="submit">Remove</button></form></li>"#,
                    esc(&f.id)
                )
            })
            .collect();
        let has = !list.is_empty();
        let main = format!(
            r#"{}
    <ul class="factors">{list}</ul>
    <form method="post" action="/login/account/totp"><input type="hidden" name="csrf" value="{csrf}"><button class="{}" type="submit" id="totp-add">Add an authenticator app</button></form>
    <p class="note"><a href="/">Back to the app</a></p>"#,
            note.map(|n| format!(r#"<p class="note" role="status">{}</p>"#, esc(n))).unwrap_or_default(),
            if has { "alt" } else { "primary" }
        );
        let state = if has { "on" } else { "off" };
        let sub = match &who.email {
            Some(e) => format!("{e} · two-step verification is {state}"),
            None => format!("Two-step verification is {state}"),
        };
        self.step_page("Your account", &sub, error, main, status)
    }

    async fn account_view(&self, flow_id: &str, who: &AccountHolder, note: Option<&str>, error: Option<&str>, status: StatusCode) -> Response {
        match self.backend().totp_factors(&who.subject).await {
            Ok(f) => self.account_page(flow_id, who, &f, note, error, status),
            Err(e) => {
                tracing::warn!(kind = e.kind(), "the account's factors could not be listed");
                self.step_page("Your account", "", Some("Your account can't be shown right now. Try again in a little while."), r#"<p class="note"><a href="/">Back to the app</a></p>"#.into(), StatusCode::SERVICE_UNAVAILABLE)
            }
        }
    }

    fn to_sign_in(&self) -> Response {
        (StatusCode::SEE_OTHER, [(header::LOCATION, "/login?next=%2Flogin%2Faccount".to_string()), (header::CACHE_CONTROL, "no-store".into())]).into_response()
    }

    /// The signed-in person whose account this backend keeps, or why not.
    #[allow(clippy::result_large_err)]
    fn holder(&self, headers: &HeaderMap) -> Result<AccountHolder, Response> {
        let Some(who) = self.cont.account(headers) else { return Err(self.to_sign_in()) };
        let backend = self.backend();
        if !backend.capabilities().totp || who.backend != backend.name() {
            return Err(self.step_page(
                "Your account",
                "",
                Some("Two-step verification isn't managed here for your sign-in."),
                r#"<p class="note"><a href="/">Back to the app</a></p>"#.into(),
                StatusCode::NOT_FOUND,
            ));
        }
        Ok(who)
    }

    #[allow(clippy::result_large_err)]
    fn account_post(&self, headers: &HeaderMap, peer: &Peer, csrf: &str) -> Result<(Req, String, AccountHolder), Response> {
        let (r, id) = self.posted(headers, peer, csrf)?;
        Ok((r, id, self.holder(headers)?))
    }

    fn enroll_retry(&self, id: &str, words: &str, status: StatusCode) -> Response {
        let csrf = self.csrf(id);
        let main = format!(
            r#"{}
    <form method="post" action="/login/account/totp/cancel"><input type="hidden" name="csrf" value="{csrf}"><button class="link" type="submit">Cancel</button></form>"#,
            self.code_form(id, "/login/account/totp/confirm", "Code from the app")
        );
        self.step_page("Add an authenticator app", "Enter the code your authenticator app shows now.", Some(words), main, status)
    }
}

async fn account(State(p): State<Arc<SignInPage>>, headers: HeaderMap) -> Response {
    let who = match p.holder(&headers) {
        Ok(w) => w,
        Err(r) => return r,
    };
    let (id, set) = p.flow_id(&headers);
    with_cookie(p.account_view(&id, &who, None, None, StatusCode::OK).await, set)
}

/// `POST /login/account/totp`: a new factor, its key shown once.
async fn enroll(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Form(f): Form<CsrfForm>) -> Response {
    let (_, id, who) = match p.account_post(&headers, &peer, &f.csrf) {
        Ok(x) => x,
        Err(r) => return r,
    };
    if now() - who.signed_in_at > p.cfg.reauth_secs {
        return p.account_view(&id, &who, None, Some("To change two-step verification, sign out and sign in again first."), StatusCode::FORBIDDEN).await;
    }
    let label = who.email.clone().unwrap_or_else(|| who.subject.clone());
    match p.backend().totp_enroll(&who.subject, &p.cfg.totp_issuer, &label).await {
        Ok(e) => {
            p.remember_enrollment(&who.subject, &e, p.backend());
            let csrf = p.csrf(&id);
            let main = format!(
                r#"{}
    {}
    <form method="post" action="/login/account/totp/cancel"><input type="hidden" name="csrf" value="{csrf}"><button class="link" type="submit">Cancel</button></form>"#,
                SignInPage::qr_block(&e),
                p.code_form(&id, "/login/account/totp/confirm", "Code from the app")
            );
            tracing::info!("a TOTP factor was enrolled, waiting for its first code");
            p.step_page("Add an authenticator app", "Add this key to your authenticator app, then enter the code it shows. It is shown only once.", None, main, StatusCode::OK)
        }
        Err(e) => {
            tracing::warn!(kind = e.kind(), "a TOTP factor could not be enrolled");
            p.account_view(&id, &who, None, Some("That didn't work. Try again in a little while."), StatusCode::SERVICE_UNAVAILABLE).await
        }
    }
}

/// `POST /login/account/totp/confirm`: the first code from the new factor.
async fn confirm(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Form(f): Form<CodeForm>) -> Response {
    let (r, id, who) = match p.account_post(&headers, &peer, &f.csrf) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    let pending = p.enrolling.lock().unwrap().get(&who.subject).filter(|e| e.expires > now()).map(|e| (e.confirm.clone(), e.backend.clone()));
    let Some((ticket, backend)) = pending else {
        return p.account_view(&id, &who, None, Some("That took too long. Add the app again."), StatusCode::BAD_REQUEST).await;
    };
    let reserved = match p.throttles.reserve_totp(&who.subject, r.info.ip) {
        Ok(x) => x,
        Err(w) => return p.enroll_retry(&id, &wait_page_words(w), StatusCode::TOO_MANY_REQUESTS),
    };
    let ok = match clean_code(&f.code) {
        Some(c) => backend.totp_confirm(&ticket, &c).await,
        None => Ok(false),
    };
    match &ok {
        Ok(true) => p.throttles.refund(reserved, true),
        Ok(false) => drop(reserved),
        Err(_) => p.throttles.refund(reserved, false),
    }
    match ok {
        Ok(true) => {
            p.enrolling.lock().unwrap().remove(&who.subject);
            tracing::info!("a TOTP factor was confirmed");
            p.account_view(&id, &who, Some("Authenticator app added. You'll be asked for a code when you sign in."), None, StatusCode::OK).await
        }
        Ok(false) => p.enroll_retry(&id, WRONG, StatusCode::UNAUTHORIZED),
        Err(e) => {
            tracing::warn!(kind = e.kind(), "a TOTP enrolment could not be confirmed");
            p.enroll_retry(&id, "That didn't work. Try again in a little while.", StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

/// `POST /login/account/totp/cancel`: the unconfirmed factor deleted.
async fn cancel(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Form(f): Form<CsrfForm>) -> Response {
    let (_, id, who) = match p.account_post(&headers, &peer, &f.csrf) {
        Ok(x) => x,
        Err(r) => return r,
    };
    let pending = p.enrolling.lock().unwrap().remove(&who.subject);
    if let Some(pf) = pending {
        if let Err(e) = pf.backend.totp_discard(&pf.factor).await {
            tracing::warn!(kind = e.kind(), "an unconfirmed TOTP factor could not be deleted");
        }
    }
    p.account_view(&id, &who, None, None, StatusCode::OK).await
}

/// `POST /login/account/totp/remove`: a factor of this user's, removed with
/// a current code from it.
async fn remove(State(p): State<Arc<SignInPage>>, headers: HeaderMap, peer: Peer, Form(f): Form<RemoveForm>) -> Response {
    let (r, id, who) = match p.account_post(&headers, &peer, &f.csrf) {
        Ok(x) => x,
        Err(resp) => return resp,
    };
    let backend = p.backend();
    let factors = match backend.totp_factors(&who.subject).await {
        Ok(fs) => fs,
        Err(e) => {
            tracing::warn!(kind = e.kind(), "the account's factors could not be listed");
            return p.account_view(&id, &who, None, Some("That didn't work. Try again in a little while."), StatusCode::SERVICE_UNAVAILABLE).await;
        }
    };
    // Only a factor of this user's.
    if !factors.iter().any(|x| x.id == f.factor) {
        return p.account_page(&id, &who, &factors, None, Some("That authenticator app is not on your account."), StatusCode::BAD_REQUEST);
    }
    let reserved = match p.throttles.reserve_totp(&who.subject, r.info.ip) {
        Ok(x) => x,
        Err(w) => return p.account_page(&id, &who, &factors, None, Some(&wait_page_words(w)), StatusCode::TOO_MANY_REQUESTS),
    };
    let ok = match clean_code(&f.code) {
        Some(c) => backend.totp_remove(&who.subject, &f.factor, &c).await,
        None => Ok(false),
    };
    match &ok {
        Ok(true) => p.throttles.refund(reserved, true),
        Ok(false) => drop(reserved),
        Err(_) => p.throttles.refund(reserved, false),
    }
    match ok {
        Ok(true) => {
            tracing::info!("a TOTP factor was removed");
            p.account_view(&id, &who, Some("Authenticator app removed."), None, StatusCode::OK).await
        }
        Ok(false) => p.account_page(&id, &who, &factors, None, Some(WRONG), StatusCode::UNAUTHORIZED),
        Err(e) => {
            tracing::warn!(kind = e.kind(), "a TOTP factor could not be removed");
            p.account_page(&id, &who, &factors, None, Some("That didn't work. Try again in a little while."), StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}
