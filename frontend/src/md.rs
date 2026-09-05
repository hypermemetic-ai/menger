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

use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};

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

/// Render markdown to HTML that is safe to inject via `inner_html`.
pub fn render_markdown(src: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);

    // Depth counters for dropped tags, so each End pairs with its Start.
    let mut dropped_links = 0u32;
    let mut dropped_images = 0u32;
    let events = Parser::new_ext(src, opts).filter_map(|ev| match ev {
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
    });

    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(split_frontmatter("--- not a fence\nx"), (None, "--- not a fence\nx"));
    }
}
