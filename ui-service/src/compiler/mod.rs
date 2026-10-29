//! UI-7: **the compiler, sealed** — `dist/ui-compiler.wasm` (UI-5's module,
//! `--ui-compiler`) run in wasmtime with nothing it can reach: no WASI, an
//! empty linker (a module that imports anything is refused at start, naming
//! each import), one [`wasmtime::Store`] per compile with [`FUEL`] fuel and
//! [`MEMORY_MAX`] bytes of memory. The server never runs the builder's
//! compiler; it runs its own copy of the same wasm.
//!
//! The ABI is UI-5's: `alloc(len) -> ptr`, `free(ptr, len)`,
//! `compile(name_ptr, name_len, src_ptr, src_len) -> ptr << 32 | len`, the
//! answer JSON `{ "ok": { tree, handlers } } | { "err": { line, message } }`.
//! A run that traps — fuel or memory exhausted above all — is
//! [`CompileFailed`] naming the component, never a hang.

use std::path::Path;
use wasmtime::{Config, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder, Trap, TypedFunc};

/// The fuel one component's compile is given: 10⁸.
pub const FUEL: u64 = 100_000_000;

/// The most memory one compile may hold: 64 MiB.
pub const MEMORY_MAX: usize = 64 * 1024 * 1024;

/// What a component compiles to: the view tree as JSON and its handlers as
/// Rhai source (UI-4's `components/<name>.json` and `.rhai`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    pub tree: String,
    pub handlers: String,
}

/// A compile that did not answer a tree: the module refused the source (at
/// `line`, 1-based; 0 is not in the file), or the run trapped — out of fuel,
/// out of memory — (`line: 0`, the message says which).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{name} does not compile (line {line}): {message}")]
pub struct CompileFailed {
    pub name: String,
    pub line: u32,
    pub message: String,
}

/// Why a module is not taken at start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModuleRefused {
    /// The module imports something; an empty linker gives it nothing.
    #[error("the compiler module imports {}: it must import nothing", .0.join(", "))]
    Imports(Vec<String>),
    /// The bytes are no wasm module wasmtime takes, or it lacks UI-5's exports.
    #[error("the compiler module is not usable: {0}")]
    Invalid(String),
}

/// The exports UI-5's ABI names.
const EXPORTS: [&str; 4] = ["alloc", "compile", "free", "memory"];

/// The compiler module, loaded once at start; each [`Session`] a fresh
/// instance in a store of its own.
#[derive(Clone)]
pub struct Compiler {
    engine: Engine,
    module: Module,
}

impl std::fmt::Debug for Compiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compiler").finish_non_exhaustive()
    }
}

/// The store's data: the memory limit.
struct Sealed {
    limits: StoreLimits,
}

impl Compiler {
    /// The module `bytes`, checked: a module that imports anything is
    /// refused here, naming each import (`module::name`); one without
    /// UI-5's exports is refused naming what it lacks.
    pub fn new(bytes: &[u8]) -> Result<Self, ModuleRefused> {
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).map_err(|e| ModuleRefused::Invalid(format!("{e}")))?;
        let module = Module::new(&engine, bytes).map_err(|e| ModuleRefused::Invalid(format!("{e}")))?;
        let imports: Vec<String> = module.imports().map(|i| format!("{}::{}", i.module(), i.name())).collect();
        if !imports.is_empty() {
            return Err(ModuleRefused::Imports(imports));
        }
        let exports: Vec<&str> = module.exports().map(|e| e.name()).collect();
        let missing: Vec<&str> = EXPORTS.iter().copied().filter(|n| !exports.contains(n)).collect();
        if !missing.is_empty() {
            return Err(ModuleRefused::Invalid(format!("it exports no {}", missing.join(", "))));
        }
        Ok(Self { engine, module })
    }

    /// The module at `path` (`--ui-compiler`), read and [`Self::new`].
    pub fn open(path: &Path) -> Result<Self, ModuleRefused> {
        let bytes = std::fs::read(path).map_err(|e| ModuleRefused::Invalid(format!("{}: {e}", path.display())))?;
        Self::new(&bytes)
    }

    /// One push's sandbox: a store of its own with [`MEMORY_MAX`] of memory,
    /// the module instantiated with an empty linker. `name` is what a
    /// failure to start names.
    pub fn session(&self, name: &str) -> Result<Session, CompileFailed> {
        let limits = StoreLimitsBuilder::new().memory_size(MEMORY_MAX).instances(1).trap_on_grow_failure(true).build();
        let mut store = Store::new(&self.engine, Sealed { limits });
        store.limiter(|s| &mut s.limits);
        let failed = |e: wasmtime::Error| CompileFailed { name: name.to_string(), line: 0, message: trap_message(&e) };
        store.set_fuel(FUEL).map_err(failed)?;
        let linker: Linker<Sealed> = Linker::new(&self.engine);
        let instance = linker.instantiate(&mut store, &self.module).map_err(failed)?;
        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| failed(wasmtime::Error::msg("no memory exported")))?;
        let alloc = instance.get_typed_func::<u32, u32>(&mut store, "alloc").map_err(failed)?;
        let free = instance.get_typed_func::<(u32, u32), ()>(&mut store, "free").map_err(failed)?;
        let compile = instance.get_typed_func::<(u32, u32, u32, u32), u64>(&mut store, "compile").map_err(failed)?;
        Ok(Session { store, memory, alloc, free, compile, broken: false })
    }

    /// Compile `src` as the component `name` in a session of its own.
    pub fn compile(&self, name: &str, src: &str) -> Result<Compiled, CompileFailed> {
        self.session(name)?.compile(name, src)
    }
}

