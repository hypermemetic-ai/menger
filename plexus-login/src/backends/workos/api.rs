//! AUTH-22: the WorkOS User Management API, as the WorkOS identity backend
//! ([`super::WorkOs`]) calls it — server side, with the environment's API
//! key. WorkOS's shapes and names live here and in `super`, nowhere else.
//!
//! | call | endpoint |
//! | --- | --- |
//! | [`WorkOsApi::send_code`] | `POST /user_management/magic_auth` |
//! | [`WorkOsApi::authenticate`] | `POST /user_management/authenticate` (every grant: [`Grant`]) |
//! | [`WorkOsApi::authorization_url`] | `GET /user_management/authorize` (built here, never fetched) |
//! | [`WorkOsApi::sso_organization`] | `GET /organizations?domains=…`, then `GET /connections?organization_id=…` |
//! | [`WorkOsApi::challenge`] | `POST /auth/factors/{id}/challenge` |
//! | [`WorkOsApi::verify`] | `POST /auth/challenges/{id}/verify` |
//! | [`WorkOsApi::enroll_totp`] | `POST /user_management/users/{id}/auth_factors` |
//! | [`WorkOsApi::factors`] | `GET /user_management/users/{id}/auth_factors` |
//! | [`WorkOsApi::delete_factor`] | `DELETE /auth/factors/{id}` |
//! | [`WorkOsApi::revoke_session`] | `POST /user_management/sessions/revoke` |
//!
//! Nothing here logs. The key is an [`ApiKey`], whose `Debug` prints no
//! secret; answers that carry a secret (a Magic Auth code, a TOTP secret, a
//! token) are read into types whose `Debug` doesn't either, and a send's
//! answer is dropped unread. Every refusal WorkOS documents is a variant of
//! [`AuthError`]; the page decides what a person is told.

use std::fmt;

use reqwest::StatusCode;
use serde_json::{json, Map, Value};

pub use crate::identity::{RequestInfo as Client, Secret};

/// The environment's API key (`sk_…`): a box-only secret. Its `Debug` and
/// `Display` say only that it is one.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// A key from its text (trimmed). Refuses anything that is not
    /// `sk_…`-shaped, so a path or an empty file is caught at startup.
    pub fn new(text: &str) -> Result<Self, String> {
        let k = text.trim();
        if k.len() < 16 || !k.starts_with("sk_") || k.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err("the WorkOS API key is not an sk_… key".into());
        }
        Ok(Self(k.to_string()))
    }

    /// The key from `<APP>_WORKOS_API_KEY` (`app` upper-cased, non-alphanumerics
    /// as `_`), else from `file`; `None` when neither is set.
    pub fn from_env_or_file(app: &str, file: Option<&std::path::Path>) -> Result<Option<Self>, String> {
        let var = Self::env_var(app);
        if let Ok(v) = std::env::var(&var) {
            if !v.trim().is_empty() {
                return Self::new(&v).map(Some).map_err(|e| format!("{var}: {e}"));
            }
        }
        match file {
            Some(path) => {
                let text = std::fs::read_to_string(path).map_err(|e| format!("the WorkOS API key file {}: {e}", path.display()))?;
                Self::new(&text).map(Some).map_err(|e| format!("{}: {e}", path.display()))
            }
            None => Ok(None),
        }
    }

    fn expose(&self) -> &str {
        &self.0
    }

    /// `<APP>_WORKOS_API_KEY` for an app called `app`.
    pub fn env_var(app: &str) -> String {
        format!("{}_WORKOS_API_KEY", app.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect::<String>())
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<secret>)")
    }
}


