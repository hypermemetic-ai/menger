//! ACCESS-8: prose stays prose, but it has a name. A [`Title`] cannot be
//! passed where a [`Body`] is expected, and each one's rule lives here, not
//! in a method's body. Rules that need the store (a body's registry and
//! input markup, the configured `--max-body-bytes`) stay the store's.

use super::{not_a, too_long};
use crate::limits::TITLE_MAX;

/// The most a body may be on the wire, whatever the server keeps (its own
/// `--max-body-bytes`, 8 MiB by default, is the store's refusal).
pub const BODY_WIRE_MAX: usize = 64 * 1024 * 1024;

/// The longest message, rationale or reason, in bytes.
pub const MESSAGE_MAX: usize = 16 * 1024;

/// The longest query `find` and `search` (and a comment's or a note's
/// `text`) take, in bytes.
pub const QUERY_MAX: usize = 1024;

fn title(text: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("a title is not blank".into());
    }
    if text.len() > TITLE_MAX {
        return Err(crate::limits::title_too_long(None, text.len()));
    }
    Ok(text.to_string())
}

text_type!(
    /// A ticket's (or a note's ticket's) title: not blank, at most
    /// [`TITLE_MAX`] bytes, kept as written (ACCESS-29).
    Title,
    title
);

fn body(text: &str) -> Result<String, String> {
    match text.len() <= BODY_WIRE_MAX {
        true => Ok(text.to_string()),
        false => Err(too_long("a body", text.len(), BODY_WIRE_MAX)),
    }
}

text_type!(
    /// A body — a ticket's markdown, a comment's, a note's — exactly as
    /// written, at most [`BODY_WIRE_MAX`] bytes on the wire. What each
    /// keeps beyond that (`--max-body-bytes`, a comment's 64 KiB, a note's,
    /// not blank) is its store's rule, refused in band.
    Body,
    body
);

fn message(text: &str) -> Result<String, String> {
    let t = text.trim();
    match t.len() <= MESSAGE_MAX && !t.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        true => Ok(t.to_string()),
        false if t.len() > MESSAGE_MAX => Err(too_long("a message", t.len(), MESSAGE_MAX)),
        false => Err(not_a(t, "a message", "text, without control characters")),
    }
}

text_type!(
    /// MENGER-13: why — a transition's message, an answer's rationale, a
    /// tombstone's reason — trimmed, at most [`MESSAGE_MAX`] bytes. Blank is
    /// no message.
    Message,
    message
);

impl Message {
    /// The message, `None` when it is blank.
    pub fn given(self) -> Option<String> {
        (!self.0.is_empty()).then_some(self.0)
    }
}

fn reason(text: &str) -> Result<String, String> {
    let t = message(text)?;
    match t.is_empty() {
        false => Ok(t),
        true => Err("a queue entry says why: `reason` is blank".into()),
    }
}

text_type!(
    /// MENGER-39: why a ticket is in front of the owner — a [`Message`]'s
    /// rule, and not blank.
    Reason,
    reason
);

fn query(text: &str) -> Result<String, String> {
    match text.len() <= QUERY_MAX && !text.chars().any(char::is_control) {
        true => Ok(text.to_string()),
        false if text.len() > QUERY_MAX => Err(too_long("a query", text.len(), QUERY_MAX)),
        false => Err(not_a(text, "a query", "words, without control characters")),
    }
}

text_type!(
    /// What a reader looks for — `find`'s and `search`'s query, a comment's
    /// or a note's `text` — as typed, at most [`QUERY_MAX`] bytes (blank:
    /// `find`'s every ticket, as before).
    Query,
    query
);

fn author(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("author is blank (leave it out for `owner`)".into());
    }
    match t.len() <= crate::comments::AUTHOR_MAX {
        true => Ok(t.to_string()),
        false => Err(format!("author is {} bytes, past the {} a comment or a note keeps", t.len(), crate::comments::AUTHOR_MAX)),
    }
}

