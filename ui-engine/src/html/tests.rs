//! UI-10's testing surface: the sanitizer and markdown.

use super::*;

fn clean(html: &str) -> String {
    to_html(&sanitize(html))
}

/// A body with `<script>`, `<style>`, `onclick`, a `javascript:` URL and an
/// `<iframe>`: none survive; `<p>`, `<a href="https://…">`, `<code>`,
/// `<ul>` do, their text kept and escaped.
#[test]
fn html_is_sanitized() {
    let body = concat!(
        r##"<p onclick="steal()">Kiln <a href="https://menger.sh/x" onmouseover="y()" target="_blank">site</a> "##,
        r##"<a href="javascript:alert(1)">bad</a> <a href="JaVaScRiPt:alert(2)">worse</a></p>"##,
        r##"<script>alert(1)</script><style>p { color: red }</style><iframe src="https://x.example"></iframe>"##,
        r##"<code>a &lt; b &amp;&amp; c</code><ul><li>one</li><li>two</li></ul>"##,
        r##"<img src="https://menger.sh/i.png" alt="i" onerror="z()"><img src="data:image/png;base64,AAAA">"##,
        r##"<a href="#top">up</a> <a href="mailto:owner@menger.sh">mail</a> <a href="/relative">rel</a>"##,
        r##"<SCRIPT>alert(3)</SCRIPT><object data="x"><p>inside</p></object>"##,
    );
    let out = clean(body);
    assert_eq!(
        out,
        concat!(
            r##"<p>Kiln <a href="https://menger.sh/x">site</a> <a>bad</a> <a>worse</a></p>"##,
            r##"<code>a &lt; b &amp;&amp; c</code><ul><li>one</li><li>two</li></ul>"##,
            r##"<img src="https://menger.sh/i.png" alt="i"><img>"##,
            r##"<a href="#top">up</a> <a href="mailto:owner@menger.sh">mail</a> <a>rel</a>"##,
        )
    );
    for gone in ["script", "style", "iframe", "object", "onclick", "onmouseover", "onerror", "javascript", "data:", "alert", "inside", "target"] {
        assert!(!out.to_ascii_lowercase().contains(gone), "`{gone}` survived: {out}");
    }
    // The tree the renderer draws: elements and text, the text decoded.
    assert_eq!(sanitize("<b>x &amp; y</b>"), vec![HNode::Element { tag: "b".into(), attrs: vec![], children: vec![HNode::Text("x & y".into())] }]);
}

/// An allow-list: a tag not listed is dropped, its text kept; an attribute
/// not listed for its tag is dropped.
#[test]
fn a_tag_not_listed_is_dropped() {
    assert_eq!(clean("<blink>x</blink><marquee><b>y</b></marquee>"), "x<b>y</b>");
    assert_eq!(clean(r##"<form action="https://x"><input name="q" value="v"></form>z"##), "z");
    assert_eq!(clean(r##"<p style="color:red" class="lead" id="p1">t</p>"##), "<p>t</p>");
    assert!(!ALLOWED_TAGS.contains(&"blink") && !ALLOWED_TAGS.contains(&"form"));
    assert!(allowed_attr("a", "href") && !allowed_attr("a", "onclick") && !allowed_attr("p", "style"));
    // Unclosed and stray tags do not unbalance what is drawn.
    assert_eq!(clean("<p><b>open</p></i>tail"), "<p><b>open</b></p>tail");
    assert_eq!(clean("a < b and <3"), "a &lt; b and &lt;3");
}

/// `markdown(text)`: headings, emphasis, a list, a fenced block, a link →
/// HTML; through the sanitizer, raw HTML and a `javascript:` link go.
#[test]
fn markdown_renders_and_is_sanitized() {
    let md = "# Kiln\n\nSome **weight** and *slant*.\n\n- one\n- two\n\n```rust\nlet a = 1 < 2;\n```\n\n[site](https://menger.sh) [bad](javascript:alert(1))\n\n<script>alert(1)</script>\n";
    let raw = markdown(md);
    for piece in ["<h1>Kiln</h1>", "<strong>weight</strong>", "<em>slant</em>", "<li>one</li>", "<pre><code class=\"language-rust\">let a = 1 &lt; 2;\n</code></pre>", "<a href=\"https://menger.sh\">site</a>"] {
        assert!(raw.contains(piece), "markdown gives `{piece}`: {raw}");
    }
    let out = clean(&raw);
    for piece in ["<h1>Kiln</h1>", "<p>Some <strong>weight</strong> and <em>slant</em>.</p>", "<ul>\n<li>one</li>\n<li>two</li>\n</ul>", "<pre><code class=\"language-rust\">let a = 1 &lt; 2;\n</code></pre>", "<a href=\"https://menger.sh\">site</a> <a>bad</a>"] {
        assert!(out.contains(piece), "sanitized, `{piece}` stays: {out}");
    }
    assert!(!out.contains("script") && !out.contains("javascript") && !out.contains("alert"), "{out}");
}

/// Entity-encoded markup is text: `&lt;script&gt;` arrives as a text node,
/// never as an element.
#[test]
fn encoded_markup_is_text() {
    let nodes = sanitize("&lt;script&gt;alert(1)&lt;/script&gt; &#60;b&#62;");
    assert_eq!(nodes, vec![HNode::Text("<script>alert(1)</script> <b>".into())], "one text node");
    assert_eq!(to_html(&nodes), "&lt;script&gt;alert(1)&lt;/script&gt; &lt;b&gt;", "escaped again when written");
}

/// Tags not on the list — a custom element, an engine component's name,
/// `base`, `meta`, `embed` — are dropped, their text kept; an `svg` is
/// dropped whole even when a tag inside starts with its name.
#[test]
fn unlisted_and_lookalike_tags_are_dropped() {
    let out = to_html(&sanitize(r##"<x-new>a</x-new><ticket-row>b</ticket-row><base href="https://x.example">c<meta charset="utf-8">d<embed src="https://x.example">e"##));
    assert_eq!(out, "abcde");
    assert_eq!(to_html(&sanitize("<svg><svgx></svgx><script>x</script></svg>ok")), "ok");
    assert_eq!(to_html(&sanitize("<svg></svgx><p>inside</p></svg>ok")), "ok", "</svgx is no </svg");
}

/// Every `(tag, attribute)` the sanitizer keeps, pinned: widening the
/// allow-list is a deliberate edit of this table. `*` is any allowed tag.
#[test]
fn the_attribute_allow_list_is_pinned() {
    let pinned: &[(&str, &str)] = &[
        ("*", "title"),
        ("a", "href"),
        ("code", "class"),
        ("img", "alt"),
        ("img", "height"),
        ("img", "src"),
        ("img", "width"),
        ("ol", "start"),
        ("pre", "class"),
        ("td", "colspan"),
        ("td", "rowspan"),
        ("th", "colspan"),
        ("th", "rowspan"),
    ];
    assert_eq!(ALLOWED_ATTRS, pinned, "the table");
    // And the function answers the table, nothing more.
    let attrs = ["href", "src", "alt", "width", "height", "class", "id", "style", "title", "start", "colspan", "rowspan", "target", "rel", "name", "onclick", "data-x"];
    for tag in ALLOWED_TAGS {
        for attr in attrs {
            let want = pinned.iter().any(|(t, a)| (*t == "*" || t == tag) && *a == attr);
            assert_eq!(allowed_attr(tag, attr), want, "({tag}, {attr})");
        }
    }
}
