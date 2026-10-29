//! UI-5: the compiler as a wasm module — nothing imported, exactly
//! `compile` / `alloc` / `free` / `memory` exported, run in wasmtime with no
//! WASI under 10⁸ fuel; a pathological source ends in `compile_failed`.

mod common;

use std::time::{Duration, Instant};

#[test]
fn the_wasm_has_no_imports() {
    let module = common::module();
    let imports: Vec<String> = module.imports().map(|i| format!("{}::{}", i.module(), i.name())).collect();
    assert!(imports.is_empty(), "the module imports {imports:?}");
    let mut exports: Vec<&str> = module.exports().map(|e| e.name()).collect();
    exports.sort();
    assert_eq!(exports, ["alloc", "compile", "free", "memory"], "the module's exports");
}

#[test]
fn it_runs_in_wasmtime_under_fuel() {
    // The contract's example, in wasmtime with no WASI and 10⁸ fuel.
    let src = common::golden("ticket-row", "svelte");
    match common::run(&src, common::FUEL) {
        common::Outcome::Ok(c) => {
            assert_eq!(c.tree, common::golden("ticket-row", "json"), "the wasm tree is the golden");
            assert_eq!(c.handlers, common::golden("ticket-row", "rhai"), "the wasm handlers are the golden");
        }
        other => panic!("the golden did not compile in wasmtime under 10⁸ fuel: {other:?}"),
    }

    // A million nested {#if}: fuel runs out, the harness answers
    // compile_failed, within a second.
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
    let outcome = common::run(&src, common::FUEL);
    let took = start.elapsed();
    match outcome {
        common::Outcome::CompileFailed { fuel: true, .. } => {}
        other => panic!("a million nested {{#if}} must run out of fuel and be compile_failed, got {other:?}"),
    }
    assert!(took < Duration::from_secs(1), "compile_failed took {took:?}, not within a second");
}
