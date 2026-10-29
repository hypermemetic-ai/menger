//! UI-7: the compiler, sealed — fuel and memory bound every run, and a
//! module that imports anything is refused at start. The real module is
//! `dist/ui-compiler.wasm`, built once per test binary by
//! `scripts/ui-compiler-wasm.sh` (UI-5); the hostile ones are written here
//! as text, each with UI-5's exports.

use super::*;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// `dist/ui-compiler.wasm`, built once per test binary.
fn module_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the workspace").to_path_buf();
        let status = std::process::Command::new("bash")
            .arg(root.join("scripts/ui-compiler-wasm.sh"))
            .current_dir(&root)
            .stdout(std::process::Stdio::null())
            .status()
            .expect("scripts/ui-compiler-wasm.sh runs");
        assert!(status.success(), "scripts/ui-compiler-wasm.sh failed: {status}");
        std::fs::read(root.join("dist/ui-compiler.wasm")).expect("dist/ui-compiler.wasm was written")
    })
}

/// A module with UI-5's exports whose `compile` grows memory by `pages`
/// (64 KiB each) at once: grown, it answers `{ ok }`; refused, it traps.
fn greedy(pages: u32) -> Vec<u8> {
    let answer = r#"{"ok":{"tree":"grew","handlers":""}}"#;
    let wat = format!(
        r#"(module
  (memory (export "memory") 1)
  (data (i32.const 0) "{escaped}")
  (func (export "alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "free") (param i32 i32))
  (func (export "compile") (param i32 i32 i32 i32) (result i64)
    (if (i32.eq (memory.grow (i32.const {pages})) (i32.const -1)) (then unreachable))
    (i64.const {len})))"#,
        escaped = answer.replace('"', "\\\""),
        len = answer.len(),
    );
    wat::parse_str(&wat).expect("the greedy module parses")
}

/// A module with UI-5's exports whose `compile` loops for ever.
fn looping() -> Vec<u8> {
    let wat = r#"(module
  (memory (export "memory") 1)
  (func (export "alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "free") (param i32 i32))
  (func (export "compile") (param i32 i32 i32 i32) (result i64)
    (loop $again (br $again))
    (i64.const 0)))"#;
    wat::parse_str(wat).expect("the looping module parses")
}

/// A module with UI-5's exports that also imports `env::clock`.
fn importing() -> Vec<u8> {
    let wat = r#"(module
  (import "env" "clock" (func $clock (result i64)))
  (memory (export "memory") 1)
  (func (export "alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "free") (param i32 i32))
  (func (export "compile") (param i32 i32 i32 i32) (result i64) (call $clock)))"#;
    wat::parse_str(wat).expect("the importing module parses")
}

/// A million nested `{#if}`: the source UI-5's harness runs out of fuel or
/// refuses.
fn million_ifs() -> String {
    let n = 1_000_000;
    let mut src = String::with_capacity(n * 12 + 32);
    src.push_str("<script>\n</script>\n");
    for _ in 0..n {
        src.push_str("{#if x}");
    }
    for _ in 0..n {
        src.push_str("{/if}");
    }
    src
}

#[test]
fn fuel_and_memory_are_bounded() {
    assert_eq!((FUEL, MEMORY_MAX), (100_000_000, 64 * 1024 * 1024), "10⁸ fuel, 64 MiB");
    let compiler = Compiler::new(module_bytes()).expect("the real module is taken");

    // The million {#if}: compile_failed naming the component, within 2 s.
    let src = million_ifs();
    let start = Instant::now();
    let answer = compiler.compile("deep-if", &src);
    let took = start.elapsed();
    match answer {
        Err(f) => {
            assert_eq!(f.name, "deep-if", "the failure names the component: {f:?}");
            assert!(!f.message.is_empty(), "it says why: {f:?}");
        }
        Ok(c) => panic!("a million nested {{#if}} must not compile: {} bytes of tree", c.tree.len()),
    }
    assert!(took < Duration::from_secs(2), "the answer took {took:?}, not within 2 s");

    // A module that never stops: out of fuel, naming the component, within 2 s.
    let spinner = Compiler::new(&looping()).expect("a module with no imports is taken");
    let start = Instant::now();
    match spinner.compile("spinner", "<p/>\n") {
        Err(f) => {
            assert_eq!(f.name, "spinner", "the failure names the component: {f:?}");
            assert!(f.message.contains("fuel"), "it says the fuel ran out: {f:?}");
        }
        Ok(c) => panic!("a loop must not answer: {c:?}"),
    }
    assert!(start.elapsed() < Duration::from_secs(2), "the loop was stopped within 2 s, took {:?}", start.elapsed());

    // Under the limit a grow is allowed: 16 MiB more answers.
    let modest = Compiler::new(&greedy(256)).expect("a module with no imports is taken");
    let grew = modest.compile("modest", "<p/>\n").expect("16 MiB is within 64 MiB");
    assert_eq!(grew.tree, "grew", "{grew:?}");

    // Past it, the grow is stopped and the run fails, naming the component.
    let greedy = Compiler::new(&greedy(2048)).expect("a module with no imports is taken");
    let start = Instant::now();
    match greedy.compile("greedy", "<p/>\n") {
        Err(f) => assert_eq!(f.name, "greedy", "the failure names the component: {f:?}"),
        Ok(c) => panic!("128 MiB must not be granted: answered {c:?}"),
    }
    assert!(start.elapsed() < Duration::from_secs(2), "stopped promptly");
}

#[test]
fn the_module_has_no_imports() {
    // The real module instantiates with an empty linker and compiles.
    let compiler = Compiler::new(module_bytes()).expect("dist/ui-compiler.wasm is taken: it imports nothing");
    let src = "<script>\n  prop(\"id\");\n</script>\n<p>{id}</p>\n";
    let c = compiler.compile("id-line", src).expect("a small source compiles in the sealed module");
    assert!(c.tree.contains("\"name\": \"id-line\""), "the tree names the component: {}", c.tree);
    assert!(c.tree.ends_with('\n'), "UI-4's pretty form, a trailing newline");

    // A module that imports anything is refused at start, naming it.
    match Compiler::new(&importing()) {
        Err(ModuleRefused::Imports(names)) => {
            assert_eq!(names, ["env::clock"], "each import named, module::name");
        }
        Err(other) => panic!("refused, but not for its import: {other}"),
        Ok(_) => panic!("a module that imports env::clock must be refused at start"),
    }
    let message = Compiler::new(&importing()).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(message.contains("env::clock"), "the refusal names the import: {message}");
}
