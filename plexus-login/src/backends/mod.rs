//! AUTH-22: the identity backends, and the spec that picks one —
//! configuration, never code.
//!
//! | `--identity` | backend | capabilities |
//! | --- | --- | --- |
//! | `workos:<client_…>[,social=google+github+apple+microsoft][,sso=off][,email=off][,base=<url>][,key=@<file>]` | [`workos::WorkOs`] | email code (Magic Auth; `email=off` where it is disabled), social, SSO by domain, TOTP, refresh |
//! | `plexus-idp:<issuer>,<audience>` | [`plexus_idp::PlexusIdp`] | password, refresh |
//! | `memory[:outbox=<file>][,mfa=required]` | [`memory::Memory`] | email code, social (simulated), TOTP — tests and local development only |
//!
//! WorkOS's API key never rides the spec (a flag is visible in `ps`): it
//! comes from `<APP>_WORKOS_API_KEY`, or from the file `key=@<path>` names.

pub mod memory;
pub mod plexus_idp;
pub mod workos;

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use crate::identity::{IdentityBackend, Social};

/// A parsed `--identity` spec. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentitySpec {
    WorkOs { client: String, base: Option<String>, key_file: Option<PathBuf>, social: Vec<Social>, sso: bool, email: bool },
    PlexusIdp { issuer: String, audience: String },
    Memory { outbox: Option<PathBuf>, require_mfa: bool },
}

const USAGE: &str = "an --identity is workos:<client_…>[,social=google+github+apple][,sso=off][,email=off][,base=<url>][,key=@<file>] · \
                     plexus-idp:<issuer>,<audience> · memory[:outbox=<file>][,mfa=required]";

impl FromStr for IdentitySpec {
    type Err = String;

    fn from_str(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        let (kind, rest) = spec.split_once(':').unwrap_or((spec, ""));
        let parts: Vec<&str> = if rest.is_empty() { Vec::new() } else { rest.split(',').map(str::trim).collect() };
        // The spec is never echoed: a mistyped one might hold a secret.
        let bad = |why: &str| format!("--identity {kind}: {why}; {USAGE}");
        match kind {
            "workos" => {
                let [client, opts @ ..] = parts.as_slice() else { return Err(bad("a WorkOS client id")) };
                if !client.starts_with("client_") || client.contains(['/', '?', '#', '=']) {
                    return Err(bad("a WorkOS client id is client_…"));
                }
                let (mut base, mut key_file, mut social, mut sso, mut email) = (None, None, Vec::new(), true, true);
                for o in opts {
                    match o.split_once('=') {
                        Some(("base", b)) if url::Url::parse(b).is_ok() => base = Some(b.to_string()),
                        Some(("key", k)) => match k.strip_prefix('@') {
                            Some(path) if !path.is_empty() => key_file = Some(PathBuf::from(path)),
                            _ => return Err(bad("the API key is never written in a flag: key=@<file>, or <APP>_WORKOS_API_KEY")),
                        },
                        Some(("social", list)) => {
                            for s in list.split('+').filter(|s| !s.trim().is_empty()) {
                                social.push(Social::parse(s).ok_or_else(|| bad(&format!("`{s}` is not google, github, apple or microsoft")))?);
                            }
                        }
                        Some(("sso", "off")) => sso = false,
                        Some(("sso", "on")) => sso = true,
                        Some(("email", "off")) => email = false,
                        Some(("email", "on")) => email = true,
                        _ => return Err(bad(&format!("`{o}` is not an option"))),
                    }
                }
                Ok(Self::WorkOs { client: client.to_string(), base, key_file, social, sso, email })
            }
            "plexus-idp" => match parts.as_slice() {
                [issuer, audience] if !issuer.is_empty() && !audience.is_empty() => Ok(Self::PlexusIdp { issuer: issuer.to_string(), audience: audience.to_string() }),
                _ => Err(bad("plexus-idp takes its issuer and audience")),
            },
            "memory" => {
                let (mut outbox, mut require_mfa) = (None, false);
                for o in &parts {
                    match o.split_once('=') {
                        Some(("outbox", p)) if !p.is_empty() => outbox = Some(PathBuf::from(p)),
                        Some(("mfa", "required")) => require_mfa = true,
                        _ => return Err(bad(&format!("`{o}` is not an option"))),
                    }
                }
                Ok(Self::Memory { outbox, require_mfa })
            }
            other => Err(bad(&format!("unknown backend `{other}`"))),
        }
    }
}

