//! IDENTITY-14: login at the boundary.
//!
//! menger does not know identity providers. Each `--auth-provider` names
//! one, and becomes that provider's adapter (plexus-auth-core's
//! `adapters`, IDENTITY-13). The adapters sit in one chain at the WebSocket
//! upgrade, and whichever claims the caller's credential turns it into
//! Plexus types. Past the upgrade there is only the connection's
//! `AuthContext`: `session.whoami` reports it, `comments.add` signs with
//! it.
//!
//! | `--auth-provider` | adapter | principal |
//! | --- | --- | --- |
//! | `idp=<issuer>,<audience>` | plexus-idp | `idp:<uuid>` |
//! | `auth0=<domain>,<audience>[,<roles-claim>]` | Auth0 | `oidc:auth0/<sub>` |
//! | `oidc:<name>=<issuer>,<audience>[,<roles-claim>]` | any OIDC provider | `oidc:<name>/<sub>` |
//! | `workos=<client-id>[,<api-base>]` | WorkOS (AuthKit, SSO) | `oidc:workos/<user id>` |
//! | `nostr=<service-url>[,<service-url>…]` | NIP-98 | `nostr:<hex>` |
//!
//! No provider, no chain: menger serves as it did before.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use menger_tickets_api::{ProviderKind, ProviderOffer};
use plexus_auth_core::adapters::{Carriers, ClaimMap, Nip98Adapter, OidcAdapter};
use plexus_auth_core::identity::{Authenticator, AuthenticatorChain};

/// One provider menger admits callers through, as `--auth-provider` names
/// it. IDENTITY-19: an OIDC provider may name the `client=<id>` menger is
/// registered as, which its login page needs for the authorization code
/// flow (Google, Apple, SSO through the IdP) and sends on a password grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    /// `idp=<issuer>,<audience>[,client=<id>]`: plexus-idp.
    PlexusIdp { issuer: String, audience: String, client: Option<String> },
    /// `auth0=<domain>,<audience>[,<roles-claim>][,client=<id>]`: an Auth0
    /// tenant and the API identifier its tokens are minted for; roles from
    /// the namespaced claim an Auth0 Action adds.
    Auth0 { domain: String, audience: String, roles_claim: Option<String>, client: Option<String> },
    /// `oidc:<name>=<issuer>,<audience>[,<roles-claim>][,client=<id>]`: any
    /// other OIDC provider, named `<name>` in its principals.
    Oidc { name: String, issuer: String, audience: String, roles_claim: Option<String>, client: Option<String> },
    /// `nostr=<service-url>[,<service-url>…]`: NIP-98 events naming one of
    /// these URLs, the WebSocket URLs callers connect to.
    Nostr { urls: Vec<String> },
    /// IDENTITY-30: `workos=<client-id>[,<api-base>]`: a WorkOS
    /// environment's client. Its issuer is `<api-base>/user_management/
    /// <client-id>` (`https://api.workos.com` unless given), and the client
    /// id is the audience and the code flow's client: no secret.
    WorkOs { client: String, base: Option<String>, issuer: String },
}

impl Provider {
    /// The provider's name, as `session.whoami`'s `provider` says it.
    pub fn name(&self) -> &str {
        match self {
            Self::PlexusIdp { .. } => "plexus-idp",
            Self::Auth0 { .. } => "auth0",
            Self::Oidc { name, .. } => name,
            Self::Nostr { .. } => "nostr",
            Self::WorkOs { .. } => "workos",
        }
    }

    /// An OIDC provider's issuer, audience and client: what a login asks
    /// it for a token with. `None` for nostr.
    pub fn oidc(&self) -> Option<(&str, &str, Option<&str>)> {
        match self {
            Self::PlexusIdp { issuer, audience, client } => Some((issuer, audience, client.as_deref())),
            Self::Auth0 { domain, audience, client, .. } => Some((domain, audience, client.as_deref())),
            Self::Oidc { issuer, audience, client, .. } => Some((issuer, audience, client.as_deref())),
            Self::WorkOs { client, issuer, .. } => Some((issuer, client, Some(client))),
            Self::Nostr { .. } => None,
        }
    }

