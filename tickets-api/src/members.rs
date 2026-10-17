//! IDENTITY-21: the members vocabulary — who may see and change what, in
//! menger's own terms. Wasm-safe, like [`crate::comments`]: the app reads it
//! through the generated `MembersClient`.
//!
//! The IdP says who someone is; menger decides what they may do. A
//! **grant** gives a principal a [`Role`] on a **scope**: a shortcode, which
//! covers every shortcode hosted under it (the filter language's `under`),
//! or `*`, every ticket. ACCESS-21: or one ticket alone (`MENGER-12`: not
//! its parent, not its children), or a ticket and its subtree (`under
//! MENGER-12`), each kept by the ticket's key.
//!
//! | role | may |
//! | --- | --- |
//! | `viewer` | read under the scope, and comment |
//! | `editor` | also create and change tickets, and answer inputs |
//! | `admin` | also grant and revoke roles on the scope |
//! | `deployer` | read under the scope, and record its deployments — a CI's key (DEPLOY-14) |

use crate::address::{Address, OrgId, ShortcodeId, TicketId};
use crate::vocab::{GrantedBy, Grantee, Principal, TeamName};
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
/// and its subtree, wherever it moves) and `ID` (the ticket alone) — each by
/// the ticket's key ([`TicketId`]), so a grant follows its ticket through
/// `set_parent` and `set_kind`: every place that reads a scope matches on
/// this enum, so a form does not compile until each says what it means.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    Everywhere,
    Org(OrgId),
    Shortcode { org: OrgId, id: ShortcodeId },
    /// ACCESS-21: `under ID` — the ticket and every ticket below it.
    Under(TicketId),
    /// ACCESS-21: `ID` — the ticket alone: not its parent, not its children.
    Ticket(TicketId),
}

/// ACCESS-21: the word before a ticket's key in a subtree's scope.
pub const UNDER: &str = "under ";

/// ACCESS-21: whether `text` is a ticket's key as a scope keeps it — upper
/// case, `CODE-N` (ORG-9: split at its last hyphen; `CODE.N-M` for a
/// namesake's), no organization.
fn stored_key(text: &str) -> bool {
    !text.contains(':')
        && text.len() <= 64
        && crate::address::split_key(text).is_some()
        && text.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
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
        // ACCESS-21: a ticket's subtree, or the ticket alone, by its key.
        if let Some(key) = t.strip_prefix(UNDER) {
            return stored_key(key).then(|| Self::Under(TicketId::new(key)));
        }
        if stored_key(t) {
            return Some(Self::Ticket(TicketId::new(t)));
        }
        let (org, rest) = t.split_once(':')?;
        let org = OrgId::new(org).ok()?;
        if rest.is_empty() {
            return Some(Self::Org(org));
        }
        // ORG-9: a shortcode's identity, its code by the one rule
        // (`SYNAPSE-CC`), `.N` after it for a namesake; never a ticket's key.
        let (code, n) = rest.split_once('.').unwrap_or((rest, "0"));
        let ok = rest.len() <= 48
            && crate::address::is_code(code)
            && !n.is_empty()
            && n.bytes().all(|b| b.is_ascii_digit())
            && rest.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'.' || b == b'-');
        ok.then(|| Self::Shortcode { org, id: ShortcodeId::new(rest) })
    }

    /// The text it is stored as, and the gate compares.
    pub fn text(&self) -> String {
        match self {
            Self::Everywhere => EVERYWHERE.to_string(),
            Self::Org(org) => format!("{org}:"),
            Self::Shortcode { org, id } => format!("{org}:{id}"),
            Self::Under(key) => format!("{UNDER}{key}"),
            Self::Ticket(key) => key.to_string(),
        }
    }

    /// The organization it names — `None` for `*`, and (ACCESS-21) for a
    /// ticket's scope, whose organization is its ticket's: the store's to
    /// say ([`crate::store::Store::org_of`]).
    pub fn org(&self) -> Option<&OrgId> {
        match self {
            Self::Everywhere | Self::Under(_) | Self::Ticket(_) => None,
            Self::Org(org) | Self::Shortcode { org, .. } => Some(org),
        }
    }

    /// ACCESS-21: the ticket it is on — `under ID`'s or `ID`'s key.
    pub fn ticket(&self) -> Option<&TicketId> {
        match self {
            Self::Under(key) | Self::Ticket(key) => Some(key),
            _ => None,
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
            format!("`{}` is not a scope as kept: `*`, an organization (`acme:`), a shortcode in one by its identity (`acme:MENGER`), `under` a ticket's key (`under MENGER-12`) or a ticket's key (`MENGER-12`)", text.trim())
        })
    }
}

// ACCESS-8: on the wire as its text, the string it always was.
as_text!(Scope, "A grant's scope as kept: *, an organization (acme:), a shortcode in one by its identity (acme:MENGER), under a ticket's key (under MENGER-12: it and its subtree), or a ticket's key (MENGER-12: it alone)");

