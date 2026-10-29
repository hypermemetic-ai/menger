//! The template subset (UI-4), parsed by hand with an explicit stack — no
//! recursion, so a deep source costs memory and fuel, never the call stack:
//! elements with static or `{expr}` attributes, `on:<event>={handler}`,
//! text, `{expr}`, `{#if}…{:else}…{/if}`, `{#each expr as x (key)}…{/each}`,
//! `<component-name prop={expr}/>` and `<slot name="…" prop={expr}/>`.
//! Anything else is refused at its line. The one `<script>` is cut out here
//! and handed back with where it starts.

use std::collections::BTreeMap;

use crate::json::{Arena, Id};
use crate::lines::Lines;
use crate::Refusal;

/// Deeper than this is refused (the contract allows it; fuel is the backstop).
pub const MAX_DEPTH: usize = 1000;

/// The `<script>`'s text and the byte offset in the file where it starts.
pub struct Script<'a> {
    pub at: usize,
    pub text: &'a str,
}

pub struct Template<'a> {
    /// The top-level nodes, an array.
    pub tree: Id,
    pub script: Option<Script<'a>>,
    /// Every `on:<event>={handler}`: the handler's name and its line.
    pub handlers: Vec<(String, u32)>,
    /// Every `{#each}`'s expression leaf, to be read as `.rows` when it is a
    /// bare live.
    pub each_exprs: Vec<(Id, String)>,
}

const ELEMENTS: &[&str] = &[
    "a", "abbr", "address", "area", "article", "aside", "audio", "b", "bdi", "bdo", "blockquote", "br", "button",
    "canvas", "caption", "cite", "code", "col", "colgroup", "data", "datalist", "dd", "del", "details", "dfn",
    "dialog", "div", "dl", "dt", "em", "embed", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2",
    "h3", "h4", "h5", "h6", "header", "hgroup", "hr", "i", "img", "input", "ins", "kbd", "label", "legend", "li",
    "main", "mark", "menu", "meter", "nav", "ol", "optgroup", "option", "output", "p", "picture", "pre",
    "progress", "q", "s", "samp", "search", "section", "select", "small", "source", "span", "strong", "sub",
    "summary", "sup", "table", "tbody", "td", "textarea", "tfoot", "th", "thead", "time", "tr", "track", "u",
    "ul", "var", "video", "wbr",
];
const VOID: &[&str] = &["area", "br", "col", "embed", "hr", "img", "input", "source", "track", "wbr"];

enum Kind {
    Root,
    El { tag: String, attrs: Vec<(String, Id)>, on: Vec<(String, Id)> },
    Component { tag: String, props: Vec<(String, Id)> },
    Slot { name: String, props: Vec<(String, Id)> },
    If { cond: String, then: Option<Vec<Id>> },
    Each { expr: Id, as_: String, key: Option<String> },
}

struct Frame {
    kind: Kind,
    line: u32,
    children: Vec<Id>,
    /// Section names drawn in this frame (its current arm, for an if).
    slots: BTreeMap<String, u32>,
    /// An if's then-arm's names, once `{:else}` is reached.
    then_slots: BTreeMap<String, u32>,
}

impl Frame {
    fn new(kind: Kind, line: u32) -> Self {
        Frame { kind, line, children: Vec::new(), slots: BTreeMap::new(), then_slots: BTreeMap::new() }
    }
    /// How the frame is named in a refusal.
    fn opening(&self) -> String {
        match &self.kind {
            Kind::Root => "the template".into(),
            Kind::El { tag, .. } | Kind::Component { tag, .. } => format!("<{tag}>"),
            Kind::Slot { .. } => "<slot>".into(),
            Kind::If { .. } => "{#if}".into(),
            Kind::Each { .. } => "{#each}".into(),
        }
    }
}

struct P<'a, 'b> {
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    lines: &'b Lines,
    j: &'b mut Arena,
    stack: Vec<Frame>,
    script: Option<Script<'a>>,
    handlers: Vec<(String, u32)>,
    each_exprs: Vec<(Id, String)>,
    text_from: Option<usize>,
}

