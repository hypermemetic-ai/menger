//! AUTH-22: an in-process [`IdentityBackend`] with every capability but
//! SSO — for tests and local development, never for people.
//!
//! - **Email codes**: six digits, ten minutes, five tries; "sent" to an
//!   outbox — [`Memory::last_code`] in process, and, when configured, a
//!   file (mode 0600) a developer reads.
//! - **Social**: Google, GitHub, Apple and Microsoft are simulated: the
//!   "provider" approves at once, sending the browser straight back to the
//!   callback with a code bound to the PKCE challenge and redirect URI. The
//!   account is the `login_hint`'s address, or `<provider>-person@example.test`.
//! - **TOTP**: real RFC 6238 codes from a random secret; enrolled,
//!   confirmed, challenged at sign-in, removed with a code. With
//!   `require_mfa`, a user with no factor is made to enrol at sign-in.
//!
//! Users live in memory (a restart forgets them). Principals are
//! `oidc:memory/<subject>`.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;
use base64::Engine as _;
use plexus_auth_core::identity::{Authenticated, Principal};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::crypto::{base32, constant_eq, totp_matches};
use crate::identity::{
    Capabilities, Factor, Federated, IdResult, IdentityBackend, IdentityError, RequestInfo, Secret, SignedIn, Social, Ticket, TokenSet, TotpEnrolment,
    UserInfo,
};
use crate::sessions::{now, random_token};

const CODE_SECS: i64 = 600;
const CODE_TRIES: u32 = 5;
const PENDING_SECS: i64 = 900;

/// The memory backend's options (`memory[:outbox=<file>][,mfa=required]`).
#[derive(Debug, Clone, Default)]
pub struct MemoryOptions {
    /// Where "sent" codes are appended, for a developer to read.
    pub outbox: Option<PathBuf>,
    /// A user with no factor enrols one at sign-in.
    pub require_mfa: bool,
}

struct User {
    email: String,
    factors: Vec<FactorRec>,
}

struct FactorRec {
    id: String,
    secret: Vec<u8>,
    confirmed: bool,
    created: i64,
}

struct Code {
    code: String,
    expires: i64,
    tries: u32,
}

enum Pending {
    Mfa { subject: String, method: String },
    Enroll { subject: String, factor: String, method: String },
}

struct FedCode {
    email: String,
    method: String,
    challenge: String,
    redirect_uri: String,
    expires: i64,
}

#[derive(Default)]
struct State {
    users: HashMap<String, User>,
    by_email: HashMap<String, String>,
    codes: HashMap<String, Code>,
    outbox: Vec<(String, String)>,
    pending: HashMap<String, (Pending, i64)>,
    federated: HashMap<String, FedCode>,
}

/// The memory backend. See the module docs.
pub struct Memory {
    opts: MemoryOptions,
    state: Mutex<State>,
}

impl Memory {
    pub fn new(opts: MemoryOptions) -> Self {
        Self { opts, state: Mutex::new(State::default()) }
    }

    /// The last code "sent" to `email` (tests, development).
    pub fn last_code(&self, email: &str) -> Option<String> {
        let e = email.trim().to_ascii_lowercase();
        self.state.lock().unwrap().outbox.iter().rev().find(|(to, _)| *to == e).map(|(_, c)| c.clone())
    }

    /// How many codes were "sent" in all.
    pub fn sent(&self) -> usize {
        self.state.lock().unwrap().outbox.len()
    }

    fn user_for(st: &mut State, email: &str) -> String {
        let e = email.trim().to_ascii_lowercase();
        if let Some(s) = st.by_email.get(&e) {
            return s.clone();
        }
        let subject = format!("mem_{}", &hex(&Sha256::digest(random_token().as_bytes()))[..16]);
        st.users.insert(subject.clone(), User { email: e.clone(), factors: Vec::new() });
        st.by_email.insert(e, subject.clone());
        subject
    }

    /// A user has signed in by `method`: a second factor first, if they
    /// have one (or must).
    fn after_first_factor(&self, st: &mut State, subject: &str, method: &str, issuer: &str) -> IdResult<SignedIn> {
        let t = now();
        st.pending.retain(|_, (_, exp)| *exp > t);
        let user = st.users.get(subject).expect("a user");
        if user.factors.iter().any(|f| f.confirmed) {
            let id = random_token();
            st.pending.insert(id.clone(), (Pending::Mfa { subject: subject.into(), method: method.into() }, t + PENDING_SECS));
            return Err(IdentityError::MfaRequired { ticket: Ticket(Secret(id)), subject: Some(subject.into()) });
        }
        if self.opts.require_mfa {
            let label = user.email.clone();
            let enrolment = Box::new(Self::enroll(st, subject, issuer, &label));
            let id = random_token();
            st.pending.insert(id.clone(), (Pending::Enroll { subject: subject.into(), factor: enrolment.factor.clone(), method: method.into() }, t + PENDING_SECS));
            return Err(IdentityError::MfaEnrollmentRequired { ticket: Ticket(Secret(id)), subject: Some(subject.into()), enrolment });
        }
        Ok(Self::signed(st, subject, method))
    }

