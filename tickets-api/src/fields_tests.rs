//! ART-5: `fields` — an HTML body, a mixed body and a legacy body; code
//! never holds a field; parts nest; the copies of the app's reading are the
//! app's.

use super::*;

/// The app's renderer, as text. The app is its own cargo unit; this reads
/// it, it does not build it.
const MD_RS: &str = include_str!("../../frontend/src/md.rs");
const THIS_RS: &str = include_str!("fields.rs");

/// The fields as a compact tree: `a b(c d(e))`.
fn tree(fs: &[Field]) -> String {
    fs.iter()
        .map(|f| match f.parts.is_empty() {
            true => f.name.clone(),
            false => format!("{}({})", f.name, tree(&f.parts)),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The 1-based line `needle` first appears on.
fn line(body: &str, needle: &str) -> u32 {
    let at = body.find(needle).unwrap_or_else(|| panic!("{needle:?} is in the body"));
    body[..at].matches('\n').count() as u32 + 1
}

fn element(tag: &str) -> Form {
    Form::Element { tag: tag.into() }
}

fn heading(text: &str) -> Form {
    Form::Heading { text: text.into() }
}

// -- an HTML body ----------------------------------------------------------------

/// An artifact written to ought's execution playbook: plain semantic HTML,
/// prose between fields, a decision with its parts.
const HTML_BODY: &str = "\
<p data-field=\"standing\"><em>Execution ticket — holds the DAG.</em></p>

<figure data-field=\"dag\">
  <pre class=\"mermaid\">
graph TD
  B0[\"B0 · type root\"] --> B1[\"B1 · adoption\"]
  </pre>
  <figcaption>Blue: builds.</figcaption>
</figure>

<p>A connecting sentence is prose, not a field.</p>

<section data-field=\"work\">
  <h2>The work</h2>
  <table>
    <thead><tr><th>ticket</th><th>scope</th></tr></thead>
    <tbody><tr><td>B0</td><td>the type root</td></tr></tbody>
  </table>
</section>

<section data-field=\"decision-gates\">
  <h2>Decision gates</h2>
  <section data-field=\"decision\">
    <p data-field=\"question\">G2 · archive or delete?</p>
    <ul>
      <li data-field=\"option\">archive</li>
      <li data-field=\"option\">delete</li>
    </ul>
    <p data-field=\"answer\"><strong>DECIDED:</strong> archive.</p>
  </section>
</section>

<ol data-field=\"status\">
  <li>2026-05-09: B0 done.</li>
</ol>
";

#[test]
fn an_html_body_yields_its_outermost_marked_elements() {
    let fs = fields(HTML_BODY);
    assert_eq!(
        tree(&fs),
        "standing dag work decision-gates(decision(question option option answer)) status"
    );
    let forms: Vec<&Form> = fs.iter().map(|f| &f.form).collect();
    assert_eq!(
        forms,
        [&element("p"), &element("figure"), &element("section"), &element("section"), &element("ol")]
    );
    let lines: Vec<u32> = fs.iter().map(|f| f.line).collect();
    assert_eq!(
        lines,
        [
            1,
            line(HTML_BODY, "<figure"),
            line(HTML_BODY, "<section data-field=\"work\""),
            line(HTML_BODY, "<section data-field=\"decision-gates\""),
            line(HTML_BODY, "<ol"),
        ]
    );
    assert_eq!(fs[0].content, "<em>Execution ticket — holds the DAG.</em>");
    assert!(fs[1].content.contains("graph TD") && fs[1].content.contains("<figcaption>"), "{:?}", fs[1]);
    assert!(fs[1].content.ends_with("</figcaption>\n"), "{:?}", fs[1].content);
    assert!(fs[2].content.contains("<td>the type root</td>") && !fs[2].content.contains("decision"), "{:?}", fs[2]);
    let decision = &fs[3].parts[0];
    assert_eq!(decision.form, element("section"));
    assert_eq!(decision.line, line(HTML_BODY, "<section data-field=\"decision\""));
    assert_eq!(decision.parts[0].content, "G2 · archive or delete?");
    assert_eq!(decision.parts[1].content, "archive");
    assert_eq!(decision.parts[1].form, element("li"));
    assert_eq!(decision.parts[3].content, "<strong>DECIDED:</strong> archive.");
    // A part is in its field's content too: the field holds the whole.
    assert!(fs[3].content.contains("<p data-field=\"question\">"));
}

// -- a mixed body ------------------------------------------------------------------

/// Markdown with field elements wrapping its prose (menger passes raw HTML
/// through, COMP-4): a frontmatter, an italic line and headings that are
/// only labels, prose between fields, HTML in code that is not HTML.
const MIXED_BODY: &str = "\
---
status: Pending
note: <section data-field=\"in-the-frontmatter\">
---
*confidence: medium — a mixed body.*

Prose before any field, with `<span data-field=\"in-a-code-span\">` in a code span.

<section data-field=\"problem\">

## Problem

The gap, in **markdown**, with a list:

- one
- two

</section>

A connecting sentence between fields.

```html
<section data-field=\"in-a-fence\"></section>
```

    <section data-field=\"in-indented-code\"></section>

<!-- <section data-field=\"in-a-comment\"></section> -->

<m-provides data-field=\"provides\">

| Output | Shape | Consumed by |
| -- | -- | -- |
| `fields` | `Vec<Field>` | COMPILE-1 |

</m-provides>

<section data-field=\"intended-usage\">

How COMPILE calls it.

<section data-field=\"divergence-notes\">
</section>

</section>

Text with an inline <span data-field=\"acceptance\">field</span> in it.

<script>const s = '<section data-field=\"in-a-script\">';</script>
";

#[test]
fn a_mixed_body_yields_its_field_elements_and_its_headings_are_labels() {
    let fs = fields(MIXED_BODY);
    assert_eq!(tree(&fs), "problem provides intended-usage(divergence-notes) acceptance");
    let tags: Vec<&Form> = fs.iter().map(|f| &f.form).collect();
    assert_eq!(tags, [&element("section"), &element("m-provides"), &element("section"), &element("span")]);
    // Lines count the frontmatter.
    assert_eq!(fs[0].line, line(MIXED_BODY, "<section data-field=\"problem\""));
    assert_eq!(fs[0].line, 9);
    assert_eq!(fs[1].line, line(MIXED_BODY, "<m-provides"));
    assert_eq!(fs[3].line, line(MIXED_BODY, "Text with an inline"));
    // The markdown inside a field is its content, as written.
    assert!(fs[0].content.contains("## Problem") && fs[0].content.contains("- two"), "{:?}", fs[0].content);
    assert!(!fs[0].content.contains("connecting sentence"), "{:?}", fs[0].content);
    assert!(fs[1].content.contains("| `fields` | `Vec<Field>` | COMPILE-1 |"), "{:?}", fs[1].content);
    assert_eq!(fs[3].content, "field");
    assert_eq!(fs[2].parts[0].line, line(MIXED_BODY, "<section data-field=\"divergence-notes\""));
}

// -- a legacy body -------------------------------------------------------------------

/// A build ticket in the markdown form ought's skill wrote before ART-3.
const LEGACY_BUILD: &str = "\
*confidence: high — measured, COMPILE-3.*

```mermaid
graph LR
  A --> B
```

## Problem

The gap.

### A sub-heading is content

More of the problem.

## Context

The terrain.

```text
## Status
a heading in a fence is text
```

## Intended Usage

How it is called.

### Divergence notes
*(append-only — dated entries)*

- 2026-10-07: the first.

### Another sub-heading

Still intended usage.

## Notes the field set does not name

Prose between fields.

> ## Acceptance criteria
> quoted, not the body's own

## What must NOT change

Nothing else.

## Acceptance criteria

1. One.

# A title heading ends a section

## Status

2026-10-07, builder: **Coding.**
";

#[test]
fn a_legacy_body_reads_its_headings_through_the_field_set() {
    let fs = fields(LEGACY_BUILD);
    assert_eq!(
        tree(&fs),
        "confidence problem context intended-usage(divergence-notes) must-not-change acceptance status"
    );
    assert_eq!(fs[0].form, Form::Lead);
    assert_eq!((fs[0].line, fs[0].content.as_str()), (1, "*confidence: high — measured, COMPILE-3.*"));
    assert_eq!(fs[1].form, heading("Problem"));
    assert_eq!(fs[1].line, line(LEGACY_BUILD, "## Problem"));
    // A section runs to the next heading of its level or higher; a deeper
    // one is its content.
    assert!(fs[1].content.starts_with("\nThe gap."), "{:?}", fs[1].content);
    assert!(fs[1].content.contains("### A sub-heading is content\n\nMore of the problem.\n"));
    assert!(fs[1].content.ends_with("More of the problem.\n\n"), "{:?}", fs[1].content);
    assert!(fs[2].content.contains("## Status\na heading in a fence is text"), "{:?}", fs[2].content);
    let usage = &fs[3];
    assert_eq!(usage.form, heading("Intended Usage"));
    assert!(usage.content.contains("Still intended usage.") && !usage.content.contains("Prose between"));
    let notes = &usage.parts[0];
    assert_eq!(notes.form, heading("Divergence notes"));
    assert_eq!(notes.line, line(LEGACY_BUILD, "### Divergence notes"));
    assert_eq!(notes.content, "*(append-only — dated entries)*\n\n- 2026-10-07: the first.\n\n");
    assert_eq!(fs[4].content, "\nNothing else.\n\n");
    assert_eq!(fs[5].content, "\n1. One.\n\n", "an `#` heading ends a `##` section");
    assert_eq!(fs[6].content, "\n2026-10-07, builder: **Coding.**\n");
}

/// A scope's and an execution's and a spike's headings, as menger's own
/// legacy tickets write them — qualifiers after the "was" text included.
#[test]
fn every_kinds_legacy_headings_are_read() {
    let scope = "\
*Scope — grilled with the owner 2026-10-06.*

## End state

A.

## Language

## Decided shape

## Proposed shape — `[confirm]`, not decided

## The core transition — what exists today

## Decisions & open questions

## Interface contract (what this milestone hands the next)

## Dependencies

## Execution

→ ART-2.
";
    let fs = fields(scope);
    assert_eq!(
        tree(&fs),
        "standing end-state language decided-shape core-transition open-questions interface-contract dependencies execution"
    );
    assert_eq!(fs[0].form, Form::Lead);
    assert_eq!(fs[4].form, heading("The core transition — what exists today"));
    let execution = "## Goal\n\n## Execution DAG\n\n## The work\n\n## Decision gates\n\n## Exit\n\n## Completion gate\n\n## Status\n";
    assert_eq!(tree(&fields(execution)), "goal dag work decision-gates exit completion-gate status");
    let spike = "## Question\n\n## Lens\n\n## Setup\n\n## Pass condition\n\n## Evidence to record (regardless of outcome)\n\n## Fail → consequence\n";
    assert_eq!(tree(&fields(spike)), "question lens setup pass-condition evidence fail-consequence");
    let build = "## Evidence\n\n## Required behavior\n\n## Provides\n\n## Consumes\n\n## Risks\n\n## Completion\n";
    assert_eq!(tree(&fields(build)), "evidence required-behavior provides consumes risks completion");
}

#[test]
fn the_lead_line_is_an_italic_paragraph_and_only_the_first_block() {
    assert_eq!(tree(&fields("*idea-grade — a sketch.*\n\n## Problem\n")), "standing problem");
    assert_eq!(fields("_Confidence: low — spike first._\n")[0].name, "confidence");
    for not_lead in [
        "*italic* and then prose\n",
        "**bold, not italic**\n",
        "Plain prose.\n\n*an italic line, but not the first block*\n",
        "## Problem\n\n*confidence: high*\n",
    ] {
        assert!(fields(not_lead).iter().all(|f| f.form != Form::Lead), "{not_lead:?}");
    }
    // After a frontmatter, the first block is still the body's first.
    let fs = fields("---\nid: X-1\n---\n*confidence: low*\n");
    assert_eq!((fs[0].name.as_str(), fs[0].line), ("confidence", 4));
}

#[test]
fn legacy_names_follow_the_was_column() {
    for (heading, name) in [
        ("Problem", Some("problem")),
        ("problem", Some("problem")),
        ("Problem:", Some("problem")),
        ("  Acceptance   criteria ", Some("acceptance")),
        ("What must not change", Some("must-not-change")),
        ("Status — 2026-10-07", Some("status")),
        ("Execution DAG", Some("dag")),
        ("Execution", Some("execution")),
        ("Execution — see ART-2", Some("execution")),
        ("Evidence to record (regardless of outcome)", Some("evidence")),
        ("Interface contract (what this milestone hands the next)", Some("interface-contract")),
        ("The core transition", Some("core-transition")),
        ("Problems", None),
        ("Exit criteria", None),
        ("Proposed shape — `[confirm]`, not decided", None),
        ("Acceptance", None),
        ("", None),
    ] {
        assert_eq!(legacy_name(heading), name, "{heading:?}");
    }
    // Every name in the table is kebab-case, as ought's field set writes it.
    for (_, name) in LEGACY {
        assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{name}");
    }
}

// -- code never yields fields -------------------------------------------------------

/// `data-field` written in a code span, a fence, indented code, a comment, a
/// script, an attribute's value or the frontmatter is not an element — so
/// such a body has none, and is read as legacy.
#[test]
fn code_spans_and_fences_never_yield_fields() {
    let body = "\
---
note: <p data-field=\"frontmatter\">x</p>
---
Write `<section data-field=\"problem\">` to mark a field.

```html
<section data-field=\"problem\">in a fence</section>
```

~~~
<p data-field=\"acceptance\">in a tilde fence</p>
~~~

    <p data-field=\"indented\">indented code</p>

- a list item holding a fence:

  ```
  <p data-field=\"nested-fence\">x</p>
  ```

<!-- <p data-field=\"commented\">x</p> -->

<style>p[data-field=\"styled\"] { color: red }</style>

<a title='data-field=\"in-a-value\"'>a link</a>

## Problem

The legacy reading applies.
";
    let fs = fields(body);
    assert_eq!(tree(&fs), "problem", "{fs:?}");
    assert_eq!(fs[0].form, heading("Problem"));
    assert!(fields(MIXED_BODY).iter().all(|f| !f.name.starts_with("in-")));
}

/// A comment's or a script's text spans the lines of an HTML block — and a
/// comment left open spans past it — and still holds no field.
#[test]
fn comments_and_raw_text_across_lines_hold_no_fields() {
    let body = "\
<!--
<section data-field=\"commented\">
-->
<script>
  document.body.innerHTML = '<p data-field=\"scripted\"></p>';
</script>
<section data-field=\"real\">x</section>

<!-- an open comment

<p data-field=\"still-commented\">y</p>
";
    assert_eq!(tree(&fields(body)), "real");
}

/// COMP-11: an element runs to its close across blank lines, as the frame
/// renders it — a field written after one, indented, is a field (CommonMark
/// alone reads it as code), on the line it is written on.
#[test]
fn a_blank_line_inside_an_element_does_not_end_it() {
    let body = "\
<section class=\"page\">
  <h2>Decide</h2>
  <p>The choice.</p>

    <section data-field=\"decision\">
      <p data-field=\"question\">Archive or delete?</p>

      <p data-field=\"recommendation\">Archive.</p>
    </section>
</section>
";
    let fs = fields(body);
    assert_eq!(tree(&fs), "decision(question recommendation)", "{fs:?}");
    assert_eq!(fs[0].line, line(body, "<section data-field"));
    assert_eq!(fs[0].parts[1].line, line(body, "<p data-field=\"recommendation"));
    assert!(fs[0].content.contains("Archive or delete?</p>\n\n      <p"), "as written: {:?}", fs[0].content);
}

// -- parts nest ---------------------------------------------------------------------

#[test]
fn parts_nest_under_their_field_at_any_depth() {
    let body = "\
<section data-field=\"open-questions\">
  <section data-field=\"decision\">
    <p data-field=\"question\">Archive or delete?</p>
    <div class=\"cards\">
      <article data-field=\"option\"><h3>Archive</h3><p>Cost: storage.</p></article>
      <article data-field=\"option\"><h3>Delete</h3></article>
    </div>
    <p data-field=\"recommendation\">Archive.</p>
  </section>
  <section data-field=\"decision\">
    <p data-field=\"question\">Hourly or daily?</p>
  </section>
</section>

<section data-field=\"intended-usage\">
<p>How it is called.</p>
<section data-field=\"divergence-notes\"><ol><li>2026-10-07: none yet.</li></ol></section>
</section>

<section data-field=\"acceptance\">
<ol>
<li>One criterion, with a sheet:
<table data-field=\"metric-sheet\"><tr><td>p50</td><td>7 ms</td></tr></table>
</li>
</ol>
</section>
";
    let fs = fields(body);
    assert_eq!(
        tree(&fs),
        "open-questions(decision(question option option recommendation) decision(question)) \
         intended-usage(divergence-notes) acceptance(metric-sheet)"
    );
    let options = &fs[0].parts[0].parts;
    assert_eq!(options[1].form, element("article"));
    assert_eq!(options[1].content, "<h3>Archive</h3><p>Cost: storage.</p>");
    assert_eq!(fs[0].parts[1].line, line(body, "<section data-field=\"decision\">\n    <p data-field=\"question\">Hourly"));
    assert_eq!(fs[2].parts[0].form, element("table"));
}

// -- an element's extent ---------------------------------------------------------------

/// What closes an element a browser closes implicitly closes it here: a
/// block after an unclosed `<p>`, the next `<li>`, the close of an element
/// it sits in, the end of the markdown block it opened in.
#[test]
fn implied_closes_end_a_field() {
    let siblings = "<section data-field=\"decision\">\n<p data-field=\"question\">Archive?\n<p data-field=\"answer\">DECIDED: archive.\n</section>\n";
    let fs = fields(siblings);
    assert_eq!(tree(&fs), "decision(question answer)");
    assert_eq!(fs[0].parts[0].content, "Archive?\n");
    assert_eq!(fs[0].parts[1].content, "DECIDED: archive.\n", "closed by </section>");

    let items = "<ul data-field=\"work\">\n<li data-field=\"b1\">one\n<li data-field=\"b2\">two\n</ul>\n";
    let fs = fields(items);
    assert_eq!(tree(&fs), "work(b1 b2)");
    assert_eq!(fs[0].parts[0].content, "one\n");

    let block_after_p = "<p data-field=\"problem\">The gap.\n<div data-field=\"evidence\">Measured.</div>\n";
    assert_eq!(tree(&fields(block_after_p)), "problem evidence");

    // Inline, in a paragraph: the paragraph's end closes it.
    let fs = fields("Prose <span data-field=\"standing\">idea-grade\nstill the paragraph\n\nNext paragraph.\n");
    assert_eq!(fs[0].content, "idea-grade\nstill the paragraph\n");

    // A close tag that is not a block's cannot reach past the markdown
    // paragraph it is written in (it is ignored, as a browser ignores it):
    // the component's own close, after it, ends the field.
    let body = "<m-dag data-field=\"dag\">\n\ntext </m-dag> more\n\n</m-dag>\n\n<section data-field=\"work\">x</section>\n";
    let fs = fields(body);
    assert_eq!(tree(&fs), "dag work");
    assert!(fs[0].content.contains("more"), "{:?}", fs[0].content);
    // …while a block's close tag does (the paragraph closes with it).
    let fs = fields("<section data-field=\"problem\">\n\nThe gap. </section>\n\n<section data-field=\"context\">x</section>\n");
    assert_eq!(tree(&fs), "problem context");
    assert_eq!(fs[0].content, "\n\nThe gap. ");
}

#[test]
fn a_self_closing_or_void_field_is_empty() {
    let body = "<m-provides data-field=\"provides\" rows=\"2\"/>\n<section data-field=\"consumes\">none — this is the root</section>\n<hr data-field=\"rule\">\n";
    let fs = fields(body);
    assert_eq!(tree(&fs), "provides consumes rule");
    assert_eq!(fs[0].content, "");
    assert_eq!(fs[0].form, element("m-provides"));
    assert_eq!(fs[1].content, "none — this is the root");
    assert_eq!(fs[2].content, "");
}

#[test]
fn an_unclosed_field_runs_to_the_end_and_names_are_as_written() {
    let fs = fields("<section data-field=\"problem\">\nnever closed\n");
    assert_eq!(fs[0].content, "\nnever closed\n");
    // Attribute names fold case; values are as written, entities decoded;
    // a bare attribute is a field with no name — a checker's to report.
    let fs = fields("<SECTION DATA-FIELD='Mixed&amp;Case'>a</SECTION>\n<p data-field>b</p>\n<div data-field=unquoted>c</div>\n");
    assert_eq!(tree(&fs), "Mixed&Case  unquoted");
    assert_eq!(fs[0].form, element("section"));
}

#[test]
fn fields_inside_markdown_containers_and_crlf_bodies_keep_their_lines() {
    let body = "intro\r\n\r\n> <section data-field=\"quoted\">\r\n> in a quote\r\n> </section>\r\n\r\n- item\r\n\r\n  <div data-field=\"listed\">\r\n  in an item\r\n  </div>\r\n";
    let fs = fields(body);
    assert_eq!(tree(&fs), "quoted listed");
    assert_eq!((fs[0].line, fs[1].line), (3, 9));
    assert!(fs[0].content.contains("in a quote"), "{:?}", fs[0].content);
    assert!(fs[1].content.contains("in an item"), "{:?}", fs[1].content);
}

#[test]
fn a_body_with_neither_has_no_fields() {
    assert!(fields("").is_empty());
    assert!(fields("Just prose.\n\n## Unnamed heading\n").is_empty());
    assert!(fields("<div>html, but nothing marked</div>\n").is_empty());
}

// -- the app's reading, word for word ----------------------------------------------------

/// The text of `fn <name>(` through its closing `}` line in `src`.
fn item<'a>(src: &'a str, name: &str) -> &'a str {
    let start = src.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("{name} is defined"));
    let end = start + src[start..].find("\n}\n").expect("it ends") + 3;
    &src[start..end]
}

#[test]
fn the_frontmatter_split_is_the_apps_word_for_word() {
    assert_eq!(
        item(THIS_RS, "split_frontmatter"),
        item(MD_RS, "split_frontmatter"),
        "md.rs's split_frontmatter changed — copy it here"
    );
}

#[test]
fn the_options_are_the_apps() {
    let inserts = |src: &str, from: &str| -> Vec<String> {
        let at = src.find(from).expect("the options");
        src[at..]
            .lines()
            .take_while(|l| *l != "}")
            .map(str::trim)
            .filter(|l| l.starts_with("opts.insert("))
            .map(str::to_string)
            .collect()
    };
    let ours = inserts(THIS_RS, "fn options() -> Options {");
    assert_eq!(ours.len(), 4, "{ours:?}");
    assert_eq!(ours, inserts(MD_RS, "fn options() -> Options {"), "md.rs's options changed — copy them here");
}
