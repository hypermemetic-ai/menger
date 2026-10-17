//! LOOK-8: every move the app makes answers to the Motion setting
//! (DESIGN-39's contract, src/motion.rs). The coverage test fails when one
//! escapes it:
//!
//! - **The stylesheet** (index.html): the global `none` rule and the
//!   reduced-motion fallback are there, word for word; the app has one
//!   `<style>`, no inline `style="…"` that animates and no shadow
//!   `<template>` the rule cannot reach; no `animation` or `transition` is
//!   `!important` but the two global rules (an `!important` would outrank
//!   them); and every animation that runs forever — ornament, by this
//!   ticket's reading of DESIGN-39 — has a `calm` rule that names its
//!   selector.
//! - **The script** (frontend/src): every `.animate(`,
//!   `request_animation_frame`, smooth scroll and inline `transition` /
//!   `animation` either sits in a function that asks `motion::current()`,
//!   or is one of the [`NOT_MOTION`] uses — a next frame taken to measure,
//!   place or focus, which moves nothing — counted per file, so a new one
//!   fails until it is gated or listed here with its reason.

use std::path::Path;

use crate::tokens_lint::app_css;

/// The global rule: under `none` nothing moves.
const NONE_RULE: &str = "html[data-motion=\"none\"] *, html[data-motion=\"none\"] *::before, html[data-motion=\"none\"] *::after {\n    animation: none !important; transition: none !important;\n  }";

/// Before the script stamps the setting, reduced motion is `none`.
const FALLBACK_RULE: &str = "@media (prefers-reduced-motion: reduce) {\n    html:not([data-motion]) *, html:not([data-motion]) *::before, html:not([data-motion]) *::after {\n      animation: none !important; transition: none !important;\n    }\n  }";

/// Uses of a next frame that move nothing: (the file's stem, how many, why).
const NOT_MOTION: &[(&str, usize, &str)] = &[
    ("annotate", 1, "places the annotation layer after layout"),
    ("calendar", 2, "focuses a day after the grid is drawn"),
    ("filter", 1, "focuses the panel's search, and brings a section asked for into view at once"),
    ("grow", 1, "fits a textarea to its text"),
    ("phonebar", 1, "the tree measures the sheet it now fills"),
    ("saved", 7, "focuses a menu's first item; fits the menu to the viewport"),
    ("waves", 2, "measures the lanes; scrolls a card into view at once"),
    ("when_view", 1, "focuses the picker after it opens"),
];

/// What the script may animate with.
const MOVES: &[&str] = &[
    ".animate(",
    "request_animation_frame",
    "ScrollBehavior::Smooth",
    "\"smooth\"",
    "scroll-behavior",
    "set_property(\"transition\"",
    "set_property(\"animation\"",
];

fn src_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// `text` without its `/* … */` comments.
fn uncommented(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = rest[i + 2..].split_once("*/").map_or("", |(_, r)| r);
    }
    out.push_str(rest);
    out
}

