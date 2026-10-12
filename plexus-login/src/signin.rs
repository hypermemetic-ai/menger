//! AUTH-22: our own sign-in page, over any [`IdentityBackend`].
//!
//! | way in | on our page | at the backend |
//! | --- | --- | --- |
//! | an email whose domain is an SSO domain | the address | [`IdentityBackend::sso_for_domain`] → a redirect to the company's IdP |
//! | any other email | the address, then the code | `send_email_code`, `verify_email_code` |
//! | a username and password | the form | `password` |
//! | Google / GitHub / Apple / Microsoft | a button | `federated_url` → the provider's own screen; the callback → `complete_federated` |
//! | a second factor, an email to verify, an extra check | a code | `continue_with_code` with the backend's [`Ticket`] |
//! | an organization to pick | a button each | `choose` |
//! | enrol / remove TOTP | `/login/account` | `totp_enroll`, `totp_confirm`, `totp_factors`, `totp_remove`, `totp_discard` |
//!
//! The page offers exactly what [`IdentityBackend::capabilities`] says.
//! What it guarantees, whatever the backend (the ticket's safety list):
//!
//! 1. Server side only: forms post here; the backend holds its secrets.
//! 2. No credential, code, token or address in a log line, an error, a URL
//!    or a redirect; one answer for a wrong code and an unknown address;
//!    a code send answers the same page for any address, and is made after
//!    the answer, so its timing says nothing either.
//! 3. Throttles per address, per user and per client IP ([`Throttles`]),
//!    with backoff; the client IP from `X-Forwarded-For` only through
//!    [`TrustedProxies`]; the IP and user agent ride every backend call.
//! 4. A CSRF token on every form — an HMAC of the flow cookie (HttpOnly,
//!    SameSite=Lax, `__Host-` over https); an `Origin` from another site is
//!    refused too; every page `no-store`, unframeable, under a strict CSP
//!    ([`crate::page`]).
//! 5. Federated round trips: `state` and the PKCE verifier kept in the
//!    flow the browser's cookie names, taken once; the redirect URI exactly
//!    `<public-url>/login/callback`; where to land is checked by
//!    [`ReturnTo`] when the flow starts and when it ends.
//! 6. Tokens: the backend verifies its own (WorkOS's against its JWKS, by
//!    the gate's own adapter) and they go to the [`Continuation`], server
//!    side.
//! 7. TOTP: enrolled from the account page after a recent sign-in, the
//!    secret shown once, deleted unless confirmed in time; removed with a
//!    code from it; challenged at sign-in whenever the backend asks.
//!
//! **Not tied to an app or a backend.** What happens once someone is signed
//! in, and how the page is drawn, is the owner's [`Continuation`]: the
//! per-app login ([`crate::web_login::Login`]) passes one that starts the
//! app's session ([`crate::app_session`]); a central sign-in service would
//! pass one that issues its own token and sends the browser back to the
//! client app — an origin on [`ReturnTo`]'s list — mounting
//! [`SignInPage::standalone_router`]. The backend can be swapped while the
//! page runs ([`SignInPage::swap`]).

mod routes;
pub use routes::CallbackQuery;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock, Weak};

use async_trait::async_trait;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::client_ip::{peer_ip, Peer, TrustedProxies};
use crate::crypto::{constant_eq, hmac_sha256};
use crate::identity::{Choice, Federated, IdentityBackend, IdentityError, Method, RequestInfo, SignedIn, Social, Ticket, TotpEnrolment};
use crate::page::{self, esc, heading, Body};
use crate::sessions::{cookie, now, random_token};
use crate::throttle::{Limits, Throttles, Verdict};

/// How long a sign-in flow lasts, and how many wait at once.
const FLOW_SECS: i64 = 900;
const MAX_FLOWS: usize = 20_000;
/// How long an enrolled factor waits to be confirmed before it is deleted.
const ENROLL_SECS: i64 = 600;
/// How long an SSO lookup is remembered, and how many.
const SSO_CACHE_SECS: i64 = 300;
const MAX_SSO_CACHE: usize = 10_000;

/// Where a browser may be sent once signed in (or out): a path on this
/// origin, or an absolute URL on one of the listed origins (default: the
/// public URL's alone; a central sign-in lists its client apps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnTo {
    origins: Vec<String>,
}

