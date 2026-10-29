//! UI-10: **HTML the page may hold** — `{@html expr}` draws the
//! expression's text as HTML, and `markdown(text)` makes HTML of
//! markdown; both go through [`sanitize`] first.
//!
//! The sanitizer is an allow-list: it parses the text into elements and
//! text, keeps a tag only when it is listed ([`ALLOWED_TAGS`]) and an
//! attribute only when it is listed for its tag ([`allowed_attr`]); a URL
//! attribute keeps only `http:`, `https:`, `#…` and `mailto:` values. A tag
//! not listed is dropped and its text kept — except `script`, `style`,
//! `iframe`, `object`, `embed`, `noscript`, `template`, `svg` and `math`,
//! dropped with everything inside. What it answers is a tree the engine
//! draws through the renderer: no HTML string reaches the page.

/// A sanitized node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HNode {
    Element { tag: String, attrs: Vec<(String, String)>, children: Vec<HNode> },
    Text(String),
}

/// The tags kept.
pub const ALLOWED_TAGS: &[&str] = &[
    "a", "abbr", "b", "blockquote", "br", "code", "dd", "del", "div", "dl", "dt", "em", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "kbd", "li",
    "mark", "ol", "p", "pre", "q", "s", "small", "span", "strong", "sub", "sup", "table", "tbody", "td", "tfoot", "th", "thead", "tr", "u", "ul",
];

/// Elements with no content and no end tag.
const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];

/// Dropped with everything inside them.
const DROPPED_WHOLE: &[&str] = &["script", "style", "iframe", "object", "embed", "noscript", "template", "svg", "math", "textarea", "title", "head", "frameset", "frame", "noembed", "xmp", "plaintext"];

/// Whose content is raw text (no tags inside, up to the end tag).
const RAW_TEXT: &[&str] = &["script", "style", "textarea", "title", "xmp", "noembed", "plaintext"];

/// Attributes that hold a URL: kept only for `http:`, `https:`, `#…`, `mailto:`.
const URL_ATTRS: &[&str] = &["href", "src"];

/// Whether `attr` is kept on `tag`.
pub fn allowed_attr(tag: &str, attr: &str) -> bool {
    match (tag, attr) {
        (_, "title") => true,
        ("a", "href") => true,
        ("img", "src" | "alt" | "width" | "height") => true,
        ("code" | "pre", "class") => true,
        ("td" | "th", "colspan" | "rowspan") => true,
        ("ol", "start") => true,
        _ => false,
    }
}

fn url_ok(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    v.starts_with("https://") || v.starts_with("http://") || v.starts_with("mailto:") || v.starts_with('#')
}

/// `&amp;`, `&lt;`, `&#39;`, `&#x27;`, … decoded; an unknown entity kept as written.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(|c: char| c == ';' || c == '&' || c.is_whitespace() || c == '<').map(|j| j + 1);
        let decoded = match end.filter(|e| rest.as_bytes().get(*e) == Some(&b';')) {
            Some(e) => {
                let name = &rest[1..e];
                let ch = match name {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32),
                    n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32),
                    _ => None,
                };
                ch.map(|c| (c, e + 1))
            }
            None => None,
        };
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

/// An open element while parsing.
struct Open {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<HNode>,
}

fn push_text(stack: &mut [Open], root: &mut Vec<HNode>, text: String) {
    if text.is_empty() {
        return;
    }
    let into = match stack.last_mut() {
        Some(o) => &mut o.children,
        None => root,
    };
    if let Some(HNode::Text(t)) = into.last_mut() {
        t.push_str(&text);
    } else {
        into.push(HNode::Text(text));
    }
}

fn close(stack: &mut Vec<Open>, root: &mut Vec<HNode>) {
    if let Some(o) = stack.pop() {
        let node = HNode::Element { tag: o.tag, attrs: o.attrs, children: o.children };
        match stack.last_mut() {
            Some(p) => p.children.push(node),
            None => root.push(node),
        }
    }
}

/// A tag's name at `s` (letters, digits, `-`), lower-cased.
fn tag_name(s: &str) -> Option<(String, usize)> {
    let b = s.as_bytes();
    if !b.first()?.is_ascii_alphabetic() {
        return None;
    }
    let n = b.iter().position(|c| !(c.is_ascii_alphanumeric() || *c == b'-' || *c == b':')).unwrap_or(b.len());
    Some((s[..n].to_ascii_lowercase(), n))
}