/// A grant `POST /user_management/authenticate` takes.
pub enum Grant<'a> {
    /// `authorization_code`, with PKCE's verifier beside the secret.
    Code { code: &'a str, verifier: &'a str },
    /// `urn:workos:oauth:grant-type:magic-auth:code`.
    MagicAuth { email: &'a str, code: &'a str },
    /// `urn:workos:oauth:grant-type:mfa-totp`.
    Totp { pending: &'a Secret, challenge: &'a str, code: &'a str },
    /// `urn:workos:oauth:grant-type:email-verification:code`.
    EmailVerification { pending: &'a Secret, code: &'a str },
    /// `urn:workos:oauth:grant-type:organization-selection`.
    OrganizationSelection { pending: &'a Secret, organization: &'a str },
    /// `urn:workos:oauth:grant-type:radar-email-challenge:code`.
    RadarEmail { pending: &'a Secret, challenge: &'a str, code: &'a str },
    /// `refresh_token`.
    Refresh { token: &'a Secret },
}

impl Grant<'_> {
    fn fields(&self, body: &mut Map<String, Value>) {
        let mut put = |k: &str, v: &str| {
            body.insert(k.into(), Value::String(v.into()));
        };
        match self {
            Grant::Code { code, verifier } => {
                put("grant_type", "authorization_code");
                put("code", code);
                put("code_verifier", verifier);
            }
            Grant::MagicAuth { email, code } => {
                put("grant_type", "urn:workos:oauth:grant-type:magic-auth:code");
                put("email", email);
                put("code", code);
            }
            Grant::Totp { pending, challenge, code } => {
                put("grant_type", "urn:workos:oauth:grant-type:mfa-totp");
                put("pending_authentication_token", pending.expose());
                put("authentication_challenge_id", challenge);
                put("code", code);
            }
            Grant::EmailVerification { pending, code } => {
                put("grant_type", "urn:workos:oauth:grant-type:email-verification:code");
                put("pending_authentication_token", pending.expose());
                put("code", code);
            }
            Grant::OrganizationSelection { pending, organization } => {
                put("grant_type", "urn:workos:oauth:grant-type:organization-selection");
                put("pending_authentication_token", pending.expose());
                put("organization_id", organization);
            }
            Grant::RadarEmail { pending, challenge, code } => {
                put("grant_type", "urn:workos:oauth:grant-type:radar-email-challenge:code");
                put("pending_authentication_token", pending.expose());
                put("radar_challenge_id", challenge);
                put("code", code);
            }
            Grant::Refresh { token } => {
                put("grant_type", "refresh_token");
                put("refresh_token", token.expose());
            }
        }
    }

    /// The grant's name, for the log (nothing secret).
    pub fn name(&self) -> &'static str {
        match self {
            Grant::Code { .. } => "authorization_code",
            Grant::MagicAuth { .. } => "magic-auth",
            Grant::Totp { .. } => "mfa-totp",
            Grant::EmailVerification { .. } => "email-verification",
            Grant::OrganizationSelection { .. } => "organization-selection",
            Grant::RadarEmail { .. } => "radar-email-challenge",
            Grant::Refresh { .. } => "refresh_token",
        }
    }
}

/// A WorkOS user, as an authenticate answer names it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

impl User {
    fn from(v: &Value) -> Option<Self> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        Some(Self { id: s("id")?, email: s("email"), first_name: s("first_name"), last_name: s("last_name") })
    }

    /// The whole `user` object, as the existing `named_by` reads it.
    pub fn json(&self) -> Value {
        json!({ "id": self.id, "email": self.email, "first_name": self.first_name, "last_name": self.last_name })
    }
}

/// A signed-in answer.
#[derive(Debug, Clone)]
pub struct Authenticated {
    pub user: User,
    pub access_token: Secret,
    pub refresh_token: Option<Secret>,
    pub organization_id: Option<String>,
    pub method: Option<String>,
}

/// An organization offered for selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organization {
    pub id: String,
    pub name: String,
}