    /// Whether the login page's password form may go to it: WorkOS takes
    /// passwords on its own page, never from menger (a password grant
    /// there needs the API key, which menger does not hold).
    pub fn takes_passwords(&self) -> bool {
        !matches!(self, Self::WorkOs { .. } | Self::Nostr { .. })
    }

    /// The authorization request's parameters that say which way in a
    /// button means. Auth0 and other OIDC providers take `connection`.
    /// WorkOS (IDENTITY-30) takes `provider` — `authkit` (its hosted page:
    /// email, password, passkey, a code by email, and SSO by the email's
    /// domain), `GoogleOAuth`, `AppleOAuth`, … — or an SSO connection
    /// (`conn_…`) or organization (`org_…`); no connection is `authkit`.
    pub fn authorize_hint(&self, connection: Option<&str>) -> Vec<(&'static str, String)> {
        match (self, connection) {
            (Self::WorkOs { .. }, None) => vec![("provider", "authkit".into())],
            (Self::WorkOs { .. }, Some(c)) if c.starts_with("conn_") => vec![("connection_id", c.into())],
            (Self::WorkOs { .. }, Some(c)) if c.starts_with("org_") => vec![("organization_id", c.into())],
            (Self::WorkOs { .. }, Some(c)) => vec![("provider", c.into())],
            (_, Some(c)) => vec![("connection", c.into())],
            (_, None) => Vec::new(),
        }
    }

    /// Where a browser goes to end the provider's own session after
    /// menger's, when the provider keeps one menger can end: WorkOS's
    /// `sessions/logout`, back to `return_to` (one of the environment's
    /// logout URIs).
    pub fn sign_out_url(&self, session_id: &str, return_to: &str) -> Option<String> {
        match self {
            Self::WorkOs { base, .. } if session_id.starts_with("session_") => {
                let base = base.as_deref().unwrap_or(plexus_auth_core::adapters::WORKOS_API).trim_end_matches('/');
                let mut url = url::Url::parse(&format!("{base}/user_management/sessions/logout")).ok()?;
                url.query_pairs_mut().append_pair("session_id", session_id).append_pair("return_to", return_to);
                Some(url.to_string())
            }
            _ => None,
        }
    }

    /// What `session.providers` says of it (IDENTITY-15): its name and how
    /// to reach it, nothing secret.
    pub fn offer(&self) -> ProviderOffer {
        match self.oidc() {
            Some((issuer, audience, _)) => ProviderOffer {
                name: self.name().to_string(),
                kind: ProviderKind::Oidc,
                issuer: Some(issuer.to_string()),
                audience: Some(audience.to_string()),
            },
            None => ProviderOffer { name: self.name().to_string(), kind: ProviderKind::Nostr, issuer: None, audience: None },
        }
    }

    /// The provider's adapter.
    pub fn adapter(&self) -> anyhow::Result<Arc<dyn Authenticator>> {
        Ok(match self {
            Self::PlexusIdp { issuer, audience, .. } => Arc::new(OidcAdapter::plexus_idp(issuer, audience)?),
            Self::Auth0 { domain, audience, roles_claim, .. } => {
                Arc::new(OidcAdapter::auth0(domain, audience, roles_claim.as_deref())?)
            }
            Self::Oidc { name, issuer, audience, roles_claim, .. } => {
                let claims = ClaimMap {
                    name: vec!["preferred_username".into(), "name".into(), "email".into()],
                    roles: roles_claim.clone(),
                };
                Arc::new(OidcAdapter::generic(name, issuer, audience, claims)?)
            }
            Self::Nostr { urls } => Arc::new(Nip98Adapter::new(urls)?),
            Self::WorkOs { client, base, .. } => Arc::new(OidcAdapter::workos(client, base.as_deref())?),
        })
    }
}

impl FromStr for Provider {
    type Err = String;

