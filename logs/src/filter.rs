//! **The filter's language** (LOGS-3, `logs.read --filter`; LOGS-1 wrote
//! `--where`, a word Rust keeps — plexus-macros 0.8 names a parameter
//! `r#where` on the wire as written): small, checked
//! against the log's columns, compiled to SQL with every value bound — the
//! caller's text never reaches SQL.
//!
//! ```text
//! expr    := or
//! or      := and ( "or" and )*
//! and     := unary ( "and" unary )*
//! unary   := "not" unary | "(" expr ")" | compare
//! compare := FIELD OP VALUE | FIELD "in" "(" VALUE ( "," VALUE )* ")"
//! OP      := "=" | "!=" | "<" | "<=" | ">" | ">="
//! VALUE   := STRING | NUMBER | "true" | "false" | "null"
//! STRING  := "…" or '…'   (\" \' \\ escape)
//! NUMBER  := -?digits[.digits][e[+-]digits]
//! FIELD   := a field of the log, or the envelope's seq, at, by, ticket
//! ```
//!
//! Words (`and`, `or`, `not`, `in`, `true`, `false`, `null`) are read
//! without regard to case. `= null` and `!= null` are "carries no value" and
//! "carries one". The logic has two values: a comparison with a field an
//! entry does not carry is false (`!=` true), so `not` is its opposite. Each comparison is checked against its column: a string
//! field takes strings, an integer or number field numbers, a boolean field
//! `true` / `false` with `=` and `!=` alone; an array or object field (kept as
//! JSON) and `tags` (`--tag`) are not filtered on here. An unknown field is
//! refused, naming the fields there are. At most 4 KiB of text, 32 levels
//! deep, 100 values in an `in`.

use crate::schema::{Field, Kind};
use rusqlite::types::Value as Sql;

/// The deepest a filter nests.
pub const DEPTH_MAX: usize = 32;
/// The most values one `in` names.
pub const IN_MAX: usize = 100;

/// A column a filter may name: its name, and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub name: String,
    pub kind: Kind,
}

/// The envelope's columns a filter may name: `seq`, `at` (unix ms), `by`,
/// `ticket` (a uid).
pub fn envelope() -> Vec<Named> {
    [("seq", Kind::Integer), ("at", Kind::Integer), ("by", Kind::String), ("ticket", Kind::String)].into_iter().map(|(n, k)| Named { name: n.into(), kind: k }).collect()
}

/// The columns of a log whose fields are `fields`: the envelope's, then each field's.
pub fn of_log(fields: &[Field]) -> Vec<Named> {
    envelope().into_iter().chain(fields.iter().map(|f| Named { name: f.name.clone(), kind: f.kind })).collect()
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Str(String),
    Num(String),
    Op(&'static str),
    Open,
    Close,
    Comma,
}

fn lex(text: &str) -> Result<Vec<(usize, Tok)>, String> {
    let b: Vec<char> = text.chars().collect();
    let (mut i, mut out) = (0, Vec::new());
    while i < b.len() {
        let c = b[i];
        let at = i;
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => (out.push((at, Tok::Open)), i += 1).1,
            ')' => (out.push((at, Tok::Close)), i += 1).1,
            ',' => (out.push((at, Tok::Comma)), i += 1).1,
            '=' => (out.push((at, Tok::Op("="))), i += 1).1,
            '!' if b.get(i + 1) == Some(&'=') => (out.push((at, Tok::Op("!="))), i += 2).1,
            '<' | '>' => {
                let eq = b.get(i + 1) == Some(&'=');
                out.push((at, Tok::Op(match (c, eq) {
                    ('<', true) => "<=",
                    ('<', false) => "<",
                    (_, true) => ">=",
                    _ => ">",
                })));
                i += 1 + eq as usize;
            }
            '"' | '\'' => {
                let (q, mut s) = (c, String::new());
                i += 1;
                loop {
                    match b.get(i) {
                        None => return Err(format!("at {at}: a string that never ends")),
                        Some('\\') => match b.get(i + 1) {
                            Some(e @ ('"' | '\'' | '\\')) => (s.push(*e), i += 2).1,
                            _ => return Err(format!("at {i}: only \\\" \\' and \\\\ escape")),
                        },
                        Some(&e) if e == q => break i += 1,
                        Some(&e) => (s.push(e), i += 1).1,
                    }
                }
                out.push((at, Tok::Str(s)));
            }
            c if c.is_ascii_digit() || (c == '-' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                let start = i;
                i += 1;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == '.' || b[i] == 'e' || b[i] == 'E' || ((b[i] == '-' || b[i] == '+') && matches!(b[i - 1], 'e' | 'E'))) {
                    i += 1;
                }
                out.push((at, Tok::Num(b[start..i].iter().collect())));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_') {
                    i += 1;
                }
                out.push((at, Tok::Word(b[start..i].iter().collect())));
            }
            c => return Err(format!("at {at}: `{c}` is no part of a filter")),
        }
    }
    Ok(out)
}