/// A scope as a reader writes it, checked for its spelling only: `*`, an
/// organization (`acme:`), or a shortcode by any short form of its address
/// (`acme:MENGER/THEME`, `MENGER/THEME`, `IDENTITY`) — its codes upper
/// case. ORG-4: which shortcode it names, and in which organization, is
/// the store's to say ([`crate::store::Store::scope_named`]), which keeps
/// it as a [`Scope`]. ACCESS-21: or a ticket by any short form of its
/// address — `under MENGER-12` (it and its subtree) or `MENGER-12` (it
/// alone), which the store keeps by the ticket's key. Anything else is
/// refused.
pub fn scope_of(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t == EVERYWHERE {
        return Ok(t.to_string());
    }
    let refused = || {
        format!("`{t}` is not a scope: `*`, an organization (`acme:`), a shortcode in one (`acme:MENGER`, or `IDENTITY` where one organization has it), `under` a ticket (`under MENGER-12`: it and its subtree) or a ticket (`MENGER-12`: it alone)")
    };
    // ACCESS-21: a ticket's subtree — `under` and a ticket's address.
    if let Some(rest) = t.strip_prefix("under ").or_else(|| t.strip_prefix("UNDER ")) {
        return match Address::parse(rest.trim()) {
            Ok(a) if a.names_ticket() && rest.len() <= 160 => Ok(format!("{UNDER}{a}")),
            _ => Err(refused()),
        };
    }
    if Scope::stored(t).is_some_and(|s| matches!(s, Scope::Ticket(_))) {
        return Ok(t.to_string());
    }
    if let Some(org) = t.strip_suffix(':') {
        return OrgId::new(org).map(|o| format!("{o}:")).map_err(|_| refused());
    }
    if Scope::stored(t).is_some_and(|s| matches!(s, Scope::Shortcode { .. })) {
        return Ok(t.to_string());
    }
    match Address::parse(t) {
        // ACCESS-21: a ticket's address is the ticket alone.
        Ok(a) if t.len() <= 160 => Ok(a.to_string()),
        _ => Err(refused()),
    }
}

/// ACCESS-21: whether a scope as [`scope_of`] spells it names a ticket —
/// `under` one, or one alone.
pub fn names_ticket(scope: &str) -> bool {
    let t = scope.trim();
    t.starts_with(UNDER) || (t != EVERYWHERE && !t.ends_with(':') && Address::parse(t).is_ok_and(|a| a.names_ticket()))
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

/// ACCESS-22: a team — a named set of principals that holds grants as one
/// grantee (`team:<name>`): each member holds what it holds. Its name is
/// unique app-wide (so `team:<name>` names one), and it lives in one
/// organization (ACCESS-37), where `members teams list` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Team {
    pub name: TeamName,
    pub org: OrgId,
    /// Who made it — a principal, or the operator's.
    pub created_by: GrantedBy,
    /// When, unix seconds.
    pub at: i64,
    /// How many principals are in it.
    pub members: u32,
}

/// ACCESS-22: one principal in a team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TeamMember {
    pub team: TeamName,
    pub principal: Principal,
    /// Who added them.
    pub added_by: GrantedBy,
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
    /// `mine`: who the caller is — before their teams and grants.
    Me { principal: Option<String>, name: Option<String>, admin_everywhere: bool },
    /// ACCESS-22: a team as it stands — `teams.list`, `teams.show`'s first,
    /// `teams.create` and `teams.rename`'s answer, and `mine`'s, one per
    /// team the caller is in (before its grants: a grant held through a team
    /// is the team's, `principal: team:<name>`).
    Team { team: Team },
    /// ACCESS-22: a principal in a team — `teams.show`'s, after the team;
    /// `teams.add`'s answer.
    TeamMember { member: TeamMember },
    /// ACCESS-22: `teams.remove`'s answer — whether they were in it.
    TeamLeft { team: TeamName, principal: Principal, existed: bool },
    /// ACCESS-22: `teams.delete`'s answer — the team, with every grant it
    /// held (`grants`, how many) and every member.
    TeamDeleted { team: TeamName, grants: u32, members: u32 },
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
        for bad in ["", "a b", "x)", "under X", "under acme:", "under *", &"A".repeat(33), "Acme:", "acme:MENGER-"] {
            assert!(scope_of(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn access_21_a_scope_is_a_ticket_alone_or_under_one() {
        // As a reader writes it: any address of a ticket.
        assert_eq!(scope_of("identity-3").unwrap(), "IDENTITY-3");
        assert_eq!(scope_of("under identity-3").unwrap(), "under IDENTITY-3");
        assert_eq!(scope_of("under acme:menger/theme-3").unwrap(), "under acme:MENGER/THEME-3");
        assert_eq!(scope_of("acme:MENGER-1").unwrap(), "acme:MENGER-1");
        // As kept: by the ticket's key.
        let key = |k: &str| TicketId::new(k);
        assert_eq!(Scope::stored("under MNG-40"), Some(Scope::Under(key("MNG-40"))));
        assert_eq!(Scope::stored("MNG-40"), Some(Scope::Ticket(key("MNG-40"))));
        assert_eq!(Scope::stored("SYNAPSE-CC.2-3"), Some(Scope::Ticket(key("SYNAPSE-CC.2-3"))));
        for s in ["under MNG-40", "MNG-40", "SYNAPSE-CC-1"] {
            assert_eq!(Scope::stored(s).unwrap().text(), s);
        }
        assert_eq!(Scope::stored("under MNG-40").unwrap().ticket(), Some(&key("MNG-40")));
        assert_eq!(Scope::stored("MNG-40").unwrap().org(), None);
        // Not a key as kept: lower case, an org, a shortcode under `under`.
        for no in ["mng-40", "under mng-40", "under MNG", "under acme:MNG-40", "acme:MNG-40", "MNG", "under  MNG-40"] {
            assert_eq!(Scope::stored(no), None, "{no}");
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

/// MACRO-21: the emitted client implements the activation's trait; a client
/// failure is the in-band error, the last item of its stream.
impl plexus_client::FromClientError for MembersEvent {
    fn from_client_error(error: plexus_client::ClientError) -> Self {
        MembersEvent::Error { message: error.to_string() }
    }
}
