//! MFE-7: ticket bodies as safe markdown.
//!
//! Sanitization happens at the event level, not by post-filtering HTML:
//! pulldown-cmark's own renderer escapes all `Text` events, so the only
//! ways script could reach the DOM are (a) raw HTML events and (b) link /
//! image destinations with an executable scheme. Both are neutralized in
//! the event stream — raw HTML (block or inline) is downgraded to `Text`
//! (it renders as visible, inert source text), and a destination outside
//! the allow-list drops its anchor/image tag while keeping the inner
//! text. Everything else push_html emits is well-formed markup it built
//! itself. No HTML parser or sanitizer crate rides into the wasm.
//!
//! Frontmatter (AGS heads-up): a body whose first line is `---` with a
//! later closing `---` line is split; the YAML block is returned
//! separately (the page renders it collapsed) and only the remainder is
//! painted as markdown.
//!
//! Mermaid (AGS-14): a fenced ```mermaid block does not become a code
//! block. Its source is collected from the event stream and emitted as
//! one placeholder — a `div.mermaid[data-state=pending]` wrapping the
//! source as an escaped `<pre><code>` — the only HTML this module writes
//! by hand, and it is built from a fixed wrapper plus escaped text, so
//! the safe-for-`inner_html` contract holds. The page (src/mermaid.rs)
//! finds the placeholders after mount and replaces each with SVG; until
//! then, and whenever rendering fails, the reader sees the source.
//!
//! ART-5: in a framed body, a diagram may also be written as HTML — ought's
//! form, mermaid source in a `<pre class="mermaid">` (its design rule,
//! "Diagrams") — in an HTML block, alone or inside a field element or a
//! component. [`render_body_framed`] puts the same placeholder in its place
//! ([`html_diagrams`]), so the frame draws it as it draws a fence's, and
//! counts it. (`diagrams/src/blocks.rs` copies that rule, so `check` finds
//! exactly these.)
//!
//! WAVE-11: a **wave reference** — `{wave: RETURNS-6}` in prose — renders as
//! that ticket's current ordinal ([`render_body_with`]): `wave 2` (a link),
//! `done`, or `missing blocker`, as the caller's resolver places it; one it
//! cannot place (an id that is no ticket, a doc, a row not fetched yet) is
//! left as written. Only in prose: a reference inside code, a link or an
//! image's text is text. The markup is a fixed wrapper around escaped text,
//! the mermaid placeholder's discipline; and a body without `{wave:` takes
//! [`render_body`]'s path, so it renders byte for byte as before.
//!
//! COMP-9: a **definition** — a ` ```component m-x ` fence — frames its body
//! ([`is_framed`]), and in the frame ([`render_body_framed`]) it is not a
//! code block: it renders as the component's docs card (src/card.rs), built
//! from the block itself — its docs parsed with the server's own parser
//! (`menger_tickets_api::component_docs`), its help through this module.
//!
//! COMP-11: in a framed body, **an element opened in an HTML block runs to
//! its close, blank lines included** — CommonMark would end the block at the
//! first blank line and read the rest as markdown (indented lines as code,
//! inline tags wrapped in `<p>`). The rule is tickets-api's
//! (`menger_tickets_api::html_runs`, which the server's readers of a body and
//! `diagrams` use too): a blank line inside an element keeps the block going,
//! except where markdown is meant — right after an element's opening (its
//! open tag, or that and a `<summary>` or heading line: CommonMark's and
//! GitHub's idiom for markdown inside HTML, the house components' too), and
//! before a fence. A block that starts as `<pre>`, `<script>`, `<style>`,
//! `<textarea>` or a comment already ran to its end, and is untouched. The
//! rule is the frame's alone: an unframed body holds no element (that is
//! what frames one), and is painted into the page as escaped text, where an
//! HTML block's extent changes nothing anyone means — so [`render_body`] and
//! [`render_body_with`] parse the body as written, byte for byte as before.
//!
//! COMP-12: in a frame, the elements markdown produced carry [`MD_MARK`]
//! ([`push_marked`]) — in a body that opts out of the SDK's base typography
//! on its own HTML (`<meta name="menger-base" content="markdown">`), the
//! base styles those and nothing the body wrote as HTML.

use menger_tickets_api::{component_docs, html_runs};
use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};
use pulldown_cmark_escape::{escape_href, escape_html};

use crate::card;

/// The class the mounter queries for — the one name both sides share.
pub const MERMAID_CLASS: &str = "mermaid";

/// A rendered body: HTML that is safe for `inner_html`, plus how many
/// mermaid placeholders it holds. Zero means the page never touches the
/// renderer (nor fetches its bundle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub html: String,
    pub diagrams: usize,
}

/// Is this fence's info string a mermaid fence? The language is the first
/// word (`mermaid title=...` still counts); exact and lowercase, as
/// GitHub's renderer treats it.
fn is_mermaid_fence(info: &str) -> bool {
    info.split_whitespace().next() == Some("mermaid")
}

/// The placeholder a ```mermaid fence becomes. Wrapper attributes are
/// fixed; the source is escaped — so nothing body-sourced reaches the DOM
/// as markup. The mounter reads the source back via `textContent`.
fn mermaid_placeholder(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 96);
    out.push_str("<div class=\"");
    out.push_str(MERMAID_CLASS);
    out.push_str("\" data-state=\"pending\"><pre class=\"mermaid-src\"><code>");
    escape_html(&mut out, src).expect("escaping into a String cannot fail");
    out.push_str("</code></pre></div>\n");
    out
}

/// Split a leading YAML frontmatter block off a ticket body. Returns
/// `(Some(frontmatter), rest)` when the body starts with a `---` line and
/// a closing `---` line exists; otherwise `(None, body)` — an unclosed
/// fence (or a mid-document thematic break) is not frontmatter.
pub fn split_frontmatter(body: &str) -> (Option<&str>, &str) {
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

/// Is this link/image destination inert? Relative destinations and the
/// http/https (+ mailto, for links) schemes are; everything else —
/// javascript:, data:, vbscript:, the unknown — is not. Default deny.
fn dest_allowed(dest: &str, allow_mailto: bool) -> bool {
    let dest = dest.trim();
    for (i, c) in dest.char_indices() {
        match c {
            // Path/query/fragment before any ':' means no scheme: relative.
            '/' | '?' | '#' => return true,
            ':' => {
                let scheme = dest[..i].to_ascii_lowercase();
                return scheme == "http"
                    || scheme == "https"
                    || (allow_mailto && scheme == "mailto");
            }
            _ => {}
        }
    }
    true // no scheme at all: relative
}

/// Render a ticket body to HTML that is safe to inject via `inner_html`,
/// plus the count of mermaid placeholders it holds.
pub fn render_body(src: &str) -> Rendered {
    render(src, None, false, false)
}

/// WAVE-11: what a `{wave: ID}` renders as — the ticket's placement, as the
/// caller holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveRef {
    /// `wave n` — a link: to the ticket, and, on a board, to its column.
    Wave(u32),
    Done,
    Missing,
}

/// The opening of a wave reference.
const WAVE_OPEN: &str = "{wave:";

/// One `{wave: ID}` at the start of `s`: the id (as written) and the length
/// of the whole reference. An id is a shortcode, `-`, a sequence; spaces
/// may stand either side of it.
fn wave_ref_at(s: &str) -> Option<(&str, usize)> {
    let rest = s.strip_prefix(WAVE_OPEN)?;
    let lead = rest.len() - rest.trim_start_matches(' ').len();
    let body = &rest[lead..];
    let end = body.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(body.len());
    let id = &body[..end];
    let (epic, seq) = id.rsplit_once('-')?;
    if epic.is_empty() || !epic.chars().all(|c| c.is_ascii_alphanumeric()) || seq.is_empty() || !seq.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let after = &body[end..];
    let trail = after.len() - after.trim_start_matches(' ').len();
    after[trail..].starts_with('}').then_some((id, WAVE_OPEN.len() + lead + end + trail + 1))
}

/// WAVE-11: every id a `{wave: ID}` in `src` names, upper-cased, in order,
/// each once — what a page fetches the rows of.
pub fn wave_ids(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut at = 0;
    while let Some(i) = src[at..].find(WAVE_OPEN) {
        let start = at + i;
        match wave_ref_at(&src[start..]) {
            Some((id, len)) => {
                let id = id.to_ascii_uppercase();
                if !out.contains(&id) {
                    out.push(id);
                }
                at = start + len;
            }
            None => at = start + WAVE_OPEN.len(),
        }
    }
    out
}

