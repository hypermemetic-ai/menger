//! THEME-2: the token contract, read from its one file (`tokens.toml`,
//! beside this one). Wasm-safe: the server checks a theme against it, the
//! app paints regions and frames with it, and the app's tests generate
//! index.html's `:root` blocks from it ([`root_css`]).
//!
//! The file is TOML in one shape only — a table per group (`[colour.paper]`),
//! a line per token (`"--bg" = ['#fbfbf9', '#101014', 'the page']`: light,
//! dark, what it paints) — read here by a small reader that keeps the file's
//! order and refuses anything else, so the crate takes no TOML dependency.

use std::sync::LazyLock;

/// The contract's source, compiled in.
pub const SOURCE: &str = include_str!("tokens.toml");

/// One token of the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// `--accent`.
    pub name: &'static str,
    /// Its group: `colour.accent`, `type.face`, `shape.radius`, …
    pub group: &'static str,
    /// Its default in light.
    pub light: &'static str,
    /// Its default in dark (the light value for every group but colour).
    pub dark: &'static str,
    /// What it paints.
    pub note: &'static str,
}

impl Token {
    /// A colour says a value per mode; every other token says one for both,
    /// so a theme that sets it for light has set it for dark too.
    pub fn is_colour(&self) -> bool {
        self.group == "colour" || self.group.starts_with("colour.")
    }
}

/// Every token, in the file's order.
pub static TOKENS: LazyLock<Vec<Token>> =
    LazyLock::new(|| parse(SOURCE).unwrap_or_else(|e| panic!("tokens.toml: {e}")));

/// The token `name` names — with or without its leading `--`, any ASCII
/// case — or `None` when it is not one of [`TOKENS`].
pub fn token(name: &str) -> Option<&'static Token> {
    let n = name.trim().trim_start_matches("--").to_ascii_lowercase();
    TOKENS.iter().find(|t| t.name[2..] == n)
}

/// One quoted TOML string at the start of `s` (literal `'…'`, or basic
/// `"…"` without escapes), and the rest after it.
fn string(s: &str) -> Result<(&str, &str), String> {
    let s = s.trim_start();
    let q = s.chars().next().filter(|c| *c == '\'' || *c == '"').ok_or("expected a quoted string")?;
    let body = &s[1..];
    let end = body.find(q).ok_or("an unterminated string")?;
    let v = &body[..end];
    if q == '"' && v.contains('\\') {
        return Err("a basic string with an escape (write it as a literal '…' string)".into());
    }
    Ok((v, &body[end + 1..]))
}

/// Reads the contract. `Err` names the line.
pub fn parse(src: &'static str) -> Result<Vec<Token>, String> {
    let mut out: Vec<Token> = Vec::new();
    let mut group: Option<&'static str> = None;
    for (i, raw) in src.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(g) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let ok = !g.is_empty() && g.split('.').all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase()));
            if !ok {
                return Err(format!("line {n}: `{line}` is not a group"));
            }
            group = Some(g);
            continue;
        }
        let err = |m: &str| format!("line {n}: {m}: `{line}`");
        let group = group.ok_or_else(|| err("a token before any group"))?;
        let (name, rest) = string(line).map_err(|e| err(&e))?;
        let rest = rest.trim_start().strip_prefix('=').ok_or_else(|| err("expected ="))?;
        let rest = rest.trim_start().strip_prefix('[').ok_or_else(|| err("expected ["))?;
        let (light, rest) = string(rest).map_err(|e| err(&e))?;
        let rest = rest.trim_start().strip_prefix(',').ok_or_else(|| err("expected ,"))?;
        let (dark, rest) = string(rest).map_err(|e| err(&e))?;
        let rest = rest.trim_start().strip_prefix(',').ok_or_else(|| err("expected ,"))?;
        let (note, rest) = string(rest).map_err(|e| err(&e))?;
        if rest.trim() != "]" {
            return Err(err("expected ] to end the line"));
        }
        let name_ok = name
            .strip_prefix("--")
            .is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        if !name_ok {
            return Err(err("a token is --lowercase-words"));
        }
        if out.iter().any(|t| t.name == name) {
            return Err(err("a token named twice"));
        }
        let t = Token { name, group, light, dark, note };
        if !t.is_colour() && light != dark {
            return Err(err("only a colour says a different value for dark"));
        }
        for v in [light, dark] {
            if !crate::themes::value_ok(v) {
                return Err(err("a value that could end its declaration"));
            }
        }
        out.push(t);
    }
    Ok(out)
}

/// THEME-2: what each `colour.diagram` token sets in mermaid's
/// `themeVariables` when a theme names a colour for it (its default, `auto`,
/// leaves mermaid's own palette for the mode). The app's renderer and the
/// frame SDK read this one table (the SDK from its init block).
pub const DIAGRAM: &[(&str, &[&str])] = &[
    ("--diagram-node", &["primaryColor", "mainBkg", "actorBkg"]),
    ("--diagram-border", &["primaryBorderColor", "nodeBorder", "actorBorder"]),
    ("--diagram-line", &["lineColor", "signalColor"]),
    ("--diagram-text", &["primaryTextColor", "textColor", "actorTextColor", "signalTextColor"]),
];

