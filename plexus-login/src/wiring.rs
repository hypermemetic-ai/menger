//! SCAFFOLD-9: the whole login wired in two calls — what menger's `main`
//! did by hand, in one place for every app.
//!
//! ```text
//! LoginOptions ──Wiring::new──▶ Wiring ──(the app builds its rules over wiring.sessions())──▶
//!            ──wiring.finish(access, dev)──▶ Wired { sessions, login, backend, gate, protected, origins }
//! ```
//!
//! Two phases, because the app's rules usually hold the session table
//! (they ask it whether a session stands, and mint in it), and the login
//! page, the gate and the auth backend all hold the rules. [`Wiring::new`]
//! checks the providers and makes the table; [`Wiring::finish`] takes the
//! rules and makes the rest. Then:
//!
//! - [`Wired::backend`] goes into the hub's `auth` activation
//!   (`DynamicHub::with_auth(AuthSetup::new(backend, …))`);
//! - [`Wired::apply`] puts the upgrade's half on a `TransportServerBuilder`:
//!   the allowed origins, and with any provider the gate, the anonymous
//!   view, the `access_token` credential cookie and RFC 9728's protected
//!   resource metadata;
//! - [`Wired::router`] is the login page's routes, to merge beside the
//!   app's own pages (or [`Wired::login`] for a shell that mounts them
//!   itself).
//!
//! With no provider, login is off and nothing is refused: no gate, no page,
//! a backend offering nothing — the server serves as it would without the
//! crate.

use std::sync::Arc;

use plexus_auth_core::adapters::Carriers;
use plexus_core::plexus::Activation;
use plexus_transport::{ProtectedResource, TransportServerBuilder};

use crate::access::{Branding, LoginAccess};
use crate::client_ip::TrustedProxies;
use crate::identity::IdentityBackend;
use crate::throttle::Limits;
use crate::auth_backend::{BrowserLogin, LoginAuth};
use crate::dev_login::DevLogin;
use crate::login::{self, LoginButton, Provider};
use crate::names::LoginNames;
use crate::sessions::{Gate, Sessions};
use crate::web_login::{self, origin_of, Login, LoginConfig};

/// What the login is configured with — menger's login flags, as data.
/// [`LoginOptions::new`] fills the defaults; set what the app's flags say.
#[derive(Clone)]
pub struct LoginOptions {
    /// `--auth-provider`s. None: login off.
    pub providers: Vec<Provider>,
    /// `--login-button`s.
    pub buttons: Vec<LoginButton>,
    /// `--login-email <provider>[:<connection>]` (e.g. `workos:authkit`).
    pub email: Option<LoginButton>,
    /// `--login-password`: the provider the password form goes to; `none`
    /// for no form; default the first OIDC provider that takes passwords.
    pub password: Option<String>,
    /// `--login-forgot`: a "Forgot password?" link.
    pub forgot: Option<String>,
    /// The WebSocket's port: a key proof's audiences
    /// ([`login::socket_urls`]) and the protected resource's URL.
    pub ws_port: u16,
    /// The shell's port: its default public URL and its own origins.
    pub shell_port: u16,
    /// `--public-url`: where people reach the shell; default
    /// `http://127.0.0.1:<shell_port>`.
    pub public_url: Option<String>,
    /// `--allowed-origin`s: more origins whose pages may open the socket
    /// (the shell's own always may).
    pub allowed_origins: Vec<String>,
    /// How long a sign-in lasts (default 12 h).
    pub session_ttl_secs: i64,
    /// `--require-login`: the shell's pages need a session.
    pub require: bool,
    /// Whether there is a shell to serve the login page on (`false` under
    /// `--no-shell` or stdio): no page, and `auth.login {browser}` is
    /// refused.
    pub page: bool,
    /// The page's title (the instance: menger's `menger` or `menger.test`).
    pub title: &'static str,
    /// The app's name, version and mark, as the page says them.
    pub branding: Branding,
    /// The cookie names and the session metadata key; default
    /// [`LoginNames::for_app`] of the branding's name.
    pub names: LoginNames,
    /// AUTH-22: the identity backend behind our own sign-in page
    /// (`--identity`, [`crate::backends::from_spec`]). Its gate provider
    /// (an OIDC adapter for its tokens as a bearer) joins `providers`.
    pub identity: Option<Arc<dyn IdentityBackend>>,
    /// AUTH-22: `--trusted-proxy` CIDRs: whose `X-Forwarded-For` is
    /// believed for the client's IP (throttles, the backend's risk checks).
    pub trusted_proxies: TrustedProxies,
    /// AUTH-22: `--return-origin`s: more origins a sign-in may send the
    /// browser back to (the shell's own always may).
    pub return_origins: Vec<String>,
    /// AUTH-22: the sign-in's throttles.
    pub limits: Limits,
    /// AUTH-24 #8: whether a development backend (`memory`, `plexus-idp`)
    /// may sign people in here — the app decides where (menger: only
    /// `--env test`, as the dev sign-in).
    pub allow_development_backends: bool,
}