/// What a trap says: out of fuel, out of memory, or the trap itself.
fn trap_message(e: &wasmtime::Error) -> String {
    match e.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => format!("the compiler ran out of fuel ({FUEL})"),
        Some(Trap::AllocationTooLarge) => format!("the compiler ran out of memory ({MEMORY_MAX} bytes)"),
        Some(t) => format!("the compiler trapped: {t}"),
        None if format!("{e:?}").contains("memory") => format!("the compiler ran out of memory ({MEMORY_MAX} bytes): {e}"),
        None => format!("the compiler failed: {e}"),
    }
}

/// The module's answer: `{ "ok": { tree, handlers } } | { "err": { line, message } }`.
#[derive(serde::Deserialize)]
enum Answer {
    #[serde(rename = "ok")]
    Ok { tree: String, handlers: String },
    #[serde(rename = "err")]
    Err { line: u32, message: String },
}

/// One push's instance: every component compiled in it, each with [`FUEL`]
/// fuel, the memory limit across them. After a trap it compiles nothing.
pub struct Session {
    store: Store<Sealed>,
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    free: TypedFunc<(u32, u32), ()>,
    compile: TypedFunc<(u32, u32, u32, u32), u64>,
    broken: bool,
}

impl Session {
    /// Compile `src` as the component `name`.
    pub fn compile(&mut self, name: &str, src: &str) -> Result<Compiled, CompileFailed> {
        let failed = |message: String| CompileFailed { name: name.to_string(), line: 0, message };
        if self.broken {
            return Err(failed("the compiler's instance trapped earlier in this push".into()));
        }
        let r = self.run(name.as_bytes(), src.as_bytes());
        let out = match r {
            Ok(out) => out,
            Err(e) => {
                self.broken = true;
                return Err(failed(trap_message(&e)));
            }
        };
        match serde_json::from_slice::<Answer>(&out) {
            Ok(Answer::Ok { tree, handlers }) => Ok(Compiled { tree, handlers }),
            Ok(Answer::Err { line, message }) => Err(CompileFailed { name: name.to_string(), line, message }),
            Err(e) => Err(failed(format!("the compiler's answer is not {{ ok }} | {{ err }}: {e}"))),
        }
    }

    fn run(&mut self, name: &[u8], src: &[u8]) -> wasmtime::Result<Vec<u8>> {
        self.store.set_fuel(FUEL)?;
        let (name_ptr, name_len) = self.put(name)?;
        let (src_ptr, src_len) = self.put(src)?;
        let packed = self.compile.call(&mut self.store, (name_ptr, name_len, src_ptr, src_len))?;
        let (out_ptr, out_len) = ((packed >> 32) as u32, packed as u32);
        let mut out = vec![0u8; out_len as usize];
        self.memory.read(&self.store, out_ptr as usize, &mut out).map_err(|e| wasmtime::Error::msg(format!("the answer is outside memory: {e}")))?;
        self.free.call(&mut self.store, (name_ptr, name_len))?;
        self.free.call(&mut self.store, (src_ptr, src_len))?;
        self.free.call(&mut self.store, (out_ptr, out_len))?;
        Ok(out)
    }

    fn put(&mut self, bytes: &[u8]) -> wasmtime::Result<(u32, u32)> {
        let len = u32::try_from(bytes.len()).map_err(|_| wasmtime::Error::msg("the input is past 4 GiB"))?;
        let ptr = self.alloc.call(&mut self.store, len)?;
        self.memory.write(&mut self.store, ptr as usize, bytes).map_err(|e| wasmtime::Error::msg(format!("alloc answered room outside memory: {e}")))?;
        Ok((ptr, len))
    }
}

#[cfg(test)]
mod tests;
