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

/// `entry` as a commit reference, in the form it is stored and compared in;
/// `None` when it is not one — [`crate::vocab::CommitRef`]'s rule, the one
/// (ACCESS-8 moved it to the wasm-safe half).
pub(super) fn reference(entry: &str) -> Option<String> {
    entry.parse::<crate::vocab::CommitRef>().ok().map(crate::vocab::CommitRef::into_string)
}
