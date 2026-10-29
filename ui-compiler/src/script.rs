//! The `<script>`: Rhai, parsed (never run) by an `Engine::new_raw()` built
//! with `no_time` and `no_module`. Its top level holds three declarations
//! the compiler reads — `prop(name)`, `state(map)`, `let x = live(spec)` —
//! and `fn`s, each a handler that must be a fold `fn name(state, ev)`.
//! `uses` is every `call("…")` in the AST (a comment calls nothing) and the
//! `live` methods. The handlers file is each `fn`'s source slice.

use std::collections::BTreeSet;

use rhai::{ASTNode, Dynamic, Engine, Expr, OptimizationLevel, Stmt};

use crate::json::{Arena, Id};
use crate::lines::Lines;
use crate::template::Script;
use crate::Refusal;

pub struct Decls {
    pub props: Vec<String>,
    pub state: Option<Id>,
    /// Each live by name, its spec as an object.
    pub live: Vec<(String, Id)>,
    pub uses: BTreeSet<String>,
    /// The handlers' names (every fn, each a fold).
    pub folds: BTreeSet<String>,
    pub handlers: String,
}

impl Decls {
    pub fn is_live(&self, name: &str) -> bool {
        self.live.iter().any(|(n, _)| n == name)
    }
}

/// The earliest refusal of several (the AST's maps iterate in no source
/// order; the answer must not depend on it).
struct First(Option<Refusal>);
impl First {
    fn note(&mut self, line: u32, message: String) {
        if self.0.as_ref().map_or(true, |r| line < r.line) {
            self.0 = Some(Refusal { line, message });
        }
    }
}

pub fn analyse(script: Option<&Script>, lines: &Lines, j: &mut Arena) -> Result<Decls, Refusal> {
    let mut d = Decls { props: Vec::new(), state: None, live: Vec::new(), uses: BTreeSet::new(), folds: BTreeSet::new(), handlers: String::new() };
    let Some(script) = script else { return Ok(d) };
    let base = lines.line(script.at);
    let line_of = |pos: rhai::Position| pos.line().map_or(base, |l| base + l as u32 - 1);
    let refuse = |line: u32, message: String| Err(Refusal { line, message });

    let mut engine = Engine::new_raw();
    engine.set_optimization_level(OptimizationLevel::None);
    let ast = match engine.compile(script.text) {
        Ok(ast) => ast,
        Err(e) => return refuse(line_of(e.position()), format!("the script: {}", e.err_type())),
    };

    // The top level: declarations only.
    for stmt in ast.statements() {
        let pos = stmt.position();
        let line = line_of(pos);
        let call = match stmt {
            Stmt::FnCall(f, _) => Some(&**f),
            Stmt::Expr(e) => match &**e {
                Expr::FnCall(f, _) => Some(&**f),
                _ => None,
            },
            Stmt::Noop(_) => continue,
            _ => None,
        };
        if let Some(f) = call {
            match (f.name.as_str(), f.args.as_slice()) {
                ("prop", [Expr::StringConstant(name, _)]) => {
                    let name = name.to_string();
                    if d.props.contains(&name) {
                        return refuse(line, format!("the prop `{name}` is declared twice"));
                    }
                    d.props.push(name);
                }
                ("prop", _) => return refuse(line, "prop(\"name\"): a prop is declared by a literal name".into()),
                ("state", [m @ Expr::Map(..)]) => {
                    if d.state.is_some() {
                        return refuse(line, "state is declared twice: one state(#{ … }) per component".into());
                    }
                    d.state = Some(constant(m, j, false).map_err(|m| Refusal { line, message: format!("state(#{{ … }}): {m}") })?);
                }
                ("state", _) => return refuse(line, "state(#{ … }): the state is one map of constants".into()),
                (other, _) => {
                    return refuse(line, format!("`{other}(…)` at the script's top level: it holds prop(…), state(…), let x = live(…) and fn only"))
                }
            }
            continue;
        }
        if let Stmt::Var(v, _, _) = stmt {
            let name = v.0.name.to_string();
            let spec = match &v.1 {
                Expr::FnCall(f, _) if f.name == "live" && f.args.len() == 1 => &f.args[0],
                _ => return refuse(line, format!("let {name}: a let at the script's top level is a live — let {name} = live(#{{ … }})")),
            };
            if d.is_live(&name) || d.props.contains(&name) {
                return refuse(line, format!("`{name}` is declared twice"));
            }
            let id = live(&name, spec, j, &mut d.uses).map_err(|message| Refusal { line, message })?;
            d.live.push((name, id));
            continue;
        }
        return refuse(line, "the script's top level holds prop(…), state(…), let x = live(…) and fn only".into());
    }

    // The handlers: every fn a fold, its slice kept in source order.
    let spans = fn_spans(script.text);
    let mut first = First(None);
    for f in ast.iter_fn_def() {
        let name = f.name.to_string();
        let span = spans.iter().find(|s| s.name == name && s.params == f.params.len()).or_else(|| spans.iter().find(|s| s.name == name));
        let line = span.map_or(base, |s| lines.line(script.at + s.start));
        if f.params.len() != 2 {
            first.note(line, format!("fn {name}({}) is not a fold: a handler is fn {name}(state, ev), returning the state", f.params.join(", ")));
        }
        d.folds.insert(name);
    }
    // Every call("…") the AST holds, by a literal method.
    ast.walk(&mut |path: &[ASTNode]| {
        let call = match path.last() {
            Some(ASTNode::Stmt(Stmt::FnCall(f, pos))) | Some(ASTNode::Expr(Expr::FnCall(f, pos))) => Some((f, *pos)),
            _ => None,
        };
        if let Some((f, pos)) = call {
            if f.name == "call" {
                match f.args.first() {
                    Some(Expr::StringConstant(m, _)) => {
                        d.uses.insert(m.to_string());
                    }
                    _ => first.note(line_of(pos), "call(…)'s method is a literal string — call(\"tickets.update\", #{ … }) — so it can be checked against uses".into()),
                }
            }
        }
        true
    });
    if let Some(r) = first.0 {
        return Err(r);
    }
    for s in &spans {
        d.handlers.push_str(&script.text[s.start..s.end]);
        d.handlers.push('\n');
    }
    Ok(d)
}

