//! V2-20: a body's **marked elements** — the custom elements an input is
//! written with, found in a body that is otherwise text. v2 keeps a body as
//! text and renders none of it on the server, so this is a scan and no HTML
//! tree: it finds the tags it is told of and their nesting, and nothing else.
//!
//! A tag counts where a reader sees markup, and so never in:
//!
//! - the **frontmatter** — a `---` first line to the next `---` line;
//! - a **fence** (``` or ~~~, to its close) or **indented code** (four
//!   spaces or a tab, after a blank line, outside any open element);
//! - a **code span** (a run of backticks to the next run as long);
//! - a **comment** (`<!-- … -->`), or inside `<pre>`, `<code>` or `<m-mockup>`
//!   (a page drawn whole inside a body: its markup is that page's).
//!
//! An element left open ends with its parent, or the body. A stray close is
//! nothing. Attribute values have their entities decoded, as does [`text`].

/// One element of the tags asked for, with the ones inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub tag: String,
    attrs: Vec<(String, Option<String>)>,
    /// What stands between its tags, as written.
    pub content: String,
    /// The asked-for elements inside it, in order.
    pub parts: Vec<Element>,
}

impl Element {
    /// The attribute `name`, its value — an attribute with none is `""`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_deref().unwrap_or(""))
    }
}

/// An element open while the scan walks.
struct Open {
    element: Element,
    /// Where its content starts.
    from: usize,
}

/// The frontmatter's end, past its closing line — `0` with none.
fn after_frontmatter(body: &str) -> usize {
    let Some(rest) = body.strip_prefix("---\n").or_else(|| body.strip_prefix("---\r\n")) else { return 0 };
    let start = body.len() - rest.len();
    let mut at = start;
    for line in rest.split_inclusive('\n') {
        at += line.len();
        if line.trim_end() == "---" {
            return at;
        }
    }
    0
}

/// The line starting at `at`: where it ends, past its newline.
fn line_end(body: &str, at: usize) -> usize {
    body[at..].find('\n').map_or(body.len(), |n| at + n + 1)
}

