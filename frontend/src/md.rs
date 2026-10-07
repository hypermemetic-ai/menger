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
//! WAVE-11: a **wave reference** — `{wave: RETURNS-6}` in prose — renders as
//! that ticket's current ordinal ([`render_body_with`]): `wave 2` (a link),
//! `done`, or `missing blocker`, as the caller's resolver places it; one it
//! cannot place (an id that is no ticket, a doc, a row not fetched yet) is
//! left as written. Only in prose: a reference inside code, a link or an
//! image's text is text. The markup is a fixed wrapper around escaped text,
//! the mermaid placeholder's discipline; and a body without `{wave:` takes
//! [`render_body`]'s path, so it renders byte for byte as before.

use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};
use pulldown_cmark_escape::{escape_href, escape_html};

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
    render(src, None, false)
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
    render(src, Some(resolve), false)
}

/// COMP-4: a framed body's markup — [`render_body_with`]'s, raw HTML passed
/// through (block and inline) instead of downgraded to text. Mermaid fences
/// still become placeholders (the frame's SDK draws them), `{wave: ID}`s in
/// prose are placed by `resolve` here, on the host, and markdown links and
/// images keep their scheme allow-list. Only for the sandboxed frame: this
/// HTML is never safe for `inner_html`.
pub fn render_body_framed(src: &str, resolve: &dyn Fn(&str) -> Option<WaveRef>) -> Rendered {
    if !src.contains(WAVE_OPEN) {
        return render(src, None, true);
    }
    render(src, Some(resolve), true)
}

/// COMP-4: does this body (its frontmatter already split off) render in a
/// frame? Yes when the markdown holds HTML outside code — a block or inline
/// tag naming an HTML element or a custom element, or a comment (see the
/// module doc for why a placeholder like `<org>` is not one). Code spans,
/// fences (mermaid's included) and indented code never frame: what is in
/// them is text, and the parser never reports it as HTML.
pub fn is_framed(src: &str) -> bool {
    if !src.contains('<') {
        return false;
    }
    Parser::new_ext(src, options()).any(|ev| match ev {
        Event::Html(s) | Event::InlineHtml(s) => holds_markup(&s),
        _ => false,
    })
}

/// The HTML elements (WHATWG's list, obsolete ones a page still renders
/// included) and the two foreign roots a body may embed.
const HTML_ELEMENTS: &[&str] = &[
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
        if name.contains('-') || HTML_ELEMENTS.contains(&name.as_str()) {
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
/// text (MFE-7) — the one difference between the two modes.
fn render(src: &str, resolve: Option<&dyn Fn(&str) -> Option<WaveRef>>, framed: bool) -> Rendered {
    let opts = options();

    // Depth counters for dropped tags, so each End pairs with its Start.
    let mut dropped_links = 0u32;
    let mut dropped_images = 0u32;
    // Inside a ```mermaid fence: the source collected so far. The fence's
    // own Start/Text/End events are swallowed; the End emits the placeholder.
    let mut mermaid_src: Option<String> = None;
    let mut diagrams = 0usize;
    let events = Parser::new_ext(src, opts).filter_map(|ev| {
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
        match ev {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref info)))
                if is_mermaid_fence(info) =>
            {
                mermaid_src = Some(String::new());
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
        None => html::push_html(&mut html, events),
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
            html::push_html(&mut html, out.into_iter());
        }
    }
    Rendered { html, diagrams }
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
            "<h1>Title</h1>\n<details>\n<summary>more</summary>\n<p>hidden <em>markdown</em></p>\n</details>\n<style>p { color: red }</style>\n<script>document.body.append('ran')</script>\n<p>A <b>bold</b> word and <m-x a=\"1\"></m-x>.</p>\n"
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
        assert!(!r.html.contains("javascript:") && r.html.contains("bad <a href=\"https://e.com\">good</a>"), "{}", r.html);
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

    #[test]
    fn the_ids_a_body_names_are_each_once_in_order() {
        assert_eq!(
            wave_ids("{wave: RETURNS-6} and {wave:context-4}, {wave: RETURNS-6} again; {wave: no}"),
            ["RETURNS-6", "CONTEXT-4"]
        );
        assert!(wave_ids("nothing here").is_empty());
    }
}
