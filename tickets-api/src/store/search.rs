//! SEARCH-2: search inside tickets — every ticket's title and the text of
//! its body in an FTS5 index in the database beside them, and the query
//! `search` answers from it.
//!
//! **Where it lives.** In the system of record, not the cache: a body has
//! not been in the cache since SCALE-14, and an index of what is not held
//! cannot be kept there. Two tables — `search_rows`, each ticket's id and
//! the row it is indexed as, and `search`, FTS5 over `(title, text)` — and
//! `search_terms`, FTS5's own view of the index's words. A file gains them
//! once, on its first open, every ticket indexed in one transaction
//! ([`migrate`]); after it a write that changes a title or a body rewrites
//! the ticket's row in the write's own transaction ([`put`]) — `create`,
//! `amend`, and a seed's chunks — so the index can never be behind a ticket
//! a reader can see. No second store, no service, nothing to deploy: the
//! index is SQLite's, in the one file.
//!
//! **What is indexed** is what a reader reads ([`text`]): the body without
//! its tags, attributes, comments, scripts, styles, component sources,
//! frontmatter and a diagram's syntax — but with the text between the tags,
//! a diagram's labels, a few attributes a reader sees as words
//! ([`READ_ATTRIBUTES`]: an `<m-ticket ref>`'s id, an option's `name`), and
//! code as it is written.
//! The tokenizer is FTS5's `unicode61`, case and diacritics folded.
//!
//! **The query** ([`parts`], [`plan`]) is words, every one required. A word
//! matches itself and anything it begins (`plac` → placement). A word of
//! four letters or more that the index does not know, or holds in only a
//! few tickets ([`RARE`] — a typo some ticket repeats), is also matched to
//! the index's words one edit away (two from eight letters; an edit is an
//! insertion, a deletion, a substitution or a swap of neighbours), the first
//! letter kept — a rare word's only to words [`COMMONER`] times as common;
//! the last word's corrections may be prefixes too (`plcem` → `placem…`).
//! A word joined by `-` `_` `.` `/` (`FILTER-23`, `set_status`) is a
//! phrase, as is a `"quoted"` one: adjacent, in order, never corrected. The
//! tickets matched without any correction come first, then those that
//! needed one; within each, BM25 with a title match weighed 4 to a body
//! match's 1. A page is a window of that order (`offset`, `limit`): without
//! a filter, the matches are counted and only the window is ranked out of
//! FTS5; with one, every match is ranked and the filter keeps those it
//! admits (one lookup each in its cells, as `member`'s), counted as kept.
//!
//! A search is a read: on a read connection, off the runtime, one snapshot;
//! the rows it answers are the cache's.

use super::filtered::view;
use super::*;
use crate::types::{Marked, SearchWord};
use rusqlite::{params, OptionalExtension};

