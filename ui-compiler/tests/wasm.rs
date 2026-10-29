//! UI-5: the compiler as a wasm module — nothing imported, exactly
//! `compile` / `alloc` / `free` / `memory` exported, no wasm-bindgen in it,
//! run in wasmtime with no WASI under 10⁸ fuel; a run out of fuel is
//! `compile_failed`, and a refusal is the native one.

mod common;

#[path = "common/cases.rs"]
mod cases;

use std::time::{Duration, Instant};

use common::Outcome;

#[test]
fn the_wasm_has_no_imports() {
    let bytes = common::module_bytes();
    for (id, _, range) in common::sections(bytes) {
        if id == 2 {
            assert_eq!(bytes[range.start], 0, "the import section is empty");
        }
    }
    let module = common::module();
    let imports: Vec<String> = module.imports().map(|i| format!("{}::{}", i.module(), i.name())).collect();
    assert!(imports.is_empty(), "the module imports {imports:?}");
    let mut exports: Vec<&str> = module.exports().map(|e| e.name()).collect();
    exports.sort();
    assert_eq!(exports, ["alloc", "compile", "free", "memory"], "the module's exports");
    // No wasm-bindgen: no export and no custom section of its.
    let customs: Vec<String> = common::sections(bytes).into_iter().filter_map(|(_, n, _)| n).collect();
    for name in exports.iter().copied().chain(customs.iter().map(String::as_str)) {
        assert!(!name.starts_with("__wbindgen"), "wasm-bindgen is in the module: {name}");
    }
}

#[test]
fn it_runs_in_wasmtime_under_fuel() {
    // The contract's example, in wasmtime with no WASI and 10⁸ fuel.
    let src = common::golden("ticket-row", "svelte");
    match common::run("ticket-row", &src, common::FUEL) {
        Outcome::Ok(c) => {
            assert_eq!(c.tree, common::golden("ticket-row", "json"), "the wasm tree is the golden");
            assert_eq!(c.handlers, common::golden("ticket-row", "rhai"), "the wasm handlers are the golden");
        }
        other => panic!("the golden did not compile in wasmtime under 10⁸ fuel: {other:?}"),
    }

    // The same golden on 10³ fuel: it runs out, and that is compile_failed.
    match common::run("ticket-row", &src, 1_000) {
        Outcome::CompileFailed { fuel: true, .. } => {}
        other => panic!("10³ fuel must run out: {other:?}"),
    }

    // A million nested {#if}: out of fuel or refused (a nesting limit) —
    // never compiled, never another trap (a stack overflow), within a second.
    let n = 1_000_000;
    let mut src = String::with_capacity(n * 12 + 16);
    src.push_str("<script>\n</script>\n");
    for _ in 0..n {
        src.push_str("{#if x}");
    }
    for _ in 0..n {
        src.push_str("{/if}");
    }
    common::module(); // built and compiled before the clock starts
    let start = Instant::now();
    let outcome = common::run("deep-if", &src, common::FUEL);
    let took = start.elapsed();
    match outcome {
        Outcome::CompileFailed { fuel: true, .. } | Outcome::Refused(_) => {}
        other => panic!("a million nested {{#if}} must run out of fuel or be refused, got {other:?}"),
    }
    assert!(took < Duration::from_secs(1), "the answer took {took:?}, not within a second");
}

#[test]
fn a_name_or_a_source_that_is_not_utf8_is_refused_at_line_0() {
    for (name, src) in [(&b"ticket-row"[..], &[0xffu8][..]), (&[0xffu8][..], &b"<p/>\n"[..])] {
        match common::run_bytes(name, src, common::FUEL) {
            Outcome::Refused(r) => {
                assert_eq!(r.line, 0, "not in the file: line 0 ({})", r.message);
                assert!(r.message.contains("UTF-8"), "the refusal says why: {}", r.message);
            }
            other => panic!("not UTF-8 must be refused: {other:?}"),
        }
    }
}

#[test]
fn a_refusal_in_wasm_is_the_native_one() {
    for c in cases::all() {
        let native = menger_ui_compiler::compile("test-view", &c.src);
        let wasm = common::run("test-view", &c.src, common::FUEL);
        match (native, wasm) {
            (Err(n), Outcome::Refused(w)) => assert_eq!(n, w, "{}: the wasm refusal differs", c.what),
            (n, w) => panic!("{}: both must refuse — native {n:?}, wasm {w:?}", c.what),
        }
    }
}

/// Acceptance 4's measurement (not a gate: timings on a shared machine):
/// a component of fifty nodes natively and in wasmtime.
/// `cargo test -p menger-ui-compiler --release --test wasm -- --ignored --nocapture`
#[test]
#[ignore]
fn fifty_nodes_timings() {
    let mut src = String::from(
        "<script>\n  prop(\"id\");\n  state(#{ open: false });\n  fn toggle(state, ev) { state.open = !state.open; state }\n</script>\n<div class=\"list\" on:click={toggle}>\n",
    );
    // 1 div + 1 ul + 16 × (li, its text, span, its text) = 66 nodes, past fifty.
    src.push_str("<ul>\n");
    for i in 0..16 {
        src.push_str(&format!("  <li data-i=\"{i}\">{{state.open}}<span>{i}</span></li>\n"));
    }
    src.push_str("</ul>\n</div>\n");
    let tree = menger_ui_compiler::compile("fifty-nodes", &src).expect("compiles");
    let nodes = tree.tree.matches("\"el\"").count() + tree.tree.matches("\"text\"").count();
    let n = 200;
    let t = std::time::Instant::now();
    for _ in 0..n {
        menger_ui_compiler::compile("fifty-nodes", &src).expect("compiles");
    }
    let native = t.elapsed() / n;
    common::module();
    let t = std::time::Instant::now();
    for _ in 0..20 {
        assert!(matches!(common::run("fifty-nodes", &src, common::FUEL), Outcome::Ok(_)));
    }
    let wasm = t.elapsed() / 20;
    println!("{nodes} nodes: native {native:?} per compile, wasmtime {wasm:?} per run (fresh instance, 10^8 fuel)");
}
