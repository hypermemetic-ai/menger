//! ACCESS-8: what names a ticket, a shortcode and the things on a ticket —
//! each a type with one rule. A name as a reader writes it is resolved by
//! the store (ORG-3's resolver); what these hold is the name *as written*,
//! checked for its spelling and folded to the store's (`mng-1` is `MNG-1`).

use super::{not_a, too_long, word};
use crate::address::{folded, Address};

/// The longest name of a ticket or a shortcode a call may write — an
/// address with every host spelled out, and room to spare.
pub const ADDRESS_MAX: usize = 512;

/// A key the store minted for a namesake (ORG-3), `CODE.N-SEQ` — or, with
/// `seq` false, a namesake shortcode's identity, `CODE.N` — upper-cased.
fn minted(text: &str, seq: bool) -> Option<String> {
    let up = text.to_ascii_uppercase();
    let id = match seq {
        true => {
            let (id, n) = up.rsplit_once('-')?;
            (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && n.parse::<u32>().is_ok()).then_some(id)?
        }
        false => up.as_str(),
    };
    let (code, n) = id.split_once('.')?;
    // ORG-9: the code by the one rule, hyphens and all (`SYNAPSE-CC.2-3`).
    let ok = crate::address::is_code(code)
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit());
    ok.then_some(up)
}

/// The text of a name, trimmed and held to [`ADDRESS_MAX`].
fn bounded<'a>(text: &'a str, what: &str) -> Result<&'a str, String> {
    let t = text.trim();
    if t.len() > ADDRESS_MAX {
        return Err(too_long(what, t.len(), ADDRESS_MAX));
    }
    Ok(t)
}

fn ticket_ref(text: &str) -> Result<String, String> {
    let t = bounded(text, "a ticket's name")?;
    let shape = "SC-N, ROOT/SC-N or org:ROOT/…/SC-N — or a path address, SHORTCODE/_MENGER/…";
    match Address::parse(t) {
        Ok(a) if a.names_ticket() => Ok(a.to_string()),
        Ok(_) => Err(not_a(t, "a ticket (it names a shortcode)", shape)),
        Err(_) => minted(t, true).ok_or_else(|| not_a(t, "a ticket", shape)),
    }
}

text_type!(
    /// A ticket as a call names it (ORG-3): its key (`MNG-1`, a namesake's
    /// `THEME.62-3`), any short form of its address (`MENGER/THEME-3`,
    /// `hypermemetic:THEME-3`) or a path address (`PLEXUS/_MENGER/board`),
    /// its codes folded to upper case — `mng-1` is `MNG-1`. The store
    /// resolves it to the one ticket it names in the caller's reach.
    TicketRef,
    ticket_ref
);

impl TicketRef {
    /// The address it is written as — `None` for a namesake's minted key,
    /// which is no address but a key.
    pub fn address(&self) -> Option<Address> {
        Address::parse(&self.0).ok()
    }

    /// Whether it names a ticket by its path (`SHORTCODE/_MENGER/…`).
    pub fn is_path(&self) -> bool {
        self.address().is_some_and(|a| matches!(a.at, crate::address::At::Path(_)))
    }
}

fn shortcode_ref(text: &str) -> Result<String, String> {
    let t = bounded(text, "a shortcode's name")?;
    let shape = "letters and digits, as MENGER or SYNAPSE-CC — or its address, org:ROOT/…/SC";
    match Address::parse(t) {
        Ok(a) if !a.names_ticket() => Ok(a.to_string()),
        Ok(_) => Err(not_a(t, "a shortcode (it names a ticket)", shape)),
        Err(_) => minted(t, false).ok_or_else(|| not_a(t, "a shortcode", shape)),
    }
}

text_type!(
    /// A shortcode as a call names it (ORG-3): its code (`MENGER`, any
    /// case), any short form of its address (`PLEXUS/THEME`, `acme:MENGER`)
    /// or a namesake's identity (`THEME.62`) — the codes folded to upper
    /// case. What `--epic` takes; the store resolves it.
    ShortcodeRef,
    shortcode_ref
);

