//! ORG-3: what names a shortcode and a ticket — the identity types, and the
//! one parser from text to an [`Address`].
//!
//! A shortcode is unique only within its **namespace** (ORG-1, owner
//! 2026-10-13: *"shortcodes are generally not unique. they must be unique
//! within their namespace"*): its organization and the shortcode it is
//! hosted under — none for a root (ORG-2's input `namespace-host`, on its
//! recommended answer: the host's *shortcode*, so the full address below is
//! unique by construction). The types say so:
//!
//! - [`Code`] is a shortcode's letters and nothing more. It has no `Hash`
//!   and no `Ord`, so it cannot key a map or a set — a bare code is never
//!   identity; it is what a reader types and what a row shows.
//!
//!   A code keys no hash map:
//!
//! ```compile_fail
//! use std::collections::HashMap;
//! use menger_tickets_api::Code;
//! let mut by_code: HashMap<Code, u32> = HashMap::new();
//! by_code.insert(Code::parse("THEME").unwrap(), 1);
//! ```
//!
//! nor an ordered one, nor a set:
//!
//! ```compile_fail
//! use std::collections::BTreeSet;
//! use menger_tickets_api::Code;
//! let mut codes = BTreeSet::new();
//! codes.insert(Code::parse("THEME").unwrap());
//! ```
//!
//! while the identity does:
//!
//! ```
//! use std::collections::HashMap;
//! use menger_tickets_api::ShortcodeId;
//! let mut by_id: HashMap<ShortcodeId, u32> = HashMap::new();
//! by_id.insert(ShortcodeId::new("THEME"), 1);
//! ```
//!
//! - [`ShortcodeId`] is the store's identity of one shortcode, minted once
//!   and never parsed: the code itself for every shortcode made before
//!   ORG-3 (so no stored row is rewritten), `CODE.N` for a namesake made
//!   after. What the store keys on.
//! - [`Namespace`] is `(org, host)`, and [`Shortcode`] is a shortcode's
//!   record: its id, its namespace and its code.
//! - [`TicketId`] is a ticket's key — today's id text, immutable and opaque
//!   (ORG-6 finding 1): nothing keyed by ticket is rewritten, and a ticket
//!   keeps its key wherever its shortcode moves.
//!
//! A ticket is written in full as `org:ROOT/…/SC-N` (ORG-1's `address`
//! decision, `hypermemetic:MENGER/THEME-3`); a **short form** — `SC-N`,
//! `ROOT/SC-N`, any hosts in order, an org or none — resolves when it names
//! one ticket in the reader's reach (the store's resolver). A segment that
//! begins with `_` begins a path (WAVE-10's `_MENGER/…`): `PLEXUS/_MENGER/board`
//! is the path `_MENGER/board` in PLEXUS, with an org or without.
//!
//! Wasm-safe (serde, schemars, std): the browser app compiles these with
//! `default-features = false`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

/// The longest code a shortcode may have (the gate's spelling of a scope
/// already capped it at 32).
pub const CODE_MAX: usize = 32;

