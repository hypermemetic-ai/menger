//! MACRO-31: the docs say what the wire does — under `typed_params` a key
//! no parameter claims is refused, `-32602 invalid_params` with the words
//! `unknown field` (synapse's heal word). README's "Typed parameters
//! (ACCESS-8)" and AGENTS.md's "Every parameter is a type" paragraph each
//! say it, beside the rule they already state.

use std::path::Path;

fn read(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The text from the line starting `start` to the next blank line after a
/// heading (`##`) or the next heading, whichever comes first — a section's
/// prose or a paragraph.
fn section<'a>(text: &'a str, start: &str, stop_at_blank: bool) -> &'a str {
    let at = text.find(start).unwrap_or_else(|| panic!("no `{start}`"));
    let rest = &text[at..];
    let mut end = rest.len();
    let mut offset = 0;
    for (i, line) in rest.lines().enumerate() {
        if i > 0 && (line.starts_with('#') || (stop_at_blank && line.trim().is_empty())) {
            end = offset;
            break;
        }
        offset += line.len() + 1;
    }
    &rest[..end.min(rest.len())]
}

fn says_an_unknown_key_is_refused(where_: &str, text: &str) {
    assert!(text.contains("MACRO-31"), "{where_} names MACRO-31:\n{text}");
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.split(". ").any(|sentence| sentence.contains("unknown field `") && sentence.contains("refused")),
        "one sentence of {where_} says an unknown key is refused, in the refusal's words (unknown field `…`):\n{text}"
    );
}

#[test]
fn the_readme_says_an_unknown_parameter_is_refused() {
    let readme = read("README.md");
    let s = section(&readme, "### Typed parameters (ACCESS-8)", false);
    says_an_unknown_key_is_refused("README \"Typed parameters (ACCESS-8)\"", s);
}

#[test]
fn agents_md_says_an_unknown_parameter_is_refused() {
    let agents = read("AGENTS.md");
    let s = section(&agents, "**Every parameter is a type**", true);
    says_an_unknown_key_is_refused("AGENTS.md \"Every parameter is a type\"", s);
}