/// The markup one placed reference becomes: a fixed wrapper, the id escaped
/// into its attributes (it is `[A-Za-z0-9-]` already).
fn wave_markup(id: &str, r: WaveRef) -> String {
    let mut esc = String::new();
    escape_html(&mut esc, id).expect("escaping into a String cannot fail");
    let mut href = String::new();
    escape_href(&mut href, id).expect("escaping into a String cannot fail");
    match r {
        WaveRef::Wave(n) => format!(
            "<a class=\"wave-ref\" data-wave-ref=\"{esc}\" data-wave=\"{n}\" href=\"#/ticket/{href}\" title=\"{esc}: wave {n}\">wave {n}</a>"
        ),
        WaveRef::Done => format!("<span class=\"wave-ref done\" data-wave-ref=\"{esc}\" title=\"{esc}: done\">done</span>"),
        WaveRef::Missing => format!(
            "<span class=\"wave-ref missing\" data-wave-ref=\"{esc}\" title=\"{esc}: waits on a ticket that does not exist\">missing blocker</span>"
        ),
    }
}

/// One stretch of prose with every reference `resolve` places swapped for
/// its markup; the rest, references it does not place included, as text.
fn substitute<'a>(text: String, resolve: &dyn Fn(&str) -> Option<WaveRef>, out: &mut Vec<Event<'a>>) {
    let mut plain = String::new();
    let mut at = 0;
    while let Some(i) = text[at..].find(WAVE_OPEN) {
        let start = at + i;
        match wave_ref_at(&text[start..]) {
            Some((id, len)) => match resolve(&id.to_ascii_uppercase()) {
                Some(r) => {
                    plain.push_str(&text[at..start]);
                    if !plain.is_empty() {
                        out.push(Event::Text(CowStr::from(std::mem::take(&mut plain))));
                    }
                    out.push(Event::Html(CowStr::from(wave_markup(&id.to_ascii_uppercase(), r))));
                    at = start + len;
                }
                None => {
                    plain.push_str(&text[at..start + len]);
                    at = start + len;
                }
            },
            None => {
                plain.push_str(&text[at..start + WAVE_OPEN.len()]);
                at = start + WAVE_OPEN.len();
            }
        }
    }
    plain.push_str(&text[at..]);
    if !plain.is_empty() {
        out.push(Event::Text(CowStr::from(plain)));
    }
}

/// WAVE-11: [`render_body`], every `{wave: ID}` in prose that `resolve`
/// places (it is handed the id upper-cased) rendered as its ordinal, and
/// every one it does not left as written. A body with no `{wave:` in it is
/// `render_body`'s, byte for byte.
pub fn render_body_with(src: &str, resolve: &dyn Fn(&str) -> Option<WaveRef>) -> Rendered {
    if !src.contains(WAVE_OPEN) {
        return render_body(src);
    }
    render(src, Some(resolve), false, false)
}

/// COMP-4: a framed body's markup — [`render_body_with`]'s, raw HTML passed
/// through (block and inline) instead of downgraded to text. Mermaid fences
/// still become placeholders (the frame's SDK draws them) — and so, ART-5,
/// does a `<pre class="mermaid">` in an HTML block ([`html_diagrams`]); `{wave: ID}`s in
/// prose are placed by `resolve` here, on the host, and markdown links and
/// images keep their scheme allow-list. Only for the sandboxed frame: this
/// HTML is never safe for `inner_html`.
pub fn render_body_framed(src: &str, resolve: &dyn Fn(&str) -> Option<WaveRef>) -> Rendered {
    if !src.contains(WAVE_OPEN) {
        return render(src, None, true, true);
    }
    render(src, Some(resolve), true, true)
}

/// COMP-9: a docs card's help — markdown rendered as a framed body is (its
/// HTML passes through; mermaid becomes a placeholder the frame draws), but
/// a definition inside it is a code block: a card holds no card.
pub fn render_help(src: &str) -> String {
    render(src, None, true, false).html
}

/// COMP-9: the card a definition block renders as in a framed body — the
/// block's tag and preview from its info string, its docs parsed from its
/// content (src/card.rs).
fn definition_card(info: &str, source: &str) -> String {
    let (name, preview) = component_docs::definition(info).unwrap_or(("", Vec::new()));
    let parsed = component_docs::parse(source);
    let docs = match &parsed {
        Ok(d) => Ok(d.as_ref()),
        Err(problem) => Err(problem.as_str()),
    };
    card::card_html(&card::Card { name, preview: &preview, docs, source, in_body: true }, &render_help)
}

