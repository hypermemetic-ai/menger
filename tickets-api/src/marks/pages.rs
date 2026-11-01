//! APP-32: a page written in its element — `<m-mockup>` … the page …
//! `</m-mockup>` — is the page's markup, not the body's: [`blank`] spaces
//! each out, offsets and lines held, for the reader of the body's own. v2
//! draws no mockup, but a v1 body holds them, and an input in one is no input.

use super::{open_end, options, raw_close, runs, tag_name, Budget, Walk};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::borrow::Cow;

/// The element's name.
const TAG: &str = "m-mockup";

/// The most elements a body's reading blanks (APP-32's).
const READ_MAX: usize = 64;

/// Where the scan of an HTML block stands between its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Markup,
    Comment,
    Raw(String),
}

/// One element with content: past its open tag, its close tag's `<`, past
/// its close tag.
#[derive(Clone, Copy)]
struct Page {
    open: usize,
    close: usize,
    end: usize,
}

/// The element at `name_end`, or `None`: its open tag broken, self-closing,
/// never closed or holding only whitespace.
fn element(src: &str, name_end: usize) -> Option<Page> {
    let (open, closing) = open_end(src, name_end)?;
    if closing {
        return None;
    }
    let (close, end) = raw_close(src, TAG, open)?;
    (!src[open..close].trim().is_empty()).then_some(Page { open, close, end })
}

/// Scan `src[from..to]` from `state` for an element with content.
fn scan(src: &str, from: usize, to: usize, state: &mut State, budget: &Budget) -> Option<Page> {
    let mut i = from;
    while i < to && !budget.spent() {
        match state.clone() {
            State::Comment => match src[i..to].find("-->") {
                Some(j) => {
                    i += j + 3;
                    *state = State::Markup;
                }
                None => return None,
            },
            State::Raw(name) => match raw_close(&src[..to], &name, i) {
                Some((_, e)) => {
                    i = e;
                    *state = State::Markup;
                }
                None => return None,
            },
            State::Markup => {
                let j = src[i..to].find('<')?;
                let lt = i + j;
                if src[lt..to].starts_with("<!--") {
                    match src[lt + 4..to].find("-->") {
                        Some(j) => i = lt + 4 + j + 3,
                        None => {
                            *state = State::Comment;
                            return None;
                        }
                    }
                    continue;
                }
                let Some((name, name_end)) = tag_name(src, lt) else {
                    i = lt + 1;
                    continue;
                };
                if name == TAG {
                    // V2-20: `element` walks past `to`.
                    budget.spend(Walk::Unended, src.len() - name_end);
                    if let Some(hit) = element(src, name_end) {
                        return Some(hit);
                    }
                    i = name_end;
                    continue;
                }
                if matches!(name.as_str(), "script" | "style" | "textarea" | "title") {
                    budget.spend(Walk::Unended, src.len() - name_end);
                    match open_end(src, name_end) {
                        Some((e, false)) => {
                            i = e;
                            *state = State::Raw(name);
                        }
                        Some((e, true)) => i = e,
                        None => i = name_end,
                    }
                    continue;
                }
                i = name_end;
            }
        }
    }
    None
}

/// The first element with content at or past `base`, read by the app's
/// parser from `base` on. `Err`: the budget ran out.
fn first(src: &str, base: usize, budget: &Budget) -> Result<Option<Page>, ()> {
    let rest = &src[base..];
    let read = runs::read(rest, options(), budget).ok_or(())?;
    if !budget.parse(read.source().len()) {
        return Err(());
    }
    let mut state = State::Markup;
    for (event, range) in Parser::new_ext(read.source(), options()).into_offset_iter() {
        if budget.spent() {
            return Err(());
        }
        let range = base + read.original(range.start)..base + read.original(range.end);
        match event {
            Event::Start(Tag::HtmlBlock) | Event::End(TagEnd::HtmlBlock) => state = State::Markup,
            Event::Html(_) => {
                if let Some(hit) = scan(src, range.start, range.end, &mut state, budget) {
                    return Ok(Some(hit));
                }
            }
            Event::InlineHtml(_) => {
                let mut inline = State::Markup;
                if let Some(hit) = scan(src, range.start, range.end, &mut inline, budget) {
                    return Ok(Some(hit));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

/// `src` with each element's content blanked — every byte but a newline a
/// space, so offsets and lines hold. A body with none is borrowed. `None`:
/// the budget ran out.
pub(super) fn blank<'a>(src: &'a str, budget: &Budget) -> Option<Cow<'a, str>> {
    let needle = b"<m-mockup";
    if !src.as_bytes().windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle)) {
        return Some(Cow::Borrowed(src));
    }
    let mut pages = Vec::new();
    let mut at = 0;
    while pages.len() < READ_MAX {
        let Some(page) = first(src, at, budget).ok()? else { break };
        at = page.end;
        pages.push(page);
    }
    if pages.is_empty() {
        return Some(Cow::Borrowed(src));
    }
    let mut out = src.as_bytes().to_vec();
    for p in pages {
        for b in &mut out[p.open..p.close] {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    }
    Some(Cow::Owned(String::from_utf8(out).expect("ASCII spaces over whole characters")))
}
