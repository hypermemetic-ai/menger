//! THEME-3: a theme is a ticket's block — ` ```theme name [extends=parent] `
//! — read and validated here. Wasm-safe: the server refuses a write whose
//! block is not a theme, and the app renders the same block as its swatch
//! sheet, reading it the same way.
//!
//! ````markdown
//! ```theme pantry extends=default
//! @import url("https://fonts.googleapis.com/css2?family=Fraunces&display=swap");
//! :root { --accent: #d4572a; --font-display: "Fraunces", serif; }
//! :root[data-theme="dark"] { --accent: #f08a5d; }
//! @font-face { font-family: "Inter"; src: url(https://example.org/inter.woff2) format("woff2"); }
//! m-code::part(frame) { border-radius: 14px; }
//! ```
//! ````
//!
//! **What a block may say** — looks only (THEME-1's `skins`: tokens and
//! `::part`, never behaviour):
//!
//! - `:root { … }` (or `:root[data-theme="light"]`) — token overrides for
//!   light; `:root[data-theme="dark"] { … }` — for dark. Only tokens of the
//!   contract (`tokens.toml`), each a value that cannot end its
//!   declaration. A token that is not a colour, said for light, holds in dark.
//! - `@font-face { … }` — `font-family`, `src`, `font-weight`, `font-style`,
//!   `font-display`, `font-stretch`, `unicode-range`; and `@import
//!   url("https://…");` — a font's stylesheet. THEME-1's `fonts` decision
//!   (owner: any URL): a font may come from any host, but only over https.
//! - `m-name::part(part) { … }` (a `:hover`, `:focus`, `:focus-visible` or
//!   `:active` after it, a comma list of such) — any property, values that
//!   cannot end their rule, a `url(…)` only https.
//!
//! Anything else — another selector, a property outside the contract in
//! `:root`, an `http:` URL, `expression(` or `javascript:` — is a
//! [`BlockError`] naming the block's line, and the write that carries it is
//! refused whole.

use std::collections::BTreeMap;

use crate::themes::{self, DEFAULT_THEME};

/// One theme, as its block says it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeDef {
    pub name: String,
    /// The theme it states its differences from (`None`: `default`).
    pub extends: Option<String>,
    pub light: BTreeMap<String, String>,
    pub dark: BTreeMap<String, String>,
    /// `@font-face` and `m-*::part(…)` rules, each normalized: `selector{prop:value;…}`.
    pub rules: Vec<String>,
    /// `@import` URLs (https), in order.
    pub imports: Vec<String>,
}

/// A block refused: the line of the block (1 is its first line after the
/// fence) and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for BlockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line == 0 {
            f.write_str(&self.message)
        } else {
            write!(f, "line {}: {}", self.line, self.message)
        }
    }
}

fn err(line: usize, message: impl Into<String>) -> BlockError {
    BlockError { line, message: message.into() }
}

/// A fence's info string, when it opens a theme: `theme name [extends=p]` →
/// its name and parent, or why the opening line is not one. `None`: the
/// fence is not a theme at all.
pub fn info(info: &str) -> Option<Result<(String, Option<String>), BlockError>> {
    let mut words = info.split_whitespace();
    if words.next() != Some("theme") {
        return None;
    }
    Some((|| {
        let raw = words.next().ok_or_else(|| err(0, "```theme names no theme: write ```theme name [extends=parent]"))?;
        let name = themes::name_of(raw).map_err(|e| err(0, e))?;
        if name == DEFAULT_THEME {
            return Err(err(0, "`default` is the contract's own values: a block cannot define it"));
        }
        let mut extends = None;
        for w in words {
            let Some(p) = w.strip_prefix("extends=") else {
                return Err(err(0, format!("`{w}` after the theme's name: only extends=parent")));
            };
            let p = themes::name_of(p).map_err(|e| err(0, e))?;
            if p == name {
                return Err(err(0, format!("`{name}` extends itself")));
            }
            extends = (p != DEFAULT_THEME).then_some(p);
        }
        Ok((name, extends))
    })())
}

/// `src` with every `/* … */` blanked, newlines kept (so lines still count).
fn uncomment(src: &str) -> Result<String, BlockError> {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find("*/").map(|e| start + e + 2) else {
            let line = out.matches('\n').count() + 1;
            return Err(err(line, "a comment that never ends"));
        };
        out.extend(rest[start..end].chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
        rest = &rest[end..];
    }
    out.push_str(rest);
    Ok(out)
}

/// A URL a theme may load: https, nothing that could end its `url("…")`
/// (a `;` may stay: Google Fonts' weights are `wght@400;600`, and a theme
/// writes every URL quoted).
pub fn url_ok(url: &str) -> bool {
    url.len() <= 500
        && url.get(..8).is_some_and(|s| s.eq_ignore_ascii_case("https://"))
        && url.len() > 8
        && !url.contains(['"', '\'', '(', ')', ' ', '\\', '<', '>', '{', '}', '\n', '\r', '\t'])
}

