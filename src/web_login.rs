//! IDENTITY-19: the login page and its routes, on the shell.
//!
//! | route | what it does |
//! | --- | --- |
//! | `GET /login` | the page: an email field handed to the IdP (`--login-email`, IDENTITY-30), a button per `--login-button`, nostr when offered, and a password form (one provider's password grant) — beside an email field, only at `/login?password`, automation's way in |
//! | `POST /login` | the password grant at that provider, from the shell |
//! | `GET /login/start` | out to an OIDC provider's authorization endpoint: code flow with PKCE, a connection hint (Google, Apple, SAML) |
//! | `GET /login/callback` | back: the state checked against the browser's own, the code traded for a token |
//! | `POST /login/nostr` | a NIP-98 event a NIP-07 extension signed for this URL |
//! | `POST /logout` | the session ended (and the provider's, where it keeps one: WorkOS) |
//! | `GET /login/me` | who the session is, as JSON, or 401 |
//! | `POST /login/dev` | IDENTITY-35, test only: be a seeded or any user, through plexus-idp ([`crate::dev_login`]) |
//!
//! Every way in verifies the provider's token (or event) with that
//! provider's adapter — the same check the socket makes — and then starts a
//! session ([`crate::sessions`]). The token stays in the shell; the browser
//! gets an `HttpOnly` session cookie.
//!
//! The page is self-contained (web/login.html), so it can later be handed
//! to an IdP as its hosted login page.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use base64::Engine as _;
use plexus_auth_core::adapters::Nip98Adapter;
use plexus_auth_core::identity::{Authenticator, Presentation};
use plexus_core::plexus::AuthContext;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use menger_tickets_api::access::Access;
use menger_tickets_api::members::Role;

use crate::dev_login::{self, DevLogin};
use crate::login::{LoginButton, Provider};
use crate::sessions::{self, Sessions};

/// The login transaction's cookie for a session cookie of that name: binds
/// a callback to the browser that started it. Named per shell as the
/// session's is (IDENTITY-38); `__Secure-` where the session is `__Host-`
/// (this one lives at `Path=/login`, which `__Host-` forbids).
fn txn_cookie(session: &str) -> String {
    match session.strip_prefix("__Host-") {
        Some(rest) => format!("__Secure-{}", rest.replace("menger_session", "menger_login")),
        None => session.replace("menger_session", "menger_login"),
    }
}

/// What the shell's login is configured with (main's flags).
pub struct LoginConfig {
    pub providers: Vec<Provider>,
    pub buttons: Vec<LoginButton>,
    /// The provider the password form goes to; `None`: the first OIDC one
    /// that takes passwords; `none`: no password form (IDENTITY-30: the IdP
    /// takes passwords on its own page).
    pub password: Option<String>,
    pub forgot: Option<String>,
    /// Where the shell is reached, e.g. `http://127.0.0.1:4451`: the
    /// redirect URI's and NIP-98 URL's base, and the origin POSTs must
    /// come from.
    pub public_url: String,
    /// More origins the shell is reached at (`localhost` beside `127.0.0.1`).
    pub also: Vec<String>,
    /// Whether anonymous visitors are sent to the page.
    pub require: bool,
    pub title: &'static str,
    /// IDENTITY-30: the email field's provider (`--login-email
    /// workos:authkit`): the address goes to it as `login_hint`, so an SSO
    /// domain goes straight to its company's IdP and anyone else finishes on
    /// the IdP's page with the address filled in. Beside it, the password
    /// form shows only at `/login?password` (automation's way in).
    pub email: Option<LoginButton>,
    /// IDENTITY-35: the dev sign-in (test only), and the rule its grants
    /// are written through.
    pub dev: Option<DevLogin>,
    pub access: Option<Access>,
}

/// The email field's button, as `/login/start?button=` names it.
const EMAIL: &str = "email";

#[derive(Clone, Debug)]
struct Discovery {
    authorization_endpoint: Option<String>,
    token_endpoint: String,
}

/// The login service. See the module docs.
pub struct Login {
    sessions: Arc<Sessions>,
    providers: HashMap<String, (Provider, Arc<dyn Authenticator>)>,
    password: Option<String>,
    buttons: Vec<LoginButton>,
    email: Option<LoginButton>,
    forgot: Option<String>,
    public_url: String,
    origins: Vec<String>,
    nostr: Option<Nip98Adapter>,
    http: reqwest::Client,
    discovery: Mutex<HashMap<String, Discovery>>,
    pub require: bool,
    title: &'static str,
    dev: Option<DevLogin>,
    access: Option<Access>,
}