/// Why `authenticate` did not sign anyone in — every documented answer.
#[derive(Debug)]
pub enum AuthError {
    /// `mfa_challenge`: the user has factors; challenge one and send a TOTP.
    MfaChallenge { pending: Secret, factors: Vec<String>, user: Option<User> },
    /// `mfa_enrollment`: the environment requires MFA and the user has none.
    MfaEnrollment { pending: Secret, user: Option<User> },
    /// `email_verification_required`: WorkOS emailed a code.
    EmailVerification { pending: Secret },
    /// `organization_selection_required`.
    OrganizationSelection { pending: Secret, organizations: Vec<Organization> },
    /// `sso_required`: only these SSO connections may sign the user in.
    SsoRequired { connections: Vec<String> },
    /// `organization_authentication_methods_required`: the organization's
    /// policy allows only these methods (WorkOS names: `google_oauth`, …).
    MethodsRequired { connections: Vec<String>, methods: Vec<String> },
    /// `radar_email_challenge`: WorkOS emailed a code.
    RadarEmail { pending: Secret, challenge: String },
    /// `radar_sms_challenge`: not offered on this page.
    RadarSms,
    /// The code (or token, or verifier) was wrong, expired, used, or named
    /// nobody (`invalid_grant`, `entity_not_found`, 400/404/422): one answer,
    /// so the page cannot tell an address with no account from a wrong code.
    Refused,
    /// 429: WorkOS's own limit.
    RateLimited { retry_after: Option<u64> },
    /// The environment refuses this app (`invalid_client`, 401, 403): an
    /// operator's problem, never the person's.
    Misconfigured(String),
    /// WorkOS could not be reached or failed (5xx), or answered oddly.
    Unavailable(String),
}

impl AuthError {
    /// The refusal's kind, for the log: never a token, code or address.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::MfaChallenge { .. } => "mfa_challenge",
            Self::MfaEnrollment { .. } => "mfa_enrollment",
            Self::EmailVerification { .. } => "email_verification_required",
            Self::OrganizationSelection { .. } => "organization_selection_required",
            Self::SsoRequired { .. } => "sso_required",
            Self::MethodsRequired { .. } => "organization_authentication_methods_required",
            Self::RadarEmail { .. } => "radar_email_challenge",
            Self::RadarSms => "radar_sms_challenge",
            Self::Refused => "refused",
            Self::RateLimited { .. } => "rate_limited",
            Self::Misconfigured(_) => "misconfigured",
            Self::Unavailable(_) => "unavailable",
        }
    }
}

/// What a non-authenticate call ended in, when not its answer.
#[derive(Debug)]
pub enum CallError {
    /// 400/404/422: the thing named is not there, or the input was refused.
    Refused(u16),
    RateLimited,
    Misconfigured(String),
    Unavailable(String),
}

/// A TOTP factor just enrolled: shown once, then forgotten.
pub struct Enrolled {
    pub factor: String,
    pub challenge: String,
    /// `data:image/png;base64,…`
    pub qr_code: String,
    pub secret: Secret,
    pub uri: Secret,
}

impl fmt::Debug for Enrolled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Enrolled").field("factor", &self.factor).field("challenge", &self.challenge).finish_non_exhaustive()
    }
}

/// A factor a user has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Factor {
    pub id: String,
    pub kind: String,
    pub created_at: Option<String>,
}

/// One WorkOS environment's client, with its key. See the module docs.
pub struct WorkOsApi {
    base: String,
    client_id: String,
    key: ApiKey,
    http: reqwest::Client,
}

impl fmt::Debug for WorkOsApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkOsApi").field("base", &self.base).field("client_id", &self.client_id).finish_non_exhaustive()
    }
}

/// The answer's status and JSON body (an empty or non-JSON body is `null`).
type Answer = (StatusCode, Value);

impl WorkOsApi {
    /// The API at `base` (`https://api.workos.com` unless given) for
    /// `client_id`, calling with `key`.
    pub fn new(base: Option<&str>, client_id: &str, key: ApiKey) -> anyhow::Result<Self> {
        let base = base.unwrap_or(plexus_auth_core::adapters::WORKOS_API).trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        Ok(Self { base, client_id: client_id.to_string(), key, http })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Answer, CallError> {
        let mut req = self.http.request(method, format!("{}{path}", self.base)).bearer_auth(self.key.expose());
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.map_err(|e| CallError::Unavailable(transport_error(&e)))?;
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| CallError::Unavailable(transport_error(&e)))?;
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Ok((status, value))
    }