/// ORG-9: the one shortcode rule — ASCII letters and digits, any case, 1 to
/// [`CODE_MAX`], in segments joined by single hyphens (owner, 2026-10-16:
/// *"synapse-cc-1 should be a valid name … only the final -N is
/// protected"*). No leading, trailing or doubled hyphen, and no segment after
/// the first all digits — so `A-1` is never a shortcode (it is ticket 1 of
/// A), and `A-1-2` reads one way only. A ticket's key splits at its last
/// hyphen ([`split_key`]).
pub fn is_code(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= CODE_MAX
        && text.split('-').enumerate().all(|(i, seg)| {
            !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_alphanumeric()) && (i == 0 || !seg.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// ORG-9: a ticket's key, `CODE-N`, split at its **last** hyphen into its
/// shortcode's identity and its number — `SYNAPSE-CC-1` is `(SYNAPSE-CC, 1)`,
/// a namesake's `THEME.62-3` is `(THEME.62, 3)`. `None` for anything that is
/// not one: a number of no digits or past `u32`, or a code [`is_code`]
/// refuses (`.N` after it, for a namesake). Case is kept as written.
pub fn split_key(id: &str) -> Option<(&str, u32)> {
    let (code, n) = id.rsplit_once('-')?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = n.parse::<u32>().ok()?;
    let letters = match code.split_once('.') {
        Some((c, k)) if !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) => c,
        Some(_) => return None,
        None => code,
    };
    is_code(letters).then_some((code, n))
}

/// ORG-3: an organization's name — lower-case ASCII letters, digits and
/// `-`, 1 to 63 of them, not starting or ending with `-`. Today's tracker is
/// the one organization [`OrgId::HYPERMEMETIC`] (ORG-1's `org-name`); ORG-4
/// makes more. Org names compare exactly: `Hypermemetic:` is no org.
///
/// ACCESS-8: deserializing one checks it ([`OrgId::new`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct OrgId(String);

impl<'de> Deserialize<'de> for OrgId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// Its name, read as text, as [`Code`]'s letters are.
impl std::ops::Deref for OrgId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for OrgId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Self::new(s.trim())
    }
}

impl OrgId {
    /// The organization today's tracker is (ORG-1, decided 2026-10-13).
    pub const HYPERMEMETIC: &'static str = "hypermemetic";

    /// `name` as an org name, or why it is not one.
    pub fn new(name: &str) -> Result<Self, String> {
        let ok = !name.is_empty()
            && name.len() <= 63
            && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !name.starts_with('-')
            && !name.ends_with('-');
        match ok {
            true => Ok(Self(name.to_string())),
            false => Err(format!(
                "`{name}` is not an organization's name: lower-case letters, digits and `-`, as `hypermemetic`"
            )),
        }
    }

