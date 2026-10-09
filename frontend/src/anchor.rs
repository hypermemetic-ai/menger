//! COMMENT-3: anchors — where a comment was left, from an element and back.
//!
//! A comment's anchor (COMMENT-2's `Anchor`) is `route` (the app page),
//! `path` (where on it), `label` and `excerpt` (what was there, in words)
//! and `ticket` (the ticket the element belongs to). This module makes one
//! from an element and finds the element again from one.
//!
//! **An element, as the anchor sees it**, is a [`Node`] chain: the element,
//! then each ancestor up to (not including) the scope's root — the app's
//! `#app`, or a framed body's `<body>`. A node is its tag, its attributes,
//! its classes, where it sits among its same-tag siblings (`nth` of `of`) and
//! a caption (a `<dd>`'s `<dt>`, a cell's column header, a section's
//! heading). The app reads one off its own DOM ([`chain_of`]); a frame's SDK
//! reads one off the body's and posts it (src/frame.rs) — the frame's origin
//! is opaque, so its DOM is the SDK's to read, and what is *decided* from the
//! chain is decided here, once, for both.
//!
//! **The path** is a CSS selector — the browser's own `querySelector` is
//! what finds the element again, in the app and in a frame. It prefers what
//! is stable over where the element sits ([`stable`]): its `id`, a
//! `data-field`, a `data-tag` naming a ticket or a shortcode (`id:DEMAND-1`,
//! `sc:RETURNS`; a status tag moves with the status), `data-ticket-id`, a
//! component's attributes; then
//! a stable ancestor with the element under it; and only then positions
//! (`:nth-of-type`) from the root. [`candidates`] lists them in that order
//! and [`choose`] takes the first the document says selects the element
//! alone. Inside a framed body the path is two selectors joined by
//! [`FRAME_SEP`]: the frame in the app (`iframe[data-ticket="DEMAND-2"]`),
//! then the element in the frame.
//!
//! **Found again** ([`still_there`]): the path selects an element — and,
//! inside a body (which only changes when the ticket is amended), where the
//! path is positional, that element still says what the excerpt says. An
//! anchor that is not found is "moved" (src/annotate.rs pins it to its
//! ticket's heading).
//!
//! What decides — the candidates, the choice, the label, the excerpt, the
//! ticket, the route, the check — is pure and pinned by native tests, which
//! hold every path to selecting its element in a model document.

use serde::Deserialize;
use wasm_bindgen::JsCast;

/// The longest excerpt an anchor keeps, in characters — "what was there, in
/// words", not the element's whole text.
pub(crate) const EXCERPT_MAX: usize = 140;

/// How much of an element's text a label quotes, in characters.
const LABEL_TEXT_MAX: usize = 40;

/// The longest attribute value a path is built on.
const VALUE_MAX: usize = 160;

/// The longest chain read: an element deeper than this under its root is
/// anchored from the deepest ancestor read.
pub(crate) const CHAIN_MAX: usize = 32;

/// Between a framed body's frame and the element in it: `iframe[data-ticket=
/// "DEMAND-2"] >> table > tbody > tr:nth-of-type(2)`. Never a CSS
/// combinator, so a path splits on it unambiguously.
pub(crate) const FRAME_SEP: &str = " >> ";

/// The app's root: every path in the app document is under it.
pub(crate) const APP_ROOT: &str = "#app";

/// A framed body's root.
pub(crate) const FRAME_ROOT: &str = "body";

/// One element: what it is and where it sits — read off the app's DOM by
/// [`chain_of`], or off a frame's by the SDK (the same fields, as JSON).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct Node {
    /// Lowercase.
    pub(crate) tag: String,
    /// Its attributes but `class` and `style`, in order.
    #[serde(default)]
    pub(crate) attrs: Vec<(String, String)>,
    #[serde(default)]
    pub(crate) classes: Vec<String>,
    /// Its place among its parent's children of its tag, from 1 …
    #[serde(default = "one")]
    pub(crate) nth: u32,
    /// … of this many.
    #[serde(default = "one")]
    pub(crate) of: u32,
    /// What names it on the page: a `<dd>`'s `<dt>`, a cell's column header,
    /// a section's heading.
    #[serde(default)]
    pub(crate) caption: Option<String>,
}

fn one() -> u32 {
    1
}

impl Node {
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c == class)
    }

    /// A custom element — a component instance (COMP-5).
    pub(crate) fn is_component(&self) -> bool {
        self.tag.starts_with(|c: char| c.is_ascii_lowercase()) && self.tag.contains('-')
    }
}

// ---------------------------------------------------------------------
// The path

/// A CSS identifier: `s` with every character but `[A-Za-z0-9_-]` escaped.
fn ident(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if c.is_ascii_graphic() {
            out.push('\\');
            out.push(c);
        } else {
            out.push_str(&format!("\\{:x} ", c as u32));
        }
    }
    out
}

