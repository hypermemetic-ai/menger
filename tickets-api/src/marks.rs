//! INPUT-3, V2-20: a body's **marked elements**, read as the app renders the
//! body — v1's reader (ART-5's scan, COMP-11's runs, APP-32's pages), which
//! is what a v1 file's inputs were read by: the frontmatter split off, the
//! rest through the app's markdown parser with its options, raw HTML only (a
//! code span, a fence or indented code is text), comments and raw text
//! skipped (`script`, `style`, `textarea`, `title`), a page inside an
//! `<m-mockup>` the page's own, each element's extent the one a browser
//! builds. Wasm-safe.
//!
//! **What v2 adds: a reading is bounded.** v1's walks were quadratic or
//! worse in a hostile body, and a body may be 8 MiB. Every walk a body can
//! lengthen spends a [`Budget`], elements borrow their content, and nesting
//! stops at [`DEPTH_MAX`]; past either the reading is [`Beyond`], which a
//! write refuses in band. So a reading costs at most the budget, whatever
//! the body. A body with no input markup is not read at all.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::cell::Cell;

mod pages;
mod runs;

/// How deep marked elements nest.
pub const DEPTH_MAX: u32 = 64;

/// What one reading may walk, in bytes looked at: an 8 MiB body read once
/// costs about a third of it, and the whole of it about 0.7 s.
const WORK_MAX: u64 = 768 << 20;

/// A parse costs more than a walk: what a byte parsed counts as.
const PARSE_COST: u64 = 16;

/// …and a step up the open elements, which compares names.
const STEP_COST: usize = 32;

/// What is left of a reading's allowance.
pub struct Budget(Cell<u64>);

impl Default for Budget {
    fn default() -> Self {
        Self::new()
    }
}

impl Budget {
    pub fn new() -> Self {
        Self(Cell::new(WORK_MAX))
    }

    /// `n` more spent; whether any is left.
    pub fn spend(&self, n: usize) -> bool {
        self.0.set(self.0.get().saturating_sub(n as u64));
        self.0.get() > 0
    }

    /// A parse of `n` bytes spent.
    pub fn parse(&self, n: usize) -> bool {
        self.spend((n as u64).saturating_mul(PARSE_COST) as usize)
    }

    /// Whether it is gone.
    pub fn spent(&self) -> bool {
        self.0.get() == 0
    }
}

/// Why a body was not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beyond {
    /// Its marked elements nest past [`DEPTH_MAX`].
    Deep,
    /// Reading it would walk past the budget.
    Complex,
}

/// One element of the tags asked for, with the ones inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element<'b> {
    /// Its tag, ASCII-lowercased.
    pub tag: String,
    /// Its open tag's attributes, as written — names lowercased, values with
    /// their entities decoded; `None` for a bare name.
    attrs: Vec<(String, Option<String>)>,
    /// What it holds, as written.
    pub content: &'b str,
    /// The named elements inside it, outermost first, in order.
    pub parts: Vec<Element<'b>>,
}

impl Element<'_> {
    /// The attribute `name`'s value — `Some("")` for a bare name.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_deref().unwrap_or(""))
    }
}

/// Every element whose tag is one of `tags` (lowercase) — see the module
/// doc — outermost first, each with the named elements inside it as its
/// parts.
pub fn marked<'b>(body: &'b str, tags: &'static [&'static str]) -> Result<Vec<Element<'b>>, Beyond> {
    let (_, rest) = split_frontmatter(body);
    let at = body.len() - rest.len();
    let scan = Scan::read(rest, at, tags)?;
    Ok(scan.top.iter().map(|&id| scan.element(id, body)).collect())
}

/// Where `body`'s scan stands: the marked elements found, and every element
/// open — raw HTML's, and the ones the markdown blocks around it render as.
#[derive(Default)]
struct Scan {
    /// The tags it marks.
    tags: &'static [&'static str],
    /// V2-20: the walks' allowance, and whether nesting went past its bound.
    budget: Budget,
    too_deep: bool,
    nodes: Vec<Node>,
    /// The outermost marked elements, in order.
    top: Vec<usize>,
    stack: Vec<Open>,
    /// The markdown blocks open, innermost last — each one's id.
    md: Vec<u32>,
    next_md: u32,
    /// Inside a `script`, `style`, `textarea` or `title`: its name.
    raw: Option<String>,
    /// Inside a comment an earlier stretch of HTML opened.
    comment: bool,
}

