//! AUTH-22: WorkOS as an [`IdentityBackend`] — its User Management API
//! behind our own page, no AuthKit screen.
//!
//! | neutral operation | WorkOS |
//! | --- | --- |
//! | `send_email_code` / `verify_email_code` | `POST /user_management/magic_auth`; `authenticate`, grant `urn:workos:oauth:grant-type:magic-auth:code` |
//! | `sso_for_domain` | `GET /organizations?domains=…` (a **verified** domain only) → `GET /connections?organization_id=…` (an **active** SAML/OIDC connection) |
//! | `federated_url` | `GET /user_management/authorize` with `provider=GoogleOAuth\|GitHubOAuth\|AppleOAuth\|MicrosoftOAuth`, `organization_id` or `connection_id`, `state`, PKCE S256 |
//! | `complete_federated` | `authenticate`, grant `authorization_code` (client secret **and** PKCE verifier) |
//! | `continue_with_code` | `authenticate`, grant `mfa-totp`, `email-verification:code` or `radar-email-challenge:code`, by the ticket |
//! | `choose` | `authenticate`, grant `organization-selection` |
//! | `totp_enroll` / `totp_confirm` | `POST /user_management/users/{id}/auth_factors`; `POST /auth/challenges/{id}/verify` |
//! | `totp_factors` / `totp_remove` / `totp_discard` | `GET /user_management/users/{id}/auth_factors`; `POST /auth/factors/{id}/challenge` + verify; `DELETE /auth/factors/{id}` |
//! | `refresh` | `authenticate`, grant `refresh_token` |
//! | `sign_out` | `POST /user_management/sessions/revoke` |
//!
//! Every authenticate call carries the client id, the API key as
//! `client_secret`, and the person's IP and user agent. Every documented
//! refusal (`mfa_challenge`, `mfa_enrollment`, `email_verification_required`,
//! `organization_selection_required`, `sso_required`,
//! `organization_authentication_methods_required`, `radar_email_challenge`,
//! `radar_sms_challenge`, `invalid_grant`, `entity_not_found`,
//! `invalid_client`, 429, 5xx) maps to one [`IdentityError`]. The access
//! token WorkOS answers with is verified against WorkOS's JWKS by the same
//! OIDC adapter the gate uses before anyone is signed in.

pub mod api;

use std::sync::Arc;

use async_trait::async_trait;
use plexus_auth_core::identity::{Authenticator, Presentation, Principal};
use serde_json::{json, Value};

use self::api::{AuthError, Authenticated, CallError, Grant, WorkOsApi};
pub use self::api::ApiKey;
use crate::identity::{
    Capabilities, Choice, Factor, Federated, IdResult, IdentityBackend, IdentityError, Method, RequestInfo, Secret, SignedIn, Social, Ticket,
    TokenSet, TotpEnrolment, UserInfo,
};
use crate::login::Provider;
use crate::sessions::now;

/// What the WorkOS backend is configured with (`workos:<client>,…`).
#[derive(Debug, Clone)]
pub struct WorkOsConfig {
    /// The environment's client (`client_…`).
    pub client: String,
    /// The API base, when not `https://api.workos.com`.
    pub base: Option<String>,
    pub key: ApiKey,
    /// The social providers enabled in the WorkOS environment.
    pub social: Vec<Social>,
    /// Whether an email's domain may route to its company's SSO.
    pub sso: bool,
    /// Whether Magic Auth (an emailed code) is enabled in the environment.
    pub email_code: bool,
    /// The TOTP issuer authenticator apps show (the app's name).
    pub totp_issuer: String,
}

/// The WorkOS backend. See the module docs.
pub struct WorkOs {
    api: WorkOsApi,
    verifier: Arc<dyn Authenticator>,
    provider: Provider,
    social: Vec<Social>,
    sso: bool,
    email_code: bool,
    issuer: String,
}

impl WorkOs {
    pub fn new(cfg: WorkOsConfig) -> anyhow::Result<Self> {
        let spec = match &cfg.base {
            Some(b) => format!("workos={},{b}", cfg.client),
            None => format!("workos={}", cfg.client),
        };
        let provider: Provider = spec.parse().map_err(anyhow::Error::msg)?;
        let verifier = provider.adapter()?;
        Ok(Self { api: WorkOsApi::new(cfg.base.as_deref(), &cfg.client, cfg.key)?, verifier, provider, social: cfg.social, sso: cfg.sso, email_code: cfg.email_code, issuer: cfg.totp_issuer })
    }

