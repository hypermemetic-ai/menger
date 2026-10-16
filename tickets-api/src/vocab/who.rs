//! ACCESS-8: who — a principal, who granted, a grantee — and where a role
//! holds as a call writes it. ACCESS-9 builds its capabilities on these.

use super::{not_a, too_long};
use crate::members::{scope_of, EVERYWHERE};
use std::fmt;

/// The longest principal any of these accepts: it bounds a hostile value,
/// not a real one (plexus-core's `auth_params` holds the same).
const PRINCIPAL_MAX: usize = 512;

/// Lower-case hex of exactly `n` digits.
fn lower_hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The identity layer's grammar (`plexus_auth_core::identity::Principal`),
/// read without it — that crate is the server's, and not wasm-safe:
/// `issuer:subject`, the issuer one of five, the subject by its issuer's
/// rule. Exactly; nothing is normalised but the trim (a principal has one
/// rendering). The server's tests hold the two to the same answers.
fn principal(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.len() > PRINCIPAL_MAX {
        return Err(too_long("a principal", t.len(), PRINCIPAL_MAX));
    }
    let refused = |why: &str| not_a(t, "a Plexus principal", why);
    let (issuer, subject) = t.split_once(':').ok_or_else(|| refused("issuer:subject — idp:<uuid>, oidc:<provider>/<sub>, nostr:<hex>, key:<hex>, apikey:<id>"))?;
    if subject.is_empty() || subject.contains(':') {
        return Err(refused("its subject is one part, not empty, with no `:`"));
    }
    let printable = |s: &str| s.bytes().all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\');
    let ok = match issuer {
        "idp" => {
            let b = subject.as_bytes();
            b.len() == 36
                && [8, 13, 18, 23].iter().all(|&i| b[i] == b'-')
                && subject.split('-').map(str::len).eq([8, 4, 4, 4, 12])
                && subject.split('-').all(|p| lower_hex(p, p.len()))
        }
        "nostr" | "key" => lower_hex(subject, 64),
        "apikey" => printable(subject),
        "oidc" => subject.split_once('/').is_some_and(|(p, sub)| {
            !p.is_empty() && p.len() <= 32 && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !sub.is_empty() && printable(sub)
        }),
        _ => return Err(refused("unknown issuer; the issuers are idp, nostr, apikey, oidc, key")),
    };
    match ok {
        true => Ok(t.to_string()),
        false => Err(refused(match issuer {
            "idp" => "idp's subject is a canonical lower-case hyphenated UUID",
            "nostr" | "key" => "its subject is 64 lower-case hex digits",
            "oidc" => "oidc's subject is <provider>/<sub>, the provider 1-32 of a-z, 0-9 and -",
            _ => "apikey's subject is printable, with no quote or backslash",
        })),
    }
}

text_type!(
    /// IDENTITY-21: a Plexus principal — `idp:<uuid>`, `oidc:<provider>/<sub>`,
    /// `nostr:<hex>`, `key:<hex>`, `apikey:<id>` — checked by the identity
    /// layer's grammar, wasm-safe. On the server it converts to and from
    /// `plexus_auth_core::identity::Principal` (`TryFrom` both ways).
    Principal,
    principal
);

#[cfg(feature = "server")]
impl From<&plexus_auth_core::identity::Principal> for Principal {
    fn from(p: &plexus_auth_core::identity::Principal) -> Self {
        Self(p.to_string())
    }
}

#[cfg(feature = "server")]
impl TryFrom<&Principal> for plexus_auth_core::identity::Principal {
    type Error = String;
    fn try_from(p: &Principal) -> Result<Self, String> {
        p.as_str().parse().map_err(|e: plexus_auth_core::identity::PrincipalParseError| e.to_string())
    }
}

/// What the operator's grants (`--admin`) say granted them.
pub const OPERATOR: &str = "--admin";

/// ACCESS-48: what a grant the dev sign-in made says granted it.
pub const DEV_LOGIN: &str = "dev-login";

/// ACCESS-48: what a grant made through `auth.grant` (AUTH-7) says granted
/// it — the gate decided it, the access layer wrote it.
pub const AUTH_GRANT: &str = "auth.grant";

/// IDENTITY-21: who gave a grant — a principal, or the operator (`--admin`
/// on the command line, which no principal can be). ACCESS-48: or the
/// server's own two writers — the dev sign-in (`dev-login`) and `auth.grant`
/// (`auth.grant`) — which no principal can be either (a principal holds a
/// `:`). Nothing else: a stored row naming anything else is held nowhere.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GrantedBy {
    Principal(Principal),
    Operator,
    DevLogin,
    AuthGrant,
}

impl TryFrom<&str> for GrantedBy {
    type Error = String;
    fn try_from(text: &str) -> Result<Self, String> {
        match text.trim() {
            OPERATOR => Ok(Self::Operator),
            DEV_LOGIN => Ok(Self::DevLogin),
            AUTH_GRANT => Ok(Self::AuthGrant),
            t => t.parse().map(Self::Principal),
        }
    }
}

