//! APP-32: **a mockup written in its element** — `<m-mockup
//! widths="390,1440">` … a whole page … `</m-mockup>` — found in a body.
//!
//! APP-29 drew a page two ways: a ` ```mockup ` fence (its content the page,
//! never markdown) and `<m-mockup ref="X"></m-mockup>` (another ticket's
//! page). The element with the page as its content is the third, and the
//! standard one: the page sits beside the prose that discusses it.
//!
//! **The rule.** An `<m-mockup` open tag the app's parser reads as HTML — in
//! an HTML block or inline, never in a code span, a fence or indented code,
//! a comment, or the text of a `<script>`, `<style>`, `<textarea>` or
//! `<title>` — whose content up to the **first** `</m-mockup>` after it
//! holds anything but whitespace is an inline mockup ([`Inline`]): that
//! content, taken as written, is the page. Nothing in it is read as markdown
//! or as the body's HTML — blank lines, `<script>`, `</script>`,
//! `<template>`, `<!doctype html>` and `data-field`s are the page's. A page
//! that itself holds the text `</m-mockup>` is cut there: such a page goes in
//! a fence. An element with content **and** a `ref` is neither inline nor by
//! reference — a write refuses it ([`Found::conflicts`], by line); an empty
//! element, a self-closing one and one never closed are left as they are
//! (APP-29's `ref` form).
//!
//! **Its description** (APP-42: a mockup is one view, and what it shows is
//! said outside it). The element's `title` and `description` attributes,
//! and a `<m-description>` child that opens its content — markdown, a
//! paragraph or two, up to the first `</m-description>` — are the mockup's
//! caption, never the page's: the page starts past the child. An element
//! whose content is that child alone is no page: with a `ref` it is the
//! reference form described ([`Inline::reference`]); without one it names
//! no page, as an empty element does.
//!
//! **Who reads with it.** The frame's renderer (`frontend/src/md.rs`, which
//! puts each page into the placeholder a fence becomes), and the server's
//! readers of a body — its fields and inputs ([`crate::fields`]) and its
//! component references (`store/components.rs`) — which read the body with
//! each page blanked ([`blank`]): a page's markup is no part of the body's,
//! as a fence's is not. Wasm-safe, like [`crate::html_runs`].

use crate::component_docs::{attributes, open_end, raw_close, tag_name};
use crate::html_runs;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::borrow::Cow;

/// The element's tag.
pub const TAG: &str = "m-mockup";

/// The most inline mockups one body is read for. Past it the body is read
/// as written — each further element its own markup. (A body draws at most
/// 16 mockups — `frontend/src/mockup.rs` — so this is only a bound on the
/// work one body costs.)
pub const READ_MAX: usize = 64;

/// One inline mockup: offsets into the text [`find`] was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inline {
    /// Where its `<m-mockup` starts.
    pub start: usize,
    /// Just past its `</m-mockup>`.
    pub end: usize,
    /// Where the page starts (past the open tag, and one newline right
    /// after it) and ends (at the close tag).
    pub page: std::ops::Range<usize>,
    /// Its `widths` attribute, entities decoded — `None` when it has none.
    pub widths: Option<String>,
    /// The 1-based line its open tag is on.
    pub line: usize,
    /// APP-42: its `title` attribute, entities decoded (none when blank).
    pub title: Option<String>,
    /// APP-42: its `description` attribute and its `<m-description>`
    /// child's markdown, in that order, a blank line between — none when
    /// both are blank. The caption the host draws above the devices.
    pub description: Option<String>,
    /// APP-42: the `ref` of an element whose content is an `<m-description>`
    /// alone — the reference form, described; its `page` is empty.
    pub reference: Option<String>,
}

/// The child that opens an element's content and describes it.
pub const DESCRIPTION_TAG: &str = "m-description";

