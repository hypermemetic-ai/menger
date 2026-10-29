//! Sources the compiler refuses, each with the line and what the refusal
//! names — shared by src/tests.rs (native) and tests/wasm.rs (the same
//! refusal from the module).
#![allow(dead_code)]

pub struct Case {
    pub what: &'static str,
    pub src: String,
    pub line: u32,
    pub found: &'static str,
}

fn case(what: &'static str, src: String, line: u32, found: &'static str) -> Case {
    Case { what, src, line, found }
}

/// The template starts on line 5.
const HEAD: &str = "<script>\n  prop(\"id\");\n</script>\n\n";

/// Forms outside the subset.
pub fn unknown_forms() -> Vec<Case> {
    vec![
        case("{#await}", format!("{HEAD}<div>\n{{#await load}}<p>…</p>{{/await}}\n</div>\n"), 6, "{#await"),
        case("bind:value", format!("{HEAD}<div>\n  <input bind:value={{name}}/>\n</div>\n"), 6, "bind:value"),
        // A bare <slot> (Svelte's default slot) is not the sections form.
        case("a bare <slot>", format!("{HEAD}<div>\n\n  <slot/>\n</div>\n"), 7, "<slot"),
        // An {#if} left open at the end of the file: refused at its opening line.
        case("an {#if} open at the end", format!("{HEAD}<div>\n</div>\n{{#if open}}\n  <p>x</p>\n"), 7, "{#if"),
        // An {#if} an element's close runs into: refused at the if's line.
        case("an {#if} open at </div>", format!("{HEAD}<div>\n{{#if open}}\n  <p>x</p>\n</div>\n"), 6, "{#if"),
        case("an unknown tag", format!("{HEAD}<section>\n  <frobnicate>x</frobnicate>\n</section>\n"), 6, "frobnicate"),
        case("{#key}", format!("{HEAD}{{#key id}}<p/>{{/key}}\n"), 5, "{#key"),
        // UI-10: {@html expr} is in the subset; another {@…} is not.
        case("{@debug}", format!("{HEAD}<p>\n  {{@debug body}}\n</p>\n"), 6, "{@debug"),
    ]
}

/// A `<slot>` takes a literal name, no `on:`, no children; a name once
/// outside an if/else.
pub fn slot_refusals() -> Vec<Case> {
    vec![
        case("a slot's name an expression", format!("{HEAD}<div>\n  <slot name={{x}}/>\n</div>\n"), 6, "name"),
        case("a slot with on:", format!("{HEAD}<div>\n  <slot name=\"main\" on:click={{f}}/>\n</div>\n"), 6, "on:click"),
        case("a slot with children", format!("{HEAD}<slot name=\"main\">x</slot>\n"), 5, "<slot"),
        case(
            "a slot's name twice",
            format!("{HEAD}<slot name=\"main\"/>\n<div>\n  <slot name=\"main\"/>\n</div>\n"),
            7,
            "main",
        ),
    ]
}

/// Lines counted in the whole file, wherever the script sits.
pub fn line_offsets() -> Vec<Case> {
    vec![
        case(
            "the template before the script",
            "<div>\n  {#await x}{/await}\n</div>\n<script>\n  prop(\"id\");\n</script>\n".into(),
            2,
            "{#await",
        ),
        case(
            "the script after blank lines",
            "\n\n\n<script>\n  prop(\"id\");\n\n  fn spin(a) { a }\n</script>\n<p/>\n".into(),
            7,
            "spin",
        ),
        case(
            "a multi-line live without key",
            "<p/>\n<script>\n  let rows = live(#{\n    snapshot: \"tickets.list\",\n    changes: \"tickets.watch\"\n  });\n</script>\n".into(),
            3,
            "key",
        ),
    ]
}

pub fn all() -> Vec<Case> {
    let mut v = unknown_forms();
    v.extend(slot_refusals());
    v.extend(line_offsets());
    v
}
