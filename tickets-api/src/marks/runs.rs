//! COMP-11: a blank line inside a body's HTML does not end it.
//!
//! CommonMark ends an HTML block that opens with a block element or a
//! custom element (its types 6 and 7: `<div>`, `<section>`, `<m-summary>`
//! …) at the first blank line, and reads what follows as markdown — so HTML
//! written with blank lines between its sections falls apart: a line
//! indented four spaces becomes a code block, a line that starts with an
//! inline tag or text becomes a `<p>`. The rule here: **an element opened in
//! an HTML block runs to its close, blank lines included** — the markup
//! inside is passed through as written — with CommonMark's own idiom for
//! markdown inside HTML kept:
//!
//! - A blank line **right after an element's opening** — its open tag
//!   (`<div>` and then a blank line; `<m-summary ticket="X">` and then one),
//!   or that and its title, a `<summary>` or a heading on one line
//!   (GitHub's `<details>` idiom; a house section's `<h2>`) — still ends the
//!   block: what follows is markdown, until the close tag starts an HTML
//!   block of its own (CommonMark's example 4.6 — `<div>`, blank, `*Emphasized*
//!   text.`, blank, `</div>`). The house components hold markdown this way,
//!   hundreds of times in the tracker, and keep doing so. Unless the next
//!   line is indented as code (four spaces, a tab): code no element means —
//!   that is HTML going on.
//! - A blank line **before a fence** (```` ``` ```` or `~~~`) still ends the
//!   block: a fence with blank lines around it, inside an element, is
//!   markdown's — a diagram, a definition — as it has been.
//! - A blank line **anywhere else inside an open element** — after the
//!   element's content has begun as HTML, and in a `<pre>` at any depth
//!   (its text) — is part of the element: the
//!   block runs on, blank lines and all, until the element closes, and then,
//!   as CommonMark's block does, to the next blank line outside an element
//!   (each such blank line read by the same rule). Only when the element
//!   closes: one the rest of the body never closes, explicitly or by the
//!   close of an element it sits in, ends at the blank line, as before.
//! - A blank line inside a comment, a multi-line tag, or the text of a
//!   `script`, `style`, `textarea` or `title` nested in the block is part of
//!   it too (when it ends). A block that *starts* with one of those —
//!   CommonMark's types 1 to 5, `<pre>` included — already runs to its end
//!   marker: it is left alone, byte for byte.
//!
//! The elements are HTML's ([`HTML_ELEMENTS`]) and custom ones (a `-` in the
//! name); a void element or a self-closing `/>` holds nothing; a close tag
//! closes the nearest open element of its name and every element opened
//! inside it (HTML's own repair). A placeholder like `<org>` is no element.
//! Only an HTML block at the top of the body is read so — not one in a list
//! item or a block quote, whose lines carry a prefix.
//!
//! **How.** [`read`] returns the body with each such blank line marked —
//! [`MARK`], a noncharacter, appended to it — so the app's parser, run on
//! [`Read::source`] with the app's options, keeps the block going: one parse,
//! so link reference definitions and footnotes still reach across it. Every
//! mark lands inside an HTML block's lines; a reader takes them back out of
//! the `Html` events it gathers ([`Read::restore`]), and maps an offset in
//! the marked text back to the body ([`Read::original`]). A body that holds
//! [`MARK`] already is read as it is (no marks: nothing to confuse).
//!
//! Who reads with it: the frame's renderer (`frontend/src/md.rs`,
//! `render_body_framed`), and the server's readers of a body as the app
//! renders it — its fields and inputs ([`crate::fields`]) and its component
//! references (`store/components.rs`); `diagrams/src/blocks.rs` copies it.
//! An unframed body holds no element (that is what frames a body), so the
//! rule never changes one. Wasm-safe, like [`super`], whose tag reading it
//! shares.

use super::{comment_end, open_end, raw_close, tag_name, Budget, Walk, TAG_COST, VOID};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::borrow::Cow;

/// The mark a blank line inside an element carries: U+FDD0, a noncharacter
/// — never text a body means.
pub const MARK: char = '\u{FDD0}';

