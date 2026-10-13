//! ART-5: a ticket's **fields** — the parts of a body a checker finds by
//! name (ART-1; ought's ticketing skill, "A ticket is an HTML artifact",
//! ADR _common/0006). This is the piece COMPILE calls: [`fields`] reads a
//! body's fields; what a kind requires of them, and telling a writer so, is
//! COMPILE-1's.
//!
//! **A body is read as the app renders it** — its frontmatter split off as
//! `frontend/src/md.rs` splits it, the rest through the app's markdown
//! parser with the app's options (COMP-3's way, `store/components.rs`). So
//! what is HTML here is what the page passes through as HTML, and a code
//! span, a fence or indented code is text: it never holds a field.
//!
//! **The artifact form.** A field is an element marked
//! `data-field="<name>"`, in an HTML block or inline HTML. A body's fields
//! are its outermost marked elements, in order; a marked element inside a
//! field is one of its **parts** ([`Field::parts`]), at any depth —
//! `decision` inside `decision-gates`, `question` inside `decision`. An
//! element's extent is the one a browser builds from the page's markup,
//! closely enough for a checker: its close tag ends it, and so does what
//! closes it implicitly — the close of an element it sits in, a block that
//! opens after an unclosed `<p>`, the next `<li>`, the end of the markdown
//! block (a paragraph, a list item) it was opened in. A self-closing `/>`
//! or a void element is empty (the author's intent; a browser ignores `/>`
//! on a non-void element). Comments, and the text of `script`, `style`,
//! `textarea` and `title`, hold no fields. A marked element is a field
//! whatever its name: one outside ought's field set, or a bare
//! `data-field`, is a checker's to report.
//!
//! **The legacy form.** A body with no marked element is legacy markdown
//! (ought: "A body with any `data-field` element is an artifact … A body
//! with none is legacy"). Its fields are its `##` headings, each read as the
//! field ought's field set maps its "was" heading to ([`legacy_name`]),
//! running to the next heading of the same or a higher level; `### Divergence
//! notes` inside `## Intended Usage` is its one part; and the italic line a
//! body may open with is its `confidence` (`*confidence: …*`) or its
//! `standing`. A heading the field set does not name is prose between
//! fields, and only the body's own headings count — not one in a block quote
//! or a list item.
//!
//! Wasm-safe, like [`crate::component_docs`] (whose tag reading it shares):
//! the app may read fields too.

use crate::component_docs::{attributes, comment_end, open_end, raw_close, tag_name, VOID};
use crate::html_runs;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::ops::Range;

/// One field of a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The field's name: the element's `data-field`, as written (`""` for a
    /// bare attribute); for a legacy heading, the name the field set gives it.
    pub name: String,
    /// How the body writes it.
    pub form: Form,
    /// The 1-based line, within the whole body (frontmatter included), the
    /// field starts on: its open tag's `<`, its heading, its italic line.
    pub line: u32,
    /// What the field holds, as written, parts included: an element's
    /// content between its open and close tags; a heading's section, from
    /// the line after the heading to the next heading of its level or
    /// higher; the italic line itself.
    pub content: String,
    /// The fields inside it — marked elements, or a legacy part heading —
    /// outermost first, in order, each with its own parts.
    pub parts: Vec<Field>,
}

/// How a body writes a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Form {
    /// An element marked `data-field` — its tag, ASCII-lowercased
    /// (`section`, `m-provides`).
    Element { tag: String },
    /// A legacy markdown heading — its text as the page shows it.
    Heading { text: String },
    /// The italic line a legacy body opens with.
    Lead,
}

/// The body's fields, in order — see the module doc for the rule. A body
/// with no marked element is read in the legacy form; one with neither
/// marked elements nor headings the field set names has none.
pub fn fields(body: &str) -> Vec<Field> {
    let (_, rest) = split_frontmatter(body);
    // `rest` is a suffix of `body`: an offset into it, plus this, is an
    // offset into the body.
    let at = body.len() - rest.len();
    let lines = LineStarts::of(body);
    let scan = Scan::read(rest, at, Mark::Field);
    if scan.top.is_empty() {
        return legacy(body, rest, at, &lines);
    }
    scan.top.iter().map(|&id| scan.field(id, body, &lines)).collect()
}

