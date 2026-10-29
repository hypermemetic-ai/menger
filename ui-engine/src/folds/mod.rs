//! UI-6: **the folds** — a component's handlers, `components/<name>.rhai`,
//! run in Rhai: every `fn name(state, ev)` returns the new state.
//!
//! One `rhai::Engine` per component with only `call(method, params)` and
//! `navigate(route)` registered — no DOM, no socket, no other component.
//! The props are constants in the scope by name. `call` is held to the
//! component's `uses`: a handler naming a method outside it is refused when
//! the component is compiled (at mount), naming the method; the engine
//! checks it again at the call.

use rhai::{Dynamic, Engine, Scope, AST};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::rc::Rc;

/// The most operations one fold may take.
const FUEL: u64 = 1_000_000;

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
    /// A handler's `call` names its method by an expression that is not a
    /// string (or strings joined by `+`): the handler, and the expression.
    Computed { handler: String, expr: String },
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
            FoldError::Computed { handler, expr } => write!(f, "{handler} calls a method named by `{expr}`: a call's method is a string as written"),
            FoldError::NotAFold { name } => write!(f, "fn {name} is not a fold (state, ev) → state"),
            FoldError::NoHandler(h) => write!(f, "no handler {h}"),
            FoldError::Run { handler, message } => write!(f, "{handler} failed: {message}"),
        }
    }
}

/// A component's compiled handlers.
pub struct Folds {
    engine: Engine,
    ast: AST,
    handlers: Vec<String>,
}

/// What the engine registers `call` as: Rhai keeps `call(…)` for calling a
/// function pointer, so a handler's `call(` is renamed before compiling.
const HOST_CALL: &str = "host_call";

/// `source` with each `call(` that is a call of the host's (not a method,
/// not inside a string or a comment) renamed [`HOST_CALL`].
fn rename_calls(source: &str) -> String {
    let b = source.as_bytes();
    let mut out = String::with_capacity(source.len() + 32);
    let mut i = 0;
    let mut last = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == b'"' || c == b'`' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let prev = source[..start].trim_end().as_bytes().last().copied();
            let next = source[i..].trim_start().as_bytes().first().copied();
            if &source[start..i] == "call" && prev != Some(b'.') && next == Some(b'(') {
                out.push_str(&source[last..start]);
                out.push_str(HOST_CALL);
                last = i;
            }
            continue;
        }
        i += 1;
    }
    out.push_str(&source[last..]);
    out
}

/// The method a `call`'s first argument names, when written as a string
/// (or strings joined by `+`).
fn literal(e: &rhai::Expr) -> Option<String> {
    match e {
        rhai::Expr::StringConstant(s, _) => Some(s.to_string()),
        rhai::Expr::FnCall(f, _) if f.name.as_str() == "+" && f.args.len() == 2 => Some(literal(&f.args[0])? + literal(&f.args[1])?.as_str()),
        _ => None,
    }
}

