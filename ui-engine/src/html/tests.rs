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