fn refuse<T>(line: u32, message: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal { line, message: message.into() })
}

fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

pub fn parse<'a>(src: &'a str, lines: &Lines, j: &mut Arena) -> Result<Template<'a>, Refusal> {
    let mut p = P {
        src,
        b: src.as_bytes(),
        pos: 0,
        lines,
        j,
        stack: vec![Frame::new(Kind::Root, 1)],
        script: None,
        handlers: Vec::new(),
        each_exprs: Vec::new(),
        text_from: None,
    };
    p.run()?;
    let tree = p.j.arr(std::mem::take(&mut p.stack[0].children));
    Ok(Template { tree, script: p.script, handlers: p.handlers, each_exprs: p.each_exprs })
}

impl<'a, 'b> P<'a, 'b> {
    fn line(&self, at: usize) -> u32 {
        self.lines.line(at)
    }
    fn at(&self, s: &str) -> bool {
        self.b[self.pos..].starts_with(s.as_bytes())
    }

    fn run(&mut self) -> Result<(), Refusal> {
        while self.pos < self.b.len() {
            let c = self.b[self.pos];
            let next = self.b.get(self.pos + 1).copied().unwrap_or(0);
            if c == b'<' && (next.is_ascii_alphabetic() || next == b'/' || self.at("<!--")) {
                self.flush_text()?;
                if self.at("<!--") {
                    self.comment()?;
                } else if next == b'/' {
                    self.close_tag()?;
                } else {
                    self.open_tag()?;
                }
            } else if c == b'{' {
                self.flush_text()?;
                self.brace()?;
            } else {
                self.text_from.get_or_insert(self.pos);
                self.pos += 1;
            }
        }
        self.flush_text()?;
        if self.stack.len() > 1 {
            let f = self.stack.last().expect("a frame");
            return refuse(f.line, format!("{} opened here is never closed", f.opening()));
        }
        Ok(())
    }

    fn flush_text(&mut self) -> Result<(), Refusal> {
        let Some(from) = self.text_from.take() else { return Ok(()) };
        let text = &self.src[from..self.pos];
        if text.trim().is_empty() && text.contains('\n') {
            return Ok(());
        }
        let t = self.j.str(text);
        let node = self.j.obj(vec![("text".into(), t)]);
        self.add(node, from)
    }

    /// A finished node into the innermost frame.
    fn add(&mut self, node: Id, at: usize) -> Result<(), Refusal> {
        let line = self.line(at);
        let top = self.stack.last_mut().expect("the root");
        match top.kind {
            Kind::Component { ref tag, .. } => refuse(line, format!("<{tag}> takes no children: a component is <{tag} prop={{…}}/>")),
            Kind::Slot { .. } => refuse(line, "a <slot> takes no children: <slot name=\"…\" prop={…}/>"),
            _ => {
                top.children.push(node);
                Ok(())
            }
        }
    }

    fn push(&mut self, frame: Frame) -> Result<(), Refusal> {
        if self.stack.len() > MAX_DEPTH {
            return refuse(frame.line, format!("nesting deeper than {MAX_DEPTH} levels"));
        }
        // Text inside a component or a slot is refused when it is added; a
        // frame opened inside one is refused here.
        if let Some(top) = self.stack.last() {
            match &top.kind {
                Kind::Component { tag, .. } => return refuse(frame.line, format!("<{tag}> takes no children: a component is <{tag} prop={{…}}/>")),
                Kind::Slot { .. } => return refuse(frame.line, "a <slot> takes no children: <slot name=\"…\" prop={…}/>"),
                _ => {}
            }
        }
        self.stack.push(frame);
        Ok(())
    }