    fn from_str(spec: &str) -> Result<Self, Self::Err> {
        let usage = "an --auth-provider is idp=<issuer>,<audience>[,client=<id>] · \
                     auth0=<domain>,<audience>[,<roles-claim>][,client=<id>] · \
                     oidc:<name>=<issuer>,<audience>[,<roles-claim>][,client=<id>] · \
                     workos=<client-id>[,<api-base>] · \
                     nostr=<service-url>[,<service-url>…]";
        let (kind, rest) = spec.trim().split_once('=').ok_or_else(|| format!("`{spec}`: {usage}"))?;
        let parts: Vec<String> = rest.split(',').map(|p| p.trim().to_string()).collect();
        if parts.iter().any(String::is_empty) {
            return Err(format!("`{spec}`: an empty value; {usage}"));
        }
        // issuer, audience, then at most one roles claim and one client=.
        let oidc = |roles_allowed: bool| -> Result<(String, String, Option<String>, Option<String>), String> {
            let [issuer, audience, more @ ..] = parts.as_slice() else {
                return Err(format!("`{spec}`: {usage}"));
            };
            let (mut roles, mut client) = (None, None);
            for p in more {
                match p.strip_prefix("client=") {
                    Some(id) if client.is_none() && !id.is_empty() => client = Some(id.to_string()),
                    None if roles_allowed && roles.is_none() => roles = Some(p.clone()),
                    _ => return Err(format!("`{spec}`: {usage}")),
                }
            }
            Ok((issuer.clone(), audience.clone(), roles, client))
        };
        match kind.trim() {
            "idp" => {
                let (issuer, audience, _, client) = oidc(false)?;
                Ok(Self::PlexusIdp { issuer, audience, client })
            }
            "auth0" => {
                let (domain, audience, roles_claim, client) = oidc(true)?;
                Ok(Self::Auth0 { domain, audience, roles_claim, client })
            }
            "nostr" => Ok(Self::Nostr { urls: parts }),
            "workos" => match parts.as_slice() {
                [client] | [client, _] if client.starts_with("client_") && !client.contains(['/', '?', '#']) => {
                    let base = parts.get(1).cloned();
                    if base.as_deref().is_some_and(|b| url::Url::parse(b).is_err()) {
                        return Err(format!("`{spec}`: the API base is not a URL; {usage}"));
                    }
                    let api = base.as_deref().unwrap_or(plexus_auth_core::adapters::WORKOS_API).trim_end_matches('/');
                    Ok(Self::WorkOs { issuer: format!("{api}/user_management/{client}"), client: client.clone(), base })
                }
                _ => Err(format!("`{spec}`: a WorkOS client id is client_…; {usage}")),
            },
            other => match other.strip_prefix("oidc:") {
                Some(name) => {
                    let (issuer, audience, roles_claim, client) = oidc(true)?;
                    Ok(Self::Oidc { name: name.to_string(), issuer, audience, roles_claim, client })
                }
                None => Err(format!("`{spec}`: unknown provider `{other}`; {usage}")),
            },
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let opt = |r: &Option<String>, pre: &str| r.as_ref().map(|r| format!(",{pre}{r}")).unwrap_or_default();
        match self {
            Self::PlexusIdp { issuer, audience, client } => {
                write!(f, "idp={issuer},{audience}{}", opt(client, "client="))
            }
            Self::Auth0 { domain, audience, roles_claim, client } => {
                write!(f, "auth0={domain},{audience}{}{}", opt(roles_claim, ""), opt(client, "client="))
            }
            Self::Oidc { name, issuer, audience, roles_claim, client } => {
                write!(f, "oidc:{name}={issuer},{audience}{}{}", opt(roles_claim, ""), opt(client, "client="))
            }
            Self::Nostr { urls } => write!(f, "nostr={}", urls.join(",")),
            Self::WorkOs { client, base, .. } => write!(f, "workos={client}{}", opt(base, "")),
        }
    }
}

/// IDENTITY-19: a button on the login page, `--login-button
/// <Label>=<provider>[:<connection>]`: it sends the browser to that OIDC
/// provider's authorization endpoint, naming the connection (Auth0's
/// `connection`, e.g. `google-oauth2`, `apple`, `samlp`; WorkOS's
/// `GoogleOAuth`, `AppleOAuth`, `authkit`, a `conn_…` or an `org_…` —
/// [`Provider::authorize_hint`]) when given. A connection containing
/// `saml` or `sso`, or the label `SSO`, asks for a work email first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginButton {
    pub label: String,
    pub provider: String,
    pub connection: Option<String>,
}