impl ReturnTo {
    pub fn new<I: IntoIterator<Item = S>, S: AsRef<str>>(origins: I) -> Self {
        Self { origins: origins.into_iter().map(|o| crate::web_login::origin_of(o.as_ref())).collect() }
    }

    /// `next` if it is allowed, else `/`. A path must start with one `/`,
    /// hold no backslash or control character, and not be a sign-in page
    /// (but `/login/account`); an absolute URL must be http(s), on a listed
    /// origin, with no user or password. At most 2048 characters.
    pub fn check(&self, next: Option<&str>) -> String {
        self.allowed(next).unwrap_or_else(|| "/".into())
    }

    pub fn allowed(&self, next: Option<&str>) -> Option<String> {
        let n = next?;
        if n.is_empty() || n.len() > 2048 || n.chars().any(|c| c.is_control()) || n.contains('\\') {
            return None;
        }
        if n.starts_with('/') {
            if n == "/login/account" {
                return Some(n.into());
            }
            return (crate::web_login::safe_next(Some(n)) == n).then(|| n.to_string());
        }
        let u = url::Url::parse(n).ok()?;
        let ok = matches!(u.scheme(), "http" | "https")
            && u.username().is_empty()
            && u.password().is_none()
            && self.origins.contains(&u.origin().ascii_serialization().to_ascii_lowercase());
        ok.then(|| u.to_string())
    }
}

/// The person on the account page.
#[derive(Debug, Clone)]
pub struct AccountHolder {
    /// The backend's id for them.
    pub subject: String,
    /// The backend that signed them in (`workos`, `memory`, …).
    pub backend: String,
    pub email: Option<String>,
    /// When they signed in (unix seconds).
    pub signed_in_at: i64,
}

/// What the page's owner does: takes over once someone is signed in, and
/// draws pages. See the module docs.
#[async_trait]
pub trait Continuation: Send + Sync + 'static {
    /// Someone signed in through `backend`: start what the owner keeps for
    /// them and send the browser to `next` (already checked by [`ReturnTo`]).
    async fn signed_in(&self, who: SignedIn, backend: Arc<dyn IdentityBackend>, next: &str) -> Response;

    /// Who the request is signed in as, for the account page.
    fn account(&self, headers: &HeaderMap) -> Option<AccountHolder>;

    /// The page, drawn around `body`.
    fn draw(&self, body: Body) -> String;
}

/// The page's settings.
pub struct PageConfig {
    pub backend: Arc<dyn IdentityBackend>,
    /// Where the page is reached; the redirect URI is
    /// `<public_url>/login/callback`.
    pub public_url: String,
    /// The origins a POST may come from.
    pub origins: Vec<String>,
    pub return_to: ReturnTo,
    pub trusted_proxies: TrustedProxies,
    pub limits: Limits,
    /// The flow cookie's name (`__Host-…` over https).
    pub flow_cookie: String,
    /// The TOTP issuer authenticator apps show.
    pub totp_issuer: String,
    /// How recent a sign-in must be to change two-step verification.
    pub reauth_secs: i64,
}

/// What a code typed on the page continues.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CodeKind {
    /// An authenticator code at sign-in.
    Totp,
    /// A factor enrolled at sign-in, its first code.
    EnrollAtSignIn,
    /// An emailed code to verify the address.
    VerifyEmail,
    /// An extra emailed code (risk checks).
    ExtraCheck,
}

enum Step {
    Idle,
    CodeSent { email: String },
    Code { ticket: Ticket, kind: CodeKind, subject: Option<String> },
    Choose { ticket: Ticket, choices: Vec<Choice> },
}

struct OAuth {
    state: String,
    verifier: String,
    expires: i64,
}

struct Flow {
    step: Step,
    next: String,
    oauth: Option<OAuth>,
    expires: i64,
}

impl Flow {
    fn new(next: String) -> Self {
        Self { step: Step::Idle, next, oauth: None, expires: now() + FLOW_SECS }
    }
}

/// A factor enrolled and not yet confirmed, and the backend it is at.
struct PendingFactor {
    factor: String,
    confirm: Ticket,
    backend: Arc<dyn IdentityBackend>,
    expires: i64,
}