/// The HTML elements (WHATWG's list, obsolete ones a page still renders
/// included) and the two foreign roots a body may embed.
pub const HTML_ELEMENTS: &[&str] = &[
    "a", "abbr", "acronym", "address", "area", "article", "aside", "audio", "b", "base", "bdi", "bdo", "big",
    "blockquote", "body", "br", "button", "canvas", "caption", "center", "cite", "code", "col", "colgroup",
    "data", "datalist", "dd", "del", "details", "dfn", "dialog", "dir", "div", "dl", "dt", "em", "embed",
    "fieldset", "figcaption", "figure", "font", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "head",
    "header", "hgroup", "hr", "html", "i", "iframe", "img", "input", "ins", "kbd", "label", "legend", "li",
    "link", "main", "map", "mark", "marquee", "math", "menu", "meta", "meter", "nav", "noscript", "object",
    "ol", "optgroup", "option", "output", "p", "param", "picture", "pre", "progress", "q", "rp", "rt", "ruby",
    "s", "samp", "script", "search", "section", "select", "slot", "small", "source", "span", "strike",
    "strong", "style", "sub", "summary", "sup", "svg", "table", "tbody", "td", "template", "textarea",
    "tfoot", "th", "thead", "time", "title", "tr", "track", "tt", "u", "ul", "var", "video", "wbr",
];

/// Is `name` (ASCII-lowercased) an element: HTML's, or a custom one?
pub fn is_element(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic()) && (name.contains('-') || HTML_ELEMENTS.contains(&name))
}

/// The elements whose text is not markup: a tag inside is text.
const RAW_TEXT: [&str; 4] = ["script", "style", "textarea", "title"];

/// A body read for the parser: its blank lines inside elements marked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read<'a> {
    source: Cow<'a, str>,
    /// Where each mark sits in `source`, in order.
    marks: Vec<usize>,
}

impl Read<'_> {
    /// The text to parse: the body, each blank line inside an element
    /// carrying a [`MARK`].
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether any blank line was marked (the body parses as written when
    /// none was).
    pub fn marked(&self) -> bool {
        !self.marks.is_empty()
    }

    /// An offset into [`Read::source`] as an offset into the body.
    pub fn original(&self, at: usize) -> usize {
        at - MARK.len_utf8() * self.marks.partition_point(|&m| m < at)
    }

    /// Raw HTML from [`Read::source`]'s parse as the body writes it: its
    /// marks taken out.
    pub fn restore<'s>(&self, html: &'s str) -> Cow<'s, str> {
        if self.marks.is_empty() || !html.contains(MARK) {
            Cow::Borrowed(html)
        } else {
            Cow::Owned(html.replace(MARK, ""))
        }
    }
}

/// `src` read by the rule in the module doc, for a parser with `options`.
pub fn read<'a>(src: &'a str, options: Options, budget: &Budget) -> Option<Read<'a>> {
    let unmarked = Read { source: Cow::Borrowed(src), marks: Vec::new() };
    if !src.contains('<') || src.contains(MARK) {
        return Some(unmarked);
    }
    let mut text = src.to_string();
    let mut marks: Vec<usize> = Vec::new();
    // Blocks before `resume` are read already. V2-20: v1 read again from the
    // top after each block that ran on (what follows it may parse
    // differently now), a parse a block. One pass now marks every block it
    // can trust: what starts past the run's true end, with nothing open
    // across it, parses as it will once the run is marked. Anything that
    // straddles that end stops the pass, and the next reads it afresh.
    let mut resume = 0;
    loop {
        let mut depth = 0u32;
        if !budget.parse(text.len()) {
            return None;
        }
        let mut pending: Vec<usize> = Vec::new();
        // Where the last run truly ends: past the block its last mark joins.
        let mut trusted = resume;
        for (event, range) in Parser::new_ext(&text, options).into_offset_iter() {
            if !pending.is_empty() && matches!(event, Event::Start(_)) && range.start < trusted && range.end > trusted {
                break;
            }
            match event {
                Event::Start(Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::FootnoteDefinition(_)) => depth += 1,
                Event::End(TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition) => {
                    depth = depth.saturating_sub(1)
                }
                Event::Start(Tag::HtmlBlock) if depth == 0 && range.start >= trusted && !runs_to_its_end(&text[range.start..]) => {
                    let blanks = blank_lines_inside(&text, range.start, budget)?;
                    let Some(&last) = blanks.last() else { continue };
                    pending.extend_from_slice(&blanks);
                    trusted = run_end(&text, last);
                }
                _ => {}
            }
        }
        let Some(&last) = pending.last() else { break };
        let mut marked = String::with_capacity(text.len() + pending.len() * MARK.len_utf8());
        let mut from = 0;
        for &at in &pending {
            marked.push_str(&text[from..at]);
            marked.push(MARK);
            from = at;
        }
        marked.push_str(&text[from..]);
        text = marked;
        // Marks already placed sit before these: unmoved.
        let shift = MARK.len_utf8();
        marks.extend(pending.iter().enumerate().map(|(k, &at)| at + shift * k));
        resume = last + shift * pending.len();
    }
    if marks.is_empty() {
        return Some(unmarked);
    }
    Some(Read { source: Cow::Owned(text), marks })
}