    /// The one organization there is until ORG-4.
    pub fn hypermemetic() -> Self {
        Self(Self::HYPERMEMETIC.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OrgId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// ORG-3: a shortcode's letters — ASCII letters and digits, upper case (a
/// code is read case-blind and kept upper case), 1 to [`CODE_MAX`]. ORG-9:
/// in hyphen-separated segments (`SYNAPSE-CC`) — [`is_code`]'s rule.
///
/// Not an identity: two shortcodes of one code live in two namespaces. So
/// it has no `Hash`, no `Ord` and no `Borrow<str>` — nothing can key a map
/// or a set by it (the module's examples hold that to the compiler) — and its `Display` is
/// its letters, never an address. It compares equal to another code of the
/// same letters, which is what a search over the letters asks; the store
/// compares shortcodes by [`ShortcodeId`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Code(String);

impl Code {
    /// `text` as a code — trimmed and upper-cased — or why it is not one.
    pub fn parse(text: &str) -> Result<Self, String> {
        let t = text.trim();
        if !is_code(t) {
            return Err(format!(
                "`{t}` is not a shortcode: ASCII letters and digits, as MENGER — or segments joined by single hyphens, \
                 as SYNAPSE-CC, none after the first all digits"
            ));
        }
        Ok(Self(t.to_ascii_uppercase()))
    }

    /// A code as stored — already upper case, never checked again.
    pub fn stored(text: &str) -> Self {
        Self(text.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// ORG-3 (owner: *"partial matches over just the shortcode"*): whether
    /// `query`, ASCII case folded, is a part of these letters — `the` is a
    /// part of THEME, wherever THEME is hosted.
    pub fn contains_folded(&self, query: &str) -> bool {
        let q = query.trim().to_ascii_uppercase();
        !q.is_empty() && self.0.contains(&q)
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Its letters, read as text (`.len()`, `.starts_with(…)`) — never a key:
/// a code has no `Hash` and no `Ord` of its own.
impl std::ops::Deref for Code {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

/// Its letters against written letters — what a row is shown or searched
/// by, never what a shortcode is keyed by.
impl PartialEq<str> for Code {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Code {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// ORG-3: the store's identity of one shortcode — minted by the store when
/// the shortcode is born, kept in its row, and never derived from what a
/// reader types (that is an [`Address`], resolved). For every shortcode
/// made before ORG-3 it is the code itself (`THEME`), so every table that
/// names a shortcode — the tickets' `epic` column, the derived tables, the
/// theme settings, the deployments — already holds it; a namesake made
/// after is `CODE.N` (`THEME.62`), which no code can be (a code holds no
/// `.`). A ticket's key is its shortcode's id, `-`, and its number, as it
/// was born.
///
/// `Ord` is its text's: today's order of shortcodes is unchanged, and a
/// namesake sorts right after the code it shares (`.` sorts before every
/// letter and digit).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShortcodeId(Arc<str>);

/// The empty id — no shortcode: what a row from a server before ORG-3
/// deserializes with, and nothing the store mints.
impl Default for ShortcodeId {
    fn default() -> Self {
        Self(Arc::from(""))
    }
}

impl ShortcodeId {
    /// The id the store minted, read back — from its table, a row, the wire.
    pub fn new(text: &str) -> Self {
        Self(Arc::from(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// ORG-3: the key a ticket of this shortcode born with number `seq`
    /// takes — `THEME-3`, `THEME.62-3`. Minted once, at birth: the key
    /// never moves with its shortcode afterwards.
    pub fn key(&self, seq: u32) -> String {
        format!("{}-{seq}", self.0)
    }
}

/// Its text — what a log line or a refusal names it by (never an address:
/// that is the registry's, `org:ROOT/…/SC`).
impl fmt::Display for ShortcodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for ShortcodeId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ShortcodeId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::new(&s))
    }
}

impl JsonSchema for ShortcodeId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ShortcodeId".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = String::json_schema(generator);
        schema.insert(
            "description".into(),
            "A shortcode's identity, minted by the store: its code for a shortcode made before ORG-3, CODE.N for a namesake made after"
                .into(),
        );
        schema
    }
}

/// ORG-3: where a shortcode is unique — its organization and the shortcode
/// it is hosted under (`None`: a root of its org). The host is a shortcode,
/// not a ticket (ORG-2 `namespace-host`): two THEMEs under DESIGN-1 and
/// DESIGN-7 would share one address, so they share one namespace. The
/// ticket a shortcode's root hangs under is still kept — it is where the
/// root sits in the tree — but it is not part of the namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Namespace {
    pub org: OrgId,
    pub host: Option<ShortcodeId>,
}

/// ORG-3: one shortcode — its identity, its namespace and its letters. No
/// two shortcodes share `(ns, code)`: every write that mints or moves one
/// checks it, and the database's index on `(org, ifnull(host, ''), code)`
/// holds it too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Shortcode {
    pub id: ShortcodeId,
    pub ns: Namespace,
    pub code: Code,
}

/// ACCESS-48: the shape a ticket's key and a ticket's recorded name share —
/// a transparent `String` newtype the store mints (its JSON the text, as it
/// always was; read back unchecked, as the store answered it), read as text
/// (`&id` where the store takes `&str`), compared with text and with each
/// other's kind, and stored as its text.
macro_rules! ticket_text {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// One the store holds, read back.
            pub fn new(text: impl Into<String>) -> Self {
                Self(text.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        /// ACCESS-8: its text, read as text, as [`Code`] is.
        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }

        impl PartialEq<$name> for String {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }

        impl PartialEq<$name> for &str {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }

        impl PartialEq<$name> for str {
            fn eq(&self, other: &$name) -> bool {
                self == other.0
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }

        /// ACCESS-8: stored as its text.
        #[cfg(feature = "server")]
        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }

        #[cfg(feature = "server")]
        impl rusqlite::types::FromSql for $name {
            fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                String::column_result(v).map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

ticket_text!(
    /// ORG-3: a ticket's key — the immutable, opaque identity a ticket keeps
    /// for good (ORG-6 finding 1): today's id text for today's tickets,
    /// [`ShortcodeId::key`] for a newborn. What every row that names a ticket
    /// names it by; an address resolves *to* one. Its JSON is the id string, as
    /// it always was.
    TicketId
);

ticket_text!(
    /// ACCESS-48: a ticket as a `blocked_by` / `unlocks` list records it —
    /// its key ([`TicketId`]) when it is a ticket, or, for a forward
    /// reference (a blocker not born yet) and a dangling one (a blocker
    /// deleted, or never born), the name as the write recorded it: the key
    /// it will take (`MNG-9`), or the address as written folded
    /// (`MENGER/THEME-12`), which a birth of that name settles into its key
    /// (`unlocks_changed`). Minted by the store alone
    /// (`Store::name_ticket`); the JSON is the text, as it always was.
    TicketName
);

impl From<TicketId> for TicketName {
    fn from(id: TicketId) -> Self {
        Self(id.0)
    }
}

impl From<&TicketId> for TicketName {
    fn from(id: &TicketId) -> Self {
        Self(id.0.clone())
    }
}

impl PartialEq<TicketId> for TicketName {
    fn eq(&self, other: &TicketId) -> bool {
        self.0 == other.0
    }
}

impl PartialEq<TicketName> for TicketId {
    fn eq(&self, other: &TicketName) -> bool {
        self.0 == other.0
    }
}

impl TicketName {
    /// The key this name is, read as one — for a name the store found to be
    /// a ticket's (a name that is no ticket answers `get` as `not_found`).
    pub fn as_key(&self) -> TicketId {
        TicketId(self.0.clone())
    }
}

/// What an [`Address`] names inside its shortcode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum At {
    /// The shortcode itself: `MENGER/THEME`.
    Shortcode,
    /// Its ticket of this number: `MENGER/THEME-3`.
    Seq(u32),
    /// WAVE-10: the ticket holding this path (`_MENGER/…`, exactly as
    /// written): `PLEXUS/_MENGER/board`.
    Path(String),
}

/// ORG-3: an address as a reader writes it — `[org:]ROOT/…/SC-N`, any
/// prefix of the hosts left out — parsed, not yet resolved. The store's
/// resolver turns one into a [`TicketId`] or a [`ShortcodeId`] in the
/// reader's reach: one match is the answer, several are `ambiguous` with
/// every candidate's full address, none is not found.
///
/// `hosts` are the hosting shortcodes named before the last, outermost
/// first: a short form may skip any of them (`MENGER/THEME-3` finds
/// `hypermemetic:MENGER/DESIGN/THEME-3`), but the ones it names appear in
/// order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub org: Option<OrgId>,
    pub hosts: Vec<Code>,
    pub code: Code,
    pub at: At,
}

impl Address {
    /// The one parser from text to an address (ORG-3; it replaces WAVE-10's
    /// `path_address`, which read `MENGER/THEME-3` as the path `THEME-3` in
    /// MENGER). An optional `org:`; then segments split on `/`; the first
    /// that begins with `_` begins a path, which runs to the end; before it,
    /// the last segment is `SC-N` (a ticket) or `SC` (a shortcode), and the
    /// ones before that are hosts. Codes fold to upper case; an org name is
    /// lower case or no org name. `Err` says what is wrong.
    pub fn parse(text: &str) -> Result<Self, String> {
        let t = text.trim();
        if t.is_empty() {
            return Err("an address names something: SC-N, ROOT/SC-N or org:ROOT/…/SC-N".into());
        }
        let (org, rest) = match t.split_once(':') {
            Some((o, rest)) => (Some(OrgId::new(o)?), rest),
            None => (None, t),
        };
        let segments: Vec<&str> = rest.split('/').collect();
        let path_at = segments.iter().position(|s| s.starts_with('_'));
        let (names, path) = match path_at {
            Some(i) => (&segments[..i], Some(segments[i..].join("/"))),
            None => (&segments[..], None),
        };
        let Some((last, hosts)) = names.split_last() else {
            return Err(format!("`{t}` names a path in no shortcode: write SHORTCODE/{}", rest));
        };
        let hosts = hosts.iter().map(|h| Code::parse(h)).collect::<Result<Vec<_>, _>>()?;
        if let Some(path) = path {
            if path.len() <= "_".len() || path.ends_with('/') {
                return Err(format!("`{t}`: a path names something after its `_`, as _MENGER/board"));
            }
            return Ok(Self { org, hosts, code: Code::parse(last)?, at: At::Path(path) });
        }
        // ORG-9: a code may hold hyphens; only the last `-N`, digits, is a
        // ticket's number — and no code ends in a segment of digits, so
        // `SYNAPSE-CC-1` is a ticket and `SYNAPSE-CC` a shortcode, one way.
        if let Some((code, n)) = last.rsplit_once('-') {
            if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
                let seq = n.parse::<u32>().map_err(|_| format!("`{t}`: {n} is past the last number a ticket can have"))?;
                return Ok(Self { org, hosts, code: Code::parse(code)?, at: At::Seq(seq) });
            }
            if !is_code(last.trim()) {
                return Err(format!("`{t}` is not an address: a ticket is SC-N, its number digits"));
            }
        }
        Ok(Self { org, hosts, code: Code::parse(last)?, at: At::Shortcode })
    }

    /// Whether this is the bare `SC-N` or `SC` a reader typed before ORG-3 —
    /// no org, no host: the form today's ids are pinned in (ORG-2 `pinned`).
    pub fn is_bare(&self) -> bool {
        self.org.is_none() && self.hosts.is_empty()
    }

    /// Whether it names a ticket (a number or a path) rather than a
    /// shortcode.
    pub fn names_ticket(&self) -> bool {
        !matches!(self.at, At::Shortcode)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(org) = &self.org {
            write!(f, "{org}:")?;
        }
        for h in &self.hosts {
            write!(f, "{h}/")?;
        }
        f.write_str(self.code.as_str())?;
        match &self.at {
            At::Shortcode => Ok(()),
            At::Seq(n) => write!(f, "-{n}"),
            At::Path(p) => write!(f, "/{p}"),
        }
    }
}

/// ORG-3: `text` as the store spells what it names — an address with its
/// codes upper-cased (its org and its path as written), anything else
/// upper-cased whole (a key the store minted, which is upper case). What a
/// refusal names a ticket by, and what replaces the old upper-casing of
/// a whole id (which would make `hypermemetic:` a code).
pub fn folded(text: &str) -> String {
    match Address::parse(text) {
        Ok(a) => a.to_string(),
        Err(_) => text.trim().to_ascii_uppercase(),
    }
}

/// ORG-3: a shortcode's full address from its chain of codes, outermost
/// first (its own last) — `hypermemetic:MENGER/THEME`.
pub fn shortcode_address(org: &OrgId, chain: &[&str]) -> String {
    format!("{org}:{}", chain.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(s: &str) -> Code {
        Code::parse(s).unwrap()
    }

    #[test]
    fn a_bare_id_is_a_code_and_a_number() {
        let a = Address::parse("theme-3").unwrap();
        assert_eq!(a, Address { org: None, hosts: vec![], code: code("THEME"), at: At::Seq(3) });
        assert!(a.is_bare() && a.names_ticket());
        assert_eq!(a.to_string(), "THEME-3");
    }

    #[test]
    fn a_full_address_names_its_org_and_every_host() {
        let a = Address::parse("hypermemetic:MENGER/DESIGN/THEME-3").unwrap();
        assert_eq!(a.org, Some(OrgId::hypermemetic()));
        assert_eq!(a.hosts, vec![code("MENGER"), code("DESIGN")]);
        assert_eq!((a.code.as_str(), &a.at), ("THEME", &At::Seq(3)));
        assert_eq!(a.to_string(), "hypermemetic:MENGER/DESIGN/THEME-3");
    }

    #[test]
    fn menger_slash_theme_3_is_a_ticket_not_a_path() {
        // ORG-6 finding 3: `path_address` read this as the path THEME-3 in MENGER.
        let a = Address::parse("MENGER/THEME-3").unwrap();
        assert_eq!((a.hosts.len(), a.code.as_str(), &a.at), (1, "THEME", &At::Seq(3)));
    }

    #[test]
    fn a_path_begins_at_its_underscore_with_an_org_or_without() {
        let a = Address::parse("PLEXUS/_MENGER/board").unwrap();
        assert_eq!((a.code.as_str(), &a.at), ("PLEXUS", &At::Path("_MENGER/board".into())));
        let a = Address::parse("hypermemetic:MENGER/PLEXUS/_MENGER/view/x").unwrap();
        assert_eq!((a.hosts.len(), a.code.as_str(), &a.at), (1, "PLEXUS", &At::Path("_MENGER/view/x".into())));
        assert!(Address::parse("_MENGER/board").is_err());
        assert!(Address::parse("PLEXUS/_").is_err());
    }

    #[test]
    fn a_shortcode_address_has_no_number() {
        let a = Address::parse("menger/theme").unwrap();
        assert_eq!((a.hosts, a.code.as_str(), a.at), (vec![code("MENGER")], "THEME", At::Shortcode));
    }

    #[test]
    fn what_is_no_address_says_why() {
        for bad in ["", "THE ME-3", "THEME--x", "THEME-x-", "Hypermemetic:MENGER-1", "MENGER//THEME-3", "THEME-", "THE.ME-3", "-3"] {
            assert!(Address::parse(bad).is_err(), "{bad:?} parsed");
        }
        assert!(Address::parse("THEME-99999999999").is_err());
        // ORG-9: `THEME-x` is no ticket — its last segment is not a number —
        // but it is a shortcode now, its letters in two segments.
        assert_eq!(Address::parse("THEME-x").unwrap().at, At::Shortcode);
    }

    #[test]
    fn a_code_may_hold_hyphens_but_only_the_last_digits_are_a_number() {
        // ORG-9: the rule, and the forms it refuses.
        for ok in ["MENGER", "synapse-cc", "A-B-C", "A1-B2", "123", "X-1B", "ab-c9"] {
            assert!(is_code(ok) && Code::parse(ok).is_ok(), "{ok:?} refused");
        }
        for bad in ["", "A-", "-A", "A--B", "A-1", "A-B-2", "A_B", "A B", "A.B", "-", &"A".repeat(CODE_MAX + 1)] {
            assert!(!is_code(bad) && Code::parse(bad).is_err(), "{bad:?} passed");
        }
        assert_eq!(code("synapse-cc").as_str(), "SYNAPSE-CC");

        let a = Address::parse("synapse-cc-1").unwrap();
        assert_eq!((a.code.as_str(), &a.at), ("SYNAPSE-CC", &At::Seq(1)));
        assert_eq!(a.to_string(), "SYNAPSE-CC-1");
        let a = Address::parse("SYNAPSE-CC").unwrap();
        assert_eq!((a.code.as_str(), &a.at), ("SYNAPSE-CC", &At::Shortcode));
        let a = Address::parse("hypermemetic:PLEXUS/SYNAPSE/SYNAPSE-CC-12").unwrap();
        assert_eq!((a.hosts.len(), a.code.as_str(), &a.at), (2, "SYNAPSE-CC", &At::Seq(12)));
        let a = Address::parse("SYNAPSE-CC/HUB-GEN-3").unwrap();
        assert_eq!((a.hosts, a.code.as_str(), a.at), (vec![code("SYNAPSE-CC")], "HUB-GEN", At::Seq(3)));
        let a = Address::parse("SYNAPSE-CC/_MENGER/board").unwrap();
        assert_eq!(a.code.as_str(), "SYNAPSE-CC");
        // `A-1` is ticket 1 of A, never a shortcode; `A-1-2` is no address.
        assert_eq!(Address::parse("A-1").unwrap().at, At::Seq(1));
        for bad in ["A--B-1", "A-B--1", "-A-1", "A-1-2", "A-1/B-2", "A--B", "A-"] {
            assert!(Address::parse(bad).is_err(), "{bad:?} parsed");
        }
        // Today's ids still read as they did.
        assert_eq!(Address::parse("MNG-1").unwrap().code.as_str(), "MNG");
    }

    #[test]
    fn a_key_splits_at_its_last_hyphen() {
        assert_eq!(split_key("SYNAPSE-CC-1"), Some(("SYNAPSE-CC", 1)));
        assert_eq!(split_key("MNG-12"), Some(("MNG", 12)));
        assert_eq!(split_key("THEME.62-3"), Some(("THEME.62", 3)));
        assert_eq!(split_key("SYNAPSE-CC.2-3"), Some(("SYNAPSE-CC.2", 3)));
        for bad in ["MNG", "MNG-", "-1", "A--B-1", "A-1-2", "MNG-x", "THEME.-3", "MNG-99999999999"] {
            assert_eq!(split_key(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn an_org_name_is_lower_case_letters_digits_and_dashes() {
        assert!(OrgId::new("acme-2").is_ok());
        for bad in ["", "Acme", "-acme", "acme-", "ac me", "ac_me"] {
            assert!(OrgId::new(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_shortcode_id_mints_the_key_and_sorts_a_namesake_beside_its_code() {
        let (plain, namesake, next) = (ShortcodeId::new("THEME"), ShortcodeId::new("THEME.62"), ShortcodeId::new("THEME2"));
        assert_eq!((plain.key(3), namesake.key(3)), ("THEME-3".to_string(), "THEME.62-3".to_string()));
        assert!(plain < namesake && namesake < next);
        assert_eq!(serde_json::to_string(&namesake).unwrap(), "\"THEME.62\"");
    }

    #[test]
    fn a_code_matches_its_letters_in_part() {
        assert!(code("THEME").contains_folded("the"));
        assert!(code("THEME").contains_folded("EM"));
        assert!(!code("THEME").contains_folded("x"));
        assert!(!code("THEME").contains_folded(" "));
    }
}

/// Test-only conveniences: a literal as a code or an id, and a comparison
/// with one — so a test can write `"A".into()`. Never in a build that
/// serves: there, text becomes an identity only through the resolver.
#[cfg(test)]
mod literals {
    use super::{Code, ShortcodeId, TicketId, TicketName};

    impl From<&str> for TicketName {
        fn from(s: &str) -> Self {
            TicketName::new(s)
        }
    }
    impl From<String> for TicketName {
        fn from(s: String) -> Self {
            TicketName::new(s)
        }
    }

    impl From<&str> for TicketId {
        fn from(s: &str) -> Self {
            TicketId::new(s)
        }
    }
    impl From<String> for TicketId {
        fn from(s: String) -> Self {
            TicketId::new(s)
        }
    }
    impl From<&String> for TicketId {
        fn from(s: &String) -> Self {
            TicketId::new(s.as_str())
        }
    }

    impl From<&str> for Code {
        fn from(s: &str) -> Self {
            Code::stored(s)
        }
    }
    impl From<String> for Code {
        fn from(s: String) -> Self {
            Code::stored(&s)
        }
    }
    impl From<&str> for ShortcodeId {
        fn from(s: &str) -> Self {
            ShortcodeId::new(s)
        }
    }
    impl From<String> for ShortcodeId {
        fn from(s: String) -> Self {
            ShortcodeId::new(&s)
        }
    }
    impl From<&String> for ShortcodeId {
        fn from(s: &String) -> Self {
            ShortcodeId::new(s)
        }
    }
    impl From<&ShortcodeId> for ShortcodeId {
        fn from(s: &ShortcodeId) -> Self {
            s.clone()
        }
    }
    impl From<Code> for ShortcodeId {
        fn from(s: Code) -> Self {
            ShortcodeId::new(s.as_str())
        }
    }
    impl From<&Code> for ShortcodeId {
        fn from(s: &Code) -> Self {
            ShortcodeId::new(s.as_str())
        }
    }
    impl PartialEq<String> for Code {
        fn eq(&self, o: &String) -> bool {
            self.as_str() == o
        }
    }
    impl PartialEq<str> for ShortcodeId {
        fn eq(&self, o: &str) -> bool {
            self.as_str() == o
        }
    }
    impl PartialEq<&str> for ShortcodeId {
        fn eq(&self, o: &&str) -> bool {
            self.as_str() == *o
        }
    }
    impl PartialEq<String> for ShortcodeId {
        fn eq(&self, o: &String) -> bool {
            self.as_str() == o
        }
    }
}