/// A fence's marker when `line` opens one: its character and length.
fn fence(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let c = t.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = t.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

/// Whether `text` at `at` opens the tag `name` (case-insensitive, then a
/// space, `>`, `/` or a line's end).
fn opens(text: &str, at: usize, name: &str) -> bool {
    let rest = &text.as_bytes()[at..];
    rest.len() > name.len()
        && rest[..name.len()].eq_ignore_ascii_case(name.as_bytes())
        && matches!(rest[name.len()], b' ' | b'>' | b'/' | b'\n' | b'\t' | b'\r')
}

/// Past the close of `<pre>` or `<code>` opened before `at`.
fn past_close(body: &str, at: usize, close: &str) -> usize {
    let lower = close.as_bytes();
    body.as_bytes()[at..].windows(lower.len()).position(|w| w.eq_ignore_ascii_case(lower)).map_or(body.len(), |n| at + n + lower.len())
}

/// The attributes between a tag's name and its `>`, and where it ends:
/// past the `>`, or `None` for a tag that never closes.
fn attributes(body: &str, mut at: usize) -> Option<(Vec<(String, Option<String>)>, usize, bool)> {
    let b = body.as_bytes();
    let mut attrs = Vec::new();
    loop {
        while at < b.len() && b[at].is_ascii_whitespace() {
            at += 1;
        }
        match *b.get(at)? {
            b'>' => return Some((attrs, at + 1, false)),
            b'/' if b.get(at + 1) == Some(&b'>') => return Some((attrs, at + 2, true)),
            b'/' => at += 1,
            _ => {
                let start = at;
                while at < b.len() && !b[at].is_ascii_whitespace() && !matches!(b[at], b'=' | b'>' | b'/') {
                    at += 1;
                }
                let name = body[start..at].to_ascii_lowercase();
                if name.is_empty() {
                    at += 1;
                    continue;
                }
                let mut probe = at;
                while probe < b.len() && b[probe].is_ascii_whitespace() {
                    probe += 1;
                }
                if b.get(probe) != Some(&b'=') {
                    attrs.push((name, None));
                    continue;
                }
                at = probe + 1;
                while at < b.len() && b[at].is_ascii_whitespace() {
                    at += 1;
                }
                let value = match *b.get(at)? {
                    q @ (b'"' | b'\'') => {
                        let end = at + 1 + body[at + 1..].find(q as char)?;
                        let v = &body[at + 1..end];
                        at = end + 1;
                        v
                    }
                    _ => {
                        let start = at;
                        while at < b.len() && !b[at].is_ascii_whitespace() && b[at] != b'>' {
                            at += 1;
                        }
                        &body[start..at]
                    }
                };
                attrs.push((name, Some(entities(value))));
            }
        }
    }
}

/// The elements of `tags` in `body`, in order, each holding those inside it.
/// `tags` are lower case.
pub fn marked(body: &str, tags: &[&str]) -> Vec<Element> {
    let mut top = Vec::new();
    let mut stack: Vec<Open> = Vec::new();
    let mut at = after_frontmatter(body);
    let mut line_start = true;
    let mut blank_before = true;
    let close_into = |stack: &mut Vec<Open>, top: &mut Vec<Element>, end: usize| {
        if let Some(mut open) = stack.pop() {
            open.element.content = body[open.from..end].to_string();
            match stack.last_mut() {
                Some(parent) => parent.element.parts.push(open.element),
                None => top.push(open.element),
            }
        }
    };
    while at < body.len() {
        if line_start {
            let end = line_end(body, at);
            let line = &body[at..end];
            if let Some((c, n)) = fence(line.trim_end()) {
                // To its close, or the body's end.
                let mut p = end;
                while p < body.len() {
                    let e = line_end(body, p);
                    let l = body[p..e].trim();
                    if l.len() >= n && l.chars().all(|x| x == c) {
                        p = e;
                        break;
                    }
                    p = e;
                }
                at = p;
                blank_before = false;
                continue;
            }
            if stack.is_empty() && blank_before && (line.starts_with("    ") || line.starts_with('\t')) && !line.trim().is_empty() {
                at = end;
                continue;
            }
            blank_before = line.trim().is_empty();
            line_start = false;
        }
        let c = body.as_bytes()[at];
        match c {
            b'\n' => {
                at += 1;
                line_start = true;
            }
            b'`' => {
                let n = body[at..].bytes().take_while(|x| *x == b'`').count();
                let ticks = &body[at..at + n];
                at += n;
                // A run with no match is text.
                let mut p = at;
                while let Some(f) = body[p..].find(ticks) {
                    let start = p + f;
                    let run = body[start..].bytes().take_while(|x| *x == b'`').count();
                    if run == n {
                        at = start + n;
                        break;
                    }
                    p = start + run;
                }
            }
            b'<' if body[at..].starts_with("<!--") => {
                at = body[at + 4..].find("-->").map_or(body.len(), |n| at + 4 + n + 3);
            }
            b'<' if opens(body, at + 1, "pre") => at = past_close(body, at, "</pre>"),
            b'<' if opens(body, at + 1, "code") => at = past_close(body, at, "</code>"),
            b'<' if opens(body, at + 1, "m-mockup") => at = past_close(body, at, "</m-mockup>"),
            b'<' if body[at + 1..].starts_with('/') => {
                let name_at = at + 2;
                let Some(tag) = tags.iter().find(|t| opens(body, name_at, t)) else {
                    at += 1;
                    continue;
                };
                let close = line_of_gt(body, name_at + tag.len());
                if let Some(depth) = stack.iter().rposition(|o| o.element.tag == *tag) {
                    while stack.len() > depth + 1 {
                        close_into(&mut stack, &mut top, at);
                    }
                    close_into(&mut stack, &mut top, at);
                }
                at = close;
            }
            b'<' => {
                let name_at = at + 1;
                let Some(tag) = tags.iter().find(|t| opens(body, name_at, t)) else {
                    at += 1;
                    continue;
                };
                let Some((attrs, end, closed)) = attributes(body, name_at + tag.len()) else {
                    at += 1;
                    continue;
                };
                let element = Element { tag: tag.to_string(), attrs, content: String::new(), parts: Vec::new() };
                stack.push(Open { element, from: end });
                if closed {
                    close_into(&mut stack, &mut top, end);
                }
                at = end;
            }
            _ => {
                // To the next character that means anything.
                let n = body[at..].find(['\n', '`', '<']).unwrap_or(body.len() - at);
                at += n.max(1);
                while !body.is_char_boundary(at) {
                    at += 1;
                }
            }
        }
    }
    while !stack.is_empty() {
        close_into(&mut stack, &mut top, body.len());
    }
    top
}

/// Past the `>` at or after `at`.
fn line_of_gt(body: &str, at: usize) -> usize {
    body[at..].find('>').map_or(body.len(), |n| at + n + 1)
}

/// `s` with its character references decoded (`&amp;`, `&#39;`, `&#x27;`).
/// One it does not know stays as written.
pub fn entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let number = |digits: &str, radix| u32::from_str_radix(digits, radix).ok().and_then(char::from_u32);
        let c = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            n => match n.strip_prefix('#') {
                Some(d) => match d.strip_prefix(['x', 'X']) {
                    Some(h) => number(h, 16),
                    None => number(d, 10),
                },
                None => None,
            },
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
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

/// `content` as text: its tags dropped, its entities decoded, its whitespace
/// collapsed. A `<` that opens no tag is text.
pub fn text(content: &str) -> String {
    let mut bare = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(at) = rest.find('<') {
        bare.push_str(&rest[..at]);
        let tail = &rest[at..];
        let tag = tail[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        match tail.find('>').filter(|_| tag) {
            Some(end) => rest = &tail[end + 1..],
            None => {
                bare.push('<');
                rest = &tail[1..];
            }
        }
    }
    bare.push_str(rest);
    entities(&bare).split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAGS: &[&str] = &["m-a", "m-b"];

    fn shape(e: &Element) -> String {
        format!("{}[{}]", e.tag, e.parts.iter().map(shape).collect::<Vec<_>>().join(","))
    }

    #[test]
    fn elements_nest_in_the_order_written() {
        let found = marked("x <m-a id=\"1\" flag><p>t</p><m-b v='2'>in</m-b><m-b/></m-a> y <M-B>z</M-B>", TAGS);
        assert_eq!(found.iter().map(shape).collect::<Vec<_>>(), ["m-a[m-b[],m-b[]]", "m-b[]"]);
        assert_eq!((found[0].attr("id"), found[0].attr("flag"), found[0].attr("no")), (Some("1"), Some(""), None));
        assert_eq!((found[0].parts[0].attr("v"), found[0].parts[0].content.as_str()), (Some("2"), "in"));
    }

    #[test]
    fn an_element_left_open_ends_with_its_parent_or_the_body() {
        let found = marked("<m-a><m-b>open</m-a> after", TAGS);
        assert_eq!(found.iter().map(shape).collect::<Vec<_>>(), ["m-a[m-b[]]"]);
        assert_eq!(marked("<m-a>to the end", TAGS)[0].content, "to the end");
        assert!(marked("</m-a> nothing", TAGS).is_empty());
        assert!(marked("<m-a never closes", TAGS).is_empty());
    }

    #[test]
    fn code_comments_and_the_frontmatter_hold_no_markup() {
        for body in [
            "```html\n<m-a></m-a>\n```\n",
            "~~~~\n<m-a></m-a>\n~~~\n```\n~~~~\n",
            "`<m-a>` said",
            "``a ` <m-a> b``",
            "para\n\n    <m-a></m-a>\n",
            "\t<m-a></m-a>\n",
            "<!-- <m-a></m-a> -->",
            "---\nn: <m-a>\n---\nbody",
            "<pre><m-a></m-a></pre>",
            "<code><m-a></code>",
            "<m-mockup><html><m-a></m-a></html></m-mockup>",
        ] {
            assert!(marked(body, TAGS).is_empty(), "{body}");
        }
    }

    /// Indented, an element's parts are markup: only outside one is it code.
    #[test]
    fn indentation_inside_an_element_is_not_code() {
        let found = marked("<m-a>\n\n    <m-b></m-b>\n\n</m-a>\n", TAGS);
        assert_eq!(found.iter().map(shape).collect::<Vec<_>>(), ["m-a[m-b[]]"]);
        // A backtick run with no match is text, and non-ASCII walks whole.
        assert_eq!(marked("` é <m-a></m-a>", TAGS).len(), 1);
    }

    #[test]
    fn text_drops_tags_and_decodes() {
        assert_eq!(text(" Which <em>checks</em>\n ran? "), "Which checks ran?");
        assert_eq!(text("Unit &amp; store &#39;x&#x27; &#96;y&#96; &nope; 1 < 2"), "Unit & store 'x' `y` &nope; 1 < 2");
        assert_eq!(entities("&lt;/m-input&gt;"), "</m-input>");
    }
}