/// APP-42: the `<m-description>` that opens `src[open..close]` (whitespace
/// before it allowed): its markdown, trimmed, and where it ends. `None`
/// when the content opens with anything else, or the child is never closed
/// before `close`.
fn lead_description(src: &str, open: usize, close: usize) -> Option<(String, usize)> {
    let content = &src[open..close];
    let at = open + (content.len() - content.trim_start().len());
    let (name, name_end) = tag_name(src, at)?;
    if name != DESCRIPTION_TAG {
        return None;
    }
    let (inner, closing) = open_end(&src[..close], name_end)?;
    if closing {
        return Some((String::new(), inner));
    }
    let (c, e) = raw_close(&src[..close], DESCRIPTION_TAG, inner)?;
    Some((src[inner..c].trim().to_string(), e))
}

/// APP-42: a mockup's description from its `description` attribute and its
/// `<m-description>` child — each trimmed, the blank ones dropped, joined by
/// a blank line; `None` when nothing is left.
pub fn described(attribute: Option<&str>, child: Option<&str>) -> Option<String> {
    let parts: Vec<&str> = [attribute, child].into_iter().flatten().map(str::trim).filter(|p| !p.is_empty()).collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// APP-42: one attribute of a fence's info string past its language
/// (` ```mockup widths=390 title="Checkout" description="…" `), entities
/// decoded, as a tag's are; `None` when absent or blank.
pub fn info_attribute(info: &str, name: &str) -> Option<String> {
    let rest = info.trim_start();
    let rest = rest.split_once(char::is_whitespace).map_or("", |(_, r)| r);
    attributes(rest)
        .into_iter()
        .find(|(k, _)| k == name)
        .and_then(|(_, v)| v)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// What [`find`] reads in a body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// Each inline mockup, in order.
    pub inline: Vec<Inline>,
    /// An element with both content and a `ref`, each as [`Inline`] (its
    /// `page` the content) — a write refuses the body, naming the line.
    pub conflicts: Vec<Inline>,
}

impl Found {
    /// Every element with content, inline or conflicting, in body order.
    fn spans(&self) -> impl Iterator<Item = &Inline> {
        let mut all: Vec<&Inline> = self.inline.iter().chain(&self.conflicts).collect();
        all.sort_by_key(|m| m.start);
        all.into_iter()
    }
}

/// Does `src` name the tag at all (any case)? A body that does not is not
/// parsed.
pub fn mentions(src: &str) -> bool {
    let needle = b"<m-mockup";
    src.as_bytes().windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

/// The markdown dialect a body is read in (`frontend/src/md.rs`'s).
fn options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts
}

/// Where the scan of an HTML block stands between its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Markup,
    /// Inside a comment.
    Comment,
    /// Inside the raw text of this element.
    Raw(String),
}

/// An element with content at `p` (its `<`), or `None` — the open tag
/// broken, self-closing, never closed or holding only whitespace — and
/// whether it conflicts (a `ref` and a page both).
fn element(src: &str, p: usize, name_end: usize) -> Option<(Inline, bool)> {
    let (open, closing) = open_end(src, name_end)?;
    if closing {
        return None;
    }
    let (close, end) = raw_close(src, TAG, open)?;
    if src[open..close].trim().is_empty() {
        return None;
    }
    let inside = src[name_end..open - 1].trim_end_matches('/');
    let attrs = attributes(inside);
    let has = |n: &str| attrs.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone().unwrap_or_default());
    let reference = has("ref").map(|r| r.trim().to_string());
    let widths = has("widths").map(|w| w.trim().to_string()).filter(|w| !w.is_empty());
    let title = has("title").map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let lead = lead_description(src, open, close);
    let from = lead.as_ref().map_or(open, |(_, e)| *e);
    let description = described(has("description").as_deref(), lead.as_ref().map(|(d, _)| d.as_str()));
    let content = &src[from..close];
    let page = !content.trim().is_empty();
    let skip = if content.starts_with("\r\n") {
        2
    } else {
        usize::from(content.starts_with('\n'))
    };
    let line = 1 + src[..p].bytes().filter(|b| *b == b'\n').count();
    let range = if page { from + skip..close } else { close..close };
    let conflict = page && reference.is_some();
    let reference = if page { None } else { reference };
    Some((Inline { start: p, end, page: range, widths, line, title, description, reference }, conflict))
}