impl fmt::Display for GrantedBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Principal(p) => f.write_str(p.as_str()),
            Self::Operator => f.write_str(OPERATOR),
            Self::DevLogin => f.write_str(DEV_LOGIN),
            Self::AuthGrant => f.write_str(AUTH_GRANT),
        }
    }
}

as_text!(GrantedBy, "Who gave a grant: a Plexus principal, --admin (the operator), dev-login or auth.grant");

fn team_name(text: &str) -> Result<String, String> {
    let t = text.trim();
    let ok = !t.is_empty()
        && t.len() <= 32
        && t.starts_with(|c: char| c.is_ascii_lowercase())
        && !t.ends_with('-')
        && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    match ok {
        true => Ok(t.to_string()),
        false => Err(not_a(t, "a team's name", "lower case letters, digits and -, starting with a letter, at most 32 (as release-crew)")),
    }
}

text_type!(
    /// ACCESS-22: a team's name — lower-kebab, starting with a letter, at
    /// most 32 (`release-crew`).
    TeamName,
    team_name
);

/// The prefix a team takes where a grantee is written: no principal's
/// issuer is `team`, so the two never collide.
pub const TEAM_PREFIX: &str = "team:";

/// ACCESS-9: who a grant is to — a principal, or a team (`team:<name>`,
/// whose members hold it; ACCESS-22 keeps the teams).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Grantee {
    Principal(Principal),
    Team(TeamName),
}

impl TryFrom<&str> for Grantee {
    type Error = String;
    fn try_from(text: &str) -> Result<Self, String> {
        match text.trim().strip_prefix(TEAM_PREFIX) {
            Some(team) => team.parse().map(Self::Team),
            None => text.parse().map(Self::Principal),
        }
    }
}

impl fmt::Display for Grantee {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Principal(p) => f.write_str(p.as_str()),
            Self::Team(t) => write!(f, "{TEAM_PREFIX}{t}"),
        }
    }
}

as_text!(Grantee, "Who a grant is to: a Plexus principal, or team:<name>");

text_type!(
    /// ORG-4: a scope as a call writes it (ACCESS-37's grammar) — `*`, an
    /// organization (`acme:`), or a shortcode in one by any short form of
    /// its address (`acme:MENGER/THEME`, `IDENTITY`), its codes upper case:
    /// [`crate::members::scope_of`]. ACCESS-21: or a ticket alone by any
    /// short form of its address (`MENGER-12`), or `under` one (`under
    /// MENGER-12`, it and its subtree). The store resolves it to the
    /// [`crate::members::Scope`] it keeps.
    ScopeRef,
    scope_of
);

/// A ceiling's scope: `*`, a shortcode, or one ticket (`SHORTCODE-<n>`),
/// in upper case. ORG-4: or an organization (`acme:`), or a shortcode in
/// one (`acme:MENGER`) — [`crate::members::scope_of`]'s spelling. (Moved
/// here from `access` by ACCESS-8: [`Ceiling`] is checked with it.)
pub fn ceiling_scope_of(text: &str) -> Result<String, String> {
    let t = text.trim();
    // ORG-9: a ticket's key splits at its last hyphen (`SYNAPSE-CC-1`).
    let ticket = t.rsplit_once('-').filter(|(code, n)| {
        !code.contains(':') && scope_of(code).is_ok_and(|c| c != EVERYWHERE && !crate::members::names_ticket(&c)) && !n.is_empty() && n.len() <= 12 && n.bytes().all(|b| b.is_ascii_digit())
    });
    match ticket {
        Some((code, n)) => Ok(format!("{}-{n}", code.to_ascii_uppercase())),
        // ACCESS-21: a ticket a ceiling names is its key, read above; any
        // other ticket's spelling a grant's scope takes is none here.
        None => scope_of(t).ok().filter(|s| !crate::members::names_ticket(s)).ok_or_else(|| {
            format!("`{t}` is not a scope: an organization (acme:), a shortcode (e.g. IDENTITY, acme:MENGER), a ticket (e.g. IDENTITY-12) or `*`")
        }),
    }
}

fn ceiling(text: &str) -> Result<String, String> {
    let t = text.trim();
    let (left, scope) = t.split_once('@').ok_or_else(|| format!("`{t}` is not role@SCOPE (e.g. editor@IDENTITY, viewer@*)"))?;
    let (left, scope) = (left.trim(), ceiling_scope_of(scope)?);
    let word = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    match left.split_once('.') {
        Some((a, m)) if word(a) && word(m) => Ok(format!("{left}@{scope}")),
        Some(_) => Err(format!("`{left}` is not a grant (e.g. tickets.get)")),
        None if word(left) => Ok(format!("{}@{scope}", left.to_ascii_lowercase())),
        None => Err(format!("`{left}` is not a role (e.g. editor)")),
    }
}

text_type!(
    /// IDENTITY-42: one entry of a session's ceiling — `role@SCOPE`
    /// (`editor@IDENTITY`, `viewer@*`) or `grant@SCOPE` (`tickets.get@MENGER-12`),
    /// the scope by [`ceiling_scope_of`]. Which roles there are, and the
    /// shortcode a scope names, are the server's to say.
    Ceiling,
    ceiling
);
