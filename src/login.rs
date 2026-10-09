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
//! | `nostr=<service-url>[,<service-url>…]` | NIP-98 | `nostr:<hex>` |
//!
//! No provider, no chain: menger serves as it did before.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use plexus_auth_core::adapters::{Carriers, ClaimMap, Nip98Adapter, OidcAdapter};
use plexus_auth_core::identity::{Authenticator, AuthenticatorChain};

/// One provider menger admits callers through, as `--auth-provider` names
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    /// `idp=<issuer>,<audience>`: plexus-idp.
    PlexusIdp { issuer: String, audience: String },
    /// `auth0=<domain>,<audience>[,<roles-claim>]`: an Auth0 tenant and the
    /// API identifier its tokens are minted for; roles from the namespaced
    /// claim an Auth0 Action adds.
    Auth0 { domain: String, audience: String, roles_claim: Option<String> },
    /// `oidc:<name>=<issuer>,<audience>[,<roles-claim>]`: any other OIDC
    /// provider, named `<name>` in its principals.
    Oidc { name: String, issuer: String, audience: String, roles_claim: Option<String> },
    /// `nostr=<service-url>[,<service-url>…]`: NIP-98 events naming one of
    /// these URLs, the WebSocket URLs callers connect to.
    Nostr { urls: Vec<String> },
}

impl Provider {
    /// The provider's name, as `session.whoami`'s `provider` says it.
    pub fn name(&self) -> &str {
        match self {
            Self::PlexusIdp { .. } => "plexus-idp",
            Self::Auth0 { .. } => "auth0",
            Self::Oidc { name, .. } => name,
            Self::Nostr { .. } => "nostr",
        }
    }

    /// The provider's adapter.
    pub fn adapter(&self) -> anyhow::Result<Arc<dyn Authenticator>> {
        Ok(match self {
            Self::PlexusIdp { issuer, audience } => Arc::new(OidcAdapter::plexus_idp(issuer, audience)?),
            Self::Auth0 { domain, audience, roles_claim } => {
                Arc::new(OidcAdapter::auth0(domain, audience, roles_claim.as_deref())?)
            }
            Self::Oidc { name, issuer, audience, roles_claim } => {
                let claims = ClaimMap {
                    name: vec!["preferred_username".into(), "name".into(), "email".into()],
                    roles: roles_claim.clone(),
                };
                Arc::new(OidcAdapter::generic(name, issuer, audience, claims)?)
            }
            Self::Nostr { urls } => Arc::new(Nip98Adapter::new(urls)?),
        })
    }
}

impl FromStr for Provider {
    type Err = String;

    fn from_str(spec: &str) -> Result<Self, Self::Err> {
        let usage = "an --auth-provider is idp=<issuer>,<audience> · \
                     auth0=<domain>,<audience>[,<roles-claim>] · \
                     oidc:<name>=<issuer>,<audience>[,<roles-claim>] · \
                     nostr=<service-url>[,<service-url>…]";
        let (kind, rest) = spec.trim().split_once('=').ok_or_else(|| format!("`{spec}`: {usage}"))?;
        let parts: Vec<String> = rest.split(',').map(|p| p.trim().to_string()).collect();
        if parts.iter().any(String::is_empty) {
            return Err(format!("`{spec}`: an empty value; {usage}"));
        }
        let oidc = |n: usize| -> Result<(String, String, Option<String>), String> {
            match parts.as_slice() {
                [issuer, audience] => Ok((issuer.clone(), audience.clone(), None)),
                [issuer, audience, roles] if n == 3 => Ok((issuer.clone(), audience.clone(), Some(roles.clone()))),
                _ => Err(format!("`{spec}`: {usage}")),
            }
        };
        match kind.trim() {
            "idp" => {
                let (issuer, audience, _) = oidc(2)?;
                Ok(Self::PlexusIdp { issuer, audience })
            }
            "auth0" => {
                let (domain, audience, roles_claim) = oidc(3)?;
                Ok(Self::Auth0 { domain, audience, roles_claim })
            }
            "nostr" => Ok(Self::Nostr { urls: parts }),
            other => match other.strip_prefix("oidc:") {
                Some(name) => {
                    let (issuer, audience, roles_claim) = oidc(3)?;
                    Ok(Self::Oidc { name: name.to_string(), issuer, audience, roles_claim })
                }
                None => Err(format!("`{spec}`: unknown provider `{other}`; {usage}")),
            },
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let roles = |r: &Option<String>| r.as_ref().map(|r| format!(",{r}")).unwrap_or_default();
        match self {
            Self::PlexusIdp { issuer, audience } => write!(f, "idp={issuer},{audience}"),
            Self::Auth0 { domain, audience, roles_claim } => {
                write!(f, "auth0={domain},{audience}{}", roles(roles_claim))
            }
            Self::Oidc { name, issuer, audience, roles_claim } => {
                write!(f, "oidc:{name}={issuer},{audience}{}", roles(roles_claim))
            }
            Self::Nostr { urls } => write!(f, "nostr={}", urls.join(",")),
        }
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
        assert_eq!(idp, Provider::PlexusIdp { issuer: "http://127.0.0.1:4481".into(), audience: "plexus:menger".into() });
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
        for bad in ["idp", "idp=http://x", "idp=a,b,c", "auth0=a,,b", "ldap=a,b", "nostr="] {
            let e = bad.parse::<Provider>().unwrap_err();
            assert!(e.contains("--auth-provider") || e.contains("empty"), "{bad}: {e}");
        }
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
}