impl LoginButton {
    /// Whether the button asks for a work email (enterprise SSO).
    pub fn wants_email(&self) -> bool {
        let c = self.connection.as_deref().unwrap_or("").to_ascii_lowercase();
        c.contains("saml") || c.contains("sso") || self.label.eq_ignore_ascii_case("sso")
    }
}

impl FromStr for LoginButton {
    type Err = String;

    fn from_str(spec: &str) -> Result<Self, Self::Err> {
        let usage = "a --login-button is <Label>=<provider>[:<connection>], e.g. Google=auth0:google-oauth2";
        let (label, rest) = spec.split_once('=').ok_or_else(|| format!("`{spec}`: {usage}"))?;
        let (provider, connection) = match rest.split_once(':') {
            Some((p, c)) => (p.trim(), Some(c.trim().to_string()).filter(|c| !c.is_empty())),
            None => (rest.trim(), None),
        };
        if label.trim().is_empty() || provider.is_empty() {
            return Err(format!("`{spec}`: {usage}"));
        }
        Ok(Self { label: label.trim().to_string(), provider: provider.to_string(), connection })
    }
}

/// The session validator for the WebSocket upgrade: every provider's
/// adapter in one chain, behind the carriers (the `access_token` and
/// `nostr` cookies, then the bearer value). `None` when no provider is
/// configured, so nothing is installed and nothing changes.
pub fn boundary(providers: &[Provider]) -> anyhow::Result<Option<Arc<Carriers>>> {
    if providers.is_empty() {
        return Ok(None);
    }
    // IDENTITY-24 #10: two providers of one name would mint one namespace.
    let mut names = std::collections::HashSet::new();
    for p in providers {
        if !names.insert(p.name()) {
            anyhow::bail!("two --auth-provider entries are named `{}`: each provider needs its own name", p.name());
        }
    }
    let mut chain = AuthenticatorChain::new();
    for p in providers {
        chain = chain.with(p.adapter().map_err(|e| anyhow::anyhow!("--auth-provider {p}: {e}"))?);
    }
    Ok(Some(Arc::new(Carriers::cookie_and_bearer(chain))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_spec_names_its_provider() {
        let idp: Provider = "idp=http://127.0.0.1:4481,plexus:menger".parse().unwrap();
        assert_eq!(idp, Provider::PlexusIdp { issuer: "http://127.0.0.1:4481".into(), audience: "plexus:menger".into(), client: None });
        let auth0: Provider = "auth0=https://acme.auth0.com/,https://menger.example,https://menger.example/roles".parse().unwrap();
        assert_eq!(auth0.name(), "auth0");
        assert_eq!(auth0.to_string(), "auth0=https://acme.auth0.com/,https://menger.example,https://menger.example/roles");
        let logto: Provider = "oidc:logto=https://acme.logto.app/oidc,https://menger.example".parse().unwrap();
        assert_eq!(logto.name(), "logto");
        let nostr: Provider = "nostr=ws://127.0.0.1:4455/,ws://localhost:4455/".parse().unwrap();
        assert_eq!(nostr, Provider::Nostr { urls: vec!["ws://127.0.0.1:4455/".into(), "ws://localhost:4455/".into()] });
    }

    #[test]
    fn a_malformed_spec_says_what_one_looks_like() {
        for bad in ["idp", "idp=http://x", "idp=a,b,c", "auth0=a,,b", "ldap=a,b", "nostr=", "auth0=a,b,r1,r2", "idp=a,b,client="] {
            let e = bad.parse::<Provider>().unwrap_err();
            assert!(e.contains("--auth-provider") || e.contains("empty"), "{bad}: {e}");
        }
    }

    #[test]
    fn a_client_id_rides_any_oidc_spec_and_prints_back() {
        let a: Provider = "auth0=https://acme.auth0.com/,https://menger.example,client=menger,https://menger.example/roles".parse().unwrap();
        assert_eq!(a.oidc(), Some(("https://acme.auth0.com/", "https://menger.example", Some("menger"))));
        assert_eq!(a.to_string(), "auth0=https://acme.auth0.com/,https://menger.example,https://menger.example/roles,client=menger");
        let i: Provider = "idp=http://127.0.0.1:4481,plexus:menger,client=menger".parse().unwrap();
        assert_eq!(i.to_string().parse::<Provider>().unwrap(), i);
    }

    #[test]
    fn a_button_names_a_provider_and_maybe_a_connection() {
        let g: LoginButton = "Google=auth0:google-oauth2".parse().unwrap();
        assert_eq!((g.label.as_str(), g.provider.as_str(), g.connection.as_deref()), ("Google", "auth0", Some("google-oauth2")));
        assert!(!g.wants_email());
        assert!("SSO=auth0:samlp".parse::<LoginButton>().unwrap().wants_email());
        assert_eq!("Okta=oidc-okta".parse::<LoginButton>().unwrap().connection, None);
        assert!("=auth0".parse::<LoginButton>().is_err());
    }

    #[test]
    fn two_providers_of_one_name_are_refused() {
        let a: Provider = "auth0=http://127.0.0.1:1/,aud".parse().unwrap();
        let b: Provider = "oidc:auth0=http://127.0.0.1:2/,aud".parse().unwrap();
        assert!(boundary(&[a, b]).is_err());
    }

    #[test]
    fn no_provider_is_no_boundary() {
        assert!(boundary(&[]).unwrap().is_none());
    }

    #[test]
    fn an_issuer_that_is_not_https_off_loopback_is_refused_at_startup() {
        let p: Provider = "idp=http://idp.example,plexus:menger".parse().unwrap();
        assert!(boundary(&[p]).is_err());
    }

    #[test]
    fn a_workos_client_is_a_provider_with_a_per_client_issuer() {
        let w: Provider = "workos=client_01M4GRE0HND6SKGPV780GPTYDX".parse().unwrap();
        assert_eq!(w.name(), "workos");
        let iss = "https://api.workos.com/user_management/client_01M4GRE0HND6SKGPV780GPTYDX";
        assert_eq!(w.oidc(), Some((iss, "client_01M4GRE0HND6SKGPV780GPTYDX", Some("client_01M4GRE0HND6SKGPV780GPTYDX"))));
        assert!(!w.takes_passwords(), "WorkOS takes passwords on its own page");
        assert_eq!(w.to_string(), "workos=client_01M4GRE0HND6SKGPV780GPTYDX");
        let local: Provider = "workos=client_x,http://127.0.0.1:4490/".parse().unwrap();
        assert_eq!(local.oidc().unwrap().0, "http://127.0.0.1:4490/user_management/client_x");
        assert_eq!(local.to_string(), "workos=client_x,http://127.0.0.1:4490/");
        for bad in ["workos=", "workos=x", "workos=client_a,client_b,c", "workos=client_a/b", "workos=client_a,not a url"] {
            assert!(bad.parse::<Provider>().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_button_says_which_way_in_as_its_provider_takes_it() {
        let w: Provider = "workos=client_x".parse().unwrap();
        let hint = |c: Option<&str>| w.authorize_hint(c);
        assert_eq!(hint(None), vec![("provider", "authkit".to_string())]);
        assert_eq!(hint(Some("GoogleOAuth")), vec![("provider", "GoogleOAuth".to_string())]);
        assert_eq!(hint(Some("conn_01ABC")), vec![("connection_id", "conn_01ABC".to_string())]);
        assert_eq!(hint(Some("org_01ABC")), vec![("organization_id", "org_01ABC".to_string())]);
        let a: Provider = "auth0=https://acme.auth0.com/,aud,client=menger".parse().unwrap();
        assert_eq!(a.authorize_hint(Some("google-oauth2")), vec![("connection", "google-oauth2".to_string())]);
        assert!(a.authorize_hint(None).is_empty());
    }

    #[test]
    fn signing_out_of_workos_ends_its_session_too() {
        let w: Provider = "workos=client_x".parse().unwrap();
        assert_eq!(
            w.sign_out_url("session_01ABC", "http://127.0.0.1:4451/login").as_deref(),
            Some("https://api.workos.com/user_management/sessions/logout?session_id=session_01ABC&return_to=http%3A%2F%2F127.0.0.1%3A4451%2Flogin")
        );
        assert_eq!(w.sign_out_url("not-a-workos-session", "http://x/login"), None);
        let a: Provider = "auth0=https://acme.auth0.com/,aud".parse().unwrap();
        assert_eq!(a.sign_out_url("session_01ABC", "http://x/login"), None);
    }
}