//! UI-5: the compiler. A component's source — `src/<name>.svelte`, a
//! `<script>` of Rhai and a template in the Svelte-like subset the contract
//! (UI-4) pins — becomes the contract's view tree (`components/<name>.json`)
//! and the component's handlers (`components/<name>.rhai`).
//!
//! A pure function: no files, no network, no clock, no randomness. The same
//! source gives the same bytes, natively and in the wasm module
//! (`--features wasm`, [`wasm`]), which imports nothing.

use serde::{Deserialize, Serialize};

#[cfg(feature = "wasm")]
pub mod wasm;

/// What a component compiles to: the view tree as JSON (keys sorted) and its
/// handlers as Rhai source. `tree`'s `handlers` field is the sha256 of
/// `handlers`, 64 lowercase hex digits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compiled {
    pub tree: String,
    pub handlers: String,
}

/// A source the compiler refuses: the line it is on (1-based, in the whole
/// `.svelte` file) and what was found there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub line: u32,
    pub message: String,
}

/// The answer as the wasm module and the binary carry it:
/// `{ "ok": Compiled } | { "err": Refusal }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    #[serde(rename = "ok")]
    Ok(Compiled),
    #[serde(rename = "err")]
    Err(Refusal),
}

impl From<Result<Compiled, Refusal>> for Answer {
    fn from(r: Result<Compiled, Refusal>) -> Self {
        match r {
            Ok(c) => Answer::Ok(c),
            Err(e) => Answer::Err(e),
        }
    }
}

/// Compile one component's source; `name` is its file's stem and goes into
/// the tree.
pub fn compile(name: &str, src: &str) -> Result<Compiled, Refusal> {
    let _ = (name, src);
    todo!("UI-5: the compiler")
}

/// [`compile`], answered as the JSON the wasm module returns.
pub fn compile_json(name: &str, src: &str) -> String {
    let answer: Answer = compile(name, src).into();
    serde_json::to_string(&answer).expect("an answer always serialises")
}

/// A refusal as the wasm module returns it, `{ "err": … }`.
pub fn refusal_json(r: Refusal) -> String {
    serde_json::to_string(&Answer::Err(r)).expect("an answer always serialises")
}

#[cfg(test)]
mod tests;