/// Scan `src[from..to]` (one line of an HTML block, or one inline tag) from
/// `state` for an element with content; the first, if any.
fn scan(src: &str, from: usize, to: usize, state: &mut State) -> Option<(Inline, bool)> {
    let mut i = from;
    while i < to {
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
                let Some(j) = src[i..to].find('<') else { return None };
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
                    if let Some(hit) = element(src, lt, name_end) {
                        return Some(hit);
                    }
                    i = name_end;
                    continue;
                }
                if matches!(name.as_str(), "script" | "style" | "textarea" | "title") {
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

/// The first element with content in `src` at or past `base`, read by the
/// app's parser from `base` on.
fn first(src: &str, base: usize) -> Option<(Inline, bool)> {
    let rest = &src[base..];
    let read = html_runs::read(rest, options());
    let mut state = State::Markup;
    for (event, range) in Parser::new_ext(read.source(), options()).into_offset_iter() {
        let range = base + read.original(range.start)..base + read.original(range.end);
        match event {
            Event::Start(Tag::HtmlBlock) | Event::End(TagEnd::HtmlBlock) => state = State::Markup,
            Event::Html(_) => {
                if let Some(hit) = scan(src, range.start, range.end, &mut state) {
                    return Some(hit);
                }
            }
            Event::InlineHtml(_) => {
                let mut inline = State::Markup;
                if let Some(hit) = scan(src, range.start, range.end, &mut inline) {
                    return Some(hit);
                }
            }
            _ => {}
        }
    }
    None
}

/// Every element with content in `src` (a body after its frontmatter) — see
/// the module doc. After each, the rest is read afresh from its close: the
/// page never steers how what follows it is read.
pub fn find(src: &str) -> Found {
    let mut found = Found::default();
    if !mentions(src) {
        return found;
    }
    let mut at = 0;
    while found.inline.len() + found.conflicts.len() < READ_MAX {
        let Some((m, reference)) = first(src, at) else { break };
        at = m.end;
        if reference {
            found.conflicts.push(m);
        } else {
            found.inline.push(m);
        }
    }
    found
}

/// `src` with each element's content blanked — every byte but a newline a
/// space, so offsets and lines hold — for a reader of the body's own markup
/// (fields, inputs, component references). A body with none is borrowed.
pub fn blank(src: &str) -> Cow<'_, str> {
    let found = find(src);
    if found.inline.is_empty() && found.conflicts.is_empty() {
        return Cow::Borrowed(src);
    }
    let mut out = src.as_bytes().to_vec();
    for m in found.spans() {
        let open = src[m.start..].find('>').map_or(m.start, |j| m.start + j + 1);
        let close = m.page.end;
        for b in &mut out[open..close] {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    }
    Cow::Owned(String::from_utf8(out).expect("ASCII spaces over whole characters"))
}

/// What a write refuses in `body` (whole, frontmatter included): each
/// element with both content and a `ref`, as `line N: …`.
pub fn problems(body: &str) -> Vec<String> {
    if !mentions(body) {
        return Vec::new();
    }
    let rest = crate::fields::split_frontmatter(body).1;
    let base_line = body[..body.len() - rest.len()].bytes().filter(|b| *b == b'\n').count();
    find(rest)
        .conflicts
        .iter()
        .map(|m| {
            format!(
                "line {}: <m-mockup> has both ref= and a page inside it — a mockup is either inline or by reference",
                m.line + base_line
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "<!doctype html>\n<html><head><script>\nlet a = \"</p>\";\n\n</script></head>\n<body><template><b>t</b></template>\n\n  <p>x</p></body></html>\n";

    #[test]
    fn an_element_with_content_is_its_page_as_written() {
        let src = format!("Intro.\n\n<m-mockup widths=\"390,1440\">\n{PAGE}</m-mockup>\n\nAfter.\n");
        let f = find(&src);
        assert_eq!(f.inline.len(), 1, "{f:?}");
        let m = &f.inline[0];
        assert_eq!(&src[m.page.clone()], PAGE);
        assert_eq!(m.widths.as_deref(), Some("390,1440"));
        assert_eq!(m.line, 3);
        assert_eq!(&src[m.start..m.end], &format!("<m-mockup widths=\"390,1440\">\n{PAGE}</m-mockup>"));
    }

    #[test]
    fn code_comments_and_raw_text_hold_no_mockup() {
        for src in [
            "`<m-mockup>x</m-mockup>`\n",
            "```html\n<m-mockup>x</m-mockup>\n```\n",
            "    <m-mockup>x</m-mockup>\n",
            "<!-- <m-mockup>x</m-mockup> -->\n",
            "<!--\n<m-mockup>x</m-mockup>\n-->\n",
            "<script>\nlet s = '<m-mockup>x</m-mockup>';\n</script>\n",
            "the word m-mockup in prose\n",
        ] {
            assert_eq!(find(src), Found::default(), "{src:?}");
        }
    }

    #[test]
    fn empty_self_closing_and_unclosed_elements_are_left_alone() {
        for src in [
            "<m-mockup ref=\"DESIGN-49\"></m-mockup>\n",
            "<m-mockup ref=\"DESIGN-49\">\n\n</m-mockup>\n",
            "<m-mockup ref=\"DESIGN-49\" />\n",
            "<m-mockup widths=\"390\">\n<p>never closed</p>\n",
        ] {
            assert_eq!(find(src), Found::default(), "{src:?}");
        }
    }

    #[test]
    fn the_first_close_ends_it_and_the_rest_reads_afresh() {
        let src = "<m-mockup>\n<p>a</p>\n</m-mockup><p>b</p></m-mockup>\n\nInline <M-MOCKUP widths=1440><p>c</p></m-mockup> too.\n";
        let f = find(src);
        assert_eq!(f.inline.len(), 2, "{f:?}");
        assert_eq!(&src[f.inline[0].page.clone()], "<p>a</p>\n");
        assert_eq!(&src[f.inline[1].page.clone()], "<p>c</p>");
        assert_eq!(f.inline[1].widths.as_deref(), Some("1440"));
    }

    #[test]
    fn a_page_that_opens_a_fence_or_a_comment_does_not_swallow_the_body() {
        let src = "<m-mockup>\n```\n<!--\n</m-mockup>\n\n<m-mockup widths=390>\n<p>second</p>\n</m-mockup>\n";
        let f = find(src);
        assert_eq!(f.inline.len(), 2, "{f:?}");
        assert_eq!(&src[f.inline[1].page.clone()], "<p>second</p>\n");
    }

    #[test]
    fn ref_and_content_conflict_naming_the_line() {
        let body = "---\nview: inline\n---\nIntro.\n\n<m-mockup ref=\"DESIGN-49\">\n<p>x</p>\n</m-mockup>\n";
        let f = find(crate::fields::split_frontmatter(body).1);
        assert!(f.inline.is_empty());
        assert_eq!(f.conflicts.len(), 1);
        assert_eq!(
            problems(body),
            vec!["line 6: <m-mockup> has both ref= and a page inside it — a mockup is either inline or by reference".to_string()]
        );
        assert!(problems("<m-mockup ref=\"X-1\"></m-mockup>\n<m-mockup>\n<p>p</p>\n</m-mockup>\n").is_empty());
    }

    #[test]
    fn blanking_keeps_offsets_and_lines_and_hides_the_page() {
        let src = format!("<m-field data-field=\"a\">\n\n<m-mockup>\n<m-decision id=\"x\"></m-decision>\n{PAGE}</m-mockup>\n\n</m-field>\n");
        let b = blank(&src);
        assert_eq!(b.len(), src.len());
        assert_eq!(b.matches('\n').count(), src.matches('\n').count());
        assert!(!b.contains("m-decision") && !b.contains("doctype"), "{b}");
        assert!(b.contains("<m-mockup>") && b.contains("</m-mockup>"));
        assert!(matches!(blank("no mockup <b>here</b>"), Cow::Borrowed(_)));
    }

    /// APP-42: the caption is the element's, never the page's — its
    /// `title` and `description` attributes and an `<m-description>` that
    /// opens its content; the page starts past the child.
    #[test]
    fn a_description_is_the_mockups_and_the_page_starts_past_it() {
        let src = format!(
            "<m-mockup widths=\"390\" title=\"Checkout\" description=\"The cart, one item.\">\n  <m-description>\nWhat the **buyer** sees first.\n\nThe total is live.\n</m-description>\n{PAGE}</m-mockup>\n"
        );
        let f = find(&src);
        assert_eq!(f.inline.len(), 1, "{f:?}");
        let m = &f.inline[0];
        assert_eq!(&src[m.page.clone()], PAGE);
        assert_eq!(m.title.as_deref(), Some("Checkout"));
        assert_eq!(m.description.as_deref(), Some("The cart, one item.\n\nWhat the **buyer** sees first.\n\nThe total is live."));
        assert_eq!(m.reference, None);
        // Blanked with the page: a reader of the body's own markup sees neither.
        assert!(!blank(&src).contains("buyer"));
        // An m-description further in is the page's own.
        let later = "<m-mockup>\n<p>x</p>\n<m-description>no</m-description>\n</m-mockup>\n";
        let m = &find(later).inline[0];
        assert_eq!(m.description, None);
        assert!(later[m.page.clone()].contains("<m-description>no"));
    }

    /// APP-42: a `ref` whose content is the description alone is the
    /// reference form, described — no conflict; a description and no page
    /// nor ref names nothing (the host says so).
    #[test]
    fn a_described_reference_is_no_conflict() {
        let src = "<m-mockup ref=\"DESIGN-49\">\n<m-description>The landing, dark.</m-description>\n</m-mockup>\n";
        let f = find(src);
        assert!(f.conflicts.is_empty(), "{f:?}");
        let m = &f.inline[0];
        assert_eq!(m.reference.as_deref(), Some("DESIGN-49"));
        assert_eq!(m.description.as_deref(), Some("The landing, dark."));
        assert!(m.page.is_empty());
        assert!(problems(src).is_empty());
        let none = find("<m-mockup><m-description>only words</m-description></m-mockup>\n");
        assert!(none.inline[0].page.is_empty() && none.inline[0].reference.is_none());
        // A ref with a page after its description still conflicts.
        let both = "<m-mockup ref=\"X-1\"><m-description>d</m-description><p>p</p></m-mockup>\n";
        assert_eq!(find(both).conflicts.len(), 1);
    }

    #[test]
    fn a_fence_names_its_caption_in_its_info_string() {
        let info = "mockup widths=390,1440 title=\"Checkout &amp; pay\" description='One view: the cart.'";
        assert_eq!(info_attribute(info, "widths").as_deref(), Some("390,1440"));
        assert_eq!(info_attribute(info, "title").as_deref(), Some("Checkout & pay"));
        assert_eq!(info_attribute(info, "description").as_deref(), Some("One view: the cart."));
        assert_eq!(info_attribute("mockup", "title"), None);
        assert_eq!(info_attribute("mockup title=\"  \"", "title"), None);
        assert_eq!(described(Some(" a "), Some("")), Some("a".into()));
        assert_eq!(described(None, Some("b")), Some("b".into()));
        assert_eq!(described(Some(""), None), None);
    }

    #[test]
    fn reading_stops_at_its_bound() {
        let src = "<m-mockup>\n<p>p</p>\n</m-mockup>\n\n".repeat(READ_MAX + 3);
        assert_eq!(find(&src).inline.len(), READ_MAX);
    }
}
