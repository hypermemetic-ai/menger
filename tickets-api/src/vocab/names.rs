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

/// FILTER-4: the fields `facets` counts, in the order it answers them — each
/// as the language spells it. FILTER-21: `menger doc` after the other flags.
/// MENGER-12: then `label` — a count per label held, in name order (a ticket with two labels is counted under each) — before
/// `in`. (Moved here from `store::filtered` by ACCESS-8, so the wire's
/// [`FacetField`] is checked against the store's own list.)
pub const FACET_FIELDS: [&str; 10] = [
    "status",
    "category",
    "kind",
    "severity",
    "blocked",
    "ready",
    "has commits",
    "menger doc",
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
