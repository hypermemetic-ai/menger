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
            crate::address::split_sub(n).is_some().then_some(id)?
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
    let shape = "SC-N, ROOT/SC-N or org:ROOT/…/SC-N — or a path address, SHORTCODE/_MENGER/… — or a uid, 7k3mq2xa, bare or in a link";
    // TICKETS-35: a uid — bare, or the app's path or link to it — names its
    // ticket; kept as the uid alone.
    if let Some(uid) = super::uid_named(t) {
        return Ok(uid.into_string());
    }
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
    /// its codes folded to upper case — `mng-1` is `MNG-1`. TICKETS-35: or
    /// its uid (`7k3mq2xa`, any case), bare or in the app's link to it
    /// (`/t/7k3mq2xa/…`, `https://app.menger.sh/t/7k3mq2xa/…`), kept as the
    /// uid alone. The store resolves it to the one ticket it names in the
    /// caller's reach.
    TicketRef,
    ticket_ref
);

impl TicketRef {
    /// TICKETS-35: the uid it names its ticket by, when it is one.
    pub fn uid(&self) -> Option<super::Uid> {
        self.0.parse().ok()
    }

    /// The address it is written as — `None` for a namesake's minted key,
    /// which is no address but a key, and (TICKETS-35) for a uid.
    pub fn address(&self) -> Option<Address> {
        match self.uid() {
            Some(_) => None,
            None => Address::parse(&self.0).ok(),
        }
    }

    /// Whether it names a ticket by its path (`SHORTCODE/_MENGER/…`).
    pub fn is_path(&self) -> bool {
        self.address().is_some_and(|a| matches!(a.at, crate::address::At::Path(_)))
    }
}

/// TICKETS-40: the word for no parent — what `--parent none` writes.
pub const NO_PARENT: &str = "none";

fn parent_ref(text: &str) -> Result<String, String> {
    match text.trim() {
        t if t.eq_ignore_ascii_case(NO_PARENT) => Ok(NO_PARENT.to_string()),
        t => ticket_ref(t).map_err(|e| format!("{e} — or none, to make it a root")),
    }
}

text_type!(
    /// TICKETS-40: a parent as `set_parent` and `update` take it — a ticket
    /// by any name [`TicketRef`] takes, or `none` (any case): no parent, the
    /// ticket made a root. `none` names no ticket — a ticket's name has a
    /// number, a uid eight characters and no `o` — so it shadows none.
    ParentRef,
    parent_ref
);

/// A ticket is a parent (`update_many`'s, which takes no `none`).
impl From<TicketRef> for ParentRef {
    fn from(r: TicketRef) -> Self {
        Self(r.0)
    }
}

impl ParentRef {
    /// The ticket it names — `None` for `none`.
    pub fn ticket(&self) -> Option<&str> {
        (self.0 != NO_PARENT).then_some(self.0.as_str())
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

/// FILTER-4: the fields `facets` counts, in the order it answers them — each
/// as the language spells it. FILTER-21: `menger doc` after the other flags.
/// MENGER-12: then `label` — a count per label held, in name order (a ticket with two labels is counted under each) — before
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

/// APP-86: the inputs a queue entry names — `--input a --input b` (or
/// `a,b`), in the order asked. A list on the wire (its schema `InputId`'s
/// array, so synapse repeats the flag), and — for every caller before
/// APP-86 — a bare string reads as the list of that one. Each is an
/// [`InputId`]; an empty list is none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct InputIds(Vec<InputId>);

impl InputIds {
    /// Each, in order.
    pub fn as_slice(&self) -> &[InputId] {
        &self.0
    }

    /// Each as text, in order.
    pub fn into_strings(self) -> Vec<String> {
        self.0.into_iter().map(InputId::into_string).collect()
    }

    /// Whether it names none.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<InputId>> for InputIds {
    fn from(ids: Vec<InputId>) -> Self {
        Self(ids)
    }
}

impl From<InputId> for InputIds {
    fn from(id: InputId) -> Self {
        Self(vec![id])
    }
}

impl<'de> serde::Deserialize<'de> for InputIds {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum OneOrMany {
            One(String),
            Many(Vec<String>),
        }
        let texts = match OneOrMany::deserialize(d).map_err(|_| serde::de::Error::custom("an input's id, or a list of them (`--input a --input b`)"))? {
            OneOrMany::One(t) => vec![t],
            OneOrMany::Many(v) => v,
        };
        texts.into_iter().map(InputId::try_from).collect::<Result<Vec<_>, _>>().map(Self).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for InputIds {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "InputIds".into()
    }
    /// Inlined where it is used, as `Vec<InputId>` is: the generators read
    /// `InputId[]`.
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let item = g.subschema_for::<InputId>();
        schemars::json_schema!({
            "type": "array",
            "description": "Inputs on the ticket, by id, in order (a bare id reads as one)",
            "items": item
        })
    }
}