/// The index. `search_rows.row` is the FTS5 row a ticket is indexed as —
/// an alias of the table's rowid, so it never moves. `search_terms` is the
/// index's vocabulary, each word with how many tickets hold it: what a
/// word is looked up in, and corrected from. No prefix index: measured at
/// 10⁵ synthetic tickets, `prefix = '2 3'` cost 54 MB (145 against 91) and
/// 2.5 s of building, and a two-letter prefix took as long either way
/// (56 against 60 ms).
pub(super) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS search_rows (
            row INTEGER PRIMARY KEY,
            id  TEXT NOT NULL UNIQUE
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(
            title, text,
            tokenize = 'unicode61 remove_diacritics 2'
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS search_terms USING fts5vocab(search, row);
    ";

/// The attributes whose values a reader reads as words: a ticket reference's
/// id, an option's name, a comparison's labels, a decision's costs.
pub const READ_ATTRIBUTES: [&str; 8] = ["ref", "name", "title", "alt", "label", "labels", "cost", "benefit"];

/// How many hits a page holds unless asked, and at most.
pub const SEARCH_LIMIT: u32 = 20;
pub const SEARCH_MAX: u32 = 100;

/// A word is corrected from this many letters; from [`TWO_TYPOS_FROM`], by
/// up to two edits.
pub const TYPO_FROM: usize = 4;
pub const TWO_TYPOS_FROM: usize = 8;

/// How many corrections one word takes — the nearest, then the commonest.
pub const MAX_ALSO: usize = 8;

/// A word the index holds in at most this many tickets (as itself, or as
/// the beginning of words) is rare: a typo some ticket repeats looks like
/// this, and is corrected too — but only to words [`COMMONER`] times as
/// common, so a rare word that is meant is not drowned by a common one.
pub const RARE: i64 = 3;
pub const COMMONER: i64 = 5;

/// How many words of a query are read; the rest are not.
pub const MAX_WORDS: usize = 12;

/// How many words a passage holds, at most (FTS5's `snippet`).
const PASSAGE_WORDS: u32 = 24;

/// A title's weight against the text's, in BM25.
const TITLE_WEIGHT: f64 = 4.0;

/// The marks a highlight is read back with — control characters, which
/// [`text`] and [`clean`] never leave in what is indexed.
const OPEN: char = '\u{2}';
const CLOSE: char = '\u{3}';

// ---------------------------------------------------------------------
// What is indexed

/// A title as it is indexed: itself, with any control character a space —
/// so the marks a highlight is read back with cannot be in it.
pub(super) fn clean(title: &str) -> String {
    title.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// The text of a body, as a reader reads it — what [`put`] indexes. Pure:
/// the same body gives the same text, which is what `--verify-derived`
/// holds the index to.
pub fn text(body: &str) -> String {
    let body = without_frontmatter(body);
    let mut out = String::with_capacity(body.len());
    let mut prose = String::new();
    let mut fence: Option<Fence> = None;
    for line in body.lines() {
        let trimmed = line.trim_start();
        match &fence {
            None => {
                if let Some(opened) = Fence::opens(trimmed) {
                    strip_markup(&prose, &mut out);
                    prose.clear();
                    out.push(' ');
                    fence = Some(opened);
                } else {
                    prose_line(line, &mut prose);
                }
            }
            Some(f) => {
                if f.closes(trimmed) {
                    fence = None;
                    out.push(' ');
                    continue;
                }
                match f.kind {
                    Code::Component => {}
                    Code::Mermaid if mermaid_styling(trimmed) => {}
                    Code::Mermaid => {
                        mermaid_labels(trimmed, &mut out);
                        out.push(' ');
                    }
                    Code::Other => {
                        out.push_str(line);
                        out.push(' ');
                    }
                }
            }
        }
    }
    strip_markup(&prose, &mut out);
    collapse(&out)
}

/// A body without the YAML frontmatter it opens with, if it opens with one
/// that closes (`---` … `---` or `...`); else the body.
fn without_frontmatter(body: &str) -> &str {
    let Some(rest) = body.strip_prefix("---\n").or_else(|| body.strip_prefix("---\r\n")) else {
        return body;
    };
    let mut at = body.len() - rest.len();
    for line in rest.split_inclusive('\n') {
        at += line.len();
        if matches!(line.trim_end(), "---" | "...") {
            return &body[at..];
        }
    }
    body
}

/// What a fenced block is.
enum Code {
    /// A ` ```component ` block: an element's source, not prose.
    Component,
    /// A ` ```mermaid ` block: its labels are words, its styling is not.
    Mermaid,
    Other,
}

/// An open fence: its character, how many of them opened it, and its kind.
struct Fence {
    mark: char,
    run: usize,
    kind: Code,
}

impl Fence {
    /// The fence a line (its indent trimmed) opens, if it opens one.
    fn opens(trimmed: &str) -> Option<Self> {
        let mark = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'))?;
        let run = trimmed.chars().take_while(|c| *c == mark).count();
        if run < 3 {
            return None;
        }
        let info = trimmed[run..].trim();
        if mark == '`' && info.contains('`') {
            return None;
        }
        let kind = match info.split_whitespace().next().map(str::to_ascii_lowercase).as_deref() {
            Some("component") => Code::Component,
            Some("mermaid") => Code::Mermaid,
            _ => Code::Other,
        };
        Some(Self { mark, run, kind })
    }

    fn closes(&self, trimmed: &str) -> bool {
        let run = trimmed.chars().take_while(|c| *c == self.mark).count();
        run >= self.run && trimmed[run..].trim().is_empty()
    }
}

/// What a reader reads of a mermaid line: its labels, not its syntax — a
/// quoted label's text (`B["the cache<br/>kept"]`, `-->|"on write"|`), or,
/// in a line with no quotes, the text inside brackets and between bars
/// (`A[the cache] -->|on write| B`); their markup stripped. Node ids,
/// arrows and keywords are not words of the diagram.
fn mermaid_labels(line: &str, out: &mut String) {
    let mut label = String::new();
    if line.contains('"') {
        for (k, part) in line.split('"').enumerate() {
            if k % 2 == 1 {
                label.push_str(part);
                label.push(SOFT);
            }
        }
    } else {
        let (mut depth, mut bar) = (0usize, false);
        for c in line.chars() {
            match c {
                '[' | '(' | '{' => depth += 1,
                ']' | ')' | '}' => depth = depth.saturating_sub(1),
                '|' => bar = !bar,
                _ if depth > 0 || bar => {
                    label.push(c);
                    continue;
                }
                _ => {}
            }
            label.push(SOFT);
        }
    }
    strip_markup(&label, out);
}

/// A mermaid line that only styles: `classDef`, `class`, `style`,
/// `linkStyle`, a `%%` comment.
fn mermaid_styling(trimmed: &str) -> bool {
    trimmed.starts_with("%%")
        || ["classDef ", "class ", "style ", "linkStyle "].iter().any(|k| trimmed.starts_with(k))
}

/// One line of prose into `prose`: a table's separator row dropped and its
/// cells parted by ` · `; a heading's `#`s and a quote's `>` dropped.
fn prose_line(line: &str, prose: &mut String) {
    let trimmed = line.trim();
    if trimmed.starts_with('|') {
        if trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t')) {
            return;
        }
        let cells: Vec<&str> = trimmed.trim_matches('|').split('|').map(str::trim).collect();
        prose.push_str(&cells.join(" · "));
    } else if let Some(heading) = trimmed.strip_prefix('#') {
        prose.push_str(heading.trim_start_matches('#').trim_start());
    } else if let Some(quote) = trimmed.strip_prefix('>') {
        prose.push_str(quote);
    } else {
        prose.push_str(line);
    }
    prose.push('\n');
}

/// The elements that run inside a line of text (`<strong>`, `<code>`, a
/// link): their tags are nothing, so `<b>word</b>,` stays `word,`.
const INLINE: [&str; 25] = [
    "a", "abbr", "b", "bdi", "bdo", "cite", "code", "data", "del", "dfn", "em", "i", "ins", "kbd", "mark", "q",
    "s", "samp", "small", "span", "strong", "sub", "sup", "u", "var",
];

/// What markup leaves between words: a space, unless punctuation follows
/// ([`collapse`]) — `→ <m-ticket ref="X"></m-ticket>.` reads `→ X.`. A
/// control character, which no indexed text keeps.
const SOFT: char = '\u{1}';

/// Prose without its markup, onto `out`: a tag is a soft space ([`SOFT`];
/// nothing for an [`INLINE`] element's) with the values of
/// [`READ_ATTRIBUTES`] it carries, a comment a soft space, a `<style>` or
/// `<script>` element nothing, an entity its character, `**` nothing; an
/// inline code span as it is written.
fn strip_markup(prose: &str, out: &mut String) {
    let b = prose.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'<' if prose[i..].starts_with("<!--") => {
                i = prose[i + 4..].find("-->").map_or(b.len(), |e| i + 4 + e + 3);
                out.push(SOFT);
            }
            b'<' if b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'/') => match tag_end(b, i + 1) {
                Some(end) => {
                    let tag = &prose[i + 1..end];
                    let closing = tag.starts_with('/');
                    let name: String = tag
                        .trim_start_matches('/')
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                        .collect::<String>()
                        .to_ascii_lowercase();
                    if !INLINE.contains(&name.as_str()) {
                        out.push(SOFT);
                    }
                    if !closing {
                        for value in read_attributes(&tag[name.len()..]) {
                            out.push(SOFT);
                            out.push_str(&value.replace('|', " "));
                            out.push(SOFT);
                        }
                    }
                    i = end + 1;
                    if !closing && !tag.ends_with('/') && matches!(name.as_str(), "style" | "script") {
                        let close = format!("</{name}");
                        let rest = prose[i..].to_ascii_lowercase();
                        i = match rest.find(&close) {
                            Some(at) => tag_end(b, i + at + 1).map_or(b.len(), |e| e + 1),
                            None => b.len(),
                        };
                    }
                }
                None => {
                    out.push('<');
                    i += 1;
                }
            },
            b'`' => {
                let run = b[i..].iter().take_while(|c| **c == b'`').count();
                let fence = &prose[i..i + run];
                match find_run(&prose[i + run..], fence) {
                    Some(at) => {
                        out.push_str(&prose[i + run..i + run + at]);
                        i += run + at + run;
                    }
                    None => {
                        out.push_str(fence);
                        i += run;
                    }
                }
            }
            b'&' => match entity(&prose[i..]) {
                Some((c, len)) => {
                    out.push(c);
                    i += len;
                }
                None => {
                    out.push('&');
                    i += 1;
                }
            },
            b'*' if b.get(i + 1) == Some(&b'*') => i += 2,
            _ => {
                let c = prose[i..].chars().next().expect("i is on a character boundary");
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
}

/// Where the tag whose name starts at `from` ends — its `>`, quotes
/// respected — if it ends.
fn tag_end(b: &[u8], from: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (at, c) in b.iter().enumerate().skip(from) {
        match (quote, *c) {
            (None, b'"' | b'\'') => quote = Some(*c),
            (Some(q), c) if c == q => quote = None,
            (None, b'>') => return Some(at),
            _ => {}
        }
    }
    None
}

/// The next run of exactly `fence` backticks in `rest` — where an inline
/// code span opened by `fence` closes.
fn find_run(rest: &str, fence: &str) -> Option<usize> {
    let b = rest.as_bytes();
    let mut i = 0;
    while let Some(at) = rest[i..].find(fence) {
        let at = i + at;
        let run = b[at..].iter().take_while(|c| **c == b'`').count();
        if run == fence.len() {
            return Some(at);
        }
        i = at + run;
    }
    None
}

/// The values of the [`READ_ATTRIBUTES`] among a tag's attributes (the tag
/// after its name).
fn read_attributes(attrs: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = attrs;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if rest.is_empty() {
            return out;
        }
        let name_len = rest.find(|c: char| c.is_whitespace() || c == '=' || c == '/').unwrap_or(rest.len()).max(1);
        let name = rest[..name_len].to_ascii_lowercase();
        rest = rest[name_len..].trim_start();
        let Some(after) = rest.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let (value, next) = match after.chars().next() {
            Some(q @ ('"' | '\'')) => match after[1..].find(q) {
                Some(end) => (&after[1..1 + end], &after[1 + end + 1..]),
                None => (&after[1..], ""),
            },
            _ => {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..end], &after[end..])
            }
        };
        if READ_ATTRIBUTES.contains(&name.as_str()) {
            let mut decoded = String::new();
            strip_markup(value, &mut decoded);
            out.push(decoded);
        }
        rest = next;
    }
}