impl std::fmt::Debug for LoginOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginOptions")
            .field("providers", &self.providers)
            .field("identity", &self.identity.as_ref().map(|b| b.name().to_string()))
            .field("public_url", &self.public_url)
            .field("trusted_proxies", &self.trusted_proxies)
            .field("require", &self.require)
            .finish_non_exhaustive()
    }
}

impl LoginOptions {
    /// Login for `branding`'s app, its socket on `ws_port` and its shell on
    /// `shell_port`: no provider yet (login off), no buttons, 12-hour
    /// sessions, a page, the names [`LoginNames::for_app`] gives.
    pub fn new(branding: Branding, ws_port: u16, shell_port: u16) -> Self {
        Self {
            providers: Vec::new(),
            buttons: Vec::new(),
            email: None,
            password: None,
            forgot: None,
            ws_port,
            shell_port,
            public_url: None,
            allowed_origins: Vec::new(),
            session_ttl_secs: 12 * 3600,
            require: false,
            page: true,
            title: branding.app,
            branding,
            names: LoginNames::for_app(branding.app),
            identity: None,
            trusted_proxies: TrustedProxies::default(),
            return_origins: Vec::new(),
            limits: Limits::default(),
            allow_development_backends: false,
        }
    }

    /// The same, admitting callers through these `--auth-provider` specs.
    pub fn with_providers<I, S>(mut self, specs: I) -> anyhow::Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for s in specs {
            self.providers.push(s.as_ref().parse().map_err(anyhow::Error::msg)?);
        }
        Ok(self)
    }
}

/// The first phase: the providers checked, the session table made. See the
/// module docs.
pub struct Wiring {
    opts: LoginOptions,
    sessions: Arc<Sessions>,
    carriers: Option<Arc<Carriers>>,
    public_url: String,
    shell_origins: Vec<String>,
}

impl Wiring {
    /// Checks the providers (a bad spec, two of one name, an issuer that is
    /// not https off loopback) and `--require-login` (it needs a provider),
    /// and makes the session table, its cookies named for this shell
    /// (IDENTITY-38).
    pub fn new(mut opts: LoginOptions) -> anyhow::Result<Self> {
        // AUTH-22: the identity backend's tokens are admitted at the gate
        // through its provider-neutral OIDC adapter, as any provider's.
        if let Some(p) = opts.identity.as_ref().and_then(|b| b.gate_provider()) {
            if !opts.providers.iter().any(|q| q.name() == p.name()) {
                opts.providers.push(p);
            }
        }
        if let Some(b) = &opts.identity {
            if b.for_development() && !opts.allow_development_backends {
                anyhow::bail!(
                    "--identity {}: a development backend (it signs anyone in, or trusts a local issuer); allowed only where the app allows it (menger: --env test)",
                    b.name()
                );
            }
        }
        // AUTH-24 #2: off loopback, a proxy in front makes every visitor one
        // client. Say which it is, or do not start.
        let public = opts.public_url.clone().unwrap_or_else(|| format!("http://127.0.0.1:{}", opts.shell_port));
        if opts.identity.is_some() && !opts.trusted_proxies.configured() && !is_loopback_url(&public) {
            anyhow::bail!(
                "the sign-in page at {public} needs --trusted-proxy: the edge in front of it (its address or network, e.g. the Docker network's subnet), \
                 or `none` when people connect to it directly — without it, every visitor behind a proxy is one client to the throttles"
            );
        }
        if opts.require && opts.providers.is_empty() && opts.identity.is_none() {
            anyhow::bail!("--require-login needs at least one --auth-provider to log in with");
        }
        let public_url = opts.public_url.clone().unwrap_or_else(|| format!("http://127.0.0.1:{}", opts.shell_port));
        // IDENTITY-42: a key proof names the socket it was made for.
        let sockets = login::socket_urls(opts.ws_port, opts.public_url.as_deref());
        let carriers = login::boundary_at(&opts.providers, &sockets)?;
        let sessions = Arc::new(Sessions::for_shell(opts.session_ttl_secs, &opts.names, &public_url));
        let shell_origins = shell_origins(opts.shell_port, &public_url);
        Ok(Self { opts, sessions, carriers, public_url, shell_origins })
    }

