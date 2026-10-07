//! COMP-8: a component's **help** — the one `<docs>` element its source may
//! hold, and the preview attributes on its block's opening line — parsed
//! into the wire's [`ComponentDocs`] and `preview` (COMP-1 decision 6).
//! Wasm-safe (no server dependency): the store reads every definition with
//! it, and the app renders a definition's card (COMP-9) from the block a
//! body holds — a draft's included.
//!
//! ```html
//! <docs>
//!   <summary>How many of a board's tickets are workable now (wave 1).</summary>
//!   <attr name="board" type="shortcode" required>The shortcode to count, e.g. RETURNS.</attr>
//!   <attr name="label" type="string">Shown before the count.</attr>
//!   <slot name="title">Optional title markup.</slot>
//!   <example><m-wave-summary board="RETURNS"></m-wave-summary></example>
//!   Longer help, as markdown prose.
//! </docs>
//! ```
//!
//! **Where `<docs>` is** ([`parse`]): at the source's top level — outside
//! `<template>` (whose content is what an instance shows), `<script>` and
//! `<style>` (raw text) and HTML comments; anywhere among them. None: the
//! component has no docs (`None`). Two: malformed.
//!
//! **What is in it**: its top-level children `<summary>`, `<attr>`, `<slot>`
//! and `<example>` — each matched to its own close tag, nesting counted, so
//! an example may hold any markup — and everything else, which is the help.
//! A child inside another element of the help (`<details><summary>`) is
//! that element's, and help; so is one inside a backtick code span or fence,
//! which is how the help shows markup. The help is markdown: the lines the
//! four elements stood on removed, the rest de-indented by its common
//! indentation, blank lines around it trimmed.
//!
//! **Malformed** — refused in-band by the write that holds it, naming the
//! problem: two `<docs>`; a `<docs>` or one of its four children never
//! closed; two `<summary>`s; an `<attr>` without a `name`; an `<attr>` whose
//! `type` is not one of `string`, `number`, `boolean`, `shortcode`,
//! `ticket`, `enum(a|b|c)` ([`AttrKind`]'s `FromStr`); an attribute or a
//! slot (the default slot included) declared twice.
//!
//! **The preview line** ([`preview`]): after ` ```component m-x `, the
//! attributes an author writes as on a tag — `board=RETURNS label="Platform
//! team"` — `[name, value]` in order, the name ASCII-lowercased, a value
//! bare, double- or single-quoted, a bare name's value `""`. Nothing on that
//! line is refused: it was ignored before COMP-8, and what cannot be read
//! as an attribute is skipped.

use crate::types::{AttrDoc, AttrKind, ComponentDocs, SlotDoc};

/// HTML's void elements: an open tag with no close tag to look for.
const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr",
];

/// The `<docs>` of a component's `source`, parsed — `Ok(None)` when it has
/// none, `Err` naming what is malformed.
pub fn parse(source: &str) -> Result<Option<ComponentDocs>, String> {
    let Some((start, end)) = find_docs(source)? else { return Ok(None) };
    docs(&source[start..end]).map(Some)
}

/// The attributes on a definition's opening line after the tag — see the
/// module doc.
pub fn preview(rest: &str) -> Vec<(String, String)> {
    attributes(rest).into_iter().map(|(k, v)| (k, v.unwrap_or_default())).collect()
}

/// A fence's info string, if it opens a definition — ` ```component m-x
/// board=RETURNS ` — the tag as written (`""` when it names none) and its
/// [`preview`] attributes; `None` for any other fence (its first word is not
/// `component`). Whether the tag is a component name is the caller's to say.
pub fn definition(info: &str) -> Option<(&str, Vec<(String, String)>)> {
    let (first, rest) = word(info);
    if first != "component" {
        return None;
    }
    let (name, rest) = word(rest);
    Some((name, preview(rest)))
}

/// The names HTML reserves among those a custom element could otherwise take
/// (SVG's and MathML's hyphenated elements).
const RESERVED: [&str; 8] = [
    "annotation-xml",
    "color-profile",
    "font-face",
    "font-face-src",
    "font-face-uri",
    "font-face-format",
    "font-face-name",
    "missing-glyph",
];

/// Whether `name` is a component's name: a valid custom element name, in
/// ASCII — a lowercase letter, then lowercase letters, digits, `-`, `.` and
/// `_`, with at least one hyphen, and not one of the reserved names.
pub fn is_component_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | '_'))
        && name.contains('-')
        && !RESERVED.contains(&name)
}

/// The first whitespace-separated word of `s`, and what follows it.
fn word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    (&s[..end], &s[end..])
}