/// The character an entity at the start of `s` names, and its length — the
/// five XML names, `nbsp`, and numeric references.
fn entity(s: &str) -> Option<(char, usize)> {
    let end = s.bytes().take(12).position(|b| b == b';')?;
    let name = &s[1..end];
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let n = name.strip_prefix('#')?;
            let code = match n.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => n.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some((c, end + 1))
}

/// Every run of whitespace or control characters one space, none at either
/// end — and none at all where the run is only markup's ([`SOFT`]) and
/// punctuation follows.
fn collapse(s: &str) -> String {
    #[derive(PartialEq)]
    enum Gap {
        None,
        Soft,
        Hard,
    }
    let mut out = String::with_capacity(s.len());
    let mut gap = Gap::None;
    for c in s.chars() {
        if c == SOFT {
            if gap == Gap::None {
                gap = Gap::Soft;
            }
        } else if c.is_whitespace() || c.is_control() {
            gap = Gap::Hard;
        } else {
            let closes = matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '…');
            if !out.is_empty() && (gap == Gap::Hard || (gap == Gap::Soft && !closes)) {
                out.push(' ');
            }
            out.push(c);
            gap = Gap::None;
        }
    }
    out
}

// ---------------------------------------------------------------------
// The index in the file

pub(super) fn has_index(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'search_rows'",
        [],
        |r| r.get::<_, i64>(0).map(|n| n > 0),
    )
}