    fn signed(st: &State, subject: &str, method: &str) -> SignedIn {
        let user = st.users.get(subject).expect("a user");
        let principal = Principal::oidc("memory", subject).expect("a principal");
        let meta = json!({ "name": user.email, "username": user.email, "email": user.email, "provider": "memory", "auth_method": method });
        let ctx = Authenticated::new(principal, format!("memsession_{}", &random_token()[..12]), vec![], meta).into_auth_context();
        SignedIn { ctx, user: UserInfo { subject: subject.into(), email: Some(user.email.clone()), name: Some(user.email.clone()) }, tokens: None, method: method.into() }
    }

    fn enroll(st: &mut State, subject: &str, issuer: &str, label: &str) -> TotpEnrolment {
        let mut secret = [0u8; 20];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut secret);
        let id = format!("factor_{}", &random_token()[..16]);
        let b32 = base32(&secret);
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let uri = format!("otpauth://totp/{}:{}?secret={b32}&issuer={}", enc(issuer), enc(label), enc(issuer));
        if let Some(u) = st.users.get_mut(subject) {
            u.factors.push(FactorRec { id: id.clone(), secret: secret.to_vec(), confirmed: false, created: now() });
        }
        TotpEnrolment { factor: id.clone(), confirm: Ticket(Secret(id)), secret: Secret(b32), uri: Secret(uri), qr: None }
    }

    fn deliver(&self, email: &str, code: &str) {
        let Some(path) = &self.opts.outbox else { return };
        let mut o = std::fs::OpenOptions::new();
        o.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        match o.open(path) {
            Ok(mut f) => {
                let _ = writeln!(f, "to: {email}\ncode: {code}\n");
            }
            Err(e) => tracing::warn!(path = %path.display(), error = %e, "the memory backend's outbox could not be written"),
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The issuer the memory backend's sign-in enrolments name.
const ISSUER: &str = "plexus";

#[async_trait]
impl IdentityBackend for Memory {
    fn name(&self) -> &str {
        "memory"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { email_code: true, password: false, social: Social::ALL.to_vec(), sso_by_domain: false, totp: true, refresh: false }
    }

    async fn send_email_code(&self, email: &str, _req: &RequestInfo) -> IdResult<()> {
        let e = email.trim().to_ascii_lowercase();
        let n: u32 = rand::Rng::gen_range(&mut rand::rngs::OsRng, 0..1_000_000);
        let code = format!("{n:06}");
        {
            let mut st = self.state.lock().unwrap();
            st.codes.insert(e.clone(), Code { code: code.clone(), expires: now() + CODE_SECS, tries: 0 });
            st.outbox.push((e.clone(), code.clone()));
            if st.outbox.len() > 1000 {
                st.outbox.remove(0);
            }
        }
        self.deliver(&e, &code);
        Ok(())
    }

    async fn verify_email_code(&self, email: &str, code: &str, _req: &RequestInfo) -> IdResult<SignedIn> {
        let e = email.trim().to_ascii_lowercase();
        let mut st = self.state.lock().unwrap();
        let ok = match st.codes.get_mut(&e) {
            Some(c) if c.expires > now() && c.tries < CODE_TRIES => {
                c.tries += 1;
                constant_eq(c.code.as_bytes(), code.as_bytes())
            }
            _ => false,
        };
        if !ok {
            return Err(IdentityError::InvalidCode);
        }
        st.codes.remove(&e);
        let subject = Self::user_for(&mut st, &e);
        self.after_first_factor(&mut st, &subject, "email_code", ISSUER)
    }

    fn federated_url(&self, target: &Federated, redirect_uri: &str, state: &str, challenge: &str, login_hint: Option<&str>) -> IdResult<String> {
        let Federated::Social(s) = target else { return Err(IdentityError::Unsupported("SSO")) };
        let code = random_token();
        let email = login_hint.map(str::to_string).unwrap_or_else(|| format!("{}-person@example.test", s.name()));
        let mut st = self.state.lock().unwrap();
        let t = now();
        st.federated.retain(|_, f| f.expires > t);
        st.federated.insert(code.clone(), FedCode { email, method: s.name().into(), challenge: challenge.into(), redirect_uri: redirect_uri.into(), expires: t + 300 });
        let mut u = url::Url::parse(redirect_uri).map_err(|_| IdentityError::Misconfigured("the redirect URI is not a URL".into()))?;
        u.query_pairs_mut().append_pair("code", &code).append_pair("state", state);
        Ok(u.to_string())
    }

    async fn complete_federated(&self, code: &str, verifier: &str, redirect_uri: &str, _req: &RequestInfo) -> IdResult<SignedIn> {
        let mut st = self.state.lock().unwrap();
        let Some(f) = st.federated.remove(code) else { return Err(IdentityError::InvalidCode) };
        let got = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        if f.expires <= now() || !constant_eq(got.as_bytes(), f.challenge.as_bytes()) || f.redirect_uri != redirect_uri {
            return Err(IdentityError::InvalidCode);
        }
        let subject = Self::user_for(&mut st, &f.email);
        self.after_first_factor(&mut st, &subject, &f.method, ISSUER)
    }

    async fn continue_with_code(&self, ticket: &Ticket, code: &str, _req: &RequestInfo) -> IdResult<SignedIn> {
        let mut st = self.state.lock().unwrap();
        let t = now();
        let Some((pending, exp)) = st.pending.get(ticket.0.expose()) else { return Err(IdentityError::InvalidCode) };
        if *exp <= t {
            return Err(IdentityError::InvalidCode);
        }
        let (subject, method, factor) = match pending {
            Pending::Mfa { subject, method } => (subject.clone(), method.clone(), None),
            Pending::Enroll { subject, factor, method } => (subject.clone(), method.clone(), Some(factor.clone())),
        };
        let user = st.users.get_mut(&subject).ok_or(IdentityError::InvalidCode)?;
        let ok = match &factor {
            None => user.factors.iter().any(|f| f.confirmed && totp_matches(&f.secret, code, t)),
            Some(id) => match user.factors.iter_mut().find(|f| &f.id == id) {
                Some(f) if totp_matches(&f.secret, code, t) => {
                    f.confirmed = true;
                    true
                }
                _ => false,
            },
        };
        if !ok {
            return Err(IdentityError::InvalidCode);
        }
        st.pending.remove(ticket.0.expose());
        Ok(Self::signed(&st, &subject, &method))
    }

    async fn totp_enroll(&self, subject: &str, issuer: &str, label: &str) -> IdResult<TotpEnrolment> {
        let mut st = self.state.lock().unwrap();
        if !st.users.contains_key(subject) {
            return Err(IdentityError::InvalidCode);
        }
        Ok(Self::enroll(&mut st, subject, issuer, label))
    }

    async fn totp_confirm(&self, confirm: &Ticket, code: &str) -> IdResult<bool> {
        let mut st = self.state.lock().unwrap();
        let t = now();
        for u in st.users.values_mut() {
            if let Some(f) = u.factors.iter_mut().find(|f| f.id == confirm.0.expose() && !f.confirmed) {
                if totp_matches(&f.secret, code, t) {
                    f.confirmed = true;
                    return Ok(true);
                }
                return Ok(false);
            }
        }
        Ok(false)
    }

    async fn totp_factors(&self, subject: &str) -> IdResult<Vec<Factor>> {
        let st = self.state.lock().unwrap();
        Ok(st
            .users
            .get(subject)
            .map(|u| u.factors.iter().filter(|f| f.confirmed).map(|f| Factor { id: f.id.clone(), kind: "totp".into(), created_at: Some(f.created.to_string()) }).collect())
            .unwrap_or_default())
    }

    async fn totp_remove(&self, subject: &str, factor: &str, code: &str) -> IdResult<bool> {
        let mut st = self.state.lock().unwrap();
        let t = now();
        let Some(u) = st.users.get_mut(subject) else { return Ok(false) };
        let Some(i) = u.factors.iter().position(|f| f.id == factor && f.confirmed) else { return Ok(false) };
        if !totp_matches(&u.factors[i].secret, code, t) {
            return Ok(false);
        }
        u.factors.remove(i);
        Ok(true)
    }

    async fn totp_discard(&self, factor: &str) -> IdResult<()> {
        let mut st = self.state.lock().unwrap();
        for u in st.users.values_mut() {
            u.factors.retain(|f| f.id != factor || f.confirmed);
        }
        Ok(())
    }

    async fn sign_out(&self, _tokens: &TokenSet) -> IdResult<()> {
        Ok(())
    }
}
