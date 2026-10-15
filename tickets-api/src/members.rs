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

use crate::address::{Address, OrgId, ShortcodeId};
use crate::vocab::{GrantedBy, Grantee};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
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

/// ORG-4: a grant's scope as stored (ACCESS-37's grammar) — `*`; an
/// organization, `org:`, every ticket in it; or a shortcode in one,
/// `org:ID`, by the shortcode's identity ([`ShortcodeId`]: its code for
/// every shortcode made before ORG-3, `CODE.N` for a namesake), covering
/// it and every shortcode hosted under it. A reader writes a shortcode by
/// any short form of its address (`acme:MENGER/THEME`, `IDENTITY`); the
/// store keeps it by identity, so a grant follows its shortcode wherever
/// its root moves, and an org's rename rewrites the org part.
///
/// ACCESS-21 adds the last two of ACCESS-37's forms — `under ID` (a ticket
/// and its subtree) and `ID` (one ticket) — as variants here: every place
/// that reads a scope matches on this enum, so a new form does not compile
/// until each says what it means.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    Everywhere,
    Org(OrgId),
    Shortcode { org: OrgId, id: ShortcodeId },
}

impl Scope {
    /// A stored scope's text read back — `None` for text that is not one
    /// (a grant from before ORG-4, `IDENTITY`, which the first open
    /// rewrites as `hypermemetic:IDENTITY`).
    pub fn stored(text: &str) -> Option<Self> {
        let t = text.trim();
        if t == EVERYWHERE {
            return Some(Self::Everywhere);
        }
        let (org, rest) = t.split_once(':')?;
        let org = OrgId::new(org).ok()?;
        if rest.is_empty() {
            return Some(Self::Org(org));
        }
        let ok = rest.len() <= 48 && rest.split('.').next().is_some_and(|c| !c.is_empty())
            && rest.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'.');
        ok.then(|| Self::Shortcode { org, id: ShortcodeId::new(rest) })
    }

    /// The text it is stored as, and the gate compares.
    pub fn text(&self) -> String {
        match self {
            Self::Everywhere => EVERYWHERE.to_string(),
            Self::Org(org) => format!("{org}:"),
            Self::Shortcode { org, id } => format!("{org}:{id}"),
        }
    }

    /// The organization it is inside — `None` for `*`.
    pub fn org(&self) -> Option<&OrgId> {
        match self {
            Self::Everywhere => None,
            Self::Org(org) | Self::Shortcode { org, .. } => Some(org),
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

/// ACCESS-8: a stored scope's text read back ([`Scope::stored`]), or why it
/// is none — what deserializing one checks.
impl TryFrom<&str> for Scope {
    type Error = String;
    fn try_from(text: &str) -> Result<Self, String> {
        Self::stored(text).ok_or_else(|| {
            format!("`{}` is not a scope as kept: `*`, an organization (`acme:`), or a shortcode in one by its identity (`acme:MENGER`)", text.trim())
        })
    }
}

// ACCESS-8: on the wire as its text, the string it always was.
as_text!(Scope, "A grant's scope as kept: *, an organization (acme:), or a shortcode in one by its identity (acme:MENGER)");

/// A scope as a reader writes it, checked for its spelling only: `*`, an
/// organization (`acme:`), or a shortcode by any short form of its address
/// (`acme:MENGER/THEME`, `MENGER/THEME`, `IDENTITY`) — its codes upper
/// case. ORG-4: which shortcode it names, and in which organization, is
/// the store's to say ([`crate::store::Store::scope_named`]), which keeps
/// it as a [`Scope`]. Anything else is refused.
pub fn scope_of(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t == EVERYWHERE {
        return Ok(t.to_string());
    }
    let refused = || {
        format!("`{t}` is not a scope: `*`, an organization (`acme:`), or a shortcode in one (`acme:MENGER`, or `IDENTITY` where one organization has it)")
    };
    if let Some(org) = t.strip_suffix(':') {
        return OrgId::new(org).map(|o| format!("{o}:")).map_err(|_| refused());
    }
    if Scope::stored(t).is_some_and(|s| matches!(s, Scope::Shortcode { .. })) {
        return Ok(t.to_string());
    }
    match Address::parse(t) {
        Ok(a) if !a.names_ticket() && t.len() <= 160 => Ok(a.to_string()),
        _ => Err(refused()),
    }
}

/// One grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Grant {
    /// The Plexus principal: `idp:<uuid>`, `oidc:<provider>/<sub>`, `nostr:<hex>`
    /// — ACCESS-48: a [`Grantee`], so a team's grant (`team:<name>`,
    /// ACCESS-9) is one too.
    pub principal: Grantee,
    /// ORG-4: `*`, an organization (`acme:`), or a shortcode in one
    /// (`acme:MENGER`, by its identity). ACCESS-48: a [`Scope`] — a grant
    /// from before ORG-4 is a stored row the store's open rewrites before
    /// it is ever one of these (`store::members::Row`).
    pub scope: Scope,
    pub role: Role,
    /// The principal who granted it, or `--admin` for the operator's
    /// (`dev-login`, `auth.grant`: the server's own writers).
    pub granted_by: GrantedBy,
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
    Revoked { principal: Grantee, scope: Scope, existed: bool },
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
        for bad in ["", "a b", "x)", "under X", &"A".repeat(33), "IDENTITY-3", "Acme:", "acme:MENGER-1"] {
            assert!(scope_of(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn org_4_a_scope_names_an_org_or_a_shortcode_in_one() {
        assert_eq!(scope_of("acme:").unwrap(), "acme:");
        assert_eq!(scope_of("acme:menger/theme").unwrap(), "acme:MENGER/THEME");
        assert_eq!(scope_of("hypermemetic:MENGER.12").unwrap(), "hypermemetic:MENGER.12");
        assert_eq!(Scope::stored("*"), Some(Scope::Everywhere));
        assert_eq!(Scope::stored("acme:"), Some(Scope::Org(OrgId::new("acme").unwrap())));
        let sc = Scope::stored("hypermemetic:THEME.62").unwrap();
        assert_eq!((sc.text(), sc.org().map(OrgId::as_str)), ("hypermemetic:THEME.62".to_string(), Some("hypermemetic")));
        // A grant from before ORG-4, and anything that is no stored form.
        for old in ["IDENTITY", "acme:menger", "acme:MENGER/THEME", "Acme:"] {
            assert_eq!(Scope::stored(old), None, "{old}");
        }
    }
}