impl fmt::Display for IdentitySpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkOs { client, base, key_file, social, sso, email } => {
                write!(f, "workos:{client}")?;
                if !social.is_empty() {
                    write!(f, ",social={}", social.iter().map(|s| s.name()).collect::<Vec<_>>().join("+"))?;
                }
                if !sso {
                    f.write_str(",sso=off")?;
                }
                if !email {
                    f.write_str(",email=off")?;
                }
                if let Some(b) = base {
                    write!(f, ",base={b}")?;
                }
                if let Some(k) = key_file {
                    write!(f, ",key=@{}", k.display())?;
                }
                Ok(())
            }
            Self::PlexusIdp { issuer, audience } => write!(f, "plexus-idp:{issuer},{audience}"),
            Self::Memory { outbox, require_mfa } => {
                f.write_str("memory")?;
                let mut opts = Vec::new();
                if let Some(o) = outbox {
                    opts.push(format!("outbox={}", o.display()));
                }
                if *require_mfa {
                    opts.push("mfa=required".into());
                }
                if !opts.is_empty() {
                    write!(f, ":{}", opts.join(","))?;
                }
                Ok(())
            }
        }
    }
}

impl IdentitySpec {
    /// The backend this spec names, for the app called `app` (its
    /// `<APP>_WORKOS_API_KEY`, and the name authenticator apps show).
    pub fn build(&self, app: &str) -> anyhow::Result<Arc<dyn IdentityBackend>> {
        Ok(match self {
            Self::WorkOs { client, base, key_file, social, sso, email } => {
                let key = workos::ApiKey::from_env_or_file(app, key_file.as_deref())
                    .map_err(anyhow::Error::msg)?
                    .ok_or_else(|| anyhow::anyhow!("--identity workos needs its API key: set {} or name a file with key=@<path>", workos::ApiKey::env_var(app)))?;
                Arc::new(workos::WorkOs::new(workos::WorkOsConfig { client: client.clone(), base: base.clone(), key, social: social.clone(), sso: *sso, email_code: *email, totp_issuer: app.to_string() })?)
            }
            Self::PlexusIdp { issuer, audience } => Arc::new(plexus_idp::PlexusIdp::new(issuer, audience)?),
            Self::Memory { outbox, require_mfa } => {
                tracing::warn!("--identity memory: users live in this process and codes go to an outbox — never for people");
                Arc::new(memory::Memory::new(memory::MemoryOptions { outbox: outbox.clone(), require_mfa: *require_mfa }))
            }
        })
    }
}

/// [`IdentitySpec`] parsed and built.
pub fn from_spec(spec: &str, app: &str) -> anyhow::Result<Arc<dyn IdentityBackend>> {
    spec.parse::<IdentitySpec>().map_err(anyhow::Error::msg)?.build(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_spec_names_its_backend_and_prints_back() {
        for s in [
            "workos:client_01ABC",
            "workos:client_01ABC,social=google+github+apple,sso=off,email=off,base=http://127.0.0.1:9/,key=@/run/secrets/workos",
            "plexus-idp:http://127.0.0.1:4481,plexus:menger",
            "memory",
            "memory:outbox=/tmp/outbox,mfa=required",
        ] {
            let p: IdentitySpec = s.parse().unwrap();
            assert_eq!(p.to_string(), s);
        }
    }

    #[test]
    fn a_key_never_rides_the_spec() {
        let e = "workos:client_01ABC,key=sk_test_123".parse::<IdentitySpec>().unwrap_err();
        assert!(e.contains("never written in a flag") && !e.contains("sk_test_123"), "{e}");
        for bad in ["workos", "workos:x", "workos:client_1,social=myspace", "plexus-idp:x", "ldap:x", "memory:what=ever"] {
            assert!(bad.parse::<IdentitySpec>().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_workos_backend_needs_its_key() {
        let s: IdentitySpec = "workos:client_01NOKEY".parse().unwrap();
        let e = s.build("nokeyapp").err().expect("no key").to_string();
        assert!(e.contains("NOKEYAPP_WORKOS_API_KEY"), "{e}");
    }
}