/// A value a filter compares with.
#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    Str(String),
    Num(f64, Option<i64>),
    Bool(bool),
    Null,
}

/// A filter, parsed.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Cmp { field: String, op: &'static str, value: Lit },
    In { field: String, values: Vec<Lit> },
}

struct Parser {
    toks: Vec<(usize, Tok)>,
    at: usize,
}

fn word_is(t: Option<&Tok>, w: &str) -> bool {
    matches!(t, Some(Tok::Word(x)) if x.eq_ignore_ascii_case(w))
}

const WORDS: &[&str] = &["and", "or", "not", "in", "true", "false", "null"];

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at).map(|(_, t)| t)
    }
    fn place(&self) -> String {
        self.toks.get(self.at).map_or_else(|| "at the end".into(), |(i, _)| format!("at {i}"))
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).map(|(_, t)| t.clone());
        self.at += 1;
        t
    }
    fn or(&mut self, depth: usize) -> Result<Expr, String> {
        let mut e = self.and(depth)?;
        while word_is(self.peek(), "or") {
            self.at += 1;
            e = Expr::Or(Box::new(e), Box::new(self.and(depth)?));
        }
        Ok(e)
    }
    fn and(&mut self, depth: usize) -> Result<Expr, String> {
        let mut e = self.unary(depth)?;
        while word_is(self.peek(), "and") {
            self.at += 1;
            e = Expr::And(Box::new(e), Box::new(self.unary(depth)?));
        }
        Ok(e)
    }
    fn unary(&mut self, depth: usize) -> Result<Expr, String> {
        if depth > DEPTH_MAX {
            return Err(format!("{}: a filter nests at most {DEPTH_MAX} deep", self.place()));
        }
        if word_is(self.peek(), "not") {
            self.at += 1;
            return Ok(Expr::Not(Box::new(self.unary(depth + 1)?)));
        }
        if self.peek() == Some(&Tok::Open) {
            self.at += 1;
            let e = self.or(depth + 1)?;
            return match self.next() {
                Some(Tok::Close) => Ok(e),
                _ => Err(format!("{}: a `(` is closed by `)`", self.place())),
            };
        }
        self.compare()
    }
    fn value(&mut self) -> Result<Lit, String> {
        let place = self.place();
        match self.next() {
            Some(Tok::Str(s)) => Ok(Lit::Str(s)),
            Some(Tok::Num(n)) => n.parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| Lit::Num(f, n.parse::<i64>().ok())).ok_or(format!("{place}: `{n}` is not a number")),
            Some(Tok::Word(w)) if w.eq_ignore_ascii_case("true") => Ok(Lit::Bool(true)),
            Some(Tok::Word(w)) if w.eq_ignore_ascii_case("false") => Ok(Lit::Bool(false)),
            Some(Tok::Word(w)) if w.eq_ignore_ascii_case("null") => Ok(Lit::Null),
            Some(Tok::Word(w)) => Err(format!("{place}: `{w}` is not a value — a string is quoted, \"{w}\"")),
            _ => Err(format!("{place}: a value — a quoted string, a number, true, false or null")),
        }
    }
    fn compare(&mut self) -> Result<Expr, String> {
        let place = self.place();
        let field = match self.next() {
            Some(Tok::Word(w)) if !WORDS.iter().any(|k| k.eq_ignore_ascii_case(&w)) => w,
            _ => return Err(format!("{place}: a field's name, or `not`, or `(`")),
        };
        if word_is(self.peek(), "in") {
            self.at += 1;
            if self.next() != Some(Tok::Open) {
                return Err(format!("{}: `in` is followed by ( values )", self.place()));
            }
            let mut values = vec![self.value()?];
            loop {
                match self.next() {
                    Some(Tok::Comma) => values.push(self.value()?),
                    Some(Tok::Close) => break,
                    _ => return Err(format!("{}: values in `in ( … )` are separated by `,`", self.place())),
                }
            }
            if values.len() > IN_MAX {
                return Err(format!("{place}: `in` names at most {IN_MAX} values"));
            }
            return Ok(Expr::In { field, values });
        }
        let op = match self.next() {
            Some(Tok::Op(op)) => op,
            _ => return Err(format!("{}: `{field}` is followed by = != < <= > >= or in", self.place())),
        };
        Ok(Expr::Cmp { field, op, value: self.value()? })
    }
}