/// The content range of the source's one top-level `<docs>`.
fn find_docs(src: &str) -> Result<Option<(usize, usize)>, String> {
    let mut found = None;
    let mut i = 0;
    while let Some(off) = src[i..].find('<') {
        let lt = i + off;
        if let Some(after) = comment_end(src, lt) {
            i = after;
            continue;
        }
        let Some((name, name_end)) = tag_name(src, lt) else {
            i = lt + 1;
            continue;
        };
        let Some((gt, self_closing)) = open_end(src, name_end) else {
            i = lt + 1;
            continue;
        };
        i = match name.as_str() {
            "script" | "style" => raw_close(src, &name, gt).map_or(src.len(), |(_, after)| after),
            "template" => close(src, &name, gt).map_or(src.len(), |(_, after)| after),
            "docs" => {
                if found.is_some() {
                    return Err("two <docs> elements — a component has one".into());
                }
                if self_closing {
                    found = Some((gt, gt));
                    gt
                } else {
                    let (content_end, after) =
                        close(src, "docs", gt).ok_or("<docs> is never closed — end it with </docs>")?;
                    found = Some((gt, content_end));
                    after
                }
            }
            _ => gt,
        };
    }
    Ok(found)
}

/// A `<docs>` element's content, read.
fn docs(content: &str) -> Result<ComponentDocs, String> {
    let mut out = ComponentDocs::default();
    let mut summary = false;
    let mut default_slot = false;
    // The ranges the four elements take — the rest is the help.
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while let Some(off) = content[i..].find(['<', '`']) {
        let at = i + off;
        if content[at..].starts_with('`') {
            i = code_span_end(content, at);
            continue;
        }
        if let Some(after) = comment_end(content, at) {
            i = after;
            continue;
        }
        let Some((name, name_end)) = tag_name(content, at) else {
            i = at + 1;
            continue;
        };
        let Some((gt, self_closing)) = open_end(content, name_end) else {
            i = at + 1;
            continue;
        };
        if !matches!(name.as_str(), "summary" | "attr" | "slot" | "example") {
            // Help: an element of it is skipped whole, so what it holds stays
            // its own.
            i = if self_closing || VOID.contains(&name.as_str()) {
                gt
            } else {
                close(content, &name, gt).map_or(gt, |(_, after)| after)
            };
            continue;
        }
        let attrs = attributes(content[name_end..gt - 1].trim_end_matches('/'));
        let (inner, end) = if self_closing {
            ("", gt)
        } else {
            let (inner_end, after) =
                close(content, &name, gt).ok_or_else(|| format!("a <{name}> is never closed — end it with </{name}>"))?;
            (&content[gt..inner_end], after)
        };
        let attr = |key: &str| attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_deref().unwrap_or(""));
        match name.as_str() {
            "summary" => {
                if summary {
                    return Err("two <summary> elements — a component has one summary".into());
                }
                summary = true;
                out.summary = text(inner);
            }
            "attr" => {
                let attr_name = attr("name").map(|n| n.trim().to_ascii_lowercase()).unwrap_or_default();
                if attr_name.is_empty() {
                    return Err("an <attr> without a name — write <attr name=\"board\">".into());
                }
                let kind = match attr("type") {
                    None => AttrKind::String,
                    Some(t) => t.parse::<AttrKind>().map_err(|e| format!("<attr name=\"{attr_name}\">: {e}"))?,
                };
                if out.attrs.iter().any(|a| a.name == attr_name) {
                    return Err(format!("the attribute '{attr_name}' is declared twice"));
                }
                out.attrs.push(AttrDoc {
                    name: attr_name,
                    kind,
                    required: attr("required").is_some(),
                    description: text(inner),
                });
            }
            "slot" => {
                let slot_name = attr("name").map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
                match &slot_name {
                    None if default_slot => return Err("the default slot is declared twice".into()),
                    None => default_slot = true,
                    Some(n) if out.slots.iter().any(|s| s.name.as_deref() == Some(n)) => {
                        return Err(format!("the slot '{n}' is declared twice"));
                    }
                    Some(_) => {}
                }
                out.slots.push(SlotDoc {
                    name: slot_name,
                    description: text(inner),
                });
            }
            _ => out.examples.push(block(inner)),
        }
        cuts.push((at, end));
        i = end;
    }
    out.help = help(content, &cuts);
    Ok(out)
}

/// Past an HTML comment opening at `at`, if one does (an unclosed one runs
/// to the end).
fn comment_end(s: &str, at: usize) -> Option<usize> {
    s[at..]
        .starts_with("<!--")
        .then(|| s[at + 4..].find("-->").map_or(s.len(), |j| at + 4 + j + 3))
}

