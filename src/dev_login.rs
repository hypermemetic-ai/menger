//! IDENTITY-35: the dev sign-in — the test environment's way to be anyone.
//!
//! Owner, 2026-10-09: "I want to be able to login as arbitrary users from
//! the main login page … seed users with different access levels to see
//! what they can see … click on a checkbox to redirect all auth to the
//! plexus-idp".
//!
//! Configured (test only: `main` starts it only with `--env test`), the
//! login page carries a checkbox, *Send all sign-in to plexus-idp*. Checked
//! (a cookie on this browser), the page offers plexus-idp alone: one click
//! per seeded user, and a form to be any username with any role on any
//! scope. Every way in is still a real password grant at plexus-idp,
//! verified by its adapter like any other sign-in; an account that does not
//! exist yet is registered there first (`identity.register` over its
//! Plexus socket), with the one dev password. The user's grants are written
//! to the store at each sign-in (granted by `dev-login`), so they come back
//! after a deploy replaces test's database with prod's.

use std::fmt;
use std::str::FromStr;

use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::WsClientBuilder;
use menger_tickets_api::members::{scope_of, Role};
use serde_json::{json, Value};

/// The cookie the checkbox sets: this browser's sign-in goes to plexus-idp.
pub const COOKIE: &str = "menger_dev_login";

/// One seeded user: `name=role@SCOPE[,role@SCOPE…]`, or `name=` for one
/// with no grant at all (who waits for an admin).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevUser {
    pub name: String,
    pub grants: Vec<(Role, String)>,
}

impl FromStr for DevUser {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (name, grants) = s.split_once('=').ok_or_else(|| format!("`{s}`: a dev user is name=role@SCOPE[,role@SCOPE…]"))?;
        let name = username(name)?;
        let grants = grants
            .split(',')
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(|g| {
                let (role, scope) = g.split_once('@').ok_or_else(|| format!("`{g}`: a grant is role@SCOPE (e.g. viewer@IDENTITY, admin@*)"))?;
                Ok((role.parse::<Role>()?, scope_of(scope)?))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { name, grants })
    }
}

impl fmt::Display for DevUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let g: Vec<String> = self.grants.iter().map(|(r, s)| format!("{r}@{s}")).collect();
        write!(f, "{}={}", self.name, g.join(","))
    }
}

/// A username plexus-idp will take and the page can show: letters, digits,
/// `-`, `_`, `.`, at most 64.
pub fn username(s: &str) -> Result<String, String> {
    let t = s.trim();
    let ok = !t.is_empty() && t.len() <= 64 && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(t.to_ascii_lowercase())
    } else {
        Err(format!("`{t}` is not a username: letters, digits, - _ . (at most 64)"))
    }
}

/// What the dev sign-in is configured with.
#[derive(Debug, Clone)]
pub struct DevLogin {
    /// The `--auth-provider` it signs in through (plexus-idp's name).
    pub provider: String,
    /// The password every dev user has.
    pub password: String,
    /// plexus-idp's Plexus socket, where accounts are registered.
    pub idp_ws: String,
    /// The seeded roster, in the order the page lists it.
    pub users: Vec<DevUser>,
}

impl DevLogin {
    /// The seeded user of that name.
    pub fn user(&self, name: &str) -> Option<&DevUser> {
        self.users.iter().find(|u| u.name == name)
    }

    /// Registers `name` at plexus-idp with the dev password. `Ok(false)`:
    /// the name was taken already.
    pub async fn register(&self, name: &str) -> Result<bool, String> {
        let client = WsClientBuilder::default()
            .build(&self.idp_ws)
            .await
            .map_err(|e| format!("plexus-idp's socket ({}) is unreachable: {e}", self.idp_ws))?;
        let mut p = ObjectParams::new();
        p.insert("method", "identity.register").map_err(|e| e.to_string())?;
        p.insert("params", json!({ "username": name, "password": self.password, "display_name": name }))
            .map_err(|e| e.to_string())?;
        let mut sub: Subscription<Value> =
            client.subscribe("idp.call", p, "idp.call_unsub").await.map_err(|e| format!("plexus-idp refused the call: {e}"))?;
        while let Some(item) = sub.next().await {
            let item = item.map_err(|e| e.to_string())?;
            match (item["type"].as_str(), item["content"]["type"].as_str()) {
                (Some("data"), Some("user_registered")) => return Ok(true),
                (Some("data"), Some("error")) => {
                    let m = item["content"]["message"].as_str().unwrap_or_default();
                    if m.contains("UNIQUE") || m.contains("exists") || m.contains("taken") {
                        return Ok(false);
                    }
                    return Err(format!("plexus-idp did not register {name}: {m}"));
                }
                (Some("error"), _) => return Err(format!("plexus-idp did not register {name}: {}", item["message"])),
                (Some("done"), _) => break,
                _ => {}
            }
        }
        Err(format!("plexus-idp answered nothing about {name}"))
    }
}

/// What a roster entry's grants read as on the page.
pub fn describe(grants: &[(Role, String)]) -> String {
    if grants.is_empty() {
        return "no access — waits for an admin".into();
    }
    grants
        .iter()
        .map(|(r, s)| if s == "*" { format!("{r} everywhere") } else { format!("{r} on {s}") })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dev_user_names_its_grants() {
        let u: DevUser = "Mixed=editor@ought, viewer@PLEXUS".parse().unwrap();
        assert_eq!(u.name, "mixed");
        assert_eq!(u.grants, vec![(Role::Editor, "OUGHT".into()), (Role::Viewer, "PLEXUS".into())]);
        assert_eq!(u.to_string(), "mixed=editor@OUGHT,viewer@PLEXUS");
        let nobody: DevUser = "nobody=".parse().unwrap();
        assert!(nobody.grants.is_empty());
        assert_eq!(describe(&nobody.grants), "no access — waits for an admin");
        assert_eq!(describe(&"a=admin@*".parse::<DevUser>().unwrap().grants), "admin everywhere");
        for bad in ["noequals", "a=owner@X", "a=viewer", "a b=viewer@X", "=viewer@X", "a=viewer@under X"] {
            assert!(bad.parse::<DevUser>().is_err(), "{bad}");
        }
    }
}