    /// The session table the app's rules ask and mint in.
    pub fn sessions(&self) -> &Arc<Sessions> {
        &self.sessions
    }

    /// Whether login is on (any provider).
    pub fn login_on(&self) -> bool {
        !self.opts.providers.is_empty() || self.opts.identity.is_some()
    }

    /// Where people reach the shell.
    pub fn public_url(&self) -> &str {
        &self.public_url
    }

    /// The shell's own origins: `http://127.0.0.1:<port>`,
    /// `http://localhost:<port>`, and the public URL's.
    pub fn shell_origins(&self) -> &[String] {
        &self.shell_origins
    }

    /// The options it was made from.
    pub fn options(&self) -> &LoginOptions {
        &self.opts
    }

    /// The second phase, over the app's rules: the login page (with a
    /// provider and a shell), the auth backend, the gate (with a provider),
    /// the protected resource. `dev`: the test environment's dev sign-in
    /// (IDENTITY-35) — the app decides where it is allowed; it rides the
    /// page, so with no page it is unused.
    pub fn finish<A: LoginAccess>(self, access: A, dev: Option<DevLogin<A>>) -> anyhow::Result<Wired<A>> {
        let Self { opts, sessions, carriers, public_url, shell_origins } = self;
        let login_on = !opts.providers.is_empty() || opts.identity.is_some();
        let login = if login_on && opts.page {
            Some(Arc::new(Login::new(
                LoginConfig {
                    // The agents' key proof is not a way in a person is offered.
                    providers: opts.providers.iter().filter(|p| !p.is_key()).cloned().collect(),
                    buttons: opts.buttons.clone(),
                    email: opts.email.clone(),
                    password: opts.password.clone(),
                    forgot: opts.forgot.clone(),
                    public_url: public_url.clone(),
                    also: shell_origins.clone(),
                    require: opts.require,
                    title: opts.title,
                    branding: opts.branding,
                    dev,
                    access: Some(access.clone()),
                    identity: opts.identity.clone(),
                    trusted_proxies: opts.trusted_proxies.clone(),
                    return_origins: opts.return_origins.clone(),
                    limits: opts.limits,
                },
                sessions.clone(),
            )?))
        } else {
            None
        };
        // AUTH-7: the protocol's auth over the app's rules; its loopback
        // sign-in goes through the login page, when there is one.
        let backend = Arc::new(LoginAuth::new(
            access.clone(),
            LoginAuth::<A>::offered(&opts.providers),
            login.as_ref().map(|_| BrowserLogin { sessions: sessions.clone(), public_url: public_url.clone() }),
            opts.branding.app,
        ));
        // AUTH-7: the gate notices who signs in holding nothing.
        // AUTH-22: with only an identity backend (no provider of its own,
        // e.g. `memory`), the gate still reads the session cookie.
        let gate = (carriers.is_some() || login_on).then(|| Arc::new(Gate::new(sessions.clone(), carriers).noticing(access.clone())));
        // RFC 9728's metadata names the OIDC issuers a client can sign in at.
        let issuers: Vec<String> = opts.providers.iter().filter_map(|p| p.oidc().map(|(issuer, _, _)| issuer.to_string())).collect();
        let protected = ProtectedResource::new(format!("http://127.0.0.1:{}", opts.ws_port), issuers);
        // IDENTITY-29: only the shell's own pages (and those named) may open
        // the socket from a browser; a client sends no Origin.
        let mut origins = shell_origins;
        origins.extend(opts.allowed_origins.iter().cloned());
        Ok(Wired { sessions, login, backend, gate, protected, origins, public_url, providers: opts.providers, access, identity: opts.identity })
    }
}

