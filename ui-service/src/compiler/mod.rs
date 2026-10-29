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
use std::sync::Arc;

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

/// The compiler module, loaded once at start; each [`Compiler::compile`] a
/// fresh instance in a store of its own.
#[derive(Clone)]
pub struct Compiler {
    // UI-7 phase 1: the bytes only; the engine and the compiled module come
    // with the build.
    bytes: Arc<Vec<u8>>,
}

impl std::fmt::Debug for Compiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compiler").field("module_bytes", &self.bytes.len()).finish()
    }
}

impl Compiler {
    /// The module `bytes`, checked: a module that imports anything is
    /// refused here, naming each import (`module::name`).
    pub fn new(bytes: &[u8]) -> Result<Self, ModuleRefused> {
        // UI-7 phase 1: unchecked.
        Ok(Self { bytes: Arc::new(bytes.to_vec()) })
    }

    /// The module at `path` (`--ui-compiler`), read and [`Self::new`].
    pub fn open(path: &Path) -> Result<Self, ModuleRefused> {
        let bytes = std::fs::read(path).map_err(|e| ModuleRefused::Invalid(format!("{}: {e}", path.display())))?;
        Self::new(&bytes)
    }

    /// Compile `src` as the component `name` in a fresh instance, nothing
    /// linked, under [`FUEL`] and [`MEMORY_MAX`].
    pub fn compile(&self, name: &str, src: &str) -> Result<Compiled, CompileFailed> {
        let _ = (name, src, &self.bytes);
        todo!("UI-7: the compile in wasmtime")
    }
}

#[cfg(test)]
mod tests;
