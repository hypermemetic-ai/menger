//! THEME-2: the token contract, held in the app's gate.
//!
//! - index.html's token blocks are the contract's ([`root_css`]), byte for
//!   byte; `MENGER_BLESS=1 cargo test` rewrites them from
//!   tickets-api/src/tokens.toml.
//! - **The lint**: no colour, face or radius is written anywhere else — not
//!   in index.html's stylesheet outside the blocks, not in the frame SDK
//!   (sdk/menger-sdk.js), not in frontend/src. Tests are exempt (a test
//!   module, a `*_tests.rs` file), and so are mockups (a body's own page,
//!   never the app's).
//! - The status table reads its pairs from the contract; frames receive
//!   every token the blocks define.
//!
//! [`root_css`]: menger_tickets_api::tokens::root_css

use crate::STATUSES;
use menger_tickets_api::tokens::{root_css, TOKENS};
use std::path::Path;

const BEGIN: &str = "  /* tokens:begin */\n";
const END: &str = "  /* tokens:end */";

fn root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

fn index_html() -> String {
    std::fs::read_to_string(format!("{}/index.html", root())).unwrap()
}

/// The app's stylesheet: what sits between the shell's <style> tags.
pub(crate) fn app_css() -> String {
    let html = index_html();
    let start = html.find("<style>").expect("<style>") + "<style>".len();
    let end = html.find("</style>").expect("</style>");
    html[start..end].to_string()
}

/// `text` with the generated blocks taken out, and the blocks.
fn split_blocks(text: &str) -> (String, String) {
    let b = text.find(BEGIN).expect("index.html has no `/* tokens:begin */` marker");
    let e = text.find(END).expect("index.html has no `/* tokens:end */` marker");
    assert!(b < e, "tokens:end before tokens:begin");
    let inside = text[b + BEGIN.len()..e].to_string();
    // Keep the line count: the lint names lines as index.html numbers them.
    let blank = "\n".repeat(inside.matches('\n').count());
    (format!("{}{BEGIN}{blank}{}", &text[..b], &text[e..]), inside)
}

/// THEME-2: index.html's blocks are generated from the contract.
#[test]
fn index_htmls_token_blocks_are_the_contracts() {
    let html = index_html();
    let (_, inside) = split_blocks(&html);
    let want = root_css();
    if inside == want {
        return;
    }
    if std::env::var_os("MENGER_BLESS").is_some() {
        let b = html.find(BEGIN).unwrap() + BEGIN.len();
        let e = html.find(END).unwrap();
        let blessed = format!("{}{want}{}", &html[..b], &html[e..]);
        std::fs::write(format!("{}/index.html", root()), blessed).unwrap();
        return;
    }
    panic!(
        "index.html's token blocks differ from tickets-api/src/tokens.toml — run `MENGER_BLESS=1 cargo test \
         index_htmls_token_blocks` in frontend/ to regenerate them (never edit the blocks by hand)"
    );
}

/// `text` with every `/* … */` comment blanked, newlines kept.
fn strip_block_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        let end = rest[start..].find("*/").map(|e| start + e + 2).unwrap_or(rest.len());
        out.extend(rest[start..end].chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// True if `line` holds a colour literal: `#` + 3/4/6/8 hex digits as a
/// whole token (an id selector like `#board` or `#error` is not one — its
/// letters are not all hex, or it runs on into more word characters), an
/// `rgb(`/`rgba(`/`hsl(` function, or a named colour.
pub(crate) fn has_color_literal(line: &str) -> bool {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'#' {
            continue;
        }
        let run = bytes[i + 1..].iter().take_while(|c| c.is_ascii_hexdigit()).count();
        let next = bytes.get(i + 1 + run).copied();
        let ends_token = !next.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
        if matches!(run, 3 | 4 | 6 | 8) && ends_token {
            return true;
        }
    }
    let lower = line.to_ascii_lowercase();
    if ["rgb(", "rgba(", "hsl(", "hsla(", "color(", "oklch(", "lab("].iter().any(|f| lower.contains(f)) {
        return true;
    }
    // The named colours a scaffold reaches for; `transparent`, `inherit`,
    // `none`, and `currentColor` are not palette choices. A hyphen is part
    // of the word: `white-space` names no colour.
    let named = [
        "white", "black", "red", "green", "blue", "gray", "grey", "yellow", "orange", "purple", "pink", "silver", "navy",
        "teal", "maroon", "olive", "lime", "aqua",
    ];
    // A named colour only counts as a CSS value: after a `:` in the line.
    let Some(colon) = lower.find(':') else { return false };
    lower[colon..].split(|c: char| !(c.is_ascii_alphabetic() || c == '-')).any(|w| named.contains(&w))
}