/// A marked element, as the scan finds it. Offsets are the body's.
struct Node {
    tag: String,
    /// How many marked elements it sits in.
    depth: u32,
    /// Its open tag's attributes.
    attrs: Vec<(String, Option<String>)>,
    /// Just past its open tag's `>`.
    content: usize,
    /// Where its content ends — `None` while it is open.
    end: Option<usize>,
    parts: Vec<usize>,
}

/// An element open: its tag, the marked element it is (if it is one), and
/// the markdown block it is (if it is one).
struct Open {
    name: String,
    node: Option<usize>,
    md: Option<u32>,
}

/// The elements a close tag cannot reach past ("has an element in scope").
const SCOPE: [&str; 9] = ["applet", "caption", "html", "table", "td", "th", "marquee", "object", "template"];

/// HTML's "special" elements: a close tag of any other name stops at one.
const SPECIAL: [&str; 82] = [
    "address", "applet", "area", "article", "aside", "base", "basefont", "bgsound", "blockquote", "body", "br",
    "button", "caption", "center", "col", "colgroup", "dd", "details", "dir", "div", "dl", "dt", "embed",
    "fieldset", "figcaption", "figure", "footer", "form", "frame", "frameset", "h1", "h2", "h3", "h4", "h5", "h6",
    "head", "header", "hgroup", "hr", "html", "iframe", "img", "input", "keygen", "li", "link", "listing", "main",
    "marquee", "menu", "meta", "nav", "noembed", "noframes", "noscript", "object", "ol", "p", "param",
    "plaintext", "pre", "script", "search", "section", "select", "source", "style", "summary", "table", "tbody",
    "td", "template", "textarea", "tfoot", "th", "thead", "title", "tr", "track", "ul", "wbr",
];

/// The block elements whose open tag closes an open `<p>`.
const CLOSES_P: [&str; 37] = [
    "address", "article", "aside", "blockquote", "center", "details", "dialog", "dir", "div", "dl", "fieldset",
    "figcaption", "figure", "footer", "header", "hgroup", "main", "menu", "nav", "ol", "p", "search", "section",
    "summary", "ul", "h1", "h2", "h3", "h4", "h5", "h6", "pre", "listing", "form", "table", "hr", "xmp",
];

fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