    /// The common refusals of a non-authenticate call; `Ok` for 2xx.
    async fn expect_ok(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value, CallError> {
        let (status, value) = self.call(method, path, body).await?;
        match status.as_u16() {
            200..=299 => Ok(value),
            401 | 403 => Err(CallError::Misconfigured(format!("{path}: HTTP {}", status.as_u16()))),
            429 => Err(CallError::RateLimited),
            // Live on staging (AUTH-22): a method switched off in the
            // environment is the operator's to switch on.
            // AUTH-24 #10: logged by its code only, never WorkOS's words.
            400 if value.get("error").and_then(Value::as_str) == Some("authentication_method_not_allowed") => {
                Err(CallError::Misconfigured(format!("{path}: authentication_method_not_allowed (the method is disabled in the WorkOS environment)")))
            }
            s @ (400 | 404 | 409 | 422) => Err(CallError::Refused(s)),
            s => Err(CallError::Unavailable(format!("{path}: HTTP {s}"))),
        }
    }

    /// Sends a Magic Auth code to `email`. The answer (which carries the
    /// code) is dropped unread: only its status counts.
    pub async fn send_code(&self, email: &str, client: &Client) -> Result<(), CallError> {
        let mut body = json!({ "email": email });
        radar(&mut body, client);
        self.expect_ok(reqwest::Method::POST, "/user_management/magic_auth", Some(body)).await.map(|_| ())
    }

    /// `POST /user_management/authenticate` with this app's client id and
    /// secret, the grant, and the client's address and user agent.
    pub async fn authenticate(&self, grant: Grant<'_>, client: &Client) -> Result<Authenticated, AuthError> {
        let mut body = json!({ "client_id": self.client_id, "client_secret": self.key.expose() });
        let obj = body.as_object_mut().expect("an object");
        grant.fields(obj);
        radar(&mut body, client);
        // The secret rides the body, not a header: no bearer here.
        let resp = self
            .http
            .post(format!("{}/user_management/authenticate", self.base))
            .json(&body)
            .send()
            .await
            .map_err(|e| AuthError::Unavailable(transport_error(&e)))?;
        let status = resp.status();
        let retry_after = resp.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
        let bytes = resp.bytes().await.map_err(|e| AuthError::Unavailable(transport_error(&e)))?;
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if status.is_success() {
            return authenticated(&v).ok_or_else(|| AuthError::Unavailable("authenticate: an answer without a user or token".into()));
        }
        Err(auth_error(status, &v, retry_after))
    }

    /// The URL a browser goes to for a social provider, an SSO organization
    /// or connection: `selector` is `provider=GoogleOAuth`,
    /// `organization_id=org_…` or `connection_id=conn_…`.
    pub fn authorization_url(&self, redirect_uri: &str, state: &str, challenge: &str, selector: (&str, &str), login_hint: Option<&str>) -> String {
        let mut url = url::Url::parse(&format!("{}/user_management/authorize", self.base)).expect("the API base is a URL");
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("response_type", "code")
                .append_pair("client_id", &self.client_id)
                .append_pair("redirect_uri", redirect_uri)
                .append_pair("state", state)
                .append_pair("code_challenge", challenge)
                .append_pair("code_challenge_method", "S256")
                .append_pair(selector.0, selector.1);
            if let Some(h) = login_hint {
                q.append_pair("login_hint", h);
            }
        }
        url.to_string()
    }