/// The second phase's result. See the module docs.
pub struct Wired<A: LoginAccess> {
    /// The session table.
    pub sessions: Arc<Sessions>,
    /// The login page and its routes (login on, a shell to serve them).
    pub login: Option<Arc<Login<A>>>,
    /// The `auth` activation's backend.
    pub backend: Arc<LoginAuth<A>>,
    /// The upgrade's validator (login on).
    pub gate: Option<Arc<Gate>>,
    /// RFC 9728's protected resource: the socket, and the issuers.
    pub protected: ProtectedResource,
    /// The origins whose pages may open the socket.
    pub origins: Vec<String>,
    /// Where people reach the shell.
    pub public_url: String,
    /// Every provider, the agents' key proof included.
    pub providers: Vec<Provider>,
    /// The app's rules.
    pub access: A,
    /// AUTH-22: the identity backend behind our own page, when there is one.
    pub identity: Option<Arc<dyn IdentityBackend>>,
}

impl<A: LoginAccess> Wired<A> {
    /// Whether login is on (any provider).
    pub fn login_on(&self) -> bool {
        !self.providers.is_empty() || self.identity.is_some()
    }

    /// AUTH-22: puts another identity backend behind the running sign-in
    /// page ([`crate::signin::SignInPage::swap`]). The gate's providers are
    /// fixed at startup: a new backend's bearer tokens are admitted after a
    /// restart (its browser sessions at once).
    pub fn swap_identity(&self, backend: Arc<dyn IdentityBackend>) -> bool {
        match self.login.as_ref().and_then(|l| l.signin()) {
            Some(p) => {
                p.swap(backend);
                true
            }
            None => false,
        }
    }

    /// The providers a person is offered (`session.providers`, the page):
    /// every one but the agents' key proof.
    pub fn offered(&self) -> impl Iterator<Item = &Provider> {
        self.providers.iter().filter(|p| !p.is_key())
    }

    /// Where RFC 9728's metadata is served, when login is on — what the
    /// hub's 401s point a client at.
    pub fn metadata_url(&self) -> Option<String> {
        self.login_on().then(|| self.protected.metadata_url())
    }

    /// The upgrade's half on a transport builder that has its WebSocket:
    /// the allowed origins, and with login on the gate, the anonymous view
    /// (AUTH-6/AUTH-7: no credential → only the auth door; a credential
    /// presented and refused → 401 with RFC 6750's challenge), the
    /// `access_token` cookie as the one credential cookie (a stale session
    /// cookie falls into the anonymous view, never a 401 a page cannot
    /// read), and the protected resource.
    pub fn apply<H: Activation>(&self, builder: TransportServerBuilder<H>) -> TransportServerBuilder<H> {
        let builder = builder.with_allowed_origins(self.origins.clone());
        match &self.gate {
            Some(gate) => builder
                .with_session_validator(gate.clone())
                .anonymous_view()
                .credential_cookies(["access_token"])
                .with_protected_resource(self.protected.clone()),
            None => builder,
        }
    }

    /// The login page's routes ([`web_login::router`]), when there is a
    /// page: merge it beside the app's own.
    pub fn router(&self) -> Option<axum::Router> {
        self.login.clone().map(web_login::router)
    }
}

/// Whether a URL's host is this machine (127.0.0.0/8, ::1, localhost).
fn is_loopback_url(url: &str) -> bool {
    match url::Url::parse(url).ok().and_then(|u| u.host().map(|h| h.to_owned())) {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost") || d.to_ascii_lowercase().ends_with(".localhost"),
        None => false,
    }
}