impl Scan {
    /// Every marked element in `rest` (the body after its frontmatter,
    /// which starts `at` bytes into it).
    fn read(rest: &str, at: usize, tags: &'static [&'static str]) -> Result<Scan, Beyond> {
        let mut scan = Scan { tags, budget: Budget::new(), too_deep: false, ..Scan::default() };
        // An HTML block's lines, gathered so a tag or a comment across them
        // is one — with where each line starts in the gathered text and in
        // the body (a line in a list item or a block quote has its prefix
        // dropped, so the two drift).
        let mut block: Option<(String, Vec<(usize, usize)>)> = None;
        // COMP-11: an element opened in an HTML block runs to its close,
        // blank lines included, as the frame renders it — the parser reads
        // the body with those blank lines marked; offsets and lines map
        // back (a mark ends its line, so a line's offsets still run on).
        // APP-32: a page inside an `<m-mockup>` is the page's, not the
        // body's markup — read blanked, offsets and lines as they were.
        let blanked = pages::blank(rest, &scan.budget).ok_or(Beyond::Complex)?;
        let read = runs::read(&blanked, options(), &scan.budget).ok_or(Beyond::Complex)?;
        if !scan.budget.parse(read.source().len()) {
            return Err(Beyond::Complex);
        }
        for (event, range) in Parser::new_ext(read.source(), options()).into_offset_iter() {
            if scan.budget.spent() {
                return Err(Beyond::Complex);
            }
            if scan.too_deep {
                return Err(Beyond::Deep);
            }
            let range = read.original(range.start)..read.original(range.end);
            match event {
                Event::Start(Tag::HtmlBlock) => block = Some((String::new(), Vec::new())),
                Event::End(TagEnd::HtmlBlock) => {
                    if let Some((html, lines)) = block.take() {
                        scan.html(&html, &lines);
                    }
                }
                Event::Html(text) => match &mut block {
                    Some((html, lines)) => {
                        lines.push((html.len(), at + range.start));
                        html.push_str(&read.restore(&text));
                    }
                    None => scan.html(&text, &[(0, at + range.start)]),
                },
                Event::InlineHtml(text) => scan.html(&text, &[(0, at + range.start)]),
                Event::Start(tag) => scan.md_start(rendered_as(&tag)),
                Event::End(_) => scan.md_end(at + range.end),
                _ => {}
            }
        }
        match (scan.budget.spent(), scan.too_deep) {
            (true, _) => Err(Beyond::Complex),
            (_, true) => Err(Beyond::Deep),
            _ => Ok(scan),
        }
    }

    /// One stretch of raw HTML: `starts` maps its offsets to the body's.
    fn html(&mut self, html: &str, starts: &[(usize, usize)]) {
        let body_at = |i: usize| {
            let k = starts.partition_point(|s| s.0 <= i).max(1) - 1;
            starts[k].1 + (i - starts[k].0)
        };
        let mut i = 0;
        if self.comment {
            match html.find("-->") {
                Some(j) => {
                    self.comment = false;
                    i = j + 3;
                }
                None => return,
            }
        }
        loop {
            if self.budget.spent() {
                return;
            }
            if let Some(raw) = self.raw.clone() {
                match raw_close(html, &raw, i) {
                    Some((lt, after)) => {
                        self.end(&raw, body_at(lt));
                        self.raw = None;
                        i = after;
                    }
                    None => return,
                }
            }
            let Some(off) = html[i..].find('<') else { return };
            let lt = i + off;
            if html[lt..].starts_with("<!--") {
                match comment_end(html, lt) {
                    Some(after) if html[lt + 4..].contains("-->") => i = after,
                    _ => {
                        self.comment = true;
                        return;
                    }
                }
                continue;
            }
            if html[lt + 1..].starts_with('/') {
                match tag_name(html, lt + 1) {
                    Some((name, name_end)) => {
                        self.end(&name, body_at(lt));
                        i = html[name_end..].find('>').map_or(html.len(), |j| name_end + j + 1);
                    }
                    None => i = lt + 2,
                }
                continue;
            }
            let Some((name, name_end)) = tag_name(html, lt) else {
                i = lt + 1;
                continue;
            };
            let Some((gt, self_closing)) = open_end(html, name_end) else {
                // V2-20: a tag that never ends walked to the next `<`, or the end.
                self.budget.spend(html.len() - name_end);
                i = lt + 1;
                continue;
            };
            let marked = self.tags.contains(&name.as_str());
            let attrs = if marked { attributes(html[name_end..gt - 1].trim_end_matches('/')) } else { Vec::new() };
            self.start(name, marked, attrs, body_at(lt), body_at(gt), self_closing);
            i = gt;
        }
    }

    /// An open tag `<name …>` at `start`, its content from `content`.
    fn start(
        &mut self,
        name: String,
        marked: bool,
        attrs: Vec<(String, Option<String>)>,
        start: usize,
        content: usize,
        self_closing: bool,
    ) {
        if CLOSES_P.contains(&name.as_str()) {
            self.close_p(start);
        }
        match name.as_str() {
            "li" => self.close_item(&["li"], start),
            "dd" | "dt" => self.close_item(&["dd", "dt"], start),
            _ => {}
        }
        let node = marked.then(|| {
            let id = self.nodes.len();
            let parent = self.stack.iter().rev().find_map(|o| o.node);
            self.budget.spend(self.stack.len() * STEP_COST);
            let depth = parent.map_or(0, |p| self.nodes[p].depth + 1);
            self.too_deep |= depth >= DEPTH_MAX;
            match parent {
                Some(parent) => self.nodes[parent].parts.push(id),
                None => self.top.push(id),
            }
            self.nodes.push(Node { tag: name.clone(), depth, attrs, content, end: None, parts: Vec::new() });
            id
        });
        if self_closing || VOID.contains(&name.as_str()) {
            if let Some(id) = node {
                self.nodes[id].end = Some(content);
            }
            return;
        }
        if matches!(name.as_str(), "script" | "style" | "textarea" | "title") {
            self.raw = Some(name.clone());
        }
        self.stack.push(Open { name, node, md: None });
    }

    /// A close tag `</name>` at `at`: it closes the nearest open `name`
    /// it can reach, and everything open inside that.
    fn end(&mut self, name: &str, at: usize) {
        for k in (0..self.stack.len()).rev() {
            self.budget.spend(STEP_COST);
            let open = self.stack[k].name.as_str();
            if open == name || (is_heading(name) && is_heading(open)) {
                self.pop_to(k, at);
                return;
            }
            let stops = match name {
                "p" => SCOPE.contains(&open) || open == "button",
                "li" => SCOPE.contains(&open) || open == "ol" || open == "ul",
                "td" | "th" | "tr" | "thead" | "tbody" | "tfoot" | "caption" => open == "table" || open == "template",
                "table" => open == "template",
                n if is_heading(n) || CLOSES_P.contains(&n) || matches!(n, "button" | "dd" | "dt" | "dialog") => {
                    SCOPE.contains(&open)
                }
                _ => SPECIAL.contains(&open),
            };
            if stops {
                return;
            }
        }
    }

    /// An open `<p>` closed by a block opening at `at`.
    fn close_p(&mut self, at: usize) {
        for k in (0..self.stack.len()).rev() {
            self.budget.spend(STEP_COST);
            let open = self.stack[k].name.as_str();
            if open == "p" {
                self.pop_to(k, at);
                return;
            }
            if SCOPE.contains(&open) || open == "button" {
                return;
            }
        }
    }

    /// An open item (`names`) closed by the next opening at `at`.
    fn close_item(&mut self, names: &[&str], at: usize) {
        for k in (0..self.stack.len()).rev() {
            self.budget.spend(STEP_COST);
            let open = self.stack[k].name.as_str();
            if names.contains(&open) {
                self.pop_to(k, at);
                return;
            }
            if SPECIAL.contains(&open) && !matches!(open, "address" | "div" | "p") {
                return;
            }
        }
    }

    /// A markdown block opens, rendered as `name`.
    fn md_start(&mut self, name: &str) {
        let id = self.next_md;
        self.next_md += 1;
        self.md.push(id);
        self.stack.push(Open {
            name: name.to_string(),
            node: None,
            md: Some(id),
        });
    }

    /// The innermost markdown block ends at `at`, and closes whatever is
    /// still open inside it — unless raw HTML closed it already.
    fn md_end(&mut self, at: usize) {
        let Some(id) = self.md.pop() else { return };
        let found = self.stack.iter().rposition(|o| o.md == Some(id));
        self.budget.spend((self.stack.len() - found.unwrap_or(0)) * STEP_COST);
        if let Some(k) = found {
            self.pop_to(k, at);
        }
    }

    /// Close the element at `k` and everything above it, at `at`.
    fn pop_to(&mut self, k: usize, at: usize) {
        for open in self.stack.drain(k..) {
            if let Some(id) = open.node {
                self.nodes[id].end.get_or_insert(at);
            }
            if self.raw.as_deref() == Some(open.name.as_str()) {
                self.raw = None;
            }
        }
    }

    /// The element `id`, with its parts: its content a slice of `body`.
    fn element<'b>(&self, id: usize, body: &'b str) -> Element<'b> {
        let n = &self.nodes[id];
        let end = n.end.unwrap_or(body.len()).max(n.content);
        Element {
            tag: n.tag.clone(),
            attrs: n.attrs.clone(),
            content: body.get(n.content..end).unwrap_or_default(),
            parts: n.parts.iter().map(|&p| self.element(p, body)).collect(),
        }
    }
}