/// The sign-in page. See the module docs.
pub struct SignInPage {
    cfg: PageConfig,
    backend: RwLock<Arc<dyn IdentityBackend>>,
    cont: Arc<dyn Continuation>,
    throttles: Throttles,
    flows: Mutex<HashMap<String, Flow>>,
    enrolling: Mutex<HashMap<String, PendingFactor>>,
    sso: Mutex<HashMap<String, (Option<Federated>, i64)>>,
    csrf_key: [u8; 32],
    sweeping: std::sync::atomic::AtomicBool,
    redirect_uri: String,
    me: Weak<SignInPage>,
}

/// The parts of a request every handler reads.
struct Req {
    flow_id: Option<String>,
    info: RequestInfo,
    foreign: bool,
}

impl SignInPage {
    pub fn new(cfg: PageConfig, cont: Arc<dyn Continuation>) -> Arc<Self> {
        let mut csrf_key = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut csrf_key);
        let redirect_uri = format!("{}/login/callback", cfg.public_url.trim_end_matches('/'));
        Arc::new_cyclic(|me| Self {
            me: me.clone(),
            throttles: Throttles::new(cfg.limits),
            backend: RwLock::new(cfg.backend.clone()),
            cfg,
            cont,
            flows: Mutex::new(HashMap::new()),
            enrolling: Mutex::new(HashMap::new()),
            sso: Mutex::new(HashMap::new()),
            csrf_key,
            sweeping: std::sync::atomic::AtomicBool::new(false),
            redirect_uri,
        })
    }

    /// The backend signing people in now.
    pub fn backend(&self) -> Arc<dyn IdentityBackend> {
        self.backend.read().unwrap().clone()
    }

    /// Puts another backend behind the page, now. Sign-ins midway start
    /// again (their steps were the old backend's); sessions already made
    /// stay as they are, and keep refreshing and signing out through the
    /// backend that made them; unconfirmed factors are still deleted at
    /// theirs.
    pub fn swap(&self, next: Arc<dyn IdentityBackend>) {
        let old = std::mem::replace(&mut *self.backend.write().unwrap(), next);
        self.flows.lock().unwrap().clear();
        self.sso.lock().unwrap().clear();
        tracing::info!(from = old.name(), to = self.backend().name(), "the sign-in page's identity backend was swapped");
    }

    /// The redirect URI a backend must list: `<public-url>/login/callback`.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    fn secure(&self) -> bool {
        self.cfg.public_url.starts_with("https://")
    }

    fn req(&self, headers: &HeaderMap, peer: &Peer) -> Req {
        let flow_id = headers
            .get(header::COOKIE)
            .and_then(|c| c.to_str().ok())
            .and_then(|c| cookie(c, &self.cfg.flow_cookie))
            .filter(|v| v.len() >= 32 && v.len() <= 64 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
            .map(str::to_string);
        let ip = self.cfg.trusted_proxies.client_ip(peer_ip(peer), headers);
        let user_agent = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(|v| v.chars().take(512).collect());
        let foreign = headers
            .get(header::ORIGIN)
            .and_then(|o| o.to_str().ok())
            .is_some_and(|o| !self.cfg.origins.contains(&crate::web_login::origin_of(o)));
        Req { flow_id, info: RequestInfo { ip, user_agent }, foreign }
    }

    // ── the flow cookie and CSRF ────────────────────────────────────────

    /// This browser's flow id — its cookie's, or a new one and the
    /// `Set-Cookie` that plants it.
    pub fn flow_id(&self, headers: &HeaderMap) -> (String, Option<HeaderValue>) {
        match self.req(headers, &None).flow_id {
            Some(id) => (id, None),
            None => {
                let id = random_token();
                let set = self.flow_cookie_header(&id);
                (id, Some(set))
            }
        }
    }

    fn flow_cookie_header(&self, id: &str) -> HeaderValue {
        let secure = if self.secure() { "; Secure" } else { "" };
        HeaderValue::from_str(&format!("{}={id}; Path=/; HttpOnly; SameSite=Lax; Max-Age=1800{secure}", self.cfg.flow_cookie)).expect("a cookie")
    }

    /// The CSRF token for a flow: HMAC-SHA256 of its id under this
    /// process's key.
    pub fn csrf(&self, flow_id: &str) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hmac_sha256(&self.csrf_key, flow_id.as_bytes()))
    }

    fn csrf_ok(&self, flow_id: Option<&str>, token: &str) -> bool {
        flow_id.is_some_and(|id| constant_eq(self.csrf(id).as_bytes(), token.as_bytes()))
    }

    fn key(id: &str) -> String {
        Sha256::digest(id.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn with_flow<R>(&self, id: &str, f: impl FnOnce(&mut Flow) -> R) -> Option<R> {
        let mut flows = self.flows.lock().unwrap();
        let k = Self::key(id);
        match flows.get_mut(&k) {
            Some(fl) if fl.expires > now() => Some(f(fl)),
            Some(_) => {
                flows.remove(&k);
                None
            }
            None => None,
        }
    }

    fn start_flow(&self, id: &str, next: String) {
        let mut flows = self.flows.lock().unwrap();
        let t = now();
        if flows.len() >= MAX_FLOWS {
            flows.retain(|_, f| f.expires > t);
        }
        while flows.len() >= MAX_FLOWS {
            let Some(k) = flows.iter().min_by_key(|(_, f)| f.expires).map(|(k, _)| k.clone()) else { break };
            flows.remove(&k);
        }
        flows.insert(Self::key(id), Flow::new(next));
    }

    /// A new flow under a fresh id, the browser's old one dropped — so a
    /// flow id another page planted in this browser never carries a
    /// sign-in: the id and the `Set-Cookie` that hands it over.
    fn fresh_flow(&self, old: Option<&str>, next: String) -> (String, HeaderValue) {
        if let Some(o) = old {
            self.end_flow(o);
        }
        let id = random_token();
        self.start_flow(&id, next);
        let set = self.flow_cookie_header(&id);
        (id, set)
    }

    fn end_flow(&self, id: &str) {
        self.flows.lock().unwrap().remove(&Self::key(id));
    }

    fn next_of(&self, id: &str) -> String {
        self.with_flow(id, |f| f.next.clone()).unwrap_or_else(|| "/".into())
    }

    // ── drawing ─────────────────────────────────────────────────────────

    /// The ways in the backend offers, for a browser whose flow is
    /// `flow_id`: the first form (`main`) and the rest (`alts`), carrying
    /// `next`.
    pub fn entry(&self, flow_id: &str, next: &str) -> (String, String) {
        let caps = self.backend().capabilities();
        let next_e = esc(next);
        let csrf = self.csrf(flow_id);
        let email = caps.email_code.then(|| {
            let caption = match (caps.sso_by_domain, self.backend().sends_codes()) {
                (true, true) => "We'll email you a code, or send you to your company's sign-in.",
                (true, false) => "Your company's sign-in, by your work email.",
                _ => "We'll email you a code.",
            };
            format!(
                r#"<form method="post" action="/login/email" id="email-form">
      <input type="hidden" name="csrf" value="{csrf}"><input type="hidden" name="next" value="{next_e}">
      <label for="email">Email</label>
      <input id="email" name="email" type="email" placeholder="you@company.com" autocomplete="email" maxlength="254" required autofocus>
      <button class="primary" type="submit" id="email-continue">Continue</button>
      <p class="caption">{caption}</p>
    </form>"#
            )
        });
        let password = caps.password.then(|| {
            format!(
                r#"<form method="post" action="/login/password" id="password-form" autocomplete="on">
      <input type="hidden" name="csrf" value="{csrf}"><input type="hidden" name="next" value="{next_e}">
      <label for="username">Username</label>
      <input id="username" name="username" type="text" autocomplete="username" maxlength="254" required{}>
      <label for="password">Password</label>
      <input id="password" name="password" type="password" autocomplete="current-password" maxlength="1024" required>
      <button class="{}" type="submit" id="password-continue">Sign in</button>
    </form>"#,
                if email.is_none() { " autofocus" } else { "" },
                if email.is_none() { "primary" } else { "alt" }
            )
        });
        let next_q = url::form_urlencoded::byte_serialize(next.as_bytes()).collect::<String>();
        let social = caps.social.iter().map(|s| {
            format!(
                r#"<a class="alt" href="/login/social?provider={}&amp;next={next_q}">{}Continue with {}</a>"#,
                s.name(),
                crate::web_login::icon_for_label(s.label(), ""),
                s.label()
            )
        });
        let (main, rest): (String, Vec<String>) = match (email, password) {
            (Some(e), Some(p)) => (e, std::iter::once(p).chain(social).collect()),
            (Some(e), None) => (e, social.collect()),
            (None, Some(p)) => (p, social.collect()),
            (None, None) => (String::new(), social.collect()),
        };
        (main, rest.join("\n      "))
    }

    fn sign_in_heading(&self) -> String {
        heading("Sign in", "to continue")
    }

    /// The first page again, with words.
    fn restart(&self, flow_id: &str, next: &str, error: &str, status: StatusCode) -> Response {
        let (main, alts) = self.entry(flow_id, next);
        page::respond(status, self.cont.draw(Body { heading: self.sign_in_heading(), error: Some(error.into()), main, alts, extra: String::new() }))
    }

    fn step_page(&self, title: &str, sub: &str, error: Option<&str>, main: String, status: StatusCode) -> Response {
        page::respond(status, self.cont.draw(Body { heading: heading(title, sub), error: error.map(str::to_string), main, alts: String::new(), extra: String::new() }))
    }

    fn code_form(&self, flow_id: &str, action: &str, label: &str) -> String {
        let csrf = self.csrf(flow_id);
        format!(
            r#"<form method="post" action="{action}" id="code-form">
      <input type="hidden" name="csrf" value="{csrf}">
      <label for="code">{}</label>
      <input id="code" name="code" class="code" type="text" inputmode="numeric" pattern="[0-9 ]{{6,8}}" maxlength="8" autocomplete="one-time-code" required autofocus>
      <button class="primary" type="submit" id="code-continue">Continue</button>
    </form>"#,
            esc(label)
        )
    }

    fn email_code_page(&self, flow_id: &str, email: &str, error: Option<&str>, status: StatusCode) -> Response {
        let csrf = self.csrf(flow_id);
        let main = format!(
            r#"{}
    <form method="post" action="/login/resend"><input type="hidden" name="csrf" value="{csrf}"><button class="link" type="submit" id="resend">Send another code</button></form>
    <p class="note"><a href="/login">Use a different email</a></p>"#,
            self.code_form(flow_id, "/login/code", "Code")
        );
        self.step_page("Check your email", &format!("We sent a code to {email}. It lasts ten minutes."), error, main, status)
    }

    fn qr_block(e: &TotpEnrolment) -> String {
        let qr = e.qr.as_deref().filter(|q| q.starts_with("data:image/png;base64,")).map(|q| format!(r#"<div class="qr"><img alt="QR code for your authenticator app" src="{}"></div>"#, esc(q)));
        format!(
            r#"{}
      <p class="caption">{}</p>
      <p class="secret" id="totp-secret">{}</p>"#,
            qr.unwrap_or_default(),
            if e.qr.is_some() { "Or enter this key by hand:" } else { "Enter this key in your authenticator app:" },
            esc(e.secret.expose())
        )
    }

    /// The page for a code a [`Step::Code`] waits for.
    fn continue_page(&self, flow_id: &str, kind: CodeKind, enrolment: Option<&TotpEnrolment>, error: Option<&str>, status: StatusCode) -> Response {
        let form = self.code_form(flow_id, "/login/continue", if kind == CodeKind::Totp || kind == CodeKind::EnrollAtSignIn { "Code from your authenticator app" } else { "Code" });
        let (title, sub) = match kind {
            CodeKind::Totp => ("Two-step verification", "Enter the code from your authenticator app."),
            CodeKind::EnrollAtSignIn => ("Set up two-step verification", "Your organization requires it. Add this key to an authenticator app, then enter its code."),
            CodeKind::VerifyEmail => ("Verify your email", "We emailed you a code to confirm the address is yours."),
            CodeKind::ExtraCheck => ("Check your email", "To keep your account safe, we emailed you a code to confirm this sign-in."),
        };
        let main = match enrolment {
            Some(e) => format!("{}\n    {form}", Self::qr_block(e)),
            None => form,
        };
        self.step_page(title, sub, error, main, status)
    }

    fn unavailable(&self, flow_id: &str, next: &str) -> Response {
        self.restart(flow_id, next, "Sign-in is unavailable right now. Try again in a little while.", StatusCode::SERVICE_UNAVAILABLE)
    }

    fn wait_words(secs: i64) -> String {
        let mins = (secs + 59) / 60;
        if mins <= 1 {
            "Too many attempts. Wait a minute and try again.".into()
        } else {
            format!("Too many attempts. Wait {mins} minutes and try again.")
        }
    }

    // ── the flows ───────────────────────────────────────────────────────

    async fn sso_for(&self, domain: &str) -> Result<Option<Federated>, IdentityError> {
        if !self.backend().capabilities().sso_by_domain {
            return Ok(None);
        }
        if let Some((target, until)) = self.sso.lock().unwrap().get(domain).cloned() {
            if until > now() {
                return Ok(target);
            }
        }
        let target = self.backend().sso_for_domain(domain).await?;
        let mut cache = self.sso.lock().unwrap();
        if cache.len() >= MAX_SSO_CACHE {
            let t = now();
            cache.retain(|_, (_, until)| *until > t);
            if cache.len() >= MAX_SSO_CACHE {
                cache.clear();
            }
        }
        cache.insert(domain.to_string(), (target.clone(), now() + SSO_CACHE_SECS));
        Ok(target)
    }

    /// Out to `target`: the state and verifier kept in this browser's flow.
    fn federated(&self, flow_id: &str, target: &Federated, login_hint: Option<&str>, set_cookie: Option<HeaderValue>) -> Response {
        let state = random_token();
        let verifier = random_token();
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let next = self.next_of(flow_id);
        let url = match self.backend().federated_url(target, &self.redirect_uri, &state, &challenge, login_hint) {
            Ok(u) => u,
            Err(e) => return self.failed(flow_id, &next, e),
        };
        let kept = self.with_flow(flow_id, |f| {
            f.oauth = Some(OAuth { state, verifier, expires: now() + 600 });
            f.expires = f.expires.max(now() + 600);
        });
        if kept.is_none() {
            return self.restart(flow_id, "/", "That sign-in took too long. Start again.", StatusCode::BAD_REQUEST);
        }
        let mut r = (StatusCode::SEE_OTHER, [(header::LOCATION, url), (header::CACHE_CONTROL, "no-store".to_string())]).into_response();
        if let Some(c) = set_cookie {
            r.headers_mut().append(header::SET_COOKIE, c);
        }
        r
    }

    /// The backend signed someone in.
    async fn finish(&self, flow_id: &str, mut who: SignedIn) -> Response {
        let backend = self.backend();
        let next = self.cfg.return_to.check(Some(&self.next_of(flow_id)));
        if let Some(meta) = who.ctx.metadata.as_object_mut() {
            meta.insert("auth_time".into(), Value::from(now()));
            meta.insert("identity_backend".into(), Value::String(backend.name().into()));
            meta.insert("subject".into(), Value::String(who.user.subject.clone()));
            if let Some(e) = &who.user.email {
                meta.entry("email").or_insert_with(|| Value::String(e.clone()));
            }
        }
        self.end_flow(flow_id);
        if let Some(e) = who.user.email.as_deref() {
            self.throttles.tried_right(&normalize(e));
        }
        tracing::info!(principal = %who.ctx.principal().map(|p| p.to_string()).unwrap_or_default(), backend = backend.name(), method = %who.method, "signed in");
        self.cont.signed_in(who, backend, &next).await
    }

    /// A backend's answer, either way; a wrong code is `wrong`'s to draw.
    async fn answer<W>(&self, flow_id: &str, r: Result<SignedIn, IdentityError>, wrong: W) -> Response
    where
        W: FnOnce(&Self) -> Response,
    {
        match r {
            Ok(who) => self.finish(flow_id, who).await,
            Err(IdentityError::InvalidCode) => wrong(self),
            Err(e) => self.after(flow_id, e),
        }
    }

    /// What a refusal means for this flow (not a wrong code: the caller's).
    fn after(&self, flow_id: &str, err: IdentityError) -> Response {
        let next = self.next_of(flow_id);
        let set_code = |ticket: Ticket, kind: CodeKind, subject: Option<String>| {
            self.with_flow(flow_id, |f| f.step = Step::Code { ticket, kind, subject });
        };
        match err {
            IdentityError::MfaRequired { ticket, subject } => {
                set_code(ticket, CodeKind::Totp, subject);
                self.continue_page(flow_id, CodeKind::Totp, None, None, StatusCode::OK)
            }
            IdentityError::MfaEnrollmentRequired { ticket, subject, enrolment } => {
                if let Some(s) = &subject {
                    self.remember_enrollment(s, &enrolment);
                }
                set_code(ticket, CodeKind::EnrollAtSignIn, subject);
                self.continue_page(flow_id, CodeKind::EnrollAtSignIn, Some(enrolment.as_ref()), None, StatusCode::OK)
            }
            IdentityError::EmailUnverified { ticket } => {
                set_code(ticket, CodeKind::VerifyEmail, None);
                self.continue_page(flow_id, CodeKind::VerifyEmail, None, None, StatusCode::OK)
            }
            IdentityError::ExtraCheck { ticket } => {
                set_code(ticket, CodeKind::ExtraCheck, None);
                self.continue_page(flow_id, CodeKind::ExtraCheck, None, None, StatusCode::OK)
            }
            IdentityError::Choose { ticket, choices } => {
                if choices.is_empty() {
                    return self.restart(flow_id, &next, "Your account belongs to no organization that can sign in here.", StatusCode::UNAUTHORIZED);
                }
                let csrf = self.csrf(flow_id);
                let buttons: String = choices
                    .iter()
                    .enumerate()
                    .map(|(i, o)| format!(r#"<form method="post" action="/login/choose"><input type="hidden" name="csrf" value="{csrf}"><input type="hidden" name="index" value="{i}"><button class="alt" type="submit">{}</button></form>"#, esc(&o.name)))
                    .collect::<Vec<_>>()
                    .join("\n      ");
                self.with_flow(flow_id, |f| f.step = Step::Choose { ticket, choices });
                self.step_page("Choose an organization", "Your account belongs to more than one.", None, format!(r#"<div class="alts">{buttons}</div>"#), StatusCode::OK)
            }
            IdentityError::FederatedRequired { target } => self.federated(flow_id, &target, None, None),
            IdentityError::MethodsRequired { methods } => {
                let names: Vec<&str> = methods
                    .iter()
                    .map(|m| match m {
                        Method::Social(s) => s.label(),
                        Method::EmailCode => "an email code",
                        Method::Password => "a password",
                        Method::Sso => "your company's sign-in",
                    })
                    .collect();
                let words = if names.is_empty() {
                    "Your organization requires another way to sign in. Ask its administrator.".to_string()
                } else {
                    format!("Your organization requires signing in with {}.", names.join(" or "))
                };
                self.restart(flow_id, &next, &words, StatusCode::UNAUTHORIZED)
            }
            IdentityError::InvalidCode => self.restart(flow_id, &next, "That sign-in didn't work. Start again.", StatusCode::UNAUTHORIZED),
            e => self.failed(flow_id, &next, e),
        }
    }

    /// A refusal that is never the person's: words, and the log.
    fn failed(&self, flow_id: &str, next: &str, e: IdentityError) -> Response {
        match e {
            IdentityError::RateLimited { .. } => self.restart(flow_id, next, &Self::wait_words(60), StatusCode::TOO_MANY_REQUESTS),
            IdentityError::Unsupported(what) => {
                tracing::info!(what, "a sign-in step this page cannot offer with this backend");
                self.restart(flow_id, next, "That way of signing in isn't offered here. Try another.", StatusCode::UNAUTHORIZED)
            }
            IdentityError::Misconfigured(m) => {
                tracing::error!(problem = %m, "the identity backend refuses this app: check its configuration");
                self.unavailable(flow_id, next)
            }
            IdentityError::Unavailable(m) => {
                tracing::warn!(problem = %m, "the identity backend is unavailable");
                self.unavailable(flow_id, next)
            }
            other => self.after(flow_id, other),
        }
    }

    fn remember_enrollment(&self, subject: &str, e: &TotpEnrolment) {
        let pending = PendingFactor { factor: e.factor.clone(), confirm: e.confirm.clone(), backend: self.backend(), expires: now() + ENROLL_SECS };
        let old = self.enrolling.lock().unwrap().insert(subject.to_string(), pending);
        if let Some(old) = old {
            self.discard(old);
        }
        if let Some(me) = self.me.upgrade() {
            me.sweep_later();
        }
    }

    /// Deletes an unconfirmed factor at its backend, off the request.
    fn discard(&self, p: PendingFactor) {
        tokio::spawn(async move {
            if let Err(e) = p.backend.totp_discard(&p.factor).await {
                tracing::warn!(kind = e.kind(), "an unconfirmed TOTP factor could not be deleted");
            }
        });
    }

    /// Unconfirmed factors past their time deleted; expired flows dropped.
    pub async fn sweep(&self) {
        let t = now();
        let stale: Vec<PendingFactor> = {
            let mut e = self.enrolling.lock().unwrap();
            let keys: Vec<String> = e.iter().filter(|(_, f)| f.expires <= t).map(|(k, _)| k.clone()).collect();
            keys.into_iter().filter_map(|k| e.remove(&k)).collect()
        };
        for p in stale {
            if let Err(e) = p.backend.totp_discard(&p.factor).await {
                tracing::warn!(kind = e.kind(), "an unconfirmed TOTP factor could not be deleted");
            }
        }
        self.flows.lock().unwrap().retain(|_, f| f.expires > t);
    }

    fn sweep_later(self: &Arc<Self>) {
        if self.sweeping.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let Some(p) = me.upgrade() else { return };
                p.sweep().await;
            }
        });
    }
}

/// An address as the throttles key it.
fn normalize(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// A plausible address's domain, lower case: one `@`, a dotted domain, no
/// spaces or controls, at most 254 characters.
fn email_domain(email: &str) -> Option<String> {
    let e = email.trim();
    if e.len() > 254 || e.chars().any(|c| c.is_whitespace() || c.is_control() || c == '<' || c == '>') {
        return None;
    }
    let (local, domain) = e.rsplit_once('@')?;
    if local.is_empty() || local.contains('@') || domain.len() < 3 || !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
        return None;
    }
    Some(domain.to_ascii_lowercase())
}

/// A code as typed: digits only, spaces dropped; six to eight of them.
fn clean_code(code: &str) -> Option<String> {
    let c: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    (c.len() >= 6 && c.len() <= 8 && c.chars().all(|c| c.is_ascii_digit())).then_some(c)
}

/// The neutral name of a social provider in a query.
fn social_of(name: Option<&str>) -> Option<Social> {
    name.and_then(Social::parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_to_takes_paths_and_listed_origins_only() {
        let r = ReturnTo::new(["https://app.example"]);
        for ok in ["/", "/app/#/ticket/X-1", "/login/account", "https://app.example/app/x?y=1"] {
            assert!(r.allowed(Some(ok)).is_some(), "{ok}");
        }
        for bad in [
            "//evil.example",
            "/\\evil.example",
            "\\\\evil.example",
            "https://evil.example/",
            "https://app.example.evil.example/",
            "https://user@app.example/",
            "http://app.example/",
            "javascript:alert(1)",
            "/login?next=/",
            "/a\nb",
            "/\t/evil.example",
            "evil.example",
            "",
        ] {
            assert_eq!(r.check(Some(bad)), "/", "{bad:?}");
        }
    }

    #[test]
    fn an_address_and_a_code_are_checked_before_anything_is_sent() {
        assert_eq!(email_domain(" Ada@Example.COM ").as_deref(), Some("example.com"));
        for bad in ["", "ada", "ada@", "@x.y", "ada@localhost", "a b@x.y", "a@x\n.y", "<a@x.y>", "a@.x", "a@x."] {
            assert!(email_domain(bad).is_none(), "{bad:?}");
        }
        assert_eq!(clean_code("123 456").as_deref(), Some("123456"));
        for bad in ["12345", "abcdef", "123456789", "12345a"] {
            assert!(clean_code(bad).is_none(), "{bad}");
        }
        assert_eq!(social_of(Some("GitHub")), Some(Social::GitHub));
        assert_eq!(social_of(Some("myspace")), None);
    }
}
