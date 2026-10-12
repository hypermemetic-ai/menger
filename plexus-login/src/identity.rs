//! AUTH-22: the identity backend — who signs people in behind our own
//! sign-in page, as one trait. The page ([`crate::signin`]), its routes,
//! throttles, CSRF and return rules, and the per-app session code
//! ([`crate::app_session`]) depend on this and nothing else.
//!
//! Owner, 2026-10-11: "our auth surface [must not be] too coupled to work
//! os. it should be abstracted and hotswappable." So:
//!
//! - every operation is phrased as the page needs it, provider-neutral:
//!   send an email code and verify it; a username and password; begin a
//!   federated sign-in (a social provider by a neutral name, or SSO for an
//!   email's domain) as a redirect, and complete it from the callback; the
//!   second-factor steps; refresh; sign-out;
//! - [`Capabilities`] says what a backend can do, and the page offers
//!   exactly that (an email-code-only backend draws only the email field);
//! - every answer is one of these types — no backend's JSON shape or name
//!   crosses the trait. A step the backend must remember between two page
//!   requests (a pending MFA challenge, an email verification) is an opaque
//!   [`Ticket`] the page keeps server side and hands back;
//! - the backend is chosen by configuration ([`crate::backends::from_spec`]:
//!   `workos:…`, `plexus-idp:…`, `memory`), and a running page can be
//!   swapped onto another ([`crate::signin::SignInPage::swap`]).
//!
//! Implementations: [`crate::backends::workos::WorkOs`] (WorkOS User
//! Management), [`crate::backends::plexus_idp::PlexusIdp`] (the local
//! stand-in: its password grant), [`crate::backends::memory::Memory`] (in
//! process, every capability; tests and local development).

use std::fmt;
use std::net::IpAddr;

use async_trait::async_trait;
use plexus_core::plexus::AuthContext;

use crate::login::Provider;

/// Text that must not be printed: a token, a pending step, a TOTP secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(pub String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<secret>")
    }
}

/// Who is asking: passed to every sign-in call (WorkOS Radar takes both).
#[derive(Debug, Clone, Default)]
pub struct RequestInfo {
    pub ip: Option<IpAddr>,
    pub user_agent: Option<String>,
}

/// A social provider, by a neutral name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Social {
    Google,
    GitHub,
    Apple,
    Microsoft,
}

impl Social {
    pub const ALL: [Social; 4] = [Social::Google, Social::GitHub, Social::Apple, Social::Microsoft];

    /// `google`, `github`, `apple`, `microsoft`.
    pub fn name(self) -> &'static str {
        match self {
            Social::Google => "google",
            Social::GitHub => "github",
            Social::Apple => "apple",
            Social::Microsoft => "microsoft",
        }
    }

    /// What the button says.
    pub fn label(self) -> &'static str {
        match self {
            Social::Google => "Google",
            Social::GitHub => "GitHub",
            Social::Apple => "Apple",
            Social::Microsoft => "Microsoft",
        }
    }

    pub fn parse(s: &str) -> Option<Social> {
        Social::ALL.into_iter().find(|x| x.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// What a backend can do. The page offers exactly this.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// A one-time code by email.
    pub email_code: bool,
    /// A username (or email) and a password.
    pub password: bool,
    /// Social providers, in the order the page shows them.
    pub social: Vec<Social>,
    /// An email's domain can send it to its company's SSO.
    pub sso_by_domain: bool,
    /// Authenticator-app (TOTP) second factors: challenged at sign-in,
    /// enrolled and removed from the account page.
    pub totp: bool,
    /// Its sessions refresh (tokens with a refresh token).
    pub refresh: bool,
}

/// An opaque step the backend asked the page to carry (server side) to
/// the next request — a pending MFA challenge, an email verification. Its
/// contents are the backend's; the page never reads them.
#[derive(Clone, PartialEq, Eq)]
pub struct Ticket(pub Secret);

impl fmt::Debug for Ticket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Ticket(<secret>)")
    }
}

/// Who signed in, as the backend knows them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserInfo {
    /// The backend's own id for the user (its subject).
    pub subject: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

/// A session at the backend: what refresh and sign-out need. Kept server
/// side only.
#[derive(Clone)]
pub struct TokenSet {
    pub access: Secret,
    pub refresh: Option<Secret>,
    /// The backend's session id, what sign-out ends.
    pub session: Option<String>,
    /// When `access` expires (unix seconds).
    pub expires_at: i64,
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSet").field("session", &self.session).field("expires_at", &self.expires_at).finish_non_exhaustive()
    }
}