fn under(text: &str) -> Result<String, String> {
    ticket_ref(text).or_else(|_| shortcode_ref(text)).map_err(|_| {
        not_a(text.trim(), "a ticket or a shortcode", "MENGER-2, FILTER, or an address of either")
    })
}

text_type!(
    /// A ticket or a shortcode — `comments.list`'s `under`: a ticket (it
    /// and its descendants) or a shortcode (it and every shortcode hosted
    /// under it), by any name [`TicketRef`] or [`ShortcodeRef`] takes.
    Under,
    under
);

text_type!(
    /// ACCESS-48: a ticket or a shortcode as a call named it, echoed back —
    /// `not_found`'s and `ambiguous`'s `id`: what was asked, by any name
    /// [`TicketRef`] or [`ShortcodeRef`] takes (the rule [`Under`] reads).
    Asked,
    under
);

impl From<TicketRef> for Asked {
    fn from(r: TicketRef) -> Self {
        Self(r.0)
    }
}

impl From<&TicketRef> for Asked {
    fn from(r: &TicketRef) -> Self {
        Self(r.0.clone())
    }
}

impl From<ShortcodeRef> for Asked {
    fn from(r: ShortcodeRef) -> Self {
        Self(r.0)
    }
}

impl From<crate::TicketId> for Asked {
    fn from(id: crate::TicketId) -> Self {
        Self(id.into_string())
    }
}

fn cursor(text: &str) -> Result<String, String> {
    word(text, "a cursor", "the `next` a page answered, e.g. MNG-9", ADDRESS_MAX).map(folded)
}

text_type!(
    /// Where a page starts: strictly after the ticket a `page` event named
    /// as its `next` (`MNG-9`). Opaque to the caller — today a ticket's
    /// key, folded to upper case as the store keeps it.
    Cursor,
    cursor
);

fn menger_path(text: &str) -> Result<String, String> {
    match crate::types::is_menger_path(text) && text.len() <= ADDRESS_MAX && !text.chars().any(char::is_control) {
        true => Ok(text.to_string()),
        false => Err(not_a(text, "a path", "_MENGER/ and a name after it, as _MENGER/board")),
    }
}

text_type!(
    /// WAVE-10: a path inside a shortcode, `_MENGER/…`, exactly as written
    /// — what `create --path` gives a doc, and what `SHORTCODE/_MENGER/…`
    /// finds it by.
    MengerPath,
    menger_path
);

fn commit_ref(text: &str) -> Result<String, String> {
    let (repo, hash) = match text.split_once('@') {
        Some((repo, hash)) => (Some(repo), hash),
        None => (None, text),
    };
    let repo_ok = repo.is_none_or(|r| {
        !r.is_empty() && r.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    });
    // A second `@` is in `hash`, which no hex digit is.
    let hash_ok = (7..=64).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit());
    match (repo_ok && hash_ok, repo) {
        (true, Some(repo)) => Ok(format!("{repo}@{}", hash.to_ascii_lowercase())),
        (true, None) => Ok(hash.to_ascii_lowercase()),
        (false, _) => Err(not_a(text, "a commit reference", "a hex object name of 7 to 64 characters, optionally repo@ before it")),
    }
}

text_type!(
    /// SDLC-9: a commit — a hexadecimal object name of 7 to 64 characters
    /// (kept lower case), optionally a repository and `@` before it
    /// (`77df9a0`, `plexus-macros@4f7114a`). Nothing is trimmed: an entry is
    /// a reference or it is not. Compared as its text — the server holds
    /// no repository (moved here from `store::commits::reference`).
    CommitRef,
    commit_ref
);

impl CommitRef {
    /// The repository it names, if it names one.
    pub fn repo(&self) -> Option<&str> {
        self.0.split_once('@').map(|(r, _)| r)
    }

    /// The object name, lower case.
    pub fn hash(&self) -> &str {
        self.0.split_once('@').map_or(&self.0, |(_, h)| h)
    }
}

text_type!(
    /// MENGER-12: a label — ASCII letters, digits and `-`, 1 to 32, kept
    /// lower case (`Needs-You` is `needs-you`): [`crate::labels::normalize`].
    Label,
    crate::labels::normalize
);