/// Where each top-level `sep` in `s` sits — outside quotes and parentheses.
fn splits(s: &str, sep: char) -> Vec<usize> {
    let (mut quote, mut depth, mut out) = (None::<char>, 0i32, Vec::new());
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, c) if c == sep && depth <= 0 => out.push(i),
            _ => {}
        }
    }
    out
}

/// Every `url(…)` in `value` is https; `Err` names the first that is not.
fn urls_ok(value: &str) -> Result<(), String> {
    let lower = value.to_ascii_lowercase();
    let mut at = 0;
    while let Some(i) = lower[at..].find("url(") {
        let start = at + i + 4;
        let end = value[start..].find(')').map(|e| start + e).ok_or("a url( that never closes")?;
        let url = value[start..end].trim().trim_matches(['"', '\'']);
        if !url_ok(url) {
            return Err(format!("`url({url})`: a theme loads only https URLs (THEME-1: fonts from any host, over https)"));
        }
        at = end;
    }
    Ok(())
}

/// A declaration's value a rule may hold.
fn rule_value_ok(v: &str) -> Result<(), String> {
    let lower = v.to_ascii_lowercase();
    if v.is_empty() || v.len() > 1000 || v.contains(['<', '>', '{', '}', '\\']) {
        return Err(format!("`{v}` is not a value a theme may write (no < > {{ }} \\, at most 1000 bytes)"));
    }
    for bad in ["expression(", "javascript:", "behavior", "-moz-binding", "@import"] {
        if lower.contains(bad) {
            return Err(format!("`{v}`: `{bad}` is behaviour, not looks"));
        }
    }
    urls_ok(v)
}

/// `m-name::part(p)`, with an optional user-action pseudo-class after it.
fn part_selector_ok(sel: &str) -> bool {
    let Some(rest) = sel.strip_prefix("m-") else { return false };
    let Some((tag, rest)) = rest.split_once("::part(") else { return false };
    let Some((parts, after)) = rest.split_once(')') else { return false };
    let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    word(tag) && parts.split_whitespace().all(word) && !parts.trim().is_empty()
        && matches!(after, "" | ":hover" | ":focus" | ":focus-visible" | ":active")
}

/// Which mode a `:root` selector says, if it is one.
fn root_mode(sel: &str) -> Option<bool> {
    let s: String = sel.chars().filter(|c| !c.is_whitespace()).collect();
    let s = s.replace('\'', "\"");
    match s.as_str() {
        ":root" | ":root[data-theme=\"light\"]" | ":root[data-theme=light]" => Some(false),
        ":root[data-theme=\"dark\"]" | ":root[data-theme=dark]" => Some(true),
        _ => None,
    }
}

const FONT_FACE: &[&str] = &["font-family", "src", "font-weight", "font-style", "font-display", "font-stretch", "unicode-range"];