/// SEARCH-2: a file without the index — written before it, or new — gains
/// it here, once, durably: the tables, and every ticket indexed, in one
/// transaction, committed before the cache loads. Every later open finds it
/// and does nothing. Returns whether it was made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    if has_index(conn)? {
        return Ok(false);
    }
    let started = std::time::Instant::now();
    let tx = conn.transaction()?;
    tx.execute_batch(TABLES)?;
    let mut indexed = 0usize;
    {
        let mut stmt = tx.prepare("SELECT id, title, body FROM tickets ORDER BY epic, seq")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            insert(&tx, r.get_ref(0)?.as_str()?, r.get_ref(1)?.as_str()?, r.get_ref(2)?.as_str()?)?;
            indexed += 1;
        }
    }
    tx.commit()?;
    tracing::warn!(
        "SEARCH-2: the file gained the search index — {indexed} tickets' titles and text — in {:.1} ms, durably; \
         every later open reads it",
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(true)
}

/// A ticket's row in the index, as its title and body give it now: written
/// for a ticket that has none, rewritten for one that has. In the caller's
/// transaction — the one that writes the ticket.
pub(super) fn put(conn: &Connection, id: &str, title: &str, body: &str) -> rusqlite::Result<()> {
    let (title, text) = (clean(title), text(body));
    let row: Option<i64> = conn
        .prepare_cached("SELECT row FROM search_rows WHERE id = ?1")?
        .query_row([id], |r| r.get(0))
        .optional()?;
    match row {
        Some(row) => {
            conn.prepare_cached("UPDATE search SET title = ?2, text = ?3 WHERE rowid = ?1")?
                .execute(params![row, title, text])?;
            Ok(())
        }
        None => indexed(conn, id, &title, &text),
    }
}

/// A row for a ticket the index has none for — what a first open and a
/// seed write, which know that already (a seed's ids are new: the
/// ticket's own insert refuses any other), so they skip the lookup.
fn insert(conn: &Connection, id: &str, title: &str, body: &str) -> rusqlite::Result<()> {
    indexed(conn, id, &clean(title), &text(body))
}

fn indexed(conn: &Connection, id: &str, title: &str, text: &str) -> rusqlite::Result<()> {
    conn.prepare_cached("INSERT INTO search_rows (id) VALUES (?1)")?.execute([id])?;
    let row = conn.last_insert_rowid();
    conn.prepare_cached("INSERT INTO search (rowid, title, text) VALUES (?1, ?2, ?3)")?
        .execute(params![row, title, text])?;
    Ok(())
}

/// A seed's tickets indexed, in the transaction that writes them.
pub(super) fn put_many(conn: &Connection, tickets: &[Ticket]) -> rusqlite::Result<()> {
    for t in tickets {
        insert(conn, &t.id, &t.title, &t.body)?;
    }
    Ok(())
}

