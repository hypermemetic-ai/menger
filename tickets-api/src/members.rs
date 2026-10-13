//! IDENTITY-21: the members vocabulary — who may see and change what, in
//! menger's own terms. Wasm-safe, like [`crate::comments`]: the app reads it
//! through the generated `MembersClient`.
//!
//! The IdP says who someone is; menger decides what they may do. A
//! **grant** gives a principal a [`Role`] on a **scope**: a shortcode, which
//! covers every shortcode hosted under it (the filter language's `under`),
//! or `*`, every ticket.
//!
//! | role | may |
//! | --- | --- |
//! | `viewer` | read under the scope, and comment |
//! | `editor` | also create and change tickets, and answer inputs |
//! | `admin` | also grant and revoke roles on the scope |
//! | `deployer` | read under the scope, and record its deployments — a CI's key (DEPLOY-14) |

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// A role. Viewer, editor and admin are ordered, each including the ones
/// before it. DEPLOY-14: `deployer` stands beside that line — a viewer's
/// reads and `deployments.record`, nothing an editor may do — so a CI's key
/// can be granted exactly what recording a deployment needs; its place in
/// the order is only for sorting. (What each role may is the gate's,
/// declared on each activation with `roles(...)`; this names the role a
/// membership holds.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Viewer,
    Deployer,
    Editor,
    Admin,
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Deployer => "deployer",
            Role::Editor => "editor",
            Role::Admin => "admin",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "viewer" => Ok(Role::Viewer),
            "deployer" => Ok(Role::Deployer),
            "editor" => Ok(Role::Editor),
            "admin" => Ok(Role::Admin),
            other => Err(format!("unknown role `{other}` — the roles are viewer, editor, admin, deployer")),
        }
    }
}

/// Every ticket: the scope a grant names to cover all of them.
pub const EVERYWHERE: &str = "*";

/// A scope as stored: `*`, or a shortcode in upper case. Anything else is
/// refused.
pub fn scope_of(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t == EVERYWHERE {
        return Ok(t.to_string());
    }
    let ok = !t.is_empty() && t.len() <= 32 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(t.to_ascii_uppercase())
    } else {
        Err(format!("`{t}` is not a scope: a shortcode (e.g. IDENTITY) or `*`"))
    }
}

/// One grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Grant {
    /// The Plexus principal: `idp:<uuid>`, `oidc:<provider>/<sub>`, `nostr:<hex>`.
    pub principal: String,
    /// A shortcode, or `*`.
    pub scope: String,
    pub role: Role,
    /// The principal who granted it, or `--admin` for the operator's.
    pub granted_by: String,
    /// When, unix seconds.
    pub at: i64,
}

/// What the `members` activation's methods yield.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MembersEvent {
    /// A grant as it stands (`list`, `mine`, and `grant`'s answer).
    Grant { grant: Grant },
    /// `revoke`'s answer: the grant that was removed, or `existed: false`.
    Revoked { principal: String, scope: String, existed: bool },
    /// `requests`: someone signed in who holds no grant yet.
    Waiting {
        principal: String,
        name: Option<String>,
        provider: Option<String>,
        /// When they first signed in without access, unix seconds.
        since: i64,
    },
    /// `mine`: who the caller is — before their grants.
    Me { principal: Option<String>, name: Option<String>, admin_everywhere: bool },
    /// A refusal, naming the problem; nothing was changed.
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_ordered_and_named() {
        assert!(Role::Viewer < Role::Editor && Role::Editor < Role::Admin);
        assert_eq!("Editor".parse::<Role>(), Ok(Role::Editor));
        assert!("owner".parse::<Role>().is_err());
        // DEPLOY-14: a CI's key holds deployer, and it reads back as itself.
        assert_eq!("Deployer".parse::<Role>(), Ok(Role::Deployer));
        assert_eq!(Role::Deployer.as_str().parse::<Role>(), Ok(Role::Deployer));
    }

    #[test]
    fn a_scope_is_a_shortcode_or_everywhere() {
        assert_eq!(scope_of("identity").unwrap(), "IDENTITY");
        assert_eq!(scope_of("*").unwrap(), "*");
        for bad in ["", "a b", "x)", "under X", &"A".repeat(33)] {
            assert!(scope_of(bad).is_err(), "{bad}");
        }
    }
}