    /// Pop the innermost frame into a node, its sections into its parent.
    fn pop(&mut self) -> Result<(), Refusal> {
        let f = self.stack.pop().expect("a frame above the root");
        let j = &mut *self.j;
        let mut names = f.then_slots;
        for (n, l) in f.slots {
            names.entry(n).or_insert(l);
        }
        let node = match f.kind {
            Kind::Root => unreachable!("the root is never popped"),
            Kind::El { tag, attrs, on } => {
                let mut e = vec![("el".to_string(), j.str(tag))];
                if !attrs.is_empty() {
                    e.push(("attrs".into(), j.obj(attrs)));
                }
                if !on.is_empty() {
                    e.push(("on".into(), j.obj(on)));
                }
                e.push(("children".into(), j.arr(f.children)));
                j.obj(e)
            }
            Kind::Component { tag, props } => {
                let (t, p) = (j.str(tag), j.obj(props));
                j.obj(vec![("component".into(), t), ("props".into(), p)])
            }
            Kind::Slot { name, props } => {
                let (s, p) = (j.str(name), j.obj(props));
                j.obj(vec![("slot".into(), s), ("props".into(), p)])
            }
            Kind::If { cond, then } => {
                let c = j.expr(cond);
                let (then, els) = match then {
                    Some(then) => (then, f.children),
                    None => (f.children, Vec::new()),
                };
                let (t, e) = (j.arr(then), j.arr(els));
                j.obj(vec![("if".into(), c), ("then".into(), t), ("else".into(), e)])
            }
            Kind::Each { expr, as_, key } => {
                let mut spec = vec![("expr".to_string(), expr), ("as".into(), j.str(as_))];
                if let Some(k) = key {
                    spec.push(("key".into(), j.str(k)));
                }
                let (s, body) = (j.obj(spec), j.arr(f.children));
                j.obj(vec![("each".into(), s), ("body".into(), body)])
            }
        };
        let parent = self.stack.last_mut().expect("the root");
        parent.children.push(node);
        for (n, l) in names {
            note_slot(parent, n, l)?;
        }
        Ok(())
    }

    fn comment(&mut self) -> Result<(), Refusal> {
        let start = self.pos;
        match self.src[start + 4..].find("-->") {
            Some(i) => {
                self.pos = start + 4 + i + 3;
                Ok(())
            }
            None => refuse(self.line(start), "<!-- opened here is never closed"),
        }
    }