/// ART-5: the diagrams an HTML block writes as HTML — ought's form, mermaid
/// source in a `<pre class="mermaid">`: each `<pre>` whose class list holds
/// `mermaid`, to the first `</pre>` after it in the same block (one the
/// block does not close is no diagram). For each, where the element starts
/// and ends in `html`, where its source starts, and the source: the
/// element's content, the newline right after its open tag dropped (as
/// HTML's parser drops it), character references decoded (`&lt;` `&gt;`
/// `&amp;` `&quot;` `&apos;` `&nbsp;`, numeric ones) and the rest as
/// written — a `<br/>` in a label stays a `<br/>`, as mermaid's own `run`
/// reads a `.mermaid` element. Never in a comment, nor in the text of a
/// `script`, `style`, `textarea` or `title`.
fn html_diagrams(html: &str) -> Vec<(usize, usize, usize, String)> {
    /// Whether the tag name at `at` in `low` is `name`, whole.
    fn named(low: &str, at: usize, name: &str) -> bool {
        low[at..].starts_with(name)
            && low[at + name.len()..].chars().next().is_none_or(|c| c.is_ascii_whitespace() || c == '/' || c == '>')
    }
    /// The first `</name` at or after `from`: where it starts, and just past
    /// its `>`.
    fn close(low: &str, name: &str, from: usize) -> Option<(usize, usize)> {
        let mut i = from;
        while let Some(off) = low[i..].find("</") {
            let lt = i + off;
            if named(low, lt + 2, name) {
                return Some((lt, low[lt..].find('>').map_or(low.len(), |j| lt + j + 1)));
            }
            i = lt + 2;
        }
        None
    }
    /// The first `class` among a tag's attributes (what follows its name).
    fn class(attrs: &str) -> Option<&str> {
        let mut rest = attrs;
        loop {
            rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
            if rest.is_empty() {
                return None;
            }
            let end = rest.find(|c: char| c.is_ascii_whitespace() || c == '=' || c == '/').unwrap_or(rest.len()).max(1);
            let name = &rest[..end];
            rest = rest[end..].trim_start();
            let mut value = "";
            if let Some(v) = rest.strip_prefix('=') {
                let v = v.trim_start();
                (value, rest) = match v.chars().next() {
                    Some(q @ ('"' | '\'')) => match v[1..].find(q) {
                        Some(j) => (&v[1..1 + j], &v[2 + j..]),
                        None => (&v[1..], ""),
                    },
                    _ => v.split_at(v.find(|c: char| c.is_ascii_whitespace()).unwrap_or(v.len())),
                };
            }
            if name.eq_ignore_ascii_case("class") {
                return Some(value);
            }
        }
    }
    /// Character references decoded: the named few, and numeric ones.
    fn decode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(amp) = rest.find('&') {
            out.push_str(&rest[..amp]);
            rest = &rest[amp..];
            let decoded = rest.find(';').filter(|&j| j <= 10).and_then(|j| {
                let c = match &rest[1..j] {
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "amp" => Some('&'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    n => n
                        .strip_prefix('#')
                        .and_then(|n| match n.strip_prefix(['x', 'X']) {
                            Some(hex) => u32::from_str_radix(hex, 16).ok(),
                            None => n.parse::<u32>().ok(),
                        })
                        .and_then(char::from_u32),
                };
                c.map(|c| (c, j + 1))
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
    // ASCII lowercasing keeps every offset.
    let low = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = low[i..].find('<') {
        let lt = i + off;
        if low[lt..].starts_with("<!--") {
            i = low[lt + 4..].find("-->").map_or(low.len(), |j| lt + 4 + j + 3);
            continue;
        }
        if !low[lt + 1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            i = lt + 1;
            continue;
        }
        let name_end = low[lt + 1..]
            .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
            .map_or(low.len(), |j| lt + 1 + j);
        // The open tag's `>`, outside quoted values.
        let mut quote = None;
        let Some(gt) = low[name_end..]
            .char_indices()
            .find(|&(_, c)| match quote {
                Some(q) => {
                    if c == q {
                        quote = None;
                    }
                    false
                }
                None if c == '"' || c == '\'' => {
                    quote = Some(c);
                    false
                }
                None => c == '>',
            })
            .map(|(j, _)| name_end + j + 1)
        else {
            break;
        };
        i = match &low[lt + 1..name_end] {
            name @ ("script" | "style" | "textarea" | "title") => close(&low, name, gt).map_or(low.len(), |(_, after)| after),
            "pre" if class(&html[name_end..gt - 1]).is_some_and(|c| c.split_ascii_whitespace().any(|w| w == "mermaid")) => {
                match close(&low, "pre", gt) {
                    Some((end, after)) => {
                        let nl = if low[gt..].starts_with("\r\n") { 2 } else { usize::from(low[gt..].starts_with('\n')) };
                        let from = gt + nl;
                        out.push((lt, after, from, decode(&html[from..end.max(from)])));
                        after
                    }
                    None => gt,
                }
            }
            _ => gt,
        };
    }
    out
}

/// ART-5: an HTML block with each diagram it writes as HTML
/// ([`html_diagrams`]) in place as the placeholder a mermaid fence becomes,
/// each counted — so the frame draws them as it draws a fence's.
fn with_diagrams(block: &str, diagrams: &mut usize) -> String {
    let found = html_diagrams(block);
    if found.is_empty() {
        return block.to_string();
    }
    let mut out = String::with_capacity(block.len() + 96 * found.len());
    let mut at = 0;
    for (start, end, _, source) in found {
        out.push_str(&block[at..start]);
        out.push_str(&mermaid_placeholder(&source));
        *diagrams += 1;
        at = end;
    }
    out.push_str(&block[at..]);
    out
}

/// COMP-4: does this body (its frontmatter already split off) render in a
/// frame? Yes when the markdown holds HTML outside code — a block or inline
/// tag naming an HTML element or a custom element, or a comment (see the
/// module doc for why a placeholder like `<org>` is not one). Code spans,
/// fences (mermaid's included) and indented code never frame: what is in
/// them is text, and the parser never reports it as HTML.
pub fn is_framed(src: &str) -> bool {
    if !src.contains('<') && !src.contains("component") {
        return false;
    }
    Parser::new_ext(src, options()).any(|ev| match ev {
        Event::Html(s) | Event::InlineHtml(s) => holds_markup(&s),
        // COMP-9: a definition renders as its card, in the frame.
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => component_docs::definition(&info).is_some(),
        _ => false,
    })
}

/// Does this stretch of raw HTML (one inline tag, or an HTML block's lines)
/// hold real markup: a comment, or an open or close tag whose name is an
/// HTML element or a custom element?
fn holds_markup(raw: &str) -> bool {
    let mut rest = raw;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        if rest.starts_with("!--") {
            return true;
        }
        let tag = rest.strip_prefix('/').unwrap_or(rest);
        let end = tag.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(tag.len());
        let name = &tag[..end];
        let closes = tag[end..].chars().next().map_or(true, |c| c.is_ascii_whitespace() || c == '>' || c == '/');
        if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic()) || !closes {
            continue;
        }
        let name = name.to_ascii_lowercase();
        if html_runs::is_element(&name) {
            return true;
        }
    }
    false
}

/// The markdown dialect every body is read in.
fn options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts
}

/// `framed`: raw HTML passes through (COMP-4's frame) rather than becoming
/// text (MFE-7) — the one difference between the two modes. `cards`
/// (framed only, COMP-9): a definition block renders as its docs card.
fn render(src: &str, resolve: Option<&dyn Fn(&str) -> Option<WaveRef>>, framed: bool, cards: bool) -> Rendered {
    let opts = options();
    // COMP-12: a NUL is [`push_marked`]'s sentinel — CommonMark's U+FFFD
    // stands for a source's own.
    let cleaned;
    let src = if framed && src.contains('\0') {
        cleaned = src.replace('\0', "\u{FFFD}");
        cleaned.as_str()
    } else {
        src
    };

    // Depth counters for dropped tags, so each End pairs with its Start.
    let mut dropped_links = 0u32;
    let mut dropped_images = 0u32;
    // Inside a ```mermaid fence: the source collected so far. The fence's
    // own Start/Text/End events are swallowed; the End emits the placeholder.
    let mut mermaid_src: Option<String> = None;
    let mut diagrams = 0usize;
    // COMP-9: inside a definition's fence (framed, with cards): its info
    // string and the source collected so far; the End emits the card.
    let mut definition: Option<(String, String)> = None;
    let cards = framed && cards;
    // ART-5 (framed only): an HTML block's lines, gathered — a diagram
    // written as HTML spans them — and emitted whole at its end, each
    // diagram in it a placeholder (`with_diagrams`).
    let mut html_block: Option<String> = None;
    // COMP-11 (framed only): an element opened in an HTML block runs to its
    // close, blank lines included — the parser reads the body with those
    // blank lines marked ([`html_runs`]), and each block's lines lose the
    // marks as they are gathered, so the markup passes through as written.
    // An unframed body holds no element: it parses as written.
    let read = framed.then(|| html_runs::read(src, opts));
    let src = read.as_ref().map_or(src, |read| read.source());
    let events = Parser::new_ext(src, opts).filter_map(|ev| {
        if definition.is_some() {
            return match ev {
                Event::Text(t) => {
                    definition.as_mut().expect("checked").1.push_str(&t);
                    None
                }
                Event::End(TagEnd::CodeBlock) => {
                    let (info, source) = definition.take().expect("checked");
                    Some(Event::Html(definition_card(&info, &source).into()))
                }
                _ => None,
            };
        }
        if mermaid_src.is_some() {
            return match ev {
                Event::Text(t) => {
                    mermaid_src.as_mut().expect("checked").push_str(&t);
                    None
                }
                Event::End(TagEnd::CodeBlock) => {
                    let collected = mermaid_src.take().expect("checked");
                    diagrams += 1;
                    Some(Event::Html(mermaid_placeholder(&collected).into()))
                }
                // A fenced block's body is Text only; anything else here
                // would be a parser surprise — drop it rather than paint it.
                _ => None,
            };
        }
        if framed {
            match &ev {
                Event::Start(Tag::HtmlBlock) => {
                    html_block = Some(String::new());
                    return None;
                }
                Event::Html(s) if html_block.is_some() => {
                    html_block.as_mut().expect("checked").push_str(&read.as_ref().expect("framed").restore(s));
                    return None;
                }
                Event::End(TagEnd::HtmlBlock) => {
                    let block = html_block.take().unwrap_or_default();
                    return Some(Event::Html(with_diagrams(&block, &mut diagrams).into()));
                }
                _ => {}
            }
        }
        match ev {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref info)))
                if is_mermaid_fence(info) =>
            {
                mermaid_src = Some(String::new());
                None
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref info)))
                if cards && component_docs::definition(info).is_some() =>
            {
                definition = Some((info.to_string(), String::new()));
                None
            }
            // Raw HTML renders as inert, visible text (push_html escapes Text)
            // — unless the body is framed (COMP-4), where it is the point.
            Event::Html(s) | Event::InlineHtml(s) if !framed => Some(Event::Text(s)),
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                if dest_allowed(&dest_url, true) {
                    Some(Event::Start(Tag::Link {
                        link_type,
                        dest_url,
                        title,
                        id,
                    }))
                } else {
                    dropped_links += 1; // keep the inner text, lose the anchor
                    None
                }
            }
            Event::End(TagEnd::Link) if dropped_links > 0 => {
                dropped_links -= 1;
                None
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                if dest_allowed(&dest_url, false) {
                    Some(Event::Start(Tag::Image {
                        link_type,
                        dest_url,
                        title,
                        id,
                    }))
                } else {
                    dropped_images += 1; // alt text renders as plain text
                    None
                }
            }
            Event::End(TagEnd::Image) if dropped_images > 0 => {
                dropped_images -= 1;
                // The alt text already flowed through as ordinary events; a
                // dropped image needs no closing output.
                None
            }
            other => Some(other),
        }
    });

    let mut html = String::new();
    match resolve {
        None => push_marked(&mut html, events, framed),
        Some(resolve) => {
            // WAVE-11: the prose, each run of text taken whole (the parser
            // may split one), its references swapped — never inside code,
            // a link or an image's text.
            let mut out: Vec<Event> = Vec::new();
            let mut text: Option<String> = None;
            let (mut code, mut link, mut image) = (0u32, 0u32, 0u32);
            for ev in events {
                if let Event::Text(t) = &ev {
                    if code == 0 && link == 0 && image == 0 {
                        text.get_or_insert_with(String::new).push_str(t);
                        continue;
                    }
                }
                if let Some(t) = text.take() {
                    substitute(t, resolve, &mut out);
                }
                match &ev {
                    Event::Start(Tag::CodeBlock(_)) => code += 1,
                    Event::End(TagEnd::CodeBlock) => code = code.saturating_sub(1),
                    Event::Start(Tag::Link { .. }) => link += 1,
                    Event::End(TagEnd::Link) => link = link.saturating_sub(1),
                    Event::Start(Tag::Image { .. }) => image += 1,
                    Event::End(TagEnd::Image) => image = image.saturating_sub(1),
                    _ => {}
                }
                out.push(ev);
            }
            if let Some(t) = text.take() {
                substitute(t, resolve, &mut out);
            }
            push_marked(&mut html, out.into_iter(), framed);
        }
    }
    Rendered { html, diagrams }
}

/// COMP-12: the attribute that marks, in a frame, an element markdown
/// produced — what the SDK's base typography (`frontend/sdk/menger-sdk.js`)
/// still styles in a body that opts out (`<meta name="menger-base"
/// content="markdown">`): there an element the body writes as HTML gets no
/// base style unless it carries the mark itself.
pub const MD_MARK: &str = "data-md";