/// What `--verify-derived` finds of the index.
pub(super) enum Checked {
    /// The file has none: no build with SEARCH-2 has opened it (the next
    /// open makes it).
    Absent,
    /// How many tickets it holds, and where it differs from what the
    /// tickets give now, one line each.
    Checked { indexed: usize, differs: Vec<String> },
}

/// How many lines of difference the index reports before it counts the rest.
const DIFFERS_SHOWN: usize = 50;

/// SEARCH-2: the index against the tickets, in the caller's (read)
/// transaction: one row per ticket and none for anything else, each holding
/// the title and text its ticket gives now ([`clean`], [`text`]), as many
/// FTS5 rows as tickets, and FTS5's own check of its inverted index
/// (`PRAGMA integrity_check(search)`, which reads only).
pub(super) fn check(conn: &Connection) -> rusqlite::Result<Checked> {
    if !has_index(conn)? {
        return Ok(Checked::Absent);
    }
    let mut rows: HashMap<String, i64> = {
        let mut stmt = conn.prepare("SELECT id, row FROM search_rows")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let indexed = rows.len();
    let mut differs = Vec::new();
    {
        let mut tickets = conn.prepare("SELECT id, title, body FROM tickets ORDER BY epic, seq")?;
        let mut stored = conn.prepare("SELECT title, text FROM search WHERE rowid = ?1")?;
        let mut all = tickets.query([])?;
        while let Some(r) = all.next()? {
            let id: String = r.get(0)?;
            let Some(row) = rows.remove(&id) else {
                differs.push(format!("search: {id} is not indexed"));
                continue;
            };
            let held: Option<(String, String)> =
                stored.query_row([row], |s| Ok((s.get(0)?, s.get(1)?))).optional()?;
            let want = (clean(r.get_ref(1)?.as_str()?), text(r.get_ref(2)?.as_str()?));
            match held {
                None => differs.push(format!("search: {id}'s index row {row} holds nothing")),
                Some((title, _)) if title != want.0 => {
                    differs.push(format!("search: {id}'s indexed title is {title:?}; its ticket's is {:?}", want.0))
                }
                Some((_, text)) if text != want.1 => {
                    differs.push(format!("search: {id}'s indexed text is not what its body gives now"))
                }
                Some(_) => {}
            }
        }
    }
    let mut strays: Vec<String> = rows.into_keys().collect();
    strays.sort();
    differs.extend(strays.into_iter().map(|id| format!("search: {id} is indexed and is no ticket")));
    let fts: i64 = conn.query_row("SELECT count(*) FROM search", [], |r| r.get(0))?;
    if fts != indexed as i64 {
        differs.push(format!("search: the index holds {fts} rows for {indexed} ids"));
    }
    let integrity: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA integrity_check(search)")?;
        let lines = stmt.query_map([], |r| r.get(0))?;
        lines.collect::<rusqlite::Result<_>>()?
    };
    if integrity != ["ok"] {
        differs.extend(integrity.into_iter().map(|l| format!("search: integrity_check: {l}")));
    }
    if differs.len() > DIFFERS_SHOWN {
        let more = differs.len() - DIFFERS_SHOWN;
        differs.truncate(DIFFERS_SHOWN);
        differs.push(format!("search: and {more} more"));
    }
    Ok(Checked::Checked { indexed, differs })
}

// ---------------------------------------------------------------------
// The query

/// One required part of a query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Part {
    /// A word: itself, as a prefix, and — when the index neither knows nor
    /// begins it — its corrections.
    Word(String),
    /// Words adjacent, in order — the last a prefix when `prefix`.
    Phrase { words: Vec<String>, prefix: bool },
}

/// The words of `s` — its runs of letters and digits, lowercased.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// A query, read: `"…"` a phrase (an unclosed quote runs to the end), every
/// other whitespace-parted chunk a word — or, when it holds several runs of
/// letters and digits (`FILTER-23`, `set_status`), a phrase whose last word
/// is a prefix. At most [`MAX_WORDS`] parts.
pub(super) fn parts(query: &str) -> Vec<Part> {
    let mut out = Vec::new();
    for (i, chunk) in query.split('"').enumerate() {
        if i % 2 == 1 {
            let ws = words(chunk);
            if !ws.is_empty() {
                out.push(Part::Phrase { words: ws, prefix: false });
            }
            continue;
        }
        for piece in chunk.split_whitespace() {
            let mut ws = words(piece);
            match ws.len() {
                0 => {}
                1 => out.push(Part::Word(ws.pop().expect("one word"))),
                _ => out.push(Part::Phrase { words: ws, prefix: true }),
            }
        }
    }
    out.truncate(MAX_WORDS);
    out
}

/// An FTS5 string: `s` quoted (it holds letters, digits and spaces only),
/// a prefix when `prefix`.
fn quoted(s: &str, prefix: bool) -> String {
    if prefix {
        format!("\"{s}\"*")
    } else {
        format!("\"{s}\"")
    }
}