    /// The organization whose **verified** domain `domain` is and which has
    /// an **active** SSO connection: `Ok(Some(org))`; `Ok(None)` when there
    /// is none. Two such organizations is a misconfiguration (refused, so
    /// an address is never sent to a guessed IdP). A pending or failed
    /// domain never routes anyone: anyone can claim a domain unverified.
    pub async fn sso_organization(&self, domain: &str) -> Result<Option<String>, CallError> {
        let path = format!("/organizations?limit=10&domains={}", enc(domain));
        let orgs = self.expect_ok(reqwest::Method::GET, &path, None).await?;
        let mut found = Vec::new();
        for org in orgs.get("data").and_then(Value::as_array).into_iter().flatten() {
            let verified = org.get("domains").and_then(Value::as_array).into_iter().flatten().any(|d| {
                d.get("domain").and_then(Value::as_str).is_some_and(|x| x.eq_ignore_ascii_case(domain))
                    && d.get("state").and_then(Value::as_str) == Some("verified")
            });
            let Some(id) = org.get("id").and_then(Value::as_str).filter(|_| verified) else { continue };
            let conns = self.expect_ok(reqwest::Method::GET, &format!("/connections?limit=100&organization_id={}", enc(id)), None).await?;
            let active = conns.get("data").and_then(Value::as_array).into_iter().flatten().any(|c| {
                c.get("state").and_then(Value::as_str) == Some("active")
                    && c.get("connection_type").and_then(Value::as_str).is_some_and(is_sso_type)
            });
            if active {
                found.push(id.to_string());
            }
        }
        match found.len() {
            0 => Ok(None),
            1 => Ok(found.pop()),
            n => Err(CallError::Misconfigured(format!("{n} organizations with an active SSO connection claim one verified domain"))),
        }
    }

    /// Challenges a TOTP factor: the challenge's id.
    pub async fn challenge(&self, factor: &str) -> Result<String, CallError> {
        let v = self.expect_ok(reqwest::Method::POST, &format!("/auth/factors/{}/challenge", enc(factor)), Some(json!({}))).await?;
        v.get("id").and_then(Value::as_str).map(str::to_string).ok_or_else(|| CallError::Unavailable("challenge: no id".into()))
    }