/// INPUT-3: an element of a body named in a [`marked`] reading — what the
/// inputs store reads a body's input markup from (`crate::inputs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Element {
    /// Its tag, ASCII-lowercased.
    pub(crate) tag: String,
    /// Its open tag's attributes, as written — names lowercased, values
    /// with their entities decoded; `None` for a bare name.
    pub(crate) attrs: Vec<(String, Option<String>)>,
    /// What it holds, as written — its content between its open and close
    /// tags, as a [`Field`]'s is.
    pub(crate) content: String,
    /// The named elements inside it, outermost first, in order.
    pub(crate) parts: Vec<Element>,
}

impl Element {
    /// The attribute `name`'s value — `Some("")` for a bare name.
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_deref().unwrap_or(""))
    }
}

/// INPUT-3: every element whose tag is one of `tags` (lowercase), read as
/// [`fields`] reads a body — the frontmatter split off, the rest through the
/// app's markdown parser, raw HTML only (never a code span, a fence or
/// indented code), comments and raw text skipped, each element's extent the
/// one a browser builds — outermost first, each with the named elements
/// inside it as its parts. The artifact form's scan with another mark: a
/// tag named, not a `data-field`.
pub(crate) fn marked(body: &str, tags: &'static [&'static str]) -> Vec<Element> {
    let (_, rest) = split_frontmatter(body);
    let at = body.len() - rest.len();
    let scan = Scan::read(rest, at, Mark::Tags(tags));
    scan.top.iter().map(|&id| scan.element(id, body)).collect()
}

/// What a scan marks: ART-5's fields — elements with a `data-field` — or
/// INPUT-3's elements of the named tags.
#[derive(Clone, Copy, Default)]
enum Mark {
    #[default]
    Field,
    Tags(&'static [&'static str]),
}

/// The legacy headings ought's field set names — its "was" column, one row
/// per heading, every kind's (build, scope, execution, spike), each with the
/// field it reads as. The table is kind-blind, as [`fields`] is: the one
/// heading two kinds would read differently is a scope's "problem" (its
/// `core-transition`, "The core transition / problem"), and a `## Problem`
/// reads as `problem` here — a scope written to ought's skeleton heads it
/// `## The core transition`. A caller that knows the kind may read a
/// scope's `problem` as its `core-transition`.
pub const LEGACY: &[(&str, &str)] = &[
    // build
    ("Problem", "problem"),
    ("Context", "context"),
    ("Evidence", "evidence"),
    ("Required behavior", "required-behavior"),
    ("Provides", "provides"),
    ("Consumes", "consumes"),
    ("Intended Usage", "intended-usage"),
    ("Risks", "risks"),
    ("What must NOT change", "must-not-change"),
    ("Acceptance criteria", "acceptance"),
    ("Completion", "completion"),
    ("Status", "status"),
    // scope
    ("End state", "end-state"),
    ("Language", "language"),
    ("Decided shape", "decided-shape"),
    ("The core transition", "core-transition"),
    ("Decisions & open questions", "open-questions"),
    ("Interface contract", "interface-contract"),
    ("Dependencies", "dependencies"),
    ("Execution", "execution"),
    // execution (and `Status`, above)
    ("Goal", "goal"),
    ("Execution DAG", "dag"),
    ("The work", "work"),
    ("Decision gates", "decision-gates"),
    ("Exit", "exit"),
    ("Completion gate", "completion-gate"),
    // spike
    ("Question", "question"),
    ("Lens", "lens"),
    ("Setup", "setup"),
    ("Pass condition", "pass-condition"),
    ("Evidence to record (regardless of outcome)", "evidence"),
    ("Fail → consequence", "fail-consequence"),
];

/// The legacy part headings: `(parent field, "was" heading, part)`. A `###`
/// heading inside its parent's section.
pub const LEGACY_PARTS: &[(&str, &str, &str)] = &[("intended-usage", "Divergence notes", "divergence-notes")];

/// The field a legacy `##` heading reads as, if the field set names it. A
/// heading matches a "was" entry ignoring case and runs of whitespace,
/// exactly or followed by a qualifier — after a dash, a colon, a
/// parenthesis, `·`, `|`, `/` or a comma: "The core transition — what
/// exists today", "Interface contract (what this milestone hands the
/// next)". The longest entry that matches wins.
pub fn legacy_name(heading: &str) -> Option<&'static str> {
    best(heading, LEGACY.iter().map(|(was, name)| (*was, *name)))
}

/// The part a legacy `###` heading inside `parent`'s section reads as.
fn legacy_part(parent: &str, heading: &str) -> Option<&'static str> {
    best(heading, LEGACY_PARTS.iter().filter(|(p, _, _)| *p == parent).map(|(_, was, name)| (*was, *name)))
}