    /// The API, for an operator's tools (registering a redirect URI is done
    /// outside the server).
    pub fn api(&self) -> &WorkOsApi {
        &self.api
    }

    /// WorkOS signed someone in: its access token verified against its
    /// JWKS, the answer's user its own subject.
    async fn signed_in(&self, auth: Authenticated, method: &str) -> IdResult<SignedIn> {
        let verified = self.verifier.authenticate(&Presentation::Bearer { token: auth.access_token.expose().to_string() }).await;
        let mut ctx = match verified {
            Ok(a) => a.into_auth_context(),
            Err(e) => {
                tracing::warn!(rejection = %e, "WorkOS answered with an access token that did not verify");
                return Err(IdentityError::Unavailable("an access token that did not verify".into()));
            }
        };
        let ours = Principal::oidc("workos", &auth.user.id).ok();
        if ours.is_none() || ctx.principal() != ours {
            tracing::warn!("WorkOS's answer named a user who is not its token's subject: refused");
            return Err(IdentityError::Unavailable("a user who is not the token's subject".into()));
        }
        crate::web_login::named_by(&mut ctx, "workos", &json!({ "user": auth.user.json() }));
        let expires_at = ctx.metadata.get("expires_at").and_then(Value::as_i64).unwrap_or_else(|| now() + 300);
        let session = Some(ctx.session_id.clone()).filter(|s| s.starts_with("session_"));
        let name = ctx.get_metadata_string("name");
        let method = auth.method.as_deref().map(str::to_ascii_lowercase).unwrap_or_else(|| method.to_string());
        Ok(SignedIn {
            user: UserInfo { subject: auth.user.id.clone(), email: auth.user.email.clone(), name },
            tokens: Some(TokenSet { access: auth.access_token, refresh: auth.refresh_token, session, expires_at }),
            ctx,
            method,
        })
    }

    /// An authenticate answer, either way.
    async fn answer(&self, r: Result<Authenticated, AuthError>, method: &str) -> IdResult<SignedIn> {
        match r {
            Ok(auth) => self.signed_in(auth, method).await,
            Err(e) => Err(self.refusal(e).await),
        }
    }

    /// A WorkOS refusal as a neutral one; the steps WorkOS asks for are
    /// started here (a factor challenged, one enrolled), so the page only
    /// asks for a code.
    async fn refusal(&self, e: AuthError) -> IdentityError {
        match e {
            AuthError::MfaChallenge { pending, factors, user } => {
                let Some(factor) = factors.first() else {
                    return IdentityError::Unsupported("this account's second factor");
                };
                match self.api.challenge(factor).await {
                    Ok(challenge) => IdentityError::MfaRequired { ticket: ticket("totp", &pending, Some(&challenge)), subject: user.map(|u| u.id) },
                    Err(e) => call(e),
                }
            }
            AuthError::MfaEnrollment { pending, user } => {
                let Some(user) = user else { return IdentityError::Unavailable("mfa_enrollment without a user".into()) };
                let label = user.email.clone().unwrap_or_else(|| user.id.clone());
                match self.api.enroll_totp(&user.id, &self.issuer, &label).await {
                    Ok(en) => IdentityError::MfaEnrollmentRequired {
                        ticket: ticket("totp", &pending, Some(&en.challenge)),
                        subject: Some(user.id),
                        enrolment: Box::new(TotpEnrolment { confirm: ticket("confirm", &Secret(String::new()), Some(&en.challenge)), factor: en.factor, secret: en.secret, uri: en.uri, qr: Some(en.qr_code) }),
                    },
                    Err(e) => call(e),
                }
            }
            AuthError::EmailVerification { pending } => IdentityError::EmailUnverified { ticket: ticket("verify", &pending, None) },
            AuthError::RadarEmail { pending, challenge } => IdentityError::ExtraCheck { ticket: ticket("radar", &pending, Some(&challenge)) },
            AuthError::RadarSms => IdentityError::Unsupported("a text-message check"),
            AuthError::OrganizationSelection { pending, organizations } => IdentityError::Choose {
                ticket: ticket("org", &pending, None),
                choices: organizations.into_iter().map(|o| Choice { id: o.id, name: o.name }).collect(),
            },
            AuthError::SsoRequired { connections } | AuthError::MethodsRequired { connections, .. } if !connections.is_empty() => {
                IdentityError::FederatedRequired { target: Federated::Connection(connections[0].clone()) }
            }
            AuthError::SsoRequired { .. } => IdentityError::MethodsRequired { methods: vec![Method::Sso] },
            AuthError::MethodsRequired { methods, .. } => IdentityError::MethodsRequired {
                methods: methods
                    .iter()
                    .filter_map(|m| match m.as_str() {
                        "google_oauth" => Some(Method::Social(Social::Google)),
                        "github_oauth" => Some(Method::Social(Social::GitHub)),
                        "apple_oauth" => Some(Method::Social(Social::Apple)),
                        "microsoft_oauth" => Some(Method::Social(Social::Microsoft)),
                        "magic_auth" => Some(Method::EmailCode),
                        "password" => Some(Method::Password),
                        _ => None,
                    })
                    .collect(),
            },
            AuthError::Refused => IdentityError::InvalidCode,
            AuthError::RateLimited { retry_after } => IdentityError::RateLimited { retry_after },
            AuthError::Misconfigured(m) => IdentityError::Misconfigured(m),
            AuthError::Unavailable(m) => IdentityError::Unavailable(m),
        }
    }
}

