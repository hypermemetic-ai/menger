//! FILTER-46: what a saved filter is, and how a name finds it — shared by the
//! server, which resolves `saved "Name"` (FILTER-36) from its `saved_filters`
//! table, and the app, which lists, saves and names them through
//! `tickets filters`.
//!
//! A saved filter is a row of its own — a name, the filter as canonical text,
//! who saved it and when ([`SavedFilter`]) — never a ticket. A name is matched
//! by its [`slug`]: `Open Builds` is `open builds` is `open-builds`.
//!
//! Wasm-safe: nothing but `std` and serde.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The longest slug.
pub const SLUG_MAX: usize = 64;
/// The longest name, in characters.
pub const NAME_MAX: usize = 64;

/// One saved filter, as `tickets filters` answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SavedFilter {
    /// Its name, as saved.
    pub name: String,
    /// The filter, as canonical text (FILTER-3's language).
    pub filter: String,
    /// How many tickets in the caller's reach it admits now (`list`, `get` and `save`; omitted elsewhere).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    /// Who saved it (the principal, as the server names it).
    pub saved_by: String,
    /// When it was first saved, unix seconds.
    pub saved_at: i64,
    /// When it was last written (a replace or a rename), unix seconds.
    pub updated_at: i64,
}

/// MENGER-43: the saved filter every reader has before saving one — "Needs
/// you", the owner's queue, what asks and what is labelled so — and the
/// text it holds until a filter of that name is saved: what `saved "Needs
/// you"` means meanwhile, on the server as in the app.
pub const NEEDS_YOU: (&str, &str) = ("Needs you", "label needs-you");

/// The text a name means while none is saved ([`NEEDS_YOU`]'s), by slug.
pub fn default_text(name: &str) -> Option<&'static str> {
    (slug(name) == slug(NEEDS_YOU.0)).then_some(NEEDS_YOU.1)
}

/// A name as a key: lower case, letters and digits, every run
/// of anything else one `-`, none at either end; at most [`SLUG_MAX`]
/// characters. Empty when the name has no letter or digit.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(ch);
        } else {
            gap = true;
        }
    }
    if out.chars().count() > SLUG_MAX {
        out = out.chars().take(SLUG_MAX).collect();
        while out.ends_with('-') {
            out.pop();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_becomes_a_slug() {
        assert_eq!(slug("open builds"), "open-builds");
        assert_eq!(slug("Open Builds"), "open-builds");
        assert_eq!(slug("Plexus-Ready"), "plexus-ready");
        assert_eq!(slug("Needs you"), "needs-you");
        assert_eq!(slug("Über Läufe"), "über-läufe");
        assert_eq!(slug("--"), "");
        assert_eq!(default_text("needs YOU"), Some("label needs-you"));
        assert_eq!(default_text("Plexus-Ready"), None);
    }
}