    fn name_at(&self, from: usize) -> &'a str {
        let src: &'a str = self.src;
        let end = self.b[from..]
            .iter()
            .position(|c| !(c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_' || *c == b':' || *c == b'.'))
            .map_or(self.b.len(), |i| from + i);
        &src[from..end]
    }

    fn skip_ws(&mut self) {
        while self.pos < self.b.len() && self.b[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// `{` at `self.pos` → its expression's text (trimmed) and the offset
    /// after its `}`; strings and nested braces are skipped.
    fn braced(&self, open: usize) -> Result<(&'a str, usize), Refusal> {
        let src: &'a str = self.src;
        let b = self.b;
        let mut depth = 0usize;
        let mut i = open;
        while i < b.len() {
            match b[i] {
                q @ (b'"' | b'\'' | b'`') => {
                    i += 1;
                    while i < b.len() && b[i] != q {
                        if b[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok((src[open + 1..i].trim(), i + 1));
                    }
                }
                _ => {}
            }
            i += 1;
        }
        refuse(self.line(open), "{ opened here is never closed")
    }

    fn open_tag(&mut self) -> Result<(), Refusal> {
        let start = self.pos;
        let line = self.line(start);
        let tag = self.name_at(start + 1);
        self.pos = start + 1 + tag.len();
        if tag == "script" {
            return self.script(start);
        }
        enum Is {
            El,
            Component,
            Slot,
        }
        let is = if tag == "slot" {
            Is::Slot
        } else if tag.contains('-') && tag.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-') {
            Is::Component
        } else if ELEMENTS.contains(&tag) {
            Is::El
        } else {
            return refuse(line, format!("unknown tag <{tag}>: not an HTML element of the subset, and a component's name holds a hyphen"));
        };
        // Attributes.
        let mut attrs: Vec<(String, Id)> = Vec::new();
        let mut on: Vec<(String, Id)> = Vec::new();
        let mut slot_name: Option<String> = None;
        let self_closing;
        loop {
            self.skip_ws();
            if self.pos >= self.b.len() {
                return refuse(line, format!("<{tag} opened here is never closed with >"));
            }
            if self.at("/>") {
                self.pos += 2;
                self_closing = true;
                break;
            }
            if self.at(">") {
                self.pos += 1;
                self_closing = false;
                break;
            }
            let at = self.pos;
            let aline = self.line(at);
            if self.b[at] == b'{' {
                return refuse(aline, format!("<{tag} {{…}}>: an attribute is name={{expr}}, never {{…}} alone"));
            }
            let name = self.name_at(at);
            if name.is_empty() {
                return refuse(aline, format!("<{tag}>: `{}` is not an attribute", self.b[at] as char));
            }
            self.pos = at + name.len();
            // The value: "…", '…', {expr}, bare, or none.
            let value: Option<Result<String, String>> = if self.at("=") {
                self.pos += 1;
                let v0 = self.pos;
                match self.b.get(v0) {
                    Some(&q @ (b'"' | b'\'')) => {
                        let end = self.src[v0 + 1..].find(q as char).map(|i| v0 + 1 + i);
                        let Some(end) = end else { return refuse(aline, format!("{name}= opened here is never closed")) };
                        let text = &self.src[v0 + 1..end];
                        if text.contains('{') {
                            return refuse(aline, format!("{name}=\"{text}\": an attribute is a string or one {{expr}}, never both"));
                        }
                        self.pos = end + 1;
                        Some(Ok(text.to_string()))
                    }
                    Some(b'{') => {
                        let (e, after) = self.braced(v0)?;
                        if e.is_empty() {
                            return refuse(aline, format!("{name}={{}}: an empty expression"));
                        }
                        self.pos = after;
                        Some(Err(e.to_string()))
                    }
                    _ => {
                        let end = self.b[v0..]
                            .iter()
                            .position(|c| c.is_ascii_whitespace() || *c == b'>' || (*c == b'/' ))
                            .map_or(self.b.len(), |i| v0 + i);
                        self.pos = end;
                        Some(Ok(self.src[v0..end].to_string()))
                    }
                }
            } else {
                None
            };
            let value = value.unwrap_or_else(|| Ok(String::new()));
            if let Some(event) = name.strip_prefix("on:") {
                match is {
                    Is::Slot => return refuse(aline, format!("a <slot> takes no {name}: a section is filled, not handled")),
                    Is::Component => return refuse(aline, format!("<{tag} {name}>: a component takes props, no on:")),
                    Is::El => {}
                }
                let handler = match value {
                    Err(h) if is_ident(&h) => h,
                    _ => return refuse(aline, format!("{name}= names a handler: {name}={{handler}}")),
                };
                if on.iter().any(|(e, _)| e == event) {
                    return refuse(aline, format!("{name} twice on <{tag}>"));
                }
                self.handlers.push((handler.clone(), aline));
                let h = self.j.str(handler);
                on.push((event.to_string(), h));
                continue;
            }
            if name.contains(':') {
                return refuse(aline, format!("{name} is not in the subset: an attribute, on:<event>, and nothing else"));
            }
            if matches!(is, Is::Slot) && name == "name" {
                match value {
                    Ok(n) if !n.is_empty() => slot_name = Some(n),
                    _ => return refuse(aline, "a <slot>'s name is a literal: <slot name=\"main\"/>"),
                }
                continue;
            }
            if attrs.iter().any(|(a, _)| a == name) {
                return refuse(aline, format!("{name} twice on <{tag}>"));
            }
            let v = match value {
                Ok(s) => self.j.str(s),
                Err(e) => self.j.expr(e),
            };
            attrs.push((name.to_string(), v));
        }
        let kind = match is {
            Is::El => Kind::El { tag: tag.to_string(), attrs, on },
            Is::Component => Kind::Component { tag: tag.to_string(), props: attrs },
            Is::Slot => match slot_name {
                Some(name) => Kind::Slot { name, props: attrs },
                None => return refuse(line, "a <slot> names its section: <slot name=\"main\"/>"),
            },
        };
        let slot = match &kind {
            Kind::Slot { name, .. } => Some(name.clone()),
            _ => None,
        };
        self.push(Frame::new(kind, line))?;
        if let Some(name) = slot {
            let parent = self.stack.len() - 2;
            note_slot(&mut self.stack[parent], name, line)?;
        }
        if self_closing || VOID.contains(&tag) {
            self.pop_slot_free()?;
        }
        Ok(())
    }

    /// Pop a frame just pushed, its slot already noted in its parent.
    fn pop_slot_free(&mut self) -> Result<(), Refusal> {
        if let Some(f) = self.stack.last_mut() {
            if matches!(f.kind, Kind::Slot { .. }) {
                f.slots.clear();
            }
        }
        self.pop()
    }

    fn script(&mut self, start: usize) -> Result<(), Refusal> {
        let line = self.line(start);
        if !self.at(">") {
            return refuse(line, "<script> takes no attributes: the script is Rhai");
        }
        if self.stack.len() > 1 {
            return refuse(line, "<script> belongs at the top level, outside every element and block");
        }
        if self.script.is_some() {
            return refuse(line, "a second <script>: a component has one");
        }
        let at = self.pos + 1;
        let Some(len) = self.src[at..].find("</script>") else {
            return refuse(line, "<script> opened here is never closed");
        };
        let src: &'a str = self.src;
        self.script = Some(Script { at, text: &src[at..at + len] });
        self.pos = at + len + "</script>".len();
        Ok(())
    }

    fn close_tag(&mut self) -> Result<(), Refusal> {
        let start = self.pos;
        let tag = self.name_at(start + 2);
        let mut end = start + 2 + tag.len();
        while end < self.b.len() && self.b[end].is_ascii_whitespace() {
            end += 1;
        }
        if self.b.get(end) != Some(&b'>') {
            return refuse(self.line(start), format!("</{tag} is not closed with >"));
        }
        let top = self.stack.last().expect("the root");
        let matches = match &top.kind {
            Kind::El { tag: t, .. } | Kind::Component { tag: t, .. } => t == tag,
            Kind::Slot { .. } => tag == "slot",
            _ => false,
        };
        if !matches {
            if self.stack.len() == 1 {
                return refuse(self.line(start), format!("</{tag}> closes nothing"));
            }
            return refuse(top.line, format!("{} opened here is not closed before </{tag}>", top.opening()));
        }
        self.pos = end + 1;
        self.pop_slot_free()
    }

    fn brace(&mut self) -> Result<(), Refusal> {
        let start = self.pos;
        let line = self.line(start);
        let (inner, after) = self.braced(start)?;
        let src: &'a str = self.src;
        let raw = &src[start + 1..after - 1];
        let word = |s: &str| s.split(|c: char| c.is_whitespace() || c == '}').next().unwrap_or("").to_string();
        match raw.as_bytes().first() {
            Some(b'#') => {
                let kw = word(&raw[1..]);
                let rest = raw[1 + kw.len()..].trim();
                match kw.as_str() {
                    "if" => {
                        if rest.is_empty() {
                            return refuse(line, "{#if} needs a condition: {#if expr}");
                        }
                        self.push(Frame::new(Kind::If { cond: rest.to_string(), then: None }, line))?;
                    }
                    "each" => {
                        let Some(i) = rest.rfind(" as ") else {
                            return refuse(line, "{#each} is {#each expr as x (key)}");
                        };
                        let (expr, tail) = (rest[..i].trim(), rest[i + 4..].trim());
                        let (as_, key) = match tail.find('(') {
                            Some(k) if tail.ends_with(')') => (tail[..k].trim(), Some(tail[k + 1..tail.len() - 1].trim())),
                            Some(_) => return refuse(line, "{#each}'s key is (expr), at the end"),
                            None => (tail, None),
                        };
                        if as_.contains(',') {
                            return refuse(line, "{#each … as x, i}: an index is not in the subset");
                        }
                        if !is_ident(as_) || expr.is_empty() || key.is_some_and(str::is_empty) {
                            return refuse(line, "{#each} is {#each expr as x (key)}");
                        }
                        let e = self.j.str(expr);
                        self.each_exprs.push((e, expr.to_string()));
                        let kind = Kind::Each { expr: e, as_: as_.to_string(), key: key.map(str::to_string) };
                        self.push(Frame::new(kind, line))?;
                    }
                    _ => return refuse(line, format!("{{#{kw} is not in the subset: {{#if}} and {{#each}} only")),
                }
            }
            Some(b':') => {
                let kw = word(&raw[1..]);
                let rest = raw[1 + kw.len()..].trim();
                if kw != "else" {
                    return refuse(line, format!("{{:{kw} is not in the subset: {{:else}} only"));
                }
                if !rest.is_empty() {
                    return refuse(line, "{:else if} is not in the subset: nest an {#if} inside {:else}");
                }
                let depth = self.stack.len();
                let top = self.stack.last_mut().expect("the root");
                let (tline, topening) = (top.line, top.opening());
                match &mut top.kind {
                    Kind::If { then: then @ None, .. } => {
                        *then = Some(std::mem::take(&mut top.children));
                        top.then_slots = std::mem::take(&mut top.slots);
                    }
                    Kind::If { .. } => return refuse(line, "a second {:else} in one {#if}"),
                    _ if depth == 1 => return refuse(line, "{:else} outside an {#if}"),
                    _ => return refuse(tline, format!("{topening} opened here is not closed before {{:else}}")),
                }
            }
            Some(b'/') => {
                let kw = word(&raw[1..]);
                if kw != "if" && kw != "each" {
                    return refuse(line, format!("{{/{kw}}} is not in the subset"));
                }
                let top = self.stack.last().expect("the root");
                let ok = matches!((&top.kind, kw.as_str()), (Kind::If { .. }, "if") | (Kind::Each { .. }, "each"));
                if !ok {
                    if self.stack.len() == 1 {
                        return refuse(line, format!("{{/{kw}}} closes nothing"));
                    }
                    return refuse(top.line, format!("{} opened here is not closed before {{/{kw}}}", top.opening()));
                }
                self.pos = after;
                return self.pop();
            }
            Some(b'@') => {
                let kw = word(&raw[1..]);
                if kw != "html" {
                    return refuse(line, format!("{{@{kw} is not in the subset: {{@html expr}} only"));
                }
                // UI-10: `{@html expr}` — the engine draws it through its sanitizer.
                let expr = raw[1 + kw.len()..].trim();
                if expr.is_empty() {
                    return refuse(line, "{@html } needs an expression: {@html expr}");
                }
                let e = self.j.expr(expr);
                let node = self.j.obj(vec![("html".into(), e)]);
                self.add(node, start)?;
                self.pos = after;
                return Ok(());
            }
            _ => {
                if inner.is_empty() {
                    return refuse(line, "{}: an empty expression");
                }
                let e = self.j.expr(inner);
                let node = self.j.obj(vec![("text".into(), e)]);
                self.add(node, start)?;
            }
        }
        self.pos = after;
        Ok(())
    }
}

/// A section named `name` (at `line`) drawn in `f`: refused when `f`
/// already draws one of that name.
fn note_slot(f: &mut Frame, name: String, line: u32) -> Result<(), Refusal> {
    if let Some(first) = f.slots.get(&name) {
        return refuse(line, format!("the section `{name}` twice (line {first} and line {line}): a slot's name is drawn once, but in the two arms of an if/else"));
    }
    f.slots.insert(name, line);
    Ok(())
}
