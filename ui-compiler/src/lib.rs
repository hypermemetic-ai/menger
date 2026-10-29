//! UI-5: the compiler. A component's source — `src/<name>.svelte`, a
//! `<script>` of Rhai and a template in the Svelte-like subset the contract
//! (UI-4) pins — becomes the contract's view tree (`components/<name>.json`)
//! and the component's handlers (`components/<name>.rhai`).
//!
//! A pure function: no files, no network, no clock, no randomness. The same
//! source gives the same bytes, natively and in the wasm module
//! (`--features wasm`, [`wasm`]), which imports nothing.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod json;
mod lines;
mod script;
mod template;

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
    if name.is_empty() {
        return Err(Refusal { line: 0, message: "a component has a name: its file's stem".into() });
    }
    let lines = lines::Lines::new(src);
    let mut j = json::Arena::default();
    let t = template::parse(src, &lines, &mut j)?;
    let d = script::analyse(t.script.as_ref(), &lines, &mut j)?;
    for (handler, line) in &t.handlers {
        if !d.folds.contains(handler) {
            return Err(Refusal {
                line: *line,
                message: format!("on: names `{handler}`, and the script has no fn {handler}(state, ev)"),
            });
        }
    }
    // A bare live in {#each} reads its rows.
    for (id, expr) in &t.each_exprs {
        if d.is_live(expr) {
            j.set_str(*id, format!("{expr}.rows"));
        }
    }
    let hash: String = Sha256::digest(d.handlers.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    let props = d.props.iter().map(|p| j.str(p.clone())).collect();
    let props = j.arr(props);
    let state = match d.state {
        Some(s) => s,
        None => j.obj(Vec::new()),
    };
    let live = j.obj(d.live.clone());
    let uses = d.uses.iter().map(|u| j.str(u.clone())).collect();
    let uses = j.arr(uses);
    let (abi, name, handlers) = (j.str("engine/1"), j.str(name), j.str(hash));
    let root = j.obj(vec![
        ("abi".into(), abi),
        ("name".into(), name),
        ("props".into(), props),
        ("state".into(), state),
        ("live".into(), live),
        ("uses".into(), uses),
        ("handlers".into(), handlers),
        ("tree".into(), t.tree),
    ]);
    Ok(Compiled { tree: j.pretty(root), handlers: d.handlers })
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