fn input_id(text: &str) -> Result<String, String> {
    let t = text.trim();
    match !t.is_empty() && t.len() <= 256 && !t.chars().any(char::is_control) {
        true => Ok(t.to_string()),
        false => Err(not_a(t, "an input's id", "the `id` of an <m-decision> or <m-input> in the ticket's body, e.g. landing")),
    }
}

text_type!(
    /// INPUT-1: an input's id — the `id` attribute of its `<m-decision>` or
    /// `<m-input>`, trimmed, unique in its ticket.
    InputId,
    input_id
);

fn component_name(text: &str) -> Result<String, String> {
    let t = text.trim();
    match crate::component_docs::is_component_name(t) {
        true => Ok(t.to_string()),
        false => Err(not_a(t, "a component name", "lower case, with a `-`, as m-chart")),
    }
}

text_type!(
    /// COMP-3: a component's tag — lower case, starting with a letter, with
    /// a `-` in it, not one HTML reserves (`m-chart`).
    ComponentName,
    component_name
);

fn referrer(text: &str) -> Result<String, String> {
    component_name(text).or_else(|_| ticket_ref(text)).map_err(|_| not_a(text.trim(), "a referrer", "a ticket (MNG-1) or a component (m-chart)"))
}

text_type!(
    /// COMP-3: what refers to a component — a ticket whose body uses it
    /// (`MNG-1`) or a component whose source does (`m-chart`); told apart
    /// by [`Referrer::is_component`].
    Referrer,
    referrer
);

impl Referrer {
    /// Whether it is a component (lower case) rather than a ticket.
    pub fn is_component(&self) -> bool {
        self.0.starts_with(|c: char| c.is_ascii_lowercase())
    }
}

fn route(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.len() > crate::comments::ANCHOR_FIELD_MAX {
        return Err(too_long("a route", t.len(), crate::comments::ANCHOR_FIELD_MAX));
    }
    match t.starts_with("#/") {
        true => Ok(t.to_string()),
        false => Err(not_a(t, "an app page", "a route begins #/ (e.g. #/ticket/DEMAND-2, or #/ for the board)")),
    }
}

text_type!(
    /// COMMENT-2: an app page, as its route — `#/ticket/DEMAND-2`, `#/` (the
    /// board) — always beginning `#/`.
    Route,
    route
);

fn anchor_path(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.len() > crate::comments::ANCHOR_FIELD_MAX {
        return Err(too_long("an anchor's path", t.len(), crate::comments::ANCHOR_FIELD_MAX));
    }
    match t.is_empty() {
        false => Ok(t.to_string()),
        true => Err(not_a(t, "an anchor's path", "where on the page, e.g. [data-tag=\"ticket DEMAND-1\"]")),
    }
}

text_type!(
    /// COMMENT-2: where on a page an anchor is — the element's path,
    /// `[data-tag="ticket DEMAND-1"]`; `comments.list --path` matches a
    /// prefix of it.
    AnchorPath,
    anchor_path
);

/// FILTER-4: the fields `facets` counts, in the order it answers them — each
/// as the language spells it. FILTER-21: `menger doc` after the other flags.
/// MENGER-12: `asking` after it, then `label` — a count per label held, in
/// name order (a ticket with two labels is counted under each) — before
/// `in`. (Moved here from `store::filtered` by ACCESS-8, so the wire's
/// [`FacetField`] is checked against the store's own list.)
pub const FACET_FIELDS: [&str; 11] = [
    "status",
    "category",
    "kind",
    "severity",
    "blocked",
    "ready",
    "has commits",
    "menger doc",
    "asking",
    "label",
    "in",
];

fn facet_field(text: &str) -> Result<String, String> {
    let t = text.trim();
    match FACET_FIELDS.contains(&t) {
        true => Ok(t.to_string()),
        false => Err(not_a(t, "a facet", &format!("one of {}", FACET_FIELDS.join(", ")))),
    }
}

text_type!(
    /// FILTER-4: a field `facets` counts by — one of [`FACET_FIELDS`].
    FacetField,
    facet_field
);

