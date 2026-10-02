//! SDLC-9: what a commit reference is, and how many a ticket holds.
//!
//! The grammar is the ticket's, exactly: a hexadecimal object name of 7 to
//! 64 characters — git's shortest default abbreviation up to a whole
//! SHA-256 name — optionally prefixed by a repository and `@`, the
//! repository being letters, digits, `.`, `_` and `-`. Nothing else is
//! accepted, and nothing is trimmed: an entry is a reference or it is not,
//! and the refusal names it as given.
//!
//! What is stored is the canonical text — the hash lowercased, the
//! repository as given — and that text is what two references are compared
//! by. The server holds no repository, so it cannot know that `77df9a0`
//! and `77df9a0c…` name one commit, or that a repository was renamed: they
//! are different references, and the caller links the form it wants kept.

/// At most this many commits on one ticket. SCALE-4's lesson: a list on a
/// row grows the row, every write of it, and every event that carries it,
/// so it is bounded. One ticket's work spans a few commits — its build and
/// an integration or two, each maybe in another repository — never
/// hundreds.
pub const MAX_PER_TICKET: usize = 256;

/// The shortest and longest object name a reference may carry.
const HASH_LEN: std::ops::RangeInclusive<usize> = 7..=64;

/// `entry` as a commit reference, in the form it is stored and compared in;
/// `None` when it is not one.
pub(super) fn reference(entry: &str) -> Option<String> {
    let (repo, hash) = match entry.split_once('@') {
        Some((repo, hash)) => (Some(repo), hash),
        None => (None, entry),
    };
    let repo_ok = repo.is_none_or(|r| {
        !r.is_empty() && r.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    });
    // A second `@` is in `hash`, which no hex digit is.
    let hash_ok = HASH_LEN.contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit());
    (repo_ok && hash_ok).then(|| match repo {
        Some(repo) => format!("{repo}@{}", hash.to_ascii_lowercase()),
        None => hash.to_ascii_lowercase(),
    })
}