/// The attributes of a start tag from `s` (after its name) to its `>`:
/// the attributes, whether it closed itself, and how far it went.
fn attributes(s: &str) -> (Vec<(String, String)>, bool, usize) {
    let b = s.as_bytes();
    let mut i = 0;
    let mut attrs = Vec::new();
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            if b[i] == b'/' && b.get(i + 1) == Some(&b'>') {
                return (attrs, true, i + 2);
            }
            i += 1;
        }
        if i >= b.len() {
            return (attrs, false, i);
        }
        if b[i] == b'>' {
            return (attrs, false, i + 1);
        }
        let start = i;
        while i < b.len() && !(b[i].is_ascii_whitespace() || b[i] == b'=' || b[i] == b'>' || b[i] == b'/') {
            i += 1;
        }
        let name = s[start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if b.get(i) == Some(&b'=') {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            match b.get(i) {
                Some(&q) if q == b'"' || q == b'\'' => {
                    let end = s[i + 1..].find(q as char).map(|e| i + 1 + e).unwrap_or(b.len());
                    value = decode(&s[i + 1..end]);
                    i = (end + 1).min(b.len());
                }
                _ => {
                    let vs = i;
                    while i < b.len() && !(b[i].is_ascii_whitespace() || b[i] == b'>') {
                        i += 1;
                    }
                    value = decode(&s[vs..i]);
                }
            }
        }
        if !name.is_empty() {
            attrs.push((name, value));
        }
    }
}

/// Where the end tag `</tag` closing a dropped element is, counting the
/// same tag opened inside it (raw text: the first end tag); none: the end.
fn skip_to_end(s: &str, tag: &str) -> usize {
    let lower = s.to_ascii_lowercase();
    let (open, close) = (format!("<{tag}"), format!("</{tag}"));
    let raw = RAW_TEXT.contains(&tag);
    let mut depth = 1;
    let mut i = 0;
    loop {
        let next_close = lower[i..].find(&close).map(|j| i + j);
        let next_open = if raw { None } else { lower[i..].find(&open).map(|j| i + j) };
        match (next_open, next_close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                i = o + open.len();
            }
            (_, Some(c)) => {
                depth -= 1;
                let gt = lower[c..].find('>').map(|g| c + g + 1).unwrap_or(s.len());
                if depth == 0 {
                    return gt;
                }
                i = gt;
            }
            (_, None) => return s.len(),
        }
    }
}

/// `html`, parsed and kept to the allow-list.
pub fn sanitize(html: &str) -> Vec<HNode> {
    let mut root = Vec::new();
    let mut stack: Vec<Open> = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    let s = html;
    while i < s.len() {
        let rest = &s[i..];
        let Some(lt) = rest.find('<') else {
            text.push_str(rest);
            break;
        };
        text.push_str(&rest[..lt]);
        i += lt;
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            push_text(&mut stack, &mut root, decode(&std::mem::take(&mut text)));
            i += rest.find("-->").map(|e| e + 3).unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            push_text(&mut stack, &mut root, decode(&std::mem::take(&mut text)));
            i += rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
            continue;
        }
        if let Some((name, n)) = rest.strip_prefix("</").and_then(tag_name) {
            push_text(&mut stack, &mut root, decode(&std::mem::take(&mut text)));
            i += 2 + n + rest[2 + n..].find('>').map(|e| e + 1).unwrap_or(rest.len() - 2 - n);
            if let Some(at) = stack.iter().rposition(|o| o.tag == name) {
                while stack.len() > at {
                    close(&mut stack, &mut root);
                }
            }
            continue;
        }
        let Some((name, n)) = tag_name(&rest[1..]) else {
            text.push('<');
            i += 1;
            continue;
        };
        push_text(&mut stack, &mut root, decode(&std::mem::take(&mut text)));
        let (attrs, self_closed, used) = attributes(&rest[1 + n..]);
        i += 1 + n + used;
        if DROPPED_WHOLE.contains(&name.as_str()) {
            if !VOID.contains(&name.as_str()) && !self_closed {
                i += skip_to_end(&s[i..], &name);
            }
            continue;
        }
        if !ALLOWED_TAGS.contains(&name.as_str()) {
            continue;
        }
        let attrs: Vec<(String, String)> =
            attrs.into_iter().filter(|(a, v)| allowed_attr(&name, a) && (!URL_ATTRS.contains(&a.as_str()) || url_ok(v))).collect();
        if VOID.contains(&name.as_str()) || self_closed {
            let node = HNode::Element { tag: name, attrs, children: Vec::new() };
            match stack.last_mut() {
                Some(p) => p.children.push(node),
                None => root.push(node),
            }
        } else {
            stack.push(Open { tag: name, attrs, children: Vec::new() });
        }
    }
    push_text(&mut stack, &mut root, decode(&text));
    while !stack.is_empty() {
        close(&mut stack, &mut root);
    }
    root
}

fn escape(text: &str, attr: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// The nodes as HTML text (escaped), for the tests and the console.
pub fn to_html(nodes: &[HNode]) -> String {
    let mut out = String::new();
    for n in nodes {
        match n {
            HNode::Text(t) => out.push_str(&escape(t, false)),
            HNode::Element { tag, attrs, children } => {
                out.push('<');
                out.push_str(tag);
                for (k, v) in attrs {
                    out.push_str(&format!(" {k}=\"{}\"", escape(v, true)));
                }
                out.push('>');
                if !VOID.contains(&tag.as_str()) {
                    out.push_str(&to_html(children));
                    out.push_str(&format!("</{tag}>"));
                }
            }
        }
    }
    out
}

/// Markdown as HTML (CommonMark with tables and strikethrough); raw HTML in
/// it passes through, to be sanitized where it is drawn.
pub fn markdown(text: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH));
    out
}

#[cfg(test)]
mod tests;