/// The element a markdown block renders as (pulldown-cmark's `push_html`).
fn rendered_as(tag: &Tag) -> &'static str {
    match tag {
        Tag::Paragraph => "p",
        Tag::Heading { level, .. } => match level {
            HeadingLevel::H1 => "h1",
            HeadingLevel::H2 => "h2",
            HeadingLevel::H3 => "h3",
            HeadingLevel::H4 => "h4",
            HeadingLevel::H5 => "h5",
            HeadingLevel::H6 => "h6",
        },
        Tag::BlockQuote(_) => "blockquote",
        Tag::CodeBlock(_) => "pre",
        Tag::List(Some(_)) => "ol",
        Tag::List(None) => "ul",
        Tag::Item => "li",
        Tag::Table(_) => "table",
        Tag::TableHead => "thead",
        Tag::TableRow => "tr",
        Tag::TableCell => "td",
        Tag::Emphasis => "em",
        Tag::Strong => "strong",
        Tag::Strikethrough => "del",
        Tag::Link { .. } => "a",
        Tag::Image { .. } => "img",
        // A footnote's definition, and what the app's options never make.
        _ => "div",
    }
}

/// HTML's void elements: an open tag with no close tag to look for. (ART-5:
/// [`crate::fields`] reads tags with these helpers too.)
pub(crate) const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr",
];