/// Each value of `prop:` in `line` (up to `;`, `}` or a quote), with every
/// `var(…)` taken out.
fn values<'a>(line: &'a str, props: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
    props.iter().flat_map(move |p| {
        line.match_indices(p).filter_map(move |(i, _)| {
            // `p` must start a property: not the tail of a longer name.
            let before = line[..i].chars().last();
            if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-') {
                return None;
            }
            let v = &line[i + p.len()..];
            let v = &v[..v.find([';', '}']).unwrap_or(v.len())];
            Some(strip_vars(v))
        })
    })
}

/// `v` without its `var(…)` calls (nested parentheses counted).
fn strip_vars(v: &str) -> String {
    let mut out = String::new();
    let mut rest = v;
    while let Some(i) = rest.find("var(") {
        out.push_str(&rest[..i]);
        let mut depth = 0;
        let mut end = rest.len();
        for (j, c) in rest[i..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + j + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// True if `line` names a face anywhere but through a token: a
/// `font-family:` or `font:` whose value, its `var(…)`s taken out, names a
/// family (a generic one, a system stack's names, or a quoted one).
pub(crate) fn has_face_literal(line: &str) -> bool {
    const FACES: &[&str] = &[
        "serif", "sans-serif", "monospace", "system-ui", "cursive", "fantasy", "ui-monospace", "ui-sans-serif",
        "ui-serif", "-apple-system", "blinkmacsystemfont", "menlo", "consolas", "roboto", "arial", "helvetica",
        "inter", "georgia", "sfmono-regular", "courier",
    ];
    values(line, &["font-family:", "font:"]).any(|v| {
        // A quoted family: a quote straight before a capital (`"Inter"`).
        let quoted = v.as_bytes().windows(2).any(|w| matches!(w[0], b'"' | b'\'') && w[1].is_ascii_uppercase());
        let v = v.to_ascii_lowercase();
        quoted || v.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).any(|w| FACES.contains(&w))
    })
}

/// True if `line` sets a radius with a length rather than a token (`0` and
/// a percentage — a circle — are shapes, not a theme's choice).
pub(crate) fn has_radius_literal(line: &str) -> bool {
    values(line, &["border-radius:", "border-top-left-radius:", "border-top-right-radius:", "border-bottom-left-radius:", "border-bottom-right-radius:"])
        .any(|v| {
            v.split(|c: char| c.is_whitespace() || c == '/' || c == ',' || c == '(' || c == ')').any(|w| {
                let num: String = w.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                let unit = &w[num.len()..];
                !num.is_empty() && num.chars().any(|c| matches!(c, '1'..='9')) && matches!(unit, "px" | "rem" | "em")
            })
        })
}

/// A line the lint reads past: one that parses a colour the browser
/// computed (mermaid.rs's luminance), marked so at its end.
const PASS: &str = "tokens-lint: ";

/// What the lint found on one line.
fn verdict(line: &str) -> Option<&'static str> {
    // DESIGN-38: an `@font-face` (one per line) names the face the contract's
    // face defaults ask for — it declares a family and paints nothing; a rule
    // that uses the family by name is still refused.
    if line.contains(PASS) || line.trim_start().starts_with("@font-face") {
        None
    } else if has_color_literal(line) {
        Some("colour")
    } else if has_face_literal(line) {
        Some("face")
    } else if has_radius_literal(line) {
        Some("radius")
    } else {
        None
    }
}

/// A Rust source without its test modules (from a `#[cfg(test)]` that
/// opens a `mod` to the end) and its `//` comment lines, blanked so line
/// numbers hold.
pub(crate) fn rust_shipped(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let cut = lines
        .windows(2)
        .position(|w| w[0].trim() == "#[cfg(test)]" && w[1].trim_start().starts_with("mod "))
        .unwrap_or(lines.len());
    lines[..cut]
        .iter()
        .map(|l| if l.trim_start().starts_with("//") { "" } else { l })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A JavaScript source without its comments.
fn js_shipped(text: &str) -> String {
    strip_block_comments(text)
        .lines()
        .map(|l| if l.trim_start().starts_with("//") { "" } else { l })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every `(file:line, kind, text)` the lint refuses in `text`.
fn lint(file: &str, text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| verdict(l).map(|k| format!("{file}:{}: {k}: {}", i + 1, l.trim())))
        .collect()
}

/// What the lint reads: each shipped source as it ships, by name.
fn shipped() -> Vec<(String, String)> {
    let mut out = Vec::new();
    // index.html: its stylesheet, the generated blocks blanked; offset so a
    // line is named as index.html numbers it.
    let html = index_html();
    let start = html.find("<style>").unwrap() + "<style>".len();
    let pad = "\n".repeat(html[..start].matches('\n').count());
    let (css, _) = split_blocks(&app_css());
    out.push(("frontend/index.html".to_string(), format!("{pad}{}", strip_block_comments(&css))));
    let sdk = std::fs::read_to_string(format!("{}/sdk/menger-sdk.js", root())).unwrap();
    out.push(("frontend/sdk/menger-sdk.js".to_string(), js_shipped(&sdk)));
    let src = Path::new(root()).join("src");
    let mut files: Vec<_> = rust_files(&src);
    files.sort();
    for p in files {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if name.ends_with("_tests.rs") || name == "tokens_lint.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        let rel = p.strip_prefix(&src).unwrap().to_string_lossy().to_string();
        out.push((format!("frontend/src/{rel}"), rust_shipped(&text)));
    }
    out
}

/// Every `.rs` under `dir`, its modules' directories too (src/menu/, ACT-4).
pub(crate) fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(rust_files(&p));
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
    out
}

/// THEME-2 (AGS-15 criterion 4, grown): no colour, face or radius outside
/// the contract.
#[test]
fn no_colour_face_or_radius_is_written_outside_the_contract() {
    let found: Vec<String> = shipped().iter().flat_map(|(f, t)| lint(f, t)).collect();
    assert!(
        found.is_empty(),
        "THEME-2: a literal outside the token contract — name it in tickets-api/src/tokens.toml and paint with var(--…):\n{}",
        found.join("\n")
    );
}

/// DESIGN-38: the faces the contract's defaults name first are menger's
/// own — each has an `@font-face` in the stylesheet whose file is in
/// frontend/fonts/ and is copied by Trunk to /app/fonts/; nothing names a
/// font host.
#[test]
fn the_default_faces_are_self_hosted() {
    let html = index_html();
    assert!(html.contains("<link data-trunk rel=\"copy-dir\" href=\"fonts\" data-target-path=\"fonts\" />"));
    let css = app_css();
    for (face, file) in [("Geist", "geist-latin.woff2"), ("Geist Mono", "geist-mono-latin.woff2")] {
        let rule = css
            .lines()
            .find(|l| l.trim_start().starts_with("@font-face") && l.contains(&format!("font-family: \"{face}\";")))
            .unwrap_or_else(|| panic!("no @font-face for {face}"));
        assert!(rule.contains(&format!("url(/app/fonts/{file})")), "{rule}");
        assert!(rule.contains("font-weight: 100 900") && rule.contains("font-display: swap"), "{rule}");
        assert!(Path::new(root()).join("fonts").join(file).is_file(), "frontend/fonts/{file}");
    }
    let (_, blocks) = split_blocks(&html);
    for token in ["--mono", "--font-text"] {
        assert!(blocks.contains(&format!("{token}: \"Geist Mono\", ")), "{token} defaults to Geist Mono");
    }
    for token in ["--sans", "--font-display"] {
        assert!(blocks.contains(&format!("{token}: \"Geist\", ")), "{token} defaults to Geist");
    }
    assert!(!html.contains("fonts.googleapis.com") && !html.contains("fonts.gstatic.com"));
}

/// The lint is live: a literal planted in each place it reads is named.
#[test]
fn the_lint_catches_a_planted_literal() {
    for plant in [
        "  #nav .row.sel { background: #cfe2f7; }",
        "  header h1 { color: black; }",
        "  td.id { color: rgb(0, 102, 204); }",
        "  td.id { font-family: \"Inter\", sans-serif; }",
        "  td.id { font: 12px/1.5 Menlo, monospace; }",
        "  td.id { border-radius: 7px; }",
        "  td.id { border-radius: var(--radius) 3px; }",
        "  .wordmark { font-family: \"Geist\"; }",
    ] {
        assert_eq!(lint("x", plant).len(), 1, "{plant}");
    }
    for clean in [
        "  #error button { color: inherit; }",
        "  td.id { color: var(--accent); white-space: nowrap; }",
        "  #app.nonav #nav { display: none; }",
        "  .st { border-color: transparent; }",
        "  .dot { border-radius: 50%; }",
        "  .x { border-radius: 0; font: inherit; }",
        "  .x { font: var(--weight-strong) calc(11px * var(--text-scale))/18px var(--mono); }",
        "  \"border-radius:var(--radius) var(--radius) var(--radius) 0;font:calc(10px * var(--text-scale))/1.5 var(--mono)\"",
        "  let s = format!(\"#{id}\");",
        "  @font-face { font-family: \"Geist\"; font-weight: 100 900; src: url(/app/fonts/geist-latin.woff2) format(\"woff2\"); }",
    ] {
        assert!(lint("x", clean).is_empty(), "{clean}");
    }
    // A planted Rust line is found; the same line in a test module is not.
    let rust = "fn a() {}\nconst X: &str = \"color:#fff\";\n#[cfg(test)]\nmod tests {\n const Y: &str = \"color:#000\";\n}\n";
    assert_eq!(lint("r.rs", &rust_shipped(rust)), vec!["r.rs:2: colour: const X: &str = \"color:#fff\";".to_string()]);
    // In index.html, a literal inside the generated blocks is not a stray.
    let (css, _) = split_blocks(&app_css());
    assert!(lint("i", &strip_block_comments(&css)).is_empty());
}

/// AGS-15: every status in the closed vocabulary has a row in the status
/// table that reads its pair from the contract (`--st-<status>-bg/-fg`,
/// both in colour.status), and the class the views put on a chip names
/// exactly that row (SDLC-7: `In Review` → `st-in-review`, one class).
#[test]
fn every_status_has_a_row_reading_its_pair_from_the_contract() {
    let css = app_css();
    let slug = |s: &str| s.to_lowercase().replace(' ', "-");
    let mut rows = 0;
    for s in STATUSES {
        let class = crate::st_class(s);
        let row = class.strip_prefix("st ").unwrap();
        assert_eq!(row, format!("st-{}", slug(s)));
        let line = css
            .lines()
            .find(|l| l.trim_start().starts_with(&format!(".{row} ")))
            .unwrap_or_else(|| panic!("no status-table row for {s}"));
        for part in ["bg", "fg"] {
            let tok = format!("--{row}-{part}");
            assert!(line.contains(&format!("--st-{part}: var({tok})")), "{s}: {line}");
            let t = TOKENS.iter().find(|t| t.name == tok).unwrap_or_else(|| panic!("{tok} is not in the contract"));
            assert_eq!(t.group, "colour.status", "{tok}");
        }
        rows += 1;
    }
    assert_eq!(css.lines().filter(|l| l.trim_start().starts_with(".st-")).count(), rows, "one row per status");
    let status_tokens = TOKENS.iter().filter(|t| t.group == "colour.status").count();
    assert_eq!(status_tokens, 2 * STATUSES.len(), "the contract's status pairs are the vocabulary's");
}

/// COMP-4 (COMP-1 pin 10): a framed body receives the app's tokens by name
/// (src/frame.rs) — exactly the custom properties the light `:root` block
/// defines; a token in the contract and not sent would be missing inside
/// every frame.
#[test]
fn frames_receive_every_token_the_light_block_defines() {
    let (_, blocks) = split_blocks(&app_css());
    let light = &blocks[..blocks.find("@media").unwrap()];
    let defined: std::collections::BTreeSet<String> = strip_block_comments(light)
        .split(';')
        .filter_map(|d| d.split_once(':').map(|(n, _)| n.trim().to_string()))
        .filter(|n| n.starts_with("--"))
        .collect();
    let sent: std::collections::BTreeSet<String> = crate::frame::theme_tokens().map(str::to_string).collect();
    assert_eq!(sent.len(), crate::frame::theme_tokens().count(), "a token listed twice");
    assert_eq!(sent, defined, "themes::TOKENS vs index.html's light :root block");
}

/// Tokens only the house components paint with (their sources live in the
/// tracker, ART-9's and DESIGN's blocks).
const FOR_COMPONENTS: &[&str] = &["--radius-pill"];

/// THEME-2: a token is painted, not just declared — every token of the
/// contract is read by a rule somewhere (the stylesheet, the SDK, or the
/// app's own painting of diagrams), so the contract names nothing dead.
#[test]
fn every_token_is_read_somewhere() {
    let (css, _) = split_blocks(&app_css());
    let sdk = std::fs::read_to_string(format!("{}/sdk/menger-sdk.js", root())).unwrap();
    let src: String = shipped().into_iter().map(|(_, t)| t).collect();
    for t in TOKENS.iter() {
        let used = format!("var({})", t.name);
        let named = format!("\"{}\"", t.name);
        assert!(
            css.contains(&used)
                || sdk.contains(&used)
                || sdk.contains(&named)
                || src.contains(&named)
                || menger_tickets_api::tokens::DIAGRAM.iter().any(|d| d.0 == t.name)
                || FOR_COMPONENTS.contains(&t.name),
            "{} is in the contract and nothing paints with it",
            t.name
        );
    }
}


/// AGS-18: focus is painted wherever a key can put it — the nav
/// tree's rows, the board's rows, the palette's input — and it is
/// painted as a *ring*, from the token layer. The selected row keeps
/// its own idiom (a `--row-sel` tint with an inset accent bar), so
/// "where I am" and "what is open" cannot collapse into one look.
#[test]
fn focus_is_a_token_ring_and_not_the_selected_row_s_tint() {
    let css = app_css();
    let lines: Vec<&str> = css.lines().map(str::trim).collect();
    let start = lines
        .iter()
        .position(|l| l.starts_with("#nav .row:focus,"))
        .expect("no focus rule for the nav and board rows");
    let block: String = lines[start..]
        .iter()
        .take_while(|l| !l.starts_with('}'))
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    for reachable in ["#nav .row:focus", "tbody#board tr:focus", "#palette-input:focus"] {
        assert!(block.contains(reachable), "{reachable} has no focus ring: {block}");
    }
    assert!(block.contains("outline:") && block.contains("var(--accent)"), "{block}");
    // A ring, not a fill: the selected row is the one with a tint.
    assert!(!block.contains("background"), "focus paints a background: {block}");
    let sel = lines
        .iter()
        .find(|l| l.starts_with("#nav .row.sel {"))
        .expect("no selected-row rule");
    assert!(sel.contains("var(--row-sel)") && sel.contains("inset"), "{sel}");
}
