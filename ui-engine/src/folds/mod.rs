//! UI-6: **the folds** — a component's handlers, `components/<name>.rhai`,
//! run in Rhai: every `fn name(state, ev)` returns the new state.
//!
//! One `rhai::Engine` per component with only `call(method, params)` and
//! `navigate(route)` registered — no DOM, no socket, no other component.
//! The props are constants in the scope by name. `call` is held to the
//! component's `uses`: a handler naming a method outside it is refused when
//! the component is compiled (at mount), naming the method; the engine
//! checks it again at the call.

use serde_json::{Map, Value};
use std::rc::Rc;

/// What a handler reaches: the engine's `call` and `navigate`.
pub trait Host {
    /// A Plexus call through the engine's one socket (its answer is not the
    /// handler's: a `live` collection hears the change).
    fn call(&self, method: &str, params: Value);
    /// Moves the page to `route` (`#/ticket/T-1`).
    fn navigate(&self, route: &str);
}

/// Why handlers did not compile or a fold did not run.
#[derive(Debug, Clone, PartialEq)]
pub enum FoldError {
    /// The Rhai did not compile: why.
    Compile(String),
    /// A handler calls `method`, which the component's `uses` does not name.
    NotInUses { handler: Option<String>, method: String },
    /// A `fn` that is not a fold `(state, ev)`.
    NotAFold { name: String },
    /// No handler of that name.
    NoHandler(String),
    /// The fold failed, or returned no map: why.
    Run { handler: String, message: String },
}

impl std::fmt::Display for FoldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FoldError::Compile(m) => write!(f, "the handlers do not compile: {m}"),
            FoldError::NotInUses { handler: Some(h), method } => write!(f, "{h} calls {method}, which this component's uses does not name"),
            FoldError::NotInUses { handler: None, method } => write!(f, "a handler calls {method}, which this component's uses does not name"),
            FoldError::NotAFold { name } => write!(f, "fn {name} is not a fold (state, ev) → state"),
            FoldError::NoHandler(h) => write!(f, "no handler {h}"),
            FoldError::Run { handler, message } => write!(f, "{handler} failed: {message}"),
        }
    }
}

/// A component's compiled handlers.
pub struct Folds {
    host: Rc<dyn Host>,
}

impl Folds {
    /// Compiles `source` for a component whose `uses` is `uses`; refused
    /// when it does not compile, when a `fn` is not a fold, or when a
    /// handler calls a method outside `uses` (naming it).
    pub fn compile(source: &str, uses: &[String], host: Rc<dyn Host>) -> Result<Self, FoldError> {
        let _ = (source, uses);
        let _ = Self { host };
        todo!("UI-6: Folds::compile")
    }

    /// The handlers by name.
    pub fn handlers(&self) -> Vec<String> {
        let _ = &self.host;
        todo!("UI-6: Folds::handlers")
    }

    /// Runs `handler` on `state` and `event`, the props in scope: the new
    /// state.
    pub fn run(&self, handler: &str, state: &Map<String, Value>, event: &Value, props: &Map<String, Value>) -> Result<Map<String, Value>, FoldError> {
        let _ = (handler, state, event, props);
        todo!("UI-6: Folds::run")
    }
}

#[cfg(test)]
mod tests;