/// Past an HTML comment opening at `at`, if one does (an unclosed one runs
/// to the end).
pub(crate) fn comment_end(s: &str, at: usize) -> Option<usize> {
    s[at..]
        .starts_with("<!--")
        .then(|| s[at + 4..].find("-->").map_or(s.len(), |j| at + 4 + j + 3))
}

/// The tag `<name` opening at `lt` — its name, ASCII-lowercased, and where
/// the name ends — if `lt` opens one (a letter after `<`; `</` closes).
pub(crate) fn tag_name(s: &str, lt: usize) -> Option<(String, usize)> {
    let start = lt + 1;
    if !s.as_bytes().get(start).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    let end = s[start..]
        .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
        .map_or(s.len(), |j| start + j);
    Some((s[start..end].to_ascii_lowercase(), end))
}

/// Past the `>` that ends an open tag whose name ends at `from`, outside
/// quoted values — and whether it is self-closing (`/>`). `None` when a `<`
/// comes first, or nothing ends it: not a tag.
pub(crate) fn open_end(s: &str, from: usize) -> Option<(usize, bool)> {
    let mut quote: Option<u8> = None;
    let bytes = s.as_bytes();
    for (j, &b) in bytes.iter().enumerate().skip(from) {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'<' => return None,
                b'>' => return Some((j + 1, j > from && bytes[j - 1] == b'/')),
                _ => {}
            },
        }
    }
    None
}

/// Whether `s` at `at` is `name` as a whole tag name (followed by
/// whitespace, `/`, `>` or the end).
fn name_at(s: &str, at: usize, name: &str) -> bool {
    s.get(at..at + name.len()).is_some_and(|n| n.eq_ignore_ascii_case(name))
        && s[at + name.len()..]
            .chars()
            .next()
            .is_none_or(|c| c.is_ascii_whitespace() || c == '/' || c == '>')
}

/// The close tag of a raw-text element (`script`, `style`): the first
/// `</name`, nothing nested.
pub(crate) fn raw_close(s: &str, name: &str, from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while let Some(off) = s[i..].find("</") {
        let lt = i + off;
        if name_at(s, lt + 2, name) {
            return Some((lt, s[lt..].find('>').map_or(s.len(), |j| lt + j + 1)));
        }
        i = lt + 2;
    }
    None
}

/// The attributes of a tag's inside (after its name): `name`, `name=value`,
/// `name="value"`, `name='value'` — names ASCII-lowercased, values with
/// HTML's common entities decoded; `None` for a bare name.
pub(crate) fn attributes(s: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return out;
        }
        let name_len = rest
            .find(|c: char| c.is_whitespace() || c == '=' || c == '/' || c == '>')
            .unwrap_or(rest.len());
        if name_len == 0 {
            // A stray `=`, `/` or `>`: skip it.
            rest = &rest[1..];
            continue;
        }
        let name = rest[..name_len].to_ascii_lowercase();
        rest = &rest[name_len..];
        let after_ws = rest.trim_start();
        let Some(value_part) = after_ws.strip_prefix('=') else {
            out.push((name, None));
            continue;
        };
        let value_part = value_part.trim_start();
        let (value, after) = match value_part.chars().next() {
            Some(q @ ('"' | '\'')) => match value_part[1..].find(q) {
                Some(j) => (&value_part[1..1 + j], &value_part[1 + j + 1..]),
                None => (&value_part[1..], ""),
            },
            _ => {
                let end = value_part.find(char::is_whitespace).unwrap_or(value_part.len());
                (&value_part[..end], &value_part[end..])
            }
        };
        out.push((name, Some(entities(value))));
        rest = after;
    }
}

