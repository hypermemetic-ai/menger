//! UI-6: the template's expressions — Rhai, over the scope: the props by
//! name, `state`, each `live` collection by name (`{ rows, status }`), and
//! each `{#each}`'s variable. An expression is compiled once; what it reads
//! is found by scanning its text ([`Reads`]), so a binding reads (and so
//! tracks) only the state keys and names it names.

use rhai::{Dynamic, Engine, Scope, AST};
use serde_json::Value;
use std::collections::BTreeSet;
use std::rc::Rc;

/// The most operations one expression may take.
const FUEL: u64 = 100_000;

thread_local! {
    static ENGINE: Rc<Engine> = Rc::new({
        let mut e = Engine::new();
        e.set_max_operations(FUEL);
        e.set_optimization_level(rhai::OptimizationLevel::Simple);
        e
    });
}

/// The expressions' engine (one per thread: the page has one).
pub fn engine() -> Rc<Engine> {
    ENGINE.with(Rc::clone)
}

/// What an expression's text reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reads {
    /// Every name it reads as a variable.
    pub names: BTreeSet<String>,
    /// The state keys it reads as `state.<key>`; `None` when it reads
    /// `state` some other way (all of it).
    pub state_keys: Option<BTreeSet<String>>,
}

/// A compiled expression.
#[derive(Clone)]
pub struct Expr {
    pub text: String,
    pub ast: Rc<AST>,
    pub reads: Reads,
}

impl std::fmt::Debug for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Expr({})", self.text)
    }
}

impl Expr {
    /// Compiles `text`; refused, naming why.
    pub fn compile(text: &str) -> Result<Self, String> {
        let ast = engine().compile_expression(text).map_err(|e| e.to_string())?;
        Ok(Self { text: text.to_string(), ast: Rc::new(ast), reads: scan(text) })
    }

    /// Evaluates it in `scope`.
    pub fn eval(&self, scope: &mut Scope<'static>) -> Result<Dynamic, String> {
        engine().eval_ast_with_scope::<Dynamic>(scope, &self.ast).map_err(|e| e.to_string())
    }
}

/// The names and `state.<key>`s in `text`, string literals skipped.
pub fn scan(text: &str) -> Reads {
    let b = text.as_bytes();
    let mut reads = Reads { names: BTreeSet::new(), state_keys: Some(BTreeSet::new()) };
    let mut i = 0;
    let mut after_dot = false;
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'`' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            after_dot = false;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let word = &text[start..i];
            if !after_dot {
                reads.names.insert(word.to_string());
                if word == "state" {
                    let mut j = i;
                    while j < b.len() && b[j] == b' ' {
                        j += 1;
                    }
                    let key = (j < b.len() && b[j] == b'.').then(|| {
                        let mut k = j + 1;
                        while k < b.len() && b[k] == b' ' {
                            k += 1;
                        }
                        let s = k;
                        while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'_') {
                            k += 1;
                        }
                        (k > s).then(|| text[s..k].to_string())
                    });
                    match (key.flatten(), reads.state_keys.as_mut()) {
                        (Some(k), Some(keys)) => {
                            keys.insert(k);
                        }
                        (None, _) => reads.state_keys = None,
                        _ => {}
                    }
                }
            }
            after_dot = false;
            continue;
        }
        if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                i += 1;
            }
            after_dot = false;
            continue;
        }
        after_dot = c == b'.' || (after_dot && c == b' ');
        i += 1;
    }
    if !reads.names.contains("state") {
        reads.state_keys = Some(BTreeSet::new());
    }
    reads
}

/// A JSON value as Rhai's.
pub fn to_dynamic(v: &Value) -> Dynamic {
    rhai::serde::to_dynamic(v).unwrap_or(Dynamic::UNIT)
}

/// Rhai's value as JSON.
pub fn to_value(d: &Dynamic) -> Value {
    rhai::serde::from_dynamic::<Value>(d).unwrap_or(Value::Null)
}

/// A value drawn as text: a string as it is, unit as nothing, anything
/// else as Rhai prints it.
pub fn as_text(d: &Dynamic) -> String {
    if d.is_unit() {
        String::new()
    } else if let Some(s) = d.clone().try_cast::<rhai::ImmutableString>() {
        s.to_string()
    } else {
        d.to_string()
    }
}

/// A value as a condition: a bool as it is; unit false; anything else true.
pub fn truthy(d: &Dynamic) -> bool {
    match d.as_bool() {
        Ok(b) => b,
        Err(_) => !d.is_unit(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_finds_names_and_state_keys() {
        let r = scan(r#""one " + id + ": " + state.n"#);
        assert!(r.names.contains("id") && r.names.contains("state"));
        assert_eq!(r.state_keys, Some(["n".to_string()].into()));
        assert_eq!(scan("rows.rows").names, ["rows".to_string()].into());
        assert_eq!(scan("state").state_keys, None);
        assert_eq!(scan(r#""state.x""#).names.len(), 0);
        assert_eq!(scan("r.id + \" \" + r.title").names, ["r".to_string()].into());
    }
}