impl Folds {
    /// Compiles `source` for a component whose `uses` is `uses`; refused
    /// when it does not compile, when a `fn` is not a fold, or when a
    /// handler calls a method outside `uses` — or by a name computed at run
    /// time — naming it.
    pub fn compile(source: &str, uses: &[String], host: Rc<dyn Host>) -> Result<Self, FoldError> {
        let uses: Rc<BTreeSet<String>> = Rc::new(uses.iter().cloned().collect());
        let mut engine = Engine::new();
        engine.set_max_operations(FUEL);
        engine.set_optimization_level(rhai::OptimizationLevel::Simple);
        let (h, u) = (host.clone(), uses.clone());
        engine.register_fn(HOST_CALL, move |method: &str, params: rhai::Map| -> Result<(), Box<rhai::EvalAltResult>> {
            if !u.contains(method) {
                return Err(format!("{method} is not in this component's uses").into());
            }
            h.call(method, rhai::serde::from_dynamic(&Dynamic::from_map(params)).unwrap_or(Value::Null));
            Ok(())
        });
        let h = host.clone();
        engine.register_fn("navigate", move |route: &str| h.navigate(route));
        let ast = engine.compile(rename_calls(source)).map_err(|e| FoldError::Compile(e.to_string()))?;
        let mut handlers = Vec::new();
        for f in ast.iter_functions() {
            if f.params.len() != 2 {
                return Err(FoldError::NotAFold { name: f.name.to_string() });
            }
            handlers.push(f.name.to_string());
        }
        // Each call attributed to the handler whose body holds it (by
        // position: rhai 1.27 names a body's type only inside itself).
        let at = |p: rhai::Position| (p.line().unwrap_or(0), p.position().unwrap_or(0));
        #[allow(deprecated)]
        let spans: Vec<(String, (usize, usize), (usize, usize))> =
            ast.lib().iter_script_fn_info().map(|(_, _, name, _, def)| (name.to_string(), at(def.body.start_position()), at(def.body.end_position()))).collect();
        let owner = |p: rhai::Position| spans.iter().find(|(_, s, e)| (*s..=*e).contains(&at(p))).map(|(n, _, _)| n.clone()).unwrap_or_else(|| "a handler".into());
        let mut refusal = None;
        ast.walk(&mut |path: &[rhai::ASTNode]| {
            let call = match path.last() {
                Some(rhai::ASTNode::Expr(rhai::Expr::FnCall(f, pos))) | Some(rhai::ASTNode::Stmt(rhai::Stmt::FnCall(f, pos))) => Some((f, *pos)),
                _ => None,
            };
            if let Some((f, pos)) = call.filter(|(f, _)| f.name.as_str() == HOST_CALL) {
                let name = owner(pos);
                refusal = match f.args.first().map(|a| (literal(a), a)) {
                    Some((Some(m), _)) if uses.contains(&m) => None,
                    Some((Some(method), _)) => Some(FoldError::NotInUses { handler: Some(name), method }),
                    Some((None, a)) => Some(FoldError::Computed { handler: name, expr: format!("{a:?}") }),
                    None => Some(FoldError::Computed { handler: name, expr: "nothing".into() }),
                };
                return refusal.is_none();
            }
            true
        });
        if let Some(r) = refusal {
            return Err(r);
        }
        Ok(Self { engine, ast, handlers })
    }

    /// The handlers by name.
    pub fn handlers(&self) -> Vec<String> {
        self.handlers.clone()
    }

    /// Runs `handler` on `state` and the event: `ev` is the DOM event's
    /// payload (`{ value, target }`) with the props as `ev.props` and each
    /// `live` collection's rows as `ev.live.<name>`. The new state.
    pub fn run(
        &self,
        handler: &str,
        state: &Map<String, Value>,
        event: &Value,
        props: &Map<String, Value>,
        live: &Map<String, Value>,
    ) -> Result<Map<String, Value>, FoldError> {
        if !self.handlers.iter().any(|h| h == handler) {
            return Err(FoldError::NoHandler(handler.to_string()));
        }
        let ev = json!({
            "value": event.get("value").cloned().unwrap_or(Value::Null),
            "target": event.get("target").cloned().unwrap_or(Value::Null),
            "props": props,
            "live": live,
        });
        let to = |v: &Value| rhai::serde::to_dynamic(v).unwrap_or(Dynamic::UNIT);
        let run = |message: String| FoldError::Run { handler: handler.to_string(), message };
        let out: Dynamic = self
            .engine
            .call_fn(&mut Scope::new(), &self.ast, handler, (to(&Value::Object(state.clone())), to(&ev)))
            .map_err(|e| run(e.to_string()))?;
        match rhai::serde::from_dynamic::<Value>(&out) {
            Ok(Value::Object(m)) => Ok(m),
            _ => Err(run(format!("returned {} where a fold returns the state, a map", out.type_name()))),
        }
    }
}

#[cfg(test)]
mod tests;