/// A query, planned against the index: the FTS5 expression matched without
/// corrections, the one with them (none when no word took one), and the
/// words as they were read, with their corrections.
#[derive(Debug)]
pub(super) struct Plan {
    pub(super) exact: String,
    pub(super) fuzzy: Option<String>,
    pub(super) words: Vec<SearchWord>,
}

/// One alternative a word was corrected to: its FTS5 string, the word it
/// shows as, its distance, how many tickets hold it.
#[derive(Debug)]
struct Alt {
    expr: String,
    word: String,
    distance: usize,
    docs: i64,
}

pub(super) fn plan(conn: &Connection, parts: &[Part]) -> rusqlite::Result<Plan> {
    let last = parts.len().saturating_sub(1);
    let (mut exact, mut fuzzy, mut out) = (Vec::new(), Vec::new(), Vec::new());
    let mut corrected = false;
    for (i, part) in parts.iter().enumerate() {
        match part {
            Part::Word(w) => {
                let own = quoted(w, w.chars().count() >= 2);
                let typo = w.chars().count() >= TYPO_FROM && w.chars().all(char::is_alphabetic);
                let held = if typo { held(conn, w)? } else { RARE + 1 };
                let also = if held <= RARE {
                    let floor = if held == 0 { 0 } else { held * COMMONER };
                    corrections(conn, w, i == last, floor)?
                } else {
                    Vec::new()
                };
                exact.push(own.clone());
                if also.is_empty() {
                    fuzzy.push(own);
                } else {
                    corrected = true;
                    let alts: Vec<&str> = also.iter().map(|a| a.expr.as_str()).collect();
                    fuzzy.push(format!("({own} OR {})", alts.join(" OR ")));
                }
                let mut shown: Vec<String> = Vec::new();
                for a in &also {
                    if !shown.contains(&a.word) {
                        shown.push(a.word.clone());
                    }
                }
                out.push(SearchWord { word: w.clone(), also: shown });
            }
            Part::Phrase { words, prefix } => {
                let e = quoted(&words.join(" "), *prefix);
                exact.push(e.clone());
                fuzzy.push(e);
                out.push(SearchWord { word: words.join(" "), also: Vec::new() });
            }
        }
    }
    Ok(Plan {
        exact: exact.join(" AND "),
        fuzzy: corrected.then(|| fuzzy.join(" AND ")),
        words: out,
    })
}

/// How many tickets hold `w`, or a word it begins — summed over those
/// words, and counted only until it is past [`RARE`]: all a caller asks is
/// whether the word is unknown (0), rare, or neither.
fn held(conn: &Connection, w: &str) -> rusqlite::Result<i64> {
    let mut stmt = conn.prepare_cached("SELECT term, doc FROM search_terms WHERE term >= ?1")?;
    let mut rows = stmt.query([w])?;
    let mut held = 0i64;
    while let Some(r) = rows.next()? {
        if !r.get_ref(0)?.as_str()?.starts_with(w) {
            break;
        }
        held += r.get::<_, i64>(1)?;
        if held > RARE {
            break;
        }
    }
    Ok(held)
}

/// The index's words within reach of `w` — [`osa`] of at most one edit, two
/// from [`TWO_TYPOS_FROM`] letters, the first letter kept — held by at least
/// `floor` tickets; nearest first, then the commonest, at most
/// [`MAX_ALSO`]. For the last word (`last`), a word whose beginning is
/// within reach is one too: the beginning, as a prefix, shown as the
/// commonest word it begins.
fn corrections(conn: &Connection, w: &str, last: bool, floor: i64) -> rusqlite::Result<Vec<Alt>> {
    let typed: Vec<char> = w.chars().collect();
    let budget = if typed.len() >= TWO_TYPOS_FROM { 2 } else { 1 };
    let first = typed[0];
    let mut found: HashMap<String, Alt> = HashMap::new();
    let mut stmt = conn.prepare_cached("SELECT term, doc FROM search_terms WHERE term >= ?1")?;
    let mut rows = stmt.query([first.to_string()])?;
    while let Some(r) = rows.next()? {
        let term = r.get_ref(0)?.as_str()?;
        if !term.starts_with(first) {
            break;
        }
        let docs: i64 = r.get(1)?;
        if docs < floor || term == w {
            continue;
        }
        let t: Vec<char> = term.chars().collect();
        let alt = match osa(&typed, &t, budget) {
            Some(distance) => Alt { expr: quoted(term, false), word: term.to_string(), distance, docs },
            None if last => match osa_prefix(&typed, &t, budget) {
                Some((distance, len)) if len < t.len() => {
                    let begins: String = t[..len].iter().collect();
                    Alt { expr: quoted(&begins, true), word: term.to_string(), distance, docs }
                }
                _ => continue,
            },
            None => continue,
        };
        // A beginning many words share is one alternative, shown as the
        // commonest of them.
        match found.get_mut(&alt.expr) {
            Some(held) if (alt.distance, -alt.docs) < (held.distance, -held.docs) => *held = alt,
            Some(_) => {}
            None => {
                found.insert(alt.expr.clone(), alt);
            }
        }
    }
    let mut alts: Vec<Alt> = found.into_values().collect();
    alts.sort_by(|a, b| (a.distance, -a.docs, &a.word).cmp(&(b.distance, -b.docs, &b.word)));
    alts.truncate(MAX_ALSO);
    Ok(alts)
}

