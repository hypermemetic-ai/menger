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

use pulldown_cmark::{html, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use pulldown_cmark_escape::escape_html;

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
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);

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
            // Raw HTML renders as inert, visible text (push_html escapes Text).
            Event::Html(s) | Event::InlineHtml(s) => Some(Event::Text(s)),
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
    html::push_html(&mut html, events);
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
}