impl Login {
    pub fn new(cfg: LoginConfig, sessions: Arc<Sessions>) -> anyhow::Result<Self> {
        let public_url = cfg.public_url.trim_end_matches('/').to_string();
        let mut providers = HashMap::new();
        let mut nostr = None;
        // The NIP-98 event names the page's own URL: any origin the shell is
        // reached at (127.0.0.1 and localhost), not only the public URL.
        let nostr_urls: Vec<String> = std::iter::once(origin_of(&public_url))
            .chain(cfg.also.iter().map(|o| origin_of(o)))
            .map(|o| format!("{o}/login/nostr"))
            .collect();
        // IDENTITY-42: the agents' key proof is no way in a person is offered.
        for p in cfg.providers.iter().filter(|p| !p.is_key()) {
            if matches!(p, Provider::Nostr { .. }) {
                nostr = Some(Nip98Adapter::new(nostr_urls.clone())?.for_method("POST"));
            }
            providers.insert(p.name().to_string(), (p.clone(), p.adapter()?));
        }
        let password = match cfg.password {
            Some(none) if none == "none" => None,
            Some(name) => {
                anyhow::ensure!(
                    providers.get(&name).is_some_and(|(p, _)| p.oidc().is_some()),
                    "--login-password {name}: no OIDC --auth-provider has that name"
                );
                anyhow::ensure!(
                    providers[&name].0.takes_passwords(),
                    "--login-password {name}: {name} takes passwords on its own page; give it a --login-button instead"
                );
                Some(name)
            }
            None => cfg.providers.iter().find(|p| p.oidc().is_some() && p.takes_passwords()).map(|p| p.name().to_string()),
        };
        let email = cfg.email.map(|e| LoginButton { label: EMAIL.into(), ..e });
        anyhow::ensure!(
            !cfg.buttons.iter().any(|b| b.label == EMAIL),
            "--login-button {EMAIL}: that label is the email field's"
        );
        for b in cfg.buttons.iter().chain(email.iter()) {
            let Some((p, _)) = providers.get(&b.provider) else {
                anyhow::bail!("--login-button {}: no --auth-provider is named {}", b.label, b.provider);
            };
            anyhow::ensure!(
                p.oidc().is_some_and(|(_, _, client)| client.is_some()),
                "--login-button {}: {} needs a client=<id> in its --auth-provider for the code flow",
                b.label,
                b.provider
            );
        }
        if let Some(d) = &cfg.dev {
            anyhow::ensure!(
                providers.get(&d.provider).is_some_and(|(p, _)| p.oidc().is_some() && p.takes_passwords()),
                "--dev-login {}: no --auth-provider of that name takes passwords (name plexus-idp's, e.g. plexus-idp)",
                d.provider
            );
            anyhow::ensure!(cfg.access.is_some(), "--dev-login needs the members rule to write its grants");
        }
        let mut origins = vec![origin_of(&public_url)];
        origins.extend(cfg.also.iter().map(|o| origin_of(o)));
        Ok(Self {
            sessions,
            providers,
            password,
            buttons: cfg.buttons,
            email,
            forgot: cfg.forgot,
            public_url,
            origins,
            nostr,
            http: reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build()?,
            discovery: Mutex::new(HashMap::new()),
            require: cfg.require,
            title: cfg.title,
            dev: cfg.dev,
            access: cfg.access,
        })
    }

    /// The `Host` values the public URL and the other origins name: `host`
    /// on a default port, else `host:port`.
    pub fn hosts(&self) -> Vec<String> {
        self.origins
            .iter()
            .filter_map(|o| url::Url::parse(o).ok())
            .filter_map(|u| {
                let host = u.host_str()?.to_ascii_lowercase();
                Some(match u.port() {
                    Some(p) => format!("{host}:{p}"),
                    None => host,
                })
            })
            .collect()
    }

    /// The session a request's cookie names, while it lasts.
    pub fn session(&self, headers: &HeaderMap) -> Option<AuthContext> {
        let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
        self.sessions.get(sessions::cookie(cookies, self.sessions.cookie_name())?)
    }