fn best(heading: &str, table: impl Iterator<Item = (&'static str, &'static str)>) -> Option<&'static str> {
    let h = normalized(heading);
    table
        .filter(|(was, _)| reads_as(&h, &normalized(was)))
        .max_by_key(|(was, _)| was.len())
        .map(|(_, name)| name)
}

fn normalized(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Whether the (normalized) heading `h` is `was`, alone or qualified.
fn reads_as(h: &str, was: &str) -> bool {
    let Some(rest) = h.strip_prefix(was) else { return false };
    rest.is_empty() || rest.trim_start().starts_with(['—', '–', '-', '(', ':', '·', '|', '/', ','])
}

// -- the artifact form ---------------------------------------------------------

/// Where `body`'s scan stands: the marked elements found, and every element
/// open — raw HTML's, and the ones the markdown blocks around it render as.
#[derive(Default)]
struct Scan {
    /// What it marks ([`Mark`]).
    mark: Mark,
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
    name: String,
    tag: String,
    /// Its open tag's attributes — kept for a [`Mark::Tags`] scan.
    attrs: Vec<(String, Option<String>)>,
    /// Its open tag's `<`.
    start: usize,
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
    fn read(rest: &str, at: usize, mark: Mark) -> Scan {
        let mut scan = Scan {
            mark,
            ..Scan::default()
        };
        // An HTML block's lines, gathered so a tag or a comment across them
        // is one — with where each line starts in the gathered text and in
        // the body (a line in a list item or a block quote has its prefix
        // dropped, so the two drift).
        let mut block: Option<(String, Vec<(usize, usize)>)> = None;
        // COMP-11: an element opened in an HTML block runs to its close,
        // blank lines included, as the frame renders it — the parser reads
        // the body with those blank lines marked; offsets and lines map
        // back (a mark ends its line, so a line's offsets still run on).
        let read = html_runs::read(rest, options());
        for (event, range) in Parser::new_ext(read.source(), options()).into_offset_iter() {
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
        scan
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
                i = lt + 1;
                continue;
            };
            let attrs = attributes(html[name_end..gt - 1].trim_end_matches('/'));
            let (field, attrs) = match self.mark {
                Mark::Field => {
                    let field = attrs.into_iter().find(|(k, _)| k == "data-field").map(|(_, v)| v.unwrap_or_default());
                    (field, Vec::new())
                }
                Mark::Tags(tags) => (tags.contains(&name.as_str()).then(|| name.clone()), attrs),
            };
            self.start(name, field, attrs, body_at(lt), body_at(gt), self_closing);
            i = gt;
        }
    }

    /// An open tag `<name …>` at `start`, its content from `content`.
    fn start(
        &mut self,
        name: String,
        field: Option<String>,
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
        let node = field.map(|f| {
            let id = self.nodes.len();
            match self.stack.iter().rev().find_map(|o| o.node) {
                Some(parent) => self.nodes[parent].parts.push(id),
                None => self.top.push(id),
            }
            self.nodes.push(Node {
                name: f,
                tag: name.clone(),
                attrs,
                start,
                content,
                end: None,
                parts: Vec::new(),
            });
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
        if let Some(k) = self.stack.iter().rposition(|o| o.md == Some(id)) {
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

    /// A [`Mark::Tags`] scan's element `id`, with its parts.
    fn element(&self, id: usize, body: &str) -> Element {
        let n = &self.nodes[id];
        let end = n.end.unwrap_or(body.len()).max(n.content);
        Element {
            tag: n.tag.clone(),
            attrs: n.attrs.clone(),
            content: body.get(n.content..end).unwrap_or_default().to_string(),
            parts: n.parts.iter().map(|&p| self.element(p, body)).collect(),
        }
    }

    fn field(&self, id: usize, body: &str, lines: &LineStarts) -> Field {
        let n = &self.nodes[id];
        let end = n.end.unwrap_or(body.len()).max(n.content);
        Field {
            name: n.name.clone(),
            form: Form::Element { tag: n.tag.clone() },
            line: lines.line_of(n.start),
            content: body.get(n.content..end).unwrap_or_default().to_string(),
            parts: n.parts.iter().map(|&p| self.field(p, body, lines)).collect(),
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

// -- the legacy form -------------------------------------------------------------

/// A heading of the body's own, as the legacy reading keeps it.
struct Head {
    level: u8,
    text: String,
    /// Where the heading starts, and where its line ends (the body's).
    start: usize,
    end: usize,
}

/// The opening paragraph, while it may still be the italic line.
struct Opening {
    range: Range<usize>,
    text: String,
    emphasis: bool,
    only: bool,
}

fn level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn legacy(body: &str, rest: &str, at: usize, lines: &LineStarts) -> Vec<Field> {
    let mut heads: Vec<Head> = Vec::new();
    let mut heading: Option<(u8, Range<usize>, String)> = None;
    let mut opening: Option<Opening> = None;
    let mut lead: Option<Field> = None;
    let mut first = true;
    let mut depth = 0usize;
    for (event, range) in Parser::new_ext(rest, options()).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    if first && matches!(tag, Tag::Paragraph) {
                        opening = Some(Opening {
                            range: range.clone(),
                            text: String::new(),
                            emphasis: false,
                            only: true,
                        });
                    }
                    if let Tag::Heading { level: l, .. } = tag {
                        heading = Some((level(l), range, String::new()));
                    }
                } else if let Some(o) = opening.as_mut().filter(|_| depth == 1) {
                    // The paragraph holds one emphasis and nothing else.
                    if matches!(tag, Tag::Emphasis) && !o.emphasis {
                        o.emphasis = true;
                    } else {
                        o.only = false;
                    }
                }
                first = false;
                depth += 1;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some((level, range, text)) = heading.take() {
                        heads.push(Head {
                            level,
                            text: text.split_whitespace().collect::<Vec<_>>().join(" "),
                            start: at + range.start,
                            end: at + range.end,
                        });
                    }
                    if let Some(o) = opening.take().filter(|o| o.emphasis && o.only) {
                        let confidence = o.text.trim_start().to_lowercase().starts_with("confidence:");
                        lead = Some(Field {
                            name: if confidence { "confidence" } else { "standing" }.to_string(),
                            form: Form::Lead,
                            line: lines.line_of(at + o.range.start),
                            content: rest[o.range].trim_end().to_string(),
                            parts: Vec::new(),
                        });
                    }
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, _, text)) = &mut heading {
                    text.push_str(&t);
                }
                if let Some(o) = &mut opening {
                    if depth > 1 {
                        o.text.push_str(&t);
                    } else if !t.trim().is_empty() {
                        o.only = false;
                    }
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, _, text)) = &mut heading {
                    text.push(' ');
                }
                if let Some(o) = &mut opening {
                    o.text.push(' ');
                }
            }
            _ => {
                first = first && depth > 0;
                if let Some(o) = opening.as_mut().filter(|_| depth == 1) {
                    o.only = false;
                }
            }
        }
    }

    let mut out: Vec<Field> = lead.into_iter().collect();
    for (k, h) in heads.iter().enumerate() {
        if h.level != 2 {
            continue;
        }
        let Some(name) = legacy_name(&h.text) else { continue };
        let end = heads[k + 1..].iter().find(|n| n.level <= 2).map_or(body.len(), |n| n.start);
        let inside: Vec<&Head> = heads[k + 1..].iter().take_while(|n| n.start < end).collect();
        let parts = inside
            .iter()
            .enumerate()
            .filter(|(_, n)| n.level == 3)
            .filter_map(|(j, n)| {
                let part = legacy_part(name, &n.text)?;
                let part_end = inside[j + 1..].iter().find(|m| m.level <= 3).map_or(end, |m| m.start);
                Some(Field {
                    name: part.to_string(),
                    form: Form::Heading { text: n.text.clone() },
                    line: lines.line_of(n.start),
                    content: body.get(n.end..part_end).unwrap_or_default().to_string(),
                    parts: Vec::new(),
                })
            })
            .collect();
        out.push(Field {
            name: name.to_string(),
            form: Form::Heading { text: h.text.clone() },
            line: lines.line_of(h.start),
            content: body.get(h.end..end).unwrap_or_default().to_string(),
            parts,
        });
    }
    out
}

// -- the app's reading, copied ---------------------------------------------------

/// The markdown dialect every body is read in — `frontend/src/md.rs`'s
/// options, the same four (the tests hold them equal).
fn options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts
}

/// `frontend/src/md.rs`'s frontmatter split, word for word (the tests hold
/// it to the original): `(Some(frontmatter), rest)` when the body opens
/// with a `---` line and a later line is `---` alone.
pub(crate) fn split_frontmatter(body: &str) -> (Option<&str>, &str) {
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

/// Where each line of a text begins, so an offset becomes a 1-based line
/// (`diagrams/src/blocks.rs`'s, for the same reason).
struct LineStarts(Vec<usize>);

impl LineStarts {
    fn of(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        LineStarts(starts)
    }

    fn line_of(&self, offset: usize) -> u32 {
        self.0.partition_point(|&start| start <= offset) as u32
    }
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod tests;