/// HTML's common character references decoded — the named five, `&nbsp;`,
/// and numeric ones; anything else is left as written.
pub(crate) fn entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        // V2-20: a reference is short: `;` is looked for that far, not to the end.
        let decoded = rest.as_bytes().iter().take(11).position(|b| *b == b';').and_then(|j| {
            let ch = match &rest[1..j] {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                n if n.starts_with("#x") || n.starts_with("#X") => {
                    u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32)
                }
                n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            ch.map(|c| (c, j + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// An element's text, as a summary or a description is: its tags dropped,
/// entities decoded, whitespace collapsed, trimmed. (INPUT-3: an input's
/// question, recommendation and choice labels are read with it too.)
pub fn text(inner: &str) -> String {
    let mut plain = String::with_capacity(inner.len());
    // V2-20: past the last `>`, a `<` opens no tag: nothing is looked for.
    let last = inner.rfind('>').unwrap_or(0);
    let mut i = 0;
    while let Some(off) = inner[i..].find('<') {
        let lt = i + off;
        plain.push_str(&inner[i..lt]);
        let is_tag = inner[lt + 1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        match (is_tag && lt < last).then(|| inner[lt..].find('>')).flatten() {
            Some(j) => i = lt + j + 1,
            None => {
                plain.push('<');
                i = lt + 1;
            }
        }
    }
    plain.push_str(&inner[i..]);
    entities(&plain).split_whitespace().collect::<Vec<_>>().join(" ")
}


/// The markdown dialect every body is read in — the app's four options.
fn options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts
}

/// The app's frontmatter split: `(Some(frontmatter), rest)` when the body opens
/// with a `---` line and a later line is `---` alone.
fn split_frontmatter(body: &str) -> (Option<&str>, &str) {
    let Some(after_open) = body.strip_prefix("---") else {
        return (None, body);
    };
    // The opening fence must be a line of its own ("---not-a-fence" isn't one).
    let Some(after_open) = after_open
        .strip_prefix("\r\n")
        .or_else(|| after_open.strip_prefix('\n'))
    else {
        return (None, body);
    };
    let mut offset = 0usize;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            let fm = &after_open[..offset];
            let rest = &after_open[offset + line.len()..];
            return (Some(fm), rest);
        }
        offset += line.len();
    }
    (None, body)
}


#[cfg(test)]
mod tests {
    use super::*;

    const TAGS: &[&str] = &["m-a", "m-b"];

    fn shape(e: &Element<'_>) -> String {
        format!("{}[{}]", e.tag, e.parts.iter().map(shape).collect::<Vec<_>>().join(","))
    }

    #[test]
    fn elements_nest_in_the_order_written() {
        let found = marked("x <m-a id=\"1\" flag><span>t</span><m-b v='2'>in</m-b><m-b/></m-a> y <M-B>z</M-B>", TAGS).unwrap();
        assert_eq!(found.iter().map(shape).collect::<Vec<_>>(), ["m-a[m-b[],m-b[]]", "m-b[]"]);
        assert_eq!((found[0].attr("id"), found[0].attr("flag"), found[0].attr("no")), (Some("1"), Some(""), None));
        assert_eq!((found[0].parts[0].attr("v"), found[0].parts[0].content), (Some("2"), "in"));
    }

    #[test]
    fn code_comments_raw_text_and_the_frontmatter_hold_no_markup() {
        for body in [
            "```html\n<m-a></m-a>\n```\n",
            "`<m-a>` said",
            "para\n\n    <m-a></m-a>\n",
            "<!-- <m-a></m-a> -->",
            "---\nn: <m-a>\n---\nbody",
            "<textarea><m-a></m-a></textarea>",
            "<m-mockup>\n<html><m-a></m-a></html>\n</m-mockup>\n",
        ] {
            assert!(marked(body, TAGS).unwrap().is_empty(), "{body}");
        }
    }

    #[test]
    fn nesting_and_work_are_bounded() {
        assert_eq!(marked(&"<m-a>".repeat(1_000), TAGS).unwrap_err(), Beyond::Deep);
        let b = Budget::new();
        assert!(b.spend(1) && !b.spent());
        assert!(!b.parse(usize::MAX / 8) && b.spent());
    }

    #[test]
    fn text_drops_tags_and_decodes() {
        assert_eq!(text(" Which <em>checks</em>\n ran? "), "Which checks ran?");
        assert_eq!(text("Unit &amp; store &#39;x&#x27; &nope; 1 < 2"), "Unit & store 'x' &nope; 1 < 2");
        assert_eq!(entities("&lt;/m-input&gt;"), "</m-input>");
    }
}