/// The optimal-string-alignment distance between `a` and `b` (insertions,
/// deletions, substitutions and swaps of neighbours), when it is at most
/// `max`.
pub(super) fn osa(a: &[char], b: &[char], max: usize) -> Option<usize> {
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let d = last_row(a, b, max)?;
    let distance = d[b.len()];
    (distance <= max).then_some(distance)
}

/// The least [`osa`] between `a` and a beginning of `b` within `max` of
/// `a`'s length, and that beginning's length (the longest of the nearest).
pub(super) fn osa_prefix(a: &[char], b: &[char], max: usize) -> Option<(usize, usize)> {
    let upto = b.len().min(a.len() + max);
    let d = last_row(a, &b[..upto], max)?;
    let from = a.len().saturating_sub(max).max(1);
    (from..=upto)
        .map(|len| (d[len], len))
        .filter(|(distance, _)| *distance <= max)
        .min_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)))
}

/// The last row of the OSA table of `a` against `b` — the distance from all
/// of `a` to each beginning of `b` — or none when every entry of some row
/// is already past `max`.
fn last_row(a: &[char], b: &[char], max: usize) -> Option<Vec<usize>> {
    let n = b.len();
    let mut before: Vec<usize> = vec![0; n + 1];
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur: Vec<usize> = vec![0; n + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        let mut least = i;
        for j in 1..=n {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(before[j - 2] + 1);
            }
            cur[j] = v;
            least = least.min(v);
        }
        if least > max {
            return None;
        }
        std::mem::swap(&mut before, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    Some(prev)
}

/// How many tickets an FTS5 expression matches — counted, not ranked.
fn count(conn: &Connection, expr: &str) -> rusqlite::Result<usize> {
    conn.prepare_cached("SELECT count(*) FROM search WHERE search MATCH ?1")?
        .query_row([expr], |r| r.get::<_, i64>(0))
        .map(|n| n as usize)
}

/// The tickets `expr` matches and `without` (if any) does not, best first
/// (a tie in the order they were indexed), each one's index row and id —
/// all of them, or the window `(offset, limit)` of that order, which FTS5
/// ranks without handing over the rest.
fn ranked(
    conn: &Connection,
    expr: &str,
    without: Option<&str>,
    window: Option<(usize, usize)>,
) -> rusqlite::Result<Vec<(i64, String)>> {
    let not = match without {
        Some(_) => "AND search.rowid NOT IN (SELECT rowid FROM search WHERE search MATCH ?2)",
        None => "AND ?2 IS NULL",
    };
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT search.rowid, search_rows.id FROM search JOIN search_rows ON search_rows.row = search.rowid
             WHERE search MATCH ?1 {not}
             ORDER BY bm25(search, {TITLE_WEIGHT:.1}, 1.0), search.rowid LIMIT ?3 OFFSET ?4"
    ))?;
    let (offset, limit) = window.map_or((0, -1), |(o, l)| (o as i64, l as i64));
    let rows = stmt.query_map(params![expr, without, limit, offset], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

/// A hit's title and passage, the words `expr` matches marked.
fn passage(conn: &Connection, expr: &str, row: i64) -> rusqlite::Result<(Vec<Marked>, Vec<Marked>)> {
    let marked: Option<(String, String)> = conn
        .prepare_cached(&format!(
            "SELECT highlight(search, 0, char(2), char(3)), snippet(search, 1, char(2), char(3), '…', {PASSAGE_WORDS})
                 FROM search WHERE search MATCH ?1 AND rowid = ?2"
        ))?
        .query_row(params![expr, row], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(marked.map(|(t, s)| (stretches(&t), stretches(&s))).unwrap_or_default())
}

/// A highlighted string as its stretches — marked between [`OPEN`] and
/// [`CLOSE`] — empty ones dropped.
pub(super) fn stretches(s: &str) -> Vec<Marked> {
    let mut out = Vec::new();
    let mut hit = false;
    let mut text = String::new();
    for c in s.chars() {
        if c == OPEN || c == CLOSE {
            if !text.is_empty() {
                out.push(Marked { text: std::mem::take(&mut text), hit });
            }
            hit = c == OPEN;
        } else {
            text.push(c);
        }
    }
    if !text.is_empty() {
        out.push(Marked { text, hit });
    }
    out
}

/// SEARCH-2: one ticket a search matched — its row (the cache's), its title
/// and passage marked, and whether it took a correction to match.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub summary: TicketSummary,
    pub title: Vec<Marked>,
    pub snippet: Vec<Marked>,
    pub corrected: bool,
}

/// SEARCH-2: a page of a search — its hits, best first; how many tickets
/// match; the offset of the next page, if any match is past this one; the
/// words as read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    pub matched: u64,
    pub next: Option<u32>,
    pub words: Vec<SearchWord>,
}