/// A live's spec → `{ snapshot, params, changes, key, on }`; its methods
/// into `uses`. The error says what is wrong, naming the live.
fn live(name: &str, spec: &Expr, j: &mut Arena, uses: &mut BTreeSet<String>) -> Result<Id, String> {
    let Expr::Map(m, _) = spec else { return Err(format!("the live `{name}` is live(#{{ snapshot, changes, key, params, on }})")) };
    let (mut snapshot, mut changes, mut key, mut params, mut on) = (None, None, None, None, None);
    for (field, e) in m.0.iter() {
        let lit = || match e {
            Expr::StringConstant(s, _) => Ok(s.to_string()),
            _ => Err(format!("the live `{name}`'s {} is a string", field.name)),
        };
        match field.name.as_str() {
            "snapshot" => snapshot = Some(lit()?),
            "changes" => changes = Some(lit()?),
            "key" => key = Some(lit()?),
            "params" => match e {
                Expr::Map(..) => params = Some(constant(e, j, true).map_err(|m| format!("the live `{name}`'s params: {m}"))?),
                _ => return Err(format!("the live `{name}`'s params is a map")),
            },
            "on" => {
                let Expr::Map(om, _) = e else { return Err(format!("the live `{name}`'s on is a map of event → \"upsert:field\" | \"remove:field\"")) };
                let mut entries = Vec::new();
                for (ev, v) in om.0.iter() {
                    let Expr::StringConstant(s, _) = v else { return Err(format!("the live `{name}`'s on.{} is a string", ev.name)) };
                    entries.push((ev.name.to_string(), j.str(s.to_string())));
                }
                on = Some(j.obj(entries));
            }
            other => return Err(format!("the live `{name}` has an unknown field `{other}`: snapshot, changes, key, params, on")),
        }
    }
    let missing = |f: &str| format!("the live `{name}` has no `{f}`: a live names its snapshot, its changes and its key");
    let snapshot = snapshot.ok_or_else(|| missing("snapshot"))?;
    let changes = changes.ok_or_else(|| missing("changes"))?;
    let key = key.ok_or_else(|| missing("key"))?;
    uses.insert(snapshot.clone());
    uses.insert(changes.clone());
    let params = match params {
        Some(p) => p,
        None => j.obj(Vec::new()),
    };
    let on = match on {
        Some(o) => o,
        None => j.obj(Vec::new()),
    };
    let (s, c, k) = (j.str(snapshot), j.str(changes), j.str(key));
    Ok(j.obj(vec![("snapshot".into(), s), ("changes".into(), c), ("key".into(), k), ("params".into(), params), ("on".into(), on)]))
}