/// Where the block holding the marked blank line at `last` ends once it is
/// marked: the next blank line after it, past which the body parses afresh.
fn run_end(text: &str, last: usize) -> usize {
    let mut at = text[last..].find('\n').map_or(text.len(), |j| last + j + 1);
    while at < text.len() {
        let end = text[at..].find('\n').map_or(text.len(), |j| at + j + 1);
        if text[at..end].trim().is_empty() {
            return end;
        }
        at = end;
    }
    text.len()
}

/// Does an HTML block starting here run to an end marker of its own —
/// CommonMark's types 1 to 5: `<pre>`, `<script>`, `<style>`, `<textarea>`,
/// a comment, a processing instruction, a declaration, CDATA?
fn runs_to_its_end(block: &str) -> bool {
    let line = block.trim_start_matches(' ');
    let low = line.get(..12).unwrap_or(line).to_ascii_lowercase();
    if low.starts_with("<!") || low.starts_with("<?") {
        return true;
    }
    ["pre", "script", "style", "textarea"].iter().any(|name| {
        low.strip_prefix('<')
            .and_then(|l| l.strip_prefix(name))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_whitespace() || c == '>'))
    })
}

/// Past the first line of what starts at `start`.
fn first_line(text: &str, start: usize) -> usize {
    text[start..].find('\n').map_or(text.len(), |j| start + j)
}

/// Whether `gap` holds a whole line of nothing.
fn holds_blank_line(gap: &str) -> bool {
    let mut lines = gap.split('\n');
    lines.next();
    let mut whole: Vec<&str> = lines.collect();
    whole.pop();
    whole.iter().any(|l| l.trim_matches([' ', '\t', '\r']).is_empty())
}

/// One element opened at or after the block's start: just past its open
/// tag, and where it closes (the `<` of the close tag that ends it, its own
/// or an enclosing element's) — `None` if nothing does.
struct Element {
    /// `pre` holds text: a blank line in it is never markdown's.
    pre: bool,
    content: usize,
    close: Option<usize>,
}

