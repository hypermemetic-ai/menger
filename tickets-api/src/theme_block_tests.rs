//! THEME-3: the ```theme block — what it may say, and every refusal named
//! by its line.

use super::*;

const PANTRY: &str = r#"@import url("https://fonts.googleapis.com/css2?family=Fraunces&display=swap");
/* the house, warmer */
:root {
  --accent: #d4572a;
  --font-display: "Fraunces", serif;
  --radius: 14px;
}
:root[data-theme="dark"] { --accent: #f08a5d; }
@font-face { font-family: "Inter"; src: url(https://example.org/inter.woff2) format("woff2"); font-display: swap; }
m-code::part(frame), m-card::part(box):hover { border-radius: 14px; background: var(--panel); }
"#;

#[test]
fn a_theme_block_says_tokens_fonts_and_parts() {
    let d = parse("pantry", Some("ocean"), PANTRY).unwrap();
    assert_eq!(d.extends.as_deref(), Some("ocean"));
    assert_eq!(d.light.get("--accent").map(String::as_str), Some("#d4572a"));
    assert_eq!(d.light.get("--font-display").map(String::as_str), Some("\"Fraunces\", serif"));
    assert_eq!(d.dark.get("--accent").map(String::as_str), Some("#f08a5d"));
    assert_eq!(d.imports, vec!["https://fonts.googleapis.com/css2?family=Fraunces&display=swap".to_string()]);
    assert_eq!(d.rules.len(), 2);
    assert!(d.rules[0].starts_with("@font-face{font-family:\"Inter\";src:url(https://example.org/inter.woff2) format(\"woff2\");"), "{}", d.rules[0]);
    assert_eq!(d.rules[1], "m-code::part(frame),m-card::part(box):hover{border-radius:14px;background:var(--panel);}");
}

#[test]
fn the_opening_line_names_the_theme_and_its_parent() {
    assert_eq!(info("theme pantry extends=ocean").unwrap().unwrap(), ("pantry".into(), Some("ocean".into())));
    assert_eq!(info("theme Pantry").unwrap().unwrap(), ("pantry".into(), None));
    assert_eq!(info("theme pantry extends=default").unwrap().unwrap(), ("pantry".into(), None));
    assert!(info("rust").is_none());
    assert!(info("themes x").is_none());
    for (bad, why) in [
        ("theme", "names no theme"),
        ("theme default", "`default` is the contract's"),
        ("theme 9x", "is not a theme name"),
        ("theme a b", "only extends=parent"),
        ("theme a extends=a", "extends itself"),
    ] {
        let e = info(bad).unwrap().unwrap_err();
        assert!(e.message.contains(why), "{bad}: {e}");
    }
}

/// Every refusal names its line — the line in the block, counting from the
/// first after the fence — and says what is wrong.
#[test]
fn every_refusal_names_its_line() {
    for (src, line, why) in [
        (":root { --nope: 1px; }", 1, "not a token of the contract"),
        ("\n:root {\n  --accent: red;\n  color: red;\n}", 4, "a theme sets tokens there"),
        ("body { color: red; }", 1, "a theme's rules are :root"),
        ("\n\n.x, m-a::part(b) { color: red; }", 3, "a theme's rules are"),
        ("m-code::part(frame)::before { color: red; }", 1, "a theme's rules are"),
        (":root[data-theme=\"sepia\"] { --accent: red; }", 1, "a theme's rules are"),
        ("@import url(\"http://fonts.example/x.css\");", 1, "only an https URL"),
        ("@import url(\"https://x.example/a.css\")", 1, "without its ;"),
        ("@font-face { font-family: x;\n src: url(http://x.example/a.woff2); }", 2, "only https URLs"),
        ("@font-face { color: red; }", 1, "in @font-face: one of"),
        ("m-code::part(frame) { background: url(javascript:alert(1)); }", 1, "behaviour, not looks"),
        ("m-code::part(frame) { width: expression(1); }", 1, "behaviour, not looks"),
        ("m-code::part(frame) { -moz-binding: url(https://x.example/a.xml); }", 1, "behaviour, not looks"),
        (":root { --accent: a}b; }", 1, "is not a rule"),
        (":root { --accent }", 1, "is not a declaration"),
        (":root { --accent: red;", 1, "never closes"),
        ("/* open", 1, "a comment that never ends"),
        ("--accent: red;", 1, "is not a rule"),
        ("@media (x) { :root { --accent: red; } }", 1, "do not nest"),
        (":root { --accent: url(http://x.example/i.png); }", 1, "only https"),
    ] {
        let e = parse("t", None, src).expect_err(src);
        assert_eq!(e.line, line, "{src}: {e}");
        assert!(e.message.contains(why), "{src}: {e}");
    }
}

#[test]
fn urls_are_https_and_cannot_end_their_url() {
    assert!(url_ok("https://fonts.gstatic.com/s/inter/v12/a.woff2"));
    assert!(url_ok("HTTPS://x.example/a"));
    for bad in ["http://x.example/a", "//x.example/a", "data:font/woff2;base64,AA", "https://", "https://x/a b", "https://x/a)", "javascript:1"] {
        assert!(!url_ok(bad), "{bad}");
    }
}

#[test]
fn a_body_reads_its_blocks_and_only_its_fences() {
    let body = "---\ntheme: pantry\n---\n# Pantry\n\n```theme pantry\n:root { --accent: #d4572a; }\n```\n\n    ```theme indented\n    :root {}\n    ```\n\n````markdown\n```theme example\n:root { --nope: 1; }\n```\n````\n\n```theme sea extends=pantry\n:root[data-theme=dark] { --bg: #001; }\n```\n";
    let defs = read(body).unwrap();
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["pantry", "sea"]);
    assert_eq!(defs[1].extends.as_deref(), Some("pantry"));
    assert_eq!(defs[1].dark.get("--bg").map(String::as_str), Some("#001"));
    let twice = "```theme a\n:root{}\n```\n\n```theme a\n:root{}\n```\n";
    assert_eq!(read(twice).unwrap_err().0, "a");
    let bad = "```theme a\n:root {\n  --accent: red;\n  --nope: 1;\n}\n```\n";
    let (name, e) = read(bad).unwrap_err();
    assert_eq!((name.as_str(), e.line), ("a", 3), "{e}");
    assert!(read("no themes here").unwrap().is_empty());
}

/// Google Fonts' URLs carry `;` (`wght@400;600`): a statement ends at a `;`
/// outside quotes and parentheses.
#[test]
fn a_semicolon_inside_a_url_does_not_end_its_statement() {
    let css = "@import url(\"https://fonts.googleapis.com/css2?family=Inter:wght@400;600&display=swap\");\n@font-face { font-family: \"X\"; src: url(\"https://x.example/a.woff2?v=1;2\") format(\"woff2\"); }\n:root { --accent: #d4572a; }";
    let d = parse("t", None, css).unwrap();
    assert_eq!(d.imports, ["https://fonts.googleapis.com/css2?family=Inter:wght@400;600&display=swap"]);
    assert!(d.rules[0].contains("src:url(\"https://x.example/a.woff2?v=1;2\") format(\"woff2\");"), "{:?}", d.rules);
    assert_eq!(d.light.get("--accent").map(String::as_str), Some("#d4572a"));
}