/// A completed sign-in.
#[derive(Debug, Clone)]
pub struct SignedIn {
    /// The identity, verified by the backend (a token checked against its
    /// keys, or the backend's own record): principal, name, email.
    pub ctx: AuthContext,
    pub user: UserInfo,
    /// The backend's session, when it keeps one.
    pub tokens: Option<TokenSet>,
    /// How (`email_code`, `password`, `google`, `sso`, …), for the log.
    pub method: String,
}

/// An organization (or account) to pick between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub name: String,
}

/// Where a federated sign-in goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Federated {
    Social(Social),
    /// SSO through an organization the backend named (from
    /// [`IdentityBackend::sso_for_domain`]).
    Organization(String),
    /// SSO through one connection the backend named.
    Connection(String),
}

/// A way in an organization's policy allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    EmailCode,
    Password,
    Social(Social),
    Sso,
}

/// A TOTP factor just enrolled: shown once, confirmed with a code.
pub struct TotpEnrolment {
    pub factor: String,
    /// What [`IdentityBackend::totp_confirm`] takes.
    pub confirm: Ticket,
    pub secret: Secret,
    /// `otpauth://…`
    pub uri: Secret,
    /// A QR code as `data:image/png;base64,…`, when the backend draws one.
    pub qr: Option<String>,
}

impl fmt::Debug for TotpEnrolment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TotpEnrolment").field("factor", &self.factor).finish_non_exhaustive()
    }
}

/// A second factor a user has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Factor {
    pub id: String,
    /// `totp`, or the backend's word for another kind.
    pub kind: String,
    pub created_at: Option<String>,
}

/// Why a sign-in step did not sign anyone in — typed, neutral.
#[derive(Debug)]
pub enum IdentityError {
    /// The code (or password, or callback) was wrong, expired or used — or
    /// named nobody. One answer, so an address with no account looks like
    /// a wrong code.
    InvalidCode,
    /// The backend's own limit.
    RateLimited { retry_after: Option<u64> },
    /// A second factor is needed: the page asks for a TOTP code and calls
    /// [`IdentityBackend::continue_with_code`] with `ticket`.
    MfaRequired { ticket: Ticket, subject: Option<String> },
    /// The backend requires a second factor and the user has none: one was
    /// enrolled; show it, then `continue_with_code(ticket, code)`.
    MfaEnrollmentRequired { ticket: Ticket, subject: Option<String>, enrolment: Box<TotpEnrolment> },
    /// The email must be verified: the backend sent a code; then
    /// `continue_with_code(ticket, code)`.
    EmailUnverified { ticket: Ticket },
    /// The backend wants an extra emailed code for this sign-in (risk
    /// checks): `continue_with_code(ticket, code)`.
    ExtraCheck { ticket: Ticket },
    /// The user picks one: then [`IdentityBackend::choose`].
    Choose { ticket: Ticket, choices: Vec<Choice> },
    /// Only a federated sign-in may sign this user in.
    FederatedRequired { target: Federated },
    /// The user's organization allows only these ways in.
    MethodsRequired { methods: Vec<Method> },
    /// A step the page cannot offer with this backend.
    Unsupported(&'static str),
    /// The backend refuses this app (bad key, unknown client): the
    /// operator's problem, never the person's.
    Misconfigured(String),
    /// The backend could not be reached, or failed.
    Unavailable(String),
}

impl IdentityError {
    /// The refusal's kind, for the log: never a token, code or address.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidCode => "invalid_code",
            Self::RateLimited { .. } => "rate_limited",
            Self::MfaRequired { .. } => "mfa_required",
            Self::MfaEnrollmentRequired { .. } => "mfa_enrollment_required",
            Self::EmailUnverified { .. } => "email_unverified",
            Self::ExtraCheck { .. } => "extra_check",
            Self::Choose { .. } => "choose",
            Self::FederatedRequired { .. } => "federated_required",
            Self::MethodsRequired { .. } => "methods_required",
            Self::Unsupported(_) => "unsupported",
            Self::Misconfigured(_) => "misconfigured",
            Self::Unavailable(_) => "unavailable",
        }
    }
}

pub type IdResult<T> = Result<T, IdentityError>;

fn unsupported<T>(what: &'static str) -> IdResult<T> {
    Err(IdentityError::Unsupported(what))
}

