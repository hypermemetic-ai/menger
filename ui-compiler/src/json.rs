//! The tree as JSON: an arena of values (flat, so a deep tree is neither
//! built nor dropped by recursion) written in serde_json's pretty form by an
//! explicit stack — two-space indent, `": "`, `[]` / `{}` when empty — with
//! every object's keys sorted here, never by serde_json's map (a workspace
//! build may unify `preserve_order` on). Leaves are written by serde_json
//! itself, so strings and numbers escape and format exactly as it does.

pub type Id = usize;

enum Val {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Id>),
    /// Sorted by key; keys unique.
    Obj(Vec<(String, Id)>),
}

#[derive(Default)]
pub struct Arena {
    vals: Vec<Val>,
}

impl Arena {
    fn push(&mut self, v: Val) -> Id {
        self.vals.push(v);
        self.vals.len() - 1
    }
    pub fn null(&mut self) -> Id {
        self.push(Val::Null)
    }
    pub fn bool(&mut self, b: bool) -> Id {
        self.push(Val::Bool(b))
    }
    pub fn int(&mut self, i: i64) -> Id {
        self.push(Val::Int(i))
    }
    pub fn float(&mut self, f: f64) -> Id {
        self.push(Val::Float(f))
    }
    pub fn str(&mut self, s: impl Into<String>) -> Id {
        self.push(Val::Str(s.into()))
    }
    /// `{ "expr": s }`.
    pub fn expr(&mut self, s: impl Into<String>) -> Id {
        let e = self.str(s);
        self.obj(vec![("expr".into(), e)])
    }
    pub fn arr(&mut self, items: Vec<Id>) -> Id {
        self.push(Val::Arr(items))
    }
    /// An object; its keys are sorted here (the caller keeps them unique).
    pub fn obj(&mut self, mut entries: Vec<(String, Id)>) -> Id {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        self.push(Val::Obj(entries))
    }
    /// Replace a string leaf (the `{#each}` over a bare live, once the
    /// script's lives are known).
    pub fn set_str(&mut self, id: Id, s: String) {
        self.vals[id] = Val::Str(s);
    }

    /// `root` in serde_json's pretty form, with a trailing newline.
    pub fn pretty(&self, root: Id) -> String {
        let mut out = String::new();
        // (container, next child) — the stack's depth is the indent level.
        let mut stack: Vec<(Id, usize)> = Vec::new();
        self.open(root, &mut out, &mut stack);
        while let Some(top) = stack.last_mut() {
            let (id, i) = *top;
            top.1 += 1;
            let len = match &self.vals[id] {
                Val::Arr(v) => v.len(),
                Val::Obj(v) => v.len(),
                _ => unreachable!("only containers are stacked"),
            };
            let level = stack.len();
            if i == len {
                out.push('\n');
                indent(&mut out, level - 1);
                out.push(if matches!(self.vals[id], Val::Arr(_)) { ']' } else { '}' });
                stack.pop();
                continue;
            }
            out.push_str(if i == 0 { "\n" } else { ",\n" });
            indent(&mut out, level);
            let child = match &self.vals[id] {
                Val::Arr(v) => v[i],
                Val::Obj(v) => {
                    out.push_str(&serde_json::to_string(&v[i].0).expect("a key"));
                    out.push_str(": ");
                    v[i].1
                }
                _ => unreachable!(),
            };
            self.open(child, &mut out, &mut stack);
        }
        out.push('\n');
        out
    }

    /// Write a leaf whole, or a container's opening (stacked when not empty).
    fn open(&self, id: Id, out: &mut String, stack: &mut Vec<(Id, usize)>) {
        match &self.vals[id] {
            Val::Null => out.push_str("null"),
            Val::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Val::Int(i) => out.push_str(&i.to_string()),
            Val::Float(f) => out.push_str(&serde_json::to_string(f).expect("a finite float")),
            Val::Str(s) => out.push_str(&serde_json::to_string(s).expect("a string")),
            Val::Arr(v) if v.is_empty() => out.push_str("[]"),
            Val::Obj(v) if v.is_empty() => out.push_str("{}"),
            Val::Arr(_) => {
                out.push('[');
                stack.push((id, 0));
            }
            Val::Obj(_) => {
                out.push('{');
                stack.push((id, 0));
            }
        }
    }
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}
