//! ACCESS-29 (ACCESS-4 L5): the sizes a request is held to, beside a
//! body's (MENGER-15, `--max-body-bytes`, 8 MiB) and a comment's or a note's
//! ([`crate::comments::BODY_MAX`], 64 KiB): a ticket's title, a filter's
//! text, and the pages `tickets.list` and `comments.list` answer.
//!
//! A page is what a call asks, else the default, and never past the most:
//! the `page` event carries the cursor to the rest, so a client that pages
//! (the app, synapse's scripts, every perf script) reads everything, and
//! one call never answers a whole store. Wasm-safe, like [`crate::filter`]:
//! the app may hold what a reader types to them.

/// The longest title `create`, `amend` and `update` keep, in bytes — about
/// four times the longest written so far (273).
pub const TITLE_MAX: usize = 1_024;

/// The longest filter text a read takes, in bytes (the caller's own, before
/// its reach is ANDed on).
pub const FILTER_MAX: usize = 4_096;

/// `tickets.list`'s page when the call names no `limit`, and the most it
/// answers whatever the call names — the REST bridge's own cap (TRANSPORT-2).
pub const LIST_PAGE_DEFAULT: u32 = 1_000;
pub const LIST_PAGE_MAX: u32 = 10_000;

/// `comments.list`'s page when the call names no `limit` (the app asks 500
/// at a time), and the most it answers.
pub const COMMENTS_PAGE_DEFAULT: u32 = 500;
pub const COMMENTS_PAGE_MAX: u32 = 1_000;

/// The page a call gets: `limit`, else `default`, at most `max`.
pub fn page(limit: Option<u32>, default: u32, max: u32) -> u32 {
    limit.unwrap_or(default).min(max)
}

/// A title past [`TITLE_MAX`], refused in these words.
pub fn title_too_long(id: Option<&str>, bytes: usize) -> String {
    let of = id.map(|id| format!(" for '{id}'")).unwrap_or_default();
    format!("the title given{of} is {bytes} bytes; a title is at most {TITLE_MAX} bytes (nothing changed)")
}

/// A filter's text past [`FILTER_MAX`]: why it is refused.
pub fn check_filter(text: &str) -> Result<(), String> {
    if text.len() > FILTER_MAX {
        return Err(format!("the filter is {} bytes; a filter is at most {FILTER_MAX} bytes", text.len()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_what_is_asked_else_the_default_never_past_the_most() {
        assert_eq!(page(None, LIST_PAGE_DEFAULT, LIST_PAGE_MAX), 1_000);
        assert_eq!(page(Some(20), LIST_PAGE_DEFAULT, LIST_PAGE_MAX), 20);
        assert_eq!(page(Some(0), LIST_PAGE_DEFAULT, LIST_PAGE_MAX), 0);
        assert_eq!(page(Some(u32::MAX), LIST_PAGE_DEFAULT, LIST_PAGE_MAX), 10_000);
        assert_eq!(page(Some(5_000), COMMENTS_PAGE_DEFAULT, COMMENTS_PAGE_MAX), 1_000);
    }

    #[test]
    fn a_filter_is_held_to_its_size() {
        assert!(check_filter(&"a".repeat(FILTER_MAX)).is_ok());
        let e = check_filter(&"a".repeat(FILTER_MAX + 1)).unwrap_err();
        assert!(e.contains("4096"), "{e}");
    }
}
