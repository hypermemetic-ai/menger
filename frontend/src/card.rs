//! COMP-9: a component's **docs card** — what a definition renders as
//! (COMP-1 decision 5), and the head of its help page.
//!
//! In the owner's body a ` ```component m-x ` block is not a code block: a
//! body holding a definition frames (md.rs, [`crate::md::is_framed`]), and
//! the block renders, inside the frame, as the card — built here from the
//! block itself, so an editor's preview shows the draft's docs as they are
//! typed:
//!
//! - its head: `<m-x>` and the summary (or "no docs yet"), and — in a body —
//!   "N references ↗", a link to `#/components/m-x` whose count the frame's
//!   SDK fills from the registry and keeps live (`data-menger-refs`);
//! - a **live preview**: an `<m-x>` with the opening line's preview
//!   attributes (`board=RETURNS` …), defined by the SDK like any instance;
//! - the attributes (name, type, required, description) and the slots;
//! - each example rendered live, its markup beside it;
//! - the help prose, markdown (md.rs, framed: its HTML passes through);
//! - in a body, the source, folded.
//!
//! The card is addressable — `data-menger-card="m-x"`: the route
//! `#/ticket/<OWNER>?component=m-x` scrolls the frame to it and flashes it
//! (src/frame.rs, the SDK). The help page (`#/components/m-x`, COMP-9)
//! frames the same card, from the registry's docs, without the references
//! link and the source, which the page shows below it.
//!
//! Everything but the live parts (the preview element, built from escaped
//! attributes; the examples; the help's own HTML) is escaped text. The card
//! is only ever rendered inside the sandboxed frame. Its styles are the
//! SDK's (`.menger-card`, at zero specificity).

use menger_tickets_api::component_docs::is_component_name;
use menger_tickets_api::{AttrDoc, ComponentDocs, PartDoc, SlotDoc};
use pulldown_cmark_escape::escape_html;

/// One card's inputs.
pub(crate) struct Card<'a> {
    /// The tag, as the block names it.
    pub(crate) name: &'a str,
    /// The opening line's preview attributes.
    pub(crate) preview: &'a [(String, String)],
    /// The docs — `Ok(None)`: the source has no `<docs>`; `Err`: a malformed
    /// one (a draft's — a saved body cannot hold one), the problem.
    pub(crate) docs: Result<Option<&'a ComponentDocs>, &'a str>,
    /// The block's content.
    pub(crate) source: &'a str,
    /// In the owner's body (the references link and the folded source), or
    /// at the head of the help page (neither: the page shows both).
    pub(crate) in_body: bool,
}

/// One row of the attributes table: name, type, "required" or "", description.
pub(crate) fn attr_rows(attrs: &[AttrDoc]) -> Vec<[String; 4]> {
    attrs
        .iter()
        .map(|a| {
            [
                a.name.clone(),
                a.kind.to_string(),
                if a.required { "required".to_string() } else { String::new() },
                a.description.clone(),
            ]
        })
        .collect()
}

/// One row of the slots table: the slot's name ("default" for the
/// unnamed one), its description.
pub(crate) fn slot_rows(slots: &[SlotDoc]) -> Vec<[String; 2]> {
    slots
        .iter()
        .map(|s| [s.name.clone().unwrap_or_else(|| "default".to_string()), s.description.clone()])
        .collect()
}

/// THEME-5: one row of the parts table — the selector a theme writes for
/// it (`m-name::part(label)`), its description.
pub(crate) fn part_rows(name: &str, parts: &[PartDoc]) -> Vec<[String; 2]> {
    parts.iter().map(|p| [format!("{name}::part({})", p.name), p.description.clone()]).collect()
}

fn esc(out: &mut String, s: &str) {
    escape_html(&mut *out, s).expect("escaping into a String cannot fail");
}