/// The blank lines of the HTML block starting at `start` in `text` that the
/// rule keeps inside it, in order — each as the offset its mark goes at (the
/// end of the line's whitespace).
fn blank_lines_inside(text: &str, start: usize, budget: &Budget) -> Option<Vec<usize>> {
    // Everything from the block's start to the end of the body, read as
    // markup: the elements, and the stretches a blank line is inside of
    // whatever the elements do — a comment, a tag, a raw-text element's text
    // — with whether each ends.
    let mut elements: Vec<Element> = Vec::new();
    let mut spans: Vec<(usize, usize, bool)> = Vec::new();
    let mut open: Vec<(String, usize)> = Vec::new();
    let mut i = start;
    while let Some(off) = text[i..].find('<') {
        if !budget.spend(Walk::Blocks, off / 8 + TAG_COST) {
            return None;
        }
        // V2-20: with nothing open, a blank line before the next tag is
        // outside everything: the lines below stop there, and what lies past
        // it is no part of this block. v1 read on to the body's end.
        if open.is_empty() && i > first_line(text, start) && holds_blank_line(&text[i..i + off]) {
            break;
        }
        let lt = i + off;
        if let Some(end) = comment_end(text, lt) {
            spans.push((lt, end, text[lt + 4..end].ends_with("-->")));
            i = end;
            continue;
        }
        if text[lt..].starts_with("<!") || text[lt..].starts_with("<?") {
            let end = text[lt..].find('>').map(|j| lt + j + 1);
            spans.push((lt, end.unwrap_or(text.len()), end.is_some()));
            i = end.unwrap_or(text.len());
            continue;
        }
        if text[lt..].starts_with("</") {
            if let Some((name, name_end)) = tag_name(text, lt + 1) {
                let gt = text[name_end..].find('>').map_or(text.len(), |j| name_end + j + 1);
                spans.push((lt, gt, gt <= text.len() && text[..gt].ends_with('>')));
                let found = open.iter().rposition(|(n, _)| *n == name);
                if !budget.spend(Walk::Open, (open.len() - found.unwrap_or(0)) * super::STEP_COST) {
                    return None;
                }
                if let Some(k) = found {
                    for (_, e) in open.drain(k..) {
                        elements[e].close = Some(lt);
                    }
                }
                i = gt;
                continue;
            }
        }
        let Some((name, name_end)) = tag_name(text, lt) else {
            i = lt + 1;
            continue;
        };
        let Some((gt, self_closing)) = open_end(text, name_end) else {
            // V2-20: a tag that never ends walked to the next `<`, or the end.
            if !budget.spend(Walk::Unended, text.len() - name_end) {
                return None;
            }
            i = lt + 1;
            continue;
        };
        spans.push((lt, gt, true));
        i = gt;
        if RAW_TEXT.contains(&name.as_str()) {
            let close = raw_close(text, &name, gt);
            let end = close.map_or(text.len(), |(_, after)| after);
            spans.push((gt, end, close.is_some()));
            i = end;
        } else if is_element(&name) && !self_closing && !VOID.contains(&name.as_str()) {
            elements.push(Element { pre: name == "pre", content: gt, close: None });
            open.push((name, elements.len() - 1));
        }
    }

    // The block's blank lines, in order, each kept or the block's end.
    let mut kept = Vec::new();
    let first_line_end = text[start..].find('\n').map_or(text.len(), |j| start + j + 1);
    let mut at = first_line_end;
    while at < text.len() {
        let line_end = text[at..].find('\n').map_or(text.len(), |j| at + j + 1);
        let line = &text[at..line_end];
        let content = line.trim_end_matches(['\n', '\r']);
        if content.trim_matches([' ', '\t']).is_empty() {
            // V2-20: what these walks cost, a blank line each.
            let ahead = text.len() - line_end - text[line_end..].trim_start().len();
            if !budget.spend(Walk::Blocks, ahead + elements.len()) {
                return None;
            }
            if text[line_end..].trim().is_empty() {
                break; // the body ends here: nothing left to keep together
            }
            let next = text[line_end..].lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            let indent = next.len() - next.trim_start_matches(' ').len();
            let next = if indent >= 4 || next[indent..].starts_with('\t') {
                Next::Code
            } else if next[indent..].starts_with("```") || next[indent..].starts_with("~~~") {
                Next::Fence
            } else {
                Next::Other
            };
            if !inside(text, at, next, &elements, &spans) {
                break;
            }
            kept.push(at + content.len());
        }
        at = line_end;
    }
    Some(kept)
}

/// Is this (an element's content up to a blank line, trimmed) only its
/// opening: nothing, or its title — a `<summary>` or a heading, on one line
/// (GitHub's `<details>` idiom; a house section's `<h2>`)?
fn opening(between: &str) -> bool {
    if between.is_empty() {
        return true;
    }
    let Some((name, _)) = tag_name(between, 0) else { return false };
    let title = name == "summary" || matches!(name.as_bytes(), [b'h', b'1'..=b'6']);
    let close = format!("</{name}>");
    title
        && !between.contains('\n')
        && between.len() > close.len()
        && between[between.len() - close.len()..].eq_ignore_ascii_case(&close)
}

/// What the line after a blank line (the next that is not blank) would be
/// as markdown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Next {
    /// Indented four spaces or a tab: code.
    Code,
    /// A fence opens: ```` ``` ```` or `~~~`.
    Fence,
    Other,
}