/// COMP-12: the elements markdown produces that carry [`MD_MARK`] — the
/// ones the SDK's base typography has a rule for (frame.rs's tests hold the
/// two lists together).
pub const MD_MARKED: &[&str] = &["h1", "h2", "h3", "h4", "code", "pre", "a", "blockquote", "table", "th", "td", "img"];

/// `push_html`; framed (COMP-12), every [`MD_MARKED`] element the markdown
/// produced marked with [`MD_MARK`], and no other. HTML events — the body's
/// own, block and inline, and the ones this module writes (placeholders,
/// cards, wave references) — are set aside as a NUL each (a source's NULs are
/// gone before parsing), the rest is rendered, its start tags marked (its
/// text is escaped, so every `<` in it opens a tag the renderer wrote), and
/// the HTML put back as it was. A sentinel keeps its HTML's trailing newline,
/// so the renderer spaces blocks as it did; inside an image's text the HTML
/// stays where it is (the renderer writes it there as escaped text).
fn push_marked<'a>(out: &mut String, events: impl Iterator<Item = Event<'a>>, framed: bool) {
    if !framed {
        html::push_html(out, events);
        return;
    }
    let mut set_aside: Vec<CowStr<'a>> = Vec::new();
    let mut image = 0u32;
    let mut written = String::new();
    html::push_html(
        &mut written,
        events.map(|ev| match ev {
            Event::Start(Tag::Image { .. }) => {
                image += 1;
                ev
            }
            Event::End(TagEnd::Image) => {
                image = image.saturating_sub(1);
                ev
            }
            Event::Html(s) | Event::InlineHtml(s) if image == 0 && !s.is_empty() => {
                let newline = s.ends_with('\n');
                set_aside.push(if newline { CowStr::from(s[..s.len() - 1].to_string()) } else { s });
                Event::Html(CowStr::Borrowed(if newline { "\0\n" } else { "\0" }))
            }
            other => other,
        }),
    );
    let mut set_aside = set_aside.into_iter();
    for (i, part) in written.split('\0').enumerate() {
        if i > 0 {
            out.push_str(set_aside.next().as_deref().unwrap_or(""));
        }
        let mut rest = part;
        while let Some(lt) = rest.find('<') {
            let after = &rest[lt + 1..];
            let name_len = after.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(after.len());
            let name = &after[..name_len];
            out.push_str(&rest[..lt + 1 + name_len]);
            if MD_MARKED.contains(&name) {
                out.push(' ');
                out.push_str(MD_MARK);
            }
            rest = &after[name_len..];
        }
        out.push_str(rest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The MFE-7 tests' spelling: just the HTML.
    fn render_markdown(src: &str) -> String {
        render_body(src).html
    }

    // -- sanitization -------------------------------------------------

    #[test]
    fn script_tag_renders_inert() {
        let out = render_markdown("before\n\n<script>window.__pwned = 1</script>\n\nafter");
        assert!(
            !out.contains("<script"),
            "no live script tag may survive:\n{out}"
        );
        assert!(
            out.contains("&lt;script&gt;"),
            "the tag renders as visible text:\n{out}"
        );
    }

    #[test]
    fn inline_html_renders_inert() {
        let out = render_markdown("hello <img src=x onerror=\"window.__pwned=1\"> world");
        assert!(!out.contains("<img"), "no live element:\n{out}");
        assert!(out.contains("&lt;img"), "escaped instead:\n{out}");
    }

    #[test]
    fn javascript_link_loses_its_anchor_but_keeps_its_text() {
        let out = render_markdown("[click me](javascript:alert(1))");
        assert!(!out.contains("javascript:"), "scheme gone:\n{out}");
        assert!(!out.contains("<a "), "anchor gone:\n{out}");
        assert!(out.contains("click me"), "text kept:\n{out}");
    }

    #[test]
    fn http_links_and_relative_links_survive() {
        let out = render_markdown("[a](https://example.com) [b](/app/#/ticket/MNG-2)");
        assert!(out.contains("href=\"https://example.com\""), "{out}");
        assert!(out.contains("href=\"/app/#/ticket/MNG-2\""), "{out}");
    }

    #[test]
    fn data_image_is_dropped_http_image_survives() {
        let out =
            render_markdown("![x](data:text/html,<script>1</script>) ![y](https://e.com/i.png)");
        assert!(!out.contains("data:"), "{out}");
        assert!(out.contains("src=\"https://e.com/i.png\""), "{out}");
    }

    // -- formatting (acceptance criterion 2's positive half) ----------

    #[test]
    fn headings_emphasis_lists_and_code_spans_format() {
        let out = render_markdown(
            "# Title\n\nSome *emphasis* and **bold**.\n\n- one\n- two\n\nA `code span`.",
        );
        assert!(out.contains("<h1>Title</h1>"), "{out}");
        assert!(out.contains("<em>emphasis</em>"), "{out}");
        assert!(out.contains("<strong>bold</strong>"), "{out}");
        assert!(out.contains("<li>one</li>"), "{out}");
        assert!(out.contains("<code>code span</code>"), "{out}");
    }

    // -- frontmatter ---------------------------------------------------

    #[test]
    fn frontmatter_splits_off_and_markdown_follows() {
        let body = "---\nid: MNG-2\nstatus: Ready\n---\n\n# After\n";
        let (fm, rest) = split_frontmatter(body);
        assert_eq!(fm, Some("id: MNG-2\nstatus: Ready\n"));
        assert_eq!(rest, "\n# After\n");
        assert!(render_markdown(rest).contains("<h1>After</h1>"));
    }

    #[test]
    fn unclosed_fence_is_not_frontmatter() {
        let body = "---\nid: MNG-2\nno closing fence";
        assert_eq!(split_frontmatter(body), (None, body));
    }

    #[test]
    fn body_not_starting_with_fence_is_untouched() {
        let body = "# Heading\n\n---\n\ntext after a thematic break";
        assert_eq!(split_frontmatter(body), (None, body));
    }

    #[test]
    fn closing_fence_may_be_the_last_line_without_newline() {
        let (fm, rest) = split_frontmatter("---\na: 1\n---");
        assert_eq!(fm, Some("a: 1\n"));
        assert_eq!(rest, "");
    }

    #[test]
    fn dash_dash_dash_alone_is_a_thematic_break_not_frontmatter() {
        assert_eq!(split_frontmatter("---"), (None, "---"));
        assert_eq!(
            split_frontmatter("--- not a fence\nx"),
            (None, "--- not a fence\nx")
        );
    }

    // -- mermaid fences (AGS-14) ----------------------------------------

    const FLOW: &str =
        "# Title\n\n```mermaid\nflowchart LR\n  A[start] --> B{ok?}\n```\n\ntail text\n";

    #[test]
    fn mermaid_fence_becomes_a_pending_placeholder_holding_the_source() {
        let r = render_body(FLOW);
        assert_eq!(r.diagrams, 1);
        assert!(
            r.html.contains(
                "<div class=\"mermaid\" data-state=\"pending\"><pre class=\"mermaid-src\"><code>"
            ),
            "placeholder wrapper:\n{}",
            r.html
        );
        assert!(
            r.html.contains("flowchart LR\n  A[start] --&gt; B{ok?}\n"),
            "source kept, escaped:\n{}",
            r.html
        );
        assert!(
            !r.html.contains("language-mermaid"),
            "no ordinary code block remains:\n{}",
            r.html
        );
        // The surrounding markdown is untouched.
        assert!(r.html.contains("<h1>Title</h1>"), "{}", r.html);
        assert!(r.html.contains("<p>tail text</p>"), "{}", r.html);
    }

    #[test]
    fn render_markdown_is_the_same_html() {
        assert_eq!(render_markdown(FLOW), render_body(FLOW).html);
    }

    #[test]
    fn other_fences_stay_code_blocks_and_count_no_diagram() {
        let r =
            render_body("```rust\nfn main() {}\n```\n\n```\nplain\n```\n\n    indented mermaid\n");
        assert_eq!(r.diagrams, 0);
        assert!(
            r.html.contains("<code class=\"language-rust\">"),
            "{}",
            r.html
        );
        assert!(!r.html.contains("class=\"mermaid\""), "{}", r.html);
    }

    #[test]
    fn a_mention_of_mermaid_is_not_a_fence() {
        let r = render_body("Use `mermaid` here.\n\n```text\nmermaid\n```\n");
        assert_eq!(r.diagrams, 0);
        assert!(!r.html.contains("class=\"mermaid\""), "{}", r.html);
    }

    #[test]
    fn fence_info_may_carry_attributes_after_the_language() {
        assert_eq!(
            render_body("```mermaid title=\"x\"\ngraph TD\n```\n").diagrams,
            1
        );
        // ...but the language itself is exact.
        assert_eq!(render_body("```mermaidx\ngraph TD\n```\n").diagrams, 0);
        assert_eq!(render_body("```Mermaid\ngraph TD\n```\n").diagrams, 0);
    }

    #[test]
    fn markup_inside_a_mermaid_fence_is_escaped_never_live() {
        let r = render_body(
            "```mermaid\nflowchart LR\n  A[\"<img src=x onerror=window.__pwned=1>\"] --> B\n  click A href \"javascript:window.__pwned=1\"\n```\n<script>window.__pwned=1</script>\n",
        );
        assert_eq!(r.diagrams, 1);
        assert!(!r.html.contains("<img"), "no live element:\n{}", r.html);
        assert!(!r.html.contains("<script"), "no live script:\n{}", r.html);
        assert!(
            r.html
                .contains("&lt;img src=x onerror=window.__pwned=1&gt;"),
            "{}",
            r.html
        );
        assert!(
            r.html.contains("&quot;javascript:window.__pwned=1&quot;"),
            "{}",
            r.html
        );
    }

    #[test]
    fn two_fences_yield_two_placeholders() {
        let r = render_body(
            "```mermaid\ngraph TD\nA-->B\n```\n\ntext\n\n```mermaid\npie\n\"a\": 1\n```\n",
        );
        assert_eq!(r.diagrams, 2);
        assert_eq!(
            r.html.matches("data-state=\"pending\"").count(),
            2,
            "{}",
            r.html
        );
    }

    #[test]
    fn unclosed_fence_still_yields_a_placeholder() {
        // A body still streaming in (AGS-7) may end mid-fence: the parser
        // closes it at EOF and the renderer will show whatever error the
        // partial source earns — never a thrown exception.
        let r = render_body("intro\n\n```mermaid\nflowchart LR\n  A -->");
        assert_eq!(r.diagrams, 1);
        assert!(r.html.contains("flowchart LR\n  A --&gt;"), "{}", r.html);
        assert!(r.html.contains("<p>intro</p>"), "{}", r.html);
    }

    // -- wave references (WAVE-11) ---------------------------------------

    fn placed(id: &str) -> Option<WaveRef> {
        match id {
            "RETURNS-6" => Some(WaveRef::Wave(2)),
            "RETURNS-4" => Some(WaveRef::Done),
            "GAP-3" => Some(WaveRef::Missing),
            _ => None,
        }
    }

    #[test]
    fn a_wave_reference_renders_as_the_ordinal_its_resolver_gives() {
        let r = render_body_with("The envelope work is {wave: RETURNS-6}, after {wave:RETURNS-4}; {wave:  GAP-3 } waits.", &placed);
        assert!(
            r.html.contains(
                "<a class=\"wave-ref\" data-wave-ref=\"RETURNS-6\" data-wave=\"2\" href=\"#/ticket/RETURNS-6\" title=\"RETURNS-6: wave 2\">wave 2</a>"
            ),
            "{}",
            r.html
        );
        assert!(r.html.contains(">done</span>") && r.html.contains("data-wave-ref=\"RETURNS-4\""), "{}", r.html);
        assert!(r.html.contains(">missing blocker</span>") && r.html.contains("data-wave-ref=\"GAP-3\""), "{}", r.html);
        assert!(r.html.starts_with("<p>The envelope work is <a"), "{}", r.html);
        assert!(r.html.contains("</span> waits.</p>"), "{}", r.html);
        // Case: the id as written is upper-cased for the resolver.
        assert!(render_body_with("{wave: returns-6}", &placed).html.contains("wave 2</a>"));
    }

    #[test]
    fn a_reference_nothing_places_is_left_as_written() {
        let src = "Unknown: {wave: NOPE-1}. Not a reference: {wave: x}, {wave RETURNS-6}, {wave: RETURNS-6 trailing}.";
        let r = render_body_with(src, &placed);
        assert_eq!(r, render_body(src), "byte for byte what render_body gives");
        assert!(r.html.contains("{wave: NOPE-1}"), "{}", r.html);
    }

    #[test]
    fn a_body_without_a_reference_renders_byte_for_byte_as_before() {
        let never = |_: &str| -> Option<WaveRef> { panic!("nothing to resolve") };
        for src in [FLOW, "# T\n\n- a\n- *b* `c`\n\n> q\n\n| x | y |\n|---|---|\n| 1 | 2 |\n", "plain {not a wave} text"] {
            assert_eq!(render_body_with(src, &never), render_body(src), "{src}");
        }
    }

    #[test]
    fn a_reference_in_code_a_link_or_an_image_is_text() {
        let src = "`{wave: RETURNS-6}`\n\n```\n{wave: RETURNS-6}\n```\n\n[{wave: RETURNS-6}](https://e.com) ![{wave: RETURNS-6}](https://e.com/i.png) {wave: RETURNS-6}";
        let r = render_body_with(src, &placed);
        assert_eq!(r.html.matches("class=\"wave-ref\"").count(), 1, "only the one in prose: {}", r.html);
        assert!(r.html.contains("<code>{wave: RETURNS-6}</code>"), "{}", r.html);
        assert!(r.html.contains(">{wave: RETURNS-6}</a>"), "{}", r.html);
    }

    #[test]
    fn a_reference_keeps_markup_out_and_mermaid_still_counts() {
        let r = render_body_with("<script>x</script> {wave: RETURNS-6}\n\n```mermaid\ngraph LR\n```\n", &placed);
        assert!(!r.html.contains("<script"), "{}", r.html);
        assert_eq!(r.diagrams, 1);
        assert!(r.html.contains("wave 2</a>"), "{}", r.html);
    }

    // -- framed bodies (COMP-4) ----------------------------------------

    /// COMP-4 acceptance 1: what frames — a raw HTML block, an inline tag, a
    /// custom element (`<m-x>`), a comment — and what does not: HTML inside
    /// a code span, a fence (mermaid's included) or indented code, an
    /// autolink, and the angle-bracketed placeholder prose is full of.
    #[test]
    fn a_body_frames_when_it_holds_html_outside_code() {
        for framed in [
            "<details>\n<summary>more</summary>\n\nhidden *markdown*\n\n</details>\n",
            "<table><tr><td>1</td></tr></table>\n",
            "text with <b>bold</b> inline",
            "a line<br>broken",
            "| a | b |\n|---|---|\n| x<br>y | z |\n",
            "uses <m-x></m-x> here",
            "<m-wave-summary board=PLEXUS>\n",
            "see <M-Wave-Summary board=PLEXUS/>",
            "<!-- a note -->\n\nprose",
            "<style>p { color: red }</style>\n",
            "<script>document.body.append('x')</script>\n",
            "<svg width=\"10\"></svg>",
            "</div>",
        ] {
            assert!(is_framed(framed), "should frame: {framed:?}");
        }
        for unframed in [
            "",
            "plain *markdown* — `code` and **bold**",
            "a code span: `<b>bold</b>` and `<m-x>`",
            "```html\n<div><script>x()</script></div>\n```\n",
            "```mermaid\ngraph LR\n  A[\"a<br/>b\"] --> B\n```\n",
            "    <div>indented code</div>\n",
            "an autolink <https://example.com> and <a@b.co>",
            "repo@<org>, Vec<String>, Option<T>, --id <id> --path <path>",
            "1 < 2 and 3 > 2, a <- b, <=>",
            "<!DOCTYPE html>",
        ] {
            assert!(!is_framed(unframed), "should not frame: {unframed:?}");
        }
    }

    /// COMP-4: the snapshot's one body CommonMark reads as raw HTML (SDLC-3,
    /// a `<org>` placeholder) stays unframed and renders as it always did.
    #[test]
    fn a_placeholder_is_not_html_and_its_body_renders_as_before() {
        let src = "Use `repo@<org>` or repo@<org> and Vec<String>.";
        assert!(!is_framed(src));
        assert_eq!(
            render_body(src).html,
            "<p>Use <code>repo@&lt;org&gt;</code> or repo@&lt;org&gt; and Vec&lt;String&gt;.</p>\n"
        );
    }

    /// COMP-4 acceptance 1: pass-through — raw HTML, block and inline, is the
    /// markup itself; the markdown around it renders as ever; markdown inside
    /// an HTML block (after a blank line) is still markdown.
    #[test]
    fn a_framed_body_passes_its_html_through() {
        let src = "# Title\n\n<details>\n<summary>more</summary>\n\nhidden *markdown*\n\n</details>\n\n<style>p { color: red }</style>\n\n<script>document.body.append('ran')</script>\n\nA <b>bold</b> word and <m-x a=\"1\"></m-x>.\n";
        let r = render_body_framed(src, &|_| None);
        assert_eq!(
            r.html,
            "<h1 data-md>Title</h1>\n<details>\n<summary>more</summary>\n<p>hidden <em>markdown</em></p>\n</details>\n<style>p { color: red }</style>\n<script>document.body.append('ran')</script>\n<p>A <b>bold</b> word and <m-x a=\"1\"></m-x>.</p>\n"
        );
        assert_eq!(r.diagrams, 0);
        // The same body unframed is MFE-7's: every tag is text.
        let inert = render_body(src).html;
        assert!(!inert.contains("<script") && !inert.contains("<details") && !inert.contains("<b>"), "{inert}");
    }

    /// COMP-4: inside a frame a mermaid fence is still a placeholder (the
    /// SDK draws it), a `{wave: ID}` in prose is still placed — by the host,
    /// before the frame sees it — and a markdown link keeps its allow-list.
    #[test]
    fn a_framed_body_keeps_mermaid_wave_references_and_the_link_rules() {
        let src = "<div class=\"x\">box</div>\n\n{wave: RETURNS-6} and {wave: NOPE-1}\n\n```mermaid\ngraph LR\n  A --> B\n```\n\n[bad](javascript:alert(1)) [good](https://e.com)\n";
        let r = render_body_framed(src, &placed);
        assert_eq!(r.diagrams, 1);
        assert!(r.html.starts_with("<div class=\"x\">box</div>\n"), "{}", r.html);
        assert!(r.html.contains("<div class=\"mermaid\" data-state=\"pending\"><pre class=\"mermaid-src\"><code>graph LR\n  A --&gt; B\n</code></pre></div>"), "{}", r.html);
        assert!(r.html.contains(">wave 2</a> and {wave: NOPE-1}</p>"), "{}", r.html);
        assert!(!r.html.contains("javascript:") && r.html.contains("bad <a data-md href=\"https://e.com\">good</a>"), "{}", r.html);
        // Without a reference, the resolver is never asked.
        let never = |_: &str| -> Option<WaveRef> { panic!("nothing to resolve") };
        assert_eq!(render_body_framed("<b>x</b>", &never).html, "<p><b>x</b></p>\n");
    }

    /// COMP-4 acceptance 1, "unframed output unchanged": the HTML md.rs gave
    /// for these fixtures before COMP-4, recorded byte for byte from the
    /// pre-COMP-4 renderer — the MFE-7, AGS-14 and WAVE-11 fixtures, plus the
    /// markdown features they did not cover. None of them frames.
    #[test]
    fn unframed_bodies_render_byte_for_byte_as_before_comp_4() {
        let golden: &[(&str, &str)] = &[
            (FLOW, "<h1>Title</h1>\n<div class=\"mermaid\" data-state=\"pending\"><pre class=\"mermaid-src\"><code>flowchart LR\n  A[start] --&gt; B{ok?}\n</code></pre></div>\n<p>tail text</p>\n"),
            ("# T\n\n- a\n- *b* `c`\n\n> q\n\n| x | y |\n|---|---|\n| 1 | 2 |\n", "<h1>T</h1>\n<ul>\n<li>a</li>\n<li><em>b</em> <code>c</code></li>\n</ul>\n<blockquote>\n<p>q</p>\n</blockquote>\n<table><thead><tr><th>x</th><th>y</th></tr></thead><tbody>\n<tr><td>1</td><td>2</td></tr>\n</tbody></table>\n"),
            ("# Title\n\nSome *emphasis* and **bold**.\n\n- one\n- two\n\nA `code span`.", "<h1>Title</h1>\n<p>Some <em>emphasis</em> and <strong>bold</strong>.</p>\n<ul>\n<li>one</li>\n<li>two</li>\n</ul>\n<p>A <code>code span</code>.</p>\n"),
            ("[click me](javascript:alert(1)) [a](https://example.com) [b](/app/#/ticket/MNG-2) ![x](data:text/html,<script>1</script>) ![y](https://e.com/i.png)", "<p>click me <a href=\"https://example.com\">a</a> <a href=\"/app/#/ticket/MNG-2\">b</a> x <img src=\"https://e.com/i.png\" alt=\"y\" /></p>\n"),
            ("```rust\nfn main() {}\n```\n\n```\nplain\n```\n\n    indented mermaid\n", "<pre><code class=\"language-rust\">fn main() {}\n</code></pre>\n<pre><code>plain\n</code></pre>\n<pre><code>indented mermaid\n</code></pre>\n"),
            ("Use `mermaid` here.\n\n```text\nmermaid\n```\n", "<p>Use <code>mermaid</code> here.</p>\n<pre><code class=\"language-text\">mermaid\n</code></pre>\n"),
            ("intro\n\n```mermaid\nflowchart LR\n  A -->", "<p>intro</p>\n<div class=\"mermaid\" data-state=\"pending\"><pre class=\"mermaid-src\"><code>flowchart LR\n  A --&gt;</code></pre></div>\n"),
            ("- [ ] task\n- [x] done\n\n~~gone~~ and a footnote[^1].\n\n[^1]: the note\n\n---\n\n1. one\n2. two\n", "<ul>\n<li><input disabled=\"\" type=\"checkbox\"/>\ntask</li>\n<li><input disabled=\"\" type=\"checkbox\" checked=\"\"/>\ndone</li>\n</ul>\n<p><del>gone</del> and a footnote<sup class=\"footnote-reference\"><a href=\"#1\">1</a></sup>.</p>\n<div class=\"footnote-definition\" id=\"1\"><sup class=\"footnote-definition-label\">1</sup>\n<p>the note</p>\n</div>\n<hr />\n<ol>\n<li>one</li>\n<li>two</li>\n</ol>\n"),
        ];
        for (src, html) in golden {
            assert!(!is_framed(src), "a golden fixture frames: {src:?}");
            assert_eq!(render_body(src).html, *html, "{src:?}");
            assert_eq!(render_body_with(src, &placed).html, *html, "{src:?}");
        }
        let golden_with: &[(&str, &str)] = &[
            ("The envelope work is {wave: RETURNS-6}, after {wave:RETURNS-4}; {wave:  GAP-3 } waits.", "<p>The envelope work is <a class=\"wave-ref\" data-wave-ref=\"RETURNS-6\" data-wave=\"2\" href=\"#/ticket/RETURNS-6\" title=\"RETURNS-6: wave 2\">wave 2</a>, after <span class=\"wave-ref done\" data-wave-ref=\"RETURNS-4\" title=\"RETURNS-4: done\">done</span>; <span class=\"wave-ref missing\" data-wave-ref=\"GAP-3\" title=\"GAP-3: waits on a ticket that does not exist\">missing blocker</span> waits.</p>\n"),
            ("`{wave: RETURNS-6}`\n\n```\n{wave: RETURNS-6}\n```\n\n[{wave: RETURNS-6}](https://e.com) ![{wave: RETURNS-6}](https://e.com/i.png) {wave: RETURNS-6}", "<p><code>{wave: RETURNS-6}</code></p>\n<pre><code>{wave: RETURNS-6}\n</code></pre>\n<p><a href=\"https://e.com\">{wave: RETURNS-6}</a> <img src=\"https://e.com/i.png\" alt=\"{wave: RETURNS-6}\" /> <a class=\"wave-ref\" data-wave-ref=\"RETURNS-6\" data-wave=\"2\" href=\"#/ticket/RETURNS-6\" title=\"RETURNS-6: wave 2\">wave 2</a></p>\n"),
        ];
        for (src, html) in golden_with {
            assert!(!is_framed(src), "{src:?}");
            assert_eq!(render_body_with(src, &placed).html, *html, "{src:?}");
        }
        // MFE-7's own fixture still renders inert through render_body — a
        // body like it is framed in the app, which never paints this HTML.
        assert_eq!(
            render_body("before\n\n<script>window.__pwned = 1</script>\n\nafter").html,
            "<p>before</p>\n&lt;script&gt;window.__pwned = 1&lt;/script&gt;\n<p>after</p>\n"
        );
    }

    /// COMP-9: a definition frames its body, and in the frame renders as
    /// its docs card — built from the block: its docs, its preview line,
    /// its source folded; the help through this module, holding no card of
    /// its own. Outside a frame (never reached for such a body) it is the
    /// code block it always was; in an example fence or indented code it
    /// is text and frames nothing.
    #[test]
    fn a_definition_frames_and_renders_as_its_card() {
        let body = "Defines it.\n\n```component m-x board=RETURNS\n<template>x</template>\n<docs>\n  <summary>Says x.</summary>\n  <attr name=\"board\" type=\"shortcode\" required>Which.</attr>\n  Help with a **bold** word.\n\n  ~~~component m-nested\n  y\n  ~~~\n</docs>\n```\n\nAfter.\n";
        assert!(is_framed(body), "a definition frames");
        let html = render_body_framed(body, &|_| None).html;
        assert!(html.contains("<p>Defines it.</p>") && html.contains("<p>After.</p>"), "{html}");
        assert!(html.contains("<section class=\"menger-card\" data-menger-card=\"m-x\">"), "{html}");
        assert!(html.contains("<span class=\"mc-summary\">Says x.</span>"), "{html}");
        assert!(html.contains("<m-x board=\"RETURNS\"></m-x>"), "the live preview: {html}");
        assert!(html.contains("<td><code>board</code></td><td><code>shortcode</code></td>"), "{html}");
        assert!(html.contains("<strong>bold</strong>"), "the help as markdown: {html}");
        assert!(html.contains("<pre data-md><code data-md class=\"language-component\">y\n</code></pre>"), "a definition in the help is a code block: {html}");
        assert_eq!(html.matches("class=\"menger-card\"").count(), 1, "no card in a card");
        assert!(html.contains("<details class=\"mc-source\">"), "{html}");
        assert!(!html.contains("class=\"language-component\">&lt;template"), "the block is not a code block: {html}");
        // Unframed, the block is the code block it was.
        assert!(render_body(body).html.contains("<pre><code class=\"language-component\">"));
        // A definition shown as an example, or indented, frames nothing.
        assert!(!is_framed("````markdown\n```component m-x\nx\n```\n````\n"));
        assert!(!is_framed("Write:\n\n    ```component m-x\n    x\n    ```\n"));
        assert!(!is_framed("```rust\nlet component = 1;\n```\n"));
        // A draft's malformed docs, and a bad tag, render as said problems.
        let bad = render_body_framed("```component m-x\n<docs><summary>a</summary><summary>b</summary></docs>\n```\n", &|_| None).html;
        assert!(bad.contains("malformed &lt;docs&gt;: two &lt;summary&gt; elements"), "{bad}");
        let unnamed = render_body_framed("```component Chart\nx\n```\n", &|_| None).html;
        assert!(unnamed.contains("is not a component name") && !unnamed.contains("<Chart"), "{unnamed}");
    }

    // -- blank lines inside HTML (COMP-11) ---------------------------------

    /// An element opened in an HTML block runs to its close, blank lines
    /// included: the markup inside passes through as written — no indented
    /// line made code, no inline tag wrapped in a `<p>` — and markdown after
    /// it is markdown again.
    #[test]
    fn html_with_blank_lines_runs_to_its_close() {
        let html = "<div class=\"board\">\n<h2>Board</h2>\n<ul>\n  <li>a</li>\n\n    <li><span>b</span> *as written*</li>\n</ul>\n\n<span class=\"note\">c</span>\n</div>\n";
        let src = format!("{html}\nAfter, **markdown**.\n");
        assert_eq!(render_body_framed(&src, &|_| None).html, format!("{html}<p>After, <strong>markdown</strong>.</p>\n"));
        // CommonMark alone (the unframed parse) ends the div at the first
        // blank line: what the frame used to draw.
        let unframed = render_body(&src).html;
        assert!(unframed.contains("<pre><code>") && unframed.contains("<p>&lt;span class=\"note\"&gt;"), "{unframed}");
    }

    /// Nested elements with blank lines, and an indented `<pre>` with blank
    /// lines of its own, pass through byte for byte.
    #[test]
    fn nested_elements_with_blank_lines_and_an_indented_pre_pass_through() {
        let src = "<section class=\"s\">\n  <div class=\"a\">\n    <p>one</p>\n\n    <div class=\"b\">\n      <span>two</span>\n\n      <pre>\nline 1\n\n    line 2, indented\n\n      </pre>\n    </div>\n  </div>\n\n  <m-x data-field=\"note\">\n\n    <b>three</b>\n  </m-x>\n</section>\n";
        let r = render_body_framed(src, &|_| None);
        assert_eq!(r.html, src);
        assert_eq!(r.diagrams, 0);
        // A `<pre class="mermaid">` there is the diagram, its blank line kept.
        let src = "<section data-field=\"shape\">\n  <h2>Shape</h2>\n  <p>The flow.</p>\n\n    <pre class=\"mermaid\">\ngraph LR\n\n  A --> B\n    </pre>\n</section>\n";
        let r = render_body_framed(src, &|_| None);
        assert_eq!(r.diagrams, 1);
        assert!(r.html.contains(&mermaid_placeholder("graph LR\n\n  A --> B\n    ")), "{}", r.html);
        // A top-level `<pre>` (CommonMark's type 1) was already whole: as it was.
        assert_eq!(render_body_framed("<pre>\na\n\n    b\n</pre>\n\nc\n", &|_| None).html, "<pre>\na\n\n    b\n</pre>\n<p>c</p>\n");
    }

    /// Markdown inside HTML is still where the house writes it: after a blank
    /// line right after an element's opening — its open tag, or that and a
    /// `<summary>` or a heading — and in a fence with blank lines around it.
    /// Data-field components, definitions, mermaid fences and `{wave: ID}`s
    /// there render as they did.
    #[test]
    fn markdown_inside_html_keeps_its_idioms() {
        let src = "<m-summary ticket=\"X-1\">\n\n<m-field data-field=\"standing\">**Standing**, {wave: RETURNS-6}.</m-field>\n\n</m-summary>\n\n<section data-field=\"shape\">\n<h2>Shape</h2>\n\n```mermaid\ngraph LR\n  A --> B\n```\n\n<p>Under it.</p>\n</section>\n\n<div>\n\n*Emphasized* text.\n\n</div>\n";
        let r = render_body_framed(src, &placed);
        assert_eq!(r.diagrams, 1);
        assert_eq!(
            r.html,
            format!(
                "<m-summary ticket=\"X-1\">\n<p><m-field data-field=\"standing\"><strong>Standing</strong>, {}.</m-field></p>\n</m-summary>\n<section data-field=\"shape\">\n<h2>Shape</h2>\n{}<p>Under it.</p>\n</section>\n<div>\n<p><em>Emphasized</em> text.</p>\n</div>\n",
                wave_markup("RETURNS-6", WaveRef::Wave(2)),
                mermaid_placeholder("graph LR\n  A --> B\n")
            )
        );
        // A definition inside an element, with blank lines around it, is its card.
        let html = render_body_framed("<div class=\"defs\">\n<p>Defines:</p>\n\n```component m-x\n<template>x</template>\n```\n\n</div>\n", &|_| None).html;
        assert!(html.contains("data-menger-card=\"m-x\"") && !html.contains("language-component"), "{html}");
        // Right after an open tag, a line indented as code is HTML going on —
        // never the code block no element means.
        let src = "<div>\n\n    <p>indented</p>\n</div>\n";
        assert_eq!(render_body_framed(src, &|_| None).html, src);
    }

    /// The DESIGN-3…9 bodies' shape — a stylesheet, then a page of nested
    /// HTML, indented, with comments — written with blank lines between its
    /// sections renders as it does without them, but for the blank lines.
    #[test]
    fn a_page_of_html_with_blank_lines_renders_as_without_them() {
        let page = "\
A candidate language, drawn below.

<style>
.x-grid{display:grid;grid-template-columns:repeat(12,1fr)}

.x-row{display:flex}
</style>
<div class=\"x\">
<!-- header -->
<header class=\"x-grid x-head\">
  <div class=\"x-word\">menger</div>
  <div class=\"x-who\"><span>signed in as</span> <b>someone</b></div>
</header>
<!-- nav -->
<div class=\"x-grid x-shell\">
<nav class=\"x-nav\" aria-label=\"Shortcodes\">
  <div class=\"x-filter\"><span class=\"x-label\">Filter</span><code>not category done</code></div>
  <ul class=\"x-tree\">
    <li class=\"x-root\">
      <div class=\"x-n\"><span class=\"x-m\" data-s=\"epic\"></span><span class=\"x-t\">ROOT</span></div>
      <ul>
        <li><a class=\"x-row\" href=\"#\">AREA <em>one</em></a></li>
        <li><a class=\"x-row\" href=\"#\">AREA <em>two</em></a></li>
      </ul>
    </li>
  </ul>
</nav>
<main class=\"x-main\">
  <section class=\"x-board\">
    <h2 class=\"x-label\">Wave 1</h2>
    <span class=\"x-count\">3 tickets</span>
    <div class=\"x-cards\">
      <article class=\"x-card\"><span class=\"x-id\">T-1</span> a_title_with_underscores *and stars*</article>
      <article class=\"x-card\"><span class=\"x-id\">T-2</span></article>
    </div>
  </section>
  <section class=\"x-ticket\">
    <pre class=\"x-code\">fn main() {
    println!(\"kept\");
}</pre>
  </section>
</main>
</div>
</div>
";
        // Blank lines put back between its sections — before each tag line
        // that does not directly follow an open tag (where a blank line asks
        // for markdown).
        let mut spaced = String::new();
        let mut prev = "";
        let mut html = false;
        for line in page.lines() {
            html |= line.starts_with("<div");
            let opener = prev.trim_end().ends_with('>') && !prev.contains("</") && prev.trim_start().starts_with('<');
            if html && line.trim_start().starts_with('<') && !opener {
                spaced.push('\n');
            }
            spaced.push_str(line);
            spaced.push('\n');
            prev = line;
        }
        assert!(spaced.matches("\n\n").count() > 20, "{spaced}");
        let bare = |s: String| s.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n");
        let without = render_body_framed(page, &|_| None).html;
        let with = render_body_framed(&spaced, &|_| None).html;
        assert_eq!(bare(with.clone()), bare(without.clone()));
        assert!(!with.contains("<pre><code>") && !with.contains("<p><"), "{with}");
        assert!(without.contains("<p>A candidate language, drawn below.</p>"));
    }

    // -- diagrams written as HTML (ART-5) ---------------------------------

    /// ought's form — mermaid source in a `<pre class="mermaid">` — is drawn
    /// in a frame as a fence is: it becomes the same placeholder, holding its
    /// source with its references decoded and a `<br/>` in a label kept.
    #[test]
    fn a_framed_pre_mermaid_becomes_the_fences_placeholder() {
        let src = "<pre class=\"mermaid\">\ngraph LR\n  A[\"a<br/>b\"] --&gt; B &amp; C\n</pre>\n";
        assert!(is_framed(src));
        let r = render_body_framed(src, &|_| None);
        assert_eq!(r.diagrams, 1);
        assert_eq!(
            r.html,
            format!("{}\n", mermaid_placeholder("graph LR\n  A[\"a<br/>b\"] --> B & C\n"))
        );
        // Inside a field element, among its other markup: only the `<pre>`
        // is replaced.
        let src = "<figure data-field=\"dag\">\n  <pre class=\"mermaid\">\ngraph TD\n  A --> B\n  </pre>\n  <figcaption>The DAG.</figcaption>\n</figure>\n";
        let r = render_body_framed(src, &|_| None);
        assert_eq!(r.diagrams, 1);
        assert!(r.html.starts_with("<figure data-field=\"dag\">\n  <div class=\"mermaid\" data-state=\"pending\"><pre class=\"mermaid-src\"><code>graph TD\n  A --&gt; B\n  </code></pre></div>\n\n  <figcaption>The DAG.</figcaption>\n</figure>\n"), "{}", r.html);
        // Unframed (the page never paints such a body unframed), it is text.
        let r = render_body(src);
        assert_eq!(r.diagrams, 0);
        assert!(r.html.contains("&lt;pre class=\"mermaid\"&gt;") && !r.html.contains("<pre"), "{}", r.html);
    }

    /// Where a diagram written as HTML is one — and where it is not. Each
    /// case is also `diagrams/src/blocks.rs`'s: its tests read these and
    /// hold `check` to the same counts.
    #[test]
    fn diagrams_written_as_html_are_counted_where_the_frame_draws_them() {
        // Alone, on one line, beside other classes.
        assert_eq!(render_body_framed("<pre class=\"mermaid\">\ngraph LR\n  A --> B\n</pre>\n", &|_| None).diagrams, 1);
        assert_eq!(render_body_framed("<PRE Class='diagram mermaid' id=d1>graph TD; A-->B</PRE>\n", &|_| None).diagrams, 1);
        // Inside a field element, or a component, with a fence beside it.
        assert_eq!(render_body_framed("<figure data-field=\"dag\">\n  <pre class=\"mermaid\">\ngraph TD\n  A --> B\n  </pre>\n  <figcaption>The DAG.</figcaption>\n</figure>\n", &|_| None).diagrams, 1);
        assert_eq!(render_body_framed("<m-dag data-field=\"dag\">\n<pre class=\"mermaid\">graph TD; A-->B</pre>\n</m-dag>\n\n<section data-field=\"shape\">\n\n```mermaid\ngraph LR\n  A --> B\n```\n\n</section>\n", &|_| None).diagrams, 2);
        // A fence inside a component, blank lines around it: markdown, a
        // fence's placeholder.
        assert_eq!(render_body_framed("<m-dag data-field=\"dag\">\n\n```mermaid\ngraph TD\n  A --> B\n```\n\n</m-dag>\n", &|_| None).diagrams, 1);
        // In a block quote and a list item.
        assert_eq!(render_body_framed("> <pre class=\"mermaid\">\n> pie\n> </pre>\n\n- item\n\n  <pre class=\"mermaid\">\n  pie\n  </pre>\n", &|_| None).diagrams, 2);
        // COMP-11: a blank line inside a `<pre>` nested in a section no
        // longer ends the section's block — it is the diagram's.
        assert_eq!(render_body_framed("<section data-field=\"dag\">\n<pre class=\"mermaid\">\ngraph TD\n\n  A --> B\n</pre>\n</section>\n", &|_| None).diagrams, 1);
        // Not a diagram: a plain `<pre>`, another class, a comment, a
        // script's text, code, inline HTML in a paragraph, and a `<pre>`
        // nothing closes (its section's close ends it; no `</pre>`).
        assert_eq!(render_body_framed("<pre>graph LR</pre>\n", &|_| None).diagrams, 0);
        assert_eq!(render_body_framed("<pre class=\"mermaid-src\">graph LR</pre>\n<pre data-class=\"mermaid\">graph LR</pre>\n", &|_| None).diagrams, 0);
        assert_eq!(render_body_framed("<!-- <pre class=\"mermaid\">graph LR</pre> -->\n", &|_| None).diagrams, 0);
        assert_eq!(render_body_framed("<script>x = '<pre class=\"mermaid\">graph LR</pre>'</script>\n", &|_| None).diagrams, 0);
        assert_eq!(render_body_framed("```html\n<pre class=\"mermaid\">graph LR</pre>\n```\n\nWrite `<pre class=\"mermaid\">` and <b>bold</b>.\n", &|_| None).diagrams, 0);
        assert_eq!(render_body_framed("text <pre class=\"mermaid\">graph LR</pre> inline\n", &|_| None).diagrams, 0);
        assert_eq!(render_body_framed("<section data-field=\"dag\">\n<pre class=\"mermaid\">\ngraph TD\n\n  A --> B\n</section>\n", &|_| None).diagrams, 0);
    }

    /// A framed body with no diagram written as HTML renders byte for byte
    /// as before: gathering an HTML block's lines changes nothing else.
    #[test]
    fn gathering_html_blocks_changes_nothing_else() {
        let src = "<table>\n<tr><td>1</td></tr>\n</table>\n\n> <div>\n> quoted\n> </div>\n\n<pre>\nplain\n\nkept\n</pre>\n";
        assert_eq!(
            render_body_framed(src, &|_| None).html,
            "<table>\n<tr><td>1</td></tr>\n</table>\n<blockquote data-md>\n<div>\nquoted\n</div>\n</blockquote>\n<pre>\nplain\n\nkept\n</pre>\n"
        );
    }

    /// COMP-12: in a frame, what markdown produced carries the mark the
    /// SDK's base typography styles, and the body's own HTML does not —
    /// block or inline, beside markdown or inside it; outside a frame
    /// nothing is marked (the golden fixtures above).
    #[test]
    fn a_framed_body_marks_what_markdown_produced_and_nothing_else() {
        let src = "## H\n\nA `code` and <code class=\"x\">own</code>, [l](https://e.com) and <a href=\"https://e.com\">own</a>.\n\n<pre><code>own block</code></pre>\n\n```rust\nfn x() {}\n```\n\n> q\n\n| a |\n|---|\n| 1 |\n\n![<b>i</b>](https://e.com/i.png)\n\n<div>\n\n`inside`\n\n</div>\n";
        let html = render_body_framed(src, &|_| None).html;
        assert_eq!(
            html,
            "<h2 data-md>H</h2>\n<p>A <code data-md>code</code> and <code class=\"x\">own</code>, <a data-md href=\"https://e.com\">l</a> and <a href=\"https://e.com\">own</a>.</p>\n<pre><code>own block</code></pre>\n<pre data-md><code data-md class=\"language-rust\">fn x() {}\n</code></pre>\n<blockquote data-md>\n<p>q</p>\n</blockquote>\n<table data-md><thead><tr><th data-md>a</th></tr></thead><tbody>\n<tr><td data-md>1</td></tr>\n</tbody></table>\n<p><img data-md src=\"https://e.com/i.png\" alt=\"&lt;b&gt;i&lt;/b&gt;\" /></p>\n<div>\n<p><code data-md>inside</code></p>\n</div>\n"
        );
        // Unmarked, it is the renderer's own output: the mark is all that moved.
        assert_eq!(html.replace(" data-md", ""), { let mut plain = String::new(); html::push_html(&mut plain, Parser::new_ext(src, options())); plain });
        // COMP-11's run — an element's HTML across a blank line — is the
        // body's own HTML, whole and unmarked.
        let run = "<section>\n<p>own</p>\n\n<code>x</code> <table><tr><td>1</td></tr></table>\n</section>\n";
        assert_eq!(render_body_framed(run, &|_| None).html, run);
        // A source's NUL is not the sentinel: the HTML around it comes back whole.
        let html = render_body_framed("a\0b <i>x</i> `c\0`\n", &|_| None).html;
        assert_eq!(html, "<p>a\u{FFFD}b <i>x</i> <code data-md>c\u{FFFD}</code></p>\n");
    }

    #[test]
    fn the_ids_a_body_names_are_each_once_in_order() {
        assert_eq!(
            wave_ids("{wave: RETURNS-6} and {wave:context-4}, {wave: RETURNS-6} again; {wave: no}"),
            ["RETURNS-6", "CONTEXT-4"]
        );
        assert!(wave_ids("nothing here").is_empty());
    }
}