/// An attribute name a preview element may carry: what an HTML parser
/// would read as one name, nothing that could end the tag.
fn attr_name_ok(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

/// `<m-x a="v" …></m-x>` — the live preview: the name is a component name,
/// each attribute's name checked, its value escaped.
pub(crate) fn preview_element(name: &str, preview: &[(String, String)]) -> String {
    let mut out = String::new();
    out.push('<');
    out.push_str(name);
    for (k, v) in preview.iter().filter(|(k, _)| attr_name_ok(k)) {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        esc(&mut out, v);
        out.push('"');
    }
    out.push_str("></");
    out.push_str(name);
    out.push('>');
    out
}

/// "source · 18 lines" — as `#/components` folds a source.
fn source_label(source: &str) -> String {
    crate::components::source_label(source)
}

/// The card's markup; `help` renders the help's markdown (md.rs, framed).
pub(crate) fn card_html(card: &Card, help: &dyn Fn(&str) -> String) -> String {
    let Card { name, preview, docs, source, in_body } = *card;
    let named = is_component_name(name);
    let mut out = String::with_capacity(source.len() * 2 + 1024);
    out.push_str("<section class=\"menger-card\"");
    if named {
        out.push_str(" data-menger-card=\"");
        out.push_str(name);
        out.push('"');
    }
    out.push_str(">\n<header class=\"mc-head\"><code class=\"mc-tag\">&lt;");
    esc(&mut out, name);
    out.push_str("&gt;</code>");
    match docs {
        Ok(Some(d)) if !d.summary.is_empty() => {
            out.push_str("<span class=\"mc-summary\">");
            esc(&mut out, &d.summary);
            out.push_str("</span>");
        }
        Ok(Some(_)) => out.push_str("<span class=\"mc-summary mc-none\">no summary</span>"),
        Ok(None) => out.push_str("<span class=\"mc-summary mc-none\">no docs yet</span>"),
        Err(_) => {}
    }
    if in_body && named {
        out.push_str("<a class=\"mc-refs\" data-menger-refs=\"");
        out.push_str(name);
        out.push_str("\" href=\"#/components/");
        out.push_str(name);
        out.push_str("\" title=\"what uses it — its help page\">references ↗</a>");
    }
    out.push_str("</header>\n");
    if !named {
        out.push_str("<p class=\"mc-error\">");
        if name.is_empty() {
            out.push_str("a <code>```component</code> block names its tag — <code>```component m-name</code>");
        } else {
            out.push('\'');
            esc(&mut out, name);
            out.push_str("' is not a component name: lowercase, starting with a letter, holding a hyphen — e.g. m-chart");
        }
        out.push_str("</p>\n");
    }
    if let Err(problem) = docs {
        out.push_str("<p class=\"mc-error\">malformed &lt;docs&gt;: ");
        esc(&mut out, problem);
        out.push_str("</p>\n");
    }
    if named {
        out.push_str("<div class=\"mc-preview\"><div class=\"mc-label\">preview</div><div class=\"mc-live\">");
        out.push_str(&preview_element(name, preview));
        out.push_str("</div></div>\n");
    }
    if let Ok(Some(d)) = docs {
        let attrs = attr_rows(&d.attrs);
        if !attrs.is_empty() {
            out.push_str("<div class=\"mc-label\">attributes</div>\n<table class=\"mc-attrs\"><thead><tr><th>attribute</th><th>type</th><th></th><th>description</th></tr></thead><tbody>\n");
            for [n, kind, required, description] in &attrs {
                out.push_str("<tr><td><code>");
                esc(&mut out, n);
                out.push_str("</code></td><td><code>");
                esc(&mut out, kind);
                out.push_str("</code></td><td class=\"mc-req\">");
                esc(&mut out, required);
                out.push_str("</td><td>");
                esc(&mut out, description);
                out.push_str("</td></tr>\n");
            }
            out.push_str("</tbody></table>\n");
        }
        let slots = slot_rows(&d.slots);
        if !slots.is_empty() {
            out.push_str("<div class=\"mc-label\">slots</div>\n<table class=\"mc-slots\"><thead><tr><th>slot</th><th>description</th></tr></thead><tbody>\n");
            for [n, description] in &slots {
                out.push_str("<tr><td><code>");
                esc(&mut out, n);
                out.push_str("</code></td><td>");
                esc(&mut out, description);
                out.push_str("</td></tr>\n");
            }
            out.push_str("</tbody></table>\n");
        }
        let parts = part_rows(name, &d.parts);
        if !parts.is_empty() {
            out.push_str("<div class=\"mc-label\">parts a theme may skin</div>\n<table class=\"mc-slots mc-parts\"><thead><tr><th>part</th><th>description</th></tr></thead><tbody>\n");
            for [n, description] in &parts {
                out.push_str("<tr><td><code>");
                esc(&mut out, n);
                out.push_str("</code></td><td>");
                esc(&mut out, description);
                out.push_str("</td></tr>\n");
            }
            out.push_str("</tbody></table>\n");
        }
        if !d.examples.is_empty() {
            out.push_str("<div class=\"mc-label\">examples</div>\n");
            for example in &d.examples {
                // Live: the example's markup as written, inside the frame.
                out.push_str("<figure class=\"mc-example\"><div class=\"mc-live\">");
                out.push_str(example);
                out.push_str("</div><pre><code>");
                esc(&mut out, example);
                out.push_str("</code></pre></figure>\n");
            }
        }
        if !d.help.trim().is_empty() {
            out.push_str("<div class=\"mc-help\">");
            out.push_str(&help(&d.help));
            out.push_str("</div>\n");
        }
    }
    if in_body {
        out.push_str("<details class=\"mc-source\"><summary>");
        esc(&mut out, &source_label(source));
        // MENGER-22: a definition's source is HTML, highlighted as such.
        out.push_str("</summary><pre><code class=\"language-html\">");
        esc(&mut out, source);
        out.push_str("</code></pre></details>\n");
    }
    out.push_str("</section>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::AttrKind;

    fn fixture() -> ComponentDocs {
        ComponentDocs {
            summary: "How many of a board's tickets are workable <now>.".into(),
            attrs: vec![
                AttrDoc { name: "board".into(), kind: AttrKind::Shortcode, required: true, description: "The shortcode to count.".into() },
                AttrDoc { name: "label".into(), kind: AttrKind::String, required: false, description: "Shown before the count.".into() },
                AttrDoc {
                    name: "size".into(),
                    kind: AttrKind::Enum { values: vec!["s".into(), "m".into()] },
                    required: false,
                    description: String::new(),
                },
            ],
            slots: vec![
                SlotDoc { name: Some("title".into()), description: "Optional title markup.".into() },
                SlotDoc { name: None, description: "The rest.".into() },
            ],
            parts: vec![PartDoc { name: "count".into(), scripted: false, description: "The number.".into() }],
            examples: vec!["<m-wave-summary board=\"RETURNS\"></m-wave-summary>".into()],
            help: "Longer *help*.".into(),
        }
    }

    fn help(md: &str) -> String {
        format!("<p>[{md}]</p>")
    }

    /// COMP-9 acceptance 1: the card's rows from a docs fixture.
    #[test]
    fn the_cards_rows_from_a_docs_fixture() {
        let d = fixture();
        assert_eq!(
            attr_rows(&d.attrs),
            vec![
                ["board".to_string(), "shortcode".into(), "required".into(), "The shortcode to count.".into()],
                ["label".to_string(), "string".into(), String::new(), "Shown before the count.".into()],
                ["size".to_string(), "enum(s|m)".into(), String::new(), String::new()],
            ]
        );
        assert_eq!(
            slot_rows(&d.slots),
            vec![["title".to_string(), "Optional title markup.".into()], ["default".to_string(), "The rest.".into()]]
        );
    }

    /// The card in a body: head, live preview with the preview attributes,
    /// the tables, each example live beside its escaped markup, the help,
    /// the references link and the source folded — in that order.
    #[test]
    fn a_card_in_the_owners_body() {
        let d = fixture();
        let preview = vec![("board".to_string(), "RETURNS".to_string()), ("label".to_string(), "A \"b\" <c>".to_string())];
        let source = "<template>x</template>\n<docs>…</docs>\n";
        let html = card_html(
            &Card { name: "m-wave-summary", preview: &preview, docs: Ok(Some(&d)), source, in_body: true },
            &help,
        );
        let at = |needle: &str| html.find(needle).unwrap_or_else(|| panic!("missing {needle} in\n{html}"));
        assert!(html.starts_with("<section class=\"menger-card\" data-menger-card=\"m-wave-summary\">"), "{html}");
        assert!(html.contains("<code class=\"mc-tag\">&lt;m-wave-summary&gt;</code><span class=\"mc-summary\">How many of a board&#39;s tickets are workable &lt;now&gt;.</span>"), "{html}");
        assert!(html.contains("<a class=\"mc-refs\" data-menger-refs=\"m-wave-summary\" href=\"#/components/m-wave-summary\""));
        assert!(html.contains("<div class=\"mc-live\"><m-wave-summary board=\"RETURNS\" label=\"A &quot;b&quot; &lt;c&gt;\"></m-wave-summary></div>"), "{html}");
        assert!(html.contains("<tr><td><code>board</code></td><td><code>shortcode</code></td><td class=\"mc-req\">required</td><td>The shortcode to count.</td></tr>"));
        assert!(html.contains("<tr><td><code>default</code></td><td>The rest.</td></tr>"));
        assert!(html.contains("<table class=\"mc-slots mc-parts\"><thead><tr><th>part</th><th>description</th></tr></thead><tbody>\n<tr><td><code>m-wave-summary::part(count)</code></td><td>The number.</td></tr>"), "{html}");
        assert!(at("mc-slots\"") < at("mc-parts") && at("mc-parts") < at("mc-example"));
        assert!(html.contains("<figure class=\"mc-example\"><div class=\"mc-live\"><m-wave-summary board=\"RETURNS\"></m-wave-summary></div><pre><code>&lt;m-wave-summary board=&quot;RETURNS&quot;&gt;&lt;/m-wave-summary&gt;</code></pre></figure>"), "{html}");
        assert!(html.contains("<div class=\"mc-help\"><p>[Longer *help*.]</p></div>"));
        assert!(html.contains("<details class=\"mc-source\"><summary>source · 2 lines</summary><pre><code class=\"language-html\">&lt;template&gt;x&lt;/template&gt;"));
        assert!(at("mc-head") < at("mc-preview") && at("mc-preview") < at("mc-attrs") && at("mc-attrs") < at("mc-slots"));
        assert!(at("mc-slots") < at("mc-example") && at("mc-example") < at("mc-help") && at("mc-help") < at("mc-source"));
        assert!(html.ends_with("</section>\n"));
    }

    /// The help page's card: no references link, no source (the page shows
    /// both); a component without docs says so; a draft's malformed docs
    /// and a bad name are said, never thrown.
    #[test]
    fn the_help_pages_card_no_docs_and_problems() {
        let d = fixture();
        let html = card_html(&Card { name: "m-x", preview: &[], docs: Ok(Some(&d)), source: "s", in_body: false }, &help);
        assert!(!html.contains("mc-refs") && !html.contains("mc-source"), "{html}");
        assert!(html.contains("<div class=\"mc-live\"><m-x></m-x></div>"), "a preview with no attributes");
        let none = card_html(&Card { name: "m-x", preview: &[], docs: Ok(None), source: "s", in_body: true }, &help);
        assert!(none.contains("<span class=\"mc-summary mc-none\">no docs yet</span>") && !none.contains("mc-attrs"), "{none}");
        assert!(none.contains("mc-preview") && none.contains("mc-source"));
        let bad = card_html(&Card { name: "m-x", preview: &[], docs: Err("two <summary> elements"), source: "s", in_body: true }, &help);
        assert!(bad.contains("<p class=\"mc-error\">malformed &lt;docs&gt;: two &lt;summary&gt; elements</p>"), "{bad}");
        let unnamed = card_html(&Card { name: "Chart", preview: &[], docs: Ok(None), source: "s", in_body: true }, &help);
        assert!(unnamed.contains("&#39;Chart&#39; is not a component name") || unnamed.contains("'Chart' is not a component name"), "{unnamed}");
        assert!(!unnamed.contains("data-menger-card") && !unnamed.contains("<Chart"), "no live element for a bad name");
    }

    #[test]
    fn a_preview_element_never_ends_its_tag_early() {
        let preview = vec![
            ("ok".to_string(), "\"><script>alert(1)</script>".to_string()),
            ("bad\"name".to_string(), "x".to_string()),
            ("x><img".to_string(), "y".to_string()),
        ];
        assert_eq!(preview_element("m-x", &preview), "<m-x ok=\"&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;\"></m-x>");
    }
}