/// Every style rule in `css` as (selector, declarations), at-rules opened
/// (`@media`, `@container`, `@supports`) and `@keyframes` skipped.
fn rules(css: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = css;
    loop {
        let Some(open) = rest.find('{') else { break };
        let head = rest[..open].trim();
        // The head is everything after the last `}` or `;` before it.
        let head = head.rsplit(['}', ';']).next().unwrap_or(head).trim().to_string();
        // The block's end, nesting counted.
        let mut depth = 0usize;
        let mut end = open;
        for (i, c) in rest[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &rest[open + 1..end];
        if head.starts_with("@keyframes") {
        } else if head.starts_with('@') {
            out.extend(rules(body));
        } else {
            out.push((head, body.to_string()));
        }
        rest = &rest[end + 1..];
    }
    out
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn nothing_the_stylesheet_moves_escapes_the_setting() {
    let html = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("index.html")).unwrap();
    assert_eq!(html.matches("<style").count(), 1, "the app's one stylesheet: a second would be one more to hold");
    assert!(!html.contains("<template"), "a shadow root's styles are out of the global rule's reach");
    for attr in html.split("style=\"").skip(1) {
        let v = attr.split('"').next().unwrap_or("");
        assert!(!v.contains("animation") && !v.contains("transition"), "an inline style moves: {v}");
    }
    let css = app_css();
    assert!(css.contains(NONE_RULE), "the global rule: under Motion none nothing moves");
    assert!(css.contains(FALLBACK_RULE), "the fallback: reduced motion before the setting is stamped");
    let css = uncommented(&css);
    let all = rules(&css);
    assert!(all.len() > 500, "the parser read the stylesheet ({} rules)", all.len());
    let calm: Vec<String> = all.iter().filter(|(s, _)| s.contains("html[data-motion=\"calm\"]")).map(|(s, _)| squash(s)).collect();
    let mut escapes = Vec::new();
    for (sel, body) in &all {
        let global = sel.contains("data-motion=\"none\"] *") || sel.contains(":not([data-motion]) *");
        for decl in body.split(';') {
            let decl = decl.trim();
            let moving = decl.starts_with("animation") || decl.starts_with("transition");
            if !moving {
                continue;
            }
            if decl.contains("!important") && !global {
                escapes.push(format!("{sel} {{ {decl} }} — !important outranks the global rule"));
            }
            if decl.contains("infinite") && !sel.contains("data-motion") {
                let sel = squash(sel);
                let held = sel.split(',').map(str::trim).all(|one| calm.iter().any(|c| c.contains(one)));
                if !held {
                    escapes.push(format!("{sel} {{ {decl} }} — runs forever (ornament) and no calm rule stops it"));
                }
            }
        }
    }
    assert!(escapes.is_empty(), "moves that escape the Motion setting:\n{}", escapes.join("\n"));
}

#[test]
fn nothing_the_script_moves_escapes_the_setting() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    let mut ungated: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for path in files {
        let name = path.strip_prefix(src_dir()).unwrap().to_string_lossy().replace('\\', "/");
        let stem = name.trim_end_matches(".rs").to_string();
        if name.ends_with("_tests.rs") || name == "motion_lint.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        // The code before its test module.
        let code = text.find("#[cfg(test)]\nmod ").map_or(text.as_str(), |i| &text[..i]);
        let lines: Vec<&str> = code.lines().collect();
        for (n, line) in lines.iter().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || !MOVES.iter().any(|m| line.contains(m)) {
                continue;
            }
            // The function it is in: the nearest `fn` above.
            let start = (0..=n).rev().find(|&i| {
                let l = lines[i].trim_start();
                ["fn ", "pub fn ", "pub(crate) fn ", "async fn ", "pub(crate) async fn ", "pub async fn "].iter().any(|p| l.starts_with(p))
            });
            let gate = if stem == "motion" { "current()" } else { "motion::current()" };
            let gated = start.is_some_and(|s| lines[s..=n].iter().any(|l| l.contains(gate)));
            if !gated {
                ungated.entry(stem.clone()).or_default().push(format!("{name}:{} {}", n + 1, line.trim()));
            }
        }
    }
    let mut wrong = Vec::new();
    for (file, hits) in &ungated {
        match NOT_MOTION.iter().find(|(f, _, _)| f == file) {
            Some((_, n, _)) if *n == hits.len() => {}
            Some((_, n, why)) => wrong.push(format!("{file}: {} ungated, {n} listed ({why}) — gate the new one on motion::current() or list it:\n  {}", hits.len(), hits.join("\n  "))),
            None => wrong.push(format!("{file}: not gated on motion::current():\n  {}", hits.join("\n  "))),
        }
    }
    for (file, n, _) in NOT_MOTION {
        if !ungated.contains_key(*file) {
            wrong.push(format!("{file}: listed with {n}, has none — take it off the list"));
        }
    }
    assert!(wrong.is_empty(), "script moves that escape the Motion setting:\n{}", wrong.join("\n"));
}

#[test]
fn the_parser_reads_nesting_and_skips_keyframes() {
    let css = "a { x: 1; } @media (w) { b, c { y: 2; } } @keyframes k { from { z: 3; } } d { animation: k 1s infinite; }";
    let r = rules(css);
    assert_eq!(r.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(), ["a", "b, c", "d"]);
    assert_eq!(uncommented("a /* b */ c"), "a  c");
}