/// A constant expression as JSON; with `names`, a name or a dotted path is
/// `{ "expr" }` (a live's params read the scope).
fn constant(e: &Expr, j: &mut Arena, names: bool) -> Result<Id, String> {
    Ok(match e {
        Expr::DynamicConstant(d, _) => dynamic(d, j)?,
        Expr::BoolConstant(b, _) => j.bool(*b),
        Expr::IntegerConstant(i, _) => j.int(*i),
        Expr::FloatConstant(f, _) => j.float(**f),
        Expr::CharConstant(c, _) => j.str(c.to_string()),
        Expr::StringConstant(s, _) => j.str(s.to_string()),
        Expr::Unit(_) => j.null(),
        Expr::Array(v, _) => {
            let items = v.iter().map(|x| constant(x, j, names)).collect::<Result<Vec<_>, _>>()?;
            j.arr(items)
        }
        Expr::Map(m, _) => {
            let mut entries = Vec::new();
            for (k, v) in m.0.iter() {
                entries.push((k.name.to_string(), constant(v, j, names)?));
            }
            j.obj(entries)
        }
        _ if names => match path(e) {
            Some(p) => j.expr(p),
            None => return Err("a value is a constant or a name".into()),
        },
        _ => return Err("a value is a constant".into()),
    })
}

fn path(e: &Expr) -> Option<String> {
    match e {
        Expr::Variable(v, _, _) => Some(v.1.to_string()),
        Expr::Dot(b, _, _) => match &b.rhs {
            Expr::Property(p, _) => Some(format!("{}.{}", path(&b.lhs)?, p.2)),
            _ => None,
        },
        _ => None,
    }
}

fn dynamic(d: &Dynamic, j: &mut Arena) -> Result<Id, String> {
    if d.is_unit() {
        return Ok(j.null());
    }
    if let Ok(b) = d.as_bool() {
        return Ok(j.bool(b));
    }
    if let Ok(i) = d.as_int() {
        return Ok(j.int(i));
    }
    if let Ok(f) = d.as_float() {
        return Ok(j.float(f));
    }
    if let Ok(c) = d.as_char() {
        return Ok(j.str(c.to_string()));
    }
    if d.is_string() {
        return Ok(j.str(d.clone().into_string().expect("a string")));
    }
    if d.is_array() {
        let a = d.as_array_ref().expect("an array");
        let items = a.iter().map(|x| dynamic(x, j)).collect::<Result<Vec<_>, _>>()?;
        return Ok(j.arr(items));
    }
    if d.is_map() {
        let m = d.as_map_ref().expect("a map");
        let mut entries = Vec::new();
        for (k, v) in m.iter() {
            entries.push((k.to_string(), dynamic(v, j)?));
        }
        return Ok(j.obj(entries));
    }
    Err(format!("a value of type {} is not a constant", d.type_name()))
}

/// A top-level `fn` in the script's text: from `fn` to its closing brace.
struct Span {
    name: String,
    params: usize,
    start: usize,
    end: usize,
}

/// The top-level `fn`s of a script Rhai has parsed, in source order —
/// strings, characters and comments skipped.
fn fn_spans(t: &str) -> Vec<Span> {
    let b = t.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = Vec::new();
    let mut depth = 0usize;
    // (name, params, start, opened)
    let mut pending: Option<(String, usize, usize, bool)> = None;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut level = 1;
                i += 2;
                while i < b.len() && level > 0 {
                    if b[i..].starts_with(b"/*") {
                        level += 1;
                        i += 2;
                    } else if b[i..].starts_with(b"*/") {
                        level -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            q @ (b'"' | b'`' | b'\'') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'{' => {
                if depth == 0 {
                    if let Some(p) = pending.as_mut() {
                        p.3 = true;
                    }
                }
                depth += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some((name, params, start, true)) = pending.take() {
                        out.push(Span { name, params, start, end: i + 1 });
                    }
                }
            }
            c if depth == 0 && pending.is_none() && ident(c) && (i == 0 || !ident(b[i - 1])) => {
                let w = b[i..].iter().position(|&c| !ident(c)).map_or(b.len(), |n| i + n);
                if &t[i..w] == "fn" {
                    let mut k = w;
                    while k < b.len() && b[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    let n_end = b[k..].iter().position(|&c| !ident(c)).map_or(b.len(), |n| k + n);
                    let name = t[k..n_end].to_string();
                    let params = match (t[n_end..].find('('), t[n_end..].find(')')) {
                        (Some(o), Some(c)) if o < c => t[n_end + o + 1..n_end + c].split(',').filter(|p| !p.trim().is_empty()).count(),
                        _ => 0,
                    };
                    pending = Some((name, params, i, false));
                }
                i = w;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}