/// Who signs people in behind the page. See the module docs. Every method
/// a backend does not support answers [`IdentityError::Unsupported`], and
/// [`IdentityBackend::capabilities`] keeps the page from asking.
#[async_trait]
pub trait IdentityBackend: Send + Sync + 'static {
    /// The backend's name (`workos`, `plexus-idp`, `memory`): the log's word,
    /// and the `provider` an identity it signs in carries.
    fn name(&self) -> &str;

    fn capabilities(&self) -> Capabilities;

    /// The provider the WebSocket gate admits this backend's access tokens
    /// through, as a bearer (an OIDC adapter over its JWKS); `None` when its
    /// sessions live only in the app's cookie.
    fn gate_provider(&self) -> Option<Provider> {
        None
    }

    /// AUTH-24 #8: a backend for development and tests only (it signs
    /// anyone in, or trusts a local issuer): refused unless the app says so
    /// ([`crate::wiring::LoginOptions::allow_development_backends`]).
    fn for_development(&self) -> bool {
        false
    }

    /// Whether the email field sends codes (else it is there for SSO
    /// domains alone). Default: whenever the backend offers email codes.
    fn sends_codes(&self) -> bool {
        true
    }

    /// Sends a one-time code to `email`. Answers alike for any address.
    async fn send_email_code(&self, _email: &str, _req: &RequestInfo) -> IdResult<()> {
        unsupported("email codes")
    }

    async fn verify_email_code(&self, _email: &str, _code: &str, _req: &RequestInfo) -> IdResult<SignedIn> {
        unsupported("email codes")
    }

    async fn password(&self, _username: &str, _password: &str, _req: &RequestInfo) -> IdResult<SignedIn> {
        unsupported("passwords")
    }

    /// Where an email's domain signs in, when it is an SSO domain.
    async fn sso_for_domain(&self, _domain: &str) -> IdResult<Option<Federated>> {
        Ok(None)
    }

    /// The URL a browser goes to for `target`, coming back to
    /// `redirect_uri` with `state`; PKCE's S256 `challenge`.
    fn federated_url(&self, _target: &Federated, _redirect_uri: &str, _state: &str, _challenge: &str, _login_hint: Option<&str>) -> IdResult<String> {
        unsupported("federated sign-in")
    }

    /// The callback's `code`, with PKCE's verifier.
    async fn complete_federated(&self, _code: &str, _verifier: &str, _redirect_uri: &str, _req: &RequestInfo) -> IdResult<SignedIn> {
        unsupported("federated sign-in")
    }

    /// The next step of a sign-in a [`Ticket`] carries, with the code the
    /// person typed (a TOTP code, an emailed verification code).
    async fn continue_with_code(&self, _ticket: &Ticket, _code: &str, _req: &RequestInfo) -> IdResult<SignedIn> {
        unsupported("multi-step sign-in")
    }

    /// The choice the person made ([`IdentityError::Choose`]).
    async fn choose(&self, _ticket: &Ticket, _choice: &str, _req: &RequestInfo) -> IdResult<SignedIn> {
        unsupported("choosing an organization")
    }

    /// A new TOTP factor for `subject`, labelled `issuer` / `label` in the
    /// authenticator app; unconfirmed until [`IdentityBackend::totp_confirm`].
    async fn totp_enroll(&self, _subject: &str, _issuer: &str, _label: &str) -> IdResult<TotpEnrolment> {
        unsupported("TOTP")
    }

    /// Whether `code` confirms an enrolment.
    async fn totp_confirm(&self, _confirm: &Ticket, _code: &str) -> IdResult<bool> {
        unsupported("TOTP")
    }

    async fn totp_factors(&self, _subject: &str) -> IdResult<Vec<Factor>> {
        unsupported("TOTP")
    }

    /// Removes `factor` (one of `subject`'s) when `code` is a current code
    /// from it; `Ok(false)` for a wrong code.
    async fn totp_remove(&self, _subject: &str, _factor: &str, _code: &str) -> IdResult<bool> {
        unsupported("TOTP")
    }

    /// Deletes a factor that was never confirmed.
    async fn totp_discard(&self, _factor: &str) -> IdResult<()> {
        unsupported("TOTP")
    }

    /// A fresh session for `tokens` (its refresh token is spent).
    async fn refresh(&self, _tokens: &TokenSet, _req: &RequestInfo) -> IdResult<SignedIn> {
        unsupported("refresh")
    }

    /// Ends the backend's session.
    async fn sign_out(&self, _tokens: &TokenSet) -> IdResult<()> {
        Ok(())
    }
}