/// Is the blank line at `at` inside markup that goes on past it?
fn inside(text: &str, at: usize, next: Next, elements: &[Element], spans: &[(usize, usize, bool)]) -> bool {
    // In a comment, a tag, a raw-text element's text: kept, if it ends.
    // V2-20: they are in order and do not overlap: the last to start before it.
    let before = spans.partition_point(|&(s, _, _)| s < at);
    if let Some(&(_, e, ends)) = before.checked_sub(1).and_then(|k| spans.get(k)) {
        if at < e {
            return ends;
        }
    }
    // The elements open here; the innermost decides.
    let open = || elements.iter().filter(|e| e.content <= at && e.close.is_none_or(|c| c > at));
    let Some(element) = open().max_by_key(|e| e.content) else {
        return false;
    };
    // In a `<pre>`, at any depth, a blank line is its text.
    if open().any(|e| e.pre) {
        return element.close.is_some();
    }
    // A fence with blank lines around it is markdown's: a fence (a diagram,
    // a definition), as it has been.
    if next == Next::Fence {
        return false;
    }
    // Right after its opening: CommonMark's markdown inside HTML — unless
    // the markdown would be indented code, which no element means.
    if next != Next::Code && opening(text[element.content..at].trim_matches([' ', '\t', '\r', '\n'])) {
        return false;
    }
    element.close.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Options {
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_TABLES);
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TASKLISTS);
        opts.insert(Options::ENABLE_FOOTNOTES);
        opts
    }

    /// v1's rule, a parse from the top after each block that ran on: what the
    /// one pass answers is held to it.
    fn one_at_a_time(src: &str) -> String {
        let budget = Budget::new();
        let mut text = src.to_string();
        let mut resume = 0;
        'parse: loop {
            let mut depth = 0u32;
            for (event, range) in Parser::new_ext(&text, opts()).into_offset_iter() {
                match event {
                    Event::Start(Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::FootnoteDefinition(_)) => depth += 1,
                    Event::End(TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition) => depth = depth.saturating_sub(1),
                    Event::Start(Tag::HtmlBlock) if depth == 0 && range.start >= resume && !runs_to_its_end(&text[range.start..]) => {
                        let blanks = blank_lines_inside(&text, range.start, &budget).unwrap();
                        let Some(&last) = blanks.last() else { continue };
                        for &at in blanks.iter().rev() {
                            text.insert(at, MARK);
                        }
                        resume = last + MARK.len_utf8() * blanks.len();
                        continue 'parse;
                    }
                    _ => {}
                }
            }
            return text;
        }
    }

    #[test]
    fn one_pass_marks_what_one_at_a_time_marks() {
        const PIECES: [&str; 16] = [
            "<div>\n<p>a</p>\n\n<p>b</p>\n</div>\n\n",
            "plain words\n\n",
            "```\n<div>\n\n```\n\n",
            "<div>\n\n*x*\n\n</div>\n\n",
            "- item\n\n  <div>\n\n  </div>\n\n",
            "<section>\n<p>a</p>\n```\n\nfence inside\n</section>\n\n",
            "<p>open\n\ntail\n\n",
            "</div>\n",
            "<pre>\n\n</pre>\n\n",
            "> <div>\n\n",
            "<div>\n<span>\n\n</span>\n\n<i>\n\n</i>\n</div>\nafter\n\n",
            "<b>\n\nx\n</b>\n\n",
            "<div>\n<p>a</p>\n\n    indented\n</div>\nthe same block\n<ul>\n<li>x</li>\n\n<li>y</li>\n</ul>\n\n",
            "<!-- open\n\n",
            "--> shut\n\n",
            "~~~\n",
        ];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        for round in 0..4_000 {
            let src: String = (0..1 + next(12)).map(|_| PIECES[next(PIECES.len())]).collect();
            let got = read(&src, opts(), &Budget::new()).unwrap().source().to_string();
            assert_eq!(got.replace(MARK, "¶"), one_at_a_time(&src).replace(MARK, "¶"), "round {round}: {src:?}");
        }
    }

    /// The marked text with each mark shown as `¶`.
    fn marked(src: &str) -> String {
        read(src, opts(), &Budget::new()).unwrap().source().to_string().replace(MARK, "¶")
    }

    #[test]
    fn a_blank_line_inside_an_element_is_kept() {
        assert_eq!(marked("<div>\n<p>a</p>\n\n    <p>b</p>\n</div>\n\nafter\n"), "<div>\n<p>a</p>\n¶\n    <p>b</p>\n</div>\n\nafter\n");
        // Every one, nested, whitespace kept before the mark.
        assert_eq!(
            marked("<section>\n<div>\n<p>a</p>\n  \n<p>b</p>\n</div>\n\n<p>c</p>\n</section>\n"),
            "<section>\n<div>\n<p>a</p>\n  ¶\n<p>b</p>\n</div>\n¶\n<p>c</p>\n</section>\n"
        );
    }

    #[test]
    fn a_blank_line_right_after_an_open_tag_is_markdown() {
        for src in [
            "<div>\n\n*Emphasized* text.\n\n</div>\n",
            "<m-summary ticket=\"X\">\n\n<m-field data-field=\"standing\">**bold**</m-field>\n\n</m-summary>\n",
            "<m-diagram data-field=\"d\">\n\n```mermaid\ngraph LR\n```\n\n</m-diagram>\n",
            "<div class=\"a\"><div class=\"b\">  \n\ntext\n\n</div></div>\n",
        ] {
            assert_eq!(marked(src), src);
        }
        // An element opened inside one that holds markdown is read again.
        assert_eq!(marked("<div>\n\n<ul>\n<li>a</li>\n\n<li>b</li>\n</ul>\n\n</div>\n"), "<div>\n\n<ul>\n<li>a</li>\n¶\n<li>b</li>\n</ul>\n\n</div>\n");
    }

    #[test]
    fn an_element_nothing_closes_ends_at_the_blank_line() {
        for src in ["<div>\n<p>a</p>\n\nmarkdown *here*\n", "<p>one\n\ntwo\n", "<div>\nx\n\n</span>\n"] {
            assert_eq!(marked(src), src);
        }
        // An unclosed `<p>` closed by its parent's close is closed.
        assert_eq!(marked("<div>\n<p>one\n\n<p>two\n</div>\n"), "<div>\n<p>one\n¶\n<p>two\n</div>\n");
    }

    #[test]
    fn blocks_that_run_to_their_end_and_placeholders_are_left_alone() {
        for src in [
            "<pre>\na\n\n    b\n</pre>\n\nafter\n",
            "<script>\nlet a = 1;\n\nlet b = 2;\n</script>\n",
            "<style>\na{}\n\nb{}\n</style>\n",
            "<!--\na\n\nb\n-->\n",
            "<org>\nx\n\ny\n</org>\n",
            "* <div>\n  a\n\n  b\n  </div>\n",
            "> <div>\n> a\n>\n> b\n> </div>\n",
            "no html at all\n\nstill none\n",
        ] {
            assert_eq!(marked(src), src);
        }
    }

    #[test]
    fn comments_scripts_and_tags_nested_in_a_block_hold_their_blank_lines() {
        assert_eq!(marked("<div>\n<!-- a\n\nb -->\n</div>\n"), "<div>\n<!-- a\n¶\nb -->\n</div>\n");
        assert_eq!(marked("<div>\n<style>\na{}\n\nb{}\n</style>\n</div>\n"), "<div>\n<style>\na{}\n¶\nb{}\n</style>\n</div>\n");
        assert_eq!(marked("<div><a\n\nhref=\"x\">y</a>\n</div>\n"), "<div><a\n¶\nhref=\"x\">y</a>\n</div>\n");
        // A nested `<pre>` keeps its blank lines (they are its text), and a
        // tag-like word inside it is no element.
        assert_eq!(
            marked("<section>\n<h2>x</h2>\n  <pre>\nVec<String>\n\n  y\n</pre>\n</section>\n"),
            "<section>\n<h2>x</h2>\n  <pre>\nVec<String>\n¶\n  y\n</pre>\n</section>\n"
        );
    }

    #[test]
    fn the_block_runs_on_to_the_next_blank_line_outside_an_element() {
        // After the close, CommonMark's block goes on to a blank line; a
        // second element opened there is read the same way.
        assert_eq!(
            marked("<div>\n<p>a</p>\n\n<p>b</p>\n</div>\n<div>\n<p>c</p>\n\n<p>d</p>\n</div>\n\n# heading\n"),
            "<div>\n<p>a</p>\n¶\n<p>b</p>\n</div>\n<div>\n<p>c</p>\n¶\n<p>d</p>\n</div>\n\n# heading\n"
        );
        // A later block is read too, after a markdown stretch.
        assert_eq!(
            marked("<div>\n\ntext\n\n</div>\n\n<nav>\n<a>x</a>\n\n    <a>y</a>\n</nav>\n"),
            "<div>\n\ntext\n\n</div>\n\n<nav>\n<a>x</a>\n¶\n    <a>y</a>\n</nav>\n"
        );
    }

    #[test]
    fn offsets_map_back_and_marks_come_out() {
        let src = "<div>\n<p>a</p>\n\n<p>b</p>\n\n</div>\n";
        let read = read(src, opts(), &Budget::new()).unwrap();
        assert!(read.marked());
        let b = read.source().find("<p>b").unwrap();
        assert_eq!(read.original(b), src.find("<p>b").unwrap());
        assert_eq!(read.original(read.source().len()), src.len());
        let mut html = String::new();
        for event in Parser::new_ext(read.source(), opts()) {
            if let Event::Html(h) = event {
                html.push_str(&read.restore(&h));
            }
        }
        assert_eq!(html, src, "one HTML block, as written");
        // A body that holds the mark already is read as written.
        let odd = format!("<div>\n<p>{MARK}</p>\n\n<p>b</p>\n</div>\n");
        assert!(!read_marked(&odd));
    }

    fn read_marked(src: &str) -> bool {
        read(src, opts(), &Budget::new()).unwrap().marked()
    }
}