/// Past a backtick code span (or fence) opening at `at`: the next run of as
/// many backticks — or, with none, just past this run.
fn code_span_end(s: &str, at: usize) -> usize {
    let run = s[at..].bytes().take_while(|b| *b == b'`').count();
    let mut i = at + run;
    while let Some(off) = s[i..].find('`') {
        let start = i + off;
        let len = s[start..].bytes().take_while(|b| *b == b'`').count();
        if len == run {
            return start + len;
        }
        i = start + len;
    }
    at + run
}

/// The tag `<name` opening at `lt` — its name, ASCII-lowercased, and where
/// the name ends — if `lt` opens one (a letter after `<`; `</` closes).
fn tag_name(s: &str, lt: usize) -> Option<(String, usize)> {
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
fn open_end(s: &str, from: usize) -> Option<(usize, bool)> {
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

/// The close tag matching an element `name` whose content starts at `from`
/// — where it starts and where it ends — counting the same name's nested
/// opens and closes, outside comments.
fn close(s: &str, name: &str, from: usize) -> Option<(usize, usize)> {
    let mut depth = 1usize;
    let mut i = from;
    while let Some(off) = s[i..].find('<') {
        let lt = i + off;
        if let Some(after) = comment_end(s, lt) {
            i = after;
            continue;
        }
        if s[lt + 1..].starts_with('/') && name_at(s, lt + 2, name) {
            let after = s[lt..].find('>').map_or(s.len(), |j| lt + j + 1);
            depth -= 1;
            if depth == 0 {
                return Some((lt, after));
            }
            i = after;
            continue;
        }
        if name_at(s, lt + 1, name) {
            if let Some((gt, self_closing)) = open_end(s, lt + 1 + name.len()) {
                if !self_closing {
                    depth += 1;
                }
                i = gt;
                continue;
            }
        }
        i = lt + 1;
    }
    None
}

/// The close tag of a raw-text element (`script`, `style`): the first
/// `</name`, nothing nested.
fn raw_close(s: &str, name: &str, from: usize) -> Option<(usize, usize)> {
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
fn attributes(s: &str) -> Vec<(String, Option<String>)> {
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
fn entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest.find(';').filter(|&j| j <= 10).and_then(|j| {
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
/// entities decoded, whitespace collapsed, trimmed.
fn text(inner: &str) -> String {
    let mut plain = String::with_capacity(inner.len());
    let mut i = 0;
    while let Some(off) = inner[i..].find('<') {
        let lt = i + off;
        plain.push_str(&inner[i..lt]);
        let is_tag = inner[lt + 1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        match inner[lt..].find('>').filter(|_| is_tag) {
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

/// A block of markup or prose as an author indented it inside the source:
/// the blank lines around it dropped and the common indentation of its lines
/// removed (a first line that starts right after the opening tag does not
/// count toward it). Line ends become `\n`.
fn block(s: &str) -> String {
    let mut lines: Vec<&str> = s.lines().collect();
    let first_inline = lines.first().is_some_and(|l| !l.trim().is_empty() && !l.starts_with([' ', '\t']));
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let indent = lines
        .iter()
        .enumerate()
        .filter(|(n, l)| !(first_inline && *n == 0) && !l.trim().is_empty())
        .map(|(_, l)| l.len() - l.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .enumerate()
        .map(|(n, l)| {
            if first_inline && n == 0 {
                l.trim_end()
            } else {
                let cut = l.len() - l.trim_start_matches([' ', '\t']).len();
                l[cut.min(indent)..].trim_end()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The help: `content` without the four elements' ranges — an element alone
/// on its lines takes the lines — as a [`block`].
fn help(content: &str, cuts: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(content.len());
    let mut from = 0;
    for &(start, end) in cuts {
        let line_start = content[..start].rfind('\n').map_or(0, |j| j + 1);
        let alone_before = content[line_start..start].trim().is_empty();
        let line_end = content[end..].find('\n').map_or(content.len(), |j| end + j + 1);
        let alone_after = content[end..line_end].trim().is_empty();
        let (cut_start, cut_end) = if alone_before && alone_after { (line_start, line_end) } else { (start, end) };
        let cut_start = cut_start.max(from);
        out.push_str(&content[from..cut_start]);
        from = cut_end.max(from);
    }
    out.push_str(&content[from..]);
    // The lines around a removed element may leave two blank lines where the
    // author meant one paragraph break: runs of blank lines become one,
    // outside fenced code (where they are the code's).
    let mut lines: Vec<&str> = Vec::new();
    let mut fence: Option<&str> = None;
    for line in out.lines() {
        let t = line.trim_start();
        let marker = if t.starts_with("```") { Some("```") } else if t.starts_with("~~~") { Some("~~~") } else { None };
        match (fence, marker) {
            (None, Some(m)) => fence = Some(m),
            (Some(open), Some(m)) if open == m => fence = None,
            _ => {}
        }
        if fence.is_none() && line.trim().is_empty() && lines.last().is_some_and(|l| l.trim().is_empty()) {
            continue;
        }
        lines.push(line);
    }
    block(&lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs_of(source: &str) -> ComponentDocs {
        parse(source).unwrap().expect("docs")
    }

    #[test]
    fn a_source_without_docs_has_none() {
        assert_eq!(parse("<template><b>x</b></template>\n<style>b{}</style>"), Ok(None));
        assert_eq!(parse(""), Ok(None));
        // A <docs> inside the template, a script or a comment is not the component's.
        assert_eq!(parse("<template><docs><summary>no</summary></docs></template>"), Ok(None));
        assert_eq!(parse("<script>this.root.innerHTML = '<docs>'</script>"), Ok(None));
        assert_eq!(parse("<!-- <docs><summary>no</summary></docs> -->"), Ok(None));
    }

    #[test]
    fn the_tickets_example_parses_into_every_row() {
        let source = r#"<template><slot name="title"></slot><span id="n"></span></template>
<docs>
  <summary>How many of a board's tickets are workable now (wave 1).</summary>
  <attr name="board" type="shortcode" required>The shortcode to count, e.g. RETURNS.</attr>
  <attr name="label" type="string">Shown before the count.</attr>
  <slot name="title">Optional title markup.</slot>
  <example><m-wave-summary board="RETURNS"></m-wave-summary></example>
  Longer help, as markdown prose.
</docs>
<script>this.root.querySelector('#n').textContent = '0'</script>
"#;
        let d = docs_of(source);
        assert_eq!(d.summary, "How many of a board's tickets are workable now (wave 1).");
        assert_eq!(
            d.attrs,
            vec![
                AttrDoc {
                    name: "board".into(),
                    kind: AttrKind::Shortcode,
                    required: true,
                    description: "The shortcode to count, e.g. RETURNS.".into()
                },
                AttrDoc {
                    name: "label".into(),
                    kind: AttrKind::String,
                    required: false,
                    description: "Shown before the count.".into()
                },
            ]
        );
        assert_eq!(
            d.slots,
            vec![SlotDoc {
                name: Some("title".into()),
                description: "Optional title markup.".into()
            }]
        );
        assert_eq!(d.examples, vec![r#"<m-wave-summary board="RETURNS"></m-wave-summary>"#.to_string()]);
        assert_eq!(d.help, "Longer help, as markdown prose.");
    }

    #[test]
    fn every_attr_kind_and_the_default() {
        let d = docs_of(
            "<docs>\
             <attr name=a>untyped</attr>\
             <attr name=b type=number></attr>\
             <attr name=c type=\"boolean\"></attr>\
             <attr name=d type='shortcode'></attr>\
             <attr name=e type=ticket required></attr>\
             <attr name=F type=\"enum(low | mid|high)\"/>\
             </docs>",
        );
        let kinds: Vec<(&str, &AttrKind, bool)> = d.attrs.iter().map(|a| (a.name.as_str(), &a.kind, a.required)).collect();
        assert_eq!(
            kinds,
            vec![
                ("a", &AttrKind::String, false),
                ("b", &AttrKind::Number, false),
                ("c", &AttrKind::Boolean, false),
                ("d", &AttrKind::Shortcode, false),
                ("e", &AttrKind::Ticket, true),
                ("f", &AttrKind::Enum { values: vec!["low".into(), "mid".into(), "high".into()] }, false),
            ],
            "names folded; a self-closing <attr/> describes nothing"
        );
        assert_eq!(d.attrs[0].description, "untyped");
        assert_eq!(d.attrs[5].description, "");
        assert_eq!(d.attrs[5].kind.to_string(), "enum(low|mid|high)");
    }

    #[test]
    fn slots_named_and_default() {
        let d = docs_of("<docs><slot>The body.</slot><slot name=\"title\">The <b>title</b>.</slot></docs>");
        assert_eq!(
            d.slots,
            vec![
                SlotDoc { name: None, description: "The body.".into() },
                SlotDoc { name: Some("title".into()), description: "The title.".into() },
            ],
            "the default slot's name is None; a description's tags are dropped"
        );
    }

    #[test]
    fn examples_are_inner_html_as_written_and_help_is_the_rest_as_markdown() {
        let source = "<docs>
    <summary>
      Spread
      over   lines &amp; tags.
    </summary>
    Intro paragraph.

    <example>
      <m-x board=\"A\">
        <span slot=\"title\">T</span>
      </m-x>
    </example>
    <example><m-x></m-x></example>

    - a list item
    - another, with `<attr>` in code

    <details><summary>more</summary>inner</details>

    ```html
    <example>shown, not parsed</example>
    ```
</docs>";
        let d = docs_of(source);
        assert_eq!(d.summary, "Spread over lines & tags.", "text, collapsed and decoded");
        assert_eq!(
            d.examples,
            vec![
                "<m-x board=\"A\">\n  <span slot=\"title\">T</span>\n</m-x>".to_string(),
                "<m-x></m-x>".to_string()
            ]
        );
        assert_eq!(
            d.help,
            "Intro paragraph.\n\n- a list item\n- another, with `<attr>` in code\n\n\
             <details><summary>more</summary>inner</details>\n\n```html\n<example>shown, not parsed</example>\n```",
            "the four elements' lines gone, de-indented; a <summary> in the help's own markup and \
             a tag in code are help"
        );
    }

    #[test]
    fn an_empty_docs_is_docs_with_nothing_in_it() {
        assert_eq!(parse("<docs></docs>"), Ok(Some(ComponentDocs::default())));
        assert_eq!(parse("<docs/>"), Ok(Some(ComponentDocs::default())));
        assert_eq!(docs_of("<docs>Only help.</docs>").help, "Only help.");
    }

    #[test]
    fn malformed_docs_are_refused_naming_the_problem() {
        for (source, needle) in [
            ("<docs><attr type=number>x</attr></docs>", "an <attr> without a name"),
            ("<docs><attr name=\"\">x</attr></docs>", "an <attr> without a name"),
            ("<docs><attr name=n type=date>x</attr></docs>", "unknown type 'date'"),
            ("<docs><attr name=n type=\"enum()\">x</attr></docs>", "names no value"),
            ("<docs><summary>a</summary><summary>b</summary></docs>", "two <summary> elements"),
            ("<docs></docs><docs></docs>", "two <docs> elements"),
            ("<docs><summary>a</summary>", "<docs> is never closed"),
            ("<docs><attr name=n>never closed</docs>", "<attr> is never closed"),
            ("<docs><attr name=n></attr><attr name=N></attr></docs>", "the attribute 'n' is declared twice"),
            ("<docs><slot name=t></slot><slot name=t></slot></docs>", "the slot 't' is declared twice"),
            ("<docs><slot></slot><slot></slot></docs>", "the default slot is declared twice"),
        ] {
            let e = parse(source).expect_err(source);
            assert!(e.contains(needle), "{source}: {e}");
        }
        assert!(parse("<docs><attr name=n type=date></attr></docs>").unwrap_err().contains("<attr name=\"n\">"));
    }

    #[test]
    fn the_preview_line_parses_quoted_and_bare_values() {
        assert_eq!(
            preview(r#"board=RETURNS label="Platform team" Note='single quoted' compact"#),
            vec![
                ("board".to_string(), "RETURNS".to_string()),
                ("label".to_string(), "Platform team".to_string()),
                ("note".to_string(), "single quoted".to_string()),
                ("compact".to_string(), String::new()),
            ]
        );
        assert_eq!(preview(""), vec![]);
        assert_eq!(preview("  a = \"x &amp; y\"  b"), vec![("a".into(), "x & y".into()), ("b".into(), String::new())]);
        assert_eq!(preview("label=\"unterminated"), vec![("label".into(), "unterminated".into())]);
    }

    #[test]
    fn a_definitions_info_string() {
        assert_eq!(
            definition("component m-x board=RETURNS label=\"A b\""),
            Some(("m-x", vec![("board".to_string(), "RETURNS".to_string()), ("label".to_string(), "A b".to_string())]))
        );
        assert_eq!(definition("component"), Some(("", vec![])));
        assert_eq!(definition("  component   m-y  "), Some(("m-y", vec![])));
        assert_eq!(definition("components m-x"), None);
        assert_eq!(definition("rust"), None);
        assert_eq!(definition(""), None);
    }

    #[test]
    fn entities_and_text() {
        assert_eq!(entities("a &lt;b&gt; &#65;&#x42; &bogus; & c"), "a <b> AB &bogus; & c");
        assert_eq!(text(" one <code>two</code>\n  three a<b "), "one two three a<b");
    }
}
