//! NOTE-2: the notes vocabulary — [`Note`], [`NoteState`], [`NotesEvent`] —
//! the rules `add` keeps a note by ([`check`]), and the title a note gives
//! the ticket it becomes ([`title_of`]). Wasm-safe, like
//! [`crate::comments`].
//!
//! A **note** is a thought written down before it is a ticket (NOTE-1): an
//! id, when it was taken, who wrote it, its text — and nothing that titles
//! it, and no shortcode. Kept by the server in one place for every client;
//! `upgrade` makes one a ticket and the note keeps the ticket's id.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Who wrote a note when nobody says: the owner, as for a comment.
pub const DEFAULT_AUTHOR: &str = crate::comments::DEFAULT_AUTHOR;

/// The longest body a note keeps, in bytes.
pub const BODY_MAX: usize = 65_536;

/// The longest author a note keeps, in bytes.
pub const AUTHOR_MAX: usize = 256;

/// The longest title a note gives its ticket, in characters.
pub const TITLE_MAX: usize = 80;

/// Where a note stands: still a note, or made a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteState {
    /// Not yet a ticket.
    Open,
    /// Made a ticket by `upgrade`; the note says which.
    Upgraded,
}

impl NoteState {
    pub const fn as_str(self) -> &'static str {
        match self {
            NoteState::Open => "open",
            NoteState::Upgraded => "upgraded",
        }
    }
}

impl fmt::Display for NoteState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NoteState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "open" => Ok(Self::Open),
            "upgraded" => Ok(Self::Upgraded),
            other => Err(format!("unknown note state '{other}' (expected open or upgraded)")),
        }
    }
}

/// A note as the server keeps it. `ticket` and `upgraded_at` are `null`
/// until `upgrade` makes it a ticket; `principal` is the signed-in identity
/// that wrote it, when login is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Note {
    pub id: crate::vocab::NoteId,
    /// When it was taken, unix seconds.
    pub at: i64,
    pub author: String,
    pub body: String,
    pub ticket: Option<String>,
    pub upgraded_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal: Option<String>,
}

impl Note {
    pub fn state(&self) -> NoteState {
        if self.ticket.is_some() {
            NoteState::Upgraded
        } else {
            NoteState::Open
        }
    }
}

/// What the `notes` activation streams. Every method returns
/// `impl Stream<Item = NotesEvent>`; a refusal is the in-band `Error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotesEvent {
    /// One note, as it is now (yielded by `add`, `get`, `upgrade`, and per
    /// row by `list`).
    Note {
        id: u64,
        at: i64,
        author: String,
        body: String,
        ticket: Option<String>,
        upgraded_at: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        principal: Option<String>,
    },
    /// Where a `list` stands, once, last: `next` is the id to pass as the
    /// next call's `after`, `null` when nothing more matches.
    Page { next: Option<u64> },
    /// In-band refusal, naming the problem; nothing was written.
    Error { message: String },
}

impl From<Note> for NotesEvent {
    fn from(n: Note) -> Self {
        let Note { id, at, author, body, ticket, upgraded_at, principal } = n;
        NotesEvent::Note { id: id.get(), at, author, body, ticket, upgraded_at, principal }
    }
}

impl NotesEvent {
    /// The note an event carries, if it carries one.
    pub fn into_note(self) -> Option<Note> {
        match self {
            NotesEvent::Note { id, at, author, body, ticket, upgraded_at, principal } => {
                Some(Note { id: crate::vocab::NoteId::new(id), at, author, body, ticket, upgraded_at, principal })
            }
            NotesEvent::Page { .. } | NotesEvent::Error { .. } => None,
        }
    }
}

/// Why `add` keeps nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    EmptyBody,
    BodyTooLong { bytes: usize },
    BlankAuthor,
    AuthorTooLong { bytes: usize },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Problem::EmptyBody => write!(f, "a note needs a body"),
            Problem::BodyTooLong { bytes } => write!(f, "a note's body is at most {BODY_MAX} bytes; this one is {bytes}"),
            Problem::BlankAuthor => write!(f, "author is blank: leave it out for {DEFAULT_AUTHOR}"),
            Problem::AuthorTooLong { bytes } => write!(f, "an author is at most {AUTHOR_MAX} bytes; this one is {bytes}"),
        }
    }
}

