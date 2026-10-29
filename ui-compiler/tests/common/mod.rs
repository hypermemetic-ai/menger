//! The harness the tests run the wasm module in, the way push (UI-7) will:
//! wasmtime, no WASI, no imports, under a fuel limit. A run that traps — fuel
//! exhausted above all — is `compile_failed`, never a hang.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use menger_ui_compiler::{Answer, Compiled, Refusal};
use wasmtime::{Config, Engine, Instance, Module, Store, Trap};

/// The fuel push gives one component: 10⁸.
pub const FUEL: u64 = 100_000_000;

/// What the harness answers for one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ok(Compiled),
    Refused(Refusal),
    /// The run trapped; `fuel` when it was fuel that ran out.
    CompileFailed { fuel: bool, reason: String },
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the workspace").to_path_buf()
}

/// `dist/ui-compiler.wasm`, built once per test binary by
/// `scripts/ui-compiler-wasm.sh` — the module the provides table names.
pub fn module_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let root = workspace_root();
        let status = std::process::Command::new("bash")
            .arg(root.join("scripts/ui-compiler-wasm.sh"))
            .current_dir(&root)
            .status()
            .expect("scripts/ui-compiler-wasm.sh runs");
        assert!(status.success(), "scripts/ui-compiler-wasm.sh failed: {status}");
        std::fs::read(root.join("dist/ui-compiler.wasm")).expect("dist/ui-compiler.wasm was written")
    })
}

pub fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::new();
        config.consume_fuel(true);
        Engine::new(&config).expect("a fuel-metered engine")
    })
}

pub fn module() -> &'static Module {
    static MODULE: OnceLock<Module> = OnceLock::new();
    MODULE.get_or_init(|| Module::new(engine(), module_bytes()).expect("the module compiles in wasmtime"))
}

/// Compile `src` in a fresh instance with `fuel`, nothing linked.
pub fn run(src: &str, fuel: u64) -> Outcome {
    let mut store = Store::new(engine(), ());
    store.set_fuel(fuel).expect("fuel is on");
    let failed = |e: wasmtime::Error| Outcome::CompileFailed {
        fuel: matches!(e.downcast_ref::<Trap>(), Some(Trap::OutOfFuel)),
        reason: format!("{e:?}"),
    };
    let instance = match Instance::new(&mut store, module(), &[]) {
        Ok(i) => i,
        Err(e) => return failed(e),
    };
    let memory = instance.get_memory(&mut store, "memory").expect("memory is exported");
    let alloc = instance.get_typed_func::<u32, u32>(&mut store, "alloc").expect("alloc(len) -> ptr");
    let free = instance.get_typed_func::<(u32, u32), ()>(&mut store, "free").expect("free(ptr, len)");
    let compile = instance.get_typed_func::<(u32, u32), u64>(&mut store, "compile").expect("compile(ptr, len) -> ptr << 32 | len");

    let len = src.len() as u32;
    let ptr = match alloc.call(&mut store, len) {
        Ok(p) => p,
        Err(e) => return failed(e),
    };
    memory.write(&mut store, ptr as usize, src.as_bytes()).expect("the source fits where alloc put it");
    let packed = match compile.call(&mut store, (ptr, len)) {
        Ok(p) => p,
        Err(e) => return failed(e),
    };
    let (out_ptr, out_len) = ((packed >> 32) as u32, packed as u32);
    let mut out = vec![0u8; out_len as usize];
    memory.read(&store, out_ptr as usize, &mut out).expect("the answer is inside memory");
    let _ = free.call(&mut store, (ptr, len));
    let _ = free.call(&mut store, (out_ptr, out_len));
    match serde_json::from_slice::<Answer>(&out).expect("the answer is { ok } | { err }") {
        Answer::Ok(c) => Outcome::Ok(c),
        Answer::Err(r) => Outcome::Refused(r),
    }
}

/// Every golden's name (`tests/golden/<name>.svelte`), sorted.
pub fn goldens() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("tests/golden")
        .filter_map(|e| {
            let p = e.ok()?.path();
            (p.extension()? == "svelte").then(|| p.file_stem()?.to_str().map(str::to_string))?
        })
        .collect();
    names.sort();
    assert!(names.len() >= 2, "the goldens are ticket-row and board-layout at least: {names:?}");
    names
}

pub fn golden(name: &str, ext: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.{ext}"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}