text_type!(
    /// DEPLOY-10: whose environment a deployment is — a shortcode's code,
    /// any case on the way in, kept upper case (`menger` is MENGER). The
    /// one shortcode rule, [`crate::address::Code::parse`]'s: [`crate::deployments::project_of`].
    Project,
    crate::deployments::project_of
);

text_type!(
    /// DEPLOY-10: an environment's name — lower case, `[a-z0-9._/-]`,
    /// starting with a letter or a digit, at most 64 bytes (`prod`,
    /// `pr-412`, `review/pr-412`): [`crate::deployments::environment_of`].
    EnvironmentName,
    crate::deployments::environment_of
);

fn release(text: &str) -> Result<String, String> {
    word(text, "a release", "the artifact's name, one word", 256).map(str::to_string)
}

text_type!(
    /// DEPLOY-10: a release — the artifact's name (`20261012T220402Z-235ff62`),
    /// one word, at most 256 bytes.
    Release,
    release
);

text_type!(
    /// THEME-8: a theme's name — lower case letters, digits and `-`,
    /// starting with a letter, at most 40 (`ocean`): [`crate::themes::name_of`].
    ThemeName,
    crate::themes::name_of
);

fn theme_scope(text: &str) -> Result<String, String> {
    let t = text.trim();
    match t.eq_ignore_ascii_case(crate::themes::SITE) {
        true => Ok(crate::themes::SITE.to_string()),
        false => shortcode_ref(t).map_err(|_| not_a(t, "a theme's scope", "site, or a shortcode (e.g. PANTRY)")),
    }
}

text_type!(
    /// THEME-4: where a theme is adopted — `site`, or a shortcode by any
    /// name [`ShortcodeRef`] takes.
    ThemeScope,
    theme_scope
);

impl ThemeScope {
    /// Whether it is the site.
    pub fn is_site(&self) -> bool {
        self.0 == crate::themes::SITE
    }
}

/// The shape of a record's number: a transparent `u64` — any number is one
/// (whether it names a record is the store's answer), so `new` is public;
/// the type keeps a comment's number from being passed as a note's.
macro_rules! number_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(n: u64) -> Self {
                Self(n)
            }

            /// Its number.
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        /// Its number against a number — `comment.id == 1`.
        impl PartialEq<u64> for $name {
            fn eq(&self, other: &u64) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<$name> for u64 {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }
    };
}

number_type!(
    /// COMMENT-2: a comment's number, from 1 in the order kept.
    CommentId
);

number_type!(
    /// NOTE-2: a note's number, from 1 in the order kept.
    NoteId
);

number_type!(
    /// DEPLOY-11: a deployment's number, from 1 in the order recorded.
    DeploymentId
);

number_type!(
    /// CHAT-3: a chat's number, from 1 in the order opened.
    ChatId
);

number_type!(
    /// CHAT-3: a chat message's number, from 1 across every chat in the
    /// order posted — a later message's is always greater.
    MessageId
);

fn token_name(text: &str) -> Result<String, String> {
    let t = text.trim();
    let name = t.trim_start_matches('-');
    let ok = t.len() - name.len() <= 2
        && !name.is_empty()
        && t.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    match ok {
        true => Ok(t.to_string()),
        false => Err(not_a(t, "a token's name", "lower case, as --accent or accent")),
    }
}

text_type!(
    /// THEME-8: a token a theme overrides, by its name — `--accent`, or
    /// `accent`. Whether the contract holds it is the store's refusal,
    /// naming every one it does not.
    TokenName,
    token_name
);

fn token_value(text: &str) -> Result<String, String> {
    match crate::themes::value_ok(text) {
        true => Ok(text.trim().to_string()),
        false => Err(not_a(
            text.trim(),
            "a token's value",
            &format!("not blank, at most {} bytes, no < > {{ }} ; \\ or line break", crate::themes::VALUE_MAX),
        )),
    }
}

text_type!(
    /// THEME-8: what a theme sets a token to (`#00796b`) — not blank, at
    /// most 200 bytes, nothing that could end its declaration:
    /// [`crate::themes::value_ok`].
    TokenValue,
    token_value
);