/// The author a note is kept with — `author` trimmed, else
/// [`DEFAULT_AUTHOR`] — or why it is refused: a blank or oversized body, a
/// blank or oversized author. The body is kept exactly as given.
pub fn check(body: &str, author: Option<&str>) -> Result<String, Problem> {
    if body.trim().is_empty() {
        return Err(Problem::EmptyBody);
    }
    if body.len() > BODY_MAX {
        return Err(Problem::BodyTooLong { bytes: body.len() });
    }
    match author {
        None => Ok(DEFAULT_AUTHOR.to_string()),
        Some(a) if a.trim().is_empty() => Err(Problem::BlankAuthor),
        Some(a) if a.trim().len() > AUTHOR_MAX => Err(Problem::AuthorTooLong { bytes: a.trim().len() }),
        Some(a) => Ok(a.trim().to_string()),
    }
}

/// A body's first line with words — a letter or a digit (a `---` rule, a
/// line of marks, has none) — its markdown marks (a heading's `#`, a
/// quote's `>`, a list's bullet, `**`) dropped and its spaces collapsed;
/// `None` when no line has words.
pub fn first_line(body: &str) -> Option<String> {
    body.lines().map(clean).find(|l| l.chars().any(char::is_alphanumeric))
}

fn clean(line: &str) -> String {
    let mut l = line.trim();
    loop {
        if let Some(rest) = l.strip_prefix(['#', '>']) {
            l = rest.trim_start_matches(['#', '>']).trim_start();
        } else if let Some(rest) = l.strip_prefix(['-', '*', '+']).filter(|r| r.starts_with(char::is_whitespace)) {
            l = rest.trim_start();
        } else {
            break;
        }
    }
    l.replace("**", "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The title a note gives the ticket it becomes: its first line with words,
/// at most [`TITLE_MAX`] characters — cut at a word, with an ellipsis, when
/// longer. `None` for a note with no words.
pub fn title_of(body: &str) -> Option<String> {
    let line = first_line(body)?;
    if line.chars().count() <= TITLE_MAX {
        return Some(line);
    }
    let cut: String = line.chars().take(TITLE_MAX - 1).collect();
    let at_word = cut.trim_end_matches(|c: char| !c.is_whitespace()).trim_end();
    let kept = if at_word.chars().count() >= TITLE_MAX / 2 { at_word } else { cut.as_str() };
    Some(format!("{kept}…"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_titled_by_its_first_line_with_words() {
        assert_eq!(title_of("\n\n## watch drops **events**\nmore").as_deref(), Some("watch drops events"));
        assert_eq!(title_of("- a bullet").as_deref(), Some("a bullet"));
        assert_eq!(title_of("> > a quote").as_deref(), Some("a quote"));
        assert_eq!(title_of("-5 degrees outside").as_deref(), Some("-5 degrees outside"));
        assert_eq!(title_of("---\n***\nthe first words").as_deref(), Some("the first words"));
        assert_eq!(title_of("  \n---\n"), None);
    }

    #[test]
    fn a_long_first_line_is_cut_at_a_word() {
        let t = title_of(&["word"; 30].join(" ")).unwrap();
        assert!(t.chars().count() <= TITLE_MAX && t.ends_with("word…"), "{t}");
        assert_eq!(title_of(&"x".repeat(100)).unwrap(), format!("{}…", "x".repeat(79)));
        let wide = title_of(&"ü".repeat(100)).unwrap();
        assert_eq!(wide.chars().count(), TITLE_MAX, "counted in characters, not bytes");
    }

    #[test]
    fn add_keeps_a_body_and_an_author_or_says_why_not() {
        assert_eq!(check("a thought", None), Ok("owner".into()));
        assert_eq!(check("a thought", Some(" ada ")), Ok("ada".into()));
        assert_eq!(check("  \n", None), Err(Problem::EmptyBody));
        assert_eq!(check(&"x".repeat(BODY_MAX + 1), None), Err(Problem::BodyTooLong { bytes: BODY_MAX + 1 }));
        assert_eq!(check("a thought", Some("  ")), Err(Problem::BlankAuthor));
        assert!(matches!(check("a thought", Some(&"a".repeat(300))), Err(Problem::AuthorTooLong { .. })));
    }

    #[test]
    fn a_state_reads_its_word_and_an_event_carries_its_note() {
        assert_eq!("Upgraded".parse::<NoteState>(), Ok(NoteState::Upgraded));
        assert!("closed".parse::<NoteState>().unwrap_err().contains("open or upgraded"));
        let n = Note { id: crate::vocab::NoteId::new(7), at: 1, author: "owner".into(), body: "b".into(), ticket: None, upgraded_at: None, principal: None };
        assert_eq!(n.state(), NoteState::Open);
        assert_eq!(NotesEvent::from(n.clone()).into_note(), Some(n.clone()));
        let json = serde_json::to_value(NotesEvent::from(n)).unwrap();
        assert_eq!(json["type"], "note");
        assert!(json["ticket"].is_null() && json.get("principal").is_none(), "{json}");
    }
}