text_type!(
    /// COMMENT-2, NOTE-2: who left a comment or a note — trimmed, not
    /// blank, at most 256 bytes. A signed-in caller's is its identity's
    /// name, whatever this says.
    Author,
    author
);

fn filter_text(text: &str) -> Result<String, String> {
    crate::limits::check_filter(text)?;
    crate::filter::Filter::parse(text).map(|_| text.to_string()).map_err(|e| e.to_string())
}

text_type!(
    /// FILTER-3's language, as text (`ready and kind build and in SCALE`) —
    /// one the language reads (every bad chunk named), at most
    /// [`crate::limits::FILTER_MAX`] bytes, kept as written: the server reads it with its
    /// cells, ANDs the caller's reach on, and answers what it admits.
    /// Blank is a filter that narrows nothing.
    FilterText,
    filter_text
);

impl FilterText {
    /// The parsed filter — infallible: the text was parsed to be made.
    pub fn filter(&self) -> crate::filter::Filter {
        crate::filter::Filter::parse(&self.0).expect("a FilterText holds a filter")
    }
}

fn zone(text: &str) -> Result<String, String> {
    let t = text.trim();
    let refused = || not_a(t, "a time zone", "an IANA name (Europe/Berlin), UTC, or an offset (+02:00, -0530)");
    if t.is_empty() || t.len() > 64 {
        return Err(refused());
    }
    if let Some(rest) = t.strip_prefix(['+', '-']) {
        let (h, m) = match rest.split_once(':') {
            Some((h, m)) => (h, m),
            None if rest.len() == 4 => (&rest[..2], &rest[2..]),
            None => (rest, "0"),
        };
        let ok = h.parse::<u32>().ok().zip(m.parse::<u32>().ok()).is_some_and(|(h, m)| h <= 18 && m < 60);
        return ok.then(|| t.to_string()).ok_or_else(refused);
    }
    let named = t.starts_with(|c: char| c.is_ascii_alphabetic())
        && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'+'));
    named.then(|| t.to_string()).ok_or_else(refused)
}

text_type!(
    /// MENGER-13: the time zone a history filter's days are read in — an
    /// IANA name (`Europe/Berlin`), `UTC`, or an offset (`+02:00`). Checked
    /// for its spelling here (wasm-safe); a name the server's tzdb does not
    /// hold is the server's in-band refusal.
    Zone,
    zone
);

/// `YYYY-MM-DDTHH:MM:SS[.f…](Z|±HH:MM)`, by its shape.
fn rfc3339(t: &str) -> bool {
    let b = t.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
    let shape = b.len() >= 20
        && digits(0..4) && b[4] == b'-' && digits(5..7) && b[7] == b'-' && digits(8..10)
        && matches!(b[10], b'T' | b't' | b' ')
        && digits(11..13) && b[13] == b':' && digits(14..16) && b[16] == b':' && digits(17..19);
    if !shape {
        return false;
    }
    let mut rest = &t[19..];
    if let Some(f) = rest.strip_prefix('.') {
        let n = f.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 {
            return false;
        }
        rest = &f[n..];
    }
    matches!(rest, "Z" | "z")
        || (rest.len() == 6
            && matches!(rest.as_bytes()[0], b'+' | b'-')
            && rest.as_bytes()[3] == b':'
            && rest[1..3].bytes().chain(rest[4..].bytes()).all(|c| c.is_ascii_digit()))
}

fn timestamp(text: &str) -> Result<String, String> {
    let t = text.trim();
    let unix = !t.is_empty() && t.len() <= 12 && t.bytes().all(|b| b.is_ascii_digit());
    match unix || rfc3339(t) {
        true => Ok(t.to_string()),
        false => Err(not_a(t, "a time", "RFC 3339 (2026-10-12T22:04:19Z) or unix seconds")),
    }
}

text_type!(
    /// DEPLOY-10: when — RFC 3339 (`2026-10-12T22:04:19Z`) or unix seconds,
    /// by its shape; the server reads the moment.
    Timestamp,
    timestamp
);