    /// A POST from a page not ours is refused (login CSRF, logout CSRF):
    /// a browser always sends `Origin` on a POST; a client may omit it.
    fn foreign(&self, headers: &HeaderMap) -> bool {
        match headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
            None => false,
            Some(o) => !self.origins.contains(&origin_of(o)),
        }
    }

    async fn discover(&self, name: &str, issuer: &str) -> Result<Discovery, String> {
        if let Some(d) = self.discovery.lock().unwrap().get(name) {
            return Ok(d.clone());
        }
        let url = format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'));
        let doc: Value = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|_| "the sign-in service is unreachable".to_string())?
            .json()
            .await
            .map_err(|_| "the sign-in service answered oddly".to_string())?;
        let s = |k: &str| doc.get(k).and_then(Value::as_str).map(str::to_string);
        let d = Discovery {
            authorization_endpoint: s("authorization_endpoint"),
            token_endpoint: s("token_endpoint").ok_or("the sign-in service names no token endpoint")?,
        };
        self.discovery.lock().unwrap().insert(name.to_string(), d.clone());
        Ok(d)
    }

    /// POSTs a form to a provider's token endpoint; the access token and
    /// the whole response, or the words to show.
    async fn token(&self, endpoint: &str, form: &[(&str, &str)]) -> Result<(String, Value), String> {
        let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
        let resp = self
            .http
            .post(endpoint)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(|_| "the sign-in service is unreachable".to_string())?;
        let v: Value = resp.json().await.map_err(|_| "the sign-in service answered oddly".to_string())?;
        if let Some(t) = v.get("access_token").and_then(Value::as_str) {
            return Ok((t.to_string(), v.clone()));
        }
        Err(match v.get("error").and_then(Value::as_str) {
            Some("invalid_grant") => "Wrong username or password.".to_string(),
            _ => "Sign-in was refused.".to_string(),
        })
    }

    /// The provider's adapter checks the token: the identity, or why not.
    async fn verify(&self, provider: &str, token: String) -> Result<AuthContext, String> {
        let (_, adapter) = self.providers.get(provider).ok_or("unknown provider")?;
        adapter
            .authenticate(&Presentation::Bearer { token })
            .await
            .map(|a| a.into_auth_context())
            .map_err(|_| "The sign-in service's answer could not be verified.".to_string())
    }

    /// A password grant at an OIDC provider, its token verified: the
    /// identity, or the words to show.
    async fn password_grant(&self, name: &str, username: &str, password: &str) -> Result<AuthContext, String> {
        let (provider, _) = self.providers.get(name).ok_or("unknown provider")?;
        let (issuer, audience, client) = provider.oidc().ok_or("that provider takes no passwords")?;
        let d = self.discover(name, issuer).await?;
        let mut form = vec![("grant_type", "password"), ("username", username), ("password", password), ("audience", audience), ("scope", "openid profile email")];
        if let Some(c) = client {
            form.push(("client_id", c));
        }
        let (token, _) = self.token(&d.token_endpoint, &form).await?;
        self.verify(name, token).await
    }

    /// IDENTITY-35: whether this browser's sign-in goes to plexus-idp (the
    /// dev sign-in configured, and its checkbox ticked).
    pub fn dev_on(&self, headers: &HeaderMap) -> bool {
        self.dev.is_some()
            && headers
                .get(header::COOKIE)
                .and_then(|c| c.to_str().ok())
                .and_then(|c| sessions::cookie(c, dev_login::COOKIE))
                .is_some_and(|v| v == "1")
    }

    /// A session for an identity, and the response that hands it over.
    fn sign_in(&self, ctx: AuthContext, next: &str) -> Response {
        self.hand_over(ctx, see_other(next))
    }

    /// A session for an identity, its cookies set on `r`.
    fn hand_over(&self, ctx: AuthContext, mut r: Response) -> Response {
        let (id, expires) = self.sessions.create(ctx);
        let max_age = (expires - sessions::now()).max(0);
        let (name, secure) = (self.sessions.cookie_name(), self.secure());
        let h = r.headers_mut();
        h.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!("{name}={id}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}")).unwrap(),
        );
        h.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!("{}=; Path=/login; HttpOnly; SameSite=Lax; Max-Age=0{secure}", txn_cookie(name))).unwrap(),
        );
        r
    }

    /// `; Secure` when the shell is reached over https.
    fn secure(&self) -> &'static str {
        if self.public_url.starts_with("https://") { "; Secure" } else { "" }
    }

    /// The page, with `error` shown and `next` carried. `with_password`:
    /// the password form even beside an email field (`/login?password`).
    pub fn page(&self, next: &str, error: Option<&str>, with_password: bool) -> String {
        self.page_for(next, error, with_password, false)
    }

    /// The page; `dev`: plexus-idp alone, the dev sign-in's panel
    /// (IDENTITY-35).
    pub fn page_for(&self, next: &str, error: Option<&str>, with_password: bool, dev: bool) -> String {
        let next_e = esc(next);
        let toggle = self.dev.as_ref().map(|d| {
            format!(
                r#"<label class="dev-toggle"><input type="checkbox" id="dev-login"{}> Send all sign-in to {}<small>Test only. Be any user, with any role.</small></label>"#,
                if dev { " checked" } else { "" },
                esc(&d.provider)
            )
        });
        if let (true, Some(d)) = (dev, &self.dev) {
            return self.fill(error, toggle, Some(dev_panel(d, &next_e)), String::new(), None, false, true);
        }
        let password = self.password.as_ref().filter(|_| self.email.is_none() || with_password).map(|_| {
            let forgot = self
                .forgot
                .as_ref()
                .map(|u| format!(r#"<a href="{}">Forgot password?</a>"#, esc(u)))
                .unwrap_or_default();
            // IDENTITY-30: beside the email field, the form is plainly not
            // where a person's IdP password goes.
            let caption = match (&self.email, &self.password) {
                (Some(_), Some(p)) => format!(r#"<p class="caption">With a {} account</p>
      "#, esc(p)),
                _ => String::new(),
            };
            format!(
                r#"<form method="post" action="/login" autocomplete="on">
      <input type="hidden" name="next" value="{next_e}">
      {caption}<label for="username">Email or username</label>
      <input id="username" name="username" type="text" autocomplete="username" required autofocus>
      <div class="row"><label for="password">Password</label>{forgot}</div>
      <input id="password" name="password" type="password" autocomplete="current-password" required>
      <button class="primary" type="submit">Sign in</button>
    </form>"#
            )
        });
        let buttons: String = self
            .buttons
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let label = esc(&b.label);
                let icon = icon_for(b);
                if b.wants_email() {
                    format!(
                        r#"<button class="alt" type="button" data-sso="sso-{i}">{icon}Continue with {label}</button>
      <form class="sso" id="sso-{i}" method="get" action="/login/start">
        <input type="hidden" name="button" value="{label}"><input type="hidden" name="next" value="{next_e}">
        <input type="email" name="login_hint" placeholder="you@company.com" autocomplete="email" required>
        <button class="alt" type="submit">Continue</button>
      </form>"#
                    )
                } else {
                    format!(
                        r#"<form method="get" action="/login/start"><input type="hidden" name="button" value="{label}"><input type="hidden" name="next" value="{next_e}"><button class="alt" type="submit">{icon}Continue with {label}</button></form>"#
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("\n      ");
        // IDENTITY-30: the email field comes first — what a person signs in
        // with; the other ways in (and, at /login?password, automation's
        // form) follow the "or".
        let (password, buttons) = match &self.email {
            Some(e) => {
                // An address with no account yet: the IdP's own sign-up, the
                // address filled in. menger cannot know which it is (nor should
                // a page say whether an address has an account).
                let sign_up = self
                    .providers
                    .get(&e.provider)
                    .and_then(|(p, _)| p.sign_up_hint())
                    .map(|_| r#"
      <button class="link" type="submit" name="signup" value="1" formnovalidate id="email-signup">New here? Create an account</button>"#)
                    .unwrap_or_default();
                let email = format!(
                    r#"<form method="get" action="/login/start" id="email-form">
      <input type="hidden" name="button" value="{EMAIL}"><input type="hidden" name="next" value="{next_e}">
      <label for="email">Email</label>
      <input id="email" name="login_hint" type="email" placeholder="you@company.com" autocomplete="email" required autofocus>
      <button class="primary" type="submit" id="email-continue">Continue</button>{sign_up}
    </form>"#
                );
                let rest = [buttons, password.unwrap_or_default()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n      ");
                (Some(email), rest)
            }
            None => (password, buttons),
        };
        let nostr = self.nostr.as_ref().map(|_| {
            format!(r#"<button class="alt" type="button" id="nostr" data-next="{next_e}">{KEY_ICON}Continue with nostr</button>"#)
        });
        let alts = !buttons.is_empty() || nostr.is_some();
        self.fill(error, toggle, password, buttons, nostr, alts, false)
    }

    /// The instance, as the page's side column lists it.
    fn facts(&self, dev: bool) -> String {
        let via = match (&self.dev, &self.email) {
            (Some(d), _) if dev => d.provider.clone(),
            (_, Some(e)) => e.provider.clone(),
            _ => {
                let mut names: Vec<&str> = self.providers.keys().map(String::as_str).collect();
                names.sort_unstable();
                names.join(", ")
            }
        };
        [("instance", self.title.to_string()), ("version", env!("CARGO_PKG_VERSION").to_string()), ("sign-in", via)]
            .iter()
            .map(|(k, v)| format!("<dt>{k}</dt><dd>{}</dd>", esc(v)))
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn fill(&self, error: Option<&str>, toggle: Option<String>, password: Option<String>, buttons: String, nostr: Option<String>, alts: bool, dev: bool) -> String {
        let heading = if dev {
            r#"<h1>Sign in as anyone</h1>
      <p class="sub">Through plexus-idp. Pick a seeded user, or name one.</p>"#.to_string()
        } else {
            format!(r#"<h1>Sign in</h1>
      <p class="sub">to continue to {}</p>"#, esc(self.title))
        };
        include_str!("../web/login.html")
            .replace("{{DEV}}", &toggle.unwrap_or_default())
            .replace("{{HEADING}}", &heading)
            .replace("{{FACTS}}", &self.facts(dev))
            .replace("{{TITLE}}", self.title)
            .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
            .replace(
                "{{ERROR}}",
                &error.map(|e| format!(r#"<p class="error" role="alert">{}</p>"#, esc(e))).unwrap_or_default(),
            )
            .replace("{{OR}}", if password.is_some() && alts { r#"<div class="or">or</div>"# } else { "" })
            .replace("{{PASSWORD}}", &password.unwrap_or_default())
            .replace("{{BUTTONS}}", &buttons)
            .replace("{{NOSTR}}", &nostr.unwrap_or_default())
    }
}

// ── Handlers ───────────────────────────────────────────────────────────

/// The page's query. IDENTITY-38: there is no `error`: the page shows only
/// words the shell wrote, never text a link carries ("sign in at evil…").
#[derive(Deserialize)]
pub struct PageQuery {
    next: Option<String>,
    /// IDENTITY-30: the password form beside an email field (automation).
    password: Option<String>,
}

pub async fn page(State(login): State<Arc<Login>>, headers: HeaderMap, Query(q): Query<PageQuery>) -> Response {
    let next = safe_next(q.next.as_deref());
    if login.session(&headers).is_some() {
        return see_other(&next);
    }
    let html = login.page_for(&next, None, q.password.is_some(), login.dev_on(&headers));
    (StatusCode::OK, [(header::CACHE_CONTROL, "no-store"), (header::X_FRAME_OPTIONS, "DENY")], Html(html)).into_response()
}

#[derive(Deserialize)]
pub struct PasswordForm {
    username: String,
    password: String,
    next: Option<String>,
}

pub async fn password(State(login): State<Arc<Login>>, headers: HeaderMap, Form(f): Form<PasswordForm>) -> Response {
    let next = safe_next(f.next.as_deref());
    if login.foreign(&headers) {
        return (StatusCode::FORBIDDEN, "Forbidden: sign in from menger's own page").into_response();
    }
    let Some(name) = login.password.clone() else {
        return page_response(&login, &next, Some("Passwords are not taken here."), StatusCode::NOT_FOUND);
    };
    match login.password_grant(&name, &f.username, &f.password).await {
        Ok(ctx) => login.sign_in(ctx, &next),
        Err(e) => page_response(&login, &next, Some(&e), StatusCode::UNAUTHORIZED),
    }
}

#[derive(Deserialize)]
pub struct StartQuery {
    button: String,
    login_hint: Option<String>,
    next: Option<String>,
    /// IDENTITY-30: open the provider's sign-up instead of its sign-in.
    signup: Option<String>,
}

pub async fn start(State(login): State<Arc<Login>>, headers: HeaderMap, Query(q): Query<StartQuery>) -> Response {
    let next = safe_next(q.next.as_deref());
    // IDENTITY-35: with the checkbox ticked, every way in is plexus-idp.
    if login.dev_on(&headers) {
        return see_other(&format!("/login?next={}", url::form_urlencoded::byte_serialize(next.as_bytes()).collect::<String>()));
    }
    let Some(b) = login.buttons.iter().chain(login.email.iter()).find(|b| b.label == q.button).cloned() else {
        return page_response(&login, &next, Some("That sign-in is not offered here."), StatusCode::NOT_FOUND);
    };
    begin(&login, &b, q.login_hint.as_deref(), q.signup.is_some(), next).await
}

/// Out to a button's provider: the authorization code flow with PKCE, the
/// state bound to this browser by the transaction cookie.
async fn begin(login: &Login, b: &LoginButton, login_hint: Option<&str>, sign_up: bool, next: String) -> Response {
    let login = login;
    let (provider, _) = &login.providers[&b.provider];
    let (issuer, audience, client) = provider.oidc().expect("checked at start");
    let client = client.expect("checked at start");
    let d = match login.discover(&b.provider, issuer).await {
        Ok(d) => d,
        Err(e) => return page_response(&login, &next, Some(&e), StatusCode::BAD_GATEWAY),
    };
    let Some(auth) = d.authorization_endpoint else {
        return page_response(&login, &next, Some("That sign-in service has no authorization endpoint."), StatusCode::BAD_GATEWAY);
    };
    let verifier = sessions::random_token();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = login.sessions.begin(&b.provider, verifier, next.clone());
    let Ok(mut url) = url::Url::parse(&auth) else {
        return page_response(&login, &next, Some("That sign-in service's address is malformed."), StatusCode::BAD_GATEWAY);
    };
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("response_type", "code")
            .append_pair("client_id", client)
            .append_pair("redirect_uri", &format!("{}/login/callback", login.public_url))
            .append_pair("scope", "openid profile email")
            .append_pair("audience", audience)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        for (k, v) in provider.authorize_hint(b.connection.as_deref()) {
            qp.append_pair(k, &v);
        }
        if let Some((k, v)) = provider.sign_up_hint().filter(|_| sign_up) {
            qp.append_pair(k, v);
        }
        if let Some(h) = login_hint.filter(|h| !h.trim().is_empty()) {
            qp.append_pair("login_hint", h.trim());
        }
    }
    let mut r = (StatusCode::FOUND, [(header::LOCATION, url.to_string())]).into_response();
    r.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{}={state}; Path=/login; HttpOnly; SameSite=Lax; Max-Age=600{}",
            txn_cookie(login.sessions.cookie_name()),
            login.secure()
        ))
        .unwrap(),
    );
    r
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub async fn callback(State(login): State<Arc<Login>>, headers: HeaderMap, Query(q): Query<CallbackQuery>) -> Response {
    let fail = |why: &str| page_response(&login, "/", Some(why), StatusCode::UNAUTHORIZED);
    // IDENTITY-38: the provider's words go to the log, not the page — a
    // link can carry any `error_description` to this URL.
    if let Some(error) = q.error.as_deref() {
        let clean = |s: &str| s.chars().filter(|c| !c.is_control()).take(300).collect::<String>();
        tracing::info!(
            "sign-in came back with an error: {} — {}",
            clean(error),
            clean(q.error_description.as_deref().unwrap_or(""))
        );
        return fail(if error == "access_denied" {
            "The sign-in was cancelled."
        } else {
            "The sign-in service did not sign you in. Start again."
        });
    }
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return fail("The sign-in service sent back nothing to sign in with.");
    };
    // The state must be the one this browser was sent out with.
    let ours = headers
        .get(header::COOKIE)
        .and_then(|c| c.to_str().ok())
        .and_then(|c| sessions::cookie(c, &txn_cookie(login.sessions.cookie_name())).map(str::to_string));
    if ours.as_deref() != Some(state.as_str()) {
        return fail("That sign-in was not started in this browser. Start again.");
    }
    let Some(pending) = login.sessions.finish(&state) else {
        return fail("That sign-in took too long or was already used. Start again.");
    };
    let (provider, _) = &login.providers[&pending.provider];
    let (issuer, _, client) = provider.oidc().expect("a button's provider");
    let attempt = async {
        let d = login.discover(&pending.provider, issuer).await?;
        let redirect = format!("{}/login/callback", login.public_url);
        let form = [
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("client_id", client.unwrap_or_default()),
            ("code_verifier", pending.verifier.as_str()),
        ];
        let (token, response) = login.token(&d.token_endpoint, &form).await?;
        let mut ctx = login.verify(&pending.provider, token).await?;
        named_by(&mut ctx, &pending.provider, &response);
        Ok::<_, String>(ctx)
    };
    match attempt.await {
        Ok(ctx) => login.sign_in(ctx, &pending.next),
        Err(e) => fail(&e),
    }
}

/// IDENTITY-30: a token response that names the person — WorkOS's `user`,
/// whose access token names nobody — gives the session its name and email,
/// when that user is the verified token's own subject. It came from the
/// provider's token endpoint over the same exchange as the token.
fn named_by(ctx: &mut AuthContext, provider: &str, response: &Value) {
    let Some(user) = response.get("user") else { return };
    let s = |k: &str| user.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    let Some(id) = s("id") else { return };
    let ours = plexus_auth_core::identity::Principal::oidc(provider, id).ok();
    if ours.is_none() || ctx.principal() != ours {
        return;
    }
    let full = [s("first_name"), s("last_name")].into_iter().flatten().collect::<Vec<_>>().join(" ");
    let name = s("name").map(str::to_string).or((!full.is_empty()).then_some(full)).or(s("email").map(str::to_string));
    let Some(meta) = ctx.metadata.as_object_mut() else { return };
    if let Some(name) = name {
        meta.insert("name".into(), Value::String(name.clone()));
        meta.insert("username".into(), Value::String(name));
    }
    if let Some(email) = s("email") {
        meta.insert("email".into(), Value::String(email.to_string()));
    }
}

#[derive(Deserialize)]
pub struct NextQuery {
    next: Option<String>,
}

pub async fn nostr(State(login): State<Arc<Login>>, headers: HeaderMap, Query(q): Query<NextQuery>) -> Response {
    let next = safe_next(q.next.as_deref());
    if login.foreign(&headers) {
        tracing::warn!("nostr sign-in refused: a POST from another page's origin {:?}", headers.get(header::ORIGIN));
        return (StatusCode::FORBIDDEN, "Forbidden: sign in from menger's own page").into_response();
    }
    let Some(adapter) = &login.nostr else {
        return (StatusCode::NOT_FOUND, "nostr is not offered here").into_response();
    };
    let event = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Nostr "))
        .map(str::to_string);
    let Some(token) = event else {
        tracing::warn!("nostr sign-in refused: no Authorization: Nostr header");
        return (StatusCode::BAD_REQUEST, "an Authorization: Nostr <event> header is required").into_response();
    };
    // IDENTITY-32: every attempt in the log, so a failing signer can be
    // diagnosed without the person's screen. The event is public; its
    // signature proves nothing to anyone else (one use, one URL).
    let detail = nip98_detail(&token);
    // The page's own script asks for JSON (IDENTITY-30): where to go, or
    // why not, in words it shows beside the button.
    let json = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("application/json"));
    let outcome = adapter.authenticate(&Presentation::Bearer { token }).await;
    match &outcome {
        Ok(_) => tracing::info!("nostr sign-in: {detail}"),
        Err(e) => tracing::warn!("nostr sign-in refused: {e} — {detail}"),
    }
    match outcome {
        Ok(a) if json => login.hand_over(a.into_auth_context(), axum::Json(json!({ "next": next })).into_response()),
        Ok(a) => login.sign_in(a.into_auth_context(), &next),
        Err(e) if json => (StatusCode::UNAUTHORIZED, axum::Json(json!({ "error": nostr_refusal(&e) }))).into_response(),
        Err(_) => (StatusCode::UNAUTHORIZED, "the nostr event was not accepted").into_response(),
    }
}

#[derive(Deserialize)]
pub struct NostrTrace {
    stage: String,
    #[serde(default)]
    detail: String,
}

/// IDENTITY-32: the page's nostr steps that never reach `/login/nostr` —
/// waiting on the extension, the extension refusing — into the log, so a
/// failing signer can be read without the person's screen. Only from the
/// shell's own pages; short, printable, and logged, nothing more.
pub async fn nostr_trace(State(login): State<Arc<Login>>, headers: HeaderMap, axum::Json(t): axum::Json<NostrTrace>) -> Response {
    if login.foreign(&headers) || headers.get(header::ORIGIN).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    let clean = |s: &str, n: usize| s.chars().filter(|c| !c.is_control()).take(n).collect::<String>();
    tracing::info!("nostr sign-in (browser): {} — {}", clean(&t.stage, 32), clean(&t.detail, 300));
    StatusCode::NO_CONTENT.into_response()
}

/// What a NIP-98 event says, for the log: its key, kind, age, URL and
/// method — or that it could not be read.
fn nip98_detail(token: &str) -> String {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(token.trim())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token.trim()));
    let Some(ev) = decoded.ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
        return format!("an unreadable event ({} chars)", token.len());
    };
    let tag = |name: &str| {
        ev.get("tags").and_then(Value::as_array).and_then(|tags| {
            tags.iter().find_map(|t| (t.get(0).and_then(Value::as_str) == Some(name)).then(|| t.get(1).and_then(Value::as_str)).flatten())
        })
    };
    let age = ev.get("created_at").and_then(Value::as_i64).map(|at| sessions::now() - at);
    format!(
        "key {}…, kind {}, signed {} s ago, u {}, method {}",
        ev.get("pubkey").and_then(Value::as_str).map(|p| &p[..p.len().min(12)]).unwrap_or("?"),
        ev.get("kind").map(Value::to_string).unwrap_or_else(|| "?".into()),
        age.map(|a| a.to_string()).unwrap_or_else(|| "?".into()),
        tag("u").unwrap_or("?"),
        tag("method").unwrap_or("?"),
    )
}

/// Why a NIP-98 event was refused, for the person who signed it.
fn nostr_refusal(e: &plexus_auth_core::identity::AuthnRejection) -> String {
    use plexus_auth_core::identity::AuthnRejection as R;
    match e {
        R::Expired => "The signature is too old: menger takes one signed within a minute. Press the button again and approve it promptly.".into(),
        R::Malformed(m) if m.contains("already used") => "That signature was already used. Press the button again.".into(),
        other => format!("Your nostr key's signature was not accepted ({other})."),
    }
}

#[derive(Deserialize)]
pub struct DevForm {
    user: String,
    /// An existing plexus-idp account's own password: signs in as it, no
    /// registration and no grants.
    password: Option<String>,
    /// For a user not in the roster: `none`, or a role on `scope`.
    role: Option<String>,
    scope: Option<String>,
    next: Option<String>,
}

/// IDENTITY-35: `POST /login/dev` — a seeded user, any username (registered
/// at plexus-idp if new, with the role asked for), or a plexus-idp account
/// by its password. Always a real password grant, verified.
pub async fn dev(State(login): State<Arc<Login>>, headers: HeaderMap, Form(f): Form<DevForm>) -> Response {
    let next = safe_next(f.next.as_deref());
    if login.foreign(&headers) {
        return (StatusCode::FORBIDDEN, "Forbidden: sign in from menger's own page").into_response();
    }
    let (Some(d), Some(access)) = (login.dev.clone(), login.access.clone()) else {
        return page_response(&login, &next, Some("The dev sign-in is not offered here."), StatusCode::NOT_FOUND);
    };
    let refuse = |e: &str, status| {
        (status, [(header::CACHE_CONTROL, "no-store"), (header::X_FRAME_OPTIONS, "DENY")], Html(login.page_for(&next, Some(e), false, true)))
            .into_response()
    };
    // A plexus-idp account by its own password: exactly that account.
    if let Some(pw) = f.password.as_deref().filter(|p| !p.is_empty()) {
        return match login.password_grant(&d.provider, f.user.trim(), pw).await {
            Ok(ctx) => login.sign_in(ctx, &next),
            Err(e) => refuse(&e, StatusCode::UNAUTHORIZED),
        };
    }
    let name = match dev_login::username(&f.user) {
        Ok(n) => n,
        Err(e) => return refuse(&e, StatusCode::BAD_REQUEST),
    };
    let roster = d.user(&name).is_some();
    let grants: Vec<(Role, String)> = match d.user(&name) {
        Some(u) => u.grants.clone(),
        None => match f.role.as_deref().map(str::trim).filter(|r| !r.is_empty() && *r != "none") {
            None => Vec::new(),
            Some(r) => {
                let scope = f.scope.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("*");
                match (r.parse::<Role>(), menger_tickets_api::members::scope_of(scope)) {
                    (Ok(r), Ok(s)) => vec![(r, s)],
                    (Err(e), _) | (_, Err(e)) => return refuse(&e, StatusCode::BAD_REQUEST),
                }
            }
        },
    };
    let ctx = match login.password_grant(&d.provider, &name, &d.password).await {
        Ok(ctx) => ctx,
        Err(first) => match d.register(&name).await {
            Ok(true) => match login.password_grant(&d.provider, &name, &d.password).await {
                Ok(ctx) => ctx,
                Err(e) => return refuse(&format!("{name} was registered, but signing in failed: {e}"), StatusCode::BAD_GATEWAY),
            },
            Ok(false) => {
                return refuse(
                    &format!("{name} already has a plexus-idp account with another password ({first}). Sign in with its password below."),
                    StatusCode::UNAUTHORIZED,
                )
            }
            Err(e) => return refuse(&e, StatusCode::BAD_GATEWAY),
        },
    };
    let principal = ctx.principal().map(|p| p.to_string()).unwrap_or_else(|| ctx.user_id.clone());
    // A roster user holds exactly its roster's grants; anyone else gains
    // the one asked for.
    let granted = if roster {
        access.dev_set_grants(&principal, &grants).await
    } else {
        futures::future::try_join_all(grants.iter().map(|(r, sc)| access.dev_grant(&principal, sc, *r))).await.map(|_| ())
    };
    if let Err(e) = granted {
        return refuse(&format!("{name} signed in, but its grants failed: {e}"), StatusCode::INTERNAL_SERVER_ERROR);
    }
    tracing::info!(user = %name, %principal, grants = %dev_login::describe(&grants), "dev sign-in");
    login.sign_in(ctx, &next)
}

/// The dev sign-in's panel: the roster, anyone, an account's own password.
fn dev_panel(d: &DevLogin, next_e: &str) -> String {
    let chips = |grants: &[(Role, String)]| -> String {
        if grants.is_empty() {
            return r#"<span class="grant none">no access</span>"#.into();
        }
        grants
            .iter()
            .map(|(r, sc)| {
                let on = if sc == "*" { "everywhere".to_string() } else { format!("<code>{}</code>", esc(sc)) };
                format!(r#"<span class="grant {r}"><b>{r}</b>{on}</span>"#)
            })
            .collect()
    };
    let roster: String = d
        .users
        .iter()
        .map(|u| {
            format!(
                r#"<form method="post" action="/login/dev"><input type="hidden" name="next" value="{next_e}"><input type="hidden" name="user" value="{n}"><button class="who" type="submit" title="Sign in as {n}: {t}"><span class="uname">{n}</span><span class="grants">{g}</span><span class="go" aria-hidden="true">→</span></button></form>"#,
                n = esc(&u.name),
                t = esc(&dev_login::describe(&u.grants)),
                g = chips(&u.grants)
            )
        })
        .collect::<Vec<_>>()
        .join("\n        ");
    let roster = if roster.is_empty() {
        String::new()
    } else {
        format!(
            r#"<h2>Seeded users</h2>
      <div class="roster">
        {roster}
      </div>"#
        )
    };
    format!(
        r#"{roster}
      <h2>Anyone</h2>
      <form method="post" action="/login/dev" class="anyone" id="dev-any">
        <input type="hidden" name="next" value="{next_e}">
        <div><label for="dev-user">Username</label>
        <input id="dev-user" name="user" type="text" required autocomplete="off" pattern="[A-Za-z0-9._\-]{{1,64}}" placeholder="e.g. reviewer-2"></div>
        <div><label for="dev-role">Role</label>
        <select id="dev-role" name="role"><option value="none">none</option><option value="viewer" selected>viewer</option><option value="editor">editor</option><option value="admin">admin</option></select></div>
        <div><label for="dev-scope">On</label>
        <input id="dev-scope" name="scope" type="text" value="*" placeholder="* or SHORTCODE" title="* for everything, or a shortcode such as IDENTITY"></div>
        <button class="primary" type="submit">Sign in</button>
      </form>
      <p class="caption">A new name is registered at plexus-idp. A seeded name keeps its own roles.</p>
      <details>
        <summary>Sign in with a plexus-idp password</summary>
        <form method="post" action="/login/dev" autocomplete="on">
          <input type="hidden" name="next" value="{next_e}">
          <label for="dev-account">Username</label>
          <input id="dev-account" name="user" type="text" autocomplete="username" required>
          <label for="dev-password">Password</label>
          <input id="dev-password" name="password" type="password" autocomplete="current-password" required>
          <button class="alt" type="submit">Sign in</button>
        </form>
      </details>"#
    )
}

pub async fn logout(State(login): State<Arc<Login>>, headers: HeaderMap) -> Response {
    if login.foreign(&headers) {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }
    // IDENTITY-30: where the provider keeps a session of its own (WorkOS),
    // the browser goes on to end it, and comes back to the page.
    let onward = login.session(&headers).and_then(|ctx| {
        let (p, _) = login.providers.get(&ctx.get_metadata_string("provider")?)?;
        p.sign_out_url(&ctx.session_id, &format!("{}/login", login.public_url))
    });
    let name = login.sessions.cookie_name();
    if let Some(id) = headers.get(header::COOKIE).and_then(|c| c.to_str().ok()).and_then(|c| sessions::cookie(c, name)) {
        login.sessions.end(id);
    }
    let mut r = match onward {
        Some(url) => (StatusCode::SEE_OTHER, [(header::LOCATION, url), (header::CACHE_CONTROL, "no-store".to_string())]).into_response(),
        None => see_other("/login"),
    };
    r.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!("{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}", login.secure())).unwrap(),
    );
    r
}

pub async fn me(State(login): State<Arc<Login>>, headers: HeaderMap) -> Response {
    match login.session(&headers) {
        Some(ctx) => {
            let principal = ctx.principal().map(|p| p.to_string()).unwrap_or_else(|| ctx.user_id.clone());
            let body = json!({
                "principal": principal,
                "name": ctx.get_metadata_string("name").unwrap_or_else(|| principal.clone()),
                "provider": ctx.get_metadata_string("provider"),
                "expires_at": ctx.metadata.get("expires_at"),
            });
            ([(header::CACHE_CONTROL, "no-store")], axum::Json(body)).into_response()
        }
        None => (StatusCode::UNAUTHORIZED, [(header::CACHE_CONTROL, "no-store")], axum::Json(json!({ "error": "signed out" }))).into_response(),
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn page_response(login: &Login, next: &str, error: Option<&str>, status: StatusCode) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store"), (header::X_FRAME_OPTIONS, "DENY")],
        Html(login.page(next, error, false)),
    )
        .into_response()
}

fn see_other(to: &str) -> Response {
    (StatusCode::SEE_OTHER, [(header::LOCATION, to.to_string()), (header::CACHE_CONTROL, "no-store".to_string())]).into_response()
}

/// Where to go after signing in: a path on this origin, else `/`.
pub fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n)
            if n.starts_with('/')
                && !n.starts_with("//")
                && !n.contains('\\')
                && n.len() <= 512
                && !n.chars().any(|c| c.is_control())
                && !n.starts_with("/login") =>
        {
            n.to_string()
        }
        _ => "/".to_string(),
    }
}

/// `scheme://host[:port]`, lowercase, no path or trailing slash.
pub fn origin_of(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => u.origin().ascii_serialization().to_ascii_lowercase(),
        Err(_) => url.trim_end_matches('/').to_ascii_lowercase(),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

const KEY_ICON: &str = r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" d="M14.5 9.5a4.5 4.5 0 1 1-1.3-3.2M14.5 9.5L21 16v3h-3v-2h-2v-2l-2.6-2.6"/><circle cx="10" cy="9.5" r="1.3" fill="currentColor"/></svg>"#;

fn icon_for(b: &LoginButton) -> &'static str {
    let k = format!("{} {}", b.label, b.connection.as_deref().unwrap_or("")).to_ascii_lowercase();
    if k.contains("google") {
        r##"<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="#4285F4" d="M23.5 12.3c0-.8-.1-1.6-.2-2.3H12v4.5h6.5a5.6 5.6 0 0 1-2.4 3.6v3h3.9c2.3-2.1 3.5-5.2 3.5-8.8z"/><path fill="#34A853" d="M12 24c3.2 0 6-1.1 8-2.9l-3.9-3c-1.1.7-2.5 1.2-4.1 1.2-3.1 0-5.8-2.1-6.7-5H1.3v3.1A12 12 0 0 0 12 24z"/><path fill="#FBBC05" d="M5.3 14.3a7.2 7.2 0 0 1 0-4.6V6.6H1.3a12 12 0 0 0 0 10.8z"/><path fill="#EA4335" d="M12 4.8c1.8 0 3.3.6 4.6 1.8l3.4-3.4A12 12 0 0 0 1.3 6.6l4 3.1c.9-2.9 3.6-4.9 6.7-4.9z"/></svg>"##
    } else if k.contains("email") || k.contains("mail ") {
        r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" d="M3 6h18v12H3zM3 7l9 6 9-6"/></svg>"#
    } else if k.contains("microsoft") {
        r##"<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="#F25022" d="M2 2h9.5v9.5H2z"/><path fill="#7FBA00" d="M12.5 2H22v9.5h-9.5z"/><path fill="#00A4EF" d="M2 12.5h9.5V22H2z"/><path fill="#FFB900" d="M12.5 12.5H22V22h-9.5z"/></svg>"##
    } else if k.contains("github") {
        r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M12 1.5a10.5 10.5 0 0 0-3.3 20.5c.5.1.7-.2.7-.5v-1.8c-2.9.6-3.5-1.4-3.5-1.4-.5-1.2-1.2-1.5-1.2-1.5-.9-.7.1-.6.1-.6 1 .1 1.6 1.1 1.6 1.1.9 1.6 2.5 1.1 3.1.9.1-.7.4-1.1.6-1.4-2.3-.3-4.8-1.2-4.8-5.2 0-1.1.4-2.1 1.1-2.8-.1-.3-.5-1.4.1-2.8 0 0 .9-.3 2.9 1.1a10 10 0 0 1 5.3 0c2-1.4 2.9-1.1 2.9-1.1.6 1.4.2 2.5.1 2.8.7.7 1.1 1.7 1.1 2.8 0 4-2.5 4.9-4.8 5.2.4.3.7 1 .7 2v3c0 .3.2.6.7.5A10.5 10.5 0 0 0 12 1.5z"/></svg>"#
    } else if k.contains("apple") {
        r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M16.4 12.7c0-2.6 2.1-3.8 2.2-3.9a4.8 4.8 0 0 0-3.8-2c-1.6-.2-3.1.9-3.9.9-.8 0-2-.9-3.4-.9a5 5 0 0 0-4.2 2.6c-1.8 3.1-.5 7.7 1.3 10.2.9 1.2 1.9 2.6 3.2 2.6 1.3-.1 1.8-.8 3.3-.8 1.6 0 2 .8 3.4.8s2.3-1.3 3.1-2.5a11 11 0 0 0 1.4-2.9 4.5 4.5 0 0 1-2.6-4.1zM13.9 5.1c.7-.9 1.2-2 1-3.1-1 0-2.2.7-2.9 1.5-.6.7-1.2 1.9-1 3 1.1.1 2.2-.6 2.9-1.4z"/></svg>"#
    } else {
        r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="1.8" d="M4 21V7l8-4 8 4v14M9 21v-5h6v5M8 10h.01M12 10h.01M16 10h.01M8 13h.01M16 13h.01"/></svg>"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_stays_on_this_origin() {
        assert_eq!(safe_next(Some("/app/#/ticket/X-1")), "/app/#/ticket/X-1");
        for bad in ["//evil.example", "https://evil.example", "/\\evil", "", "/login?next=/", "/a\nb"] {
            assert_eq!(safe_next(Some(bad)), "/", "{bad:?}");
        }
        assert_eq!(safe_next(None), "/");
    }

    #[test]
    fn the_response_names_only_the_token_s_own_subject() {
        use plexus_auth_core::identity::{Authenticated, Principal};
        let ctx = || {
            let p = Principal::oidc("workos", "user_01ADA").unwrap();
            Authenticated::new(p, "session_01ADA", vec![], json!({ "name": "user_01ADA", "provider": "workos" })).into_auth_context()
        };
        let user = |id: &str| json!({ "user": { "id": id, "email": "ada@example.com", "first_name": "Ada", "last_name": "Lovelace" } });
        let mut c = ctx();
        named_by(&mut c, "workos", &user("user_01ADA"));
        assert_eq!(c.get_metadata_string("name").as_deref(), Some("Ada Lovelace"));
        assert_eq!(c.get_metadata_string("email").as_deref(), Some("ada@example.com"));
        let mut c = ctx();
        named_by(&mut c, "workos", &user("user_01EVE"));
        assert_eq!(c.get_metadata_string("name").as_deref(), Some("user_01ADA"), "another user's name is not taken");
        let mut c = ctx();
        named_by(&mut c, "workos", &json!({ "user": { "id": "user_01ADA", "email": "ada@example.com" } }));
        assert_eq!(c.get_metadata_string("name").as_deref(), Some("ada@example.com"), "no name: the email");
    }

    #[test]
    fn an_origin_is_scheme_host_and_port() {
        assert_eq!(origin_of("http://127.0.0.1:4451/"), "http://127.0.0.1:4451");
        assert_eq!(origin_of("HTTP://LOCALHOST:4451/app"), "http://localhost:4451");
    }

    #[test]
    fn text_is_escaped_into_the_page() {
        assert_eq!(esc(r#"<b a="1">'&"#), "&lt;b a=&quot;1&quot;&gt;&#39;&amp;");
    }
}