    /// Whether `code` answers the challenge.
    pub async fn verify(&self, challenge: &str, code: &str) -> Result<bool, CallError> {
        match self.expect_ok(reqwest::Method::POST, &format!("/auth/challenges/{}/verify", enc(challenge)), Some(json!({ "code": code }))).await {
            Ok(v) => Ok(v.get("valid").and_then(Value::as_bool).unwrap_or(false)),
            // A wrong or expired challenge is a wrong code.
            Err(CallError::Refused(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Enrolls a TOTP factor for `user`, labelled `issuer` / `label` in the
    /// authenticator app.
    pub async fn enroll_totp(&self, user: &str, issuer: &str, label: &str) -> Result<Enrolled, CallError> {
        let body = json!({ "type": "totp", "totp_issuer": issuer, "totp_user": label });
        let v = self.expect_ok(reqwest::Method::POST, &format!("/user_management/users/{}/auth_factors", enc(user)), Some(body)).await?;
        let factor = v.get("authentication_factor");
        let s = |o: Option<&Value>, k: &str| o.and_then(|o| o.get(k)).and_then(Value::as_str).map(str::to_string);
        let totp = factor.and_then(|f| f.get("totp"));
        match (s(factor, "id"), s(v.get("authentication_challenge"), "id"), s(totp, "qr_code"), s(totp, "secret"), s(totp, "uri")) {
            (Some(factor), Some(challenge), Some(qr), Some(secret), Some(uri)) if qr.starts_with("data:image/") => {
                Ok(Enrolled { factor, challenge, qr_code: qr, secret: Secret(secret), uri: Secret(uri) })
            }
            _ => Err(CallError::Unavailable("enroll: an answer without a factor, a challenge or a QR code".into())),
        }
    }

    /// The factors `user` has.
    pub async fn factors(&self, user: &str) -> Result<Vec<Factor>, CallError> {
        let v = self.expect_ok(reqwest::Method::GET, &format!("/user_management/users/{}/auth_factors?limit=100", enc(user)), None).await?;
        Ok(v.get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|f| {
                Some(Factor {
                    id: f.get("id")?.as_str()?.to_string(),
                    kind: f.get("type").and_then(Value::as_str).unwrap_or("unknown").to_string(),
                    created_at: f.get("created_at").and_then(Value::as_str).map(str::to_string),
                })
            })
            .collect())
    }

    pub async fn delete_factor(&self, factor: &str) -> Result<(), CallError> {
        match self.expect_ok(reqwest::Method::DELETE, &format!("/auth/factors/{}", enc(factor)), None).await {
            Ok(_) | Err(CallError::Refused(404)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Ends a WorkOS session (`sid`).
    pub async fn revoke_session(&self, session: &str) -> Result<(), CallError> {
        match self.expect_ok(reqwest::Method::POST, "/user_management/sessions/revoke", Some(json!({ "session_id": session }))).await {
            Ok(_) | Err(CallError::Refused(404)) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// An SSO connection type (SAML or OIDC), not a social OAuth one or a magic
/// link.
fn is_sso_type(t: &str) -> bool {
    t.ends_with("SAML") || t.ends_with("Oidc") || t.ends_with("OIDC")
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// `ip_address` and `user_agent`, when known.
fn radar(body: &mut Value, client: &Client) {
    let obj = body.as_object_mut().expect("an object");
    if let Some(ip) = client.ip {
        obj.insert("ip_address".into(), Value::String(ip.to_string()));
    }
    if let Some(ua) = client.user_agent.as_deref().filter(|u| !u.is_empty()) {
        obj.insert("user_agent".into(), Value::String(ua.chars().take(512).collect()));
    }
}

/// A transport failure in words with no URL query and no body.
fn transport_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "timed out".into()
    } else if e.is_connect() {
        "could not connect".into()
    } else if e.is_decode() || e.is_body() {
        "an unreadable answer".into()
    } else {
        "a transport error".into()
    }
}

fn authenticated(v: &Value) -> Option<Authenticated> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    Some(Authenticated {
        user: User::from(v.get("user")?)?,
        access_token: Secret(s("access_token")?),
        refresh_token: s("refresh_token").map(Secret),
        organization_id: s("organization_id"),
        method: s("authentication_method"),
    })
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

/// A refused authenticate answer, read by its documented `code` / `error`.
fn auth_error(status: StatusCode, v: &Value, retry_after: Option<u64>) -> AuthError {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let pending = s("pending_authentication_token").map(Secret);
    let user = v.get("user").and_then(User::from);
    let code = s("code").or_else(|| s("error")).unwrap_or_default();
    match (code.as_str(), pending) {
        ("mfa_challenge", Some(pending)) => AuthError::MfaChallenge {
            pending,
            factors: v
                .get("authentication_factors")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|f| f.get("type").and_then(Value::as_str) == Some("totp"))
                .filter_map(|f| f.get("id").and_then(Value::as_str).map(str::to_string))
                .collect(),
            user,
        },
        ("mfa_enrollment", Some(pending)) => AuthError::MfaEnrollment { pending, user },
        ("email_verification_required", Some(pending)) => AuthError::EmailVerification { pending },
        ("organization_selection_required", Some(pending)) => AuthError::OrganizationSelection {
            pending,
            organizations: v
                .get("organizations")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|o| {
                    Some(Organization {
                        id: o.get("id")?.as_str()?.to_string(),
                        name: o.get("name").and_then(Value::as_str).unwrap_or("an organization").to_string(),
                    })
                })
                .collect(),
        },
        ("radar_email_challenge", Some(pending)) => match s("radar_challenge_id") {
            Some(challenge) => AuthError::RadarEmail { pending, challenge },
            None => AuthError::Unavailable("radar_email_challenge without a challenge id".into()),
        },
        ("radar_sms_challenge", _) => AuthError::RadarSms,
        ("sso_required", _) => AuthError::SsoRequired { connections: strings(v.get("connection_ids")) },
        ("organization_authentication_methods_required", _) => AuthError::MethodsRequired {
            connections: strings(v.get("sso_connection_ids")),
            methods: v
                .get("auth_methods")
                .and_then(Value::as_object)
                .map(|m| m.iter().filter(|(_, on)| on.as_bool() == Some(true)).map(|(k, _)| k.clone()).collect())
                .unwrap_or_default(),
        },
        ("invalid_client" | "unauthorized_client" | "authentication_method_not_allowed", _) => AuthError::Misconfigured(format!("authenticate: {code}")),
        _ => match status.as_u16() {
            429 => AuthError::RateLimited { retry_after },
            401 | 403 => AuthError::Misconfigured(format!("authenticate: HTTP {}", status.as_u16())),
            400 | 404 | 409 | 422 => AuthError::Refused,
            s => AuthError::Unavailable(format!("authenticate: HTTP {s}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_never_prints() {
        let k = ApiKey::new("sk_test_abcdefghijklmnop\n").unwrap();
        assert_eq!(format!("{k:?}"), "ApiKey(<secret>)");
        assert!(ApiKey::new("not-a-key").is_err());
        assert!(ApiKey::new("sk_short").is_err());
        assert_eq!(format!("{:?}", Secret("tok".into())), "<secret>");
    }

    #[test]
    fn every_documented_refusal_has_its_variant() {
        let e = |status: u16, v: Value| auth_error(StatusCode::from_u16(status).unwrap(), &v, None).kind();
        let p = "pending_authentication_token";
        assert_eq!(e(400, json!({ "code": "mfa_challenge", p: "x", "authentication_factors": [{ "id": "auth_factor_1", "type": "totp" }] })), "mfa_challenge");
        assert_eq!(e(400, json!({ "code": "mfa_enrollment", p: "x" })), "mfa_enrollment");
        assert_eq!(e(400, json!({ "code": "email_verification_required", p: "x" })), "email_verification_required");
        assert_eq!(e(400, json!({ "code": "organization_selection_required", p: "x", "organizations": [] })), "organization_selection_required");
        assert_eq!(e(400, json!({ "code": "radar_email_challenge", p: "x", "radar_challenge_id": "r" })), "radar_email_challenge");
        assert_eq!(e(400, json!({ "code": "radar_sms_challenge", p: "x" })), "radar_sms_challenge");
        assert_eq!(e(400, json!({ "error": "sso_required", "connection_ids": ["conn_1"] })), "sso_required");
        assert_eq!(e(400, json!({ "error": "organization_authentication_methods_required", "auth_methods": { "google_oauth": true } })), "organization_authentication_methods_required");
        assert_eq!(e(400, json!({ "error": "invalid_grant" })), "refused");
        assert_eq!(e(404, json!({ "code": "entity_not_found" })), "refused", "an unknown address is a wrong code");
        assert_eq!(e(400, json!({ "error": "invalid_client" })), "misconfigured");
        assert_eq!(e(400, json!({ "error": "authentication_method_not_allowed", "error_description": "Magic Auth is disabled." })), "misconfigured");
        assert_eq!(e(401, json!({})), "misconfigured");
        assert_eq!(e(429, json!({})), "rate_limited");
        assert_eq!(e(503, json!({})), "unavailable");
        // A pending-token refusal without its token is not trusted as one.
        assert_eq!(e(400, json!({ "code": "mfa_challenge" })), "refused");
    }

    #[test]
    fn only_saml_and_oidc_connections_are_sso() {
        for t in ["OktaSAML", "GenericSAML", "GenericOIDC", "AdpOidc", "LoginGovOidc"] {
            assert!(is_sso_type(t), "{t}");
        }
        for t in ["GoogleOAuth", "MagicLink", "Pending"] {
            assert!(!is_sso_type(t), "{t}");
        }
    }

    #[test]
    fn the_authorization_url_carries_pkce_and_one_selector() {
        let api = WorkOsApi::new(Some("https://api.example"), "client_x", ApiKey::new("sk_test_0123456789abcdef").unwrap()).unwrap();
        let u = url::Url::parse(&api.authorization_url("https://app/login/callback", "st", "ch", ("provider", "GoogleOAuth"), Some("a@b"))).unwrap();
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(u.path(), "/user_management/authorize");
        assert_eq!((q["client_id"].as_str(), q["state"].as_str(), q["code_challenge"].as_str(), q["code_challenge_method"].as_str()), ("client_x", "st", "ch", "S256"));
        assert_eq!((q["provider"].as_str(), q["login_hint"].as_str(), q["redirect_uri"].as_str()), ("GoogleOAuth", "a@b", "https://app/login/callback"));
        assert!(!u.as_str().contains("sk_test"), "the key never rides a URL");
    }
}