/// A non-authenticate call's failure, neutral.
fn call(e: CallError) -> IdentityError {
    match e {
        CallError::RateLimited => IdentityError::RateLimited { retry_after: None },
        CallError::Misconfigured(m) => IdentityError::Misconfigured(m),
        CallError::Unavailable(m) => IdentityError::Unavailable(m),
        CallError::Refused(s) => IdentityError::Unavailable(format!("WorkOS refused the call (HTTP {s})")),
    }
}

/// A ticket: the step's kind, WorkOS's pending authentication token, and
/// the challenge it goes with.
fn ticket(kind: &str, pending: &Secret, challenge: Option<&str>) -> Ticket {
    Ticket(Secret(json!({ "k": kind, "p": pending.expose(), "c": challenge }).to_string()))
}

fn read(t: &Ticket) -> Option<(String, Secret, Option<String>)> {
    let v: Value = serde_json::from_str(t.0.expose()).ok()?;
    Some((v.get("k")?.as_str()?.to_string(), Secret(v.get("p")?.as_str()?.to_string()), v.get("c").and_then(Value::as_str).map(str::to_string)))
}

fn provider_name(s: Social) -> &'static str {
    match s {
        Social::Google => "GoogleOAuth",
        Social::GitHub => "GitHubOAuth",
        Social::Apple => "AppleOAuth",
        Social::Microsoft => "MicrosoftOAuth",
    }
}

#[async_trait]
impl IdentityBackend for WorkOs {
    fn name(&self) -> &str {
        "workos"
    }

    fn capabilities(&self) -> Capabilities {
        // SSO by domain rides the email field: without Magic Auth the field
        // stays for SSO domains alone.
        Capabilities { email_code: self.email_code || self.sso, password: false, social: self.social.clone(), sso_by_domain: self.sso, totp: true, refresh: true }
    }

    fn gate_provider(&self) -> Option<Provider> {
        Some(self.provider.clone())
    }

    fn sends_codes(&self) -> bool {
        self.email_code
    }

    async fn send_email_code(&self, email: &str, req: &RequestInfo) -> IdResult<()> {
        if !self.email_code {
            return Err(IdentityError::Unsupported("email codes (Magic Auth is off)"));
        }
        match self.api.send_code(email, req).await {
            Ok(()) => Ok(()),
            // WorkOS declining an address is not the person's to know.
            Err(CallError::Refused(s)) => {
                tracing::info!(status = s, "WorkOS declined to send a sign-in code");
                Ok(())
            }
            Err(e) => Err(call(e)),
        }
    }

    async fn verify_email_code(&self, email: &str, code: &str, req: &RequestInfo) -> IdResult<SignedIn> {
        let r = self.api.authenticate(Grant::MagicAuth { email, code }, req).await;
        self.answer(r, "email_code").await
    }

    async fn sso_for_domain(&self, domain: &str) -> IdResult<Option<Federated>> {
        if !self.sso {
            return Ok(None);
        }
        self.api.sso_organization(domain).await.map(|o| o.map(Federated::Organization)).map_err(call)
    }

    fn federated_url(&self, target: &Federated, redirect_uri: &str, state: &str, challenge: &str, login_hint: Option<&str>) -> IdResult<String> {
        let selector = match target {
            Federated::Social(s) if self.social.contains(s) => ("provider", provider_name(*s).to_string()),
            Federated::Social(_) => return Err(IdentityError::Unsupported("that provider")),
            Federated::Organization(o) => ("organization_id", o.clone()),
            Federated::Connection(c) => ("connection_id", c.clone()),
        };
        Ok(self.api.authorization_url(redirect_uri, state, challenge, (selector.0, &selector.1), login_hint))
    }

