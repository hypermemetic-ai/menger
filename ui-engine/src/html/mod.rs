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

/// Whether `attr` is kept on `tag`.
pub fn allowed_attr(tag: &str, attr: &str) -> bool {
    let _ = (tag, attr);
    todo!("UI-10: allowed_attr")
}

/// `html`, parsed and kept to the allow-list.
pub fn sanitize(html: &str) -> Vec<HNode> {
    let _ = html;
    todo!("UI-10: sanitize")
}

/// The nodes as HTML text (escaped), for the tests and the console.
pub fn to_html(nodes: &[HNode]) -> String {
    let _ = nodes;
    todo!("UI-10: to_html")
}

/// Markdown as HTML (CommonMark with tables and strikethrough); raw HTML in
/// it passes through, to be sanitized where it is drawn.
pub fn markdown(text: &str) -> String {
    let _ = text;
    todo!("UI-10: markdown")
}

#[cfg(test)]
mod tests;