/// `text` read as a filter, or where and why not.
pub fn parse(text: &str) -> Result<Expr, String> {
    let mut p = Parser { toks: lex(text)?, at: 0 };
    if p.toks.is_empty() {
        return Err("an empty filter".into());
    }
    let e = p.or(0)?;
    match p.at < p.toks.len() {
        true => Err(format!("{}: nothing more was expected (and / or?)", p.place())),
        false => Ok(e),
    }
}

/// A filter compiled: SQL with `?N` marks numbered from `first`, and the
/// values bound to them, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    pub sql: String,
    pub values: Vec<Sql>,
}

/// `e` checked against `columns` (`where` names the place: "the log
/// `perf`'s" or "the envelope's") and compiled, marks numbered from `first`.
pub fn compile(e: &Expr, columns: &[Named], whose: &str, first: usize) -> Result<Compiled, String> {
    let mut c = Compiled { sql: String::new(), values: Vec::new() };
    walk(e, columns, whose, first, &mut c)?;
    Ok(c)
}

fn column<'a>(field: &str, columns: &'a [Named], whose: &str) -> Result<&'a Named, String> {
    if field == "tags" {
        return Err("`tags` is filtered by --tag, not in --filter".into());
    }
    columns.iter().find(|c| c.name == field).ok_or_else(|| format!("`{field}` is no field of {whose} (its fields: {})", columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")))
}

fn bound(col: &Named, v: &Lit) -> Result<Sql, String> {
    let n = &col.name;
    match (col.kind, v) {
        (Kind::Array | Kind::Object, _) => Err(format!("`{n}` is kept as JSON ({}): it is not filtered on", col.kind.as_str())),
        (Kind::String, Lit::Str(s)) => Ok(Sql::Text(s.clone())),
        (Kind::Integer, Lit::Num(_, Some(i))) => Ok(Sql::Integer(*i)),
        (Kind::Integer | Kind::Number, Lit::Num(f, _)) => Ok(Sql::Real(*f)),
        (Kind::Boolean, Lit::Bool(b)) => Ok(Sql::Integer(*b as i64)),
        (k, v) => Err(format!("`{n}` is {}, compared with {}", k.as_str(), match v {
            Lit::Str(_) => "a string",
            Lit::Num(..) => "a number",
            Lit::Bool(_) => "a boolean",
            Lit::Null => "null",
        })),
    }
}

fn walk(e: &Expr, columns: &[Named], whose: &str, first: usize, c: &mut Compiled) -> Result<(), String> {
    let mark = |c: &Compiled| format!("?{}", first + c.values.len());
    match e {
        Expr::And(a, b) | Expr::Or(a, b) => {
            c.sql.push('(');
            walk(a, columns, whose, first, c)?;
            c.sql.push_str(if matches!(e, Expr::And(..)) { " AND " } else { " OR " });
            walk(b, columns, whose, first, c)?;
            c.sql.push(')');
        }
        Expr::Not(a) => {
            c.sql.push_str("(NOT ");
            walk(a, columns, whose, first, c)?;
            c.sql.push(')');
        }
        Expr::Cmp { field, op, value } => {
            let col = column(field, columns, whose)?;
            let q = crate::store::quoted(&col.name).map_err(|e| e.to_string())?;
            match (value, *op) {
                (Lit::Null, "=") => c.sql.push_str(&format!("({q} IS NULL)")),
                (Lit::Null, "!=") => c.sql.push_str(&format!("({q} IS NOT NULL)")),
                (Lit::Null, op) => return Err(format!("`{field} {op} null`: null is compared with = and != alone")),
                (Lit::Bool(_), op) if op != "=" && op != "!=" => return Err(format!("`{field} {op}`: a boolean is compared with = and != alone")),
                (v, op) => {
                    let b = bound(col, v)?;
                    let m = mark(c);
                    c.values.push(b);
                    c.sql.push_str(&match op {
                        "=" => format!("({q} IS {m})"),
                        "!=" => format!("({q} IS NOT {m})"),
                        op => format!("({q} IS NOT NULL AND {q} {op} {m})"),
                    });
                }
            }
        }
        Expr::In { field, values } => {
            let col = column(field, columns, whose)?;
            let q = crate::store::quoted(&col.name).map_err(|e| e.to_string())?;
            let mut marks = Vec::new();
            for v in values {
                if *v == Lit::Null {
                    return Err(format!("`{field} in (…)`: null is not among the values (write {field} = null)"));
                }
                let b = bound(col, v)?;
                marks.push(mark(c));
                c.values.push(b);
            }
            c.sql.push_str(&format!("({q} IS NOT NULL AND {q} IN ({}))", marks.join(", ")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::parse as schema;

    fn perf() -> Vec<Named> {
        of_log(&schema(r#"{"type": "object", "properties": {"app": {"type": "string"}, "scenario": {"type": "string"}, "transport": {"type": "string"},
            "value": {"type": "number"}, "runs": {"type": "integer"}, "warm": {"type": "boolean"}, "config": {"type": "object"}}}"#).unwrap())
    }

    fn sql(text: &str) -> Result<Compiled, String> {
        compile(&parse(text)?, &perf(), "the log `perf`", 1)
    }

    /// Scenario "one scenario over time": the ticket's own words, compiled
    /// with every value bound.
    #[test]
    fn the_scenario_filter_compiles_to_bound_sql() {
        let c = sql(r#"app = "plexus" and scenario = "stream.10k.64B" and transport = "websocket""#).unwrap();
        assert_eq!(c.sql, r#"((("app" IS ?1) AND ("scenario" IS ?2)) AND ("transport" IS ?3))"#);
        assert_eq!(c.values, vec![Sql::Text("plexus".into()), Sql::Text("stream.10k.64B".into()), Sql::Text("websocket".into())]);
    }

    #[test]
    fn every_word_of_the_language() {
        let c = sql(r#"not (value >= 1.5 OR runs < 3) and warm = true and app in ('a', "b\"c") and transport != null and seq > 10 and by != "key:x""#).unwrap();
        assert_eq!(
            c.sql,
            r#"((((((NOT (("value" IS NOT NULL AND "value" >= ?1) OR ("runs" IS NOT NULL AND "runs" < ?2))) AND ("warm" IS ?3)) AND ("app" IS NOT NULL AND "app" IN (?4, ?5))) AND ("transport" IS NOT NULL)) AND ("seq" IS NOT NULL AND "seq" > ?6)) AND ("by" IS NOT ?7))"#
        );
        assert_eq!(c.values, vec![Sql::Real(1.5), Sql::Integer(3), Sql::Integer(1), Sql::Text("a".into()), Sql::Text("b\"c".into()), Sql::Integer(10), Sql::Text("key:x".into())]);
        assert_eq!(sql("machine_gone = null").unwrap_err(), "`machine_gone` is no field of the log `perf` (its fields: seq, at, by, ticket, app, scenario, transport, value, runs, warm, config)");
        assert_eq!(sql("value <= -2e3").unwrap().values, vec![Sql::Real(-2000.0)]);
        assert_eq!(compile(&parse("app = 1").unwrap(), &perf(), "x", 4).unwrap_err(), "`app` is string, compared with a number");
    }

    /// An unknown field is refused naming the log's fields; what is no
    /// filter is refused saying where.
    #[test]
    fn what_is_not_a_filter_is_refused() {
        for (text, says) in [
            ("nope = 1", "`nope` is no field of the log `perf` (its fields: seq, at, by, ticket, app"),
            ("tags = \"x\"", "`tags` is filtered by --tag"),
            ("config = 1", "kept as JSON"),
            ("warm > true", "a boolean is compared with = and != alone"),
            ("app = plexus", "a string is quoted"),
            ("app =", "a value"),
            ("app \"x\"", "followed by = != < <= > >= or in"),
            ("(app = \"x\"", "closed by `)`"),
            ("app = \"x\" app = \"y\"", "nothing more was expected"),
            ("app = \"x", "never ends"),
            ("app ~ 1", "`~` is no part of a filter"),
            ("", "an empty filter"),
            ("app in (null)", "null is not among the values"),
            ("app < null", "null is compared with = and != alone"),
            ("and = 1", "a field's name"),
            ("app = \"x\"; DROP TABLE logs", "`;` is no part of a filter"),
        ] {
            let e = sql(text).unwrap_err();
            assert!(e.contains(says), "{text:?}: {e}");
        }
        let deep = format!("{}app = \"x\"{}", "(".repeat(40), ")".repeat(40));
        assert!(sql(&deep).unwrap_err().contains("nests at most"));
        let many = format!("runs in ({})", (0..=IN_MAX).map(|i| i.to_string()).collect::<Vec<_>>().join(","));
        assert!(sql(&many).unwrap_err().contains("at most 100 values"));
        // Across logs: the envelope's alone.
        let e = compile(&parse("app = \"x\"").unwrap(), &envelope(), "the envelope (name a log to filter on its fields)", 1).unwrap_err();
        assert!(e.starts_with("`app` is no field of the envelope (name a log to filter on its fields) (its fields: seq, at, by, ticket)"), "{e}");
    }
}