    async fn complete_federated(&self, code: &str, verifier: &str, _redirect_uri: &str, req: &RequestInfo) -> IdResult<SignedIn> {
        let r = self.api.authenticate(Grant::Code { code, verifier }, req).await;
        self.answer(r, "federated").await
    }

    async fn continue_with_code(&self, t: &Ticket, code: &str, req: &RequestInfo) -> IdResult<SignedIn> {
        let Some((kind, pending, challenge)) = read(t) else { return Err(IdentityError::InvalidCode) };
        let r = match (kind.as_str(), challenge.as_deref()) {
            ("totp", Some(c)) => self.api.authenticate(Grant::Totp { pending: &pending, challenge: c, code }, req).await,
            ("verify", _) => self.api.authenticate(Grant::EmailVerification { pending: &pending, code }, req).await,
            ("radar", Some(c)) => self.api.authenticate(Grant::RadarEmail { pending: &pending, challenge: c, code }, req).await,
            _ => return Err(IdentityError::InvalidCode),
        };
        self.answer(r, &kind).await
    }

    async fn choose(&self, t: &Ticket, choice: &str, req: &RequestInfo) -> IdResult<SignedIn> {
        let Some(("org", pending, _)) = read(t).as_ref().map(|(k, p, c)| (k.as_str(), p.clone(), c.clone())) else {
            return Err(IdentityError::InvalidCode);
        };
        let r = self.api.authenticate(Grant::OrganizationSelection { pending: &pending, organization: choice }, req).await;
        self.answer(r, "organization").await
    }

    async fn totp_enroll(&self, subject: &str, issuer: &str, label: &str) -> IdResult<TotpEnrolment> {
        let en = self.api.enroll_totp(subject, issuer, label).await.map_err(call)?;
        Ok(TotpEnrolment { confirm: ticket("confirm", &Secret(String::new()), Some(&en.challenge)), factor: en.factor, secret: en.secret, uri: en.uri, qr: Some(en.qr_code) })
    }

    async fn totp_confirm(&self, t: &Ticket, code: &str) -> IdResult<bool> {
        match read(t) {
            Some((k, _, Some(challenge))) if k == "confirm" => self.api.verify(&challenge, code).await.map_err(call),
            _ => Ok(false),
        }
    }

    async fn totp_factors(&self, subject: &str) -> IdResult<Vec<Factor>> {
        Ok(self.api.factors(subject).await.map_err(call)?.into_iter().map(|f| Factor { id: f.id, kind: f.kind, created_at: f.created_at }).collect())
    }

    async fn totp_remove(&self, subject: &str, factor: &str, code: &str) -> IdResult<bool> {
        // Only a factor of this user's.
        if !self.api.factors(subject).await.map_err(call)?.iter().any(|f| f.id == factor) {
            return Ok(false);
        }
        let challenge = self.api.challenge(factor).await.map_err(call)?;
        if !self.api.verify(&challenge, code).await.map_err(call)? {
            return Ok(false);
        }
        self.api.delete_factor(factor).await.map_err(call)?;
        Ok(true)
    }

    async fn totp_discard(&self, factor: &str) -> IdResult<()> {
        self.api.delete_factor(factor).await.map_err(call)
    }

    async fn refresh(&self, tokens: &TokenSet, req: &RequestInfo) -> IdResult<SignedIn> {
        let Some(refresh) = &tokens.refresh else { return Err(IdentityError::InvalidCode) };
        let r = self.api.authenticate(Grant::Refresh { token: refresh }, req).await;
        self.answer(r, "refresh").await
    }

    async fn sign_out(&self, tokens: &TokenSet) -> IdResult<()> {
        match &tokens.session {
            Some(s) => self.api.revoke_session(s).await.map_err(call),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ticket_carries_its_step_and_never_prints() {
        let t = ticket("totp", &Secret("pat_1".into()), Some("auth_challenge_1"));
        assert_eq!(format!("{t:?}"), "Ticket(<secret>)");
        let (k, p, c) = read(&t).unwrap();
        assert_eq!((k.as_str(), p.expose(), c.as_deref()), ("totp", "pat_1", Some("auth_challenge_1")));
        assert!(read(&Ticket(Secret("garbage".into()))).is_none());
    }

    #[test]
    fn neutral_providers_name_workos_providers() {
        assert_eq!(Social::ALL.map(provider_name), ["GoogleOAuth", "GitHubOAuth", "AppleOAuth", "MicrosoftOAuth"]);
    }
}
