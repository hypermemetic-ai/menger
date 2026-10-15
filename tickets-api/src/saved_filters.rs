//! FILTER-36: where a saved filter lives, and how a name finds it — moved
//! here from the app's `saved.rs` (FILTER-6) so the server, which resolves
//! `saved "Name"` (FILTER-36), and the app, which writes the docs, keep one
//! rule.
//!
//! A saved filter is a doc in the shortcode [`EPIC`] at the path
//! `_MENGER/view/<slug>` ([`PATH_PREFIX`]), under the collection [`COLLECTION`]
//! (whose body lists the names, in order), titled with its name, whose body
//! holds the filter as canonical text on one line: `filter: <text>`
//! ([`FILTER_LINE`], [`filter_line`]). A name is matched by its [`slug`] —
//! `Open Builds` is `open builds` is `open-builds`.
//!
//! Wasm-safe: nothing but `std`.

/// The collection: the doc whose body lists the saved filters.
pub const COLLECTION: &str = "VIEW-1";
/// The collection's shortcode — every saved filter's doc is in it.
pub const EPIC: &str = "VIEW";
/// Where a saved filter's doc sits in the collection's shortcode.
pub const PATH_PREFIX: &str = "_MENGER/view/";
/// The line of a saved filter's body that holds it.
pub const FILTER_LINE: &str = "filter:";
/// The longest slug.
pub const SLUG_MAX: usize = 64;

/// MENGER-43: the saved filter every reader has before saving one — "Needs
/// you", the owner's queue, what asks and what is labelled so — and the
/// text it holds until a doc of that name is saved: what `saved "Needs
/// you"` means meanwhile, on the server as in the app.
pub const NEEDS_YOU: (&str, &str) = ("Needs you", "queued or asking or label needs-you");

/// The text a name means while no doc holds it ([`NEEDS_YOU`]'s), by slug.
pub fn default_text(name: &str) -> Option<&'static str> {
    (slug(name) == slug(NEEDS_YOU.0)).then_some(NEEDS_YOU.1)
}

/// A name as a path's last part: lower case, letters and digits, every run
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

/// A saved filter's path in the collection's shortcode.
pub fn path_of(id: &str) -> String {
    format!("{PATH_PREFIX}{id}")
}

/// The slug a saved filter's path ends in — `None` for a path that is not
/// one.
pub fn path_slug(path: &str) -> Option<&str> {
    path.strip_prefix(PATH_PREFIX).filter(|s| !s.is_empty())
}

/// The filter text a saved filter's body holds: its first `filter:` line,
/// trimmed — unread (the language reads it).
pub fn filter_line(body: &str) -> Option<&str> {
    body.lines().find_map(|l| l.trim().strip_prefix(FILTER_LINE)).map(str::trim)
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
        assert_eq!(path_slug("_MENGER/view/open-builds"), Some("open-builds"));
        assert_eq!(path_slug("_MENGER/wave/x"), None);
        assert_eq!(filter_line("filter: kind build\n"), Some("kind build"));
        assert_eq!(filter_line("intro\n  filter:   status Ready  \nfilter: x"), Some("status Ready"));
        assert_eq!(filter_line("nothing"), None);
        assert_eq!(default_text("needs YOU"), Some("queued or asking or label needs-you"));
        assert_eq!(default_text("Plexus-Ready"), None);
    }
}