/// The top-level group: `colour`, `type`, `space`, `shape`, `motion`.
pub fn family(group: &str) -> &str {
    group.split('.').next().unwrap_or(group)
}

/// index.html's token blocks, as the contract says them, each line
/// indented as the stylesheet indents it: `:root { … }` (every token, light),
/// then the dark values twice — under `prefers-color-scheme` guarded so a
/// stored "light" opts out, and under the toggle's `data-theme="dark"` —
/// colours only (every other token holds in both modes).
pub fn root_css() -> String {
    let mut s = String::new();
    let decl = |s: &mut String, indent: &str, t: &Token, v: &str| {
        s.push_str(&format!("{indent}{}: {v}; /* {} */\n", t.name, t.note.replace("*/", "* /")));
    };
    let groups = |s: &mut String, indent: &str, dark: bool| {
        let mut last = "";
        for t in TOKENS.iter().filter(|t| !dark || t.is_colour()) {
            if t.group != last {
                s.push_str(&format!("{indent}/* {} */\n", t.group));
                last = t.group;
            }
            decl(s, indent, t, if dark { t.dark } else { t.light });
        }
    };
    s.push_str("  :root {\n    color-scheme: light;\n");
    groups(&mut s, "    ", false);
    s.push_str("  }\n");
    s.push_str("  @media (prefers-color-scheme: dark) {\n    :root:not([data-theme=\"light\"]) {\n      color-scheme: dark;\n");
    groups(&mut s, "      ", true);
    s.push_str("    }\n  }\n");
    s.push_str("  :root[data-theme=\"dark\"] {\n    color-scheme: dark;\n");
    groups(&mut s, "    ", true);
    s.push_str("  }\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_contract_reads_and_names_every_group() {
        let t = &*TOKENS;
        assert!(t.len() > 80, "{}", t.len());
        for fam in ["colour", "type", "space", "shape", "motion"] {
            assert!(t.iter().any(|x| family(x.group) == fam), "no {fam} group");
        }
        for g in ["colour.paper", "colour.ink", "colour.line", "colour.accent", "colour.status", "colour.kind", "colour.code",
                  "type.face", "type.scale", "type.weight", "shape.radius", "shape.border"] {
            assert!(t.iter().any(|x| x.group == g), "no {g}");
        }
        assert_eq!(token("accent").map(|t| t.light), Some("#0066cc"));
        assert_eq!(token("--MONO").map(|t| t.group), Some("type.face"));
        assert!(token("--nope").is_none());
    }

    #[test]
    fn the_reader_refuses_what_it_cannot_read_naming_the_line() {
        for (src, want) in [
            ("\"--a\" = ['1', '1', 'x']\n", "line 1: a token before any group"),
            ("[space]\n\"--a\" = ['1', '2', 'x']\n", "line 2: only a colour"),
            ("[space]\n\"--a\" = ['1', '1', 'x']\n\"--a\" = ['1', '1', 'x']\n", "line 3: a token named twice"),
            ("[Space]\n", "line 1: `[Space]` is not a group"),
            ("[space]\n\"a\" = ['1', '1', 'x']\n", "line 2: a token is"),
            ("[space]\n\"--a\" = ['1;', '1;', 'x']\n", "line 2: a value that could end"),
            ("[space]\n\"--a\" = ['1', '1']\n", "line 2: expected ,"),
            ("[space]\n\"--a\" = [\"1\\n\", '1', 'x']\n", "line 2: a basic string with an escape"),
        ] {
            let src: &'static str = Box::leak(src.to_string().into_boxed_str());
            let e = parse(src).expect_err(src);
            assert!(e.starts_with(want), "{src}: {e}");
        }
    }

    #[test]
    fn the_diagram_table_is_the_diagram_group() {
        let group: Vec<&str> = TOKENS.iter().filter(|t| t.group == "colour.diagram").map(|t| t.name).collect();
        let table: Vec<&str> = DIAGRAM.iter().map(|d| d.0).collect();
        assert_eq!(group, table);
    }

    #[test]
    fn the_blocks_say_every_token_in_light_and_every_colour_in_dark() {
        let css = root_css();
        for t in TOKENS.iter() {
            assert!(css.contains(&format!("    {}: {};", t.name, t.light)), "{}", t.name);
            let dark = css.matches(&format!("{}: {};", t.name, t.dark)).count();
            if t.is_colour() {
                assert!(dark >= 2, "{} in both dark blocks", t.name);
            }
        }
        assert_eq!(css.matches("color-scheme: dark;").count(), 2);
    }
}