/// A plain identifier — usable as `#id` and as an attribute name as is.
fn plain(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A double-quoted CSS string.
pub(crate) fn quote(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('"');
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' | '\x0c' => out.push_str("\\a "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The attributes a path is built on, in the order they are preferred, and
/// whether the selector names the tag with it.
const STABLE_ATTRS: [(&str, bool); 8] = [
    ("data-field", true),
    ("data-tag", false),
    ("data-menger-card", false),
    ("data-ticket-id", true),
    ("data-dag-id", true),
    ("data-wave-ref", true),
    ("data-ticket", true),
    ("name", true),
];

/// A component's attributes that name nothing about the instance.
const NOT_IDENTITY: [&str; 6] = ["class", "style", "slot", "part", "hidden", "tabindex"];

/// The selectors that pick `node` out by what it *is*, most stable first —
/// its id, a `data-field`, a `data-tag` naming a ticket or a shortcode (a
/// status, kind or severity tag moves with the ticket), `data-ticket-id`,
/// …, a component's attributes and its tag, an app link's route. Each
/// selects `node`; none is known to select it alone.
pub(crate) fn stable(node: &Node) -> Vec<String> {
    let tag = ident(&node.tag);
    let ok = |v: &str| !v.trim().is_empty() && v.len() <= VALUE_MAX;
    let mut out: Vec<String> = Vec::new();
    if let Some(id) = node.attr("id").filter(|v| ok(v)) {
        out.push(if plain(id) { format!("#{id}") } else { format!("{tag}[id={}]", quote(id)) });
    }
    for (name, tagged) in STABLE_ATTRS {
        let Some(v) = node.attr(name).filter(|v| ok(v)) else { continue };
        // A tag names a ticket (`id:DEMAND-1`) or a shortcode (`sc:RETURNS`)
        // for good; a status, kind or severity tag moves (src/tags.rs).
        if name == "data-tag" && !(v.starts_with("id:") || v.starts_with("sc:")) {
            continue;
        }
        if name == "data-ticket" && node.tag != "iframe" {
            continue;
        }
        let at = if tagged { tag.as_str() } else { "" };
        out.push(format!("{at}[{name}={}]", quote(v)));
    }
    if node.is_component() {
        for (name, v) in &node.attrs {
            if name == "id" || NOT_IDENTITY.contains(&name.as_str()) || name.starts_with("data-") || name.starts_with("aria-") {
                continue;
            }
            if plain(name) && ok(v) {
                out.push(format!("{tag}[{name}={}]", quote(v)));
            }
        }
        out.push(tag.clone());
    }
    if node.tag == "a" {
        if let Some(href) = node.attr("href").filter(|h| h.starts_with("#/") && ok(h)) {
            out.push(format!("a[href={}]", quote(href)));
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert(s.clone()));
    out
}

/// Where `node` sits among its siblings: its tag, and `:nth-of-type` when
/// it has siblings of that tag.
fn step(node: &Node) -> String {
    let tag = ident(&node.tag);
    if node.of > 1 {
        format!("{tag}:nth-of-type({})", node.nth.max(1))
    } else {
        tag
    }
}

/// `nodes` (an element first, its ancestors after) as child steps, the
/// outermost first: `tbody > tr:nth-of-type(2) > td`.
fn child_steps(nodes: &[Node]) -> String {
    nodes.iter().rev().map(step).collect::<Vec<_>>().join(" > ")
}

/// Every path that selects the chain's element, most stable first: its own
/// stable selectors; then, for each ancestor nearest first, each of the
/// ancestor's with the element's under it, and with the steps down to it;
/// and last the steps from `root` — which, by construction, selects the
/// element alone.
pub(crate) fn candidates(chain: &[Node], root: &str) -> Vec<String> {
    let chain = &chain[..chain.len().min(CHAIN_MAX)];
    let Some(el) = chain.first() else { return vec![root.to_string()] };
    let own = stable(el);
    let mut out: Vec<String> = own.clone();
    for i in 1..chain.len() {
        let below = child_steps(&chain[..i]);
        for a in stable(&chain[i]) {
            for e in &own {
                out.push(format!("{a} {e}"));
            }
            out.push(format!("{a} > {below}"));
        }
    }
    out.push(format!("{root} > {}", child_steps(chain)));
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert(s.clone()));
    out
}

/// The first candidate `alone` says selects the element and nothing else —
/// else the last, the steps from the root.
pub(crate) fn choose(candidates: &[String], mut alone: impl FnMut(&str) -> bool) -> String {
    candidates.iter().find(|c| alone(c)).or(candidates.last()).cloned().unwrap_or_default()
}

/// A path inside a framed body: the frame's selector in the app, and the
/// element's in the frame. `None` for a path in the app itself.
pub(crate) fn split(path: &str) -> (Option<&str>, &str) {
    match path.split_once(FRAME_SEP) {
        Some((frame, inner)) => (Some(frame), inner),
        None => (None, path),
    }
}

/// A frame's selector and the element's in it, as one path.
pub(crate) fn join(frame: &str, inner: &str) -> String {
    format!("{frame}{FRAME_SEP}{inner}")
}

/// Whether a selector finds its element by position rather than by what it
/// is: it steps by `:nth-of-type`, or names no id or attribute at all.
pub(crate) fn positional(selector: &str) -> bool {
    selector.contains(":nth-of-type(") || !(selector.contains('[') || selector.contains('#'))
}

// ---------------------------------------------------------------------
// What was there, in words

/// Whitespace collapsed, trimmed.
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters of `s`.
fn cut(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((at, _)) => &s[..at],
        None => s,
    }
}

/// The excerpt an element's text makes: its words, at most [`EXCERPT_MAX`]
/// characters of them.
pub(crate) fn excerpt_of(text: &str) -> String {
    cut(&words(text), EXCERPT_MAX).trim_end().to_string()
}

/// A short quote of `text` for a label — `None` when it has none.
fn quoted(text: &str) -> Option<String> {
    let w = words(text);
    if w.is_empty() {
        return None;
    }
    if w.chars().count() <= LABEL_TEXT_MAX {
        return Some(w);
    }
    Some(format!("{}…", cut(&w, LABEL_TEXT_MAX - 1).trim_end()))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// `data-field="recommended_option"` → `recommended option`.
fn humanize(s: &str) -> String {
    s.replace(['-', '_'], " ")
}

/// What kind of thing `node` is, in a word.
fn role_of(node: &Node) -> &'static str {
    let chipish = node.attr("data-tag").is_some()
        || ["code", "chip", "crumb", "tag", "status", "pill", "badge"].iter().any(|c| node.has_class(c));
    match node.tag.as_str() {
        "a" if chipish => "chip",
        "a" => "link",
        "button" => "button",
        "select" => "control",
        "option" => "option",
        "input" | "textarea" => "field",
        "tr" => "row",
        "td" | "th" => "cell",
        "table" => "table",
        "thead" => "table head",
        "tbody" => "table body",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
        "p" => "paragraph",
        "li" => "item",
        "ul" | "ol" | "dl" => "list",
        "img" | "svg" | "picture" => "image",
        "pre" => "code block",
        "code" | "kbd" => "code",
        "dd" => "value",
        "dt" => "label",
        "section" => "section",
        "article" => "article",
        "aside" => "panel",
        "nav" => "nav",
        "header" => "header",
        "main" => "page",
        "details" => "fold",
        "summary" => "fold's heading",
        "blockquote" => "quote",
        "iframe" => "body",
        "form" => "form",
        "label" => "label",
        "span" | "div" if chipish => "chip",
        "span" | "strong" | "em" | "b" | "i" | "small" => "text",
        "div" => "block",
        _ => "element",
    }
}

/// What a comment box and a hover label call the chain's element: a
/// component by its tag (`<m-ticket> “RETURNS-1”`); anything else by what
/// names it — its `aria-label`, its caption, its nearest ancestor's caption
/// (a chip in the Parent row: `Parent chip`), its `data-field` — and its
/// role, or by its role and its words (`Row “Support by layer”`).
pub(crate) fn label_of(chain: &[Node], text: &str) -> String {
    let Some(el) = chain.first() else { return "Page".to_string() };
    if el.is_component() {
        let tag = format!("<{}>", el.tag);
        return match quoted(text) {
            Some(t) => format!("{tag} “{t}”"),
            None => tag,
        };
    }
    let role = role_of(el);
    let name = el
        .attr("aria-label")
        .map(words)
        .filter(|n| !n.is_empty())
        .or_else(|| el.caption.as_deref().map(words).filter(|n| !n.is_empty()))
        .or_else(|| chain.iter().skip(1).take(3).find_map(|n| n.caption.as_deref().map(words).filter(|n| !n.is_empty())))
        .or_else(|| el.attr("data-field").map(humanize));
    match (name, quoted(text)) {
        // A cell is named by its column; its words say which row.
        (Some(n), Some(t)) if role == "cell" => format!("{} “{t}”", capitalize(&format!("{} {role}", cut(&n, LABEL_TEXT_MAX)))),
        (Some(n), _) => capitalize(&format!("{} {role}", cut(&n, LABEL_TEXT_MAX))),
        (None, Some(t)) => format!("{} “{t}”", capitalize(role)),
        (None, None) => capitalize(role),
    }
}

// ---------------------------------------------------------------------
// Whose it is, and where

/// The ticket the element belongs to: on a ticket's page, everything the
/// page draws is that ticket's; elsewhere the nearest `data-ticket-id` (a
/// board row, a nav row) or the frame of a ticket's body (`iframe
/// data-ticket`). `route` is [`crate::route_text`]'s.
pub(crate) fn ticket_of(route: &str, chain: &[Node]) -> Option<String> {
    if let Some(id) = route.strip_prefix("#/ticket/").filter(|id| !id.is_empty()) {
        if chain.iter().any(|n| n.attr("id") == Some("ticket-page")) {
            return Some(id.to_string());
        }
    }
    chain.iter().find_map(|n| {
        n.attr("data-ticket-id")
            .or_else(|| (n.tag == "iframe").then(|| n.attr("data-ticket")).flatten())
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    })
}

/// Whether an anchor still finds its element: the path selected one, whose
/// text is `text_now` — and, inside a framed body where the path goes by
/// position, that text still makes the excerpt (a body changes only when it
/// is amended, and a position then may name another element). `None`: the
/// path selected nothing.
pub(crate) fn still_there(path: &str, excerpt: &str, text_now: Option<&str>) -> bool {
    let Some(text) = text_now else { return false };
    match split(path) {
        (Some(_), inner) if positional(inner) && !excerpt.is_empty() => excerpt_of(text) == excerpt,
        _ => true,
    }
}

// ---------------------------------------------------------------------
// The app's DOM

/// A caption for `el`: a `<dd>`'s `<dt>`, a cell's column header, a
/// section's (or fold's) first heading.
fn caption_of(el: &web_sys::Element) -> Option<String> {
    let text = |e: &web_sys::Element| e.text_content().map(|t| cut(&words(&t), 60).to_string()).filter(|t| !t.is_empty());
    match el.tag_name().to_ascii_lowercase().as_str() {
        "dd" => el.previous_element_sibling().filter(|p| p.tag_name().eq_ignore_ascii_case("dt")).and_then(|dt| text(&dt)),
        "td" | "th" => {
            let row = el.parent_element()?;
            if row.parent_element().is_some_and(|p| p.tag_name().eq_ignore_ascii_case("thead")) {
                return None;
            }
            let at = (0..row.children().length()).position(|i| row.children().item(i).is_some_and(|c| c.is_same_node(Some(el))))?;
            let table = el.closest("table").ok().flatten()?;
            let head = table.query_selector("thead tr").ok().flatten()?;
            head.children().item(at as u32).and_then(|h| text(&h))
        }
        "section" | "article" | "aside" | "details" => {
            el.query_selector(":scope > h1, :scope > h2, :scope > h3, :scope > h4, :scope > summary").ok().flatten().and_then(|h| text(&h))
        }
        _ => None,
    }
}

/// One element as a [`Node`].
pub(crate) fn node_of(el: &web_sys::Element) -> Node {
    let tag = el.tag_name().to_ascii_lowercase();
    let mut attrs = Vec::new();
    for name in el.get_attribute_names().iter().filter_map(|n| n.as_string()).take(24) {
        if name != "class" && name != "style" {
            if let Some(v) = el.get_attribute(&name) {
                attrs.push((name, cut(&v, 300).to_string()));
            }
        }
    }
    let classes = (0..el.class_list().length().min(8)).filter_map(|i| el.class_list().item(i)).collect();
    let (mut nth, mut of) = (1, 0);
    if let Some(parent) = el.parent_element() {
        let kids = parent.children();
        for i in 0..kids.length() {
            if let Some(k) = kids.item(i) {
                if k.tag_name() == el.tag_name() {
                    of += 1;
                    if k.is_same_node(Some(el)) {
                        nth = of;
                    }
                }
            }
        }
    }
    Node { tag, attrs, classes, nth, of: of.max(1), caption: caption_of(el) }
}

/// The chain of an element in the app: it, then its ancestors up to (not
/// including) `#app` — `None` for an element outside `#app` (the annotation
/// layer itself, the document's body).
pub(crate) fn chain_of(el: &web_sys::Element) -> Option<Vec<Node>> {
    let mut out = Vec::new();
    let mut cur = Some(el.clone());
    while let Some(e) = cur {
        if e.id() == "app" {
            return Some(out);
        }
        if e.tag_name().eq_ignore_ascii_case("body") || out.len() >= CHAIN_MAX {
            return (out.len() >= CHAIN_MAX && el.closest(APP_ROOT).ok().flatten().is_some()).then_some(out);
        }
        out.push(node_of(&e));
        cur = e.parent_element();
    }
    None
}

/// Whether `selector` selects `el` and nothing else in the app document.
pub(crate) fn selects_alone(selector: &str, el: &web_sys::Element) -> bool {
    let Some(doc) = crate::dom::window().document() else { return false };
    match doc.query_selector_all(selector) {
        Ok(list) => list.length() == 1 && list.item(0).is_some_and(|n| n.is_same_node(Some(el))),
        Err(_) => false,
    }
}

/// The path of an element in the app — its chain's candidates, the first
/// that selects it alone.
pub(crate) fn path_in_app(el: &web_sys::Element, chain: &[Node]) -> String {
    choose(&candidates(chain, APP_ROOT), |sel| selects_alone(sel, el))
}

/// The element a path in the app selects.
pub(crate) fn find_in_app(path: &str) -> Option<web_sys::Element> {
    crate::dom::window().document()?.query_selector(path).ok().flatten()
}

/// The frame a framed path's first half selects — the one on screen
/// (`data-state="live"`) when a re-render has a second beside it.
pub(crate) fn find_frame(selector: &str) -> Option<web_sys::HtmlIFrameElement> {
    let list = crate::dom::window().document()?.query_selector_all(selector).ok()?;
    let frames: Vec<web_sys::HtmlIFrameElement> =
        (0..list.length()).filter_map(|i| list.item(i)).filter_map(|n| n.dyn_into::<web_sys::HtmlIFrameElement>().ok()).collect();
    frames.iter().find(|f| f.get_attribute("data-state").as_deref() == Some("live")).or(frames.first()).cloned()
}

/// The path of a frame in the app: `iframe[data-ticket="…"]` for a ticket's
/// body (whichever of its frames is on screen is the one); otherwise its
/// chain's candidates, as any element's — the frame's own step without its
/// position, since a re-render puts a second frame beside it for a moment.
pub(crate) fn frame_path(frame: &web_sys::Element) -> String {
    if let Some(t) = frame.get_attribute("data-ticket").filter(|t| !t.is_empty() && t.len() <= VALUE_MAX) {
        return format!("iframe[data-ticket={}]", quote(&t));
    }
    let Some(mut chain) = chain_of(frame) else { return "iframe".to_string() };
    if let Some(me) = chain.first_mut() {
        me.nth = 1;
        me.of = 1;
    }
    let cands = candidates(&chain, APP_ROOT);
    choose(&cands, |sel| {
        let Some(doc) = crate::dom::window().document() else { return false };
        doc.query_selector(sel).ok().flatten().is_some_and(|n| n.is_same_node(Some(frame)))
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // -- a model document, and the selectors this module writes, matched in it

    /// A model element: its node (tag, attributes, classes; its place and
    /// caption are worked out by the tree), its text, its children.
    #[derive(Clone, Debug)]
    pub(crate) struct El {
        tag: String,
        attrs: Vec<(String, String)>,
        classes: Vec<String>,
        text: String,
        kids: Vec<El>,
    }

    /// `tag#id.class[attr=value]…`, as a model element.
    pub(crate) fn el(spec: &str, kids: Vec<El>) -> El {
        let mut tag = String::new();
        let mut attrs = Vec::new();
        let mut classes = Vec::new();
        let mut rest = spec;
        let end = rest.find(['#', '.', '[']).unwrap_or(rest.len());
        tag.push_str(&rest[..end]);
        rest = &rest[end..];
        while !rest.is_empty() {
            match rest.as_bytes()[0] {
                b'#' | b'.' => {
                    let end = rest[1..].find(['#', '.', '[']).map_or(rest.len(), |e| e + 1);
                    let v = rest[1..end].to_string();
                    if rest.starts_with('#') {
                        attrs.push(("id".to_string(), v));
                    } else {
                        classes.push(v);
                    }
                    rest = &rest[end..];
                }
                _ => {
                    let end = rest.find(']').expect("]");
                    let (k, v) = rest[1..end].split_once('=').expect("attr=value");
                    attrs.push((k.to_string(), v.to_string()));
                    rest = &rest[end + 1..];
                }
            }
        }
        El { tag, attrs, classes, text: String::new(), kids }
    }

    /// `el` with `text` of its own.
    pub(crate) fn txt(mut e: El, text: &str) -> El {
        e.text = text.to_string();
        e
    }

    /// A model document: its elements flattened, each with its parent.
    pub(crate) struct Doc {
        els: Vec<(El, Option<usize>)>,
    }

    impl Doc {
        pub(crate) fn new(root: El) -> Self {
            let mut d = Doc { els: Vec::new() };
            d.add(root, None);
            d
        }

        fn add(&mut self, e: El, parent: Option<usize>) {
            let at = self.els.len();
            let kids = e.kids.clone();
            self.els.push((El { kids: Vec::new(), ..e }, parent));
            for k in kids {
                self.add(k, Some(at));
            }
        }

        fn kids(&self, i: usize) -> Vec<usize> {
            (0..self.els.len()).filter(|&j| self.els[j].1 == Some(i)).collect()
        }

        fn attr(&self, i: usize, name: &str) -> Option<&str> {
            self.els[i].0.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
        }

        /// The element with this id.
        pub(crate) fn by_id(&self, id: &str) -> usize {
            (0..self.els.len()).find(|&i| self.attr(i, "id") == Some(id)).unwrap_or_else(|| panic!("no #{id}"))
        }

        /// The first element whose attribute `name` is `value`.
        pub(crate) fn by_attr(&self, name: &str, value: &str) -> usize {
            (0..self.els.len()).find(|&i| self.attr(i, name) == Some(value)).unwrap_or_else(|| panic!("no [{name}={value}]"))
        }

        /// All of an element's text, its descendants' included.
        pub(crate) fn text(&self, i: usize) -> String {
            let mut t = self.els[i].0.text.clone();
            for k in self.kids(i) {
                t.push(' ');
                t.push_str(&self.text(k));
            }
            t
        }

        fn node(&self, i: usize) -> Node {
            let (e, parent) = &self.els[i];
            let (mut nth, mut of) = (1, 0);
            if let Some(p) = parent {
                for k in self.kids(*p) {
                    if self.els[k].0.tag == e.tag {
                        of += 1;
                        if k == i {
                            nth = of;
                        }
                    }
                }
            }
            let caption = match e.tag.as_str() {
                "dd" => parent.and_then(|p| {
                    let kids = self.kids(p);
                    let at = kids.iter().position(|&k| k == i)?;
                    let prev = *kids.get(at.checked_sub(1)?)?;
                    (self.els[prev].0.tag == "dt").then(|| words(&self.text(prev)))
                }),
                _ => None,
            };
            Node { tag: e.tag.clone(), attrs: e.attrs.clone(), classes: e.classes.clone(), nth, of: of.max(1), caption }
        }

        /// The chain of element `i` up to (not including) the first
        /// ancestor `root` names (`#app`, or `body`).
        pub(crate) fn chain(&self, i: usize, root: &str) -> Vec<Node> {
            let mut out = Vec::new();
            let mut cur = Some(i);
            while let Some(c) = cur {
                if self.is_root(c, root) {
                    return out;
                }
                out.push(self.node(c));
                cur = self.els[c].1;
            }
            panic!("{i} is not under {root}")
        }

        fn is_root(&self, i: usize, root: &str) -> bool {
            match root.strip_prefix('#') {
                Some(id) => self.attr(i, "id") == Some(id),
                None => self.els[i].0.tag == root,
            }
        }

        /// Every element `selector` selects, in document order.
        pub(crate) fn select(&self, selector: &str) -> Vec<usize> {
            let parts = parse(selector);
            (0..self.els.len()).filter(|&i| self.matches(i, &parts)).collect()
        }

        fn matches(&self, i: usize, parts: &[(Comb, Compound)]) -> bool {
            let Some(((comb, last), rest)) = parts.split_last() else { return true };
            if !self.compound(i, last) {
                return false;
            }
            if rest.is_empty() {
                return true;
            }
            match comb {
                Comb::Child => self.els[i].1.is_some_and(|p| self.matches(p, rest)),
                Comb::Desc => {
                    let mut p = self.els[i].1;
                    while let Some(a) = p {
                        if self.matches(a, rest) {
                            return true;
                        }
                        p = self.els[a].1;
                    }
                    false
                }
                Comb::None => unreachable!("only the first part has none"),
            }
        }

        fn compound(&self, i: usize, c: &Compound) -> bool {
            let n = self.node(i);
            c.tag.as_ref().is_none_or(|t| *t == n.tag)
                && c.attrs.iter().all(|(k, v)| n.attr(k) == Some(v.as_str()))
                && c.nth.is_none_or(|k| k == n.nth)
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Comb {
        None,
        Child,
        Desc,
    }

    #[derive(Debug, Default)]
    struct Compound {
        tag: Option<String>,
        attrs: Vec<(String, String)>,
        nth: Option<u32>,
    }

    /// The selectors this module writes: compounds of a tag, `#id`,
    /// `[name="value"]` and `:nth-of-type(n)`, joined by ` ` and ` > `.
    fn parse(sel: &str) -> Vec<(Comb, Compound)> {
        let b: Vec<char> = sel.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        let mut comb = Comb::None;
        while i < b.len() {
            let mut c = Compound::default();
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '-' || b[i] == '_' || b[i] == '\\') {
                i += if b[i] == '\\' { 2 } else { 1 };
            }
            if i > start {
                c.tag = Some(b[start..i].iter().filter(|c| **c != '\\').collect());
            }
            loop {
                match b.get(i) {
                    Some('#') => {
                        let s = i + 1;
                        i = s;
                        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '-' || b[i] == '_') {
                            i += 1;
                        }
                        c.attrs.push(("id".into(), b[s..i].iter().collect()));
                    }
                    Some('[') => {
                        let s = i + 1;
                        while b[i] != '=' {
                            i += 1;
                        }
                        let name: String = b[s..i].iter().collect();
                        assert_eq!(b[i + 1], '"', "{sel}");
                        i += 2;
                        let mut v = String::new();
                        while b[i] != '"' {
                            if b[i] == '\\' {
                                i += 1;
                            }
                            v.push(b[i]);
                            i += 1;
                        }
                        assert_eq!(b[i + 1], ']', "{sel}");
                        i += 2;
                        c.attrs.push((name, v));
                    }
                    Some(':') => {
                        let rest: String = b[i..].iter().collect();
                        let inner = rest.strip_prefix(":nth-of-type(").expect(":nth-of-type");
                        let close = inner.find(')').unwrap();
                        c.nth = Some(inner[..close].parse().unwrap());
                        i += ":nth-of-type(".len() + close + 1;
                    }
                    _ => break,
                }
            }
            out.push((comb, c));
            if i >= b.len() {
                break;
            }
            if b[i..].starts_with(&[' ', '>', ' ']) {
                comb = Comb::Child;
                i += 3;
            } else if b[i] == ' ' {
                comb = Comb::Desc;
                i += 1;
            } else {
                panic!("unexpected {:?} at {i} in {sel}", b[i]);
            }
        }
        out
    }

    /// The path a document gives element `i` under `root`: the first
    /// candidate that selects it alone.
    fn path(d: &Doc, i: usize, root: &str) -> String {
        choose(&candidates(&d.chain(i, root), root), |s| d.select(s) == [i])
    }

    /// The app, as the ticket page DEMAND-2 draws it — its crumb, its title,
    /// its properties (the Parent row's chip DEMAND-1, the status chip whose
    /// tag moves with the status), its body's frame — and the nav beside it,
    /// which names DEMAND-1 too.
    pub(crate) fn app() -> Doc {
        Doc::new(el(
            "body",
            vec![el(
                "div#app",
                vec![
                    el(
                        "nav#nav",
                        vec![el(
                            "ul",
                            vec![
                                el("li", vec![txt(el("a.row[data-ticket-id=DEMAND-1]", vec![]), "DEMAND-1 demands")]),
                                el("li", vec![txt(el("a.row[data-ticket-id=DEMAND-2]", vec![]), "DEMAND-2 the chip")]),
                            ],
                        )],
                    ),
                    el(
                        "div.content",
                        vec![
                            el("header", vec![txt(el("button#annotate", vec![]), "Annotate"), txt(el("a.hlink", vec![]), "waves")]),
                            el(
                                "main",
                                vec![el(
                                    "div#ticket-page",
                                    vec![
                                        el("p.crumbs", vec![txt(el("a.crumb[data-ticket-id=DEMAND-1]", vec![]), "DEMAND-1")]),
                                        el(
                                            "article.ticket",
                                            vec![
                                                el(
                                                    "h2[data-ticket-id=DEMAND-2]",
                                                    vec![txt(el("span.id.code[data-tag=id:DEMAND-2]", vec![]), "DEMAND-2"), txt(el("span.t", vec![]), "the chip")],
                                                ),
                                                el("div.mframe", vec![el("iframe.mframe-doc[data-ticket=DEMAND-2][data-state=live]", vec![])]),
                                                el("section.kidsec", vec![txt(el("h3", vec![]), "Children"), txt(el("p.none", vec![]), "None")]),
                                            ],
                                        ),
                                        el(
                                            "aside.props",
                                            vec![el(
                                                "dl.meta",
                                                vec![
                                                    el("div", vec![txt(el("dt", vec![]), "Status"), txt(el("dd#t-status.status[data-tag=status:Pending]", vec![]), "Pending")]),
                                                    el(
                                                        "div",
                                                        vec![
                                                            txt(el("dt", vec![]), "Parent"),
                                                            el(
                                                                "dd#t-parent",
                                                                vec![txt(el("a.crumb.code[data-ticket-id=DEMAND-1][data-tag=id:DEMAND-1][href=#/ticket/DEMAND-1]", vec![]), "DEMAND-1")],
                                                            ),
                                                        ],
                                                    ),
                                                    el("div", vec![txt(el("dt", vec![]), "Commits"), el("dd#t-commits", vec![txt(el("code.commit", vec![]), "f70eb2f"), txt(el("code.commit", vec![]), "d923fb9")])]),
                                                ],
                                            )],
                                        ),
                                    ],
                                )],
                            ),
                        ],
                    ),
                ],
            )],
        ))
    }

    /// A framed body: a table with a header and three rows (the second a
    /// `m-ticket` chip), a decision with fields, a paragraph.
    pub(crate) fn body() -> Doc {
        let row = |a: &str, b: &str| el("tr", vec![txt(el("td", vec![]), a), txt(el("td", vec![]), b)]);
        Doc::new(el(
            "body",
            vec![
                txt(el("h2", vec![]), "Support by layer"),
                el(
                    "table",
                    vec![
                        el("thead", vec![el("tr", vec![txt(el("th", vec![]), "layer"), txt(el("th", vec![]), "ticket")])]),
                        el(
                            "tbody",
                            vec![
                                row("Support by layer", "RETURNS-1"),
                                el("tr", vec![txt(el("td", vec![]), "Chips"), el("td", vec![txt(el("m-ticket[ticket=RETURNS-2]", vec![]), "RETURNS-2")])]),
                                row("Docs", "RETURNS-3"),
                            ],
                        ),
                    ],
                ),
                el(
                    "m-decision#landing[data-field=decision]",
                    vec![
                        txt(el("m-question[data-field=question]", vec![]), "Archive or delete?"),
                        el("m-option[data-field=option][name=Archive][value=archive]", vec![]),
                        el("m-option[data-field=option][name=Delete][value=delete]", vec![]),
                    ],
                ),
                txt(el("p", vec![]), "A closing paragraph."),
            ],
        ))
    }

    /// COMMENT-3: every element of the app and of a body has a path, and the
    /// path selects that element and nothing else — the anchor from the
    /// element, and back.
    #[test]
    fn every_path_selects_its_element_alone() {
        for (d, root) in [(app(), APP_ROOT), (body(), FRAME_ROOT)] {
            let under = d.select(root)[0];
            for i in 0..d.els.len() {
                let mut up = Some(i);
                let mut inside = false;
                while let Some(u) = up {
                    if u == under && u != i {
                        inside = true;
                    }
                    up = d.els[u].1;
                }
                if !inside {
                    continue;
                }
                let p = path(&d, i, root);
                assert_eq!(d.select(&p), [i], "{p}");
            }
        }
    }

    /// The path prefers what an element is over where it sits: an id; a
    /// ticket's tag (the COMMENT-1 example's Parent chip — while it is the
    /// only one); a stable ancestor with the element under it once the tag
    /// is not alone; a component's attributes; a `data-field` under its
    /// input; and a status tag never (it moves with the status).
    #[test]
    fn a_path_prefers_what_an_element_is_over_where_it_sits() {
        let d = app();
        assert_eq!(path(&d, d.by_id("t-status"), APP_ROOT), "#t-status", "an id, not the status tag");
        assert_eq!(path(&d, d.by_id("annotate"), APP_ROOT), "#annotate");
        let chip = d.by_attr("data-tag", "id:DEMAND-1");
        assert_eq!(path(&d, chip, APP_ROOT), "[data-tag=\"id:DEMAND-1\"]", "COMMENT-1's example, in the app's spelling of a ticket tag");
        // The nav and the crumb name DEMAND-1 too, by `data-ticket-id`: not
        // the chip's tag, which is still alone.
        assert_eq!(d.select("a[data-ticket-id=\"DEMAND-1\"]").len(), 3);
        // A second chip with the same tag: the stable ancestor takes it.
        // (The crumb above the title gets the chip's tag and its link.)
        let mut twice = app();
        twice.els.iter_mut().filter(|(e, _)| e.classes.contains(&"crumb".to_string()) && !e.classes.contains(&"code".to_string())).for_each(|(e, _)| {
            e.attrs.push(("data-tag".into(), "id:DEMAND-1".into()));
            e.attrs.push(("href".into(), "#/ticket/DEMAND-1".into()));
        });
        let chip = twice.select("#t-parent > a")[0];
        assert_eq!(path(&twice, chip, APP_ROOT), "#t-parent [data-tag=\"id:DEMAND-1\"]");
        // A commit has nothing of its own: under its row, by position.
        let commit = d.select("code")[1];
        assert_eq!(path(&d, commit, APP_ROOT), "#t-commits > code:nth-of-type(2)");
        // The body's frame: its ticket.
        let frame = d.by_attr("data-ticket", "DEMAND-2");
        assert_eq!(path(&d, frame, APP_ROOT), "iframe[data-ticket=\"DEMAND-2\"]");

        let b = body();
        let chip = b.by_attr("ticket", "RETURNS-2");
        assert_eq!(path(&b, chip, FRAME_ROOT), "m-ticket[ticket=\"RETURNS-2\"]", "a component by its attributes");
        let q = b.by_attr("data-field", "question");
        assert_eq!(path(&b, q, FRAME_ROOT), "m-question[data-field=\"question\"]");
        let delete = b.by_attr("name", "Delete");
        assert_eq!(path(&b, delete, FRAME_ROOT), "m-option[name=\"Delete\"]", "data-field is not alone: the name is");
        assert_eq!(path(&b, b.by_id("landing"), FRAME_ROOT), "#landing");
        // A row has nothing of its own: its position from the root.
        let row = b.select("tr")[1];
        assert_eq!(path(&b, row, FRAME_ROOT), "body > table > tbody > tr:nth-of-type(1)");
        assert!(positional("body > table > tbody > tr:nth-of-type(1)"));
        assert!(!positional("m-ticket[ticket=\"RETURNS-2\"]") && !positional("#landing"));
        assert!(positional("body > p"), "steps alone are positions");
    }

    #[test]
    fn a_framed_path_is_the_frame_then_the_element() {
        let p = join("iframe[data-ticket=\"DEMAND-2\"]", "body > table > tbody > tr:nth-of-type(2)");
        assert_eq!(p, "iframe[data-ticket=\"DEMAND-2\"] >> body > table > tbody > tr:nth-of-type(2)");
        assert_eq!(split(&p), (Some("iframe[data-ticket=\"DEMAND-2\"]"), "body > table > tbody > tr:nth-of-type(2)"));
        assert_eq!(split("#t-status"), (None, "#t-status"));
        // A value is quoted whole, whatever it holds.
        assert_eq!(quote("a \"b\" \\c\nd"), "\"a \\\"b\\\" \\\\c\\a d\"");
        let n = Node { tag: "m-x".into(), attrs: vec![("label".into(), "say \"hi\"".into())], ..Node::default() };
        assert_eq!(stable(&n), ["m-x[label=\"say \\\"hi\\\"\"]", "m-x"]);
        let weird = Node { tag: "div".into(), attrs: vec![("id".into(), "1 two".into())], ..Node::default() };
        assert_eq!(stable(&weird), ["div[id=\"1 two\"]"]);
    }

    /// The label names what was there; the excerpt is its words.
    #[test]
    fn labels_and_excerpts_say_what_was_there() {
        let d = app();
        let chip = d.by_attr("data-tag", "id:DEMAND-1");
        let at = |i: usize| label_of(&d.chain(i, APP_ROOT), &d.text(i));
        assert_eq!(at(chip), "Parent chip", "COMMENT-1's example");
        assert_eq!(at(d.by_id("t-status")), "Status value");
        assert_eq!(at(d.by_id("annotate")), "Button “Annotate”");
        let h2 = d.select("h2")[0];
        assert_eq!(at(h2), "Heading “DEMAND-2 the chip”");
        let b = body();
        let at = |i: usize| label_of(&b.chain(i, FRAME_ROOT), &b.text(i));
        assert_eq!(at(b.select("tr")[1]), "Row “Support by layer RETURNS-1”");
        assert_eq!(at(b.by_attr("ticket", "RETURNS-2")), "<m-ticket> “RETURNS-2”");
        assert_eq!(at(b.by_attr("data-field", "question")), "<m-question> “Archive or delete?”");
        assert_eq!(at(b.select("table")[0]), "Table “layer ticket Support by layer RETURNS-1…”");
        // A cell: its column, and its words (which row).
        let mut cell = b.chain(b.select("td")[0], FRAME_ROOT);
        cell[0].caption = Some("layer".into());
        assert_eq!(label_of(&cell, "Support by layer"), "Layer cell “Support by layer”");
        assert_eq!(label_of(&[Node { tag: "div".into(), ..Node::default() }], "  "), "Block");
        assert_eq!(label_of(&[], ""), "Page");

        assert_eq!(excerpt_of("  DEMAND-1 \n "), "DEMAND-1");
        let long = "word ".repeat(100);
        let e = excerpt_of(&long);
        assert_eq!(e.chars().count(), EXCERPT_MAX - 1, "cut at the bound, the trailing space trimmed");
        assert!(long.starts_with(&e));
        assert_eq!(excerpt_of(&"é".repeat(300)).chars().count(), EXCERPT_MAX, "on a character boundary");
    }

    /// Whose an element is: the ticket page's ticket for anything it draws,
    /// else a row's `data-ticket-id`, else a body frame's ticket.
    #[test]
    fn an_element_belongs_to_its_page_or_its_row() {
        let d = app();
        let chip = d.by_attr("data-tag", "id:DEMAND-1");
        assert_eq!(ticket_of("#/ticket/DEMAND-2", &d.chain(chip, APP_ROOT)).as_deref(), Some("DEMAND-2"), "the page's, not the chip's");
        let nav = d.by_attr("data-ticket-id", "DEMAND-1");
        assert_eq!(ticket_of("#/ticket/DEMAND-2", &d.chain(nav, APP_ROOT)).as_deref(), Some("DEMAND-1"), "the nav is not the page");
        let header = d.by_id("annotate");
        assert_eq!(ticket_of("#/ticket/DEMAND-2", &d.chain(header, APP_ROOT)), None);
        assert_eq!(ticket_of("#/", &d.chain(chip, APP_ROOT)).as_deref(), Some("DEMAND-1"), "off the page, its own row's");
        let frame = d.by_attr("data-ticket", "DEMAND-2");
        assert_eq!(ticket_of("#/waves/DEMAND", &d.chain(frame, APP_ROOT)).as_deref(), Some("DEMAND-2"), "a body's frame");
    }

    /// Found again: the path selects something — inside a body, by
    /// position, something that still says the excerpt.
    #[test]
    fn an_anchor_is_found_while_its_element_is_there() {
        let row = "iframe[data-ticket=\"R-1\"] >> body > table > tbody > tr:nth-of-type(2)";
        assert!(still_there(row, "Chips RETURNS-2", Some(" Chips\n RETURNS-2 ")));
        assert!(!still_there(row, "Chips RETURNS-2", Some("Docs RETURNS-3")), "the body changed under the position");
        assert!(!still_there(row, "Chips RETURNS-2", None), "nothing there");
        let chip = "iframe[data-ticket=\"R-1\"] >> m-ticket[ticket=\"RETURNS-2\"]";
        assert!(still_there(chip, "RETURNS-2", Some("RETURNS-2 (Ready)")), "what it is, whatever it says now");
        assert!(still_there("#t-status", "Pending", Some("Ready")), "the app's elements say what is live");
        assert!(still_there(row, "", Some("anything")), "no excerpt to hold it to");
    }
}