/// The origins a shell on `shell_port` is reached at: `127.0.0.1`,
/// `localhost`, and `public_url`'s.
pub fn shell_origins(shell_port: u16, public_url: &str) -> Vec<String> {
    let mut origins = vec![format!("http://127.0.0.1:{shell_port}"), format!("http://localhost:{shell_port}")];
    let origin = origin_of(public_url);
    if !origins.contains(&origin) {
        origins.push(origin);
    }
    origins
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::test_access::TestAccess;

    const BRAND: Branding = Branding { app: "toy", version: "0.0.1", mark: "t", motto: "" };

    #[test]
    fn no_provider_is_login_off_and_nothing_refused() {
        let w = Wiring::new(LoginOptions::new(BRAND, 4590, 4591)).unwrap();
        assert!(!w.login_on());
        assert_eq!(w.sessions().cookie_name(), "toy_session_4591");
        let wired = w.finish(TestAccess, None).unwrap();
        assert!(wired.login.is_none() && wired.gate.is_none() && wired.router().is_none() && wired.metadata_url().is_none());
        assert_eq!(wired.origins, ["http://127.0.0.1:4591", "http://localhost:4591"]);
    }

    #[test]
    fn a_provider_wires_the_page_the_gate_and_the_metadata() {
        let opts = LoginOptions { public_url: Some("https://toy.example/".into()), allowed_origins: vec!["https://other.example".into()], ..LoginOptions::new(BRAND, 4590, 4591) }
            .with_providers(["idp=http://127.0.0.1:4592,plexus:toy", "key"])
            .unwrap();
        let wired = Wiring::new(opts).unwrap().finish(TestAccess, None).unwrap();
        assert!(wired.login.is_some() && wired.gate.is_some() && wired.router().is_some());
        assert_eq!(wired.sessions.cookie_name(), "__Host-toy_session");
        assert_eq!(wired.offered().map(Provider::name).collect::<Vec<_>>(), ["plexus-idp"]);
        assert_eq!(wired.metadata_url().as_deref(), Some("http://127.0.0.1:4590/.well-known/oauth-protected-resource"));
        assert_eq!(wired.origins, ["http://127.0.0.1:4591", "http://localhost:4591", "https://toy.example", "https://other.example"]);
    }

    #[test]
    fn require_login_needs_a_provider_and_a_bad_spec_stops_startup() {
        let opts = LoginOptions { require: true, ..LoginOptions::new(BRAND, 1, 2) };
        assert!(Wiring::new(opts).is_err());
        assert!(LoginOptions::new(BRAND, 1, 2).with_providers(["ldap=x"]).is_err());
        let off_loopback = LoginOptions::new(BRAND, 1, 2).with_providers(["idp=http://idp.example,plexus:toy"]).unwrap();
        assert!(Wiring::new(off_loopback).is_err());
    }

    #[test]
    fn a_page_off_loopback_must_name_its_proxy_and_development_backends_are_refused() {
        use crate::backends::memory::{Memory, MemoryOptions};
        let mem: Arc<dyn IdentityBackend> = Arc::new(Memory::new(MemoryOptions::default()));
        let opts = |public: &str, proxies: &[&str], dev: bool| LoginOptions {
            identity: Some(mem.clone()),
            public_url: Some(public.into()),
            trusted_proxies: TrustedProxies::parse(proxies.iter().copied()).unwrap(),
            allow_development_backends: dev,
            ..LoginOptions::new(BRAND, 1, 2)
        };
        let e = Wiring::new(opts("http://127.0.0.1:2", &[], false)).err().unwrap().to_string();
        assert!(e.contains("development backend"), "{e}");
        assert!(Wiring::new(opts("http://127.0.0.1:2", &[], true)).is_ok(), "loopback: the peer is the client");
        let e = Wiring::new(opts("https://toy.example", &[], true)).err().unwrap().to_string();
        assert!(e.contains("--trusted-proxy"), "{e}");
        assert!(Wiring::new(opts("https://toy.example", &["172.18.0.0/16"], true)).is_ok());
        assert!(Wiring::new(opts("https://toy.example", &["none"], true)).is_ok(), "direct, said so");
    }

    #[test]
    fn no_shell_is_no_page() {
        let opts = LoginOptions { page: false, ..LoginOptions::new(BRAND, 1, 2) }.with_providers(["idp=http://127.0.0.1:3,plexus:toy"]).unwrap();
        let wired = Wiring::new(opts).unwrap().finish(TestAccess, None).unwrap();
        assert!(wired.login.is_none() && wired.gate.is_some());
    }
}