/// The declarations of a rule's body, each with its line (counted from
/// `line`, the line its `{` is on).
fn declarations(body: &str, line: usize) -> Result<Vec<(usize, String, String)>, BlockError> {
    let mut out = Vec::new();
    let mut at_line = line;
    let mut parts = Vec::new();
    let mut from = 0;
    for at in splits(body, ';') {
        parts.push(&body[from..at]);
        from = at + 1;
    }
    parts.push(&body[from..]);
    for d in parts {
        let here = at_line + d[..d.len() - d.trim_start().len()].matches('\n').count();
        at_line += d.matches('\n').count();
        let d = d.trim();
        if d.is_empty() {
            continue;
        }
        let Some((p, v)) = d.split_once(':') else {
            return Err(err(here, format!("`{d}` is not a declaration (property: value)")));
        };
        out.push((here, p.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
    Ok(out)
}

/// The block's content, read: see the module doc. `Err` names its line.
pub fn parse(name: &str, extends: Option<&str>, content: &str) -> Result<ThemeDef, BlockError> {
    let mut def = ThemeDef { name: name.to_string(), extends: extends.map(str::to_string), ..Default::default() };
    let src = uncomment(content)?;
    let mut rest: &str = &src;
    let mut line = 1;
    loop {
        let lead = rest.len() - rest.trim_start().len();
        line += rest[..lead].matches('\n').count();
        rest = &rest[lead..];
        if rest.is_empty() {
            break;
        }
        if rest.to_ascii_lowercase().starts_with("@import") {
            let end = splits(rest, ';').first().copied().ok_or_else(|| err(line, "an @import without its ;"))?;
            let stmt = rest[7..end].trim();
            let url = stmt
                .strip_prefix("url(")
                .and_then(|u| u.strip_suffix(')'))
                .unwrap_or(stmt)
                .trim()
                .trim_matches(['"', '\'']);
            if !url_ok(url) {
                return Err(err(line, format!("`@import {stmt}`: a theme imports only an https URL (a font's stylesheet)")));
            }
            def.imports.push(url.to_string());
            line += rest[..end].matches('\n').count();
            rest = &rest[end + 1..];
            continue;
        }
        let open = rest.find('{').ok_or_else(|| err(line, format!("`{}` is not a rule (selector {{ … }})", first_line(rest))))?;
        let sel = rest[..open].trim();
        if sel.contains('}') {
            return Err(err(line, "a } with no rule to close"));
        }
        let open_line = line + rest[..open].matches('\n').count();
        let close = rest[open..].find('}').map(|c| open + c).ok_or_else(|| err(line, format!("`{sel} {{` never closes")))?;
        let body = &rest[open + 1..close];
        if body.contains('{') {
            return Err(err(line, format!("`{sel}`: a rule inside a rule — a theme's rules do not nest")));
        }
        let decls = declarations(body, open_line)?;
        if let Some(dark) = root_mode(sel) {
            let map = if dark { &mut def.dark } else { &mut def.light };
            for (l, p, v) in decls {
                if !p.starts_with("--") {
                    return Err(err(l, format!("`{p}` in {sel}: a theme sets tokens there, nothing else (`--accent: …`)")));
                }
                let Some(t) = themes::token(&p) else {
                    return Err(err(l, format!("`{p}` is not a token of the contract (tickets-api/src/tokens.toml)")));
                };
                if !themes::value_ok(&v) {
                    return Err(err(l, format!("`{p}: {v}` — not a token's value (no < > {{ }} ; \\, at most {} bytes)", themes::VALUE_MAX)));
                }
                urls_ok(&v).map_err(|e| err(l, e))?;
                map.insert(t.name.to_string(), v);
            }
        } else if sel.eq_ignore_ascii_case("@font-face") {
            let mut out = String::from("@font-face{");
            for (l, p, v) in decls {
                if !FONT_FACE.contains(&p.as_str()) {
                    return Err(err(l, format!("`{p}` in @font-face: one of {}", FONT_FACE.join(", "))));
                }
                rule_value_ok(&v).map_err(|e| err(l, e))?;
                out.push_str(&format!("{p}:{v};"));
            }
            out.push('}');
            def.rules.push(out);
        } else if sel.split(',').map(str::trim).all(part_selector_ok) {
            let sel = sel.split(',').map(str::trim).collect::<Vec<_>>().join(",");
            let mut out = format!("{sel}{{");
            for (l, p, v) in decls {
                if !p.chars().all(|c| c.is_ascii_lowercase() || c == '-') || p.is_empty() {
                    return Err(err(l, format!("`{p}` is not a property")));
                }
                if matches!(p.as_str(), "behavior" | "-moz-binding" | "pointer-events" | "content") {
                    return Err(err(l, format!("`{p}` is behaviour, not looks (THEME-1: a theme skins, never changes what a component does)")));
                }
                rule_value_ok(&v).map_err(|e| err(l, e))?;
                out.push_str(&format!("{p}:{v};"));
            }
            out.push('}');
            def.rules.push(out);
        } else {
            return Err(err(
                line,
                format!(
                    "`{sel}`: a theme's rules are :root, :root[data-theme=\"dark\"], @font-face, @import url(\"https://…\") \
                     or m-name::part(part) — looks only (THEME-1)"
                ),
            ));
        }
        line = open_line + body.matches('\n').count();
        rest = &rest[close + 1..];
    }
    Ok(def)
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

/// Every ` ```theme ` block in a markdown body, as `(info string, content)`,
/// in order — fences only (an indented block, or one inside another fence,
/// is text), the body's frontmatter skipped. Used by the app (the swatch
/// sheet) and the server (the registry) alike.
pub fn blocks(body: &str) -> Vec<(String, String)> {
    use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
    if !body.contains("theme") {
        return Vec::new();
    }
    let rendered = crate::fields::split_frontmatter(body).1;
    let mut out = Vec::new();
    let mut open: Option<(String, String)> = None;
    for ev in Parser::new(rendered) {
        match ev {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) if info.split_whitespace().next() == Some("theme") => {
                open = Some((info.to_string(), String::new()));
            }
            Event::Text(t) => {
                if let Some((_, c)) = &mut open {
                    c.push_str(&t);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(b) = open.take() {
                    out.push(b);
                }
            }
            _ => {}
        }
    }
    out
}

/// A body's themes, read: each block's [`ThemeDef`], or the first block
/// that is not one — `(name as written, error)`; a name two blocks define is
/// refused too.
pub fn read(body: &str) -> Result<Vec<ThemeDef>, (String, BlockError)> {
    let mut out: Vec<ThemeDef> = Vec::new();
    for (inf, content) in blocks(body) {
        let written = inf.split_whitespace().nth(1).unwrap_or("").to_string();
        let (name, extends) = info(&inf).expect("a theme fence").map_err(|e| (written.clone(), e))?;
        let def = parse(&name, extends.as_deref(), &content).map_err(|e| (name.clone(), e))?;
        if out.iter().any(|d| d.name == def.name) {
            return Err((name, err(0, "a second ```theme block defines it: one block per theme")));
        }
        out.push(def);
    }
    Ok(out)
}

#[cfg(test)]
#[path = "theme_block_tests.rs"]
mod tests;