impl Store {
    /// SEARCH-2: `search { query, filter, limit, offset }` — the tickets
    /// whose title or text holds every part of `query` ([`parts`]), those
    /// matched without a correction first, best first within each (BM25,
    /// the title weighed [`TITLE_WEIGHT`] to the text's 1); of them the
    /// ones `lens` admits; the page `offset … offset + limit` (`limit`
    /// [`SEARCH_LIMIT`] unless given, at most [`SEARCH_MAX`]), each with its
    /// title and passage marked. On a read connection, off the runtime, in
    /// one transaction; the rows from the cache, the lens tested in its
    /// cells (one lookup a match). A query holding no word matches nothing.
    pub async fn search(
        &self,
        query: &str,
        lens: Option<Lens>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<SearchPage, StoreError> {
        let parts = parts(query);
        if parts.is_empty() {
            return Ok(SearchPage::default());
        }
        let limit = limit.unwrap_or(SEARCH_LIMIT).min(SEARCH_MAX) as usize;
        let offset = offset.unwrap_or(0) as usize;
        let cache = Arc::clone(&self.tickets);
        self.readers
            .read(move |conn| {
                let tx = conn.transaction()?;
                let plan = plan(&tx, &parts)?;
                let (exact, fuzzy) = (plan.exact.as_str(), plan.fuzzy.as_deref());
                let (page, matched) = match lens {
                    // No filter: every match is counted (the corrected
                    // expression holds the exact one), and only the window
                    // is ranked — the uncorrected matches' part of it, then
                    // the corrected ones'.
                    None => {
                        let matched = count(&tx, fuzzy.unwrap_or(exact))?;
                        let first = if fuzzy.is_some() { count(&tx, exact)? } else { matched };
                        let mut window: Vec<(i64, String, bool)> = Vec::new();
                        if offset < first {
                            let take = limit.min(first - offset);
                            window.extend(ranked(&tx, exact, None, Some((offset, take)))?.into_iter().map(|(r, id)| (r, id, false)));
                        }
                        if let Some(fuzzy) = fuzzy.filter(|_| window.len() < limit) {
                            let at = offset.max(first) - first;
                            let take = limit - window.len();
                            window.extend(ranked(&tx, fuzzy, Some(exact), Some((at, take)))?.into_iter().map(|(r, id)| (r, id, true)));
                        }
                        let cache = cache.read().expect("store lock poisoned");
                        let page: Vec<(i64, Row, bool)> = window
                            .into_iter()
                            .filter_map(|(row, id, corrected)| {
                                let key = cache.at.get(&id)?;
                                Some((row, cache.order.get(key)?.clone(), corrected))
                            })
                            .collect();
                        (page, matched)
                    }
                    // A filter: every match ranked, the uncorrected first,
                    // and kept or not by the filter's cells.
                    Some(lens) => {
                        let mut all: Vec<(i64, String, bool)> =
                            ranked(&tx, exact, None, None)?.into_iter().map(|(r, id)| (r, id, false)).collect();
                        if let Some(fuzzy) = fuzzy {
                            all.extend(ranked(&tx, fuzzy, Some(exact), None)?.into_iter().map(|(r, id)| (r, id, true)));
                        }
                        let cache = cache.read().expect("store lock poisoned");
                        let lensed = Filtered {
                            lens,
                            epic: None,
                            status: None,
                            parent: None,
                            keep: None,
                        };
                        let admits = view(&cache, &lensed);
                        let mut page: Vec<(i64, Row, bool)> = Vec::new();
                        let mut matched = 0usize;
                        for (row, id, corrected) in all {
                            let Some(key) = cache.at.get(&id) else { continue };
                            if !admits.contains(&key.epic, key.seq) {
                                continue;
                            }
                            if matched >= offset && page.len() < limit {
                                if let Some(summary) = cache.order.get(key) {
                                    page.push((row, summary.clone(), corrected));
                                }
                            }
                            matched += 1;
                        }
                        (page, matched)
                    }
                };
                let marks = plan.fuzzy.as_deref().unwrap_or(&plan.exact);
                let hits = page
                    .into_iter()
                    .map(|(row, summary, corrected)| {
                        let (title, snippet) = passage(&tx, marks, row)?;
                        Ok(SearchHit { summary, title, snippet, corrected })
                    })
                    .collect::<Result<Vec<SearchHit>, StoreError>>()?;
                tx.commit()?;
                let shown = offset.saturating_add(hits.len());
                Ok(SearchPage {
                    next: (shown < matched).then_some(shown as u32),
                    matched: matched as u64,
                    hits,
                    words: plan.words,
                })
            })
            .await
    }
}
